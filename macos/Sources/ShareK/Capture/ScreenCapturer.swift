import AppKit
import CoreMedia
import ScreenCaptureKit

enum CaptureError: LocalizedError {
    case displayNotFound
    case windowNotFound
    case unreadable

    var errorDescription: String? {
        switch self {
        case .displayNotFound: "No se encontró la pantalla."
        case .windowNotFound: "No se encontró la ventana activa."
        case .unreadable: "La captura no tiene un formato compatible."
        }
    }
}

extension NSScreen {
    var displayID: CGDirectDisplayID {
        (deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value ?? 0
    }

    static var underMouse: NSScreen? {
        let location = NSEvent.mouseLocation
        return screens.first { NSMouseInRect(location, $0.frame, false) } ?? main
    }
}

@MainActor
enum ScreenCapturer {
    static func content() async throws -> SCShareableContent {
        try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
    }

    static func capture(screen: NSScreen, in content: SCShareableContent) async throws -> SurfaceImage {
        guard let display = content.displays.first(where: { $0.displayID == screen.displayID }) else {
            throw CaptureError.displayNotFound
        }
        return try await image(for: SCContentFilter(display: display, excludingWindows: []))
    }

    static func capture(windowID: CGWindowID, in content: SCShareableContent) async throws -> SurfaceImage {
        guard let window = content.windows.first(where: { $0.windowID == windowID }) else {
            throw CaptureError.windowNotFound
        }
        return try await image(for: SCContentFilter(desktopIndependentWindow: window))
    }

    private static func image(for filter: SCContentFilter) async throws -> SurfaceImage {
        let scale = CGFloat(filter.pointPixelScale)
        let configuration = SCStreamConfiguration()
        configuration.width = Int((filter.contentRect.width * scale).rounded())
        configuration.height = Int((filter.contentRect.height * scale).rounded())
        configuration.showsCursor = Settings.showsCursor
        configuration.captureResolution = .best
        configuration.pixelFormat = kCVPixelFormatType_32BGRA
        configuration.colorSpaceName = CGColorSpace.displayP3
        let sample = try await SCScreenshotManager.captureSampleBuffer(contentFilter: filter, configuration: configuration)
        guard let buffer = CMSampleBufferGetImageBuffer(sample),
              let image = SurfaceImage(buffer, colorSpace: CGColorSpace(name: CGColorSpace.displayP3))
        else { throw CaptureError.unreadable }
        return image
    }
}
