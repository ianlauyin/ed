//! `railroad-abnf-beta`: ABNF as in RFC 5234, with each rule closed by a semicolon. Port of
//! Pele's `src/diagrams/railroad/abnf.ts`.

use super::common::{
    ACC_DESCR, ACC_TITLE, AstNode, Grammar, LETTERS, TITLE, add, attach, choice, first, opening,
    optional, parse_grammar, push, sequence, to_rules, unquote, whitespace, word,
};
use super::types::{RailroadNode, RailroadRule};
use crate::diagrams::common::tokens::{
    DIRECTIVE, Pattern, Reader, SINGLE_LINE_COMMENT, TokenType, YAML, literal,
};
use crate::error::Error;

/// Token kinds, as indices into [`ABNF_TOKENS`].
mod t {
    pub(super) const EQUALS: u16 = 1;
    pub(super) const SEMICOLON: u16 = 2;
    pub(super) const SLASH: u16 = 3;
    pub(super) const GROUP: u16 = 4;
    pub(super) const OPTION: u16 = 6;
    pub(super) const TITLE: u16 = 8;
    pub(super) const NAME: u16 = 11;
    pub(super) const STRING: u16 = 12;
    pub(super) const NUMBER: u16 = 13;
    pub(super) const REPEAT: u16 = 14;
    pub(super) const EXACT: u16 = 15;
    pub(super) const COMMENT: u16 = 20;
}

/// Where a run of bytes that `class` takes ends, from `at`.
fn run(src: &str, at: usize, class: fn(&u8) -> bool) -> usize {
    let mut i = at;
    while src.as_bytes().get(i).is_some_and(class) {
        i += 1;
    }
    i
}

/// `[A-Za-z][A-Za-z0-9-]*`.
fn rule_name(src: &str, at: usize) -> Option<usize> {
    if !src.as_bytes().get(at)?.is_ascii_alphabetic() {
        return None;
    }
    Some(run(src, at + 1, |b| {
        b.is_ascii_alphanumeric() || *b == b'-'
    }))
}

/// `"[^"]*"`.
fn string(src: &str, at: usize) -> Option<usize> {
    if src.as_bytes().get(at) != Some(&b'"') {
        return None;
    }
    let close = src.get(at + 1..)?.find('"')?;
    Some(at + 1 + close + 1)
}

/// `%[xXdDbB][0-9A-Fa-f]+(?:-[0-9A-Fa-f]+|\.[0-9A-Fa-f]+)*`.
fn num_val(src: &str, at: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if bytes.get(at) != Some(&b'%')
        || !matches!(
            bytes.get(at + 1),
            Some(b'x' | b'X' | b'd' | b'D' | b'b' | b'B')
        )
    {
        return None;
    }
    let mut end = run(src, at + 2, u8::is_ascii_hexdigit);
    if end == at + 2 {
        return None;
    }
    while matches!(bytes.get(end), Some(b'-' | b'.')) {
        let next = run(src, end + 1, u8::is_ascii_hexdigit);
        if next == end + 1 {
            break;
        }
        end = next;
    }
    Some(end)
}

/// `[0-9]*\*[0-9]*`.
fn repeat(src: &str, at: usize) -> Option<usize> {
    let star = run(src, at, u8::is_ascii_digit);
    (src.as_bytes().get(star) == Some(&b'*')).then(|| run(src, star + 1, u8::is_ascii_digit))
}

/// `[0-9]+`.
fn exact(src: &str, at: usize) -> Option<usize> {
    let end = run(src, at, u8::is_ascii_digit);
    (end > at).then_some(end)
}

/// `;[^\n\r]*`.
fn comment(src: &str, at: usize) -> Option<usize> {
    if src.as_bytes().get(at) != Some(&b';') {
        return None;
    }
    Some(
        src.get(at + 1..)
            .and_then(|rest| rest.find(['\n', '\r']))
            .map_or(src.len(), |found| at + 1 + found),
    )
}

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

/// Pele's `ABNF_TOKENS`, in matching order.
pub(crate) const ABNF_TOKENS: &[TokenType] = &[
    opening("railroad-abnf-beta", &[t::NAME]),
    literal("="),
    // A semicolon closes a rule only at the end of its line; with anything after it, it starts
    // a comment.
    word(";", &[t::COMMENT]),
    literal("/"),
    literal("("),
    literal(")"),
    literal("["),
    literal("]"),
    TITLE,
    ACC_TITLE,
    ACC_DESCR,
    matched("ABNF_RULENAME", LETTERS, rule_name),
    matched("ABNF_STRING", "\"", string),
    matched("ABNF_NUMVAL", "%", num_val),
    matched("ABNF_REPEAT", "0123456789*", repeat),
    matched("ABNF_EXACT_REPEAT", "0123456789", exact),
    whitespace("ABNF_WHITESPACE"),
    TokenType {
        first: Some("-"),
        name: "ABNF_YAML",
        ..YAML
    },
    TokenType {
        first: Some("\t %"),
        name: "ABNF_DIRECTIVE",
        ..DIRECTIVE
    },
    TokenType {
        first: Some("\t %"),
        name: "ABNF_SINGLE_LINE_COMMENT",
        ..SINGLE_LINE_COMMENT
    },
    TokenType {
        hidden: true,
        ..matched("ABNF_COMMENT", ";", comment)
    },
];

/// A node of the ABNF syntax tree. Its parts are an alternation's concatenations, a
/// concatenation's elements, an element's primary, or a group's alternation.
#[derive(Clone, Debug)]
pub(crate) enum Abnf {
    /// Alternatives.
    Alternation,
    /// Elements one after another.
    Concatenation,
    /// A primary with the repeat written before it.
    Element(Option<String>),
    /// `( ... )`.
    Group,
    /// `%x41` and the like, as written.
    NumVal(String),
    /// `[ ... ]`.
    OptionalGroup,
    /// A rule name.
    RuleName(String),
    /// A string, without its quotes.
    StringLiteral(String),
}

/// One open bracket, or the rule itself at the bottom of the stack.
struct Frame {
    /// The bracket's alternation.
    alternation: usize,
    /// The token that closes the bracket, or none for the rule.
    close: Option<u16>,
    /// The concatenation being read.
    concatenation: usize,
    /// The bracket's node.
    node: Option<usize>,
    /// The repeat written before the bracket.
    repeat: Option<String>,
}

/// A frame for the alternation `alternation`, with its first concatenation.
fn frame(
    ast: &mut Vec<AstNode<Abnf>>,
    node: Option<usize>,
    alternation: usize,
    close: Option<u16>,
    repeat: Option<String>,
) -> Frame {
    let concatenation = add(ast, Abnf::Concatenation);
    attach(ast, alternation, concatenation);
    Frame {
        alternation,
        close,
        concatenation,
        node,
        repeat,
    }
}

/// Reads a rule's definition into `ast`, returning its alternation.
fn alternation(r: &mut Reader<'_>, ast: &mut Vec<AstNode<Abnf>>) -> Result<usize, Error> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = add(ast, Abnf::Alternation);
    let mut top = frame(ast, None, root, None, None);
    loop {
        let mut repeat = if r.kind() == t::REPEAT || r.kind() == t::EXACT {
            Some(r.take().to_owned())
        } else {
            None
        };
        let kind = r.kind();
        let mut primary = match kind {
            t::STRING => add(ast, Abnf::StringLiteral(unquote(r.take()))),
            t::NUMBER => add(ast, Abnf::NumVal(r.take().to_owned())),
            t::NAME => add(ast, Abnf::RuleName(r.take().to_owned())),
            t::GROUP | t::OPTION => {
                r.i += 1;
                let element = add(ast, Abnf::Alternation);
                let bracket = add(
                    ast,
                    if kind == t::GROUP {
                        Abnf::Group
                    } else {
                        Abnf::OptionalGroup
                    },
                );
                attach(ast, bracket, element);
                let next = frame(ast, Some(bracket), element, Some(kind + 1), repeat);
                stack.push(std::mem::replace(&mut top, next));
                continue;
            }
            _ => return Err(r.fail("a string, a number, a name, or an opening bracket")),
        };
        loop {
            let element = add(ast, Abnf::Element(repeat.take()));
            attach(ast, element, primary);
            attach(ast, top.concatenation, element);
            let k = r.kind();
            if top.close == Some(k) {
                r.i += 1;
                primary = top.node.unwrap_or(usize::MAX);
                repeat = top.repeat.take();
                let Some(outer) = stack.pop() else {
                    return Err(r.fail("an expression"));
                };
                top = outer;
                continue;
            }
            match k {
                t::SLASH => {
                    r.i += 1;
                    let concatenation = add(ast, Abnf::Concatenation);
                    attach(ast, top.alternation, concatenation);
                    top.concatenation = concatenation;
                }
                t::STRING | t::NUMBER | t::NAME | t::GROUP | t::OPTION | t::REPEAT | t::EXACT => {}
                _ => {
                    let Some(close) = top.close.filter(|_| !stack.is_empty()) else {
                        return Ok(top.alternation);
                    };
                    let name = ABNF_TOKENS.get(usize::from(close)).map_or("", |ty| ty.name);
                    let mut expected = String::from("token of type '");
                    expected.push_str(name);
                    expected.push('\'');
                    return Err(r.fail(&expected));
                }
            }
            break;
        }
    }
}

/// Parses `railroad-abnf-beta` `src` as Pele's `parseAbnf` does, returning the statements and
/// each rule's name and definition in `ast`.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_abnf(
    src: &str,
    ast: &mut Vec<AstNode<Abnf>>,
) -> Result<Grammar<(String, usize)>, Error> {
    parse_grammar(src, ABNF_TOKENS, t::TITLE, unquote, |r| {
        let name = r.expect(t::NAME)?.to_owned();
        r.expect(t::EQUALS)?;
        let definition = alternation(r, ast)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

/// `parseInt(text, 10)` of text that starts with a digit.
fn parse_int(text: &str) -> f64 {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text.get(..end)
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(f64::NAN)
}

/// The node a part of the syntax tree builds from its parts' nodes.
fn node(part: &Abnf, kids: Vec<usize>, nodes: &mut Vec<RailroadNode>) -> usize {
    match part {
        Abnf::Alternation => choice(nodes, kids),
        Abnf::Concatenation => sequence(nodes, kids),
        Abnf::Element(repeat) => {
            let kid = first(&kids);
            let Some(repeat) = repeat.as_deref().filter(|repeat| !repeat.is_empty()) else {
                return kid;
            };
            let star = repeat.find('*');
            let min = match star {
                Some(0) => 0.0,
                _ => parse_int(repeat),
            };
            let max = match star {
                None => min,
                Some(star) if star < repeat.len() - 1 => {
                    parse_int(repeat.get(star + 1..).unwrap_or(""))
                }
                Some(_) => f64::INFINITY,
            };
            #[expect(clippy::float_cmp, reason = "Pele compares the counts exactly")]
            if min == 0.0 && max == 1.0 {
                optional(nodes, kid)
            } else {
                push(
                    nodes,
                    RailroadNode::Repetition {
                        element: kid,
                        max,
                        min,
                    },
                )
            }
        }
        Abnf::StringLiteral(value) | Abnf::NumVal(value) => {
            push(nodes, RailroadNode::Terminal(value.clone()))
        }
        Abnf::RuleName(name) => push(nodes, RailroadNode::NonTerminal(name.clone())),
        Abnf::Group => first(&kids),
        Abnf::OptionalGroup => optional(nodes, first(&kids)),
    }
}

/// The rules of an ABNF grammar: Pele's `abnfRules`.
pub(crate) fn abnf_rules(
    ast: &[AstNode<Abnf>],
    rules: &[(String, usize)],
    nodes: &mut Vec<RailroadNode>,
) -> Vec<RailroadRule> {
    to_rules(ast, rules, nodes, node)
}
