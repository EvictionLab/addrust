use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Short,
    Long,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct OutputConfig {
    #[serde(skip_serializing_if = "is_long")]
    pub suffix: OutputFormat,
    #[serde(skip_serializing_if = "is_short")]
    pub direction: OutputFormat,
    #[serde(skip_serializing_if = "is_long")]
    pub unit_type: OutputFormat,
    #[serde(skip_serializing_if = "is_long")]
    pub unit_location: OutputFormat,
    #[serde(skip_serializing_if = "is_short")]
    pub state: OutputFormat,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            suffix: OutputFormat::Long,
            direction: OutputFormat::Short,
            unit_type: OutputFormat::Long,
            unit_location: OutputFormat::Long,
            state: OutputFormat::Short,
        }
    }
}

impl OutputConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn is_long(f: &OutputFormat) -> bool { *f == OutputFormat::Long }
fn is_short(f: &OutputFormat) -> bool { *f == OutputFormat::Short }

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Config {
    #[serde(skip_serializing_if = "RulesConfig::is_empty")]
    pub rules: RulesConfig,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub dictionaries: HashMap<String, DictOverrides>,
    #[serde(default, skip_serializing_if = "OutputConfig::is_default")]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled_groups: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub pattern_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DictOverrides {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<DictEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
    #[serde(rename = "override", skip_serializing_if = "Vec::is_empty")]
    pub override_entries: Vec<DictEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DictEntry {
    pub short: String,
    pub long: String,
}

impl RulesConfig {
    /// Returns true if both disabled and disabled_groups are empty.
    pub fn is_empty(&self) -> bool {
        self.disabled.is_empty() && self.disabled_groups.is_empty() && self.pattern_overrides.is_empty()
    }
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

    /// Serialize to a pretty TOML string. Empty configs produce an empty string.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Save config to a TOML file. Removes the file if the config is empty (all defaults).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let content = self.to_toml();
        if content.trim().is_empty() {
            // Config is all defaults — remove the file if it exists
            match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            std::fs::write(path, content)
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

    #[test]
    fn test_serialize_empty_config_is_empty() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert_eq!(toml_str.trim(), "");
    }

    #[test]
    fn test_serialize_disabled_rules() {
        let mut config = Config::default();
        config.rules.disabled = vec!["po_box_number".to_string()];
        let toml_str = config.to_toml();
        assert!(toml_str.contains("[rules]"));
        assert!(toml_str.contains("po_box_number"));
    }

    #[test]
    fn test_serialize_dict_overrides() {
        let mut config = Config::default();
        config.dictionaries.insert("suffix".to_string(), DictOverrides {
            add: vec![DictEntry { short: "PSGE".into(), long: "PASSAGE".into() }],
            remove: vec!["TRAILER".into()],
            override_entries: vec![],
        });
        let toml_str = config.to_toml();
        assert!(toml_str.contains("[dictionaries.suffix]"));
        assert!(toml_str.contains("PSGE"));
        assert!(toml_str.contains("TRAILER"));
    }

    #[test]
    fn test_parse_pattern_overrides() {
        let toml_str = r#"
[rules]
disabled = ["unit_pound"]

[rules.pattern_overrides]
unit_type_value = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+)\s*$'
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.disabled, vec!["unit_pound"]);
        let override_val = config.rules.pattern_overrides.get("unit_type_value").unwrap();
        assert!(override_val.contains("\\d+"));
        assert!(!override_val.contains("[A-Z])\\s*$"));
    }

    #[test]
    fn test_serialize_pattern_overrides() {
        let mut config = Config::default();
        config.rules.pattern_overrides.insert(
            "suffix_common".to_string(),
            r"(?<!^)\b({suffix_common})\s*$".to_string(),
        );
        let toml_str = config.to_toml();
        assert!(toml_str.contains("[rules.pattern_overrides]"));
        assert!(toml_str.contains("suffix_common"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert!(parsed.rules.pattern_overrides.contains_key("suffix_common"));
    }

    #[test]
    fn test_empty_pattern_overrides_not_serialized() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert!(!toml_str.contains("pattern_overrides"));
    }

    #[test]
    fn test_roundtrip_config() {
        let mut config = Config::default();
        config.rules.disabled = vec!["po_box_number".to_string()];
        config.rules.disabled_groups = vec!["suffix".to_string()];
        config.dictionaries.insert("unit_type".to_string(), DictOverrides {
            add: vec![DictEntry { short: "WH".into(), long: "WAREHOUSE".into() }],
            remove: vec![],
            override_entries: vec![DictEntry { short: "STE".into(), long: "SUITE NUMBER".into() }],
        });
        let toml_str = config.to_toml();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.rules.disabled, vec!["po_box_number"]);
        assert_eq!(parsed.rules.disabled_groups, vec!["suffix"]);
        let unit = parsed.dictionaries.get("unit_type").unwrap();
        assert_eq!(unit.add.len(), 1);
        assert_eq!(unit.override_entries.len(), 1);
    }

    #[test]
    fn test_parse_output_config() {
        let toml_str = r#"
[output]
suffix = "short"
direction = "long"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.output.suffix, OutputFormat::Short);
        assert_eq!(config.output.direction, OutputFormat::Long);
        assert_eq!(config.output.unit_type, OutputFormat::Long);
        assert_eq!(config.output.state, OutputFormat::Short);
    }

    #[test]
    fn test_output_config_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.output.suffix, OutputFormat::Long);
        assert_eq!(config.output.direction, OutputFormat::Short);
        assert_eq!(config.output.unit_type, OutputFormat::Long);
        assert_eq!(config.output.unit_location, OutputFormat::Long);
        assert_eq!(config.output.state, OutputFormat::Short);
    }

    #[test]
    fn test_serialize_output_config_only_non_defaults() {
        let mut config = Config::default();
        config.output.suffix = OutputFormat::Short;
        let toml_str = config.to_toml();
        assert!(toml_str.contains("[output]"));
        assert!(toml_str.contains("suffix"));
        assert!(!toml_str.contains("direction"));
    }

    #[test]
    fn test_roundtrip_output_config() {
        let mut config = Config::default();
        config.output.suffix = OutputFormat::Short;
        config.output.direction = OutputFormat::Long;
        let toml_str = config.to_toml();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.output.suffix, OutputFormat::Short);
        assert_eq!(parsed.output.direction, OutputFormat::Long);
        assert_eq!(parsed.output.unit_type, OutputFormat::Long);
    }

    #[test]
    fn test_empty_config_no_output_section() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert!(!toml_str.contains("[output]"));
    }
}
