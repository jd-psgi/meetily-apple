// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "apple-speech-helper",
    platforms: [.macOS(.v26)],
    targets: [
        .executableTarget(
            name: "apple-speech-helper",
            path: "Sources/apple-speech-helper"
        )
    ]
)
