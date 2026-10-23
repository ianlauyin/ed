//! Mermaid's box-drawing preprocessor: turns `├──`, `└──`, `│` (and the heavy variants) into the
//! indentation the grammar reads. Port of Pele's `src/diagrams/treeview/boxDrawing.ts`.

use crate::error::{Code, Error, push_u32};
use crate::svg::builder::js_round;
use crate::text::label::{trim, trim_end};
use crate::util::chars::{is_blank, is_space};

/// `[─━│┃└┗├┣]`.
fn is_box(c: char) -> bool {
    matches!(c, '─' | '━' | '│' | '┃' | '└' | '┗' | '├' | '┣')
}

/// `[└┗├┣]`.
fn is_branch(c: char) -> bool {
    matches!(c, '└' | '┗' | '├' | '┣')
}

/// `[─━]`.
fn is_dash(c: char) -> bool {
    matches!(c, '─' | '━')
}

/// `^[\s│┃]+$`.
fn decoration_only(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|c| is_space(u32::from(c)) || matches!(c, '│' | '┃'))
}

/// `^[\s─━│┃└┗├┣]+$`.
fn box_only(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| is_space(u32::from(c)) || is_box(c))
}

/// `^\s*(title[\t ]|accTitle[\t ]*:|accDescr[\t ]*[:{])`.
fn metadata_line(line: &str) -> bool {
    let rest = line.trim_start_matches(|c: char| is_space(u32::from(c)));
    let after_blanks = |word: &str, then: &[u8]| {
        rest.strip_prefix(word).is_some_and(|tail| {
            let tail = tail.trim_start_matches([' ', '\t']);
            tail.as_bytes().first().is_some_and(|b| then.contains(b))
        })
    };
    rest.strip_prefix("title").is_some_and(|tail| {
        tail.as_bytes()
            .first()
            .is_some_and(|&b| is_blank(u32::from(b)))
    }) || after_blanks("accTitle", b":")
        || after_blanks("accDescr", b":{")
}

/// `^\s*%%`.
fn comment_line(line: &str) -> bool {
    line.trim_start_matches(|c: char| is_space(u32::from(c)))
        .starts_with("%%")
}

/// A line that goes through unchanged: blank, a comment, or a title or accessibility statement.
fn passes(line: &str) -> bool {
    trim(line).is_empty() || comment_line(line) || metadata_line(line)
}

/// `line.replace(/\t/g, '    ')`.
fn untab(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        if c == '\t' {
            out.push_str("    ");
        } else {
            out.push(c);
        }
    }
    out
}

/// The first branch character of `line`: its UTF-16 index and UTF-8 offset.
fn branch_at(line: &str) -> Option<(usize, usize)> {
    let mut units = 0;
    for (at, c) in line.char_indices() {
        if is_branch(c) {
            return Some((units, at));
        }
        units += c.len_utf16();
    }
    None
}

/// Pele's `inferSegmentWidth`: where the first branch not at a line's start is, or 4.
fn infer_segment_width(content: &[String]) -> usize {
    content
        .iter()
        .find_map(|line| {
            branch_at(line)
                .map(|(index, _)| index)
                .filter(|&index| index > 0)
        })
        .unwrap_or(4)
}

/// The text after preprocessing, and each output line's original line, both from 1. The map is
/// empty when the text is unchanged.
#[derive(Clone, Debug, Default)]
pub(crate) struct Preprocessed<'a> {
    /// Output line number to original line number, in output order.
    pub(crate) line_map: Vec<(u32, u32)>,
    /// The text the grammar reads.
    pub(crate) text: std::borrow::Cow<'a, str>,
}

impl Preprocessed<'_> {
    /// The original line of output line `line`.
    pub(crate) fn original(&self, line: u32) -> Option<u32> {
        self.line_map
            .iter()
            .find(|(out, _)| *out == line)
            .map(|&(_, original)| original)
    }
}

/// `message.replace(/\bline\s+(\d+)\b/gi, ...)`: each mention of a line the map knows names the
/// original line instead.
pub(crate) fn remap_error_lines(message: &str, pre: &Preprocessed<'_>) -> String {
    let bytes = message.as_bytes();
    let word = |at: usize| {
        bytes
            .get(at)
            .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
    };
    let mut out = String::with_capacity(message.len());
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        let hit = bytes
            .get(at..at + 4)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"line"))
            && (at == 0 || !word(at - 1));
        if !hit {
            at += 1;
            continue;
        }

        let mut digits = at + 4;
        while let Some(c) = message.get(digits..).and_then(|rest| rest.chars().next())
            && is_space(u32::from(c))
        {
            digits += c.len_utf8();
        }
        let mut end = digits;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if digits == at + 4 || end == digits || word(end) {
            at += 1;
            continue;
        }

        let original = message
            .get(digits..end)
            .and_then(|text| text.parse::<u32>().ok())
            .and_then(|line| pre.original(line));
        if let Some(original) = original {
            out.push_str(message.get(copied..at).unwrap_or(""));
            out.push_str("line ");
            push_u32(&mut out, original);
            copied = end;
        }
        at = end;
    }
    out.push_str(message.get(copied..).unwrap_or(""));
    out
}

/// A syntax error on original line `line`.
#[cold]
fn fail(line: usize, message: &str) -> Error {
    let line = u32::try_from(line).unwrap_or(u32::MAX);
    let mut text = String::from("Line ");
    push_u32(&mut text, line);
    text.push_str(": ");
    text.push_str(message);
    let mut error = Error::new(Code::Syntax, text);
    error.line = line;
    error.r#type = Some("treeView");
    error
}

/// Rewrites box-drawing input as indented lines, as Pele's `preprocessBoxDrawing` does.
///
/// # Errors
///
/// Returns a syntax error for a branch with no name after it, or an indented line without
/// box-drawing characters.
pub(crate) fn preprocess_box_drawing(input: &str) -> Result<Preprocessed<'_>, Error> {
    let unchanged = || Preprocessed {
        line_map: Vec::new(),
        text: std::borrow::Cow::Borrowed(input),
    };
    if !input.contains(is_box) {
        return Ok(unchanged());
    }
    let lines: Vec<&str> = input.split('\n').collect();

    let Some(keyword) = lines.iter().position(|line| trim(line) == "treeView-beta") else {
        return Ok(unchanged());
    };
    let rest = lines.get(keyword + 1..).unwrap_or(&[]);

    let content: Vec<String> = rest
        .iter()
        .filter(|line| !passes(line) && !decoration_only(line))
        .map(|line| untab(line))
        .collect();
    if !content.iter().any(|line| line.contains(is_box)) {
        return Ok(unchanged());
    }

    let segment = infer_segment_width(&content);
    let mut out: Vec<String> = Vec::new();
    let mut line_map = Vec::new();
    let mut keep = |out: &mut Vec<String>, text: String, index: usize| {
        out.push(text);
        line_map.push((
            u32::try_from(out.len()).unwrap_or(u32::MAX),
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        ));
    };

    for (i, line) in lines.iter().enumerate().take(keyword + 1) {
        keep(&mut out, (*line).to_owned(), i);
    }
    for (i, &line) in lines.iter().enumerate().skip(keyword + 1) {
        if passes(line) {
            keep(&mut out, line.to_owned(), i);
            continue;
        }
        if decoration_only(line) {
            continue;
        }

        let normalized = untab(line);
        if let Some((index, at)) = branch_at(&normalized) {
            #[expect(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "line widths stay far below 2^52, and the depth is a small whole number"
            )]
            let depth = (js_round(index as f64 / segment as f64) + 1.0) as usize;
            let tail = normalized.get(at..).unwrap_or("");
            let mut chars = tail.chars();
            chars.next();
            let tail = chars.as_str().trim_start_matches(is_dash);
            let content = trim_end(tail.trim_start_matches(' '));
            if content.is_empty() {
                return Err(fail(
                    i + 1,
                    "Empty node — expected a filename or directory name after the box-drawing prefix",
                ));
            }
            let mut text = "    ".repeat(depth);
            text.push_str(content);
            keep(&mut out, text, i);
        } else if box_only(&normalized) {
        } else if normalized.contains(is_box) {
            // A name that happens to contain a box character, such as "Section ─ A.txt", is a
            // root item.
            keep(&mut out, line.to_owned(), i);
        } else if normalized
            .chars()
            .next()
            .is_some_and(|c| is_space(u32::from(c)))
        {
            return Err(fail(
                i + 1,
                "Unexpected indentation without box-drawing characters. In box-drawing format, use ├── or └── prefixes for indented nodes.",
            ));
        } else {
            keep(&mut out, line.to_owned(), i);
        }
    }
    Ok(Preprocessed {
        line_map,
        text: std::borrow::Cow::Owned(out.join("\n")),
    })
}
