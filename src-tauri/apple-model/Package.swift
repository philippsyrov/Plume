// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "PlumeAppleModel",
    platforms: [.macOS(.v26)],
    products: [
        .executable(name: "plume-apple-model", targets: ["PlumeAppleModel"]),
    ],
    targets: [
        .executableTarget(
            name: "PlumeAppleModel",
            linkerSettings: [.linkedFramework("FoundationModels")],
        ),
        .testTarget(name: "PlumeAppleModelTests", dependencies: ["PlumeAppleModel"]),
    ],
)
