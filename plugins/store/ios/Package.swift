// swift-tools-version:5.5
// Requires iOS 15.0+

import PackageDescription

let package = Package(
    name: "store",
    platforms: [
        .iOS(.v15)
    ],
    products: [
        .library(
            name: "store",
            type: .dynamic,          // Using dynamic to ensure Swift runtime is properly linked
            targets: ["StorePlugin"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "StorePlugin",
            dependencies: ["Tauri"],
            path: "Sources",
            swiftSettings: [
                .unsafeFlags([
                    "-Xlinker", "-rpath", "-Xlinker", "@executable_path/Frameworks",
                    "-Xlinker", "-rpath", "-Xlinker", "/usr/lib/swift"
                ])
            ])
    ]
)