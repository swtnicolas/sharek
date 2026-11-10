import AppKit

enum MenuAction {
    static let region = "region"
    static let screen = "screen"
    static let window = "window"
    static let scrolling = "scrolling"
    static let text = "text"
    static let pin = "pin"
    static let video = "video"
    static let gif = "gif"
    static let color = "color"
    static let ruler = "ruler"
    static let history = "history"
    static let quit = "quit"
}

@MainActor
enum MainMenu {
    static func build(target: AnyObject) -> NSMenu {
        let main = NSMenu()

        let app = submenu(in: main, title: "ShareK")
        app.addItem(withTitle: "Acerca de ShareK", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Ocultar ShareK", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let quit = app.addItem(withTitle: "Salir de ShareK", action: #selector(CaptureCoordinator.performMenuAction(_:)), keyEquivalent: "q")
        quit.representedObject = MenuAction.quit
        quit.target = target

        let file = submenu(in: main, title: "Archivo")
        file.addItem(withTitle: "Guardar", action: #selector(CapturePage.saveCapture(_:)), keyEquivalent: "s")
        file.addItem(withTitle: "Guardar como…", action: #selector(CapturePage.saveCaptureAs(_:)), keyEquivalent: "S")
        file.addItem(.separator())
        file.addItem(withTitle: "Cerrar pestaña", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        file.addItem(withTitle: "Descartar", action: #selector(CapturePage.discardCapture(_:)), keyEquivalent: "\u{8}")

        let edit = submenu(in: main, title: "Edición")
        edit.addItem(withTitle: "Deshacer", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Rehacer", action: Selector(("redo:")), keyEquivalent: "Z")
        edit.addItem(.separator())
        edit.addItem(withTitle: "Copiar", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Pegar", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Seleccionar todo", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")

        let view = submenu(in: main, title: "Visualización")
        view.addItem(withTitle: "Acercar", action: #selector(CapturePage.zoomIn(_:)), keyEquivalent: "+")
        view.addItem(withTitle: "Alejar", action: #selector(CapturePage.zoomOut(_:)), keyEquivalent: "-")
        view.addItem(withTitle: "Tamaño real", action: #selector(CapturePage.zoomActualSize(_:)), keyEquivalent: "0")
        view.addItem(withTitle: "Ajustar a la ventana", action: #selector(CapturePage.zoomToFit(_:)), keyEquivalent: "9")

        let capture = submenu(in: main, title: "Captura")
        let captureModifiers: NSEvent.ModifierFlags = [.command, .shift]
        let toolModifiers: NSEvent.ModifierFlags = [.control, .shift]
        for (title, key, modifiers, action) in [
            ("Capturar región", "4", captureModifiers, MenuAction.region),
            ("Capturar pantalla completa", "3", captureModifiers, MenuAction.screen),
            ("Capturar ventana activa", "5", captureModifiers, MenuAction.window),
            ("Captura con desplazamiento", "8", toolModifiers, MenuAction.scrolling),
            ("Capturar texto (OCR)", "0", toolModifiers, MenuAction.text),
            ("Fijar región en pantalla", "9", toolModifiers, MenuAction.pin),
            ("Grabar video (MP4)", "6", toolModifiers, MenuAction.video),
            ("Grabar GIF", "7", toolModifiers, MenuAction.gif),
            ("Selector de color", "1", toolModifiers, MenuAction.color),
            ("Regla", "2", toolModifiers, MenuAction.ruler),
            ("Historial…", "h", toolModifiers, MenuAction.history),
        ] {
            let item = capture.addItem(withTitle: title, action: #selector(CaptureCoordinator.performMenuAction(_:)), keyEquivalent: key)
            item.keyEquivalentModifierMask = modifiers
            item.representedObject = action
            item.target = target
        }

        let window = submenu(in: main, title: "Ventana")
        window.addItem(withTitle: "Minimizar", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        NSApp.windowsMenu = window

        return main
    }

    private static func submenu(in main: NSMenu, title: String) -> NSMenu {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        let menu = NSMenu(title: title)
        item.submenu = menu
        main.addItem(item)
        return menu
    }
}
