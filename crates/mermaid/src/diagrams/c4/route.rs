//! Routes C4 relations between shapes and boundaries. Port of Pele's
//! `src/diagrams/c4/route.ts`.
//!
//! Pele compares boxes by identity. This port gives every box an index, and compares those.

use std::collections::BTreeMap;

use crate::svg::builder::push_num;

/// A laid out shape or boundary: top-left corner, size, and the shared shape it is drawn as.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Box {
    /// The shared shape: `person`, `rect`, `cyl` or `h-cyl`.
    pub(crate) form: Form,
    /// Height.
    pub(crate) h: f64,
    /// Width.
    pub(crate) w: f64,
    /// Left edge.
    pub(crate) x: f64,
    /// Top edge.
    pub(crate) y: f64,
}

/// The shared shape a box is drawn as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Form {
    /// A cylinder standing up.
    Cyl,
    /// A cylinder lying down.
    HCyl,
    /// A person.
    Person,
    /// A rectangle.
    #[default]
    Rect,
}

impl Form {
    /// The name of the shared shape.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Cyl => "cyl",
            Self::HCyl => "h-cyl",
            Self::Person => "person",
            Self::Rect => "rect",
        }
    }
}

/// A routed relation.
#[derive(Clone, Debug, Default)]
pub(crate) struct Route {
    /// The path data.
    pub(crate) d: String,
    /// The direction of the end marker.
    pub(crate) edx: f64,
    /// The direction of the end marker.
    pub(crate) edy: f64,
    /// The tip of the end marker.
    pub(crate) ex: f64,
    /// The tip of the end marker.
    pub(crate) ey: f64,
    /// When set, the label stays over the line and moves that way by at most this much.
    pub(crate) lean: f64,
    /// The side of the label's stretch the label is pushed to, as a unit vector. Zero centers it.
    pub(crate) nx: f64,
    /// The side of the label's stretch the label is pushed to, as a unit vector. Zero centers it.
    pub(crate) ny: f64,
    /// The corners of the line, as x, y pairs.
    pub(crate) points: Vec<f64>,
    /// The widest the label should be.
    pub(crate) room: f64,
    /// The direction of the start marker.
    pub(crate) sdx: f64,
    /// The direction of the start marker.
    pub(crate) sdy: f64,
    /// The tip of the start marker.
    pub(crate) sx: f64,
    /// The tip of the start marker.
    pub(crate) sy: f64,
    /// The start of the stretch the label may sit on.
    pub(crate) x0: f64,
    /// The end of the stretch the label may sit on.
    pub(crate) x1: f64,
    /// The start of the stretch the label may sit on.
    pub(crate) y0: f64,
    /// The end of the stretch the label may sit on.
    pub(crate) y1: f64,
}

const LOOP: f64 = 28.0;
/// Room for a relation to run along the inside of a boundary.
pub(crate) const LANE: f64 = 20.0;
const LANE_STEP: f64 = 7.0;
const CORNER: f64 = 8.0;
/// A wide label on an upright lane leans this far out from it, to the middle of the gap between
/// columns.
const LEAN: f64 = 30.0;
/// Label widths that fit the gap between two columns, and where there is room to spare.
const NARROW: f64 = 88.0;
const WIDE: f64 = 160.0;
/// The shared person shape: a head of radius 13 centered 14 below the top, over a body that
/// starts 26 below it.
pub(crate) const HEAD: f64 = 26.0;

/// Parallel stretches closer than this read as one line, and a line this close to a boundary's
/// frame reads as part of it.
const NEAR: f64 = 6.0;
const FRAME_NEAR: f64 = 3.0;
const PORT_STEP: f64 = 12.0;

/// JavaScript's `Math.sign`: zero and `NaN` stay as they are.
pub(crate) fn sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        value
    }
}

/// JavaScript's `Math.hypot` of two values, as V8 computes it: the sum of the squares of each
/// over the larger, compensated, then the square root times the larger.
pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    let x = x.abs();
    let y = y.abs();
    if x == f64::INFINITY || y == f64::INFINITY {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let max = if y > x { y } else { x };
    if max == 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in [x, y] {
        let n = value / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// The cap radius `draw_shape` uses for a standing cylinder.
pub(crate) fn cyl_ry(w: f64) -> f64 {
    12.0_f64.min(6.0_f64.max(w / 14.0))
}

/// The cap radius `draw_shape` uses for a lying cylinder.
pub(crate) fn queue_rx(h: f64) -> f64 {
    12.0_f64.min(h / 4.0)
}

/// How far along the segment from (`px`, `py`) by (`dx`, `dy`) it first meets the rectangle.
/// Infinity when it misses.
#[expect(clippy::too_many_arguments, reason = "mirrors Pele's hitRect")]
fn hit_rect(x0: f64, y0: f64, x1: f64, y1: f64, px: f64, py: f64, dx: f64, dy: f64) -> f64 {
    let mut enter = 0.0_f64;
    let mut leave = 1.0_f64;
    if dx == 0.0 {
        if px < x0 || px > x1 {
            return f64::INFINITY;
        }
    } else {
        let a = (x0 - px) / dx;
        let b = (x1 - px) / dx;
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
    }
    if dy == 0.0 {
        if py < y0 || py > y1 {
            return f64::INFINITY;
        }
    } else {
        let a = (y0 - py) / dy;
        let b = (y1 - py) / dy;
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
    }
    if enter <= leave { enter } else { f64::INFINITY }
}

#[expect(clippy::too_many_arguments, reason = "mirrors Pele's hitEllipse")]
fn hit_ellipse(cx: f64, cy: f64, rx: f64, ry: f64, px: f64, py: f64, dx: f64, dy: f64) -> f64 {
    let ux = dx / rx;
    let uy = dy / ry;
    let vx = (px - cx) / rx;
    let vy = (py - cy) / ry;
    let a = ux * ux + uy * uy;
    let b = 2.0 * (ux * vx + uy * vy);
    let c = vx * vx + vy * vy - 1.0;
    let disc = b * b - 4.0 * a * c;
    if a == 0.0 || disc < 0.0 {
        return f64::INFINITY;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    if (0.0..=1.0).contains(&t) {
        t
    } else if c <= 0.0 {
        0.0
    } else {
        f64::INFINITY
    }
}

/// Where a segment from outside first meets the outline of a shape, as a fraction of the segment.
#[expect(
    clippy::many_single_char_names,
    reason = "mirrors Pele's geometry names"
)]
fn hit(b: &Box, px: f64, py: f64, qx: f64, qy: f64) -> f64 {
    let (x, y, w, h) = (b.x, b.y, b.w, b.h);
    let dx = qx - px;
    let dy = qy - py;
    match b.form {
        Form::Person => hit_rect(x, y + HEAD, x + w, y + h, px, py, dx, dy).min(hit_ellipse(
            x + w / 2.0,
            y + 14.0,
            13.0,
            13.0,
            px,
            py,
            dx,
            dy,
        )),
        Form::Cyl => {
            let ry = cyl_ry(w);
            hit_rect(x, y + ry, x + w, y + h - ry, px, py, dx, dy)
                .min(hit_ellipse(
                    x + w / 2.0,
                    y + ry,
                    w / 2.0,
                    ry,
                    px,
                    py,
                    dx,
                    dy,
                ))
                .min(hit_ellipse(
                    x + w / 2.0,
                    y + h - ry,
                    w / 2.0,
                    ry,
                    px,
                    py,
                    dx,
                    dy,
                ))
        }
        Form::HCyl => {
            let rx = queue_rx(h);
            hit_rect(x + rx, y, x + w - rx, y + h, px, py, dx, dy)
                .min(hit_ellipse(
                    x + rx,
                    y + h / 2.0,
                    rx,
                    h / 2.0,
                    px,
                    py,
                    dx,
                    dy,
                ))
                .min(hit_ellipse(
                    x + w - rx,
                    y + h / 2.0,
                    rx,
                    h / 2.0,
                    px,
                    py,
                    dx,
                    dy,
                ))
        }
        Form::Rect => hit_rect(x, y, x + w, y + h, px, py, dx, dy),
    }
}

/// The stretches of the line through `points`, each as its start and end x, y.
fn segments(points: &[f64]) -> impl Iterator<Item = [f64; 4]> + '_ {
    points
        .windows(4)
        .step_by(2)
        .filter_map(|seg| <[f64; 4]>::try_from(seg).ok())
}

/// Whether the line through `points` crosses the rectangle.
pub(crate) fn crosses(points: &[f64], x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    segments(points)
        .any(|[px, py, qx, qy]| hit_rect(x0, y0, x1, y1, px, py, qx - px, qy - py) <= 1.0)
}

fn contains(outer: &Box, inner: &Box) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.x + outer.w >= inner.x + inner.w
        && outer.y + outer.h >= inner.y + inner.h
}

/// The straight line from a to b: x, y of its start, then of its end.
fn connect(a: &Box, b: &Box, shift: f64) -> [f64; 4] {
    let ax = a.x + a.w / 2.0;
    let ay = a.y + a.h / 2.0;
    let bx = b.x + b.w / 2.0;
    let by = b.y + b.h / 2.0;
    let down = contains(a, b);
    if down || contains(b, a) {
        // One holds the other: a short line from the nearest side of the outer box.
        let (outer, inner) = if down { (a, b) } else { (b, a) };
        let ix = inner.x + inner.w / 2.0;
        let iy = inner.y + inner.h / 2.0;
        let left = inner.x - outer.x;
        let right = outer.x + outer.w - inner.x - inner.w;
        let top = inner.y - outer.y;
        let bottom = outer.y + outer.h - inner.y - inner.h;
        let least = left.min(right).min(top).min(bottom);
        #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
        let ox = if least == left {
            outer.x
        } else if least == right {
            outer.x + outer.w
        } else {
            ix
        };
        #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
        let oy = if least == left || least == right {
            iy
        } else if least == bottom {
            outer.y + outer.h
        } else {
            outer.y
        };
        let t = hit(inner, ox, oy, ix, iy).min(1.0);
        let mx = ox + (ix - ox) * t;
        let my = oy + (iy - oy) * t;
        return if down {
            [ox, oy, mx, my]
        } else {
            [mx, my, ox, oy]
        };
    }

    let mut px = ax;
    let mut py = ay;
    let mut qx = bx;
    let mut qy = by;
    // Shapes that share a column or a row are joined by an upright or level line through the
    // shared span.
    let x0 = a.x.max(b.x);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y0 = a.y.max(b.y);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x0 < x1 {
        px = f64::midpoint(x0, x1);
        qx = px;
    } else if y0 < y1 {
        py = f64::midpoint(y0, y1);
        qy = py;
    }
    let len = match hypot(qx - px, qy - py) {
        0.0 => 1.0,
        len if len.is_nan() => 1.0,
        len => len,
    };
    let nx = (-(qy - py) / len) * shift;
    let ny = ((qx - px) / len) * shift;
    let mut ta = hit(a, qx + nx, qy + ny, px + nx, py + ny);
    let mut tb = hit(b, px + nx, py + ny, qx + nx, qy + ny);
    if ta <= 1.0 && tb <= 1.0 {
        px += nx;
        py += ny;
        qx += nx;
        qy += ny;
    } else {
        px = ax;
        py = ay;
        qx = bx;
        qy = by;
        ta = hit(a, qx, qy, px, py).min(1.0);
        tb = hit(b, px, py, qx, qy).min(1.0);
    }
    [
        qx + (px - qx) * ta,
        qy + (py - qy) * ta,
        px + (qx - px) * tb,
        py + (qy - py) * tb,
    ]
}

/// What the routes drawn so far take up, so that the next keeps apart from them: upright
/// stretches as x, top, bottom; level ones as y, left, right; and where lines leave or reach
/// each side of a shape.
#[derive(Debug, Default)]
pub(crate) struct Lanes {
    frames: [Vec<f64>; 2],
    level: Vec<f64>,
    /// Routes that found no lane of their own: a line left straight behind a shape, or a lane
    /// that runs along another.
    pub(crate) squeezed: usize,
    taken: BTreeMap<usize, Vec<f64>>,
    upright: Vec<f64>,
}

impl Lanes {
    fn free(&self, upright: bool, at: f64, a0: f64, a1: f64) -> bool {
        let lo = a0.min(a1);
        let hi = a0.max(a1);
        let near = |list: &[f64], gap: f64| {
            list.as_chunks::<3>()
                .0
                .iter()
                .any(|&[x, from, to]| (x - at).abs() < gap && from < hi && to > lo)
        };
        let (own, frames) = if upright {
            (&self.upright, &self.frames[0])
        } else {
            (&self.level, &self.frames[1])
        };
        !near(own, NEAR) && !near(frames, FRAME_NEAR)
    }

    fn crossings(&self, points: &[f64]) -> usize {
        let mut count = 0;
        for [px, py, qx, qy] in segments(points) {
            #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
            let upright = qx == px;
            let at = if upright { qx } else { qy };
            let (a, b) = if upright { (py, qy) } else { (px, qx) };
            let lo = a.min(b);
            let hi = a.max(b);
            let list = if upright { &self.level } else { &self.upright };
            count += list
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|&&[x, from, to]| x > lo && x < hi && from < at && to > at)
                .count();
        }
        count
    }

    /// Records a boundary's frame, which lines keep off.
    pub(crate) fn frame(&mut self, b: &Box) {
        self.frames[0].extend([b.x, b.y, b.y + b.h, b.x + b.w, b.y, b.y + b.h]);
        self.frames[1].extend([b.y, b.x, b.x + b.w, b.y + b.h, b.x, b.x + b.w]);
    }

    fn take(&mut self, points: &[f64]) {
        for [px, py, qx, qy] in segments(points) {
            #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
            if qx == px {
                self.upright.extend([qx, py.min(qy), py.max(qy)]);
            } else if qy == py {
                self.level.extend([qy, px.min(qx), px.max(qx)]);
            }
        }
    }

    /// Where on one side of a shape (0 left, 1 right, 2 top, 3 bottom) a line can leave or
    /// arrive, between `lo` and `hi`: the places clear of the lines already there, nearest `want`
    /// first.
    fn ports(&self, id: usize, side: f64, want: f64, lo: f64, hi: f64) -> Vec<f64> {
        let used = self.taken.get(&id).map_or(&[][..], Vec::as_slice);
        let mut out = Vec::new();
        for k in 0..9_u32 {
            if out.len() >= 4 {
                break;
            }
            let way = if k & 1 == 1 { 1.0 } else { -1.0 };
            let at = want + way * f64::from(k.div_ceil(2)) * PORT_STEP;
            if at < lo || at > hi {
                continue;
            }
            #[expect(clippy::float_cmp, reason = "sides are small whole numbers")]
            let clear = !used
                .as_chunks::<2>()
                .0
                .iter()
                .any(|&[own, place]| own == side && (place - at).abs() < PORT_STEP);
            if clear {
                out.push(at);
            }
        }
        if out.is_empty() { vec![want] } else { out }
    }

    fn claim(&mut self, id: usize, side: f64, at: f64) {
        self.taken.entry(id).or_default().extend([side, at]);
    }
}

#[expect(
    clippy::many_single_char_names,
    reason = "mirrors Pele's geometry names"
)]
fn path(points: &[f64], start_trim: f64, end_trim: f64) -> String {
    let n = points.len();
    let at = |i: usize| points.get(i).copied().unwrap_or(f64::NAN);
    let ux = sign(at(2) - at(0));
    let uy = sign(at(3) - at(1));
    let mut d = String::from("M");
    push_num(&mut d, at(0) + ux * start_trim);
    d.push(',');
    push_num(&mut d, at(1) + uy * start_trim);
    let mut i = 2;
    while i + 2 < n {
        let x = at(i);
        let y = at(i + 1);
        let ix = sign(x - at(i - 2));
        let iy = sign(y - at(i - 1));
        let ox = sign(at(i + 2) - x);
        let oy = sign(at(i + 3) - y);
        let r = CORNER
            .min((x - at(i - 2) + y - at(i - 1)).abs() / 2.0)
            .min((at(i + 2) - x + at(i + 3) - y).abs() / 2.0);
        if ix == 0.0 {
            d.push('V');
            push_num(&mut d, y - iy * r);
        } else {
            d.push('H');
            push_num(&mut d, x - ix * r);
        }
        d.push('Q');
        push_num(&mut d, x);
        d.push(',');
        push_num(&mut d, y);
        d.push(' ');
        push_num(&mut d, x + ox * r);
        d.push(',');
        push_num(&mut d, y + oy * r);
        i += 2;
    }
    let lx = sign(at(n - 2) - at(n - 4));
    let ly = sign(at(n - 1) - at(n - 3));
    if lx == 0.0 {
        d.push('V');
        push_num(&mut d, at(n - 1) - ly * end_trim);
    } else {
        d.push('H');
        push_num(&mut d, at(n - 2) - lx * end_trim);
    }
    d
}

/// Whether the line through `points` keeps clear of every box but `a` and `b`.
fn clear_of(points: &[f64], boxes: &[Box], others: &[usize], a: usize, b: usize) -> bool {
    others.iter().all(|&other| {
        other == a
            || other == b
            || boxes
                .get(other)
                .is_none_or(|o| !crosses(points, o.x, o.y, o.x + o.w, o.y + o.h))
    })
}

/// What a bent route sets about its label, where it differs from the defaults.
#[derive(Clone, Copy, Default)]
struct Bend {
    lean: f64,
    nx: f64,
    room: f64,
}

/// The route for a line with corners through `points`, from a's outline to b's. `along` is the
/// stretch its label sits on.
fn bent(points: Vec<f64>, along: usize, start_trim: f64, end_trim: f64, label: Bend) -> Route {
    let n = points.len();
    let at = |i: usize| points.get(i).copied().unwrap_or(f64::NAN);
    Route {
        d: path(&points, start_trim, end_trim),
        edx: sign(at(n - 2) - at(n - 4)),
        edy: sign(at(n - 1) - at(n - 3)),
        ex: at(n - 2),
        ey: at(n - 1),
        lean: label.lean,
        nx: label.nx,
        ny: 0.0,
        room: label.room,
        sdx: -sign(at(2) - at(0)),
        sdy: -sign(at(3) - at(1)),
        sx: at(0),
        sy: at(1),
        x0: at(along),
        x1: at(along + 2),
        y0: at(along + 1),
        y1: at(along + 3),
        points,
    }
}

fn across(lo: f64, hi: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mid = f64::midpoint(lo, hi);
    let step = LANE_STEP.max((hi - lo) / 4.0);
    for k in 0..9_u32 {
        let way = if k & 1 == 1 { 1.0 } else { -1.0 };
        let at = mid + way * f64::from(k.div_ceil(2)) * step;
        if at >= lo + NEAR && at <= hi - NEAR {
            out.push(at);
        }
    }
    out
}

/// The best elbow so far.
struct Best {
    cost: f64,
    points: Vec<f64>,
    sides: [f64; 2],
    upright: bool,
}

/// Considers `points` as an elbow from `a` to `b`, keeping it when it is clear and cheaper.
/// Returns whether it was kept.
#[expect(
    clippy::too_many_arguments,
    reason = "the state of Pele's elbow closure"
)]
fn consider(
    best: &mut Option<Best>,
    points: Vec<f64>,
    upright: bool,
    sides: [f64; 2],
    boxes: &[Box],
    others: &[usize],
    lanes: &Lanes,
    ends: (usize, usize),
) -> bool {
    if !clear_of(&points, boxes, others, ends.0, ends.1) {
        return false;
    }
    for [px, py, qx, qy] in segments(&points) {
        #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
        let up = qx == px;
        let free = if up {
            lanes.free(up, qx, py, qy)
        } else {
            lanes.free(up, qy, px, qx)
        };
        if !free {
            return false;
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "counts stay far below 2^52")]
    let mut cost = lanes.crossings(&points) as f64 * 1e6;
    for [px, py, qx, qy] in segments(&points) {
        cost += (qx - px).abs() + (qy - py).abs();
    }
    if best.as_ref().is_none_or(|best| cost < best.cost) {
        *best = Some(Best {
            cost,
            points,
            sides,
            upright,
        });
        return true;
    }
    false
}

fn elbow(
    boxes: &[Box],
    ids: (usize, usize),
    others: &[usize],
    lanes: &mut Lanes,
    start_trim: f64,
    end_trim: f64,
) -> Option<Route> {
    let a = boxes.get(ids.0).copied().unwrap_or_default();
    let b = boxes.get(ids.1).copied().unwrap_or_default();
    let ax = a.x + a.w / 2.0;
    let ay = a.y + a.h / 2.0;
    let bx = b.x + b.w / 2.0;
    let by = b.y + b.h / 2.0;
    let mut best: Option<Best> = None;
    if a.x + a.w <= b.x || b.x + b.w <= a.x {
        let right = bx > ax;
        let down = sign(by - ay);
        let sides = if right { [1.0, 0.0] } else { [0.0, 1.0] };
        let lanes_across = if right {
            across(a.x + a.w, b.x)
        } else {
            across(b.x + b.w, a.x)
        };
        'search: for y0 in lanes.ports(
            ids.0,
            sides[0],
            ay + (down * a.h) / 4.0,
            a.y + 8.0,
            a.y + a.h - 8.0,
        ) {
            for y1 in lanes.ports(
                ids.1,
                sides[1],
                by - (down * b.h) / 4.0,
                b.y + 8.0,
                b.y + b.h - 8.0,
            ) {
                for &lane in &lanes_across {
                    let x0 = lane + (ax - lane) * hit(&a, lane, y0, ax, y0).min(1.0);
                    let x1 = lane + (bx - lane) * hit(&b, lane, y1, bx, y1).min(1.0);
                    if consider(
                        &mut best,
                        vec![x0, y0, lane, y0, lane, y1, x1, y1],
                        true,
                        sides,
                        boxes,
                        others,
                        lanes,
                        ids,
                    ) {
                        break 'search;
                    }
                }
            }
        }
    }
    if a.y + a.h <= b.y || b.y + b.h <= a.y {
        let below = by > ay;
        let sideways = sign(bx - ax);
        let sides = if below { [3.0, 2.0] } else { [2.0, 3.0] };
        let lanes_across = if below {
            across(a.y + a.h, b.y)
        } else {
            across(b.y + b.h, a.y)
        };
        'search: for x0 in lanes.ports(
            ids.0,
            sides[0],
            ax + (sideways * a.w) / 4.0,
            a.x + 8.0,
            a.x + a.w - 8.0,
        ) {
            for x1 in lanes.ports(
                ids.1,
                sides[1],
                bx - (sideways * b.w) / 4.0,
                b.x + 8.0,
                b.x + b.w - 8.0,
            ) {
                for &lane in &lanes_across {
                    let y0 = lane + (ay - lane) * hit(&a, x0, lane, x0, ay).min(1.0);
                    let y1 = lane + (by - lane) * hit(&b, x1, lane, x1, by).min(1.0);
                    if consider(
                        &mut best,
                        vec![x0, y0, x0, lane, x1, lane, x1, y1],
                        false,
                        sides,
                        boxes,
                        others,
                        lanes,
                        ids,
                    ) {
                        break 'search;
                    }
                }
            }
        }
    }
    let best = best?;
    let at = |i: usize| best.points.get(i).copied().unwrap_or(f64::NAN);
    lanes.take(&best.points);
    lanes.claim(
        ids.0,
        best.sides[0],
        if best.upright { at(1) } else { at(0) },
    );
    lanes.claim(
        ids.1,
        best.sides[1],
        if best.upright { at(7) } else { at(6) },
    );
    // The label sits on the middle stretch, across the gap.
    let room = if best.upright { NARROW } else { WIDE };
    Some(bent(
        best.points,
        2,
        start_trim,
        end_trim,
        Bend {
            room,
            ..Bend::default()
        },
    ))
}

/// One side's detour around the shapes between a and b.
struct Detour {
    p0: f64,
    p1: f64,
    points: Vec<f64>,
    side: f64,
    /// How many lanes out it went. 5 when none was free.
    step: f64,
}

/// Routes a relation from `boxes[a]` to `boxes[b]`. `shift` moves the line sideways when another
/// runs the opposite way. `others` are the boxes to steer clear of, `None` to skip that check.
/// `lanes` holds what the routes before it take up, so that this one keeps apart from them.
#[expect(clippy::too_many_lines, reason = "mirrors Pele's route")]
pub(crate) fn route(
    boxes: &[Box],
    ids: (usize, usize),
    shift: f64,
    others: Option<&[usize]>,
    lanes: &mut Lanes,
    start_trim: f64,
    end_trim: f64,
) -> Route {
    let a = boxes.get(ids.0).copied().unwrap_or_default();
    let b = boxes.get(ids.1).copied().unwrap_or_default();
    if ids.0 == ids.1 {
        let spread = (a.h / 2.0 - 4.0).min(10.0);
        let len = hypot(LOOP, spread);
        let x = a.x + a.w;
        let cy = a.y + a.h / 2.0;
        let ux = LOOP / len;
        let uy = spread / len;
        let mut d = String::from("M");
        push_num(&mut d, x + ux * start_trim);
        d.push(',');
        push_num(&mut d, cy - spread - uy * start_trim);
        d.push('C');
        push_num(&mut d, x + LOOP);
        d.push(',');
        push_num(&mut d, cy - spread * 2.0);
        d.push(' ');
        push_num(&mut d, x + LOOP);
        d.push(',');
        push_num(&mut d, cy + spread * 2.0);
        d.push(' ');
        push_num(&mut d, x + ux * end_trim);
        d.push(',');
        push_num(&mut d, cy + spread + uy * end_trim);
        return Route {
            d,
            edx: -ux,
            edy: -uy,
            ex: x,
            ey: cy + spread,
            lean: 0.0,
            nx: 1.0,
            ny: 0.0,
            points: Vec::new(),
            room: 64.0,
            sdx: -ux,
            sdy: uy,
            sx: x,
            sy: cy - spread,
            x0: x + LOOP + 4.0,
            x1: x + LOOP + 4.0,
            y0: cy,
            y1: cy,
        };
    }

    let [sx, sy, ex, ey] = connect(&a, &b, shift);
    #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
    let upright = sx == ex;
    #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
    let level = sy == ey;
    let is_end = |other: usize| other == ids.0 || other == ids.1;
    let in_way = |other: &Box| {
        hit_rect(
            other.x,
            other.y,
            other.x + other.w,
            other.y + other.h,
            sx,
            sy,
            ex - sx,
            ey - sy,
        ) <= 1.0
    };
    let mut blocked = false;
    if let Some(others) = others
        && !contains(&a, &b)
        && !contains(&b, &a)
    {
        for &other in others {
            if !is_end(other) && boxes.get(other).is_some_and(in_way) {
                blocked = true;
            }
        }
    }
    if let Some(others) = others
        && blocked
        && !upright
        && !level
    {
        if let Some(around) = elbow(boxes, ids, others, lanes, start_trim, end_trim) {
            return around;
        }
        lanes.squeezed += 1;
    }
    if let Some(others) = others
        && blocked
        && (upright || level)
    {
        let lane_on = |forward: bool| {
            let mut lane = if forward {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
            let mut widen = |o: &Box| {
                let edge = if upright { o.x } else { o.y };
                lane = if forward {
                    lane.max(edge + if upright { o.w } else { o.h })
                } else {
                    lane.min(edge)
                };
            };
            widen(&a);
            widen(&b);
            for &other in others {
                if let Some(o) = boxes.get(other)
                    && !is_end(other)
                    && in_way(o)
                {
                    widen(o);
                }
            }
            lane
        };
        let lo = if upright { a.y.min(b.y) } else { a.x.min(b.x) };
        let hi = if upright {
            (a.y + a.h).max(b.y + b.h)
        } else {
            (a.x + a.w).max(b.x + b.w)
        };
        let outside = |forward: bool, lane: f64| {
            others.iter().all(|&other| {
                let Some(o) = boxes.get(other).filter(|_| !is_end(other)) else {
                    return true;
                };
                let near = if upright {
                    o.y < hi && o.y + o.h > lo
                } else {
                    o.x < hi && o.x + o.w > lo
                };
                let start = if upright { o.x } else { o.y };
                let end = start + if upright { o.w } else { o.h };
                !(near && if forward { end > lane } else { start < lane })
            })
        };
        let ax = a.x + a.w / 2.0;
        let ay = a.y + a.h / 2.0;
        let bx = b.x + b.w / 2.0;
        let by = b.y + b.h / 2.0;
        let towards = if upright {
            sign(by - ay)
        } else {
            sign(bx - ax)
        };
        // The route on one side: each end leaves its shape a quarter of the way from the middle
        // towards the other, clear of the straight lines that meet the middle of that side and
        // of other routes there. The lane runs a little further out for each route already
        // along that gap, and never behind a shape.
        let along = |lanes: &Lanes, forward: bool| -> Detour {
            let lane = lane_on(forward);
            let way = if forward { 1.0 } else { -1.0 };
            let side = if upright {
                if forward { 1.0 } else { 0.0 }
            } else if forward {
                3.0
            } else {
                2.0
            };
            let first = |list: Vec<f64>, want: f64| list.first().copied().unwrap_or(want);
            let p0 = if upright {
                let want = ay + (towards * a.h) / 4.0;
                first(
                    lanes.ports(ids.0, side, want, a.y + 8.0, a.y + a.h - 8.0),
                    want,
                )
            } else {
                let want = ax + (towards * a.w) / 4.0;
                first(
                    lanes.ports(ids.0, side, want, a.x + 8.0, a.x + a.w - 8.0),
                    want,
                )
            };
            let p1 = if upright {
                let want = by - (towards * b.h) / 4.0;
                first(
                    lanes.ports(ids.1, side, want, b.y + 8.0, b.y + b.h - 8.0),
                    want,
                )
            } else {
                let want = bx - (towards * b.w) / 4.0;
                first(
                    lanes.ports(ids.1, side, want, b.x + 8.0, b.x + b.w - 8.0),
                    want,
                )
            };
            let points_at = |at: f64| -> Vec<f64> {
                if upright {
                    let x0 = at + (ax - at) * hit(&a, at, p0, ax, p0).min(1.0);
                    let x1 = at + (bx - at) * hit(&b, at, p1, bx, p1).min(1.0);
                    return vec![x0, p0, at, p0, at, p1, x1, p1];
                }
                let y0 = at + (ay - at) * hit(&a, p0, at, p0, ay).min(1.0);
                let y1 = at + (by - at) * hit(&b, p1, at, p1, by).min(1.0);
                vec![p0, y0, p0, at, p1, at, p1, y1]
            };
            let mut points = points_at(lane + way * LANE);
            for k in 0..=4_u32 {
                let at = lane + way * (LANE + f64::from(k) * LANE_STEP);
                let tried = points_at(at);
                if !clear_of(&tried, boxes, others, ids.0, ids.1) {
                    continue;
                }
                points = tried;
                if lanes.free(upright, at, p0, p1) {
                    return Detour {
                        p0,
                        p1,
                        points,
                        side,
                        step: f64::from(k),
                    };
                }
            }
            Detour {
                p0,
                p1,
                points,
                side,
                step: 5.0,
            }
        };
        // A detour takes the outside of the drawing when only one side is outside, leaving the
        // gaps between columns and rows to the relations that cross them. Otherwise it takes the
        // side where it crosses fewer lines, then the one where it runs nearer its shapes, so
        // that detours spread over both sides.
        let natural = if upright { ey > sy } else { ex < sx };
        let out = outside(natural, lane_on(natural));
        let back = outside(!natural, lane_on(!natural));
        let first = along(lanes, natural);
        let other = (out == back).then(|| along(lanes, !natural));
        #[expect(clippy::cast_precision_loss, reason = "counts stay far below 2^52")]
        let cost = |way: &Detour| lanes.crossings(&way.points) as f64 * 8.0 + way.step;
        let forward = if (back && !out)
            || other
                .as_ref()
                .is_some_and(|other| cost(other) < cost(&first))
        {
            !natural
        } else {
            natural
        };
        let chosen = if forward == natural {
            first
        } else {
            match other {
                Some(other) => other,
                None => along(lanes, !natural),
            }
        };
        let way = if forward { 1.0 } else { -1.0 };
        if chosen.step > 4.0 {
            lanes.squeezed += 1;
        }
        lanes.take(&chosen.points);
        lanes.claim(ids.0, chosen.side, chosen.p0);
        lanes.claim(ids.1, chosen.side, chosen.p1);
        let label = if upright {
            Bend {
                lean: LEAN,
                nx: way,
                room: NARROW,
            }
        } else {
            Bend {
                room: WIDE,
                ..Bend::default()
            }
        };
        return bent(chosen.points, 2, start_trim, end_trim, label);
    }

    let len = match hypot(ex - sx, ey - sy) {
        0.0 => 1.0,
        len if len.is_nan() => 1.0,
        len => len,
    };
    let dx = (ex - sx) / len;
    let dy = (ey - sy) / len;
    if upright || level {
        lanes.take(&[sx, sy, ex, ey]);
    }
    let mut d = String::from("M");
    push_num(&mut d, sx + dx * start_trim);
    d.push(',');
    push_num(&mut d, sy + dy * start_trim);
    d.push('L');
    push_num(&mut d, ex - dx * end_trim);
    d.push(',');
    push_num(&mut d, ey - dy * end_trim);
    Route {
        d,
        edx: dx,
        edy: dy,
        ex,
        ey,
        lean: 0.0,
        nx: if shift == 0.0 { 0.0 } else { -dy },
        ny: if shift == 0.0 { 0.0 } else { dx },
        points: vec![sx, sy, ex, ey],
        room: if dx.abs() > dy.abs() { NARROW } else { WIDE },
        sdx: -dx,
        sdy: -dy,
        sx,
        sy,
        x0: sx,
        x1: ex,
        y0: sy,
        y1: ey,
    }
}
