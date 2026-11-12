import AppKit
@preconcurrency import ApplicationServices
@preconcurrency import ScreenCaptureKit

@MainActor
final class ScrollingCapture {
    enum Status {
        case automatic
        case manual
        case tooFast

        var title: String {
            switch self {
            case .automatic: "Desplazamiento"
            case .manual: "Desplázate"
            case .tooFast: "Más despacio"
            }
        }
    }

    let selection: RegionSelection
    let automatic: Bool

    private let chrome: RecordingChrome
    private var stopRequested = false
    private var cancelled = false
    private var manualRequested = false
    private var status: Status
    private let maximumAutomaticSteps = 400
    private let maximumHeight = 30_000
    private let firstRequest = 6.0
    private let targetShare = 0.4
    private let maximumMisses = 4

    static func canScrollAutomatically(prompt: Bool) -> Bool {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): prompt] as CFDictionary
        return AXIsProcessTrustedWithOptions(options)
    }

    init(selection: RegionSelection, automatic: Bool, onManual: @escaping () -> Void, onStop: @escaping () -> Void, onCancel: @escaping () -> Void) {
        self.selection = selection
        self.automatic = automatic
        status = automatic ? .automatic : .manual
        chrome = RecordingChrome(
            rect: selection.globalRect, screen: selection.screen,
            title: status.title,
            onManual: automatic ? onManual : nil,
            onStop: onStop, onCancel: onCancel
        )
    }

    func goManual() {
        manualRequested = true
        show(.manual)
    }

    func stop() {
        stopRequested = true
        chrome.showProcessing()
    }

    func cancel() {
        cancelled = true
        stopRequested = true
        chrome.showProcessing()
    }

    private func show(_ next: Status) {
        guard next != status else { return }
        status = next
        chrome.setTitle(next.title, showsManual: next == .automatic)
    }

    func run() async throws -> Pixels? {
        chrome.show()
        defer { chrome.close() }

        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first(where: { $0.displayID == selection.screen.displayID }) else {
            throw CaptureError.displayNotFound
        }
        let process = ProcessInfo.processInfo.processIdentifier
        let excluded = content.windows.filter { $0.owningApplication?.processID == process }
        let filter = SCContentFilter(display: display, excludingWindows: excluded)
        let source = selection.topLeftRect
        let pixelWidth = Int((source.width * selection.scale).rounded())
        let pixelHeight = Int((source.height * selection.scale).rounded())
        let capture = { () async throws -> CGImage in
            let configuration = SCStreamConfiguration()
            configuration.sourceRect = source
            configuration.width = pixelWidth
            configuration.height = pixelHeight
            configuration.showsCursor = false
            configuration.captureResolution = .best
            return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration)
        }

        let stitcher = ScrollStitcher(ignoredTrailingColumns: Int(16 * selection.scale))
        guard let first = try await settled(capture) else { return nil }
        stitcher.pointer = automatic ? first.height / 2 : nil
        _ = stitcher.add(first)

        let global = selection.globalRect
        let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
        let center = CGPoint(x: global.midX, y: primaryHeight - global.midY)
        if automatic { CGWarpMouseCursorPosition(center) }
        var request = firstRequest
        var direction = -1.0
        var directionChecked = false
        var progressed = false
        var unchanged = 0
        var misses = 0
        var steps = 0

        while true {
            if cancelled { return nil }
            if stopRequested || stitcher.height >= maximumHeight { break }
            if automatic, !manualRequested, !global.contains(NSEvent.mouseLocation) { manualRequested = true }
            if !automatic || manualRequested || misses > maximumMisses {
                if status == .automatic { show(.manual) }
                stitcher.pointer = nil
                try await Task.sleep(for: .milliseconds(80))
                let shot = try await capture()
                if cancelled { return nil }
                switch stitcher.add(shot) {
                case .noMatch: show(.tooFast)
                case .reversed: break
                default: show(.manual)
                }
                continue
            }
            steps += 1
            if steps > maximumAutomaticSteps { break }
            scroll(by: request * direction)
            guard let shot = try await settled(capture) else { return nil }
            switch stitcher.add(shot) {
            case .appended(rows: let moved):
                progressed = true
                directionChecked = true
                unchanged = 0
                misses = 0
                let movedPoints = Double(moved) / Double(selection.scale)
                request = min(4000, max(1, request * Double(source.height) * targetShare / max(1, movedPoints)))
            case .unchanged where progressed:
                unchanged += 1
                if unchanged >= 2 { stopRequested = true }
            case .unchanged:
                request *= 4
                misses += 1
            case .reversed where !directionChecked:
                direction = -direction
                directionChecked = true
                scroll(by: request * direction)
                if let back = try await settled(capture) { _ = stitcher.add(back) }
            case .reversed, .noMatch:
                scroll(by: -request * direction)
                if let back = try await settled(capture) { _ = stitcher.add(back) }
                request = max(1, request / 4)
                misses += 1
            case .started:
                break
            }
        }
        chrome.showProcessing()
        return cancelled ? nil : stitcher.compose()
    }

    private func settled(_ capture: () async throws -> CGImage) async throws -> CGImage? {
        let started = Date()
        var last = try await capture()
        while Date().timeIntervalSince(started) < 1.5, !cancelled {
            try await Task.sleep(for: .milliseconds(60))
            let shot = try await capture()
            if ScrollStitcher.settled(last, shot) { return shot }
            last = shot
        }
        return cancelled ? nil : last
    }

    private func scroll(by amount: Double) {
        guard selection.globalRect.contains(NSEvent.mouseLocation) else {
            manualRequested = true
            return
        }
        let event = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: Int32(amount.rounded()), wheel2: 0, wheel3: 0)
        event?.post(tap: .cghidEventTap)
    }
}
