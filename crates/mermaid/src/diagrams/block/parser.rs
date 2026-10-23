//! The block parser: a hand-written equivalent of Mermaid's Jison grammar that reports the same
//! errors at the same tokens. Port of Pele's `src/diagrams/block/parser.ts`.
//!
//! Pele keeps a statement that links blocks as a list, and flattens lists when it places them.
//! Lists here are flattened as they are built, which keeps the same order.

use super::db::{
    BlockDb, edge_str_to_edge_data, edge_str_to_edge_start_data, edge_str_to_pattern,
    edge_str_to_thickness, type_str_to_type,
};
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::Block;
use crate::error::Error;
use crate::text::label::trim;

/// What a node statement names.
struct NodeInfo<'a> {
    directions: Option<Vec<String>>,
    id: &'a str,
    label: Option<&'a str>,
    type_str: Option<String>,
}

/// A parsed statement: one block, or a list of blocks and the edges between them.
enum Statement {
    Chain(Vec<usize>),
    One(usize),
}

/// A composite being read: the statements of its parent, and its header, if it has one.
struct Frame {
    header: Option<Statement>,
    list: Vec<usize>,
}

/// The tokens a statement may start with.
const STATEMENT_START: &str = "'NODE_ID', 'COLUMNS', 'SPACE_BLOCK', 'BLOCK_DIAGRAM_KEY', 'id-block', 'classDef', 'class', 'style'";

/// Parses JavaScript `parseInt` of a run of decimal digits, or of `-1`.
fn parse_int(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// Parser state over the token list.
struct Parser<'a, 'd> {
    db: &'d mut BlockDb,
    i: usize,
    src: &'a str,
    tokens: Vec<Token<'a>>,
}

impl<'a> Parser<'a, '_> {
    fn kind(&self) -> T {
        self.tokens.get(self.i).map_or(T::End, |token| token.kind)
    }

    fn text(&self) -> &'a str {
        self.tokens.get(self.i).map_or("", |token| token.text)
    }

    /// Takes the current token's text.
    fn take(&mut self) -> &'a str {
        let text = self.text();
        self.i += 1;
        text
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let token = self.tokens.get(self.i);
        let start = token.map_or(self.src.len(), |token| token.start);
        let kind = self.kind();
        if kind == T::Error {
            return Error::syntax("block", self.src, start, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(kind.name());
        detail.push('\'');
        Error::syntax("block", self.src, start, &detail, false)
    }

    fn expect(&mut self, kind: T) -> Result<&'a str, Error> {
        if self.kind() != kind {
            let mut expected = String::from("'");
            expected.push_str(kind.name());
            expected.push('\'');
            return Err(self.fail(&expected));
        }
        Ok(self.take())
    }

    fn node(&mut self) -> Result<NodeInfo<'a>, Error> {
        let id = self.expect(T::NodeId)?;
        if self.kind() == T::NodeDStart {
            let mut type_str = self.take().to_owned();
            let label = self.expect(T::Str)?;
            type_str.push_str(self.expect(T::NodeDEnd)?);
            return Ok(NodeInfo {
                directions: None,
                id,
                label: Some(label),
                type_str: Some(type_str),
            });
        }
        if self.kind() == T::BlockArrowStart {
            let mut type_str = self.take().to_owned();
            let label = self.expect(T::Str)?;
            let mut directions = vec![self.expect(T::Dir)?.to_owned()];
            while self.kind() == T::Dir {
                directions.push(self.take().to_owned());
            }
            type_str.push_str(self.expect(T::BlockArrowEnd)?);
            return Ok(NodeInfo {
                directions: Some(directions),
                id,
                label: Some(label),
                type_str: Some(type_str),
            });
        }
        Ok(NodeInfo {
            directions: None,
            id,
            label: None,
            type_str: None,
        })
    }

    fn node_statement(&mut self) -> Result<Statement, Error> {
        let first = self.node()?;
        let size = if self.kind() == T::Size {
            parse_int(self.take())
        } else {
            1.0
        };
        let from = Block {
            directions: first.directions.clone(),
            id: Some(first.id.to_owned()),
            label: first.label.map(str::to_owned),
            r#type: type_str_to_type(first.type_str.as_deref()),
            width_in_columns: Some(size),
            ..Block::default()
        };
        let mut chain: Option<Vec<usize>> = None;
        let mut last_id = first.id.to_owned();

        while matches!(self.kind(), T::Link | T::StartLink) {
            let mut label = "";
            if self.kind() == T::StartLink {
                self.i += 1;
                self.expect(T::LinkLabel)?;
                label = self.expect(T::Str)?;
                self.expect(T::Link)?;
            } else {
                self.i += 1;
            }
            let link = self.tokens.get(self.i - 1).map_or("", |token| token.text);
            let to = self.node()?;
            // Mermaid keeps only the last link of `A --> B --> C` and loses its start. Here every
            // link counts.
            let list = chain.get_or_insert_with(|| {
                vec![self.db.add(Block {
                    directions: from.directions.clone(),
                    id: from.id.clone(),
                    label: from.label.clone(),
                    r#type: from.r#type,
                    ..Block::default()
                })]
            });
            let mut edge_id = last_id.clone();
            edge_id.push('-');
            edge_id.push_str(to.id);
            let edge = self.db.add(Block {
                arrow_type_end: Some(edge_str_to_edge_data(link)),
                arrow_type_start: Some(edge_str_to_edge_start_data(link)),
                directions: to.directions.clone(),
                end: Some(to.id.to_owned()),
                id: Some(edge_id),
                label: Some(label.to_owned()),
                pattern: Some(edge_str_to_pattern(link)),
                start: Some(last_id.clone()),
                thickness: Some(edge_str_to_thickness(link)),
                r#type: "edge",
                ..Block::default()
            });
            let next = self.db.add(Block {
                directions: to.directions,
                id: Some(to.id.to_owned()),
                label: to.label.map(str::to_owned),
                r#type: type_str_to_type(to.type_str.as_deref()),
                ..Block::default()
            });
            list.push(edge);
            list.push(next);
            to.id.clone_into(&mut last_id);
        }
        Ok(match chain {
            Some(list) => Statement::Chain(list),
            None => Statement::One(self.db.add(from)),
        })
    }
}

/// Appends a statement's blocks to a list.
fn push(list: &mut Vec<usize>, statement: Statement) {
    match statement {
        Statement::Chain(blocks) => list.extend(blocks),
        Statement::One(block) => list.push(block),
    }
}

/// Parses preprocessed block diagram `src` into `db`.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a limit error for too many spaces.
pub(crate) fn parse_block(src: &str, db: &mut BlockDb) -> Result<(), Error> {
    let mut p = Parser {
        db,
        i: 0,
        src,
        tokens: tokenize(src),
    };

    p.expect(T::Key)?;
    // Composites nest without recursion: each open one keeps the statement list of its parent.
    let mut open: Vec<Frame> = Vec::new();
    let mut list: Vec<usize> = Vec::new();
    loop {
        match p.kind() {
            T::NodeId => {
                let statement = p.node_statement()?;
                push(&mut list, statement);
            }
            T::Columns => {
                let columns = parse_int(p.take());
                list.push(p.db.add(Block {
                    columns: Some(columns),
                    r#type: "column-setting",
                    ..Block::default()
                }));
            }
            T::SpaceBlock => {
                let width = parse_int(p.take());
                let id = p.db.generate_id();
                list.push(p.db.add(Block {
                    children: Some(Vec::new()),
                    id: Some(id),
                    label: Some(String::new()),
                    r#type: "space",
                    width: Some(width),
                    ..Block::default()
                }));
            }
            T::Key => {
                p.i += 1;
                open.push(Frame {
                    header: None,
                    list: std::mem::take(&mut list),
                });
            }
            T::IdBlock => {
                p.i += 1;
                if p.kind() != T::NodeId {
                    return Err(p.fail("'NODE_ID'"));
                }
                let header = p.node_statement()?;
                open.push(Frame {
                    header: Some(header),
                    list: std::mem::take(&mut list),
                });
            }
            T::EndKeyword => {
                let Some(frame) = open.pop().filter(|_| !list.is_empty()) else {
                    return Err(p.fail(STATEMENT_START));
                };
                p.i += 1;
                let id = p.db.generate_id();
                let children = std::mem::replace(&mut list, frame.list);
                let composite = match frame.header {
                    None => p.db.add(Block {
                        children: Some(children),
                        id: Some(id),
                        label: Some(String::new()),
                        r#type: "composite",
                        ..Block::default()
                    }),
                    Some(Statement::One(header)) => {
                        if let Some(block) = p.db.arena.get_mut(header) {
                            block.r#type = "composite";
                            block.children = Some(children);
                        }
                        header
                    }
                    // Spreading a list into an object keeps none of the block fields.
                    Some(Statement::Chain(_)) => p.db.add(Block {
                        children: Some(children),
                        r#type: "composite",
                        ..Block::default()
                    }),
                };
                list.push(composite);
            }
            T::ClassDef => {
                p.i += 1;
                let id = trim(p.expect(T::ClassDefId)?).to_owned();
                let css = trim(p.expect(T::ClassDefStyleOpts)?).to_owned();
                list.push(p.db.add(Block {
                    css: Some(css),
                    id: Some(id),
                    r#type: "classDef",
                    ..Block::default()
                }));
            }
            T::Class => {
                p.i += 1;
                let id = trim(p.expect(T::ClassEntityIds)?).to_owned();
                let style_class = trim(p.expect(T::StyleClass)?).to_owned();
                list.push(p.db.add(Block {
                    id: Some(id),
                    style_class: Some(style_class),
                    r#type: "applyClass",
                    ..Block::default()
                }));
            }
            T::Style => {
                p.i += 1;
                let id = trim(p.expect(T::StyleEntityIds)?).to_owned();
                let styles = trim(p.expect(T::StyleDefinitionData)?).to_owned();
                list.push(p.db.add(Block {
                    id: Some(id),
                    styles_str: Some(styles),
                    r#type: "applyStyles",
                    ..Block::default()
                }));
            }
            T::Eof => {
                if !open.is_empty() {
                    return Err(p.fail(&[STATEMENT_START, ", 'end'"].concat()));
                }
                if list.is_empty() {
                    return Err(p.fail(STATEMENT_START));
                }
                return p.db.set_hierarchy(list);
            }
            _ => {
                let expected = if list.is_empty() {
                    STATEMENT_START.to_owned()
                } else if open.is_empty() {
                    [STATEMENT_START, ", 'EOF'"].concat()
                } else {
                    [STATEMENT_START, ", 'end'"].concat()
                };
                return Err(p.fail(&expected));
            }
        }
    }
}
