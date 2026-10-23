//! Plain labels, and labels cut to fit a box. Port of Pele's `src/diagrams/common/fit.ts`.

use std::borrow::Cow;

use crate::text::label::{Label, layout_label, trim};
use crate::text::measure;
use crate::util::chars::is_space;

/// Lays out text the way Mermaid's journey and timeline draw it: as plain text in which only
/// `<br>` is markup, so other tags and icon names show as written instead of being read as
/// formatting.
pub(crate) fn plain_label(raw: Option<&str>, size: f64, max: f64, base: u8) -> Label {
    let text = raw.map(|raw| {
        let text = if raw.contains('<') {
            Cow::Owned(escape_tags(raw))
        } else {
            Cow::Borrowed(raw)
        };
        if text.contains(":fa-") {
            Cow::Owned(text.split(":fa-").collect::<Vec<_>>().join("&colon;fa-"))
        } else {
            text
        }
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
        let word = tail
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if word > 0
            && tail
                .get(word..)
                .is_some_and(|after| after.starts_with("¶ß"))
        {
            return rest.len() - tail.len() + word + "¶ß".len();
        }
    }

    // `&#?\w+;`
    if let Some(tail) = rest.strip_prefix('&') {
        let tail = tail.strip_prefix('#').unwrap_or(tail);
        let word = tail
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
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

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
pub(crate) mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `plainLabel` and `fitLabel` with `bun` and the monospace branch of its
    //! `metricsMeasurer`, never from this port. Results compare FNV-1a hashes of the same
    //! serialization on both sides.

    use super::*;
    use crate::diagrams::common::tokens::tests::{CORPUS, fnv, inputs};

    /// A label as the Pele scripts print it.
    pub(crate) fn label(l: &Label) -> String {
        let lines = l
            .lines
            .iter()
            .map(|spans| {
                spans
                    .iter()
                    .map(|s| {
                        format!(
                            "{}~{}~{}~{}",
                            s.text,
                            s.style,
                            s.width,
                            s.icon.as_deref().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("^")
            })
            .collect::<Vec<_>>()
            .join("¦");
        let widths = l
            .widths
            .iter()
            .map(f64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "L{}|{}|{}|{}|{lines}|{widths}",
            l.size, l.line_height, l.width, l.height
        )
    }

    fn fits(text: &str) -> String {
        let mut out = Vec::new();
        for max in [40.0, 120.0, 400.0] {
            for base in [0, 1] {
                out.push(label(&plain_label(Some(text), 14.0, max, base)));
                out.push(label(&fit_label(Some(text), 14.0, max, base)));
            }
        }
        out.join("\n")
    }

    #[test]
    fn fit_label_matches_pele_on_edge_cases() {
        let cases: &[(&str, u32)] = &[
            ("", 0x4a76f4ad),
            ("a", 0x4ac7295b),
            ("hello world", 0xc87363bb),
            ("averyveryverylongwordthatcannotfitanywhere", 0x8028bb6b),
            ("src/components/VeryLongFileName.tsx", 0x46757061),
            ("two  spaces   here and\ttab", 0x73cf538c),
            (
                "line one\nline two that is longer\n\nafter blank",
                0xa3fb2210,
            ),
            ("trailing   \nspaces", 0x6940d169),
            ("<b>bold</b> and <br> break <BR/> and <br />x", 0x870f8018),
            ("a <script>x</script> fa:fa-car :fa-x", 0x5aeb03cc),
            ("fa:fa-car drives fa:fa-none far away from here", 0xb947a1f8),
            (
                "**bold** and *italic* text in markdown that wraps",
                0x23d4b1db,
            ),
            ("&amp; &lt;tag&gt; &#35; ﬂ°°35¶ß ﬂ°lt¶ß entity", 0xf168d2fe),
            ("ünïcödé wörds and 😀 emoji 😀😀😀😀😀😀😀😀😀", 0xf8a779d9),
            ("宽字符宽字符宽字符宽字符宽字符", 0xfff3f798),
            ("e\u{301} combining and zero\u{200b}width", 0x6bd4c33),
            ("   leading spaces", 0xfc9e458f),
            ("x\u{3000}ideographic\u{3000}space", 0x99d4c35a),
            ("List~int~ and Map~K,V~", 0x4f1c598),
        ];
        for &(text, hash) in cases {
            assert_eq!(fnv(&fits(text)), hash, "{text:?}");
        }
    }

    #[test]
    fn fit_label_matches_pele_on_corpus_lines() {
        let expected: &[(&str, u32)] = &[
            ("agentflow", 0x6ba34bc0),
            ("architecture", 0xf1508be7),
            ("block", 0x2b09809d),
            ("c4", 0xd5c24b4),
            ("class", 0xbcbe78fc),
            ("cynefin", 0x1672c9ba),
            ("er", 0xca53c491),
            ("eventmodeling", 0x4da64343),
            ("flowchart", 0xdb326471),
            ("gantt", 0xa19a9263),
            ("git", 0x7e9ca498),
            ("info", 0x3bb5b25c),
            ("ishikawa", 0xd6d04dcd),
            ("journey", 0xe08904e),
            ("kanban", 0x9362d31f),
            ("mindmap", 0xc5d164ea),
            ("packet", 0xe1efa9ab),
            ("pie", 0x94cef0ae),
            ("quadrant", 0x9856de53),
            ("radar", 0xfb91c0d4),
            ("railroad", 0xa7a4ac8a),
            ("requirement", 0x2e826e7f),
            ("sankey", 0xaaaec4b4),
            ("sequence", 0xf969e398),
            ("state", 0x9772c6e2),
            ("swimlane", 0x6cbd7204),
            ("timeline", 0x3f27bf19),
            ("treemap", 0xea43ef57),
            ("treeview", 0x79bf04af),
            ("usecase", 0x9975789e),
            ("venn", 0xba73f2a8),
            ("wardley", 0x607b9eeb),
            ("xychart", 0xc89aca12),
        ];
        for (&(name, hash), &(file, json)) in expected.iter().zip(CORPUS) {
            assert_eq!(name, file);
            let sources = inputs(json);
            let all: Vec<String> = sources
                .iter()
                .flat_map(|src| src.split('\n'))
                .map(fits)
                .collect();
            assert_eq!(fnv(&all.join("\n\n")), hash, "{name}");
        }
    }

    #[test]
    fn plain_label_without_text_is_empty() {
        assert_eq!(label(&plain_label(None, 14.0, 40.0, 0)), "L0|0|0|0||");
        assert_eq!(label(&fit_label(None, 14.0, 40.0, 0)), "L0|0|0|0||");
    }
}
