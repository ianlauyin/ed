//! The geometry of Mermaid's Cynefin boundaries: seeded wavy lines between the domains and the
//! S-shaped cliff between Clear and Chaotic. Curves are flat lists of numbers: a start point,
//! then six numbers (two control points and an end point) per cubic segment. Port of Pele's
//! `src/diagrams/cynefin/boundaries.ts`.

use crate::svg::builder::push_num;

/// JavaScript's `ToInt32`, as `x | 0` applies it, as the bits of a `u32`.
fn to_int32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    let truncated = value.trunc().rem_euclid(4_294_967_296.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is in [0, 2^32)"
    )]
    let bits = truncated as u32;
    bits
}

/// mulberry32, one step.
pub(crate) fn seeded_random(seed: f64) -> f64 {
    let mut t = to_int32(seed + f64::from(0x6d2b_79f5_u32));
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3d));
    f64::from(t ^ (t >> 14)) / 4_294_967_296.0
}

/// Java's string hash over UTF-16 units, as a signed 32-bit integer.
pub(crate) fn hash_string(text: &str) -> f64 {
    let mut hash: u32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(unit));
    }
    f64::from(hash.cast_signed())
}

/// A configured seed other than zero wins; otherwise the seed comes from the diagram's id.
pub(crate) fn resolve_seed(configured: f64, id: &str) -> f64 {
    if configured.is_finite() && configured != 0.0 {
        return configured;
    }
    hash_string(id)
}

/// How a wavy line runs: Pele's `WaveOptions`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaveOptions {
    /// Seed multiplier per control point.
    pub(crate) control_shift: f64,
    /// Seed offset of control points.
    pub(crate) control_step: f64,
    /// Whether the end stays on the center line.
    pub(crate) pin_end: bool,
    /// Whether the start stays on the center line.
    pub(crate) pin_start: bool,
    /// Seed multiplier per point.
    pub(crate) point_step: f64,
    /// The number of cubic segments.
    pub(crate) segments: u32,
    /// Whether the line runs down rather than across.
    pub(crate) vertical: bool,
}

/// The vertical boundary between the top quadrants.
pub(crate) const FOLD: WaveOptions = WaveOptions {
    control_shift: 7.0,
    control_step: 31.0,
    pin_end: false,
    pin_start: false,
    point_step: 17.0,
    segments: 7,
    vertical: true,
};

/// The horizontal boundaries between the top and bottom quadrants.
pub(crate) const HORIZON: WaveOptions = WaveOptions {
    control_shift: 11.0,
    control_step: 37.0,
    pin_end: false,
    pin_start: false,
    point_step: 23.0,
    segments: 7,
    vertical: false,
};

/// A wavy line along one axis from `from` to `to`, centered on `center` in the other axis.
/// Points jitter by up to the amplitude; a pinned end stays on the center line.
pub(crate) fn wave(
    from: f64,
    to: f64,
    center: f64,
    seed: f64,
    amplitude: f64,
    o: WaveOptions,
) -> Vec<f64> {
    let segments = f64::from(o.segments);
    let step = (to - from) / segments;
    let mut along = Vec::new();
    let mut across = Vec::new();
    for i in 0..=o.segments {
        let k = f64::from(i);
        let pinned = (i == 0 && o.pin_start) || (i == o.segments && o.pin_end);
        along.push(from + k * step);
        across.push(if pinned {
            center
        } else {
            center + seeded_random(seed + k * o.point_step) * amplitude * 2.0 - amplitude
        });
    }

    let at = |list: &[f64], i: usize| list.get(i).copied().unwrap_or(0.0);
    let mut out = if o.vertical {
        vec![at(&across, 0), at(&along, 0)]
    } else {
        vec![at(&along, 0), at(&across, 0)]
    };
    for i in 0..o.segments {
        let k = f64::from(i);
        let j = usize::try_from(i).unwrap_or(0);
        let mid = f64::midpoint(at(&along, j), at(&along, j + 1));
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let offset =
            amplitude * 1.5 * sign * seeded_random(seed + k * o.control_step + o.control_shift);
        if o.vertical {
            out.extend([
                at(&across, j) + offset,
                mid,
                at(&across, j + 1) - offset,
                mid,
                at(&across, j + 1),
                at(&along, j + 1),
            ]);
        } else {
            out.extend([
                mid,
                at(&across, j) + offset,
                mid,
                at(&across, j + 1) - offset,
                at(&along, j + 1),
                at(&across, j + 1),
            ]);
        }
    }
    out
}

/// The cliff: a steep S from (`cx`, `top`) down to (`cx`, `bottom`).
pub(crate) fn cliff(cx: f64, top: f64, bottom: f64, amplitude: f64) -> Vec<f64> {
    let h = bottom - top;
    vec![
        cx,
        top,
        cx + amplitude,
        top + h * 0.2,
        cx - amplitude * 1.5,
        top + h * 0.55,
        cx + amplitude * 0.5,
        top + h * 0.75,
        cx - amplitude,
        top + h * 0.85,
        cx + amplitude * 0.3,
        top + h * 0.95,
        cx,
        bottom,
    ]
}

/// Appends `x,y`.
fn push_pair(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Appends path commands that continue from the curve's start point to its end.
pub(crate) fn forward(out: &mut String, curve: &[f64]) {
    for segment in curve.get(2..).unwrap_or(&[]).chunks_exact(6) {
        if let [ax, ay, bx, by, x, y] = *segment {
            out.push('C');
            push_pair(out, ax, ay);
            out.push(' ');
            push_pair(out, bx, by);
            out.push(' ');
            push_pair(out, x, y);
        }
    }
}

/// Appends path commands that continue from the curve's end point back to its start.
pub(crate) fn backward(out: &mut String, curve: &[f64]) {
    let points = curve.len() / 2;
    let point = |k: usize| {
        (
            curve.get(2 * k).copied().unwrap_or(0.0),
            curve.get(2 * k + 1).copied().unwrap_or(0.0),
        )
    };
    let mut k = points.saturating_sub(1);
    while k >= 3 {
        let (bx, by) = point(k - 1);
        let (ax, ay) = point(k - 2);
        let (x, y) = point(k - 3);
        out.push('C');
        push_pair(out, bx, by);
        out.push(' ');
        push_pair(out, ax, ay);
        out.push(' ');
        push_pair(out, x, y);
        k -= 3;
    }
}

/// The path of a curve.
pub(crate) fn curve_path(curve: &[f64]) -> String {
    let mut out = String::from("M");
    push_pair(
        &mut out,
        curve.first().copied().unwrap_or(0.0),
        curve.get(1).copied().unwrap_or(0.0),
    );
    forward(&mut out, curve);
    out
}

/// The path of an ellipse centered on (`cx`, `cy`).
pub(crate) fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    let mut r = String::new();
    push_pair(&mut r, rx, ry);
    let mut out = String::from("M");
    push_pair(&mut out, cx - rx, cy);
    out.push('A');
    out.push_str(&r);
    out.push_str(" 0 1,1 ");
    push_pair(&mut out, cx + rx, cy);
    out.push('A');
    out.push_str(&r);
    out.push_str(" 0 1,1 ");
    push_pair(&mut out, cx - rx, cy);
    out.push('Z');
    out
}
