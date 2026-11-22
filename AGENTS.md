# Agent Notes — eqswift

This file contains build steps, conventions, and context for AI agents working on this project.

## Project Structure

This is a Cargo workspace with three crates:

- **`eqswift-macros/`** — proc-macro crate (`proc-macro = true`)
  - `#[eqswift::export]` — wraps `#[uniffi::export]`
  - `eqswift::setup!()` — wraps `uniffi::setup_scaffolding!()`
- **`eq-swift/`** — the main library crate (library name = `eqswift`)
  - `src/lib.rs` — user-facing Rust code + re-exports
  - `src/bin/uniffi-bindgen.rs` — CLI binary for generating bindings
  - `tests/integration.rs` — smoke tests (library compiles, bindings generate correctly)
- **`cargo-eqswift/`** — cargo subcommand for better DX
  - `cargo eqswift swift` — generate Swift bindings
  - `cargo eqswift build` — build + generate Swift
  - `cargo eqswift swift --static` — generate raw bindings from a static archive
  - `cargo eqswift package` — build Apple static slices, an XCFramework, and a Swift package (macOS/full Xcode required)

## Build Steps

1. **Build the Rust library:**
   ```bash
   cargo build
   # or for release:
   cargo build --release
   ```

2. **Generate Swift bindings (via cargo-eqswift):**
   ```bash
   cargo eqswift swift
   # or with explicit output directory:
   cargo eqswift swift --out-dir swift/Generated
   ```

3. **Or generate via uniffi-bindgen directly:**
   ```bash
   # macOS
   cargo run --features cli --bin uniffi-bindgen generate \
     --library target/debug/libeqswift.dylib \
     --language swift --out-dir eq-swift/swift/Generated

   # Linux
   cargo run --features cli --bin uniffi-bindgen generate \
     --library target/debug/libeqswift.so \
     --language swift --out-dir eq-swift/swift/Generated
   ```

4. **Verify bindings were created:**
   ```bash
   ls swift/Generated/
   # should show: eqswift.swift, eqswiftFFI.h, eqswiftFFI.modulemap
   ```

5. **Run integration tests:**
   ```bash
   cargo test -p eqswift --test integration
   ```

## Key Conventions

- **No UDL files.** The project uses UniFFI's proc-macro mode exclusively.
- **No build.rs.** Everything is driven by proc macros at compile time.
- **Library crate-type:** `cdylib`, `staticlib`, `rlib` (required for FFI).
- **Proc-macro re-exports:** `eqswift::Record`, `eqswift::Object`, `eqswift::Enum`, `eqswift::Error` are just re-exports of `uniffi::Record`, `uniffi::Object`, etc. This lets users write `#[derive(eqswift::Record)]` instead of `#[derive(uniffi::Record)]`.
- **Standalone binary:** `uniffi-bindgen` requires the eqswift `cli` feature. `cargo-eqswift` embeds its own generator; downstream crates need no bindgen binary.
- **cargo subcommand:** `cargo-eqswift`. Install with `cargo install --path cargo-eqswift`.

## Important: UniFFI must be a direct dependency

Downstream crates using `eqswift` must also add `uniffi` as a direct dependency:

```toml
[dependencies]
eqswift = "0.1"
uniffi = "0.32"
```

This is because UniFFI's proc-macros (`#[uniffi::export]`, `#[uniffi::constructor]`, `uniffi::setup_scaffolding!()`) emit code that references `::uniffi::...` paths, which can only be resolved when `uniffi` is a direct dependency of the consuming crate.

## Adding New Macros

If you need to add a new derive or attribute macro to `eqswift-macros`:

1. Add the proc-macro definition in `eqswift-macros/src/lib.rs`.
2. Re-export it in `eq-swift/src/lib.rs` via `pub use eqswift_macros::your_macro;`.
3. Update the rustdoc example in `eq-swift/src/lib.rs` and `README.md`.

## Modifying the Rust API

When editing `eq-swift/src/lib.rs`:

- Keep the example structs/functions (`Person`, `Greeter`, `add`, `version`) — they serve as a smoke test.
- Remember that only `pub` items in `#[eqswift::export]` or with `#[derive(...)]` are visible to Swift.
- If you add a new exported function, rebuild and regenerate bindings to verify.

## Swift Side

The `swift/` directory is a sample consumer of the generated binary package. This repo is an FFI framework only — no app transports or product logic in the `eqswift` crate.

- Generate with `cargo eqswift package --package eqswift --name EqSwift --out-dir swift/Package`, then run `swift run --package-path swift EqSwiftSmoke`.
- `examples/swift-consumer` consumes the demo's generated `DemoKit` package. See README for the command.
- Generated package directories must not exist before packaging. The CLI never overwrites an existing package.

## Testing

Integration tests live in `eq-swift/tests/integration.rs` and verify:

1. Library compiles and `.dylib`/`.so` exists
2. Swift bindings are generated with correct files
3. Exported items (`Person`, `Greeter`, `add`, `greet`) appear in generated Swift

`cargo-eqswift/tests/workflow.rs` tests downstream generation without a bindgen binary. On macOS it also builds all five Apple targets, relocates the generated package, runs a SwiftPM consumer, and links iOS device/simulator builds. Install the five Rust Apple targets listed in README before running it. `EQSWIFT_TEST_SIMULATOR` optionally selects an already booted simulator UUID for execution.

A quick smoke test is:

```bash
cargo build
cargo eqswift swift --out-dir /tmp/eqswift-test
head -n 20 /tmp/eqswift-test/eqswift.swift
```

## Common Issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no bin target named uniffi-bindgen` | Direct invocation of a binary the downstream crate does not own | Use `cargo eqswift build` or `cargo eqswift package` |
| `setup_scaffolding!` not found | Wrong UniFFI version or missing `setup!()` | Ensure `eqswift::setup!()` is called |
| Empty generated Swift | Bindings generated before library built | Build library first, then run bindgen |
| Type not in Swift | Not public / not in exported signature | Make type `pub` and use it in an exported function |
| `cargo eqswift` not found | Not installed | Run `cargo install --path cargo-eqswift` |
| Old library artifact picked up | `libeq_swift.dylib` vs `libeqswift.dylib` | Delete old artifacts from `target/debug/` |

## Dependencies

- `uniffi = "0.32"` — core FFI framework
- `uniffi_bindgen = "=0.32.1"` — CLI-owned Swift generator
- `eqswift-macros` — our thin proc-macro wrappers
- `cargo-eqswift` — cargo subcommand for binding generation

Do not bump the UniFFI minor version without checking the [upgrade guide](https://mozilla.github.io/uniffi-rs/latest/Upgrading.html).
