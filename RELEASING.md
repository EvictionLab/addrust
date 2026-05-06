# Releasing addrust

## Versioning policy

addrust follows [Semantic Versioning](https://semver.org/), with one project-specific rule:

**While in 0.x**, the defaults in `data/defaults/*.toml` are explicitly *not* a stability contract — they're being actively iterated. Default-pipeline changes are PATCH bumps, signaled in the CHANGELOG under a `Default pipeline changes` subsection so downstream embedders can see the diff. Only public Rust API or FFI surface changes force MINOR.

| Change type | 0.x bump | After 1.0 |
|---|---|---|
| Public Rust API removed or renamed | MINOR | MAJOR |
| FFI/C-API surface change | MINOR | MAJOR |
| Default-pipeline behavior change (edits to `data/defaults/*.toml`, semantics of `prepare`/`finalize`/extraction steps) | PATCH (CHANGELOG-flagged) | MAJOR |
| Bug fix | PATCH | PATCH |
| New opt-in feature (Cargo feature, new pub item) | PATCH | MINOR |
| Internal refactor, docs, tests | PATCH | PATCH |

The 1.0 transition is the moment defaults become contractual. Until then, the CHANGELOG carries the load — embedders watch the `Default pipeline changes` subsection, not the version number.

## Release checklist

1. **Update `CHANGELOG.md`.**
   - Move `[Unreleased]` entries into a new `[X.Y.Z] - YYYY-MM-DD` section.
   - Confirm any default-pipeline or behavior changes are flagged in their own subsection so embedders see them.
   - Update the link references at the bottom.
2. **Bump `version` in `Cargo.toml`.**
3. **Run the full test suite.** `cargo test` — all of `unit`, `golden`, `config`, `duckdb_integration` must pass.
4. **Commit.** Message: `chore: release vX.Y.Z`.
5. **Tag.** `git tag vX.Y.Z && git push origin main --tags`.
6. **Notify downstream.** Bump the `addrust` submodule in `duckdb-address-standardizer` to the new tag and update its goldens if any default-pipeline changes are flagged in this release's CHANGELOG.

## Commit conventions

Commits follow [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new functionality (often MINOR)
- `fix:` — bug fix (often PATCH; MINOR if behavior-correcting)
- `refactor:` — internal restructure, no behavior change (PATCH)
- `docs:` — documentation only (PATCH)
- `test:` — tests only (PATCH)
- `chore:` — tooling, releases (PATCH)

Anything that touches `data/defaults/*.toml` or pipeline semantics should call that out in the commit body so it's easy to identify at release time.
