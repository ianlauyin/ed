//! The Ishikawa parser. Port of Pele's `src/diagrams/ishikawa/parser.ts`.

use super::db::IshikawaDb;
use super::lexer::{T, Token, tokenize};
use crate::error::Error;
use crate::text::label::trim;

/// What may end a statement.
const STOP: &str = "'SPACELINE', 'NL', 'EOF'";

/// Follows the parser Jison builds from Mermaid's grammar: blank lines and comments may come
/// first, then the keyword and at most one line break, then statements. A statement is a line of
/// text, with or without indentation, or a blank line. Each is ended by a blank line, a line break
/// or the end of the text, and any number of line breaks after that.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects.
pub(crate) fn parse_ishikawa(src: &str, db: &mut IshikawaDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let token = |at: usize| {
        tokens.get(at).copied().unwrap_or(Token {
            end: src.len(),
            start: src.len(),
            t: T::End,
        })
    };
    let fail = |at: usize, expected: &str| {
        let token = token(at);
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(token.t.name());
        detail.push('\'');
        Error::syntax("ishikawa", src, token.start, &detail, false)
    };
    let ends = |t: T| matches!(t, T::Spaceline | T::Nl | T::Eof);

    let mut i = 0;
    if token(i).t == T::Spaceline {
        i += 1;
        while matches!(token(i).t, T::Spaceline | T::Nl) {
            i += 1;
        }
        if token(i).t != T::Ishikawa {
            return Err(fail(i, "'SPACELINE', 'NL', 'ISHIKAWA'"));
        }
    } else if token(i).t != T::Ishikawa {
        return Err(fail(i, "'SPACELINE', 'ISHIKAWA'"));
    }
    i += 1;
    if token(i).t == T::Nl {
        i += 1;
    }

    loop {
        let t = token(i).t;
        if t == T::Text || (t == T::Spacelist && token(i + 1).t == T::Text) {
            let text = if t == T::Text { i } else { i + 1 };
            i = text + 1;
            if !ends(token(i).t) {
                return Err(fail(i, STOP));
            }
            let level = if t == T::Text {
                0
            } else {
                let indent = token(text - 1);
                src.get(indent.start..indent.end)
                    .map_or(0, |run| run.encode_utf16().count())
            };
            let body = token(text);
            db.add_node(
                level,
                trim(src.get(body.start..body.end).unwrap_or("")).to_owned(),
            );
        } else if t == T::Spaceline || t == T::Spacelist {
            i += 1;
            if !ends(token(i).t) {
                return Err(if t == T::Spacelist {
                    fail(i, "'SPACELINE', 'NL', 'EOF', 'TEXT'")
                } else {
                    fail(i, STOP)
                });
            }
        } else {
            return Err(fail(i, "'SPACELINE', 'SPACELIST', 'TEXT'"));
        }
        i += 1;
        while matches!(token(i).t, T::Nl | T::Eof) {
            i += 1;
        }
        if token(i).t == T::End {
            return Ok(());
        }
    }
}
