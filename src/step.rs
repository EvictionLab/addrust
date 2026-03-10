use fancy_regex::Regex;
use serde::Deserialize;

use crate::address::Field;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardizeMode {
    WholeField,
    PerWord,
}

#[derive(Debug)]
pub enum Step {
    Validate {
        label: String,
        pattern: Regex,
        pattern_template: String,
        warning: String,
        clear: bool,
        enabled: bool,
    },
    Rewrite {
        label: String,
        pattern: Regex,
        pattern_template: String,
        replacement: Option<String>,
        rewrite_table: Option<String>,
        enabled: bool,
    },
    Extract {
        label: String,
        pattern: Regex,
        pattern_template: String,
        target: Field,
        skip_if_filled: bool,
        replacement: Option<(Regex, String)>,
        enabled: bool,
    },
    Standardize {
        label: String,
        target: Field,
        matching_table: String,
        format_table: String,
        mode: StandardizeMode,
        enabled: bool,
    },
}

impl Step {
    pub fn label(&self) -> &str {
        match self {
            Step::Validate { label, .. } => label,
            Step::Rewrite { label, .. } => label,
            Step::Extract { label, .. } => label,
            Step::Standardize { label, .. } => label,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Step::Validate { enabled, .. } => *enabled,
            Step::Rewrite { enabled, .. } => *enabled,
            Step::Extract { enabled, .. } => *enabled,
            Step::Standardize { enabled, .. } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, value: bool) {
        match self {
            Step::Validate { enabled, .. } => *enabled = value,
            Step::Rewrite { enabled, .. } => *enabled = value,
            Step::Extract { enabled, .. } => *enabled = value,
            Step::Standardize { enabled, .. } => *enabled = value,
        }
    }

    pub fn pattern_template(&self) -> Option<&str> {
        match self {
            Step::Validate {
                pattern_template, ..
            } => Some(pattern_template),
            Step::Rewrite {
                pattern_template, ..
            } => Some(pattern_template),
            Step::Extract {
                pattern_template, ..
            } => Some(pattern_template),
            Step::Standardize { .. } => None,
        }
    }

    pub fn step_type(&self) -> &'static str {
        match self {
            Step::Validate { .. } => "validate",
            Step::Rewrite { .. } => "rewrite",
            Step::Extract { .. } => "extract",
            Step::Standardize { .. } => "standardize",
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct StepDef {
    #[serde(rename = "type")]
    pub step_type: String,
    pub label: String,
    pub pattern: Option<String>,
    pub table: Option<String>,
    pub target: Option<String>,
    pub replacement: Option<String>,
    pub warning: Option<String>,
    pub clear: Option<bool>,
    pub skip_if_filled: Option<bool>,
    pub matching_table: Option<String>,
    pub format_table: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StepsDef {
    pub step: Vec<StepDef>,
}

fn parse_field(name: &str) -> Field {
    match name {
        "street_number" => Field::StreetNumber,
        "pre_direction" => Field::PreDirection,
        "street_name" => Field::StreetName,
        "suffix" => Field::Suffix,
        "post_direction" => Field::PostDirection,
        "unit" => Field::Unit,
        "unit_type" => Field::UnitType,
        "po_box" => Field::PoBox,
        "building" => Field::Building,
        "extra_front" => Field::ExtraFront,
        "extra_back" => Field::ExtraBack,
        _ => panic!("Unknown field name: {}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_def_deserialize_extract() {
        let toml_str = r#"
[[step]]
type = "extract"
label = "Extract suffix"
pattern = '(?i)\b(ST|AVE|BLVD)\b'
target = "suffix"
skip_if_filled = true
"#;
        let steps: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(steps.step.len(), 1);
        let s = &steps.step[0];
        assert_eq!(s.step_type, "extract");
        assert_eq!(s.label, "Extract suffix");
        assert_eq!(s.target.as_deref(), Some("suffix"));
        assert_eq!(s.skip_if_filled, Some(true));
        assert!(s.pattern.is_some());
    }

    #[test]
    fn test_step_def_deserialize_rewrite() {
        let toml_str = r#"
[[step]]
type = "rewrite"
label = "Normalize hyphens"
pattern = '--+'
replacement = "-"
"#;
        let steps: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(steps.step.len(), 1);
        let s = &steps.step[0];
        assert_eq!(s.step_type, "rewrite");
        assert_eq!(s.label, "Normalize hyphens");
        assert_eq!(s.replacement.as_deref(), Some("-"));
    }

    #[test]
    fn test_step_def_deserialize_validate() {
        let toml_str = r#"
[[step]]
type = "validate"
label = "Check for digits"
pattern = '\d'
warning = "No digits found"
clear = true
"#;
        let steps: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(steps.step.len(), 1);
        let s = &steps.step[0];
        assert_eq!(s.step_type, "validate");
        assert_eq!(s.warning.as_deref(), Some("No digits found"));
        assert_eq!(s.clear, Some(true));
    }

    #[test]
    fn test_step_def_deserialize_standardize() {
        let toml_str = r#"
[[step]]
type = "standardize"
label = "Standardize suffix"
target = "suffix"
matching_table = "suffix_all"
format_table = "suffix_usps"
mode = "whole_field"
"#;
        let steps: StepsDef = toml::from_str(toml_str).unwrap();
        assert_eq!(steps.step.len(), 1);
        let s = &steps.step[0];
        assert_eq!(s.step_type, "standardize");
        assert_eq!(s.target.as_deref(), Some("suffix"));
        assert_eq!(s.matching_table.as_deref(), Some("suffix_all"));
        assert_eq!(s.format_table.as_deref(), Some("suffix_usps"));
        assert_eq!(s.mode.as_deref(), Some("whole_field"));
    }

    #[test]
    fn test_parse_field() {
        assert_eq!(parse_field("street_number"), Field::StreetNumber);
        assert_eq!(parse_field("pre_direction"), Field::PreDirection);
        assert_eq!(parse_field("street_name"), Field::StreetName);
        assert_eq!(parse_field("suffix"), Field::Suffix);
        assert_eq!(parse_field("post_direction"), Field::PostDirection);
        assert_eq!(parse_field("unit"), Field::Unit);
        assert_eq!(parse_field("unit_type"), Field::UnitType);
        assert_eq!(parse_field("po_box"), Field::PoBox);
        assert_eq!(parse_field("building"), Field::Building);
        assert_eq!(parse_field("extra_front"), Field::ExtraFront);
        assert_eq!(parse_field("extra_back"), Field::ExtraBack);
    }

    #[test]
    #[should_panic(expected = "Unknown field name: bogus")]
    fn test_parse_field_unknown() {
        parse_field("bogus");
    }

    #[test]
    fn test_step_accessors() {
        let step = Step::Rewrite {
            label: "test".to_string(),
            pattern: Regex::new("x").unwrap(),
            pattern_template: "x".to_string(),
            replacement: Some("y".to_string()),
            rewrite_table: None,
            enabled: true,
        };
        assert_eq!(step.label(), "test");
        assert_eq!(step.step_type(), "rewrite");
        assert!(step.enabled());
        assert_eq!(step.pattern_template(), Some("x"));

        // Test Standardize returns None for pattern_template
        let std_step = Step::Standardize {
            label: "std".to_string(),
            target: Field::Suffix,
            matching_table: "suffix_all".to_string(),
            format_table: "suffix_usps".to_string(),
            mode: StandardizeMode::WholeField,
            enabled: false,
        };
        assert_eq!(std_step.label(), "std");
        assert_eq!(std_step.step_type(), "standardize");
        assert!(!std_step.enabled());
        assert_eq!(std_step.pattern_template(), None);
    }

    #[test]
    fn test_default_steps_toml_parses() {
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert!(defs.step.len() > 20, "Expected 20+ steps, got {}", defs.step.len());
        assert_eq!(defs.step[0].step_type, "validate");
        assert_eq!(defs.step[0].label, "na_check");
        let last = defs.step.last().unwrap();
        assert_eq!(last.step_type, "standardize");
    }

    #[test]
    fn test_step_set_enabled() {
        let mut step = Step::Validate {
            label: "check".to_string(),
            pattern: Regex::new(".").unwrap(),
            pattern_template: ".".to_string(),
            warning: "warn".to_string(),
            clear: false,
            enabled: true,
        };
        assert!(step.enabled());
        step.set_enabled(false);
        assert!(!step.enabled());
    }
}
