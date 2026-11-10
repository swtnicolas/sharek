import ServiceManagement

@MainActor
enum LoginItem {
    static var isEnabled: Bool {
        SMAppService.mainApp.status == .enabled
    }

    static func setEnabled(_ enabled: Bool) {
        let service = SMAppService.mainApp
        do {
            if enabled {
                try service.register()
            } else {
                try service.unregister()
            }
        } catch {
            Toast.show("No se pudo cambiar el inicio de sesión", detail: error.localizedDescription)
        }
        if service.status == .requiresApproval {
            SMAppService.openSystemSettingsLoginItems()
        }
    }

    static func enableOnFirstLaunch() {
        guard !Settings.loginItemConfigured else { return }
        Settings.loginItemConfigured = true
        setEnabled(true)
    }
}
