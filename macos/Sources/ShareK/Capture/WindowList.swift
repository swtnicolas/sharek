import AppKit

struct OnScreenWindow {
    let id: CGWindowID
    let ownerPID: pid_t
    let layer: Int
    let frame: CGRect
}

enum WindowList {
    static func current() -> [OnScreenWindow] {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
            return []
        }
        let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
        return list.compactMap { info in
            guard let id = info[kCGWindowNumber as String] as? CGWindowID,
                  let pid = info[kCGWindowOwnerPID as String] as? pid_t,
                  let layer = info[kCGWindowLayer as String] as? Int,
                  let boundsInfo = info[kCGWindowBounds as String] as? NSDictionary,
                  let bounds = CGRect(dictionaryRepresentation: boundsInfo),
                  (0...25).contains(layer),
                  (info[kCGWindowAlpha as String] as? Double ?? 1) > 0,
                  bounds.width > 4, bounds.height > 4
            else { return nil }
            let frame = CGRect(x: bounds.minX, y: primaryHeight - bounds.maxY, width: bounds.width, height: bounds.height)
            return OnScreenWindow(id: id, ownerPID: pid, layer: layer, frame: frame)
        }
    }

    static func frontmostAppWindow() -> OnScreenWindow? {
        guard let pid = NSWorkspace.shared.frontmostApplication?.processIdentifier else { return nil }
        return current().first { $0.ownerPID == pid && $0.layer == 0 }
    }
}
