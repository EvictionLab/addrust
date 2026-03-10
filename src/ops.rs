use fancy_regex::Regex;

/// Extract a pattern from `source`, remove it, and trim whitespace.
/// Equivalent to R's `extract_remove_squish`.
pub fn extract_remove(source: &mut String, pattern: &Regex) -> Option<String> {
    let m = pattern.find(source.as_str()).ok()??;
    let extracted = m.as_str().to_string();
    let start = m.start();
    let end = m.end();

    source.replace_range(start..end, "");
    squish(source);

    let extracted = extracted.trim().to_string();
    if extracted.is_empty() {
        None
    } else {
        Some(extracted)
    }
}

/// Extract a pattern from `source`, replace it with a placeholder tag.
/// Equivalent to R's `extract_replace` (positional tags in tidy_address).
pub fn extract_replace(source: &mut String, pattern: &Regex, tag: &str) -> Option<String> {
    let m = pattern.find(source.as_str()).ok()??;
    let extracted = m.as_str().trim().to_string();
    let start = m.start();
    let end = m.end();

    let placeholder = format!("<{}>", tag);
    source.replace_range(start..end, &placeholder);
    squish(source);

    if extracted.is_empty() {
        None
    } else {
        Some(extracted)
    }
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

/// Switch abbreviations using a lookup table.
pub fn switch_abbr(input: &str, pairs: &[(String, String)]) -> String {
    let mut result = input.to_string();
    for (pattern, replacement) in pairs {
        let re_str = format!(r"\b{}\b", fancy_regex::escape(pattern));
        if let Ok(re) = Regex::new(&re_str) {
            result = re.replace_all(&result, replacement.as_str()).to_string();
        }
    }
    result
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
        let extracted = extract_remove(&mut s, &re);
        assert_eq!(extracted, Some("123".to_string()));
        assert_eq!(s, "MAIN ST");
    }

    #[test]
    fn test_extract_remove_no_match() {
        let re = Regex::new(r"^\d+").unwrap();
        let mut s = "MAIN ST".to_string();
        let extracted = extract_remove(&mut s, &re);
        assert_eq!(extracted, None);
        assert_eq!(s, "MAIN ST");
    }

    #[test]
    fn test_extract_replace_placeholder() {
        let re = Regex::new(r"^\d+").unwrap();
        let mut s = "123 MAIN ST".to_string();
        let extracted = extract_replace(&mut s, &re, "street_number");
        assert_eq!(extracted, Some("123".to_string()));
        assert_eq!(s, "<street_number> MAIN ST");
    }

    #[test]
    fn test_squish() {
        let mut s = "  123   MAIN    ST  ".to_string();
        squish(&mut s);
        assert_eq!(s, "123 MAIN ST");
    }

    #[test]
    fn test_switch_abbr() {
        let pairs = vec![
            ("ST".to_string(), "STREET".to_string()),
            ("AVE".to_string(), "AVENUE".to_string()),
        ];
        assert_eq!(switch_abbr("MAIN ST", &pairs), "MAIN STREET");
        assert_eq!(switch_abbr("5TH AVE", &pairs), "5TH AVENUE");
        assert_eq!(switch_abbr("STANTON", &pairs), "STANTON");
    }
}
