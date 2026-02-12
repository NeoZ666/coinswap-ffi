// swift-tools-version: 5.7
import PackageDescription

let package = Package(
    name: "Coinswap",
    platforms: [
        .iOS(.v13),
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "Coinswap",
            targets: ["Coinswap"]
        )
    ],
    targets: [
        .target(
            name: "CoinswapFFI",
            path: "Sources/CoinswapFFI",
            publicHeadersPath: "include"
        ),
        .target(
            name: "Coinswap",
            dependencies: ["CoinswapFFI"],
            path: "Sources/Coinswap",
            linkerSettings: [
                .linkedLibrary("coinswap_ffi"),
                .unsafeFlags(["-L", "Sources/CoinswapFFI"])
            ]
        ),
        .testTarget(
            name: "CoinswapTests",
            dependencies: ["Coinswap"],
            path: "Tests/CoinswapTests"
        )
    ]
)
