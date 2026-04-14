use fancy_regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

use crate::address::Col;
use crate::config::OutputFormat;
use crate::ops::{extract_remove, none_if_empty, replace_pattern, squish};
use crate::tables::abbreviations::AbbrTable;
use crate::tables::Abbreviations;

/// Expand all `{...}` placeholders in a template using abbreviation tables.
///
/// - `{table_name}` → `table.all_values().join("|")`
/// - `{table_name$short}` → `table.short_values().join("|")`
///
/// Special cases:
/// - `state` uses `bounded_regex()` (word-boundary-wrapped)
/// - `unit_type` excludes `#` from `all_values()`
///
/// Regex quantifiers like `{5}` or `{1,6}` are left untouched.
pub fn expand_template(template: &str, abbr: &Abbreviations) -> String {
    let mut result = template.to_string();
    let mut search_from = 0;
    while let Some(s) = result[search_from..].find('{') {
        let start = search_from + s;
        let end = match result[start..].find('}') {
            Some(e) => start + e,
            None => break,
        };
        let placeholder = result[start + 1..end].to_string();

        // Skip regex quantifiers like {5} or {1,6}
        if placeholder.chars().all(|c| c.is_ascii_digit() || c == ',') {
            search_from = end + 1;
            continue;
        }

        let (table_name, accessor) = if let Some(idx) = placeholder.find('$') {
            (&placeholder[..idx], Some(&placeholder[idx + 1..]))
        } else {
            (placeholder.as_str(), None)
        };

        if let Some(table) = abbr.get(table_name) {
            let values = match accessor {
                Some("short") => table.short_values().join("|"),
                _ => table.bounded_regex(),
            };
            let before = &result[..start];
            let after = &result[end + 1..];
            let new_result = format!("{}{}{}", before, values, after);
            search_from = start + values.len();
            result = new_result;
        } else {
            // Unknown table — skip past to avoid infinite loop
            search_from = end + 1;
        }
    }
    result
}

/// Expand a replacement template with capture group backrefs and table lookups.
///
/// Syntax:
/// - `$N` — capture group N (single digit, standard backref)
/// - `${N}` — capture group N (braced)
/// - `${N:table_name}` — capture group N, looked up in table (via to_long)
/// - `${N/M:fraction}` — fraction expansion (N=numerator group, M=denominator group)
pub fn expand_replacement(template: &str, caps: &Captures, tables: &Abbreviations) -> String {
    let mut result = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            if chars[i + 1] == '{' {
                // ${...} syntax
                if let Some(close) = chars[i + 2..].iter().position(|&c| c == '}') {
                    let inner: String = chars[i + 2..i + 2 + close].iter().collect();
                    result.push_str(&resolve_template_token(&inner, caps, tables));
                    i = i + 2 + close + 1;
                    continue;
                }
            } else if chars[i + 1].is_ascii_digit() {
                // $N syntax (single digit)
                let n = (chars[i + 1] as u8 - b'0') as usize;
                if let Some(m) = caps.get(n) {
                    result.push_str(m.as_str());
                }
                i += 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Resolve a single template token (the content inside ${...}).
fn resolve_template_token(token: &str, caps: &Captures, tables: &Abbreviations) -> String {
    // Check for fraction: N/M:fraction
    if let Some(frac_idx) = token.find(":fraction") {
        let nums = &token[..frac_idx];
        if let Some(slash) = nums.find('/') {
            let num_group: usize = nums[..slash].parse().unwrap_or(0);
            let den_group: usize = nums[slash + 1..].parse().unwrap_or(0);
            let num_val: u16 = caps.get(num_group)
                .map(|m| m.as_str().trim().parse().unwrap_or(0))
                .unwrap_or(0);
            let den_val: u16 = caps.get(den_group)
                .map(|m| m.as_str().trim().parse().unwrap_or(0))
                .unwrap_or(0);
            if num_val > 0 && den_val >= 2 && num_val <= 999 && den_val <= 999 {
                return crate::tables::numbers::fraction(num_val, den_val);
            }
            // Denominator < 2 or out of range — keep original text (e.g. "3/1")
            let num_str = caps.get(num_group).map(|m| m.as_str()).unwrap_or("");
            let den_str = caps.get(den_group).map(|m| m.as_str()).unwrap_or("");
            return format!("{}/{}", num_str, den_str);
        }
        return String::new();
    }

    // Check for table lookup: N:table_name
    if let Some(colon) = token.find(':') {
        let group_num: usize = token[..colon].parse().unwrap_or(0);
        let table_name = &token[colon + 1..];
        if let Some(m) = caps.get(group_num) {
            let captured = m.as_str().trim();
            if let Some(table) = tables.get(table_name) {
                return table.to_long(captured).unwrap_or(captured).to_string();
            }
            return captured.to_string();
        }
        return String::new();
    }

    // Plain group number
    let group_num: usize = token.parse().unwrap_or(0);
    caps.get(group_num).map(|m| m.as_str().to_string()).unwrap_or_default()
}

/// Expand a simple replacement template using pre-extracted capture groups.
///
/// Supports `$N` syntax only (no `${N:table}` or `${N/M:fraction}`).
/// Used by extract steps where captures are already collected as strings.
fn expand_groups(template: &str, groups: &[Option<String>]) -> String {
    let mut result = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            let n = (chars[i + 1] as u8 - b'0') as usize;
            if let Some(Some(val)) = groups.get(n) {
                result.push_str(val);
            }
            i += 2;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Compiled step types
// ---------------------------------------------------------------------------

/// The compiled data that differs between step types.
/// String fields shared by both types live in `StepDef`.
#[derive(Debug)]
enum CompiledStep {
    Rewrite {
        pattern: Option<Regex>,
        source: Option<Col>,
    },
    Extract {
        pattern: Regex,
        pattern_template: String,
        target: Option<Col>,
        targets: Option<std::collections::HashMap<Col, usize>>,
        source: Option<Col>,
    },
}

/// A compiled, ready-to-execute pipeline step.
///
/// Holds the original `StepDef` (for serialization, display, diffing)
/// plus compiled regex and parsed column enums.
#[derive(Debug)]
pub struct Step {
    pub def: StepDef,
    pub enabled: bool,
    compiled: CompiledStep,
}

impl Step {
    pub fn label(&self) -> &str { &self.def.label }
    pub fn enabled(&self) -> bool { self.enabled }
    pub fn set_enabled(&mut self, value: bool) { self.enabled = value; }

    pub fn pattern_template(&self) -> Option<&str> {
        match &self.compiled {
            CompiledStep::Rewrite { pattern, .. } => {
                if pattern.is_some() { self.def.pattern.as_deref() } else { None }
            }
            CompiledStep::Extract { pattern_template, .. } => Some(pattern_template),
        }
    }

    pub fn step_type(&self) -> &str { &self.def.step_type }
}

// ---------------------------------------------------------------------------
// Step execution
// ---------------------------------------------------------------------------

/// Scan `s` for all non-overlapping matches of `re`, compute a replacement for each
/// via `make_replacement`, and apply them in reverse order to preserve positions.
fn replace_all_captures(
    s: &mut String,
    re: &Regex,
    mut make_replacement: impl FnMut(&Captures) -> String,
) {
    let mut replacements = Vec::new();
    let mut pos = 0;
    while pos <= s.len() {
        match re.captures(&s[pos..]) {
            Ok(Some(caps)) => {
                let full = caps.get(0).unwrap();
                let abs_start = pos + full.start();
                let abs_end = pos + full.end();
                replacements.push((abs_start, abs_end, make_replacement(&caps)));
                if abs_end == abs_start { pos = abs_end + 1; } else { pos = abs_end; }
            }
            _ => break,
        }
    }
    for (start, end, repl) in replacements.into_iter().rev() {
        s.replace_range(start..end, &repl);
    }
}

fn standardize_value(
    value: &str,
    table: &AbbrTable,
    format: OutputFormat,
) -> String {
    match table.standardize(value) {
        Some((_, short, long)) => match format {
            OutputFormat::Short => short.to_string(),
            OutputFormat::Long => long.to_string(),
        },
        None => value.to_string(),
    }
}

pub fn apply_step(
    state: &mut crate::address::AddressState,
    step: &Step,
    tables: &Abbreviations,
    output: &crate::config::OutputConfig,
) {
    if !step.enabled {
        return;
    }

    match &step.compiled {
        CompiledStep::Rewrite { pattern, source } => {
            let target_str = match source {
                Some(field) => match state.fields.field(*field) {
                    Some(v) => v.clone(),
                    None => return,
                },
                None => state.working.clone(),
            };

            let replacement = step.def.replacement.as_deref();
            let table_name = step.def.table.as_deref();
            let mode = step.def.mode.as_deref();

            let mut result = if let Some(table_name) = table_name {
                let t = match tables.get(table_name) {
                    Some(t) => t,
                    None => return,
                };
                let fmt = source
                    .map(|f| output.format_for_field(f))
                    .unwrap_or(OutputFormat::Long);

                if mode == Some("per_word") {
                    target_str.split_whitespace()
                        .map(|w| standardize_value(w, t, fmt))
                        .collect::<Vec<_>>()
                        .join(" ")
                } else if let Some(re) = pattern {
                    if !re.is_match(&target_str).unwrap_or(false) {
                        return;
                    }
                    let mut s = target_str;
                    replace_all_captures(&mut s, re, |caps| {
                        let full = caps.get(0).unwrap();
                        let captured = caps.get(1).unwrap_or(full).as_str();
                        standardize_value(captured, t, fmt)
                    });
                    s
                } else {
                    standardize_value(&target_str, t, fmt)
                }
            } else if let Some(re) = pattern {
                if !re.is_match(&target_str).unwrap_or(false) {
                    return;
                }
                let mut s = target_str;
                if let Some(repl) = replacement {
                    if repl.contains("${") {
                        replace_all_captures(&mut s, re, |caps| {
                            expand_replacement(repl, caps, tables)
                        });
                    } else {
                        replace_pattern(&mut s, re, repl);
                    }
                }
                s
            } else {
                return;
            };

            squish(&mut result);
            match source {
                Some(field) => *state.fields.field_mut(*field) = none_if_empty(result),
                None => state.working = result,
            }
        }
        CompiledStep::Extract { pattern, target, targets, source, .. } => {
            let skip_if_filled = step.def.skip_if_filled.unwrap_or(false);
            if skip_if_filled {
                if let Some(targets_map) = targets {
                    if targets_map.keys().any(|f| state.fields.field(*f).is_some()) {
                        return;
                    }
                } else if let Some(target_field) = target
                    && state.fields.field(*target_field).is_some() {
                        return;
                    }
            }
            let extract_result = match source {
                Some(field) => {
                    let field_val = match state.fields.field(*field) {
                        Some(v) => v.clone(),
                        None => return,
                    };
                    let mut src = field_val;
                    let result = extract_remove(&mut src, pattern);
                    *state.fields.field_mut(*field) = none_if_empty(src);
                    result
                },
                None => extract_remove(&mut state.working, pattern),
            };
            if let Some(groups) = extract_result {
                if let Some(targets_map) = targets {
                    for (field, group_num) in targets_map {
                        if let Some(Some(val)) = groups.get(*group_num)
                            && !val.is_empty() {
                                *state.fields.field_mut(*field) = Some(val.clone());
                            }
                    }
                } else if let Some(target_field) = target {
                    let val = if let Some(ref repl) = step.def.replacement {
                        expand_groups(repl, &groups)
                    } else {
                        groups[0].clone().unwrap_or_default()
                    };
                    *state.fields.field_mut(*target_field) = none_if_empty(val);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// StepDef and compilation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputCol {
    Single(String),
    Multi(std::collections::HashMap<String, usize>),
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, PartialEq)]
pub struct StepDef {
    #[serde(rename = "type")]
    pub step_type: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_col: Option<OutputCol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_if_filled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_col: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StepsDef {
    pub step: Vec<StepDef>,
}

/// Compile a single StepDef into a Step, expanding table references in patterns.
pub fn compile_step(def: &StepDef, abbr: &Abbreviations) -> Result<Step, String> {
    let (parsed_target, parsed_targets) = match &def.output_col {
        Some(OutputCol::Single(name)) => (Some(Col::from_key(name)?), None),
        Some(OutputCol::Multi(map)) => {
            let mut parsed = std::collections::HashMap::new();
            for (col_name, group_num) in map {
                parsed.insert(Col::from_key(col_name)?, *group_num);
            }
            (None, Some(parsed))
        }
        None => (None, None),
    };

    let source = def.input_col.as_ref().map(|s| Col::from_key(s)).transpose()?;

    match def.step_type.as_str() {
        "rewrite" => {
            let pattern = if let Some(ref template) = def.pattern {
                let expanded = expand_template(template, abbr);
                Some(Regex::new(&expanded)
                    .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?)
            } else {
                None
            };

            if pattern.is_none() && def.table.is_none() {
                return Err(format!("rewrite step '{}' needs pattern or table", def.label));
            }

            Ok(Step {
                def: def.clone(),
                enabled: true,
                compiled: CompiledStep::Rewrite { pattern, source },
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

            if parsed_targets.is_some() {
                if def.replacement.is_some() {
                    return Err(format!("extract step '{}' has both multi output_col and replacement", def.label));
                }
            } else if parsed_target.is_none() {
                return Err(format!("extract step '{}' missing output_col", def.label));
            }

            Ok(Step {
                def: def.clone(),
                enabled: true,
                compiled: CompiledStep::Extract {
                    pattern,
                    pattern_template: template,
                    target: parsed_target,
                    targets: parsed_targets,
                    source,
                },
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
    fn test_default_steps_toml_parses() {
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert!(defs.step.len() > 20, "Expected 20+ steps, got {}", defs.step.len());
        assert_eq!(defs.step[0].step_type, "rewrite");
        assert_eq!(defs.step[0].label, "fix_ampersand");
        let last = defs.step.last().unwrap();
        assert_eq!(last.step_type, "rewrite");
    }

    #[test]
    fn test_compile_all_default_steps() {
        use crate::tables::abbreviations::load_default_tables;
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let abbr = load_default_tables();
        let steps = compile_steps(&defs.step, &abbr);
        assert!(steps.len() > 20);
        assert_eq!(steps[0].step_type(), "rewrite");
        assert_eq!(steps[0].label(), "fix_ampersand");
    }

    #[test]
    fn test_apply_rewrite_step() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let def = StepDef {
            step_type: "rewrite".to_string(),
            label: "test_rewrite".to_string(),
            pattern: Some(r"STAPT".to_string()),
            replacement: Some("ST APT".to_string()),
            ..Default::default()
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
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let toml_str = r#"
[[step]]
type = "rewrite"
label = "street_name"
pattern = '\b({street_name$short})\b'
table = "street_name"
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
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let def = StepDef {
            step_type: "extract".to_string(),
            label: "test_number".to_string(),
            pattern: Some(r"^\d+\b".to_string()),
            output_col: Some(OutputCol::Single("street_number".to_string())),
            skip_if_filled: Some(true),
            ..Default::default()
        };
        let step = compile_step(&def, &abbr).unwrap();
        let mut state = AddressState::new_from_prepared("123 MAIN ST".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.fields.street_number.as_deref(), Some("123"));
        assert_eq!(state.working, "MAIN ST");
    }

    #[test]
    fn test_apply_rewrite_table_on_field() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let def = StepDef {
            step_type: "rewrite".to_string(),
            label: "std_suffix".to_string(),
            table: Some("suffix_all".to_string()),
            output_col: Some(OutputCol::Single("suffix".to_string())),
            input_col: Some("suffix".to_string()),
            ..Default::default()
        };
        let step = compile_step(&def, &abbr).unwrap();
        let output = OutputConfig::default();
        let mut state = AddressState::new_from_prepared(String::new());
        state.fields.suffix = Some("AV".to_string());
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.fields.suffix.as_deref(), Some("AVENUE"));
    }

    #[test]
    fn test_expand_template_all_values() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let expanded = expand_template("{direction}", &abbr);
        assert!(expanded.contains("NORTH"));
        assert!(expanded.contains("NE"));
    }

    #[test]
    fn test_expand_template_short_accessor() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let expanded = expand_template("{direction$short}", &abbr);
        assert!(expanded.contains("NE"));
        assert!(!expanded.contains("NORTH"));
    }

    #[test]
    fn test_expand_template_state_bounded() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let expanded = expand_template("{state}", &abbr);
        assert!(expanded.starts_with(r"(?:"));
    }

    #[test]
    fn test_expand_template_unit_type_includes_all() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let expanded = expand_template("{unit_type}", &abbr);
        assert!(expanded.contains("APARTMENT"));
        // # is included in the bounded regex; \b in the step pattern prevents false matches
        assert!(expanded.contains("#"));
    }

    #[test]
    fn test_expand_template_regex_quantifiers_preserved() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let expanded = expand_template(r"\d{5}(?:\W\d{4})?", &abbr);
        assert_eq!(expanded, r"\d{5}(?:\W\d{4})?");
    }

    #[test]
    fn test_expand_template_mixed() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let expanded = expand_template(
            r"^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s([A-Z]{3,20})",
            &abbr,
        );
        assert!(expanded.contains("NORTH"));
        assert!(expanded.contains(r"\d{1,6}"));
        assert!(expanded.contains(r"[A-Z]{3,20}"));
    }

    #[test]
    fn test_stepdef_roundtrip_serialize() {
        let def = StepDef {
            step_type: "extract".to_string(),
            label: "custom_box".to_string(),
            pattern: Some(r"\bBOX (\d+)".to_string()),
            output_col: Some(OutputCol::Single("po_box".to_string())),
            skip_if_filled: Some(true),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&def).unwrap();
        let parsed: StepDef = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.step_type, "extract");
        assert_eq!(parsed.label, "custom_box");
        assert!(matches!(&parsed.output_col, Some(OutputCol::Single(s)) if s == "po_box"));
        // Optional None fields should not appear in serialized output
        assert!(!toml_str.contains("table"));
        assert!(!toml_str.contains("replacement"));
    }

    #[test]
    fn test_col_from_key_invalid_returns_error() {
        let result = Col::from_key("nonexistent_field");
        assert!(result.is_err());
    }

    #[test]
    fn test_rewrite_with_source_field() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let def = StepDef {
            step_type: "rewrite".to_string(),
            label: "strip_hash".to_string(),
            pattern: Some(r"^#\s*".to_string()),
            replacement: Some("".to_string()),
            input_col: Some("unit".to_string()),
            ..Default::default()
        };
        let step = compile_step(&def, &abbr).unwrap();
        let mut state = AddressState::new_from_prepared("123 MAIN ST".to_string());
        state.fields.unit = Some("#4B".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.fields.unit.as_deref(), Some("4B"));
        assert_eq!(state.working, "123 MAIN ST");
    }

    #[test]
    fn test_extract_with_source_field_move() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let def = StepDef {
            step_type: "extract".to_string(),
            label: "promote_unit".to_string(),
            pattern: Some(r"^.+$".to_string()),
            output_col: Some(OutputCol::Single("street_number".to_string())),
            input_col: Some("unit".to_string()),
            skip_if_filled: Some(true),
            ..Default::default()
        };
        let step = compile_step(&def, &abbr).unwrap();
        let mut state = AddressState::new_from_prepared("MAIN ST".to_string());
        state.fields.unit = Some("42".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &step, &abbr, &output);
        assert_eq!(state.fields.street_number.as_deref(), Some("42"));
        assert!(state.fields.unit.is_none());
    }

    #[test]
    fn test_extract_with_targets() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let toml_str = r#"
[[step]]
type = "extract"
label = "unit_split"
pattern = '(APT)\W*(\d+[A-Z]?)\s*$'
output_col = { unit_type = 1, unit = 2 }
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let steps = compile_steps(&defs.step, &abbr);
        let mut state = AddressState::new_from_prepared("123 MAIN ST APT 4B".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &steps[0], &abbr, &output);
        assert_eq!(state.fields.unit_type.as_deref(), Some("APT"));
        assert_eq!(state.fields.unit.as_deref(), Some("4B"));
        assert_eq!(state.working, "123 MAIN ST");
    }

    #[test]
    fn test_extract_targets_skip_if_filled() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::load_default_tables;
        use crate::config::OutputConfig;
        let abbr = load_default_tables();
        let toml_str = r#"
[[step]]
type = "extract"
label = "unit_split"
pattern = '(APT)\W*(\d+)\s*$'
output_col = { unit_type = 1, unit = 2 }
skip_if_filled = true
"#;
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let steps = compile_steps(&defs.step, &abbr);
        let mut state = AddressState::new_from_prepared("123 MAIN ST APT 4B".to_string());
        state.fields.unit = Some("EXISTING".to_string());
        let output = OutputConfig::default();
        apply_step(&mut state, &steps[0], &abbr, &output);
        assert_eq!(state.fields.unit.as_deref(), Some("EXISTING"));
        assert!(state.fields.unit_type.is_none());
    }

    #[test]
    fn test_expand_replacement_simple_backref() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let re = Regex::new(r"(HIGHWAY)\s+(\d{1,3})").unwrap();
        let caps = re.captures("HIGHWAY 42").unwrap().unwrap();
        let result = expand_replacement("$1 ${2:number_cardinal}", &caps, &abbr);
        assert_eq!(result, "HIGHWAY FORTYTWO");
    }

    #[test]
    fn test_expand_replacement_ordinal() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let re = Regex::new(r"(\d{1,3})(ST|ND|RD|TH)").unwrap();
        let caps = re.captures("21ST").unwrap().unwrap();
        let result = expand_replacement("${1:number_ordinal}", &caps, &abbr);
        assert_eq!(result, "TWENTYFIRST");
    }

    #[test]
    fn test_expand_replacement_fraction() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let re = Regex::new(r"(\d{1,3})\s+(\d+)/(\d+)").unwrap();
        let caps = re.captures("8 5/8").unwrap().unwrap();
        let result = expand_replacement("${1:number_cardinal} AND ${2/3:fraction}", &caps, &abbr);
        assert_eq!(result, "EIGHT AND FIVEEIGHTHS");
    }

    #[test]
    fn test_expand_replacement_fraction_half() {
        use crate::tables::abbreviations::load_default_tables;
        let abbr = load_default_tables();
        let re = Regex::new(r"(\d{1,3})\s+(\d+)/(\d+)").unwrap();
        let caps = re.captures("8 1/2").unwrap().unwrap();
        let result = expand_replacement("${1:number_cardinal} AND ${2/3:fraction}", &caps, &abbr);
        assert_eq!(result, "EIGHT AND ONEHALF");
    }

    #[test]
    fn test_expand_groups() {
        let groups = vec![
            Some("P O BOX 123".to_string()),
            Some("123".to_string()),
        ];
        assert_eq!(expand_groups("PO BOX $1", &groups), "PO BOX 123");
    }

    #[test]
    fn test_expand_groups_missing() {
        let groups = vec![Some("FULL".to_string())];
        assert_eq!(expand_groups("$0 keep $1", &groups), "FULL keep ");
    }
}
