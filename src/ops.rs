use fancy_regex::Regex;

/// Extract a pattern from `source`, remove it, trim whitespace,
/// and clean up any non-word characters left at the boundaries.
/// Returns all capture groups (index 0 = full match). Returns None if no match or full match is empty.
pub fn extract_remove(source: &mut String, pattern: &Regex) -> Option<Vec<Option<String>>> {
    let caps = pattern.captures(source.as_str()).ok()??;
    let full_match = caps.get(0)?;
    let start = full_match.start();
    let end = full_match.end();

    // Collect all groups
    let groups: Vec<Option<String>> = (0..caps.len())
        .map(|i| caps.get(i).map(|m| m.as_str().trim().to_string()))
        .collect();

    source.replace_range(start..end, "");
    squish(source);
    trim_nonword_boundaries(source);

    // Return None if full match was empty
    if groups[0].as_ref().is_none_or(|s| s.is_empty()) {
        None
    } else {
        Some(groups)
    }
}

/// Strip non-word characters (punctuation, symbols) from the start and end of a string.
/// Preserves internal punctuation — only trims boundaries.
fn trim_nonword_boundaries(s: &mut String) {
    let trimmed = s
        .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
        .trim_end_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
        .to_string();
    let trimmed = trimmed.trim().to_string();
    *s = trimmed;
}

/// Apply a regex replacement to a string (for standardization).
pub fn replace_pattern(source: &mut String, pattern: &Regex, replacement: &str) {
    let result = pattern.replace_all(source.as_str(), replacement).to_string();
    *source = result;
}

/// Collapse internal whitespace and trim.
/// Equivalent to R's `str_squish()`.
pub fn squish(s: &mut String) {
    let trimmed = s.trim().to_string();

    let mut result = String::with_capacity(trimmed.len());
    let mut prev_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(ch);
            prev_space = false;
        }
    }

    *s = result;
}

/// Return None if string is empty or whitespace-only.
pub fn none_if_empty(s: String) -> Option<String> {
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_remove() {
        let re = Regex::new(r"^\d+").unwrap();
        let mut s = "123 MAIN ST".to_string();
        let groups = extract_remove(&mut s, &re);
        assert_eq!(groups.unwrap()[0].as_deref(), Some("123"));
        assert_eq!(s, "MAIN ST");
    }

    #[test]
    fn test_extract_remove_no_match() {
        let re = Regex::new(r"^\d+").unwrap();
        let mut s = "MAIN ST".to_string();
        let groups = extract_remove(&mut s, &re);
        assert!(groups.is_none());
        assert_eq!(s, "MAIN ST");
    }

    #[test]
    fn test_extract_remove_groups() {
        let re = Regex::new(r"(APT)\W*(\d+[A-Z]?)\s*$").unwrap();
        let mut s = "123 MAIN ST APT 4B".to_string();
        let groups = extract_remove(&mut s, &re);
        assert!(groups.is_some());
        let groups = groups.unwrap();
        assert_eq!(groups[0].as_deref(), Some("APT 4B"));
        assert_eq!(groups[1].as_deref(), Some("APT"));
        assert_eq!(groups[2].as_deref(), Some("4B"));
        assert_eq!(s, "123 MAIN ST");
    }

    #[test]
    fn test_squish() {
        let mut s = "  123   MAIN    ST  ".to_string();
        squish(&mut s);
        assert_eq!(s, "123 MAIN ST");
    }
}
