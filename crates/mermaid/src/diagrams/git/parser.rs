//! Reads git graph source into its syntax tree. Port of Pele's `src/diagrams/git/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, EOF, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING,
    TITLE, TokenType, WHITESPACE, YAML, acc_descr_head, acc_descr_value, acc_title_value, keyword,
    literal, string_value, title_value, tokenize,
};
use crate::error::Error;

/// Token kinds, as indices into [`GIT_TOKENS`]. The order is the matching order.
mod t {
    pub(super) const CHERRY_PICK: u16 = 0;
    pub(super) const GIT_GRAPH_COLON: u16 = 1;
    pub(super) const HIGHLIGHT: u16 = 2;
    pub(super) const GIT_GRAPH: u16 = 3;
    pub(super) const CHECKOUT: u16 = 4;
    pub(super) const REVERSE: u16 = 5;
    pub(super) const PARENT: u16 = 6;
    pub(super) const COMMIT: u16 = 7;
    pub(super) const NORMAL: u16 = 8;
    pub(super) const BRANCH: u16 = 9;
    pub(super) const ORDER: u16 = 10;
    pub(super) const SWITCH: u16 = 11;
    pub(super) const TYPE: u16 = 12;
    pub(super) const MERGE: u16 = 13;
    pub(super) const MSG: u16 = 14;
    pub(super) const TAG: u16 = 15;
    pub(super) const ID: u16 = 16;
    pub(super) const LR: u16 = 17;
    pub(super) const TB: u16 = 18;
    pub(super) const BT: u16 = 19;
    pub(super) const COLON: u16 = 20;
    pub(super) const ACC_DESCR: u16 = 21;
    pub(super) const ACC_TITLE: u16 = 22;
    pub(super) const TITLE: u16 = 23;
    pub(super) const INT: u16 = 24;
    pub(super) const STRING: u16 = 25;
    pub(super) const NEWLINE: u16 = 26;
    pub(super) const REFERENCE: u16 = 31;
}

/// Whether `b` is a regular expression `\w` character.
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\w([-\./\w]*[-\w])?`: a word character, then the longest run of `-`, `.`, `/`, and word
/// characters that ends in `-` or a word character.
fn reference(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if !bytes.get(p).copied().is_some_and(is_word) {
        return None;
    }
    let mut end = p + 1;
    let mut at = p + 1;
    while let Some(&b) = bytes.get(at) {
        if is_word(b) || b == b'-' {
            at += 1;
            end = at;
        } else if b == b'.' || b == b'/' {
            at += 1;
        } else {
            break;
        }
    }
    Some(end)
}

/// `0|[1-9][0-9]*(?!\.)`. The lookahead only guards the second branch, which gives back a digit
/// when a `.` follows.
fn int(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    match bytes.get(p) {
        Some(b'0') => Some(p + 1),
        Some(b'1'..=b'9') => {
            let mut end = p + 1;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if bytes.get(end) != Some(&b'.') {
                return Some(end);
            }
            // Any shorter run is followed by a digit.
            (end > p + 1).then_some(end - 1)
        }
        _ => None,
    }
}

/// `[\t ]*accDescr\s*{`: the braced form's start.
fn acc_descr_opener(src: &str, p: usize) -> bool {
    acc_descr_head(src, p).is_some_and(|(_, braced)| braced)
}

/// A keyword that is the start of a longer name is part of that name: `commits` is a branch.
const fn word(name: &'static str) -> TokenType {
    TokenType {
        longer: &[t::REFERENCE],
        ..literal(name)
    }
}

/// The token types, in matching order.
pub(crate) const GIT_TOKENS: &[TokenType] = &[
    word("cherry-pick"),
    literal("gitGraph:"),
    word("HIGHLIGHT"),
    TokenType {
        longer: &[t::REFERENCE],
        ..keyword("gitGraph")
    },
    word("checkout"),
    word("REVERSE"),
    literal("parent:"),
    word("commit"),
    word("NORMAL"),
    word("branch"),
    literal("order:"),
    word("switch"),
    literal("type:"),
    word("merge"),
    literal("msg:"),
    literal("tag:"),
    literal("id:"),
    word("LR"),
    word("TB"),
    word("BT"),
    literal(":"),
    // Without a closing brace the block form scans to the end of the text, and a brace is not a
    // token on its own, so the first failure is final.
    TokenType {
        opener: Some(acc_descr_opener),
        ..ACC_DESCR
    },
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
    TokenType {
        first: Some("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz"),
        name: "REFERENCE",
        pattern: Pattern::Match(reference),
        ..literal("")
    },
];

/// A commit's type as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitTypeName {
    /// `HIGHLIGHT`.
    Highlight,
    /// `NORMAL`.
    Normal,
    /// `REVERSE`.
    Reverse,
}

/// A statement of the graph.
#[derive(Clone, Debug)]
pub(crate) enum StatementAst {
    /// `branch name [order: n]`.
    Branch {
        /// The branch name.
        name: String,
        /// The written order.
        order: Option<f64>,
    },
    /// `checkout name` or `switch name`.
    Checkout {
        /// The branch name.
        branch: String,
    },
    /// `cherry-pick id: ... [tag: ...] [parent: ...]`.
    CherryPicking {
        /// The source commit id.
        id: Option<String>,
        /// The parent of a merge commit to pick.
        parent: Option<String>,
        /// Tags, in written order.
        tags: Vec<String>,
    },
    /// `commit [id: ...] [msg: ...] [tag: ...] [type: ...]`.
    Commit {
        /// The written id.
        id: Option<String>,
        /// The message.
        message: Option<String>,
        /// Tags, in written order.
        tags: Vec<String>,
        /// The written type.
        r#type: Option<CommitTypeName>,
    },
    /// `merge name [id: ...] [tag: ...] [type: ...]`.
    Merge {
        /// The branch merged in.
        branch: String,
        /// The written id.
        id: Option<String>,
        /// Tags, in written order.
        tags: Vec<String>,
        /// The written type.
        r#type: Option<CommitTypeName>,
    },
}

/// The syntax tree of a git graph.
#[derive(Clone, Debug, Default)]
pub(crate) struct GitGraphAst {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// The direction the header gives: `LR`, `TB`, or `BT`.
    pub(crate) dir: Option<&'static str>,
    /// Statements in written order.
    pub(crate) statements: Vec<StatementAst>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// Parses git graph source.
///
/// # Errors
///
/// Returns a syntax error for text Mermaid's grammar rejects.
pub(crate) fn parse_git(src: &str) -> Result<GitGraphAst, Error> {
    let tokens = tokenize(src, GIT_TOKENS, "gitGraph")?;
    let mut r = Reader::new(tokens, GIT_TOKENS, "gitGraph");
    let mut ast = GitGraphAst::default();

    while r.kind() == t::NEWLINE {
        r.i += 1;
    }
    if !r.accept(t::GIT_GRAPH_COLON) {
        r.expect(t::GIT_GRAPH)?;
        let next = r.tokens.tokens.get(r.i + 1).map(|token| token.kind);
        let dir = match r.kind() {
            t::LR => Some("LR"),
            t::TB => Some("TB"),
            t::BT => Some("BT"),
            _ => None,
        };
        if let Some(dir) = dir
            && next == Some(t::COLON)
        {
            ast.dir = Some(dir);
            r.i += 2;
        } else {
            r.accept(t::COLON);
        }
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
            t::COMMIT => {
                r.i += 1;
                let mut id = None;
                let mut message = None;
                let mut tags = Vec::new();
                let mut r#type = None;
                loop {
                    if r.accept(t::ID) {
                        id = Some(string(&mut r)?);
                    } else if r.accept(t::TAG) {
                        tags.push(string(&mut r)?);
                    } else if r.accept(t::TYPE) {
                        r#type = Some(commit_type(&mut r)?);
                    } else if r.accept(t::MSG) || r.kind() == t::STRING {
                        message = Some(string(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Commit {
                    id,
                    message,
                    tags,
                    r#type,
                });
            }
            t::BRANCH => {
                r.i += 1;
                let name = name(&mut r)?;
                let order = if r.accept(t::ORDER) {
                    Some(parse_int(r.expect(t::INT)?))
                } else {
                    None
                };
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Branch { name, order });
            }
            t::MERGE => {
                r.i += 1;
                let branch = name(&mut r)?;
                let mut id = None;
                let mut tags = Vec::new();
                let mut r#type = None;
                loop {
                    if r.accept(t::ID) {
                        id = Some(string(&mut r)?);
                    } else if r.accept(t::TAG) {
                        tags.push(string(&mut r)?);
                    } else if r.accept(t::TYPE) {
                        r#type = Some(commit_type(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Merge {
                    branch,
                    id,
                    tags,
                    r#type,
                });
            }
            t::CHECKOUT | t::SWITCH => {
                r.i += 1;
                let branch = name(&mut r)?;
                end_of_line(&mut r)?;
                ast.statements.push(StatementAst::Checkout { branch });
            }
            t::CHERRY_PICK => {
                r.i += 1;
                let mut id = None;
                let mut parent = None;
                let mut tags = Vec::new();
                loop {
                    if r.accept(t::ID) {
                        id = Some(string(&mut r)?);
                    } else if r.accept(t::TAG) {
                        tags.push(string(&mut r)?);
                    } else if r.accept(t::PARENT) {
                        parent = Some(string(&mut r)?);
                    } else {
                        break;
                    }
                }
                end_of_line(&mut r)?;
                ast.statements
                    .push(StatementAst::CherryPicking { id, parent, tags });
            }
            _ => return Err(r.fail("a statement, a title, or a line break")),
        }
    }
    Ok(ast)
}

/// The end of a statement: line breaks, or the end of input.
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

/// A branch name: a reference or a quoted string.
fn name(r: &mut Reader<'_>) -> Result<String, Error> {
    match r.kind() {
        t::REFERENCE => Ok(r.take().to_owned()),
        t::STRING => Ok(string_value(r.take()).into_owned()),
        _ => Err(r.fail("a branch name")),
    }
}

/// A quoted string's value.
fn string(r: &mut Reader<'_>) -> Result<String, Error> {
    Ok(string_value(r.expect(t::STRING)?).into_owned())
}

/// `NORMAL`, `REVERSE`, or `HIGHLIGHT`.
fn commit_type(r: &mut Reader<'_>) -> Result<CommitTypeName, Error> {
    let name = match r.kind() {
        t::NORMAL => CommitTypeName::Normal,
        t::REVERSE => CommitTypeName::Reverse,
        t::HIGHLIGHT => CommitTypeName::Highlight,
        _ => return Err(r.fail("NORMAL, REVERSE, or HIGHLIGHT")),
    };
    r.i += 1;
    Ok(name)
}

/// `parseInt` of an `INT` token, which holds only decimal digits.
fn parse_int(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}
