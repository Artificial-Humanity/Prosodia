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
        .library(name: "Misaki", targets: ["Misaki"]),
        .library(name: "ActorEspeak", targets: ["ActorEspeak"])
    ],
    dependencies: [
        .package(url: "https://github.com/ml-explore/mlx-swift", revision: "dc43e62d7055353c7f99fa071a4e71d29dfddc44"),
        .package(url: "https://github.com/ml-explore/mlx-swift-lm", revision: "ee5320ddcf8cdc2765165e0350b1f9a76362a24a"),
        .package(url: "https://github.com/huggingface/swift-transformers", revision: "50843f91d5563ae2f448eaf1756f148f5a291f6e"),
        .package(name: "espeak-ng", path: "./Vendor/espeak-ng-spm"),
        .package(url: "https://github.com/weichsel/ZIPFoundation", from: "0.9.0"),
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
            name: "Misaki",
            path: "Sources/Misaki",
            resources: [
                .process("Resources")
            ]
        ),
        .target(
            name: "Stage",
            dependencies: [
                "Kit",
                .product(name: "ZIPFoundation", package: "ZIPFoundation")
            ],
            path: "Sources/Stage"
        ),
        .target(
            name: "Director",
            dependencies: [
                "Stage",
                "Kit",
                .product(name: "MLX", package: "mlx-swift"),
                .product(name: "MLXLLM", package: "mlx-swift-lm"),
                .product(name: "MLXLMCommon", package: "mlx-swift-lm"),
                .product(name: "MLXEmbedders", package: "mlx-swift-lm"),
                .product(name: "Tokenizers", package: "swift-transformers"),
                .product(name: "LiteRTLM", package: "LiteRT-LM")
            ],
            path: "Sources/Director"
        ),
        .target(
            name: "Actor",
            dependencies: [
                "Stage",
                "Kit",
                "Misaki",
                "Audio",
                .product(name: "MLX", package: "mlx-swift"),
                .product(name: "MLXFast", package: "mlx-swift"),
                .product(name: "MLXNN", package: "mlx-swift"),
                .product(name: "LiteRTLM", package: "LiteRT-LM")
            ],
            path: "Sources/Actor"
        ),
        .target(
            name: "ActorEspeak",
            dependencies: [
                "Actor",
                .product(name: "libespeak-ng", package: "espeak-ng"),
                .product(name: "espeak-ng-data", package: "espeak-ng")
            ],
            path: "Sources/ActorEspeak"
        )
    ]
)
