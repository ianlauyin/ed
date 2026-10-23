//! Tick steps and values for numeric axes. Port of Pele's `src/diagrams/xychart/ticks.ts`.

use crate::error::push_u32;
use crate::svg::builder::js_round;

/// `10 ** power` for a whole `power`, correctly rounded as V8 computes it.
fn pow10(power: f64) -> f64 {
    if power.is_nan() {
        return f64::NAN;
    }
    if power > 400.0 {
        return f64::INFINITY;
    }
    if power < -400.0 {
        return 0.0;
    }
    let mut text = String::from("1e");
    if power < 0.0 {
        text.push('-');
    }
    // The bounds above keep the magnitude a small whole number.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a whole number of at most 400"
    )]
    let magnitude = power.abs() as u32;
    push_u32(&mut text, magnitude);
    text.parse().unwrap_or(f64::NAN)
}

/// A step of 1, 2 or 5 times a power of ten that divides `span` into about `count` parts. A step
/// below one comes back as the negative of its inverse, which is a whole number, so that tick
/// values are found by division and print without rounding noise (0.3, not 0.30000000000000004).
pub(crate) fn tick_step(span: f64, count: f64) -> f64 {
    let raw = span / count.max(1.0);
    let power = raw.log10().floor();
    let unit = raw / pow10(power);
    let factor = if unit >= 7.07 {
        10.0
    } else if unit >= 3.16 {
        5.0
    } else if unit >= 1.41 {
        2.0
    } else {
        1.0
    };
    if power < 0.0 {
        -pow10(-power) / factor
    } else {
        factor * pow10(power)
    }
}

/// The index of `value` among the multiples of `step`.
pub(crate) fn tick_index(step: f64, value: f64) -> f64 {
    if step > 0.0 {
        value / step
    } else {
        value * -step
    }
}

/// The multiple `index` of `step`.
pub(crate) fn tick_value(step: f64, index: f64) -> f64 {
    if step > 0.0 {
        index * step
    } else {
        index / -step
    }
}

/// The multiples of a step from `lo` to `hi`, both included, at most 100 of them.
pub(crate) fn ticks_between(lo: f64, hi: f64, step: f64) -> Vec<f64> {
    let mut first = js_round(tick_index(step, lo));
    if tick_value(step, first) < lo {
        first += 1.0;
    }
    let mut last = js_round(tick_index(step, hi));
    if tick_value(step, last) > hi {
        last -= 1.0;
    }
    let mut out = Vec::new();
    let mut i = first;
    while i <= last && out.len() < 100 {
        out.push(tick_value(step, i));
        i += 1.0;
    }
    out
}
