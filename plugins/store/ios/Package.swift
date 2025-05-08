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
            type: .static,          // Original static configuration
            targets: ["StorePlugin"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "StorePlugin",
            dependencies: ["Tauri"],
            path: "Sources")
    ]
)