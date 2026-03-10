# Pipeline Configuration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Let users configure the parsing pipeline (enable/disable rules, patch dictionaries) via a `.addrust.toml` config file, with CLI subcommands for discoverability.

**Architecture:** A `Config` struct deserializes from TOML. Dictionary patches are applied to cloned abbreviation tables before `build_rules()` runs. The CLI moves to clap subcommands. `Pipeline` gains `from_config()` and `default()` constructors.

**Tech Stack:** serde + toml for config parsing, clap subcommands for CLI.

---

### Task 1: Add serde and toml dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add dependencies**

Add to `[dependencies]` in `Cargo.toml`:

```toml
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no new errors

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add serde and toml for config file support"
```

---

### Task 2: Create config module with TOML deserialization

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs` (add `pub mod config;`)
- Test: `src/config.rs` (inline tests)

**Step 1: Write the failing test**

In `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_config() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.rules.disabled.is_empty());
        assert!(config.rules.disabled_groups.is_empty());
        assert!(config.dictionaries.is_empty());
    }

    #[test]
    fn test_parse_rules_config() {
        let toml_str = r#"
[rules]
disabled = ["po_box_number", "unit_location"]
disabled_groups = ["po_box"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.disabled, vec!["po_box_number", "unit_location"]);
        assert_eq!(config.rules.disabled_groups, vec!["po_box"]);
    }

    #[test]
    fn test_parse_dictionary_overrides() {
        let toml_str = r#"
[dictionaries.suffix]
add = [{ short = "PSGE", long = "PASSAGE" }]
remove = ["TRAILER"]

[dictionaries.unit_type]
override = [{ short = "STE", long = "SUITE NUMBER" }]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let suffix = config.dictionaries.get("suffix").unwrap();
        assert_eq!(suffix.add.len(), 1);
        assert_eq!(suffix.add[0].short, "PSGE");
        assert_eq!(suffix.remove, vec!["TRAILER"]);

        let unit = config.dictionaries.get("unit_type").unwrap();
        assert_eq!(unit.override_entries.len(), 1);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let config = Config::load(Path::new("nonexistent.toml"));
        assert!(config.rules.disabled.is_empty());
    }
}
```

Add `pub mod config;` to `src/lib.rs` after the existing module declarations.

**Step 2: Run tests to verify they fail**

Run: `cargo test config::tests`
Expected: FAIL — module doesn't exist yet

**Step 3: Implement the config structs**

In `src/config.rs`:

```rust
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub rules: RulesConfig,
    pub dictionaries: HashMap<String, DictOverrides>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    pub disabled: Vec<String>,
    pub disabled_groups: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DictOverrides {
    pub add: Vec<DictEntry>,
    pub remove: Vec<String>,
    #[serde(rename = "override")]
    pub override_entries: Vec<DictEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DictEntry {
    pub short: String,
    pub long: String,
}

impl Config {
    /// Load config from a TOML file. Returns default if file doesn't exist.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
                eprintln!("Warning: failed to parse {}: {}", path.display(), e);
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test config::tests`
Expected: all 4 tests PASS

**Step 5: Commit**

```bash
git add src/config.rs src/lib.rs
git commit -m "feat: add Config struct with TOML deserialization"
```

---

### Task 3: Add dictionary patching to Abbreviations

**Files:**
- Modify: `src/tables/abbreviations.rs`
- Test: `src/tables/abbreviations.rs` (inline tests)

**Step 1: Write the failing test**

Add to `src/tables/abbreviations.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DictEntry, DictOverrides};

    #[test]
    fn test_patch_add() {
        let table = AbbrTable::new(vec![abbr("ST", "STREET")]);
        let overrides = DictOverrides {
            add: vec![DictEntry { short: "PSGE".into(), long: "PASSAGE".into() }],
            remove: vec![],
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert!(patched.to_long("PSGE").is_some());
        assert_eq!(patched.to_long("PSGE"), Some("PASSAGE"));
        assert_eq!(patched.to_long("ST"), Some("STREET"));
    }

    #[test]
    fn test_patch_remove() {
        let table = AbbrTable::new(vec![
            abbr("ST", "STREET"),
            abbr("AVE", "AVENUE"),
        ]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec!["STREET".into()],
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert!(patched.to_long("ST").is_none());
        assert_eq!(patched.to_long("AVE"), Some("AVENUE"));
    }

    #[test]
    fn test_patch_override() {
        let table = AbbrTable::new(vec![abbr("STE", "SUITE")]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec![],
            override_entries: vec![DictEntry { short: "STE".into(), long: "SUITE NUMBER".into() }],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.to_long("STE"), Some("SUITE NUMBER"));
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test tables::abbreviations::tests`
Expected: FAIL — `patch` method doesn't exist

**Step 3: Implement `patch` on AbbrTable**

Add to `impl AbbrTable` in `src/tables/abbreviations.rs`:

```rust
    /// Apply dictionary overrides: add, remove, then override entries.
    pub fn patch(&self, overrides: &crate::config::DictOverrides) -> Self {
        let mut entries = self.entries.clone();

        // Remove: filter out entries matching short or long form
        for remove_val in &overrides.remove {
            let upper = remove_val.to_uppercase();
            entries.retain(|e| e.short != upper && e.long != upper);
        }

        // Override: replace long form for matching short
        for ov in &overrides.override_entries {
            let short_upper = ov.short.to_uppercase();
            let long_upper = ov.long.to_uppercase();
            for entry in &mut entries {
                if entry.short == short_upper {
                    entry.long = long_upper.clone();
                }
            }
        }

        // Add: append new entries
        for add in &overrides.add {
            entries.push(Abbr {
                short: add.short.to_uppercase(),
                long: add.long.to_uppercase(),
            });
        }

        AbbrTable::new(entries)
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test tables::abbreviations::tests`
Expected: all 3 tests PASS

**Step 5: Commit**

```bash
git add src/tables/abbreviations.rs
git commit -m "feat: add dictionary patching (add/remove/override)"
```

---

### Task 4: Make Abbreviations patchable and build_rules accept tables

**Files:**
- Modify: `src/tables/abbreviations.rs` (add `patch` to `Abbreviations`, add `build_default_tables`)
- Modify: `src/tables/rules.rs` (change `build_rules` signature)
- Modify: `src/tables/mod.rs` (re-export)

**Step 1: Add `patch` to `Abbreviations` and `build_default_tables`**

In `src/tables/abbreviations.rs`, add to `impl Abbreviations`:

```rust
    /// Apply config overrides to matching tables, returning a new Abbreviations.
    pub fn patch(&self, dict_overrides: &HashMap<String, crate::config::DictOverrides>) -> Self {
        let mut tables = self.tables.clone();
        for (name, overrides) in dict_overrides {
            if let Some(table) = tables.get(name) {
                tables.insert(name.clone(), table.patch(overrides));
            }
        }
        Abbreviations { tables }
    }

    /// List available table names.
    pub fn table_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tables.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }
```

Add a public function to build default tables (non-static, for patching):

```rust
/// Build the default abbreviation tables (non-static, for patching).
pub fn build_default_tables() -> Abbreviations {
    let mut tables = HashMap::new();
    tables.insert("direction".to_string(), build_directions());
    tables.insert("unit_type".to_string(), build_unit_types());
    tables.insert("unit_location".to_string(), build_unit_locations());
    tables.insert("state".to_string(), build_states());
    tables.insert("usps_suffix".to_string(), build_usps_suffixes());
    tables.insert("all_suffix".to_string(), build_all_suffixes());
    tables.insert("common_suffix".to_string(), build_common_suffixes());
    Abbreviations { tables }
}
```

**Step 2: Change `build_rules` to accept `&Abbreviations`**

In `src/tables/rules.rs`, change the signature:

```rust
pub fn build_rules(abbr: &Abbreviations) -> Vec<Rule> {
    // Remove: let abbr = &*ABBR;
    // Rest of function stays the same, it already uses `abbr` as a local variable
```

Remove the `use crate::tables::abbreviations::ABBR;` import, add `use crate::tables::abbreviations::Abbreviations;` if needed.

**Step 3: Update all callers of `build_rules()`**

In `src/lib.rs`, update both functions:

```rust
use tables::abbreviations::{build_default_tables, ABBR};

pub fn parse(input: &str) -> Address {
    let rules = build_rules(&ABBR);
    // ...
}

pub fn parse_batch(inputs: &[&str]) -> Vec<Address> {
    let rules = build_rules(&ABBR);
    // ...
}
```

In `src/main.rs`:

```rust
use addrust::tables::abbreviations::ABBR;

// ...
let rules = build_rules(&ABBR);
```

In `tests/golden.rs`:

```rust
use addrust::tables::abbreviations::ABBR;

fn pipeline() -> Pipeline {
    Pipeline::new(build_rules(&ABBR), &PipelineConfig::default())
}
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: all existing tests PASS (no behavior change)

**Step 5: Commit**

```bash
git add src/tables/abbreviations.rs src/tables/rules.rs src/lib.rs src/main.rs tests/golden.rs
git commit -m "refactor: make build_rules accept Abbreviations parameter"
```

---

### Task 5: Add Pipeline::from_config and Pipeline::default

**Files:**
- Modify: `src/pipeline.rs`
- Modify: `src/lib.rs`
- Test: inline in `src/pipeline.rs`

**Step 1: Write the failing test**

Add to `src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_default_parses() {
        let p = Pipeline::default();
        let addr = p.parse("123 Main St");
        assert_eq!(addr.street_number.as_deref(), Some("123"));
        assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
        assert_eq!(addr.suffix.as_deref(), Some("STREET"));
    }

    #[test]
    fn test_pipeline_from_config_with_disabled_rule() {
        let toml_str = r#"
[rules]
disabled_groups = ["suffix"]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 Main St");
        assert_eq!(addr.street_number.as_deref(), Some("123"));
        assert!(addr.suffix.is_none()); // suffix extraction disabled
    }

    #[test]
    fn test_pipeline_from_config_with_dict_override() {
        let toml_str = r#"
[dictionaries.suffix]
remove = ["STREET"]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 Main St");
        // ST no longer recognized as suffix, becomes part of street name
        assert!(addr.suffix.is_none());
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test pipeline::tests`
Expected: FAIL — `Pipeline::default()` and `Pipeline::from_config()` don't exist

**Step 3: Implement the constructors**

Add to `impl Pipeline` in `src/pipeline.rs`:

```rust
    /// Build pipeline from a Config (file-based configuration).
    pub fn from_config(config: &crate::config::Config) -> Self {
        use crate::tables::abbreviations::build_default_tables;
        use crate::tables::build_rules;

        let tables = build_default_tables();
        let tables = if config.dictionaries.is_empty() {
            tables
        } else {
            tables.patch(&config.dictionaries)
        };

        let rules = build_rules(&tables);

        let pipeline_config = PipelineConfig {
            disabled_rules: config.rules.disabled.clone(),
            disabled_groups: config.rules.disabled_groups.clone(),
        };

        Self::new(rules, &pipeline_config)
    }
```

```rust
impl Default for Pipeline {
    fn default() -> Self {
        use crate::tables::abbreviations::ABBR;
        use crate::tables::build_rules;

        let rules = build_rules(&ABBR);
        Self { rules }
    }
}
```

Update `src/lib.rs` to use the new constructors:

```rust
pub fn parse(input: &str) -> Address {
    let pipeline = Pipeline::default();
    pipeline.parse(input)
}

pub fn parse_batch(inputs: &[&str]) -> Vec<Address> {
    let pipeline = Pipeline::default();
    pipeline.parse_batch(inputs)
}
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 5: Commit**

```bash
git add src/pipeline.rs src/lib.rs
git commit -m "feat: add Pipeline::from_config and Pipeline::default"
```

---

### Task 6: Refactor CLI to subcommands

**Files:**
- Modify: `src/main.rs`

**Step 1: Rewrite main.rs with clap subcommands**

```rust
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

use addrust::address::Address;
use addrust::config::Config;
use addrust::pipeline::Pipeline;

#[derive(Parser)]
#[command(name = "addrust", about = "Parse and standardize US addresses")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to config file (default: .addrust.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse addresses from stdin
    Parse {
        /// Output format: "clean" (default), "full", or "tsv"
        #[arg(long, default_value = "clean")]
        format: String,
        /// Show timing information
        #[arg(long)]
        time: bool,
    },
    /// Generate a default .addrust.toml config file
    Init,
    /// List pipeline rules or dictionary tables
    List {
        #[command(subcommand)]
        what: ListCommands,
    },
    /// Interactive pipeline editor (coming soon)
    Configure,
}

#[derive(Subcommand)]
enum ListCommands {
    /// List all pipeline rules in order
    Rules,
    /// List dictionary tables (optionally show entries for a specific table)
    Tables {
        /// Table name to show entries for
        name: Option<String>,
    },
}
```

The `None` case for `command` (bare `addrust` with stdin) should behave like `parse --format clean` for backwards compatibility.

**Step 2: Verify it compiles and existing behavior works**

Run: `echo "123 Main St" | cargo run -- parse`
Expected: outputs cleaned address

Run: `echo "123 Main St" | cargo run`
Expected: same output (backwards compat)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "refactor: move CLI to subcommand structure"
```

---

### Task 7: Implement `addrust list rules`

**Files:**
- Modify: `src/main.rs` (handler)
- Modify: `src/pipeline.rs` (expose rule metadata)

**Step 1: Add rule metadata access to Pipeline**

Add to `impl Pipeline` in `src/pipeline.rs`:

```rust
    /// Get metadata about all rules for display purposes.
    pub fn rule_summaries(&self) -> Vec<RuleSummary> {
        self.rules.iter().map(|r| RuleSummary {
            label: r.label.clone(),
            group: r.group.clone(),
            action: r.action,
            enabled: r.enabled,
        }).collect()
    }
```

Add a public struct:

```rust
#[derive(Debug)]
pub struct RuleSummary {
    pub label: String,
    pub group: String,
    pub action: Action,
    pub enabled: bool,
}
```

**Step 2: Implement the handler in main.rs**

In the `List { what: ListCommands::Rules }` arm:

```rust
let pipeline = Pipeline::from_config(&config);
for (i, rule) in pipeline.rule_summaries().iter().enumerate() {
    let status = if rule.enabled { " " } else { "x" };
    println!("{:>3}. [{}] {:30} {:12} {:?}", i + 1, status, rule.label, rule.group, rule.action);
}
```

**Step 3: Test manually**

Run: `cargo run -- list rules`
Expected: numbered list of all rules with labels, groups, actions

**Step 4: Commit**

```bash
git add src/main.rs src/pipeline.rs
git commit -m "feat: add 'addrust list rules' command"
```

---

### Task 8: Implement `addrust list tables`

**Files:**
- Modify: `src/main.rs` (handler)

**Step 1: Implement the handler**

In the `List { what: ListCommands::Tables { name } }` arm:

```rust
use addrust::tables::abbreviations::build_default_tables;

let tables = build_default_tables();
// Apply config patches if present
let tables = if config.dictionaries.is_empty() {
    tables
} else {
    tables.patch(&config.dictionaries)
};

match name {
    None => {
        for name in tables.table_names() {
            let table = tables.get(name).unwrap();
            println!("{:20} ({} entries)", name, table.entries.len());
        }
    }
    Some(name) => {
        match tables.get(&name) {
            Some(table) => {
                println!("{} ({} entries):", name, table.entries.len());
                for entry in &table.entries {
                    println!("  {:20} → {}", entry.short, entry.long);
                }
            }
            None => {
                eprintln!("Unknown table: {}", name);
                eprintln!("Available: {}", tables.table_names().join(", "));
                std::process::exit(1);
            }
        }
    }
}
```

**Step 2: Test manually**

Run: `cargo run -- list tables`
Expected: list of table names with entry counts

Run: `cargo run -- list tables direction`
Expected: direction entries (N → NORTH, etc.)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add 'addrust list tables' command"
```

---

### Task 9: Implement `addrust init`

**Files:**
- Modify: `src/main.rs` (handler)
- Create: `src/init.rs` (config file generator)
- Modify: `src/lib.rs` (add `pub mod init;`)

**Step 1: Write the init module**

`src/init.rs` generates a commented TOML string showing all rules and dictionary entries:

```rust
use crate::config::Config;
use crate::pipeline::Pipeline;
use crate::tables::abbreviations::build_default_tables;

/// Generate a default .addrust.toml content string with all rules and tables.
pub fn generate_default_config() -> String {
    let mut out = String::new();

    out.push_str("# addrust pipeline configuration\n");
    out.push_str("# Uncomment and edit to customize parsing behavior.\n\n");

    // Rules section
    out.push_str("[rules]\n");
    out.push_str("# Disable individual rules by label:\n");
    out.push_str("# disabled = []\n");
    out.push_str("#\n");
    out.push_str("# Disable entire groups:\n");
    out.push_str("# disabled_groups = []\n");
    out.push_str("#\n");
    out.push_str("# Available rules (in pipeline order):\n");

    let pipeline = Pipeline::default();
    for rule in pipeline.rule_summaries() {
        out.push_str(&format!("#   {:30} (group: {}, action: {:?})\n", rule.label, rule.group, rule.action));
    }

    out.push_str("\n");

    // Dictionary sections
    let tables = build_default_tables();
    for name in tables.table_names() {
        let table = tables.get(name).unwrap();
        out.push_str(&format!("# [dictionaries.{}]\n", name));
        out.push_str(&format!("# {} entries. Examples:\n", table.entries.len()));
        for entry in table.entries.iter().take(3) {
            out.push_str(&format!("#   {} → {}\n", entry.short, entry.long));
        }
        out.push_str("# add = [{ short = \"XX\", long = \"EXAMPLE\" }]\n");
        out.push_str("# remove = [\"VALUE\"]\n");
        out.push_str("# override = [{ short = \"XX\", long = \"NEW LONG\" }]\n");
        out.push_str("\n");
    }

    out
}
```

**Step 2: Wire up the handler in main.rs**

In the `Init` arm:

```rust
use addrust::init::generate_default_config;
use std::path::Path;

let path = Path::new(".addrust.toml");
if path.exists() {
    eprintln!(".addrust.toml already exists. Overwrite? (y/N)");
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).unwrap();
    if !answer.trim().eq_ignore_ascii_case("y") {
        eprintln!("Aborted.");
        std::process::exit(0);
    }
}
let content = generate_default_config();
std::fs::write(path, content).unwrap();
println!("Created .addrust.toml");
```

**Step 3: Test manually**

Run: `cargo run -- init`
Expected: creates `.addrust.toml` with commented-out rules and dictionary examples

Run: `cat .addrust.toml`
Expected: readable, self-documenting config file

**Step 4: Commit**

```bash
git add src/init.rs src/lib.rs src/main.rs
git commit -m "feat: add 'addrust init' to generate default config"
```

---

### Task 10: Wire config loading into parse subcommand

**Files:**
- Modify: `src/main.rs`

**Step 1: Update parse handler to load config**

The parse command (and bare stdin fallback) should load `.addrust.toml` if it exists:

```rust
let config_path = cli.config.unwrap_or_else(|| PathBuf::from(".addrust.toml"));
let config = Config::load(&config_path);
let pipeline = Pipeline::from_config(&config);
```

This replaces the current `build_rules()` + `PipelineConfig` construction.

**Step 2: Test with a config file**

Create a test `.addrust.toml`:
```toml
[rules]
disabled_groups = ["suffix"]
```

Run: `echo "123 Main St" | cargo run -- parse --format full`
Expected: suffix field is empty (suffix extraction disabled)

Remove the test config, run again:
Expected: suffix field shows "STREET"

**Step 3: Run all tests**

Run: `cargo test`
Expected: all tests PASS

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: load .addrust.toml config in parse command"
```

---

### Task 11: Add integration test for config-driven parsing

**Files:**
- Create: `tests/config.rs`

**Step 1: Write integration tests**

```rust
use addrust::config::Config;
use addrust::pipeline::Pipeline;

#[test]
fn test_config_disables_suffix_group() {
    let config: Config = toml::from_str(r#"
[rules]
disabled_groups = ["suffix"]
"#).unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main St");
    assert_eq!(addr.street_number.as_deref(), Some("123"));
    assert!(addr.suffix.is_none());
    assert_eq!(addr.street_name.as_deref(), Some("MAIN ST"));
}

#[test]
fn test_config_adds_custom_suffix() {
    let config: Config = toml::from_str(r#"
[dictionaries.all_suffix]
add = [{ short = "PSGE", long = "PASSAGE" }]
"#).unwrap();
    let p = Pipeline::from_config(&config);
    let addr = p.parse("123 Main Psge");
    assert_eq!(addr.suffix.as_deref(), Some("PASSAGE"));
}

#[test]
fn test_default_pipeline_matches_no_config() {
    let default_p = Pipeline::default();
    let config_p = Pipeline::from_config(&Config::default());

    let addr1 = default_p.parse("123 N Main St Apt 4");
    let addr2 = config_p.parse("123 N Main St Apt 4");

    assert_eq!(addr1.street_number, addr2.street_number);
    assert_eq!(addr1.pre_direction, addr2.pre_direction);
    assert_eq!(addr1.street_name, addr2.street_name);
    assert_eq!(addr1.suffix, addr2.suffix);
    assert_eq!(addr1.unit, addr2.unit);
}
```

**Step 2: Run tests**

Run: `cargo test --test config`
Expected: all 3 PASS

**Step 3: Commit**

```bash
git add tests/config.rs
git commit -m "test: add integration tests for config-driven parsing"
```

---

### Task 12: Add .addrust.toml to .gitignore

**Files:**
- Modify: `.gitignore`

**Step 1: Add config file to gitignore**

The generated `.addrust.toml` is per-project user config, shouldn't be committed by default (users can override with `git add -f`):

Add to `.gitignore`:
```
.addrust.toml
```

**Step 2: Commit**

```bash
git add .gitignore
git commit -m "chore: add .addrust.toml to gitignore"
```
