//! The sankey parser: rows of source, target, and amount. Port of Pele's
//! `src/diagrams/sankey/parser.ts`.

use super::db::SankeyDb;
use super::lexer::{T, Token, tokenize};
use crate::error::Error;
use crate::text::label::trim;

/// A blank as Mermaid's sankeyUtils strips it: JavaScript's `\s` without line breaks.
fn is_blank(c: char) -> bool {
    let c = u32::from(c);
    if c == 10 || c == 13 {
        return false;
    }
    if c < 128 {
        return c == 32 || c == 9 || c == 11 || c == 12;
    }
    c == 0xa0
        || c == 0x1680
        || (0x2000..=0x200a).contains(&c)
        || c == 0x2028
        || c == 0x2029
        || c == 0x202f
        || c == 0x205f
        || c == 0x3000
        || c == 0xfeff
}

/// Mermaid's sankeyUtils: strips blanks at both ends of the text, then collapses runs of line
/// breaks so that no row is empty.
pub(crate) fn prepare_text_for_parsing(text: &str) -> String {
    let text = text.trim_matches(is_blank);
    let mut out = String::with_capacity(text.len());
    let mut breaking = false;
    for c in text.chars() {
        if c == '\n' || c == '\r' {
            if !breaking {
                out.push('\n');
            }
            breaking = true;
        } else {
            out.push(c);
            breaking = false;
        }
    }
    let trimmed = trim(&out);
    if trimmed.len() == out.len() {
        out
    } else {
        trimmed.to_owned()
    }
}

/// JavaScript's `parseFloat`: the longest leading decimal literal after whitespace, or `NaN`.
pub(crate) fn parse_float(text: &str) -> f64 {
    let text = trim(text);
    let bytes = text.as_bytes();
    let sign = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let negative = bytes.first() == Some(&b'-');

    if text
        .get(sign..)
        .is_some_and(|rest| rest.starts_with("Infinity"))
    {
        return if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }

    let digits = |from: usize| {
        bytes
            .get(from..)
            .unwrap_or(&[])
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let int = digits(sign);
    let mut end = sign + int;
    let mut fraction = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        if int > 0 || fraction > 0 {
            end += 1 + fraction;
        }
    }
    if int == 0 && fraction == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent_sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + exponent_sign);
        if exponent > 0 {
            end += 1 + exponent_sign + exponent;
        }
    }
    text.get(..end)
        .and_then(|number| number.parse().ok())
        .unwrap_or(f64::NAN)
}

/// `text` with each `""` read as one `"`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\"\"") {
        out.push_str(rest.get(..at).unwrap_or(""));
        out.push('"');
        rest = rest.get(at + 2..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// Parses prepared sankey text into `db`.
///
/// # Errors
///
/// Returns a syntax error, with Mermaid's message, where the text leaves the grammar.
pub(crate) fn parse_sankey(src: &str, db: &mut SankeyDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let mut parser = Parser {
        i: 0,
        src,
        tokens: &tokens,
    };

    parser.expect(T::Sankey, "")?;
    parser.expect(T::Newline, "")?;
    loop {
        let from = parser.name()?;
        parser.expect(T::Comma, ", 'NEWLINE', 'EOF'")?;
        let to = parser.name()?;
        parser.expect(T::Comma, ", 'NEWLINE', 'EOF'")?;
        let amount = parser.field()?;
        let source = db.find_or_create_node(from);
        let target = db.find_or_create_node(to);
        db.add_link(source, target, parse_float(amount));
        if parser.kind() != Some(T::Newline) {
            break;
        }
        parser.i += 1;
    }
    if parser.kind() == Some(T::Eof) {
        parser.i += 1;
    }
    if parser.kind() != Some(T::End) {
        return Err(parser.fail("'NEWLINE', 'EOF'"));
    }
    Ok(())
}

/// The parser's position in the token list.
struct Parser<'s, 't> {
    i: usize,
    src: &'s str,
    tokens: &'t [Token<'s>],
}

impl<'s> Parser<'s, '_> {
    fn kind(&self) -> Option<T> {
        self.tokens.get(self.i).map(|token| token.kind)
    }

    #[cold]
    fn fail(&self, expected: &str) -> Error {
        let token = self.tokens.get(self.i);
        let got = match token.map(|token| token.kind) {
            Some(T::Stuck) => self
                .i
                .checked_sub(1)
                .and_then(|i| self.tokens.get(i))
                .map_or("undefined", |token| token.kind.name()),
            Some(kind) => kind.name(),
            None => "undefined",
        };
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(got);
        detail.push('\'');
        let offset = token.map_or(self.src.len(), |token| token.start);
        Error::syntax("sankey", self.src, offset, &detail, false)
    }

    fn expect(&mut self, kind: T, also: &str) -> Result<(), Error> {
        if self.kind() != Some(kind) {
            let mut expected = String::from("'");
            expected.push_str(kind.name());
            expected.push('\'');
            expected.push_str(also);
            return Err(self.fail(&expected));
        }
        self.i += 1;
        Ok(())
    }

    fn field(&mut self) -> Result<&'s str, Error> {
        if let Some(token) = self.tokens.get(self.i)
            && token.kind == T::NonEscapedText
        {
            self.i += 1;
            return Ok(token.text);
        }
        if self.kind() != Some(T::Dquote) {
            return Err(self.fail("'DQUOTE', 'NON_ESCAPED_TEXT'"));
        }
        self.i += 1;
        let Some(token) = self
            .tokens
            .get(self.i)
            .filter(|token| token.kind == T::EscapedText)
        else {
            return Err(self.fail("'ESCAPED_TEXT'"));
        };
        self.i += 1;
        self.expect(T::Dquote, "")?;
        Ok(token.text)
    }

    fn name(&mut self) -> Result<String, Error> {
        Ok(unescape(trim(self.field()?)))
    }
}
