//! Block arrows: their directions, sizes, and outlines. Port of Pele's
//! `src/diagrams/block/arrows.ts`.

use crate::svg::builder::push_num;

/// Points right.
pub(crate) const RIGHT: u32 = 1;
/// Points left.
pub(crate) const LEFT: u32 = 2;
/// Points up.
pub(crate) const UP: u32 = 4;
/// Points down.
pub(crate) const DOWN: u32 = 8;

/// How far a head reaches past each side of its shaft.
pub(crate) const OVERHANG: f64 = 7.0;
const MAX_DEPTH: f64 = 20.0;
const NUB: f64 = 12.0;

/// The size of an arrow around its label.
pub(crate) struct ArrowSize {
    /// Shift of the label from the center, away from the heads.
    pub(crate) dx: f64,
    /// Shift of the label from the center, away from the heads.
    pub(crate) dy: f64,
    /// Height.
    pub(crate) h: f64,
    /// Width.
    pub(crate) w: f64,
}

/// The direction bits of an arrow. Mermaid compares directions exactly, so `( up )` draws no
/// arrow there. Here spaces are ignored.
pub(crate) fn arrow_mask(directions: Option<&[String]>) -> u32 {
    let mut mask = 0;
    for direction in directions.unwrap_or(&[]) {
        mask |= match crate::text::label::trim(direction) {
            "right" => RIGHT,
            "left" => LEFT,
            "up" => UP,
            "down" => DOWN,
            "x" => RIGHT | LEFT,
            "y" => UP | DOWN,
            _ => 0,
        };
    }
    mask
}

fn bit(mask: u32, flag: u32) -> f64 {
    if mask & flag == 0 { 0.0 } else { 1.0 }
}

/// The axis an arrow points along: `x`, `y`, or neither. An arrow along one axis has a shaft and
/// heads wider than it. One that points along both axes is a box with a small point on each side
/// it names.
pub(crate) fn arrow_axis(mask: u32) -> u8 {
    if mask != 0 && mask & (UP | DOWN) == 0 {
        return b'x';
    }
    if mask != 0 && mask & (RIGHT | LEFT) == 0 {
        return b'y';
    }
    0
}

/// The size of an arrow around a label `tw` by `th`.
pub(crate) fn arrow_size(mask: u32, tw: f64, th: f64) -> ArrowSize {
    let axis = arrow_axis(mask);
    let across = bit(mask, LEFT) - bit(mask, RIGHT);
    let along = bit(mask, UP) - bit(mask, DOWN);
    if axis == b'x' {
        let h = (th + 8.0).max(14.0) + 2.0 * OVERHANG;
        let depth = (h / 2.0).min(MAX_DEPTH);
        let heads = if mask == RIGHT | LEFT { 2.0 } else { 1.0 };
        return ArrowSize {
            dx: (across * depth) / 2.0,
            dy: 0.0,
            h,
            w: (tw + 12.0).max(16.0) + depth * heads,
        };
    }
    if axis == b'y' {
        let w = (tw + 16.0).max(14.0) + 2.0 * OVERHANG;
        let depth = (w / 2.0).min(MAX_DEPTH);
        let heads = if mask == UP | DOWN { 2.0 } else { 1.0 };
        return ArrowSize {
            dx: 0.0,
            dy: (along * depth) / 2.0,
            h: (th + 8.0).max(16.0) + depth * heads,
            w,
        };
    }
    ArrowSize {
        dx: (across * NUB) / 2.0,
        dy: (along * NUB) / 2.0,
        h: (th + 8.0).max(24.0) + NUB * (bit(mask, UP) + bit(mask, DOWN)),
        w: (tw + 16.0).max(24.0) + NUB * (bit(mask, LEFT) + bit(mask, RIGHT)),
    }
}

/// The outline of an arrow `w` by `h` around the origin, with `attrs` on its polygon.
#[expect(clippy::many_single_char_names, reason = "mirrors Pele's drawArrow")]
pub(crate) fn draw_arrow(mask: u32, w: f64, h: f64, attrs: &str) -> String {
    let axis = arrow_axis(mask);
    let x = w / 2.0;
    let y = h / 2.0;
    let has = |flag: u32| mask & flag != 0;
    let mut p: Vec<f64> = Vec::with_capacity(20);
    if axis == b'x' {
        let depth = y.min(MAX_DEPTH);
        let s = y - OVERHANG;
        let x0 = if has(LEFT) { -x + depth } else { -x };
        let x1 = if has(RIGHT) { x - depth } else { x };
        p.extend([x0, -s, x1, -s]);
        if has(RIGHT) {
            p.extend([x1, -y, x, 0.0, x1, y]);
        }
        p.extend([x1, s, x0, s]);
        if has(LEFT) {
            p.extend([x0, y, -x, 0.0, x0, -y]);
        }
    } else if axis == b'y' {
        let depth = x.min(MAX_DEPTH);
        let s = x - OVERHANG;
        let y0 = if has(UP) { -y + depth } else { -y };
        let y1 = if has(DOWN) { y - depth } else { y };
        p.extend([s, y0, s, y1]);
        if has(DOWN) {
            p.extend([x, y1, 0.0, y, -x, y1]);
        }
        p.extend([-s, y1, -s, y0]);
        if has(UP) {
            p.extend([-x, y0, 0.0, -y, x, y0]);
        }
    } else {
        let x0 = if has(LEFT) { -x + NUB } else { -x };
        let x1 = if has(RIGHT) { x - NUB } else { x };
        let y0 = if has(UP) { -y + NUB } else { -y };
        let y1 = if has(DOWN) { y - NUB } else { y };
        let cx = f64::midpoint(x0, x1);
        let cy = f64::midpoint(y0, y1);
        let kx = ((x1 - x0) / 2.0).min(10.0);
        let ky = ((y1 - y0) / 2.0).min(10.0);
        p.extend([x0, y0]);
        if has(UP) {
            p.extend([cx - kx, y0, cx, -y, cx + kx, y0]);
        }
        p.extend([x1, y0]);
        if has(RIGHT) {
            p.extend([x1, cy - ky, x, cy, x1, cy + ky]);
        }
        p.extend([x1, y1]);
        if has(DOWN) {
            p.extend([cx + kx, y1, cx, y, cx - kx, y1]);
        }
        p.extend([x0, y1]);
        if has(LEFT) {
            p.extend([x0, cy + ky, -x, cy, x0, cy - ky]);
        }
    }

    let mut out = String::from("<polygon points=\"");
    for (i, pair) in p.chunks_exact(2).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if let [px, py] = pair {
            push_num(&mut out, *px);
            out.push(',');
            push_num(&mut out, *py);
        }
    }
    out.push_str("\" stroke-linejoin=\"round\"");
    out.push_str(attrs);
    out.push_str("/>");
    out
}
