//! The parts of dayjs 1.11 that Mermaid's gantt code relies on, with the plugins it loads (custom
//! parse format, advanced format, ISO week) and the English locale: strict parsing against a
//! format, formatting, and calendar arithmetic, all in local time (UTC here). Quirks are kept,
//! because a date is accepted only when formatting the parsed result gives back the input. Port of
//! Pele's `src/diagrams/gantt/dates.ts`.

use super::js_date::{self, DAY, Date};
use crate::util::chars::{is_digit, is_space};
use crate::yaml::push_number;

/// English month names.
pub(crate) const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// English weekday names, from Sunday.
pub(crate) const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// A milliseconds-per-week constant.
const WEEK: f64 = 7.0 * DAY;

/// A duration unit, as dayjs names it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Unit {
    /// `d`.
    Day,
    /// `h`.
    Hour,
    /// `ms`.
    Millisecond,
    /// `m`.
    Minute,
    /// `M`.
    Month,
    /// `s`.
    Second,
    /// `w`.
    Week,
    /// `y`.
    Year,
}

/// Appends a number as `String(value)` writes it.
pub(crate) fn push_int(out: &mut String, value: f64) {
    push_number(out, value);
}

/// `String(value)`.
pub(crate) fn int_string(value: f64) -> String {
    let mut out = String::new();
    push_number(&mut out, value);
    out
}

/// `String(value)` padded with leading zeros to `length`, as dayjs pads, sign included.
fn pad(value: f64, length: usize) -> String {
    let s = int_string(value);
    let count = s.encode_utf16().count();
    if count >= length {
        return s;
    }
    let mut out = "0".repeat(length - count);
    out.push_str(&s);
    out
}

/// `n` with its English ordinal suffix, such as `1st`.
fn ordinal(n: f64) -> String {
    /// Ordinal suffixes, indexed as dayjs indexes them.
    const SUFFIX: [&str; 4] = ["th", "st", "nd", "rd"];
    let v = n % 100.0;
    let pick = |index: f64| -> Option<&str> {
        if (0.0..4.0).contains(&index) && index.fract() == 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0 to 3"
            )]
            SUFFIX.get(index as usize).copied()
        } else {
            None
        }
    };
    let suffix = pick((v - 20.0) % 10.0).or_else(|| pick(v)).unwrap_or("th");
    let mut out = int_string(n);
    out.push_str(suffix);
    out
}

/// `+00:00`: the zone of every date, since local time is UTC.
fn zone() -> &'static str {
    "+00:00"
}

/// `isValid`.
pub(crate) fn is_valid(date: Date) -> bool {
    date.is_valid()
}

/// The ISO weekday: 1 for Monday to 7 for Sunday.
pub(crate) fn iso_weekday(date: Date) -> f64 {
    let day = date.weekday();
    if day == 0.0 || day.is_nan() { 7.0 } else { day }
}

/// A copy of `date` moved by `days` calendar days.
pub(crate) fn add_days(date: Date, days: f64) -> Date {
    let mut next = date;
    next.set_date(next.date() + days);
    next
}

/// The number of days in the month of `date`.
fn days_in_month(date: Date) -> f64 {
    let mut last = Date::local(date.year(), date.month() + 1.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    last.set_hours(23.0, 59.0, 59.0, 999.0);
    last.date()
}

/// Sets the month or the year, keeping the day of the month unless the new month is shorter.
fn set_calendar(date: Date, apply: impl Fn(&mut Date)) -> Date {
    let mut next = date;
    next.set_date(1.0);
    apply(&mut next);
    next.set_date(js_min(date.date(), days_in_month(next)));
    next
}

/// `date` plus `amount` of `unit`, as dayjs adds.
pub(crate) fn add(date: Date, amount: f64, unit: Unit) -> Date {
    let time = date.time();
    match unit {
        Unit::Month => set_calendar(date, |d| d.set_month(date.month() + amount, None)),
        Unit::Year => set_calendar(date, |d| d.set_full_year(date.year() + amount)),
        Unit::Day => add_days(date, js_round(amount)),
        Unit::Week => add_days(date, js_round(7.0 * amount)),
        Unit::Hour => Date(js_date::time_clip(time + amount * 3_600_000.0)),
        Unit::Minute => Date(js_date::time_clip(time + amount * 60_000.0)),
        Unit::Second => Date(js_date::time_clip(time + amount * 1000.0)),
        Unit::Millisecond => Date(js_date::time_clip(time + amount)),
    }
}

/// `Math.min` of two numbers: NaN when either is.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.max` of two numbers: NaN when either is.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Math.round`, with NaN kept.
fn js_round(value: f64) -> f64 {
    if value.is_nan() {
        return value;
    }
    crate::svg::builder::js_round(value)
}

/// The Thursday of the ISO week of `date`.
fn week_thursday(date: Date) -> Date {
    add_days(date, 4.0 - iso_weekday(date))
}

/// The ISO week number of `date`.
fn iso_week(date: Date) -> f64 {
    let thursday = week_thursday(date);
    let jan1 = Date::local(thursday.year(), 0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
    let weekday = iso_weekday(jan1);
    let first = add_days(jan1, 4.0 - weekday + if weekday > 4.0 { 7.0 } else { 0.0 });
    let weeks = (thursday.time() - first.time()) / WEEK;
    let whole = if weeks < 0.0 {
        weeks.ceil() + 0.0
    } else {
        weeks.floor()
    };
    whole + 1.0
}

/// Code units of `text` from `at` that equal `word`.
fn starts(units: &[u16], at: usize, word: &str) -> bool {
    word.bytes()
        .enumerate()
        .all(|(i, b)| units.get(at + i) == Some(&u16::from(b)))
}

/// A `[...]` literal at `at`, as `\[([^\]]+)]` matches it: its end, after the `]`.
fn bracket(units: &[u16], at: usize) -> Option<usize> {
    if units.get(at) != Some(&u16::from(b'[')) {
        return None;
    }
    let close = units
        .get(at + 1..)?
        .iter()
        .position(|&u| u == u16::from(b']'))?;
    (close > 0).then_some(at + 1 + close + 1)
}

/// The longest of `words` at `at`, in the order given: the first that matches wins.
fn first_of<'w>(units: &[u16], at: usize, words: &[&'w str]) -> Option<&'w str> {
    words.iter().copied().find(|word| starts(units, at, word))
}

/// The advanced-format tokens, in `RE_ADVANCED` alternation order after the bracket literal.
const ADVANCED: [&str; 16] = [
    "Q", "wo", "ww", "w", "WW", "W", "zzz", "z", "gggg", "GGGG", "Do", "X", "x", "kk", "k", "S",
];

/// The format tokens, in `RE_FORMAT` alternation order after the bracket literal, with each
/// `{1,n}` repetition written longest first.
const FORMAT: [&str; 23] = [
    "YYYY", "YY", "MMMM", "MMM", "MM", "M", "DD", "D", "dddd", "ddd", "dd", "d", "HH", "H", "hh",
    "h", "a", "A", "mm", "m", "ss", "s", "ZZ",
];

/// `date` formatted with a dayjs `template`. `None` for the week-of-year and time zone name
/// tokens, which need dayjs plugins Mermaid does not load and so fail there.
pub(crate) fn format(date: Date, template: &str) -> Option<String> {
    if !date.is_valid() {
        return Some("Invalid Date".to_owned());
    }
    let template = if template.is_empty() {
        "YYYY-MM-DDTHH:mm:ssZ"
    } else {
        template
    };
    if template == "YYYY-MM-DD" {
        return Some(iso_date(date));
    }

    let units: Vec<u16> = template.encode_utf16().collect();
    let mut advanced: Vec<u16> = Vec::with_capacity(units.len());
    let mut at = 0;
    while at < units.len() {
        if let Some(end) = bracket(&units, at) {
            advanced.extend_from_slice(units.get(at..end).unwrap_or(&[]));
            at = end;
            continue;
        }
        let Some(token) = first_of(&units, at, &ADVANCED) else {
            advanced.extend(units.get(at).copied());
            at += 1;
            continue;
        };
        at += token.len();
        let replaced = match token {
            "Q" => int_string(((date.month() + 1.0) / 3.0).ceil()),
            "Do" => {
                let mut out = String::from("[");
                out.push_str(&ordinal(date.date()));
                out.push(']');
                out
            }
            "GGGG" => int_string(week_thursday(date).year()),
            "W" => int_string(iso_week(date)),
            "WW" => pad(iso_week(date), 2),
            "k" => int_string(if date.hours() == 0.0 {
                24.0
            } else {
                date.hours()
            }),
            "kk" => pad(
                if date.hours() == 0.0 {
                    24.0
                } else {
                    date.hours()
                },
                2,
            ),
            "X" => int_string((date.time() / 1000.0).floor()),
            "x" => int_string(date.time()),
            "gggg" | "wo" | "w" | "ww" | "z" | "zzz" => return None,
            other => other.to_owned(),
        };
        advanced.extend(replaced.encode_utf16());
    }

    let hours = date.hours();
    let mut out: Vec<u16> = Vec::with_capacity(advanced.len() + 16);
    let mut at = 0;
    while at < advanced.len() {
        if let Some(end) = bracket(&advanced, at) {
            out.extend_from_slice(advanced.get(at + 1..end - 1).unwrap_or(&[]));
            at = end;
            continue;
        }
        let token = if starts(&advanced, at, "SSS") {
            Some("SSS")
        } else {
            first_of(&advanced, at, &FORMAT).or_else(|| starts(&advanced, at, "Z").then_some("Z"))
        };
        let Some(token) = token else {
            out.extend(advanced.get(at).copied());
            at += 1;
            continue;
        };
        at += token.len();
        let text: String = match token {
            "YY" => {
                let year = int_string(date.year());
                let units: Vec<u16> = year.encode_utf16().collect();
                String::from_utf16_lossy(units.get(units.len().saturating_sub(2)..).unwrap_or(&[]))
            }
            "YYYY" => pad(date.year(), 4),
            "M" => int_string(date.month() + 1.0),
            "MM" => pad(date.month() + 1.0, 2),
            "MMM" => month_name(date).get(..3).unwrap_or("").to_owned(),
            "MMMM" => month_name(date).to_owned(),
            "D" => int_string(date.date()),
            "DD" => pad(date.date(), 2),
            "d" => int_string(date.weekday()),
            "dd" => weekday_name(date).get(..2).unwrap_or("").to_owned(),
            "ddd" => weekday_name(date).get(..3).unwrap_or("").to_owned(),
            "dddd" => weekday_name(date).to_owned(),
            "H" => int_string(hours),
            "HH" => pad(hours, 2),
            "h" => int_string(twelve(hours)),
            "hh" => pad(twelve(hours), 2),
            "a" => (if hours < 12.0 { "am" } else { "pm" }).to_owned(),
            "A" => (if hours < 12.0 { "AM" } else { "PM" }).to_owned(),
            "m" => int_string(date.minutes()),
            "mm" => pad(date.minutes(), 2),
            "s" => int_string(date.seconds()),
            "ss" => pad(date.seconds(), 2),
            "SSS" => pad(date.ms(), 3),
            "Z" => zone().to_owned(),
            _ => "+0000".to_owned(),
        };
        out.extend(text.encode_utf16());
    }
    Some(String::from_utf16_lossy(&out))
}

/// The hour on a twelve-hour clock.
fn twelve(hours: f64) -> f64 {
    let h = hours % 12.0;
    if h == 0.0 { 12.0 } else { h }
}

/// The English name of the month of `date`.
fn month_name(date: Date) -> &'static str {
    index(date.month())
        .and_then(|m| MONTHS.get(m))
        .copied()
        .unwrap_or("")
}

/// The English name of the weekday of `date`.
fn weekday_name(date: Date) -> &'static str {
    index(date.weekday())
        .and_then(|d| WEEKDAYS.get(d))
        .copied()
        .unwrap_or("")
}

/// A small non-negative whole number as an index.
pub(crate) fn index(value: f64) -> Option<usize> {
    if (0.0..1e6).contains(&value) && value.fract() == 0.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "bounded"
        )]
        Some(value as usize)
    } else {
        None
    }
}

/// `date` as `YYYY-MM-DD`, as dayjs writes that format.
pub(crate) fn iso_date(date: Date) -> String {
    let year = date.year();
    let month = date.month() + 1.0;
    let day = date.date();
    let mut out = if (1000.0..=9999.0).contains(&year) {
        int_string(year)
    } else {
        pad(year, 4)
    };
    out.push_str(if month < 10.0 { "-0" } else { "-" });
    push_int(&mut out, month);
    out.push_str(if day < 10.0 { "-0" } else { "-" });
    push_int(&mut out, day);
    out
}

/// The fields a format reads.
#[derive(Default)]
struct Fields {
    /// Whether `A` or `a` read `PM`.
    afternoon: Option<bool>,
    /// The day of the month.
    day: Option<f64>,
    /// Whether `Do` gave a day, even 0.
    day_given: bool,
    /// Hours.
    hours: Option<f64>,
    /// Milliseconds.
    milliseconds: Option<f64>,
    /// Minutes.
    minutes: Option<f64>,
    /// The month, from 1.
    month: Option<f64>,
    /// The UTC offset in minutes, negated.
    offset: Option<f64>,
    /// Seconds.
    seconds: Option<f64>,
    /// A week number, which dayjs cannot apply.
    week: Option<f64>,
    /// The year.
    year: Option<f64>,
}

/// What text a reader takes, as dayjs's `match*` expressions find it.
#[derive(Clone, Copy)]
enum Pattern {
    /// `\d`.
    One,
    /// `\d\d?`.
    OneToTwo,
    /// `[+-]\d\d:?(\d\d)?|Z`.
    Offset,
    /// `[+-]?\d+`.
    Signed,
    /// `\d\d`, `\d{3}`, or `\d{4}`.
    Exact(usize),
    /// `\d*[^-_:/,()\s\d]+`.
    Word,
}

/// A step of a compiled format: literal text to skip, or a token to read.
enum Step {
    /// Literal text of the given length in code units.
    Literal(usize),
    /// A token and what its reader matches.
    Read(&'static str, Pattern),
}

/// The reader for a format token, as dayjs's parse table has it.
fn reader(token: &str) -> Option<(&'static str, Pattern)> {
    Some(match token {
        "A" => ("A", Pattern::Word),
        "a" => ("a", Pattern::Word),
        "Q" => ("Q", Pattern::One),
        "S" => ("S", Pattern::One),
        "SS" => ("SS", Pattern::Exact(2)),
        "SSS" => ("SSS", Pattern::Exact(3)),
        "s" => ("s", Pattern::OneToTwo),
        "ss" => ("ss", Pattern::OneToTwo),
        "m" => ("m", Pattern::OneToTwo),
        "mm" => ("mm", Pattern::OneToTwo),
        "H" => ("H", Pattern::OneToTwo),
        "h" => ("h", Pattern::OneToTwo),
        "HH" => ("HH", Pattern::OneToTwo),
        "hh" => ("hh", Pattern::OneToTwo),
        "D" => ("D", Pattern::OneToTwo),
        "DD" => ("DD", Pattern::Exact(2)),
        "Do" => ("Do", Pattern::Word),
        "w" => ("w", Pattern::OneToTwo),
        "ww" => ("ww", Pattern::Exact(2)),
        "M" => ("M", Pattern::OneToTwo),
        "MM" => ("MM", Pattern::Exact(2)),
        "MMM" => ("MMM", Pattern::Word),
        "MMMM" => ("MMMM", Pattern::Word),
        "Y" => ("Y", Pattern::Signed),
        "YY" => ("YY", Pattern::Exact(2)),
        "YYYY" => ("YYYY", Pattern::Exact(4)),
        "Z" => ("Z", Pattern::Offset),
        "ZZ" => ("ZZ", Pattern::Offset),
        _ => return None,
    })
}

/// Whether a code unit exists and passes `f`.
fn unit_is(unit: Option<&u16>, f: impl Fn(u32) -> bool) -> bool {
    unit.is_some_and(|&u| f(u32::from(u)))
}

/// A separator of `RE_TOKENS`: `[-_:/.,()\s]`.
fn is_separator(c: u32) -> bool {
    matches!(c, 45 | 95 | 58 | 47 | 46 | 44 | 40 | 41) || is_space(c)
}

/// The tokens of a format, as `template.match(RE_TOKENS)` lists them.
fn tokens(units: &[u16]) -> Vec<&[u16]> {
    /// The format tokens of `RE_TOKENS`, in alternation order.
    const WORDS: [&str; 26] = [
        "A", "a", "Q", "YYYY", "YY", "Y", "ww", "w", "MMMM", "MMM", "MM", "M", "Do", "DD", "D",
        "hh", "h", "HH", "H", "mm", "m", "ss", "s", "SSS", "SS", "S",
    ];
    let mut out = Vec::new();
    let mut at = 0;
    while at < units.len() {
        // `\[[^[]*\]`: up to the last `]` before the next `[`.
        if units.get(at) == Some(&u16::from(b'[')) {
            let rest = units.get(at + 1..).unwrap_or(&[]);
            let span = rest
                .iter()
                .position(|&u| u == u16::from(b'['))
                .unwrap_or(rest.len());
            if let Some(close) = rest
                .get(..span)
                .and_then(|r| r.iter().rposition(|&u| u == u16::from(b']')))
            {
                let end = at + 1 + close + 1;
                out.push(units.get(at..end).unwrap_or(&[]));
                at = end;
                continue;
            }
        }
        if unit_is(units.get(at), is_separator) {
            let mut end = at + 1;
            while unit_is(units.get(end), is_separator) {
                end += 1;
            }
            out.push(units.get(at..end).unwrap_or(&[]));
            at = end;
            continue;
        }
        let word = first_of(units, at, &WORDS).or_else(|| {
            if starts(units, at, "z") {
                Some("z")
            } else if starts(units, at, "ZZ") {
                Some("ZZ")
            } else if starts(units, at, "Z") {
                Some("Z")
            } else {
                None
            }
        });
        if let Some(word) = word {
            out.push(units.get(at..at + word.len()).unwrap_or(&[]));
            at += word.len();
        } else {
            at += 1;
        }
    }
    out
}

/// Compiles a format into steps, or `None` when dayjs cannot parse with it.
fn compile(template: &str) -> Option<Vec<Step>> {
    let units: Vec<u16> = template.encode_utf16().collect();

    // dayjs expands localized tokens through a locale table the English locale does not have.
    let mut at = 0;
    while at < units.len() {
        if let Some(end) = bracket(&units, at) {
            at = end;
            continue;
        }
        if unit_is(units.get(at), |c| c == 76 || c == 108) {
            return None;
        }
        at += 1;
    }

    let tokens = tokens(&units);
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .into_iter()
            .map(|token| {
                let text = String::from_utf16_lossy(token);
                if let Some((name, pattern)) = reader(&text) {
                    return Step::Read(name, pattern);
                }
                // `token.replace(/^\[|\]$/g, '')`.
                let mut length = token.len();
                if token.first() == Some(&u16::from(b'[')) {
                    length -= 1;
                }
                if token.len() > usize::from(token.first() == Some(&u16::from(b'[')))
                    && token.last() == Some(&u16::from(b']'))
                {
                    length -= 1;
                }
                Step::Literal(length)
            })
            .collect(),
    )
}

/// Where `pattern` first matches in `text`, as an unanchored regular expression search.
fn find(text: &[u16], pattern: Pattern) -> Option<(usize, usize)> {
    let digit = |at: usize| unit_is(text.get(at), is_digit);
    let digits_from = |mut at: usize| {
        while digit(at) {
            at += 1;
        }
        at
    };
    for start in 0..text.len() {
        let end = match pattern {
            Pattern::One => digit(start).then_some(start + 1),
            Pattern::OneToTwo => digit(start).then(|| {
                if digit(start + 1) {
                    start + 2
                } else {
                    start + 1
                }
            }),
            Pattern::Exact(n) => (0..n).all(|i| digit(start + i)).then_some(start + n),
            Pattern::Signed => {
                let signed = unit_is(text.get(start), |c| c == 43 || c == 45) && digit(start + 1);
                if signed {
                    Some(digits_from(start + 1))
                } else {
                    digit(start).then(|| digits_from(start))
                }
            }
            Pattern::Offset => {
                if unit_is(text.get(start), |c| c == 43 || c == 45)
                    && digit(start + 1)
                    && digit(start + 2)
                {
                    let mut end = start + 3;
                    if text.get(end) == Some(&u16::from(b':')) {
                        end += 1;
                    }
                    if digit(end) && digit(end + 1) {
                        end += 2;
                    }
                    Some(end)
                } else {
                    (text.get(start) == Some(&u16::from(b'Z'))).then_some(start + 1)
                }
            }
            Pattern::Word => {
                let word = |at: usize| {
                    unit_is(text.get(at), |c| !is_separator(c) || c == 46)
                        && !unit_is(text.get(at), is_digit)
                };
                let after = digits_from(start);
                word(after).then(|| {
                    let mut end = after;
                    while word(end) {
                        end += 1;
                    }
                    end
                })
            }
        };
        if let Some(end) = end {
            return Some((start, end));
        }
    }
    None
}

/// `+text` for the digit strings the readers see.
fn to_number(text: &[u16]) -> f64 {
    String::from_utf16_lossy(text).parse().unwrap_or(f64::NAN)
}

/// Reads one matched token into `fields`. `false` when dayjs's reader rejects it.
fn read(fields: &mut Fields, name: &str, input: &[u16]) -> bool {
    let number = || to_number(input);
    let text = || String::from_utf16_lossy(input);
    match name {
        "A" => fields.afternoon = Some(text() == "PM"),
        "a" => fields.afternoon = Some(text() == "pm"),
        "Q" => fields.month = Some((number() - 1.0) * 3.0 + 1.0),
        "S" => fields.milliseconds = Some(number() * 100.0),
        "SS" => fields.milliseconds = Some(number() * 10.0),
        "SSS" => fields.milliseconds = Some(number()),
        "s" | "ss" => fields.seconds = Some(number()),
        "m" | "mm" => fields.minutes = Some(number()),
        "H" | "h" | "HH" | "hh" => fields.hours = Some(number()),
        "D" | "DD" => fields.day = Some(number()),
        "Do" => {
            let Some(start) = input.iter().position(|&u| is_digit(u32::from(u))) else {
                return false;
            };
            let rest = input.get(start..).unwrap_or(&[]);
            let end = rest
                .iter()
                .position(|&u| !is_digit(u32::from(u)))
                .unwrap_or(rest.len());
            // dayjs keeps the digits as a string here, so even "0" counts as a day being given.
            fields.day = Some(to_number(rest.get(..end).unwrap_or(&[])));
            fields.day_given = true;
            let text = text();
            for i in 1..=31 {
                if ordinal(f64::from(i)) == text {
                    fields.day = Some(f64::from(i));
                }
            }
        }
        "w" | "ww" => fields.week = Some(number()),
        "M" | "MM" => fields.month = Some(number()),
        "MMM" | "MMMM" => {
            let text = text();
            let found = MONTHS.iter().position(|month| {
                if name == "MMM" {
                    month.get(..3) == Some(text.as_str())
                } else {
                    *month == text
                }
            });
            let Some(found) = found else {
                return false;
            };
            #[expect(clippy::cast_precision_loss, reason = "a month index")]
            let month = (found + 1) as f64;
            fields.month = Some(month);
        }
        "Y" | "YYYY" => fields.year = Some(number()),
        "YY" => {
            let n = number();
            fields.year = Some(n + if n > 68.0 { 1900.0 } else { 2000.0 });
        }
        "Z" | "ZZ" => {
            if input == [u16::from(b'Z')] {
                fields.offset = Some(0.0);
                return true;
            }
            // `input.match(/([+-]|\d\d)/g)`: the sign, then pairs of digits.
            let mut pairs = Vec::new();
            let mut at = 1;
            while at + 1 < input.len() + 1 {
                if unit_is(input.get(at), is_digit) && unit_is(input.get(at + 1), is_digit) {
                    pairs.push(to_number(input.get(at..at + 2).unwrap_or(&[])));
                    at += 2;
                } else {
                    at += 1;
                }
            }
            let hours = pairs.first().copied().unwrap_or(f64::NAN);
            let minutes = pairs.get(1).copied().unwrap_or(0.0);
            let total = hours * 60.0 + minutes;
            fields.offset = Some(if total == 0.0 {
                0.0
            } else if input.first() == Some(&u16::from(b'+')) {
                -total
            } else {
                total
            });
        }
        _ => {}
    }
    true
}

/// A number that is set and not zero or NaN, as JavaScript's truthiness reads it.
fn truthy(value: Option<f64>) -> Option<f64> {
    value.filter(|&v| v != 0.0 && !v.is_nan())
}

/// Parses `input` with `template` without checking that it formats back, as dayjs's custom parse format does.
fn loose(input: &str, template: &str, now: Date) -> Option<Date> {
    if template == "x" || template == "X" {
        // `Number(input)`. Only a plain integer can format back to the input, so any other
        // text gives an invalid date here.
        let plain = {
            let digits = input.strip_prefix('-').unwrap_or(input);
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        };
        let n = if plain {
            input.parse::<f64>().unwrap_or(f64::NAN)
        } else {
            f64::NAN
        };
        let scale = if template == "X" { 1000.0 } else { 1.0 };
        return Some(Date(js_date::time_clip(scale * n)));
    }

    let steps = compile(template)?;
    let mut fields = Fields::default();
    let mut rest: Vec<u16> = input.encode_utf16().collect();
    let mut start = 0;
    for step in steps {
        let (name, pattern) = match step {
            Step::Literal(length) => {
                start += length;
                continue;
            }
            Step::Read(name, pattern) => (name, pattern),
        };
        let slice = rest.get(start..).unwrap_or(&[]);
        let (from, to) = find(slice, pattern)?;
        let matched: Vec<u16> = slice.get(from..to).unwrap_or(&[]).to_vec();
        if !read(&mut fields, name, &matched) {
            return None;
        }
        // dayjs drops the first occurrence of the matched text, wherever it is, and does not
        // move `start`.
        if let Some(at) = (!matched.is_empty())
            .then(|| {
                rest.windows(matched.len())
                    .position(|w| w == matched.as_slice())
            })
            .flatten()
        {
            rest.drain(at..at + matched.len());
        }
    }

    if let (Some(afternoon), Some(hours)) = (fields.afternoon, fields.hours) {
        if afternoon {
            if hours < 12.0 {
                fields.hours = Some(hours + 12.0);
            }
        } else if index(hours) == Some(12) {
            fields.hours = Some(0.0);
        }
    }

    let day = if fields.day_given || truthy(fields.day).is_some() {
        fields.day.unwrap_or(f64::NAN)
    } else if truthy(fields.year).is_none() && truthy(fields.month).is_none() {
        now.date()
    } else {
        1.0
    };
    let year = truthy(fields.year).unwrap_or_else(|| now.year());
    let mut month = 0.0;
    if !(truthy(fields.year).is_some() && truthy(fields.month).is_none()) {
        month = match fields.month {
            Some(m) if m > 0.0 => m - 1.0,
            _ => now.month(),
        };
    }
    let hours = truthy(fields.hours).unwrap_or(0.0);
    let minutes = truthy(fields.minutes).unwrap_or(0.0);
    let seconds = truthy(fields.seconds).unwrap_or(0.0);
    let ms = truthy(fields.milliseconds).unwrap_or(0.0);
    if let Some(offset) = fields.offset {
        return Some(Date(js_date::time_clip(js_date::utc(
            year,
            month,
            day,
            hours,
            minutes,
            seconds,
            ms + offset * 60_000.0,
        ))));
    }
    // Applying a week number needs a dayjs plugin Mermaid does not load.
    if truthy(fields.week).is_some() {
        return None;
    }
    Some(Date::local(year, month, day, hours, minutes, seconds, ms))
}

/// Parses the way `dayjs(input, format, true)` does. `now` supplies the fields the format
/// leaves out.
pub(crate) fn parse_strict(input: &str, template: &str, now: Date) -> Option<Date> {
    let date = loose(input, template, now)?;
    (date.is_valid() && format(date, template).as_deref() == Some(input)).then_some(date)
}
