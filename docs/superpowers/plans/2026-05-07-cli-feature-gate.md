# CLI Feature Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate `clap`, `ratatui`, and `crossterm` behind a default-on `cli` feature so library consumers compile a smaller dependency tree.

**Architecture:** Mark the three CLI crates as optional dependencies in `Cargo.toml`. Introduce a `cli` feature that activates them, add it to `default`, and guard `pub mod tui;` in `src/lib.rs` with `#[cfg(feature = "cli")]`. Convert the implicit `addrust` binary to an explicit `[[bin]]` declaration with `required-features = ["cli"]` so non-CLI builds skip it. `init` stays unconditionally public — it has no heavy deps and library consumers may want to generate default configs without pulling in CLI crates.

**Tech Stack:** Cargo manifest, Rust 2024 edition, conditional compilation via `#[cfg(feature = ...)]`.

**Spec:** `docs/superpowers/specs/2026-05-07-cli-feature-gate.md`

---

## File Map

- **Modify:** `Cargo.toml` — mark `clap`/`ratatui`/`crossterm` optional, add `cli` feature, add `[[bin]]` block for the `addrust` binary
- **Modify:** `src/lib.rs:10` — gate `pub mod tui;` behind `#[cfg(feature = "cli")]`
- **Modify:** `CHANGELOG.md` — Unreleased entry under the `### Added` heading

No source-file `#[cfg]` annotations are needed inside `src/main.rs`: the binary itself requires the `cli` feature, so its imports of `clap` and `addrust::tui::run` are guaranteed to resolve whenever the binary builds.

No tests need modification: nothing in `tests/` imports `clap`, `ratatui`, or `crossterm`. The single test inside `src/tui/mod.rs:631` (`use crossterm::event::KeyCode;`) lives inside the gated module and is excluded with it.

No wrapper changes: `~/projects/duckdb-address-standardizer/addrust-ffi/Cargo.toml` already passes `default-features = false` with no other features, so it's already on the bare-lib build target.

---

## Task 1: Establish current baseline

**Files:** none modified.

The point of this task is to record the *pre-change* behavior so each later step has a concrete delta to verify against.

- [ ] **Step 1: Confirm working directory and clean tree**

```sh
pwd
git status
```

Expected: `pwd` prints `/Users/sj2690/projects/addrust`. `git status` shows `nothing to commit, working tree clean` and the branch is `main`.

- [ ] **Step 2: Confirm current default build succeeds**

Run:

```sh
cargo build
```

Expected: succeeds. The `addrust` binary and `generate-suffixes` binary both compile.

- [ ] **Step 3: Confirm current `cargo tree` includes all three CLI crates regardless of features**

Run:

```sh
cargo tree --no-default-features --prefix none -e normal | grep -E '^(clap|ratatui|crossterm) ' | sort -u
```

Expected: three lines printed (`clap v4...`, `ratatui v0.29...`, `crossterm v0.28...`). This proves the deps are unconditional today — turning off defaults does NOT exclude them. After the change, this exact command should produce zero matching lines.

- [ ] **Step 4: Confirm tests pass with default features**

Run:

```sh
cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Record the baseline in a scratch note (no commit)**

No file write. Just internalize: default build works, tests pass, all three CLI crates currently appear in `cargo tree --no-default-features`. Task 2 will flip those expectations.

---

## Task 2: Make CLI deps optional and introduce the `cli` feature

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs:10`

This is the core change. Done as one logical commit because the manifest edits and the `lib.rs` gate must land together — partial changes leave the tree in a non-building state.

- [ ] **Step 1: Update `Cargo.toml` dependencies and features**

Open `Cargo.toml`. Replace the `[dependencies]` and `[features]` blocks with the following exact content. The only changes are: `optional = true` added to `clap`, `ratatui`, `crossterm`; the `[features]` block grows a `cli` entry and includes it in `default`.

```toml
[dependencies]
regex = "1"
fancy-regex = "0.14"
rayon = "1"
clap = { version = "4", features = ["derive"], optional = true }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
ratatui = { version = "0.29", optional = true }
crossterm = { version = "0.28", optional = true }
duckdb = { version = "1.4", optional = true, features = ["bundled"] }

[features]
default = ["duckdb", "cli"]
duckdb = ["dep:duckdb"]
cli = ["dep:clap", "dep:ratatui", "dep:crossterm"]
```

- [ ] **Step 2: Add an explicit `[[bin]]` block for the `addrust` binary**

In `Cargo.toml`, immediately above the existing `[[bin]] name = "generate-suffixes"` block, insert:

```toml
[[bin]]
name = "addrust"
path = "src/main.rs"
required-features = ["cli"]

```

(Note the blank line after — it separates this block from the `generate-suffixes` block.)

The final `[[bin]]` section should read:

```toml
[[bin]]
name = "addrust"
path = "src/main.rs"
required-features = ["cli"]

[[bin]]
name = "generate-suffixes"
path = "src/bin/generate_suffixes.rs"
```

The explicit declaration is required because Cargo's auto-discovery of `src/main.rs` doesn't expose a way to attach `required-features`. Defining the bin explicitly overrides auto-discovery for that one file; `generate-suffixes` is unaffected.

- [ ] **Step 3: Gate `pub mod tui;` in `src/lib.rs`**

In `src/lib.rs`, find this line (currently line 10):

```rust
pub mod tui;
```

Replace it with:

```rust
#[cfg(feature = "cli")]
pub mod tui;
```

Leave the existing `#[cfg(feature = "duckdb")] pub mod duckdb_io;` (currently lines 12–13) unchanged.

- [ ] **Step 4: Verify default build still succeeds**

Run:

```sh
cargo build
```

Expected: succeeds. Both binaries compile because `default = ["duckdb", "cli"]` activates everything.

- [ ] **Step 5: Verify default tests still pass**

Run:

```sh
cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Verify the bare-lib build (wrapper's exact build target) succeeds**

Run:

```sh
cargo build --no-default-features
```

Expected: succeeds. Library compiles with neither `duckdb_io` nor `tui` modules. The `addrust` binary is skipped because its `required-features = ["cli"]` is not satisfied. The `generate-suffixes` binary still compiles (it has no feature requirements and uses only stdlib).

- [ ] **Step 7: Verify the dependency tree no longer contains the CLI crates without the feature**

Run:

```sh
cargo tree --no-default-features --prefix none -e normal | grep -E '^(clap|ratatui|crossterm) ' | sort -u
```

Expected: no output. (Compare to Task 1 / Step 3, which printed three lines.) This is the headline assertion of this release.

- [ ] **Step 8: Verify the wrapper-build-but-with-duckdb path succeeds**

Run:

```sh
cargo build --no-default-features --features duckdb
```

Expected: succeeds. Library + `duckdb_io` module compile, no CLI crates pulled.

- [ ] **Step 9: Verify the binary-only-no-duckdb path succeeds**

Run:

```sh
cargo build --no-default-features --features cli
```

Expected: succeeds. Library + `addrust` binary compile, no `duckdb_io` module, no `duckdb` crate.

- [ ] **Step 10: Verify `cargo test --no-default-features --features duckdb` passes**

Run:

```sh
cargo test --no-default-features --features duckdb
```

Expected: tests pass. `tests/duckdb_integration.rs` runs against the duckdb feature, `tests/config.rs` and `tests/golden.rs` are independent of CLI deps.

- [ ] **Step 11: Verify `cargo test --no-default-features` passes**

Run:

```sh
cargo test --no-default-features
```

Expected: tests pass. `tests/duckdb_integration.rs` is skipped automatically — it begins with `#![cfg(feature = "duckdb")]` and compiles to an empty crate when the feature is absent. `tests/config.rs` and `tests/golden.rs` test the core library and run normally.

- [ ] **Step 12: Verify the wrapper still builds against this addrust**

Run:

```sh
cargo build --manifest-path ~/projects/duckdb-address-standardizer/addrust-ffi/Cargo.toml
```

Expected: succeeds. The wrapper's `default-features = false` consumption of addrust now produces a strictly smaller compile (no clap, no ratatui, no crossterm).

- [ ] **Step 13: Commit**

```sh
git add Cargo.toml Cargo.lock src/lib.rs
git commit -m "$(cat <<'EOF'
feat: gate clap/ratatui/crossterm behind default-on cli feature

Library consumers (notably the duckdb-address-standardizer wrapper) now
compile a strictly smaller dependency tree when they opt out with
default-features = false.

Spec: docs/superpowers/specs/2026-05-07-cli-feature-gate.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Verify:

```sh
git log -1 --stat
git status
```

Expected: commit lands with `Cargo.toml`, `Cargo.lock`, and `src/lib.rs` modified. `git status` shows clean tree.

---

## Task 3: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

The repo follows Keep a Changelog. The `## [Unreleased]` section is currently empty.

- [ ] **Step 1: Read the current CHANGELOG header**

Read `CHANGELOG.md` lines 1–10 to confirm the `## [Unreleased]` section exists and is empty.

Expected: line 6 reads `## [Unreleased]` and the next non-blank section is `## [0.1.1] - 2026-05-05`.

- [ ] **Step 2: Add the Unreleased entry**

In `CHANGELOG.md`, replace the empty `## [Unreleased]` section (line 6 plus its blank line) with:

```markdown
## [Unreleased]

### Added

- New `cli` feature (default-on) that gates `clap`, `ratatui`, and `crossterm`. Library consumers depending on `addrust` with `default-features = false` no longer compile these crates. The `addrust` CLI binary requires the `cli` feature; `cargo install addrust` and `cargo build` continue to produce it because the feature is on by default.
- The `addrust` binary is now declared explicitly in `Cargo.toml` with `required-features = ["cli"]`. Behavior is unchanged for the default `cargo build` path.

### Changed

- `addrust::tui` module is now gated behind the `cli` feature. Library consumers who do not enable `cli` will not see this module exposed. The `init`, `address`, `config`, `pipeline`, and other core modules remain unconditionally public so library consumers can generate default configs and run the parser without enabling CLI deps.
```

- [ ] **Step 3: Verify the file parses as Markdown and the structure is intact**

Read `CHANGELOG.md` lines 1–25 to confirm the new section sits cleanly above the `## [0.1.1]` header.

- [ ] **Step 4: Commit**

```sh
git add CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs: changelog for cli feature gate

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

Verify:

```sh
git log -2 --oneline
git status
```

Expected: two new commits on `main` (this one and the Task 2 commit). Tree is clean.

---

## Task 4: Final verification pass

**Files:** none modified.

Replays the Task 2 verifications in sequence as a single sanity check, and also runs the wrapper integration build to confirm nothing has drifted between commits.

- [ ] **Step 1: Run the full feature matrix**

```sh
cargo build && \
cargo build --no-default-features && \
cargo build --no-default-features --features duckdb && \
cargo build --no-default-features --features cli && \
cargo test && \
cargo test --no-default-features --features duckdb
```

Expected: all six commands succeed.

- [ ] **Step 2: Confirm the dep-tree assertion holds**

```sh
cargo tree --no-default-features --prefix none -e normal | grep -E '^(clap|ratatui|crossterm) ' | sort -u
```

Expected: no output.

- [ ] **Step 3: Confirm wrapper still builds**

```sh
cargo build --manifest-path ~/projects/duckdb-address-standardizer/addrust-ffi/Cargo.toml
```

Expected: succeeds.

- [ ] **Step 4: Confirm the working tree is clean and the branch is ahead by the expected number of commits**

```sh
git status
git log --oneline origin/main..HEAD
```

Expected: clean tree. `git log` shows the spec commit (`a871fdc`), the implementation plan commit (this plan, committed during planning), the Task 2 implementation commit, and the Task 3 CHANGELOG commit — plus whatever was already ahead of origin from prior 0.1.3 docs work. Total: 4 plan/spec/impl commits on top of the 0.1.3 plan/spec commits already pending push.

---

## Notes for the implementer

- **No release tag here.** This plan implements the changes that *will* go into 0.1.2, but does not bump `version` in `Cargo.toml` or tag the release. That happens later via the `RELEASING.md` workflow when Sarah is ready to ship.
- **No tests are added.** A Cargo feature-gate change is a manifest concern; the verification is the build matrix, not unit tests. The existing test suite passing under both default and reduced feature sets is the regression guard.
- **The `init` simplification is intentionally not in this plan.** It's deferred to 0.1.3, where the tidy-data redesign forces a rewrite of the template anyway. See `project_addrust_roadmap` memory for the deferred follow-up.

---

## Task 5: Gate `rayon` behind a default-on `parallel` feature

**Added 2026-05-07 mid-PR**, after the duckdb-address-standardizer maintainer pointed out that the wrapper doesn't need rayon — DuckDB threads already execute a separate addrust call per thread, so `Pipeline::parse_batch`'s intra-batch work-stealing is unused on the wrapper path. Folded into 0.1.2 because it's the same conceptual scope as the `cli` gate (minimize the wrapper's compile surface).

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/pipeline.rs:157-161`

- [ ] **Step 1: Make `rayon` optional and add the `parallel` feature**

In `Cargo.toml`, change:

```toml
rayon = "1"
```

to:

```toml
rayon = { version = "1", optional = true }
```

In the `[features]` block, change:

```toml
default = ["duckdb", "cli"]
duckdb = ["dep:duckdb"]
cli = ["dep:clap", "dep:ratatui", "dep:crossterm"]
```

to:

```toml
default = ["duckdb", "cli", "parallel"]
duckdb = ["dep:duckdb"]
cli = ["dep:clap", "dep:ratatui", "dep:crossterm"]
parallel = ["dep:rayon"]
```

- [ ] **Step 2: Switch `Pipeline::parse_batch` to a feature-gated body**

In `src/pipeline.rs:157-161`, replace:

```rust
/// Parse a batch of addresses (parallel with rayon).
pub fn parse_batch(&self, inputs: &[&str]) -> Vec<Address> {
    use rayon::prelude::*;
    inputs.par_iter().map(|input| self.parse(input)).collect()
}
```

with:

```rust
/// Parse a batch of addresses.
///
/// With the default-on `parallel` feature, work is distributed across rayon's
/// thread pool. Without the feature, the batch is processed serially — the API
/// is unchanged so callers don't need to know which build they're against.
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

The signature is preserved. `lib.rs::parse_batch` and `duckdb_io::run_duckdb:168` (the only callers) work uniformly against both bodies.

- [ ] **Step 3: Verify the full build matrix**

Run:

```sh
cargo build
cargo test
cargo build --no-default-features
cargo build --no-default-features --features duckdb
cargo build --no-default-features --features cli
cargo build --no-default-features --features parallel
cargo test --no-default-features --features duckdb
cargo test --no-default-features
cargo build --manifest-path /Users/sj2690/projects/duckdb-address-standardizer/addrust-ffi/Cargo.toml
cargo clippy --all-targets
cargo clippy --no-default-features --all-targets
```

All must succeed.

- [ ] **Step 4: Verify rayon is also absent from the wrapper-style dep tree**

Run:

```sh
cargo tree --no-default-features --prefix none -e normal | grep -E '^(clap|ratatui|crossterm|rayon) ' | sort -u
```

Expected: zero output (now including rayon).

Sanity-check that rayon DOES appear when `parallel` is on:

```sh
cargo tree --no-default-features --features parallel --prefix none -e normal | grep -E '^rayon '
```

Expected: one line (`rayon v1.x`).

- [ ] **Step 5: Update CHANGELOG**

In `CHANGELOG.md` `## [Unreleased]` section, append a third bullet to `### Added` and a second bullet to `### Changed` for the new gate. (Exact text in the implementation commit.)

- [ ] **Step 6: Commit**

Three commits to keep the story discoverable in the PR history:

1. `docs: extend 0.1.2 spec and plan to include parallel feature` (spec + plan changes)
2. `feat: gate rayon behind default-on parallel feature` (Cargo.toml + pipeline.rs)
3. `docs: changelog for parallel feature` (CHANGELOG)
