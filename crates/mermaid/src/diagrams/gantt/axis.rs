//! Time axis support, following d3-time and d3-time-format (English locale, local time), which
//! Mermaid uses: the `axisFormat` specifiers, tick intervals with d3's `every` rule, and its choice
//! of automatic ticks. Port of Pele's `src/diagrams/gantt/axis.ts`.

use super::dates::{MONTHS, WEEKDAYS, index, int_string, js_max, js_min};
use super::js_date::{DAY, Date, time_clip};

/// Milliseconds in a second.
const SECOND: f64 = 1000.0;
/// Milliseconds in a minute.
const MINUTE: f64 = 60.0 * SECOND;
/// Milliseconds in an hour.
const HOUR: f64 = 60.0 * MINUTE;
/// Milliseconds in a week.
const WEEK: f64 = 7.0 * DAY;
/// The approximate length of a month that d3 uses to choose ticks.
const MONTH: f64 = 30.0 * DAY;
/// The approximate length of a year that d3 uses to choose ticks.
const YEAR: f64 = 365.0 * DAY;

/// A calendar unit, as d3 names its intervals.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Unit {
    /// Days.
    Day,
    /// Hours.
    Hour,
    /// Milliseconds.
    Millisecond,
    /// Minutes.
    Minute,
    /// Months.
    Month,
    /// Seconds.
    Second,
    /// Weeks starting on the given weekday, 0 for Sunday.
    Week(f64),
    /// Years.
    Year,
}

impl Unit {
    /// Moves `d` back to the start of its unit.
    fn floor(self, d: &mut Date) {
        match self {
            Self::Millisecond => {}
            Self::Second => d.set_time(d.time() - d.ms()),
            Self::Minute => d.set_time(d.time() - d.ms() - d.seconds() * SECOND),
            Self::Hour => {
                d.set_time(d.time() - d.ms() - d.seconds() * SECOND - d.minutes() * MINUTE);
            }
            Self::Day => d.set_hours(0.0, 0.0, 0.0, 0.0),
            Self::Week(start) => {
                d.set_date(d.date() - (d.weekday() + 7.0 - start) % 7.0);
                d.set_hours(0.0, 0.0, 0.0, 0.0);
            }
            Self::Month => {
                d.set_date(1.0);
                d.set_hours(0.0, 0.0, 0.0, 0.0);
            }
            Self::Year => {
                d.set_month(0.0, Some(1.0));
                d.set_hours(0.0, 0.0, 0.0, 0.0);
            }
        }
    }

    /// Moves `d` forward by `step` units.
    fn offset(self, d: &mut Date, step: f64) {
        match self {
            Self::Millisecond => d.set_time(d.time() + step),
            Self::Second => d.set_time(d.time() + step * SECOND),
            Self::Minute => d.set_time(d.time() + step * MINUTE),
            Self::Hour => d.set_time(d.time() + step * HOUR),
            Self::Day => d.set_date(d.date() + step),
            Self::Week(_) => d.set_date(d.date() + step * 7.0),
            Self::Month => d.set_month(d.month() + step, None),
            Self::Year => d.set_full_year(d.year() + step),
        }
    }

    /// The value d3 tests against a step: a tick falls where it is a multiple of the step.
    fn field(self, d: Date) -> f64 {
        match self {
            Self::Second => d.seconds(),
            Self::Minute => d.minutes(),
            Self::Hour => d.hours(),
            Self::Day => d.date() - 1.0,
            Self::Month => d.month(),
            Self::Millisecond | Self::Week(_) | Self::Year => f64::NAN,
        }
    }

    /// How many values `field` cycles through, which bounds how far apart filtered ticks are.
    fn cycle(self) -> f64 {
        match self {
            Self::Second | Self::Minute => 60.0,
            Self::Hour => 24.0,
            Self::Day => 31.0,
            Self::Month => 12.0,
            Self::Millisecond | Self::Week(_) | Self::Year => f64::INFINITY,
        }
    }

    /// The approximate length of the unit in milliseconds.
    fn ms(self) -> f64 {
        match self {
            Self::Millisecond => 1.0,
            Self::Second => SECOND,
            Self::Minute => MINUTE,
            Self::Hour => HOUR,
            Self::Day => DAY,
            Self::Week(_) => WEEK,
            Self::Month => MONTH,
            Self::Year => YEAR,
        }
    }
}

/// The interval d3's `unit.every(step)` gives.
#[derive(Clone, Copy)]
enum Calendar {
    /// The unit itself.
    Base(Unit),
    /// Values of the unit's field that are multiples of the step.
    Filtered(Unit, f64),
    /// Multiples of the step since the epoch.
    Millis(f64),
    /// Every `step` weeks, counted from the week of the epoch.
    Weeks(Unit, f64, Date),
    /// Years that are multiples of the step.
    Years(f64),
}

impl Calendar {
    /// The unit the interval counts in.
    fn unit(self) -> Unit {
        match self {
            Self::Base(unit) | Self::Filtered(unit, _) | Self::Weeks(unit, _, _) => unit,
            Self::Millis(_) => Unit::Millisecond,
            Self::Years(_) => Unit::Year,
        }
    }

    /// Moves `d` back to the latest tick at or before it.
    fn floor(self, d: &mut Date) {
        match self {
            Self::Base(unit) => unit.floor(d),
            Self::Millis(step) => d.set_time((d.time() / step).floor() * step),
            Self::Years(step) => {
                d.set_full_year((d.year() / step).floor() * step);
                Unit::Year.floor(d);
            }
            Self::Weeks(unit, step, epoch) => {
                unit.floor(d);
                unit.offset(d, -modulo(span(epoch, *d, WEEK), step));
            }
            Self::Filtered(unit, step) => {
                // Date overflow makes the field NaN, which can never match the interval.
                unit.floor(d);
                while d.time().is_finite() && unit.field(*d) % step != 0.0 {
                    d.set_time(d.time() - 1.0);
                    unit.floor(d);
                }
            }
        }
    }

    /// Moves `d` forward by `n` ticks.
    fn offset(self, d: &mut Date, n: f64) {
        match self {
            Self::Base(unit) => unit.offset(d, n),
            Self::Millis(step) => d.set_time(d.time() + n * step),
            Self::Years(step) => d.set_full_year(d.year() + n * step),
            Self::Weeks(unit, step, _) => unit.offset(d, n * step),
            Self::Filtered(unit, step) => {
                let mut i = 0.0;
                while i < n && d.time().is_finite() {
                    loop {
                        unit.offset(d, 1.0);
                        if !(d.time().is_finite() && unit.field(*d) % step != 0.0) {
                            break;
                        }
                    }
                    i += 1.0;
                }
            }
        }
    }
}

/// Whole units between two floored dates. The UTC offset never changes in UTC.
fn span(start: Date, end: Date, unit: f64) -> f64 {
    ((end.time() - start.time()) / unit).floor()
}

/// The remainder of `a` by `b`, with the sign of `b`.
fn modulo(a: f64, b: f64) -> f64 {
    ((a % b) + b) % b
}

/// The interval d3's `unit.every(step)` gives.
fn every(unit: Unit, step: f64) -> Calendar {
    if step <= 1.0 {
        return Calendar::Base(unit);
    }
    match unit {
        Unit::Millisecond => Calendar::Millis(step),
        Unit::Year => Calendar::Years(step),
        Unit::Week(_) => {
            let mut epoch = Date(0.0);
            unit.floor(&mut epoch);
            Calendar::Weeks(unit, step, epoch)
        }
        _ => Calendar::Filtered(unit, step),
    }
}

/// Ticks from `start` to `stop` inclusive, or `None` when there would be more than `limit`.
fn range(calendar: Calendar, start: f64, stop: f64, limit: f64) -> Option<Vec<f64>> {
    let mut d = Date(time_clip(start - 1.0));
    calendar.floor(&mut d);
    calendar.offset(&mut d, 1.0);
    calendar.floor(&mut d);
    let mut out = Vec::new();
    let mut previous = f64::NEG_INFINITY;
    while d.time() <= stop && d.time() > previous {
        #[expect(clippy::cast_precision_loss, reason = "tick counts stay small")]
        if out.len() as f64 >= limit {
            return None;
        }
        previous = d.time();
        out.push(previous);
        calendar.offset(&mut d, 1.0);
        calendar.floor(&mut d);
    }
    Some(out)
}

/// d3's `tickStep`: a round step that splits the range into about `count` parts.
fn tick_step(start: f64, stop: f64, count: f64) -> f64 {
    let step = (stop - start) / js_max(0.0, count);
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    let factor = if error >= 50f64.sqrt() {
        10.0
    } else if error >= 10f64.sqrt() {
        5.0
    } else if error >= std::f64::consts::SQRT_2 {
        2.0
    } else {
        1.0
    };
    10f64.powf(power) * factor
}

/// d3's automatic intervals: unit, step, and approximate length.
const AUTO: [(Unit, f64, f64); 18] = [
    (Unit::Second, 1.0, SECOND),
    (Unit::Second, 5.0, 5.0 * SECOND),
    (Unit::Second, 15.0, 15.0 * SECOND),
    (Unit::Second, 30.0, 30.0 * SECOND),
    (Unit::Minute, 1.0, MINUTE),
    (Unit::Minute, 5.0, 5.0 * MINUTE),
    (Unit::Minute, 15.0, 15.0 * MINUTE),
    (Unit::Minute, 30.0, 30.0 * MINUTE),
    (Unit::Hour, 1.0, HOUR),
    (Unit::Hour, 3.0, 3.0 * HOUR),
    (Unit::Hour, 6.0, 6.0 * HOUR),
    (Unit::Hour, 12.0, 12.0 * HOUR),
    (Unit::Day, 1.0, DAY),
    (Unit::Day, 2.0, 2.0 * DAY),
    (Unit::Week(0.0), 1.0, WEEK),
    (Unit::Month, 1.0, MONTH),
    (Unit::Month, 3.0, 3.0 * MONTH),
    (Unit::Year, 1.0, YEAR),
];

/// About `count` ticks at round times, chosen the way d3's time scale chooses them.
pub(crate) fn auto_ticks(start: f64, stop: f64, count: f64) -> Vec<f64> {
    let target = (stop - start) / count;
    let i = AUTO
        .iter()
        .position(|&(_, _, ms)| ms > target || target.is_nan())
        .unwrap_or(AUTO.len());
    let calendar = match (i.checked_sub(1).and_then(|p| AUTO.get(p)), AUTO.get(i)) {
        (_, None) => every(
            Unit::Year,
            js_max(1.0, tick_step(start / YEAR, stop / YEAR, count)),
        ),
        (None, _) => every(
            Unit::Millisecond,
            js_max(tick_step(start, stop, count), 1.0),
        ),
        (Some(&before), Some(&after)) => {
            let (unit, step, _) = if target / before.2 < after.2 / target {
                before
            } else {
                after
            };
            every(unit, step)
        }
    };
    range(calendar, start, stop, 4.0 * count + 8.0).unwrap_or_default()
}

/// The interval units `tickInterval` accepts.
const INTERVALS: [(&str, Unit); 7] = [
    ("millisecond", Unit::Millisecond),
    ("second", Unit::Second),
    ("minute", Unit::Minute),
    ("hour", Unit::Hour),
    ("day", Unit::Day),
    ("week", Unit::Week(0.0)),
    ("month", Unit::Month),
];

/// The days a week can start on, from Sunday.
const WEEK_STARTS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

/// Ticks for a `tickInterval` such as `1week`, or `None` when it is not one or asks for more
/// than `limit`.
pub(crate) fn interval_ticks(
    interval: &str,
    weekday: &str,
    start: f64,
    stop: f64,
    limit: f64,
) -> Option<Vec<f64>> {
    // `^([1-9]\d*)(millisecond|second|...|month)$`.
    let digits = interval.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || interval.starts_with('0') {
        return None;
    }
    let name = interval.get(digits..)?;
    let (_, unit) = INTERVALS.iter().find(|(own, _)| *own == name)?;
    let step: f64 = interval.get(..digits)?.parse().ok()?;
    let unit = match unit {
        Unit::Week(_) => {
            let start = WEEK_STARTS
                .iter()
                .position(|day| *day == weekday)
                .unwrap_or(0);
            #[expect(clippy::cast_precision_loss, reason = "0 to 6")]
            Unit::Week(start as f64)
        }
        other => *other,
    };
    let calendar = every(unit, step);
    let base = calendar.unit();
    if (stop - start) / (js_min(step, base.cycle()) * base.ms()) > limit {
        return None;
    }
    range(calendar, start, stop, limit)
}

/// `String(value)` with a sign, padded to `width` with `fill`, as d3-time-format pads.
fn pad(value: f64, fill: &str, width: usize) -> String {
    let negative = value < 0.0;
    let digits = int_string(if negative { -value } else { value });
    let mut out = String::from(if negative { "-" } else { "" });
    let length = digits.encode_utf16().count();
    if length < width {
        out.push_str(&fill.repeat(width - length));
    }
    out.push_str(&digits);
    out
}

/// A copy of `date` at the start of its `unit`.
fn floored(date: Date, unit: Unit) -> Date {
    let mut d = date;
    unit.floor(&mut d);
    d
}

/// Weeks since the last `week_start` day before the year began: d3's `%U` and `%W`.
fn week_of_year(date: Date, week_start: f64) -> f64 {
    let week = Unit::Week(week_start);
    let year_start = floored(date, Unit::Year);
    span(
        floored(Date(time_clip(year_start.time() - 1.0)), week),
        floored(date, week),
        WEEK,
    )
}

/// The Thursday that decides which ISO week and ISO year a date belongs to.
fn iso_thursday(date: Date) -> Date {
    let thursday = Unit::Week(4.0);
    let day = date.weekday();
    let late = day >= 4.0 || day == 0.0;
    let mut d = floored(
        if late {
            date
        } else {
            Date(time_clip(date.time() - 1.0))
        },
        thursday,
    );
    if !late {
        thursday.offset(&mut d, 1.0);
        thursday.floor(&mut d);
    }
    d
}

/// Formats a date with a d3 time format specifier such as `%Y-%m-%d`.
pub(crate) fn time_format(specifier: &str, date: Date) -> String {
    let mut out = String::with_capacity(specifier.len() + 8);
    let mut chars = specifier.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut c = chars.next();
        let fill = match c {
            Some('-') => Some(""),
            Some('_') => Some(" "),
            Some('0') => Some("0"),
            _ => None,
        };
        let fill = if let Some(fill) = fill {
            c = chars.next();
            fill
        } else if c == Some('e') {
            " "
        } else {
            "0"
        };
        if let Some(c) = c {
            directive(&mut out, c, date, fill);
        }
    }
    out
}

/// The name at `value` in `names`, or an empty string.
fn name(names: &[&'static str], value: f64) -> &'static str {
    index(value)
        .and_then(|i| names.get(i))
        .copied()
        .unwrap_or("")
}

/// Appends the d3 directive `c` for `d`, padded with `p`.
fn directive(out: &mut String, c: char, d: Date, p: &str) {
    let text = match c {
        'a' => name(&WEEKDAYS, d.weekday())
            .get(..3)
            .unwrap_or("")
            .to_owned(),
        'A' => name(&WEEKDAYS, d.weekday()).to_owned(),
        'b' => name(&MONTHS, d.month()).get(..3).unwrap_or("").to_owned(),
        'B' => name(&MONTHS, d.month()).to_owned(),
        'c' => time_format("%x, %X", d),
        'd' | 'e' => pad(d.date(), p, 2),
        'f' => {
            let mut text = pad(d.ms(), p, 3);
            text.push_str("000");
            text
        }
        'g' => pad(iso_thursday(d).year() % 100.0, p, 2),
        'G' => pad(iso_thursday(d).year() % 10000.0, p, 4),
        'H' => pad(d.hours(), p, 2),
        'I' => pad(twelve(d.hours()), p, 2),
        'j' => pad(
            1.0 + span(floored(d, Unit::Year), floored(d, Unit::Day), DAY),
            p,
            3,
        ),
        'L' => pad(d.ms(), p, 3),
        'm' => pad(d.month() + 1.0, p, 2),
        'M' => pad(d.minutes(), p, 2),
        'p' => (if d.hours() >= 12.0 { "PM" } else { "AM" }).to_owned(),
        'q' => int_string(1.0 + (d.month() / 3.0).floor()),
        'Q' => int_string(d.time()),
        's' => int_string((d.time() / 1000.0).floor()),
        'S' => pad(d.seconds(), p, 2),
        'u' => int_string(if d.weekday() == 0.0 { 7.0 } else { d.weekday() }),
        'U' => pad(week_of_year(d, 0.0), p, 2),
        'V' => {
            let thursday = iso_thursday(d);
            let year_start = floored(thursday, Unit::Year);
            let first = floored(year_start, Unit::Week(4.0));
            let extra = if index(year_start.weekday()) == Some(4) {
                1.0
            } else {
                0.0
            };
            pad(span(first, thursday, WEEK) + extra, p, 2)
        }
        'w' => int_string(d.weekday()),
        'W' => pad(week_of_year(d, 1.0), p, 2),
        'x' => time_format("%-m/%-d/%Y", d),
        'X' => time_format("%-I:%M:%S %p", d),
        'y' => pad(d.year() % 100.0, p, 2),
        'Y' => pad(d.year() % 10000.0, p, 4),
        // The UTC offset is always zero.
        'Z' => "+0000".to_owned(),
        '%' => "%".to_owned(),
        other => {
            out.push(other);
            return;
        }
    };
    out.push_str(&text);
}

/// The hour on a twelve-hour clock.
fn twelve(hours: f64) -> f64 {
    let h = hours % 12.0;
    if h == 0.0 || h.is_nan() { 12.0 } else { h }
}
