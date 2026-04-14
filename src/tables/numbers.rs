const ONES: &[&str] = &[
    "", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE", "TEN", "ELEVEN",
    "TWELVE", "THIRTEEN", "FOURTEEN", "FIFTEEN", "SIXTEEN", "SEVENTEEN", "EIGHTEEN", "NINETEEN",
];

const TENS: &[&str] = &[
    "", "", "TWENTY", "THIRTY", "FORTY", "FIFTY", "SIXTY", "SEVENTY", "EIGHTY", "NINETY",
];

const ORDINAL_ONES: &[&str] = &[
    "",
    "FIRST",
    "SECOND",
    "THIRD",
    "FOURTH",
    "FIFTH",
    "SIXTH",
    "SEVENTH",
    "EIGHTH",
    "NINTH",
    "TENTH",
    "ELEVENTH",
    "TWELFTH",
    "THIRTEENTH",
    "FOURTEENTH",
    "FIFTEENTH",
    "SIXTEENTH",
    "SEVENTEENTH",
    "EIGHTEENTH",
    "NINETEENTH",
];

const ORDINAL_TENS: &[&str] = &[
    "",
    "",
    "TWENTIETH",
    "THIRTIETH",
    "FORTIETH",
    "FIFTIETH",
    "SIXTIETH",
    "SEVENTIETH",
    "EIGHTIETH",
    "NINETIETH",
];

/// Converts 1–999 to English cardinal words (e.g. 342 → "THREE HUNDRED FORTY TWO").
pub fn cardinal(n: u16) -> String {
    assert!((1..=999).contains(&n), "cardinal: n must be 1–999, got {n}");

    let hundreds = (n / 100) as usize;
    let remainder = (n % 100) as usize;

    let mut parts: Vec<String> = Vec::new();

    if hundreds > 0 {
        parts.push(ONES[hundreds].to_string());
        parts.push("HUNDRED".to_string());
    }

    if remainder > 0 {
        if remainder < 20 {
            parts.push(ONES[remainder].to_string());
        } else {
            let t = remainder / 10;
            let o = remainder % 10;
            if o == 0 {
                parts.push(TENS[t].to_string());
            } else {
                parts.push(format!("{} {}", TENS[t], ONES[o]));
            }
        }
    }

    parts.join(" ").replace(' ', "")
}

/// Converts 1–999 to English ordinal words (e.g. 342 → "THREEHUNDREDFORTYSECOND").
pub fn ordinal(n: u16) -> String {
    assert!((1..=999).contains(&n), "ordinal: n must be 1–999, got {n}");

    let hundreds = (n / 100) as usize;
    let remainder = (n % 100) as usize;

    if hundreds > 0 && remainder == 0 {
        // e.g. 100 → "ONE HUNDREDTH", 200 → "TWO HUNDREDTH"
        return format!("{}HUNDREDTH", ONES[hundreds]);
    }

    // Build the cardinal prefix for the hundreds part (if any)
    let mut prefix = if hundreds > 0 {
        format!("{} HUNDRED ", ONES[hundreds])
    } else {
        String::new()
    };

    // Now form the ordinal suffix for the remainder
    let ordinal_suffix = if remainder < 20 {
        ORDINAL_ONES[remainder].to_string()
    } else {
        let t = remainder / 10;
        let o = remainder % 10;
        if o == 0 {
            ORDINAL_TENS[t].to_string()
        } else {
            format!("{} {}", TENS[t], ORDINAL_ONES[o])
        }
    };

    prefix.push_str(&ordinal_suffix);
    prefix.replace(' ', "")
}

/// Converts a fraction (numerator/denominator) to English words.
///
/// - Denominator 2: always "HALF" regardless of numerator.
/// - Other denominators: ordinal form, append "S" if numerator > 1.
///
/// Examples: 1/2 → "ONE HALF", 5/2 → "FIVE HALF",
///           1/4 → "ONE FOURTH", 3/4 → "THREE FOURTHS",
///           1/8 → "ONE EIGHTH", 5/8 → "FIVE EIGHTHS".
pub fn fraction(num: u16, den: u16) -> String {
    assert!((1..=999).contains(&num), "fraction: numerator must be 1–999, got {num}");
    assert!((2..=999).contains(&den), "fraction: denominator must be 2–999, got {den}");

    let num_word = cardinal(num);

    let den_word = if den == 2 {
        "HALF".to_string()
    } else {
        let ord = ordinal(den);
        if num > 1 {
            format!("{ord}S")
        } else {
            ord
        }
    };

    format!("{num_word}{den_word}")
}

use super::abbreviations::{AbbrGroup, AbbrTable};

/// Build cardinal and ordinal lookup tables for 1-999.
pub fn build_number_tables() -> (AbbrTable, AbbrTable) {
    let mut cardinal_groups = Vec::with_capacity(999);
    let mut ordinal_groups = Vec::with_capacity(999);

    for n in 1..=999u16 {
        cardinal_groups.push(AbbrGroup {
            short: n.to_string(),
            long: cardinal(n),
            variants: vec![],
            tags: vec![],
        });
        ordinal_groups.push(AbbrGroup {
            short: n.to_string(),
            long: ordinal(n),
            variants: vec![],
            tags: vec![],
        });
    }

    (AbbrTable::from_groups(cardinal_groups), AbbrTable::from_groups(ordinal_groups))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cardinal_ones() {
        assert_eq!(cardinal(1), "ONE");
        assert_eq!(cardinal(9), "NINE");
    }

    #[test]
    fn test_cardinal_teens() {
        assert_eq!(cardinal(11), "ELEVEN");
        assert_eq!(cardinal(19), "NINETEEN");
    }

    #[test]
    fn test_cardinal_tens() {
        assert_eq!(cardinal(20), "TWENTY");
        assert_eq!(cardinal(42), "FORTYTWO");
        assert_eq!(cardinal(99), "NINETYNINE");
    }

    #[test]
    fn test_cardinal_hundreds() {
        assert_eq!(cardinal(100), "ONEHUNDRED");
        assert_eq!(cardinal(101), "ONEHUNDREDONE");
        assert_eq!(cardinal(999), "NINEHUNDREDNINETYNINE");
        assert_eq!(cardinal(250), "TWOHUNDREDFIFTY");
    }

    #[test]
    fn test_ordinal_basic() {
        assert_eq!(ordinal(1), "FIRST");
        assert_eq!(ordinal(2), "SECOND");
        assert_eq!(ordinal(3), "THIRD");
        assert_eq!(ordinal(12), "TWELFTH");
    }

    #[test]
    fn test_ordinal_regular() {
        assert_eq!(ordinal(4), "FOURTH");
        assert_eq!(ordinal(21), "TWENTYFIRST");
        assert_eq!(ordinal(100), "ONEHUNDREDTH");
        assert_eq!(ordinal(101), "ONEHUNDREDFIRST");
        assert_eq!(ordinal(999), "NINEHUNDREDNINETYNINTH");
    }

    #[test]
    fn test_fraction_half() {
        assert_eq!(fraction(1, 2), "ONEHALF");
        assert_eq!(fraction(5, 2), "FIVEHALF");
    }

    #[test]
    fn test_fraction_regular() {
        assert_eq!(fraction(1, 4), "ONEFOURTH");
        assert_eq!(fraction(3, 4), "THREEFOURTHS");
        assert_eq!(fraction(1, 8), "ONEEIGHTH");
        assert_eq!(fraction(5, 8), "FIVEEIGHTHS");
    }
}
