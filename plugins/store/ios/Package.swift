// swift-tools-version:5.3

import PackageDescription

let package = Package(
    name: "store",
    platforms: [
        .iOS(.v13)
    ],
    products: [
        .library(
            name: "store",
            type: .static,          // Make SwiftPM emit libstore.a
            targets: ["StorePlugin"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "StorePlugin",
            dependencies: ["Tauri"],
            path: "Sources"),
    ]
)