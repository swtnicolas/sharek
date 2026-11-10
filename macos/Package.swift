// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "ShareK",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(
            name: "ShareK",
            path: "Sources/ShareK",
            linkerSettings: [.linkedLibrary("z")]
        ),
        .testTarget(name: "ShareKTests", dependencies: ["ShareK"], path: "Tests/ShareKTests")
    ]
)
