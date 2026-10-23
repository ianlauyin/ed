//! The requirement parser: a hand-written equivalent of Mermaid's Jison grammar. Port of Pele's
//! `src/diagrams/requirement/parser.ts`.

use super::db::RequirementDb;
use super::lexer::{Token, tokenize};
use super::tokens::T;
use crate::error::Error;
use crate::text::label::trim;

/// What the keyword tokens stand for, from [`T::Requirement`] to [`T::Traces`] in token order.
const VALUES: [&str; 21] = [
    "Requirement",
    "Functional Requirement",
    "Interface Requirement",
    "Performance Requirement",
    "Physical Requirement",
    "Design Constraint",
    "Low",
    "Medium",
    "High",
    "Analysis",
    "Demonstration",
    "Inspection",
    "Test",
    "",
    "contains",
    "copies",
    "derives",
    "satisfies",
    "verifies",
    "refines",
    "traces",
];

fn is_style_part(t: T) -> bool {
    matches!(t, T::Alpha | T::Colon | T::Brkt | T::Minus | T::Semicolon)
}

/// The parser's position in the token list.
struct Parser<'s, 't> {
    i: usize,
    src: &'s str,
    tokens: &'t [Token<'s>],
}

impl<'s> Parser<'s, '_> {
    fn kind(&self) -> T {
        self.tokens.get(self.i).map_or(T::End, |token| token.kind)
    }

    fn text(&self) -> &'s str {
        self.tokens.get(self.i).map_or("", |token| token.text)
    }

    /// Fails with Jison's wording, naming the token kinds `from` to `to` and `others` as expected.
    #[cold]
    fn fail(&self, from: T, to: T, others: &[T]) -> Error {
        let offset = self
            .tokens
            .get(self.i)
            .map_or(self.src.len(), |token| token.start);
        let t = self.kind();
        if t == T::Error {
            return Error::syntax("requirement", self.src, offset, "", true);
        }
        let mut detail = String::from("Expecting ");
        for (k, kind) in from.through(to).chain(others.iter().copied()).enumerate() {
            if k > 0 {
                detail.push_str(", ");
            }
            detail.push('\'');
            detail.push_str(kind.name());
            detail.push('\'');
        }
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("requirement", self.src, offset, &detail, false)
    }

    fn expect(&mut self, kind: T) -> Result<&'s str, Error> {
        if self.kind() != kind {
            return Err(self.fail(kind, kind, &[]));
        }
        let text = self.text();
        self.i += 1;
        Ok(text)
    }

    /// The value of the next token if it is a keyword in the given range of token kinds.
    fn keyword(&mut self, from: T, to: T) -> Result<&'static str, Error> {
        let t = self.kind();
        if t < from || t > to {
            return Err(self.fail(from, to, &[]));
        }
        self.i += 1;
        Ok(VALUES
            .get((t as usize).wrapping_sub(T::Requirement as usize))
            .copied()
            .unwrap_or(""))
    }

    fn string(&mut self) -> Result<&'s str, Error> {
        if !matches!(self.kind(), T::UnqString | T::QString) {
            return Err(self.fail(T::QString, T::UnqString, &[]));
        }
        let text = self.text();
        self.i += 1;
        Ok(text)
    }

    fn directive(&mut self, db: &mut RequirementDb) -> Result<bool, Error> {
        match self.kind() {
            T::AccTitle => {
                self.i += 1;
                db.set_acc_title(trim(self.expect(T::AccTitleValue)?));
            }
            T::AccDescr => {
                self.i += 1;
                db.set_acc_description(trim(self.expect(T::AccDescrValue)?));
            }
            T::AccDescrMultilineValue => {
                db.set_acc_description(trim(self.text()));
                self.i += 1;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn id_list(&mut self) -> Result<Vec<&'s str>, Error> {
        let mut list = Vec::new();
        loop {
            if !matches!(self.kind(), T::Alpha | T::UnqString | T::QString) {
                return Err(self.fail(T::QString, T::UnqString, &[T::Alpha]));
            }
            list.push(self.text());
            self.i += 1;
            if self.kind() != T::Comma {
                return Ok(list);
            }
            self.i += 1;
        }
    }

    fn styles_opt(&mut self) -> Result<Vec<String>, Error> {
        let mut styles = Vec::new();
        loop {
            if !is_style_part(self.kind()) {
                return Err(self.fail(T::Alpha, T::Semicolon, &[T::Minus, T::Brkt]));
            }
            let mut style = String::from(self.text());
            self.i += 1;
            while is_style_part(self.kind()) {
                style.push_str(self.text());
                self.i += 1;
            }
            styles.push(style);
            if self.kind() != T::Comma {
                return Ok(styles);
            }
            self.i += 1;
        }
    }

    fn class_list(&mut self) -> Result<Option<Vec<&'s str>>, Error> {
        let mut classes = None;
        match self.kind() {
            T::StyleSeparator => {
                self.i += 1;
                classes = Some(self.id_list()?);
                self.expect(T::StructStart)?;
            }
            T::StructStart => self.i += 1,
            _ => {
                return Err(self.fail(T::StructStart, T::StructStart, &[T::StyleSeparator]));
            }
        }
        self.expect(T::Newline)?;
        Ok(classes)
    }

    /// Reads a body up to its closing brace. The grammar is right recursive, so Mermaid applies
    /// the fields last to first and the first of two repeated fields wins.
    fn body(&mut self, db: &mut RequirementDb, requirement: bool) -> Result<(), Error> {
        let mut fields: Vec<(T, &str)> = Vec::new();
        loop {
            let t = self.kind();
            if t == T::StructStop {
                self.i += 1;
                break;
            }
            if t == T::Newline {
                self.i += 1;
                continue;
            }
            let field = if requirement {
                (T::Id..=T::Verifymthd).contains(&t)
            } else {
                t == T::Type || t == T::Docref
            };
            if !field {
                if requirement {
                    return Err(self.fail(T::Id, T::Verifymthd, &[T::Newline, T::StructStop]));
                }
                return Err(self.fail(T::Type, T::Docref, &[T::Newline, T::StructStop]));
            }
            self.i += 1;
            self.expect(T::Colonsep)?;
            let value = match t {
                T::Risk => self.keyword(T::LowRisk, T::HighRisk)?,
                T::Verifymthd => self.keyword(T::VerifyAnalysis, T::VerifyTest)?,
                _ => self.string()?,
            };
            fields.push((t, value));
            self.expect(T::Newline)?;
        }
        for &(field, value) in fields.iter().rev() {
            match field {
                T::Id => db.set_new_req_id(value),
                T::Text => db.set_new_req_text(value),
                T::Risk => db.set_new_req_risk(value),
                T::Verifymthd => db.set_new_req_verify_method(value),
                T::Type => db.set_new_element_type(value),
                _ => db.set_new_element_doc_ref(value),
            }
        }
        Ok(())
    }
}

/// Parses requirement diagram source into `db`.
///
/// # Errors
///
/// Returns a syntax error, with Mermaid's message, where the source leaves the grammar, and a
/// limit error when classes copy too many declarations.
pub(crate) fn parse_requirement(src: &str, db: &mut RequirementDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let mut p = Parser {
        i: 0,
        src,
        tokens: &tokens,
    };

    while p.directive(db)? {
        if p.kind() == T::Newline {
            p.i += 1;
        }
    }
    if p.kind() != T::Rd {
        return Err(p.fail(
            T::Rd,
            T::Rd,
            &[T::AccTitle, T::AccDescr, T::AccDescrMultilineValue],
        ));
    }
    p.i += 1;
    p.expect(T::Newline)?;

    loop {
        let t = p.kind();
        if t == T::Eof {
            return Ok(());
        }
        if t == T::Newline {
            p.i += 1;
        } else if p.directive(db)? {
        } else if (T::DirectionTb..=T::DirectionLr).contains(&t) {
            p.i += 1;
            db.set_direction(match t {
                T::DirectionTb => "TB",
                T::DirectionBt => "BT",
                T::DirectionRl => "RL",
                _ => "LR",
            });
        } else if (T::Requirement..=T::DesignConstraint).contains(&t) {
            let r#type = p.keyword(T::Requirement, T::DesignConstraint)?;
            let name = p.string()?;
            let classes = p.class_list()?;
            p.body(db, true)?;
            db.add_requirement(name, r#type);
            if let Some(classes) = classes {
                db.set_class(&[name], &classes)?;
            }
        } else if t == T::Element {
            p.i += 1;
            let name = p.string()?;
            let classes = p.class_list()?;
            p.body(db, false)?;
            db.add_element(name);
            if let Some(classes) = classes {
                db.set_class(&[name], &classes)?;
            }
        } else if t == T::Style {
            p.i += 1;
            let ids = p.id_list()?;
            let styles = p.styles_opt()?;
            db.set_css_style(&ids, &styles)?;
        } else if t == T::Classdef {
            p.i += 1;
            let ids = p.id_list()?;
            let styles = p.styles_opt()?;
            db.define_class(&ids, &styles)?;
        } else if t == T::Class {
            p.i += 1;
            let ids = p.id_list()?;
            let classes = p.id_list()?;
            db.set_class(&ids, &classes)?;
        } else if t == T::UnqString || t == T::QString {
            let id = p.text();
            p.i += 1;
            let next = p.kind();
            if next == T::StyleSeparator {
                p.i += 1;
                let classes = p.id_list()?;
                db.set_class(&[id], &classes)?;
            } else if next == T::EndArrowL || next == T::Line {
                p.i += 1;
                let relationship = p.keyword(T::Contains, T::Traces)?;
                p.expect(if next == T::Line {
                    T::EndArrowR
                } else {
                    T::Line
                })?;
                let other = p.string()?;
                if next == T::Line {
                    db.add_relationship(relationship, id, other);
                } else {
                    db.add_relationship(relationship, other, id);
                }
            } else {
                return Err(p.fail(
                    T::StyleSeparator,
                    T::StyleSeparator,
                    &[T::EndArrowL, T::Line],
                ));
            }
        } else {
            return Err(p.fail(
                T::Newline,
                T::Eof,
                &[
                    T::AccTitle,
                    T::AccDescr,
                    T::AccDescrMultilineValue,
                    T::DirectionTb,
                    T::DirectionBt,
                    T::DirectionRl,
                    T::DirectionLr,
                    T::Requirement,
                    T::FunctionalRequirement,
                    T::InterfaceRequirement,
                    T::PerformanceRequirement,
                    T::PhysicalRequirement,
                    T::DesignConstraint,
                    T::Element,
                    T::Classdef,
                    T::Class,
                    T::Style,
                    T::UnqString,
                    T::QString,
                ],
            ));
        }
    }
}
