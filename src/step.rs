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
    loop {
        let start = match result[search_from..].find('{') {
            Some(s) => search_from + s,
            None => break,
        };
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
            let values = match (table_name, accessor) {
                ("state", _) => table.bounded_regex(),
                ("unit_type", None) => table
                    .all_values()
                    .into_iter()
                    .filter(|v| *v != "#")
                    .collect::<Vec<_>>()
                    .join("|"),
                (_, Some("short")) => table.short_values().join("|"),
                _ => table.all_values().join("|"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardizeMode {
    WholeField,
    PerWord,
}

#[derive(Debug)]
pub enum Step {
    Rewrite {
        label: String,
        pattern: Regex,
        pattern_template: String,
        replacement: Option<String>,
        rewrite_table: Option<String>,
        source: Option<Col>,
        enabled: bool,
    },
    Extract {
        label: String,
        pattern: Regex,
        pattern_template: String,
        target: Option<Col>,
        targets: Option<std::collections::HashMap<Col, usize>>,
        skip_if_filled: bool,
        replacement: Option<(Regex, String)>,
        source: Option<Col>,
        enabled: bool,
    },
    Standardize {
        label: String,
        target: Col,
        table: String,
        pattern: Option<(Regex, String)>,
        mode: StandardizeMode,
        enabled: bool,
    },
}

impl Step {
    pub fn label(&self) -> &str {
        match self {
            Step::Rewrite { label, .. } => label,
            Step::Extract { label, .. } => label,
            Step::Standardize { label, .. } => label,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Step::Rewrite { enabled, .. } => *enabled,
            Step::Extract { enabled, .. } => *enabled,
            Step::Standardize { enabled, .. } => *enabled,
        }
    }

    pub fn set_enabled(&mut self, value: bool) {
        match self {
            Step::Rewrite { enabled, .. } => *enabled = value,
            Step::Extract { enabled, .. } => *enabled = value,
            Step::Standardize { enabled, .. } => *enabled = value,
        }
    }

    pub fn pattern_template(&self) -> Option<&str> {
        match self {
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
            Step::Rewrite { .. } => "rewrite",
            Step::Extract { .. } => "extract",
            Step::Standardize { .. } => "standardize",
        }
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
    if !step.enabled() {
        return;
    }

    match step {
        Step::Rewrite { pattern, replacement, rewrite_table, source, .. } => {
            let target_str = match source {
                Some(field) => match state.fields.field(*field) {
                    Some(v) => v.clone(),
                    None => return,
                },
                None => state.working.clone(),
            };
            if !pattern.is_match(&target_str).unwrap_or(false) {
                return;
            }
            let mut result = target_str;
            if let Some(table_name) = rewrite_table {
                if let Some(table) = tables.get(table_name) {
                    for (short, long) in table.short_to_long_pairs() {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        replace_pattern(&mut result, &re, &long);
                    }
                }
            } else if let Some(repl) = replacement {
                if repl.contains("${") {
                    // Table lookup replacement — replace all matches right-to-left
                    let mut matches = Vec::new();
                    let mut search_start = 0;
                    while search_start <= result.len() {
                        if let Ok(Some(caps)) = pattern.captures(&result[search_start..]) {
                            if let Some(full_match) = caps.get(0) {
                                let abs_start = search_start + full_match.start();
                                let abs_end = search_start + full_match.end();
                                let expanded = expand_replacement(repl, &caps, tables);
                                matches.push((abs_start, abs_end, expanded));
                                if abs_end == full_match.start() {
                                    // zero-width match — advance by one char to avoid infinite loop
                                    search_start = abs_end + 1;
                                } else {
                                    search_start = abs_end;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    // Replace right-to-left to preserve offsets
                    for (start, end, expanded) in matches.into_iter().rev() {
                        result.replace_range(start..end, &expanded);
                    }
                } else {
                    replace_pattern(&mut result, pattern, repl);
                }
            }
            squish(&mut result);
            match source {
                Some(field) => *state.fields.field_mut(*field) = none_if_empty(result),
                None => state.working = result,
            }
        }
        Step::Extract { pattern, target, targets, skip_if_filled, replacement, source, .. } => {
            if *skip_if_filled {
                if let Some(targets_map) = targets {
                    if targets_map.keys().any(|f| state.fields.field(*f).is_some()) {
                        return;
                    }
                } else if let Some(target_field) = target {
                    if state.fields.field(*target_field).is_some() {
                        return;
                    }
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
                        if let Some(Some(val)) = groups.get(*group_num) {
                            if !val.is_empty() {
                                *state.fields.field_mut(*field) = Some(val.clone());
                            }
                        }
                    }
                } else if let Some(target_field) = target {
                    let mut val = groups[0].clone().unwrap_or_default();
                    if let Some((re, repl)) = replacement {
                        replace_pattern(&mut val, re, repl);
                    }
                    *state.fields.field_mut(*target_field) = none_if_empty(val);
                }
            }
        }
        Step::Standardize { target, table, pattern, mode, .. } => {
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

            let t = match tables.get(table) {
                Some(t) => t,
                None => return,
            };

            let fmt = output.format_for_field(*target);

            match mode {
                StandardizeMode::WholeField => {
                    *state.fields.field_mut(*target) = Some(standardize_value(&val, t, fmt));
                }
                StandardizeMode::PerWord => {
                    let words: Vec<&str> = val.split_whitespace().collect();
                    let result: Vec<String> = words.iter()
                        .map(|w| standardize_value(w, t, fmt))
                        .collect();
                    *state.fields.field_mut(*target) = Some(result.join(" "));
                }
            }
        }
    }
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
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_if_filled: Option<bool>,
    /// Deprecated: use `table` instead. Kept for backward compat with user configs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matching_table: Option<String>,
    /// Deprecated: ignored. Use `table` instead. Kept for backward compat deserialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<std::collections::HashMap<String, usize>>,
}

#[derive(Debug, Deserialize)]
pub struct StepsDef {
    pub step: Vec<StepDef>,
}

fn parse_col(name: &str) -> Result<Col, String> {
    match name {
        "street_number" => Ok(Col::StreetNumber),
        "pre_direction" => Ok(Col::PreDirection),
        "street_name" => Ok(Col::StreetName),
        "suffix" => Ok(Col::Suffix),
        "post_direction" => Ok(Col::PostDirection),
        "unit" => Ok(Col::Unit),
        "unit_type" => Ok(Col::UnitType),
        "po_box" => Ok(Col::PoBox),
        "building" => Ok(Col::Building),
        "extra_front" => Ok(Col::ExtraFront),
        "extra_back" => Ok(Col::ExtraBack),
        _ => Err(format!("Unknown column name: {}", name)),
    }
}

/// Compile a single StepDef into a Step, expanding table references in patterns.
pub fn compile_step(def: &StepDef, abbr: &Abbreviations) -> Result<Step, String> {
    match def.step_type.as_str() {
        "rewrite" => {
            let template = def
                .pattern
                .as_ref()
                .ok_or_else(|| format!("rewrite step '{}' missing pattern", def.label))?;
            let expanded = expand_template(template, abbr);
            let pattern = Regex::new(&expanded)
                .map_err(|e| format!("Bad regex in step '{}': {}", def.label, e))?;
            let source = def.source.as_ref().map(|s| parse_col(s)).transpose()?;
            Ok(Step::Rewrite {
                label: def.label.clone(),
                pattern,
                pattern_template: template.clone(),
                replacement: def.replacement.clone(),
                rewrite_table: def.table.clone(),
                source,
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

            let targets = if let Some(ref t) = def.targets {
                let mut map = std::collections::HashMap::new();
                for (field_name, group_num) in t {
                    map.insert(parse_col(field_name)?, *group_num);
                }
                Some(map)
            } else {
                None
            };

            let target = if targets.is_some() {
                if def.target.is_some() {
                    return Err(format!("extract step '{}' has both target and targets", def.label));
                }
                if def.replacement.is_some() {
                    return Err(format!("extract step '{}' has both targets and replacement", def.label));
                }
                None
            } else {
                Some(parse_col(
                    def.target
                        .as_ref()
                        .ok_or_else(|| format!("extract step '{}' missing target or targets", def.label))?
                )?)
            };

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

            let source = def.source.as_ref().map(|s| parse_col(s)).transpose()?;
            Ok(Step::Extract {
                label: def.label.clone(),
                pattern,
                pattern_template: template,
                target,
                targets,
                skip_if_filled: def.skip_if_filled.unwrap_or(false),
                replacement,
                source,
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

            let pattern = if let Some(ref p) = def.pattern {
                let expanded = expand_template(p, abbr);
                let re = Regex::new(&expanded)
                    .map_err(|e| format!("standardize step '{}' bad pattern: {}", def.label, e))?;
                let repl = def.replacement.clone().unwrap_or_default();
                Some((re, repl))
            } else {
                None
            };

            let table = if pattern.is_none() {
                def.table.clone()
                    .or(def.matching_table.clone()) // backward compat: accept old field name
                    .ok_or_else(|| format!(
                        "standardize step '{}' needs either pattern+replacement or table",
                        def.label
                    ))?
            } else {
                def.table.clone().unwrap_or_default()
            };

            Ok(Step::Standardize {
                label: def.label.clone(),
                target: parse_col(target)?,
                table,
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
    fn test_default_steps_toml_parses() {
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        assert!(defs.step.len() > 20, "Expected 20+ steps, got {}", defs.step.len());
        assert_eq!(defs.step[0].step_type, "rewrite");
        assert_eq!(defs.step[0].label, "prep_fix_ampersand");
        let last = defs.step.last().unwrap();
        assert_eq!(last.step_type, "standardize");
    }

    #[test]
    fn test_compile_all_default_steps() {
        use crate::tables::abbreviations::build_default_tables;
        let toml_str = include_str!("../data/defaults/steps.toml");
        let defs: StepsDef = toml::from_str(toml_str).unwrap();
        let abbr = build_default_tables();
        let steps = compile_steps(&defs.step, &abbr);
        assert!(steps.len() > 20);
        assert_eq!(steps[0].step_type(), "rewrite");
        assert_eq!(steps[0].label(), "prep_fix_ampersand");
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
            table: None, target: None, source: None,
            skip_if_filled: None, matching_table: None, format_table: None, mode: None,
            targets: None,
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
            table: None, target: Some("street_number".to_string()), source: None,
            skip_if_filled: Some(true),
            matching_table: None, format_table: None, mode: None,
            targets: None,
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
            pattern: None, replacement: None,
            table: Some("suffix_all".to_string()),
            source: None,
            target: Some("suffix".to_string()),
            skip_if_filled: None,
            matching_table: None,
            format_table: None,
            mode: None,
            targets: None,
        };
        let step = compile_step(&def, &abbr).unwrap();
        let output = OutputConfig::default();
        let mut state = AddressState::new_from_prepared(String::new());
        state.fields.suffix = Some("AV".to_string());
        apply_step(&mut state, &step, &abbr, &output);
        // AV → finds AVENUE group → canonical long (default output) = AVENUE
        assert_eq!(state.fields.suffix.as_deref(), Some("AVENUE"));
    }

    #[test]
    fn test_expand_template_all_values() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let expanded = expand_template("{direction}", &abbr);
        assert!(expanded.contains("NORTH"));
        assert!(expanded.contains("NE"));
    }

    #[test]
    fn test_expand_template_short_accessor() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let expanded = expand_template("{direction$short}", &abbr);
        assert!(expanded.contains("NE"));
        assert!(!expanded.contains("NORTH"));
    }

    #[test]
    fn test_expand_template_state_bounded() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let expanded = expand_template("{state}", &abbr);
        assert!(expanded.starts_with(r"(?:"));
    }

    #[test]
    fn test_expand_template_unit_type_excludes_hash() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let expanded = expand_template("{unit_type}", &abbr);
        assert!(!expanded.contains("#"));
        assert!(expanded.contains("APARTMENT"));
    }

    #[test]
    fn test_expand_template_regex_quantifiers_preserved() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let expanded = expand_template(r"\d{5}(?:\W\d{4})?", &abbr);
        assert_eq!(expanded, r"\d{5}(?:\W\d{4})?");
    }

    #[test]
    fn test_expand_template_mixed() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
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
            table: None,
            target: Some("po_box".to_string()),
            replacement: None,
            source: None,
            skip_if_filled: Some(true),
            matching_table: None,
            format_table: None,
            mode: None,
            targets: None,
        };
        let toml_str = toml::to_string_pretty(&def).unwrap();
        let parsed: StepDef = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.step_type, "extract");
        assert_eq!(parsed.label, "custom_box");
        assert_eq!(parsed.target.as_deref(), Some("po_box"));
        // Optional None fields should not appear in serialized output
        assert!(!toml_str.contains("table"));
        assert!(!toml_str.contains("replacement"));
    }

    #[test]
    fn test_parse_field_invalid_returns_error() {
        let result = parse_col("nonexistent_field");
        assert!(result.is_err());
    }

    #[test]
    fn test_rewrite_with_source_field() {
        use crate::address::AddressState;
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let def = StepDef {
            step_type: "rewrite".to_string(),
            label: "strip_hash".to_string(),
            pattern: Some(r"^#\s*".to_string()),
            replacement: Some("".to_string()),
            table: None, target: None, source: Some("unit".to_string()),
            skip_if_filled: None, matching_table: None, format_table: None, mode: None,
            targets: None,
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
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let def = StepDef {
            step_type: "extract".to_string(),
            label: "promote_unit".to_string(),
            pattern: Some(r"^.+$".to_string()),
            replacement: None,
            table: None, target: Some("street_number".to_string()),
            source: Some("unit".to_string()),
            skip_if_filled: Some(true),
            matching_table: None, format_table: None, mode: None,
            targets: None,
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
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let toml_str = r#"
[[step]]
type = "extract"
label = "unit_split"
pattern = '(APT)\W*(\d+[A-Z]?)\s*$'
targets = { unit_type = 1, unit = 2 }
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
        use crate::tables::abbreviations::build_default_tables;
        use crate::config::OutputConfig;
        let abbr = build_default_tables();
        let toml_str = r#"
[[step]]
type = "extract"
label = "unit_split"
pattern = '(APT)\W*(\d+)\s*$'
targets = { unit_type = 1, unit = 2 }
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
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let re = Regex::new(r"(HIGHWAY)\s+(\d{1,3})").unwrap();
        let caps = re.captures("HIGHWAY 42").unwrap().unwrap();
        let result = expand_replacement("$1 ${2:number_cardinal}", &caps, &abbr);
        assert_eq!(result, "HIGHWAY FORTYTWO");
    }

    #[test]
    fn test_expand_replacement_ordinal() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let re = Regex::new(r"(\d{1,3})(ST|ND|RD|TH)").unwrap();
        let caps = re.captures("21ST").unwrap().unwrap();
        let result = expand_replacement("${1:number_ordinal}", &caps, &abbr);
        assert_eq!(result, "TWENTYFIRST");
    }

    #[test]
    fn test_expand_replacement_fraction() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let re = Regex::new(r"(\d{1,3})\s+(\d+)/(\d+)").unwrap();
        let caps = re.captures("8 5/8").unwrap().unwrap();
        let result = expand_replacement("${1:number_cardinal} AND ${2/3:fraction}", &caps, &abbr);
        assert_eq!(result, "EIGHT AND FIVEEIGHTHS");
    }

    #[test]
    fn test_expand_replacement_fraction_half() {
        use crate::tables::abbreviations::build_default_tables;
        let abbr = build_default_tables();
        let re = Regex::new(r"(\d{1,3})\s+(\d+)/(\d+)").unwrap();
        let caps = re.captures("8 1/2").unwrap().unwrap();
        let result = expand_replacement("${1:number_cardinal} AND ${2/3:fraction}", &caps, &abbr);
        assert_eq!(result, "EIGHT AND ONEHALF");
    }
}
