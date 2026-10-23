//! The XY chart parser. Port of Pele's `src/diagrams/xychart/parser.ts`.

use super::db::{DataPoint, XyChartDb};
use super::lexer::{Token, tokenize};
use super::tokens::T;
use crate::error::Error;
use crate::text::label::trim;

/// Tokens that continue a text.
fn is_word(t: T) -> bool {
    matches!(
        t,
        T::Amp
            | T::Num
            | T::Alpha
            | T::Plus
            | T::Equals
            | T::Mult
            | T::Dot
            | T::Brkt
            | T::Minus
            | T::Underscore
    )
}

/// Tokens that end a statement.
fn is_eol(t: T) -> bool {
    matches!(t, T::Newline | T::Semi | T::Eof)
}

/// Tokens a text may start with.
fn is_text(t: T) -> bool {
    t == T::Str || is_word(t)
}

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

    fn text_at(&self, at: usize) -> &'a str {
        self.tokens
            .get(at)
            .and_then(|token| self.src.get(token.start..token.end))
            .unwrap_or("")
    }

    fn take(&mut self) -> &'a str {
        let text = self.text_at(self.i);
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
            return Error::syntax("xychart", self.src, start, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(t.name());
        detail.push('\'');
        Error::syntax("xychart", self.src, start, &detail, false)
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

    fn text(&mut self) -> Result<String, Error> {
        if !is_text(self.t()) {
            return Err(self.fail("'STR', 'ALPHA', 'NUM'"));
        }
        let quoted = self.t() == T::Str;
        let mut text = self.take().to_owned();
        if !quoted {
            while is_word(self.t()) {
                text.push_str(self.take());
            }
        }
        Ok(text)
    }

    fn number(&mut self) -> Result<f64, Error> {
        Ok(self
            .expect(T::NumberWithDecimal)?
            .parse()
            .unwrap_or(f64::NAN))
    }

    fn range(&mut self) -> Result<(f64, f64), Error> {
        let min = self.number()?;
        self.expect(T::ArrowDelimiter)?;
        Ok((min, self.number()?))
    }

    fn points(&mut self) -> Result<Vec<DataPoint>, Error> {
        let mut out = Vec::new();
        self.expect(T::SquareBracesStart)?;
        loop {
            let value = self.number()?;
            let label = if self.t() == T::Str {
                self.take().to_owned()
            } else {
                String::new()
            };
            out.push(DataPoint { label, value });
            if self.t() != T::Comma {
                break;
            }
            self.i += 1;
        }
        self.expect(T::SquareBracesEnd)?;
        Ok(out)
    }
}

/// Parses an XY chart into `db`, following the parser Jison generates from Mermaid's grammar.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse_xychart(src: &str, db: &mut XyChartDb) -> Result<(), Error> {
    let mut p = Parser {
        i: 0,
        src,
        tokens: tokenize(src),
    };

    loop {
        if is_eol(p.t()) {
            p.i += 1;
        } else if p.t() != T::Xychart {
            break;
        } else {
            p.i += 1;
            if p.t() == T::ChartOrientation {
                let orientation = p.take();
                db.set_orientation(orientation);
            }
        }
    }

    while p.t() != T::End {
        let t = p.t();
        p.i += 1;
        match t {
            T::Title => {
                let text = p.text()?;
                db.set_diagram_title(trim(&text));
            }
            T::XAxis | T::YAxis => {
                let title = if is_text(p.t()) {
                    Some(p.text()?)
                } else {
                    None
                };
                if t == T::XAxis && p.t() == T::SquareBracesStart {
                    let mut categories = Vec::new();
                    loop {
                        p.i += 1;
                        categories.push(p.text()?);
                        if p.t() != T::Comma {
                            break;
                        }
                    }
                    p.expect(T::SquareBracesEnd)?;
                    db.set_x_axis_band(&categories);
                } else if p.t() == T::NumberWithDecimal || title.is_none() {
                    let (min, max) = p.range()?;
                    if t == T::XAxis {
                        db.set_x_axis_range_data(min, max);
                    } else {
                        db.set_y_axis_range_data(min, max);
                    }
                }
                let title = title.unwrap_or_default();
                if t == T::XAxis {
                    db.set_x_axis_title(&title);
                } else {
                    db.set_y_axis_title(&title);
                }
            }
            T::Line | T::Bar => {
                let title = if p.t() == T::SquareBracesStart {
                    String::new()
                } else {
                    p.text()?
                };
                let data = p.points()?;
                if t == T::Line {
                    db.set_line_data(&title, &data);
                } else {
                    db.set_bar_data(&title, &data);
                }
            }
            T::AccTitle => {
                let value = p.expect(T::AccTitleValue)?;
                db.set_acc_title(trim(value));
            }
            T::AccDescr => {
                let value = p.expect(T::AccDescrValue)?;
                db.set_acc_description(trim(value));
            }
            T::AccDescrMultilineValue => {
                let value = p.text_at(p.i - 1);
                db.set_acc_description(trim(value));
            }
            _ => {
                p.i -= 1;
                return Err(p.fail(
                    "'title', 'X_AXIS', 'Y_AXIS', 'LINE', 'BAR', 'acc_title', 'acc_descr', 'acc_descr_multiline_value'",
                ));
            }
        }
        while is_eol(p.t()) {
            p.i += 1;
        }
    }
    Ok(())
}
