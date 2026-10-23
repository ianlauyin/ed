//! The body of a `json` node: a JSON object whose keys keep the order they were written in, with
//! the errors Node's `JSON.parse` reports. Port of Pele's `src/diagrams/usecase/json.ts`.
//!
//! Pele parses with `JSON.parse` and walks the text again for its key order. This reads the text
//! once, as V8's JSON parser does, and reports V8's messages at V8's positions.

use super::lexer::{push_usize, utf16};
use crate::error::{Code, Error, push_u32};
use crate::util::chars::{is_digit, is_hex, is_word};

/// A JSON value in a [`JsonDoc`]. Children are indices into the document's nodes, so deep
/// nesting needs no recursion to build, read, or drop.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    /// An array of nodes.
    Array(Vec<usize>),
    /// `true` or `false`.
    Bool(bool),
    /// `null`.
    Null,
    /// A number.
    Number(f64),
    /// An object: each key once, where it was first written, with the last value written for it.
    Object(Vec<(String, usize)>),
    /// A string. A lone surrogate escape becomes U+FFFD.
    String(String),
}

/// A parsed JSON document. The root is the first node.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct JsonDoc {
    /// Every value, parents before their children.
    pub(crate) nodes: Vec<JsonValue>,
}

impl JsonDoc {
    /// The node at `index`.
    pub(crate) fn get(&self, index: usize) -> Option<&JsonValue> {
        self.nodes.get(index)
    }
}

/// Parses the body of a `json` node. `origin` is the line and column the text starts at in the
/// diagram.
///
/// # Errors
///
/// Returns `Invalid JSON: <V8's message>` at the position V8 names, mapped into the diagram, or
/// an error when the root is not an object.
pub(crate) fn parse_ordered_json(text: &str, origin: (u32, u32)) -> Result<JsonDoc, Error> {
    let (line, column) = origin;
    let doc = match V8::parse(text) {
        Ok(doc) => doc,
        Err(message) => {
            let (line, column) = location_from_message(&message, text, line, column)
                .unwrap_or_else(|| location_at(text, walk_error(text), line, column));
            let mut full = String::from("Invalid JSON: ");
            full.push_str(&message);
            return Err(json_error(full, line, column));
        }
    };
    if !matches!(doc.get(0), Some(JsonValue::Object(_))) {
        return Err(json_error(
            String::from("JSON value must have an object root"),
            line,
            column,
        ));
    }
    Ok(doc)
}

#[cold]
fn json_error(mut message: String, line: u32, column: u32) -> Error {
    message.push_str(" (line ");
    push_u32(&mut message, line);
    message.push_str(", column ");
    push_u32(&mut message, column);
    message.push(')');
    Error {
        code: Code::Syntax,
        column,
        line,
        message,
        snippet: String::new(),
        r#type: Some("usecase"),
    }
}

/// The byte at `i`, or a value no test matches past the end.
fn at(b: &[u8], i: usize) -> u32 {
    b.get(i).map_or(u32::MAX, |&c| u32::from(c))
}

/// An open object or array, waiting for its next value.
enum Frame {
    /// An array, by node.
    Array(usize),
    /// An object, by node, and the key whose value comes next.
    Object(usize, String),
}

/// V8's JSON parser, reduced to what decides its result and its errors.
struct V8<'a> {
    /// The cursor.
    at: usize,
    /// The text as bytes.
    b: &'a [u8],
    /// The values read so far.
    nodes: Vec<JsonValue>,
    /// The text.
    text: &'a str,
}

impl V8<'_> {
    /// Parses `text`, or returns V8's message for it.
    fn parse(text: &str) -> Result<JsonDoc, String> {
        let mut parser = V8 {
            at: 0,
            b: text.as_bytes(),
            nodes: Vec::new(),
            text,
        };
        parser.run()?;
        Ok(JsonDoc {
            nodes: parser.nodes,
        })
    }

    /// Skips JSON whitespace.
    fn skip(&mut self) {
        while matches!(self.b.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// The byte at the cursor.
    fn peek(&self) -> u32 {
        at(self.b, self.at)
    }

    /// Adds a node and returns its index.
    fn push(&mut self, value: JsonValue) -> usize {
        self.nodes.push(value);
        self.nodes.len() - 1
    }

    /// `template` at byte `pos`, with V8's position, line, and column.
    #[cold]
    fn positioned(&self, template: &str, pos: usize) -> String {
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let end = utf16(self.text, pos);
        // JSON allows only CR and LF as line terminators, and CRLF counts once.
        let mut line: u32 = 1;
        let mut last_break = 0;
        let mut cursor = 0;
        while cursor < end {
            let unit = units.get(cursor).copied().unwrap_or(0);
            if unit == 13 && cursor + 1 < end && units.get(cursor + 1) == Some(&10) {
                cursor += 1;
            }
            let unit = units.get(cursor).copied().unwrap_or(0);
            if unit == 13 || unit == 10 {
                line = line.saturating_add(1);
                last_break = cursor + 1;
            }
            cursor += 1;
        }
        let mut out = String::from(template);
        out.push_str(" in JSON at position ");
        push_usize(&mut out, end);
        out.push_str(" (line ");
        push_u32(&mut out, line);
        out.push_str(" column ");
        push_usize(&mut out, 1 + cursor - last_break);
        out.push(')');
        out
    }

    /// V8's message for the character at byte `pos` where no token may start.
    #[cold]
    fn unexpected(&self, pos: usize) -> String {
        /// How many characters V8 quotes on each side of the error.
        const CONTEXT: usize = 10;
        let c = at(self.b, pos);
        if c == u32::MAX {
            return String::from("Unexpected end of JSON input");
        }
        if c == 34 {
            return self.positioned("Unexpected string", pos);
        }
        if c == 45 || is_digit(c) {
            return self.positioned("Unexpected number", pos);
        }
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let length = units.len();
        let p = utf16(self.text, pos);
        let mut out = String::from("Unexpected token '");
        if let Some(ch) = self.text.get(pos..).and_then(|rest| rest.chars().next()) {
            out.push(ch);
        }
        out.push_str("', ");
        let slice = |from: usize, to: usize| {
            String::from_utf16_lossy(units.get(from..to.min(length)).unwrap_or(&[]))
        };
        if length < CONTEXT * 2 + 1 {
            out.push('"');
            out.push_str(self.text);
            out.push_str("\" is not valid JSON");
        } else if p < CONTEXT {
            out.push('"');
            out.push_str(&slice(0, p + CONTEXT));
            out.push_str("\"... is not valid JSON");
        } else if p < length - CONTEXT {
            out.push_str("...\"");
            out.push_str(&slice(p - CONTEXT, p + CONTEXT));
            out.push_str("\"... is not valid JSON");
        } else {
            out.push_str("...\"");
            out.push_str(&slice(p - CONTEXT, length));
            out.push_str("\" is not valid JSON");
        }
        out
    }

    /// Reads a key after the comma or brace before it: its string, then a colon.
    fn key(&mut self, missing: &str) -> Result<String, String> {
        self.skip();
        if self.peek() != 34 {
            return Err(self.positioned(missing, self.at));
        }
        self.at += 1;
        let key = self.string()?;
        self.skip();
        if self.peek() != 58 {
            return Err(self.positioned("Expected ':' after property name", self.at));
        }
        self.at += 1;
        Ok(key)
    }

    /// Reads one value and checks that only whitespace follows it.
    fn run(&mut self) -> Result<(), String> {
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            let mut value = loop {
                self.skip();
                let c = self.peek();
                match c {
                    34 => {
                        self.at += 1;
                        let text = self.string()?;
                        break self.push(JsonValue::String(text));
                    }
                    45 | 48..=57 => {
                        let number = self.number()?;
                        break self.push(JsonValue::Number(number));
                    }
                    123 => {
                        self.at += 1;
                        self.skip();
                        let node = self.push(JsonValue::Object(Vec::new()));
                        if self.peek() == 125 {
                            self.at += 1;
                            break node;
                        }
                        let key = self.key("Expected property name or '}'")?;
                        stack.push(Frame::Object(node, key));
                    }
                    91 => {
                        self.at += 1;
                        self.skip();
                        let node = self.push(JsonValue::Array(Vec::new()));
                        if self.peek() == 93 {
                            self.at += 1;
                            break node;
                        }
                        stack.push(Frame::Array(node));
                    }
                    116 => {
                        self.literal(b"true")?;
                        break self.push(JsonValue::Bool(true));
                    }
                    102 => {
                        self.literal(b"false")?;
                        break self.push(JsonValue::Bool(false));
                    }
                    110 => {
                        self.literal(b"null")?;
                        break self.push(JsonValue::Null);
                    }
                    _ => return Err(self.unexpected(self.at)),
                }
            };

            loop {
                match stack.last_mut() {
                    None => {
                        self.skip();
                        if self.at < self.b.len() {
                            return Err(self.positioned(
                                "Unexpected non-whitespace character after JSON",
                                self.at,
                            ));
                        }
                        return Ok(());
                    }
                    Some(Frame::Object(node, key)) => {
                        let node = *node;
                        let key = std::mem::take(key);
                        if let Some(JsonValue::Object(entries)) = self.nodes.get_mut(node) {
                            // ponytail: a linear search per key, quadratic in an object's size.
                            match entries.iter_mut().find(|(own, _)| *own == key) {
                                Some(entry) => entry.1 = value,
                                None => entries.push((key, value)),
                            }
                        }
                        self.skip();
                        if self.peek() == 44 {
                            self.at += 1;
                            let next = self.key("Expected double-quoted property name")?;
                            if let Some(Frame::Object(_, key)) = stack.last_mut() {
                                *key = next;
                            }
                            break;
                        }
                        if self.peek() != 125 {
                            return Err(self
                                .positioned("Expected ',' or '}' after property value", self.at));
                        }
                        self.at += 1;
                        stack.pop();
                        value = node;
                    }
                    Some(Frame::Array(node)) => {
                        let node = *node;
                        if let Some(JsonValue::Array(items)) = self.nodes.get_mut(node) {
                            items.push(value);
                        }
                        self.skip();
                        if self.peek() == 44 {
                            self.at += 1;
                            break;
                        }
                        if self.peek() != 93 {
                            return Err(
                                self.positioned("Expected ',' or ']' after array element", self.at)
                            );
                        }
                        self.at += 1;
                        stack.pop();
                        value = node;
                    }
                }
            }
        }
    }

    /// Reads a string whose opening quote is behind the cursor.
    fn string(&mut self) -> Result<String, String> {
        let mut units: Vec<u16> = Vec::new();
        loop {
            let c = self.peek();
            if c == u32::MAX {
                return Err(self.positioned("Unterminated string", self.at));
            }
            if c == 34 {
                self.at += 1;
                return Ok(String::from_utf16_lossy(&units));
            }
            if c < 0x20 {
                return Err(self.positioned("Bad control character in string literal", self.at));
            }
            if c != 92 {
                let Some(ch) = self
                    .text
                    .get(self.at..)
                    .and_then(|rest| rest.chars().next())
                else {
                    return Err(self.positioned("Unterminated string", self.at));
                };
                let mut buffer = [0u16; 2];
                units.extend_from_slice(ch.encode_utf16(&mut buffer));
                self.at += ch.len_utf8();
                continue;
            }
            self.at += 1;
            let unit = match self.peek() {
                34 => 34,
                92 => 92,
                47 => 47,
                98 => 8,
                102 => 12,
                110 => 10,
                114 => 13,
                116 => 9,
                117 => {
                    let mut value: u16 = 0;
                    for _ in 0..4 {
                        self.at += 1;
                        let digit = self.peek();
                        if !is_hex(digit) {
                            return Err(self.positioned("Bad Unicode escape", self.at));
                        }
                        let digit = char::from_u32(digit)
                            .and_then(|d| d.to_digit(16))
                            .unwrap_or(0);
                        value = value
                            .wrapping_mul(16)
                            .wrapping_add(u16::try_from(digit).unwrap_or(0));
                    }
                    value
                }
                _ => return Err(self.positioned("Bad escaped character", self.at)),
            };
            units.push(unit);
            self.at += 1;
        }
    }

    /// Reads a number at the cursor.
    fn number(&mut self) -> Result<f64, String> {
        let start = self.at;
        if self.peek() == 45 {
            self.at += 1;
        }
        if self.peek() == 48 {
            self.at += 1;
            if is_digit(self.peek()) {
                return Err(self.unexpected_number());
            }
        } else {
            if !is_digit(self.peek()) {
                return Err(self.positioned("No number after minus sign", self.at));
            }
            while is_digit(self.peek()) {
                self.at += 1;
            }
        }
        if self.peek() == 46 {
            self.at += 1;
            if !is_digit(self.peek()) {
                return Err(self.positioned("Unterminated fractional number", self.at));
            }
            while is_digit(self.peek()) {
                self.at += 1;
            }
        }
        if self.peek() | 32 == 101 {
            self.at += 1;
            if self.peek() == 43 || self.peek() == 45 {
                self.at += 1;
            }
            if !is_digit(self.peek()) {
                return Err(self.positioned("Exponent part is missing a number", self.at));
            }
            while is_digit(self.peek()) {
                self.at += 1;
            }
        }
        Ok(self
            .text
            .get(start..self.at)
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(0.0))
    }

    #[cold]
    fn unexpected_number(&self) -> String {
        self.positioned("Unexpected number", self.at)
    }

    /// Reads `word`, whose first character was already seen.
    fn literal(&mut self, word: &[u8]) -> Result<(), String> {
        let remaining = self.b.len() - self.at;
        if self.b.get(self.at + 1..self.at + word.len()) == word.get(1..) {
            self.at += word.len();
            return Ok(());
        }
        self.at += 1;
        for i in 0..(word.len() - 1).min(remaining - 1) {
            if word.get(1 + i).map(|&c| u32::from(c)) != Some(self.peek()) {
                return Err(self.unexpected(self.at));
            }
            self.at += 1;
        }
        Err(String::from("Unexpected end of JSON input"))
    }
}

/// The line and column of UTF-16 `offset` in `text`, which starts at `line` and `column`.
fn location_at(text: &str, offset: usize, mut line: u32, mut column: u32) -> (u32, u32) {
    let mut previous = 0u16;
    for (i, unit) in text.encode_utf16().take(offset).enumerate() {
        if unit == 13 {
            line = line.saturating_add(1);
            column = 1;
        } else if unit == 10 {
            if i == 0 || previous != 13 {
                line = line.saturating_add(1);
                column = 1;
            }
        } else {
            column = column.saturating_add(1);
        }
        previous = unit;
    }
    (line, column)
}

/// The digits starting at `i`, as a number, and where they end. Saturates past `u32::MAX`.
fn digits(b: &[u8], i: usize) -> Option<(u32, usize)> {
    let mut end = i;
    let mut value: u32 = 0;
    while is_digit(at(b, end)) {
        value = value.saturating_mul(10).saturating_add(at(b, end) - 48);
        end += 1;
    }
    (end > i).then_some((value, end))
}

/// Whether a `\b` sits before byte `i`, at the start of a word.
fn word_start(b: &[u8], i: usize) -> bool {
    i == 0 || !is_word(at(b, i - 1))
}

/// Pele's `/\bposition (\d+)\b/`.
fn find_position(b: &[u8]) -> Option<u32> {
    (0..b.len()).find_map(|i| {
        if !word_start(b, i) || b.get(i..i + 9) != Some(b"position ") {
            return None;
        }
        digits(b, i + 9).and_then(|(value, end)| (!is_word(at(b, end))).then_some(value))
    })
}

/// Pele's `/\bline (\d+) column (\d+)\b/`.
fn find_line_column(b: &[u8]) -> Option<(u32, u32)> {
    (0..b.len()).find_map(|i| {
        if !word_start(b, i) || b.get(i..i + 5) != Some(b"line ") {
            return None;
        }
        let (line, end) = digits(b, i + 5)?;
        if b.get(end..end + 8) != Some(b" column ") {
            return None;
        }
        let (column, end) = digits(b, end + 8)?;
        (!is_word(at(b, end))).then_some((line, column))
    })
}

/// Whether `b` contains `word`, ignoring ASCII case.
fn contains_no_case(b: &[u8], word: &str) -> bool {
    b.windows(word.len())
        .any(|window| window.eq_ignore_ascii_case(word.as_bytes()))
}

/// Engines word `JSON.parse` errors differently: some give an offset, some a line and column.
fn location_from_message(message: &str, text: &str, line: u32, column: u32) -> Option<(u32, u32)> {
    let b = message.as_bytes();
    if let Some(position) = find_position(b) {
        let offset = usize::try_from(position).unwrap_or(usize::MAX);
        return Some(location_at(text, offset, line, column));
    }
    if let Some((local_line, local_column)) = find_line_column(b) {
        return Some((
            line.saturating_add(local_line).saturating_sub(1),
            if local_line == 1 {
                column.saturating_add(local_column).saturating_sub(1)
            } else {
                local_column
            },
        ));
    }
    if contains_no_case(b, "unexpected end") || contains_no_case(b, "end of json input") {
        return Some(location_at(text, usize::MAX, line, column));
    }
    None
}

/// Where Pele's key-order walk of `text` stops, as a UTF-16 offset: the first character that is
/// not valid JSON.
fn walk_error(text: &str) -> usize {
    let b = text.as_bytes();
    let offset = walk(b).err().unwrap_or(0);
    utf16(text, offset)
}

/// Pele's `propertyOrder` walk, kept for where it fails. Each frame is whether it is an object.
fn walk(b: &[u8]) -> Result<(), usize> {
    #[derive(PartialEq)]
    enum State {
        /// After a value.
        After,
        /// At an object key.
        Key,
        /// At a value.
        Value,
    }
    let n = b.len();
    let mut i = 0;
    let skip = |i: &mut usize| {
        while matches!(b.get(*i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            *i += 1;
        }
    };
    let read_string = |i: &mut usize| -> Result<(), usize> {
        *i += 1;
        while *i < n {
            let start = *i;
            let ch = at(b, *i);
            *i += 1;
            if ch == 34 {
                return Ok(());
            }
            if ch < 0x20 {
                return Err(start);
            }
            if ch != 92 {
                continue;
            }
            let escape_at = *i;
            let escape = at(b, *i);
            *i += 1;
            if escape == u32::from(b'u') {
                if !(0..4).all(|k| is_hex(at(b, *i + k))) {
                    return Err(*i);
                }
                *i += 4;
            } else if !b"\"\\/bfnrt".iter().any(|&c| u32::from(c) == escape) {
                return Err(escape_at);
            }
        }
        Err(*i)
    };
    let mut stack: Vec<bool> = Vec::new();
    let mut state = State::Value;
    skip(&mut i);
    loop {
        match state {
            State::Value => {
                skip(&mut i);
                let c = at(b, i);
                state = State::After;
                if c == 123 {
                    i += 1;
                    skip(&mut i);
                    if at(b, i) == 125 {
                        i += 1;
                    } else {
                        stack.push(true);
                        state = State::Key;
                    }
                } else if c == 91 {
                    i += 1;
                    skip(&mut i);
                    if at(b, i) == 93 {
                        i += 1;
                    } else {
                        stack.push(false);
                        state = State::Value;
                    }
                } else if c == 34 {
                    read_string(&mut i)?;
                } else if c == 116 || c == 102 || c == 110 {
                    let word: &[u8] = match c {
                        116 => b"true",
                        102 => b"false",
                        _ => b"null",
                    };
                    for (k, &w) in word.iter().enumerate() {
                        if at(b, i + k) != u32::from(w) {
                            return Err(i + k);
                        }
                    }
                    i += word.len();
                } else if c == 45 || is_digit(c) {
                    if at(b, i) == 45 {
                        i += 1;
                    }
                    if at(b, i) == 48 {
                        i += 1;
                    } else if is_digit(at(b, i)) {
                        while is_digit(at(b, i)) {
                            i += 1;
                        }
                    } else {
                        return Err(i);
                    }
                    if at(b, i) == 46 {
                        i += 1;
                        if !is_digit(at(b, i)) {
                            return Err(i);
                        }
                        while is_digit(at(b, i)) {
                            i += 1;
                        }
                    }
                    if at(b, i) | 32 == 101 {
                        i += 1;
                        if at(b, i) == 43 || at(b, i) == 45 {
                            i += 1;
                        }
                        if !is_digit(at(b, i)) {
                            return Err(i);
                        }
                        while is_digit(at(b, i)) {
                            i += 1;
                        }
                    }
                } else {
                    return Err(i);
                }
            }
            State::Key => {
                if at(b, i) != 34 {
                    return Err(i);
                }
                read_string(&mut i)?;
                skip(&mut i);
                if at(b, i) != 58 {
                    return Err(i);
                }
                i += 1;
                state = State::Value;
            }
            State::After => {
                let Some(&object) = stack.last() else {
                    break;
                };
                skip(&mut i);
                let c = at(b, i);
                if c == if object { 125 } else { 93 } {
                    i += 1;
                    stack.pop();
                } else if c != 44 {
                    return Err(i);
                } else {
                    i += 1;
                    skip(&mut i);
                    state = if object { State::Key } else { State::Value };
                }
            }
        }
    }
    skip(&mut i);
    if i != n {
        return Err(i);
    }
    Ok(())
}
