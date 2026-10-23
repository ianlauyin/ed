//! The class parser: a hand-written equivalent of Mermaid's Jison grammar. Port of Pele's
//! `src/diagrams/class/parser.ts`.

use super::db::ClassDb;
use super::lexer::{Token, tokenize};
use super::tokens::T;
use super::types::{ClassRelation, RelationEnd};
use crate::error::Error;
use crate::text::label::trim;

/// The kinds that a name is made of.
fn is_alpha_num(t: T) -> bool {
    matches!(t, T::UnicodeText | T::Alpha | T::Minus)
}

/// The kinds that make up a style declaration.
fn is_style_part(t: T) -> bool {
    matches!(
        t,
        T::Alpha | T::Colon | T::Brkt | T::Style | T::Pct | T::Label
    )
}

/// Mermaid's relation type of an end marker token.
fn end_type(t: T) -> Option<u8> {
    match t {
        T::Aggregation => Some(0),
        T::Extension => Some(1),
        T::Composition => Some(2),
        T::Dependency => Some(3),
        T::Lollipop => Some(4),
        _ => None,
    }
}

/// The kinds a name may start with, as Jison lists them.
const NAME_TOKENS: &str = "'UNICODE_TEXT', 'NUM', 'ALPHA', 'MINUS', 'BQUOTE_STR'";

/// A namespace being parsed, with what it holds.
struct Frame {
    classes: Vec<String>,
    id: String,
    notes: Vec<String>,
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
        self.kind_at(self.i)
    }

    /// The kind of token `i`.
    fn kind_at(&self, i: usize) -> T {
        self.tokens.get(i).map_or(T::End, |token| token.kind)
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
            return Error::syntax("class", self.src, offset, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("class", self.src, offset, &detail, false)
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

    /// Takes one part of a name.
    fn alpha_num(&mut self) -> Result<&'s str, Error> {
        if !is_alpha_num(self.kind()) {
            return Err(self.fail("'UNICODE_TEXT', 'NUM', 'ALPHA', 'MINUS'"));
        }
        Ok(self.take())
    }

    /// A class name, dotted or in backticks, with its generic type when `generic`.
    fn name(&mut self, generic: bool) -> Result<String, Error> {
        let mut out = String::new();
        loop {
            if self.kind() == T::BquoteStr {
                out.push_str(self.take());
                break;
            }
            if !is_alpha_num(self.kind()) {
                return Err(self.fail(NAME_TOKENS));
            }
            out.push_str(self.take());
            let t = self.kind();
            if t == T::Dot {
                out.push('.');
                self.i += 1;
            } else if !is_alpha_num(t) && t != T::BquoteStr {
                break;
            }
        }
        if generic && self.kind() == T::Generictype {
            out.push('~');
            out.push_str(self.take());
            out.push('~');
        }
        Ok(out)
    }

    /// A `["label"]` after a name.
    fn class_label(&mut self) -> Result<&'s str, Error> {
        self.i += 1;
        let label = self.expect(T::Str)?;
        self.expect(T::Sqe)?;
        Ok(label)
    }

    /// The member lines of a class body, through its closing brace.
    fn members(&mut self) -> Result<Vec<&'s str>, Error> {
        if self.kind() != T::Member {
            return Err(self.fail("'STRUCT_STOP', 'MEMBER'"));
        }
        let mut list = Vec::new();
        while self.kind() == T::Member {
            list.push(self.take());
        }
        self.expect(T::StructStop)?;
        // The grammar collects members last to first and `addMembers` turns them back, so they
        // keep their order.
        Ok(list)
    }

    /// A `class` statement. Returns the class name as written.
    fn class_statement(&mut self, db: &mut ClassDb) -> Result<String, Error> {
        self.i += 1;
        let id = self.name(true)?;
        db.add_class(&id);
        if self.kind() == T::Sqs {
            let label = self.class_label()?;
            db.set_class_label(&id, label);
        }
        match self.kind() {
            T::StyleSeparator => {
                self.i += 1;
                let css = self.alpha_num()?;
                db.set_css_class(&id, css);
                if self.kind() == T::StructStart {
                    self.i += 1;
                    let members = self.members()?;
                    db.add_members(&id, &members);
                }
            }
            T::StructStart => {
                self.i += 1;
                if self.kind() == T::StructStop {
                    self.i += 1;
                } else {
                    let members = self.members()?;
                    db.add_members(&id, &members);
                }
            }
            T::AnnotationStart => {
                self.i += 1;
                let annotation = self.alpha_num()?;
                self.expect(T::AnnotationEnd)?;
                db.add_annotation(&id, annotation);
                if self.kind() == T::StructStart {
                    self.i += 1;
                    if self.kind() == T::StructStop {
                        self.i += 1;
                    } else {
                        let members = self.members()?;
                        db.add_members(&id, &members);
                    }
                }
            }
            _ => {}
        }
        Ok(id)
    }

    /// A `note` or `note for` statement. Returns the note's id.
    fn note_statement(&mut self, db: &mut ClassDb) -> Result<String, Error> {
        let kind = self.kind();
        self.i += 1;
        if kind == T::Note {
            let text = self.expect(T::Str)?;
            return Ok(db.add_note(text, None));
        }
        let id = self.name(true)?;
        let text = self.expect(T::Str)?;
        Ok(db.add_note(text, Some(&id)))
    }

    /// Namespaces nest, so they are parsed with a stack of open ones.
    fn namespace_statement(&mut self, db: &mut ClassDb) -> Result<(), Error> {
        let mut open: Vec<Frame> = Vec::new();
        loop {
            self.i += 1;
            let ns = self.name(false)?;
            let id = if self.kind() == T::Sqs {
                let label = self.class_label()?;
                db.add_namespace(&ns, Some(label))
            } else {
                db.add_namespace(&ns, None)
            };
            self.expect(T::StructStart)?;
            if self.kind() == T::Newline {
                self.i += 1;
            }
            open.push(Frame {
                classes: Vec::new(),
                id,
                notes: Vec::new(),
            });

            let mut item = true;
            while item {
                let t = self.kind();
                if t == T::Namespace {
                    break;
                }
                if t == T::Class {
                    let id = self.class_statement(db)?;
                    if let Some(frame) = open.last_mut() {
                        frame.classes.push(id);
                    }
                } else if t == T::Note || t == T::NoteFor {
                    let id = self.note_statement(db)?;
                    if let Some(frame) = open.last_mut() {
                        frame.notes.push(id);
                    }
                } else {
                    return Err(self.fail("'NAMESPACE', 'CLASS', 'NOTE_FOR', 'NOTE'"));
                }

                loop {
                    if self.kind() == T::Newline {
                        let next = self.kind_at(self.i + 1);
                        if matches!(next, T::Namespace | T::Class | T::Note | T::NoteFor) {
                            self.i += 1;
                            break;
                        }
                        self.i += 1;
                    }
                    self.expect(T::StructStop)?;
                    if let Some(done) = open.pop() {
                        db.add_classes_to_namespace(&done.id, &done.classes, &done.notes);
                    }
                    db.pop_namespace();
                    if open.is_empty() {
                        item = false;
                        break;
                    }
                }
            }
            if open.is_empty() {
                return Ok(());
            }
        }
    }

    /// An optional end marker.
    fn relation_end(&mut self) -> RelationEnd {
        match end_type(self.kind()) {
            Some(r#type) => {
                self.i += 1;
                RelationEnd::Type(r#type)
            }
            None => RelationEnd::None,
        }
    }

    /// A statement that starts with a name: a member line, a relation, or a bare class.
    fn class_or_relation(&mut self, db: &mut ClassDb) -> Result<(), Error> {
        let id1 = self.name(true)?;
        let mut t = self.kind();
        if t == T::Label {
            let label = self.take();
            db.add_member(&id1, ClassDb::cleanup_label(label));
            return Ok(());
        }
        let mut title1 = "none";
        if t == T::Str {
            title1 = self.take();
        } else if end_type(t).is_none() && t != T::Line && t != T::DottedLine {
            return Ok(());
        }
        let type1 = self.relation_end();
        t = self.kind();
        if t != T::Line && t != T::DottedLine {
            return Err(self.fail(
                "'AGGREGATION', 'EXTENSION', 'COMPOSITION', 'DEPENDENCY', 'LOLLIPOP', 'LINE', 'DOTTED_LINE'",
            ));
        }
        self.i += 1;
        let type2 = self.relation_end();
        let title2 = if self.kind() == T::Str {
            self.take()
        } else {
            "none"
        };
        let id2 = self.name(true)?;
        let title = if self.kind() == T::Label {
            Some(ClassDb::cleanup_label(self.take()).to_owned())
        } else {
            None
        };
        db.add_relation(ClassRelation {
            id1,
            id2,
            line_type: u8::from(t != T::Line),
            relation_title1: title1.to_owned(),
            relation_title2: title2.to_owned(),
            title,
            type1,
            type2,
        });
        Ok(())
    }

    /// Comma-separated style declarations.
    fn styles(&mut self) -> Result<Vec<String>, Error> {
        let mut out = Vec::new();
        loop {
            if !is_style_part(self.kind()) {
                return Err(self.fail(
                    "'ALPHA', 'NUM', 'COLON', 'UNIT', 'SPACE', 'BRKT', 'STYLE', 'PCT', 'LABEL'",
                ));
            }
            let mut s = String::from(self.take());
            while is_style_part(self.kind()) {
                s.push_str(self.take());
            }
            out.push(s);
            if self.kind() != T::Comma {
                return Ok(out);
            }
            self.i += 1;
        }
    }

    /// An optional tooltip string.
    fn tooltip(&mut self, db: &mut ClassDb, id: &str) {
        if self.kind() == T::Str {
            let tip = self.take();
            db.set_tooltip(id, Some(tip));
        }
    }

    /// The address, tooltip, and target of a link.
    fn link_statement(&mut self, db: &mut ClassDb, id: &str) -> Result<(), Error> {
        let href = self.expect(T::Str)?;
        let tip = if self.kind() == T::Str {
            Some(self.take())
        } else {
            None
        };
        if self.kind() == T::LinkTarget {
            let target = self.take();
            db.set_link(id, href, Some(target));
        } else {
            db.set_link(id, href, None);
        }
        db.set_tooltip(id, tip);
        Ok(())
    }

    /// One statement.
    fn statement(&mut self, db: &mut ClassDb) -> Result<(), Error> {
        let t = self.kind();
        match t {
            T::UnicodeText | T::Alpha | T::Minus | T::BquoteStr => self.class_or_relation(db),
            T::Namespace => self.namespace_statement(db),
            T::Class => self.class_statement(db).map(|_| ()),
            T::Member => {
                self.i += 1;
                Ok(())
            }
            T::AnnotationStart => {
                self.i += 1;
                let annotation = self.alpha_num()?;
                self.expect(T::AnnotationEnd)?;
                let id = self.name(true)?;
                db.add_annotation(&id, annotation);
                Ok(())
            }
            T::Callback => {
                self.i += 1;
                let id = self.name(true)?;
                self.expect(T::Str)?;
                db.set_click_event(&id);
                self.tooltip(db, &id);
                Ok(())
            }
            T::Link => {
                self.i += 1;
                let id = self.name(true)?;
                self.link_statement(db, &id)
            }
            T::Click => {
                self.i += 1;
                let id = self.name(true)?;
                if self.kind() == T::Href {
                    self.i += 1;
                    return self.link_statement(db, &id);
                }
                self.expect(T::CallbackName)?;
                if self.kind() == T::CallbackArgs {
                    self.i += 1;
                }
                db.set_click_event(&id);
                self.tooltip(db, &id);
                Ok(())
            }
            T::Style => {
                self.i += 1;
                let id = self.expect(T::Alpha)?;
                let styles = self.styles()?;
                db.set_css_style(id, &styles);
                Ok(())
            }
            T::Cssclass => {
                self.i += 1;
                let ids = self.expect(T::Str)?;
                let class_name = self.expect(T::Alpha)?;
                db.set_css_class(ids, class_name);
                Ok(())
            }
            T::Note | T::NoteFor => self.note_statement(db).map(|_| ()),
            T::Classdef => {
                self.i += 1;
                let mut ids = vec![self.expect(T::Alpha)?];
                while self.kind() == T::Comma {
                    self.i += 1;
                    ids.push(self.expect(T::Alpha)?);
                }
                let styles = self.styles()?;
                db.define_class(&ids, &styles);
                Ok(())
            }
            T::DirectionTb | T::DirectionBt | T::DirectionRl | T::DirectionLr => {
                self.i += 1;
                db.set_direction(match t {
                    T::DirectionTb => "TB",
                    T::DirectionBt => "BT",
                    T::DirectionRl => "RL",
                    _ => "LR",
                });
                Ok(())
            }
            T::AccTitle => {
                self.i += 1;
                let value = self.expect(T::AccTitleValue)?;
                db.set_acc_title(trim(value));
                Ok(())
            }
            T::AccDescr => {
                self.i += 1;
                let value = self.expect(T::AccDescrValue)?;
                db.set_acc_description(trim(value));
                Ok(())
            }
            T::AccDescrMultilineValue => {
                let value = self.take();
                db.set_acc_description(trim(value));
                Ok(())
            }
            _ => Err(self.fail(
                "'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'NAMESPACE', 'ANNOTATION_START', 'CLASS', 'MEMBER', 'NOTE_FOR', 'NOTE', 'CLASSDEF', 'direction_tb', 'CALLBACK', 'LINK', 'CLICK', 'STYLE', 'CSSCLASS', 'ALPHA', 'MINUS', 'BQUOTE_STR'",
            )),
        }
    }
}

/// Parses a class diagram into `db`, as Pele's `parseClassDiagram` does.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse_class_diagram(src: &str, db: &mut ClassDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let mut p = Parser {
        i: 0,
        src,
        tokens: &tokens,
    };

    // Mermaid's grammar also accepts statements with no `classDiagram` line before them.
    let header = p.kind() == T::ClassDiagram;
    if header {
        p.i += 1;
        p.expect(T::Newline)?;
    }
    loop {
        p.statement(db)?;
        if p.kind() != T::Newline {
            break;
        }
        p.i += 1;
        if matches!(p.kind(), T::Eof | T::End) {
            break;
        }
    }
    if header {
        p.expect(T::Eof)?;
    }
    p.expect(T::End)?;
    Ok(())
}
