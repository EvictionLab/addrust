use std::collections::HashMap;

use fancy_regex::Regex;

use crate::address::Field;
use crate::pipeline::{Action, Rule};
use crate::tables::abbreviations::Abbreviations;

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

/// Build the full ordered pipeline of rules.
/// Extraction order: outside-in, most certain first.
pub fn build_rules(abbr: &Abbreviations, pattern_overrides: &HashMap<String, String>) -> Vec<Rule> {

    // Closure captures shared state — each call site only passes rule-specific args.
    let rule = |label: &str, group: &str, pattern_template: &str,
                action: Action, target: Option<Field>,
                standardize: Option<(&str, &str)>, skip_if_filled: bool| -> Rule {
        let final_template = pattern_overrides
            .get(label)
            .cloned()
            .unwrap_or_else(|| pattern_template.to_string());

        let final_pattern = expand_template(&final_template, abbr);

        let std = standardize.map(|(m, r)| {
            let expanded_m = expand_template(m, abbr);
            (Regex::new(&expanded_m).unwrap(), r.to_string())
        });
        Rule {
            label: label.to_string(),
            group: group.to_string(),
            pattern: Regex::new(&final_pattern)
                .unwrap_or_else(|e| panic!("Bad regex in rule {}: {}", label, e)),
            pattern_template: final_template,
            action,
            target,
            standardize: std,
            standardize_pairs: Vec::new(),
            skip_if_filled,
            enabled: true,
        }
    };

    let mut rules = Vec::new();

    // ═══════════════════════════════════════════════════════════════════
    // 1. NA CHECK
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "change_na_address",
        "na_check",
        r"(?i)^(N/?A|{na_values})$",
        Action::Warn,
        None,
        None,
        false,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 2. CITY / STATE / ZIP — extract from end
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "city_state_zip",
        "city_state",
        r",\s*([A-Z][A-Z ]+)\W+{state}\W+(\d{5}(?:\W\d{4})?)(?:\s*US)?$",
        Action::Extract,
        Some(Field::ExtraBack),
        None,
        false,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 3. PO BOX
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "po_box_number",
        "po_box",
        r"\bP\W*O\W+?BOX\W*(\d+)\b",
        Action::Extract,
        Some(Field::PoBox),
        Some((r"P\W*O\W+?BOX\W*(\d+)", "PO BOX $1")),
        true,
    ));
    rules.push(rule(
        "po_box_word",
        "po_box",
        r"\bP\W*O\W+?BOX\W+(\w+)\b",
        Action::Extract,
        Some(Field::PoBox),
        Some((r"P\W*O\W+?BOX\W+(\w+)", "PO BOX $1")),
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 4. PRE-CHECKS — fix common issues before extraction
    // ═══════════════════════════════════════════════════════════════════
    // Unstick suffix from unit type (e.g., "STAPT" → "ST APT")
    rules.push(rule(
        "change_unstick_suffix_unit",
        "pre_check",
        r"\b({suffix_common})({unit_type})\b",
        Action::Change,
        None,
        Some((r"({suffix_common})({unit_type})", "$1 $2")),
        false,
    ));

    // Saint vs Street: "123 N ST JUDE" → "123 N SAINT JUDE"
    // Condition: after number + optional direction, "ST" before a 3+ letter word
    // that is NOT a suffix/unit_type/unit_location.
    rules.push(rule(
        "change_st_to_saint",
        "pre_check",
        r"^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{suffix_all})\b)([A-Z]{3,20})",
        Action::Change,
        None,
        Some((
            r"^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{suffix_all})\b)([A-Z]{3,20})",
            "${1}SAINT $2",
        )),
        false,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 5. EXTRA FRONT — non-address text before street number
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "extra_front",
        "extra",
        r"^(?:(?:[A-Z\W]+\s)+(?=(?:{direction})\s\d))|^(?:(?:[A-Z\W]+\s)+(?=\d))",
        Action::Extract,
        Some(Field::ExtraFront),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 6. STREET NUMBER — extract from front
    // ═══════════════════════════════════════════════════════════════════
    // Coordinate-style: N123 E456
    rules.push(rule(
        "street_number_coords_two",
        "street_number",
        r"^([NSEW])\W?(\d+)\W?([NSEW])\W?(\d+)\b",
        Action::Extract,
        Some(Field::StreetNumber),
        Some((r"([NSEW])\W?(\d+)\W?([NSEW])\W?(\d+)", "${1}${2} ${3}${4}")),
        true,
    ));
    // Simple: 123 (also strips leading zeros via standardize)
    rules.push(rule(
        "street_number_simple",
        "street_number",
        r"^\d+\b",
        Action::Extract,
        Some(Field::StreetNumber),
        Some((r"^0+(\d+)", "$1")),
        true,
    ));

    // Fraction after street number (e.g., "1/2" from "156 1/2 Main St") → unit
    rules.push(rule(
        "unit_fraction",
        "street_number",
        r"^[1-9]/\d+\b",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 7. UNIT — extract from end (before suffix, because unit is outermost)
    // ═══════════════════════════════════════════════════════════════════
    // Unit type + value: APT 4B, UNIT 12, STE 100
    rules.push(rule(
        "unit_type_value",
        "unit",
        r"(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));
    // Pound sign unit: #4B
    rules.push(rule(
        "unit_pound",
        "unit",
        r"#\W*(\w+)\s*$",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));
    // Location: UPPER, LOWER, REAR, etc. at end
    rules.push(rule(
        "unit_location",
        "unit",
        r"\b({unit_location})\s*$",
        Action::Extract,
        Some(Field::Unit),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 8. POST-DIRECTION — direction at end (after unit removed)
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "post_direction",
        "direction",
        r"(?<!^)\b({direction})\s*$",
        Action::Extract,
        Some(Field::PostDirection),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 9. SUFFIX — extract from end (after unit and post-direction removed)
    // ═══════════════════════════════════════════════════════════════════
    // Common suffix at end
    rules.push(rule(
        "suffix_common",
        "suffix",
        r"(?<!^)\b({suffix_common})\s*$",
        Action::Extract,
        Some(Field::Suffix),
        None,
        true,
    ));
    // All suffixes at end (if common didn't match)
    rules.push(rule(
        "suffix_all",
        "suffix",
        r"(?<!^)\b({suffix_all})\s*$",
        Action::Extract,
        Some(Field::Suffix),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 10. PRE-DIRECTION — direction at start of remaining string
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "pre_direction",
        "direction",
        r"^\b({direction})\b(?!$)",
        Action::Extract,
        Some(Field::PreDirection),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 11. STREET NAME STANDARDIZATION — on whatever remains
    // ═══════════════════════════════════════════════════════════════════
    // Table-driven street name abbreviation replacement (MT→MOUNT, FT→FORT, etc.)
    {
        let template = r"\b({street_name_abbr$short})\b";
        let final_template = pattern_overrides
            .get("change_street_name_abbr")
            .cloned()
            .unwrap_or_else(|| template.to_string());
        let final_pattern = expand_template(&final_template, abbr);

        let pairs: Vec<(Regex, String)> = abbr
            .get("street_name_abbr")
            .map(|t| {
                t.short_to_long_pairs()
                    .into_iter()
                    .map(|(short, long)| {
                        let re = Regex::new(&format!(r"\b{}\b", fancy_regex::escape(&short))).unwrap();
                        (re, long)
                    })
                    .collect()
            })
            .unwrap_or_default();

        rules.push(Rule {
            label: "change_street_name_abbr".to_string(),
            group: "street_name".to_string(),
            pattern: Regex::new(&final_pattern)
                .unwrap_or_else(|e| panic!("Bad regex in rule change_street_name_abbr: {}", e)),
            pattern_template: final_template,
            action: Action::Change,
            target: None,
            standardize: None,
            standardize_pairs: pairs,
            skip_if_filled: false,
            enabled: true,
        });
    }
    // ST → SAINT when at start of remaining name and followed by 3+ letter word
    rules.push(rule(
        "change_name_st_to_saint",
        "street_name",
        r"(?:^|\s)ST\b(?=\s[A-Z]{3,})",
        Action::Change,
        None,
        Some((r"(?:^|\b)ST\b(?=\s[A-Z]{3,})", "SAINT")),
        false,
    ));

    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::abbreviations::build_default_tables;

    #[test]
    fn test_expand_template_all_values() {
        let abbr = build_default_tables();
        let expanded = expand_template("{direction}", &abbr);
        assert!(expanded.contains("NORTH"));
        assert!(expanded.contains("NE"));
    }

    #[test]
    fn test_expand_template_short_accessor() {
        let abbr = build_default_tables();
        let expanded = expand_template("{direction$short}", &abbr);
        assert!(expanded.contains("NE"));
        assert!(!expanded.contains("NORTH"));
    }

    #[test]
    fn test_expand_template_state_bounded() {
        let abbr = build_default_tables();
        let expanded = expand_template("{state}", &abbr);
        assert!(expanded.starts_with(r"\b("));
    }

    #[test]
    fn test_expand_template_unit_type_excludes_hash() {
        let abbr = build_default_tables();
        let expanded = expand_template("{unit_type}", &abbr);
        assert!(!expanded.contains("#"));
        assert!(expanded.contains("APARTMENT"));
    }

    #[test]
    fn test_expand_template_regex_quantifiers_preserved() {
        let abbr = build_default_tables();
        let expanded = expand_template(r"\d{5}(?:\W\d{4})?", &abbr);
        assert_eq!(expanded, r"\d{5}(?:\W\d{4})?");
    }

    #[test]
    fn test_expand_template_mixed() {
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
    fn test_build_rules_count() {
        let abbr = build_default_tables();
        let rules = build_rules(&abbr, &HashMap::new());
        assert!(rules.len() > 10);
    }
}
