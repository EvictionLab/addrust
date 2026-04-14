/// A segment of a pattern template — either literal text or an alternation group.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternSegment {
    /// Literal regex text (not an alternation group).
    Literal(String),
    /// A table placeholder like {suffix:common}.
    TableRef(String),
    /// An alternation group with individually toggleable alternatives.
    AlternationGroup {
        /// The full text of the group including parens, e.g., `(\d+|[A-Z])`.
        full_text: String,
        /// Individual alternatives.
        alternatives: Vec<Alternative>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alternative {
    pub text: String,
    pub enabled: bool,
}

/// Parse a pattern template into segments.
pub fn parse_pattern(template: &str) -> Vec<PatternSegment> {
    let chars: Vec<char> = template.chars().collect();
    let mut segments = Vec::new();
    let mut i = 0;
    let mut literal_start = 0;

    while i < chars.len() {
        // Check for table reference {name}
        if chars[i] == '{'
            && let Some(end) = find_table_ref(&chars, i) {
                // Flush literal before this
                if i > literal_start {
                    segments.push(PatternSegment::Literal(
                        chars[literal_start..i].iter().collect(),
                    ));
                }
                let name: String = chars[i + 1..end].iter().collect();
                segments.push(PatternSegment::TableRef(name));
                i = end + 1;
                literal_start = i;
                continue;
            }

        // Check for group (...)
        if chars[i] == '('
            && let Some(end) = find_matching_paren(&chars, i) {
                let group_text: String = chars[i..=end].iter().collect();
                let inner = extract_inner(&chars, i, end);

                // Check if inner has top-level alternation (| not inside nested parens/brackets)
                let top_level_alts = split_alternation(&inner);
                if top_level_alts.len() > 1 {
                    // Flush literal before this
                    if i > literal_start {
                        segments.push(PatternSegment::Literal(
                            chars[literal_start..i].iter().collect(),
                        ));
                    }
                    let alternatives = top_level_alts
                        .into_iter()
                        .map(|text| Alternative { text, enabled: true })
                        .collect();
                    segments.push(PatternSegment::AlternationGroup {
                        full_text: group_text,
                        alternatives,
                    });
                    i = end + 1;
                    literal_start = i;
                    continue;
                }
            }

        i += 1;
    }

    // Flush remaining literal
    if literal_start < chars.len() {
        segments.push(PatternSegment::Literal(
            chars[literal_start..].iter().collect(),
        ));
    }

    segments
}

/// Find matching } for a {name} table reference. Returns index of }.
fn find_table_ref(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '}' {
            let name: String = chars[start + 1..i].iter().collect();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == ':') {
                return Some(i);
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Find matching ) for a ( at `start`. Respects nesting and character classes.
fn find_matching_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_class = false;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => { i += 1; } // skip escaped char
            '[' if !in_class => { in_class = true; }
            ']' if in_class => { in_class = false; }
            '(' if !in_class => { depth += 1; }
            ')' if !in_class => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract the inner content of a group (skipping the prefix like `(?:`, `(?!`, etc.)
fn extract_inner(chars: &[char], start: usize, end: usize) -> Vec<char> {
    // Skip opening paren and any group modifier (?:, (?!, (?<!, (?=
    let mut i = start + 1;
    if i < end && chars[i] == '?' {
        i += 1;
        while i < end && chars[i] != ':' && chars[i] != ')' && !chars[i].is_alphanumeric() {
            i += 1;
        }
        if i < end && chars[i] == ':' {
            i += 1;
        }
    }
    chars[i..end].to_vec()
}

/// Split content at top-level | (not inside nested parens, char classes, or table refs).
fn split_alternation(content: &[char]) -> Vec<String> {
    let mut alternatives = Vec::new();
    let mut current_start = 0;
    let mut depth = 0;
    let mut in_class = false;
    let mut i = 0;

    while i < content.len() {
        match content[i] {
            '\\' => { i += 1; } // skip escaped char
            '[' if !in_class => { in_class = true; }
            ']' if in_class => { in_class = false; }
            '(' if !in_class => { depth += 1; }
            ')' if !in_class => { depth -= 1; }
            '{' if !in_class && depth == 0 => {
                // Skip over table ref
                while i < content.len() && content[i] != '}' {
                    i += 1;
                }
            }
            '|' if !in_class && depth == 0 => {
                alternatives.push(content[current_start..i].iter().collect());
                current_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    alternatives.push(content[current_start..].iter().collect());
    alternatives
}

/// Rebuild a pattern template from segments (with disabled alternatives removed).
pub fn rebuild_pattern(segments: &[PatternSegment]) -> String {
    let mut out = String::new();
    for segment in segments {
        match segment {
            PatternSegment::Literal(text) => out.push_str(text),
            PatternSegment::TableRef(name) => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
            PatternSegment::AlternationGroup { full_text, alternatives } => {
                let enabled: Vec<&str> = alternatives
                    .iter()
                    .filter(|a| a.enabled)
                    .map(|a| a.text.as_str())
                    .collect();
                if enabled.is_empty() {
                    // All disabled — keep the group as a never-match
                    out.push_str("(?:$^)");
                } else {
                    // Reconstruct: find the group prefix (e.g., "(", "(?:", "(?!")
                    let prefix = extract_group_prefix(full_text);
                    out.push_str(&prefix);
                    out.push_str(&enabled.join("|"));
                    out.push(')');
                }
            }
        }
    }
    out
}

/// Extract the prefix of a group: "(", "(?:", "(?!", "(?<!".
fn extract_group_prefix(full_text: &str) -> String {
    let chars: Vec<char> = full_text.chars().collect();
    let mut i = 1; // skip opening (
    if i < chars.len() && chars[i] == '?' {
        i += 1;
        while i < chars.len() && (chars[i] == '<' || chars[i] == '!' || chars[i] == '=') {
            i += 1;
        }
        if i < chars.len() && chars[i] == ':' {
            i += 1;
        }
    }
    chars[..i].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_literal() {
        let segments = parse_pattern(r"^\d+\b");
        assert_eq!(segments, vec![PatternSegment::Literal(r"^\d+\b".to_string())]);
    }

    #[test]
    fn test_parse_table_ref() {
        let segments = parse_pattern(r"(?<!^)\b({suffix:common})\s*$");
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], PatternSegment::Literal(r"(?<!^)\b(".to_string()));
        assert_eq!(segments[1], PatternSegment::TableRef("suffix:common".to_string()));
        assert_eq!(segments[2], PatternSegment::Literal(r")\s*$".to_string()));
    }

    #[test]
    fn test_parse_alternation_group() {
        let segments = parse_pattern(r"(\d+|[A-Z])");
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            PatternSegment::AlternationGroup { alternatives, .. } => {
                assert_eq!(alternatives.len(), 2);
                assert_eq!(alternatives[0].text, r"\d+");
                assert_eq!(alternatives[1].text, "[A-Z]");
                assert!(alternatives.iter().all(|a| a.enabled));
            }
            _ => panic!("Expected alternation group"),
        }
    }

    #[test]
    fn test_parse_mixed() {
        let segments = parse_pattern(
            r"(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$"
        );
        // Both groups with | are alternation groups
        let alt_groups: Vec<_> = segments.iter().filter(|s| matches!(s, PatternSegment::AlternationGroup { .. })).collect();
        assert_eq!(alt_groups.len(), 2);
        // First group: (?:\b({unit_type})|#) has 2 alternatives
        match &alt_groups[0] {
            PatternSegment::AlternationGroup { alternatives, .. } => {
                assert_eq!(alternatives.len(), 2);
                assert_eq!(alternatives[0].text, r"\b({unit_type})");
                assert_eq!(alternatives[1].text, "#");
            }
            _ => unreachable!(),
        }
        // Second group: (\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z]) has 4 alternatives
        match &alt_groups[1] {
            PatternSegment::AlternationGroup { alternatives, .. } => {
                assert_eq!(alternatives.len(), 4);
                assert_eq!(alternatives[0].text, r"\d+\W?[A-Z]?");
                assert_eq!(alternatives[1].text, r"[A-Z]\W?\d+");
                assert_eq!(alternatives[2].text, r"\d+");
                assert_eq!(alternatives[3].text, "[A-Z]");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_parse_non_capturing_group_no_alternation() {
        // (?:...) without | should be literal, not an alternation group
        let segments = parse_pattern(r"(?:abc)");
        assert_eq!(segments, vec![PatternSegment::Literal(r"(?:abc)".to_string())]);
    }

    #[test]
    fn test_rebuild_all_enabled() {
        let segments = parse_pattern(r"(\d+|[A-Z])");
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, r"(\d+|[A-Z])");
    }

    #[test]
    fn test_rebuild_with_disabled() {
        let mut segments = parse_pattern(r"(\d+|[A-Z])");
        if let PatternSegment::AlternationGroup { alternatives, .. } = &mut segments[0] {
            alternatives[1].enabled = false;
        }
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, r"(\d+)");
    }

    #[test]
    fn test_rebuild_preserves_table_refs() {
        let segments = parse_pattern(r"(?<!^)\b({suffix:common})\s*$");
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, r"(?<!^)\b({suffix:common})\s*$");
    }

    #[test]
    fn test_parse_table_ref_with_accessor() {
        let segments = parse_pattern(r"\b({street_name$short})\b");
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0], PatternSegment::Literal(r"\b(".to_string()));
        assert_eq!(segments[1], PatternSegment::TableRef("street_name$short".to_string()));
        assert_eq!(segments[2], PatternSegment::Literal(r")\b".to_string()));
    }

    #[test]
    fn test_roundtrip_complex() {
        let template = r"(?:\b({unit_type})|#)\W*(\d+\W?[A-Z]?|[A-Z]\W?\d+|\d+|[A-Z])\s*$";
        let segments = parse_pattern(template);
        let rebuilt = rebuild_pattern(&segments);
        assert_eq!(rebuilt, template);
    }
}
