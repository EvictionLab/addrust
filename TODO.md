# addrust TODO

## Upcoming releases

### 0.1.2 — `cli` feature gate
- Make `clap`, `ratatui`, `crossterm` optional and gate them behind a `cli` feature.
- Default `cli` to **on** so `cargo install --path .` keeps working with no flags. Wrapper crates already pass `default-features = false` and will get the slim build.
- Verify `addrust-ffi` build in `~/projects/duckdb-address-standardizer` shrinks accordingly.

### 0.1.3+ — additional regex changes
- (Placeholder — regex changes Sarah has in mind, to be filled in when we get to them.)

## Lower priority / future

- **Document panic-across-FFI behavior somewhere embedders will see it.** addrust panics freely; FFI consumers must set `panic = "abort"` (the wrapper does). Where to put this is open — not the README. Could be a doc comment on a future C-API module, or in the wrapper's own embedding docs.

## Notes for next time we're in the wrapper repo

- **Slim down `test/sql/addrust_parse_golden.test`.** Its header says "mirrors addrust tests/golden.rs" — it's a literal duplicate of addrust's own golden suite. Parsing-output goldens belong in addrust; the wrapper should test FFI/extension behavior (NULL handling, batch, config-cache invalidation, struct round-trip, multi-arg overloads). Consider deleting this file entirely and trimming `addrust_parse.test` to FFI-behavior smoke tests.
- The wrapper's own `TODO.md` already lists the addrust submodule bump and behavior-change notes — coordinate so we're not duplicating CHANGELOG content there.
