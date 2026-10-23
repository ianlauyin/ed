//! The radar parser: Mermaid's Langium radar grammar over the shared tokenizer. Port of Pele's
//! `src/diagrams/radar/parser.ts`.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, DIRECTIVE, EOF, Memo, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING, TITLE,
    TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, literal, string_value,
    title_value, tokenize,
};
use crate::error::Error;

/// Token kinds, as indices into [`RADAR_TOKENS`].
mod t {
    pub(super) const RADAR_COLON: u16 = 0;
    pub(super) const RADAR: u16 = 1;
    pub(super) const SHOW_LEGEND: u16 = 2;
    pub(super) const GRATICULE: u16 = 3;
    pub(super) const CURVE: u16 = 4;
    pub(super) const TICKS: u16 = 5;
    pub(super) const AXIS: u16 = 6;
    pub(super) const MAX: u16 = 7;
    pub(super) const MIN: u16 = 8;
    pub(super) const COLON: u16 = 9;
    pub(super) const COMMA: u16 = 10;
    pub(super) const OPEN: u16 = 11;
    pub(super) const CLOSE: u16 = 12;
    pub(super) const OPEN_BRACE: u16 = 13;
    pub(super) const CLOSE_BRACE: u16 = 14;
    pub(super) const GRATICULE_VALUE: u16 = 15;
    pub(super) const BOOLEAN: u16 = 16;
    pub(super) const ACC_DESCR: u16 = 17;
    pub(super) const ACC_TITLE: u16 = 18;
    pub(super) const TITLE: u16 = 19;
    pub(super) const NUMBER: u16 = 20;
    pub(super) const STRING: u16 = 21;
    pub(super) const ID: u16 = 22;
    pub(super) const NEWLINE: u16 = 23;
}

/// Where a run of ASCII digits from `at` ends.
fn digits(src: &[u8], at: usize) -> usize {
    let mut end = at;
    while src.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    end
}

/// The `NUMBER` pattern `(?:[0-9]+\.[0-9]+(?!\.))|(?:0|[1-9][0-9]*(?!\.))` without its
/// backtracking. As a regular expression it reads to the end of a run of digits from every
/// position in the run, which is quadratic for a long run of zeros, so `memo[0]` and `memo[1]`
/// keep the run's start and end.
fn number(src: &str, at: usize, memo: &mut Memo) -> Option<usize> {
    let bytes = src.as_bytes();
    if at < memo[0] || at >= memo[1] {
        memo[0] = at;
        memo[1] = digits(bytes, at);
    }
    let whole = memo[1];
    if whole == at {
        return None;
    }

    let dot = bytes.get(whole) == Some(&b'.');
    if dot {
        let fraction = digits(bytes, whole + 1);
        let length = fraction - whole - 1;
        if length > 0 && bytes.get(fraction) != Some(&b'.') {
            return Some(fraction);
        }
        if length > 1 {
            return Some(fraction - 1);
        }
    }
    if bytes.get(at) == Some(&b'0') {
        return Some(at + 1);
    }
    if !dot {
        return Some(whole);
    }
    (whole - 1 > at).then(|| whole - 1)
}

/// A character of JavaScript's `\w`.
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `[\w]([-\w]*\w)?`: a word that may hold dashes but does not end with one.
fn id(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if !bytes.get(p).copied().is_some_and(is_word) {
        return None;
    }
    let mut end = p + 1;
    let mut at = p + 1;
    while let Some(&c) = bytes.get(at)
        && (is_word(c) || c == b'-')
    {
        at += 1;
        if is_word(c) {
            end = at;
        }
    }
    Some(end)
}

/// The first of `words` at `p`.
fn one_of(src: &str, p: usize, words: &[&str]) -> Option<usize> {
    let rest = src.get(p..)?;
    words
        .iter()
        .find(|word| rest.starts_with(*word))
        .map(|word| p + word.len())
}

/// `circle|polygon`.
fn graticule(src: &str, p: usize) -> Option<usize> {
    one_of(src, p, &["circle", "polygon"])
}

/// `true|false`.
fn boolean(src: &str, p: usize) -> Option<usize> {
    one_of(src, p, &["true", "false"])
}

/// A word that gives way to a longer `ID`.
const fn word(text: &'static str) -> TokenType {
    TokenType {
        longer: &[t::ID],
        ..literal(text)
    }
}

/// Pele's `RADAR_TOKENS`, in matching order.
pub(crate) const RADAR_TOKENS: &[TokenType] = &[
    literal("radar-beta:"),
    TokenType {
        longer: &[t::ID],
        ..keyword("radar-beta")
    },
    word("showLegend"),
    word("graticule"),
    word("curve"),
    word("ticks"),
    word("axis"),
    word("max"),
    word("min"),
    literal(":"),
    literal(","),
    literal("["),
    literal("]"),
    literal("{"),
    literal("}"),
    TokenType {
        first: Some("cp"),
        name: "GRATICULE",
        pattern: Pattern::Match(graticule),
        ..literal("")
    },
    TokenType {
        first: Some("ft"),
        name: "BOOLEAN",
        pattern: Pattern::Match(boolean),
        ..literal("")
    },
    // In the other grammars an `accDescr {` that never closes ends the lexing. Here `accDescr` is
    // also a name and `{` a token, so each unclosed one would read to the end of the text again.
    acc_descr("ACC_DESCR"),
    ACC_TITLE,
    TITLE,
    TokenType {
        first: Some("0123456789"),
        name: "NUMBER",
        pattern: Pattern::Scan(number),
        ..literal("")
    },
    STRING,
    TokenType {
        first: Some("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz"),
        name: "ID",
        pattern: Pattern::Match(id),
        ..literal("")
    },
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
];

/// An axis as written: Pele's `AxisAst`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AxisAst {
    /// The label, when one is given.
    pub(crate) label: Option<String>,
    /// The name.
    pub(crate) name: String,
}

/// A curve value as written: Pele's `EntryAst`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntryAst {
    /// The axis named before the value, in the detailed form.
    pub(crate) axis: Option<String>,
    /// The value.
    pub(crate) value: f64,
}

/// A curve as written: Pele's `CurveAst`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CurveAst {
    /// The values, never empty.
    pub(crate) entries: Vec<EntryAst>,
    /// The label, when one is given.
    pub(crate) label: Option<String>,
    /// The name.
    pub(crate) name: String,
}

/// An option's value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum OptionValue {
    /// `showLegend`'s value.
    Bool(bool),
    /// `ticks`, `max`, or `min`'s value.
    Number(f64),
    /// `graticule`'s value.
    Text(String),
}

/// An option as written: Pele's `OptionAst`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OptionAst {
    /// `showLegend`, `ticks`, `max`, `min`, or `graticule`.
    pub(crate) name: String,
    /// The value.
    pub(crate) value: OptionValue,
}

/// The parsed radar chart: Pele's `RadarAst`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RadarAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Axes in authored order.
    pub(crate) axes: Vec<AxisAst>,
    /// Curves in authored order.
    pub(crate) curves: Vec<CurveAst>,
    /// Options in authored order.
    pub(crate) options: Vec<OptionAst>,
    /// The `title` value.
    pub(crate) title: Option<String>,
}

/// Skips line breaks.
fn newlines(r: &mut Reader<'_>) {
    while r.accept(t::NEWLINE) {}
}

/// Skips line breaks after a statement, which must end its line.
fn end_of_line(r: &mut Reader<'_>) -> Result<(), Error> {
    if r.kind() == EOF {
        return Ok(());
    }
    if r.kind() != t::NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    newlines(r);
    Ok(())
}

/// JavaScript's `Number` of a `NUMBER` token.
fn number_value(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// `["label"]`, when the next token opens one.
fn label(r: &mut Reader<'_>) -> Result<Option<String>, Error> {
    if !r.accept(t::OPEN) {
        return Ok(None);
    }
    let text = string_value(r.expect(t::STRING)?).into_owned();
    r.expect(t::CLOSE)?;
    Ok(Some(text))
}

/// A curve value, after its axis name in the detailed form.
fn entry(r: &mut Reader<'_>, detailed: bool) -> Result<EntryAst, Error> {
    if !detailed {
        return Ok(EntryAst {
            axis: None,
            value: number_value(r.expect(t::NUMBER)?),
        });
    }
    let axis = r.expect(t::ID)?.to_owned();
    r.accept(t::COLON);
    Ok(EntryAst {
        axis: Some(axis),
        value: number_value(r.expect(t::NUMBER)?),
    })
}

/// A curve: its name, label, and braced values.
fn curve(r: &mut Reader<'_>) -> Result<CurveAst, Error> {
    let name = r.expect(t::ID)?.to_owned();
    let label = label(r)?;
    let mut node = CurveAst {
        entries: Vec::new(),
        label,
        name,
    };
    r.expect(t::OPEN_BRACE)?;
    newlines(r);
    if r.kind() != t::NUMBER && r.kind() != t::ID {
        return Err(r.fail("a number or an axis name"));
    }
    let detailed = r.kind() == t::ID;
    node.entries.push(entry(r, detailed)?);
    while r.accept(t::COMMA) {
        newlines(r);
        node.entries.push(entry(r, detailed)?);
    }
    newlines(r);
    r.expect(t::CLOSE_BRACE)?;
    Ok(node)
}

/// An axis: its name and label.
fn axis(r: &mut Reader<'_>) -> Result<AxisAst, Error> {
    let name = r.expect(t::ID)?.to_owned();
    Ok(AxisAst {
        label: label(r)?,
        name,
    })
}

/// An option and its value.
fn option(r: &mut Reader<'_>) -> Result<OptionAst, Error> {
    let kind = r.kind();
    let name = r.take().to_owned();
    let value = match kind {
        t::SHOW_LEGEND => OptionValue::Bool(r.expect(t::BOOLEAN)? == "true"),
        t::GRATICULE => OptionValue::Text(r.expect(t::GRATICULE_VALUE)?.to_owned()),
        _ => OptionValue::Number(number_value(r.expect(t::NUMBER)?)),
    };
    Ok(OptionAst { name, value })
}

/// Whether the current token starts an option.
fn is_option(r: &Reader<'_>) -> bool {
    matches!(
        r.kind(),
        t::SHOW_LEGEND | t::TICKS | t::MAX | t::MIN | t::GRATICULE
    )
}

/// Parses radar `src` as Pele's `parseRadar` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_radar(src: &str) -> Result<RadarAst, Error> {
    let mut r = Reader::new(tokenize(src, RADAR_TOKENS, "radar")?, RADAR_TOKENS, "radar");
    let mut ast = RadarAst::default();

    newlines(&mut r);
    if !r.accept(t::RADAR_COLON) {
        r.expect(t::RADAR)?;
        r.accept(t::COLON);
    }
    newlines(&mut r);
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
            t::AXIS => {
                r.i += 1;
                ast.axes.push(axis(&mut r)?);
                while r.accept(t::COMMA) {
                    ast.axes.push(axis(&mut r)?);
                }
            }
            t::CURVE => {
                r.i += 1;
                ast.curves.push(curve(&mut r)?);
                while r.accept(t::COMMA) {
                    ast.curves.push(curve(&mut r)?);
                }
            }
            _ => {
                if !is_option(&r) {
                    return Err(r.fail("an axis, a curve, an option, a title, or a line break"));
                }
                ast.options.push(option(&mut r)?);
                while r.accept(t::COMMA) {
                    if !is_option(&r) {
                        return Err(r.fail("an option"));
                    }
                    ast.options.push(option(&mut r)?);
                }
            }
        }
    }
    Ok(ast)
}
