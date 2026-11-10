import Darwin
import Foundation

@MainActor
enum Memory {
    nonisolated static let allocatorMode = "MallocSpaceEfficient"
    private static var pending: Task<Void, Never>?

    static var footprint: UInt64 {
        var usage = rusage_info_v2()
        let status = withUnsafeMutablePointer(to: &usage) { pointer in
            pointer.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(getpid(), RUSAGE_INFO_V2, $0) }
        }
        return status == 0 ? usage.ri_phys_footprint : 0
    }

    static func releaseSoon() {
        pending?.cancel()
        pending = Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            _ = malloc_zone_pressure_relief(nil, 0)
        }
    }
}

enum MappedData {
    private static let diskThreshold = 1 << 20
    private static let diskReserve: UInt64 = 1 << 30
    private static let directory = NSTemporaryDirectory()

    static func make(count: Int, _ fill: (UnsafeMutableRawPointer) -> Bool) -> Data? {
        make(capacity: count) { fill($0) ? count : nil }
    }

    static func make(capacity: Int, _ fill: (UnsafeMutableRawPointer) -> Int?) -> Data? {
        guard capacity > 0, let pointer = map(capacity) else { return nil }
        guard let count = fill(pointer), count > 0, count <= capacity else {
            _ = munmap(pointer, capacity)
            return nil
        }
        return Data(bytesNoCopy: pointer, count: count, deallocator: .custom { start, _ in _ = munmap(start, capacity) })
    }

    fileprivate static func map(_ size: Int) -> UnsafeMutableRawPointer? {
        if size >= diskThreshold, let pointer = mapFile(size) { return pointer }
        return valid(mmap(nil, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0))
    }

    private static func mapFile(_ size: Int) -> UnsafeMutableRawPointer? {
        var volume = statfs()
        guard statfs(directory, &volume) == 0,
              UInt64(volume.f_bavail) * UInt64(volume.f_bsize) >= UInt64(size) + diskReserve
        else { return nil }
        var path = Array((directory as NSString).appendingPathComponent("ShareK.XXXXXX").utf8CString)
        let descriptor = mkstemp(&path)
        guard descriptor >= 0 else { return nil }
        defer { _ = close(descriptor) }
        _ = unlink(path)
        guard ftruncate(descriptor, off_t(size)) == 0 else { return nil }
        return valid(mmap(nil, size, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0))
    }

    private static func valid(_ pointer: UnsafeMutableRawPointer?) -> UnsafeMutableRawPointer? {
        pointer == UnsafeMutableRawPointer(bitPattern: -1) ? nil : pointer
    }
}

final class MappedScratch: @unchecked Sendable {
    let pointer: UnsafeMutableRawPointer
    let capacity: Int

    init?(capacity: Int) {
        guard capacity > 0, let pointer = MappedData.map(capacity) else { return nil }
        self.pointer = pointer
        self.capacity = capacity
    }

    deinit {
        _ = munmap(pointer, capacity)
    }
}
