# eqswift

Export Rust APIs to Swift with UniFFI, then build a Swift package containing an Apple XCFramework with `cargo eqswift package`. Consumers need neither Rust nor a custom build script. The Rust interface uses proc macros: no UDL files or `build.rs`.

## Build an Apple Swift package

### 1. Install the tool

This workflow requires macOS, full Xcode selected with `xcode-select`, and Rust installed through rustup. No signing identity is needed to create the package.

The packaging command is currently available from this checkout, not the published 0.1.3 CLI:

```bash
cargo install --path cargo-eqswift --locked
rustup target add aarch64-apple-darwin x86_64-apple-darwin \
  aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

### 2. Configure your Rust library

```toml
[package]
name = "my-rust-api"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib", "cdylib", "rlib"]

[dependencies]
eqswift = "0.1.3"
uniffi = "=0.32.1"
```

UniFFI must be a direct dependency because its macros emit `::uniffi` paths. `cargo-eqswift` owns the generator, so your library needs no `uniffi-bindgen` binary or `cli` feature. The CLI uses UniFFI 0.32.1. Keep your runtime compatible with that generator.

```rust
// src/lib.rs
eqswift::setup!();

#[eqswift::export]
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

#[derive(eqswift::Record)]
pub struct Person {
    pub name: String,
    pub age: u32,
}

#[derive(eqswift::Object)]
pub struct Greeter;

#[eqswift::export]
impl Greeter {
    #[uniffi::constructor]
    pub fn new() -> Self { Self }

    pub fn greet_person(&self, person: Person) -> String {
        format!("Hello, {}! You are {} years old.", person.name, person.age)
    }
}
```

### 3. Package the library

Run from your Rust crate, or select its manifest and workspace package explicitly:

```bash
cargo eqswift package --name MyRustAPI --out-dir dist/MyRustAPI
# From another directory:
cargo eqswift package --manifest-path path/to/Cargo.toml --package my-rust-api \
  --name MyRustAPI --out-dir dist/MyRustAPI
```

The command builds release static libraries for five targets and combines them into three platform slices:

- macOS: `aarch64-apple-darwin` and `x86_64-apple-darwin`.
- iOS device: `aarch64-apple-ios`.
- iOS Simulator: `aarch64-apple-ios-sim` and `x86_64-apple-ios`. Despite its name, the latter is a simulator target.

Bindings are read from the actual static archives. Each slice must export the same Swift interface. Target-dependent exports fail packaging rather than silently shipping incompatible bindings.

The result contains `Package.swift`, `Sources/MyRustAPI/*.swift`, and `Artifacts/MyRustAPIFFI.xcframework`. Headers and Clang module maps travel inside the XCFramework. You can move the entire directory without the Cargo target directory or Rust source tree.

Output paths must not already exist. Packaging uses a temporary sibling directory and publishes the finished package only after assembly succeeds. To rebuild, choose a new versioned directory or remove your disposable generated output first.

### 4. Consume from Swift

In Xcode, add the generated directory as a local package and select the `MyRustAPI` product. In another Swift package:

```swift
// In Package.swift:
dependencies: [.package(path: "../dist/MyRustAPI")],
targets: [
    .executableTarget(
        name: "App",
        dependencies: [.product(name: "MyRustAPI", package: "MyRustAPI")]
    )
]
```

```swift
import MyRustAPI

print(greet(name: "香港 🦀"))
let person = Person(name: "Alice", age: 30)
print(Greeter().greetPerson(person: person))
```

The generated package uses a local SwiftPM binary target, not unsafe linker flags, `DYLD_LIBRARY_PATH`, or consumer-side Cargo commands. Keep the generated `Package.swift`, `Sources`, and `Artifacts` together. Remote ZIP hosting, checksums, signing, notarization, and publication are not automated.

## Command reference

- `cargo eqswift package`: release XCFramework and Swift package. Defaults to `swift/Package`; `--name` defaults to the Rust library target name with hyphens replaced by underscores.
- `package --targets aarch64-apple-darwin`: package a subset of supported targets. Consumers can build only for included architectures and platforms.
- `package --debug`: use debug Rust libraries.
- `cargo eqswift build`: build the selected library and emit raw Swift bindings. Works on Linux and macOS without Xcode.
- `cargo eqswift swift`: emit raw bindings from existing artifacts. `--release`, `--target`, `--static`, and `--out-dir` select the input and output. `--static` prefers an archive; it does not create an XCFramework.
- All three commands accept `--manifest-path`, `--package`, `--features feature1,feature2`, `--no-default-features`, `--locked`, and `--config path/to/uniffi.toml`.
- `build -- <cargo arguments>` passes additional arguments to Cargo. Use the named eqswift options for target/profile/features so artifact lookup matches the build.

Per-crate `uniffi.toml` configuration is loaded through Cargo metadata, including renamed Swift modules. `--config` overrides it. The generated product name controls the Swift import; it does not rename Rust symbols. Packages with more than one UniFFI component include all generated sources and module maps.

The generated manifest declares macOS 13 and iOS 13 minimums. Use deployment targets compatible with your Rust dependencies. If you build with higher `MACOSX_DEPLOYMENT_TARGET` or `IPHONEOS_DEPLOYMENT_TARGET`, raise the generated manifest's platform minimums to match. Native dependencies still need their cross-compilation toolchains and SDK support. Additional native libraries or frameworks may require linker settings in the generated manifest. tvOS, watchOS, visionOS, and Mac Catalyst are not supported by this packaging command.

## Run the repository examples

On macOS, after installing the CLI and targets above:

```bash
cargo eqswift package --manifest-path examples/demo/Cargo.toml \
  --name DemoKit --out-dir examples/demo/swift/DemoKit
swift run --package-path examples/swift-consumer Consumer
```

Expected output: `PASS: Swift consumer called Rust; Unicode, records, object creation/drop, and toggles`.

The repo's `swift/` directory is another consumer, not a build-script workaround:

```bash
cargo eqswift package --package eqswift --name EqSwift --out-dir swift/Package
swift run --package-path swift EqSwiftSmoke
```

For raw binding generation on Linux:

```bash
cargo eqswift build --manifest-path examples/demo/Cargo.toml --out-dir target/demo-swift
```

## Troubleshooting

- **Missing Rust targets:** run the exact `rustup target add ...` command printed by eqswift, using the active toolchain.
- **Xcode unavailable:** install full Xcode, accept its license, and select its developer directory with `xcode-select`. Command Line Tools alone cannot create XCFrameworks.
- **Ambiguous workspace:** pass `--package`. Dependency crates are never guessed as the workspace library.
- **Missing static archive:** include `staticlib` in `[lib].crate-type`.
- **Binding metadata/version error:** use UniFFI compatible with the CLI's 0.32.1 generator. Do not mix generated sources from different builds.
- **Different interfaces between targets:** remove platform-specific exported APIs or produce separate packages for those platforms.
- **Output already exists:** choose a new `--out-dir`. eqswift never overwrites an existing package.
- **No module maps:** keep UniFFI Swift `generate_module_map` enabled.
- **Unsupported architecture in Xcode:** rebuild the package with the required target. A macOS-only package cannot build an iOS app.

## How it works

`eqswift::setup!()` and `#[eqswift::export]` wrap UniFFI's proc macros. `Record`, `Object`, `Enum`, and `Error` re-export UniFFI derives. UniFFI owns the generated ABI, conversion code, and object lifetime machinery. eqswift owns Cargo package selection, generation, Apple slice assembly, and the consumable Swift package layout. Product transports and app logic do not belong in the framework.

The earlier 0.1.2 workflow deliberately removed transport code but stopped at raw bindings and linker hints. The 0.1.3 maintenance work verified generation, not Swift compilation or XCFramework distribution. The new packaging workflow closes that tooling gap without replacing UniFFI.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --features cli -- -D warnings
cargo test --locked --workspace --features cli
```

On macOS, install all five Rust targets first. `cargo test -p cargo-eqswift --test workflow` packages the real demo without a bindgen binary, moves its output, and runs a separate SwiftPM consumer. It also builds that consumer with Xcode for generic iOS device and simulator destinations without signing. Set `EQSWIFT_TEST_SIMULATOR` to an already booted simulator UUID to run the consumer inside that simulator as well. CI installs the targets and runs the packaging tests.

Verified on an Apple Silicon Mac satellite with Xcode 27.3: all five Rust targets, macOS Swift execution, iOS device and both simulator-architecture links, and arm64 iOS Simulator execution. Intel macOS execution, physical iOS device execution, signing, App Store distribution, and older deployment-target runtime behavior are not verified.

## Proposed FFI ownership contract

The [cross-family FFI ownership contract](https://github.com/softwarecorporation/equilibrium/blob/main/docs/FFI-OWNERSHIP-PROPOSAL.md) is a proposal, not a claim that eqswift conforms or interoperates with other projects. The packaging smoke consumer does not establish full ownership-contract conformance. Before an adapter opts in, verify string and object ownership, error cleanup, callback shutdown, and incompatible contract-version rejection where supported.

## License

ISC
