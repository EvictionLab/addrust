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
    assert!(n >= 1 && n <= 999, "cardinal: n must be 1–999, got {n}");

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

    parts.join(" ")
}

/// Converts 1–999 to English ordinal words (e.g. 342 → "THREE HUNDRED FORTY SECOND").
pub fn ordinal(n: u16) -> String {
    assert!(n >= 1 && n <= 999, "ordinal: n must be 1–999, got {n}");

    let hundreds = (n / 100) as usize;
    let remainder = (n % 100) as usize;

    if hundreds > 0 && remainder == 0 {
        // e.g. 100 → "ONE HUNDREDTH", 200 → "TWO HUNDREDTH"
        return format!("{} HUNDREDTH", ONES[hundreds]);
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
    // trim trailing space that could appear if prefix was non-empty but ordinal_suffix was empty
    prefix.trim().to_string()
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
    assert!(num >= 1 && num <= 999, "fraction: numerator must be 1–999, got {num}");
    assert!(den >= 2 && den <= 999, "fraction: denominator must be 2–999, got {den}");

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

    format!("{num_word} {den_word}")
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
        assert_eq!(cardinal(42), "FORTY TWO");
        assert_eq!(cardinal(99), "NINETY NINE");
    }

    #[test]
    fn test_cardinal_hundreds() {
        assert_eq!(cardinal(100), "ONE HUNDRED");
        assert_eq!(cardinal(101), "ONE HUNDRED ONE");
        assert_eq!(cardinal(999), "NINE HUNDRED NINETY NINE");
        assert_eq!(cardinal(250), "TWO HUNDRED FIFTY");
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
        assert_eq!(ordinal(21), "TWENTY FIRST");
        assert_eq!(ordinal(100), "ONE HUNDREDTH");
        assert_eq!(ordinal(101), "ONE HUNDRED FIRST");
        assert_eq!(ordinal(999), "NINE HUNDRED NINETY NINTH");
    }

    #[test]
    fn test_fraction_half() {
        assert_eq!(fraction(1, 2), "ONE HALF");
        assert_eq!(fraction(5, 2), "FIVE HALF");
    }

    #[test]
    fn test_fraction_regular() {
        assert_eq!(fraction(1, 4), "ONE FOURTH");
        assert_eq!(fraction(3, 4), "THREE FOURTHS");
        assert_eq!(fraction(1, 8), "ONE EIGHTH");
        assert_eq!(fraction(5, 8), "FIVE EIGHTHS");
    }
}
