use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::io;

use crate::step::StepDef;

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

    pub fn format_for_field(&self, field: crate::address::Field) -> OutputFormat {
        use crate::address::Field;
        match field {
            Field::Suffix => self.suffix,
            Field::PreDirection | Field::PostDirection => self.direction,
            Field::Unit => self.unit_location,
            _ => OutputFormat::Long,
        }
    }
}

fn is_long(f: &OutputFormat) -> bool { *f == OutputFormat::Long }
fn is_short(f: &OutputFormat) -> bool { *f == OutputFormat::Short }

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct StepsConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub pattern_overrides: HashMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub step_order: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_steps: Vec<StepDef>,
}

impl StepsConfig {
    pub fn is_empty(&self) -> bool {
        self.disabled.is_empty()
            && self.pattern_overrides.is_empty()
            && self.step_order.is_empty()
            && self.custom_steps.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Config {
    #[serde(skip_serializing_if = "StepsConfig::is_empty")]
    pub steps: StepsConfig,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub dictionaries: HashMap<String, DictOverrides>,
    #[serde(default, skip_serializing_if = "OutputConfig::is_default")]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct DictOverrides {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<DictEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
    /// Deprecated: use `add` with `canonical = true` instead.
    /// Kept for backward compatibility with existing user config files.
    /// Treated as `add` entries with `canonical = true` during patch.
    #[serde(rename = "override", skip_serializing_if = "Vec::is_empty")]
    pub override_entries: Vec<DictEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct DictEntry {
    pub short: String,
    pub long: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<bool>,
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
    fn test_empty_config_serializes_empty() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert_eq!(toml_str.trim(), "");
        assert!(!toml_str.contains("[output]"));
        assert!(!toml_str.contains("pattern_overrides"));
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let config = Config::load(Path::new("nonexistent.toml"));
        assert!(config.steps.disabled.is_empty());
    }

    #[test]
    fn test_roundtrip_full_config() {
        let mut config = Config::default();
        config.steps.disabled = vec!["po_box".to_string()];
        config.steps.pattern_overrides.insert(
            "suffix_common".to_string(),
            r"(?<!^)\b({suffix_common})\s*$".to_string(),
        );
        config.dictionaries.insert("unit_type".to_string(), DictOverrides {
            add: vec![DictEntry { short: "WH".into(), long: "WAREHOUSE".into(), ..Default::default() }],
            remove: vec![],
            override_entries: vec![DictEntry { short: "STE".into(), long: "SUITE NUMBER".into(), ..Default::default() }],
        });
        config.output.suffix = OutputFormat::Short;
        config.output.direction = OutputFormat::Long;

        let toml_str = config.to_toml();
        assert!(toml_str.contains("[steps]"));
        assert!(toml_str.contains("[steps.pattern_overrides]"));
        assert!(toml_str.contains("[output]"));
        // Non-default output fields serialized, defaults omitted
        assert!(toml_str.contains("suffix"));
        assert!(!toml_str.contains("unit_type = "));

        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.steps.disabled, vec!["po_box"]);
        assert!(parsed.steps.pattern_overrides.contains_key("suffix_common"));
        assert_eq!(parsed.output.suffix, OutputFormat::Short);
        assert_eq!(parsed.output.direction, OutputFormat::Long);
        assert_eq!(parsed.output.unit_type, OutputFormat::Long); // default preserved
        let unit = parsed.dictionaries.get("unit_type").unwrap();
        assert_eq!(unit.add.len(), 1);
        assert_eq!(unit.override_entries.len(), 1);
    }

    #[test]
    fn test_step_order_roundtrip() {
        let mut config = Config::default();
        config.steps.step_order = vec!["po_box".to_string(), "na_check".to_string()];
        let toml_str = config.to_toml();
        assert!(toml_str.contains("step_order"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.steps.step_order, vec!["po_box", "na_check"]);
    }

    #[test]
    fn test_step_order_empty_not_serialized() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert!(!toml_str.contains("step_order"));
    }

    #[test]
    fn test_steps_config_is_empty_with_step_order() {
        let mut sc = StepsConfig::default();
        assert!(sc.is_empty());
        sc.step_order = vec!["na_check".to_string()];
        assert!(!sc.is_empty());
    }

    #[test]
    fn test_custom_steps_roundtrip() {
        let toml_str = r#"
[[steps.custom_steps]]
type = "extract"
label = "custom_po_box_digits"
pattern = '\bBOX (\d+)'
target = "po_box"
skip_if_filled = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.steps.custom_steps.len(), 1);
        assert_eq!(config.steps.custom_steps[0].label, "custom_po_box_digits");

        let serialized = config.to_toml();
        assert!(serialized.contains("custom_po_box_digits"));
        let parsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.steps.custom_steps.len(), 1);
    }

    #[test]
    fn test_custom_steps_empty_not_serialized() {
        let config = Config::default();
        let toml_str = config.to_toml();
        assert!(!toml_str.contains("custom_steps"));
    }

    #[test]
    fn test_steps_config_is_empty_with_custom_steps() {
        let mut sc = StepsConfig::default();
        assert!(sc.is_empty());
        sc.custom_steps = vec![crate::step::StepDef {
            step_type: "rewrite".to_string(),
            label: "test".to_string(),
            pattern: None, table: None, target: None, replacement: None, source: None,
            skip_if_filled: None, matching_table: None, format_table: None, mode: None,
            targets: None,
        }];
        assert!(!sc.is_empty());
    }
}
