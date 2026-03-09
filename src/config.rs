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
