//! The venn parser: a port of Mermaid's venn grammar. Port of Pele's
//! `src/diagrams/venn/parser.ts`.
//!
//! A token is read only when the grammar has to look at one to decide, because the lexer asks the
//! model whether an indented `text` continues a set, and the statements are what tell the model so.

use super::db::{VennDb, semantic};
use super::lexer::{T, VennLexer};
use crate::error::Error;

const STATEMENT: &str = "'EOF', 'NEWLINE', 'TITLE', 'SET', 'UNION', 'TEXT', 'INDENT_TEXT', 'STYLE'";
const VALUE: &str = "'NUMERIC', 'IDENTIFIER', 'STRING', 'HEXCOLOR', 'RGBCOLOR', 'RGBACOLOR'";

/// Parser state: the lexer, the token read ahead, and the model it builds.
struct Parser<'a, 'd> {
    db: &'d mut VennDb,
    kind: Option<T>,
    lexer: VennLexer<'a>,
    src: &'a str,
}

impl<'a> Parser<'a, '_> {
    fn peek(&mut self) -> T {
        if let Some(kind) = self.kind {
            return kind;
        }
        let kind = self.lexer.next(self.db);
        self.kind = Some(kind);
        kind
    }

    #[cold]
    fn fail(&mut self, expected: &str) -> Error {
        let kind = self.peek();
        if kind == T::Error {
            return Error::syntax("venn", self.src, self.lexer.start, "", true);
        }
        let detail = ["Expecting ", expected, ", got '", kind.name(), "'"].concat();
        Error::syntax("venn", self.src, self.lexer.start, &detail, false)
    }

    fn take(&mut self) -> &'a str {
        self.kind = None;
        self.lexer.text
    }

    fn expect(&mut self, wanted: T) -> Result<&'a str, Error> {
        if self.peek() != wanted {
            return Err(self.fail(&["'", wanted.name(), "'"].concat()));
        }
        Ok(self.take())
    }

    fn identifier(&mut self) -> Result<&'a str, Error> {
        if !matches!(self.peek(), T::Identifier | T::String) {
            return Err(self.fail("'IDENTIFIER', 'STRING'"));
        }
        Ok(self.take())
    }

    fn identifiers(&mut self) -> Result<Vec<&'a str>, Error> {
        let mut list = vec![self.identifier()?];
        while self.peek() == T::Comma {
            self.take();
            list.push(self.identifier()?);
        }
        Ok(list)
    }

    fn subset(&mut self, ids: &[&str], union: bool) -> Result<(), Error> {
        let label = if self.peek() == T::BracketLabel {
            Some(self.take())
        } else {
            None
        };
        let mut size = None;
        if self.peek() == T::Colon {
            self.take();
            size = Some(parse_float(self.expect(T::Numeric)?));
        }
        if union {
            if ids.len() < 2 {
                return Err(semantic("union requires multiple identifiers".to_owned()));
            }
            self.db.validate_union_identifiers(ids)?;
        }
        self.db.add_subset_data(ids, label, size);
        self.db.set_indent_mode(true);
        Ok(())
    }

    /// The id of a text, and the label in brackets that an id which is not a number may carry.
    /// `ids` are the sets of the region, or `None` for those of the last set or union.
    fn text(&mut self, ids: Option<&[&str]>) -> Result<(), Error> {
        let kind = self.peek();
        if !matches!(kind, T::Identifier | T::String | T::Numeric) {
            return Err(self.fail("'NUMERIC', 'IDENTIFIER', 'STRING'"));
        }
        let id = self.take();
        let label = if kind != T::Numeric && self.peek() == T::BracketLabel {
            Some(self.take())
        } else {
            None
        };
        if let Some(ids) = ids {
            self.db.add_text_data(ids, id, label);
            return Ok(());
        }
        let Some(current) = self.db.current_sets().map(<[String]>::to_vec) else {
            return Err(semantic("text requires set".to_owned()));
        };
        let current: Vec<&str> = current.iter().map(String::as_str).collect();
        self.db.add_text_data(&current, id, label);
        Ok(())
    }

    fn style(&mut self) -> Result<(), Error> {
        let ids = self.identifiers()?;
        let mut styles: Vec<(&str, String)> = Vec::new();
        loop {
            let key = self.expect(T::Identifier)?;
            self.expect(T::Colon)?;
            let value = if self.peek() == T::String {
                self.take().to_owned()
            } else {
                let mut parts: Vec<&str> = Vec::new();
                while matches!(
                    self.peek(),
                    T::Identifier | T::Numeric | T::HexColor | T::RgbColor | T::RgbaColor
                ) {
                    parts.push(self.take());
                }
                if parts.is_empty() {
                    return Err(self.fail(VALUE));
                }
                parts.join(" ")
            };
            styles.push((key, value));
            if self.peek() != T::Comma {
                break;
            }
            self.take();
        }
        self.db.add_style_data(&ids, &styles);
        Ok(())
    }
}

/// JavaScript's `parseFloat` of a numeric token: an optional sign, digits, and a fraction.
fn parse_float(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

/// Parses preprocessed venn diagram `src` into `db`.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a union of one set
/// or of unknown sets, and for an indented text before any set.
pub(crate) fn parse_venn(src: &str, db: &mut VennDb) -> Result<(), Error> {
    let mut p = Parser {
        db,
        kind: None,
        lexer: VennLexer::new(src),
        src,
    };

    while p.peek() == T::Newline {
        p.take();
    }
    p.expect(T::Venn)?;
    loop {
        let statement = p.peek();
        if statement == T::Eof {
            return Ok(());
        }
        if statement < T::Newline || statement > T::Style || statement == T::Venn {
            return Err(p.fail(STATEMENT));
        }
        let text = p.take();
        match statement {
            T::Title => {
                // The token starts with `title` and one whitespace character.
                let rest = text.get(5..).unwrap_or("");
                let skip = rest.chars().next().map_or(0, char::len_utf8);
                p.db.set_diagram_title(rest.get(skip..).unwrap_or(""));
            }
            T::Set => {
                let id = p.identifier()?;
                p.subset(&[id], false)?;
            }
            T::Union => {
                let ids = p.identifiers()?;
                p.subset(&ids, true)?;
            }
            T::Text => {
                let ids = p.identifiers()?;
                p.text(Some(&ids))?;
            }
            T::IndentText => p.text(None)?,
            T::Style => p.style()?,
            _ => {}
        }
    }
}
