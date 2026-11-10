import Carbon.HIToolbox

@MainActor
final class HotKeyCenter {
    private var actions: [UInt32: () -> Void] = [:]
    private var references: [EventHotKeyRef] = []
    private var handler: EventHandlerRef?
    private let signature: OSType = 0x5348_4B48

    @discardableResult
    func register(keyCode: UInt32, modifiers: UInt32, action: @escaping () -> Void) -> Bool {
        installHandlerIfNeeded()
        let id = UInt32(actions.count + 1)
        var reference: EventHotKeyRef?
        let status = RegisterEventHotKey(
            keyCode, modifiers, EventHotKeyID(signature: signature, id: id),
            GetApplicationEventTarget(), 0, &reference
        )
        guard status == noErr, let reference else { return false }
        references.append(reference)
        actions[id] = action
        return true
    }

    private func fire(_ id: UInt32) {
        actions[id]?()
    }

    private func installHandlerIfNeeded() {
        guard handler == nil else { return }
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(
            GetApplicationEventTarget(),
            { _, event, userData in
                guard let event, let userData else { return OSStatus(eventNotHandledErr) }
                var hotKeyID = EventHotKeyID()
                GetEventParameter(
                    event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
                    nil, MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID
                )
                let center = Unmanaged<HotKeyCenter>.fromOpaque(userData).takeUnretainedValue()
                MainActor.assumeIsolated { center.fire(hotKeyID.id) }
                return noErr
            },
            1, &spec, Unmanaged.passUnretained(self).toOpaque(), &handler
        )
    }
}
