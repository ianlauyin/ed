//! The packet parser: Mermaid's Langium packet grammar over the shared tokenizer. Port of Pele's
//! `src/diagrams/packet/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, EOF, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING,
    TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, literal,
    string_value, title_value, tokenize,
};
use crate::error::Error;

/// Token kinds, as indices into [`PACKET_TOKENS`].
mod t {
    pub(super) const PACKET_BETA: u16 = 0;
    pub(super) const PACKET: u16 = 1;
    pub(super) const DASH: u16 = 2;
    pub(super) const PLUS: u16 = 3;
    pub(super) const COLON: u16 = 4;
    pub(super) const ACC_DESCR: u16 = 5;
    pub(super) const ACC_TITLE: u16 = 6;
    pub(super) const TITLE: u16 = 7;
    pub(super) const INT: u16 = 8;
    pub(super) const STRING: u16 = 9;
    pub(super) const NEWLINE: u16 = 10;
    pub(super) const YAML: u16 = 12;
}

/// `0|[1-9][0-9]*(?!\.)`: a lone zero, or digits not followed by a dot. Before a dot the run
/// gives up its last digit.
fn int(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    match bytes.get(p)? {
        b'0' => return Some(p + 1),
        b'1'..=b'9' => {}
        _ => return None,
    }
    let mut end = p + 1;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if bytes.get(end) != Some(&b'.') {
        return Some(end);
    }
    (end - 1 > p).then(|| end - 1)
}

/// Pele's `PACKET_TOKENS`, in matching order.
pub(crate) const PACKET_TOKENS: &[TokenType] = &[
    literal("packet-beta"),
    keyword("packet"),
    // A front matter block also starts with dashes, so Langium lets it win over a dash.
    TokenType {
        longer: &[t::YAML],
        ..literal("-")
    },
    literal("+"),
    literal(":"),
    ACC_DESCR,
    ACC_TITLE,
    TITLE,
    TokenType {
        first: Some("0123456789"),
        name: "INT",
        pattern: Pattern::Match(int),
        ..literal("")
    },
    STRING,
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
];

/// One field as written: Pele's `PacketBlockAst`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PacketBlockAst {
    /// The width in bits, for a `+bits` field.
    pub(crate) bits: Option<f64>,
    /// The last bit, for a `start-end` field.
    pub(crate) end: Option<f64>,
    /// The label.
    pub(crate) label: String,
    /// The first bit, for a `start` or `start-end` field.
    pub(crate) start: Option<f64>,
}

/// The parsed packet diagram: Pele's `PacketAst`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PacketAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Fields in authored order.
    pub(crate) blocks: Vec<PacketBlockAst>,
    /// The `title` value.
    pub(crate) title: Option<String>,
}

/// Skips line breaks after a statement, which must end its line.
fn end_of_line(r: &mut Reader<'_>) -> Result<(), Error> {
    if r.kind() == EOF {
        return Ok(());
    }
    if r.kind() != t::NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    while r.accept(t::NEWLINE) {}
    Ok(())
}

/// JavaScript's `Number` of an `INT` token.
fn number(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// Parses packet `src` as Pele's `parsePacket` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_packet(src: &str) -> Result<PacketAst, Error> {
    let mut r = Reader::new(
        tokenize(src, PACKET_TOKENS, "packet")?,
        PACKET_TOKENS,
        "packet",
    );
    let mut ast = PacketAst::default();

    while r.accept(t::NEWLINE) {}
    if !r.accept(t::PACKET_BETA) {
        r.expect(t::PACKET)?;
    }
    while r.kind() != EOF {
        match r.kind() {
            t::NEWLINE => r.i += 1,
            t::ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(r.take()));
                end_of_line(&mut r)?;
            }
            t::ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(r.take()));
                end_of_line(&mut r)?;
            }
            t::TITLE => {
                ast.title = Some(title_value(r.take()));
                end_of_line(&mut r)?;
            }
            t::INT | t::PLUS => {
                let mut block = PacketBlockAst::default();
                if r.accept(t::PLUS) {
                    block.bits = Some(number(r.expect(t::INT)?));
                } else {
                    block.start = Some(number(r.take()));
                    if r.accept(t::DASH) {
                        block.end = Some(number(r.expect(t::INT)?));
                    }
                }
                r.expect(t::COLON)?;
                block.label = string_value(r.expect(t::STRING)?).into_owned();
                ast.blocks.push(block);
                end_of_line(&mut r)?;
            }
            _ => return Err(r.fail("a field, a title, or a line break")),
        }
    }
    Ok(ast)
}
