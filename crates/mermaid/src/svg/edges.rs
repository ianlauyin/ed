//! Edge paths, end markers, edge labels, and self-loops. Port of Pele's `src/svg/edges.ts`.
//!
//! Marker types are Mermaid's: `arrow_point`, `arrow_circle`, and `arrow_cross`. Any other type
//! draws nothing.

use std::collections::BTreeMap;

use crate::layout::compound::CEdge;
use crate::layout::compound::CNode;
use crate::layout::layered::ease;
use crate::svg::builder::esc_text;
use crate::svg::builder::label_svg;
use crate::svg::builder::push_num;
use crate::text::icons::IconResolver;
use crate::text::label::Label;

/// The length of an arrowhead.
pub(crate) const ARROW: f64 = 8.0;

/// A drawn edge: its path, and where and which way it leaves and arrives, for its markers.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EdgePath {
    /// The path data.
    pub(crate) d: String,
    /// The x of the way the edge arrives.
    pub(crate) edx: f64,
    /// The y of the way the edge arrives.
    pub(crate) edy: f64,
    /// The x of the end, before trimming.
    pub(crate) ex: f64,
    /// The y of the end, before trimming.
    pub(crate) ey: f64,
    /// The x of the way back out of the start, for a marker there.
    pub(crate) sdx: f64,
    /// The y of the way back out of the start, for a marker there.
    pub(crate) sdy: f64,
    /// The x of the start, before trimming.
    pub(crate) sx: f64,
    /// The y of the start, before trimming.
    pub(crate) sy: f64,
}

/// The most a corner is rounded by in a stepped route.
const CORNER: f64 = 12.0;
/// How far the handles of a wide turn reach along its two legs. A little more than a circle's,
/// which makes the turn fuller.
const HANDLE: f64 = 0.62;
/// How far an edge runs straight before a marker at its end, where it has the room.
pub(crate) const LEAD: f64 = 6.0;

/// JavaScript's `Math.sign`: zero for zero and `NaN` for `NaN`.
fn js_sign(value: f64) -> f64 {
    if value == 0.0 || value.is_nan() { value } else { value.signum() }
}

/// `Math.sign(value) || 1`: the sign, or 1 where there is none.
fn sign_or_one(value: f64) -> f64 {
    if value == 0.0 || value.is_nan() { 1.0 } else { value.signum() }
}

/// How far from each side of each node the edges that meet it start to turn: as far as the
/// largest marker there needs, for every edge on the side, so none turns sooner than its
/// neighbor and crosses it.
#[derive(Clone, Debug, Default)]
pub(crate) struct SideReach {
    /// The reach by `node * 4 + side`.
    reach: BTreeMap<usize, f64>,
}

impl SideReach {
    /// Records an end with a marker of `type` on `side` of `node`. A side below 0 is none.
    pub(crate) fn add(&mut self, node: usize, side: i32, r#type: &str) {
        let trim = marker_trim(r#type);
        let Ok(side) = usize::try_from(side) else {
            return;
        };
        let key = node * 4 + side;
        if trim > 0.0 && trim + LEAD > self.reach.get(&key).copied().unwrap_or(0.0) {
            self.reach.insert(key, trim + LEAD);
        }
    }

    /// How far from `side` of `node` its edges start to turn.
    pub(crate) fn of(&self, node: usize, side: i32) -> f64 {
        usize::try_from(side).map_or(0.0, |side| self.reach.get(&(node * 4 + side)).copied().unwrap_or(0.0))
    }
}

/// A route's points as [`route_path`] reshapes them.
struct Points {
    /// The axis each point is entered along: 0 vertically, 1 horizontally.
    axes: Vec<f64>,
    /// Whether the route is drawn as straight lines.
    linear: bool,
    /// The x of each point.
    xs: Vec<f64>,
    /// The y of each point.
    ys: Vec<f64>,
}

impl Points {
    /// The x of point `i`, or `NaN` past the ends, as JavaScript reads an absent element.
    fn x(&self, i: usize) -> f64 {
        self.xs.get(i).copied().unwrap_or(f64::NAN)
    }

    /// The y of point `i`, or `NaN` past the ends.
    fn y(&self, i: usize) -> f64 {
        self.ys.get(i).copied().unwrap_or(f64::NAN)
    }

    /// The axis of point `i`, or `NaN` past the ends.
    fn axis(&self, i: usize) -> f64 {
        self.axes.get(i).copied().unwrap_or(f64::NAN)
    }

    /// Whether points `i - 1` and `i` are in line.
    fn aligned(&self, i: usize) -> bool {
        let j = i.wrapping_sub(1);
        (self.x(i) - self.x(j)).abs() < 0.01 || (self.y(i) - self.y(j)).abs() < 0.01
    }

    /// Whether the route runs straight into point `i`.
    fn straight(&self, i: usize) -> bool {
        self.linear || self.aligned(i)
    }

    /// The way the route heads from point `i` to point `j`: along the flow where an S joins them.
    fn unit(&self, i: usize, j: usize, axis: f64) -> (f64, f64) {
        if !self.straight(i.max(j)) {
            return if axis == 0.0 {
                (0.0, sign_or_one(self.y(j) - self.y(i)))
            } else {
                (sign_or_one(self.x(j) - self.x(i)), 0.0)
            };
        }
        let dx = self.x(j) - self.x(i);
        let dy = self.y(j) - self.y(i);
        let len = dx.hypot(dy);
        let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
        (dx / len, dy / len)
    }

    /// The length of the leg into point `i`, along both axes.
    fn length(&self, i: usize) -> f64 {
        let j = i.wrapping_sub(1);
        (self.x(i) - self.x(j)).abs() + (self.y(i) - self.y(j)).abs()
    }

    /// Whether the route turns a corner at point `i`, from running one way to running the other.
    fn turns(&self, i: usize, last: usize) -> bool {
        let j = i.wrapping_sub(1);
        !self.linear
            && i > 0
            && i < last
            && self.aligned(i)
            && self.aligned(i + 1)
            && self.length(i) >= 0.01
            && self.length(i + 1) >= 0.01
            && ((self.x(i) - self.x(j)).abs() < 0.01) != ((self.x(i + 1) - self.x(i)).abs() < 0.01)
    }

    /// How far the leg into point `i` runs along (`ux`, `uy`).
    fn along(&self, i: usize, ux: f64, uy: f64) -> f64 {
        let j = i.wrapping_sub(1);
        (self.x(i) - self.x(j)).abs() * ux.abs() + (self.y(i) - self.y(j)).abs() * uy.abs()
    }
}

/// Whether two points are within a hundredth of each other on both axes.
fn near(ax: f64, ay: f64, bx: f64, by: f64) -> bool {
    (ax - bx).abs() < 0.01 && (ay - by).abs() < 0.01
}

/// Of the way from the end of a run to where it turns, the part that makes up for a larger marker
/// beside this one is kept where it can be, and the rest is at most a quarter of what is left, so
/// ends on one side of a node turn equally far from it.
fn lead(reach: f64, trim: f64, run: f64) -> f64 {
    let room = js_min(js_max(0.0, reach - LEAD - trim), run / 4.0);
    room + js_min(reach - trim - room, (run - room) / 4.0)
}

/// JavaScript's two-argument `Math.min`, which passes `NaN` on.
const fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// JavaScript's two-argument `Math.max`, which passes `NaN` on.
const fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// Appends `x,y`.
fn push_point(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Builds the path for a route of `x, y, axis` triples, trimming both ends to leave room for
/// markers.
///
/// Between two points that are not in line, a route is one S that leaves and arrives along the
/// flow, or a step in a stepped route. Where the route itself turns a corner, a stepped route
/// rounds it a little, and any other but a linear one turns as widely as it can: the turn takes
/// the whole of a leg that ends at a node and half of a leg it shares with the next turn.
/// `start_reach` and `end_reach` say how far from its nodes the edge starts to turn, where the
/// other edges on the same side of a node need it to. By default, far enough to clear its own
/// marker.
#[expect(clippy::many_single_char_names, reason = "mirrors Pele's geometry names")]
pub(crate) fn route_path(
    route: &[f64],
    curve: Option<&str>,
    start_trim: f64,
    end_trim: f64,
    start_reach: Option<f64>,
    end_reach: Option<f64>,
) -> EdgePath {
    let start_reach = start_reach.unwrap_or(if start_trim > 0.0 { start_trim + LEAD } else { 0.0 });
    let end_reach = end_reach.unwrap_or(if end_trim > 0.0 { end_trim + LEAD } else { 0.0 });
    let linear = curve == Some("linear");
    let stepped = matches!(curve, Some("step" | "stepBefore" | "stepAfter"));
    let mut p = Points { axes: Vec::new(), linear, xs: Vec::new(), ys: Vec::new() };
    let at = |i: usize| route.get(i).copied().unwrap_or(f64::NAN);
    for i in (0..route.len()).step_by(3) {
        let x = at(i);
        let y = at(i + 1);
        let axis = at(i + 2);
        // A stepped route turns at right angles where a smooth one would curve.
        if stepped
            && let (Some(&px), Some(&py)) = (p.xs.last(), p.ys.last())
            && (x - px).abs() >= 0.01
            && (y - py).abs() >= 0.01
        {
            let (mx, my) = match curve {
                Some("stepBefore") => (px, py),
                Some("stepAfter") => (x, y),
                _ => (f64::midpoint(px, x), f64::midpoint(py, y)),
            };
            let turns = if axis == 0.0 { [px, my, x, my] } else { [mx, py, mx, y] };
            for [tx, ty] in [[turns[0], turns[1]], [turns[2], turns[3]]] {
                if near(tx, ty, px, py) || near(tx, ty, x, y) {
                    continue;
                }
                p.xs.push(tx);
                p.ys.push(ty);
                p.axes.push(axis);
            }
        }
        p.xs.push(x);
        p.ys.push(y);
        p.axes.push(axis);
    }

    // An empty route has no path. Pele would write `NaN` coordinates.
    let Some(mut last) = p.xs.len().checked_sub(1) else {
        return EdgePath::default();
    };
    let (sdx, sdy) = p.unit(0, 1, p.axis(1));
    let (edx, edy) = p.unit(last.wrapping_sub(1), last, p.axis(last));
    let sx = p.x(0);
    let sy = p.y(0);
    let ex = p.x(last);
    let ey = p.y(last);
    if let (Some(x), Some(y)) = (p.xs.first_mut(), p.ys.first_mut()) {
        *x += sdx * start_trim;
        *y += sdy * start_trim;
    }
    if let (Some(x), Some(y)) = (p.xs.get_mut(last), p.ys.get_mut(last)) {
        *x -= edx * end_trim;
        *y -= edy * end_trim;
    }

    // An edge that curves into a marker runs straight for a little way first, where it has the
    // room, so the marker does not sit on the bend.
    let arrives = if end_reach > end_trim && (!p.straight(last) || p.turns(last.wrapping_sub(1), last)) {
        lead(end_reach, end_trim, p.along(last, edx, edy))
    } else {
        0.0
    };
    let leaves = if start_reach > start_trim && (!p.straight(1) || p.turns(1, last)) {
        lead(start_reach, start_trim, p.along(1, sdx, sdy))
    } else {
        0.0
    };
    if arrives >= 1.0 {
        let (x, y, axis) = (p.x(last) - edx * arrives, p.y(last) - edy * arrives, p.axis(last));
        p.xs.insert(last, x);
        p.ys.insert(last, y);
        p.axes.insert(last, axis);
        last += 1;
    }
    if leaves >= 1.0 {
        let (x, y, axis) = (p.x(0) + sdx * leaves, p.y(0) + sdy * leaves, p.axis(1));
        p.xs.insert(1, x);
        p.ys.insert(1, y);
        p.axes.insert(1, axis);
        last += 1;
    }

    let mut d = String::from("M");
    push_point(&mut d, p.x(0), p.y(0));
    let mut at_x = p.x(0);
    let mut at_y = p.y(0);
    let line_to = |d: &mut String, at_x: &mut f64, at_y: &mut f64, x: f64, y: f64| {
        if near(x, y, *at_x, *at_y) {
            return;
        }
        d.push('L');
        push_point(d, x, y);
        *at_x = x;
        *at_y = y;
    };
    for i in 1..=last {
        let x = p.x(i);
        let y = p.y(i);
        if !p.straight(i) {
            let down = p.axis(i) == 0.0;
            let along = if down { y - at_y } else { x - at_x };
            let k = along.abs() * ease(if down { x - at_x } else { y - at_y }, along);
            let ux = if down { 0.0 } else { js_sign(along) * k };
            let uy = if down { js_sign(along) * k } else { 0.0 };
            d.push('C');
            push_point(&mut d, at_x + ux, at_y + uy);
            d.push(' ');
            push_point(&mut d, x - ux, y - uy);
            d.push(' ');
            push_point(&mut d, x, y);
            at_x = x;
            at_y = y;
            continue;
        }
        if !p.turns(i, last) {
            // A point part of the way along one straight run adds nothing to the path.
            if i < last
                && p.aligned(i)
                && p.aligned(i + 1)
                && (x - p.x(i - 1)) * (p.x(i + 1) - x) + (y - p.y(i - 1)) * (p.y(i + 1) - y) > 0.0
            {
                continue;
            }
            line_to(&mut d, &mut at_x, &mut at_y, x, y);
            continue;
        }

        // How much of the leg before the turn and of the leg after it the turn takes: all of one
        // that ends at a node, short of the straight run into a marker, and half of one it shares.
        let mut before = if p.turns(i - 1, last) { p.length(i) / 2.0 } else { p.length(i) };
        let mut after = if p.turns(i + 1, last) { p.length(i + 1) / 2.0 } else { p.length(i + 1) };
        if stepped {
            before = CORNER.min(before).min(after);
            after = before;
        }
        let (ux, uy) = p.unit(i - 1, i, p.axis(i));
        let (vx, vy) = p.unit(i, i + 1, p.axis(i + 1));
        line_to(&mut d, &mut at_x, &mut at_y, x - ux * before, y - uy * before);
        at_x = x + vx * after;
        at_y = y + vy * after;
        if stepped {
            d.push('Q');
            push_point(&mut d, x, y);
            d.push(' ');
            push_point(&mut d, at_x, at_y);
        } else {
            d.push('C');
            push_point(&mut d, x - ux * before * (1.0 - HANDLE), y - uy * before * (1.0 - HANDLE));
            d.push(' ');
            push_point(&mut d, x + vx * after * (1.0 - HANDLE), y + vy * after * (1.0 - HANDLE));
            d.push(' ');
            push_point(&mut d, at_x, at_y);
        }
    }
    EdgePath { d, edx, edy, ex, ey, sdx: -sdx, sdy: -sdy, sx, sy }
}

/// How far a marker of `type` trims the end of its edge.
pub(crate) fn marker_trim(r#type: &str) -> f64 {
    match r#type {
        "arrow_point" => ARROW - 1.0,
        "arrow_circle" => ARROW,
        "arrow_cross" => 4.0,
        _ => 0.0,
    }
}

/// Draws a marker of `type` whose tip is at (`x`, `y`), pointing along (`dx`, `dy`), filled with
/// `color` as written.
pub(crate) fn marker(r#type: &str, x: f64, y: f64, dx: f64, dy: f64, color: &str) -> String {
    let nx = -dy;
    let ny = dx;
    let mut out = String::new();
    match r#type {
        "arrow_point" => {
            let bx = x - dx * ARROW;
            let by = y - dy * ARROW;
            let half = ARROW * 0.42;
            out.push_str("<path class=\"ondine-marker\" d=\"M");
            push_point(&mut out, x, y);
            out.push('L');
            push_point(&mut out, bx + nx * half, by + ny * half);
            out.push('L');
            push_point(&mut out, bx - nx * half, by - ny * half);
            out.push_str("Z\" fill=\"");
            out.push_str(color);
            out.push_str("\" stroke=\"none\"/>");
        }
        "arrow_circle" => {
            out.push_str("<circle class=\"ondine-marker\" cx=\"");
            push_num(&mut out, x - dx * 4.0);
            out.push_str("\" cy=\"");
            push_num(&mut out, y - dy * 4.0);
            out.push_str("\" r=\"3.5\" fill=\"");
            out.push_str(color);
            out.push_str("\" stroke=\"none\"/>");
        }
        "arrow_cross" => {
            let cx = x - dx * 4.0;
            let cy = y - dy * 4.0;
            let a = 3.5;
            let ux = (dx + nx) * a;
            let uy = (dy + ny) * a;
            let vx = (dx - nx) * a;
            let vy = (dy - ny) * a;
            out.push_str("<path class=\"ondine-marker\" d=\"M");
            push_point(&mut out, cx - ux, cy - uy);
            out.push('L');
            push_point(&mut out, cx + ux, cy + uy);
            out.push('M');
            push_point(&mut out, cx - vx, cy - vy);
            out.push('L');
            push_point(&mut out, cx + vx, cy + vy);
            out.push_str("\"/>");
        }
        _ => {}
    }
    out
}

/// An edge label centered on (`x`, `y`) on a patch of the background, so the line does not run
/// through the text.
pub(crate) fn edge_label_svg(
    id: &str,
    label: &Label,
    x: f64,
    y: f64,
    attrs: &str,
    icons: Option<IconResolver<'_>>,
) -> String {
    let w = label.width + 8.0;
    let h = label.height;
    let mut out = String::from("<g class=\"ondine-edge-label\" data-id=\"");
    out.push_str(&esc_text(id));
    out.push_str("\"><rect x=\"");
    push_num(&mut out, x - w / 2.0);
    out.push_str("\" y=\"");
    push_num(&mut out, y - h / 2.0);
    out.push_str("\" width=\"");
    push_num(&mut out, w);
    out.push_str("\" height=\"");
    push_num(&mut out, h);
    out.push_str("\" rx=\"3\" fill=\"var(--_bg)\"/>");
    out.push_str(&label_svg(label, x, y, attrs, icons));
    out.push_str("</g>");
    out
}

/// A self-loop beside node `c` of drawn size `w` by `h`: out of the side that lies across the
/// flow and back in. `k` counts the loops already on the node, so each one reaches a little
/// further than `base`. `trim` leaves room for the end marker. Also places the edge's label
/// beyond the loop.
#[expect(clippy::too_many_arguments, clippy::many_single_char_names, reason = "mirrors Pele's loopPath")]
pub(crate) fn loop_path(
    c: &CNode,
    w: f64,
    h: f64,
    k: f64,
    sideways: bool,
    base: f64,
    trim: f64,
    e: &mut CEdge,
) -> EdgePath {
    let half = (if sideways { w } else { h }) / 2.0;
    let spread = (half - 4.0).min(8.0 + k * 6.0);
    let reach = base + k * 8.0;
    let len = reach.hypot(spread);
    let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
    // Local frame: `out` points away from the node, `along` runs along its side.
    let at = |d: &mut String, out: f64, along: f64| {
        if sideways {
            push_point(d, c.x + along, c.y + h / 2.0 + out);
        } else {
            push_point(d, c.x + w / 2.0 + out, c.y + along);
        }
    };
    let tx = (reach / len) * trim;
    let ty = (spread / len) * trim;
    if sideways {
        e.label_x = c.x;
        e.label_y = c.y + h / 2.0 + reach + 4.0 + e.label_h / 2.0;
    } else {
        e.label_x = c.x + w / 2.0 + reach + 4.0 + e.label_w / 2.0;
        e.label_y = c.y;
    }

    let mut d = String::from("M");
    at(&mut d, 0.0, -spread);
    d.push('C');
    at(&mut d, reach, -spread * 2.0);
    d.push(' ');
    at(&mut d, reach, spread * 2.0);
    d.push(' ');
    at(&mut d, tx, spread + ty);
    EdgePath {
        d,
        edx: if sideways { -spread / len } else { -reach / len },
        edy: if sideways { -reach / len } else { -spread / len },
        ex: if sideways { c.x + spread } else { c.x + w / 2.0 },
        ey: if sideways { c.y + h / 2.0 } else { c.y + spread },
        sdx: if sideways { 0.0 } else { -1.0 },
        sdy: if sideways { -1.0 } else { 0.0 },
        sx: if sideways { c.x - spread } else { c.x + w / 2.0 },
        sy: if sideways { c.y + h / 2.0 } else { c.y - spread },
    }
}
