//! The pie parser: Mermaid's Langium pie grammar over the shared tokenizer. Port of Pele's
//! `src/diagrams/pie/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, EOF, Memo, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT,
    STRING, TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, literal,
    string_value, title_value, tokenize,
};
use crate::error::Error;

/// Token kinds, as indices into [`PIE_TOKENS`].
mod t {
    pub(super) const SHOW_DATA: u16 = 0;
    pub(super) const PIE: u16 = 1;
    pub(super) const COLON: u16 = 2;
    pub(super) const NUMBER: u16 = 3;
    pub(super) const ACC_DESCR: u16 = 4;
    pub(super) const ACC_TITLE: u16 = 5;
    pub(super) const TITLE: u16 = 6;
    pub(super) const STRING: u16 = 7;
    pub(super) const NEWLINE: u16 = 8;
}

/// Where a run of ASCII digits from `from` ends.
fn digits(src: &[u8], from: usize) -> usize {
    let mut at = from;
    while src.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    at
}

/// Answers as `NUMBER_PIE`'s pattern `(?:-?[0-9]+\.[0-9]+(?!\.))|(?:-?(0|[1-9][0-9]*)(?!\.))`
/// does. The pattern reads a whole run of digits before it finds there is no fraction, and would
/// do so again from every digit of a long run of zeros, so the run is remembered: `memo[0]` and
/// `memo[1]` are its start and end, and `memo[2]` is one past the fraction's end, or 0.
fn number(src: &str, p: usize, memo: &mut Memo) -> Option<usize> {
    let bytes = src.as_bytes();
    let at = if bytes.get(p) == Some(&b'-') {
        p + 1
    } else {
        p
    };
    if at < memo[0] || at >= memo[1] {
        let end = digits(bytes, at);
        memo[0] = at;
        memo[1] = end;
        memo[2] = 0;
        if end > at && bytes.get(end) == Some(&b'.') {
            let stop = digits(bytes, end + 1);
            let length = stop - end - 1;
            if length > 0 && bytes.get(stop) != Some(&b'.') {
                memo[2] = stop + 1;
            } else if length > 1 {
                memo[2] = stop;
            }
        }
    }

    let end = memo[1];
    if end == at {
        return None;
    }
    if memo[2] > 0 {
        return Some(memo[2] - 1);
    }
    if bytes.get(at) == Some(&b'0') {
        return (bytes.get(at + 1) != Some(&b'.')).then_some(at + 1);
    }
    if bytes.get(end) != Some(&b'.') {
        return Some(end);
    }
    (end - 1 > at).then(|| end - 1)
}

/// Pele's `PIE_TOKENS`, in matching order.
pub(crate) const PIE_TOKENS: &[TokenType] = &[
    keyword("showData"),
    keyword("pie"),
    literal(":"),
    TokenType {
        first: Some("-0123456789"),
        name: "NUMBER_PIE",
        pattern: Pattern::Scan(number),
        ..literal("")
    },
    ACC_DESCR,
    ACC_TITLE,
    TITLE,
    STRING,
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
];

/// One slice as written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PieSection {
    /// The label.
    pub(crate) label: String,
    /// The value.
    pub(crate) value: f64,
}

/// The parsed pie chart: Pele's `PieAst`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PieAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Slices in authored order.
    pub(crate) sections: Vec<PieSection>,
    /// Whether `showData` follows `pie`.
    pub(crate) show_data: bool,
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

/// Parses pie `src` as Pele's `parsePie` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_pie(src: &str) -> Result<PieAst, Error> {
    let mut r = Reader::new(tokenize(src, PIE_TOKENS, "pie")?, PIE_TOKENS, "pie");
    let mut ast = PieAst::default();

    while r.accept(t::NEWLINE) {}
    r.expect(t::PIE)?;
    ast.show_data = r.accept(t::SHOW_DATA);
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
            t::STRING => {
                let label = string_value(r.take()).into_owned();
                r.expect(t::COLON)?;
                let value = r.expect(t::NUMBER)?.parse().unwrap_or(f64::NAN);
                ast.sections.push(PieSection { label, value });
                end_of_line(&mut r)?;
            }
            _ => return Err(r.fail("a section, a title, or a line break")),
        }
    }
    Ok(ast)
}
