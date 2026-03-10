use fancy_regex::Regex;

use crate::address::{Address, AddressState};
use crate::ops::squish;
use crate::prepare;
use crate::tables::abbreviations::Abbreviations;

/// Summary of a step for display purposes.
#[derive(Debug)]
pub struct StepSummary {
    pub label: String,
    pub step_type: String,
    pub pattern_template: Option<String>,
    pub enabled: bool,
}

/// The parsing pipeline — an ordered sequence of steps.
pub struct Pipeline {
    steps: Vec<crate::step::Step>,
    output: crate::config::OutputConfig,
    tables: Abbreviations,
}

impl Pipeline {
    /// Build pipeline from a Config (file-based configuration).
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self::from_steps_config(config)
    }

    /// Build a step-based pipeline from a Config (file-based configuration).
    pub fn from_steps_config(config: &crate::config::Config) -> Self {
        use crate::step::{compile_steps, StepsDef};
        use crate::tables::abbreviations::build_default_tables;

        let tables = build_default_tables();
        let tables = if config.dictionaries.is_empty() {
            tables
        } else {
            tables.patch(&config.dictionaries)
        };

        let toml_str = include_str!("../data/defaults/steps.toml");
        let mut defs: StepsDef = toml::from_str(toml_str)
            .expect("Failed to parse default steps.toml");

        // Apply pattern overrides from config
        for def in &mut defs.step {
            if let Some(override_pattern) = config.steps.pattern_overrides.get(&def.label) {
                def.pattern = Some(override_pattern.clone());
            }
        }

        let mut steps = compile_steps(&defs.step, &tables);

        // Apply disabled list
        for step in &mut steps {
            if config.steps.disabled.contains(&step.label().to_string()) {
                step.set_enabled(false);
            }
        }

        Self {
            steps,
            output: config.output.clone(),
            tables,
        }
    }

    /// Build a pipeline using the step-based parse path with default tables and steps.
    pub fn from_steps_default() -> Self {
        use crate::tables::abbreviations::build_default_tables;
        use crate::step::{compile_steps, StepsDef};

        let tables = build_default_tables();
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str)
            .expect("Failed to parse default steps.toml");
        let steps = compile_steps(&defs.step, &tables);

        Self {
            steps,
            output: crate::config::OutputConfig::default(),
            tables,
        }
    }

    /// Get metadata about all steps for display purposes.
    pub fn step_summaries(&self) -> Vec<StepSummary> {
        self.steps.iter().map(|s| StepSummary {
            label: s.label().to_string(),
            step_type: s.step_type().to_string(),
            pattern_template: s.pattern_template().map(|p| p.to_string()),
            enabled: s.enabled(),
        }).collect()
    }

    /// Parse a single address string.
    pub fn parse(&self, input: &str) -> Address {
        // Prepare: uppercase, clean punctuation
        let prepared = match prepare::prepare(input) {
            Some(s) => {
                #[cfg(test)]
                eprintln!("[PREPARE] {:?} → {:?}", input, s);
                s
            }
            None => {
                let mut addr = Address::default();
                addr.warnings.push("na_address".to_string());
                return addr;
            }
        };

        let mut state = AddressState::new_from_prepared(prepared);

        for step in &self.steps {
            crate::step::apply_step(&mut state, step, &self.tables, &self.output);
        }

        // Finalize: whatever remains in working string becomes street_name
        self.finalize(&mut state);

        state.fields
    }

    /// Parse a batch of addresses (parallel with rayon).
    pub fn parse_batch(&self, inputs: &[&str]) -> Vec<Address> {
        use rayon::prelude::*;
        inputs.par_iter().map(|input| self.parse(input)).collect()
    }

    /// After all steps, assign remaining working string to street_name
    /// and perform final cleanup.
    fn finalize(&self, state: &mut AddressState) {
        // Remove any leftover placeholder tags
        let re_tags = Regex::new(r"<[a-z0-9_]+>").unwrap();
        let remaining = re_tags.replace_all(&state.working, "").to_string();
        let mut remaining = remaining.trim().to_string();
        squish(&mut remaining);

        if state.fields.street_name.is_none() && !remaining.is_empty() {
            state.fields.street_name = Some(remaining);
        }

        // Clean up unit: strip leading # and whitespace
        if let Some(ref u) = state.fields.unit {
            let cleaned = u.trim_start_matches('#').trim().to_string();
            state.fields.unit = if cleaned.is_empty() { None } else { Some(cleaned) };
        }

        // If no street number but unit exists, promote unit to street number
        if state.fields.street_number.is_none() && state.fields.unit.is_some() {
            state.fields.street_number = state.fields.unit.take();
        }

        // Strip leading zeros from street number
        if let Some(ref num) = state.fields.street_number {
            let stripped = num.trim_start_matches('0');
            if !stripped.is_empty() && stripped != num {
                state.fields.street_number = Some(stripped.to_string());
            }
        }
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::from_steps_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_default_parses() {
        let p = Pipeline::default();
        let addr = p.parse("123 N Main St Apt 4B");
        assert_eq!(addr.street_number.as_deref(), Some("123"));
        assert_eq!(addr.pre_direction.as_deref(), Some("N"));
        assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
        assert_eq!(addr.suffix.as_deref(), Some("STREET"));
        assert!(addr.unit.is_some());
        assert!(addr.unit.as_deref().unwrap().contains("4B"));
    }

    #[test]
    fn test_config_disabled_steps() {
        let toml_str = r#"
[steps]
disabled = ["suffix_common", "suffix_all"]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 Main St");
        assert!(addr.suffix.is_none());
    }

    #[test]
    fn test_config_pattern_override() {
        let toml_str = r#"
[steps.pattern_overrides]
unit_type_value = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+)\s*$'
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 Main St B");
        assert!(addr.unit.is_none() || addr.unit.as_deref() != Some("B"));
    }

    #[test]
    fn test_config_dict_override() {
        let toml_str = r#"
[dictionaries.suffix_all]
add = [{ short = "PSGE", long = "PASSAGE" }]

[dictionaries.suffix_common]
add = [{ short = "PSGE", long = "PASSAGE" }]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 Main Psge");
        assert_eq!(addr.suffix.as_deref(), Some("PASSAGE"));
    }

    #[test]
    fn test_output_format_short() {
        let mut config = crate::config::Config::default();
        config.output.suffix = crate::config::OutputFormat::Short;
        config.output.direction = crate::config::OutputFormat::Long;
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 N Main Drive");
        assert_eq!(addr.suffix.as_deref(), Some("DR"));
        assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
    }

    #[test]
    fn test_po_box_variants() {
        let p = Pipeline::default();
        for (input, expected) in [
            ("PO BOX 123", "PO BOX 123"),
            ("P O BOX 123", "PO BOX 123"),
            ("P.O. BOX 123", "PO BOX 123"),
            ("POBOX 123", "PO BOX 123"),
            ("PO BOX A", "PO BOX A"),
        ] {
            let addr = p.parse(input);
            assert_eq!(addr.po_box.as_deref(), Some(expected), "input: {}", input);
        }
    }

    #[test]
    fn test_st_to_saint() {
        let p = Pipeline::default();
        let addr = p.parse("42 W St James Pl");
        assert_eq!(addr.street_name.as_deref(), Some("SAINT JAMES"));
        assert_eq!(addr.suffix.as_deref(), Some("PLACE"));
    }

    #[test]
    fn test_step_summaries() {
        let p = Pipeline::default();
        let summaries = p.step_summaries();
        assert!(!summaries.is_empty());
        assert_eq!(summaries[0].step_type, "validate");
        assert_eq!(summaries[0].label, "na_check");
    }
}
