//! Tokens and parser of Mermaid's `architecture-beta` Langium grammar. Port of Pele's
//! `src/diagrams/architecture/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, EOF, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING,
    TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, literal, string_value,
    title_value, tokenize,
};
use crate::error::Error;
use crate::text::label::trim;
use crate::util::chars::{is_line_end, is_word};

/// Token kinds: indices into [`ARCHITECTURE_TOKENS`], whose order is the matching order.
mod t {
    pub(super) const HEADER: u16 = 0;
    pub(super) const JUNCTION: u16 = 1;
    pub(super) const SERVICE: u16 = 2;
    pub(super) const COLUMN: u16 = 3;
    pub(super) const GROUP: u16 = 4;
    pub(super) const ALIGN: u16 = 5;
    pub(super) const ROW: u16 = 6;
    pub(super) const DASHES: u16 = 7;
    pub(super) const IN: u16 = 8;
    pub(super) const COLON: u16 = 9;
    pub(super) const DASH: u16 = 10;
    pub(super) const DIRECTION: u16 = 11;
    pub(super) const ARROW_GROUP: u16 = 12;
    pub(super) const INTO: u16 = 13;
    pub(super) const ACC_DESCR: u16 = 14;
    pub(super) const ACC_TITLE: u16 = 15;
    pub(super) const TITLE: u16 = 16;
    pub(super) const STRING: u16 = 17;
    pub(super) const ID: u16 = 18;
    pub(super) const NEWLINE: u16 = 19;
    pub(super) const YAML: u16 = 21;
    pub(super) const ICON: u16 = 24;
    pub(super) const LABEL: u16 = 25;
}

/// `[\w]([-\w]*\w)?`.
const ID: TokenType = TokenType {
    name: "ID",
    pattern: Pattern::Match(id),
    ..literal("")
};

/// A word of the grammar, which an identifier that goes on longer takes over.
const fn word(text: &'static str) -> TokenType {
    TokenType {
        longer: &[t::ID],
        ..literal(text)
    }
}

/// Mermaid's architecture tokens, in matching order.
///
/// The direction letters come before identifiers and have no longer alternative, so Mermaid
/// reads an id that starts with L, R, T or B as a direction followed by the rest.
pub(crate) const ARCHITECTURE_TOKENS: &[TokenType] = &[
    word("architecture-beta"),
    word("junction"),
    word("service"),
    word("column"),
    word("group"),
    word("align"),
    word("row"),
    TokenType {
        longer: &[t::YAML],
        ..literal("--")
    },
    word("in"),
    literal(":"),
    TokenType {
        longer: &[t::YAML],
        ..literal("-")
    },
    TokenType {
        name: "ARROW_DIRECTION",
        pattern: Pattern::Match(direction),
        ..literal("")
    },
    TokenType {
        name: "ARROW_GROUP",
        ..literal("{group}")
    },
    TokenType {
        name: "ARROW_INTO",
        pattern: Pattern::Match(into),
        ..literal("")
    },
    ACC_DESCR,
    ACC_TITLE,
    TITLE,
    STRING,
    ID,
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
    TokenType {
        name: "ARCH_ICON",
        pattern: Pattern::Match(icon),
        ..literal("")
    },
    TokenType {
        name: "ARCH_TITLE",
        pattern: Pattern::Match(label),
        ..literal("")
    },
];

/// The byte at `p`, or 0 past the end.
fn byte(src: &str, p: usize) -> u8 {
    src.as_bytes().get(p).copied().unwrap_or(0)
}

/// Whether the byte at `p` is a `\w` character.
fn word_at(src: &str, p: usize) -> bool {
    is_word(u32::from(byte(src, p)))
}

/// `[\w]([-\w]*\w)?`: the greedy run backs off to its last word character.
fn id(src: &str, p: usize) -> Option<usize> {
    if !word_at(src, p) {
        return None;
    }
    let mut end = p + 1;
    let mut q = p + 1;
    while word_at(src, q) || byte(src, q) == b'-' {
        if word_at(src, q) {
            end = q + 1;
        }
        q += 1;
    }
    Some(end)
}

/// `L|R|T|B`.
fn direction(src: &str, p: usize) -> Option<usize> {
    matches!(byte(src, p), b'L' | b'R' | b'T' | b'B').then_some(p + 1)
}

/// `<|>`.
fn into(src: &str, p: usize) -> Option<usize> {
    matches!(byte(src, p), b'<' | b'>').then_some(p + 1)
}

/// `\([\w-:]+\)`.
fn icon(src: &str, p: usize) -> Option<usize> {
    if byte(src, p) != b'(' {
        return None;
    }
    let mut q = p + 1;
    while word_at(src, q) || matches!(byte(src, q), b'-' | b':') {
        q += 1;
    }
    (q > p + 1 && byte(src, q) == b')').then_some(q + 1)
}

/// `\[(?:"([^"\\]|\\.)*"|'([^'\\]|\\.)*'|[^\[\]\r\n]+)\]`. Each alternative is tried in turn
/// and must be followed by the closing bracket.
fn label(src: &str, p: usize) -> Option<usize> {
    if byte(src, p) != b'[' {
        return None;
    }
    let q = p + 1;
    for quote in ['"', '\''] {
        if let Some(end) = quoted(src, q, quote)
            && byte(src, end) == b']'
        {
            return Some(end + 1);
        }
    }
    let rest = src.get(q..)?;
    let run = rest.find(['[', ']', '\r', '\n']).unwrap_or(rest.len());
    (run > 0 && byte(src, q + run) == b']').then_some(q + run + 1)
}

/// `Q([^Q\\]|\\.)*Q` from `p` for quote `Q`, where `.` is any character but a line terminator.
fn quoted(src: &str, p: usize, quote: char) -> Option<usize> {
    let mut chars = src.get(p..)?.char_indices();
    if chars.next()?.1 != quote {
        return None;
    }
    while let Some((at, c)) = chars.next() {
        if c == quote {
            return Some(p + at + 1);
        }
        if c == '\\' {
            let (_, next) = chars.next()?;
            if is_line_end(u32::from(next)) {
                return None;
            }
        }
    }
    None
}

/// A side of a node an edge attaches to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// Bottom.
    B,
    /// Left.
    L,
    /// Right.
    R,
    /// Top.
    T,
}

impl Side {
    /// The side named by a direction token.
    fn of(text: &str) -> Self {
        match text {
            "L" => Self::L,
            "R" => Self::R,
            "T" => Self::T,
            _ => Self::B,
        }
    }

    /// The side's letter.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::B => "B",
            Self::L => "L",
            Self::R => "R",
            Self::T => "T",
        }
    }
}

/// The direction of an alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// `align column`.
    Column,
    /// `align row`.
    Row,
}

impl Direction {
    /// The direction's keyword.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Column => "column",
            Self::Row => "row",
        }
    }
}

/// A `group` statement.
#[derive(Clone, Debug, Default)]
pub(crate) struct GroupAst {
    /// The icon name.
    pub(crate) icon: Option<String>,
    /// The id.
    pub(crate) id: String,
    /// The parent group's id.
    pub(crate) r#in: Option<String>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// A `service` statement.
#[derive(Clone, Debug, Default)]
pub(crate) struct ServiceAst {
    /// The icon name.
    pub(crate) icon: Option<String>,
    /// Text drawn in place of an icon.
    pub(crate) icon_text: Option<String>,
    /// The id.
    pub(crate) id: String,
    /// The parent group's id.
    pub(crate) r#in: Option<String>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// A `junction` statement.
#[derive(Clone, Debug, Default)]
pub(crate) struct JunctionAst {
    /// The id.
    pub(crate) id: String,
    /// The parent group's id.
    pub(crate) r#in: Option<String>,
}

/// An edge statement.
#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors Mermaid's edge fields"
)]
pub(crate) struct EdgeAst {
    /// Side of the left-hand node.
    pub(crate) lhs_dir: Side,
    /// Whether the left-hand end crosses its group's border.
    pub(crate) lhs_group: bool,
    /// Left-hand node id.
    pub(crate) lhs_id: String,
    /// Whether the left-hand end has an arrowhead.
    pub(crate) lhs_into: bool,
    /// Side of the right-hand node.
    pub(crate) rhs_dir: Side,
    /// Whether the right-hand end crosses its group's border.
    pub(crate) rhs_group: bool,
    /// Right-hand node id.
    pub(crate) rhs_id: String,
    /// Whether the right-hand end has an arrowhead.
    pub(crate) rhs_into: bool,
    /// The label.
    pub(crate) title: Option<String>,
}

/// An `align` statement.
#[derive(Clone, Debug)]
pub(crate) struct AlignmentAst {
    /// Row or column.
    pub(crate) direction: Direction,
    /// Ids of the aligned nodes.
    pub(crate) members: Vec<String>,
}

/// A parsed architecture document.
#[derive(Clone, Debug, Default)]
pub(crate) struct ArchitectureAst {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Alignments in source order.
    pub(crate) alignments: Vec<AlignmentAst>,
    /// Edges in source order.
    pub(crate) edges: Vec<EdgeAst>,
    /// Groups in source order.
    pub(crate) groups: Vec<GroupAst>,
    /// Junctions in source order.
    pub(crate) junctions: Vec<JunctionAst>,
    /// Services in source order.
    pub(crate) services: Vec<ServiceAst>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// Mermaid's `ArchitectureValueConverter` for `ARCH_TITLE`.
fn label_value(text: &str) -> String {
    let inner = text.get(1..text.len().saturating_sub(1)).unwrap_or("");
    let result = trim(inner);
    let quoted = result.len() >= 2
        && ((result.starts_with('"') && result.ends_with('"'))
            || (result.starts_with('\'') && result.ends_with('\'')));
    // `"` alone starts and ends with a quote too, and JavaScript's `slice(1, -1)` makes it empty.
    let single = result.len() == 1 && (result == "\"" || result == "'");
    if single {
        return String::new();
    }
    if !quoted {
        return result.to_owned();
    }
    let body = result.get(1..result.len() - 1).unwrap_or("");
    let body = unescape(&unescape(body, '"'), '\'');
    trim(&body).to_owned()
}

/// `text.replace(/\\Q/g, 'Q')` for quote `Q`.
fn unescape(text: &str, quote: char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&quote) {
            continue;
        }
        out.push(c);
    }
    out
}

/// Parses an architecture document.
///
/// # Errors
///
/// Returns a syntax error for text Mermaid's grammar rejects.
pub(crate) fn parse_architecture(src: &str) -> Result<ArchitectureAst, Error> {
    let mut r = Reader::new(
        tokenize(src, ARCHITECTURE_TOKENS, "architecture")?,
        ARCHITECTURE_TOKENS,
        "architecture",
    );
    let mut ast = ArchitectureAst::default();

    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    r.expect(t::HEADER)?;
    while r.kind() != EOF {
        match r.kind() {
            t::NEWLINE => {
                r.i += 1;
                continue;
            }
            t::ACC_DESCR => ast.acc_descr = Some(acc_descr_value(r.take())),
            t::ACC_TITLE => ast.acc_title = Some(acc_title_value(r.take())),
            t::TITLE => ast.title = Some(title_value(r.take())),
            t::GROUP => {
                r.i += 1;
                let mut group = GroupAst {
                    id: r.expect(t::ID)?.to_owned(),
                    ..GroupAst::default()
                };
                if r.kind() == t::ICON {
                    group.icon = Some(inner(r.take()));
                }
                if r.kind() == t::LABEL {
                    group.title = Some(label_value(r.take()));
                }
                if r.accept(t::IN) {
                    group.r#in = Some(r.expect(t::ID)?.to_owned());
                }
                ast.groups.push(group);
            }
            t::SERVICE => {
                r.i += 1;
                let mut service = ServiceAst {
                    id: r.expect(t::ID)?.to_owned(),
                    ..ServiceAst::default()
                };
                if r.kind() == t::STRING {
                    service.icon_text = Some(string_value(r.take()).into_owned());
                } else if r.kind() == t::ICON {
                    service.icon = Some(inner(r.take()));
                }
                if r.kind() == t::LABEL {
                    service.title = Some(label_value(r.take()));
                }
                if r.accept(t::IN) {
                    service.r#in = Some(r.expect(t::ID)?.to_owned());
                }
                ast.services.push(service);
            }
            t::JUNCTION => {
                r.i += 1;
                let mut junction = JunctionAst {
                    id: r.expect(t::ID)?.to_owned(),
                    r#in: None,
                };
                if r.accept(t::IN) {
                    junction.r#in = Some(r.expect(t::ID)?.to_owned());
                }
                ast.junctions.push(junction);
            }
            t::ID => ast.edges.push(edge(&mut r)?),
            t::ALIGN => {
                r.i += 1;
                if r.kind() != t::ROW && r.kind() != t::COLUMN {
                    return Err(r.fail("'row' or 'column'"));
                }
                let direction = if r.take() == "row" {
                    Direction::Row
                } else {
                    Direction::Column
                };
                let mut members = vec![r.expect(t::ID)?.to_owned(), r.expect(t::ID)?.to_owned()];
                while r.kind() == t::ID {
                    members.push(r.take().to_owned());
                }
                ast.alignments.push(AlignmentAst { direction, members });
            }
            _ => {
                return Err(r.fail(
                    "a group, a service, a junction, an edge, an alignment, a title, or a line break",
                ));
            }
        }
        end_of_line(&mut r)?;
    }
    Ok(ast)
}

/// The text between the first and last characters of a token whose ends are ASCII.
fn inner(text: &str) -> String {
    text.get(1..text.len().saturating_sub(1))
        .unwrap_or("")
        .to_owned()
}

/// An edge, from its left-hand id.
fn edge(r: &mut Reader<'_>) -> Result<EdgeAst, Error> {
    let lhs_id = r.take().to_owned();
    let lhs_group = r.accept(t::ARROW_GROUP);
    r.expect(t::COLON)?;
    let lhs_dir = Side::of(r.expect(t::DIRECTION)?);
    let lhs_into = r.accept(t::INTO);
    let mut title = None;
    if !r.accept(t::DASHES) {
        if r.kind() != t::DASH {
            return Err(r.fail("'--' or '-'"));
        }
        r.i += 1;
        title = Some(label_value(r.expect(t::LABEL)?));
        r.expect(t::DASH)?;
    }
    let rhs_into = r.accept(t::INTO);
    let rhs_dir = Side::of(r.expect(t::DIRECTION)?);
    r.expect(t::COLON)?;
    let rhs_id = r.expect(t::ID)?.to_owned();
    let rhs_group = r.accept(t::ARROW_GROUP);
    Ok(EdgeAst {
        lhs_dir,
        lhs_group,
        lhs_id,
        lhs_into,
        rhs_dir,
        rhs_group,
        rhs_id,
        rhs_into,
        title,
    })
}

/// A line break or the end of input, then any further line breaks.
fn end_of_line(r: &mut Reader<'_>) -> Result<(), Error> {
    if r.kind() == EOF {
        return Ok(());
    }
    if r.kind() != t::NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    Ok(())
}
