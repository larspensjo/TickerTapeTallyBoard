//! Shared text helpers for CSV imports.

/// Normalize a numeric cell into a `Decimal`-parsable string:
/// trims, maps the Unicode minus (U+2212) to ASCII `-`, comma decimal to dot,
/// and strips spaces, NBSP (U+00A0) and narrow NBSP (U+202F) thousands marks.
/// Returns an empty string for a blank cell.
pub fn normalize_decimal(value: &str) -> String {
    value
        .trim()
        .replace('\u{2212}', "-")
        .replace(',', ".")
        .replace([' ', '\u{00a0}', '\u{202f}'], "")
}

#[cfg(test)]
mod tests {
    use super::normalize_decimal;

    #[test]
    fn maps_comma_unicode_minus_and_strips_thousands() {
        assert_eq!(normalize_decimal("1\u{00a0}259,60"), "1259.60");
        assert_eq!(normalize_decimal("\u{2212}504,00"), "-504.00");
        assert_eq!(normalize_decimal("  12,50 "), "12.50");
        assert_eq!(normalize_decimal(""), "");
    }
}
