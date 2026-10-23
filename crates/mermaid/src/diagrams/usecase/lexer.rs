//! The tokens of Mermaid's Chevrotain lexer for use case diagrams, and source positions for
//! errors. Port of Pele's `src/diagrams/usecase/lexer.ts`.
//!
//! Offsets are UTF-8 bytes. Every character the lexer tests is ASCII, so a byte outside ASCII
//! fails each test as a non-ASCII UTF-16 unit would. Positions in messages are UTF-16.

use super::tokens::T;
use crate::error::{Code, Error, push_u32};
use crate::text::label::trim;
use crate::util::chars::{is_blank, is_digit, is_hex, is_letter, is_word};

/// A token: its type and its byte range in the source.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tok {
    /// End offset, exclusive.
    pub(crate) end: usize,
    /// Token type.
    pub(crate) kind: T,
    /// Start offset.
    pub(crate) start: usize,
}

/// The byte at `i`, or a value no test matches past the end, as `charCodeAt` gives `NaN`.
fn at(b: &[u8], i: usize) -> u32 {
    b.get(i).map_or(u32::MAX, |&c| u32::from(c))
}

/// Whether `word` is at `i`.
fn starts(b: &[u8], i: usize, word: &[u8]) -> bool {
    b.get(i..i + word.len()) == Some(word)
}

/// Mermaid's INCLUDE and EXTEND are the only keywords matched without regard to case.
fn starts_no_case(b: &[u8], i: usize, word: &[u8]) -> bool {
    word.iter()
        .enumerate()
        .all(|(k, &c)| at(b, i + k) | 32 == u32::from(c))
}

/// The length of each keyword.
fn keyword_length(t: T) -> usize {
    match t {
        T::Usecase => 12,
        T::Actor | T::Style | T::Class | T::False => 5,
        T::SystemBoundary => 14,
        T::End | T::For => 3,
        T::Direction => 9,
        T::Note | T::Json | T::True => 4,
        T::ClassDef => 8,
        T::Include => 7,
        T::Extend => 6,
        _ => 2,
    }
}

/// The keyword that is a prefix of the text at `i`, tried in the order of Mermaid's token list.
fn keyword_at(b: &[u8], i: usize, c: u8) -> Option<T> {
    let found = |word: &[u8], t: T| starts(b, i, word).then_some(t);
    match c {
        b'a' => found(b"actor", T::Actor),
        b's' => found(b"systemBoundary", T::SystemBoundary).or_else(|| found(b"style", T::Style)),
        b'e' => {
            found(b"end", T::End).or_else(|| starts_no_case(b, i, b"extend").then_some(T::Extend))
        }
        b'E' => starts_no_case(b, i, b"extend").then_some(T::Extend),
        b'd' => found(b"direction", T::Direction),
        b'T' => found(b"TD", T::Td).or_else(|| found(b"TB", T::Tb)),
        b'B' => found(b"BT", T::Bt),
        b'L' => found(b"LR", T::Lr),
        b'R' => found(b"RL", T::Rl),
        b'n' => found(b"note", T::Note),
        b'f' => found(b"for", T::For).or_else(|| found(b"false", T::False)),
        b'j' => found(b"json", T::Json),
        b'c' => found(b"classDef", T::ClassDef).or_else(|| found(b"class", T::Class)),
        b'i' | b'I' => starts_no_case(b, i, b"include").then_some(T::Include),
        b't' => found(b"true", T::True),
        b'u' => found(b"usecase-beta", T::Usecase),
        _ => None,
    }
}

/// The number of UTF-16 units before byte `offset` of `src`.
pub(crate) fn utf16(src: &str, offset: usize) -> usize {
    src.get(..offset).map_or_else(
        || src.encode_utf16().count(),
        |head| head.encode_utf16().count(),
    )
}

/// Appends a count in decimal.
pub(crate) fn push_usize(out: &mut String, value: usize) {
    push_u32(out, u32::try_from(value).unwrap_or(u32::MAX));
}

/// The one-based line and UTF-16 column of byte `offset`. CR, LF, and CRLF each end a line.
pub(crate) fn line_column(src: &str, offset: usize) -> (u32, u32) {
    let b = src.as_bytes();
    let mut line: u32 = 1;
    let mut line_start = 0;
    let mut i = 0;
    while i < offset.min(b.len()) {
        let c = at(b, i);
        if c == 10 || c == 13 {
            if c == 13 && i + 1 < offset && at(b, i + 1) == 10 {
                i += 1;
            }
            line = line.saturating_add(1);
            line_start = i + 1;
        }
        i += 1;
    }
    let column = src
        .get(line_start..offset)
        .map_or(0, |text| text.encode_utf16().count());
    (line, u32::try_from(column + 1).unwrap_or(u32::MAX))
}

/// `line L, column C [start,end)`, with UTF-16 offsets.
pub(crate) fn location_text(src: &str, start: usize, end: usize) -> String {
    let (line, column) = line_column(src, start);
    let mut out = String::from("line ");
    push_u32(&mut out, line);
    out.push_str(", column ");
    push_u32(&mut out, column);
    out.push_str(" [");
    push_usize(&mut out, utf16(src, start));
    out.push(',');
    push_usize(&mut out, utf16(src, end));
    out.push(')');
    out
}

/// A use case error at byte `offset`, with Pele's code and message.
#[cold]
pub(crate) fn error(src: &str, code: Code, message: String, offset: usize) -> Error {
    let (line, column) = line_column(src, offset);
    Error {
        code,
        column,
        line,
        message,
        snippet: String::new(),
        r#type: Some("usecase"),
    }
}

/// The end of a quoted string that opens at `i`, or `None` when the line ends first.
fn string_end(b: &[u8], i: usize, quote: u32) -> Option<usize> {
    for k in i + 1..b.len() {
        let c = at(b, k);
        if c == quote {
            return Some(k + 1);
        }
        if c == 10 || c == 13 {
            return None;
        }
    }
    None
}

/// Whether a token may start at `i`: anything but a quote that opens no string.
fn starts_token(b: &[u8], i: usize) -> bool {
    let c = at(b, i);
    if c != 34 && c != 39 {
        return true;
    }
    (c == 34 && at(b, i + 1) == 96) || string_end(b, i, c).is_some()
}

/// Mermaid reports the first character no token starts with, and how many characters its lexer
/// skipped before one did.
#[cold]
fn lex_error(src: &str, offset: usize, length: usize) -> Error {
    let (line, column) = line_column(src, offset);
    let start = utf16(src, offset);
    let end = utf16(src, offset + length);
    let mut message = String::from("Error lexing usecase diagram: unexpected character: ->");
    if let Some(c) = src.get(offset..).and_then(|rest| rest.chars().next()) {
        message.push(c);
    }
    message.push_str("<- at offset: ");
    push_usize(&mut message, start);
    message.push_str(", skipped ");
    push_usize(&mut message, end - start);
    message.push_str(" characters. at line ");
    push_u32(&mut message, line);
    message.push_str(", column ");
    push_u32(&mut message, column);
    message.push_str(" [");
    push_usize(&mut message, start);
    message.push(',');
    push_usize(&mut message, end);
    message.push(')');
    error(src, Code::Syntax, message, offset)
}

/// Produces the tokens of Mermaid's Chevrotain lexer for use case diagrams: the first pattern in
/// its list that matches wins, a keyword gives way to a longer identifier, and `json id@` and
/// `<<` switch to modes that read a JSON object and a stereotype. Three `Eof` tokens end the
/// list, for lookahead.
///
/// # Errors
///
/// Returns Mermaid's lexing error for an unclosed quoted string or an empty stereotype.
#[expect(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "mirrors Pele's tokenize"
)]
pub(crate) fn tokenize(src: &str) -> Result<Vec<Tok>, Error> {
    let b = src.as_bytes();
    let n = b.len();
    let mut out: Vec<Tok> = Vec::new();
    let push = |out: &mut Vec<Tok>, kind: T, start: usize, end: usize| {
        out.push(Tok { end, kind, start });
    };
    let mut i = 0;
    // Whether only blanks came before on this line, where comments and accessibility lines may
    // start.
    let mut line_start = true;
    let mut no_brace = false;

    while i < n {
        let c = at(b, i);
        if c == 32 || c == 9 {
            i += 1;
            continue;
        }
        let mut kind: Option<T> = None;
        let mut end = i + 1;
        let mut fresh = false;

        if is_word(c) {
            if c == u32::from(b'a') && line_start && starts(b, i, b"acc") {
                let descr = starts(b, i + 3, b"Descr");
                if descr || starts(b, i + 3, b"Title") {
                    let mut j = i + 8;
                    while is_blank(at(b, j)) {
                        j += 1;
                    }
                    let next = at(b, j);
                    if next == 58 {
                        kind = Some(if descr {
                            T::AccDescrLine
                        } else {
                            T::AccTitleLine
                        });
                        end = j + 1;
                        while end < n && at(b, end) != 10 && at(b, end) != 13 {
                            end += 1;
                        }
                    } else if next == 123 && descr && !no_brace {
                        match src.get(j + 1..).and_then(|rest| rest.find('}')) {
                            None => no_brace = true,
                            Some(close) => {
                                kind = Some(T::AccDescrBlock);
                                end = j + 1 + close + 1;
                            }
                        }
                    }
                }
            } else if c == u32::from(b'j') && starts(b, i, b"json") && is_blank(at(b, i + 4)) {
                let mut j = i + 5;
                while is_blank(at(b, j)) {
                    j += 1;
                }
                let id = j;
                while is_word(at(b, j)) {
                    j += 1;
                }
                if j > id {
                    while is_blank(at(b, j)) {
                        j += 1;
                    }
                    if at(b, j) == 64 {
                        j += 1;
                        while is_blank(at(b, j)) {
                            j += 1;
                        }
                        if at(b, j) == 123 {
                            push(&mut out, T::JsonDeclarationStart, i, j);
                            let mut depth = 0usize;
                            let mut quoted = false;
                            let mut escaped = false;
                            let mut k = j;
                            while k < n {
                                let ch = at(b, k);
                                if quoted {
                                    if escaped {
                                        escaped = false;
                                    } else if ch == 92 {
                                        escaped = true;
                                    } else if ch == 34 {
                                        quoted = false;
                                    }
                                } else if ch == 34 {
                                    quoted = true;
                                } else if ch == 123 {
                                    depth += 1;
                                } else if ch == 125 {
                                    depth = depth.saturating_sub(1);
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                k += 1;
                            }
                            let closed = k < n;
                            i = if closed { k + 1 } else { n };
                            push(
                                &mut out,
                                if closed {
                                    T::JsonObjectLiteral
                                } else {
                                    T::UnclosedJsonObjectLiteral
                                },
                                j,
                                i,
                            );
                            line_start = false;
                            continue;
                        }
                    }
                }
            }
            if kind.is_none() {
                while end < n && is_word(at(b, end)) {
                    end += 1;
                }
                let byte = b.get(i).copied().unwrap_or(0);
                if let Some(keyword) = keyword_at(b, i, byte) {
                    // A keyword that is only the start of a longer word is an identifier.
                    if keyword == T::Usecase || end - i == keyword_length(keyword) {
                        kind = Some(keyword);
                        end = i + keyword_length(keyword);
                    } else {
                        kind = Some(T::Identifier);
                    }
                } else if (c == u32::from(b'o') || c == u32::from(b'x'))
                    && at(b, i + 1) == 45
                    && at(b, i + 2) == 45
                {
                    kind = Some(if c == u32::from(b'o') {
                        T::BackwardCircle
                    } else {
                        T::BackwardCross
                    });
                    end = i + 3;
                } else if is_digit(c) {
                    // `1mg` is an identifier, and `1.5px` is a number because the identifier
                    // stops at the dot.
                    let mut digits = i + 1;
                    while is_digit(at(b, digits)) {
                        digits += 1;
                    }
                    if at(b, digits) == 46 && is_digit(at(b, digits + 1)) {
                        kind = Some(T::Number);
                        end = digits + 2;
                        while is_digit(at(b, end)) {
                            end += 1;
                        }
                        while is_letter(at(b, end)) {
                            end += 1;
                        }
                    } else {
                        kind = Some(T::Identifier);
                    }
                } else {
                    kind = Some(T::Identifier);
                    while at(b, end) == 45 && is_word(at(b, end + 1)) {
                        kind = Some(T::CssIdentifier);
                        end += 2;
                        while is_word(at(b, end)) {
                            end += 1;
                        }
                    }
                }
            }
        } else if c == 10 {
            kind = Some(T::Newline);
            fresh = true;
        } else if c == 13 {
            kind = Some(T::Newline);
            fresh = true;
            if at(b, end) == 10 {
                end += 1;
            }
        } else if c == 34 || c == 39 {
            if c == 34 && at(b, i + 1) == 96 {
                if let Some(close) = src.get(i + 2..).and_then(|rest| rest.find("`\"")) {
                    kind = Some(T::MarkdownString);
                    end = i + 2 + close + 2;
                } else {
                    kind = Some(T::UnclosedMarkdownString);
                    end = n;
                }
            } else {
                kind = Some(T::PlainString);
                if let Some(close) = string_end(b, i, c) {
                    end = close;
                } else {
                    let mut resume = i + 1;
                    while resume < n && !starts_token(b, resume) {
                        resume += 1;
                    }
                    return Err(lex_error(src, i, resume - i));
                }
            }
        } else if c == 37 {
            if line_start && at(b, end) == 37 {
                kind = Some(T::Comment);
                while end < n && at(b, end) != 10 && at(b, end) != 13 {
                    end += 1;
                }
            } else {
                kind = Some(T::Percent);
            }
        } else if c == 45 {
            if at(b, end) == 45 {
                while at(b, end) == 45 {
                    end += 1;
                }
                let next = at(b, end);
                let two = end - i == 2;
                if two && next == 124 && at(b, end + 1) == 62 {
                    kind = Some(T::Generalization);
                    end += 2;
                } else if next == 62 {
                    kind = Some(T::ForwardSolid);
                    end += 1;
                } else if two && (next == u32::from(b'o') || next == u32::from(b'x')) {
                    kind = Some(if next == u32::from(b'o') {
                        T::ForwardCircle
                    } else {
                        T::ForwardCross
                    });
                    end += 1;
                } else {
                    kind = Some(T::MarkerlessSolid);
                }
            } else {
                kind = Some(T::Dash);
            }
        } else if c == 46 {
            let next = at(b, end);
            if next == 46 && at(b, end + 1) == 62 {
                kind = Some(T::DependencyArrow);
                end += 2;
            } else if is_digit(next) {
                kind = Some(T::Number);
                while is_digit(at(b, end)) {
                    end += 1;
                }
                while is_letter(at(b, end)) {
                    end += 1;
                }
            } else {
                kind = Some(T::Dot);
            }
        } else if c == 60 {
            if at(b, end) == 60 {
                push(&mut out, T::StereotypeStart, i, i + 2);
                i += 2;
                if i >= n {
                    break;
                }
                if at(b, i) == 62 && at(b, i + 1) == 62 {
                    kind = Some(T::StereotypeEnd);
                    end = i + 2;
                } else {
                    let mut close = i;
                    let mut found = false;
                    while close < n {
                        let ch = at(b, close);
                        if ch == 10 || ch == 13 {
                            break;
                        }
                        if ch == 62 && at(b, close + 1) == 62 {
                            found = true;
                            break;
                        }
                        close += 1;
                    }
                    if found && !trim(src.get(i..close).unwrap_or("")).is_empty() {
                        push(&mut out, T::StereotypeText, i, close);
                        kind = Some(T::StereotypeEnd);
                        i = close;
                        end = close + 2;
                    } else {
                        while close < n && at(b, close) != 10 && at(b, close) != 13 {
                            close += 1;
                        }
                        if close == i {
                            while close < n && (at(b, close) == 10 || at(b, close) == 13) {
                                close += 1;
                            }
                            return Err(lex_error(src, i, close - i));
                        }
                        kind = Some(T::UnclosedStereotypeText);
                        end = close;
                    }
                }
            } else if at(b, end) == 45 && at(b, end + 1) == 45 {
                kind = Some(T::BackwardSolid);
                end += 2;
                while at(b, end) == 45 {
                    end += 1;
                }
            } else {
                kind = Some(T::LabelPunctuation);
            }
        } else if c == 58 {
            if at(b, end) == 58 && at(b, end + 1) == 58 {
                kind = Some(T::ClassSeparator);
                end += 2;
            } else {
                kind = Some(T::Colon);
            }
        } else if c == 64 {
            if at(b, end) == 123 {
                kind = Some(T::MetadataStart);
                end += 1;
            } else {
                kind = Some(T::At);
            }
        } else if c == 123 {
            kind = Some(T::LBrace);
        } else if c == 125 {
            kind = Some(T::RBrace);
        } else if c == 91 {
            kind = Some(T::LBracket);
        } else if c == 93 {
            kind = Some(T::RBracket);
        } else if c == 40 {
            kind = Some(T::LParen);
        } else if c == 41 {
            kind = Some(T::RParen);
        } else if c == 44 {
            kind = Some(T::Comma);
        } else if c == 35 {
            while is_hex(at(b, end)) {
                end += 1;
            }
            kind = Some(if end > i + 1 {
                T::HashColor
            } else {
                T::CssPunctuation
            });
        } else if c == 92 {
            if at(b, end) == 44 {
                kind = Some(T::CssEscapedComma);
                end += 1;
            } else {
                kind = Some(T::LabelPunctuation);
            }
        } else if c < 127 && c > 32 {
            kind = Some(
                if b"!#$&*+/=?^|~".contains(&b.get(i).copied().unwrap_or(0)) {
                    T::CssPunctuation
                } else {
                    T::LabelPunctuation
                },
            );
        } else {
            kind = Some(T::LabelSymbol);
            while end < n {
                let ch = at(b, end);
                if (32..127).contains(&ch) || ch == 9 || ch == 10 || ch == 13 {
                    break;
                }
                end += 1;
            }
        }

        push(&mut out, kind.unwrap_or(T::LabelSymbol), i, end);
        i = end;
        line_start = fresh;
    }

    for _ in 0..3 {
        push(&mut out, T::Eof, n, n);
    }
    Ok(out)
}
