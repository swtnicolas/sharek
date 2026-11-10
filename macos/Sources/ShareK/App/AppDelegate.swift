import AppKit
import Carbon.HIToolbox

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let coordinator = CaptureCoordinator()
    private let hotKeys = HotKeyCenter()
    private var statusMenu: StatusMenu?

    func applicationDidFinishLaunching(_ notification: Notification) {
        MediaCapture.clearTemporaryFiles()
        NSApp.mainMenu = MainMenu.build(target: coordinator)
        EditorProcess.shared.onAction = { [coordinator] action in coordinator.performAction(action) }
        let statusMenu = StatusMenu(coordinator: coordinator)
        self.statusMenu = statusMenu
        coordinator.onRecordingChange = { [weak statusMenu] recording in statusMenu?.setRecording(recording) }

        LoginItem.enableOnFirstLaunch()
        DistributedNotificationCenter.default().addObserver(self, selector: #selector(quitRequested), name: Notification.Name("local.sharek.quit"), object: nil)

        let captureModifiers = UInt32(cmdKey | shiftKey)
        let modifiers = UInt32(controlKey | shiftKey)
        let bindings: [(Int, UInt32, String, () -> Void)] = [
            (kVK_ANSI_4, captureModifiers, "⇧⌘4", { [coordinator] in coordinator.captureRegion() }),
            (kVK_ANSI_3, captureModifiers, "⇧⌘3", { [coordinator] in coordinator.captureFullScreen() }),
            (kVK_ANSI_5, captureModifiers, "⇧⌘5", { [coordinator] in coordinator.captureActiveWindow() }),
            (kVK_ANSI_6, modifiers, "⌃⇧6", { [coordinator] in coordinator.toggleRecording(.video) }),
            (kVK_ANSI_7, modifiers, "⌃⇧7", { [coordinator] in coordinator.toggleRecording(.gif) }),
            (kVK_ANSI_8, modifiers, "⌃⇧8", { [coordinator] in coordinator.toggleScrollingCapture() }),
            (kVK_ANSI_9, modifiers, "⌃⇧9", { [coordinator] in coordinator.pinRegion() }),
            (kVK_ANSI_0, modifiers, "⌃⇧0", { [coordinator] in coordinator.recognizeText() }),
            (kVK_ANSI_1, modifiers, "⌃⇧1", { [coordinator] in coordinator.pickColor() }),
            (kVK_ANSI_2, modifiers, "⌃⇧2", { [coordinator] in coordinator.measure() }),
            (kVK_ANSI_H, modifiers, "⌃⇧H", { EditorProcess.shared.showHistory() }),
        ]
        let missing = bindings.filter { !hotKeys.register(keyCode: UInt32($0.0), modifiers: $0.1, action: $0.3) }.map { $0.2 }
        if !missing.isEmpty {
            Toast.show("No se pudieron registrar los atajos globales", detail: "Otra app ya usa \(missing.joined(separator: ", ")).")
        }
    }

    @objc private func quitRequested() {
        NSApp.terminate(nil)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationWillTerminate(_ notification: Notification) {
        coordinator.discardForQuit()
        ClipboardManager.shared.flush()
    }
}
