//! Generates `data/defaults/suffixes.toml` from the USPS street suffix CSV.
//!
//! Run with: `cargo run --bin generate-suffixes`

use std::collections::HashMap;
use std::fs;

struct AbbrGroup {
    short: String,
    long: String,
    variants: Vec<String>,
    tags: Vec<String>,
}

fn main() {
    let csv_data = fs::read_to_string("data-raw/usps-street-suffix.csv")
        .expect("Failed to read data-raw/usps-street-suffix.csv");

    let mut groups: Vec<AbbrGroup> = Vec::new();
    let mut usps_to_idx: HashMap<String, usize> = HashMap::new();

    for line in csv_data.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 3 {
            continue;
        }
        let primary = cols[0].trim().to_uppercase();
        let variant = cols[1].trim().to_uppercase();
        let usps = cols[2].trim().to_uppercase();

        if usps == "TRAILER" || usps == "HIGHWAY" {
            continue;
        }

        let canonical_short =
            if ["PARK", "WALK", "SPUR", "LOOP"].contains(&usps.as_str())
                && ["PARKS", "WALKS", "SPURS", "LOOPS"].contains(&primary.as_str())
            {
                format!("{}S", usps)
            } else {
                usps.clone()
            };

        if let Some(&idx) = usps_to_idx.get(&canonical_short) {
            let group = &mut groups[idx];
            if variant != group.short
                && variant != group.long
                && !group.variants.contains(&variant)
            {
                group.variants.push(variant.clone());
            }
            if primary != group.short
                && primary != group.long
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
                tags: vec![],
            });
            usps_to_idx.insert(canonical_short, idx);
        }
    }

    // Manual variant overrides
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

    // Mark common suffixes
    let common: &[&str] = &[
        "DR", "LN", "AVE", "RD", "ST", "CIR", "CT", "PL", "WAY", "BLVD", "STRA", "CV", "LOOP",
    ];
    for tag_short in common {
        if let Some(&idx) = usps_to_idx.get(*tag_short) {
            groups[idx].tags.push("common".to_string());
        }
    }

    // Build TOML output
    let mut out = String::from("[suffix]\ngroups = [\n");

    for group in &groups {
        out.push_str("    { short = ");
        out.push_str(&toml_quote(&group.short));
        out.push_str(", long = ");
        out.push_str(&toml_quote(&group.long));
        out.push_str(", variants = [");
        for (i, v) in group.variants.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&toml_quote(v));
        }
        out.push(']');
        if !group.tags.is_empty() {
            out.push_str(", tags = [");
            for (i, t) in group.tags.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&toml_quote(t));
            }
            out.push(']');
        }
        out.push_str(" },\n");
    }

    out.push_str("]\n");

    fs::write("data/defaults/suffixes.toml", &out)
        .expect("Failed to write data/defaults/suffixes.toml");

    println!(
        "Wrote {} suffix groups to data/defaults/suffixes.toml",
        groups.len()
    );
}

/// Quote a string for TOML. Uses single quotes (literal strings) if the value
/// contains no single quotes. Falls back to double quotes with escaping otherwise.
fn toml_quote(s: &str) -> String {
    if s.contains('\'') {
        // Double-quoted with escaping
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        format!("'{}'", s)
    }
}
