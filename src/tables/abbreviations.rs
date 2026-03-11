use std::collections::HashMap;
use std::sync::LazyLock;

/// A group of abbreviation variants with one canonical short/long pair.
#[derive(Debug, Clone)]
pub struct AbbrGroup {
    pub short: String,
    pub long: String,
    pub variants: Vec<String>,
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
            literal_pairs.push((group.short.to_uppercase(), idx));
            literal_pairs.push((group.long.to_uppercase(), idx));
            for v in &group.variants {
                if has_regex_chars(v) {
                    // Compile as full-match regex
                    if let Ok(re) = fancy_regex::Regex::new(&format!("^(?:{})$", v)) {
                        regex_variants.push((re, idx));
                    }
                } else {
                    literal_pairs.push((v.to_uppercase(), idx));
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
        let upper = value.to_uppercase();
        // Tier 1: hashmap lookup
        if let Some(&idx) = self.lookup.get(&upper) {
            let g = &self.groups[idx];
            return Some((idx, &g.short, &g.long));
        }
        // Tier 2: regex fallback
        for (re, idx) in &self.regex_variants {
            if re.is_match(&upper).unwrap_or(false) {
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
        values.sort_by(|a, b| b.len().cmp(&a.len()));
        values
    }

    /// True when all long forms are empty — a value-list table (not a short↔long mapping).
    pub fn is_value_list(&self) -> bool {
        !self.groups.is_empty() && self.groups.iter().all(|g| g.long.is_empty())
    }

    /// Construct from (short, long) pairs. Each pair becomes its own group with no variants.
    pub fn from_pairs(pairs: Vec<(&str, &str)>) -> Self {
        let groups = pairs.into_iter()
            .map(|(s, l)| AbbrGroup {
                short: s.to_uppercase(),
                long: l.to_uppercase(),
                variants: vec![],
            })
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
        vals.sort_by(|a, b| b.len().cmp(&a.len()));
        vals
    }

    /// All unique values (short + long + variants), sorted by length descending.
    /// Used to build alternation regex patterns. Skips empty strings.
    pub fn all_values(&self) -> Vec<&str> {
        // Delegate to all_match_values, filtering empty strings
        self.all_match_values().into_iter().filter(|v| !v.is_empty()).collect()
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
                    && !g.variants.iter().any(|v| remove_set.contains(&v.to_uppercase()))
            });
        }

        // Add/merge phase: process `add` entries, then deprecated `override` entries
        // (override entries are treated as canonical adds for backward compat)
        let add_iter = overrides.add.iter().map(|e| (e, e.canonical.unwrap_or(false)));
        let override_iter = overrides.override_entries.iter().map(|e| (e, true));

        for (entry, is_canonical) in add_iter.chain(override_iter) {
            let short = entry.short.to_uppercase();
            let long = entry.long.to_uppercase();
            let new_variants: Vec<String> = entry.variants.iter()
                .map(|v| v.to_uppercase())
                .collect();

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
                    short,
                    long,
                    variants: new_variants,
                });
            }
        }

        Self::from_groups(groups)
    }
}

fn has_regex_chars(s: &str) -> bool {
    s.contains(['[', ']', '(', ')', '{', '}', '?', '+', '*', '|', '\\'])
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

// --- Static data definitions ---

fn build_directions() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec![] },
        AbbrGroup { short: "NW".into(), long: "NORTHWEST".into(), variants: vec![] },
        AbbrGroup { short: "SE".into(), long: "SOUTHEAST".into(), variants: vec![] },
        AbbrGroup { short: "SW".into(), long: "SOUTHWEST".into(), variants: vec![] },
        AbbrGroup { short: "N".into(), long: "NORTH".into(), variants: vec![] },
        AbbrGroup { short: "S".into(), long: "SOUTH".into(), variants: vec![] },
        AbbrGroup { short: "E".into(), long: "EAST".into(), variants: vec![] },
        AbbrGroup { short: "W".into(), long: "WEST".into(), variants: vec![] },
    ])
}

fn build_unit_types() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "APT".into(), long: "APARTMENT".into(), variants: vec![] },
        AbbrGroup { short: "UNIT".into(), long: "UNIT".into(), variants: vec![] },
        AbbrGroup { short: "STE".into(), long: "SUITE".into(), variants: vec![] },
        AbbrGroup { short: "FL".into(), long: "FLOOR".into(), variants: vec![] },
        AbbrGroup { short: "FLT".into(), long: "FLAT".into(), variants: vec![] },
        AbbrGroup { short: "BLDG".into(), long: "BUILDING".into(), variants: vec![] },
        AbbrGroup { short: "RM".into(), long: "ROOM".into(), variants: vec![] },
        AbbrGroup { short: "PH".into(), long: "PENTHOUSE".into(), variants: vec![] },
        AbbrGroup { short: "TOWNHOUSE".into(), long: "TOWNHOUSE".into(), variants: vec![] },
        AbbrGroup { short: "DEPT".into(), long: "DEPARTMENT".into(), variants: vec![] },
        AbbrGroup { short: "DUPLEX".into(), long: "DUPLEX".into(), variants: vec![] },
        AbbrGroup { short: "ATTIC".into(), long: "ATTIC".into(), variants: vec![] },
        AbbrGroup { short: "BSMT".into(), long: "BASEMENT".into(), variants: vec![] },
        AbbrGroup { short: "LOT".into(), long: "LOT".into(), variants: vec![] },
        AbbrGroup { short: "LVL".into(), long: "LEVEL".into(), variants: vec![] },
        AbbrGroup { short: "OFC".into(), long: "OFFICE".into(), variants: vec![] },
        AbbrGroup { short: "NUM".into(), long: "NUMBER".into(), variants: vec!["NO".into()] },
        AbbrGroup { short: "HSE".into(), long: "HOUSE".into(), variants: vec![] },
        AbbrGroup { short: "GARAGE".into(), long: "GARAGE".into(), variants: vec![] },
        AbbrGroup { short: "CONDO".into(), long: "CONDO".into(), variants: vec![] },
        AbbrGroup { short: "TRLR".into(), long: "TRAILER".into(), variants: vec![] },
        AbbrGroup { short: "#".into(), long: "#".into(), variants: vec![] },
    ])
}

fn build_unit_locations() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "UPPR".into(), long: "UPPER".into(), variants: vec!["UP".into()] },
        AbbrGroup { short: "LOWR".into(), long: "LOWER".into(), variants: vec!["LWR".into(), "LW".into()] },
        AbbrGroup { short: "FRNT".into(), long: "FRONT".into(), variants: vec!["FRT".into()] },
        AbbrGroup { short: "REAR".into(), long: "REAR".into(), variants: vec![] },
        AbbrGroup { short: "BACK".into(), long: "BACK".into(), variants: vec![] },
        AbbrGroup { short: "MID".into(), long: "MIDDLE".into(), variants: vec![] },
        AbbrGroup { short: "ENTIRE".into(), long: "ENTIRE".into(), variants: vec![] },
        AbbrGroup { short: "WHOLE".into(), long: "WHOLE".into(), variants: vec![] },
        AbbrGroup { short: "SINGLE".into(), long: "SINGLE".into(), variants: vec![] },
        AbbrGroup { short: "DOWN".into(), long: "DOWN".into(), variants: vec![] },
        AbbrGroup { short: "RIGHT".into(), long: "RIGHT".into(), variants: vec![] },
        AbbrGroup { short: "LEFT".into(), long: "LEFT".into(), variants: vec![] },
        AbbrGroup { short: "DOWNSTAIRS".into(), long: "DOWNSTAIRS".into(), variants: vec![] },
        AbbrGroup { short: "UPSTAIRS".into(), long: "UPSTAIRS".into(), variants: vec![] },
        AbbrGroup { short: "SIDE".into(), long: "SIDE".into(), variants: vec![] },
    ])
}

fn build_states() -> AbbrTable {
    AbbrTable::from_pairs(vec![
        ("AL", "ALABAMA"), ("AK", "ALASKA"), ("AZ", "ARIZONA"),
        ("AR", "ARKANSAS"), ("CA", "CALIFORNIA"), ("CO", "COLORADO"),
        ("CT", "CONNECTICUT"), ("DE", "DELAWARE"), ("FL", "FLORIDA"),
        ("GA", "GEORGIA"), ("HI", "HAWAII"), ("ID", "IDAHO"),
        ("IL", "ILLINOIS"), ("IN", "INDIANA"), ("IA", "IOWA"),
        ("KS", "KANSAS"), ("KY", "KENTUCKY"), ("LA", "LOUISIANA"),
        ("ME", "MAINE"), ("MD", "MARYLAND"), ("MA", "MASSACHUSETTS"),
        ("MI", "MICHIGAN"), ("MN", "MINNESOTA"), ("MS", "MISSISSIPPI"),
        ("MO", "MISSOURI"), ("MT", "MONTANA"), ("NE", "NEBRASKA"),
        ("NV", "NEVADA"), ("NH", "NEW HAMPSHIRE"), ("NJ", "NEW JERSEY"),
        ("NM", "NEW MEXICO"), ("NY", "NEW YORK"), ("NC", "NORTH CAROLINA"),
        ("ND", "NORTH DAKOTA"), ("OH", "OHIO"), ("OK", "OKLAHOMA"),
        ("OR", "OREGON"), ("PA", "PENNSYLVANIA"), ("RI", "RHODE ISLAND"),
        ("SC", "SOUTH CAROLINA"), ("SD", "SOUTH DAKOTA"), ("TN", "TENNESSEE"),
        ("TX", "TEXAS"), ("UT", "UTAH"), ("VT", "VERMONT"),
        ("VA", "VIRGINIA"), ("WA", "WASHINGTON"), ("WV", "WEST VIRGINIA"),
        ("WI", "WISCONSIN"), ("WY", "WYOMING"), ("DC", "DISTRICT OF COLUMBIA"),
    ])
}

fn build_all_suffixes() -> AbbrTable {
    let csv_data = include_str!("../../data/usps-street-suffix.csv");
    let mut groups: Vec<AbbrGroup> = Vec::new();
    let mut usps_to_idx: HashMap<String, usize> = HashMap::new();

    for line in csv_data.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 3 { continue; }
        let primary = cols[0].trim().to_uppercase();
        let variant = cols[1].trim().to_uppercase();
        let usps = cols[2].trim().to_uppercase();

        if usps == "TRAILER" || usps == "HIGHWAY" { continue; }

        // Handle plural forms
        let canonical_short = if ["PARK", "WALK", "SPUR", "LOOP"].contains(&usps.as_str())
            && ["PARKS", "WALKS", "SPURS", "LOOPS"].contains(&primary.as_str())
        {
            format!("{}S", usps)
        } else {
            usps.clone()
        };

        if let Some(&idx) = usps_to_idx.get(&canonical_short) {
            let group = &mut groups[idx];
            // Add variant if it differs from canonical short and long and not already present
            if variant != group.short && variant != group.long
                && !group.variants.contains(&variant)
            {
                group.variants.push(variant.clone());
            }
            // Also add the primary long form as variant if it differs
            if primary != group.short && primary != group.long
                && !group.variants.contains(&primary)
            {
                group.variants.push(primary);
            }
        } else {
            let idx = groups.len();
            let mut variants = vec![];
            if variant != canonical_short && variant != primary {
                variants.push(variant);
            }
            groups.push(AbbrGroup {
                short: canonical_short.clone(),
                long: primary,
                variants,
            });
            usps_to_idx.insert(canonical_short, idx);
        }
    }

    // Add manual overrides from R package's abbr_more_suffix
    let manual_variants: &[(&str, &[&str])] = &[
        ("BLVD", &["BVD", "BV", "BLV", "BL"]),
        ("CIR", &["CI"]),
        ("CT", &["CRT"]),
        ("EXPY", &["EX", "EXPWY"]),
        ("IS", &["ISLD"]),
        ("LN", &["LA"]),
        ("PKWY", &["PY", "PARK WAY", "PKW"]),
        ("TER", &["TE"]),
        ("TRCE", &["TR"]),
        ("PARK", &["PK"]),
        ("PL", &["PLC"]),
        ("AVE", &["AE"]),
        ("DR", &["DIRVE"]),
    ];
    for (usps_short, extras) in manual_variants {
        if let Some(&idx) = usps_to_idx.get(*usps_short) {
            for extra in *extras {
                let e = extra.to_uppercase();
                let group = &mut groups[idx];
                if e != group.short && e != group.long && !group.variants.contains(&e) {
                    group.variants.push(e);
                }
            }
        }
    }

    AbbrTable::from_groups(groups)
}

fn build_na_values() -> AbbrTable {
    AbbrTable::from_pairs(vec![
        ("NULL", ""), ("NAN", ""), ("MISSING", ""), ("NONE", ""),
        ("UNKNOWN", ""), ("NO ADDRESS", ""),
    ])
}

fn build_street_name_abbr() -> AbbrTable {
    AbbrTable::from_pairs(vec![("MT", "MOUNT"), ("FT", "FORT")])
}

fn build_common_suffixes() -> AbbrTable {
    AbbrTable::from_groups(vec![
        AbbrGroup { short: "DR".into(), long: "DRIVE".into(), variants: vec![] },
        AbbrGroup { short: "LN".into(), long: "LANE".into(), variants: vec![] },
        AbbrGroup { short: "AVE".into(), long: "AVENUE".into(), variants: vec![] },
        AbbrGroup { short: "RD".into(), long: "ROAD".into(), variants: vec![] },
        AbbrGroup { short: "ST".into(), long: "STREET".into(), variants: vec![] },
        AbbrGroup { short: "CIR".into(), long: "CIRCLE".into(), variants: vec![] },
        AbbrGroup { short: "CT".into(), long: "COURT".into(), variants: vec![] },
        AbbrGroup { short: "PL".into(), long: "PLACE".into(), variants: vec![] },
        AbbrGroup { short: "WAY".into(), long: "WAY".into(), variants: vec![] },
        AbbrGroup { short: "BLVD".into(), long: "BOULEVARD".into(), variants: vec![] },
        AbbrGroup { short: "STRA".into(), long: "STRAVENUE".into(), variants: vec![] },
        AbbrGroup { short: "CV".into(), long: "COVE".into(), variants: vec![] },
        AbbrGroup { short: "LOOP".into(), long: "LOOP".into(), variants: vec![] },
    ])
}

/// Build the default abbreviation tables (non-static, for patching).
pub fn build_default_tables() -> Abbreviations {
    let mut tables = HashMap::new();
    tables.insert("direction".to_string(), build_directions());
    tables.insert("unit_type".to_string(), build_unit_types());
    tables.insert("unit_location".to_string(), build_unit_locations());
    tables.insert("state".to_string(), build_states());
    tables.insert("suffix_all".to_string(), build_all_suffixes());
    tables.insert("suffix_common".to_string(), build_common_suffixes());
    tables.insert("na_values".to_string(), build_na_values());
    tables.insert("street_name_abbr".to_string(), build_street_name_abbr());
    let (number_cardinal, number_ordinal) = crate::tables::numbers::build_number_tables();
    tables.insert("number_cardinal".to_string(), number_cardinal);
    tables.insert("number_ordinal".to_string(), number_ordinal);
    Abbreviations { tables }
}

/// Global abbreviation tables, built once.
pub static ABBR: LazyLock<Abbreviations> = LazyLock::new(|| {
    let mut tables = HashMap::new();
    tables.insert("direction".to_string(), build_directions());
    tables.insert("unit_type".to_string(), build_unit_types());
    tables.insert("unit_location".to_string(), build_unit_locations());
    tables.insert("state".to_string(), build_states());
    tables.insert("suffix_all".to_string(), build_all_suffixes());
    tables.insert("suffix_common".to_string(), build_common_suffixes());
    tables.insert("na_values".to_string(), build_na_values());
    tables.insert("street_name_abbr".to_string(), build_street_name_abbr());
    let (number_cardinal, number_ordinal) = crate::tables::numbers::build_number_tables();
    tables.insert("number_cardinal".to_string(), number_cardinal);
    tables.insert("number_ordinal".to_string(), number_ordinal);
    Abbreviations { tables }
});

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
        let tables = build_default_tables();
        let na = tables.get("na_values").unwrap();
        assert!(na.is_value_list());
        let vals = na.all_values();
        assert!(vals.contains(&"NULL"));
        assert!(vals.contains(&"NO ADDRESS"));
    }

    #[test]
    fn test_street_name_abbr_table_exists() {
        let tables = build_default_tables();
        let sna = tables.get("street_name_abbr").unwrap();
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
        let abbr = build_default_tables();
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
        let tables = build_default_tables();
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
            },
            AbbrGroup {
                short: "DR".into(),
                long: "DRIVE".into(),
                variants: vec!["DRIV".into()],
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
            },
            AbbrGroup {
                short: "NE".into(),
                long: "NORTHEAST".into(),
                variants: vec!["N E".into()],
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
            AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec![] },
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NE".into(), long: "NORTHEAST".into(),
                variants: vec!["N E".into(), "NEAST".into()],
                canonical: None,
            }],
            remove: vec![],
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("N E"), Some((0, "NE", "NORTHEAST")));
        assert_eq!(patched.standardize("NEAST"), Some((0, "NE", "NORTHEAST")));
    }

    #[test]
    fn test_patch_canonical_override_demotes_old() {
        use crate::config::{DictEntry, DictOverrides};
        let table = AbbrTable::from_groups(vec![
            AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec![] },
        ]);
        let overrides = DictOverrides {
            add: vec![DictEntry {
                short: "NEAST".into(), long: "NORTHEAST".into(),
                variants: vec![],
                canonical: Some(true),
            }],
            remove: vec![],
            override_entries: vec![],
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
            }],
            remove: vec![],
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("WHSE"), Some((0, "WH", "WAREHOUSE")));
    }

    #[test]
    fn test_patch_remove_group() {
        use crate::config::{DictEntry, DictOverrides};
        let _ = DictEntry::default(); // verify Default works
        let table = AbbrTable::from_groups(vec![
            AbbrGroup { short: "NE".into(), long: "NORTHEAST".into(), variants: vec!["N E".into()] },
            AbbrGroup { short: "NW".into(), long: "NORTHWEST".into(), variants: vec![] },
        ]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec!["N E".into()], // matches a variant -> removes the whole NE group
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.standardize("NE"), None);
        assert_eq!(patched.standardize("NW"), Some((0, "NW", "NORTHWEST")));
    }
}
