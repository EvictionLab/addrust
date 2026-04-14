use fancy_regex::Regex;

use crate::address::{Address, AddressState};
use crate::ops::squish;
use crate::prepare;
use crate::tables::abbreviations::Abbreviations;

/// The parsing pipeline — an ordered sequence of steps.
pub struct Pipeline {
    steps: Vec<crate::step::Step>,
    output: crate::config::OutputConfig,
    tables: Abbreviations,
}

impl Pipeline {
    /// Build pipeline from a Config (file-based configuration).
    pub fn from_config(config: &crate::config::Config) -> Self {
        use crate::step::{compile_step, compile_steps, StepsDef};
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

        // Apply step_overrides from config, tracking label renames
        let mut label_renames: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for def in &mut defs.step {
            if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
                let old_label = def.label.clone();
                step_override.apply_to(def);
                if def.label != old_label {
                    label_renames.insert(old_label, def.label.clone());
                }
            }
        }

        let mut steps = compile_steps(&defs.step, &tables);

        // Compile and append custom steps (with step_overrides applied)
        for custom_def in &config.steps.custom_steps {
            let mut def = custom_def.clone();
            if let Some(step_override) = config.steps.step_overrides.get(&def.label) {
                let old_label = def.label.clone();
                step_override.apply_to(&mut def);
                if def.label != old_label {
                    label_renames.insert(old_label, def.label.clone());
                }
            }
            match compile_step(&def, &tables) {
                Ok(step) => steps.push(step),
                Err(e) => eprintln!("Warning: skipping invalid custom step '{}': {}", def.label, e),
            }
        }

        // Apply step_order reordering (resolving any label renames from overrides)
        if !config.steps.step_order.is_empty() {
            let order: Vec<String> = config.steps.step_order.iter()
                .map(|l| label_renames.get(l).cloned().unwrap_or_else(|| l.clone()))
                .collect();
            // Build position map: label -> index in step_order
            let pos_map: std::collections::HashMap<&str, usize> = order
                .iter()
                .enumerate()
                .map(|(i, label)| (label.as_str(), i))
                .collect();

            // Partition into ordered (in step_order) and unordered (not in step_order)
            let mut ordered: Vec<(usize, crate::step::Step)> = Vec::new();
            let mut unordered: Vec<crate::step::Step> = Vec::new();
            for step in steps {
                if let Some(&pos) = pos_map.get(step.label()) {
                    ordered.push((pos, step));
                } else {
                    unordered.push(step);
                }
            }
            ordered.sort_by_key(|(pos, _)| *pos);
            steps = ordered.into_iter().map(|(_, s)| s).collect();
            steps.extend(unordered);
        }

        // Apply disabled list (resolving any label renames from overrides)
        let disabled: Vec<String> = config.steps.disabled.iter()
            .map(|l| label_renames.get(l).cloned().unwrap_or_else(|| l.clone()))
            .collect();
        for step in &mut steps {
            if disabled.contains(&step.label().to_string()) {
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

    /// Access the compiled steps.
    pub fn steps(&self) -> &[crate::step::Step] {
        &self.steps
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

    /// After all steps, assign remaining working string to street_name.
    fn finalize(&self, state: &mut AddressState) {
        // Remove any leftover placeholder tags
        let re_tags = Regex::new(r"<[a-z0-9_]+>").unwrap();
        let remaining = re_tags.replace_all(&state.working, "").to_string();
        let mut remaining = remaining.trim().to_string();
        squish(&mut remaining);

        if state.fields.street_name.is_none() && !remaining.is_empty() {
            state.fields.street_name = Some(remaining);
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
[steps.step_overrides.unit_type_value]
pattern = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+)\s*$'
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
        let steps = p.steps();
        assert!(!steps.is_empty());
        assert_eq!(steps[0].step_type(), "rewrite");
        assert_eq!(steps[0].label(), "fix_ampersand");
    }

    #[test]
    fn test_config_step_order() {
        let toml_str = r#"
[steps]
step_order = ["pre_direction", "suffix_common", "na_check"]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let steps = p.steps();
        assert_eq!(steps[0].label(), "pre_direction");
        assert_eq!(steps[1].label(), "suffix_common");
        assert_eq!(steps[2].label(), "na_check");
    }

    #[test]
    fn test_config_step_order_unknown_labels_ignored() {
        let toml_str = r#"
[steps]
step_order = ["nonexistent", "na_check", "po_box"]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let steps = p.steps();
        assert_eq!(steps[0].label(), "na_check");
        assert_eq!(steps[1].label(), "po_box");
    }

    #[test]
    fn test_config_step_order_missing_labels_appended() {
        let toml_str = r#"
[steps]
step_order = ["suffix_common", "na_check"]
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let steps = p.steps();
        assert_eq!(steps[0].label(), "suffix_common");
        assert_eq!(steps[1].label(), "na_check");
        assert_eq!(steps[2].label(), "fix_ampersand");
        let labels: Vec<&str> = steps.iter().map(|s| s.label()).collect();
        let csz_pos = labels.iter().position(|l| *l == "city_state_zip").unwrap();
        let po_box_pos = labels.iter().position(|l| *l == "po_box").unwrap();
        assert!(csz_pos < po_box_pos);
    }

    #[test]
    fn test_step_order_with_disabled_and_overrides() {
        let toml_str = r#"
[steps]
disabled = ["na_check"]
step_order = ["po_box", "na_check", "city_state_zip"]

[steps.step_overrides.po_box]
pattern = '(?i)P\.?\s*O\.?\s*BOX\s+(\w+)'
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let steps = p.steps();
        assert_eq!(steps[0].label(), "po_box");
        assert_eq!(steps[1].label(), "na_check");
        assert!(!steps[1].enabled()); // na_check is disabled
        assert_eq!(steps[2].label(), "city_state_zip");
    }

    #[test]
    fn test_label_rename_updates_step_order() {
        // Override renames unit_fraction -> street_number_unit_fraction.
        // step_order uses the OLD label — code should resolve the rename.
        let toml_str = r#"
[steps]
step_order = ["unit_fraction", "street_number"]

[steps.step_overrides.unit_fraction]
label = "street_number_unit_fraction"
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let steps = p.steps();
        assert_eq!(steps[0].label(), "street_number_unit_fraction");
        assert_eq!(steps[1].label(), "street_number");
    }

    #[test]
    fn test_label_rename_updates_disabled() {
        // Override renames na_check -> na_check_v2.
        // disabled list uses the OLD label — code should resolve the rename.
        let toml_str = r#"
[steps]
disabled = ["na_check"]

[steps.step_overrides.na_check]
label = "na_check_v2"
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        let steps = p.steps();
        let na_step = steps.iter().find(|s| s.label() == "na_check_v2").unwrap();
        assert!(!na_step.enabled());
    }
}
