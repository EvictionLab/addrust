# CLI Feature Gate

**Date:** 2026-05-07
**Author:** Sarah Johnson
**Scope:** addrust 0.1.2

---

## 1. Motivation

`addrust` ships as both a Rust library (consumed by the `duckdb-address-standardizer` extension) and a self-contained CLI binary with an interactive TUI configurator. Today, the CLI's three heavy dependencies — `clap`, `ratatui`, `crossterm` — are unconditional in `Cargo.toml`. Anyone depending on `addrust` as a library compiles all three even though they never reach the lib API surface.

The wrapper at `~/projects/duckdb-address-standardizer/addrust-ffi/` already passes `default-features = false` against `addrust`, anticipating this gate. This release delivers it: gate the CLI-only dependencies behind a `cli` feature, default-on, so the binary remains a `cargo install` away while library consumers compile a strictly smaller dependency tree.

## 2. Non-goals

- **Splitting `cli` into separate `cli` (clap) and `tui` (ratatui+crossterm) features.** The binary requires both; no current consumer needs an intermediate "CLI without TUI" build. Adding the split later is a manifest edit if a use case appears.
- **Gating `init` behind `cli`.** `init::generate_default_config` has no heavy deps and produces a starting TOML template that is useful to any library consumer. Coupling "I want a config template" to "I want clap+ratatui+crossterm" would be backwards.
- **Touching `duckdb_io` or its feature gate.** It's already correctly gated under the `duckdb` feature and that stays unchanged.
- **Bumping the wrapper.** The wrapper already passes `default-features = false`, so this release is invisible to it. No coordinated bump.

## 3. Design

### 3.1 Feature shape

A single feature `cli`, on by default:

```toml
[features]
default = ["duckdb", "cli"]
duckdb = ["dep:duckdb"]
cli = ["dep:clap", "dep:ratatui", "dep:crossterm"]
```

### 3.2 Dependency gating

The three CLI crates become optional:

```toml
clap = { version = "4", features = ["derive"], optional = true }
ratatui = { version = "0.29", optional = true }
crossterm = { version = "0.28", optional = true }
```

### 3.3 Library surface

`src/lib.rs` gates the TUI module:

```rust
#[cfg(feature = "cli")]
pub mod tui;
```

All other modules (`address`, `config`, `init`, `ops`, `pattern`, `pipeline`, `prepare`, `step`, `tables`) remain unconditionally public. Notably, `init` stays public so library consumers — including the wrapper — can offer "generate a default config" functionality without pulling in `clap`/`ratatui`/`crossterm`.

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

| Invocation                                                       | What you get                                  |
| ---------------------------------------------------------------- | --------------------------------------------- |
| `cargo build`                                                    | full lib + `addrust` bin + duckdb_io          |
| `cargo build --no-default-features`                              | bare lib (wrapper's exact build)              |
| `cargo build --no-default-features --features duckdb`            | lib + duckdb_io (no CLI deps)                 |
| `cargo build --no-default-features --features cli`               | lib + `addrust` bin (no duckdb_io)            |
| `cargo build --bin generate-suffixes --no-default-features`      | suffix generator only                         |

The wrapper's `addrust-ffi/Cargo.toml` passes `default-features = false` with no additional features enabled, so it consumes addrust as a bare Rust library — no `clap`, `ratatui`, `crossterm`, or `duckdb` crate. (The wrapper integrates DuckDB on the C++ side, not via `addrust::duckdb_io`.)

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

> Made `clap`, `ratatui`, and `crossterm` optional behind a new default-on `cli` feature. Library consumers can now opt out with `default-features = false` to skip compiling the CLI/TUI dependencies. The `duckdb-address-standardizer` wrapper already passes `default-features = false`, so this release is a no-op for that consumer; downstream library users compiling against `addrust` directly will see a smaller dependency tree when they disable defaults.
