// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "ApplePlatform",
    platforms: [
        .macOS(.v14), .iOS(.v17)
    ],
    products: [
        .library(name: "Kit", targets: ["Kit"]),
        .library(name: "Audio", targets: ["Audio"]),
        .library(name: "Director", targets: ["Director"]),
        .library(name: "Actor", targets: ["Actor"]),
        .library(name: "Stage", targets: ["Stage"]),
        .executable(name: "ProsodiaCLI", targets: ["CLI"])
    ],
    dependencies: [
        .package(url: "https://github.com/google-ai-edge/LiteRT-LM", branch: "main")
    ],
    targets: [
        // FFI Binary targets
        .binaryTarget(
            name: "folioparserFFI",
            path: "folioparserFFI.xcframework"
        ),
        .binaryTarget(
            name: "directorFFI",
            path: "directorFFI.xcframework"
        ),
        .binaryTarget(
            name: "actorFFI",
            path: "actorFFI.xcframework"
        ),
        .binaryTarget(
            name: "stageFFI",
            path: "stageFFI.xcframework"
        ),
        
        // Swift modules
        .target(
            name: "Kit",
            dependencies: [
                "folioparserFFI",
                "directorFFI",
                "actorFFI",
                "stageFFI"
            ],
            path: "Sources/Kit"
        ),
        .target(
            name: "Audio",
            dependencies: [],
            path: "Sources/Audio"
        ),
        .target(
            name: "Stage",
            dependencies: [
                "Kit",
                "Audio"
            ],
            path: "Sources/Stage"
        ),
        .target(
            name: "Director",
            dependencies: [
                "Stage",
                "Kit",
                .product(name: "LiteRTLM", package: "LiteRT-LM")
            ],
            path: "Sources/Director"
        ),
        .target(
            name: "Actor",
            dependencies: [
                "Stage",
                "Kit",
                "Audio",
                .product(name: "LiteRTLM", package: "LiteRT-LM")
            ],
            path: "Sources/Actor"
        ),
        .executableTarget(
            name: "CLI",
            dependencies: [
                "Actor",
                "Kit"
            ],
            path: "Sources/CLI"
        )
    ]
)
