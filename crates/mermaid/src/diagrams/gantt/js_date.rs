//! JavaScript `Date` semantics for the gantt port: time values, the calendar arithmetic of the
//! ECMAScript specification, and V8's `Date` string parser, which `new Date(text)` runs.
//!
//! Pele reads and writes dates in the host's local time zone. The compiler reads no environment,
//! so local time here is UTC, and Ondine matches Pele running with `TZ=UTC`.

use crate::util::chars::is_space;

/// Milliseconds in a day.
pub(crate) const DAY: f64 = 86_400_000.0;
/// The largest time value a `Date` holds, in milliseconds either side of the epoch.
const MAX_TIME: f64 = 8.64e15;

/// A `Date`: milliseconds since the epoch, or NaN for an invalid date.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Date(pub(crate) f64);

/// The calendar fields of a valid time value.
#[derive(Clone, Copy)]
struct Fields {
    /// Day of the month, from 1.
    date: i64,
    /// Days since the epoch.
    day: i64,
    /// Month, from 0.
    month: i64,
    /// Milliseconds since midnight.
    time: i64,
    /// Full year.
    year: i64,
}

/// `TimeClip`: NaN outside the representable range, otherwise truncated toward zero.
pub(crate) fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > MAX_TIME {
        return f64::NAN;
    }
    time.trunc() + 0.0
}

/// Days from 1970-01-01 to the first of `month` (0 to 11) in `year`.
fn days_from_civil(year: i64, month: i64) -> i64 {
    let y = if month < 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month + 1;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year, month (0 to 11), and day of the month of a day count since the epoch.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 2 } else { mp - 10 };
    let year = yoe + era * 400 + i64::from(month < 2);
    (year, month, date)
}

/// `MakeDay`, with V8's bounds on the year and month.
pub(crate) fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if !(year.is_finite() && month.is_finite() && date.is_finite()) {
        return f64::NAN;
    }
    let (year, month) = (year.trunc(), month.trunc());
    if !(-1e6..=1e6).contains(&year) || !(-1e7..=1e7).contains(&month) {
        return f64::NAN;
    }

    #[expect(clippy::cast_possible_truncation, reason = "bounded above")]
    let (year, month) = (year as i64, month as i64);
    let year = year + month.div_euclid(12);
    let month = month.rem_euclid(12);
    #[expect(clippy::cast_precision_loss, reason = "a day count far below 2^52")]
    let first = days_from_civil(year, month) as f64;
    first + date.trunc() - 1.0
}

/// `MakeTime`.
pub(crate) fn make_time(hours: f64, minutes: f64, seconds: f64, ms: f64) -> f64 {
    if !(hours.is_finite() && minutes.is_finite() && seconds.is_finite() && ms.is_finite()) {
        return f64::NAN;
    }
    hours.trunc() * 3_600_000.0 + minutes.trunc() * 60_000.0 + seconds.trunc() * 1000.0 + ms.trunc()
}

/// `MakeDate`.
pub(crate) fn make_date(day: f64, time: f64) -> f64 {
    let date = day * DAY + time;
    if date.is_finite() { date } else { f64::NAN }
}

/// The year `new Date(year, ...)` and `Date.UTC` use: 0 to 99 mean 1900 to 1999.
fn full_year(year: f64) -> f64 {
    if year.is_finite() && (0.0..100.0).contains(&year.trunc()) {
        1900.0 + year.trunc()
    } else {
        year
    }
}

impl Date {
    /// `new Date(year, month, date, hours, minutes, seconds, ms)` in local time, which is UTC.
    pub(crate) fn local(
        year: f64,
        month: f64,
        date: f64,
        hours: f64,
        minutes: f64,
        seconds: f64,
        ms: f64,
    ) -> Self {
        let day = make_day(full_year(year), month, date);
        Self(time_clip(make_date(
            day,
            make_time(hours, minutes, seconds, ms),
        )))
    }

    /// The time value, as `+date` reads it.
    pub(crate) fn time(self) -> f64 {
        self.0
    }

    /// Whether the time value is a number.
    pub(crate) fn is_valid(self) -> bool {
        !self.0.is_nan()
    }

    /// The calendar fields, or `None` for an invalid date.
    fn fields(self) -> Option<Fields> {
        if self.0.is_nan() {
            return None;
        }
        #[expect(clippy::cast_possible_truncation, reason = "a clipped time value")]
        let t = self.0 as i64;
        let day = t.div_euclid(86_400_000);
        let time = t.rem_euclid(86_400_000);
        let (year, month, date) = civil_from_days(day);
        Some(Fields {
            date,
            day,
            month,
            time,
            year,
        })
    }

    #[expect(clippy::cast_precision_loss, reason = "calendar fields stay small")]
    /// One calendar field, or NaN for an invalid date.
    fn get(self, field: impl Fn(Fields) -> i64) -> f64 {
        self.fields()
            .map_or(f64::NAN, |fields| field(fields) as f64)
    }

    /// `getFullYear`.
    pub(crate) fn year(self) -> f64 {
        self.get(|f| f.year)
    }

    /// `getMonth`, from 0.
    pub(crate) fn month(self) -> f64 {
        self.get(|f| f.month)
    }

    /// `getDate`, from 1.
    pub(crate) fn date(self) -> f64 {
        self.get(|f| f.date)
    }

    /// `getDay`: 0 for Sunday.
    pub(crate) fn weekday(self) -> f64 {
        self.get(|f| (f.day + 4).rem_euclid(7))
    }

    /// `getHours`.
    pub(crate) fn hours(self) -> f64 {
        self.get(|f| f.time / 3_600_000)
    }

    /// `getMinutes`.
    pub(crate) fn minutes(self) -> f64 {
        self.get(|f| f.time / 60_000 % 60)
    }

    /// `getSeconds`.
    pub(crate) fn seconds(self) -> f64 {
        self.get(|f| f.time / 1000 % 60)
    }

    /// `getMilliseconds`.
    pub(crate) fn ms(self) -> f64 {
        self.get(|f| f.time % 1000)
    }

    /// `setTime`.
    pub(crate) fn set_time(&mut self, time: f64) {
        self.0 = time_clip(time);
    }

    /// Replaces the day of a valid date, keeping its time of day. An invalid date stays invalid.
    fn set_day(&mut self, day: impl Fn(Fields) -> f64) {
        let Some(fields) = self.fields() else {
            return;
        };
        #[expect(clippy::cast_precision_loss, reason = "milliseconds in a day")]
        let time = fields.time as f64;
        self.0 = time_clip(make_date(day(fields), time));
    }

    /// `setDate`.
    pub(crate) fn set_date(&mut self, date: f64) {
        #[expect(clippy::cast_precision_loss, reason = "calendar fields stay small")]
        self.set_day(|f| make_day(f.year as f64, f.month as f64, date));
    }

    /// `setMonth(month)`, or `setMonth(month, date)` when `date` is given.
    pub(crate) fn set_month(&mut self, month: f64, date: Option<f64>) {
        #[expect(clippy::cast_precision_loss, reason = "calendar fields stay small")]
        self.set_day(|f| make_day(f.year as f64, month, date.unwrap_or(f.date as f64)));
    }

    /// `setFullYear(year)`. An invalid date counts as the epoch, as the specification says.
    pub(crate) fn set_full_year(&mut self, year: f64) {
        if self.0.is_nan() {
            self.0 = 0.0;
        }
        #[expect(clippy::cast_precision_loss, reason = "calendar fields stay small")]
        self.set_day(|f| make_day(year, f.month as f64, f.date as f64));
    }

    /// `setHours(hours, minutes, seconds, ms)`.
    pub(crate) fn set_hours(&mut self, hours: f64, minutes: f64, seconds: f64, ms: f64) {
        let Some(fields) = self.fields() else {
            return;
        };
        #[expect(clippy::cast_precision_loss, reason = "a day count far below 2^52")]
        let day = fields.day as f64;
        self.0 = time_clip(make_date(day, make_time(hours, minutes, seconds, ms)));
    }
}

/// `Date.UTC(year, month, date, hours, minutes, seconds, ms)`.
pub(crate) fn utc(
    year: f64,
    month: f64,
    date: f64,
    hours: f64,
    minutes: f64,
    seconds: f64,
    ms: f64,
) -> f64 {
    Date::local(year, month, date, hours, minutes, seconds, ms).0
}

/// `new Date(text)`: V8's `DateParser`, which reads ES5 ISO strings and falls back to legacy
/// forms such as `Jan 5 2014 10:00 PM`. Returns NaN for what it does not accept. Port of
/// `src/date/dateparser-inl.h` and `dateparser.cc` in V8.
pub(crate) fn parse(text: &str) -> Date {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut scanner = Scanner::new(&units);
    let mut day = DayComposer::default();
    let mut time = TimeComposer::default();
    let mut tz = ZoneComposer::default();

    let Some(fields) = parse_tokens(&mut scanner, &mut day, &mut time, &mut tz) else {
        return Date(f64::NAN);
    };

    let [year, month, date, hours, minutes, seconds, ms, offset] = fields;
    let value = make_date(
        make_day(year, month, date),
        make_time(hours, minutes, seconds, ms),
    );
    if offset.is_nan() {
        if !(-(MAX_TIME + 864e6)..=MAX_TIME + 864e6).contains(&value) {
            return Date(f64::NAN);
        }
        return Date(time_clip(value));
    }
    let value = value - offset * 1000.0;
    if !(-MAX_TIME..=MAX_TIME).contains(&value) {
        return Date(f64::NAN);
    }
    Date(time_clip(value))
}

/// No value: V8's `kNone`.
const NONE: i32 = i32::MAX;

/// What a keyword names. V8's `KeywordType`.
#[derive(Clone, Copy, PartialEq)]
enum Keyword {
    /// `am` or `pm`.
    AmPm,
    /// A word that is not a keyword.
    Invalid,
    /// A month name.
    MonthName,
    /// `T`.
    TimeSeparator,
    /// A time zone such as `UTC` or `PST`.
    TimeZoneName,
}

/// A token of a date string. V8's `DateToken`.
#[derive(Clone, Copy, PartialEq)]
enum Token {
    /// The end of the string.
    End,
    /// A string that cannot be a date.
    Invalid,
    /// A word.
    Keyword {
        /// What the word names.
        kind: Keyword,
        /// The length in code units.
        length: usize,
        /// The value the keyword stands for.
        value: i32,
    },
    /// A run of digits: its value from the first nine and its length.
    Number {
        /// The number of digits.
        length: usize,
        /// The value of the first nine digits.
        value: i32,
    },
    /// One of `:`, `-`, `+`, `.`, and `)`.
    Symbol(u16),
    /// Any other character, or a parenthesized comment.
    Unknown,
    /// One whitespace character.
    WhiteSpace,
}

impl Token {
    /// Whether the token is a number.
    fn is_number(self) -> bool {
        matches!(self, Self::Number { .. })
    }

    /// Whether the token is a number of exactly `n` digits.
    fn is_fixed(self, n: usize) -> bool {
        matches!(self, Self::Number { length, .. } if length == n)
    }

    /// The value of a number token, or 0.
    fn number(self) -> i32 {
        match self {
            Self::Number { value, .. } => value,
            _ => 0,
        }
    }

    /// Whether the token is the symbol `c`.
    fn is_symbol(self, c: u8) -> bool {
        self == Self::Symbol(u16::from(c))
    }

    /// Whether the token is `+` or `-`.
    fn is_sign(self) -> bool {
        self.is_symbol(b'+') || self.is_symbol(b'-')
    }

    /// The sign a `+` or `-` token gives.
    fn sign(self) -> i32 {
        if self.is_symbol(b'-') { -1 } else { 1 }
    }

    /// Whether the token is a keyword of `kind`.
    fn is_keyword(self, kind: Keyword) -> bool {
        matches!(self, Self::Keyword { kind: own, .. } if own == kind)
    }

    /// Whether the token is the time zone `Z`.
    fn is_z(self) -> bool {
        matches!(
            self,
            Self::Keyword {
                kind: Keyword::TimeZoneName,
                length: 1,
                value: 0
            }
        )
    }
}

/// The keywords V8 knows by their first three letters, in its table order.
const KEYWORDS: [([u8; 3], Keyword, i32); 27] = [
    (*b"jan", Keyword::MonthName, 1),
    (*b"feb", Keyword::MonthName, 2),
    (*b"mar", Keyword::MonthName, 3),
    (*b"apr", Keyword::MonthName, 4),
    (*b"may", Keyword::MonthName, 5),
    (*b"jun", Keyword::MonthName, 6),
    (*b"jul", Keyword::MonthName, 7),
    (*b"aug", Keyword::MonthName, 8),
    (*b"sep", Keyword::MonthName, 9),
    (*b"oct", Keyword::MonthName, 10),
    (*b"nov", Keyword::MonthName, 11),
    (*b"dec", Keyword::MonthName, 12),
    (*b"am\0", Keyword::AmPm, 0),
    (*b"pm\0", Keyword::AmPm, 12),
    (*b"ut\0", Keyword::TimeZoneName, 0),
    (*b"utc", Keyword::TimeZoneName, 0),
    (*b"z\0\0", Keyword::TimeZoneName, 0),
    (*b"gmt", Keyword::TimeZoneName, 0),
    (*b"cdt", Keyword::TimeZoneName, -5),
    (*b"cst", Keyword::TimeZoneName, -6),
    (*b"edt", Keyword::TimeZoneName, -4),
    (*b"est", Keyword::TimeZoneName, -5),
    (*b"mdt", Keyword::TimeZoneName, -6),
    (*b"mst", Keyword::TimeZoneName, -7),
    (*b"pdt", Keyword::TimeZoneName, -7),
    (*b"pst", Keyword::TimeZoneName, -8),
    (*b"t\0\0", Keyword::TimeSeparator, 0),
];

/// Reads tokens one ahead, as V8's `DateStringTokenizer` over its `InputReader`.
struct Scanner<'a> {
    /// The current character, or 0 past the end.
    ch: u32,
    /// The index of the next code unit.
    index: usize,
    /// The token ahead.
    next: Token,
    /// The string as UTF-16.
    units: &'a [u16],
}

impl<'a> Scanner<'a> {
    /// A scanner at the start of `units`, with the first token read.
    fn new(units: &'a [u16]) -> Self {
        let mut scanner = Self {
            ch: 0,
            index: 0,
            next: Token::End,
            units,
        };
        scanner.advance();
        scanner.next = scanner.scan();
        scanner
    }

    /// Reads the next code unit.
    fn advance(&mut self) {
        self.ch = self
            .units
            .get(self.index)
            .map_or(0, |&unit| u32::from(unit));
        self.index += 1;
    }

    /// Reads past `c` when it is the current character.
    fn skip(&mut self, c: u8) -> bool {
        if self.ch == u32::from(c) {
            self.advance();
            return true;
        }
        false
    }

    /// Whether the current character is an ASCII digit.
    fn is_digit(&self) -> bool {
        (48..=57).contains(&self.ch)
    }

    /// Whether the current character can be part of a word: at or above `A` and not a space.
    fn is_word(&self) -> bool {
        self.ch >= 65 && !is_space(self.ch)
    }

    /// Reads one token. V8's `DateStringTokenizer::Scan`.
    fn scan(&mut self) -> Token {
        let start = self.index;
        if self.ch == 0 {
            return Token::End;
        }
        if self.is_digit() {
            let mut value: i32 = 0;
            let mut digits = 0;
            while self.is_digit() {
                if digits < 9 {
                    value = value * 10 + i32::try_from(self.ch - 48).unwrap_or(0);
                }
                digits += 1;
                self.advance();
            }
            return Token::Number {
                length: self.index - start,
                value,
            };
        }
        for symbol in *b":-+.)" {
            if self.skip(symbol) {
                return Token::Symbol(u16::from(symbol));
            }
        }
        if self.is_word() {
            let mut prefix = [0u32; 3];
            let mut length = 0;
            while self.is_word() {
                if let Some(slot) = prefix.get_mut(length) {
                    *slot = self.ch | 0x20;
                }
                length += 1;
                self.advance();
            }
            let found = KEYWORDS.iter().find(|(word, kind, _)| {
                word.iter().zip(prefix).all(|(&w, p)| u32::from(w) == p)
                    && (length <= 3 || *kind == Keyword::MonthName)
            });
            let (kind, value) =
                found.map_or((Keyword::Invalid, 0), |&(_, kind, value)| (kind, value));
            return Token::Keyword {
                kind,
                length,
                value,
            };
        }
        if is_space(self.ch) {
            self.advance();
            return Token::WhiteSpace;
        }
        if self.ch == 40 {
            let mut balance = 0;
            loop {
                if self.ch == 41 {
                    balance -= 1;
                } else if self.ch == 40 {
                    balance += 1;
                }
                self.advance();
                if balance <= 0 || self.ch == 0 {
                    break;
                }
            }
            return Token::Unknown;
        }
        self.advance();
        Token::Unknown
    }

    /// Returns the token ahead and reads the one after it.
    fn next(&mut self) -> Token {
        let token = self.next;
        self.next = self.scan();
        token
    }

    /// The token ahead.
    fn peek(&self) -> Token {
        self.next
    }

    /// Reads past the token ahead when it is the symbol `c`.
    fn skip_symbol(&mut self, c: u8) -> bool {
        if self.next.is_symbol(c) {
            self.next = self.scan();
            return true;
        }
        false
    }
}

/// Whether `x` is from `low` to `high`, inclusive.
fn between(x: i32, low: i32, high: i32) -> bool {
    (low..=high).contains(&x)
}

/// The year, month, and day read so far. V8's `DayComposer`.
#[derive(Default)]
struct DayComposer {
    /// The numbers read, in order.
    comp: [i32; 3],
    /// How many numbers were read.
    index: usize,
    /// Whether the date was an ES5 ISO date, which fixes its field order.
    iso: bool,
    /// A month given by name.
    named_month: Option<i32>,
}

impl DayComposer {
    /// Adds a component. `false` when all are taken.
    fn add(&mut self, n: i32) -> bool {
        let Some(slot) = self.comp.get_mut(self.index) else {
            return false;
        };
        *slot = n;
        self.index += 1;
        true
    }

    /// The year, month from 0, and day, or `None` when they are not a date.
    fn write(&mut self) -> Option<[f64; 3]> {
        if self.index < 1 {
            return None;
        }
        // Day and month default to 1.
        for slot in self.comp.iter_mut().skip(self.index) {
            *slot = 1;
        }
        let [a, b, c] = self.comp;
        let is_day = |x: i32| between(x, 1, 31);
        let (mut year, month, day) = match self.named_month {
            None if self.iso || !is_day(a) => (a, b, c),
            None => (c, a, b),
            Some(month) if !is_day(a) => (a, month, b),
            Some(month) => (b, month, a),
        };
        if !self.iso {
            if between(year, 0, 49) {
                year += 2000;
            } else if between(year, 50, 99) {
                year += 1900;
            }
        }
        if !between(month, 1, 12) || !is_day(day) {
            return None;
        }
        Some([f64::from(year), f64::from(month - 1), f64::from(day)])
    }
}

/// The time of day read so far. V8's `TimeComposer`.
struct TimeComposer {
    /// Hours, minutes, seconds, and milliseconds read, in order.
    comp: [i32; 4],
    /// The hours `AM` or `PM` adds, or `NONE`.
    hour_offset: i32,
    /// How many components were read.
    index: usize,
}

impl Default for TimeComposer {
    fn default() -> Self {
        Self {
            comp: [0; 4],
            hour_offset: NONE,
            index: 0,
        }
    }
}

impl TimeComposer {
    /// Whether no time component was read.
    fn is_empty(&self) -> bool {
        self.index == 0
    }

    /// Whether `n` can be the next minute, second, or millisecond.
    fn is_expecting(&self, n: i32) -> bool {
        (self.index == 1 && between(n, 0, 59))
            || (self.index == 2 && between(n, 0, 59))
            || (self.index == 3 && between(n, 0, 999))
    }

    /// Adds a component. `false` when all are taken.
    fn add(&mut self, n: i32) -> bool {
        let Some(slot) = self.comp.get_mut(self.index) else {
            return false;
        };
        *slot = n;
        self.index += 1;
        true
    }

    /// Adds the last component the time has.
    fn add_final(&mut self, n: i32) -> bool {
        if !self.add(n) {
            return false;
        }
        self.index = 4;
        true
    }

    /// Hours, minutes, seconds, and milliseconds, or `None` when they are not a time.
    fn write(&mut self) -> Option<[f64; 4]> {
        for slot in self.comp.iter_mut().skip(self.index) {
            *slot = 0;
        }
        let [mut hour, minute, second, ms] = self.comp;
        if self.hour_offset != NONE {
            if !between(hour, 0, 12) {
                return None;
            }
            hour = hour % 12 + self.hour_offset;
        }
        let valid = between(hour, 0, 23)
            && between(minute, 0, 59)
            && between(second, 0, 59)
            && between(ms, 0, 999);
        if !valid && (hour != 24 || minute != 0 || second != 0 || ms != 0) {
            return None;
        }
        Some([
            f64::from(hour),
            f64::from(minute),
            f64::from(second),
            f64::from(ms),
        ])
    }
}

/// The UTC offset read so far. V8's `TimeZoneComposer`.
struct ZoneComposer {
    /// Hours, or `NONE`.
    hour: i32,
    /// Minutes, or `NONE`.
    minute: i32,
    /// 1 or -1, or `NONE` for local time.
    sign: i32,
}

impl Default for ZoneComposer {
    fn default() -> Self {
        Self {
            hour: NONE,
            minute: NONE,
            sign: NONE,
        }
    }
}

impl ZoneComposer {
    /// Sets a whole-hour offset.
    fn set(&mut self, hours: i32) {
        self.sign = if hours < 0 { -1 } else { 1 };
        self.hour = hours.abs();
        self.minute = 0;
    }

    /// Whether `n` can be the offset's minutes.
    fn is_expecting(&self, n: i32) -> bool {
        self.hour != NONE && self.minute == NONE && between(n, 0, 59)
    }

    /// Whether the offset is zero.
    fn is_utc(&self) -> bool {
        self.hour == 0 && self.minute == 0
    }

    /// The offset in seconds, NaN when none was given.
    fn write(&mut self) -> Option<f64> {
        if self.sign == NONE {
            return Some(f64::NAN);
        }
        let hour = if self.hour == NONE { 0 } else { self.hour };
        let minute = if self.minute == NONE { 0 } else { self.minute };
        let total = i64::from(hour) * 3600 + i64::from(minute) * 60;
        if !(0..=1_073_741_823).contains(&total) {
            return None;
        }
        #[expect(clippy::cast_precision_loss, reason = "bounded above")]
        let total = total as f64;
        Some(if self.sign < 0 { -total } else { total })
    }
}

/// The first three significant digits of a fraction of a second.
fn read_ms(token: Token) -> i32 {
    let Token::Number { length, value } = token else {
        return 0;
    };
    match length {
        1 => value * 100,
        2 => value * 10,
        3 => value,
        _ => {
            let mut length = length.min(9);
            let mut factor = 1;
            while length > 3 {
                factor *= 10;
                length -= 1;
            }
            value / factor
        }
    }
}

/// Reads an ES5 date-time string and returns the first token it leaves, `End` when it read the
/// whole string, or `Invalid` when the string cannot be a date. V8's `ParseES5DateTime`.
fn parse_es5(
    scanner: &mut Scanner<'_>,
    day: &mut DayComposer,
    time: &mut TimeComposer,
    tz: &mut ZoneComposer,
) -> Token {
    if scanner.peek().is_sign() {
        let sign_token = scanner.next();
        if !scanner.peek().is_fixed(6) {
            return sign_token;
        }
        let year = scanner.next().number();
        if sign_token.sign() < 0 && year == 0 {
            return sign_token;
        }
        day.add(sign_token.sign() * year);
    } else if scanner.peek().is_fixed(4) {
        day.add(scanner.next().number());
    } else {
        return scanner.next();
    }

    if scanner.skip_symbol(b'-') {
        if !scanner.peek().is_fixed(2) || !between(scanner.peek().number(), 1, 12) {
            return scanner.next();
        }
        day.add(scanner.next().number());
        if scanner.skip_symbol(b'-') {
            if !scanner.peek().is_fixed(2) || !between(scanner.peek().number(), 1, 31) {
                return scanner.next();
            }
            day.add(scanner.next().number());
        }
    }

    if scanner.peek().is_keyword(Keyword::TimeSeparator) {
        scanner.next();
        if !scanner.peek().is_fixed(2) || !between(scanner.peek().number(), 0, 24) {
            return Token::Invalid;
        }
        // 24:00, 24:00:00, and 24:00:00.000 are allowed, and no other time in hour 24.
        let hour_is_24 = scanner.peek().number() == 24;
        time.add(scanner.next().number());
        if !scanner.skip_symbol(b':') {
            return Token::Invalid;
        }
        if !scanner.peek().is_fixed(2)
            || !between(scanner.peek().number(), 0, 59)
            || (hour_is_24 && scanner.peek().number() > 0)
        {
            return Token::Invalid;
        }
        time.add(scanner.next().number());
        if scanner.skip_symbol(b':') {
            if !scanner.peek().is_fixed(2)
                || !between(scanner.peek().number(), 0, 59)
                || (hour_is_24 && scanner.peek().number() > 0)
            {
                return Token::Invalid;
            }
            time.add(scanner.next().number());
            if scanner.skip_symbol(b'.') {
                if !scanner.peek().is_number() || (hour_is_24 && scanner.peek().number() > 0) {
                    return Token::Invalid;
                }
                time.add(read_ms(scanner.next()));
            }
        }

        if scanner.peek().is_z() {
            scanner.next();
            tz.set(0);
        } else if scanner.peek().is_sign() {
            tz.sign = scanner.next().sign();
            if scanner.peek().is_fixed(4) {
                let hour_minute = scanner.next().number();
                let (hour, minute) = (hour_minute / 100, hour_minute % 100);
                if !between(hour, 0, 23) || !between(minute, 0, 59) {
                    return Token::Invalid;
                }
                tz.hour = hour;
                tz.minute = minute;
            } else {
                if !scanner.peek().is_fixed(2) || !between(scanner.peek().number(), 0, 23) {
                    return Token::Invalid;
                }
                tz.hour = scanner.next().number();
                if !scanner.skip_symbol(b':') {
                    return Token::Invalid;
                }
                if !scanner.peek().is_fixed(2) || !between(scanner.peek().number(), 0, 59) {
                    return Token::Invalid;
                }
                tz.minute = scanner.next().number();
            }
        }
        if scanner.peek() != Token::End {
            return Token::Invalid;
        }
    } else if scanner.peek() != Token::End {
        return scanner.next();
    }

    // Date-only forms without an offset are UTC. Date-time forms are local time.
    if tz.hour == NONE && time.is_empty() {
        tz.set(0);
    }
    day.iso = true;
    Token::End
}

/// Reads every token into the composers, then writes year, month, date, hours, minutes,
/// seconds, milliseconds, and the UTC offset in seconds (NaN for local time).
fn parse_tokens(
    scanner: &mut Scanner<'_>,
    day: &mut DayComposer,
    time: &mut TimeComposer,
    tz: &mut ZoneComposer,
) -> Option<[f64; 8]> {
    let mut token = parse_es5(scanner, day, time, tz);
    if token == Token::Invalid {
        return None;
    }
    let mut has_read_number = day.index > 0;

    while token != Token::End {
        match token {
            Token::Number { value: n, .. } => {
                has_read_number = true;
                if scanner.skip_symbol(b':') {
                    if scanner.skip_symbol(b':') {
                        if !time.is_empty() {
                            return None;
                        }
                        time.add(n);
                        time.add(0);
                    } else {
                        if !time.add(n) {
                            return None;
                        }
                        if scanner.peek().is_symbol(b'.') {
                            scanner.next();
                        }
                    }
                } else if scanner.skip_symbol(b'.') && time.is_expecting(n) {
                    time.add(n);
                    if !scanner.peek().is_number() {
                        return None;
                    }
                    let ms = read_ms(scanner.next());
                    if ms < 0 {
                        return None;
                    }
                    time.add_final(ms);
                } else if tz.is_expecting(n) {
                    tz.minute = n;
                } else if time.is_expecting(n) {
                    time.add_final(n);
                    // A finished time must be followed by the end, a space, `Z`, or a sign.
                    let peek = scanner.peek();
                    if peek != Token::End
                        && peek != Token::WhiteSpace
                        && !peek.is_z()
                        && !peek.is_sign()
                    {
                        return None;
                    }
                } else {
                    if !day.add(n) {
                        return None;
                    }
                    scanner.skip_symbol(b'-');
                }
            }
            Token::Keyword { kind, value, .. } => {
                if kind == Keyword::AmPm && !time.is_empty() {
                    time.hour_offset = value;
                } else if kind == Keyword::MonthName {
                    day.named_month = Some(value);
                    scanner.skip_symbol(b'-');
                } else if kind == Keyword::TimeZoneName && has_read_number {
                    tz.set(value);
                } else {
                    // A word after a number is garbage, and so is a number right after a word.
                    if has_read_number || scanner.peek().is_number() {
                        return None;
                    }
                }
            }
            _ if token.is_sign() && (tz.is_utc() || !time.is_empty()) => {
                tz.sign = token.sign();
                let (mut n, mut length) = (0, 0);
                if let Token::Number {
                    length: digits,
                    value,
                } = scanner.peek()
                {
                    scanner.next();
                    n = value;
                    length = digits;
                }
                has_read_number = true;

                if scanner.peek().is_symbol(b':') {
                    tz.hour = n;
                    tz.minute = NONE;
                } else if length == 1 || length == 2 {
                    tz.hour = n;
                    tz.minute = 0;
                } else if length == 3 || length == 4 {
                    tz.hour = n / 100;
                    tz.minute = n % 100;
                } else {
                    return None;
                }
            }
            _ if (token.is_sign() || token.is_symbol(b')')) && has_read_number => return None,
            _ => {}
        }
        token = scanner.next();
    }

    let [year, month, date] = day.write()?;
    let [hours, minutes, seconds, ms] = time.write()?;
    let offset = tz.write()?;
    Some([year, month, date, hours, minutes, seconds, ms, offset])
}
