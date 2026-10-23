//! Parses gantt tokens into a [`GanttDb`], with Mermaid's error messages. Port of Pele's
//! `src/diagrams/gantt/parser.ts`.

use super::db::GanttDb;
use super::lexer::{Token, tokenize};
use super::tokens::{T, WEEKDAYS};
use crate::error::Error;
use crate::text::label::trim;

/// The weekday names, from Monday.
const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// The tokens a line can start with, as parse errors list them.
const LINE: &str = "'EOF', 'SPACE', 'NL', 'weekday_monday', 'weekday_tuesday', 'weekday_wednesday', \
'weekday_thursday', 'weekday_friday', 'weekday_saturday', 'weekday_sunday', 'weekend_friday', \
'weekend_saturday', 'dateFormat', 'inclusiveEndDates', 'topAxis', 'axisFormat', 'tickInterval', \
'excludes', 'includes', 'todayMarker', 'title', 'acc_title', 'acc_descr', \
'acc_descr_multiline_value', 'section', 'taskTxt', 'click'";

/// Tokens that can start a line, which are also the only ones that can follow a statement.
fn starts_line(t: T) -> bool {
    WEEKDAYS.contains(&t)
        || matches!(
            t,
            T::WeekendFriday
                | T::WeekendSaturday
                | T::Eof
                | T::Nl
                | T::DateFormat
                | T::InclusiveEndDates
                | T::TopAxis
                | T::AxisFormat
                | T::TickInterval
                | T::Excludes
                | T::Includes
                | T::TodayMarker
                | T::Title
                | T::AccTitle
                | T::AccDescr
                | T::AccDescrMultilineValue
                | T::Section
                | T::TaskTxt
                | T::Click
        )
}

/// `text` from its `units`-th UTF-16 unit, as `text.slice(units)` reads it.
fn slice_units(text: &str, units: usize) -> &str {
    let mut count = 0;
    for (at, c) in text.char_indices() {
        if count >= units {
            return text.get(at..).unwrap_or("");
        }
        count += c.len_utf16();
    }
    ""
}

/// The token list and the reading position.
struct Parser<'s, 't> {
    /// The current token.
    i: usize,
    /// The source.
    src: &'s str,
    /// The tokens.
    tokens: &'t [Token<'s>],
}

impl<'s> Parser<'s, '_> {
    /// The kind of the current token.
    fn kind(&self) -> T {
        self.tokens.get(self.i).map_or(T::End, |token| token.r#type)
    }

    /// The text of the current token.
    fn text(&self) -> &'s str {
        self.tokens.get(self.i).map_or("", |token| token.text)
    }

    #[cold]
    /// A parse error at the current token, listing what was `expected`.
    fn fail(&self, expected: &str) -> Error {
        let token = self.tokens.get(self.i);
        let got = match token.map(|token| token.r#type) {
            None | Some(T::End) => "1",
            Some(t) => t.name(),
        };
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(got);
        detail.push('\'');
        let offset = token.map_or(self.src.len(), |token| token.start);
        Error::syntax("gantt", self.src, offset, &detail, false)
    }

    /// Mermaid's parser runs a statement's action only after seeing that the next token can
    /// follow it.
    fn end(&self, also: &str) -> Result<(), Error> {
        if starts_line(self.kind()) {
            return Ok(());
        }
        let mut expected = String::from(LINE);
        expected.push_str(also);
        Err(self.fail(&expected))
    }
}

/// Parses gantt `src` into `db`.
///
/// # Errors
///
/// Returns a syntax error for input Mermaid rejects, and a semantic error for a task with no
/// end or too many fields.
pub(crate) fn parse_gantt(src: &str, db: &mut GanttDb) -> Result<(), Error> {
    let tokens = tokenize(src);
    let mut p = Parser {
        i: 0,
        src,
        tokens: &tokens,
    };

    if p.kind() != T::Gantt {
        return Err(p.fail("'gantt'"));
    }
    p.i += 1;

    loop {
        let t = p.kind();
        let text = p.text();
        if !starts_line(t) {
            return Err(p.fail(LINE));
        }
        p.i += 1;
        match t {
            T::Eof => {
                if p.kind() == T::End {
                    return Ok(());
                }
                p.end("")?;
            }
            T::Nl => {}
            T::DateFormat => {
                p.end("")?;
                db.set_date_format(slice_units(text, 11));
            }
            T::InclusiveEndDates => {
                p.end("")?;
                db.inclusive_end_dates = true;
            }
            T::TopAxis => {
                p.end("")?;
                db.top_axis = true;
            }
            T::AxisFormat => {
                p.end("")?;
                db.axis_format = slice_units(text, 11).to_owned();
            }
            T::TickInterval => {
                p.end("")?;
                db.tick_interval = Some(slice_units(text, 13).to_owned());
            }
            T::Excludes => {
                p.end("")?;
                db.set_excludes(slice_units(text, 9));
            }
            T::Includes => {
                p.end("")?;
                db.set_includes(slice_units(text, 9));
            }
            T::TodayMarker => {
                p.end("")?;
                db.today_marker = slice_units(text, 12).to_owned();
            }
            T::WeekendFriday => {
                p.end("")?;
                "friday".clone_into(&mut db.weekend);
            }
            T::WeekendSaturday => {
                p.end("")?;
                "saturday".clone_into(&mut db.weekend);
            }
            T::Title => {
                p.end("")?;
                db.title = Some(slice_units(text, 6).to_owned());
            }
            T::AccTitle => {
                if p.kind() != T::AccTitleValue {
                    return Err(p.fail("'acc_title_value'"));
                }
                let value = p.text();
                p.i += 1;
                p.end("")?;
                db.set_acc_title(trim(value));
            }
            T::AccDescr => {
                if p.kind() != T::AccDescrValue {
                    return Err(p.fail("'acc_descr_value'"));
                }
                let value = p.text();
                p.i += 1;
                p.end("")?;
                db.set_acc_description(trim(value));
            }
            T::AccDescrMultilineValue => {
                p.end("")?;
                db.set_acc_description(trim(text));
            }
            T::Section => {
                p.end("")?;
                db.add_section(slice_units(text, 8));
            }
            T::TaskTxt => {
                if p.kind() != T::TaskData {
                    return Err(p.fail("'taskData'"));
                }
                let data = p.text();
                p.i += 1;
                p.end("")?;
                db.add_task(text, data)?;
            }
            T::Click => click(&mut p, db, text)?,
            _ => {
                p.end("")?;
                let index = WEEKDAYS.iter().position(|&day| day == t).unwrap_or(0);
                db.weekday = DAYS.get(index).map(|day| (*day).to_owned());
            }
        }
    }
}

/// Reads the rest of a `click` statement: a callback, a link, or both in either order.
fn click(p: &mut Parser<'_, '_>, db: &mut GanttDb, ids: &str) -> Result<(), Error> {
    let mut name = None;
    let mut href = None;
    if p.kind() == T::Callbackname {
        name = Some(p.text());
        p.i += 1;
        let mut args = false;
        if p.kind() == T::Callbackargs {
            args = true;
            p.i += 1;
        }
        if p.kind() == T::Href {
            href = Some(p.text());
            p.i += 1;
            p.end("")?;
        } else {
            p.end(if args {
                ", 'href'"
            } else {
                ", 'callbackargs', 'href'"
            })?;
        }
    } else if p.kind() == T::Href {
        href = Some(p.text());
        p.i += 1;
        if p.kind() == T::Callbackname {
            name = Some(p.text());
            p.i += 1;
            if p.kind() == T::Callbackargs {
                p.i += 1;
                p.end("")?;
            } else {
                p.end(", 'callbackargs'")?;
            }
        } else {
            p.end(", 'callbackname'")?;
        }
    } else {
        return Err(p.fail("'callbackname', 'href'"));
    }

    // Callbacks are recognized and never run. The task is only marked clickable, as in Mermaid.
    if name.is_some() {
        db.set_class(ids, "clickable");
    }
    if let Some(href) = href {
        db.set_link(ids, href);
    }
    Ok(())
}
