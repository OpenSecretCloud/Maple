// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MapleApp",
    defaultLocalization: "en",
    platforms: [.iOS("26.0"), .macOS(.v15)],
    products: [
        .library(name: "MapleAppCore", targets: ["MapleAppCore"]),
        .library(name: "MapleAppUI", targets: ["MapleAppUI"]),
    ],
    dependencies: [
        .package(path: "../../sdk/swift"),
        .package(path: "Vendor/textual"),
        .package(
            url: "https://github.com/gonzalezreal/swiftui-math.git",
            revision: "0b5c2cfaaec8d6193db206f675048eeb5ce95f71"),
        .package(
            url: "https://github.com/pointfreeco/swift-concurrency-extras.git",
            revision: "82a4ae7170d98d8538ec77238b7eb8e7199ef2e8"
        ),
    ],
    targets: [
        .target(name: "MapleAppCore", dependencies: [.product(name: "MapleSDK", package: "swift")]),
        .target(
            name: "MapleAppUI",
            dependencies: [
                "MapleAppCore", .product(name: "MapleSDK", package: "swift"),
                .product(name: "Textual", package: "textual"),
                .product(name: "SwiftUIMath", package: "swiftui-math"),
                .product(name: "ConcurrencyExtras", package: "swift-concurrency-extras"),
            ],
            resources: [
                .process("Resources/Localizable.xcstrings"), .process("Resources/Auth.xcstrings"),
                .process("Resources/Library.xcstrings"),
                .process("Settings/Settings.xcstrings"),
                .process("Dictation/Dictation.xcstrings"), .process("Audio/Audio.xcstrings"),
                .copy("Resources/EquationRenderer"),
                .copy("Resources/ThirdPartyNotices"),
                .copy("Resources/Fonts"),
                .process("Resources/BrandAssets.xcassets"),
            ]
        ),
        .testTarget(
            name: "MapleAppCoreTests",
            dependencies: ["MapleAppCore", .product(name: "MapleSDK", package: "swift")]
        ),
        .testTarget(
            name: "MapleAppUITests", dependencies: ["MapleAppUI", .product(name: "Textual", package: "textual")]),
    ]
)
