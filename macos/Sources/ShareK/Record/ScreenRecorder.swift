import AppKit
import AVFoundation
import ScreenCaptureKit

enum RecordingFormat {
    case video
    case gif

    var fileExtension: String { self == .video ? "mp4" : "gif" }
    var title: String { self == .video ? "Video" : "GIF" }
}

enum RecordingError: LocalizedError {
    case noFrames

    var errorDescription: String? {
        switch self {
        case .noFrames: "La grabación no tiene cuadros."
        }
    }
}

@MainActor
final class ScreenRecorder: NSObject {
    let format: RecordingFormat
    let url: URL
    var onUnexpectedStop: (() -> Void)?

    private var stream: SCStream?
    private var streamStopped = false
    private var videoOutput: SCRecordingOutput?
    private var videoFinished = false
    private var videoError: Error?
    private var finishContinuation: CheckedContinuation<Void, Error>?
    private var gifOutput: GIFStreamOutput?
    private let sampleQueue = DispatchQueue(label: "ShareK.recording", qos: .userInitiated)

    init(format: RecordingFormat, url: URL) {
        self.format = format
        self.url = url
    }

    func start(selection: RegionSelection, excludingWindowNumbers windowNumbers: [Int]) async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first(where: { $0.displayID == selection.screen.displayID }) else {
            throw CaptureError.displayNotFound
        }
        let excluded = content.windows.filter { windowNumbers.contains(Int($0.windowID)) }
        let filter = SCContentFilter(display: display, excludingWindows: excluded)

        let source = selection.topLeftRect
        let pixelScale = format == .video ? selection.scale : 1
        let configuration = SCStreamConfiguration()
        configuration.sourceRect = source
        configuration.width = Self.even(source.width * pixelScale)
        configuration.height = Self.even(source.height * pixelScale)
        configuration.showsCursor = true
        configuration.queueDepth = 5
        configuration.colorSpaceName = CGColorSpace.sRGB
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: format == .video ? 30 : 15)

        switch format {
        case .video:
            configuration.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            configuration.capturesAudio = Settings.recordsAudio
            configuration.excludesCurrentProcessAudio = true
        case .gif:
            configuration.pixelFormat = kCVPixelFormatType_32BGRA
        }

        let stream = SCStream(filter: filter, configuration: configuration, delegate: self)
        switch format {
        case .video:
            let recording = SCRecordingOutputConfiguration()
            recording.outputURL = url
            recording.outputFileType = .mp4
            recording.videoCodecType = max(configuration.width, configuration.height) > 4096 ? .hevc : .h264
            let output = SCRecordingOutput(configuration: recording, delegate: self)
            try stream.addRecordingOutput(output)
            videoOutput = output
        case .gif:
            let output = try GIFStreamOutput(url: url, width: configuration.width, height: configuration.height)
            try stream.addStreamOutput(output, type: .screen, sampleHandlerQueue: sampleQueue)
            gifOutput = output
        }
        self.stream = stream
        try await stream.startCapture()
    }

    func stop() async throws {
        let stopTime = CMClockGetTime(CMClockGetHostTimeClock()).seconds
        if let stream, !streamStopped {
            streamStopped = true
            try? await stream.stopCapture()
        }
        stream = nil

        switch format {
        case .video:
            if !videoFinished, videoError == nil {
                try await withCheckedThrowingContinuation { finishContinuation = $0 }
            }
            if let videoError { throw videoError }
        case .gif:
            guard let gifOutput else { return }
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                sampleQueue.async {
                    do {
                        try gifOutput.finish(at: stopTime)
                        continuation.resume()
                    } catch {
                        continuation.resume(throwing: error)
                    }
                }
            }
        }
    }

    private func completeVideo(_ error: Error?) {
        videoFinished = true
        videoError = error
        let continuation = finishContinuation
        finishContinuation = nil
        if let error { continuation?.resume(throwing: error) } else { continuation?.resume() }
    }

    private static func even(_ value: CGFloat) -> Int {
        max(2, Int(value.rounded()) & ~1)
    }
}

extension ScreenRecorder: SCStreamDelegate, SCRecordingOutputDelegate {
    nonisolated func stream(_ stream: SCStream, didStopWithError error: Error) {
        Task { @MainActor in
            self.streamStopped = true
            self.onUnexpectedStop?()
        }
    }

    nonisolated func recordingOutputDidFinishRecording(_ recordingOutput: SCRecordingOutput) {
        Task { @MainActor in self.completeVideo(nil) }
    }

    nonisolated func recordingOutput(_ recordingOutput: SCRecordingOutput, didFailWithError error: Error) {
        Task { @MainActor in self.completeVideo(error) }
    }
}

private final class GIFStreamOutput: NSObject, SCStreamOutput, @unchecked Sendable {
    private let encoder: GIFEncoder
    private var failure: Error?

    init(url: URL, width: Int, height: Int) throws {
        encoder = try GIFEncoder(url: url, width: width, height: height)
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, failure == nil,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let rawStatus = attachments.first?[.status] as? Int,
              SCFrameStatus(rawValue: rawStatus) == .complete,
              let pixelBuffer = sampleBuffer.imageBuffer
        else { return }

        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(pixelBuffer),
              CVPixelBufferGetWidth(pixelBuffer) == encoder.width,
              CVPixelBufferGetHeight(pixelBuffer) == encoder.height
        else { return }
        do {
            try encoder.append(bgra: base, bytesPerRow: CVPixelBufferGetBytesPerRow(pixelBuffer), time: sampleBuffer.presentationTimeStamp.seconds)
        } catch {
            failure = error
        }
    }

    func finish(at time: Double) throws {
        if let failure { throw failure }
        guard !encoder.isEmpty else { throw RecordingError.noFrames }
        try encoder.finish(at: time)
    }
}
