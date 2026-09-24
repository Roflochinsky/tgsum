//! String helpers that reproduce the JavaScript semantics the output format was
//! originally defined with: UTF-16 lengths and slices, `String.prototype.trim`
//! and `Number.prototype.toString`.

/// Length in UTF-16 code units, i.e. what JS `s.length` reports.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// JS `s.slice(start, end)` by UTF-16 offsets. A character that straddles a
/// boundary is dropped whole (JS would leave a lone surrogate there).
pub fn utf16_slice(s: &str, start: usize, end: usize) -> &str {
    let mut pos = 0;
    let mut from = s.len();
    let mut to = s.len();
    let mut started = false;
    for (i, c) in s.char_indices() {
        if !started && pos >= start {
            from = i;
            started = true;
        }
        let w = c.len_utf16();
        if pos + w > end {
            to = i;
            break;
        }
        pos += w;
    }
    if from >= to {
        return "";
    }
    &s[from..to]
}

/// JS `String.prototype.trim`: Unicode spaces and line terminators, including
/// U+FEFF but not U+0085 (which Rust's `char::is_whitespace` would strip).
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}'))
}

/// JS `String(number)` for the values a JSON export can hold: integral numbers
/// print without a fraction (and `-0` as `0`), others use the shortest form.
pub fn js_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{}", v as i128)
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_len_counts_surrogate_pairs_twice() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("привет"), 6);
        assert_eq!(utf16_len("👍"), 2);
    }

    #[test]
    fn utf16_slice_matches_js_slice() {
        assert_eq!(utf16_slice("2026-06-18T09:31:00", 0, 10), "2026-06-18");
        assert_eq!(utf16_slice("2026-06-18T09:31:00", 11, 16), "09:31");
        assert_eq!(utf16_slice("short", 11, 16), "");
        assert_eq!(utf16_slice("привет мир", 0, 6), "привет");
        assert_eq!(utf16_slice("abc", 0, 40), "abc");
    }

    #[test]
    fn utf16_slice_never_splits_a_character() {
        // "a👍b": 👍 occupies UTF-16 offsets 1..3.
        assert_eq!(utf16_slice("a👍b", 0, 2), "a");
        assert_eq!(utf16_slice("a👍b", 0, 3), "a👍");
        assert_eq!(utf16_slice("a👍b", 2, 4), "b");
    }

    #[test]
    fn js_trim_matches_js() {
        assert_eq!(js_trim("  hi \n"), "hi");
        assert_eq!(js_trim("\u{feff}hi\u{3000}"), "hi");
        assert_eq!(js_trim("\u{85}hi"), "\u{85}hi");
    }

    #[test]
    fn js_number_formats_like_js() {
        assert_eq!(js_number(0.0), "0");
        assert_eq!(js_number(-0.0), "0");
        assert_eq!(js_number(42.0), "42");
        assert_eq!(js_number(42.5), "42.5");
        assert_eq!(js_number(1_234_567_890_123.0), "1234567890123");
    }
}
