//! Lays out label text in lines of styled spans: inline HTML emphasis, markdown emphasis, line
//! breaks, Font Awesome icons, and wrapping. Port of Pele's `src/text/label.ts`.

use std::borrow::Cow;

use crate::svg::builder::js_round;
use crate::text::entities::decode;
use crate::text::icons::IconResolver;
use crate::text::icons::icon_markup;
use crate::text::measure::BOLD;
use crate::text::measure::ITALIC;
use crate::text::measure::{self};
use crate::util::chars::has;
use crate::util::chars::is_space;
use crate::util::chars::is_word;

/// A run of text in one style, or an icon.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Span {
    /// The icon's name, such as `fa:fa-car`, when the span is an icon.
    pub(crate) icon: Option<String>,
    /// Emphasis bits from [`crate::text::measure`].
    pub(crate) style: u8,
    /// The text, with entities decoded. Empty for an icon.
    pub(crate) text: String,
    /// The measured width.
    pub(crate) width: f64,
}

/// A label laid out in lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Label {
    /// Total height: the number of lines times the line height.
    pub(crate) height: f64,
    /// Height of one line.
    pub(crate) line_height: f64,
    /// The spans of each line.
    pub(crate) lines: Vec<Vec<Span>>,
    /// Font size.
    pub(crate) size: f64,
    /// Width of the widest line.
    pub(crate) width: f64,
    /// Width of each line.
    pub(crate) widths: Vec<f64>,
}

/// Line height as a multiple of the font size.
pub(crate) const LINE_HEIGHT: f64 = 1.5;

/// `text` without JavaScript whitespace at either end, as `String.prototype.trim` leaves it.
pub(crate) fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| is_space(u32::from(c)))
}

/// `text` without JavaScript whitespace at its start.
pub(crate) fn trim_start(text: &str) -> &str {
    text.trim_start_matches(|c: char| is_space(u32::from(c)))
}

/// `text` without JavaScript whitespace at its end.
pub(crate) fn trim_end(text: &str) -> &str {
    text.trim_end_matches(|c: char| is_space(u32::from(c)))
}

/// The number of whitespace characters a line starts with.
fn indent(line: &str) -> usize {
    line.chars().take_while(|&c| is_space(u32::from(c))).count()
}

/// Removes the indentation that every non-blank line shares.
fn dedent(text: &str) -> Cow<'_, str> {
    let mut min = usize::MAX;
    for line in text.split('\n') {
        if trim(line).is_empty() {
            continue;
        }
        min = min.min(indent(line));
    }
    if min == usize::MAX || min == 0 {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        // Indentation is whitespace, which is one UTF-16 unit per character, as Pele counts it.
        let skip = min.min(indent(line));
        let at = line.char_indices().nth(skip).map_or(line.len(), |(at, _)| at);
        out.push_str(line.get(at..).unwrap_or(""));
    }
    Cow::Owned(out)
}

/// Adds text to a line, joining the last span when it has the same style.
fn push_text(line: &mut Vec<Span>, text: &str, style: u8) {
    if text.is_empty() {
        return;
    }

    let decoded = decode(text);
    if let Some(last) = line.last_mut()
        && last.style == style
        && last.icon.is_none()
    {
        last.text.push_str(&decoded);
    } else {
        line.push(Span { icon: None, style, text: decoded.into_owned(), width: 0.0 });
    }
}

/// Whether `rest` starts with Pele's `fa[bklrs]?:fa-`, and the length of that prefix.
fn icon_prefix(rest: &[u8]) -> Option<usize> {
    if !rest.starts_with(b"fa") {
        return None;
    }
    if let Some(c) = rest.get(2)
        && b"bklrs".contains(c)
        && rest.get(3..).is_some_and(|tail| tail.starts_with(b":fa-"))
    {
        return Some(7);
    }
    rest.get(2..).is_some_and(|tail| tail.starts_with(b":fa-")).then_some(6)
}

/// The next match of Pele's `/(fa[bklrs]?):fa-([\w-]+)/g` at or after `from`, as byte offsets.
fn find_icon(text: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < bytes.len() {
        let rest = bytes.get(at..).unwrap_or(&[]);
        if let Some(prefix) = icon_prefix(rest) {
            let name = rest
                .get(prefix..)
                .unwrap_or(&[])
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_' || **c == b'-')
                .count();
            if name > 0 {
                return Some((at, at + prefix + name));
            }
            // Without the optional letter the prefix needs `:` where the letter is, so no shorter
            // match starts here.
        }
        at += 1;
    }
    None
}

/// Adds text to a line, turning `fa:fa-name` references the host has icons for into icon spans.
fn push_icons(line: &mut Vec<Span>, text: &str, style: u8, icons: Option<IconResolver<'_>>) {
    if !has(text, ":fa-") {
        push_text(line, text, style);
        return;
    }

    let mut last = 0;
    let mut from = 0;
    while let Some((start, end)) = find_icon(text, from) {
        let icon = text.get(start..end).unwrap_or("");
        push_text(line, text.get(last..start).unwrap_or(""), style);
        last = end;
        from = end;
        if !icon_markup(icons, icon).is_empty() {
            line.push(Span { icon: Some(icon.to_owned()), style, text: String::new(), width: 0.0 });
        } else if text.as_bytes().get(last) == Some(&b' ') {
            // An icon the host does not have leaves no gap, nor the space that set it off from
            // the text.
            last += 1;
        } else if last == text.len()
            && let Some(before) = line.last_mut()
            && before.icon.is_none()
        {
            let trimmed = trim_end(&before.text).len();
            before.text.truncate(trimmed);
        }
    }
    push_text(line, text.get(last..).unwrap_or(""), style);
}

/// Matches Pele's sticky `/<\/?([a-zA-Z][\w-]*)(?:\s[^<>]*)?\/?>/y` at the start of `rest`, and
/// returns the length of the tag and its name.
fn match_tag(rest: &str) -> Option<(usize, &str)> {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }

    let start = if bytes.get(1) == Some(&b'/') { 2 } else { 1 };
    if !bytes.get(start)?.is_ascii_alphabetic() {
        return None;
    }
    let mut end = start + 1;
    while bytes.get(end).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-') {
        end += 1;
    }
    let name = rest.get(start..end)?;

    let after = rest.get(end..)?;
    if after.chars().next().is_some_and(|c| is_space(u32::from(c))) {
        // Attributes run to the first `<` or `>`, and only a `>` closes the tag.
        let close = after.find(['<', '>'])?;
        return (after.as_bytes().get(close) == Some(&b'>')).then_some((end + close + 1, name));
    }

    let close = if bytes.get(end) == Some(&b'/') { end + 1 } else { end };
    (bytes.get(close) == Some(&b'>')).then_some((close + 1, name))
}

/// Splits inline HTML and, for markdown labels, emphasis markers into styled spans.
fn inline(line: &mut Vec<Span>, source: &str, markdown: bool, base: u8, icons: Option<IconResolver<'_>>) {
    let bytes = source.as_bytes();
    let n = source.len();
    let mut style = base;
    let mut marked = 0;
    let mut text = String::new();
    let mut i = 0;
    while let Some(ch) = source.get(i..).and_then(|rest| rest.chars().next()) {
        if ch == '<' {
            if let Some((len, tag)) = match_tag(source.get(i..).unwrap_or("")) {
                let closing = bytes.get(i + 1) == Some(&b'/');
                let bit = if tag.eq_ignore_ascii_case("b") || tag.eq_ignore_ascii_case("strong") {
                    BOLD
                } else if tag.eq_ignore_ascii_case("i") || tag.eq_ignore_ascii_case("em") {
                    ITALIC
                } else {
                    0
                };
                if bit != 0 {
                    push_icons(line, &text, style, icons);
                    text.clear();
                    style = if closing { (style & !bit) | (base & bit) } else { style | bit };
                }
                i += len;
                continue;
            }
        } else if markdown && (ch == '*' || ch == '_') {
            let double = bytes.get(i + 1) == Some(&(ch as u8));
            let bit = if double { BOLD } else { ITALIC };
            let len = if double { 2 } else { 1 };
            let open = marked & bit == 0;
            let prev = source.get(..i).and_then(|head| head.chars().next_back()).unwrap_or(' ');
            let after = source.get(i + len..).unwrap_or("");
            let next = if i + len < n { after.chars().next().unwrap_or(' ') } else { ' ' };
            let mark = if double { source.get(i..i + 2).unwrap_or("") } else { source.get(i..=i).unwrap_or("") };
            let can_open = open && !is_space(u32::from(next)) && after.contains(mark);
            let can_close = !open && !is_space(u32::from(prev));
            let wordy = ch == '_' && is_word(u32::from(prev)) && is_word(u32::from(next));
            if (can_open || can_close) && !wordy {
                push_icons(line, &text, style, icons);
                text.clear();
                marked = if can_open { marked | bit } else { marked & !bit };
                style = if can_open { style | bit } else { (style & !bit) | (base & bit) };
                i += len;
                continue;
            }
        }
        text.push(ch);
        i += ch.len_utf8();
    }
    push_icons(line, &text, style, icons);
}

/// Measures each span of a line and returns the line's width.
fn measure_spans(line: &mut [Span], size: f64) -> f64 {
    let mut total = 0.0;
    for span in line {
        span.width = if span.icon.is_some() { size * 1.25 } else { measure::width(&span.text, size, span.style) };
        total += span.width;
    }
    total
}

/// Ends a wrapped line, trimming the space at its end.
fn commit(current: &mut Vec<Span>, width: &mut f64, size: f64, out: &mut Vec<Vec<Span>>, widths: &mut Vec<f64>) {
    if let Some(last) = current.last_mut()
        && last.icon.is_none()
    {
        let trimmed = trim_end(&last.text).len();
        if trimmed != last.text.len() {
            last.text.truncate(trimmed);
            let w = measure::width(&last.text, size, last.style);
            *width += w - last.width;
            last.width = w;
        }
    }
    out.push(std::mem::take(current));
    widths.push(*width);
    *width = 0.0;
}

/// The pieces Pele's `/\s*\S+\s*|\s+/g` matches: words with the whitespace around them.
fn pieces(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let word = trim_start(rest);
        let end = if word.is_empty() {
            rest.len()
        } else {
            let tail = word.trim_start_matches(|c: char| !is_space(u32::from(c)));
            rest.len() - trim_start(tail).len()
        };
        let (piece, tail) = rest.split_at(end);
        rest = tail;
        Some(piece)
    })
}

/// Wraps a line at whitespace so each part fits in `max`, adding the parts to `out`.
fn wrap(line: &[Span], size: f64, max: f64, out: &mut Vec<Vec<Span>>, widths: &mut Vec<f64>) {
    let mut current: Vec<Span> = Vec::new();
    let mut width = 0.0;
    for span in line {
        if span.icon.is_some() {
            let w = size * 1.25;
            if width > 0.0 && width + w > max {
                commit(&mut current, &mut width, size, out, widths);
            }
            current.push(Span { width: w, ..span.clone() });
            width += w;
            continue;
        }

        for piece in pieces(&span.text) {
            let mut part = piece;
            if width > 0.0 && width + measure::width(trim_end(part), size, span.style) > max {
                commit(&mut current, &mut width, size, out, widths);
                part = trim_start(part);
            }
            if let Some(last) = current.last_mut()
                && last.style == span.style
                && last.icon.is_none()
            {
                last.text.push_str(part);
                last.width = measure::width(&last.text, size, last.style);
                width = 0.0;
                for s in &current {
                    width += s.width;
                }
            } else {
                let w = measure::width(part, size, span.style);
                current.push(Span { icon: None, style: span.style, text: part.to_owned(), width: w });
                width += w;
            }
        }
    }
    if !current.is_empty() || out.is_empty() {
        commit(&mut current, &mut width, size, out, widths);
    }
}

/// Whether text may hold markup: Pele's `/[<\\\n*_]|fa[bklrs]?:fa-/`.
fn has_markup(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().any(|c| matches!(c, b'<' | b'\\' | b'\n' | b'*' | b'_'))
        || (0..bytes.len()).any(|at| icon_prefix(bytes.get(at..).unwrap_or(&[])).is_some())
}

/// The length of a line break at the start of `rest`: Pele's `/<br\s*\/?>|\\n|\n/i`.
fn line_break(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    match bytes.first()? {
        b'\n' => Some(1),
        b'\\' => (bytes.get(1) == Some(&b'n')).then_some(2),
        b'<' => {
            if !bytes.get(1..3).is_some_and(|br| br.eq_ignore_ascii_case(b"br")) {
                return None;
            }
            let after = rest.get(3..)?;
            let mut end = 3 + after.len() - trim_start(after).len();
            if bytes.get(end) == Some(&b'/') {
                end += 1;
            }
            (bytes.get(end) == Some(&b'>')).then_some(end + 1)
        }
        _ => None,
    }
}

/// Splits text at `<br>` tags, escaped `\n`, and newlines.
fn split_breaks(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < text.len() {
        if let Some(len) = text.get(at..).and_then(line_break) {
            parts.push(text.get(start..at).unwrap_or(""));
            at += len;
            start = at;
        } else {
            at += text.get(at..).and_then(|rest| rest.chars().next()).map_or(1, char::len_utf8);
        }
    }
    parts.push(text.get(start..).unwrap_or(""));
    parts
}

/// A line count as a float.
fn lines_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Turns `\r\n` and `\r` into `\n`.
fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            chars.next_if_eq(&'\n');
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Lays out a label at font `size`, wrapping lines wider than `max_width`. `markdown` reads
/// emphasis markers and dedents, `base` is the style of unmarked text, and `icons` says which
/// `fa:` references the host can draw. An absent or empty label has no lines.
pub(crate) fn layout_label(
    raw: Option<&str>,
    markdown: bool,
    size: f64,
    max_width: f64,
    base: u8,
    icons: Option<IconResolver<'_>>,
) -> Label {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Label::default();
    };
    let line_height = js_round(size * LINE_HEIGHT);

    if !has_markup(raw) && !raw.contains('&') && !raw.contains('\u{fb02}') {
        let text = trim(raw);
        let width = measure::width(text, size, base);
        let span = Span { icon: None, style: base, text: text.to_owned(), width };
        if width <= max_width || !text.contains(' ') {
            return Label {
                height: line_height,
                line_height,
                lines: vec![vec![span]],
                size,
                width,
                widths: vec![width],
            };
        }

        let mut lines = Vec::new();
        let mut widths = Vec::new();
        wrap(&[span], size, max_width, &mut lines, &mut widths);
        let mut widest = 0.0;
        for &w in &widths {
            if w > widest {
                widest = w;
            }
        }
        return Label { height: lines_f64(lines.len()) * line_height, line_height, lines, size, width: widest, widths };
    }

    let normalized;
    let source = if markdown {
        normalized = dedent(&normalize_newlines(raw)).into_owned();
        trim(&normalized)
    } else {
        trim(raw)
    };
    let mut lines = Vec::new();
    let mut widths = Vec::new();
    for part in split_breaks(source) {
        let mut line = Vec::new();
        inline(&mut line, trim(part), markdown, base, icons);
        let w = measure_spans(&mut line, size);
        if w > max_width {
            wrap(&line, size, max_width, &mut lines, &mut widths);
        } else {
            lines.push(line);
            widths.push(w);
        }
    }

    let mut width = 0.0;
    for &w in &widths {
        if w > width {
            width = w;
        }
    }
    Label { height: lines_f64(lines.len()) * line_height, line_height, lines, size, width, widths }
}
