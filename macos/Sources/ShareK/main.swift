import AppKit

let mode = CommandLine.arguments.dropFirst().first

if let mode, ["--editor", "--ocr"].contains(mode), getenv(Memory.allocatorMode) == nil, let path = Bundle.main.executablePath {
    setenv(Memory.allocatorMode, "1", 1)
    execv(path, CommandLine.unsafeArgv)
}

signal(SIGPIPE, SIG_IGN)

switch mode {
case "--editor":
    let delegate = EditorAppDelegate()
    NSApplication.shared.delegate = delegate
    NSApplication.shared.setActivationPolicy(.regular)
    withExtendedLifetime(delegate) { NSApplication.shared.run() }
case "--ocr":
    TextRecognizer.runHelper()
default:
    let delegate = AppDelegate()
    NSApplication.shared.delegate = delegate
    NSApplication.shared.setActivationPolicy(.accessory)
    withExtendedLifetime(delegate) { NSApplication.shared.run() }
}
