//! The event modeling parser: Mermaid's Langium event modeling grammar over the shared
//! tokenizer. Port of Pele's `src/diagrams/eventmodeling/parser.ts`.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, DIRECTIVE, EOF, Pattern, Reader, SINGLE_LINE_COMMENT, TITLE, TokenType, YAML,
    keyword, literal, tokenize,
};
use crate::error::Error;
use crate::util::chars::{is_line_end, is_space};

/// Token kinds, as indices into [`EVENTMODELING_TOKENS`].
mod t {
    pub(super) const KEYWORD: u16 = 0;
    pub(super) const RESETFRAME: u16 = 1;
    pub(super) const READMODEL: u16 = 2;
    pub(super) const PROCESSOR: u16 = 3;
    pub(super) const TIMEFRAME: u16 = 4;
    pub(super) const COMMAND: u16 = 5;
    pub(super) const ENTITY: u16 = 6;
    pub(super) const EVENT: u16 = 7;
    pub(super) const JSOBJ: u16 = 8;
    pub(super) const FIGMA: u16 = 9;
    pub(super) const GIVEN: u16 = 10;
    pub(super) const JSON: u16 = 11;
    pub(super) const SALT: u16 = 12;
    pub(super) const HTML: u16 = 13;
    pub(super) const TEXT: u16 = 14;
    pub(super) const DATA: u16 = 15;
    pub(super) const NOTE: u16 = 16;
    pub(super) const WHEN: u16 = 17;
    pub(super) const THEN: u16 = 18;
    pub(super) const RMO: u16 = 19;
    pub(super) const CMD: u16 = 20;
    pub(super) const EVT: u16 = 21;
    pub(super) const PCR: u16 = 22;
    pub(super) const URI: u16 = 23;
    pub(super) const ARROW: u16 = 24;
    pub(super) const GWT: u16 = 25;
    pub(super) const UI: u16 = 26;
    pub(super) const MD: u16 = 27;
    pub(super) const TF: u16 = 28;
    pub(super) const OPEN: u16 = 29;
    pub(super) const CLOSE: u16 = 30;
    pub(super) const RF: u16 = 31;
    pub(super) const TICK: u16 = 32;
    pub(super) const DOT: u16 = 33;
    pub(super) const ID: u16 = 34;
    pub(super) const FRAME_ID: u16 = 35;
    pub(super) const INLINE: u16 = 36;
    pub(super) const BLOCK: u16 = 37;
    pub(super) const ACC_DESCR: u16 = 38;
    pub(super) const ACC_TITLE: u16 = 39;
    pub(super) const TITLE: u16 = 40;
}

/// `[_a-zA-Z][\w_]*`.
fn em_id(src: &str, at: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let first = *bytes.get(at)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut i = at + 1;
    while bytes
        .get(i)
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
    {
        i += 1;
    }
    Some(i)
}

/// `\d{1,3}`.
fn em_fid(src: &str, at: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = at;
    while i < at + 3 && bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    (i > at).then_some(i)
}

/// Where the line from `at` ends: at the first line terminator, which `.` does not match.
fn dot_run(src: &str, at: usize) -> usize {
    src.get(at..)
        .and_then(|rest| rest.find(|c: char| is_line_end(u32::from(c))))
        .map_or(src.len(), |found| at + found)
}

/// `\{(.*)\}|"(.*)"|'(.*)'`: an opener and the last matching closer on its line.
fn em_data_inline(src: &str, at: usize) -> Option<usize> {
    let open = *src.as_bytes().get(at)?;
    let close = match open {
        b'{' => '}',
        b'"' => '"',
        b'\'' => '\'',
        _ => return None,
    };
    let end = dot_run(src, at + 1);
    let found = src.get(at + 1..end)?.rfind(close)?;
    Some(at + 1 + found + 1)
}

/// `\r?\n` at `at`.
fn newline(src: &str, at: usize) -> Option<usize> {
    let p = if src.as_bytes().get(at) == Some(&b'\r') {
        at + 1
    } else {
        at
    };
    (src.as_bytes().get(p) == Some(&b'\n')).then_some(p + 1)
}

/// `\}(?:\r?\n|(?!\S))` at `at`.
fn block_close(src: &str, at: usize) -> Option<usize> {
    if src.as_bytes().get(at) != Some(&b'}') {
        return None;
    }
    if let Some(end) = newline(src, at + 1) {
        return Some(end);
    }
    src.get(at + 1..)?
        .chars()
        .next()
        .is_none_or(|c| is_space(u32::from(c)))
        .then_some(at + 1)
}

/// `\{[\t ]*\r?\n(?:[\S\s]*?\r?\n)?\}(?:\r?\n|(?!\S))`.
fn em_data_block(src: &str, at: usize) -> Option<usize> {
    if src.as_bytes().get(at) != Some(&b'{') {
        return None;
    }
    let mut head = at + 1;
    while matches!(src.as_bytes().get(head), Some(b' ' | b'\t')) {
        head += 1;
    }
    let head = newline(src, head)?;

    // The optional group comes first, and its lazy body ends at the earliest line break that a
    // closing brace follows.
    let mut from = head;
    while let Some(found) = src.get(from..).and_then(|rest| rest.find('\n')) {
        let after = from + found + 1;
        if let Some(end) = block_close(src, after) {
            return Some(end);
        }
        from = after;
    }
    block_close(src, head)
}

/// `\s+`.
fn em_ws(src: &str, at: usize) -> Option<usize> {
    let rest = src.get(at..)?;
    let end = rest
        .find(|c: char| !is_space(u32::from(c)))
        .unwrap_or(rest.len());
    (end > 0).then_some(at + end)
}

/// `\/\*[\s\S]*?\*\/`.
fn em_ml_comment(src: &str, at: usize) -> Option<usize> {
    if !src.get(at..)?.starts_with("/*") {
        return None;
    }
    let close = src.get(at + 2..)?.find("*/")?;
    Some(at + 2 + close + 2)
}

/// `\/\/[^\n\r]*`.
fn em_sl_comment(src: &str, at: usize) -> Option<usize> {
    if !src.get(at..)?.starts_with("//") {
        return None;
    }
    Some(
        src.get(at + 2..)
            .and_then(|rest| rest.find(['\n', '\r']))
            .map_or(src.len(), |found| at + 2 + found),
    )
}

/// The ASCII characters an identifier starts with.
const ID_FIRST: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_";

/// A token type with a hand-written matcher.
const fn matched(
    name: &'static str,
    first: &'static str,
    matcher: fn(&str, usize) -> Option<usize>,
) -> TokenType {
    TokenType {
        first: Some(first),
        name,
        pattern: Pattern::Match(matcher),
        ..literal("")
    }
}

/// A keyword that is also an identifier. Langium gives each the identifier as a longer
/// alternative.
const fn word(text: &'static str) -> TokenType {
    TokenType {
        longer: &[t::ID],
        ..literal(text)
    }
}

/// Pele's `EVENTMODELING_TOKENS`, in matching order.
pub(crate) const EVENTMODELING_TOKENS: &[TokenType] = &[
    TokenType {
        longer: &[t::ID],
        ..keyword("eventmodeling")
    },
    word("resetframe"),
    word("readmodel"),
    word("processor"),
    word("timeframe"),
    word("command"),
    word("entity"),
    word("event"),
    word("jsobj"),
    word("figma"),
    word("given"),
    word("json"),
    word("salt"),
    word("html"),
    word("text"),
    word("data"),
    word("note"),
    word("when"),
    word("then"),
    word("rmo"),
    word("cmd"),
    word("evt"),
    word("pcr"),
    word("uri"),
    literal("->>"),
    word("gwt"),
    word("ui"),
    word("md"),
    word("tf"),
    literal("[["),
    literal("]]"),
    word("rf"),
    literal("`"),
    literal("."),
    matched("EM_ID", ID_FIRST, em_id),
    matched("EM_FID", "0123456789", em_fid),
    matched("EM_DATA_INLINE", "{\"'", em_data_inline),
    matched("EM_DATA_BLOCK", "{", em_data_block),
    acc_descr("EM_ACC_DESCR"),
    TokenType {
        name: "EM_ACC_TITLE",
        ..ACC_TITLE
    },
    TokenType {
        name: "EM_TITLE",
        ..TITLE
    },
    TokenType {
        hidden: true,
        ..matched("EM_WS", "\t\n\u{b}\u{c}\r ", em_ws)
    },
    TokenType {
        name: "EM_YAML",
        ..YAML
    },
    TokenType {
        name: "EM_DIRECTIVE",
        ..DIRECTIVE
    },
    TokenType {
        name: "EM_SINGLE_LINE_COMMENT",
        ..SINGLE_LINE_COMMENT
    },
    TokenType {
        hidden: true,
        ..matched("EM_ML_COMMENT", "/", em_ml_comment)
    },
    TokenType {
        hidden: true,
        ..matched("EM_SL_COMMENT", "/", em_sl_comment)
    },
];

/// `tf` or `timeframe`.
fn is_time_frame(kind: u16) -> bool {
    matches!(kind, t::TF | t::TIMEFRAME)
}

/// `rf` or `resetframe`.
fn is_reset_frame(kind: u16) -> bool {
    matches!(kind, t::RF | t::RESETFRAME)
}

/// A model entity type keyword.
fn is_entity_type(kind: u16) -> bool {
    matches!(
        kind,
        t::RMO
            | t::READMODEL
            | t::UI
            | t::CMD
            | t::COMMAND
            | t::EVT
            | t::EVENT
            | t::PCR
            | t::PROCESSOR
    )
}

/// A data type keyword.
fn is_data_type(kind: u16) -> bool {
    matches!(
        kind,
        t::JSON | t::JSOBJ | t::FIGMA | t::SALT | t::URI | t::MD | t::HTML | t::TEXT
    )
}

/// A time frame or a reset frame: Pele's `EmFrame`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmFrame<'a> {
    /// The data block the frame refers to.
    pub(crate) data_reference: Option<&'a str>,
    /// The data type of the inline data.
    pub(crate) data_type: Option<&'a str>,
    /// The inline data token, as written.
    pub(crate) data_inline_value: Option<&'a str>,
    /// The entity's qualified name.
    pub(crate) entity_identifier: String,
    /// The entity type keyword.
    pub(crate) model_entity_type: &'a str,
    /// The frame number as written.
    pub(crate) name: &'a str,
    /// The UTF-8 offset of the frame in the source, for error messages.
    pub(crate) offset: usize,
    /// Whether the frame is a reset frame.
    pub(crate) reset: bool,
    /// The frames that feed this one.
    pub(crate) source_frames: Vec<&'a str>,
}

/// A data block: Pele's `EmDataEntity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmDataEntity<'a> {
    /// The block token, as written.
    pub(crate) data_block_value: &'a str,
    /// The block's name.
    pub(crate) name: &'a str,
}

/// A note: Pele's `EmNoteEntity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmNoteEntity<'a> {
    /// The block token, as written.
    pub(crate) data_block_value: &'a str,
    /// The data type.
    pub(crate) data_type: Option<&'a str>,
    /// The frame the note belongs to.
    pub(crate) source_frame: &'a str,
}

/// A given/when/then specification: Pele's `EmGwt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EmGwt<'a> {
    /// The `given` entities.
    pub(crate) given: Vec<&'a str>,
    /// The frame the specification belongs to.
    pub(crate) source_frame: &'a str,
    /// The `then` entities.
    pub(crate) then: Vec<&'a str>,
    /// The `when` entities.
    pub(crate) when: Vec<&'a str>,
}

/// The parsed event model: Pele's `EventModelAst`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EventModelAst<'a> {
    /// The `accDescr` token, as written.
    pub(crate) acc_descr: Option<&'a str>,
    /// The `accTitle` token, as written.
    pub(crate) acc_title: Option<&'a str>,
    /// Data blocks in authored order.
    pub(crate) data_entities: Vec<EmDataEntity<'a>>,
    /// Frames in authored order.
    pub(crate) frames: Vec<EmFrame<'a>>,
    /// Specifications in authored order.
    pub(crate) gwt_entities: Vec<EmGwt<'a>>,
    /// Model entity names in authored order.
    pub(crate) model_entities: Vec<String>,
    /// Notes in authored order.
    pub(crate) note_entities: Vec<EmNoteEntity<'a>>,
    /// The `title` token, as written. The grammar's own terminals get no value conversion.
    pub(crate) title: Option<&'a str>,
}

/// The current token when its kind is in `set`, moving past it.
fn one<'a>(r: &mut Reader<'a>, set: fn(u16) -> bool, expected: &str) -> Result<&'a str, Error> {
    if set(r.kind()) {
        Ok(r.take())
    } else {
        Err(r.fail(expected))
    }
}

/// A dotted name.
fn qualified_name(r: &mut Reader<'_>) -> Result<String, Error> {
    let mut name = r.expect(t::ID)?.to_owned();
    while r.accept(t::DOT) {
        name.push('.');
        name.push_str(r.expect(t::ID)?);
    }
    Ok(name)
}

/// A data type between backticks, when one is written.
fn data_type<'a>(r: &mut Reader<'a>) -> Result<Option<&'a str>, Error> {
    if !r.accept(t::TICK) {
        return Ok(None);
    }
    let kind = one(r, is_data_type, "a data type")?;
    r.expect(t::TICK)?;
    Ok(Some(kind))
}

/// One or more statements of an entity type and a name.
fn statements<'a>(r: &mut Reader<'a>) -> Result<Vec<&'a str>, Error> {
    let mut out = Vec::new();
    loop {
        one(r, is_entity_type, "an entity type")?;
        out.push(r.expect(t::ID)?);
        if !is_entity_type(r.kind()) {
            return Ok(out);
        }
    }
}

/// Parses event modeling `src` as Pele's `parseEventModel` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_event_model(src: &str) -> Result<EventModelAst<'_>, Error> {
    let mut r = Reader::new(
        tokenize(src, EVENTMODELING_TOKENS, "eventmodeling")?,
        EVENTMODELING_TOKENS,
        "eventmodeling",
    );
    let mut ast = EventModelAst::default();

    r.expect(t::KEYWORD)?;
    while r.kind() != EOF {
        let kind = r.kind();
        match kind {
            t::ACC_DESCR => ast.acc_descr = Some(r.take()),
            t::ACC_TITLE => ast.acc_title = Some(r.take()),
            t::TITLE => ast.title = Some(r.take()),
            t::ENTITY => {
                r.i += 1;
                ast.model_entities.push(qualified_name(&mut r)?);
            }
            _ if is_time_frame(kind) || is_reset_frame(kind) => {
                let offset = r.start();
                r.i += 1;
                let name = r.expect(t::FRAME_ID)?;
                let model_entity_type = one(&mut r, is_entity_type, "an entity type")?;
                let entity_identifier = qualified_name(&mut r)?;
                let mut source_frames = Vec::new();
                while r.accept(t::ARROW) {
                    source_frames.push(r.expect(t::FRAME_ID)?);
                }
                let mut data_reference = None;
                if r.accept(t::OPEN) {
                    data_reference = Some(r.expect(t::ID)?);
                    r.expect(t::CLOSE)?;
                }
                let mut frame_data_type = None;
                let mut data_inline_value = None;
                if r.kind() == t::TICK || r.kind() == t::INLINE {
                    frame_data_type = data_type(&mut r)?;
                    data_inline_value = Some(r.expect(t::INLINE)?);
                }
                ast.frames.push(EmFrame {
                    data_reference,
                    data_type: frame_data_type,
                    data_inline_value,
                    entity_identifier,
                    model_entity_type,
                    name,
                    offset,
                    reset: is_reset_frame(kind),
                    source_frames,
                });
            }
            t::DATA => {
                r.i += 1;
                let name = r.expect(t::ID)?;
                data_type(&mut r)?;
                let data_block_value = r.expect(t::BLOCK)?;
                ast.data_entities.push(EmDataEntity {
                    data_block_value,
                    name,
                });
            }
            t::NOTE => {
                r.i += 1;
                let source_frame = r.expect(t::FRAME_ID)?;
                let note_data_type = data_type(&mut r)?;
                let data_block_value = r.expect(t::BLOCK)?;
                ast.note_entities.push(EmNoteEntity {
                    data_block_value,
                    data_type: note_data_type,
                    source_frame,
                });
            }
            t::GWT => {
                r.i += 1;
                let source_frame = r.expect(t::FRAME_ID)?;
                r.expect(t::GIVEN)?;
                let given = statements(&mut r)?;
                let when = if r.accept(t::WHEN) {
                    statements(&mut r)?
                } else {
                    Vec::new()
                };
                r.expect(t::THEN)?;
                let then = statements(&mut r)?;
                ast.gwt_entities.push(EmGwt {
                    given,
                    source_frame,
                    then,
                    when,
                });
            }
            _ => {
                return Err(r.fail("a frame, an entity, a data block, a note, or a specification"));
            }
        }
    }
    Ok(ast)
}
