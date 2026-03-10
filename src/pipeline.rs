use fancy_regex::Regex;

use crate::address::{Address, AddressState, Field};
use crate::config::OutputFormat;
use crate::ops::{extract_remove, extract_replace, none_if_empty, replace_pattern, squish};
use crate::prepare;
use crate::tables::abbreviations::{AbbrTable, Abbreviations};

/// What a rule does when it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Extract into target field, remove from working string.
    Extract,
    /// Extract into target field, leave `<label>` placeholder in working string.
    Replace,
    /// Modify the working string in place (no extraction).
    Change,
    /// Add label to warnings, don't modify.
    Warn,
}

/// A single pipeline rule — one row of the data dictionary.
#[derive(Debug)]
pub struct Rule {
    pub label: String,
    pub group: String,
    pub pattern: Regex,
    /// Human-readable pattern with table placeholders (e.g., `{suffix_common}` instead of expanded alternation).
    pub pattern_template: String,
    pub action: Action,
    pub target: Option<Field>,
    /// Regex replacement for standardization (applied to extracted value or working string).
    pub standardize: Option<(Regex, String)>,
    /// Table-driven standardization: pairs of (match_regex, replacement_string).
    /// Used when a single rule needs to replace multiple different matched values.
    pub standardize_pairs: Vec<(Regex, String)>,
    /// If true, skip this rule when the target field is already filled.
    pub skip_if_filled: bool,
    pub enabled: bool,
}

/// Apply a single rule to an address state.
fn apply_rule(state: &mut AddressState, rule: &Rule) {
    if !rule.enabled {
        return;
    }

    // Skip if target already filled (the reduce2 early-exit)
    if rule.skip_if_filled {
        if let Some(field) = rule.target {
            if state.fields.field(field).is_some() {
                return;
            }
        }
    }

    // Check if pattern matches
    if !rule.pattern.is_match(&state.working).unwrap_or(false) {
        return;
    }

    match rule.action {
        Action::Extract => {
            let extracted = extract_remove(&mut state.working, &rule.pattern);
            if let (Some(mut val), Some(field)) = (extracted, rule.target) {
                if let Some((ref match_re, ref replacement)) = rule.standardize {
                    replace_pattern(&mut val, match_re, replacement);
                }
                #[cfg(test)]
                eprintln!("[EXTRACT {}] {:?} → field {:?}, remaining {:?}", rule.label, val, field, state.working);
                *state.fields.field_mut(field) = none_if_empty(val);
            }
        }
        Action::Replace => {
            let extracted = extract_replace(&mut state.working, &rule.pattern, &rule.label);
            if let (Some(mut val), Some(field)) = (extracted, rule.target) {
                if let Some((ref match_re, ref replacement)) = rule.standardize {
                    replace_pattern(&mut val, match_re, replacement);
                }
                *state.fields.field_mut(field) = none_if_empty(val);
            }
        }
        Action::Change => {
            if !rule.standardize_pairs.is_empty() {
                #[cfg(test)]
                let before = state.working.clone();
                for (match_re, replacement) in &rule.standardize_pairs {
                    replace_pattern(&mut state.working, match_re, replacement);
                }
                squish(&mut state.working);
                #[cfg(test)]
                if before != state.working {
                    eprintln!("[CHANGE {}] {:?} → {:?}", rule.label, before, state.working);
                }
            } else if let Some((ref match_re, ref replacement)) = rule.standardize {
                #[cfg(test)]
                let before = state.working.clone();
                replace_pattern(&mut state.working, match_re, replacement);
                squish(&mut state.working);
                #[cfg(test)]
                if before != state.working {
                    eprintln!("[CHANGE {}] {:?} → {:?}", rule.label, before, state.working);
                }
            }
        }
        Action::Warn => {
            state.fields.warnings.push(rule.label.clone());
            // For NA-type warnings, clear the working string
            if rule.label.contains("na") {
                state.working.clear();
            }
        }
    }
}

/// Configuration for which rules are enabled.
#[derive(Debug, Clone, Default)]
pub struct PipelineConfig {
    /// Disable specific rules by label.
    pub disabled_rules: Vec<String>,
    /// Disable entire groups.
    pub disabled_groups: Vec<String>,
}

/// Summary of a rule for display purposes.
#[derive(Debug)]
pub struct RuleSummary {
    pub label: String,
    pub group: String,
    pub action: Action,
    pub pattern_template: String,
    pub enabled: bool,
}

/// Standardize a value using the two-step canonicalize→format flow.
///
/// 1. Canonicalize: look up the value in `matching_table` to get the short form.
/// 2. Format: based on preference, keep short or expand to long via `canonical_table`.
fn standardize_value(
    value: &str,
    matching_table: &AbbrTable,
    canonical_table: &AbbrTable,
    format: OutputFormat,
) -> String {
    // Step 1: Canonicalize to short form
    let short = matching_table.to_short(value).unwrap_or(value);

    // Step 2: Format based on preference
    match format {
        OutputFormat::Short => short.to_string(),
        OutputFormat::Long => canonical_table
            .to_long(short)
            .unwrap_or(short)
            .to_string(),
    }
}

/// The parsing pipeline — an ordered sequence of rules.
pub struct Pipeline {
    rules: Vec<Rule>,
    output: crate::config::OutputConfig,
    tables: Abbreviations,
}

impl Pipeline {
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

        let rules = build_rules(&tables, &config.rules.pattern_overrides);

        let pipeline_config = PipelineConfig {
            disabled_rules: config.rules.disabled.clone(),
            disabled_groups: config.rules.disabled_groups.clone(),
        };

        let mut pipeline = Self::new(rules, &pipeline_config);
        pipeline.output = config.output.clone();
        pipeline.tables = tables;
        pipeline
    }

    /// Get metadata about all rules for display purposes.
    pub fn rule_summaries(&self) -> Vec<RuleSummary> {
        self.rules.iter().map(|r| RuleSummary {
            label: r.label.clone(),
            group: r.group.clone(),
            action: r.action,
            pattern_template: r.pattern_template.clone(),
            enabled: r.enabled,
        }).collect()
    }

    pub fn new(mut rules: Vec<Rule>, config: &PipelineConfig) -> Self {
        use crate::tables::abbreviations::ABBR;

        // Apply config: disable rules by label or group
        for rule in &mut rules {
            if config.disabled_rules.contains(&rule.label)
                || config.disabled_groups.contains(&rule.group)
            {
                rule.enabled = false;
            }
        }
        Self {
            rules,
            output: crate::config::OutputConfig::default(),
            tables: ABBR.clone(),
        }
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

        // Apply all rules in order
        for rule in &self.rules {
            apply_rule(&mut state, rule);
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

    /// After all rules, assign remaining working string to street_name.
    fn finalize(&self, state: &mut AddressState) {
        // Remove any leftover placeholder tags
        let re_tags = Regex::new(r"<[a-z0-9_]+>").unwrap();
        let remaining = re_tags.replace_all(&state.working, "").to_string();
        let mut remaining = remaining.trim().to_string();
        squish(&mut remaining);

        if state.fields.street_name.is_none() && !remaining.is_empty() {
            state.fields.street_name = Some(remaining);
        }

        // Standardize directions
        if let Some(ref dir) = state.fields.pre_direction {
            if let Some(table) = self.tables.get("direction") {
                state.fields.pre_direction = Some(
                    standardize_value(dir, table, table, self.output.direction)
                );
            }
        }
        if let Some(ref dir) = state.fields.post_direction {
            if let Some(table) = self.tables.get("direction") {
                state.fields.post_direction = Some(
                    standardize_value(dir, table, table, self.output.direction)
                );
            }
        }

        // Standardize suffix: canonicalize via suffix_all, format via suffix_usps
        if let Some(ref sfx) = state.fields.suffix {
            let matching = self.tables.get("suffix_all");
            let canonical = self.tables.get("suffix_usps");
            if let (Some(m), Some(c)) = (matching, canonical) {
                state.fields.suffix = Some(
                    standardize_value(sfx, m, c, self.output.suffix)
                );
            }
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
        use crate::tables::abbreviations::ABBR;
        use crate::tables::build_rules;

        let rules = build_rules(&ABBR, &std::collections::HashMap::new());
        Self {
            rules,
            output: crate::config::OutputConfig::default(),
            tables: ABBR.clone(),
        }
    }
}

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
        assert!(addr.suffix.is_none());
    }

    #[test]
    fn test_pipeline_from_config_with_pattern_override() {
        // Override unit_type_value to remove [A-Z] alternative (single letter unit)
        let toml_str = r#"
[rules.pattern_overrides]
unit_type_value = '(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+)\s*$'
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        let p = Pipeline::from_config(&config);
        // Single letter unit should NOT match (removed [A-Z] alternative)
        let addr = p.parse("123 Main St B");
        // B should end up in street name, not unit
        assert!(addr.unit.is_none() || addr.unit.as_deref() != Some("B"));
    }

    #[test]
    fn test_pipeline_from_config_with_dict_override() {
        // Add a custom suffix "PSGE" → "PASSAGE" so that "123 Main Psge" parses
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
        assert_eq!(addr.street_name.as_deref(), Some("MAIN"));
    }

    #[test]
    fn test_suffix_standardize_short_output() {
        let mut config = crate::config::Config::default();
        config.output.suffix = crate::config::OutputFormat::Short;
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 Main Drive");
        assert_eq!(addr.suffix.as_deref(), Some("DR"));
    }

    #[test]
    fn test_direction_standardize_long_output() {
        let mut config = crate::config::Config::default();
        config.output.direction = crate::config::OutputFormat::Long;
        let p = Pipeline::from_config(&config);
        let addr = p.parse("123 N Main St");
        assert_eq!(addr.pre_direction.as_deref(), Some("NORTH"));
    }
}
