//! The ER parser: a hand-written equivalent of Mermaid's Jison grammar. Port of Pele's
//! `src/diagrams/er/parser.ts`.

use super::db::ErDb;
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::{Cardinality, DocItem, ErAttribute, RelSpec};
use crate::error::Error;
use crate::text::label::trim;

/// The cardinality a token stands for.
fn cardinality(t: T) -> Option<Cardinality> {
    match t {
        T::ZeroOrOne => Some(Cardinality::ZeroOrOne),
        T::ZeroOrMore => Some(Cardinality::ZeroOrMore),
        T::OneOrMore => Some(Cardinality::OneOrMore),
        T::OnlyOne => Some(Cardinality::OnlyOne),
        T::MdParent => Some(Cardinality::MdParent),
        _ => None,
    }
}

/// Whether a token can be an entity name.
fn is_name(t: T) -> bool {
    matches!(
        t,
        T::UnicodeText | T::EntityName | T::Num | T::DecimalNum | T::EntityOne
    )
}

/// Whether a token can be part of a style declaration.
fn is_style_component(t: T) -> bool {
    matches!(t, T::StyleText | T::Num | T::Colon | T::Brkt)
}

/// The kinds an entity name may be, as Jison lists them.
const NAME: &str = "'UNICODE_TEXT', 'NUM', 'ENTITY_NAME', 'DECIMAL_NUM', 'ENTITY_ONE'";
/// The cardinality kinds, as Jison lists them.
const CARDS: &str = "'ZERO_OR_ONE', 'ZERO_OR_MORE', 'ONE_OR_MORE', 'ONLY_ONE', 'MD_PARENT'";
/// The kinds a statement may start with, as Jison lists them.
const LINE: &str = "'EOF', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'SUBGRAPH', 'direction_tb', 'direction_bt', 'direction_rl', 'direction_lr', 'CLASSDEF', 'CLASS', 'STYLE', 'UNICODE_TEXT', 'NUM', 'ENTITY_NAME', 'DECIMAL_NUM', 'ENTITY_ONE'";

/// `text` without its first and last characters, as `slice(1, -1)` leaves a quoted string.
fn unquote(text: &str) -> &str {
    let mut chars = text.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// A subgraph being parsed: what Mermaid's grammar hands to `addSubGraph`, the value of every
/// statement inside. Besides entity names that includes accessibility text and the keywords
/// `style`, `class`, and `classDef`.
struct Frame {
    id: String,
    list: Vec<DocItem>,
    title: String,
}

/// The parser's position in the token list.
struct Parser<'s, 't> {
    i: usize,
    src: &'s str,
    tokens: &'t [Token<'s>],
}

impl<'s> Parser<'s, '_> {
    /// The current token's kind.
    fn kind(&self) -> T {
        self.tokens.get(self.i).map_or(T::End, |token| token.kind)
    }

    /// The current token's text.
    fn text(&self) -> &'s str {
        self.tokens.get(self.i).map_or("", |token| token.text)
    }

    /// Takes the current token's text.
    fn take(&mut self) -> &'s str {
        let text = self.text();
        self.i += 1;
        text
    }

    /// Fails with Jison's wording, naming `expected`.
    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let offset = self
            .tokens
            .get(self.i)
            .map_or(self.src.len(), |token| token.start);
        let t = self.kind();
        if t == T::Error {
            return Error::syntax("er", self.src, offset, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(if t == T::Char { self.text() } else { t.name() });
        detail.push('\'');
        Error::syntax("er", self.src, offset, &detail, false)
    }

    /// Takes a token of `kind`, or fails.
    fn expect(&mut self, kind: T) -> Result<&'s str, Error> {
        if self.kind() != kind {
            let mut expected = String::from("'");
            expected.push_str(kind.name());
            expected.push('\'');
            return Err(self.fail(&expected));
        }
        Ok(self.take())
    }

    /// The end of a statement.
    fn separator(&mut self) -> Result<(), Error> {
        if !matches!(self.kind(), T::Newline | T::Semi | T::Eof) {
            return Err(self.fail("'EOF', 'NEWLINE', 'SEMI'"));
        }
        self.i += 1;
        Ok(())
    }

    /// An entity name, without its quotes.
    fn entity_name(&mut self) -> Result<&'s str, Error> {
        let t = self.kind();
        if !is_name(t) {
            return Err(self.fail(NAME));
        }
        let text = self.take();
        Ok(if t == T::EntityName {
            unquote(text)
        } else {
            text
        })
    }

    /// Comma-separated names.
    fn id_list(&mut self) -> Result<Vec<&'s str>, Error> {
        let mut ids = Vec::new();
        loop {
            if !matches!(self.kind(), T::UnicodeText | T::StyleText) {
                return Err(self.fail("'UNICODE_TEXT', 'STYLE_TEXT'"));
            }
            ids.push(self.take());
            if self.kind() != T::Comma {
                return Ok(ids);
            }
            self.i += 1;
        }
    }

    /// Comma-separated style declarations.
    fn styles_opt(&mut self) -> Result<Vec<String>, Error> {
        let mut styles = Vec::new();
        loop {
            if !is_style_component(self.kind()) {
                return Err(self.fail("'STYLE_TEXT', 'COLON', 'NUM', 'BRKT'"));
            }
            let mut style = String::from(self.take());
            while is_style_component(self.kind()) {
                style.push_str(self.take());
            }
            styles.push(style);
            if self.kind() != T::Comma {
                return Ok(styles);
            }
            self.i += 1;
        }
    }

    /// The cardinalities and identification of a relationship.
    fn rel_spec(&mut self) -> Result<RelSpec, Error> {
        let card_b = cardinality(self.kind()).unwrap_or(Cardinality::OnlyOne);
        self.i += 1;
        let r = self.kind();
        if r != T::Identifying && r != T::NonIdentifying {
            return Err(self.fail("'NON_IDENTIFYING', 'IDENTIFYING'"));
        }
        self.i += 1;
        let Some(card_a) = cardinality(self.kind()) else {
            return Err(self.fail(CARDS));
        };
        self.i += 1;
        Ok(RelSpec {
            card_a,
            card_b,
            identifying: r == T::Identifying,
        })
    }

    /// A relationship's label, without its quotes.
    fn role(&mut self) -> Result<&'s str, Error> {
        let t = self.kind();
        if !matches!(t, T::Word | T::EntityName | T::UnicodeText) {
            return Err(self.fail("'UNICODE_TEXT', 'ENTITY_NAME', 'WORD'"));
        }
        let text = self.take();
        Ok(if t == T::UnicodeText {
            text
        } else {
            unquote(text)
        })
    }

    /// Whether the current token is the unnamed character `ch`.
    fn is_char(&self, ch: &str) -> bool {
        self.kind() == T::Char && self.text() == ch
    }

    /// The attributes of a block, in authored order.
    fn attributes(&mut self) -> Result<Vec<ErAttribute>, Error> {
        let mut list = Vec::new();
        loop {
            let mut r#type = String::from(self.expect(T::AttributeWord)?);
            if self.is_char("?") {
                r#type.push_str(self.take());
            }
            if self.kind() != T::AttributeWord {
                return Err(self.fail(if r#type.ends_with('?') {
                    "'ATTRIBUTE_WORD'"
                } else {
                    "'ATTRIBUTE_WORD', '?'"
                }));
            }
            let name = self.take().to_owned();
            let mut keys = Vec::new();
            if self.kind() == T::AttributeKey {
                keys.push(self.take().to_owned());
                while self.is_char(",") {
                    self.i += 1;
                    keys.push(self.expect(T::AttributeKey)?.to_owned());
                }
            }
            let comment = if self.kind() == T::Comment {
                unquote(self.take()).to_owned()
            } else {
                String::new()
            };
            list.push(ErAttribute {
                comment,
                keys,
                name,
                r#type,
            });
            if self.kind() == T::BlockStop {
                return Ok(list);
            }
        }
    }

    /// Parses an optional `{ ... }` block. `None` when there is none or it is empty.
    fn block(&mut self) -> Result<Option<Vec<ErAttribute>>, Error> {
        if self.kind() != T::BlockStart {
            return Ok(None);
        }
        self.i += 1;
        if self.kind() == T::BlockStop {
            self.i += 1;
            return Ok(None);
        }
        let list = self.attributes()?;
        self.i += 1;
        Ok(Some(list))
    }

    /// A statement that starts with an entity name. Returns the names it mentions.
    fn entity_statement(&mut self, db: &mut ErDb) -> Result<Vec<String>, Error> {
        let a = self.entity_name()?;
        let mut t = self.kind();
        if t == T::Sqs {
            self.i += 1;
            let alias = self.entity_name()?;
            self.expect(T::Sqe)?;
            let classes = if self.kind() == T::StyleSeparator {
                self.i += 1;
                Some(self.id_list()?)
            } else {
                None
            };
            let attrs = self.block()?;
            db.add_entity(a, alias);
            if let Some(attrs) = attrs {
                db.add_attributes(a, attrs);
            }
            if let Some(classes) = classes {
                db.set_class(&[a], &classes);
            }
            return Ok(vec![a.to_owned()]);
        }
        let mut classes = None;
        if t == T::StyleSeparator {
            self.i += 1;
            classes = Some(self.id_list()?);
            t = self.kind();
        }
        if cardinality(t).is_some() {
            let spec = self.rel_spec()?;
            let b = self.entity_name()?;
            let classes_b = if self.kind() == T::StyleSeparator {
                self.i += 1;
                Some(self.id_list()?)
            } else {
                None
            };
            self.expect(T::Colon)?;
            let label = self.role()?;
            db.add_entity(a, "");
            db.add_entity(b, "");
            db.add_relationship(a, label, b, spec);
            if let Some(classes) = classes {
                db.set_class(&[a], &classes);
            }
            if let Some(classes) = classes_b {
                db.set_class(&[b], &classes);
            }
            return Ok(vec![a.to_owned(), b.to_owned()]);
        }
        let attrs = self.block()?;
        db.add_entity(a, "");
        if let Some(attrs) = attrs {
            db.add_attributes(a, attrs);
        }
        if let Some(classes) = classes {
            db.set_class(&[a], &classes);
        }
        Ok(vec![a.to_owned()])
    }
}

/// Parses an ER diagram into `db`, as Pele's `parseEr` does.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
#[expect(clippy::too_many_lines, reason = "mirrors Pele's parseEr")]
pub(crate) fn parse_er(src: &str, db: &mut ErDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let mut p = Parser {
        i: 0,
        src,
        tokens: &tokens,
    };

    p.expect(T::ErDiagram)?;
    // Subgraphs nest on this stack rather than on the call stack. The diagram itself is the
    // first frame.
    let mut frames: Vec<Frame> = vec![Frame {
        id: String::new(),
        list: Vec::new(),
        title: String::new(),
    }];

    loop {
        let nested = frames.len() > 1;
        let t = p.kind();
        match t {
            T::Newline => p.i += 1,
            T::Eof => {
                // The end of input is consumed first, so an open subgraph is reported at `$end`.
                p.i += 1;
                if nested {
                    let mut expected = String::from("'END', ");
                    expected.push_str(LINE);
                    return Err(p.fail(&expected));
                }
                return Ok(());
            }
            T::EndKw => {
                if !nested {
                    return Err(p.fail(LINE));
                }
                p.i += 1;
                if let Some(frame) = frames.pop() {
                    let id = db.add_sub_graph(&frame.id, &frame.list, &frame.title);
                    if frames.len() > 1
                        && let Some(parent) = frames.last_mut()
                    {
                        parent.list.push(DocItem::Name(id));
                    }
                }
            }
            T::Subgraph => {
                p.i += 1;
                let id = p.entity_name()?.to_owned();
                let mut title = id.clone();
                if p.kind() == T::Sqs {
                    p.i += 1;
                    p.entity_name()?.clone_into(&mut title);
                    while is_name(p.kind()) {
                        title.push(' ');
                        title.push_str(p.entity_name()?);
                    }
                    p.expect(T::Sqe)?;
                }
                p.separator()?;
                frames.push(Frame {
                    id,
                    list: Vec::new(),
                    title,
                });
            }
            T::AccTitle | T::AccDescr | T::AccDescrMultilineValue => {
                let value = match t {
                    T::AccTitle => {
                        p.i += 1;
                        trim(p.expect(T::AccTitleValue)?)
                    }
                    T::AccDescr => {
                        p.i += 1;
                        trim(p.expect(T::AccDescrValue)?)
                    }
                    _ => trim(p.take()),
                };
                if t == T::AccTitle {
                    db.set_acc_title(value);
                } else {
                    db.set_acc_description(value);
                }
                if nested && let Some(frame) = frames.last_mut() {
                    frame.list.push(DocItem::Name(value.to_owned()));
                }
            }
            T::DirectionTb | T::DirectionBt | T::DirectionRl | T::DirectionLr => {
                p.i += 1;
                let value = match t {
                    T::DirectionTb => "TB",
                    T::DirectionBt => "BT",
                    T::DirectionRl => "RL",
                    _ => "LR",
                };
                if nested {
                    if let Some(frame) = frames.last_mut() {
                        frame.list.push(DocItem::Dir(value));
                    }
                } else {
                    db.set_direction(value);
                }
            }
            T::Classdef | T::Style => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let styles = p.styles_opt()?;
                p.separator()?;
                if t == T::Classdef {
                    db.add_class(&ids, &styles);
                } else {
                    db.add_css_styles(&ids, &styles);
                }
                if nested && let Some(frame) = frames.last_mut() {
                    frame.list.push(DocItem::Name(keyword.to_owned()));
                }
            }
            T::Class => {
                let keyword = p.take();
                let ids = p.id_list()?;
                let names = p.id_list()?;
                db.set_class(&ids, &names);
                if nested && let Some(frame) = frames.last_mut() {
                    frame.list.push(DocItem::Name(keyword.to_owned()));
                }
            }
            _ => {
                if !is_name(t) {
                    if nested {
                        let mut expected = String::from("'END', ");
                        expected.push_str(LINE);
                        return Err(p.fail(&expected));
                    }
                    return Err(p.fail(LINE));
                }
                let names = p.entity_statement(db)?;
                if nested && let Some(frame) = frames.last_mut() {
                    frame.list.extend(names.into_iter().map(DocItem::Name));
                }
            }
        }
    }
}
