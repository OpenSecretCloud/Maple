// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MapleSDK",
    platforms: [.iOS("26.0"), .macOS(.v15)],
    products: [.library(name: "MapleSDK", targets: ["MapleSDK"])],
    targets: [
        .target(name: "MapleSDK", resources: [.process("Trust/Resources")]),
        .testTarget(name: "MapleSDKTests", dependencies: ["MapleSDK"], resources: [.process("Trust/Fixtures")]),
    ]
)
