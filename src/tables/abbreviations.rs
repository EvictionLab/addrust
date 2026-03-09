use std::collections::HashMap;
use std::sync::LazyLock;

/// A single abbreviation entry: short form ↔ long form.
#[derive(Debug, Clone)]
pub struct Abbr {
    pub short: String,
    pub long: String,
}

/// A typed collection of abbreviations with fast lookup.
#[derive(Debug, Clone)]
pub struct AbbrTable {
    pub entries: Vec<Abbr>,
    short_to_long: HashMap<String, String>,
    long_to_short: HashMap<String, String>,
}

impl AbbrTable {
    pub fn new(entries: Vec<Abbr>) -> Self {
        let mut s2l = HashMap::new();
        let mut l2s = HashMap::new();
        for e in &entries {
            // For regex-containing shorts, skip the hashmap (they need regex matching)
            if !has_regex_chars(&e.short) {
                s2l.entry(e.short.clone()).or_insert(e.long.clone());
            }
            if !has_regex_chars(&e.long) {
                l2s.entry(e.long.clone()).or_insert(e.short.clone());
            }
        }
        Self {
            entries,
            short_to_long: s2l,
            long_to_short: l2s,
        }
    }

    /// Look up short → long (exact match, O(1)).
    pub fn to_long(&self, short: &str) -> Option<&str> {
        self.short_to_long.get(short).map(|s| s.as_str())
    }

    /// Look up long → short (exact match, O(1)).
    pub fn to_short(&self, long: &str) -> Option<&str> {
        self.long_to_short.get(long).map(|s| s.as_str())
    }

    /// All unique values (short + long), sorted by length descending.
    /// Used to build alternation regex patterns.
    pub fn all_values(&self) -> Vec<&str> {
        let mut vals: Vec<&str> = self
            .entries
            .iter()
            .flat_map(|e| [e.short.as_str(), e.long.as_str()])
            .collect();
        vals.sort_unstable();
        vals.dedup();
        // Sort by length descending so longer patterns match first
        vals.sort_by(|a, b| b.len().cmp(&a.len()));
        vals
    }

    /// Apply dictionary overrides: remove, override, then add entries.
    pub fn patch(&self, overrides: &crate::config::DictOverrides) -> Self {
        let mut entries = self.entries.clone();

        // Remove: filter out entries matching short or long form
        for remove_val in &overrides.remove {
            let upper = remove_val.to_uppercase();
            entries.retain(|e| e.short != upper && e.long != upper);
        }

        // Override: replace long form for matching short
        for ov in &overrides.override_entries {
            let short_upper = ov.short.to_uppercase();
            let long_upper = ov.long.to_uppercase();
            for entry in &mut entries {
                if entry.short == short_upper {
                    entry.long = long_upper.clone();
                }
            }
        }

        // Add: append new entries
        for add in &overrides.add {
            entries.push(Abbr {
                short: add.short.to_uppercase(),
                long: add.long.to_uppercase(),
            });
        }

        AbbrTable::new(entries)
    }

    /// Build a word-bounded alternation regex: \b(VAL1|VAL2|...)\b
    pub fn bounded_regex(&self) -> String {
        let vals = self.all_values();
        format!(r"\b({})\b", vals.join("|"))
    }

    /// Get (short, long) pairs for abbreviation switching.
    pub fn short_to_long_pairs(&self) -> Vec<(String, String)> {
        // Sort by short length descending (longer matches first)
        let mut pairs: Vec<_> = self
            .entries
            .iter()
            .filter(|e| !has_regex_chars(&e.short))
            .map(|e| (e.short.clone(), e.long.clone()))
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        pairs.dedup();
        pairs
    }

    pub fn long_to_short_pairs(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<_> = self
            .entries
            .iter()
            .filter(|e| !has_regex_chars(&e.long))
            .map(|e| (e.long.clone(), e.short.clone()))
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        pairs.dedup();
        pairs
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
}

// --- Static data definitions ---

fn abbr(short: &str, long: &str) -> Abbr {
    Abbr {
        short: short.to_string(),
        long: long.to_string(),
    }
}

fn build_directions() -> AbbrTable {
    AbbrTable::new(vec![
        abbr("NE", "NORTHEAST"),
        abbr("NW", "NORTHWEST"),
        abbr("SE", "SOUTHEAST"),
        abbr("SW", "SOUTHWEST"),
        abbr("N", "NORTH"),
        abbr("S", "SOUTH"),
        abbr("E", "EAST"),
        abbr("W", "WEST"),
    ])
}

fn build_unit_types() -> AbbrTable {
    AbbrTable::new(vec![
        abbr("APT", "APARTMENT"),
        abbr("UNIT", "UNIT"),
        abbr("STE", "SUITE"),
        abbr("FL", "FLOOR"),
        abbr("FLT", "FLAT"),
        abbr("BLDG", "BUILDING"),
        abbr("RM", "ROOM"),
        abbr("PH", "PENTHOUSE"),
        abbr("TOWNHOUSE", "TOWNHOUSE"),
        abbr("DEPT", "DEPARTMENT"),
        abbr("DUPLEX", "DUPLEX"),
        abbr("ATTIC", "ATTIC"),
        abbr("BSMT", "BASEMENT"),
        abbr("LOT", "LOT"),
        abbr("LVL", "LEVEL"),
        abbr("OFC", "OFFICE"),
        abbr("NUM", "NUMBER"),
        abbr("NO", "NUMBER"),
        abbr("HSE", "HOUSE"),
        abbr("GARAGE", "GARAGE"),
        abbr("CONDO", "CONDO"),
        abbr("TRLR", "TRAILER"),
        abbr("#", "#"),
    ])
}

fn build_unit_locations() -> AbbrTable {
    AbbrTable::new(vec![
        abbr("UPPR", "UPPER"),
        abbr("UP", "UPPER"),
        abbr("LOWR", "LOWER"),
        abbr("LWR", "LOWER"),
        abbr("LW", "LOWER"),
        abbr("FRNT", "FRONT"),
        abbr("FRT", "FRONT"),
        abbr("REAR", "REAR"),
        abbr("BACK", "BACK"),
        abbr("MID", "MIDDLE"),
        abbr("ENTIRE", "ENTIRE"),
        abbr("WHOLE", "WHOLE"),
        abbr("SINGLE", "SINGLE"),
        abbr("DOWN", "DOWN"),
        abbr("RIGHT", "RIGHT"),
        abbr("LEFT", "LEFT"),
        abbr("DOWNSTAIRS", "DOWNSTAIRS"),
        abbr("UPSTAIRS", "UPSTAIRS"),
        abbr("SIDE", "SIDE"),
    ])
}

fn build_states() -> AbbrTable {
    AbbrTable::new(vec![
        abbr("AL", "ALABAMA"), abbr("AK", "ALASKA"), abbr("AZ", "ARIZONA"),
        abbr("AR", "ARKANSAS"), abbr("CA", "CALIFORNIA"), abbr("CO", "COLORADO"),
        abbr("CT", "CONNECTICUT"), abbr("DE", "DELAWARE"), abbr("FL", "FLORIDA"),
        abbr("GA", "GEORGIA"), abbr("HI", "HAWAII"), abbr("ID", "IDAHO"),
        abbr("IL", "ILLINOIS"), abbr("IN", "INDIANA"), abbr("IA", "IOWA"),
        abbr("KS", "KANSAS"), abbr("KY", "KENTUCKY"), abbr("LA", "LOUISIANA"),
        abbr("ME", "MAINE"), abbr("MD", "MARYLAND"), abbr("MA", "MASSACHUSETTS"),
        abbr("MI", "MICHIGAN"), abbr("MN", "MINNESOTA"), abbr("MS", "MISSISSIPPI"),
        abbr("MO", "MISSOURI"), abbr("MT", "MONTANA"), abbr("NE", "NEBRASKA"),
        abbr("NV", "NEVADA"), abbr("NH", "NEW HAMPSHIRE"), abbr("NJ", "NEW JERSEY"),
        abbr("NM", "NEW MEXICO"), abbr("NY", "NEW YORK"), abbr("NC", "NORTH CAROLINA"),
        abbr("ND", "NORTH DAKOTA"), abbr("OH", "OHIO"), abbr("OK", "OKLAHOMA"),
        abbr("OR", "OREGON"), abbr("PA", "PENNSYLVANIA"), abbr("RI", "RHODE ISLAND"),
        abbr("SC", "SOUTH CAROLINA"), abbr("SD", "SOUTH DAKOTA"), abbr("TN", "TENNESSEE"),
        abbr("TX", "TEXAS"), abbr("UT", "UTAH"), abbr("VT", "VERMONT"),
        abbr("VA", "VIRGINIA"), abbr("WA", "WASHINGTON"), abbr("WV", "WEST VIRGINIA"),
        abbr("WI", "WISCONSIN"), abbr("WY", "WYOMING"), abbr("DC", "DISTRICT OF COLUMBIA"),
    ])
}

fn build_usps_suffixes() -> AbbrTable {
    // Parse the embedded USPS CSV
    let csv = include_str!("../../data/usps-street-suffix.csv");
    let mut entries = Vec::new();

    for line in csv.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() >= 3 {
            let long = cols[0].trim();   // primary name (e.g., AVENUE)
            let common = cols[1].trim(); // common variant (e.g., AV)
            let short = cols[2].trim();  // USPS standard (e.g., AVE)

            // long → short mapping (the official USPS abbreviation)
            if long != short {
                entries.push(abbr(short, long));
            }
            // common → short mapping (variant to official)
            if common != short && common != long {
                entries.push(abbr(short, common));
            }
        }
    }

    // Deduplicate
    entries.dedup_by(|a, b| a.short == b.short && a.long == b.long);

    AbbrTable::new(entries)
}

fn build_all_suffixes() -> AbbrTable {
    let csv = include_str!("../../data/usps-street-suffix.csv");
    let mut entries = Vec::new();

    for line in csv.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() >= 3 {
            let long = cols[0].trim();
            let common = cols[1].trim();
            let short = cols[2].trim();

            // Skip TRAILER (used as unit type instead) and HIGHWAY (handled separately)
            if long == "TRAILER" || long == "HIGHWAY" {
                continue;
            }

            // Handle PARKS/WALKS/SPURS → rename to plural form
            let actual_short = if ["PARK", "WALK", "SPUR"].contains(&short)
                && ["PARKS", "WALKS", "SPURS"].contains(&long)
            {
                format!("{}S", short)
            } else {
                short.to_string()
            };

            entries.push(abbr(&actual_short, long));
            if common != long && common != short {
                entries.push(abbr(&actual_short, common));
            }
        }
    }

    // Manual additions (from R package's abbr_more_suffix)
    let extras = vec![
        abbr("BLVD", "BVD"), abbr("BLVD", "BV"), abbr("BLVD", "BLV"), abbr("BLVD", "BL"),
        abbr("CIR", "CI"), abbr("CT", "CRT"), abbr("EXPY", "EX"), abbr("EXPY", "EXPWY"),
        abbr("IS", "ISLD"), abbr("LN", "LA"), abbr("PKWY", "PY"), abbr("PKWY", "PARK WAY"),
        abbr("PKWY", "PKW"), abbr("TER", "TE"), abbr("TRCE", "TR"), abbr("PARK", "PK"),
        abbr("PL", "PLC"), abbr("AVE", "AE"), abbr("DR", "DIRVE"),
    ];
    entries.extend(extras);

    AbbrTable::new(entries)
}

fn build_common_suffixes() -> AbbrTable {
    let common_longs = [
        "DRIVE", "LANE", "AVENUE", "ROAD", "STREET", "CIRCLE", "COURT",
        "PLACE", "WAY", "BOULEVARD", "STRAVENUE", "COVE", "LOOP",
    ];

    let all = build_all_suffixes();
    let entries: Vec<Abbr> = all
        .entries
        .into_iter()
        .filter(|e| common_longs.contains(&e.long.as_str()) || common_longs.contains(&e.short.as_str()))
        .collect();

    AbbrTable::new(entries)
}

/// Global abbreviation tables, built once.
pub static ABBR: LazyLock<Abbreviations> = LazyLock::new(|| {
    let mut tables = HashMap::new();
    tables.insert("direction".to_string(), build_directions());
    tables.insert("unit_type".to_string(), build_unit_types());
    tables.insert("unit_location".to_string(), build_unit_locations());
    tables.insert("state".to_string(), build_states());
    tables.insert("usps_suffix".to_string(), build_usps_suffixes());
    tables.insert("all_suffix".to_string(), build_all_suffixes());
    tables.insert("common_suffix".to_string(), build_common_suffixes());
    Abbreviations { tables }
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DictEntry, DictOverrides};

    #[test]
    fn test_patch_add() {
        let table = AbbrTable::new(vec![abbr("ST", "STREET")]);
        let overrides = DictOverrides {
            add: vec![DictEntry { short: "PSGE".into(), long: "PASSAGE".into() }],
            remove: vec![],
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert!(patched.to_long("PSGE").is_some());
        assert_eq!(patched.to_long("PSGE"), Some("PASSAGE"));
        assert_eq!(patched.to_long("ST"), Some("STREET"));
    }

    #[test]
    fn test_patch_remove() {
        let table = AbbrTable::new(vec![
            abbr("ST", "STREET"),
            abbr("AVE", "AVENUE"),
        ]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec!["STREET".into()],
            override_entries: vec![],
        };
        let patched = table.patch(&overrides);
        assert!(patched.to_long("ST").is_none());
        assert_eq!(patched.to_long("AVE"), Some("AVENUE"));
    }

    #[test]
    fn test_patch_override() {
        let table = AbbrTable::new(vec![abbr("STE", "SUITE")]);
        let overrides = DictOverrides {
            add: vec![],
            remove: vec![],
            override_entries: vec![DictEntry { short: "STE".into(), long: "SUITE NUMBER".into() }],
        };
        let patched = table.patch(&overrides);
        assert_eq!(patched.to_long("STE"), Some("SUITE NUMBER"));
    }
}
