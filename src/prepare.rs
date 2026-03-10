use fancy_regex::Regex;
use std::sync::LazyLock;

use crate::ops::squish;

struct PrepRule {
    pattern: Regex,
    replacement: &'static str,
}

fn prep(pattern: &str, replacement: &'static str) -> PrepRule {
    PrepRule {
        pattern: Regex::new(pattern).unwrap(),
        replacement,
    }
}

static PREP_RULES: LazyLock<Vec<PrepRule>> = LazyLock::new(|| {
    vec![
        // fix bad ampersand
        prep(r"&AMP;", "&"),
        // dedupe non-word characters
        prep(r"(\W)\1+", "$1"),
        // replace period between letters/numbers with space
        prep(r"([^\s])\.([^\s])", "$1 $2"),
        // remove spaces around hyphens
        prep(r"([A-Z0-9])\s*-\s*([A-Z0-9])", "$1-$2"),
        // add space between number and 2+ letters at start
        prep(r"^(\d+)([A-Z]{2,})", "$1 $2"),
        // remove space between first direction and number
        prep(r"^([NSEW]) (\d+)\b", "${1}${2}"),
        // remove pound sign before ordinal
        prep(r"#(\d+[RNTS][DHT])", "$1"),
        // replace word & word with AND
        prep(r"([A-Z])\s*&\s*([A-Z])", "$1 AND $2"),
        // replace random / with space (not between digits)
        prep(r"(?<!\d)/(?!\d)", " "),
        // remove trailing non-word characters
        prep(r"\W+$", ""),
        // remove periods and apostrophes
        prep(r"[.']+", ""),
        // remove other punctuation
        prep(r#"[;<>$()"]+|`"#, ""),
        // standardize MLK
        prep(
            r"(?:(?:DR|DOCTOR)\W*)?M(?:ARTIN)?\W*L(?:UTHER)?\W*K(?:ING)?(?:\W+(?:JR|JUNIOR))?",
            "MARTIN LUTHER KING",
        ),
    ]
});

/// Prepare an address string for parsing.
pub fn prepare(input: &str) -> Option<String> {
    let mut s = input.to_uppercase();

    for rule in PREP_RULES.iter() {
        s = rule.pattern.replace_all(&s, rule.replacement).to_string();
    }

    squish(&mut s);

    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_basic() {
        assert_eq!(prepare("123 main st"), Some("123 MAIN ST".into()));
    }

    #[test]
    fn test_prepare_periods() {
        assert_eq!(prepare("123 N. Main St."), Some("123 N MAIN ST".into()));
    }

    #[test]
    fn test_prepare_ampersand() {
        assert_eq!(
            prepare("123 Smith & Jones Rd"),
            Some("123 SMITH AND JONES RD".into()),
        );
    }

    #[test]
    fn test_prepare_mlk() {
        assert_eq!(
            prepare("456 Dr. M.L. King Jr. Blvd"),
            Some("456 MARTIN LUTHER KING BLVD".into()),
        );
    }

    #[test]
    fn test_prepare_empty() {
        assert_eq!(prepare(""), None);
    }
}
