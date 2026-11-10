import AppKit

@MainActor
final class StatusMenu: NSObject, NSMenuDelegate {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let coordinator: CaptureCoordinator
    private let menu = NSMenu()
    private var isRecording = false

    init(coordinator: CaptureCoordinator) {
        self.coordinator = coordinator
        super.init()
        item.button?.image = SwirlIcon.menuBarImage()
        menu.delegate = self
        menu.autoenablesItems = false
        item.menu = menu
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        if isRecording {
            menu.addItem(entry("Detener", #selector(stopRecording)))
            menu.addItem(.separator())
        }
        menu.addItem(entry("Capturar región", #selector(region), key: "4", modifiers: [.command, .shift]))
        menu.addItem(entry("Capturar pantalla completa", #selector(fullScreen), key: "3", modifiers: [.command, .shift]))
        menu.addItem(entry("Capturar ventana activa", #selector(activeWindow), key: "5", modifiers: [.command, .shift]))
        menu.addItem(entry("Captura con desplazamiento", #selector(scrollingCapture), key: "8"))
        menu.addItem(entry("Capturar texto (OCR)", #selector(recognizeText), key: "0"))
        menu.addItem(.separator())
        menu.addItem(entry("Fijar región en pantalla", #selector(pinRegion), key: "9"))
        menu.addItem(entry("Fijar imagen del portapapeles", #selector(pinClipboard)))
        let closePins = entry("Cerrar imágenes fijadas", #selector(closePins))
        closePins.isEnabled = PinManager.hasPins
        menu.addItem(closePins)
        menu.addItem(.separator())
        menu.addItem(entry("Grabar video (MP4)", #selector(recordVideo), key: "6"))
        menu.addItem(entry("Grabar GIF", #selector(recordGIF), key: "7"))
        menu.addItem(.separator())
        menu.addItem(entry("Selector de color", #selector(pickColor), key: "1"))
        menu.addItem(entry("Regla", #selector(measure), key: "2"))
        menu.addItem(entry("Historial…", #selector(showHistory), key: "h"))
        menu.addItem(.separator())
        menu.addItem(toggle("Copiar al portapapeles al capturar", Settings.copyOnCapture, #selector(toggleCopy)))
        menu.addItem(toggle("Guardar automáticamente", Settings.autoSave, #selector(toggleAutoSave)))
        menu.addItem(toggle("Abrir editor al capturar", Settings.openEditor, #selector(toggleEditor)))
        menu.addItem(toggle("Incluir cursor en capturas", Settings.showsCursor, #selector(toggleCursor)))
        menu.addItem(toggle("Grabar audio del sistema", Settings.recordsAudio, #selector(toggleAudio)))
        menu.addItem(toggle("Abrir al iniciar sesión", LoginItem.isEnabled, #selector(toggleLaunchAtLogin)))
        menu.addItem(.separator())
        menu.addItem(entry("Elegir carpeta de guardado…", #selector(chooseFolder)))
        menu.addItem(entry("Abrir carpeta de guardado", #selector(openFolder)))
        menu.addItem(.separator())
        menu.addItem(entry("Salir de ShareK", #selector(quit), key: "q", modifiers: .command))
    }

    private func entry(_ title: String, _ action: Selector, key: String = "", modifiers: NSEvent.ModifierFlags = [.control, .shift]) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = self
        return item
    }

    private func toggle(_ title: String, _ value: Bool, _ action: Selector) -> NSMenuItem {
        let item = entry(title, action)
        item.state = value ? .on : .off
        return item
    }

    @objc private func region() { coordinator.captureRegion(afterMenu: true) }
    @objc private func fullScreen() { coordinator.captureFullScreen(afterMenu: true) }
    @objc private func activeWindow() { coordinator.captureActiveWindow(afterMenu: true) }
    @objc private func toggleCopy() { Settings.copyOnCapture.toggle() }
    @objc private func toggleAutoSave() { Settings.autoSave.toggle() }
    @objc private func toggleEditor() { Settings.openEditor.toggle() }
    @objc private func toggleCursor() { Settings.showsCursor.toggle() }
    @objc private func toggleAudio() { Settings.recordsAudio.toggle() }
    @objc private func toggleLaunchAtLogin() { LoginItem.setEnabled(!LoginItem.isEnabled) }
    @objc private func recordVideo() { coordinator.toggleRecording(.video, afterMenu: true) }
    @objc private func recordGIF() { coordinator.toggleRecording(.gif, afterMenu: true) }
    @objc private func scrollingCapture() { coordinator.toggleScrollingCapture(afterMenu: true) }
    @objc private func recognizeText() { coordinator.recognizeText(afterMenu: true) }
    @objc private func pinRegion() { coordinator.pinRegion(afterMenu: true) }
    @objc private func pickColor() { coordinator.pickColor() }
    @objc private func measure() { coordinator.measure(afterMenu: true) }
    @objc private func showHistory() { EditorProcess.shared.showHistory() }
    @objc private func pinClipboard() { coordinator.pinClipboardImage() }
    @objc private func closePins() { PinManager.closeAll() }
    @objc private func stopRecording() { coordinator.stopRecording() }

    func setRecording(_ recording: Bool) {
        isRecording = recording
        guard let button = item.button else { return }
        button.image = recording ? SwirlIcon.stopImage() : SwirlIcon.menuBarImage()
    }
    @objc private func quit() { NSApp.terminate(nil) }

    @objc private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = Settings.saveDirectory
        panel.prompt = "Elegir"
        NSApp.activate()
        if panel.runModal() == .OK, let url = panel.url {
            Settings.saveDirectory = url
        }
    }

    @objc private func openFolder() {
        let directory = Settings.saveDirectory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        NSWorkspace.shared.open(directory)
    }
}
