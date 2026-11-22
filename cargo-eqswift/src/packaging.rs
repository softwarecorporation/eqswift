use super::{modulemaps, run, Library};
use anyhow::{bail, Context, Result};
use clap::Args;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

#[derive(Args)]
pub struct PackageArgs {
    /// New output directory; existing paths are never overwritten.
    #[arg(long, default_value = "swift/Package")]
    out_dir: PathBuf,
    /// Swift product/import name (defaults to the Rust library target name).
    #[arg(long)]
    name: Option<String>,
    /// Apple Rust targets. Architectures of each platform are merged with lipo.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "aarch64-apple-darwin,x86_64-apple-darwin,aarch64-apple-ios,aarch64-apple-ios-sim,x86_64-apple-ios"
    )]
    targets: Vec<String>,
    /// Use debug Rust artifacts rather than release artifacts.
    #[arg(long)]
    debug: bool,
}

fn platform(target: &str) -> Result<&'static str> {
    match target {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => Ok("macos"),
        "aarch64-apple-ios" => Ok("ios"),
        "aarch64-apple-ios-sim" | "x86_64-apple-ios" => Ok("ios-simulator"),
        _ => bail!("unsupported Apple target '{target}'; supported: aarch64-apple-darwin, x86_64-apple-darwin, aarch64-apple-ios, aarch64-apple-ios-sim, x86_64-apple-ios"),
    }
}

pub fn package(library: &Library, args: PackageArgs) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("XCFramework packaging requires macOS with full Xcode selected (`sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`); raw bindings can be generated here with `cargo eqswift build`");
    }
    let name = args
        .name
        .unwrap_or_else(|| library.target.name.replace('-', "_"));
    if name.is_empty()
        || !name
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
    {
        bail!("Swift package name must be an ASCII identifier; pass --name MyLibrary");
    }
    if !library.target.crate_types.iter().any(|t| t == "staticlib") {
        bail!("XCFramework packaging requires [lib] crate-type to include \"staticlib\"");
    }
    match fs_err::symlink_metadata(&args.out_dir) {
        Ok(_) => bail!("output {} already exists; choose a new --out-dir (existing packages are never overwritten)", args.out_dir.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(error) => return Err(error.into()),
    }
    let mut groups = BTreeMap::<&str, Vec<&str>>::new();
    for target in &args.targets {
        let targets = groups.entry(platform(target)?).or_default();
        if targets.contains(&target.as_str()) {
            bail!("duplicate target '{target}'");
        }
        targets.push(target);
    }
    run(Command::new("xcrun").args(["--find", "xcodebuild"]))
        .context("full Xcode is required; select it with xcode-select, not Command Line Tools")?;
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .context("rustup is required; install Rust using rustup.rs")?;
    if !installed.status.success() {
        bail!("rustup target list failed; check the active Rust toolchain");
    }
    let installed = String::from_utf8(installed.stdout)?;
    let missing: Vec<_> = args
        .targets
        .iter()
        .filter(|t| !installed.lines().any(|i| i == t.as_str()))
        .collect();
    if !missing.is_empty() {
        bail!(
            "missing Rust targets; run: rustup target add {}",
            missing
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    let out = std::env::current_dir()?.join(&args.out_dir);
    let parent = out.parent().context("output directory needs a parent")?;
    fs_err::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".eqswift-package-")
        .tempdir_in(parent)?;
    let work = staging.path().join("work");
    let generated = work.join("generated");
    fs_err::create_dir_all(&generated)?;
    let release = !args.debug;
    for target in &args.targets {
        library.build(release, Some(target), &[]).with_context(|| format!("building slice {target}; ensure its Xcode SDK and native dependency cross-compilers are installed"))?;
    }
    // Read metadata from the exact archive being shipped, not an unrelated host build.
    library.generate(
        &library.artifact(release, Some(&args.targets[0]), ".a"),
        &generated,
    )?;
    for target in args.targets.iter().skip(1) {
        let other = work.join(target);
        library.generate(&library.artifact(release, Some(target), ".a"), &other)?;
        let read_files = |dir: &std::path::Path| -> Result<BTreeMap<std::ffi::OsString, Vec<u8>>> {
            fs_err::read_dir(dir)?
                .map(|entry| {
                    let entry = entry?;
                    Ok((entry.file_name(), fs_err::read(entry.path())?))
                })
                .collect()
        };
        if read_files(&generated)? != read_files(&other)? {
            bail!("exported Swift interface differs between {} and {target}; remove target-specific exports or package those targets separately", args.targets[0]);
        }
    }
    let headers = work.join("headers");
    let sources = staging.path().join("Sources").join(&name);
    fs_err::create_dir_all(&headers)?;
    fs_err::create_dir_all(&sources)?;
    for entry in fs_err::read_dir(&generated)? {
        let path = entry?.path();
        let destination = match path.extension().and_then(|s| s.to_str()) {
            Some("h") => &headers,
            Some("swift") => &sources,
            _ => continue,
        };
        fs_err::copy(&path, destination.join(path.file_name().unwrap()))?;
    }
    if fs_err::read_dir(&sources)?.next().is_none() {
        bail!("UniFFI generated no Swift sources");
    }
    fs_err::write(headers.join("module.modulemap"), modulemaps(&generated)?)?;
    let binary = format!("{name}FFI");
    let artifacts = staging.path().join("Artifacts");
    fs_err::create_dir_all(&artifacts)?;
    let mut create = Command::new("xcodebuild");
    create.arg("-create-xcframework");
    for (platform, targets) in groups {
        let slice = work.join(platform);
        fs_err::create_dir_all(&slice)?;
        let archive = slice.join(format!("lib{binary}.a"));
        let inputs: Vec<_> = targets
            .iter()
            .map(|t| library.artifact(release, Some(t), ".a"))
            .collect();
        if inputs.len() == 1 {
            fs_err::copy(&inputs[0], &archive)?;
        } else {
            run(Command::new("xcrun")
                .arg("lipo")
                .arg("-create")
                .args(&inputs)
                .arg("-output")
                .arg(&archive))?;
        }
        create
            .arg("-library")
            .arg(archive)
            .arg("-headers")
            .arg(&headers);
    }
    create
        .arg("-output")
        .arg(artifacts.join(format!("{binary}.xcframework")));
    run(&mut create)
        .context("XCFramework assembly failed; inspect xcodebuild's diagnostic above")?;
    fs_err::write(
        staging.path().join("Package.swift"),
        format!(
            r#"// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "{name}",
    platforms: [.macOS(.v13), .iOS(.v13)],
    products: [.library(name: "{name}", targets: ["{name}"])],
    targets: [
        .binaryTarget(name: "{binary}", path: "Artifacts/{binary}.xcframework"),
        .target(name: "{name}", dependencies: ["{binary}"])
    ]
)
"#
        ),
    )?;
    fs_err::remove_dir_all(&work)?;
    fs_err::rename(staging.path(), &out)?;
    eprintln!("✓ Swift package: {}\n  Add this directory as a local Swift package dependency; import {name}.\n  Built targets: {}", out.display(), args.targets.join(", "));
    Ok(())
}
