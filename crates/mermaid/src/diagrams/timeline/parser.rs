//! The timeline parser: a port of Mermaid's timeline grammar. Port of Pele's
//! `src/diagrams/timeline/parser.ts`.

use super::db::TimelineDb;
use super::lexer::{
    ACC_DESCR, ACC_DESCR_MULTILINE_VALUE, ACC_TITLE, END, EOF, EVENT, NEWLINE, PERIOD, SECTION,
    TIMELINE, TIMELINE_LR, TIMELINE_TD, TITLE, TOKEN_NAMES, tokenize,
};
use crate::error::Error;
use crate::text::label::trim;

const STATEMENT: &str = "'EOF', 'SPACE', 'NEWLINE', 'title', 'acc_title', 'acc_descr', 'acc_descr_multiline_value', 'section', 'period', 'event'";

/// `text` without the ASCII `keyword` bytes at its start and the one whitespace character after
/// them, as Pele's `substring` leaves it.
fn after(text: &str, keyword: usize) -> &str {
    let space = text
        .get(keyword..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8);
    text.get(keyword + space..).unwrap_or("")
}

/// Parses preprocessed timeline source into `db`, making the calls the grammar's actions make.
///
/// # Errors
///
/// Returns the syntax error the generated parser reports, and a semantic error for an event
/// before any period.
pub(crate) fn parse_timeline(src: &str, db: &mut TimelineDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let kind = |i: usize| tokens.types.get(i).copied().unwrap_or(END);
    let text = |i: usize| tokens.texts.get(i).copied().unwrap_or("");
    let fail = |i: usize, expected: &str| -> Error {
        let got = match kind(i) {
            END => "1",
            other => TOKEN_NAMES.get(usize::from(other)).copied().unwrap_or("1"),
        };
        let start = tokens.starts.get(i).copied().unwrap_or(src.len());
        let detail = ["Expecting ", expected, ", got '", got, "'"].concat();
        Error::syntax("timeline", src, start, &detail, false)
    };

    let head = kind(0);
    if !matches!(head, TIMELINE | TIMELINE_LR | TIMELINE_TD) {
        return Err(fail(0, "'timeline', 'timeline_lr', 'timeline_td'"));
    }
    if head != TIMELINE {
        db.set_direction(if head == TIMELINE_LR { "LR" } else { "TD" });
    }

    let mut i = 1;
    loop {
        match kind(i) {
            NEWLINE => {}
            TITLE => db.set_diagram_title(after(text(i), 5)),
            ACC_TITLE => {
                i += 1;
                db.set_acc_title(trim(text(i)));
            }
            ACC_DESCR => {
                i += 1;
                db.set_acc_description(trim(text(i)));
            }
            ACC_DESCR_MULTILINE_VALUE => db.set_acc_description(trim(text(i))),
            SECTION => db.add_section(after(text(i), 7)),
            PERIOD => db.add_task(text(i), ""),
            EVENT => db.add_event(after(text(i), 1))?,
            EOF => return Ok(()),
            _ => return Err(fail(i, STATEMENT)),
        }
        i += 1;
    }
}
