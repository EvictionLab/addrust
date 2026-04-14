use serde::Deserialize;
use std::collections::HashMap;

/// A group of abbreviation variants with one canonical short/long pair.
/// Short and long are always stored uppercase. Variants are stored as-is
/// (they may contain regex patterns where case matters).
#[derive(Debug, Clone)]
pub struct AbbrGroup {
    pub short: String,
    pub long: String,
    pub variants: Vec<String>,
    pub tags: Vec<String>,
}

impl AbbrGroup {
    /// Create a new group, normalizing short/long to uppercase.
    pub fn new(short: impl Into<String>, long: impl Into<String>, variants: Vec<String>) -> Self {
        Self {
            short: short.into().to_uppercase(),
            long: long.into().to_uppercase(),
            variants,
            tags: vec![],
        }
    }
}

/// A typed collection of abbreviation groups with fast lookup.
#[derive(Debug, Clone)]
pub struct AbbrTable {
    pub groups: Vec<AbbrGroup>,
    /// Optional extraction pattern template for this table.
    pub pattern_template: Option<String>,
    /// Maps every literal value (canonical short, long, non-regex variants) → group index.
    lookup: HashMap<String, usize>,
    /// Compiled regexes for groups with regex variants: (regex, group_index).
    regex_variants: Vec<(fancy_regex::Regex, usize)>,
}

impl AbbrTable {
    /// Construct from the new AbbrGroup model.
    pub fn from_groups(groups: Vec<AbbrGroup>) -> Self {
        let mut lookup = HashMap::new();
        let mut regex_variants = Vec::new();

        // Collect all (value, group_index) pairs, sort longest-first so longer
        // keys take priority in the hashmap (inserted last = wins on collision).
        let mut literal_pairs: Vec<(String, usize)> = Vec::new();
        for (idx, group) in groups.iter().enumerate() {
            literal_pairs.push((group.short.clone(), idx));
            literal_pairs.push((group.long.clone(), idx));
            for v in &group.variants {
                if has_regex_chars(v) {
                    // Strip zero-width assertions for standardize lookup —
                    // they already did their job during pattern matching;
                    // the isolated captured text has no surrounding context.
                    let stripped = strip_zero_width_assertions(v);
                    if stripped.is_empty() { continue; }
                    if let Ok(re) = fancy_regex::Regex::new(&format!("^(?:{})$", stripped)) {
                        regex_variants.push((re, idx));
                    }
                } else {
                    literal_pairs.push((v.clone(), idx));
                }
            }
        }

        // Sort shortest-first so longest keys are inserted last and win
        literal_pairs.sort_by(|a, b| a.0.len().cmp(&b.0.len()));
        for (value, idx) in literal_pairs {
            if !value.is_empty() {
                lookup.insert(value, idx);
            }
        }

        Self {
            groups,
            pattern_template: None,
            lookup,
            regex_variants,
        }
    }

    /// Look up a value in the table. Returns (group_index, canonical_short, canonical_long).
    pub fn standardize(&self, value: &str) -> Option<(usize, &str, &str)> {
        if let Some(&idx) = self.lookup.get(value) {
            let g = &self.groups[idx];
            return Some((idx, &g.short, &g.long));
        }
        for (re, idx) in &self.regex_variants {
            if re.is_match(value).unwrap_or(false) {
                let g = &self.groups[*idx];
                return Some((*idx, &g.short, &g.long));
            }
        }
        None
    }

    /// All matchable values (canonical shorts + longs + variants), deduped, sorted longest-first.
    /// Regex variants included as-is (not escaped). Literals are plain strings.
    pub fn all_match_values(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let mut values = Vec::new();
        for group in &self.groups {
            if seen.insert(group.short.as_str()) {
                values.push(group.short.as_str());
            }
            if seen.insert(group.long.as_str()) {
                values.push(group.long.as_str());
            }
            for v in &group.variants {
                if seen.insert(v.as_str()) {
                    values.push(v.as_str());
                }
            }
        }
        values.sort_by_key(|b| std::cmp::Reverse(b.len()));
        values
    }

    /// True when all long forms are empty — a value-list table (not a short↔long mapping).
    pub fn is_value_list(&self) -> bool {
        !self.groups.is_empty() && self.groups.iter().all(|g| g.long.is_empty())
    }

    /// Construct from (short, long) pairs. Each pair becomes its own group with no variants.
    pub fn from_pairs(pairs: Vec<(&str, &str)>) -> Self {
        let groups = pairs.into_iter()
            .map(|(s, l)| AbbrGroup::new(s, l, vec![]))
            .collect();
        Self::from_groups(groups)
    }

    /// Builder: set pattern_template on an existing table.
    pub fn with_pattern_template(mut self, template: Option<String>) -> Self {
        self.pattern_template = template;
        self
    }

    /// Short → long lookup (finds group, returns canonical long).
    pub fn to_long(&self, short: &str) -> Option<&str> {
        self.standardize(short).map(|(_, _, long)| long)
    }

    /// Value → canonical short lookup (finds group, returns canonical short).
    pub fn to_short(&self, value: &str) -> Option<&str> {
        self.standardize(value).map(|(_, short, _)| short)
    }

    /// All short→long pairs for iteration (used by PerWord standardize and pattern expansion).
    pub fn short_to_long_pairs(&self) -> Vec<(&str, &str)> {
        self.groups.iter()
            .map(|g| (g.short.as_str(), g.long.as_str()))
            .collect()
    }

    /// Bounded regex from all match values (used by pattern expansion).
    pub fn bounded_regex(&self) -> String {
        let values = self.all_match_values();
        let parts: Vec<String> = values.iter().map(|v| {
            if has_regex_chars(v) {
                v.to_string()
            } else {
                fancy_regex::escape(v).to_string()
            }
        }).collect();
        format!(r"(?:{})", parts.join("|"))
    }

    /// Only the short column values, sorted by length descending.
    pub fn short_values(&self) -> Vec<&str> {
        let mut vals: Vec<&str> = self.groups.iter()
            .map(|g| g.short.as_str())
            .collect();
        vals.sort_unstable();
        vals.dedup();
        vals.sort_by_key(|b| std::cmp::Reverse(b.len()));
        vals
    }

    /// All unique values (short + long + variants), sorted by length descending.
    /// Used to build alternation regex patterns. Skips empty strings.
    pub fn all_values(&self) -> Vec<&str> {
        // Delegate to all_match_values, filtering empty strings
        self.all_match_values().into_iter().filter(|v| !v.is_empty()).collect()
    }

    /// Groups that have a specific tag.
    pub fn groups_with_tag(&self, tag: &str) -> Vec<&AbbrGroup> {
        self.groups.iter().filter(|g| g.tags.contains(&tag.to_string())).collect()
    }

    /// All unique values from groups matching a tag, sorted longest-first.
    /// Skips empty strings.
    pub fn all_values_with_tag(&self, tag: &str) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let mut values = Vec::new();
        for group in &self.groups {
            if !group.tags.contains(&tag.to_string()) { continue; }
            if seen.insert(group.short.as_str()) { values.push(group.short.as_str()); }
            if seen.insert(group.long.as_str()) { values.push(group.long.as_str()); }
            for v in &group.variants {
                if seen.insert(v.as_str()) { values.push(v.as_str()); }
            }
        }
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        values.into_iter().filter(|v| !v.is_empty()).collect()
    }

    /// Bounded regex from values matching a tag.
    pub fn bounded_regex_with_tag(&self, tag: &str) -> String {
        let values = self.all_values_with_tag(tag);
        let parts: Vec<String> = values.iter().map(|v| {
            if has_regex_chars(v) {
                v.to_string()
            } else {
                fancy_regex::escape(v).to_string()
            }
        }).collect();
        format!(r"(?:{})", parts.join("|"))
    }

    /// Apply dictionary overrides: remove groups by any value match, then add/merge groups.
    pub fn patch(&self, overrides: &crate::config::DictOverrides) -> Self {
        let mut groups = self.groups.clone();

        // Remove phase: remove groups where any value matches
        if !overrides.remove.is_empty() {
            let remove_set: std::collections::HashSet<String> = overrides.remove.iter()
                .map(|v| v.to_uppercase())
                .collect();
            groups.retain(|g| {
                !remove_set.contains(&g.short)
                    && !remove_set.contains(&g.long)
                    && !g.variants.iter().any(|v| remove_set.contains(v))
            });
        }

        // Add/merge phase: process `add` entries
        let add_iter = overrides.add.iter().map(|e| (e, e.canonical.unwrap_or(false)));

        for (entry, is_canonical) in add_iter {
            // AbbrGroup::new normalizes short/long to uppercase
            let normalized = AbbrGroup::new(&entry.short, &entry.long, entry.variants.clone());
            let short = normalized.short;
            let long = normalized.long;
            let new_variants = normalized.variants;

            // Find existing group by canonical short or long (skip empty-string matches)
            let existing = groups.iter().position(|g| {
                g.short == short
                    || (!long.is_empty() && g.short == long)
                    || (!short.is_empty() && g.long == short)
                    || (!long.is_empty() && g.long == long)
            });

            if let Some(idx) = existing {
                let group = &mut groups[idx];
                // Merge variants
                for v in &new_variants {
                    if *v != group.short && *v != group.long && !group.variants.contains(v) {
                        group.variants.push(v.clone());
                    }
                }
                // Merge tags from entry
                for t in &entry.tags {
                    let upper = t.to_uppercase();
                    if !group.tags.contains(&upper) {
                        group.tags.push(upper);
                    }
                }
                if is_canonical {
                    // Demote old canonical short to variant (if different from new)
                    if group.short != short {
                        let old_short = group.short.clone();
                        if !group.variants.contains(&old_short) {
                            group.variants.push(old_short);
                        }
                        group.short = short;
                    }
                    if group.long != long {
                        let old_long = group.long.clone();
                        if !group.variants.contains(&old_long) {
                            group.variants.push(old_long);
                        }
                        group.long = long;
                    }
                }
            } else {
                // New group
                groups.push(AbbrGroup {
                    short: short.to_uppercase(),
                    long: long.to_uppercase(),
                    variants: new_variants,
                    tags: entry.tags.iter().map(|t| t.to_uppercase()).collect(),
                });
            }
        }

        Self::from_groups(groups)
    }
}

fn has_regex_chars(s: &str) -> bool {
    s.contains(['[', ']', '(', ')', '{', '}', '?', '+', '*', '|', '\\'])
}

/// Strip zero-width assertions (lookahead/lookbehind) from a regex pattern.
/// These assertions reference surrounding context that doesn't exist when
/// testing an isolated captured string in `standardize()`.
fn strip_zero_width_assertions(pattern: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Skip escaped characters
        if chars[i] == '\\' && i + 1 < chars.len() {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        }
        // Detect (?=...) (?!...) (?<=...) (?<!...)
        if chars[i] == '(' && i + 2 < chars.len() && chars[i + 1] == '?' {
            let is_assertion = chars[i + 2] == '=' || chars[i + 2] == '!'
                || (chars[i + 2] == '<'
                    && i + 3 < chars.len()
                    && (chars[i + 3] == '=' || chars[i + 3] == '!'));
            if is_assertion {
                // Skip the entire assertion group (handle nesting)
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '\\' {
                        i += 1; // skip escaped char
                    } else if chars[i] == '(' {
                        depth += 1;
                    } else if chars[i] == ')' {
                        depth -= 1;
                    }
                    i += 1;
                }
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// All abbreviation tables, keyed by type name.
#[derive(Debug, Clone)]
pub struct Abbreviations {
    tables: HashMap<String, AbbrTable>,
}

impl Abbreviations {
    pub fn get(&self, table_type: &str) -> Option<&AbbrTable> {
        self.tables.get(table_type)
    }

    /// Apply config overrides to matching tables, returning a new Abbreviations.
    pub fn patch(&self, dict_overrides: &std::collections::HashMap<String, crate::config::DictOverrides>) -> Self {
        let mut tables = self.tables.clone();
        for (name, overrides) in dict_overrides {
            if let Some(table) = tables.get(name) {
                tables.insert(name.clone(), table.patch(overrides));
            }
        }
        Abbreviations { tables }
    }

    /// List available table names.
    pub fn table_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tables.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }
}

// --- TOML deserialization structs ---

#[derive(Deserialize)]
struct GroupDef {
    short: String,
    #[serde(default)]
    long: String,
    #[serde(default)]
    variants: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct TableDef {
    groups: Vec<GroupDef>,
}

#[derive(Deserialize)]
struct SuffixFileDef {
    suffix: TableDef,
}

/// Load abbreviation tables from a TOML string (tables.toml format).
/// Each top-level key becomes a table name.
pub fn load_tables_from_toml(toml_str: &str) -> HashMap<String, AbbrTable> {
    let raw: HashMap<String, TableDef> = toml::from_str(toml_str)
        .expect("Failed to parse tables TOML");
    raw.into_iter()
        .map(|(name, def)| {
            let groups = def.groups.into_iter()
                .map(|g| AbbrGroup {
                    short: g.short,
                    long: g.long,
                    variants: g.variants,
                    tags: g.tags,
                })
                .collect();
            (name, AbbrTable::from_groups(groups))
        })
        .collect()
}

/// Load suffix table from TOML, producing one "suffix" entry with tags preserved.
pub fn load_suffixes_from_toml(toml_str: &str) -> HashMap<String, AbbrTable> {
    let raw: SuffixFileDef = toml::from_str(toml_str)
        .expect("Failed to parse suffixes TOML");

    let groups: Vec<AbbrGroup> = raw.suffix.groups.into_iter()
        .map(|g| AbbrGroup {
            short: g.short,
            long: g.long,
            variants: g.variants,
            tags: g.tags,
        })
        .collect();

    let mut tables = HashMap::new();
    tables.insert("suffix".to_string(), AbbrTable::from_groups(groups));
    tables
}

/// Build the default abbreviation tables (non-static, for patching).
pub fn load_default_tables() -> Abbreviations {
    let mut tables = load_tables_from_toml(
        include_str!("../../data/defaults/tables.toml")
    );
    tables.extend(load_suffixes_from_toml(
        include_str!("../../data/defaults/suffixes.toml")
    ));
    let (number_cardinal, number_ordinal) = crate::tables::numbers::build_number_tables();
    tables.insert("number_cardinal".to_string(), number_cardinal);
    tables.insert("number_ordinal".to_string(), number_ordinal);
    Abbreviations { tables }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_value_list_true() {
        let table = AbbrTable::from_pairs(vec![
            ("NULL", ""),
            ("NAN", ""),
            ("MISSING", ""),
        ]);
        assert!(table.is_value_list());
    }

    #[test]
    fn test_is_value_list_false() {
        let table = AbbrTable::from_pairs(vec![("ST", "STREET")]);
        assert!(!table.is_value_list());
    }

    #[test]
    fn test_all_values_skips_empty() {
        let table = AbbrTable::from_pairs(vec![
            ("NULL", ""),
            ("NAN", ""),
        ]);
        let vals = table.all_values();
        assert!(vals.contains(&"NULL"));
        assert!(vals.contains(&"NAN"));
        assert!(!vals.contains(&""));
    }

    #[test]
    fn test_na_values_table_exists() {
        let tables = load_default_tables();
        let na = tables.get("na_values").unwrap();
        assert!(na.is_value_list());
        let vals = na.all_values();
        assert!(vals.contains(&"NULL"));
        assert!(vals.contains(&"NO ADDRESS"));
    }

    #[test]
    fn test_street_name_table_exists() {
        let tables = load_default_tables();
        let sna = tables.get("street_name").unwrap();
        assert!(!sna.is_value_list());
        assert_eq!(sna.to_long("MT"), Some("MOUNT"));
        assert_eq!(sna.to_long("FT"), Some("FORT"));
    }

    #[test]
    fn test_short_values() {
        let table = AbbrTable::from_pairs(vec![
            ("ST", "STREET"),
            ("AVE", "AVENUE"),
        ]);
        let shorts = table.short_values();
        // Sorted by length descending
        assert_eq!(shorts, vec!["AVE", "ST"]);
    }

    #[test]
    fn test_table_pattern_field() {
        let abbr = load_default_tables();
        let direction = abbr.get("direction").unwrap();
        assert!(direction.pattern_template.is_none());
    }

    #[test]
    fn test_table_with_pattern() {
        let table = AbbrTable::from_pairs(vec![("N", "NORTH"), ("S", "SOUTH")])
            .with_pattern_template(Some(r"\b({direction})\b".to_string()));
        assert_eq!(table.pattern_template.as_deref(), Some(r"\b({direction})\b"));
        assert_eq!(table.to_long("N"), Some("NORTH"));
    }

    #[test]
    fn test_number_tables_registered() {
        let tables = load_default_tables();
        let cardinal = tables.get("number_cardinal").unwrap();
        assert_eq!(cardinal.to_long("1"), Some("ONE"));
        assert_eq!(cardinal.to_long("42"), Some("FORTYTWO"));
        assert_eq!(cardinal.to_long("999"), Some("NINEHUNDREDNINETYNINE"));

        let ordinal = tables.get("number_ordinal").unwrap();
        assert_eq!(ordinal.to_long("1"), Some("FIRST"));
        assert_eq!(ordinal.to_long("21"), Some("TWENTYFIRST"));
    }

    #[test]
    fn test_abbr_group_standardize_literal() {
        let table = AbbrTable::from_groups(vec![
            AbbrGroup {
                short: "AVE".into(),
                long: "AVENUE".into(),
                variants: vec!["AV".into(), "AVEN".into()],
                tags: vec![],
            },
            AbbrGroup {
                short: "DR".into(),
                long: "DRIVE".into(),
                variants: vec!["DRIV".into()],
                tags: vec![],
            },
        ]);
        // Canonical short
        assert_eq!(table.standardize("AVE"), Some((0, "AVE", "AVENUE")));
        // Canonical long
        assert_eq!(table.standardize("AVENUE"), Some((0, "AVE", "AVENUE")));
        // Variant
        assert_eq!(table.standardize("AV"), Some((0, "AVE", "AVENUE")));
        assert_eq!(table.standardize("AVEN"), Some((0, "AVE", "AVENUE")));
        // Different group
        assert_eq!(table.standardize("DRIV"), Some((1, "DR", "DRIVE")));
        // No match
        assert_eq!(table.standardize("BLVD"), None);
    }

    #[test]
    fn test_abbr_group_standardize_regex_variant() {
        let table = AbbrTable::from_groups(vec![
            AbbrGroup {
                short: "CIR".into(),
                long: "CIRCLE".into(),
                variants: vec!["CIRC".into(), "C[IL]".into()],
                tags: vec![],
            },
        ]);
        // Literal variant
        assert_eq!(table.standardize("CIRC"), Some((0, "CIR", "CIRCLE")));
        // Regex variant matches
        assert_eq!(table.standardize("CI"), Some((0, "CIR", "CIRCLE")));
        assert_eq!(table.standardize("CL"), Some((0, "CIR", "CIRCLE")));
    }

    #[test]
    fn test_abbr_group_longest_match_wins() {
        let table = AbbrTable::from_groups(vec![
            AbbrGroup {
                short: "N".into(),
                long: "NORTH".into(),
                variants: vec![],
                tags: vec![],
            },
            AbbrGroup {
                short: "NE".into(),
                long: "NORTHEAST".into(),
                variants: vec!["N E".into()],
                tags: vec![],
            },
        ]);
        // "N E" should match NE group, not N group
        assert_eq!(table.standardize("N E"), Some((1, "NE", "NORTHEAST")));
        // "N" matches N group
        assert_eq!(table.standardize("N"), Some((0, "N", "NORTH")));
    }

    #[test]
    fn test_all_match_values() {
        let table = AbbrTable::from_groups(vec![
            AbbrGroup {
                short: "AVE".into(),
                long: "AVENUE".into(),
                variants: vec!["AV".into()],
                tags: vec![],
            },
        ]);
        let values = table.all_match_values();
        // Should contain canonical short, long, and variants — sorted longest first
        assert!(values[0] == "AVENUE"); // longest
        assert!(values.contains(&"AVE"));
        assert!(values.contains(&"AV"));
    }

    #[test]
    fn test_patch_add_variant_to_existing_group() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("NE", "NORTHEAST", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NE".into(), long: "NORTHEAST".into(),
                variants: vec!["N E".into(), "NEAST".into()],
                canonical: None,
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("N E"), Some((0, "NE", "NORTHEAST")));
        assert_eq!(patched.standardize("NEAST"), Some((0, "NE", "NORTHEAST")));
    }

    #[test]
    fn test_patch_canonical_override_demotes_old() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("NE", "NORTHEAST", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NEAST".into(), long: "NORTHEAST".into(),
                variants: vec![],
                canonical: Some(true),
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        let result = patched.standardize("NORTHEAST").unwrap();
        assert_eq!(result.1, "NEAST");
        // Old short demoted to variant, still findable
        assert_eq!(patched.standardize("NE").unwrap().1, "NEAST");
    }

    #[test]
    fn test_patch_add_new_group() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "WH".into(), long: "WAREHOUSE".into(),
                variants: vec!["WHSE".into()],
                canonical: None,
                ..Default::default()
            }],
            remove: vec![],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("WHSE"), Some((0, "WH", "WAREHOUSE")));
    }

    #[test]
    fn test_patch_remove_group() {
        use crate::config::{DictEntry, DictOverrides};
        let _ = DictEntry::default(); // verify Default works
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("NE", "NORTHEAST", vec!["N E".into()]),
            AbbrGroup::new("NW", "NORTHWEST", vec![]),
        ]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec!["N E".into()], // matches a variant -> removes the whole NE group
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("NE"), None);
        assert_eq!(patched.standardize("NW"), Some((0, "NW", "NORTHWEST")));
    }

    #[test]
    fn test_standardize_regex_variant_with_lookahead() {
        // Variant FM(?=\s?\d+) should match isolated "FM" in standardize
        // even though the lookahead can't see surrounding context.
        let table = AbbrTable::from_groups(vec![
            AbbrGroup::new("FM RD", "FARM ROAD", vec![r"FM(?=\s?\d+)".into()]),
        ]);
        // Literal lookups
        assert_eq!(table.standardize("FM RD"), Some((0, "FM RD", "FARM ROAD")));
        assert_eq!(table.standardize("FARM ROAD"), Some((0, "FM RD", "FARM ROAD")));
        // Regex variant with lookahead stripped — "FM" should match
        assert_eq!(table.standardize("FM"), Some((0, "FM RD", "FARM ROAD")));
    }

    #[test]
    fn test_strip_zero_width_assertions() {
        assert_eq!(strip_zero_width_assertions(r"FM(?=\s?\d+)"), "FM");
        assert_eq!(strip_zero_width_assertions(r"(?<=\d )NO"), "NO");
        assert_eq!(strip_zero_width_assertions(r"(?<!FOO)BAR(?=\d)"), "BAR");
        assert_eq!(strip_zero_width_assertions(r"HELLO"), "HELLO");
        assert_eq!(strip_zero_width_assertions(r"A\(B"), r"A\(B"); // escaped paren preserved
    }

    #[test]
    fn test_load_tables_from_toml() {
        let toml_str = r#"
[direction]
groups = [
    { short = "N", long = "NORTH" },
    { short = "S", long = "SOUTH" },
]

[na_values]
groups = [
    { short = "NULL" },
    { short = "NAN" },
]
"#;
        let tables = load_tables_from_toml(toml_str);
        assert_eq!(tables.len(), 2);

        let dir = tables.get("direction").unwrap();
        assert_eq!(dir.to_long("N"), Some("NORTH"));
        assert_eq!(dir.to_long("S"), Some("SOUTH"));

        let na = tables.get("na_values").unwrap();
        assert!(na.is_value_list());
        assert!(na.all_values().contains(&"NULL"));
    }

    #[test]
    fn test_load_suffixes_from_toml() {
        let toml_str = r#"
[suffix]
groups = [
    { short = "AVE", long = "AVENUE", variants = ["AV"], tags = ["common"] },
    { short = "STRA", long = "STRAVENUE" },
    { short = "BLVD", long = "BOULEVARD", tags = ["common"] },
]
"#;
        let tables = load_suffixes_from_toml(toml_str);

        let suffix = tables.get("suffix").unwrap();
        assert_eq!(suffix.groups.len(), 3);
        assert_eq!(suffix.to_long("AV"), Some("AVENUE"));
        assert_eq!(suffix.to_long("STRA"), Some("STRAVENUE"));
        assert_eq!(suffix.to_long("BLVD"), Some("BOULEVARD"));

        // Tag filtering works
        let common_groups = suffix.groups_with_tag("common");
        assert_eq!(common_groups.len(), 2);

        let common_values = suffix.all_values_with_tag("common");
        assert!(common_values.contains(&"AVENUE"));
        assert!(common_values.contains(&"BOULEVARD"));
        assert!(!common_values.contains(&"STRAVENUE"));

        // Tags preserved on groups
        assert_eq!(suffix.groups[0].tags, vec!["common"]);
        assert!(suffix.groups[1].tags.is_empty());
        assert_eq!(suffix.groups[2].tags, vec!["common"]);
    }

}
