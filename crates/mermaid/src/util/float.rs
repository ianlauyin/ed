//! Reads decimal text as the nearest `f64`, accepting and rounding exactly as `str::parse::<f64>`.
//!
//! Core's reader brings about 14 kB of power tables and fast paths into every WebAssembly module.
//! This one takes Clinger's exact fast path when it applies and otherwise divides big integers,
//! which is slower but only runs for long or extreme numbers.

/// Significant digits kept. The nearest double is decided within 767 digits, and any nonzero
/// digit after the kept ones rounds as a sticky bit.
const MAX_DIGITS: usize = 800;

/// `text` as the nearest `f64` with ties to even, or `None` where `str::parse::<f64>` fails.
pub(crate) fn parse(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let (negative, rest) = match bytes.first() {
        Some(b'-') => (true, bytes.get(1..)?),
        Some(b'+') => (false, bytes.get(1..)?),
        _ => (false, bytes),
    };

    let value = decimal(rest).or_else(|| special(rest))?;
    Some(if negative { -value } else { value })
}

/// `inf`, `infinity`, and `nan` in any case, which core also reads.
fn special(text: &[u8]) -> Option<f64> {
    if text.eq_ignore_ascii_case(b"inf") || text.eq_ignore_ascii_case(b"infinity") {
        return Some(f64::INFINITY);
    }
    text.eq_ignore_ascii_case(b"nan").then_some(f64::NAN)
}

/// Unsigned `digits[.digits][e[+-]digits]`, with at least one mantissa digit.
fn decimal(text: &[u8]) -> Option<f64> {
    // The value is the integer `digits` times `10^exponent`.
    let mut digits = Vec::new();
    let mut exponent = 0_i64;
    let mut mantissa = 0;
    let mut point = false;
    let mut truncated = false;
    let mut at = 0;

    while let Some(&c) = text.get(at) {
        if c == b'.' && !point {
            point = true;
        } else if c.is_ascii_digit() {
            mantissa += 1;
            if digits.is_empty() && c == b'0' {
                exponent -= i64::from(point);
            } else if digits.len() < MAX_DIGITS {
                digits.push(c - b'0');
                exponent -= i64::from(point);
            } else {
                truncated |= c != b'0';
                exponent += i64::from(!point);
            }
        } else {
            break;
        }
        at += 1;
    }
    if mantissa == 0 {
        return None;
    }

    if matches!(text.get(at), Some(b'e' | b'E')) {
        at += 1;
        let sign = match text.get(at) {
            Some(b'-') => -1,
            Some(b'+') => 1,
            _ => 0,
        };
        at += usize::from(sign != 0);

        let start = at;
        let mut value = 0_i64;
        while let Some(&c) = text.get(at).filter(|c| c.is_ascii_digit()) {
            // Saturates far beyond any finite double's exponent.
            value = (value * 10 + i64::from(c - b'0')).min(1 << 40);
            at += 1;
        }
        if at == start {
            return None;
        }
        exponent += if sign < 0 { -value } else { value };
    }
    if at != text.len() {
        return None;
    }

    Some(nearest(&digits, exponent, truncated))
}

/// The double nearest `digits * 10^exponent`, just above it when `truncated`.
fn nearest(digits: &[u8], exponent: i64, truncated: bool) -> f64 {
    let count = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    if digits.is_empty() || count + exponent < -325 {
        return 0.0;
    }
    if count + exponent > 310 {
        return f64::INFINITY;
    }

    // Clinger's fast path: both operands are exact, so one IEEE operation rounds correctly.
    if !truncated && count <= 15 && exponent.abs() <= 22 {
        let mut value = 0_u64;
        for &digit in digits {
            value = value * 10 + u64::from(digit);
        }
        let mut power = 1.0;
        for _ in 0..exponent.abs() {
            power *= 10.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "fewer than 16 digits fit in 53 bits"
        )]
        let value = value as f64;
        return if exponent < 0 {
            value / power
        } else {
            value * power
        };
    }

    let mut number = Vec::new();
    for &digit in digits {
        mul_add(&mut number, 10, u32::from(digit));
    }

    if exponent >= 0 {
        for _ in 0..exponent {
            mul_add(&mut number, 10, 0);
        }
        let shift = (bits(&number) - 64).max(0);
        let dropped = shr(&mut number, shift);
        return round(low64(&number), shift, dropped || truncated);
    }

    // `number / 10^k` is `number / (5^k * 2^k)`. Scale the dividend so the quotient has 63 or 64
    // bits, then divide by shifting and subtracting.
    let k = -exponent;
    let mut divisor = vec![1];
    for _ in 0..k {
        mul_add(&mut divisor, 5, 0);
    }
    let scale = bits(&divisor) - bits(&number) + 63;
    if scale >= 0 {
        shl(&mut number, scale);
    } else {
        shl(&mut divisor, -scale);
    }

    shl(&mut divisor, 63);
    let mut quotient = 0_u64;
    for bit in (0..64).rev() {
        if !less(&number, &divisor) {
            sub(&mut number, &divisor);
            quotient |= 1 << bit;
        }
        shr(&mut divisor, 1);
    }

    round(quotient, -scale - k, !number.is_empty() || truncated)
}

/// The double nearest `value * 2^exponent`, just above it when `sticky`, with ties to even.
fn round(value: u64, exponent: i64, sticky: bool) -> f64 {
    if value == 0 {
        return 0.0;
    }

    let length = 64 - i64::from(value.leading_zeros());
    let last = (length + exponent - 53).max(-1074);
    let drop = (last - exponent).clamp(0, 127);
    let wide = u128::from(value);
    let mut mantissa = wide >> drop;
    if drop > 0 {
        let rest = wide & ((1 << drop) - 1);
        let half = 1 << (drop - 1);
        if rest > half || (rest == half && (sticky || mantissa & 1 == 1)) {
            mantissa += 1;
        }
    }

    // At most 2^53, so exact. Scaling by a power of two is exact until it overflows to infinity.
    #[expect(
        clippy::cast_precision_loss,
        reason = "the mantissa has at most 54 bits"
    )]
    let mantissa = mantissa as f64;
    let last = exponent + drop;
    let power = if last > 1023 {
        f64::INFINITY
    } else if last >= -1022 {
        f64::from_bits(u64::try_from(last + 1023).unwrap_or(0) << 52)
    } else {
        f64::from_bits(1 << (last + 1074).clamp(0, 63))
    };
    mantissa * power
}

/// Little-endian base-2^32 limbs with no high zero limb.
fn mul_add(number: &mut Vec<u32>, factor: u32, add: u32) {
    let mut carry = u64::from(add);
    for limb in number.iter_mut() {
        let product = u64::from(*limb) * u64::from(factor) + carry;
        *limb = low32(product);
        carry = product >> 32;
    }
    if carry > 0 {
        number.push(low32(carry));
    }
}

fn bits(number: &[u32]) -> i64 {
    let top = number
        .last()
        .map_or(0, |limb| 32 - i64::from(limb.leading_zeros()));
    i64::try_from(number.len().saturating_sub(1)).unwrap_or(0) * 32 + top
}

fn shl(number: &mut Vec<u32>, count: i64) {
    let count = usize::try_from(count).unwrap_or(0);
    let (limbs, offset) = (count / 32, count % 32);
    if offset > 0 {
        let mut carry = 0;
        for limb in number.iter_mut() {
            let wide = (u64::from(*limb) << offset) | carry;
            *limb = low32(wide);
            carry = wide >> 32;
        }
        if carry > 0 {
            number.push(low32(carry));
        }
    }
    if !number.is_empty() {
        number.splice(0..0, std::iter::repeat_n(0, limbs));
    }
}

/// Shifts right by `count` bits and reports whether any dropped bit was set.
fn shr(number: &mut Vec<u32>, count: i64) -> bool {
    let count = usize::try_from(count).unwrap_or(0);
    let (limbs, offset) = (count / 32, count % 32);
    let mut dropped = number.iter().take(limbs).any(|&limb| limb != 0);
    number.drain(..limbs.min(number.len()));

    if offset > 0 {
        dropped |= number
            .first()
            .is_some_and(|limb| limb & ((1 << offset) - 1) != 0);
        let mut carry = 0;
        for limb in number.iter_mut().rev() {
            let wide = (u64::from(*limb) << 32) >> offset;
            *limb = low32(wide >> 32) | carry;
            carry = low32(wide);
        }
    }
    trim(number);
    dropped
}

fn less(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return a.len() < b.len();
    }
    a.iter().rev().lt(b.iter().rev())
}

/// `a - b`, where `b` is at most `a`.
fn sub(a: &mut Vec<u32>, b: &[u32]) {
    let mut borrow = false;
    for (at, limb) in a.iter_mut().enumerate() {
        let (value, first) = limb.overflowing_sub(b.get(at).copied().unwrap_or(0));
        let (value, second) = value.overflowing_sub(u32::from(borrow));
        *limb = value;
        borrow = first || second;
    }
    trim(a);
}

fn trim(number: &mut Vec<u32>) {
    while number.last() == Some(&0) {
        number.pop();
    }
}

fn low64(number: &[u32]) -> u64 {
    number
        .iter()
        .take(2)
        .rev()
        .fold(0, |value, &limb| (value << 32) | u64::from(limb))
}

fn low32(value: u64) -> u32 {
    u32::try_from(value & 0xffff_ffff).unwrap_or(0)
}
