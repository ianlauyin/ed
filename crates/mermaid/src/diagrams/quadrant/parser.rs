//! The quadrant chart parser. Port of Pele's `src/diagrams/quadrant/parser.ts`.

use super::db::{QuadrantDb, QuadrantText};
use super::lexer::{Token, tokenize};
use super::tokens::T;
use crate::error::Error;
use crate::text::label::trim;

/// Tokens that continue a label.
fn is_word(t: T) -> bool {
    matches!(
        t,
        T::Punctuation
            | T::Amp
            | T::Num
            | T::Alpha
            | T::Comma
            | T::Plus
            | T::Equals
            | T::Mult
            | T::Dot
            | T::Brkt
            | T::Underscore
            | T::UnicodeText
    )
}

/// Tokens of a class name.
fn is_id(t: T) -> bool {
    matches!(
        t,
        T::Alpha
            | T::Num
            | T::Minus
            | T::Comma
            | T::Colon
            | T::Amp
            | T::Brkt
            | T::Mult
            | T::UnicodeText
    )
}

/// Tokens of a style.
fn is_style(t: T) -> bool {
    matches!(
        t,
        T::Alpha | T::Num | T::Colon | T::Space | T::Brkt | T::Minus
    )
}

/// Tokens that end a statement.
fn is_eol(t: T) -> bool {
    matches!(t, T::Newline | T::Semi | T::Eof)
}

/// Tokens a label may start with.
fn is_text(t: T) -> bool {
    t == T::Str || t == T::MdStr || is_word(t)
}

const TEXT_TOKENS: &str = "'STR', 'MD_STR', 'ALPHA', 'NUM', 'UNICODE_TEXT', 'PUNCTUATION'";

/// Parser state over the token list.
struct Parser<'a> {
    i: usize,
    src: &'a str,
    tokens: Vec<Token>,
}

impl<'a> Parser<'a> {
    fn t(&self) -> T {
        self.tokens.get(self.i).map_or(T::End, |token| token.t)
    }

    fn text(&self) -> &'a str {
        self.tokens
            .get(self.i)
            .and_then(|token| self.src.get(token.start..token.end))
            .unwrap_or("")
    }

    fn take(&mut self) -> &'a str {
        let text = self.text();
        self.i += 1;
        text
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let start = self
            .tokens
            .get(self.i)
            .map_or(self.src.len(), |token| token.start);
        let t = self.t();
        if t == T::Error {
            return Error::syntax("quadrantChart", self.src, start, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("quadrantChart", self.src, start, &detail, false)
    }

    fn expect(&mut self, t: T) -> Result<&'a str, Error> {
        if self.t() != t {
            let mut expected = String::from("'");
            expected.push_str(t.name());
            expected.push('\'');
            return Err(self.fail(&expected));
        }
        Ok(self.take())
    }

    fn label(&mut self) -> Result<QuadrantText, Error> {
        let first = self.t();
        if !is_text(first) {
            return Err(self.fail(TEXT_TOKENS));
        }
        let mut text = self.take().to_owned();
        while is_word(self.t()) || self.t() == T::Space || self.t() == T::Minus {
            text.push_str(self.take());
        }
        Ok(QuadrantText {
            markdown: first == T::MdStr,
            text,
        })
    }

    fn styles(&mut self) -> Result<Vec<String>, Error> {
        let mut out = Vec::new();
        loop {
            if !is_style(self.t()) {
                return Err(self.fail("'ALPHA', 'NUM', 'COLON', 'SPACE', 'BRKT', 'MINUS'"));
            }
            let mut style = self.take().to_owned();
            while is_style(self.t()) {
                style.push_str(self.take());
            }
            out.push(trim(&style).to_owned());
            if self.t() != T::Comma {
                return Ok(out);
            }
            self.i += 1;
        }
    }
}

/// Parses a quadrant chart into `db`, following the parser Jison generates from Mermaid's grammar.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for an invalid point
/// style.
pub(crate) fn parse_quadrant(src: &str, db: &mut QuadrantDb) -> Result<(), Error> {
    let mut p = Parser {
        i: 0,
        src,
        tokens: tokenize(src),
    };

    while is_eol(p.t()) || p.t() == T::Space {
        p.i += 1;
    }
    p.expect(T::Quadrant)?;

    while p.t() != T::End {
        while p.t() == T::Space {
            p.i += 1;
        }
        let t = p.t();
        match t {
            T::Classdef => {
                p.i += 1;
                p.expect(T::Space)?;
                if !is_id(p.t()) {
                    return Err(p.fail("'ALPHA', 'NUM', 'UNICODE_TEXT'"));
                }
                let mut id = p.take().to_owned();
                while is_id(p.t()) {
                    id.push_str(p.take());
                }
                p.expect(T::Space)?;
                let styles = p.styles()?;
                db.add_class(id, &styles)?;
            }
            T::XAxis | T::YAxis => {
                p.i += 1;
                let mut from = p.label()?;
                let mut to = None;
                if p.t() == T::Delimiter {
                    p.i += 1;
                    if is_text(p.t()) {
                        to = Some(p.label()?);
                    } else {
                        from.text.push_str(" ⟶ ");
                    }
                }
                if t == T::XAxis {
                    db.set_x_axis_left_text(from);
                    if let Some(to) = to {
                        db.set_x_axis_right_text(to);
                    }
                } else {
                    db.set_y_axis_bottom_text(from);
                    if let Some(to) = to {
                        db.set_y_axis_top_text(to);
                    }
                }
            }
            T::Quadrant1 | T::Quadrant2 | T::Quadrant3 | T::Quadrant4 => {
                p.i += 1;
                let label = p.label()?;
                let index = match t {
                    T::Quadrant1 => 0,
                    T::Quadrant2 => 1,
                    T::Quadrant3 => 2,
                    _ => 3,
                };
                db.set_quadrant_text(index, label);
            }
            T::Title => {
                p.i += 1;
                let value = p.expect(T::TitleValue)?;
                db.set_diagram_title(trim(value));
            }
            T::AccTitle => {
                p.i += 1;
                let value = p.expect(T::AccTitleValue)?;
                db.set_acc_title(trim(value));
            }
            T::AccDescr => {
                p.i += 1;
                let value = p.expect(T::AccDescrValue)?;
                db.set_acc_description(trim(value));
            }
            T::AccDescrMultilineValue => {
                let value = p.take();
                db.set_acc_description(trim(value));
            }
            _ => {
                if is_text(t) {
                    let label = p.label()?;
                    let class_name = if p.t() == T::ClassName {
                        p.take().to_owned()
                    } else {
                        String::new()
                    };
                    p.expect(T::PointStart)?;
                    let x = p.expect(T::PointX)?;
                    let y = p.expect(T::PointY)?;
                    let styles = if is_style(p.t()) {
                        p.styles()?
                    } else {
                        Vec::new()
                    };
                    db.add_point(label, class_name, x, y, &styles)?;
                }
            }
        }
        if !is_eol(p.t()) {
            return Err(p.fail("'NEWLINE', 'SEMI', 'EOF'"));
        }
        p.i += 1;
    }
    Ok(())
}
