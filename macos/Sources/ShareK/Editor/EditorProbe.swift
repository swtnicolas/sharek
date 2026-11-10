import AppKit

@MainActor
enum EditorProbe {
    static func run(_ command: String) {
        Task { @MainActor in
            let result = await perform(command.split(separator: " ").map(String.init))
            MenuBarLink.shared.send(.probed(result: result))
        }
    }

    private static func perform(_ words: [String]) async -> String {
        let manager = EditorManager.shared
        let controller = manager.window
        let capture = controller?.page as? CapturePage
        let argument = words.dropFirst().joined(separator: " ")
        var result = "ok"
        switch words.first {
        case "state":
            return "windows=\(NSApp.windows.count) tabs=\(manager.tabs.count) current=\(manager.current) \(controller?.probeState ?? "window=none")"
        case "select":
            guard let index = Int(argument), manager.tabs.indices.contains(index) else { return "missing" }
            manager.select(index)
        case "click":
            guard let index = Int(argument), manager.tabs.indices.contains(index), let controller, let window = controller.window else { return "missing" }
            NSApp.activate(ignoringOtherApps: true)
            let point = controller.probeTabPoint(index)
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                if let event = NSEvent.mouseEvent(
                    with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1
                ) {
                    NSApp.postEvent(event, atStart: false)
                }
            }
        case "draw":
            guard let tool = Tool(rawValue: argument), let capture else { return "missing" }
            await capture.probeDraw(tool)
        case "resize":
            let size = argument.split(separator: " ").compactMap { Double($0) }
            guard size.count == 2, let controller else { return "missing" }
            controller.probeResize(CGSize(width: size[0], height: size[1]))
        case "scale":
            guard let backing = Double(argument) else { return "missing" }
            AnnotationView.probeBacking = CGFloat(backing)
            capture?.probeRedraw()
        case "zoom":
            guard let magnification = Double(argument), let capture else { return "missing" }
            capture.probeZoom(CGFloat(magnification))
        case "undo":
            guard let capture else { return "missing" }
            capture.probeUndo()
        case "minimize":
            controller?.window?.miniaturize(nil)
        case "restore":
            controller?.window?.deminiaturize(nil)
        case "close":
            guard let index = Int(argument), manager.tabs.indices.contains(index) else { return "missing" }
            manager.close(index)
        case "shut":
            controller?.window?.performClose(NSMenuItem())
        case "untag":
            AnnotationView.probeKeepsWindowSpace = true
            controller?.window?.colorSpace = .genericRGB
            FrameBuffers.probeMeasureAgain()
            capture?.probeRedraw()
        case "pin":
            guard let capture else { return "missing" }
            capture.probePin()
        case "unpin":
            PinManager.closeAll()
        case "frame":
            guard let capture else { return "missing" }
            result = capture.probeFrame(to: URL(fileURLWithPath: argument)) ? "ok" : "failed"
        default:
            return "unknown"
        }
        if let wake = NSEvent.otherEvent(
            with: .applicationDefined, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: 0, context: nil, subtype: 0, data1: 0, data2: 0
        ) {
            NSApp.postEvent(wake, atStart: false)
        }
        try? await Task.sleep(for: .milliseconds(400))
        return result
    }
}
