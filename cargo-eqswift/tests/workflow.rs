use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-eqswift"));
    command.current_dir(root()).arg("eqswift");
    command
}

fn success(command: &mut Command) -> Output {
    let output = command.output().expect("command should execute");
    assert!(
        output.status.success(),
        "{command:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn downstream_without_bindgen_binary() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("bindings");
    success(
        cli()
            .args([
                "build",
                "--manifest-path",
                "examples/demo/Cargo.toml",
                "--static",
                "--out-dir",
            ])
            .arg(&out),
    );
    let source = std::fs::read_to_string(out.join("eqswift_demo.swift")).unwrap();
    assert!(source.contains("public func greet"));
    assert!(source.contains("open class TaskManager"));
    let map = std::fs::read_to_string(out.join("module.modulemap")).unwrap();
    assert!(map.contains("module eqswift_demoFFI"));
    // A repeat generation must refresh the SPM map, not leave a stale copy.
    std::fs::write(out.join("module.modulemap"), "stale").unwrap();
    success(
        cli()
            .args([
                "swift",
                "--manifest-path",
                "examples/demo/Cargo.toml",
                "--out-dir",
            ])
            .arg(&out),
    );
    assert_eq!(
        map,
        std::fs::read_to_string(out.join("module.modulemap")).unwrap()
    );
}

#[test]
fn features_config_and_custom_library_name() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"
[package]
name = "eqswift-config-test"
version = "0.0.0"
edition = "2021"
[workspace]
[lib]
name = "different_bridge"
crate-type = ["staticlib"]
[features]
default = ["block-build"]
block-build = []
extra = []
[dependencies]
eqswift = {{ path = {:?} }}
uniffi = "=0.32.1"
"#,
            root().join("eq-swift")
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"
eqswift::setup!();
#[cfg(feature = "block-build")]
compile_error!("--no-default-features must reach cargo build");
#[cfg(feature = "extra")]
#[eqswift::export]
pub fn feature_export() -> u32 { 73 }
#[cfg(target_os = "ios")]
#[eqswift::export]
pub fn ios_only() -> bool { true }
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("uniffi.toml"),
        "[bindings.swift]\nmodule_name = 'CustomBridge'\nffi_module_name = 'CustomFFI'\n",
    )
    .unwrap();
    let manifest = temp.path().join("Cargo.toml");
    let out = temp.path().join("bindings");
    let mut command = cli();
    command
        .args(["build", "--manifest-path"])
        .arg(&manifest)
        .args(["--features", "extra", "--no-default-features", "--out-dir"])
        .arg(&out);
    success(&mut command);
    assert!(std::fs::read_to_string(out.join("CustomBridge.swift"))
        .unwrap()
        .contains("public func featureExport"));
    assert!(std::fs::read_to_string(out.join("module.modulemap"))
        .unwrap()
        .contains("module CustomFFI"));
    #[cfg(target_os = "macos")]
    {
        let packaged = temp.path().join("package");
        let output = cli()
            .args(["package", "--manifest-path"])
            .arg(&manifest)
            .args([
                "--features",
                "extra",
                "--no-default-features",
                "--targets",
                "aarch64-apple-darwin,aarch64-apple-ios",
                "--out-dir",
            ])
            .arg(&packaged)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("exported Swift interface differs"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!packaged.exists(), "a failed package must not be published");
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn apple_packaging_requires_macos() {
    let output = cli().arg("package").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires macOS with full Xcode"));
}

#[cfg(target_os = "macos")]
#[test]
fn xcframework_is_consumable_after_relocation() {
    let temp = tempfile::tempdir().unwrap();
    let built = temp.path().join("built");
    success(
        cli()
            .args([
                "package",
                "--manifest-path",
                "examples/demo/Cargo.toml",
                "--name",
                "DemoKit",
                "--out-dir",
            ])
            .arg(&built),
    );
    let plist = success(
        Command::new("plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(built.join("Artifacts/DemoKitFFI.xcframework/Info.plist")),
    );
    let plist = String::from_utf8(plist.stdout).unwrap();
    for slice in [
        "ios-arm64",
        "ios-arm64_x86_64-simulator",
        "macos-arm64_x86_64",
    ] {
        assert!(plist.contains(slice), "missing {slice}: {plist}");
    }
    let before = std::fs::read(built.join("Package.swift")).unwrap();
    let output = cli()
        .args([
            "package",
            "--manifest-path",
            "examples/demo/Cargo.toml",
            "--out-dir",
        ])
        .arg(&built)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(before, std::fs::read(built.join("Package.swift")).unwrap());

    let relocated = temp.path().join("demo/swift/DemoKit");
    std::fs::create_dir_all(relocated.parent().unwrap()).unwrap();
    std::fs::rename(&built, &relocated).unwrap();
    let consumer = temp.path().join("swift-consumer");
    std::fs::create_dir_all(consumer.join("Sources/Consumer")).unwrap();
    for file in ["Package.swift", "Sources/Consumer/main.swift"] {
        std::fs::copy(
            root().join("examples/swift-consumer").join(file),
            consumer.join(file),
        )
        .unwrap();
    }
    let output = success(
        Command::new("swift")
            .arg("run")
            .arg("--package-path")
            .arg(&consumer)
            .arg("Consumer"),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("PASS: Swift consumer called Rust"));
    let derived = temp.path().join("DerivedData");
    for destination in ["generic/platform=iOS Simulator", "generic/platform=iOS"] {
        success(
            Command::new("xcodebuild")
                .current_dir(&consumer)
                .args([
                    "-scheme",
                    "EqSwiftConsumer",
                    "-destination",
                    destination,
                    "-derivedDataPath",
                ])
                .arg(&derived)
                .args(["CODE_SIGNING_ALLOWED=NO", "build"]),
        );
    }
    if let Ok(device) = std::env::var("EQSWIFT_TEST_SIMULATOR") {
        let output = success(
            Command::new("xcrun")
                .args(["simctl", "spawn", &device])
                .arg(derived.join("Build/Products/Debug-iphonesimulator/Consumer")),
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("PASS: Swift consumer called Rust")
        );
    }
}
