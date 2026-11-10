import Foundation

enum Settings {
    private static func bool(_ key: String, default value: Bool) -> Bool {
        UserDefaults.standard.object(forKey: key) as? Bool ?? value
    }

    static var copyOnCapture: Bool {
        get { bool("copyOnCapture", default: true) }
        set { UserDefaults.standard.set(newValue, forKey: "copyOnCapture") }
    }

    static var autoSave: Bool {
        get { bool("autoSave", default: false) }
        set { UserDefaults.standard.set(newValue, forKey: "autoSave") }
    }

    static var openEditor: Bool {
        get { bool("openEditor", default: true) }
        set { UserDefaults.standard.set(newValue, forKey: "openEditor") }
    }

    static var showsCursor: Bool {
        get { bool("showsCursor", default: false) }
        set { UserDefaults.standard.set(newValue, forKey: "showsCursor") }
    }

    static var recordsAudio: Bool {
        get { bool("recordsAudio", default: false) }
        set { UserDefaults.standard.set(newValue, forKey: "recordsAudio") }
    }

    static var loginItemConfigured: Bool {
        get { bool("loginItemConfigured", default: false) }
        set { UserDefaults.standard.set(newValue, forKey: "loginItemConfigured") }
    }

    static var saveDirectory: URL {
        get {
            if let path = UserDefaults.standard.string(forKey: "saveDirectory") {
                return URL(fileURLWithPath: path, isDirectory: true)
            }
            return FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Pictures/ShareK", isDirectory: true)
        }
        set { UserDefaults.standard.set(newValue.path, forKey: "saveDirectory") }
    }
}
