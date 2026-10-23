//! Plain labels, and labels cut to fit a box. Port of Pele's `src/diagrams/common/fit.ts`.

use std::borrow::Cow;

use crate::text::label::Label;
use crate::text::label::layout_label;
use crate::text::label::trim;
use crate::text::measure;
use crate::util::chars::is_space;

/// Lays out text the way Mermaid's journey and timeline draw it: as plain text in which only
/// `<br>` is markup, so other tags and icon names show as written instead of being read as
/// formatting.
pub(crate) fn plain_label(raw: Option<&str>, size: f64, max: f64, base: u8) -> Label {
    let text = raw.map(|raw| {
        let text = if raw.contains('<') { Cow::Owned(escape_tags(raw)) } else { Cow::Borrowed(raw) };
        if text.contains(":fa-") { Cow::Owned(text.split(":fa-").collect::<Vec<_>>().join("&colon;fa-")) } else { text }
    });
    // Icon names are escaped above, so no host icon can be drawn.
    layout_label(text.as_deref(), false, size, max, base, None)
}

/// `text.replace(/<(?!br\s*\/?>)/gi, '&lt;')`.
fn escape_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for (at, c) in text.char_indices() {
        if c == '<' && line_break_len(text, at).is_none() {
            out.push_str("&lt;");
        } else {
            out.push(c);
        }
    }
    out
}

/// The length of `<br\s*\/?>` at `at`, with `br` in either case.
fn line_break_len(text: &str, at: usize) -> Option<usize> {
    let rest = text.get(at..)?;
    if !rest.starts_with('<') || !rest.get(1..3)?.eq_ignore_ascii_case("br") {
        return None;
    }
    let mut end = 3;
    while let Some(c) = rest.get(end..).and_then(|tail| tail.chars().next())
        && is_space(u32::from(c))
    {
        end += c.len_utf8();
    }
    if rest.as_bytes().get(end) == Some(&b'/') {
        end += 1;
    }
    (rest.as_bytes().get(end) == Some(&b'>')).then_some(end + 1)
}

/// The length of the piece at `at` that a word can be cut before or after: a line break, an
/// encoded entity, or one character. Mirrors Pele's `RE_UNIT`.
fn unit_len(text: &str, at: usize) -> usize {
    let rest = text.get(at..).unwrap_or("");
    if let Some(len) = line_break_len(text, at) {
        return len;
    }

    // `ﬂ°°?\w+¶ß`, the form `encodeEntities` gives an entity.
    if let Some(tail) = rest.strip_prefix("\u{fb02}°") {
        let tail = tail.strip_prefix('°').unwrap_or(tail);
        let word = tail.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
        if word > 0 && tail.get(word..).is_some_and(|after| after.starts_with("¶ß")) {
            return rest.len() - tail.len() + word + "¶ß".len();
        }
    }

    // `&#?\w+;`
    if let Some(tail) = rest.strip_prefix('&') {
        let tail = tail.strip_prefix('#').unwrap_or(tail);
        let word = tail.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
        if word > 0 && tail.as_bytes().get(word) == Some(&b';') {
            return rest.len() - tail.len() + word + 1;
        }
    }

    rest.chars().next().map_or(1, char::len_utf8)
}

/// [`plain_label`], and for a box of fixed width, also cuts any word too long to fit `max`.
pub(crate) fn fit_label(raw: Option<&str>, size: f64, max: f64, base: u8) -> Label {
    let label = plain_label(raw, size, max, base);
    let Some(raw) = raw else {
        return label;
    };
    if label.width <= max {
        return label;
    }

    let mut out = String::with_capacity(raw.len() + 8);
    let mut width = 0.0;
    let mut at = 0;
    while at < raw.len() {
        let len = unit_len(raw, at);
        let unit = raw.get(at..at + len).unwrap_or("");
        at += len;

        // Pele measures a piece longer than two UTF-16 units as one `x`.
        let long = unit.encode_utf16().count() > 2;
        if trim(unit).is_empty() || (long && unit.starts_with('<')) {
            width = 0.0;
        } else {
            let w = measure::width(if long { "x" } else { unit }, size, base);
            if width > 0.0 && width + w > max {
                out.push('\n');
                width = 0.0;
            }
            width += w;
        }
        out.push_str(unit);
    }
    plain_label(Some(&out), size, max, base)
}
