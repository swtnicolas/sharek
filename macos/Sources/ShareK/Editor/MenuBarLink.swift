import AppKit
import Darwin

@MainActor
final class EditorAppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.mainMenu = MainMenu.build(target: MenuBarLink.shared)
        Toast.forward = { title, detail in MenuBarLink.shared.send(.toast(title: title, detail: detail)) }
        MenuBarLink.shared.start()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let manager = EditorManager.shared
        manager.closeAll()
        guard manager.isBusy else {
            MenuBarLink.shared.drain()
            return .terminateNow
        }
        manager.onQuiet = {
            MenuBarLink.shared.drain()
            NSApp.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}

@MainActor
final class MenuBarLink: NSObject {
    static let shared = MenuBarLink()

    private(set) var clipboardOwner: UInt64?
    private var channel: Channel?
    private var received: UInt64 = 0
    private var offered: UInt64?
    private var outbox: [(token: Int, frame: (ToMenuBar, Data)?, ready: Bool)] = []
    private var tokens = 0

    func start() {
        let output = dup(STDOUT_FILENO)
        let null = Darwin.open("/dev/null", O_WRONLY)
        if null >= 0 {
            _ = dup2(null, STDOUT_FILENO)
            _ = Darwin.close(null)
        }
        let channel = Channel(read: STDIN_FILENO, write: output)
        _ = Darwin.close(output)
        self.channel = channel
        channel.receive(ToEditor.self) { [weak self] message, payload in
            self?.handle(message, payload)
        } closed: {
            NSApp.terminate(nil)
        }
    }

    func send(_ message: ToMenuBar, payload: Data = Data()) {
        tokens += 1
        outbox.append((tokens, (message, payload), true))
        flush()
    }

    func sendLater(_ produce: @escaping @Sendable () -> (ToMenuBar, Data)?) {
        tokens += 1
        let token = tokens
        outbox.append((token, nil, false))
        Task.detached(priority: .userInitiated) { [weak self] in
            let frame = produce()
            await self?.complete(token, frame)
        }
    }

    var isSending: Bool { !outbox.isEmpty }

    func drain() {
        channel?.drain()
    }

    func copied(_ id: UInt64) {
        clipboardOwner = id
    }

    func maybeExit() {
        guard channel != nil, offered != received, EditorManager.shared.isIdle else { return }
        offered = received
        send(.exiting(received: received))
    }

    @objc func performMenuAction(_ sender: NSMenuItem) {
        guard let action = sender.representedObject as? String else { return }
        if action != MenuAction.history, let parent = NSRunningApplication(processIdentifier: getppid()) {
            NSApp.yieldActivation(to: parent)
        }
        send(.action(name: action))
    }

    private func complete(_ token: Int, _ frame: (ToMenuBar, Data)?) {
        guard let index = outbox.firstIndex(where: { $0.token == token }) else { return }
        outbox[index].frame = frame
        outbox[index].ready = true
        flush()
        Memory.releaseSoon()
    }

    private func flush() {
        while let first = outbox.first, first.ready {
            outbox.removeFirst()
            if let frame = first.frame { channel?.send(frame.0, payload: frame.1) }
        }
        guard outbox.isEmpty else { return }
        EditorManager.shared.finishIfQuiet()
        maybeExit()
    }

    private func handle(_ message: ToEditor, _ payload: Data) {
        received += 1
        switch message {
        case .open(let id, let format, let scale, let created, let saved):
            if let pixels = Pixels(format: format, data: payload) {
                let document = CaptureDocument(id: id, pixels: pixels, scale: CGFloat(scale), createdAt: created)
                if let saved { document.markAutoSaved(at: URL(fileURLWithPath: saved)) }
                EditorManager.shared.open(document)
            }
        case .openMedia(let id, let path, let gif, let width, let height, let created, let savedAutomatically):
            let media = MediaCapture(
                id: id, url: URL(fileURLWithPath: path), format: gif ? .gif : .video,
                pointSize: CGSize(width: width, height: height), createdAt: created,
                savedAutomatically: savedAutomatically
            )
            EditorManager.shared.open(media)
        case .showHistory:
            EditorManager.shared.showHistory()
        case .clipboardOwner(let id):
            clipboardOwner = id
        case .exitAccepted:
            guard EditorManager.shared.isIdle, outbox.isEmpty else { return }
            channel?.drain()
            exit(0)
        case .probe(let command):
            EditorProbe.run(command)
        }
        maybeExit()
    }
}
