//! Character classes as Mermaid's lexers test them, over Unicode scalar values.

/// Whitespace as JavaScript's `\s` matches it.
pub(crate) fn is_space(c: u32) -> bool {
    if c < 128 {
        return c == 32 || (9..=13).contains(&c);
    }

    c == 0xa0
        || c == 0x1680
        || (0x2000..=0x200a).contains(&c)
        || c == 0x2028
        || c == 0x2029
        || c == 0x202f
        || c == 0x205f
        || c == 0x3000
        || c == 0xfeff
}

/// Whether `text` contains `needle`. Unlike `str::contains`, it sets up no searcher, which costs
/// more than the scan over source lines and labels.
pub(crate) fn has(text: &str, needle: &str) -> bool {
    let bytes = text.as_bytes();
    let Some(&first) = needle.as_bytes().first() else {
        return true;
    };
    bytes.iter().enumerate().any(|(at, &byte)| {
        byte == first
            && bytes
                .get(at..)
                .is_some_and(|rest| rest.starts_with(needle.as_bytes()))
    })
}

/// A space or a tab.
pub(crate) fn is_blank(c: u32) -> bool {
    c == 32 || c == 9
}

/// A line terminator: what JavaScript's `.` does not match.
pub(crate) fn is_line_end(c: u32) -> bool {
    c == 10 || c == 13 || c == 0x2028 || c == 0x2029
}

/// An ASCII digit.
pub(crate) fn is_digit(c: u32) -> bool {
    (48..=57).contains(&c)
}

/// An ASCII letter.
pub(crate) fn is_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}

/// An ASCII hexadecimal digit.
pub(crate) fn is_hex(c: u32) -> bool {
    is_digit(c) || (65..=70).contains(&c) || (97..=102).contains(&c)
}

/// A character of JavaScript's `\w`.
pub(crate) fn is_word(c: u32) -> bool {
    is_digit(c) || is_letter(c) || c == 95
}
