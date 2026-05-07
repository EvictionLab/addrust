# Library Feature Gates (0.1.2)

**Date:** 2026-05-07
**Author:** Sarah Johnson
**Scope:** addrust 0.1.2

---

## 1. Motivation

`addrust` ships as both a Rust library (consumed by the `duckdb-address-standardizer` extension) and a self-contained CLI binary with an interactive TUI configurator. Several heavy dependencies are unconditional in `Cargo.toml` today even though library consumers never reach the code paths that use them:

- `clap` — only used in `src/main.rs` for CLI argument parsing.
- `ratatui` and `crossterm` — only used inside `src/tui/` for the interactive configurator.
- `rayon` — only used in `Pipeline::parse_batch` for parallel batch parsing. The `duckdb-address-standardizer` wrapper calls `Pipeline::parse` per row; DuckDB itself runs a separate addrust call per thread, so rayon's work-stealing inside a single batch is dead code on the wrapper path.

The wrapper already passes `default-features = false` against `addrust`, anticipating this work. This release delivers two default-on feature gates so the binary remains a `cargo install` away while library consumers compile a strictly smaller dependency tree:

- `cli` gates `clap`, `ratatui`, and `crossterm` (and the `tui` module).
- `parallel` gates `rayon`. `Pipeline::parse_batch` keeps the same signature; without the feature it falls back to a serial iteration, so the API stays uniform.

## 2. Non-goals

- **Splitting `cli` into separate `cli` (clap) and `tui` (ratatui+crossterm) features.** The binary requires both; no current consumer needs an intermediate "CLI without TUI" build. Adding the split later is a manifest edit if a use case appears.
- **Gating `init` behind `cli`.** `init::generate_default_config` has no heavy deps and produces a starting TOML template that is useful to any library consumer. Coupling "I want a config template" to "I want clap+ratatui+crossterm" would be backwards.
- **Touching `duckdb_io` or its feature gate.** It's already correctly gated under the `duckdb` feature and that stays unchanged.
- **Removing `Pipeline::parse_batch` when `parallel` is off.** The serial fallback is small and keeps the API uniform across builds. Consumers that need to detect "this build has no parallelism" can check `cfg!(feature = "parallel")` themselves.
- **Bumping the wrapper.** The wrapper already passes `default-features = false`, so this release is invisible to it. No coordinated bump.

## 3. Design

### 3.1 Feature shape

Two default-on features, plus the existing `duckdb` gate:

```toml
[features]
default = ["duckdb", "cli", "parallel"]
duckdb = ["dep:duckdb"]
cli = ["dep:clap", "dep:ratatui", "dep:crossterm"]
parallel = ["dep:rayon"]
```

### 3.2 Dependency gating

The four optional crates:

```toml
clap = { version = "4", features = ["derive"], optional = true }
ratatui = { version = "0.29", optional = true }
crossterm = { version = "0.28", optional = true }
rayon = { version = "1", optional = true }
```

### 3.3 Library surface

`src/lib.rs` gates the TUI module:

```rust
#[cfg(feature = "cli")]
pub mod tui;
```

All other modules (`address`, `config`, `init`, `ops`, `pattern`, `pipeline`, `prepare`, `step`, `tables`) remain unconditionally public. Notably, `init` stays public so library consumers — including the wrapper — can offer "generate a default config" functionality without pulling in `clap`/`ratatui`/`crossterm`.

`Pipeline::parse_batch` keeps its signature in both builds; the `parallel` feature only switches the body:

```rust
pub fn parse_batch(&self, inputs: &[&str]) -> Vec<Address> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        inputs.par_iter().map(|input| self.parse(input)).collect()
    }
    #[cfg(not(feature = "parallel"))]
    inputs.iter().map(|input| self.parse(input)).collect()
}
```

The top-level `addrust::parse_batch` convenience wrapper and `addrust::duckdb_io::run_duckdb` (which calls `parse_batch` for the addrust CLI's `--duckdb` flag) work uniformly against both bodies.

### 3.4 Binary surface

`src/main.rs` becomes feature-gated via `required-features`:

```toml
[[bin]]
name = "addrust"
path = "src/main.rs"
required-features = ["cli"]
```

(The binary section is implicit today via Cargo's default `src/main.rs` discovery; this redesign makes it explicit.)

`src/main.rs` itself is unchanged in shape — it imports `clap` and `addrust::tui::run`, both of which are guaranteed present whenever the binary builds. No internal `#[cfg(feature = "cli")]` annotations are needed inside `main.rs`.

The `generate-suffixes` binary at `src/bin/generate_suffixes.rs` uses only `std` and stays unconditional — it's a build-time data generator, not a CLI tool.

### 3.5 Build matrices

After the change, these all build cleanly:

| Invocation                                                       | What you get                                              |
| ---------------------------------------------------------------- | --------------------------------------------------------- |
| `cargo build`                                                    | full lib + `addrust` bin + duckdb_io + parallel batch     |
| `cargo build --no-default-features`                              | bare lib, serial batch (wrapper's exact build)            |
| `cargo build --no-default-features --features duckdb`            | lib + duckdb_io, serial batch                             |
| `cargo build --no-default-features --features cli`               | lib + `addrust` bin, serial batch                         |
| `cargo build --no-default-features --features parallel`          | bare lib + parallel batch (no CLI, no duckdb)             |
| `cargo build --bin generate-suffixes --no-default-features`      | suffix generator only                                     |

The wrapper's `addrust-ffi/Cargo.toml` passes `default-features = false` with no additional features enabled, so it consumes addrust as a bare Rust library — no `clap`, `ratatui`, `crossterm`, `rayon`, or `duckdb` crate. (The wrapper integrates DuckDB on the C++ side, not via `addrust::duckdb_io`, and DuckDB itself spawns a separate addrust call per thread — rayon's intra-batch work-stealing isn't useful on that path.)

## 4. Verification

CI and local checks must include the no-default-features build to catch accidental unconditional uses of gated crates:

```sh
cargo build --no-default-features
cargo build --no-default-features --features duckdb
cargo test --no-default-features --features duckdb
cargo build  # default features
cargo test   # default features
```

The wrapper's existing `addrust-ffi` build is the integration check. Running `cargo build` in `~/projects/duckdb-address-standardizer/addrust-ffi/` after the change should succeed without modification.

## 5. Release notes

CHANGELOG entry under 0.1.2:

> Made `clap`, `ratatui`, `crossterm`, and `rayon` optional behind two new default-on features (`cli` and `parallel`). Library consumers can now opt out with `default-features = false` to skip compiling the CLI/TUI dependencies and the rayon thread pool. `Pipeline::parse_batch` keeps the same signature in both builds; without `parallel` it falls back to a serial iteration. The `duckdb-address-standardizer` wrapper already passes `default-features = false`, so this release is a no-op for that consumer at runtime — it just produces a smaller compile.
