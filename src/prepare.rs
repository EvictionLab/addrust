use crate::ops::squish;

/// Prepare an address string for parsing: uppercase and normalize whitespace.
/// Domain-specific cleaning rules are now pipeline steps in steps.toml.
pub fn prepare(input: &str) -> Option<String> {
    let mut s = input.to_uppercase();
    squish(&mut s);
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_basic() {
        assert_eq!(prepare("  hello   world  "), Some("HELLO WORLD".into()));
    }

    #[test]
    fn test_prepare_empty() {
        assert_eq!(prepare(""), None);
    }
}
