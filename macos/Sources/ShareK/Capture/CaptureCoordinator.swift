import AppKit

@MainActor
final class CaptureCoordinator: NSObject {
    private struct ActiveRecording {
        let recorder: ScreenRecorder
        let chrome: RecordingChrome
        let pointSize: CGSize
        let savedAutomatically: Bool
        var stopping = false
    }

    var onRecordingChange: ((Bool) -> Void)?

    private var selector: RegionSelector?
    private var busy = false
    private var recording: ActiveRecording?
    private var scrolling: ScrollingCapture?
    private var accessibilityPrompted = false

    @objc func performMenuAction(_ sender: NSMenuItem) {
        performAction(sender.representedObject as? String ?? "")
    }

    func performAction(_ action: String) {
        switch action {
        case MenuAction.region: captureRegion(afterMenu: true)
        case MenuAction.screen: captureFullScreen(afterMenu: true)
        case MenuAction.window: captureActiveWindow(afterMenu: true)
        case MenuAction.scrolling: toggleScrollingCapture(afterMenu: true)
        case MenuAction.text: recognizeText(afterMenu: true)
        case MenuAction.pin: pinRegion(afterMenu: true)
        case MenuAction.video: toggleRecording(.video, afterMenu: true)
        case MenuAction.gif: toggleRecording(.gif, afterMenu: true)
        case MenuAction.color: pickColor()
        case MenuAction.ruler: measure(afterMenu: true)
        case MenuAction.history: EditorProcess.shared.showHistory()
        case MenuAction.quit: NSApp.terminate(nil)
        default: break
        }
    }

    func pickColor() {
        guard !busy, recording == nil, scrolling == nil else { return }
        ColorPicker.pick()
    }

    func measure(afterMenu: Bool = false) {
        run(afterMenu: afterMenu) { [self] in
            _ = try await selectRegion(measuring: true)
        }
    }

    func pinRegion(afterMenu: Bool = false) {
        run(afterMenu: afterMenu) { [self] in
            guard let selection = try await selectRegion(crops: true), let pixels = selection.pixels else { return }
            PinManager.pin(pixels, scale: selection.scale, at: selection.globalRect)
        }
    }

    func recognizeText(afterMenu: Bool = false) {
        run(afterMenu: afterMenu) { [self] in
            guard let pixels = try await selectRegion(crops: true)?.pixels else { return }
            TextRecognizer.recognizeAndCopy(pixels)
        }
    }

    func pinClipboardImage() {
        guard let image = NSImage(pasteboard: .general),
              let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil),
              let pixels = Pixels(cgImage)
        else {
            Toast.show("El portapapeles no tiene una imagen")
            return
        }
        let scale = image.size.width > 0 ? max(1, CGFloat(cgImage.width) / image.size.width) : 1
        PinManager.pin(pixels, scale: scale)
    }

    func captureRegion(afterMenu: Bool = false) {
        run(afterMenu: afterMenu) { [self] in
            guard let selection = try await selectRegion(crops: true), let pixels = selection.pixels else { return }
            deliver(pixels, scale: selection.scale)
        }
    }

    func captureFullScreen(afterMenu: Bool = false) {
        run(afterMenu: afterMenu) { [self] in
            guard let screen = NSScreen.underMouse else { throw CaptureError.displayNotFound }
            let image = try await ScreenCapturer.capture(screen: screen, in: try await ScreenCapturer.content())
            guard let pixels = image.pixels() else { throw CaptureError.unreadable }
            deliver(pixels, scale: screen.backingScaleFactor)
        }
    }

    func captureActiveWindow(afterMenu: Bool = false) {
        run(afterMenu: afterMenu) { [self] in
            guard let window = WindowList.frontmostAppWindow() else { throw CaptureError.windowNotFound }
            let image = try await ScreenCapturer.capture(windowID: window.id, in: try await ScreenCapturer.content())
            guard let pixels = image.pixels() else { throw CaptureError.unreadable }
            let scale = NSScreen.screens.first { $0.frame.intersects(window.frame) }?.backingScaleFactor ?? 2
            deliver(pixels, scale: scale)
        }
    }

    func toggleScrollingCapture(afterMenu: Bool = false) {
        if stopActiveSession() { return }
        run(afterMenu: afterMenu) { [self] in
            guard let selection = try await selectRegion() else { return }
            let automatic = ScrollingCapture.canScrollAutomatically(prompt: !accessibilityPrompted)
            accessibilityPrompted = true
            let capture = ScrollingCapture(
                selection: selection, automatic: automatic,
                onManual: { [weak self] in self?.scrolling?.goManual() },
                onStop: { [weak self] in self?.scrolling?.stop() },
                onCancel: { [weak self] in self?.scrolling?.cancel() }
            )
            scrolling = capture
            onRecordingChange?(true)
            defer {
                scrolling = nil
                onRecordingChange?(false)
            }
            if let pixels = try await capture.run() {
                deliver(pixels, scale: selection.scale)
            }
        }
    }

    func toggleRecording(_ format: RecordingFormat, afterMenu: Bool = false) {
        if stopActiveSession() { return }
        run(afterMenu: afterMenu) { [self] in
            guard let selection = try await selectRegion() else { return }
            try await startRecording(format, selection: selection)
        }
    }

    func stopRecording() {
        if let scrolling {
            scrolling.stop()
        } else {
            finishRecording(keep: true)
        }
    }

    private func stopActiveSession() -> Bool {
        guard recording != nil || scrolling != nil else { return false }
        stopRecording()
        return true
    }

    func cancelRecording() {
        if let scrolling {
            scrolling.cancel()
        } else {
            finishRecording(keep: false)
        }
    }

    func discardForQuit() {
        scrolling?.cancel()
        guard let active = recording else { return }
        active.chrome.close()
        try? FileManager.default.removeItem(at: active.recorder.url)
        recording = nil
    }

    private func selectRegion(measuring: Bool = false, crops: Bool = false) async throws -> RegionSelection? {
        let windowFrames = WindowList.current().map(\.frame)
        let content = try await ScreenCapturer.content()
        var frozen: [FrozenScreen] = []
        for screen in NSScreen.screens {
            frozen.append(FrozenScreen(screen: screen, image: try await ScreenCapturer.capture(screen: screen, in: content)))
        }
        return await withCheckedContinuation { continuation in
            let selector = RegionSelector()
            self.selector = selector
            selector.begin(screens: frozen, windowFrames: windowFrames, measuring: measuring, crops: crops) { [weak self] selection in
                self?.selector = nil
                continuation.resume(returning: selection)
            }
        }
    }

    private func startRecording(_ format: RecordingFormat, selection: RegionSelection) async throws {
        let savedAutomatically = Settings.autoSave
        let url = savedAutomatically
            ? ImageWriter.uniqueURL(in: Settings.saveDirectory, date: Date(), fileExtension: format.fileExtension)
            : MediaCapture.temporaryURL(for: format)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)

        let chrome = RecordingChrome(
            rect: selection.globalRect, screen: selection.screen, title: format.title,
            onStop: { [weak self] in self?.stopRecording() },
            onCancel: { [weak self] in self?.cancelRecording() }
        )
        chrome.show()
        let recorder = ScreenRecorder(format: format, url: url)
        recorder.onUnexpectedStop = { [weak self] in self?.stopRecording() }
        recording = ActiveRecording(recorder: recorder, chrome: chrome, pointSize: selection.rect.size, savedAutomatically: savedAutomatically)
        onRecordingChange?(true)

        do {
            try await recorder.start(selection: selection, excludingWindowNumbers: chrome.windowNumbers)
        } catch {
            chrome.close()
            recording = nil
            onRecordingChange?(false)
            try? FileManager.default.removeItem(at: url)
            throw error
        }
    }

    private func finishRecording(keep: Bool) {
        guard var active = recording, !active.stopping else { return }
        active.stopping = true
        recording = active
        let session = active
        session.chrome.showProcessing()

        let recorder = session.recorder
        let timeout = Task { @MainActor [weak self] in
            try await Task.sleep(for: .seconds(15))
            guard let self, self.recording?.recorder === recorder else { return }
            session.chrome.close()
            self.recording = nil
            self.onRecordingChange?(false)
            Toast.show("No se pudo terminar la grabación", detail: "La grabación no respondió al detenerla.")
        }
        Task { @MainActor in
            defer { Memory.releaseSoon() }
            do {
                try await recorder.stop()
                timeout.cancel()
                guard recording?.recorder === recorder else { return }
                if keep {
                    deliver(recorder.url, format: recorder.format, pointSize: session.pointSize, savedAutomatically: session.savedAutomatically)
                } else {
                    try? FileManager.default.removeItem(at: recorder.url)
                }
            } catch {
                timeout.cancel()
                try? FileManager.default.removeItem(at: recorder.url)
                guard recording?.recorder === recorder else { return }
                if keep { Toast.show("No se pudo terminar la grabación", detail: error.localizedDescription) }
            }
            session.chrome.close()
            recording = nil
            onRecordingChange?(false)
        }
    }

    private func run(afterMenu: Bool, _ work: @escaping @MainActor () async throws -> Void) {
        guard !busy, recording == nil, scrolling == nil else { return }
        ClipboardManager.shared.forgetIfReplaced()
        guard CGPreflightScreenCaptureAccess() else {
            CGRequestScreenCaptureAccess()
            Alerts.show(
                "ShareK necesita permiso de Grabación de pantalla",
                info: "Actívalo en Ajustes del Sistema › Privacidad y seguridad › Grabación de pantalla y vuelve a abrir la app."
            )
            return
        }
        busy = true
        Task { @MainActor in
            defer {
                busy = false
                Memory.releaseSoon()
            }
            if afterMenu { try? await Task.sleep(for: .milliseconds(200)) }
            do {
                try await work()
            } catch {
                Toast.show("No se pudo capturar", detail: error.localizedDescription)
            }
        }
    }

    private func deliver(_ pixels: Pixels, scale: CGFloat) {
        let id = CaptureID.next()
        let created = Date()
        let saveURL = Settings.autoSave ? AutoSave.shared.reserve(id, date: created) : nil
        let opensEditor = Settings.openEditor || !(Settings.copyOnCapture || saveURL != nil)
        let opened = opensEditor && EditorProcess.shared.open(pixels, id: id, scale: scale, created: created, saved: saveURL)
        let provider = Settings.copyOnCapture ? ClipboardManager.shared.copy(id, pixels: pixels, scale: scale) : nil
        EditorProcess.shared.syncClipboardOwner()
        guard let saveURL else {
            provider?.encode()
            return
        }
        Task.detached(priority: .userInitiated) {
            let png = PNGEncoder.encode(pixels, scale: scale, fast: true)
            await provider?.finish(png)
            await MainActor.run {
                AutoSave.shared.write(png, to: saveURL, id: id, tracked: opened)
            }
        }
    }

    private func deliver(_ url: URL, format: RecordingFormat, pointSize: CGSize, savedAutomatically: Bool) {
        let id = CaptureID.next()
        if savedAutomatically { History.shared.record(url) }
        if Settings.copyOnCapture {
            ClipboardManager.shared.copy(id, .file(url, gif: format == .gif))
        }
        if Settings.openEditor || !(Settings.copyOnCapture || savedAutomatically) {
            EditorProcess.shared.openMedia(id: id, url: url, format: format, pointSize: pointSize, savedAutomatically: savedAutomatically)
        }
        EditorProcess.shared.syncClipboardOwner()
    }
}

@MainActor
enum Alerts {
    static func show(_ message: String, info: String) {
        NSApp.activate()
        let alert = NSAlert()
        alert.messageText = message
        alert.informativeText = info
        alert.runModal()
    }
}
