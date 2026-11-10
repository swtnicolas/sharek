import AppKit
import ImageIO
import Vision

enum TextRecognitionError: LocalizedError {
    case failed(String)

    var errorDescription: String? {
        switch self {
        case .failed(let detail): detail
        }
    }
}

@MainActor
enum TextRecognizer {
    static func recognize(_ image: CGImage) async throws -> String {
        var request = RecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        request.automaticallyDetectsLanguage = true
        let observations = try await request.perform(on: image)
        return observations.compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
    }

    static func runHelper() -> Never {
        let input = FileHandle.standardInput.readDataToEndOfFile()
        Task {
            var status: Int32 = 0
            var output = ""
            if let source = CGImageSourceCreateWithData(input as CFData, nil),
               let image = CGImageSourceCreateImageAtIndex(source, 0, nil) {
                do {
                    output = try await recognize(image)
                } catch {
                    output = error.localizedDescription
                    status = 1
                }
            } else {
                status = 1
            }
            FileHandle.standardOutput.write(Data(output.utf8))
            exit(status)
        }
        dispatchMain()
    }

    private static func recognizeInHelper(_ pixels: Pixels) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(with: Result { try runInHelper(pixels) })
            }
        }
    }

    private nonisolated static func runInHelper(_ pixels: Pixels) throws -> String {
        guard let executable = Bundle.main.executableURL,
              let png = PNGEncoder.encode(pixels, scale: nil, fast: true)
        else { throw TextRecognitionError.failed("No se pudo preparar la imagen.") }
        let process = Process()
        process.executableURL = executable
        process.arguments = ["--ocr"]
        let input = Pipe()
        let output = Pipe()
        process.standardInput = input
        process.standardOutput = output
        try process.run()
        try input.fileHandleForWriting.write(contentsOf: png)
        try input.fileHandleForWriting.close()
        let result = try output.fileHandleForReading.readToEnd() ?? Data()
        process.waitUntilExit()
        let text = String(decoding: result, as: UTF8.self)
        guard process.terminationStatus == 0 else { throw TextRecognitionError.failed(text) }
        return text
    }

    static func recognizeAndCopy(_ pixels: Pixels) {
        Task { @MainActor in
            do {
                let text = try await recognizeInHelper(pixels)
                guard !text.isEmpty else {
                    Toast.show("No se encontró texto")
                    return
                }
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
                Toast.show("Texto copiado", detail: text)
            } catch {
                Toast.show("No se pudo reconocer el texto", detail: error.localizedDescription)
            }
        }
    }

    static func presentResult(of pixels: Pixels, on window: NSWindow) {
        Task { @MainActor in
            do {
                let text = try await recognizeInHelper(pixels)
                TextResultSheet.present(text, on: window)
            } catch {
                Toast.show("No se pudo reconocer el texto", detail: error.localizedDescription)
            }
        }
    }
}

@MainActor
private final class TextResultSheet: NSObject {
    private static var active: [TextResultSheet] = []

    private let sheet: NSWindow
    private let textView: NSTextView
    private weak var parent: NSWindow?

    static func present(_ text: String, on window: NSWindow) {
        let controller = TextResultSheet(text: text, parent: window)
        active.append(controller)
        window.beginSheet(controller.sheet)
    }

    private init(text: String, parent: NSWindow) {
        self.parent = parent
        sheet = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 520, height: 360), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        let scroll = NSTextView.scrollableTextView()
        textView = scroll.documentView as! NSTextView
        super.init()

        textView.string = text.isEmpty ? "No se encontró texto." : text
        textView.font = .systemFont(ofSize: 13)
        textView.isRichText = false
        textView.textContainerInset = CGSize(width: 8, height: 8)

        let copy = NSButton(title: "Copiar", target: self, action: #selector(copyText))
        copy.keyEquivalent = "\r"
        let close = NSButton(title: "Cerrar", target: self, action: #selector(dismiss))
        close.keyEquivalent = "\u{1b}"
        let buttons = NSStackView(views: [close, copy])
        buttons.spacing = 8

        let root = NSView()
        for view in [scroll, buttons] {
            view.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(view)
        }
        NSLayoutConstraint.activate([
            scroll.topAnchor.constraint(equalTo: root.topAnchor, constant: 12),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            buttons.topAnchor.constraint(equalTo: scroll.bottomAnchor, constant: 12),
            buttons.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            buttons.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12),
        ])
        sheet.contentView = root
        sheet.initialFirstResponder = textView
    }

    @objc private func copyText() {
        let range = textView.selectedRange()
        let text = range.length > 0 ? (textView.string as NSString).substring(with: range) : textView.string
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        dismiss()
    }

    @objc private func dismiss() {
        parent?.endSheet(sheet)
        Self.active.removeAll { $0 === self }
    }
}

@MainActor
enum Toast {
    static var forward: ((String, String?) -> Void)?

    private static var current: NSPanel?

    static func show(_ message: String, detail: String? = nil) {
        if let forward {
            forward(message, detail)
            return
        }
        current?.orderOut(nil)

        let title = NSTextField(labelWithString: message)
        title.font = .systemFont(ofSize: 13, weight: .semibold)
        var views: [NSView] = [title]
        if let detail {
            let preview = detail.split(separator: "\n").prefix(3).joined(separator: "\n")
            let body = NSTextField(wrappingLabelWithString: preview.count > 180 ? String(preview.prefix(180)) + "…" : preview)
            body.font = .systemFont(ofSize: 11)
            body.textColor = .secondaryLabelColor
            body.preferredMaxLayoutWidth = 320
            views.append(body)
        }
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        stack.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 12, right: 16)

        let background = NSVisualEffectView()
        background.material = .hudWindow
        background.state = .active
        background.wantsLayer = true
        background.layer?.cornerRadius = 12
        background.layer?.masksToBounds = true
        stack.translatesAutoresizingMaskIntoConstraints = false
        background.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: background.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: background.trailingAnchor),
            stack.topAnchor.constraint(equalTo: background.topAnchor),
            stack.bottomAnchor.constraint(equalTo: background.bottomAnchor),
        ])
        let size = stack.fittingSize

        let panel = NSPanel(contentRect: CGRect(origin: .zero, size: size), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.level = NSWindow.Level(rawValue: NSWindow.Level.screenSaver.rawValue + 1)
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.isReleasedWhenClosed = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = background
        let visible = (NSScreen.underMouse ?? NSScreen.main)?.visibleFrame ?? .zero
        panel.setFrameOrigin(CGPoint(x: visible.midX - size.width / 2, y: visible.minY + 80))
        panel.orderFrontRegardless()
        current = panel

        Task { @MainActor in
            try? await Task.sleep(for: .seconds(2.2))
            guard current === panel else { return }
            await NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.25
                panel.animator().alphaValue = 0
            }
            panel.orderOut(nil)
            if current === panel { current = nil }
        }
    }
}
