import Darwin
import Foundation

enum ToEditor: Codable {
    case open(id: UInt64, image: PixelFormat, scale: Double, created: Date, saved: String?)
    case openMedia(id: UInt64, path: String, gif: Bool, width: Double, height: Double, created: Date, savedAutomatically: Bool)
    case showHistory
    case clipboardOwner(id: UInt64?)
    case exitAccepted
    case probe(command: String)
}

enum ToMenuBar: Codable {
    case copy(id: UInt64)
    case changed(id: UInt64)
    case discard(id: UInt64)
    case release(id: UInt64)
    case trash(id: UInt64)
    case copyFile(id: UInt64, path: String, gif: Bool)
    case copyFiles(paths: [String])
    case pin(image: PixelFormat, scale: Double)
    case toast(title: String, detail: String?)
    case action(name: String)
    case activate
    case exiting(received: UInt64)
    case probed(result: String)
}

@MainActor
enum CaptureID {
    private static var last: UInt64 = 0

    static func next() -> UInt64 {
        last += 1
        return last
    }
}

final class Channel: Sendable {
    private static let headerLimit = 1 << 20
    private static let payloadLimit = 1 << 31

    private let readFD: Int32
    private let writeFD: Int32
    private let writer = DispatchQueue(label: "ShareK.ipc")

    init(read: Int32, write: Int32) {
        readFD = fcntl(read, F_DUPFD_CLOEXEC, 0)
        writeFD = fcntl(write, F_DUPFD_CLOEXEC, 0)
    }

    @discardableResult
    func send<Message: Encodable>(_ message: Message, payload: Data = Data()) -> Bool {
        guard let header = try? JSONEncoder().encode(message),
              header.count <= Self.headerLimit,
              payload.count <= Self.payloadLimit
        else { return false }
        let fd = writeFD
        writer.async {
            var frame = Data()
            withUnsafeBytes(of: UInt32(header.count).littleEndian) { frame.append(contentsOf: $0) }
            frame.append(header)
            withUnsafeBytes(of: UInt64(payload.count).littleEndian) { frame.append(contentsOf: $0) }
            if Self.write(fd, frame) { _ = Self.write(fd, payload) }
        }
        return true
    }

    func drain() {
        writer.sync {}
    }

    func close() {
        let fd = writeFD
        writer.async { _ = Darwin.close(fd) }
    }

    func receive<Message: Decodable & Sendable>(
        _ type: Message.Type,
        handler: @escaping @Sendable @MainActor (Message, Data) -> Void,
        closed: @escaping @Sendable @MainActor () -> Void
    ) {
        let fd = readFD
        let thread = Thread {
            while let frame = Self.frame(fd) {
                guard let message = try? JSONDecoder().decode(Message.self, from: frame.header) else { continue }
                let payload = frame.payload
                DispatchQueue.main.async {
                    MainActor.assumeIsolated { handler(message, payload) }
                }
            }
            _ = Darwin.close(fd)
            DispatchQueue.main.async {
                MainActor.assumeIsolated { closed() }
            }
        }
        thread.name = "ShareK.ipc.reader"
        thread.start()
    }

    private static func frame(_ fd: Int32) -> (header: Data, payload: Data)? {
        guard let length = read(fd, count: 4)?.withUnsafeBytes({ UInt32(littleEndian: $0.loadUnaligned(as: UInt32.self)) }),
              length <= headerLimit,
              let header = read(fd, count: Int(length)),
              let size = read(fd, count: 8)?.withUnsafeBytes({ UInt64(littleEndian: $0.loadUnaligned(as: UInt64.self)) }),
              size <= payloadLimit,
              let payload = read(fd, count: Int(size))
        else { return nil }
        return (header, payload)
    }

    private static func read(_ fd: Int32, count: Int) -> Data? {
        guard count > 0 else { return Data() }
        if count >= 1 << 18 {
            return MappedData.make(count: count) { fill(fd, $0, count) }
        }
        var data = Data(count: count)
        let complete = data.withUnsafeMutableBytes { (buffer: UnsafeMutableRawBufferPointer) -> Bool in
            guard let pointer = buffer.baseAddress else { return false }
            return fill(fd, pointer, count)
        }
        return complete ? data : nil
    }

    private static func fill(_ fd: Int32, _ start: UnsafeMutableRawPointer, _ count: Int) -> Bool {
        var pointer = start
        var remaining = count
        while remaining > 0 {
            let got = Darwin.read(fd, pointer, remaining)
            if got < 0, errno == EINTR { continue }
            guard got > 0 else { return false }
            pointer = pointer.advanced(by: got)
            remaining -= got
        }
        return true
    }

    private static func write(_ fd: Int32, _ data: Data) -> Bool {
        data.withUnsafeBytes { buffer -> Bool in
            guard var pointer = buffer.baseAddress else { return true }
            var remaining = buffer.count
            while remaining > 0 {
                let written = Darwin.write(fd, pointer, remaining)
                if written < 0, errno == EINTR { continue }
                guard written > 0 else { return false }
                pointer = pointer.advanced(by: written)
                remaining -= written
            }
            return true
        }
    }
}
