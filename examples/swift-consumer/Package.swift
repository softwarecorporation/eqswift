// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "EqSwiftConsumer",
    platforms: [.macOS(.v13), .iOS(.v13)],
    dependencies: [.package(path: "../demo/swift/DemoKit")],
    targets: [
        .executableTarget(
            name: "Consumer",
            dependencies: [.product(name: "DemoKit", package: "DemoKit")]
        )
    ]
)
