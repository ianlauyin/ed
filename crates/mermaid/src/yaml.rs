//! A small YAML reader covering what Mermaid feeds to js-yaml with the JSON schema: block and flow
//! collections, quoted and plain scalars, and block scalars. Port of Pele's `src/util/yaml.ts`.

use crate::error::push_u32;
use crate::util::chars::{is_line_end, is_space};

/// A parsed YAML value, shaped like the JavaScript value Pele's reader returns.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    /// A sequence.
    Array(Vec<Value>),
    /// `true` or `false`.
    Bool(bool),
    /// `null`, `~`, or an empty value.
    Null,
    /// A number.
    Number(f64),
    /// A mapping, in JavaScript property order: integer-like keys ascending, then insertion order.
    Object(Vec<(String, Value)>),
    /// A string.
    String(String),
}

impl Value {
    /// Appends the value as JavaScript's `String(value)` writes it.
    pub(crate) fn push_js_string(&self, out: &mut String) {
        match self {
            Self::Array(items) => {
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    if !matches!(item, Self::Null) {
                        item.push_js_string(out);
                    }
                }
            }
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Null => out.push_str("null"),
            Self::Number(value) => push_number(out, *value),
            Self::Object(_) => out.push_str("[object Object]"),
            Self::String(value) => out.push_str(value),
        }
    }

    /// Whether JavaScript treats the value as true in a condition.
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Self::Array(_) | Self::Object(_) => true,
            Self::Bool(value) => *value,
            Self::Null => false,
            Self::Number(value) => *value != 0.0 && !value.is_nan(),
            Self::String(value) => !value.is_empty(),
        }
    }
}

/// Sets a property the way assigning to a JavaScript object does. An existing key keeps its place,
/// and a new array-index key goes before every larger index and every other key.
pub(crate) fn set(object: &mut Vec<(String, Value)>, key: String, value: Value) {
    if let Some(entry) = object.iter_mut().find(|entry| entry.0 == key) {
        entry.1 = value;
        return;
    }

    let at = match array_index(&key) {
        Some(index) => object
            .iter()
            .position(|entry| array_index(&entry.0).is_none_or(|other| other > index))
            .unwrap_or(object.len()),
        None => object.len(),
    };
    object.insert(at, (key, value));
}

// An array index as JavaScript orders property keys: a canonical integer below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || (bytes.len() > 1 && bytes.first() == Some(&b'0'))
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return None;
    }

    let value = bytes
        .iter()
        .fold(0u64, |acc, &b| acc * 10 + u64::from(b - b'0'));
    u32::try_from(value).ok().filter(|&index| index != u32::MAX)
}

/// Appends `value` as JavaScript's `Number.prototype.toString` writes it: the shortest digits that
/// read back as the same double, in plain or exponent notation.
pub(crate) fn push_number(out: &mut String, value: f64) {
    if value.is_nan() {
        out.push_str("NaN");
        return;
    }
    if value == 0.0 {
        out.push('0');
        return;
    }
    if value < 0.0 {
        out.push('-');
    }

    let value = value.abs();
    if value.is_infinite() {
        out.push_str("Infinity");
        return;
    }
    // Sizes and counts are mostly whole numbers, whose digits need no search.
    if value <= f64::from(u32::MAX) && value.fract() == 0.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the value is a whole number in range"
        )]
        push_u32(out, value as u32);
        return;
    }

    let (digits, n) = shortest(value);
    let k = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let push_digits = |out: &mut String, digits: &[u8]| {
        for &digit in digits {
            out.push(char::from(digit));
        }
    };

    if k <= n && n <= 21 {
        push_digits(out, &digits);
        for _ in k..n {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let (int, fraction) = digits.split_at(usize::try_from(n).unwrap_or(0));
        push_digits(out, int);
        out.push('.');
        push_digits(out, fraction);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in n..0 {
            out.push('0');
        }
        push_digits(out, &digits);
    } else {
        let (first, rest) = digits.split_at(1.min(digits.len()));
        push_digits(out, first);
        if !rest.is_empty() {
            out.push('.');
            push_digits(out, rest);
        }
        out.push('e');
        out.push(if n > 0 { '+' } else { '-' });
        push_u32(out, (n - 1).unsigned_abs());
    }
}

const BASE: u64 = 1_000_000_000;

// The shortest digits of a positive `value` that needs at most 22 decimals and stays below 2^53
// when scaled by them. The fewest decimals give the fewest digits. Powers of ten to 10^22 and
// integers below 2^53 are exact, so `m / 10^d == value` is the read-back test, and every candidate
// lies within two of the scaled value's floor. `None` when no candidate is unique.
fn shortest_scaled(value: f64) -> Option<(Vec<u8>, i32)> {
    let mut power = 1.0;
    for decimals in 0..23 {
        let scaled = value * power;
        if scaled >= 9e15 {
            return None;
        }

        let floor = scaled.floor();
        let mut found = None;
        for offset in [-2.0, -1.0, 0.0, 1.0, 2.0] {
            let candidate = floor + offset;
            if candidate >= 1.0 && (candidate / power).to_bits() == value.to_bits() {
                if found.is_some() {
                    return None;
                }
                found = Some(candidate);
            }
        }

        if let Some(found) = found {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the candidate is a whole number from 1 to 2^53"
            )]
            let found = found as u64;
            // `decimal` reads limbs without a leading zero limb.
            let limbs = [found % BASE, found / BASE];
            let used = if found < BASE { 1 } else { 2 };
            let mut digits = decimal(limbs.get(..used).unwrap_or_default());
            let n = i32::try_from(digits.len()).unwrap_or(0) - decimals;
            while digits.len() > 1 && digits.last() == Some(&b'0') {
                digits.pop();
            }
            return Some((digits, n));
        }
        power *= 10.0;
    }
    None
}

// The shortest decimal digits that read back as `value`, and the decimal point position `n`, so
// that `value` is `0.digits * 10^n`.
fn shortest(value: f64) -> (Vec<u8>, i32) {
    shortest_scaled(value).unwrap_or_else(|| shortest_exact(value))
}

// Expands the double exactly, then rounds to more and more digits.
fn shortest_exact(value: f64) -> (Vec<u8>, i32) {
    let bits = value.to_bits();
    let biased = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let fraction = bits & ((1 << 52) - 1);
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    };

    let mut limbs = Vec::new();
    let mut rest = mantissa;
    while rest > 0 {
        limbs.push(rest % BASE);
        rest /= BASE;
    }
    if exponent >= 0 {
        scale(&mut limbs, exponent.unsigned_abs(), 2, 29);
    } else {
        scale(&mut limbs, exponent.unsigned_abs(), 5, 13);
    }

    let exact = decimal(&limbs);
    let length = i32::try_from(exact.len()).unwrap_or(i32::MAX);
    let n = if exponent >= 0 {
        length
    } else {
        length + exponent
    };

    for precision in 1..=17 {
        let (mut digits, point) = round(&exact, n, precision);
        if reads_back(&digits, point, value) {
            while digits.len() > 1 && digits.last() == Some(&b'0') {
                digits.pop();
            }
            return (digits, point);
        }
    }
    round(&exact, n, 17)
}

// Multiplies base-10^9 limbs by `base^count`, `chunk` powers at a time.
fn scale(limbs: &mut Vec<u64>, mut count: u32, base: u64, chunk: u32) {
    while count > 0 {
        let step = count.min(chunk);
        let factor = base.pow(step);
        let mut carry = 0;
        for limb in limbs.iter_mut() {
            let product = *limb * factor + carry;
            *limb = product % BASE;
            carry = product / BASE;
        }
        while carry > 0 {
            limbs.push(carry % BASE);
            carry /= BASE;
        }
        count -= step;
    }
}

fn decimal(limbs: &[u64]) -> Vec<u8> {
    let mut out = Vec::new();
    for (at, &limb) in limbs.iter().rev().enumerate() {
        let mut chunk = [b'0'; 9];
        let mut rest = limb;
        for slot in chunk.iter_mut().rev() {
            *slot = b'0' + u8::try_from(rest % 10).unwrap_or(0);
            rest /= 10;
        }
        let skip = if at == 0 {
            chunk.iter().take_while(|&&b| b == b'0').count()
        } else {
            0
        };
        out.extend(chunk.iter().skip(skip));
    }
    out
}

// Rounds exact digits to `precision` digits, ties to even, carrying into the point position.
fn round(exact: &[u8], n: i32, precision: usize) -> (Vec<u8>, i32) {
    if exact.len() <= precision {
        return (exact.to_vec(), n);
    }

    let mut digits = exact.iter().take(precision).copied().collect::<Vec<_>>();
    let next = exact.get(precision).copied().unwrap_or(b'0');
    let odd = digits.last().is_some_and(|&d| (d - b'0') % 2 == 1);
    let sticky = exact.iter().skip(precision + 1).any(|&d| d != b'0');
    let up = next > b'5' || (next == b'5' && (sticky || odd));
    if !up {
        return (digits, n);
    }

    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return (digits, n);
        }
    }
    if let Some(first) = digits.first_mut() {
        *first = b'1';
    }
    (digits, n + 1)
}

fn reads_back(digits: &[u8], n: i32, value: f64) -> bool {
    let mut text = String::with_capacity(digits.len() + 6);
    for &digit in digits {
        text.push(char::from(digit));
    }
    let exponent = n - i32::try_from(digits.len()).unwrap_or(0);
    text.push('e');
    if exponent < 0 {
        text.push('-');
    }
    push_u32(&mut text, exponent.unsigned_abs());
    crate::util::float::parse(&text).is_some_and(|parsed| parsed.to_bits() == value.to_bits())
}

/// A failure while reading.
enum Fail {
    /// An error to report as is.
    Error(String),
    /// A quoted scalar or flow collection ran past the text read so far.
    Incomplete(&'static str),
}

impl Fail {
    fn into_message(self) -> String {
        match self {
            Self::Error(message) => message,
            Self::Incomplete(message) => message.to_owned(),
        }
    }
}

#[cold]
fn error(message: &str) -> Fail {
    Fail::Error(message.to_owned())
}

const MAX_DEPTH: usize = 64;

fn space(c: char) -> bool {
    is_space(u32::from(c))
}

fn trim(text: &str) -> &str {
    text.trim_matches(space)
}

// The length of the leading whitespace in UTF-16 units. JavaScript whitespace is all in the BMP.
fn leading(text: &str) -> usize {
    text.chars().take_while(|&c| space(c)).count()
}

// Drops the first `count` characters, as `slice(count)` does over BMP text.
fn after_chars(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((at, _)) => text.get(at..).unwrap_or(""),
        None => "",
    }
}

struct Reader {
    i: usize,
    lines: Vec<String>,
}

impl Reader {
    fn new(src: &str) -> Self {
        let mut normalized = String::with_capacity(src.len());
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                chars.next_if_eq(&'\n');
                normalized.push('\n');
            } else {
                normalized.push(c);
            }
        }
        Self {
            i: 0,
            lines: normalized.split('\n').map(str::to_owned).collect(),
        }
    }

    fn line(&self) -> &str {
        self.lines.get(self.i).map_or("", String::as_str)
    }

    fn skip_blank(&mut self) {
        while self.i < self.lines.len() {
            let t = trim(self.line());
            if !t.is_empty() && !t.starts_with('#') && t != "---" {
                break;
            }
            self.i += 1;
        }
    }

    fn indent(&self) -> Result<usize, Fail> {
        let bytes = self.line().as_bytes();
        let k = bytes.iter().take_while(|&&b| b == b' ').count();
        if bytes.get(k) == Some(&b'\t') {
            return Err(error("YAML: tabs are not allowed for indentation"));
        }
        Ok(k)
    }
}

fn is_blank(b: Option<&u8>) -> bool {
    matches!(b, Some(b' ' | b'\t' | b'\n' | b'\r'))
}

// Splits `key: rest` at the start of a line. Returns where the raw key ends and where the value starts.
fn split_key(line: &str) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let first = *bytes.first()?;
    let end;
    let colon;

    if first == b'"' || first == b'\'' {
        let mut i = 1;
        loop {
            let c = *bytes.get(i)?;
            if first == b'"' && c == b'\\' {
                i += 2;
            } else if c == first {
                if first == b'"' || bytes.get(i + 1) != Some(&b'\'') {
                    break;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        end = i + 1;
        let mut at = end;
        while is_blank(bytes.get(at)) {
            at += 1;
        }
        if bytes.get(at) != Some(&b':') {
            return None;
        }
        colon = at;
    } else {
        let c = line.chars().next()?;
        if space(c) || "#:,[]{}&*!|>'\"%@`".contains(c) {
            return None;
        }
        if first == b'-'
            && (bytes.len() < 2
                || is_blank(bytes.get(1))
                || matches!(bytes.get(1), Some(b':' | b'#')))
        {
            return None;
        }
        let mut at = 1;
        while at < bytes.len() && !matches!(bytes.get(at), Some(b':' | b'#')) {
            at += 1;
        }
        if bytes.get(at) != Some(&b':') {
            return None;
        }
        colon = at;
        let mut back = colon;
        while back > 0 && is_blank(bytes.get(back - 1)) {
            back -= 1;
        }
        end = back;
    }

    let mut length = colon + 1;
    if length < bytes.len() && !is_blank(bytes.get(length)) {
        return None;
    }
    while is_blank(bytes.get(length)) {
        length += 1;
    }
    Some((end, length))
}

fn scalar(raw: &str) -> Value {
    let s = trim(raw);
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        _ => {}
    }

    if is_int(s) || is_float(s) {
        return Value::Number(crate::util::float::parse(s).unwrap_or(f64::NAN));
    }

    let negative = s.starts_with('-');
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    let radix = match unsigned.get(..2) {
        Some("0x") => Some((16, unsigned.get(2..).unwrap_or(""))),
        Some("0o") => Some((8, unsigned.get(2..).unwrap_or(""))),
        Some("0b") => Some((2, unsigned.get(2..).unwrap_or(""))),
        _ => None,
    };
    if let Some((base, digits)) = radix
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_digit(base))
    {
        let value = radix_number(digits, base);
        return Value::Number(if negative { -value } else { value });
    }

    if let Some(name) = unsigned.strip_prefix('.')
        && matches!(name, "inf" | "Inf" | "INF")
    {
        return Value::Number(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if matches!(s, ".nan" | ".NaN" | ".NAN") {
        return Value::Number(f64::NAN);
    }
    Value::String(s.to_owned())
}

// /^[-+]?(?:0|[1-9][0-9]*)$/
fn is_int(s: &str) -> bool {
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    digits == "0"
        || (!digits.is_empty()
            && !digits.starts_with('0')
            && digits.bytes().all(|b| b.is_ascii_digit()))
}

// /^[-+]?(?:[0-9][0-9]*(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?$/
fn is_float(s: &str) -> bool {
    let bytes = s.strip_prefix(['-', '+']).unwrap_or(s).as_bytes();
    let digits = |from: usize| {
        bytes
            .iter()
            .skip(from)
            .take_while(|b| b.is_ascii_digit())
            .count()
    };

    let int = digits(0);
    let mut at = int;
    if bytes.get(at) == Some(&b'.') {
        let fraction = digits(at + 1);
        if int == 0 && fraction == 0 {
            return false;
        }
        at += 1 + fraction;
    } else if int == 0 {
        return false;
    }

    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'-' | b'+')) {
            at += 1;
        }
        let exponent = digits(at);
        if exponent == 0 {
            return false;
        }
        at += exponent;
    }
    at == bytes.len()
}

// JavaScript's `Number` of a `0x`, `0o`, or `0b` literal, rounded once to the nearest double.
fn radix_number(digits: &str, base: u32) -> f64 {
    let mut exact: Option<u128> = Some(0);
    let mut approx = 0.0;
    for c in digits.chars() {
        let digit = c.to_digit(base).unwrap_or(0);
        exact = exact
            .and_then(|value| value.checked_mul(u128::from(base)))
            .and_then(|value| value.checked_add(u128::from(digit)));
        approx = approx * f64::from(base) + f64::from(digit);
    }
    // ponytail: past 128 bits the value accumulates in doubles and may round twice.
    #[expect(
        clippy::cast_precision_loss,
        reason = "u128 to f64 rounds to nearest, as JavaScript does"
    )]
    exact.map_or(approx, |value| value as f64)
}

fn escape(e: u16) -> Option<u16> {
    let c = match u8::try_from(e).ok()? {
        b'0' => 0,
        b'a' => 7,
        b'b' => 8,
        b't' => 9,
        b'n' => 10,
        b'v' => 11,
        b'f' => 12,
        b'r' => 13,
        b'e' => 0x1b,
        b' ' => 32,
        b'"' => 34,
        b'/' => 47,
        b'\\' => 92,
        b'N' => 0x85,
        b'_' => 0xa0,
        b'L' => return Some(0x2028),
        b'P' => return Some(0x2029),
        _ => return None,
    };
    Some(c)
}

// Folds the line breaks of a scalar: blanks around a break go, one break becomes a space, and
// each further break stays.
fn fold(text: &str) -> String {
    if !text.contains('\n') {
        return text.to_owned();
    }

    let mut joined = String::with_capacity(text.len());
    let last = text.split('\n').count() - 1;
    for (i, line) in text.split('\n').enumerate() {
        let mut part = line;
        if i > 0 {
            part = part.trim_start_matches([' ', '\t']);
        }
        if i < last {
            part = part.trim_end_matches([' ', '\t']);
        }
        if i > 0 {
            joined.push('\n');
        }
        joined.push_str(part);
    }

    let mut out = String::with_capacity(joined.len());
    let mut breaks = 0;
    for c in joined.chars() {
        if c == '\n' {
            breaks += 1;
            continue;
        }
        push_breaks(&mut out, breaks);
        breaks = 0;
        out.push(c);
    }
    push_breaks(&mut out, breaks);
    out
}

fn push_breaks(out: &mut String, breaks: usize) {
    match breaks {
        0 => {}
        1 => out.push(' '),
        _ => {
            for _ in 1..breaks {
                out.push('\n');
            }
        }
    }
}

fn unquote_double(body: &str) -> Result<String, Fail> {
    let mut joined = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut from = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'\\') && bytes.get(i + 1) == Some(&b'\n') {
            joined.push_str(body.get(from..i).unwrap_or(""));
            i += 2;
            while matches!(bytes.get(i), Some(b' ' | b'\t')) {
                i += 1;
            }
            from = i;
        } else {
            i += 1;
        }
    }
    joined.push_str(body.get(from..).unwrap_or(""));

    // Escapes count UTF-16 units, as Pele's string indexing does.
    let s = fold(&joined).encode_utf16().collect::<Vec<_>>();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while let Some(&ch) = s.get(i) {
        if ch != u16::from(b'\\') {
            out.push(ch);
            i += 1;
            continue;
        }
        i += 1;
        let e = s.get(i).copied().unwrap_or(0);
        let length = match e {
            0x78 => 2,
            0x75 => 4,
            0x55 => 8,
            _ => 0,
        };
        if length > 0 {
            let digits = s.get(i + 1..(i + 1 + length).min(s.len())).unwrap_or(&[]);
            push_code_point(&mut out, parse_hex(digits))?;
            i += length + 1;
        } else if let Some(c) = s.get(i).copied().and_then(escape) {
            out.push(c);
            i += 1;
        } else {
            return Err(error("YAML: unknown escape sequence"));
        }
    }
    Ok(String::from_utf16_lossy(&out))
}

// JavaScript's `parseInt(text, 16)`.
fn parse_hex(text: &[u16]) -> f64 {
    let mut rest = text;
    while let Some((&c, tail)) = rest.split_first()
        && is_space(u32::from(c))
    {
        rest = tail;
    }

    let mut negative = false;
    if let Some((&c, tail)) = rest.split_first()
        && (c == u16::from(b'-') || c == u16::from(b'+'))
    {
        negative = c == u16::from(b'-');
        rest = tail;
    }
    if let [zero, x, tail @ ..] = rest
        && *zero == u16::from(b'0')
        && (*x == u16::from(b'x') || *x == u16::from(b'X'))
    {
        rest = tail;
    }

    let mut value = 0.0;
    let mut any = false;
    for &c in rest {
        let Some(digit) = char::from_u32(u32::from(c)).and_then(|c| c.to_digit(16)) else {
            break;
        };
        value = value * 16.0 + f64::from(digit);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if negative { -value } else { value }
}

// `String.fromCodePoint`, which throws a RangeError for a value that is not a code point.
fn push_code_point(out: &mut Vec<u16>, value: f64) -> Result<(), Fail> {
    if !(0.0..=1_114_111.0).contains(&value) || value.fract() != 0.0 {
        let mut message = String::from("Invalid code point ");
        push_number(&mut message, value);
        return Err(Fail::Error(message));
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked to be an integer in the code point range"
    )]
    let code = value as u32;
    match char::from_u32(code) {
        Some(c) => out.extend_from_slice(c.encode_utf16(&mut [0; 2])),
        None => out.push(u16::try_from(code).unwrap_or(0xfffd)),
    }
    Ok(())
}

fn skip(s: &str, mut pos: usize, commas: bool) -> usize {
    let bytes = s.as_bytes();
    while let Some(&c) = bytes.get(pos) {
        if matches!(c, b' ' | b'\n' | b'\t' | b'\r') || (commas && c == b',') {
            pos += 1;
        } else if c == b'#' && (pos == 0 || previous_is_space(s, pos)) {
            pos = s
                .get(pos..)
                .and_then(|rest| rest.find('\n'))
                .map_or(s.len(), |at| pos + at);
        } else {
            break;
        }
    }
    pos
}

fn previous_is_space(s: &str, pos: usize) -> bool {
    s.get(..pos)
        .and_then(|head| head.chars().next_back())
        .is_some_and(space)
}

fn next_is(s: &str, pos: usize, test: impl Fn(char) -> bool) -> bool {
    s.get(pos..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(test)
}

// Entries of a flow collection are separated by exactly one comma, as js-yaml requires.
fn entry_start(c: u8, separated: bool) -> Result<(), Fail> {
    if !separated {
        return Err(error("YAML: missed comma between flow collection entries"));
    }
    if c == b',' {
        return Err(error("YAML: expected the node content, but found ','"));
    }
    Ok(())
}

// Parses a flow value starting at `pos`; returns the value and the index after it.
fn flow(s: &str, pos: usize, stops: &str, depth: usize) -> Result<(Value, usize), Fail> {
    if depth > MAX_DEPTH {
        return Err(error("YAML: nesting is too deep"));
    }

    let bytes = s.as_bytes();
    let mut pos = skip(s, pos, false);
    match bytes.get(pos) {
        Some(b'{') => {
            let mut out = Vec::new();
            pos += 1;
            let mut separated = true;
            loop {
                pos = skip(s, pos, false);
                let Some(&c) = bytes.get(pos) else {
                    return Err(Fail::Incomplete(
                        "YAML: unexpected end of the stream within a flow collection",
                    ));
                };
                if c == b'}' {
                    return Ok((Value::Object(out), pos + 1));
                }
                entry_start(c, separated)?;
                let (key, after) = flow(s, pos, ":,}", depth + 1)?;
                pos = skip(s, after, false);
                let mut value = Value::Null;
                if bytes.get(pos) == Some(&b':') {
                    (value, pos) = flow(s, pos + 1, ",}", depth + 1)?;
                }
                let mut name = String::new();
                key.push_js_string(&mut name);
                set(&mut out, name, value);
                pos = skip(s, pos, false);
                separated = bytes.get(pos) == Some(&b',');
                if separated {
                    pos += 1;
                }
            }
        }
        Some(b'[') => {
            let mut out = Vec::new();
            pos += 1;
            let mut separated = true;
            loop {
                pos = skip(s, pos, false);
                let Some(&c) = bytes.get(pos) else {
                    return Err(Fail::Incomplete(
                        "YAML: unexpected end of the stream within a flow collection",
                    ));
                };
                if c == b']' {
                    return Ok((Value::Array(out), pos + 1));
                }
                entry_start(c, separated)?;
                let (value, after) = flow(s, pos, ",]", depth + 1)?;
                out.push(value);
                pos = skip(s, after, false);
                separated = bytes.get(pos) == Some(&b',');
                if separated {
                    pos += 1;
                }
            }
        }
        Some(b'"') => {
            let mut end = pos + 1;
            while let Some(&c) = bytes.get(end)
                && c != b'"'
            {
                end += if c == b'\\' { 2 } else { 1 };
            }
            if end >= bytes.len() {
                return Err(Fail::Incomplete(
                    "YAML: unexpected end of the stream within a double quoted scalar",
                ));
            }
            let body = s.get(pos + 1..end).unwrap_or("");
            Ok((Value::String(unquote_double(body)?), end + 1))
        }
        Some(b'\'') => {
            let mut end = pos + 1;
            while end < bytes.len() {
                if bytes.get(end) == Some(&b'\'') {
                    if bytes.get(end + 1) != Some(&b'\'') {
                        break;
                    }
                    end += 1;
                }
                end += 1;
            }
            if end >= bytes.len() {
                return Err(Fail::Incomplete(
                    "YAML: unexpected end of the stream within a single quoted scalar",
                ));
            }
            let folded = fold(s.get(pos + 1..end).unwrap_or(""));
            let mut out = String::with_capacity(folded.len());
            let mut chars = folded.chars().peekable();
            while let Some(c) = chars.next() {
                out.push(c);
                if c == '\'' {
                    chars.next_if_eq(&'\'');
                }
            }
            Ok((Value::String(out), end + 1))
        }
        _ => {
            let mut end = pos;
            while let Some(&c) = bytes.get(end) {
                if stops.as_bytes().contains(&c)
                    && (c != b':'
                        || end + 1 >= bytes.len()
                        || next_is(s, end + 1, |n| space(n) || ",[]{}".contains(n)))
                {
                    break;
                }
                if c == b'#' && end > pos && previous_is_space(s, end) {
                    let raw = trim(s.get(pos..end).unwrap_or(""));
                    let after = s
                        .get(end..)
                        .and_then(|rest| rest.find('\n'))
                        .map_or(s.len(), |at| end + at);
                    return Ok((scalar(&fold(raw)), after));
                }
                end += 1;
            }
            let raw = trim(s.get(pos..end).unwrap_or(""));
            Ok((scalar(&fold(raw)), end))
        }
    }
}

fn strip_comment(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut quote = 0;
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        if quote != 0 {
            if c == b'\\' && quote == b'"' {
                i += 1;
            } else if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'#' && (i == 0 || matches!(bytes.get(i - 1), Some(b' ' | b'\t'))) {
            return s.get(..i).unwrap_or(s);
        }
        i += 1;
    }
    s
}

// /^[|>][-+0-9]*\s*(?:#.*)?$/
fn is_block_header(rest: &str) -> bool {
    let tail = rest
        .get(1..)
        .unwrap_or("")
        .trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit())
        .trim_start_matches(space);
    tail.is_empty() || (tail.starts_with('#') && !tail.chars().any(|c| is_line_end(u32::from(c))))
}

fn block_scalar(r: &mut Reader, header: &str, parent_indent: isize) -> String {
    let folded = header.starts_with('>');
    let chomp = if header.contains('-') {
        b'-'
    } else if header.contains('+') {
        b'+'
    } else {
        0
    };
    let explicit = header
        .chars()
        .find(|c| ('1'..='9').contains(c))
        .and_then(|c| c.to_digit(10));
    let mut indent = explicit.map_or(-1, |n| parent_indent + isize::try_from(n).unwrap_or(0));

    let mut out: Vec<String> = Vec::new();
    while r.i < r.lines.len() {
        let line = r.line();
        if trim(line).is_empty() {
            out.push(String::new());
            r.i += 1;
            continue;
        }
        let k = isize::try_from(leading(line)).unwrap_or(isize::MAX);
        if indent == -1 {
            if k <= parent_indent {
                break;
            }
            indent = k;
        }
        if k < indent {
            break;
        }
        out.push(after_chars(line, indent.unsigned_abs()).to_owned());
        r.i += 1;
    }

    let mut trailing = 0;
    while out.last().is_some_and(String::is_empty) {
        out.pop();
        trailing += 1;
    }

    let mut text = String::new();
    if folded {
        for (i, cur) in out.iter().enumerate() {
            if let Some(prev) = i.checked_sub(1).and_then(|p| out.get(p)) {
                if cur.is_empty() {
                    text.push('\n');
                } else if !prev.is_empty() {
                    let indented = |s: &str| s.starts_with([' ', '\t']);
                    text.push(if indented(prev) || indented(cur) {
                        '\n'
                    } else {
                        ' '
                    });
                }
            }
            text.push_str(cur);
        }
    } else {
        for (i, cur) in out.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            text.push_str(cur);
        }
    }

    if out.is_empty() {
        return if chomp == b'+' {
            "\n".repeat(trailing)
        } else {
            String::new()
        };
    }
    match chomp {
        b'-' => {}
        b'+' => text.push_str(&"\n".repeat(trailing + 1)),
        _ => text.push('\n'),
    }
    text
}

fn inline_value(r: &mut Reader, rest: &str, indent: isize) -> Result<Value, Fail> {
    if rest.starts_with(['|', '>']) && is_block_header(rest) {
        return Ok(Value::String(block_scalar(r, rest, indent)));
    }

    if rest.starts_with(['{', '[', '"', '\'']) {
        // The value may continue on later lines. Read them in growing batches until it closes.
        let start = r.i;
        let mut text = rest.to_owned();
        let mut taken = 0;
        let mut batch = 1;
        loop {
            match flow(&text, 0, "", 0) {
                Ok((value, end)) => {
                    let head = text.get(..end).unwrap_or("");
                    let used = head.matches('\n').count();
                    let tail = text.get(end..).unwrap_or("");
                    let line = tail
                        .find('\n')
                        .map_or(tail, |at| tail.get(..at).unwrap_or(""));
                    if !trim(strip_comment(line)).is_empty() {
                        return Err(error("YAML: unexpected content after a value"));
                    }
                    r.i = start + used;
                    return Ok(value);
                }
                Err(Fail::Incomplete(message)) => {
                    if start + taken >= r.lines.len() {
                        return Err(Fail::Error(message.to_owned()));
                    }
                    let to = (start + taken + batch).min(r.lines.len());
                    for line in r.lines.get(start + taken..to).unwrap_or(&[]) {
                        text.push('\n');
                        text.push_str(line);
                    }
                    taken += to - (start + taken);
                    batch *= 2;
                }
                Err(fail) => return Err(fail),
            }
        }
    }

    let mut text = trim(strip_comment(rest)).to_owned();
    let deeper = |line: &str| isize::try_from(leading(line)).unwrap_or(isize::MAX) > indent;
    while r.i < r.lines.len() {
        let line = r.line();
        if trim(line).is_empty() {
            let mut j = r.i;
            while r.lines.get(j).is_some_and(|line| trim(line).is_empty()) {
                j += 1;
            }
            if r.lines.get(j).is_none_or(|line| !deeper(line)) {
                break;
            }
            text.push_str(&"\n".repeat(j - r.i));
            r.i = j;
            continue;
        }
        if !deeper(line) {
            break;
        }
        let more = trim(strip_comment(line));
        if !text.ends_with('\n') {
            text.push(' ');
        }
        text.push_str(more);
        r.i += 1;
    }
    Ok(scalar(&text))
}

fn is_sequence_item(line: &str) -> bool {
    line == "-" || line.starts_with("- ")
}

fn block(r: &mut Reader, min_indent: usize, depth: usize) -> Result<Value, Fail> {
    if depth > MAX_DEPTH {
        return Err(error("YAML: nesting is too deep"));
    }

    r.skip_blank();
    if r.i >= r.lines.len() {
        return Ok(Value::Null);
    }
    let indent = r.indent()?;
    if indent < min_indent {
        return Ok(Value::Null);
    }
    let line = r.line().get(indent..).unwrap_or("").to_owned();
    let signed = isize::try_from(indent).unwrap_or(isize::MAX);

    if is_sequence_item(&line) {
        let mut out = Vec::new();
        while r.i < r.lines.len() {
            r.skip_blank();
            if r.i >= r.lines.len() || r.indent()? != indent {
                break;
            }
            let cur = r.line().get(indent..).unwrap_or("");
            if !is_sequence_item(cur) {
                break;
            }
            let rest = cur.get(1..).unwrap_or("").to_owned();
            let pad = leading(&rest) + 1;
            let body = trim(strip_comment(&rest));
            if body.is_empty() {
                r.i += 1;
                out.push(block(r, indent + 1, depth + 1)?);
            } else if split_key(body).is_some() {
                let mut rewritten = " ".repeat(indent + pad);
                rewritten.push_str(rest.trim_start_matches(space));
                if let Some(slot) = r.lines.get_mut(r.i) {
                    *slot = rewritten;
                }
                out.push(block(r, indent + pad, depth + 1)?);
            } else {
                r.i += 1;
                out.push(inline_value(r, trim(&rest), signed)?);
            }
        }
        return Ok(Value::Array(out));
    }

    if split_key(&line).is_none() {
        r.i += 1;
        return inline_value(r, trim(&line), signed - 1);
    }

    let mut out = Vec::new();
    while r.i < r.lines.len() {
        r.skip_blank();
        if r.i >= r.lines.len() {
            break;
        }
        let k = r.indent()?;
        if k < indent {
            break;
        }
        if k > indent {
            return Err(error("YAML: bad indentation of a mapping entry"));
        }
        let cur = r.line().get(indent..).unwrap_or("").to_owned();
        let Some((end, length)) = split_key(&cur) else {
            return Err(error("YAML: can not read a block mapping entry"));
        };
        let mut key = String::new();
        flow(cur.get(..end).unwrap_or(""), 0, "", 0)?
            .0
            .push_js_string(&mut key);
        let rest = cur.get(length..).unwrap_or("");
        r.i += 1;

        let value = if trim(strip_comment(rest)).is_empty() {
            r.skip_blank();
            if r.i < r.lines.len() {
                let next = r.indent()?;
                let seq = is_sequence_item(r.line().get(next..).unwrap_or(""));
                if next > indent || (seq && next == indent) {
                    block(r, if seq { indent } else { indent + 1 }, depth + 1)?
                } else {
                    Value::Null
                }
            } else {
                Value::Null
            }
        } else {
            inline_value(r, trim(rest), signed)?
        };
        set(&mut out, key, value);
    }
    Ok(Value::Object(out))
}

/// Parses `src`. Errors carry js-yaml's message.
pub(crate) fn parse(src: &str) -> Result<Value, String> {
    let trimmed = trim(src);
    if trimmed.starts_with(['{', '[']) {
        match flow(trimmed, 0, "", 0) {
            Ok((value, end)) => {
                if skip(trimmed, end, false) == trimmed.len() {
                    return Ok(value);
                }
            }
            Err(fail) => return Err(fail.into_message()),
        }
    }

    let mut r = Reader::new(src);
    let value = block(&mut r, 0, 0).map_err(Fail::into_message)?;
    r.skip_blank();
    if r.i < r.lines.len() && trim(r.line()) != "..." {
        return Err("YAML: end of the stream or a document separator is expected".to_owned());
    }
    Ok(value)
}

#[cfg(test)]
pub(crate) mod tests {
    //! Expectations come from running Pele's `parseYaml` and `String(number)` at 458322a.

    use super::{Value, parse, push_number};

    /// Appends `value` as `JSON.stringify` writes it.
    pub(crate) fn push_json(out: &mut String, value: &Value) {
        match value {
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_json(out, item);
                }
                out.push(']');
            }
            Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Value::Null => out.push_str("null"),
            Value::Number(value) if !value.is_finite() => out.push_str("null"),
            Value::Number(value) => push_number(out, *value),
            Value::Object(entries) => {
                out.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_json_string(out, key);
                    out.push(':');
                    push_json(out, value);
                }
                out.push('}');
            }
            Value::String(text) => push_json_string(out, text),
        }
    }

    /// Appends `text` as a JSON string literal, as `JSON.stringify` escapes it.
    pub(crate) fn push_json_string(out: &mut String, text: &str) {
        out.push('"');
        for c in text.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if u32::from(c) < 0x20 => {
                    out.push_str("\\u00");
                    out.push(char::from(b"0123456789abcdef"[usize::from(c as u8 >> 4)]));
                    out.push(char::from(b"0123456789abcdef"[usize::from(c as u8 & 15)]));
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }

    #[test]
    fn parse_matches_pele() {
        for &(src, expected) in CASES {
            let actual = match parse(src) {
                Ok(value) => {
                    let mut out = String::new();
                    push_json(&mut out, &value);
                    out
                }
                Err(message) => format!("ERR:{message}"),
            };
            assert_eq!(actual, expected, "{src:?}");
        }
    }

    #[test]
    fn push_number_matches_javascript() {
        for &(value, expected) in NUMBERS {
            let mut out = String::new();
            push_number(&mut out, value);
            assert_eq!(out, expected, "{value:e}");
        }
    }

    #[test]
    fn refuses_absurd_nesting() {
        let src = format!("{{ a: {} }}", "[".repeat(20000));
        assert_eq!(parse(&src), Err("YAML: nesting is too deep".to_owned()));
    }

    const CASES: &[(&str, &str)] = &[
        ("{\n shape: rounded \n}", "{\"shape\":\"rounded\"}"),
        (
            "{\nshape: rounded , label: \"DD\"\n}",
            "{\"shape\":\"rounded\",\"label\":\"DD\"}",
        ),
        (
            "{\n label: \"This is }\", other: \"clock\"\n}",
            "{\"label\":\"This is }\",\"other\":\"clock\"}",
        ),
        (
            "{\n animate: true, animation: fast, w: 100, h: 50.5, x: null, y: ~\n}",
            "{\"animate\":true,\"animation\":\"fast\",\"w\":100,\"h\":50.5,\"x\":null,\"y\":null}",
        ),
        (
            "{\n label: 'single quoted', icon: \"fa:fa-user\", form: circle, pos: t \n}",
            "{\"label\":\"single quoted\",\"icon\":\"fa:fa-user\",\"form\":\"circle\",\"pos\":\"t\"}",
        ),
        (
            "{\n a: [1, 2, three], b: {c: d, e: [f]}\n}",
            "{\"a\":[1,2,\"three\"],\"b\":{\"c\":\"d\",\"e\":[\"f\"]}}",
        ),
        ("{\n}", "{}"),
        (
            "\n        shape: circle\n        other: \"clock\"\n     \n",
            "{\"shape\":\"circle\",\"other\":\"clock\"}",
        ),
        (
            "\n        label: |\n          This is a\n          multiline string\n        other: \"clock\"\n     \n",
            "{\"label\":\"This is a\\nmultiline string\\n\",\"other\":\"clock\"}",
        ),
        (
            "\n        label: >\n          folded\n          text\n\n          second paragraph\n        other: 1\n",
            "{\"label\":\"folded text\\nsecond paragraph\\n\",\"other\":1}",
        ),
        (
            "\n        label: \"This is a<br/>multiline string\"\n        other: \"clock\"\n",
            "{\"label\":\"This is a<br/>multiline string\",\"other\":\"clock\"}",
        ),
        (
            "title: Hello world\nconfig:\n  theme: dark\n  flowchart:\n    curve: linear\n    htmlLabels: false\n",
            "{\"title\":\"Hello world\",\"config\":{\"theme\":\"dark\",\"flowchart\":{\"curve\":\"linear\",\"htmlLabels\":false}}}",
        ),
        (
            "title: \"Quoted: title\"\ndisplayMode: compact\n",
            "{\"title\":\"Quoted: title\",\"displayMode\":\"compact\"}",
        ),
        (
            "config:\n  themeVariables:\n    primaryColor: \"#ff0000\"\n    fontSize: 16px\n  look: handDrawn # trailing comment\n",
            "{\"config\":{\"themeVariables\":{\"primaryColor\":\"#ff0000\",\"fontSize\":\"16px\"},\"look\":\"handDrawn\"}}",
        ),
        (
            "list:\n  - a\n  - b: 1\n    c: 2\n  - [x, y]\nempty:\n",
            "{\"list\":[\"a\",{\"b\":1,\"c\":2},[\"x\",\"y\"]],\"empty\":null}",
        ),
        (
            "a: 0x1f\nb: -12\nc: 1e3\nd: .5\ne: \"12\"\nf: True\ng: FALSE\nh: Null\n",
            "{\"a\":31,\"b\":-12,\"c\":1000,\"d\":0.5,\"e\":\"12\",\"f\":true,\"g\":false,\"h\":null}",
        ),
        (
            "key with spaces: plain value\nurl: http://example.com/a#b\n",
            "{\"key with spaces\":\"plain value\",\"url\":\"http://example.com/a#b\"}",
        ),
        (
            "multi: this is\n  continued on the next line\nnext: 1\n",
            "{\"multi\":\"this is continued on the next line\",\"next\":1}",
        ),
        ("# only a comment\n", "null"),
        ("", "null"),
        ("just a scalar", "\"just a scalar\""),
        ("- 1\n- 2\n", "[1,2]"),
        (
            "a: {\n  b: 1,\n  c: [2,\n    3]\n}\nd: \"two\n  lines\"\ne: 5\n",
            "{\"a\":{\"b\":1,\"c\":[2,3]},\"d\":\"two lines\",\"e\":5}",
        ),
        ("{\n a: 1, # comment\n b: 2\n}", "{\"a\":1,\"b\":2}"),
        (
            "__proto__: 1\nb:\n  __proto__: {x: 1}\n",
            "{\"__proto__\":1,\"b\":{\"__proto__\":{\"x\":1}}}",
        ),
        (
            "{ __proto__: { polluted: true } }",
            "{\"__proto__\":{\"polluted\":true}}",
        ),
        (
            "{ a: [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[ }",
            "ERR:YAML: nesting is too deep",
        ),
        (
            "{ a: 1",
            "ERR:YAML: unexpected end of the stream within a flow collection",
        ),
        (
            "a: {\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n b: 1,\n",
            "ERR:YAML: unexpected end of the stream within a flow collection",
        ),
        (
            "{1.50: a, 1e21: b, 0.000001: c, 1e-7: d, 123456789012345678901234: e, 0x1F: f, -0: g, .inf: h, .nan: i, 5e-324: j, 1.7976931348623157e308: k, 0.1: l, 100: m, 1e20: n, 123.456: o}",
            "{\"0\":\"g\",\"31\":\"f\",\"100\":\"m\",\"1.5\":\"a\",\"1e+21\":\"b\",\"0.000001\":\"c\",\"1e-7\":\"d\",\"1.2345678901234569e+23\":\"e\",\"Infinity\":\"h\",\"NaN\":\"i\",\"5e-324\":\"j\",\"1.7976931348623157e+308\":\"k\",\"0.1\":\"l\",\"100000000000000000000\":\"n\",\"123.456\":\"o\"}",
        ),
        (
            "b: 1\n2: x\n1: y\na: z\n10: w\n01: v\n4294967295: big\n4294967294: max\n-1: neg",
            "{\"1\":\"v\",\"2\":\"x\",\"10\":\"w\",\"4294967294\":\"max\",\"b\":1,\"a\":\"z\",\"4294967295\":\"big\",\"-1\":\"neg\"}",
        ),
        (
            "a: \"\\x41\\u00e9\\U0001F600\\ud83d\\ude00\\t\\N\\_\\L\\P\\0\\a\\b\\v\\f\\r\\e\\ \\/\\\\\\\"\"",
            "{\"a\":\"Aé😀😀\\t\u{85}\u{a0}\u{2028}\u{2029}\\u0000\\u0007\\b\\u000b\\f\\r\\u001b /\\\\\\\"\"}",
        ),
        ("a: \"\\q\"", "ERR:YAML: unknown escape sequence"),
        ("a: \"\\xZZ\"", "ERR:Invalid code point NaN"),
        ("a: \"\\U00110000\"", "ERR:Invalid code point 1114112"),
        ("a: \"\\x\"", "ERR:Invalid code point NaN"),
        ("a: \"\\u 12z\"", "{\"a\":\"\\u0012\"}"),
        ("a: \"\\u-001\"", "ERR:Invalid code point -1"),
        ("a: \"\\x0x\"", "ERR:Invalid code point NaN"),
        ("a: \"line\\\n    joined\"", "{\"a\":\"linejoined\"}"),
        (
            "a: \"fold\n\n  keep   \n  two\"",
            "{\"a\":\"fold\\nkeep two\"}",
        ),
        (
            "a: 'it''s'\nb: '''x'''\nc: 'multi\n  line'",
            "{\"a\":\"it's\",\"b\":\"'x'\",\"c\":\"multi line\"}",
        ),
        (
            "a: |-\n  x\n  y\n\nb: |+\n  x\n\n\nc: >2\n    x\n   y\nd: |\n\ne: 1",
            "{\"a\":\"x\\ny\",\"b\":\"x\\n\\n\\n\",\"c\":\"  x\\n y\\n\",\"d\":\"\",\"e\":1}",
        ),
        ("a: | # 3 items\n     x\n    y\n", "{\"a\":\"  x\\n y\\n\"}"),
        (
            "a: >\n  folded\n   indented\n  back\n\n  para\n",
            "{\"a\":\"folded\\n indented\\nback\\npara\\n\"}",
        ),
        ("a: |\n", "{\"a\":\"\"}"),
        ("a: |+\n\n\n", "{\"a\":\"\\n\\n\\n\"}"),
        ("a: >-\n  x\n", "{\"a\":\"x\"}"),
        ("a: >+\n  x\n\n", "{\"a\":\"x\\n\\n\\n\"}"),
        ("a: |x\n  y\n", "{\"a\":\"|x y\"}"),
        (
            "a:\n\tb: 1",
            "ERR:YAML: tabs are not allowed for indentation",
        ),
        ("\ta: 1", "ERR:YAML: tabs are not allowed for indentation"),
        ("a: 1\n  b: 2", "{\"a\":\"1 b: 2\"}"),
        ("a: 1\nb", "ERR:YAML: can not read a block mapping entry"),
        ("a: 1\n b: 2\n", "{\"a\":\"1 b: 2\"}"),
        (
            "  a: 1\nb: 2",
            "ERR:YAML: end of the stream or a document separator is expected",
        ),
        (
            "- a\n- - b\n  - c\n-\n  x: 1\n- y: 2\n  z: 3",
            "[\"a\",\"- b - c\",{\"x\":1},{\"y\":2,\"z\":3}]",
        ),
        (
            "a: [1, 2",
            "ERR:YAML: unexpected end of the stream within a flow collection",
        ),
        ("{a: 1}}", "ERR:YAML: unexpected content after a value"),
        (
            "[1,,2]",
            "ERR:YAML: expected the node content, but found ','",
        ),
        ("[,1]", "ERR:YAML: expected the node content, but found ','"),
        ("[1 2]", "[\"1 2\"]"),
        ("{a: 1 b: 2}", "{\"a\":\"1 b: 2\"}"),
        ("{a: 1 b}", "{\"a\":\"1 b\"}"),
        ("[a, b c]", "[\"a\",\"b c\"]"),
        (
            "[1]\n[2]",
            "ERR:YAML: end of the stream or a document separator is expected",
        ),
        ("[1] # c", "[1]"),
        ("[1] x", "ERR:YAML: unexpected content after a value"),
        ("a: [1] x", "ERR:YAML: unexpected content after a value"),
        ("a: [1] # ok", "{\"a\":[1]}"),
        ("a: \"x\" y", "ERR:YAML: unexpected content after a value"),
        ("key: 'v' x", "ERR:YAML: unexpected content after a value"),
        ("key: value # comment", "{\"key\":\"value\"}"),
        ("key: \"value\" # c", "{\"key\":\"value\"}"),
        ("a: &anchor 1", "{\"a\":\"&anchor 1\"}"),
        (
            "---\na: 1\n...\n",
            "ERR:YAML: can not read a block mapping entry",
        ),
        ("a: 1\n---\nb: 2", "{\"a\":1,\"b\":2}"),
        (
            "a: 1\n...\nb: 2",
            "ERR:YAML: can not read a block mapping entry",
        ),
        ("a: b: c", "{\"a\":\"b: c\"}"),
        ("a:b", "\"a:b\""),
        ("a :b", "\"a :b\""),
        ("a : b", "{\"a\":\"b\"}"),
        (
            "\"quoted key\": 1\n'q''k': 2\n\"a\\\"b\": 3\n\"x\" : 4",
            "{\"quoted key\":1,\"q'k\":2,\"a\\\"b\":3,\"x\":4}",
        ),
        (
            "\"unterminated: 1",
            "ERR:YAML: unexpected end of the stream within a double quoted scalar",
        ),
        (
            "a: 0o17\nb: 0b101\nc: -0x10\nd: +12\ne: 012\nf: 1_000\ng: 1.5e+3\nh: -.inf\ni: .NaN\nj: 0x\nk: 1.\nl: +.5\nm: 9007199254740993\nn: 0xFFFFFFFFFFFFFFFFFFFF\no: -0\np: 0.1e-5\nq: 1E5\nr: +.inf\ns: .Inf\nt: 1e400\nu: 0O17\nv: 0X1",
            "{\"a\":15,\"b\":5,\"c\":-16,\"d\":12,\"e\":12,\"f\":\"1_000\",\"g\":1500,\"h\":null,\"i\":null,\"j\":\"0x\",\"k\":1,\"l\":0.5,\"m\":9007199254740992,\"n\":1.2089258196146292e+24,\"o\":0,\"p\":0.000001,\"q\":100000,\"r\":null,\"s\":null,\"t\":null,\"u\":\"0O17\",\"v\":\"0X1\"}",
        ),
        ("list:\n- a\n- b", "{\"list\":[\"a\",\"b\"]}"),
        (
            "a:\n  - b\n  -\n    c: d\n  - e: f\n    g: h",
            "{\"a\":[\"b\",{\"c\":\"d\"},{\"e\":\"f\",\"g\":\"h\"}]}",
        ),
        ("x: { a: [ { b: c } ] }", "{\"x\":{\"a\":[{\"b\":\"c\"}]}}"),
        ("a: [\n  1,\n  2\n]\nb: 3", "{\"a\":[1,2],\"b\":3}"),
        (
            "a: {\n  \"k\": 'v',\n  n: [a,\n b]}\nc: 1",
            "{\"a\":{\"k\":\"v\",\"n\":[\"a\",\"b\"]},\"c\":1}",
        ),
        (
            "tïtle: ünïcode ✓ 😀\nb: c",
            "{\"tïtle\":\"ünïcode ✓ 😀\",\"b\":\"c\"}",
        ),
        ("a: x\n\u{3000}b: c", "{\"a\":\"x b: c\"}"),
        ("a: x\n\u{a0} b: c\nd: 1", "{\"a\":\"x b: c\",\"d\":1}"),
        ("   ", "null"),
        ("#c", "null"),
        (
            "a: \"unterminated",
            "ERR:YAML: unexpected end of the stream within a double quoted scalar",
        ),
        (
            "a: 'unterminated",
            "ERR:YAML: unexpected end of the stream within a single quoted scalar",
        ),
        (
            "__proto__: 1\nconstructor: 2\nprototype: 3\ntoString: 4",
            "{\"__proto__\":1,\"constructor\":2,\"prototype\":3,\"toString\":4}",
        ),
        ("a: 1\na: 2\nb: 3", "{\"a\":2,\"b\":3}"),
        ("{\"a\": 1, \"a\": 2}", "{\"a\":2}"),
        ("- [a, b]\n- {c: d}", "[[\"a\",\"b\"],{\"c\":\"d\"}]"),
        ("-a: 1", "{\"-a\":1}"),
        ("- : x", "[\": x\"]"),
        ("-#: 1", "\"-#: 1\""),
        ("-", "[null]"),
        ("- ", "[null]"),
        ("-\n-", "[null,null]"),
        ("a: -", "{\"a\":\"-\"}"),
        ("a: - b", "{\"a\":\"- b\"}"),
        (
            "a: x #c\n  more # d\n\n\n  tail",
            "{\"a\":\"x more\\n\\ntail\"}",
        ),
        ("a: x\n\n\nb: y", "{\"a\":\"x\",\"b\":\"y\"}"),
        ("a: x\n\n   \n  y", "{\"a\":\"x\\n\\ny\"}"),
        ("{a: b #c\n, d: e}", "{\"a\":\"b\",\"d\":\"e\"}"),
        (
            "{a: \"b\"#c\n}",
            "ERR:YAML: missed comma between flow collection entries",
        ),
        (
            "[a:b, c: d, e :f, g:]",
            "[\"a:b\",\"c: d\",\"e :f\",\"g:\"]",
        ),
        ("{a:b}", "{\"a:b\":null}"),
        ("{a: [b:c]}", "{\"a\":[\"b:c\"]}"),
        ("{: x}", "{\"null\":\"x\"}"),
        ("{a}", "{\"a\":null}"),
        ("{a, b: c}", "{\"a\":null,\"b\":\"c\"}"),
        ("[{a: 1}, {b: 2}]", "[{\"a\":1},{\"b\":2}]"),
        (
            "{\"key with: colon\": \"v\"}",
            "{\"key with: colon\":\"v\"}",
        ),
        ("{a: 1}\n", "{\"a\":1}"),
        ("  {a: 1}  ", "{\"a\":1}"),
        ("[1, [2, [3]]]\n# c", "[1,[2,[3]]]"),
        ("[1]\n...", "[1]"),
        ("a: 1 # c\n# full\nb: 2", "{\"a\":1,\"b\":2}"),
        ("a:    \n  b: 1", "{\"a\":{\"b\":1}}"),
        ("a: # c\n  b: 1", "{\"a\":{\"b\":1}}"),
        ("a:\nb: 1", "{\"a\":null,\"b\":1}"),
        ("a:\n- 1\n- 2\nb: 3", "{\"a\":[1,2],\"b\":3}"),
        (
            "a:\n  - 1\n - 2",
            "ERR:YAML: bad indentation of a mapping entry",
        ),
        ("a: 'x\n\n\n  y'", "{\"a\":\"x\\n\\ny\"}"),
        ("a: \"  lead  \n  trail  \"", "{\"a\":\"  lead trail  \"}"),
        ("a: \"\\\n\"", "{\"a\":\"\"}"),
        ("a: \"x\\\\\ny\"", "ERR:YAML: unknown escape sequence"),
        (
            "a: true\nb: TRUE\nc: tRUE\nd: yes\ne: on\nf: ~\ng: null\nh: Null\ni: nULL",
            "{\"a\":true,\"b\":true,\"c\":\"tRUE\",\"d\":\"yes\",\"e\":\"on\",\"f\":null,\"g\":null,\"h\":null,\"i\":\"nULL\"}",
        ),
        (
            "a: 1.0\nb: 1.5\nc: 100\nd: 1e2\ne: -1.5e-10\nf: 0.000001\ng: 0.0000001\nh: 123456789012345680000\ni: 1234567890123456789012",
            "{\"a\":1,\"b\":1.5,\"c\":100,\"d\":100,\"e\":-1.5e-10,\"f\":0.000001,\"g\":1e-7,\"h\":123456789012345680000,\"i\":1.2345678901234568e+21}",
        ),
    ];

    #[expect(
        clippy::unreadable_literal,
        reason = "written as JavaScript's toExponential prints them"
    )]
    const NUMBERS: &[(f64, &str)] = &[
        (0e0, "0"),
        (-0.0, "0"),
        (1e0, "1"),
        (-1e0, "-1"),
        (1e-1, "0.1"),
        (5e-1, "0.5"),
        (1.5e0, "1.5"),
        (1e2, "100"),
        (1e20, "100000000000000000000"),
        (1e21, "1e+21"),
        (1.5e21, "1.5e+21"),
        (1.2345678901234568e20, "123456789012345680000"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (1.234e-7, "1.234e-7"),
        (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
        (9.007199254740992e15, "9007199254740992"),
        (9.007199254740994e15, "9007199254740994"),
        (3.0000000000000004e-1, "0.30000000000000004"),
        (3.333333333333333e-1, "0.3333333333333333"),
        (1.23456e2, "123.456"),
        (4.35e0, "4.35"),
        (1.234e-6, "0.000001234"),
        (9.95e0, "9.95"),
        (1e100, "1e+100"),
        (2.5e-5, "0.000025"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
        (2.99792458e8, "299792458"),
        (6.02214076e23, "6.02214076e+23"),
        (1.0000000000000002e0, "1.0000000000000002"),
    ];
}
