//! The state parser: a hand-written equivalent of Mermaid's Jison grammar that reports the same
//! errors at the same tokens. Port of Pele's `src/diagrams/state/parser.ts`.

use super::db::StateDb;
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::{Description, Doc, RelationStmt, StateNote, StateStmt, Stmt};
use crate::error::{Code, Error, locate};
use crate::text::label::{trim, trim_start};

/// The statement tokens the grammar expects, as its errors list them.
const STATEMENT: &str = "'NL', 'ID', 'HIDE_EMPTY', 'scale', 'COMPOSIT_STATE', 'STATE_DESCR', \
    'FORK', 'JOIN', 'CHOICE', 'CONCURRENT', 'note', 'acc_title', 'acc_descr', \
    'acc_descr_multiline_value', 'CLICK', 'classDef', 'style', 'class', 'direction_tb', \
    'direction_bt', 'direction_rl', 'direction_lr', 'EDGE_STATE'";

/// Parser state over the token list.
struct Parser<'a> {
    error: Option<String>,
    i: usize,
    src: &'a str,
    tokens: Vec<Token<'a>>,
}

impl<'a> Parser<'a> {
    fn token(&self, at: usize) -> Option<&Token<'a>> {
        self.tokens.get(at)
    }

    fn kind(&self) -> T {
        self.kind_at(self.i)
    }

    fn kind_at(&self, at: usize) -> T {
        self.token(at).map_or(T::End, |token| token.kind)
    }

    /// The current token's text, then moves past it.
    fn take(&mut self) -> &'a str {
        let text = self.token(self.i).map_or("", |token| token.text);
        self.i += 1;
        text
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let t = self.kind();
        let start = self
            .token(self.i)
            .map_or(self.src.len(), |token| token.start);
        if t == T::Error {
            if let Some(message) = &self.error {
                let location = locate(self.src, start);
                let mut error = Error::new(Code::Syntax, message.as_str());
                error.column = location.column;
                error.line = location.line;
                error.snippet = location.snippet;
                error.r#type = Some("state");
                return error;
            }
            return Error::syntax("state", self.src, start, "", true);
        }

        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("state", self.src, start, &detail, false)
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

    fn id_statement(&mut self) -> Result<StateStmt, Error> {
        if !matches!(self.kind(), T::Id | T::EdgeState) {
            return Err(self.fail("'ID', 'EDGE_STATE'"));
        }
        let id = trim(self.take()).to_owned();
        let classes = if self.kind() == T::StyleSeparator {
            self.i += 1;
            Some(vec![trim(self.expect(T::Id)?).to_owned()])
        } else {
            None
        };
        Ok(StateStmt {
            classes,
            description: Some(Description::One(String::new())),
            id,
            r#type: Some("default"),
            ..StateStmt::default()
        })
    }
}

/// A composite state that opens a body.
fn composite(id: &str, description: &str) -> Stmt {
    Stmt::State(StateStmt {
        description: Some(Description::One(description.to_owned())),
        doc: Some(Doc::default()),
        id: id.to_owned(),
        r#type: Some("default"),
        ..StateStmt::default()
    })
}

/// A state of `type` named by a stereotype.
fn typed(id: &str, r#type: &'static str) -> Stmt {
    Stmt::State(StateStmt {
        id: id.to_owned(),
        r#type: Some(r#type),
        ..StateStmt::default()
    })
}

/// Parses preprocessed state diagram source into `db`, as Pele's `parseState` does.
#[expect(
    clippy::too_many_lines,
    reason = "mirrors Pele's parser loop, one arm per statement"
)]
pub(crate) fn parse_state(src: &str, db: &mut StateDb) -> Result<(), Error> {
    let lexed = tokenize(src);
    let mut p = Parser {
        error: lexed.error,
        i: 0,
        src,
        tokens: lexed.tokens,
    };

    while p.kind() == T::Nl {
        p.i += 1;
    }
    if p.kind() != T::Sd {
        return Err(p.fail("'NL', 'SD'"));
    }
    p.i += 1;

    // The open documents, innermost last. Each composite's body is built here and moved into its
    // state when the body closes.
    let mut docs: Vec<Vec<Stmt>> = vec![Vec::new()];
    // The grammar drops a statement whose value is the string it uses for a blank line.
    let add = |docs: &mut Vec<Vec<Stmt>>, value: Stmt| {
        if matches!(&value, Stmt::Text(text) if text == "nl") {
            return;
        }
        if let Some(doc) = docs.last_mut() {
            doc.push(value);
        }
    };
    let open = |p: &mut Parser<'_>, docs: &mut Vec<Vec<Stmt>>, stmt: Stmt| {
        p.i += 1;
        if let Some(doc) = docs.last_mut() {
            doc.push(stmt);
        }
        docs.push(Vec::new());
    };

    loop {
        let t = p.kind();
        match t {
            T::Nl => p.i += 1,
            T::Id | T::EdgeState => {
                let mut first = p.id_statement()?;
                if p.kind() == T::Descr {
                    first.description = Some(Description::One(trim_colon(p.take())));
                    add(&mut docs, Stmt::State(first));
                } else if p.kind() == T::Arrow {
                    p.i += 1;
                    let second = p.id_statement()?;
                    let description = if p.kind() == T::Descr {
                        Some(trim_colon(p.take()))
                    } else {
                        None
                    };
                    add(
                        &mut docs,
                        Stmt::Relation(Box::new(RelationStmt {
                            description,
                            state1: first,
                            state2: second,
                        })),
                    );
                } else {
                    add(&mut docs, Stmt::State(first));
                }
            }
            T::HideEmpty => {
                let text = p.take().to_owned();
                add(&mut docs, Stmt::Text(text));
            }
            T::Scale => {
                let text = p.take().to_owned();
                p.expect(T::Width)?;
                add(&mut docs, Stmt::Text(text));
            }
            T::CompositState => {
                if p.kind_at(p.i + 1) == T::StructStart {
                    let id = p.take();
                    let stmt = composite(id, "");
                    open(&mut p, &mut docs, stmt);
                } else {
                    let text = p.take().to_owned();
                    add(&mut docs, Stmt::Text(text));
                }
            }
            T::StateDescr => {
                let description = p.take();
                p.expect(T::As)?;
                let id = p.expect(T::Id)?;
                if p.kind() == T::StructStart {
                    let stmt = composite(id, description);
                    open(&mut p, &mut docs, stmt);
                } else if let Some(colon) = id.find(':') {
                    let head = id.get(..colon).unwrap_or("");
                    let rest = id.get(colon + 1..).unwrap_or("");
                    let second = rest.split(':').next().unwrap_or("");
                    add(
                        &mut docs,
                        Stmt::State(StateStmt {
                            description: Some(Description::Many(vec![
                                trim(description).to_owned(),
                                second.to_owned(),
                            ])),
                            id: head.to_owned(),
                            r#type: Some("default"),
                            ..StateStmt::default()
                        }),
                    );
                } else {
                    add(
                        &mut docs,
                        Stmt::State(StateStmt {
                            description: Some(Description::One(trim(description).to_owned())),
                            id: id.to_owned(),
                            r#type: Some("default"),
                            ..StateStmt::default()
                        }),
                    );
                }
            }
            T::Fork => {
                let id = p.take();
                add(&mut docs, typed(id, "fork"));
            }
            T::Join => {
                let id = p.take();
                add(&mut docs, typed(id, "join"));
            }
            T::Choice => {
                let id = p.take();
                add(&mut docs, typed(id, "choice"));
            }
            T::Concurrent => {
                p.i += 1;
                let id = db.divider_id();
                add(
                    &mut docs,
                    Stmt::State(StateStmt {
                        id,
                        r#type: Some("divider"),
                        ..StateStmt::default()
                    }),
                );
            }
            T::Note => {
                let keyword = p.take();
                if matches!(p.kind(), T::LeftOf | T::RightOf) {
                    let position = trim(p.take()).to_owned();
                    let id = trim(p.expect(T::Id)?).to_owned();
                    let text = trim(p.expect(T::NoteText)?).to_owned();
                    add(
                        &mut docs,
                        Stmt::State(StateStmt {
                            id,
                            note: Some(StateNote {
                                position: Some(position),
                                text,
                            }),
                            ..StateStmt::default()
                        }),
                    );
                } else {
                    if p.kind() != T::NoteText {
                        return Err(p.fail("'NOTE_TEXT', 'left_of', 'right_of'"));
                    }
                    p.i += 1;
                    p.expect(T::As)?;
                    p.expect(T::Id)?;
                    add(&mut docs, Stmt::Text(keyword.to_owned()));
                }
            }
            T::DirectionTb | T::DirectionBt | T::DirectionRl | T::DirectionLr => {
                let value = match t {
                    T::DirectionBt => "BT",
                    T::DirectionRl => "RL",
                    T::DirectionLr => "LR",
                    _ => "TB",
                };
                p.i += 1;
                // Pele's `db.setDirection` writes into the root document the grammar replaces
                // when it finishes, so it has no effect and is left out.
                add(&mut docs, Stmt::Dir(value));
            }
            T::AccTitle => {
                p.i += 1;
                let value = trim(p.expect(T::AccTitleValue)?);
                db.acc_title = Some(trim_start(value).to_owned());
                add(&mut docs, Stmt::Text(value.to_owned()));
            }
            T::AccDescr => {
                p.i += 1;
                let value = trim(p.expect(T::AccDescrValue)?);
                db.acc_descr = Some(acc_description(value));
                add(&mut docs, Stmt::Text(value.to_owned()));
            }
            T::AccDescrMultilineValue => {
                let value = trim(p.take());
                db.acc_descr = Some(acc_description(value));
                add(&mut docs, Stmt::Text(value.to_owned()));
            }
            T::Click => {
                p.i += 1;
                let id = p.id_statement()?;
                let (url, tooltip) = if p.kind() == T::Href {
                    p.i += 1;
                    (p.expect(T::String)?, "")
                } else {
                    if p.kind() != T::String {
                        return Err(p.fail("'STRING', 'HREF'"));
                    }
                    let url = p.take();
                    (url, p.expect(T::String)?)
                };
                p.expect(T::Nl)?;
                add(
                    &mut docs,
                    Stmt::Click {
                        id,
                        tooltip: tooltip.to_owned(),
                        url: url.to_owned(),
                    },
                );
            }
            T::ClassDef => {
                p.i += 1;
                let id = trim(p.expect(T::ClassDefId)?).to_owned();
                let classes = trim(p.expect(T::ClassDefStyleOpts)?).to_owned();
                add(&mut docs, Stmt::ClassDef { classes, id });
            }
            T::Style => {
                p.i += 1;
                let id = trim(p.expect(T::StyleIds)?).to_owned();
                let style_class = trim(p.expect(T::StyleDefStyleOpts)?).to_owned();
                add(&mut docs, Stmt::Style { id, style_class });
            }
            T::Class => {
                p.i += 1;
                let id = trim(p.expect(T::ClassEntityIds)?).to_owned();
                let style_class = trim(p.expect(T::StyleClass)?).to_owned();
                add(&mut docs, Stmt::ApplyClass { id, style_class });
            }
            T::StructStop => {
                if docs.len() <= 1 {
                    return Err(p.fail(&expected("'$end'")));
                }
                p.i += 1;
                close(&mut docs);
            }
            T::End => {
                if docs.len() > 1 {
                    return Err(p.fail(&expected("'STRUCT_STOP'")));
                }
                let doc = docs.pop().unwrap_or_default();
                return db.set_root_doc(doc);
            }
            _ => {
                let first = if docs.len() > 1 {
                    "'STRUCT_STOP'"
                } else {
                    "'$end'"
                };
                return Err(p.fail(&expected(first)));
            }
        }
    }
}

/// `first` followed by every statement token.
fn expected(first: &str) -> String {
    let mut out = String::from(first);
    out.push_str(", ");
    out.push_str(STATEMENT);
    out
}

/// Closes the innermost body: moves it into the composite state that opened it.
fn close(docs: &mut Vec<Vec<Stmt>>) {
    let Some(body) = docs.pop() else { return };
    if let Some(Stmt::State(state)) = docs.last_mut().and_then(|doc| doc.last_mut()) {
        state.doc = Some(Doc(body));
    }
}

/// Pele's `trimColon`.
fn trim_colon(text: &str) -> String {
    match text.strip_prefix(':') {
        Some(rest) => trim(rest).to_owned(),
        None => trim(text).to_owned(),
    }
}

/// Pele's `setAccDescription`: each line break and the whitespace after it become one line break.
fn acc_description(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars
                .peek()
                .is_some_and(|&c| crate::util::chars::is_space(u32::from(c)))
            {
                chars.next();
            }
        }
    }
    out
}
