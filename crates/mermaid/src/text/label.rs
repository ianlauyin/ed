//! Lays out label text in lines of styled spans: inline HTML emphasis, markdown emphasis, line
//! breaks, Font Awesome icons, and wrapping. Port of Pele's `src/text/label.ts`.

use std::borrow::Cow;

use crate::svg::builder::js_round;
use crate::text::entities::decode;
use crate::text::icons::IconResolver;
use crate::text::icons::icon_markup;
use crate::text::measure::BOLD;
use crate::text::measure::ITALIC;
use crate::text::measure::SUB;
use crate::text::measure::SUP;
use crate::text::measure::UNDERLINE;
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
/// Line height of a label with several lines, which sit closer so the label reads as one block.
const WRAPPED_LINE_HEIGHT: f64 = 1.3;

/// A soft hyphen, which shows as `-` only where a line breaks at it.
const SHY: char = '\u{ad}';

/// The style an inline HTML tag gives its text, or `None` for a tag that is only dropped.
/// `<code>`, `<kbd>`, `<samp>`, and `<tt>` are dropped: every label is already monospace.
fn tag_style(tag: &str) -> Option<u8> {
    const STYLES: &[(&str, u8)] = &[
        ("b", BOLD),
        ("cite", ITALIC),
        ("dfn", ITALIC),
        ("em", ITALIC),
        ("i", ITALIC),
        ("ins", UNDERLINE),
        ("strong", BOLD),
        ("sub", SUB),
        ("sup", SUP),
        ("u", UNDERLINE),
        ("var", ITALIC),
    ];
    STYLES.iter().find(|(name, _)| tag.eq_ignore_ascii_case(name)).map(|&(_, style)| style)
}

/// Whether `tag` names an HTML element. Its tags are dropped and its text kept; anything else
/// shaped like a tag, such as the `<T>` in `List<T>`, is text.
fn is_html_tag(tag: &str) -> bool {
    const TAGS: &[&str] = &[
        "a",
        "abbr",
        "address",
        "article",
        "aside",
        "audio",
        "b",
        "bdi",
        "bdo",
        "big",
        "blockquote",
        "body",
        "br",
        "button",
        "caption",
        "center",
        "cite",
        "code",
        "col",
        "colgroup",
        "data",
        "dd",
        "del",
        "details",
        "dfn",
        "dialog",
        "div",
        "dl",
        "dt",
        "em",
        "embed",
        "fieldset",
        "figcaption",
        "figure",
        "font",
        "footer",
        "form",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "i",
        "iframe",
        "img",
        "input",
        "ins",
        "kbd",
        "label",
        "legend",
        "li",
        "link",
        "main",
        "map",
        "mark",
        "meta",
        "meter",
        "nav",
        "noscript",
        "object",
        "ol",
        "optgroup",
        "option",
        "output",
        "p",
        "picture",
        "pre",
        "progress",
        "q",
        "rp",
        "rt",
        "ruby",
        "s",
        "samp",
        "script",
        "section",
        "select",
        "small",
        "source",
        "span",
        "strike",
        "strong",
        "style",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "template",
        "textarea",
        "tfoot",
        "th",
        "thead",
        "time",
        "title",
        "tr",
        "track",
        "tt",
        "u",
        "ul",
        "var",
        "video",
        "wbr",
    ];
    TAGS.iter().any(|name| tag.eq_ignore_ascii_case(name))
}

/// Whether `c` is Chinese or Japanese, where a line may break between any two characters.
/// Hangul is left out: Korean puts spaces between words.
const fn is_cjk(c: char) -> bool {
    matches!(c, '\u{2e80}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{ff00}'..='\u{ff60}')
}

/// Whether `c` is closing punctuation, which stays on the line it follows.
fn is_close(c: char) -> bool {
    "、。，．：；！？）］｝」』】〕〉》〙〗ー…‥・ゝゞぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮ,.:;!?)]}".contains(c)
}

/// Whether `c` is an opening bracket, which stays with the character after it.
fn is_open(c: char) -> bool {
    "（［｛「『【〔〈《〘〖([{".contains(c)
}

/// Whether `c` is a break hint: a zero-width space or a soft hyphen.
const fn is_hint(c: char) -> bool {
    c == '\u{200b}' || c == SHY
}

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

/// The HTML styling open at the end of a line, which goes on past a line break. Markdown emphasis
/// stays within its line.
struct Open {
    /// Emphasis from HTML tags.
    style: u8,
    /// Open `<sub>` and `<sup>` tags, innermost last. Only the innermost shifts text.
    shifts: Vec<u8>,
}

impl Open {
    /// The subscript or superscript bit of the innermost open tag.
    fn shift(&self) -> u8 {
        self.shifts.last().copied().unwrap_or(0)
    }
}

/// Splits inline HTML and, for markdown labels, emphasis markers into styled spans.
fn inline(
    line: &mut Vec<Span>,
    source: &str,
    markdown: bool,
    base: u8,
    open: &mut Open,
    icons: Option<IconResolver<'_>>,
) {
    let bytes = source.as_bytes();
    let n = source.len();
    let mut style = open.style;
    let mut marked = 0;
    let mut text = String::new();
    let mut i = 0;
    while let Some(ch) = source.get(i..).and_then(|rest| rest.chars().next()) {
        if ch == '<' {
            if let Some((len, tag)) = match_tag(source.get(i..).unwrap_or("")) {
                let closing = bytes.get(i + 1) == Some(&b'/');
                let bit = tag_style(tag).unwrap_or(0);
                if bit & (SUB | SUP) != 0 {
                    push_icons(line, &text, style | open.shift(), icons);
                    text.clear();
                    let at = open.shifts.iter().rposition(|&shift| shift == bit);
                    if !closing {
                        open.shifts.push(bit);
                    } else if let Some(at) = at {
                        open.shifts.remove(at);
                    }
                } else if bit != 0 {
                    push_icons(line, &text, style | open.shift(), icons);
                    text.clear();
                    open.style = if closing { (open.style & !bit) | (base & bit) } else { open.style | bit };
                    style = if closing { (style & !bit) | (base & bit) } else { style | bit };
                }
                if bit != 0 || is_html_tag(tag) {
                    i += len;
                    continue;
                }
            }
        } else if markdown && (ch == '*' || ch == '_') {
            let double = bytes.get(i + 1) == Some(&(ch as u8));
            let bit = if double { BOLD } else { ITALIC };
            let len = if double { 2 } else { 1 };
            let unmarked = marked & bit == 0;
            let prev = source.get(..i).and_then(|head| head.chars().next_back()).unwrap_or(' ');
            let after = source.get(i + len..).unwrap_or("");
            let next = if i + len < n { after.chars().next().unwrap_or(' ') } else { ' ' };
            let mark = if double { source.get(i..i + 2).unwrap_or("") } else { source.get(i..=i).unwrap_or("") };
            let can_open = unmarked && !is_space(u32::from(next)) && after.contains(mark);
            let can_close = !unmarked && !is_space(u32::from(prev));
            let wordy = ch == '_' && is_word(u32::from(prev)) && is_word(u32::from(next));
            if (can_open || can_close) && !wordy {
                push_icons(line, &text, style | open.shift(), icons);
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
    push_icons(line, &text, style | open.shift(), icons);
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

/// Ends a wrapped line, trimming the space at its end. With `hyphen`, a soft hyphen the line ends
/// at shows as `-`.
fn commit(
    current: &mut Vec<Span>,
    width: &mut f64,
    size: f64,
    hyphen: bool,
    out: &mut Vec<Vec<Span>>,
    widths: &mut Vec<f64>,
) {
    if let Some(last) = current.last_mut()
        && last.icon.is_none()
    {
        let trimmed = trim_end(&last.text).len();
        let soft = hyphen && last.text.ends_with(SHY);
        if soft || trimmed != last.text.len() {
            last.text.truncate(trimmed);
            if soft {
                last.text.pop();
                last.text.push('-');
            }
            let w = measure::width(&last.text, size, last.style);
            *width += w - last.width;
            last.width = w;
        }
    }
    out.push(std::mem::take(current));
    widths.push(*width);
    *width = 0.0;
}

/// The length of the piece of Pele's `RE_PIECE` group at the start of `rest`, after its leading
/// whitespace: an opening bracket run, one Chinese or Japanese character, and closing punctuation;
/// or a run of other text, break hints, and closing punctuation; or a run of break hints.
fn piece_body(rest: &str) -> Option<usize> {
    /// The byte offset past the run of characters from `at` that `keep` accepts.
    fn run(rest: &str, at: usize, keep: impl Fn(char) -> bool) -> usize {
        let tail = rest.get(at..).unwrap_or("");
        at + tail.find(|c: char| !keep(c)).unwrap_or(tail.len())
    }
    let first = rest.chars().next()?;

    // `[OPEN]*[CJK][CLOSE]*`, giving opening brackets back when one is needed as the character.
    let opens = run(rest, 0, is_open);
    let after_opens = rest.get(opens..).and_then(|tail| tail.chars().next());
    if let Some(c) = after_opens.filter(|&c| is_cjk(c)) {
        return Some(run(rest, opens + c.len_utf8(), is_close));
    }
    if let Some((at, c)) = rest.get(..opens).and_then(|head| head.char_indices().rev().find(|&(_, c)| is_cjk(c))) {
        return Some(run(rest, at + c.len_utf8(), is_close));
    }

    // `[^\s CJK HINT]+[HINT]*[CLOSE]*`
    let other = |c: char| !is_space(u32::from(c)) && !is_cjk(c) && !is_hint(c);
    if other(first) {
        let word = run(rest, 0, other);
        return Some(run(rest, run(rest, word, is_hint), is_close));
    }

    // `[HINT]+`
    let hints = run(rest, 0, is_hint);
    (hints > 0).then_some(hints)
}

/// The pieces a line may break between, as Pele's `RE_PIECE` matches them: words with the
/// whitespace around them, where a Chinese or Japanese character is a word of its own, a word
/// may break after a zero-width space or a soft hyphen, and closing punctuation stays with what
/// it follows.
fn pieces(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let word = trim_start(rest);
        // Without a body, only whitespace is left.
        let end =
            piece_body(word).map_or(rest.len(), |body| rest.len() - trim_start(word.get(body..).unwrap_or("")).len());
        let (piece, tail) = rest.split_at(end);
        rest = tail;
        Some(piece)
    })
}

/// `piece` without the soft hyphen it ends with.
fn without_shy(piece: &str) -> &str {
    piece.strip_suffix(SHY).unwrap_or(piece)
}

/// Wraps a line at whitespace and break hints so each part fits in `max`, adding the parts to
/// `out`.
fn wrap(line: &[Span], size: f64, max: f64, out: &mut Vec<Vec<Span>>, widths: &mut Vec<f64>) {
    let mut current: Vec<Span> = Vec::new();
    let mut width = 0.0;
    for (index, span) in line.iter().enumerate() {
        if span.icon.is_some() {
            let w = size * 1.25;
            if width > 0.0 && width + w > max {
                commit(&mut current, &mut width, size, false, out, widths);
            }
            current.push(Span { width: w, ..span.clone() });
            width += w;
            continue;
        }

        let pieces: Vec<&str> = pieces(&span.text).collect();
        // A piece ending in a soft hyphen fits if its `-` does, or if the rest of the word does,
        // which hides the hyphen. The line only grows along a word, so no later hyphen in it fits
        // either, and the remaining width of each word makes this one check.
        let soft = span.text.contains(SHY);
        let n = if soft { pieces.len() } else { 0 };
        let mut own = vec![0.0; n];
        let mut rest = vec![0.0; n + 1];
        for q in (0..n).rev() {
            let piece = pieces.get(q).copied().unwrap_or("");
            // A soft hyphen before a space is no break of its own: the space is.
            let shy = piece.ends_with(SHY);
            let end = trim_end(piece);
            let w = measure::width(if shy { without_shy(end) } else { end }, size, span.style);
            let next = rest.get(q + 1).copied().unwrap_or(0.0);
            let remaining = if !shy {
                w
            } else if q + 1 == n {
                // Word width beyond this span is unknown.
                if index + 1 == line.len() { w } else { f64::INFINITY }
            } else {
                w + next
            };
            if let Some(slot) = own.get_mut(q) {
                *slot = w;
            }
            if let Some(slot) = rest.get_mut(q) {
                *slot = remaining;
            }
        }

        for (p, &piece) in pieces.iter().enumerate() {
            let mut part = piece;
            let part_width = if soft {
                own.get(p).copied().unwrap_or(0.0)
            } else {
                measure::width(trim_end(part), size, span.style)
            };
            let fits = if soft && part.ends_with(SHY) {
                let hyphenated = format!("{}-", without_shy(part));
                width + measure::width(&hyphenated, size, span.style) <= max
                    || width + rest.get(p).copied().unwrap_or(0.0) <= max
            } else {
                width + part_width <= max
            };
            if width > 0.0 && !fits {
                // Breaking at whitespace leaves a preceding soft hyphen hidden.
                let hyphen = trim_start(part).len() == part.len();
                commit(&mut current, &mut width, size, hyphen, out, widths);
                part = trim_start(part);
            }
            if let Some(last) = current.last_mut()
                && last.style == span.style
                && last.icon.is_none()
            {
                last.text.push_str(part);
                // Remeasuring for each zero-width piece would take quadratic time.
                if part_width > 0.0 || trim_end(part).len() != part.len() {
                    let before = last.width;
                    last.width = measure::width(&last.text, size, last.style);
                    width += last.width - before;
                }
            } else {
                let w = measure::width(part, size, span.style);
                current.push(Span { icon: None, style: span.style, text: part.to_owned(), width: w });
                width += w;
            }
        }
    }
    if !current.is_empty() || out.is_empty() {
        commit(&mut current, &mut width, size, false, out, widths);
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
        if width <= max_width || !text.contains(|c: char| c == ' ' || is_cjk(c) || is_hint(c)) {
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
        return finish(lines, widths, size, line_height);
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
    // HTML tags can span explicit breaks; Markdown emphasis stays local to each line.
    let mut open = Open { style: base, shifts: Vec::new() };
    for part in split_breaks(source) {
        let mut line = Vec::new();
        inline(&mut line, trim(part), markdown, base, &mut open, icons);
        let w = measure_spans(&mut line, size);
        if w > max_width {
            wrap(&line, size, max_width, &mut lines, &mut widths);
        } else {
            lines.push(line);
            widths.push(w);
        }
    }
    finish(lines, widths, size, line_height)
}

/// A label of laid out `lines`, which sit closer when there are several.
fn finish(lines: Vec<Vec<Span>>, widths: Vec<f64>, size: f64, line_height: f64) -> Label {
    let width = widths.iter().fold(0.0, |widest: f64, &w| if w > widest { w } else { widest });
    let line_height = if lines.len() > 1 { js_round(size * WRAPPED_LINE_HEIGHT) } else { line_height };
    Label { height: lines_f64(lines.len()) * line_height, line_height, lines, size, width, widths }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JA: &str = "在庫引当の結果を基幹システムへ連携し、出荷指示を倉庫管理システムに送信して完了を確認する";
    const ZWSP: char = '\u{200b}';

    fn layout(raw: &str, max: f64) -> Label {
        layout_label(Some(raw), false, 16.0, max, 0, None)
    }

    fn lines(raw: &str, max: f64) -> Vec<String> {
        layout(raw, max).lines.iter().map(|line| line.iter().map(|span| span.text.as_str()).collect()).collect()
    }

    fn styles(raw: &str) -> Vec<Vec<(String, u8)>> {
        layout(raw, 400.0)
            .lines
            .iter()
            .map(|line| line.iter().map(|span| (span.text.clone(), span.style)).collect())
            .collect()
    }

    fn spans(list: &[(&str, u8)]) -> Vec<(String, u8)> {
        list.iter().map(|&(text, style)| (text.to_owned(), style)).collect()
    }

    fn assert_fits(raw: &str, max: f64) {
        for width in layout(raw, max).widths {
            assert!(width <= max, "{raw:?}: {width} > {max}");
        }
    }

    #[test]
    fn wraps_japanese_between_characters() {
        let out = lines(JA, 200.0);
        assert!(out.len() > 1);
        assert_eq!(out.concat(), JA);
        assert_fits(JA, 200.0);
        assert_eq!(lines("在庫確認", 200.0), ["在庫確認"]);
    }

    #[test]
    fn keeps_cjk_punctuation_with_its_character() {
        let source = format!("「{JA}」（確認済み）。");
        for max in (40..=240).step_by(8) {
            let out = lines(&source, f64::from(max));
            assert_eq!(out.concat(), source);
            for line in &out {
                assert!(!line.starts_with(['、', '。', '，', '．', '）', '」', '』', '】']), "{line}");
                assert!(!line.ends_with(['「', '『', '（', '【']), "{line}");
            }
        }
    }

    #[test]
    fn keeps_latin_and_korean_words_whole() {
        let out = lines("AWS Lambda から呼び出されるバッチ処理の完了通知を送信する", 160.0);
        assert!(out.len() > 1);
        assert!(out.iter().any(|line| line.contains("Lambda")));
        assert_eq!(lines("one two three four five six", 100.0), ["one two", "three four", "five six"]);
        let ko = lines("안녕하세요 반갑습니다 여러분 모두", 90.0);
        for word in ["안녕하세요", "반갑습니다", "여러분", "모두"] {
            assert!(ko.iter().any(|line| line.contains(word)), "{ko:?}");
        }
        assert_eq!(lines("Supercalifragilisticexpialidocious", 60.0), ["Supercalifragilisticexpialidocious"]);
    }

    #[test]
    fn breaks_at_zero_width_spaces() {
        let out = lines(&format!("RRCConnection{ZWSP}Reconfiguration{ZWSP}Complete"), 150.0);
        let out: Vec<String> = out.iter().map(|line| line.replace(ZWSP, "")).collect();
        assert_eq!(out, ["RRCConnection", "Reconfiguration", "Complete"]);
    }

    #[test]
    fn shows_soft_hyphens_only_at_breaks() {
        let source = format!("RRCConnection{SHY}Reconfiguration{SHY}Complete");
        assert_eq!(lines(&source, 160.0), ["RRCConnection-", "Reconfiguration-", "Complete"]);
        assert_fits(&source, 160.0);
        assert_eq!(
            lines("RRCConnection&shy;Reconfiguration&shy;Complete", 160.0),
            ["RRCConnection-", "Reconfiguration-", "Complete"]
        );
        let short = format!("Re{SHY}con{SHY}fig");
        assert_eq!(lines(&short, 200.0), std::slice::from_ref(&short));
        assert!((measure::width(&format!("ab{SHY}c"), 16.0, 0) - measure::width("abc", 16.0, 0)).abs() < f64::EPSILON);
    }

    #[test]
    fn keeps_soft_hyphenated_words_within_the_width() {
        assert_eq!(lines(&format!("a uni{SHY}t test"), 58.0), [format!("a uni{SHY}t"), "test".to_owned()]);
        assert_fits(&format!("a uni{SHY}t{SHY}ed"), 58.0);
        assert_fits(&format!("a uni{SHY}<b>t</b> test"), 58.0);
        assert_fits(&format!("a uni{SHY} t test"), 58.0);
        assert!(!lines(&format!("a uni{SHY} t test"), 58.0).concat().contains('-'));
        assert_eq!(lines(&format!("ab{SHY}<i> cd</i>"), 25.0), [format!("ab{SHY}"), "cd".to_owned()]);
    }

    #[test]
    fn wraps_many_zero_width_pieces() {
        for hint in [SHY, ZWSP] {
            let source = format!("{}{}i xx", "a".repeat(22), format!("\u{300}{hint}").repeat(24000));
            assert!(layout(&source, 200.0).lines.len() > 1);
        }
    }

    #[test]
    fn sets_wrapped_lines_closer() {
        let one = layout("one two", 200.0);
        assert!((one.line_height - 24.0).abs() < f64::EPSILON);
        let wrapped = layout("one two three four five six", 100.0);
        assert!((wrapped.line_height - 21.0).abs() < f64::EPSILON);
        assert!((wrapped.height - 63.0).abs() < f64::EPSILON);
        assert!((layout("one<br>two", 200.0).line_height - 21.0).abs() < f64::EPSILON);
    }

    #[test]
    fn styles_inline_tags() {
        for (tag, style) in [
            ("cite", ITALIC),
            ("dfn", ITALIC),
            ("var", ITALIC),
            ("u", UNDERLINE),
            ("ins", UNDERLINE),
            ("sub", SUB),
            ("sup", SUP),
        ] {
            let found = styles(&format!("plain <{tag}>tagged</{tag}> plain"));
            assert_eq!(found, [spans(&[("plain ", 0), ("tagged", style), (" plain", 0)])], "{tag}");
        }
        // Monospace tags are dropped: every label is monospace already.
        assert_eq!(styles("plain <code>tagged</code> plain"), [spans(&[("plain tagged plain", 0)])]);
    }

    #[test]
    fn strips_only_html_tags() {
        assert_eq!(lines("List<T>", 400.0), ["List<T>"]);
        assert_eq!(lines("<T>", 400.0), ["<T>"]);
        assert_eq!(lines("<span title=x>red</span> and <small>print</small><img src=x>", 400.0), ["red and print"]);
        assert_eq!(lines("<b>bold</b> and <T>", 400.0), ["bold and <T>"]);
    }

    #[test]
    fn shifts_by_the_innermost_sub_or_sup() {
        assert_eq!(styles("x<sup>a<sub>b</sub>c</sup>"), [spans(&[("x", 0), ("a", SUP), ("b", SUB), ("c", SUP)])]);
        assert_eq!(
            styles("<sup>a<sub>b<sup>c</sup>d</sub>e</sup>"),
            [spans(&[("a", SUP), ("b", SUB), ("c", SUP), ("d", SUB), ("e", SUP)])]
        );
        assert_eq!(
            styles("<sup>a<sub>b<br/>c</sub>d</sup>"),
            [spans(&[("a", SUP), ("b", SUB)]), spans(&[("c", SUB), ("d", SUP)])]
        );
        assert_eq!(styles("<b>H<sub>2</sub>O</b>"), [spans(&[("H", BOLD), ("2", BOLD | SUB), ("O", BOLD)])]);
        assert!(measure::width("2", 16.0, SUB) < measure::width("2", 16.0, 0));
    }

    #[test]
    fn carries_html_emphasis_across_breaks() {
        for tag in ["b", "strong", "i", "em"] {
            for br in ["<br/>", "<br>", "<BR />", "\\n", "\n"] {
                let carried = layout(&format!("<{tag}>First{br}Second</{tag}> plain{br}Third"), 400.0);
                let reopened = layout(&format!("<{tag}>First</{tag}>{br}<{tag}>Second</{tag}> plain{br}Third"), 400.0);
                assert_eq!(carried, reopened, "{tag} across {br:?}");
            }
        }
        for base in [0, BOLD] {
            let carried =
                layout_label(Some("<b><i>First<br/><br/>Second</i> bold</b> plain"), false, 16.0, 400.0, base, None);
            let reopened = layout_label(
                Some("<b><i>First</i></b><br/><br/><b><i>Second</i> bold</b> plain"),
                false,
                16.0,
                400.0,
                base,
                None,
            );
            assert_eq!(carried, reopened);
        }
        // Markdown emphasis stays within its line.
        let markdown = layout_label(Some("**First\nSecond**"), true, 16.0, 400.0, 0, None);
        assert!(markdown.lines.iter().flatten().all(|span| span.style == 0), "{markdown:?}");
    }
}
