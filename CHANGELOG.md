All notable changes to addrust are recorded here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Under 0.x, changes to default-pipeline behavior are PATCH bumps signaled here under `Default pipeline changes`; only public Rust API or FFI surface changes force MINOR. After 1.0, default-pipeline changes will be MAJOR. See `RELEASING.md` for the full policy.

## [Unreleased]

## [0.1.1] - 2026-05-05

### Default pipeline changes

#### New steps
- **`street_number_coord`** — single-pair coord-style street number. `N 123 MAIN ST` → `street_number = N123`. Complements the existing two-pair `street_number_coords` (`N123 W456` Wisconsin grid).
- **`unit_bare`** — extracts trailing alphanumeric units with no `UNIT`/`APT` prefix (e.g. `MAIN ST 5B` → unit `5B`). Single-letter alternative excludes `N/S/E/W` so post-direction extraction can still claim them.
- **`dedupe_word`** — collapses adjacent duplicate suffix or direction words (e.g. `MAIN ST ST` → `MAIN ST`). Only matches literal repeats — `ST STREET` (different forms of the same suffix) is not collapsed; that requires post-standardization deduping, which is deferred to a future release.
- **`unstick_unit_type_unit`** — splits glued unit-type patterns like `APT5` → `APT 5`. Complements the existing `unstick_suffix_unit`.
- **`ordinal_to_word_no_num`** — converts space-separated ordinals like `1 ST AVE` → `FIRST AVE` (existing `ordinal_to_word` already handled the no-space form).
- **`street_name_squish`** — joins prefixes like `O BRIEN` → `OBRIEN`, `MC DONALD` → `MCDONALD`.

#### Removed steps
- **`stick_dir_number`** — direction-digit gluing was redundant; `street_number_coords` already tolerates spaces between segments via `\W?`. Removing this step also fixes `N 123 MAIN ST` parsing, which the old behavior would prevent from being recognized as `pre_direction=N, street_number=123`.
- **`pound_ordinal`** — folded into `ordinal_to_word` via an optional `#?` prefix in the pattern. Bonus: removes the `[RNTS][DHT]` cross-product that was matching false positives like `RH`/`NT`/`SD`.

#### Reordered
- **`street_name_highway` now runs BEFORE `highway_number_to_word`.** This lets highway-tag variants (e.g. `I` → `INTERSTATE`) expand first, so number-to-word conversion sees the canonical form. Previously the order meant variants like the new `I` prefix wouldn't fully resolve (digits got word-ified before the variant could match).

#### Pattern updates
- **`period_between`** — char class tightened from `[^\s]` to `[A-Z0-9]`. The step's purpose is to keep words from gluing when periods are stripped; broadening it to all non-whitespace was eating periods that should have stayed (e.g., between punctuation).
- **`ordinal_to_word`** — pattern now allows an optional leading `#`: `#?\b(\d{1,3})(ST|ND|RD|TH)\b`. Replaces the dropped `pound_ordinal` step with no behavior change for typical inputs.
- **`unstick_suffix_unit`** — pattern now also splits trailing digits: `\b({suffix:common})({unit_type}|\d)\b`. `ST5` → `ST 5`.
- **`highway_number_to_word`** — separator now allows hyphens: `[\s-]+`. `I-95` is now handled.
- **`unit_type_value`** — pattern alternation extended with `\d[A-Z]\d+` to catch unit values like `5B12`.
- **`city_state_zip`** — pattern split into asymmetric alternatives: `(?:^[A-Z][A-Z-]+|,\s*[A-Z][A-Z -]+)\W+({state})\W+(\d{5}...)$`. The `^`-anchored branch allows hyphen-joined single-word cities (e.g. `WINSTON-SALEM NC 12345`); the comma-anchored branch keeps multi-word support (e.g. `MAIN ST, WINSTON SALEM NC 12345`). Multi-word cities without a leading comma remain unsupported because the regex can't disambiguate them from street content.
- **`extra_front`** — added negative lookahead `(?!{direction}\s)` so a leading direction letter isn't captured as front-junk. Pattern keeps its two-alternative form (direction-then-digit case first) so the greedy match doesn't swallow a trailing direction letter inside the prefix. Lets the new `street_number_coord` step claim it.

#### Dictionary additions
- **`street_name`**: `PT` → `PORT` (with `PRT` variant, `start` tag); `ST RD` adds `STH` variant; `INTERSTATE` adds `I(?=\W\d{1,3})` variant so a bare `I` followed by a number expands to `INTERSTATE`.
- **`suffix`**: `BLVD` adds `BD` variant; `LOOP` adds `LP` variant.

### Added
- **`$long` accessor in pattern templates.** Mirrors the existing `$short` — `{street_name:highway$long}` expands to only the long forms of highway-tagged entries. Combinable with tag filters: `{table}`, `{table:tag}`, `{table$short}`, `{table$long}`, `{table:tag$short}`, `{table:tag$long}` are all supported. Empty longs (na-style tables) are filtered.

### Fixed
- `unstick_number_letters` now handles single-letter direction suffixes. `3862S LAKE DR` (and similar `123N`, `456W`, etc.) now correctly splits the digits from the trailing direction. Previously the regex required two or more trailing letters, leaving these inputs un-split.
- `street_number_coords` default replacement was using `${N}` braced syntax, but extract-step replacements only support unbraced `$N`. Inputs like `N123 W456 MAIN ST` were producing the literal string `${1}${2} ${3}${4}` as the street number. Switched the default replacement to `$1$2 $3$4`. Note: braced `${N}` syntax remains supported in rewrite-step replacements (where it's needed for table lookups like `${1:state}` and fraction expansion).

### Notes for embedders bumping past pre-tag SHAs

`0.1.0` was never tagged. If you were pinning to a SHA on `main` before this release, several user-visible behavior changes have already happened that may affect goldens beyond what's listed above:

- **`finalize` no longer expands directional/highway abbreviations on `street_name`.** Previously did an implicit dictionary lookup that turned `I` → `INTERSTATE`, `HWY` → `HIGHWAY`, etc., post-extraction. Now finalize only assigns the leftover working string to `street_name`. To preserve the old behavior in your config, add an explicit `street_name_highway` (or equivalent) step.
- Default pipeline steps overhauled (`feat: overhaul default pipeline steps`).
- Default dictionary entries expanded (`feat: expand default dictionary entries`).
- `step_overrides` with only `pattern` set now correctly preserves untouched fields (`replacement`, `table`, etc.). Previously corrupted overridden steps — this was the symptom that caused `highway_number_to_word` to be silently broken in the published extension.
- `highway_number_to_word` now fires correctly (was a no-op).
- MT/FT defaults now have correct start tags.
- Canonical overrides now replace tags instead of merging (was producing wrong output).
- `duckdb` is now a default feature. Embedders not wanting DuckDB pulled in should set `default-features = false` (most already do).

[Unreleased]: https://github.com/EvictionLab/addrust/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/EvictionLab/addrust/releases/tag/v0.1.1
