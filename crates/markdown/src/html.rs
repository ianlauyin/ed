//! HTML inside markdown, mapped onto markdown events.
//!
//! A small tag lexer runs over pulldown-cmark's `Html`/`InlineHtml` events, and
//! [`HtmlConverter`] turns a whitelist of tags into the same [`MarkdownEvent`]s
//! markdown syntax produces, so there is a single renderer. Unknown tags render
//! their children; `style`, `class` and scripts are ignored. Tags may span
//! several HTML blocks (`<div align="center">`, markdown, `</div>`).

use std::{borrow::Cow, ops::Range};

use gpui::{DefiniteLength, SharedString, TextAlign, px, relative};
use pulldown_cmark::{Alignment, HeadingLevel, LinkType};

use crate::parser::{
    CodeBlockKind, CodeBlockMetadata, HtmlStyle, MarkdownEvent, MarkdownTag, MarkdownTagEnd,
    ParseState,
};

#[derive(Debug, PartialEq)]
pub(crate) enum Token {
    Text(Range<usize>),
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
        range: Range<usize>,
    },
    End {
        name: String,
        range: Range<usize>,
    },
}

/// Splits HTML into text and tags. Comments, doctypes and processing
/// instructions are dropped; a `<` that does not start a tag is text.
pub(crate) fn lex(html: &str) -> Vec<Token> {
    let bytes = html.as_bytes();
    let mut tokens = Vec::new();
    let mut text_start = 0;
    let mut ix = 0;
    let flush_text = |tokens: &mut Vec<Token>, start: usize, end: usize| {
        if start < end {
            tokens.push(Token::Text(start..end));
        }
    };

    while ix < bytes.len() {
        if bytes[ix] != b'<' {
            ix += 1;
            continue;
        }
        let rest = &html[ix..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            flush_text(&mut tokens, text_start, ix);
            ix = comment
                .find("-->")
                .map_or(html.len(), |end| ix + 4 + end + 3);
            text_start = ix;
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            flush_text(&mut tokens, text_start, ix);
            ix = rest.find('>').map_or(html.len(), |end| ix + end + 1);
            text_start = ix;
        } else if let Some(after_slash) = rest.strip_prefix("</")
            && after_slash.starts_with(|c: char| c.is_ascii_alphabetic())
        {
            flush_text(&mut tokens, text_start, ix);
            let name = tag_name(after_slash);
            let end = rest.find('>').map_or(html.len(), |end| ix + end + 1);
            tokens.push(Token::End {
                name,
                range: ix..end,
            });
            ix = end;
            text_start = ix;
        } else if rest[1..].starts_with(|c: char| c.is_ascii_alphabetic())
            && let Some((name, attrs, self_closing, len)) = lex_start_tag(rest)
        {
            flush_text(&mut tokens, text_start, ix);
            let end = ix + len;
            let raw_text = matches!(name.as_str(), "script" | "style" | "textarea" | "title");
            tokens.push(Token::Start {
                name: name.clone(),
                attrs,
                self_closing,
                range: ix..end,
            });
            ix = end;
            text_start = ix;
            if raw_text && !self_closing {
                let close = format!("</{name}");
                let content_end = find_ascii_case_insensitive(&html[ix..], &close)
                    .map_or(html.len(), |offset| ix + offset);
                flush_text(&mut tokens, ix, content_end);
                ix = content_end;
                text_start = ix;
            }
        } else {
            ix += 1;
        }
    }
    flush_text(&mut tokens, text_start, html.len());
    tokens
}

fn tag_name(text: &str) -> String {
    text.split(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Lexes `<name attr=value ...>` at the start of `text`. Returns the lowercase
/// name, attributes (lowercase names, entity-decoded values), whether the tag
/// is self-closing and the tag's length. `None` if the tag is not closed.
fn lex_start_tag(text: &str) -> Option<(String, Vec<(String, String)>, bool, usize)> {
    let bytes = text.as_bytes();
    let name = tag_name(&text[1..]);
    let mut ix = 1 + name.len();
    let mut attrs = Vec::new();
    loop {
        while ix < bytes.len() && bytes[ix].is_ascii_whitespace() {
            ix += 1;
        }
        match bytes.get(ix)? {
            b'>' => return Some((name, attrs, false, ix + 1)),
            b'/' if bytes.get(ix + 1) == Some(&b'>') => return Some((name, attrs, true, ix + 2)),
            b'/' => {
                ix += 1;
                continue;
            }
            _ => {}
        }

        let name_start = ix;
        while ix < bytes.len()
            && !bytes[ix].is_ascii_whitespace()
            && !matches!(bytes[ix], b'=' | b'>' | b'/')
        {
            ix += 1;
        }
        let attr_name = text[name_start..ix].to_ascii_lowercase();
        while ix < bytes.len() && bytes[ix].is_ascii_whitespace() {
            ix += 1;
        }
        let mut value = String::new();
        if bytes.get(ix) == Some(&b'=') {
            ix += 1;
            while ix < bytes.len() && bytes[ix].is_ascii_whitespace() {
                ix += 1;
            }
            let value_range = match bytes.get(ix)? {
                quote @ (b'"' | b'\'') => {
                    let start = ix + 1;
                    let end = start + text[start..].find(*quote as char)?;
                    ix = end + 1;
                    start..end
                }
                _ => {
                    let start = ix;
                    while ix < bytes.len() && !bytes[ix].is_ascii_whitespace() && bytes[ix] != b'>'
                    {
                        ix += 1;
                    }
                    start..ix
                }
            };
            value = decode_entities(&text[value_range]).into_owned();
        }
        attrs.push((attr_name, value));
    }
}

/// Decodes numeric character references and the common named entities.
pub(crate) fn decode_entities(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        decoded.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let entity = rest[1..]
            .find(';')
            .filter(|end| *end <= 32)
            .and_then(|end| Some((decode_entity(&rest[1..1 + end])?, end + 2)));
        match entity {
            Some((character, len)) => {
                decoded.push(character);
                rest = &rest[len..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    Cow::Owned(decoded)
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|c| *c != '\0');
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "middot" => '·',
        "bull" => '•',
        "laquo" => '«',
        "raquo" => '»',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "times" => '×',
        "divide" => '÷',
        "deg" => '°',
        "plusmn" => '±',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "check" => '✓',
        "star" => '☆',
        "hearts" => '♥',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwj" => '\u{200d}',
        "zwnj" => '\u{200c}',
        _ => return None,
    })
}

/// Maps offsets in concatenated HTML text back to markdown source offsets. An
/// HTML block arrives as one event per line, without container prefixes such
/// as `> `, so its text is not contiguous in the source.
pub(crate) struct SourceMap {
    /// `(offset in the HTML text, offset in the source)` for each chunk.
    chunks: Vec<(usize, usize)>,
}

impl SourceMap {
    pub(crate) fn new(chunks: Vec<(usize, usize)>) -> Self {
        Self { chunks }
    }

    fn offset(&self, offset: usize) -> usize {
        let chunk = self.chunks.partition_point(|(start, _)| *start <= offset);
        let (html_start, source_start) = self.chunks[chunk.saturating_sub(1)];
        source_start + offset - html_start
    }

    fn range(&self, range: &Range<usize>) -> Range<usize> {
        self.offset(range.start)..self.offset(range.end).max(self.offset(range.start))
    }
}

/// What closing an open HTML element emits, beyond its plain end events.
#[derive(Clone, Copy, Debug, PartialEq)]
enum FrameKind {
    Plain,
    List,
    Table,
    /// `<thead>`, or a leading row of `<th>` cells.
    TableHead,
    TableRow,
    TableCell,
    Pre,
}

#[derive(Debug)]
enum FrameEnd {
    Tag(MarkdownTagEnd),
    Style,
}

#[derive(Debug)]
struct Frame {
    name: String,
    kind: FrameKind,
    /// Markdown container depth when this element opened. Markdown containers
    /// close the HTML elements opened inside them.
    markdown_depth: usize,
    ends: Vec<FrameEnd>,
}

#[derive(Default)]
struct TableContext {
    columns: usize,
    first_row_is_header: bool,
    head_done: bool,
    cells_in_row: usize,
}

#[derive(Default)]
pub(crate) struct HtmlConverter {
    stack: Vec<Frame>,
    tables: Vec<TableContext>,
    /// Open `Start` events this converter pushed, so the markdown container
    /// depth is `state.depth - open_starts`.
    open_starts: usize,
    /// Name of a skipped element (`<script>`, `<style>`, ...) whose content is
    /// dropped up to its end tag.
    skipping: Option<String>,
    /// Whether the last thing emitted was inline, so whitespace between tags
    /// is significant.
    last_inline: bool,
    link_depth: usize,
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

const SKIPPED_ELEMENTS: &[&str] = &[
    "script", "style", "title", "head", "template", "svg", "caption", "iframe", "textarea",
    "noscript",
];

const BLOCK_ELEMENTS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "center",
    "details",
    "div",
    "dl",
    "figcaption",
    "figure",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
];

impl HtmlConverter {
    pub(crate) fn in_link(&self) -> bool {
        self.link_depth > 0
    }

    fn markdown_depth(&self, state: &ParseState) -> usize {
        state.depth - self.open_starts
    }

    /// Converts the tokens of one HTML block (or one inline HTML event).
    pub(crate) fn convert(
        &mut self,
        source: &str,
        html: &str,
        tokens: &[Token],
        map: &SourceMap,
        block: bool,
        state: &mut ParseState,
    ) {
        if block {
            self.last_inline = false;
        }
        for (index, token) in tokens.iter().enumerate() {
            match token {
                Token::Text(range) => {
                    self.text(source, &html[range.clone()], map.range(range), state)
                }
                Token::Start {
                    name,
                    attrs,
                    self_closing,
                    range,
                } => {
                    if self.skipping.is_some() {
                        continue;
                    }
                    let range = map.range(range);
                    let lookahead = &tokens[index + 1..];
                    self.start_tag(name, attrs, *self_closing, range, lookahead, html, state);
                }
                Token::End { name, range } => {
                    if let Some(skipped) = &self.skipping {
                        if skipped == name {
                            self.skipping = None;
                        }
                        continue;
                    }
                    self.end_tag(name, map.range(range), state);
                }
            }
        }
    }

    /// Closes the HTML elements opened inside the markdown container about to
    /// end.
    pub(crate) fn before_markdown_end(&mut self, range: &Range<usize>, state: &mut ParseState) {
        let depth = self.markdown_depth(state);
        while self
            .stack
            .last()
            .is_some_and(|frame| frame.markdown_depth >= depth)
        {
            self.pop_frame(range.end..range.end, state);
        }
    }

    /// Closes everything still open at the end of the document.
    pub(crate) fn finish(&mut self, end: usize, state: &mut ParseState) {
        while !self.stack.is_empty() {
            self.pop_frame(end..end, state);
        }
    }

    fn in_pre(&self) -> bool {
        self.stack.iter().any(|frame| frame.kind == FrameKind::Pre)
    }

    fn text(&mut self, source: &str, text: &str, range: Range<usize>, state: &mut ParseState) {
        if self.skipping.is_some() {
            return;
        }
        if self.in_pre() {
            // A newline right after `<pre>` is not part of its content.
            let after_start = matches!(
                state.events.last(),
                Some((_, MarkdownEvent::Start(MarkdownTag::CodeBlock { .. })))
            );
            let (text, range) = match text.strip_prefix('\n') {
                Some(text) if after_start => (text, range.start + 1..range.end),
                _ => (text, range),
            };
            push_text(source, state, range, decode_entities(text));
            return;
        }

        let collapsed = collapse_whitespace(text);
        let text = if collapsed.trim().is_empty() {
            if !self.last_inline || text.contains('\n') {
                return;
            }
            Cow::Borrowed(" ")
        } else if self.last_inline {
            collapsed
        } else {
            match collapsed {
                Cow::Borrowed(text) => Cow::Borrowed(text.trim_start()),
                Cow::Owned(text) => Cow::Owned(text.trim_start().to_string()),
            }
        };
        let text = match decode_entities(&text) {
            Cow::Borrowed(_) => text,
            Cow::Owned(decoded) => Cow::Owned(decoded),
        };
        push_text(source, state, range, text);
        self.last_inline = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn start_tag(
        &mut self,
        name: &str,
        attrs: &[(String, String)],
        self_closing: bool,
        range: Range<usize>,
        lookahead: &[Token],
        html: &str,
        state: &mut ParseState,
    ) {
        let attr = |key: &str| {
            attrs
                .iter()
                .find(|(attr, _)| attr == key)
                .map(|(_, value)| value.as_str())
        };
        let align = || match name {
            "center" => Some(TextAlign::Center),
            _ => attr("align").and_then(parse_text_align),
        };

        if SKIPPED_ELEMENTS.contains(&name) {
            if !self_closing {
                self.skipping = Some(name.to_string());
            }
            return;
        }
        if self.in_pre() {
            return;
        }
        if BLOCK_ELEMENTS.contains(&name) {
            self.close_open_paragraph(&range, state);
        }

        let mut kind = FrameKind::Plain;
        let mut starts = Vec::new();
        let mut styles = Vec::new();
        match name {
            "br" => {
                state.push_event(range, MarkdownEvent::HardBreak);
                self.last_inline = false;
                return;
            }
            "hr" => {
                state.push_event(range, MarkdownEvent::Rule);
                self.last_inline = false;
                return;
            }
            "img" => {
                let Some(src) = attr("src") else {
                    return;
                };
                let image = MarkdownTag::Image {
                    link_type: LinkType::Inline,
                    dest_url: SharedString::from(src.to_string()),
                    title: SharedString::from(attr("title").unwrap_or_default().to_string()),
                    id: SharedString::default(),
                    width: attr("width").and_then(parse_dimension),
                    height: attr("height").and_then(parse_dimension),
                };
                state.push_event(range.clone(), MarkdownEvent::Start(image));
                if let Some(alt) = attr("alt").filter(|alt| !alt.is_empty()) {
                    state.push_event(
                        range.clone(),
                        MarkdownEvent::SubstitutedText(alt.to_string()),
                    );
                }
                state.push_event(range, MarkdownEvent::End(MarkdownTagEnd::Image));
                self.last_inline = true;
                return;
            }
            "p" => {
                if let Some(align) = align() {
                    starts.push(MarkdownTag::HtmlBlock { align: Some(align) });
                }
                starts.push(MarkdownTag::Paragraph);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if let Some(align) = align() {
                    starts.push(MarkdownTag::HtmlBlock { align: Some(align) });
                }
                starts.push(MarkdownTag::Heading {
                    level: match name {
                        "h1" => HeadingLevel::H1,
                        "h2" => HeadingLevel::H2,
                        "h3" => HeadingLevel::H3,
                        "h4" => HeadingLevel::H4,
                        "h5" => HeadingLevel::H5,
                        _ => HeadingLevel::H6,
                    },
                    id: attr("id").map(|id| SharedString::from(id.to_string())),
                    classes: Vec::new(),
                    attrs: Vec::new(),
                });
            }
            "summary" => {
                starts.push(MarkdownTag::HtmlBlock { align: None });
                starts.push(MarkdownTag::Strong);
            }
            "blockquote" => starts.push(MarkdownTag::BlockQuote(None)),
            "ul" | "ol" => {
                kind = FrameKind::List;
                let start = (name == "ol").then(|| {
                    attr("start")
                        .and_then(|start| start.parse().ok())
                        .unwrap_or(1)
                });
                starts.push(MarkdownTag::List(start));
            }
            "li" => {
                self.close_up_to(&["li"], &["ul", "ol"], &range, state);
                if self.innermost(&[FrameKind::List]) {
                    starts.push(MarkdownTag::Item);
                }
            }
            "pre" => {
                kind = FrameKind::Pre;
                let content = pre_content(html, lookahead);
                let content = content.strip_prefix('\n').unwrap_or(&content);
                let content_start =
                    range.end + usize::from(pre_starts_with_newline(lookahead, html));
                starts.push(MarkdownTag::CodeBlock {
                    kind: CodeBlockKind::Indented,
                    metadata: CodeBlockMetadata {
                        content_range: content_start..content_start,
                        line_count: content.matches('\n').count(),
                        is_fenced_closed: false,
                    },
                });
            }
            "table" => {
                let (columns, first_row_is_header) = table_shape(lookahead);
                if columns > 0 {
                    kind = FrameKind::Table;
                    self.tables.push(TableContext {
                        columns,
                        first_row_is_header,
                        ..Default::default()
                    });
                    starts.push(MarkdownTag::Table(vec![Alignment::None; columns]));
                }
            }
            "thead" | "tbody" | "tfoot" => {
                self.close_up_to(&["thead", "tbody", "tfoot"], &["table"], &range, state);
                if name == "thead"
                    && self.innermost(&[FrameKind::Table])
                    && let Some(table) = self.tables.last_mut()
                    && !table.head_done
                {
                    table.head_done = true;
                    kind = FrameKind::TableHead;
                    starts.push(MarkdownTag::TableHead);
                }
            }
            "tr" => {
                self.close_up_to(&["tr"], &["table"], &range, state);
                if self.innermost(&[FrameKind::TableHead]) {
                    kind = FrameKind::TableRow;
                } else if self.innermost(&[FrameKind::Table])
                    && let Some(table) = self.tables.last_mut()
                {
                    kind = FrameKind::TableRow;
                    if table.first_row_is_header && !table.head_done {
                        table.head_done = true;
                        starts.push(MarkdownTag::TableHead);
                    } else {
                        table.head_done = true;
                        starts.push(MarkdownTag::TableRow);
                    }
                }
                if let Some(table) = self.tables.last_mut() {
                    table.cells_in_row = 0;
                }
            }
            "td" | "th" => {
                self.close_up_to(&["td", "th"], &["tr", "table"], &range, state);
                if self.innermost(&[FrameKind::TableRow])
                    && let Some(table) = self.tables.last_mut()
                {
                    kind = FrameKind::TableCell;
                    table.cells_in_row += 1;
                    starts.push(MarkdownTag::TableCell);
                    let in_head = self.in_table_head();
                    if name == "th" && !in_head {
                        starts.push(MarkdownTag::Strong);
                    }
                }
            }
            "div" | "center" | "address" | "article" | "aside" | "details" | "dl"
            | "figcaption" | "figure" | "footer" | "header" | "main" | "nav" | "section" => {
                starts.push(MarkdownTag::HtmlBlock { align: align() });
            }
            "a" => {
                if let Some(href) = attr("href") {
                    self.link_depth += 1;
                    starts.push(MarkdownTag::Link {
                        link_type: LinkType::Inline,
                        dest_url: SharedString::from(href.to_string()),
                        title: SharedString::from(attr("title").unwrap_or_default().to_string()),
                        id: SharedString::default(),
                    });
                }
            }
            "b" | "strong" => starts.push(MarkdownTag::Strong),
            "i" | "em" | "cite" | "var" | "dfn" => starts.push(MarkdownTag::Emphasis),
            "s" | "del" | "strike" => starts.push(MarkdownTag::Strikethrough),
            "sup" => starts.push(MarkdownTag::Superscript),
            "sub" => starts.push(MarkdownTag::Subscript),
            "u" | "ins" => styles.push(HtmlStyle::Underline),
            "mark" => styles.push(HtmlStyle::Mark),
            "code" | "kbd" | "samp" | "tt" => styles.push(HtmlStyle::Code),
            _ => {}
        }

        if self_closing || VOID_ELEMENTS.contains(&name) {
            return;
        }

        let block = BLOCK_ELEMENTS.contains(&name)
            || matches!(
                kind,
                FrameKind::Table | FrameKind::TableRow | FrameKind::TableCell
            )
            || name == "li";
        let markdown_depth = self.markdown_depth(state);
        let mut ends = Vec::new();
        for tag in starts {
            ends.push(FrameEnd::Tag(tag_end(&tag)));
            state.push_event(range.clone(), MarkdownEvent::Start(tag));
            self.open_starts += 1;
        }
        for style in styles {
            ends.push(FrameEnd::Style);
            state.push_event(range.clone(), MarkdownEvent::StartHtmlStyle(style));
        }
        self.stack.push(Frame {
            name: name.to_string(),
            kind,
            markdown_depth,
            ends,
        });
        if block {
            self.last_inline = false;
        }
    }

    /// Whether the innermost table row is part of the table head: a row in
    /// `<thead>`, or a leading row of `<th>` cells.
    fn in_table_head(&self) -> bool {
        let mut frames = self
            .stack
            .iter()
            .rev()
            .filter(|frame| frame.kind != FrameKind::Plain);
        frames.next().is_some_and(|row| {
            row.kind == FrameKind::TableRow
                && (matches!(
                    row.ends.first(),
                    Some(FrameEnd::Tag(MarkdownTagEnd::TableHead))
                ) || frames
                    .next()
                    .is_some_and(|frame| frame.kind == FrameKind::TableHead))
        })
    }

    /// Whether the innermost structural element (ignoring plain wrappers) at
    /// the current markdown depth is one of `kinds`.
    fn innermost(&self, kinds: &[FrameKind]) -> bool {
        self.stack
            .iter()
            .rev()
            .map(|frame| frame.kind)
            .find(|kind| *kind != FrameKind::Plain)
            .is_some_and(|kind| kinds.contains(&kind))
    }

    fn end_tag(&mut self, name: &str, range: Range<usize>, state: &mut ParseState) {
        if name == "br" {
            state.push_event(range, MarkdownEvent::HardBreak);
            return;
        }
        let depth = self.markdown_depth(state);
        let Some(index) = self
            .stack
            .iter()
            .rposition(|frame| frame.name == name || frame.markdown_depth < depth)
        else {
            return;
        };
        if self.stack[index].name != name || self.stack[index].markdown_depth != depth {
            return;
        }
        while self.stack.len() > index {
            self.pop_frame(range.clone(), state);
        }
    }

    /// Implicitly closes an open element named in `names`, unless an element
    /// named in `stop_at` is nearer.
    fn close_up_to(
        &mut self,
        names: &[&str],
        stop_at: &[&str],
        range: &Range<usize>,
        state: &mut ParseState,
    ) {
        let depth = self.markdown_depth(state);
        let Some(index) = self.stack.iter().rposition(|frame| {
            names.contains(&frame.name.as_str())
                || stop_at.contains(&frame.name.as_str())
                || frame.markdown_depth < depth
        }) else {
            return;
        };
        let frame = &self.stack[index];
        if names.contains(&frame.name.as_str()) && frame.markdown_depth == depth {
            while self.stack.len() > index {
                self.pop_frame(range.start..range.start, state);
            }
        }
    }

    fn close_open_paragraph(&mut self, range: &Range<usize>, state: &mut ParseState) {
        let depth = self.markdown_depth(state);
        let Some(index) = self
            .stack
            .iter()
            .rposition(|frame| frame.name == "p" || BLOCK_ELEMENTS.contains(&frame.name.as_str()))
        else {
            return;
        };
        if self.stack[index].name == "p" && self.stack[index].markdown_depth == depth {
            while self.stack.len() > index {
                self.pop_frame(range.start..range.start, state);
            }
        }
    }

    fn pop_frame(&mut self, range: Range<usize>, state: &mut ParseState) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        match frame.kind {
            FrameKind::TableRow => {
                if let Some(table) = self.tables.last_mut() {
                    for _ in table.cells_in_row..table.columns {
                        state.push_event(
                            range.clone(),
                            MarkdownEvent::Start(MarkdownTag::TableCell),
                        );
                        state.push_event(
                            range.clone(),
                            MarkdownEvent::End(MarkdownTagEnd::TableCell),
                        );
                    }
                    table.cells_in_row = table.columns;
                }
            }
            FrameKind::Table => {
                self.tables.pop();
            }
            _ => {}
        }
        for end in frame.ends.into_iter().rev() {
            match end {
                FrameEnd::Tag(tag) => {
                    if tag == MarkdownTagEnd::Link {
                        self.link_depth = self.link_depth.saturating_sub(1);
                    }
                    if tag == MarkdownTagEnd::CodeBlock
                        && let Some(start) =
                            state
                                .events
                                .iter_mut()
                                .rev()
                                .find_map(|(_, event)| match event {
                                    MarkdownEvent::Start(MarkdownTag::CodeBlock {
                                        metadata,
                                        ..
                                    }) => Some(metadata),
                                    _ => None,
                                })
                    {
                        start.content_range.end = range.start.max(start.content_range.start);
                    }
                    self.open_starts -= 1;
                    state.push_event(range.clone(), MarkdownEvent::End(tag));
                }
                FrameEnd::Style => state.push_event(range.clone(), MarkdownEvent::EndHtmlStyle),
            }
        }
        if BLOCK_ELEMENTS.contains(&frame.name.as_str()) || frame.kind != FrameKind::Plain {
            self.last_inline = false;
        }
    }
}

fn push_text(source: &str, state: &mut ParseState, range: Range<usize>, text: Cow<'_, str>) {
    if text.is_empty() {
        return;
    }
    match text {
        Cow::Borrowed(text) if source.get(range.clone()) == Some(text) => {
            state.push_event(range, MarkdownEvent::Text)
        }
        text => state.push_event(range, MarkdownEvent::SubstitutedText(text.into_owned())),
    }
}

fn tag_end(tag: &MarkdownTag) -> MarkdownTagEnd {
    match tag {
        MarkdownTag::Paragraph => MarkdownTagEnd::Paragraph,
        MarkdownTag::Heading { level, .. } => MarkdownTagEnd::Heading(*level),
        MarkdownTag::BlockQuote(kind) => MarkdownTagEnd::BlockQuote(*kind),
        MarkdownTag::CodeBlock { .. } => MarkdownTagEnd::CodeBlock,
        MarkdownTag::HtmlBlock { .. } => MarkdownTagEnd::HtmlBlock,
        MarkdownTag::List(start) => MarkdownTagEnd::List(start.is_some()),
        MarkdownTag::Item => MarkdownTagEnd::Item,
        MarkdownTag::Table(_) => MarkdownTagEnd::Table,
        MarkdownTag::TableHead => MarkdownTagEnd::TableHead,
        MarkdownTag::TableRow => MarkdownTagEnd::TableRow,
        MarkdownTag::TableCell => MarkdownTagEnd::TableCell,
        MarkdownTag::Emphasis => MarkdownTagEnd::Emphasis,
        MarkdownTag::Strong => MarkdownTagEnd::Strong,
        MarkdownTag::Strikethrough => MarkdownTagEnd::Strikethrough,
        MarkdownTag::Superscript => MarkdownTagEnd::Superscript,
        MarkdownTag::Subscript => MarkdownTagEnd::Subscript,
        MarkdownTag::Link { .. } => MarkdownTagEnd::Link,
        MarkdownTag::Image { .. } => MarkdownTagEnd::Image,
        MarkdownTag::FootnoteDefinition(_) => MarkdownTagEnd::FootnoteDefinition,
        MarkdownTag::MetadataBlock(kind) => MarkdownTagEnd::MetadataBlock(*kind),
        MarkdownTag::DefinitionList => MarkdownTagEnd::DefinitionList,
        MarkdownTag::DefinitionListTitle => MarkdownTagEnd::DefinitionListTitle,
        MarkdownTag::DefinitionListDefinition => MarkdownTagEnd::DefinitionListDefinition,
    }
}

/// Column count and whether the first row is all `<th>`, from the tokens
/// after `<table>` up to its end tag.
fn table_shape(tokens: &[Token]) -> (usize, bool) {
    let mut columns = 0;
    let mut cells = 0;
    let mut rows = 0;
    let mut first_row_is_header = true;
    let mut nested = 0;
    for token in tokens {
        match token {
            Token::Start { name, .. } if name == "table" => nested += 1,
            Token::End { name, .. } if name == "table" => {
                if nested == 0 {
                    break;
                }
                nested -= 1;
            }
            _ if nested > 0 => {}
            Token::Start { name, .. } if name == "tr" => {
                columns = columns.max(cells);
                cells = 0;
                rows += 1;
            }
            Token::Start { name, attrs, .. } if name == "td" || name == "th" => {
                let span = attrs
                    .iter()
                    .find(|(attr, _)| attr == "colspan")
                    .and_then(|(_, span)| span.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                cells += span;
                if rows <= 1 && name == "td" {
                    first_row_is_header = false;
                }
            }
            Token::Start { name, .. } if name == "thead" => first_row_is_header = false,
            _ => {}
        }
    }
    (columns.max(cells), first_row_is_header && rows > 0)
}

fn pre_starts_with_newline(tokens: &[Token], html: &str) -> bool {
    matches!(tokens.first(), Some(Token::Text(range)) if html[range.clone()].starts_with('\n'))
}

/// The text of a `<pre>` element, from the tokens after its start tag.
fn pre_content(html: &str, tokens: &[Token]) -> String {
    let mut content = String::new();
    for token in tokens {
        match token {
            Token::End { name, .. } if name == "pre" => break,
            Token::Text(range) => content.push_str(&html[range.clone()]),
            _ => {}
        }
    }
    content
}

fn collapse_whitespace(text: &str) -> Cow<'_, str> {
    let needs_collapse = text
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0].is_ascii_whitespace() && pair[1].is_ascii_whitespace())
        || text
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() && byte != b' ');
    if !needs_collapse {
        return Cow::Borrowed(text);
    }
    let mut collapsed = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for character in text.chars() {
        if character.is_ascii_whitespace() {
            if !in_whitespace {
                collapsed.push(' ');
            }
            in_whitespace = true;
        } else {
            collapsed.push(character);
            in_whitespace = false;
        }
    }
    Cow::Owned(collapsed)
}

fn parse_text_align(value: &str) -> Option<TextAlign> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" => Some(TextAlign::Left),
        "center" | "middle" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        _ => None,
    }
}

fn parse_dimension(value: &str) -> Option<DefiniteLength> {
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%') {
        percent
            .parse::<f32>()
            .ok()
            .map(|value| relative(value / 100.))
    } else {
        value
            .trim_end_matches("px")
            .parse::<f32>()
            .ok()
            .map(|value| px(value).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_tags_text_and_comments() {
        let html = r#"<p align="center">a &amp; b<!-- c --><img src='x.png' alt=logo/></p>"#;
        let tokens = lex(html);
        assert_eq!(
            tokens,
            vec![
                Token::Start {
                    name: "p".into(),
                    attrs: vec![("align".into(), "center".into())],
                    self_closing: false,
                    range: 0..18,
                },
                Token::Text(18..27),
                Token::Start {
                    name: "img".into(),
                    attrs: vec![
                        ("src".into(), "x.png".into()),
                        ("alt".into(), "logo/".into())
                    ],
                    self_closing: false,
                    range: 37..64,
                },
                Token::End {
                    name: "p".into(),
                    range: 64..68,
                },
            ]
        );
    }

    #[test]
    fn lexes_raw_text_and_stray_angle_brackets() {
        let html = "a < b<script>if (a<b) {}</script>";
        let tokens = lex(html);
        assert_eq!(tokens[0], Token::Text(0..5));
        assert!(matches!(&tokens[1], Token::Start { name, .. } if name == "script"));
        assert_eq!(tokens[2], Token::Text(13..24));
        assert!(matches!(&tokens[3], Token::End { name, .. } if name == "script"));
    }

    fn events(markdown: &str) -> Vec<String> {
        let parsed = crate::parser::parse_markdown_with_options(
            markdown,
            crate::MarkdownOptions {
                parse_html: true,
                ..Default::default()
            },
        );
        let mut depth = 0i32;
        let events = parsed
            .events
            .iter()
            .filter_map(|(range, event)| match event {
                MarkdownEvent::RootStart | MarkdownEvent::RootEnd(_) => None,
                MarkdownEvent::Text => Some(format!("Text({:?})", &markdown[range.clone()])),
                MarkdownEvent::Start(tag) => {
                    depth += 1;
                    Some(match tag {
                        MarkdownTag::Image { width, .. } => format!("Image({width:?})"),
                        MarkdownTag::Heading { level, .. } => format!("Heading({level})"),
                        tag => format!("{tag:?}"),
                    })
                }
                MarkdownEvent::End(tag) => {
                    depth -= 1;
                    Some(format!("/{tag:?}"))
                }
                event => Some(format!("{event:?}")),
            })
            .collect();
        assert_eq!(depth, 0, "unbalanced events for {markdown:?}");
        events
    }

    #[test]
    fn converts_block_html() {
        assert_eq!(
            events(
                "<p align=\"center\">\n  <img src=\"a.png\" width=\"100\" alt=\"A\">\n  <b>bold</b> &amp; more\n</p>\n"
            ),
            [
                "HtmlBlock { align: Some(Center) }",
                "Paragraph",
                "Image(Some(100px))",
                "SubstitutedText(\"A\")",
                "/Image",
                "Strong",
                "Text(\"bold\")",
                "/Strong",
                "SubstitutedText(\" & more \")",
                "/Paragraph",
                "/HtmlBlock",
            ]
        );
    }

    #[test]
    fn html_elements_span_markdown_blocks() {
        assert_eq!(
            events(
                "<div align=\"center\">\n\n# Title\n\n</div>\n\n<details>\n<summary>More</summary>\n\nBody\n\n</details>\n"
            ),
            [
                "HtmlBlock { align: Some(Center) }",
                "Heading(h1)",
                "Text(\"Title\")",
                "/Heading(H1)",
                "/HtmlBlock",
                "HtmlBlock { align: None }",
                "HtmlBlock { align: None }",
                "Strong",
                "Text(\"More\")",
                "/Strong",
                "/HtmlBlock",
                "Paragraph",
                "Text(\"Body\")",
                "/Paragraph",
                "/HtmlBlock",
            ]
        );
    }

    #[test]
    fn converts_inline_html() {
        assert_eq!(
            events("Press <kbd>Ctrl</kbd> and <b>this<br>that\n\nnot bold"),
            [
                "Paragraph",
                "Text(\"Press \")",
                "StartHtmlStyle(Code)",
                "Text(\"Ctrl\")",
                "EndHtmlStyle",
                "Text(\" and \")",
                "Strong",
                "Text(\"this\")",
                "HardBreak",
                "Text(\"that\")",
                "/Strong",
                "/Paragraph",
                "Paragraph",
                "Text(\"not bold\")",
                "/Paragraph",
            ]
        );
    }

    #[test]
    fn converts_tables_and_lists() {
        assert_eq!(
            events(
                "<table>\n<tr><th>A</th><th>B</th></tr>\n<tr><td>1\n</table>\n\n<ul><li>one<li>two</ul>\n"
            ),
            [
                "Table([None, None])",
                "TableHead",
                "TableCell",
                "Text(\"A\")",
                "/TableCell",
                "TableCell",
                "Text(\"B\")",
                "/TableCell",
                "/TableHead",
                "TableRow",
                "TableCell",
                "SubstitutedText(\"1 \")",
                "/TableCell",
                "TableCell",
                "/TableCell",
                "/TableRow",
                "/Table",
                "List(None)",
                "Item",
                "Text(\"one\")",
                "/Item",
                "Item",
                "Text(\"two\")",
                "/Item",
                "/List(false)",
            ]
        );
    }

    #[test]
    fn drops_comments_scripts_and_unknown_tags() {
        assert_eq!(
            events(
                "<!-- note -->\n\n<script>alert('<b>')</script>\n\n<span>kept</span>\n\n<pre>\nfn a() {}\n</pre>\n"
            ),
            [
                "Paragraph",
                "Text(\"kept\")",
                "/Paragraph",
                "CodeBlock { kind: Indented, metadata: CodeBlockMetadata { content_range: 71..81, line_count: 1, is_fenced_closed: false } }",
                "Text(\"fn a() {}\\n\")",
                "/CodeBlock",
            ]
        );
    }

    #[test]
    fn converts_html_documents() {
        let html = "<!doctype html>\n<html>\n  <head><title>T</title><style>p {}</style></head>\n  <body>\n    <h1>Title</h1>\n\n        <p>Indented, not code</p>\n  </body>\n</html>\n";
        let parsed = crate::parser::parse_markdown_with_options(
            html,
            crate::MarkdownOptions {
                html_document: true,
                ..Default::default()
            },
        );
        let events = parsed
            .events
            .iter()
            .filter(|(_, event)| {
                !matches!(event, MarkdownEvent::RootStart | MarkdownEvent::RootEnd(_))
            })
            .map(|(range, event)| match event {
                MarkdownEvent::Text => format!("Text({:?})", &html[range.clone()]),
                MarkdownEvent::Start(MarkdownTag::Heading { .. }) => "Heading".to_string(),
                event => format!("{event:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            events,
            [
                "Heading",
                "Text(\"Title\")",
                "End(Heading(H1))",
                "Start(Paragraph)",
                "Text(\"Indented, not code\")",
                "End(Paragraph)",
            ]
        );
        assert_eq!(parsed.root_block_starts.len(), 2);
    }

    #[test]
    fn unclosed_html_is_closed_at_the_end() {
        events("<div>\n\n- <b>item\n\n<p>text");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(
            decode_entities("a &amp; &#65;&#x42; &bogus; &"),
            "a & AB &bogus; &"
        );
    }
}
