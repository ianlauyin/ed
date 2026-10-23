//! The parts of d3-format's specifier that treemap values use: an optional `$`, `,` for
//! thousands separators, a precision, and a type of `f`, `%`, `d`, `e` or none. Fill, alignment
//! and width are read and ignored. Anything else falls back to the default, `,`. Port of Pele's
//! `src/diagrams/treemap/format.ts`, with JavaScript's `toFixed`, `toExponential`, and
//! `toPrecision` written out over the exact decimal value of a double.

use crate::svg::builder::js_round;
use crate::util::chars::is_line_end;
use crate::yaml::push_number;

/// A parsed specifier: the groups of Pele's `RE_SPEC` that change the output.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the flags of a format specifier"
)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Format {
    /// Whether thousands are separated by commas.
    comma: bool,
    /// Whether a `$` goes before the number.
    currency: bool,
    /// Whether a `+` goes before a number that is not negative.
    plus: bool,
    /// Digits after the point or significant digits, at most 20.
    precision: Option<u32>,
    /// Whether trailing zeros are trimmed.
    trim: bool,
    /// The type character, or `None`.
    r#type: Option<char>,
}

/// Reads `^([-+( ])?(\$)?0?\d*(,)?(?:\.(\d+))?(~)?([a-zA-Z%])?$` from `rest`, which follows the
/// optional fill and alignment.
fn tail(rest: &str) -> Option<Format> {
    let bytes = rest.as_bytes();
    let mut at = 0;
    let mut format = Format::default();

    if let Some(&sign) = bytes.get(at)
        && matches!(sign, b'-' | b'+' | b'(' | b' ')
    {
        format.plus = sign == b'+';
        at += 1;
    }
    if bytes.get(at) == Some(&b'$') {
        format.currency = true;
        at += 1;
    }
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    if bytes.get(at) == Some(&b',') {
        format.comma = true;
        at += 1;
    }
    if bytes.get(at) == Some(&b'.') {
        let from = at + 1;
        let mut end = from;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == from {
            return None;
        }
        // `Math.min(Number(digits), 20)`: past its leading zeros, a run of three digits is
        // above 20.
        let digits = rest.get(from..end).unwrap_or("").trim_start_matches('0');
        format.precision = Some(if digits.len() > 2 {
            20
        } else {
            digits.parse::<u32>().unwrap_or(0).min(20)
        });
        at = end;
    }
    if bytes.get(at) == Some(&b'~') {
        format.trim = true;
        at += 1;
    }
    if let Some(&c) = bytes.get(at)
        && (c.is_ascii_alphabetic() || c == b'%')
    {
        format.r#type = Some(char::from(c));
        at += 1;
    }
    (at == bytes.len()).then_some(format)
}

/// A d3-format alignment character.
fn is_align(c: char) -> bool {
    matches!(c, '<' | '>' | '=' | '^')
}

/// Reads a specifier as `RE_SPEC` does: `^(?:.?[<>=^])?` and then [`tail`]. The fill is one
/// UTF-16 unit that is not a line terminator, so a character outside the BMP cannot be one.
fn spec(text: &str) -> Option<Format> {
    let mut chars = text.chars();
    let first = chars.next();
    let second = chars.next();

    if let (Some(fill), Some(align)) = (first, second)
        && fill.len_utf16() == 1
        && !is_line_end(u32::from(fill))
        && is_align(align)
        && let Some(format) = tail(text.get(fill.len_utf8() + 1..).unwrap_or(""))
    {
        return Some(format);
    }
    if let Some(align) = first
        && is_align(align)
        && let Some(format) = tail(text.get(1..).unwrap_or(""))
    {
        return Some(format);
    }
    tail(text)
}

/// Reads `spec` for [`format`]. Mermaid documents `$0,0` for a dollar amount with thousands
/// separators.
pub(crate) fn formatter(text: &str) -> Format {
    let text = if text == "$0,0" { "$," } else { text };
    spec(text).or_else(|| spec(",")).unwrap_or_default()
}

/// The base of the limbs of the exact decimal expansion.
const BASE: u64 = 1_000_000_000;

/// Multiplies base-10^9 limbs by `factor`.
fn multiply(limbs: &mut Vec<u64>, factor: u64) {
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
}

/// The exact decimal digits of a finite positive `value` and the point position `n`, so that
/// `value` is `0.digits * 10^n`.
fn exact(value: f64) -> (Vec<u8>, i32) {
    let bits = value.to_bits();
    let biased = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let fraction = bits & ((1 << 52) - 1);
    let (mantissa, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), biased - 1075)
    };

    let mut limbs = vec![
        mantissa % BASE,
        (mantissa / BASE) % BASE,
        mantissa / BASE / BASE,
    ];
    let (mut count, base, chunk) = if exponent >= 0 {
        (exponent.unsigned_abs(), 2u64, 29)
    } else {
        (exponent.unsigned_abs(), 5u64, 13)
    };
    while count > 0 {
        let step = count.min(chunk);
        multiply(&mut limbs, base.pow(step));
        count -= step;
    }

    let mut digits = Vec::new();
    for &limb in limbs.iter().rev() {
        let mut chunk = [b'0'; 9];
        let mut rest = limb;
        for slot in chunk.iter_mut().rev() {
            *slot = b'0' + u8::try_from(rest % 10).unwrap_or(0);
            rest /= 10;
        }
        digits.extend_from_slice(&chunk);
    }
    let lead = digits.iter().take_while(|&&d| d == b'0').count();
    digits.drain(..lead);
    let length = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let n = if exponent >= 0 {
        length
    } else {
        length + exponent
    };
    (digits, n)
}

/// The first `keep` of `digits` (padded with zeros), rounded half up by the digit after them,
/// and the point position, which a carry moves up by one and lengthens the digits.
fn round(digits: &[u8], n: i32, keep: i32) -> (Vec<u8>, i32) {
    let Ok(keep) = usize::try_from(keep) else {
        return (Vec::new(), n);
    };
    let mut out: Vec<u8> = digits.iter().copied().take(keep).collect();
    out.resize(keep, b'0');
    if digits.get(keep).is_none_or(|&d| d < b'5') {
        return (out, n);
    }
    for digit in out.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return (out, n);
        }
    }
    out.insert(0, b'1');
    (out, n + 1)
}

/// Appends ASCII digits.
fn push_digits(out: &mut String, digits: &[u8]) {
    for &digit in digits {
        out.push(char::from(digit));
    }
}

/// Appends `e+n` or `e-n`, as JavaScript writes an exponent.
fn push_exponent(out: &mut String, e: i32) {
    out.push('e');
    out.push(if e >= 0 { '+' } else { '-' });
    push_number(out, f64::from(e.unsigned_abs()));
}

/// `value.toFixed(digits)` for a value that is not negative.
fn to_fixed(value: f64, digits: u32) -> String {
    let mut out = String::new();
    if !value.is_finite() || value >= 1e21 {
        push_number(&mut out, value);
        return out;
    }
    let f = usize::try_from(digits).unwrap_or(0);
    let mut integer = if value == 0.0 {
        Vec::new()
    } else {
        let (exact, n) = exact(value);
        round(&exact, n, n + i32::try_from(digits).unwrap_or(0)).0
    };
    if integer.len() <= f {
        let mut padded = vec![b'0'; f + 1 - integer.len()];
        padded.append(&mut integer);
        integer = padded;
    }
    let point = integer.len() - f;
    push_digits(&mut out, integer.get(..point).unwrap_or(&[]));
    if f > 0 {
        out.push('.');
        push_digits(&mut out, integer.get(point..).unwrap_or(&[]));
    }
    out
}

/// `value` rounded to `p` significant digits, and its decimal exponent.
fn significant(value: f64, p: u32) -> (Vec<u8>, i32) {
    let keep = usize::try_from(p).unwrap_or(1);
    if value == 0.0 {
        return (vec![b'0'; keep], 0);
    }
    let (exact, n) = exact(value);
    let (mut digits, n) = round(&exact, n, i32::try_from(p).unwrap_or(1));
    digits.truncate(keep);
    (digits, n - 1)
}

/// `value.toExponential(digits)` for a finite value that is not negative.
fn to_exponential(value: f64, digits: u32) -> String {
    let mut out = String::new();
    if !value.is_finite() {
        push_number(&mut out, value);
        return out;
    }
    let (digits, e) = significant(value, digits + 1);
    push_digits(&mut out, digits.get(..1).unwrap_or(&[]));
    if digits.len() > 1 {
        out.push('.');
        push_digits(&mut out, digits.get(1..).unwrap_or(&[]));
    }
    push_exponent(&mut out, e);
    out
}

/// `value.toPrecision(p)` for a finite value that is not negative.
fn to_precision(value: f64, p: u32) -> String {
    let mut out = String::new();
    if !value.is_finite() {
        push_number(&mut out, value);
        return out;
    }
    let (digits, e) = significant(value, p);
    let p = i32::try_from(p).unwrap_or(1);
    if e < -6 || e >= p {
        push_digits(&mut out, digits.get(..1).unwrap_or(&[]));
        if digits.len() > 1 {
            out.push('.');
            push_digits(&mut out, digits.get(1..).unwrap_or(&[]));
        }
        push_exponent(&mut out, e);
        return out;
    }
    if e >= 0 {
        let point = usize::try_from(e + 1).unwrap_or(0);
        push_digits(&mut out, digits.get(..point).unwrap_or(&[]));
        if point < digits.len() {
            out.push('.');
            push_digits(&mut out, digits.get(point..).unwrap_or(&[]));
        }
        return out;
    }
    out.push_str("0.");
    for _ in 0..(-(e + 1)) {
        out.push('0');
    }
    push_digits(&mut out, &digits);
    out
}

/// `digits` with a comma before each group of three from the right.
fn group(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let count = digits.chars().count();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (count - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `body.replace(/\.?0+$/, '')`: trailing zeros, and a point they leave last.
fn trim_zeros(body: &str) -> &str {
    let trimmed = body.trim_end_matches('0');
    if trimmed.len() == body.len() {
        return body;
    }
    trimmed.strip_suffix('.').unwrap_or(trimmed)
}

/// Writes `value` as `format` reads it.
pub(crate) fn format(format: &Format, value: f64) -> String {
    let abs = value.abs();
    let mut suffix = "";
    let mut body = match format.r#type {
        Some('f') => to_fixed(abs, format.precision.unwrap_or(6)),
        Some('%') => {
            suffix = "%";
            to_fixed(abs * 100.0, format.precision.unwrap_or(6))
        }
        Some('d') => to_fixed(js_round(abs), 0),
        Some('e') => to_exponential(abs, format.precision.unwrap_or(6)),
        _ => {
            let text = to_precision(abs, format.precision.unwrap_or(12).max(1));
            let mut out = String::new();
            push_number(&mut out, text.parse().unwrap_or(f64::NAN));
            out
        }
    };

    if format.trim && body.contains('.') && !body.contains('e') {
        body = trim_zeros(&body).to_owned();
    }
    if format.comma && !body.contains('e') {
        body = match body.find('.') {
            None => group(&body),
            Some(dot) => {
                let mut out = group(body.get(..dot).unwrap_or(""));
                out.push_str(body.get(dot..).unwrap_or(""));
                out
            }
        };
    }

    let mut out = String::with_capacity(body.len() + 3);
    if value < 0.0 {
        out.push('-');
    } else if format.plus {
        out.push('+');
    }
    if format.currency {
        out.push('$');
    }
    out.push_str(&body);
    out.push_str(suffix);
    out
}
