// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "CodexSwitch",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "CodexSwitch", targets: ["CodexSwitch"])],
    targets: [.executableTarget(name: "CodexSwitch")]
)
