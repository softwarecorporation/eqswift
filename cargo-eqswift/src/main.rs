use anyhow::{bail, Context, Result};
use cargo_metadata::{Metadata, Package, Target};
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;
use uniffi_bindgen::bindings::{GenerateOptions, TargetLanguage};
use uniffi_bindgen::{BindgenPaths, BindgenPathsLayer};

mod packaging;

/// Build Rust libraries, generate Swift bindings, and package Apple XCFrameworks.
#[derive(Parser)]
#[command(name = "cargo-eqswift", bin_name = "cargo", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Eqswift {
        #[command(flatten)]
        source: SourceArgs,
        #[command(subcommand)]
        cmd: EqswiftCmd,
    },
}

#[derive(Args, Default)]
struct SourceArgs {
    #[arg(long, global = true)]
    manifest_path: Option<PathBuf>,
    /// Select a workspace library package (required when selection is ambiguous).
    #[arg(short, long, global = true)]
    package: Option<String>,
    #[arg(long, global = true, value_delimiter = ',')]
    features: Vec<String>,
    #[arg(long, global = true)]
    no_default_features: bool,
    #[arg(long, global = true)]
    locked: bool,
    /// UniFFI configuration override; otherwise each crate's uniffi.toml is used.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum EqswiftCmd {
    /// Generate raw bindings from an already compiled library.
    Swift {
        #[arg(long)]
        release: bool,
        #[arg(long, default_value = "swift/Generated")]
        out_dir: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long = "static")]
        static_link: bool,
    },
    /// Build the library and generate raw bindings.
    Build {
        #[arg(long)]
        release: bool,
        #[arg(long, default_value = "swift/Generated")]
        out_dir: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long = "static")]
        static_link: bool,
        #[arg(last = true)]
        cargo_args: Vec<String>,
    },
    /// Build a self-contained Swift package containing a static Apple XCFramework.
    Package(packaging::PackageArgs),
}

fn main() -> Result<()> {
    let Commands::Eqswift { source, cmd } = Cli::parse().command;
    let library = Library::resolve(source)?;
    match cmd {
        EqswiftCmd::Swift {
            release,
            out_dir,
            target,
            static_link,
        } => library.generate_raw(release, &out_dir, target.as_deref(), static_link),
        EqswiftCmd::Build {
            release,
            out_dir,
            target,
            static_link,
            cargo_args,
        } => {
            library.build(release, target.as_deref(), &cargo_args)?;
            library.generate_raw(release, &out_dir, target.as_deref(), static_link)
        }
        EqswiftCmd::Package(args) => packaging::package(&library, args),
    }
}

struct Library {
    args: SourceArgs,
    metadata: Metadata,
    package: Package,
    target: Target,
}

impl Library {
    fn resolve(args: SourceArgs) -> Result<Self> {
        let mut command = cargo_metadata::MetadataCommand::new();
        if let Some(path) = &args.manifest_path {
            command.manifest_path(path);
        }
        if !args.features.is_empty() {
            command.features(cargo_metadata::CargoOpt::SomeFeatures(
                args.features.clone(),
            ));
        }
        if args.no_default_features {
            command.features(cargo_metadata::CargoOpt::NoDefaultFeatures);
        }
        if args.locked {
            command.other_options(vec!["--locked".into()]);
        }
        let metadata = command.exec().context("failed to run cargo metadata")?;
        let is_library = |p: &&Package| {
            p.targets.iter().any(|t| {
                t.crate_types
                    .iter()
                    .any(|k| k == "cdylib" || k == "staticlib")
            })
        };
        let members: Vec<_> = metadata
            .packages
            .iter()
            .filter(|p| metadata.workspace_members.contains(&p.id))
            .collect();
        let package = if let Some(name) = &args.package {
            members
                .iter()
                .copied()
                .find(|p| p.name == *name)
                .with_context(|| format!("workspace package '{name}' not found"))?
        } else if let Some(root) = metadata.root_package() {
            root
        } else {
            let candidates: Vec<_> = members.into_iter().filter(is_library).collect();
            if candidates.len() != 1 {
                bail!(
                    "select a library with --package; workspace candidates: {}",
                    candidates
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            candidates[0]
        }
        .clone();
        let target = package
            .targets
            .iter()
            .find(|t| {
                t.crate_types
                    .iter()
                    .any(|k| k == "cdylib" || k == "staticlib")
            })
            .cloned()
            .context(
                "selected package needs [lib] crate-type = [\"staticlib\", \"cdylib\", \"rlib\"]",
            )?;
        Ok(Self {
            args,
            metadata,
            package,
            target,
        })
    }

    fn build(&self, release: bool, target: Option<&str>, extra: &[String]) -> Result<()> {
        let mut command = Command::new("cargo");
        command
            .arg("build")
            .arg("--lib")
            .arg("--manifest-path")
            .arg(&self.package.manifest_path)
            .args(["-p", &self.package.name]);
        if release {
            command.arg("--release");
        }
        if let Some(target) = target {
            command.args(["--target", target]);
        }
        if !self.args.features.is_empty() {
            command.arg("--features").arg(self.args.features.join(","));
        }
        if self.args.no_default_features {
            command.arg("--no-default-features");
        }
        if self.args.locked {
            command.arg("--locked");
        }
        command.args(extra);
        run(&mut command).context("Rust library build failed; inspect Cargo's diagnostic above")
    }

    fn artifact(&self, release: bool, target: Option<&str>, suffix: &str) -> PathBuf {
        let mut path = self.metadata.target_directory.as_std_path().to_path_buf();
        if let Some(target) = target {
            path.push(target);
        }
        let prefix = if suffix == ".dll" || suffix == ".lib" {
            ""
        } else {
            "lib"
        };
        path.join(if release { "release" } else { "debug" })
            .join(format!(
                "{prefix}{}{}",
                self.target.name.replace('-', "_"),
                suffix
            ))
    }

    fn generate(&self, artifact: &Path, out: &Path) -> Result<()> {
        let mut paths = BindgenPaths::default();
        paths.add_layer(CratePaths(self.metadata.clone()));
        uniffi_bindgen::bindings::generate_with_bindgen_paths(GenerateOptions {
            languages: vec![TargetLanguage::Swift],
            source: camino::Utf8PathBuf::from_path_buf(artifact.to_path_buf())
                .map_err(|p| anyhow::anyhow!("non-UTF-8 library path: {}", p.display()))?,
            out_dir: camino::Utf8PathBuf::from_path_buf(out.to_path_buf())
                .map_err(|p| anyhow::anyhow!("non-UTF-8 output path: {}", p.display()))?,
            config_override: self.args.config.as_ref().map(|p| camino::Utf8PathBuf::from_path_buf(p.clone())
                .map_err(|_| anyhow::anyhow!("configuration path must be UTF-8"))).transpose()?,
            ..Default::default()
        }, paths).context("Swift binding generation failed (cargo-eqswift uses UniFFI 0.32.1; use compatible UniFFI in the library)")
    }

    fn generate_raw(
        &self,
        release: bool,
        out: &Path,
        target: Option<&str>,
        static_link: bool,
    ) -> Result<()> {
        let suffixes = if static_link {
            vec![".a", ".lib", ".dylib", ".so", ".dll"]
        } else {
            vec![".dylib", ".so", ".dll", ".a", ".lib"]
        };
        let artifact = suffixes.into_iter().map(|s| self.artifact(release, target, s))
            .find(|p| p.exists()).context("compiled library not found; run `cargo eqswift build` with the same target/profile first")?;
        self.generate(&artifact, out)?;
        fs_err::write(out.join("module.modulemap"), modulemaps(out)?)?;
        eprintln!("✓ Generated Swift bindings in {}\n  Library: {}\n  For a distributable Swift package use `cargo eqswift package` on macOS.", out.display(), artifact.display());
        Ok(())
    }
}

struct CratePaths(Metadata);
impl BindgenPathsLayer for CratePaths {
    fn get_crate_root(&self, name: &str) -> Option<camino::Utf8PathBuf> {
        self.0
            .packages
            .iter()
            .find(|p| p.targets.iter().any(|t| t.name.replace('-', "_") == name))
            .and_then(|p| p.manifest_path.parent().map(ToOwned::to_owned))
    }
}

fn modulemaps(out: &Path) -> Result<String> {
    let mut maps: Vec<_> = fs_err::read_dir(out)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|s| s == "modulemap")
                && p.file_name().is_some_and(|n| n != "module.modulemap")
        })
        .collect();
    maps.sort();
    if maps.is_empty() {
        bail!("UniFFI generated no module maps; enable generate_module_map in uniffi.toml");
    }
    maps.into_iter()
        .map(fs_err::read_to_string)
        .collect::<std::io::Result<Vec<_>>>()
        .map(|maps| maps.join("\n"))
        .map_err(Into::into)
}

fn run(command: &mut Command) -> Result<()> {
    eprintln!("  Running: {command:?}");
    let status = command
        .status()
        .with_context(|| format!("could not execute {command:?}"))?;
    if !status.success() {
        bail!("command failed ({status}): {command:?}");
    }
    Ok(())
}
