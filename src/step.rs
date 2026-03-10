use fancy_regex::Regex;
use serde::Deserialize;

use crate::address::Field;
use crate::config::OutputFormat;
use crate::ops::{extract_remove, none_if_empty, replace_pattern, squish};
use crate::tables::abbreviations::AbbrTable;
use crate::tables::expand_template;
use crate::tables::Abbreviations;

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
        matching_table: Option<String>,
        format_table: Option<String>,
        pattern: Option<(Regex, String)>,
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

fn standardize_value(
    value: &str,
    matching_table: &AbbrTable,
    canonical_table: &AbbrTable,
    format: OutputFormat,
) -> String {
    let short = matching_table.to_short(value).unwrap_or(value);
    match format {
        OutputFormat::Short => short.to_string(),
        OutputFormat::Long => canonical_table
            .to_long(short)
            .unwrap_or(short)
            .to_string(),
    }
}

pub fn apply_step(
    state: &mut crate::address::AddressState,
    step: &Step,
    tables: &Abbreviations,
    output: &crate::config::OutputConfig,
) {
    if !step.enabled() {
        return;
    }

    match step {
        Step::Validate { pattern, warning, clear, .. } => {
            if pattern.is_match(&state.working).unwrap_or(false) {
                state.fields.warnings.push(warning.clone());
                if *clear {
                    state.working.clear();
                }
            }
        }
        Step::Rewrite { pattern, replacement, rewrite_table, .. } => {
            if !pattern.is_match(&state.working).unwrap_or(false) {
                return;
            }
            if let Some(table_name) = rewrite_table {
                if let Some(table) = tables.get(table_name) {
                    for (short, long) in table.short_to_long_pairs() {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        replace_pattern(&mut state.working, &re, &long);
                    }
                }
            } else if let Some(repl) = replacement {
                replace_pattern(&mut state.working, pattern, repl);
            }
            squish(&mut state.working);
        }
        Step::Extract { pattern, target, skip_if_filled, replacement, .. } => {
            if *skip_if_filled {
                if state.fields.field(*target).is_some() {
                    return;
                }
            }
            if let Some(mut val) = extract_remove(&mut state.working, pattern) {
                if let Some((re, repl)) = replacement {
                    replace_pattern(&mut val, re, repl);
                }
                *state.fields.field_mut(*target) = none_if_empty(val);
            }
        }
        Step::Standardize { target, matching_table, format_table, pattern, mode, .. } => {
            // Handle regex-based standardize (like po_box)
            if let Some((re, repl)) = pattern {
                if let Some(val) = state.fields.field(*target) {
                    let mut result = val.clone();
                    replace_pattern(&mut result, re, repl);
                    *state.fields.field_mut(*target) = none_if_empty(result);
                }
                return;
            }

            // Table-based standardize
            let val = match state.fields.field(*target) {
                Some(v) => v.to_string(),
                None => return,
            };
            let m_name = match matching_table {
                Some(n) => n,
                None => return,
            };
            let f_name = match format_table {
                Some(n) => n,
                None => return,
            };
            let m = match tables.get(m_name) {
                Some(t) => t,
                None => return,
            };
            let c = match tables.get(f_name) {
                Some(t) => t,
                None => return,
            };
            let fmt = output.format_for_field(*target);

            match mode {
                StandardizeMode::WholeField => {
                    *state.fields.field_mut(*target) = Some(standardize_value(&val, m, c, fmt));
                }
                StandardizeMode::PerWord => {
                    let mut result = val.clone();
                    for (short, long) in m.short_to_long_pairs() {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        replace_pattern(&mut result, &re, &long);
                    }
                    *state.fields.field_mut(*target) = none_if_empty(result);
                }
            }
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

/// Compile a single StepDef into a Step, expanding table references in patterns.
pub fn compile_step(def: &StepDef, abbr: &Abbreviations) -> Result<Step, String> {
    match def.step_type.as_str() {
        "validate" => {
            let template = def
                .pattern
                .as_ref()
                .ok_or_else(|| format!("validate step '{}' missing pattern", def.label))?;
            let expanded = expand_template(template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;
            Ok(Step::Validate {
                label: def.label.clone(),
                pattern,
                pattern_template: template.clone(),
                warning: def.warning.clone().unwrap_or_else(|| def.label.clone()),
                clear: def.clear.unwrap_or(false),
                enabled: true,
            })
        }
        "rewrite" => {
            let template = def
                .pattern
                .as_ref()
                .ok_or_else(|| format!("rewrite step '{}' missing pattern", def.label))?;
            let expanded = expand_template(template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;
            Ok(Step::Rewrite {
                label: def.label.clone(),
                pattern,
                pattern_template: template.clone(),
                replacement: def.replacement.clone(),
                rewrite_table: def.table.clone(),
                enabled: true,
            })
        }
        "extract" => {
            let template = if let Some(ref p) = def.pattern {
                p.clone()
            } else if let Some(ref table_name) = def.table {
                let table = abbr.get(table_name).ok_or_else(|| {
                    format!(
                        "extract step '{}' references unknown table '{}'",
                        def.label, table_name
                    )
                })?;
                table
                    .pattern_template
                    .as_ref()
                    .ok_or_else(|| format!("table '{}' has no pattern_template", table_name))?
                    .clone()
            } else {
                return Err(format!(
                    "extract step '{}' needs either pattern or table",
                    def.label
                ));
            };

            let expanded = expand_template(&template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;

            let target = def
                .target
                .as_ref()
                .ok_or_else(|| format!("extract step '{}' missing target", def.label))?;

            let replacement = if let Some(ref r) = def.replacement {
                let expanded_r = expand_template(r, abbr);
                // The extract pattern serves as the match regex for replacement;
                // the replacement field is the substitution template.
                Some((
                    Regex::new(&expanded).map_err(|e| {
                        format!("Bad replacement regex in step '{}': {}", def.label, e)
                    })?,
                    expanded_r,
                ))
            } else {
                None
            };

            Ok(Step::Extract {
                label: def.label.clone(),
                pattern,
                pattern_template: template,
                target: parse_field(target),
                skip_if_filled: def.skip_if_filled.unwrap_or(false),
                replacement,
                enabled: true,
            })
        }
        "standardize" => {
            let target = def
                .target
                .as_ref()
                .ok_or_else(|| format!("standardize step '{}' missing target", def.label))?;
            let mode = match def.mode.as_deref() {
                Some("per_word") => StandardizeMode::PerWord,
                _ => StandardizeMode::WholeField,
            };

            // Regex-based standardize: has pattern+replacement instead of tables
            let pattern = if let Some(ref p) = def.pattern {
                let expanded = expand_template(p, abbr);
                let re = Regex::new(&expanded)
                    .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;
                let repl = def
                    .replacement
                    .clone()
                    .unwrap_or_default();
                Some((re, repl))
            } else {
                None
            };

            // Table-based standardize requires both tables
            if pattern.is_none() {
                if def.matching_table.is_none() || def.format_table.is_none() {
                    return Err(format!(
                        "standardize step '{}' needs either pattern+replacement or matching_table+format_table",
                        def.label
                    ));
                }
            }

            Ok(Step::Standardize {
                label: def.label.clone(),
                target: parse_field(target),
                matching_table: def.matching_table.clone(),
                format_table: def.format_table.clone(),
                pattern,
                mode,
                enabled: true,
            })
        }
        other => Err(format!(
            "Unknown step type '{}' in step '{}'",
            other, def.label
        )),
    }
}

/// Compile all step definitions into executable Steps.
pub fn compile_steps(defs: &[StepDef], abbr: &Abbreviations) -> Vec<Step> {
    defs.iter()
        .map(|d| compile_step(d, abbr).unwrap_or_else(|e| panic!("{}", e)))
        .collect()
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
            matching_table: Some("suffix_all".to_string()),
            format_table: Some("suffix_usps".to_string()),
            pattern: None,
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

    #[test]
    fn test_compile_rewrite_step() {
        use crate::tables::abbreviations::build_default_tables;
        let def = StepDef {
            step_type: "rewrite".to_string(),
            label: "test_rewrite".to_string(),
            pattern: Some(r"\b({direction})\b".to_string()),
            replacement: Some("$1".to_string()),
            table: None,
            target: None,
            warning: None,
            clear: None,
            skip_if_filled: None,
            matching_table: None,
            format_table: None,
            mode: None,
        };
        let abbr = build_default_tables();
        let step = compile_step(&def, &abbr).unwrap();
        assert_eq!(step.label(), "test_rewrite");
        assert_eq!(step.step_type(), "rewrite");
        if let Step::Rewrite {
            pattern_template, ..
        } = &step
        {
            assert!(pattern_template.contains("{direction}"));
        }
    }

    #[test]
    fn test_compile_extract_step() {
        use crate::tables::abbreviations::build_default_tables;
        let def = StepDef {
            step_type: "extract".to_string(),
            label: "test_suffix".to_string(),
            pattern: Some(r"(?<!^)\b({suffix_common})\s*$".to_string()),
            replacement: None,
            table: None,
            target: Some("suffix".to_string()),
            warning: None,
            clear: None,
            skip_if_filled: Some(true),
            matching_table: None,
            format_table: None,
            mode: None,
        };
        let abbr = build_default_tables();
        let step = compile_step(&def, &abbr).unwrap();
        if let Step::Extract {
            target,
            skip_if_filled,
            ..
        } = &step
        {
            assert_eq!(*target, Field::Suffix);
            assert!(*skip_if_filled);
        } else {
            panic!("Expected Extract step");
        }
    }

    #[test]
    fn test_compile_standardize_step() {
        use crate::tables::abbreviations::build_default_tables;
        let def = StepDef {
            step_type: "standardize".to_string(),
            label: "std_suffix".to_string(),
            pattern: None,
            replacement: None,
            table: None,
            target: Some("suffix".to_string()),
            warning: None,
            clear: None,
            skip_if_filled: None,
            matching_table: Some("suffix_all".to_string()),
            format_table: Some("suffix_usps".to_string()),
            mode: None,
        };
        let abbr = build_default_tables();
        let step = compile_step(&def, &abbr).unwrap();
        if let Step::Standardize {
            target,
            matching_table,
            format_table,
            mode,
            ..
        } = &step
        {
            assert_eq!(*target, Field::Suffix);
            assert_eq!(matching_table.as_deref(), Some("suffix_all"));
            assert_eq!(format_table.as_deref(), Some("suffix_usps"));
            assert_eq!(*mode, StandardizeMode::WholeField);
        } else {
            panic!("Expected Standardize step");
        }
    }

    #[test]
    fn test_compile_all_default_steps() {
        use crate::tables::abbreviations::build_default_tables;
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let abbr = build_default_tables();
        let steps = compile_steps(&defs.step, &abbr);
        assert!(steps.len() > 20);
        assert_eq!(steps[0].step_type(), "validate");
        assert_eq!(steps[0].label(), "na_check");
    }

    #[test]
    fn test_apply_validate_step() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let toml_str = r#"
[[step]]
type = "validate"
label = "na_check"
pattern = '(?i)^(N/?A)$'
warning = "na_address"
clear = true
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let steps = compile_steps(&defs.step, &abbr);
        let mut state = AddressState::new_from_prepared("N/A".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &steps[0], &abbr, &output);
        assert!(state.fields.warnings.contains(&"na_address".to_string()));
        assert!(state.working.is_empty());
    }

    #[test]
    fn test_apply_rewrite_step() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let def = StepDef {
            step_type: "rewrite".to_string(),
            label: "test_rewrite".to_string(),
            pattern: Some(r"STAPT".to_string()),
            replacement: Some("ST APT".to_string()),
            table: None, target: None, warning: None, clear: None,
            skip_if_filled: None, matching_table: None, format_table: None, mode: None,
        };
        let step = compile_step(&def, &abbr).unwrap();
        let mut state = AddressState::new_from_prepared("123 N STAPT 4B".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.working, "123 N ST APT 4B");
    }

    #[test]
    fn test_apply_rewrite_from_table() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let toml_str = r#"
[[step]]
type = "rewrite"
label = "street_name_abbr"
pattern = '\b({street_name_abbr$short})\b'
table = "street_name_abbr"
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let steps = compile_steps(&defs.step, &abbr);
        let mut state = AddressState::new_from_prepared("MT VERNON".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &steps[0], &abbr, &output);
        assert_eq!(state.working, "MOUNT VERNON");
    }

    #[test]
    fn test_apply_extract_step() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let def = StepDef {
            step_type: "extract".to_string(),
            label: "test_number".to_string(),
            pattern: Some(r"^\d+\b".to_string()),
            replacement: None,
            table: None, target: Some("street_number".to_string()),
            warning: None, clear: None,
            skip_if_filled: Some(true),
            matching_table: None, format_table: None, mode: None,
        };
        let step = compile_step(&def, &abbr).unwrap();
        let mut state = AddressState::new_from_prepared("123 MAIN ST".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.fields.street_number.as_deref(), Some("123"));
        assert_eq!(state.working, "MAIN ST");
    }

    #[test]
    fn test_apply_standardize_step() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let def = StepDef {
            step_type: "standardize".to_string(),
            label: "std_suffix".to_string(),
            pattern: None, replacement: None, table: None,
            target: Some("suffix".to_string()),
            warning: None, clear: None, skip_if_filled: None,
            matching_table: Some("suffix_all".to_string()),
            format_table: Some("suffix_usps".to_string()),
            mode: None,
        };
        let step = compile_step(&def, &abbr).unwrap();
        let mut state = AddressState::new_from_prepared(String::new());
        state.fields.suffix = Some("ST".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.fields.suffix.as_deref(), Some("STREET"));
    }
}
