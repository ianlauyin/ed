//! Normalizes Mermaid source before detection: line endings, attribute quotes, front matter,
//! directives, and comments. Port of Pele's `src/preprocess.ts`.

use std::borrow::Cow;

use crate::error::Code;
use crate::error::Error;
use crate::util::chars::has;
use crate::util::chars::is_line_end;
use crate::util::chars::is_space;
use crate::util::chars::is_word;
use crate::yaml::Value;
use crate::yaml::{self};

/// Configuration from front matter and init directives, as JavaScript object properties in order.
pub(crate) type Config = Vec<(String, Value)>;

/// Source with front matter, directives, and comments removed.
#[derive(Debug)]
pub(crate) struct Preprocessed {
    /// Configuration from front matter and init directives.
    pub(crate) config: Config,
    /// The number of front matter lines before `with_comments`.
    pub(crate) line_offset: u32,
    /// The diagram text without comment lines, trimmed at the start.
    pub(crate) text: String,
    /// The front matter title.
    pub(crate) title: Option<String>,
    /// The diagram text before its comment lines are removed.
    pub(crate) with_comments: String,
}

const UNSAFE_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

fn space(c: char) -> bool {
    is_space(u32::from(c))
}

fn line_end(c: char) -> bool {
    is_line_end(u32::from(c))
}

fn word(c: char) -> bool {
    is_word(u32::from(c))
}

/// Removes comment lines (`%%` not followed by `{`) and leading whitespace.
pub(crate) fn cleanup_comments(text: &str) -> String {
    let stripped = if has(text, "%%") { Cow::Owned(remove_comments(text)) } else { Cow::Borrowed(text) };
    stripped.trim_start_matches(space).to_owned()
}

// `text.replace(/^\s*%%(?!{)[^\n]+\n?/gm, '')`.
fn remove_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut p = 0;
    loop {
        let line_start = p == 0 || text.get(..p).and_then(|head| head.chars().next_back()).is_some_and(line_end);
        if line_start {
            if let Some(end) = comment_at(text, p) {
                out.push_str(text.get(last..p).unwrap_or(""));
                last = end;
                p = end;
                continue;
            }
            // Every line start inside this whitespace run reaches the same `%%` test and fails.
            p = text.get(p..).map_or(p, |rest| text.len() - rest.trim_start_matches(space).len());
        }

        let Some((offset, c)) = text.get(p..).and_then(|rest| rest.char_indices().find(|&(_, c)| line_end(c))) else {
            break;
        };
        p += offset + c.len_utf8();
    }
    out.push_str(text.get(last..).unwrap_or(""));
    out
}

// The end of a comment matched at line start `p`. `\s*` cannot give back characters to `%%`, so
// its greedy run is the only candidate.
fn comment_at(text: &str, p: usize) -> Option<usize> {
    let rest = text.get(p..)?;
    let body = rest.trim_start_matches(space).strip_prefix("%%")?;
    if body.starts_with(['{', '\n']) || body.is_empty() {
        return None;
    }

    let start = text.len() - body.len();
    Some(match body.find('\n') {
        Some(at) => start + at + 1,
        None => text.len(),
    })
}

// The byte length of the character at `at` when it is whitespace.
fn space_at(text: &str, at: usize) -> Option<usize> {
    let c = text.get(at..)?.chars().next()?;
    space(c).then_some(c.len_utf8())
}

fn is_break(text: &str, at: usize) -> bool {
    matches!(text.as_bytes().get(at), Some(b'\n' | b'\r'))
}

// The start of the last `needle` that starts at or before `from`.
fn last_index_of(text: &str, needle: &str, from: usize) -> Option<usize> {
    let end = from.saturating_add(needle.len()).min(text.len());
    text.as_bytes().get(..end)?.windows(needle.len()).rposition(|window| window == needle.as_bytes())
}

// The start of the first `needle` that starts at or after `from`.
fn index_of(text: &str, needle: &str, from: usize) -> Option<usize> {
    text.as_bytes()
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle.as_bytes())
        .map(|at| from + at)
}

struct FrontMatter<'a> {
    body: &'a str,
    indent: &'a str,
    length: usize,
}

// Finds a `---` fenced YAML block at the start of the text the way Mermaid's front matter regex does.
fn match_front_matter(text: &str) -> Option<FrontMatter<'_>> {
    let mut cursor = 0;
    while let Some(length) = space_at(text, cursor)
        && !is_break(text, cursor)
    {
        cursor += length;
    }
    let indent = text.get(..cursor)?;
    if !text.get(cursor..)?.starts_with("---") {
        return None;
    }

    let mut opening_ends = Vec::new();
    let mut i = cursor + 3;
    while let Some(length) = space_at(text, i) {
        if is_break(text, i) {
            opening_ends.push(i);
        }
        i += length;
    }
    let first = *opening_ends.first()?;

    let mut fence = String::from(indent);
    fence.push_str("---");
    let closing_end = |i: usize| -> Option<usize> {
        if !is_break(text, i) || !text.get(i + 1..).is_some_and(|t| t.starts_with(&fence)) {
            return None;
        }
        let mut last = None;
        let mut j = i + 1 + fence.len();
        while let Some(length) = space_at(text, j) {
            if is_break(text, j) {
                last = Some(j);
            }
            j += length;
        }
        last
    };

    let mut needle = String::from("\n");
    needle.push_str(&fence);
    let mut last_fence = None;
    let mut at = last_index_of(text, &needle, text.len());
    while let Some(i) = at
        && i > first
    {
        if closing_end(i).is_some() {
            last_fence = Some(i);
            break;
        }
        at = last_index_of(text, &needle, i - 1);
    }
    let last_fence = last_fence?;

    let opening_end = opening_ends.iter().copied().rev().find(|&candidate| candidate < last_fence).unwrap_or(first);

    let body_start = opening_end + 1;
    let mut at = index_of(text, &needle, body_start - 1);
    while let Some(i) = at
        && i <= last_fence
    {
        if i >= body_start
            && let Some(end) = closing_end(i)
        {
            return Some(FrontMatter { body: text.get(body_start..i)?, indent, length: end + 1 });
        }
        at = index_of(text, &needle, i + 1);
    }
    None
}

fn get<'a>(object: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    object.iter().find(|entry| entry.0 == key).map(|entry| &entry.1)
}

fn merge(target: &mut Config, source: &[(String, Value)]) {
    for (key, value) in source {
        if UNSAFE_KEYS.contains(&key.as_str()) {
            continue;
        }
        let current = target.iter_mut().find(|entry| entry.0 == *key);
        match (value, current) {
            (Value::Object(inner), Some((_, Value::Object(current)))) => merge(current, inner),
            (Value::Object(inner), _) => {
                let mut copy = Config::new();
                merge(&mut copy, inner);
                yaml::set(target, key.clone(), Value::Object(copy));
            }
            _ => yaml::set(target, key.clone(), value.clone()),
        }
    }
}

/// One match of Mermaid's directive pattern,
/// `/%{2}{\s*(?:(\w+)\s*:|(\w+))\s*(?:(\w+)|((?:(?!}%{2}).|\r?\n)*))?\s*(?:}%{2})?/gi`.
struct Directive<'a> {
    /// The fourth group: the text after `type:` up to `}%%`, when it is not a single word.
    args: Option<&'a str>,
    end: usize,
    /// The first or second group.
    kind: &'a str,
    start: usize,
}

fn find_directive(s: &str, from: usize) -> Option<Directive<'_>> {
    let mut at = from;
    loop {
        let start = index_of(s, "%%{", at)?;
        if let Some(directive) = directive_at(s, start) {
            return Some(directive);
        }
        at = start + 1;
    }
}

// Every part after the type is optional, so the first greedy choice at each step is the match.
fn directive_at(s: &str, start: usize) -> Option<Directive<'_>> {
    let skip_space = |at: usize| s.get(at..).map_or(at, |rest| s.len() - rest.trim_start_matches(space).len());
    let skip_word = |at: usize| s.get(at..).map_or(at, |rest| s.len() - rest.trim_start_matches(word).len());

    let word_start = skip_space(start + 3);
    let word_end = skip_word(word_start);
    if word_end == word_start {
        return None;
    }
    let kind = s.get(word_start..word_end)?;

    let mut pos = skip_space(word_end);
    if s.as_bytes().get(pos) == Some(&b':') {
        pos += 1;
    } else {
        pos = word_end;
    }
    pos = skip_space(pos);

    let mut args = None;
    let value_end = skip_word(pos);
    if value_end > pos {
        pos = value_end;
    } else {
        let args_start = pos;
        while let Some(rest) = s.get(pos..) {
            if rest.starts_with("}%%") {
                break;
            }
            if rest.starts_with("\r\n") {
                pos += 2;
                continue;
            }
            match rest.chars().next() {
                Some('\n') => pos += 1,
                Some(c) if !line_end(c) => pos += c.len_utf8(),
                _ => break,
            }
        }
        args = s.get(args_start..pos);
    }

    pos = skip_space(pos);
    if s.get(pos..).is_some_and(|rest| rest.starts_with("}%%")) {
        pos += 3;
    }
    Some(Directive { args, end: pos, kind, start })
}

fn directives(text: &str, config: &mut Config) -> String {
    if !has(text, "%%{") {
        return text.to_owned();
    }

    let normalized = text.trim_matches(space).chars().map(|c| if c == '\'' { '"' } else { c }).collect::<String>();
    let mut from = 0;
    while let Some(directive) = find_directive(&normalized, from) {
        from = directive.end;
        match directive.kind {
            "wrap" => yaml::set(config, "wrap".to_owned(), Value::Bool(true)),
            "init" | "initialize" => {
                // Mermaid ignores an init directive whose body is not valid JSON.
                if let Some(args) = directive.args.filter(|args| !args.is_empty())
                    && let Some(Value::Object(args)) = json(args.trim_matches(space))
                {
                    merge(config, &args);
                }
            }
            _ => {}
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    while let Some(directive) = find_directive(text, last) {
        out.push_str(text.get(last..directive.start).unwrap_or(""));
        last = directive.end;
    }
    out.push_str(text.get(last..).unwrap_or(""));
    out
}

/// Deepest nesting `JSON.parse` reads here. V8 has no fixed limit, but a deeper directive is
/// ignored instead of overflowing the stack.
const MAX_JSON_DEPTH: usize = 1000;

/// `JSON.parse`, or `None` where it throws.
pub(crate) fn json(text: &str) -> Option<Value> {
    let mut parser = Json { pos: 0, text };
    let value = parser.value(0)?;
    parser.skip();
    (parser.pos == text.len()).then_some(value)
}

struct Json<'a> {
    pos: usize,
    text: &'a str,
}

impl Json<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn skip(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> bool {
        let found = self.text.get(self.pos..).is_some_and(|rest| rest.starts_with(literal));
        if found {
            self.pos += literal.len();
        }
        found
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_JSON_DEPTH {
            return None;
        }

        self.skip();
        match self.peek()? {
            b'{' => {
                self.pos += 1;
                let mut out = Vec::new();
                self.skip();
                if self.eat("}") {
                    return Some(Value::Object(out));
                }
                loop {
                    self.skip();
                    let key = self.string()?;
                    self.skip();
                    if !self.eat(":") {
                        return None;
                    }
                    let value = self.value(depth + 1)?;
                    yaml::set(&mut out, key, value);
                    self.skip();
                    if self.eat("}") {
                        return Some(Value::Object(out));
                    }
                    if !self.eat(",") {
                        return None;
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                let mut out = Vec::new();
                self.skip();
                if self.eat("]") {
                    return Some(Value::Array(out));
                }
                loop {
                    out.push(self.value(depth + 1)?);
                    self.skip();
                    if self.eat("]") {
                        return Some(Value::Array(out));
                    }
                    if !self.eat(",") {
                        return None;
                    }
                }
            }
            b'"' => self.string().map(Value::String),
            b't' => self.eat("true").then_some(Value::Bool(true)),
            b'f' => self.eat("false").then_some(Value::Bool(false)),
            b'n' => self.eat("null").then_some(Value::Null),
            _ => self.number(),
        }
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat("\"") {
            return None;
        }

        // Escapes may pair surrogates, so the text is built from UTF-16 units.
        let mut out: Vec<u16> = Vec::new();
        loop {
            let rest = self.text.get(self.pos..)?;
            let c = rest.chars().next()?;
            self.pos += c.len_utf8();
            match c {
                '"' => return Some(String::from_utf16_lossy(&out)),
                '\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    let unit = match e {
                        b'"' => 0x22,
                        b'\\' => 0x5c,
                        b'/' => 0x2f,
                        b'b' => 8,
                        b'f' => 12,
                        b'n' => 10,
                        b'r' => 13,
                        b't' => 9,
                        b'u' => {
                            let hex = self.text.get(self.pos..self.pos + 4)?;
                            self.pos += 4;
                            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                                return None;
                            }
                            u16::from_str_radix(hex, 16).ok()?
                        }
                        _ => return None,
                    };
                    out.push(unit);
                }
                c if u32::from(c) < 0x20 => return None,
                c => out.extend_from_slice(c.encode_utf16(&mut [0; 2])),
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.pos;
        let digits = |parser: &mut Self| {
            let from = parser.pos;
            while parser.peek().is_some_and(|b| b.is_ascii_digit()) {
                parser.pos += 1;
            }
            parser.pos - from
        };

        self.eat("-");
        if !self.eat("0") && digits(self) == 0 {
            return None;
        }
        if self.eat(".") && digits(self) == 0 {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if !self.eat("+") {
                self.eat("-");
            }
            if digits(self) == 0 {
                return None;
            }
        }
        let number = crate::util::float::parse(self.text.get(start..self.pos)?)?;
        Some(Value::Number(number))
    }
}

// `/<(\w+)([^>]*)>/g` with `="..."` attribute quotes turned to single quotes.
fn quote_attributes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut at = 0;
    while let Some(offset) = text.get(at..).and_then(|rest| rest.find('<')) {
        let open = at + offset;
        let name_end = text.get(open + 1..).map_or(open + 1, |rest| text.len() - rest.trim_start_matches(word).len());
        let close = text.get(name_end..).and_then(|rest| rest.find('>'));
        let (true, Some(close)) = (name_end > open + 1, close) else {
            at = open + 1;
            continue;
        };
        let close = name_end + close;
        out.push_str(text.get(last..name_end).unwrap_or(""));
        push_single_quoted(&mut out, text.get(name_end..close).unwrap_or(""));
        out.push('>');
        last = close + 1;
        at = last;
    }
    out.push_str(text.get(last..).unwrap_or(""));
    out
}

// `attrs.replace(/="([^"]*)"/g, "='$1'")`.
fn push_single_quoted(out: &mut String, attrs: &str) {
    let mut last = 0;
    let mut at = 0;
    while let Some(offset) = attrs.get(at..).and_then(|rest| rest.find("=\"")) {
        let open = at + offset;
        let Some(length) = attrs.get(open + 2..).and_then(|rest| rest.find('"')) else {
            at = open + 1;
            continue;
        };
        out.push_str(attrs.get(last..open).unwrap_or(""));
        out.push_str("='");
        out.push_str(attrs.get(open + 2..open + 2 + length).unwrap_or(""));
        out.push('\'');
        last = open + 3 + length;
        at = last;
    }
    out.push_str(attrs.get(last..).unwrap_or(""));
}

/// Normalizes line endings and attribute quotes, reads front matter and directives into the title
/// and config, and removes comments.
pub(crate) fn preprocess(source: &str) -> Result<Preprocessed, Error> {
    let mut text = if source.contains('\r') {
        let mut normalized = String::with_capacity(source.len());
        let mut chars = source.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                chars.next_if_eq(&'\n');
                normalized.push('\n');
            } else {
                normalized.push(c);
            }
        }
        normalized
    } else {
        source.to_owned()
    };

    // A tag needs a closing bracket, so nothing after the last one can match.
    let last_close = if text.contains('<') { text.rfind('>') } else { None };
    if let Some(last_close) = last_close
        && last_close > 0
    {
        let mut quoted = quote_attributes(text.get(..=last_close).unwrap_or(""));
        quoted.push_str(text.get(last_close + 1..).unwrap_or(""));
        text = quoted;
    }

    let mut config = Config::new();
    let mut title = None;
    let mut line_offset = 0;

    let front = if has(&text, "---") { match_front_matter(&text) } else { None };
    if let Some(front) = front {
        let body = if front.indent.is_empty() {
            front.body.to_owned()
        } else {
            let mut body = String::with_capacity(front.body.len());
            for (i, line) in front.body.split('\n').enumerate() {
                if i > 0 {
                    body.push('\n');
                }
                body.push_str(line.strip_prefix(front.indent).unwrap_or(line));
            }
            body
        };

        let parsed = yaml::parse(&body).map_err(|message| {
            let mut full = String::from("Front matter is not valid YAML. ");
            full.push_str(&message);
            Error::new(Code::Syntax, full)
        })?;
        if let Value::Object(parsed) = &parsed {
            if let Some(value) = get(parsed, "title")
                && value.truthy()
            {
                let mut out = String::new();
                value.push_js_string(&mut out);
                title = Some(out);
            }
            if let Some(Value::Object(inner)) = get(parsed, "config") {
                merge(&mut config, inner);
            }
            if let Some(value) = get(parsed, "displayMode")
                && value.truthy()
            {
                let mut mode = String::new();
                value.push_js_string(&mut mode);
                let entry = ("displayMode".to_owned(), Value::String(mode));
                match config.iter_mut().find(|entry| entry.0 == "gantt") {
                    Some((_, Value::Object(gantt))) => yaml::set(gantt, entry.0, entry.1),
                    _ => yaml::set(&mut config, "gantt".to_owned(), Value::Object(vec![entry])),
                }
            }
        }

        let head = text.get(..front.length).unwrap_or("");
        line_offset = u32::try_from(head.matches('\n').count()).unwrap_or(u32::MAX);
        text = text.get(front.length..).unwrap_or("").to_owned();
    }

    let with_comments = directives(&text, &mut config);
    Ok(Preprocessed { config, line_offset, text: cleanup_comments(&with_comments), title, with_comments })
}

// Does what Mermaid's `text.replace(/keyword.*:\S*#.*;/g, drop the last character)` does: on a
// line where the keyword is followed by a colon, a color-like `#`, and later a semicolon, the last
// semicolon goes. Mermaid's regex backtracks without bound; this is one pass.
fn drop_color_semicolons<'a>(text: &'a str, keyword: &str) -> Cow<'a, str> {
    let Some(mut from) = text.find(keyword) else {
        return Cow::Borrowed(text);
    };

    let mut out = String::new();
    let mut last = 0;
    loop {
        let (end, width) = text
            .get(from..)
            .and_then(|rest| rest.char_indices().find(|&(_, c)| line_end(c)))
            .map_or((text.len(), 0), |(at, c)| (from + at, c.len_utf8()));

        let mut after_colon = false;
        let mut hash = None;
        let scanned = text.get(from + keyword.len()..end).unwrap_or("");
        for (j, c) in scanned.char_indices() {
            if c == ':' {
                after_colon = true;
            } else if c == '#' {
                if after_colon {
                    hash = Some(from + keyword.len() + j);
                    break;
                }
            } else if space(c) {
                after_colon = false;
            }
        }
        if let Some(hash) = hash
            && let Some(semi) = text.get(..end).and_then(|head| head.rfind(';'))
            && semi > hash
        {
            out.push_str(text.get(last..semi).unwrap_or(""));
            last = semi + 1;
        }

        if end >= text.len() {
            break;
        }
        let next = end + width;
        match text.get(next..).and_then(|rest| rest.find(keyword)) {
            Some(at) => from = next + at,
            None => break,
        }
    }

    if last == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(text.get(last..).unwrap_or(""));
    Cow::Owned(out)
}

/// Hides `#name;` entity codes behind Mermaid's placeholder characters so `#` and `;` survive
/// parsing.
pub(crate) fn encode_entities(text: &str) -> Cow<'_, str> {
    if !text.contains('#') {
        return Cow::Borrowed(text);
    }

    let styled = drop_color_semicolons(text, "style");
    let dropped = drop_color_semicolons(&styled, "classDef");
    let mut out = String::with_capacity(dropped.len() + 16);
    let mut last = 0;
    let mut at = 0;
    while let Some(offset) = dropped.get(at..).and_then(|rest| rest.find('#')) {
        let hash = at + offset;
        let rest = dropped.get(hash + 1..).unwrap_or("");
        let inner = rest.get(..rest.len() - rest.trim_start_matches(word).len()).unwrap_or("");
        if inner.is_empty() || !rest.get(inner.len()..).is_some_and(|tail| tail.starts_with(';')) {
            at = hash + 1;
            continue;
        }

        out.push_str(dropped.get(last..hash).unwrap_or(""));
        out.push_str(if inner.bytes().all(|b| b.is_ascii_digit()) { "\u{fb02}\u{b0}\u{b0}" } else { "\u{fb02}\u{b0}" });
        out.push_str(inner);
        out.push_str("\u{b6}\u{df}");
        last = hash + inner.len() + 2;
        at = last;
    }
    out.push_str(dropped.get(last..).unwrap_or(""));
    Cow::Owned(out)
}
