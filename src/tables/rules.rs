use std::collections::HashMap;

use fancy_regex::Regex;

use crate::address::Field;
use crate::pipeline::{Action, Rule};
use crate::tables::abbreviations::Abbreviations;

/// Build the full ordered pipeline of rules.
/// Extraction order: outside-in, most certain first.
pub fn build_rules(abbr: &Abbreviations, pattern_overrides: &HashMap<String, String>) -> Vec<Rule> {

    let dir_table = abbr.get("direction").unwrap();
    let suffix_table = abbr.get("all_suffix").unwrap();
    let common_suffix_table = abbr.get("common_suffix").unwrap();
    let unit_type_table = abbr.get("unit_type").unwrap();
    let unit_loc_table = abbr.get("unit_location").unwrap();
    let state_table = abbr.get("state").unwrap();

    let b_state = state_table.bounded_regex();

    let nb_dir: String = dir_table.all_values().join("|");
    let nb_common_suffix: String = common_suffix_table.all_values().join("|");
    let nb_all_suffix: String = suffix_table.all_values().join("|");
    let nb_unit_type: String = unit_type_table
        .all_values()
        .iter()
        .filter(|v| **v != "#")
        .cloned()
        .collect::<Vec<_>>()
        .join("|");
    let nb_unit_loc: String = unit_loc_table.all_values().join("|");

    let mut table_values: HashMap<&str, &str> = HashMap::new();
    table_values.insert("direction", &nb_dir);
    table_values.insert("common_suffix", &nb_common_suffix);
    table_values.insert("all_suffix", &nb_all_suffix);
    table_values.insert("unit_type", &nb_unit_type);
    table_values.insert("unit_location", &nb_unit_loc);
    table_values.insert("state", &b_state);

    // Closure captures shared state — each call site only passes rule-specific args.
    let rule = |label: &str, group: &str, pattern: &str, pattern_template: &str,
                action: Action, target: Option<Field>,
                standardize: Option<(&str, &str)>, skip_if_filled: bool| -> Rule {
        let (final_pattern, final_template) =
            if let Some(override_template) = pattern_overrides.get(label) {
                let mut expanded = override_template.clone();
                for (name, values) in &table_values {
                    expanded = expanded.replace(&format!("{{{}}}", name), values);
                }
                (expanded, override_template.clone())
            } else {
                (pattern.to_string(), pattern_template.to_string())
            };

        let std = standardize.map(|(m, r)| (Regex::new(m).unwrap(), r.to_string()));
        Rule {
            label: label.to_string(),
            group: group.to_string(),
            pattern: Regex::new(&final_pattern)
                .unwrap_or_else(|e| panic!("Bad regex in rule {}: {}", label, e)),
            pattern_template: final_template,
            action,
            target,
            standardize: std,
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
        r"(?i)^(N/?A|NULL|NAN|MISSING|NONE|UNKNOWN|NO ADDRESS)$",
        r"(?i)^(N/?A|NULL|NAN|MISSING|NONE|UNKNOWN|NO ADDRESS)$",
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
        &format!(
            r",\s*([A-Z][A-Z ]+)\W+{}\W+(\d{{5}}(?:\W\d{{4}})?)(?:\s*US)?$",
            b_state
        ),
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
        &format!(r"\b({nb_common_suffix})({nb_unit_type})\b"),
        r"\b({common_suffix})({unit_type})\b",
        Action::Change,
        None,
        Some((&format!(r"({nb_common_suffix})({nb_unit_type})"), "$1 $2")),
        false,
    ));

    // Saint vs Street: "123 N ST JUDE" → "123 N SAINT JUDE"
    // Condition: after number + optional direction, "ST" before a 3+ letter word
    // that is NOT a suffix/unit_type/unit_location.
    rules.push(rule(
        "change_st_to_saint",
        "pre_check",
        &format!(
            r"^(\d{{1,6}}\s(?:(?:{nb_dir})\s)?)ST\s(?!(?:{nb_unit_loc}|{nb_unit_type}|{nb_all_suffix})\b)([A-Z]{{3,20}})"
        ),
        r"^(\d{1,6}\s(?:(?:{direction})\s)?)ST\s(?!(?:{unit_location}|{unit_type}|{all_suffix})\b)([A-Z]{3,20})",
        Action::Change,
        None,
        Some((
            &format!(r"^(\d{{1,6}}\s(?:(?:{nb_dir})\s)?)ST\s(?!(?:{nb_unit_loc}|{nb_unit_type}|{nb_all_suffix})\b)([A-Z]{{3,20}})"),
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
        &format!(r"^(?:(?:[A-Z\W]+\s)+(?=(?:{nb_dir})\s\d))|^(?:(?:[A-Z\W]+\s)+(?=\d))"),
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
        &format!(
            r"(?:\b({nb_unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$"
        ),
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
        &format!(r"\b({nb_unit_loc})\s*$"),
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
        &format!(r"(?<!^)\b({nb_dir})\s*$"),
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
        &format!(r"(?<!^)\b({nb_common_suffix})\s*$"),
        r"(?<!^)\b({common_suffix})\s*$",
        Action::Extract,
        Some(Field::Suffix),
        None,
        true,
    ));
    // All suffixes at end (if common didn't match)
    rules.push(rule(
        "suffix_all",
        "suffix",
        &format!(r"(?<!^)\b({nb_all_suffix})\s*$"),
        r"(?<!^)\b({all_suffix})\s*$",
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
        &format!(r"^\b({nb_dir})\b(?!$)"),
        r"^\b({direction})\b(?!$)",
        Action::Extract,
        Some(Field::PreDirection),
        None,
        true,
    ));

    // ═══════════════════════════════════════════════════════════════════
    // 11. STREET NAME STANDARDIZATION — on whatever remains
    // ═══════════════════════════════════════════════════════════════════
    rules.push(rule(
        "change_name_mt_to_mount",
        "street_name",
        r"\bMT\b",
        r"\bMT\b",
        Action::Change,
        None,
        Some((r"\bMT\b", "MOUNT")),
        false,
    ));
    rules.push(rule(
        "change_name_ft_to_fort",
        "street_name",
        r"\bFT\b",
        r"\bFT\b",
        Action::Change,
        None,
        Some((r"\bFT\b", "FORT")),
        false,
    ));
    // ST → SAINT when at start of remaining name and followed by 3+ letter word
    rules.push(rule(
        "change_name_st_to_saint",
        "street_name",
        r"(?:^|\s)ST\b(?=\s[A-Z]{3,})",
        r"(?:^|\s)ST\b(?=\s[A-Z]{3,})",
        Action::Change,
        None,
        Some((r"(?:^|\b)ST\b(?=\s[A-Z]{3,})", "SAINT")),
        false,
    ));

    rules
}
