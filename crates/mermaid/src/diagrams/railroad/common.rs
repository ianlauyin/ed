//! What the four railroad grammars share: the title and accessibility tokens (which, unlike the
//! ones in Mermaid's common grammar, do not take leading blanks), the hidden tokens, the
//! statements before the first rule, string values, and the walk from a syntax tree to the rule
//! model. Port of Pele's `src/diagrams/railroad/common.ts`.

use super::types::RailroadNode;
use crate::diagrams::common::tokens::{
    Memo, Pattern, Reader, STRING, TokenType, acc_descr_value, acc_title_value, keyword, literal,
    title_value, tokenize,
};
use crate::error::Error;
use crate::util::chars::is_space;

/// The ASCII letters.
pub(crate) const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The ASCII letters and `_`.
const LETTERS_: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_";

/// Where the line from `at` ends: at `\n`, `\r`, or `%%`.
fn to_comment(src: &str, at: usize) -> usize {
    let bytes = src.as_bytes();
    let mut i = at;
    while let Some(&c) = bytes.get(i) {
        if c == b'\n' || c == b'\r' || (c == b'%' && bytes.get(i + 1) == Some(&b'%')) {
            break;
        }
        i += 1;
    }
    i
}

/// `title(?:[\t ][^\n\r]*?(?=%%)|[\t ][^\n\r]*|)`.
fn title(src: &str, at: usize) -> Option<usize> {
    if !src.get(at..)?.starts_with("title") {
        return None;
    }
    let r = at + 5;
    if matches!(src.as_bytes().get(r), Some(b' ' | b'\t')) {
        return Some(to_comment(src, r + 1));
    }
    Some(r)
}

/// `accTitle[\t ]*:(?:[^\n\r]*?(?=%%)|[^\n\r]*)`.
fn acc_title(src: &str, at: usize) -> Option<usize> {
    if !src.get(at..)?.starts_with("accTitle") {
        return None;
    }
    let mut i = at + 8;
    while matches!(src.as_bytes().get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    (src.as_bytes().get(i) == Some(&b':')).then(|| to_comment(src, i + 1))
}

/// `accDescr(?:[\t ]*:([^\n\r]*?(?=%%)|[^\n\r]*)|\s*{([^}]*)})`. The braced form scans to the
/// next closing brace. Once none is found, none exists further on either, so later braced
/// descriptions fail without scanning again. `memo[0]` is one past the brace that found none.
fn acc_descr(src: &str, at: usize, memo: &mut Memo) -> Option<usize> {
    if !src.get(at..)?.starts_with("accDescr") {
        return None;
    }
    let mut i = at + 8;
    while matches!(src.as_bytes().get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    if src.as_bytes().get(i) == Some(&b':') {
        return Some(to_comment(src, i + 1));
    }
    while let Some(c) = src.get(i..).and_then(|rest| rest.chars().next())
        && is_space(u32::from(c))
    {
        i += c.len_utf8();
    }
    if src.as_bytes().get(i) != Some(&b'{') || (memo[0] != 0 && i + 1 >= memo[0]) {
        return None;
    }
    let close = src.get(i + 1..).and_then(|rest| rest.find('}'));
    if close.is_none() {
        memo[0] = i + 1;
    }
    close.map(|close| i + 1 + close + 1)
}

/// The railroad `title` token type.
pub(crate) const TITLE: TokenType = TokenType {
    first: Some("t"),
    name: "TITLE",
    pattern: Pattern::Match(title),
    ..literal("")
};

/// The railroad `accTitle` token type.
pub(crate) const ACC_TITLE: TokenType = TokenType {
    first: Some("a"),
    name: "ACC_TITLE",
    pattern: Pattern::Match(acc_title),
    ..literal("")
};

/// The railroad `accDescr` token type.
pub(crate) const ACC_DESCR: TokenType = TokenType {
    first: Some("a"),
    name: "ACC_DESCR",
    pattern: Pattern::Scan(acc_descr),
    ..literal("")
};

/// A literal token type with longer alternatives.
pub(crate) const fn word(name: &'static str, longer: &'static [u16]) -> TokenType {
    TokenType {
        longer,
        ..literal(name)
    }
}

/// The diagram's keyword. A name that starts with it and goes on is a name.
pub(crate) const fn opening(word: &'static str, name: &'static [u16]) -> TokenType {
    TokenType {
        longer: name,
        ..keyword(word)
    }
}

/// `[A-Z_a-z][\w-]*`.
fn identifier_end(src: &str, at: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let first = *bytes.get(at)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut i = at + 1;
    while bytes
        .get(i)
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        i += 1;
    }
    Some(i)
}

/// An identifier token type: `[A-Z_a-z][\w-]*`.
pub(crate) const fn identifier(name: &'static str) -> TokenType {
    TokenType {
        first: Some(LETTERS_),
        name,
        pattern: Pattern::Match(identifier_end),
        ..literal("")
    }
}

/// A quoted string token type: `"([^"\\]|\\.)*"|'([^'\\]|\\.)*'`.
pub(crate) const fn quoted(name: &'static str) -> TokenType {
    TokenType { name, ..STRING }
}

/// `[\t \r\n]+`.
fn whitespace_end(src: &str, at: usize) -> Option<usize> {
    let mut i = at;
    while matches!(src.as_bytes().get(i), Some(b'\t' | b' ' | b'\r' | b'\n')) {
        i += 1;
    }
    (i > at).then_some(i)
}

/// The hidden whitespace token type every railroad grammar lists first among its hidden tokens.
pub(crate) const fn whitespace(name: &'static str) -> TokenType {
    TokenType {
        first: Some("\t \r\n"),
        hidden: true,
        name,
        pattern: Pattern::Match(whitespace_end),
        ..literal("")
    }
}

/// `\/\*`.
pub(crate) fn block_comment_opener(src: &str, at: usize) -> bool {
    src.get(at..).is_some_and(|rest| rest.starts_with("/*"))
}

/// `\/\*[\s\S]*?\*\/`.
pub(crate) fn block_comment(src: &str, at: usize) -> Option<usize> {
    if !block_comment_opener(src, at) {
        return None;
    }
    let close = src.get(at + 2..)?.find("*/")?;
    Some(at + 2 + close + 2)
}

/// A string token without its quotes, the first and last characters, which are ASCII quotes.
pub(crate) fn unquote(text: &str) -> String {
    let mut chars = text.chars();
    chars.next();
    chars.next_back();
    chars.as_str().to_owned()
}

/// Mermaid's railroad value converters know three escapes; any other escaped character stands
/// for itself. A backslash just before the closing quote stays.
pub(crate) fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return unquote(text);
    }
    let mut chars = text.chars();
    chars.next();
    chars.next_back();
    let mut inner = chars.as_str().chars().peekable();
    let mut out = String::with_capacity(text.len());
    while let Some(c) = inner.next() {
        if c == '\\'
            && let Some(next) = inner.next()
        {
            out.push(match next {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
        } else {
            out.push(c);
        }
    }
    out
}

/// The statements before the first rule, and the rules: Pele's `Grammar`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Grammar<R> {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Rules in authored order.
    pub(crate) rules: Vec<R>,
    /// The `title` value, unquoted.
    pub(crate) title: Option<String>,
}

/// Parses the keyword, then any titles and accessibility statements, then rules to the end of
/// the text. Every table starts with its keyword and has `TITLE`, `ACC_TITLE`, and `ACC_DESCR`
/// in a row from `meta`.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_grammar<'a, R>(
    src: &'a str,
    types: &'a [TokenType],
    meta: u16,
    unquote: fn(&str) -> String,
    mut rule: impl FnMut(&mut Reader<'a>) -> Result<R, Error>,
) -> Result<Grammar<R>, Error> {
    let mut r = Reader::new(tokenize(src, types, "railroad")?, types, "railroad");
    let mut ast = Grammar {
        acc_descr: None,
        acc_title: None,
        rules: Vec::new(),
        title: None,
    };
    r.expect(0)?;
    loop {
        let kind = r.kind();
        if kind == meta {
            let title = title_value(r.take());
            let quoted = match title.as_bytes().first() {
                Some(&quote @ (b'"' | b'\'')) => title.as_bytes().last() == Some(&quote),
                _ => false,
            };
            ast.title = Some(if quoted { unquote(&title) } else { title });
        } else if kind == meta + 1 {
            ast.acc_title = Some(acc_title_value(r.take()));
        } else if kind == meta + 2 {
            ast.acc_descr = Some(acc_descr_value(r.take()));
        } else {
            break;
        }
    }
    while r.kind() != crate::diagrams::common::tokens::EOF {
        ast.rules.push(rule(&mut r)?);
    }
    Ok(ast)
}

/// A node of a grammar's syntax tree: its kind and its parts, as indices into the same tree.
#[derive(Clone, Debug)]
pub(crate) struct AstNode<K> {
    /// What the node is.
    pub(crate) kind: K,
    /// The node's parts, in order.
    pub(crate) parts: Vec<usize>,
}

/// Adds a node of `kind` without parts to `ast`, returning its index.
pub(crate) fn add<K>(ast: &mut Vec<AstNode<K>>, kind: K) -> usize {
    ast.push(AstNode {
        kind,
        parts: Vec::new(),
    });
    ast.len() - 1
}

/// Adds `part` to the parts of node `node` of `ast`.
pub(crate) fn attach<K>(ast: &mut [AstNode<K>], node: usize, part: usize) {
    if let Some(node) = ast.get_mut(node) {
        node.parts.push(part);
    }
}

/// Rebuilds a tree bottom-up without recursion, so that nesting depth is bounded by memory, not
/// by the stack. `children` gives a node's parts and `build` makes a node's result from its
/// parts' results.
pub(crate) fn fold<'a, R>(
    root: usize,
    children: impl Fn(usize) -> &'a [usize],
    mut build: impl FnMut(usize, Vec<R>) -> R,
) -> R {
    let mut nodes = vec![root];
    let mut lists = vec![children(root)];
    let mut done: Vec<Vec<R>> = vec![Vec::new()];
    while let (Some(list), Some(results)) = (lists.last(), done.last()) {
        if let Some(&child) = list.get(results.len()) {
            nodes.push(child);
            lists.push(children(child));
            done.push(Vec::new());
            continue;
        }
        if nodes.len() == 1 {
            break;
        }
        let (Some(node), Some(results)) = (nodes.pop(), done.pop()) else {
            break;
        };
        lists.pop();
        let value = build(node, results);
        if let Some(parent) = done.last_mut() {
            parent.push(value);
        }
    }
    build(root, done.pop().unwrap_or_default())
}

/// Adds `node` to `nodes`, returning its index.
pub(crate) fn push(nodes: &mut Vec<RailroadNode>, node: RailroadNode) -> usize {
    nodes.push(node);
    nodes.len() - 1
}

/// A sequence of `elements`. A sequence of one is that one, in every notation.
pub(crate) fn sequence(nodes: &mut Vec<RailroadNode>, elements: Vec<usize>) -> usize {
    match elements.as_slice() {
        [one] => *one,
        _ => push(nodes, RailroadNode::Sequence(elements)),
    }
}

/// A choice of `alternatives`. A choice of one is that one, in every notation.
pub(crate) fn choice(nodes: &mut Vec<RailroadNode>, alternatives: Vec<usize>) -> usize {
    match alternatives.as_slice() {
        [one] => *one,
        _ => push(nodes, RailroadNode::Choice(alternatives)),
    }
}

/// An optional `element`.
pub(crate) fn optional(nodes: &mut Vec<RailroadNode>, element: usize) -> usize {
    push(nodes, RailroadNode::Optional(element))
}

/// `element` repeated at least `min` times, without bound.
pub(crate) fn repetition(nodes: &mut Vec<RailroadNode>, element: usize, min: f64) -> usize {
    push(
        nodes,
        RailroadNode::Repetition {
            element,
            max: f64::INFINITY,
            min,
        },
    )
}

/// The first part's result, which every node with parts has.
pub(crate) fn first(kids: &[usize]) -> usize {
    kids.first().copied().unwrap_or(usize::MAX)
}

/// Turns each rule's syntax tree in `ast` into the common model, given how one node is built
/// from its parts: Pele's `toRules`.
pub(crate) fn to_rules<K>(
    ast: &[AstNode<K>],
    rules: &[(String, usize)],
    nodes: &mut Vec<RailroadNode>,
    mut build: impl FnMut(&K, Vec<usize>, &mut Vec<RailroadNode>) -> usize,
) -> Vec<super::types::RailroadRule> {
    rules
        .iter()
        .map(|(name, definition)| super::types::RailroadRule {
            definition: fold(
                *definition,
                |node| ast.get(node).map_or(&[][..], |node| node.parts.as_slice()),
                |node, kids| match ast.get(node) {
                    Some(part) => build(&part.kind, kids, nodes),
                    None => usize::MAX,
                },
            ),
            name: name.clone(),
        })
        .collect()
}
