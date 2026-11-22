// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "EqSwiftSample",
    platforms: [.macOS(.v13), .iOS(.v13)],
    dependencies: [.package(name: "EqSwift", path: "Package")],
    targets: [
        .executableTarget(
            name: "EqSwiftSmoke",
            dependencies: [.product(name: "EqSwift", package: "EqSwift")]
        )
    ]
)
