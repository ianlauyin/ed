//! Lays out nested groups, each in its own direction, on top of the layered layout.
//! Port of Pele's `src/layout/compound.ts`.
//!
//! Each group is laid out on its own and placed in its parent as a single box. An edge that
//! crosses a group's border passes through a port on the side facing its other end.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::layout::layered::Groups;
use crate::layout::layered::Kind;
use crate::layout::layered::LEdge;
use crate::layout::layered::LNode;
use crate::layout::layered::LayeredOptions;
use crate::layout::layered::LayeredResult;
use crate::layout::layered::ease;
use crate::layout::layered::hypot;
use crate::layout::layered::layered;
use crate::layout::layered::layered_with;
use crate::layout::layered::ledge;
use crate::layout::layered::lnode;
use crate::layout::layered::orient;
use crate::layout::layered::rank;

/// A flow direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dir {
    /// Bottom to top.
    Bt,
    /// Left to right.
    Lr,
    /// Right to left.
    Rl,
    /// Top to bottom.
    Tb,
}

/// The direction a diagram names, or top to bottom for anything else.
pub(crate) fn direction(dir: Option<&str>) -> Dir {
    match dir {
        Some("BT") => Dir::Bt,
        Some("LR") => Dir::Lr,
        Some("RL") => Dir::Rl,
        _ => Dir::Tb,
    }
}

/// A node or group in a compound layout. Mirrors Pele's `CNode`.
#[derive(Clone, Debug)]
pub(crate) struct CNode {
    /// The direction inside a group, when it sets its own.
    pub(crate) dir: Option<Dir>,
    /// Height.
    pub(crate) h: f64,
    /// Whether the node is a group.
    pub(crate) is_group: bool,
    /// Least width of a group.
    pub(crate) min_w: f64,
    /// Padding below a group's members.
    pub(crate) pad_bottom: f64,
    /// Padding beside a group's members.
    pub(crate) pad_x: f64,
    /// Padding above a group's members.
    pub(crate) pad_top: f64,
    /// Index of the enclosing group, or -1 at the top level.
    pub(crate) parent: i32,
    /// Declaration order; a group takes the smallest value among its members.
    pub(crate) seq: f64,
    /// How much of a side its edges may spread along, measured across the flow; 0 keeps them at
    /// its middle.
    pub(crate) span: f64,
    /// Width.
    pub(crate) w: f64,
    /// Center x after layout.
    pub(crate) x: f64,
    /// Center y after layout.
    pub(crate) y: f64,
}

/// An edge in a compound layout. Mirrors Pele's `CEdge`.
#[derive(Clone, Debug, Default)]
pub(crate) struct CEdge {
    /// Index of the head node.
    pub(crate) dst: usize,
    /// Label height.
    pub(crate) label_h: f64,
    /// Label width.
    pub(crate) label_w: f64,
    /// Label center x after layout.
    pub(crate) label_x: f64,
    /// Label center y after layout.
    pub(crate) label_y: f64,
    /// Least number of ranks the edge spans.
    pub(crate) minlen: u32,
    /// `x, y, axis` triples. Axis is 0 where the edge runs vertically into the point, 1
    /// horizontally.
    pub(crate) route: Vec<f64>,
    /// Index of the tail node.
    pub(crate) src: usize,
}

/// The size of a compound layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CompoundResult {
    /// Height.
    pub(crate) height: f64,
    /// Width.
    pub(crate) width: f64,
}

/// A side of a box, clockwise from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// The bottom side.
    Bottom,
    /// The left side.
    Left,
    /// The right side.
    Right,
    /// The top side.
    Top,
}

/// Where an edge crosses the border of a group, as seen from inside the group.
struct PortEdge {
    /// The edge.
    e: usize,
    /// Where the edge goes across the flow outside the group, from an earlier layout; `NaN` if
    /// unknown.
    hint: f64,
    /// The member of the group that the edge leaves or reaches.
    inner: usize,
    /// Whether the edge leaves the group here.
    out: bool,
    /// Where the port sits on the group's border, in the group's box.
    px: f64,
    /// Where the port sits on the group's border, in the group's box.
    py: f64,
    /// The edge's route inside the group's box, between the port and `inner`.
    route: Vec<f64>,
    /// The side of the group the port is on.
    side: Side,
}

/// An edge between two items of one level.
struct LevelEdge {
    /// The item at the tail end.
    a: usize,
    /// The item at the head end.
    b: usize,
    /// The edge.
    e: usize,
}

/// The diagram or a group, laid out on its own.
struct Level {
    /// The direction of its flow.
    dir: Dir,
    /// The edges between its items.
    edges: Vec<LevelEdge>,
    /// The group, or `None` for the diagram.
    id: Option<usize>,
    /// Its members, in declaration order.
    items: Vec<usize>,
    /// Each port by `edge * 2`, plus 1 where the edge arrives, as an index into `ports`.
    port_of: BTreeMap<usize, usize>,
    /// The edges that cross its border.
    ports: Vec<PortEdge>,
    /// Left of its box in diagram coordinates.
    x: f64,
    /// Top of its box in diagram coordinates.
    y: f64,
}

/// Past this many group border crossings, edges are drawn between the groups that hold their
/// ends.
const MAX_PORTS: usize = 40000;

/// Past this many group border crossings, a drawing is laid out only once.
const MAX_ORDERED_PORTS: usize = 400;

/// Past this many edges, a drawing is laid out only once.
const MAX_ORDERED_EDGES: usize = 500;

/// Each group orders itself when the routes cross more than this many times as often as the
/// flat layout's order does, plus `FLAT_SPARE`.
const FLAT_SLACK: f64 = 1.5;

/// See `FLAT_SLACK`.
const FLAT_SPARE: f64 = 4.0;

/// Crossings this near a node that both edges meet are not counted.
const FAN: f64 = 24.0;

/// How far out of line two points may be and still count as in line.
const IN_LINE: f64 = 0.5;

/// The straight pieces that a curve between two points is counted as.
const CURVE_PIECES: usize = 3;

/// The straight pieces that a rounded corner is counted as.
const CORNER_PIECES: usize = 2;

/// The most pieces one step of a route is counted as: a step, an S, or a straight run and a
/// corner.
const STEP_PIECES: usize = {
    let most = if CURVE_PIECES > 3 { CURVE_PIECES } else { 3 };
    if CORNER_PIECES + 1 > most { CORNER_PIECES + 1 } else { most }
};

/// How far along its legs the handles of a rounded corner reach, as the drawing has them.
const HANDLE: f64 = 0.62;

/// Past this many pieces in a band, they are sorted rather than put in place one by one.
const MAX_INSERT: usize = 128;

/// How the drawing joins the points of a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// Curves, with corners rounded where a route turns at right angles.
    Curved,
    /// Straight lines from point to point.
    Linear,
    /// Steps at right angles.
    Stepped,
}

/// The style that a layout with `opt` is drawn in.
pub(crate) fn style_of(opt: &LayeredOptions) -> Style {
    if opt.tracks {
        Style::Stepped
    } else if opt.straight {
        Style::Linear
    } else {
        Style::Curved
    }
}

/// An index or count as a float.
#[expect(clippy::cast_precision_loss, reason = "indices and counts stay far below 2^52")]
fn float(i: usize) -> f64 {
    i as f64
}

/// The whole number at or below `x`, which is not negative.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "callers pass small non-negative whole numbers"
)]
fn whole(x: f64) -> usize {
    x as usize
}

/// The pieces of routes as straight lines, each from its left end (`ax`, `ay`) to its right
/// (`bx`, `by`).
struct Lines {
    /// Left end x.
    ax: Vec<f64>,
    /// Left end y.
    ay: Vec<f64>,
    /// Right end x.
    bx: Vec<f64>,
    /// Right end y.
    by: Vec<f64>,
    /// Whether each piece is its route's first (1), last (2), or both.
    ends: Vec<u8>,
    /// Where the last piece added starts.
    from: (f64, f64),
    /// The larger y of each piece.
    high: Vec<f64>,
    /// The smaller y of each piece.
    low: Vec<f64>,
    /// The edge of each piece.
    owner: Vec<usize>,
    /// Where the last piece added ends.
    to: (f64, f64),
    /// The largest y of any piece.
    bottom: f64,
    /// The smallest y of any piece.
    top: f64,
}

impl Lines {
    /// Adds the piece of edge `ei` from `x0, y0` to `x1, y1`. A piece that carries on the last
    /// one straight is joined to it.
    #[expect(clippy::float_cmp, reason = "Pele compares points exactly")]
    fn add(&mut self, ei: usize, part: u8, mut x0: f64, mut y0: f64, x1: f64, y1: f64) {
        let on =
            x0 == self.to.0 && y0 == self.to.1 && ((y1 == y0 && self.from.1 == y0) || (x1 == x0 && self.from.0 == x0));
        if on
            && self.owner.last() == Some(&ei)
            && self.ends.last() == Some(&part)
            && (x0 - self.from.0) * (x1 - x0) + (y0 - self.from.1) * (y1 - y0) > 0.0
        {
            for list in [&mut self.ax, &mut self.ay, &mut self.bx, &mut self.by, &mut self.low, &mut self.high] {
                list.pop();
            }
            self.ends.pop();
            self.owner.pop();
            (x0, y0) = self.from;
        }
        self.from = (x0, y0);
        self.to = (x1, y1);
        self.ends.push(part);
        self.ax.push(x0.min(x1));
        self.bx.push(x0.max(x1));
        self.ay.push(if x0 <= x1 { y0 } else { y1 });
        self.by.push(if x0 <= x1 { y1 } else { y0 });
        let (low, high) = (y0.min(y1), y0.max(y1));
        self.low.push(low);
        self.high.push(high);
        if low < self.top {
            self.top = low;
        }
        if high > self.bottom {
            self.bottom = high;
        }
        self.owner.push(ei);
    }
}

/// How often the routes of different edges cross, drawn in `style`. Crossings where two edges
/// fan out from or into a node they share are not counted: near the node, where both are on
/// their first or last piece, or where both are on the straight run that joins them to it.
#[expect(clippy::float_cmp, reason = "Pele compares route axes exactly")]
#[expect(clippy::many_single_char_names, reason = "names mirror Pele's compound.ts for side-by-side review")]
pub(crate) fn route_crossings(nodes: &[CNode], edges: &[CEdge], style: Style) -> usize {
    let mut size = 0;
    for e in edges {
        if e.src != e.dst && e.route.len() > 3 {
            size += (e.route.len() / 3 - 1) * STEP_PIECES;
        }
    }
    let mut lines = Lines {
        ax: Vec::with_capacity(size),
        ay: Vec::with_capacity(size),
        bx: Vec::with_capacity(size),
        by: Vec::with_capacity(size),
        ends: Vec::with_capacity(size),
        from: (f64::NAN, f64::NAN),
        high: Vec::with_capacity(size),
        low: Vec::with_capacity(size),
        owner: Vec::with_capacity(size),
        to: (f64::NAN, f64::NAN),
        bottom: f64::NEG_INFINITY,
        top: f64::INFINITY,
    };
    for (ei, e) in edges.iter().enumerate() {
        let r = &e.route;
        let len = r.len() / 3 * 3;
        if e.src == e.dst || len < 6 {
            continue;
        }
        let aligned = |k: usize| (r[k] - r[k - 3]).abs() < 0.01 || (r[k + 1] - r[k - 2]).abs() < 0.01;
        let length = |k: usize| (r[k] - r[k - 3]).abs() + (r[k + 1] - r[k - 2]).abs();
        let turns = |k: usize| {
            style == Style::Curved
                && k > 0
                && k + 3 < len
                && aligned(k)
                && aligned(k + 3)
                && length(k) >= 0.01
                && length(k + 3) >= 0.01
                && ((r[k] - r[k - 3]).abs() < 0.01) != ((r[k + 3] - r[k]).abs() < 0.01)
        };
        let mut at = (r[0], r[1]);
        for k in (3..len).step_by(3) {
            let part = u8::from(k == 3) | if k == len - 3 { 2 } else { 0 };
            let (x0, y0) = at;
            let (x1, y1) = (r[k], r[k + 1]);
            at = (x1, y1);
            if aligned(k) || style == Style::Linear {
                if !turns(k) {
                    lines.add(ei, part, x0, y0, x1, y1);
                    continue;
                }
                // Two corners next to each other share the leg between them, half each.
                let before = if turns(k - 3) { length(k) / 2.0 } else { length(k) };
                let after = if turns(k + 3) { length(k + 3) / 2.0 } else { length(k + 3) };
                let ux = (x1 - r[k - 3]) / length(k);
                let uy = (y1 - r[k - 2]) / length(k);
                let vx = (r[k + 3] - x1) / length(k + 3);
                let vy = (r[k + 4] - y1) / length(k + 3);
                let sx = x1 - ux * before;
                let sy = y1 - uy * before;
                lines.add(ei, part, x0, y0, sx, sy);
                let c1x = x1 - ux * before * (1.0 - HANDLE);
                let c1y = y1 - uy * before * (1.0 - HANDLE);
                let c2x = x1 + vx * after * (1.0 - HANDLE);
                let c2y = y1 + vy * after * (1.0 - HANDLE);
                at = (x1 + vx * after, y1 + vy * after);
                let turn = part | if k + 3 == len - 3 { 2 } else { 0 };
                let (mut px, mut py) = (sx, sy);
                for i in 1..=CORNER_PIECES {
                    let t = float(i) / float(CORNER_PIECES);
                    let u = 1.0 - t;
                    let x = u * u * u * sx + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * at.0;
                    let y = u * u * u * sy + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * at.1;
                    lines.add(ei, turn, px, py, x, y);
                    px = x;
                    py = y;
                }
                continue;
            }
            let down = r[k + 2] == 0.0;
            if style == Style::Stepped {
                let mx = if down { x0 } else { f64::midpoint(x0, x1) };
                let my = if down { f64::midpoint(y0, y1) } else { y0 };
                let (nx, ny) = if down { (x1, my) } else { (mx, y1) };
                lines.add(ei, part, x0, y0, mx, my);
                lines.add(ei, part, mx, my, nx, ny);
                lines.add(ei, part, nx, ny, x1, y1);
                continue;
            }
            // The S-curve the drawing has, as a few straight pieces.
            let c = if down { ease(x1 - x0, y1 - y0) } else { ease(y1 - y0, x1 - x0) };
            let (mut px, mut py) = (x0, y0);
            for i in 1..=CURVE_PIECES {
                let t = float(i) / float(CURVE_PIECES);
                let u = 1.0 - t;
                let across = t * t * (3.0 - 2.0 * t);
                let along = 3.0 * c * t * u * u + 3.0 * (1.0 - c) * t * t * u + t * t * t;
                let x = x0 + (x1 - x0) * if down { across } else { along };
                let y = y0 + (y1 - y0) * if down { along } else { across };
                lines.add(ei, part, px, py, x, y);
                px = x;
                py = y;
            }
        }
    }
    let Lines { ax, ay, bx, by, ends, high, low, owner, bottom, top, .. } = lines;
    let count = ax.len();

    // The pieces go into bands across the drawing, and each pair is counted only in the first
    // band they share.
    let bands = if bottom > top { whole(float(count).sqrt().round()) } else { 1 };
    let scale = if bottom > top { float(bands) / (bottom - top) } else { 0.0 };
    let band_of = |y: f64| whole(((y - top) * scale).floor()).min(bands - 1);
    let mut first = vec![0; count];
    let mut band_start = vec![0usize; bands + 1];
    for p in 0..count {
        first[p] = band_of(low[p]);
        for b in first[p]..=band_of(high[p]) {
            band_start[b + 1] += 1;
        }
    }
    for b in 0..bands {
        band_start[b + 1] += band_start[b];
    }
    let mut fill = band_start[..bands].to_vec();
    let mut in_band = vec![0; band_start[bands]];
    for p in 0..count {
        for b in first[p]..=band_of(high[p]) {
            in_band[fill[b]] = p;
            fill[b] += 1;
        }
    }

    // Where each edge runs straight out of its first point and into its last, for edges that
    // merge into one run at a node they share.
    let mut runs = vec![0.0; edges.len() * 8];
    let in_line = |r: &[f64], a: usize, b: usize, c: usize| {
        ((r[a] - r[b]).abs() < IN_LINE && (r[a] - r[c]).abs() < IN_LINE)
            || ((r[a + 1] - r[b + 1]).abs() < IN_LINE && (r[a + 1] - r[c + 1]).abs() < IN_LINE)
    };
    for (ei, e) in edges.iter().enumerate() {
        let r = &e.route;
        let len = r.len() / 3 * 3;
        if len < 6 {
            continue;
        }
        let mut k = 3;
        while k + 3 < len && in_line(r, 0, k, k + 3) {
            k += 3;
        }
        let last = len - 3;
        let mut j = last - 3;
        while j >= 3 && in_line(r, last, j, j - 3) {
            j -= 3;
        }
        runs[ei * 8..ei * 8 + 8].copy_from_slice(&[r[0], r[1], r[k], r[k + 1], r[j], r[j + 1], r[last], r[last + 1]]);
    }
    let on_run = |ei: usize, end: u8, x: f64, y: f64| {
        let o = ei * 8 + if end == 1 { 0 } else { 4 };
        let (x0, y0) = (runs[o], runs[o + 1]);
        let dx = runs[o + 2] - x0;
        let dy = runs[o + 3] - y0;
        let length = dx * dx + dy * dy;
        let t = if length > 0.0 { (((x - x0) * dx + (y - y0) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
        hypot(x - x0 - t * dx, y - y0 - t * dy) < IN_LINE
    };

    let mut total = 0;
    for band in 0..bands {
        let (start, end) = (band_start[band], band_start[band + 1]);
        let list = &mut in_band[start..end];
        if list.len() > MAX_INSERT {
            list.sort_by(|&p, &q| ax[p].partial_cmp(&ax[q]).unwrap_or(Ordering::Equal).then(p.cmp(&q)));
        } else {
            for i in 1..list.len() {
                let p = list[i];
                let mut j = i;
                while j > 0 && ax[list[j - 1]] > ax[p] {
                    list[j] = list[j - 1];
                    j -= 1;
                }
                list[j] = p;
            }
        }
        let list = &in_band[start..end];
        for (s, &p) in list.iter().enumerate() {
            let dx = bx[p] - ax[p];
            let dy = by[p] - ay[p];
            let here = first[p] == band;
            for &q in &list[s + 1..] {
                if ax[q] > bx[p] {
                    break;
                }
                if high[q] < low[p] || low[q] > high[p] || owner[p] == owner[q] || (!here && first[q] != band) {
                    continue;
                }
                let d1 = dx * (ay[q] - ay[p]) - dy * (ax[q] - ax[p]);
                let d2 = dx * (by[q] - ay[p]) - dy * (bx[q] - ax[p]);
                // A point on the line counts as on one side, so that a crossing where two pieces
                // meet is counted once.
                if (d1 > 0.0) == (d2 > 0.0) {
                    continue;
                }
                let ex = bx[q] - ax[q];
                let ey = by[q] - ay[q];
                let d3 = ex * (ay[p] - ay[q]) - ey * (ax[p] - ax[q]);
                let d4 = ex * (by[p] - ay[q]) - ey * (bx[p] - ax[q]);
                if (d3 > 0.0) == (d4 > 0.0) {
                    continue;
                }
                let ep = &edges[owner[p]];
                let eq = &edges[owner[q]];
                let at = d3 / (d3 - d4);
                let x = ax[p] + at * dx;
                let y = ay[p] + at * dy;
                let fan = |v: usize, pe: u8, qe: u8| {
                    (ends[p] & pe != 0 && ends[q] & qe != 0)
                        || ((x - nodes[v].x).abs() < nodes[v].w / 2.0 + FAN
                            && (y - nodes[v].y).abs() < nodes[v].h / 2.0 + FAN)
                        || (on_run(owner[p], pe, x, y) && on_run(owner[q], qe, x, y))
                };
                if (ep.src == eq.src && fan(ep.src, 1, 1))
                    || (ep.src == eq.dst && fan(ep.src, 1, 2))
                    || (ep.dst == eq.src && fan(ep.dst, 2, 1))
                    || (ep.dst == eq.dst && fan(ep.dst, 2, 2))
                {
                    continue;
                }
                total += 1;
            }
        }
    }
    total
}

/// Moves a finished layout by `ox`, `oy`: nodes, routes, and labels.
pub(crate) fn shift_layout(nodes: &mut [CNode], edges: &mut [CEdge], ox: f64, oy: f64) {
    for c in nodes.iter_mut() {
        c.x += ox;
        c.y += oy;
    }
    for e in edges.iter_mut() {
        for point in e.route.chunks_exact_mut(3) {
            point[0] += ox;
            point[1] += oy;
        }
        e.label_x += ox;
        e.label_y += oy;
    }
}

/// A node of the given size inside `parent`, as Pele's `cnode` creates it.
pub(crate) fn cnode(w: f64, h: f64, parent: i32) -> CNode {
    CNode {
        dir: None,
        h,
        is_group: false,
        min_w: 0.0,
        pad_bottom: 0.0,
        pad_x: 0.0,
        pad_top: 0.0,
        parent,
        seq: 0.0,
        span: 0.0,
        w,
        x: 0.0,
        y: 0.0,
    }
}

/// The side that a flow starts from.
fn start_side(dir: Dir) -> Side {
    match dir {
        Dir::Tb => Side::Top,
        Dir::Bt => Side::Bottom,
        Dir::Lr => Side::Left,
        Dir::Rl => Side::Right,
    }
}

/// The side across from `side`.
fn opposite(side: Side) -> Side {
    match side {
        Side::Top => Side::Bottom,
        Side::Right => Side::Left,
        Side::Bottom => Side::Top,
        Side::Left => Side::Right,
    }
}

/// Adds a level for group `id`, or the diagram for `None`, and returns its index.
fn push_level(levels: &mut Vec<Level>, id: Option<usize>, dir: Dir, items: Vec<usize>) -> usize {
    levels.push(Level {
        dir,
        edges: Vec::new(),
        id,
        items,
        port_of: BTreeMap::new(),
        ports: Vec::new(),
        x: 0.0,
        y: 0.0,
    });
    levels.len() - 1
}

/// Places the items of the levels from `from` on, breadth first, adding a level for each group
/// among them, which comes in its parent's direction unless it sets its own.
fn descend(
    levels: &mut Vec<Level>,
    level_of: &mut [Option<usize>],
    depth: &mut [usize],
    placed: &mut [bool],
    nodes: &[CNode],
    children: &[Vec<usize>],
    from: usize,
) {
    let mut q = from;
    while q < levels.len() {
        let (dir, id) = (levels[q].dir, levels[q].id);
        for k in 0..levels[q].items.len() {
            let item = levels[q].items[k];
            placed[item] = true;
            depth[item] = id.map_or(0, |id| depth[id] + 1);
            if nodes[item].is_group {
                let dir = nodes[item].dir.unwrap_or(dir);
                level_of[item] = Some(push_level(levels, Some(item), dir, children[item].clone()));
            }
        }
        q += 1;
    }
}

/// The key of an edge's port in `Level::port_of`.
fn port_key(e: usize, out: bool) -> usize {
    e * 2 + usize::from(!out)
}

/// The port that edge `e` passes through on the border of group `item`.
fn port_of<'a>(
    levels: &'a [Level],
    level_of: &[Option<usize>],
    item: usize,
    e: usize,
    out: bool,
) -> Option<&'a PortEdge> {
    let level = levels.get(level_of.get(item).copied().flatten()?)?;
    level.ports.get(*level.port_of.get(&port_key(e, out))?)
}

/// Whether the edge's end lies below `item` rather than at it.
fn endpoint_below(item: usize, e: &CEdge, out: bool) -> bool {
    (if out { e.src } else { e.dst }) != item
}

/// The enclosing group of a node whose parent has been checked.
fn parent_of(nodes: &[CNode], i: usize) -> Option<usize> {
    usize::try_from(nodes[i].parent).ok()
}

/// The level that holds the members of `parent`, the diagram's for `None`.
fn level_index(level_of: &[Option<usize>], parent: Option<usize>) -> Option<usize> {
    parent.map_or(Some(0), |p| level_of.get(p).copied().flatten())
}

/// The layered graph of one level: its items, the edges between them, and an edge to a port
/// node for each crossing on the start or end side. `dummies` gives each port's node. Without
/// `sized`, every node is a point, which is enough to rank them. With `flat`, every node starts
/// from its place in the flat layout, and a port from the place of the edge's other end there.
#[expect(clippy::too_many_arguments, reason = "mirrors Pele's closure, which reads these from its scope")]
fn build(
    levels: &[Level],
    level_of: &[Option<usize>],
    q: usize,
    nodes: &[CNode],
    edges: &[CEdge],
    local: &mut [usize],
    sized: bool,
    flat: Option<&Flat>,
) -> (Vec<LNode>, Vec<LEdge>, Vec<Option<usize>>) {
    let level = &levels[q];
    let vertical = matches!(level.dir, Dir::Tb | Dir::Bt);
    let start = start_side(level.dir);
    let mut lnodes = Vec::with_capacity(level.items.len() + level.ports.len());
    let mut ledges = Vec::with_capacity(level.edges.len() + level.ports.len());
    for (i, &item) in level.items.iter().enumerate() {
        local[item] = i;
        let node = &nodes[item];
        let mut ln = if sized {
            let (w, h) = if vertical { (node.w, node.h) } else { (node.h, node.w) };
            lnode(w, h, Kind::Node)
        } else {
            lnode(0.0, 0.0, Kind::Node)
        };
        ln.span = if node.is_group { node.span.min(ln.w - 2.0 * node.pad_x) } else { node.span };
        ln.key = flat.map_or(f64::NAN, |flat| flat.key[item]);
        lnodes.push(ln);
    }
    let held = |item: usize, ei: usize, out: bool| nodes[item].is_group && endpoint_below(item, &edges[ei], out);
    let offset = |item: usize, ei: usize, out: bool| {
        if !sized || !held(item, ei, out) {
            return 0.0;
        }
        port_of(levels, level_of, item, ei, out)
            .map_or(0.0, |port| if vertical { port.px - nodes[item].w / 2.0 } else { port.py - nodes[item].h / 2.0 })
    };
    for le in &level.edges {
        let e = &edges[le.e];
        let mut edge = ledge(local[le.a], local[le.b], e.minlen, 0.0, 0.0);
        if sized {
            edge.label_w = if vertical { e.label_w } else { e.label_h };
            edge.label_h = if vertical { e.label_h } else { e.label_w };
            edge.tail_dx = offset(le.a, le.e, true);
            edge.head_dx = offset(le.b, le.e, false);
            edge.tail_fixed = held(le.a, le.e, true);
            edge.head_fixed = held(le.b, le.e, false);
        }
        ledges.push(edge);
    }
    let mut dummies = vec![None; level.ports.len()];
    for (i, port) in level.ports.iter().enumerate() {
        if port.side == start || port.side == opposite(start) {
            let d = lnodes.len();
            let kind = if port.side == start { Kind::StartPort } else { Kind::EndPort };
            let mut ln = lnode(0.0, 0.0, kind);
            let e = &edges[port.e];
            ln.key = flat.map_or(f64::NAN, |flat| flat.key[if port.out { e.dst } else { e.src }]);
            ln.hint = match flat {
                Some(flat) if port.hint.is_nan() => flat.far_hint(level, port, edges),
                _ => port.hint,
            };
            lnodes.push(ln);
            dummies[i] = Some(d);
            let inner = local[port.inner];
            let mut edge = if port.out { ledge(inner, d, 1, 0.0, 0.0) } else { ledge(d, inner, 1, 0.0, 0.0) };
            if port.out {
                edge.tail_dx = offset(port.inner, port.e, true);
                edge.tail_fixed = held(port.inner, port.e, true);
            } else {
                edge.head_dx = offset(port.inner, port.e, false);
                edge.head_fixed = held(port.inner, port.e, false);
            }
            ledges.push(edge);
        } else if let Some(ln) = lnodes.get_mut(local[port.inner]) {
            ln.pin = if matches!(port.side, Side::Top | Side::Left) { -1.0 } else { 1.0 };
        }
    }
    (lnodes, ledges, dummies)
}

/// Lays out a graph whose nodes may be nested in groups. Sets every node's center and size of
/// every group, and every edge's route and label position, and returns the size of the
/// drawing. A parent that is not a group, or groups that contain each other, put the node at
/// the top level. Past `MAX_PORTS` border crossings, an edge's `src` and `dst` move to the
/// groups that hold its ends. An edge whose end names no node makes the layout empty, where
/// Pele throws.
pub(crate) fn compound_layout(
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    root_dir: Dir,
    opt: &LayeredOptions,
) -> CompoundResult {
    let n = nodes.len();
    if edges.iter().any(|e| e.src >= n || e.dst >= n) {
        return CompoundResult::default();
    }
    let mut levels: Vec<Level> = Vec::new();
    let mut level_of: Vec<Option<usize>> = vec![None; n];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut root_items = Vec::new();
    for i in 0..n {
        let p = usize::try_from(nodes[i].parent).ok().filter(|&p| p < n);
        match p {
            Some(p) if p != i && nodes[p].is_group => children[p].push(i),
            _ => {
                nodes[i].parent = -1;
                root_items.push(i);
            }
        }
    }
    push_level(&mut levels, None, root_dir, root_items);
    let mut depth = vec![0usize; n];
    let mut placed = vec![false; n];
    descend(&mut levels, &mut level_of, &mut depth, &mut placed, nodes, &children, 0);
    // Groups that contain each other are unreachable from the root; lift them out.
    for i in 0..n {
        if placed[i] {
            continue;
        }
        if let Some(former) = parent_of(nodes, i) {
            children[former].retain(|&c| c != i);
        }
        nodes[i].parent = -1;
        levels[0].items.push(i);
        placed[i] = true;
        depth[i] = 0;
        if nodes[i].is_group {
            let from = levels.len();
            let dir = nodes[i].dir.unwrap_or(root_dir);
            level_of[i] = Some(push_level(&mut levels, Some(i), dir, children[i].clone()));
            descend(&mut levels, &mut level_of, &mut depth, &mut placed, nodes, &children, from);
        }
    }

    for q in (1..levels.len()).rev() {
        let mut seq = f64::INFINITY;
        for &item in &levels[q].items {
            if nodes[item].seq < seq {
                seq = nodes[item].seq;
            }
        }
        if seq != f64::INFINITY
            && let Some(id) = levels[q].id
        {
            nodes[id].seq = seq;
        }
    }
    for level in &mut levels {
        level.items.sort_by(|&a, &b| {
            nodes[a].seq.partial_cmp(&nodes[b].seq).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
        });
    }

    // Every group border an edge crosses costs a port. Past the budget, an edge is drawn
    // between the groups that hold its ends, at the level they share.
    let up = |i: usize| parent_of(nodes, i).unwrap_or(i);
    let mut ports = 0;
    for e in edges.iter_mut() {
        let mut a = e.src;
        let mut b = e.dst;
        let mut crossed = 0;
        while depth[a] > depth[b] {
            a = up(a);
            crossed += 1;
        }
        while depth[b] > depth[a] {
            b = up(b);
            crossed += 1;
        }
        while a != b && nodes[a].parent != nodes[b].parent {
            a = up(a);
            b = up(b);
            crossed += 2;
        }
        if a == b {
            continue;
        }
        if ports + crossed > MAX_PORTS {
            e.src = a;
            e.dst = b;
        } else {
            ports += crossed;
        }
    }

    let mut loops = Vec::new();
    let mut stray = Vec::new();
    let mut a_path = Vec::new();
    let mut b_path = Vec::new();
    for (ei, e) in edges.iter_mut().enumerate() {
        e.route = Vec::new();
        if e.src == e.dst {
            loops.push(ei);
            continue;
        }
        let mut a = e.src;
        let mut b = e.dst;
        a_path.clear();
        b_path.clear();
        while depth[a] > depth[b] {
            a_path.push(a);
            a = up(a);
        }
        while depth[b] > depth[a] {
            b_path.push(b);
            b = up(b);
        }
        if a == b {
            stray.push(ei);
            continue;
        }
        while nodes[a].parent != nodes[b].parent {
            a_path.push(a);
            b_path.push(b);
            a = up(a);
            b = up(b);
        }
        if let Some(q) = level_index(&level_of, parent_of(nodes, a)) {
            levels[q].edges.push(LevelEdge { a, b, e: ei });
        }
        for (path, out) in [(&a_path, true), (&b_path, false)] {
            for &inner in path {
                let Some(q) = level_index(&level_of, parent_of(nodes, inner)) else {
                    continue;
                };
                let level = &mut levels[q];
                level.port_of.insert(port_key(ei, out), level.ports.len());
                level.ports.push(PortEdge {
                    e: ei,
                    hint: f64::NAN,
                    inner,
                    out,
                    px: 0.0,
                    py: 0.0,
                    route: Vec::new(),
                    side: Side::Top,
                });
            }
        }
    }

    // Small drawings are laid out more than once, to order their groups and ports well.
    let ordered = ports > 1 && ports <= MAX_ORDERED_PORTS && edges.len() <= MAX_ORDERED_EDGES;
    // A layout of the whole drawing, groups left out, orders the nodes and ports of every group
    // as one. Only where every group flows the way the drawing does.
    let flat = (levels.len() > 1 && ordered && levels.iter().all(|level| level.dir == root_dir))
        .then(|| flat_order(nodes, edges, root_dir, opt));

    let mut placer =
        Placer { laid: Vec::new(), level_of, levels, local: vec![0usize; n], opt, ordered, routes: Vec::new() };
    placer.laid.resize_with(placer.levels.len(), Vec::new);
    let style = style_of(opt);
    let mut keyed = flat.is_some();
    let mut total = placer.place(nodes, edges, flat.as_ref().map(|(flat, _)| flat));
    // The flat layout leaves out the groups' boxes, so each group ordering itself may cross less.
    let mut crossings = None;
    if let Some((_, flat_crossings)) = &flat {
        let first = route_crossings(nodes, edges, style);
        crossings = Some(first);
        if float(first) > float(*flat_crossings) * FLAT_SLACK + FLAT_SPARE {
            let kept = snapshot(nodes, edges, &placer.levels, total);
            keyed = false;
            total = placer.place(nodes, edges, None);
            let own = route_crossings(nodes, edges, style);
            if own >= first {
                keyed = true;
                total = restore(&kept, nodes, edges, &mut placer.levels);
            } else {
                crossings = Some(own);
            }
        }
    }

    // Lay the groups out again with their ports in the order of where their edges turn outside,
    // where they are out of that order. Not where each group's own order won: it seldom helps
    // enough to pay for another layout.
    if keyed == flat.is_some() && ordered && placer.order_ports(nodes, edges) {
        let before = crossings.unwrap_or_else(|| route_crossings(nodes, edges, style));
        if before > 0 {
            let kept = snapshot(nodes, edges, &placer.levels, total);
            total = placer.place(nodes, edges, flat.as_ref().filter(|_| keyed).map(|(flat, _)| flat));
            if route_crossings(nodes, edges, style) >= before {
                total = restore(&kept, nodes, edges, &mut placer.levels);
            }
        }
    }

    for ei in stray {
        let e = &mut edges[ei];
        let a = &nodes[e.src];
        let b = &nodes[e.dst];
        e.route = vec![a.x, a.y, 0.0, b.x, b.y, 0.0];
        e.label_x = f64::midpoint(a.x, b.x);
        e.label_y = f64::midpoint(a.y, b.y);
    }
    for ei in loops {
        let e = &mut edges[ei];
        let node = &nodes[e.src];
        e.label_x = node.x + node.w / 2.0 + 28.0 + e.label_w / 2.0;
        e.label_y = node.y;
    }

    total
}

/// Whether two inputs are the same as JavaScript's `Object.is` tells: `NaN` is itself, and 0 is
/// not -0.
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// The layout of a whole drawing, groups left out, that orders the nodes and ports of each
/// group.
struct Flat {
    /// Each node's place across the flow: a leaf's own, a group's the mean of its leaves'.
    key: Vec<f64>,
    /// The last rank that each group's leaves take, or -1.
    last: Vec<f64>,
    /// The flat layout's edge for each edge, if it has one.
    ledge_of: Vec<Option<usize>>,
    /// The flat layout's edges, whose points are `rank, place` pairs.
    ledges: Vec<LEdge>,
}

impl Flat {
    /// Where an edge that leaves a group through a port on its end side goes outside it: its
    /// place at the group's last rank, ties broken by its place at the next.
    #[expect(clippy::float_cmp, reason = "ranks are whole numbers")]
    fn far_hint(&self, level: &Level, port: &PortEdge, edges: &[CEdge]) -> f64 {
        let (Some(le), Some(id)) = (self.ledge_of[port.e], level.id) else {
            return f64::NAN;
        };
        if !port.out || port.side != opposite(start_side(level.dir)) {
            return f64::NAN;
        }
        let mut inside = self.key[edges[port.e].src];
        let mut outside = 0.0;
        for point in self.ledges[le].points.chunks_exact(2) {
            if point[0] == self.last[id] {
                inside = point[1];
            }
            if point[0] == self.last[id] + 1.0 {
                outside = point[1];
            }
        }
        inside + outside * 1e-6
    }
}

/// Lays out the leaves of the drawing as one flat graph, each group's leaves side by side, and
/// returns the order found with the fewer of its crossings and those of the order given.
fn flat_order(nodes: &[CNode], edges: &[CEdge], root_dir: Dir, opt: &LayeredOptions) -> (Flat, usize) {
    let n = nodes.len();
    let vertical = matches!(root_dir, Dir::Tb | Dir::Bt);
    let mut leaf = vec![None; n];
    let mut group = vec![None; n];
    let mut lnodes = Vec::new();
    let mut groups = Groups::default();
    for i in 0..n {
        if nodes[i].is_group {
            group[i] = Some(groups.parent.len());
            groups.parent.push(None);
        }
    }
    for i in 0..n {
        let p = parent_of(nodes, i).and_then(|p| group[p]);
        if let Some(g) = group[i] {
            groups.parent[g] = p;
        } else {
            leaf[i] = Some(lnodes.len());
            let node = &nodes[i];
            let (w, h) = if vertical { (node.w, node.h) } else { (node.h, node.w) };
            lnodes.push(lnode(w, h, Kind::Node));
            groups.of.push(p);
        }
    }
    let mut ledges = Vec::new();
    let mut ledge_of = vec![None; edges.len()];
    for (ei, e) in edges.iter().enumerate() {
        let (Some(a), Some(b)) = (leaf[e.src], leaf[e.dst]) else {
            continue;
        };
        if e.src == e.dst {
            continue;
        }
        ledge_of[ei] = Some(ledges.len());
        let (label_w, label_h) = if vertical { (e.label_w, e.label_h) } else { (e.label_h, e.label_w) };
        ledges.push(ledge(a, b, e.minlen, label_w, label_h));
    }
    let result = layered_with(&mut lnodes, &mut ledges, opt, Some(&groups), true);
    let mut key = vec![f64::NAN; n];
    let mut sum = vec![0.0; n];
    let mut members = vec![0usize; n];
    let mut last = vec![-1.0; n];
    for i in 0..n {
        let Some(l) = leaf[i] else {
            continue;
        };
        key[i] = lnodes[l].x;
        let rank = float(lnodes[l].rank);
        let mut p = parent_of(nodes, i);
        while let Some(at) = p {
            sum[at] += key[i];
            members[at] += 1;
            last[at] = f64::max(last[at], rank);
            p = parent_of(nodes, at);
        }
    }
    for i in 0..n {
        if members[i] > 0 {
            key[i] = sum[i] / float(members[i]);
        }
    }
    (Flat { key, last, ledge_of, ledges }, usize::try_from(result.crossings.min(result.declared)).unwrap_or(usize::MAX))
}

/// A level laid out before, with what it was laid out from.
struct Laid {
    /// The inputs: every node's and edge's fields that the layout reads.
    given: Vec<f64>,
    /// The edges after layout.
    ledges: Vec<LEdge>,
    /// The nodes after layout.
    lnodes: Vec<LNode>,
    /// The size of the layout.
    size: LayeredResult,
}

/// The levels of a compound layout and what laying them out needs.
struct Placer<'a> {
    /// The layouts of each level so far, when the drawing is laid out more than once.
    laid: Vec<Vec<Laid>>,
    /// The level of each group.
    level_of: Vec<Option<usize>>,
    /// The diagram and its groups, parents before children.
    levels: Vec<Level>,
    /// Each item's place in its level's layered graph.
    local: Vec<usize>,
    /// The options of every level's layout.
    opt: &'a LayeredOptions,
    /// Whether the drawing is laid out more than once, so that layouts are worth keeping.
    ordered: bool,
    /// Each edge's route inside the level that holds both its ends.
    routes: Vec<Vec<f64>>,
}

impl Placer<'_> {
    /// Lays out the layered graph of level `q`, or takes the layout of the same graph from
    /// before.
    fn lay_out(
        &mut self,
        q: usize,
        mut lnodes: Vec<LNode>,
        mut ledges: Vec<LEdge>,
    ) -> (Vec<LNode>, Vec<LEdge>, LayeredResult) {
        if !self.ordered {
            let size = layered(&mut lnodes, &mut ledges, self.opt);
            return (lnodes, ledges, size);
        }
        let mut given = Vec::with_capacity(lnodes.len() * 7 + ledges.len() * 9);
        for ln in &lnodes {
            given.extend([ln.w, ln.h, f64::from(ln.kind as u8), ln.pin, ln.hint, ln.key, ln.span]);
        }
        for le in &ledges {
            given.extend([
                float(le.tail),
                float(le.head),
                f64::from(le.minlen),
                le.label_w,
                le.label_h,
                le.tail_dx,
                le.head_dx,
                f64::from(u8::from(le.tail_fixed)),
                f64::from(u8::from(le.head_fixed)),
            ]);
        }
        let past = &mut self.laid[q];
        if let Some(p) =
            past.iter().find(|p| p.given.len() == given.len() && p.given.iter().zip(&given).all(|(&a, &b)| same(a, b)))
        {
            return (p.lnodes.clone(), p.ledges.clone(), p.size);
        }
        let size = layered(&mut lnodes, &mut ledges, self.opt);
        past.push(Laid { given, ledges: ledges.clone(), lnodes: lnodes.clone(), size });
        (lnodes, ledges, size)
    }

    /// Lays out every level and puts the drawing together: sets every node's center, every
    /// group's size, and every edge's route and label, and returns the size of the drawing. With
    /// `flat`, each level starts from the order of the flat layout.
    fn place(&mut self, nodes: &mut [CNode], edges: &mut [CEdge], flat: Option<&Flat>) -> CompoundResult {
        // Top-down: rank each level to learn which side of a group every crossing edge leaves
        // from.
        for q in 0..self.levels.len() {
            for k in 0..self.levels[q].ports.len() {
                let port = &self.levels[q].ports[k];
                let (inner, e, out, side) = (port.inner, port.e, port.out, port.side);
                if nodes[inner].is_group && endpoint_below(inner, &edges[e], out) {
                    set_side(&mut self.levels, &self.level_of, inner, e, out, side);
                }
            }
            if self.levels[q].edges.is_empty() {
                continue;
            }
            let needed = self.levels[q].edges.iter().any(|le| {
                let e = &edges[le.e];
                (nodes[le.a].is_group && e.src != le.a) || (nodes[le.b].is_group && e.dst != le.b)
            });
            if !needed {
                continue;
            }
            let (mut lnodes, mut ledges, _) =
                build(&self.levels, &self.level_of, q, nodes, edges, &mut self.local, false, flat);
            orient(&lnodes, &mut ledges);
            rank(&mut lnodes, &ledges, 1);
            let start = start_side(self.levels[q].dir);
            let count = self.levels[q].edges.len();
            for (i, ledge) in ledges.iter().enumerate().take(count) {
                let LevelEdge { a, b, e: ei } = self.levels[q].edges[i];
                let e = &edges[ei];
                let a_above = !ledge.reversed;
                if nodes[a].is_group && e.src != a {
                    let side = if a_above { opposite(start) } else { start };
                    set_side(&mut self.levels, &self.level_of, a, ei, true, side);
                }
                if nodes[b].is_group && e.dst != b {
                    let side = if a_above { start } else { opposite(start) };
                    set_side(&mut self.levels, &self.level_of, b, ei, false, side);
                }
            }
        }

        // Bottom-up: lay out each level now that the sizes of its groups are known.
        self.routes.clear();
        self.routes.resize(edges.len(), Vec::new());
        let mut total = CompoundResult::default();
        for q in (0..self.levels.len()).rev() {
            let group = self.levels[q].id;
            let dir = self.levels[q].dir;
            let vertical = matches!(dir, Dir::Tb | Dir::Bt);
            let flip = matches!(dir, Dir::Bt | Dir::Rl);
            let (lnodes, ledges, dummies) =
                build(&self.levels, &self.level_of, q, nodes, edges, &mut self.local, true, flat);
            let (lnodes, ledges, size) = self.lay_out(q, lnodes, ledges);
            let content_w = if vertical { size.width } else { size.height };
            let content_h = if vertical { size.height } else { size.width };
            let (pad_x, pad_top, pad_bottom, min_w) = group.map_or((0.0, 0.0, 0.0, 0.0), |g| {
                let g = &nodes[g];
                (g.pad_x, g.pad_top, g.pad_bottom, g.min_w)
            });
            let box_w = (content_w + 2.0 * pad_x).max(min_w);
            let box_h = content_h + pad_top + pad_bottom;
            let off_x = (box_w - content_w) / 2.0;
            let off_y = pad_top;
            let tx = |lx: f64, ly: f64| {
                off_x
                    + if vertical {
                        lx
                    } else if flip {
                        size.height - ly
                    } else {
                        ly
                    }
            };
            let ty = |lx: f64, ly: f64| {
                off_y
                    + if !vertical {
                        lx
                    } else if flip {
                        size.height - ly
                    } else {
                        ly
                    }
            };
            let axis = if vertical { 0.0 } else { 1.0 };
            let convert = |points: &[f64]| {
                let mut out = Vec::with_capacity(points.len() / 2 * 3);
                for p in points.chunks_exact(2) {
                    out.extend([tx(p[0], p[1]), ty(p[0], p[1]), axis]);
                }
                out
            };

            let level = &self.levels[q];
            for (i, &item) in level.items.iter().enumerate() {
                nodes[item].x = tx(lnodes[i].x, lnodes[i].y);
                nodes[item].y = ty(lnodes[i].x, lnodes[i].y);
            }
            for (i, le) in level.edges.iter().enumerate() {
                let edge = &ledges[i];
                self.routes[le.e] = convert(&edge.points);
                let e = &mut edges[le.e];
                if e.label_w > 0.0 {
                    e.label_x = tx(edge.label_x, edge.label_y);
                    e.label_y = ty(edge.label_x, edge.label_y);
                }
            }
            let mut k = level.edges.len();
            for (i, dummy) in dummies.iter().enumerate() {
                let port = &self.levels[q].ports[i];
                let (inner, ei, out, side) = (port.inner, port.e, port.out, port.side);
                let border = |x: f64, y: f64| match side {
                    Side::Top => (x, 0.0),
                    Side::Bottom => (x, box_h),
                    Side::Left => (0.0, y),
                    Side::Right => (box_w, y),
                };
                let (px, py, route) = if dummy.is_some() {
                    let mut route = ledges.get(k).map_or_else(Vec::new, |l| convert(&l.points));
                    k += 1;
                    let at = if out { route.len().saturating_sub(3) } else { 0 };
                    let (bx, by) =
                        border(route.get(at).copied().unwrap_or(0.0), route.get(at + 1).copied().unwrap_or(0.0));
                    if out {
                        route.extend([bx, by, axis]);
                    } else {
                        route.splice(0..0, [bx, by, axis]);
                    }
                    (bx, by, route)
                } else {
                    let item = &nodes[inner];
                    let mut ix = item.x;
                    let mut iy = item.y;
                    if item.is_group && endpoint_below(inner, &edges[ei], out) {
                        if let Some(p) = port_of(&self.levels, &self.level_of, inner, ei, out) {
                            ix += p.px - item.w / 2.0;
                            iy += p.py - item.h / 2.0;
                        }
                    } else {
                        match side {
                            Side::Top => iy -= item.h / 2.0,
                            Side::Bottom => iy += item.h / 2.0,
                            Side::Left => ix -= item.w / 2.0,
                            Side::Right => ix += item.w / 2.0,
                        }
                    }
                    let (bx, by) = border(ix, iy);
                    let a = if matches!(side, Side::Top | Side::Bottom) { 0.0 } else { 1.0 };
                    let route = if out { vec![ix, iy, a, bx, by, a] } else { vec![bx, by, a, ix, iy, a] };
                    (bx, by, route)
                };
                let port = &mut self.levels[q].ports[i];
                port.px = px;
                port.py = py;
                port.route = route;
            }

            if let Some(g) = group {
                nodes[g].w = box_w;
                nodes[g].h = box_h;
            } else {
                total = CompoundResult { height: box_h, width: box_w };
            }
        }

        // Top-down again: turn positions inside each box into diagram coordinates.
        for q in 0..self.levels.len() {
            let (ox, oy) = (self.levels[q].x, self.levels[q].y);
            for k in 0..self.levels[q].items.len() {
                let item = self.levels[q].items[k];
                let node = &mut nodes[item];
                node.x += ox;
                node.y += oy;
                if node.is_group
                    && let Some(child) = self.level_of[item]
                {
                    let (x, y) = (node.x - node.w / 2.0, node.y - node.h / 2.0);
                    self.levels[child].x = x;
                    self.levels[child].y = y;
                }
            }
        }

        let mut outward = Vec::new();
        let mut inward = Vec::new();
        for level in &self.levels {
            for le in &level.edges {
                let e = &mut edges[le.e];
                let mut route = Vec::new();
                outward.clear();
                let mut item = e.src;
                while let Some(p) = parent_of(nodes, item)
                    && Some(p) != level.id
                {
                    outward.extend(self.level_of[p]);
                    item = p;
                }
                for &lv in &outward {
                    let lv = &self.levels[lv];
                    if let Some(&k) = lv.port_of.get(&port_key(le.e, true)) {
                        append(&mut route, &lv.ports[k].route, lv.x, lv.y);
                    }
                }
                append(&mut route, &self.routes[le.e], level.x, level.y);
                inward.clear();
                let mut item = e.dst;
                while let Some(p) = parent_of(nodes, item)
                    && Some(p) != level.id
                {
                    inward.extend(self.level_of[p]);
                    item = p;
                }
                for &lv in inward.iter().rev() {
                    let lv = &self.levels[lv];
                    if let Some(&k) = lv.port_of.get(&port_key(le.e, false)) {
                        append(&mut route, &lv.ports[k].route, lv.x, lv.y);
                    }
                }
                e.route = route;
                if e.label_w > 0.0 {
                    e.label_x += level.x;
                    e.label_y += level.y;
                }
            }
        }
        total
    }

    /// Gives every port on a group's end side the place across the flow where its edge turns
    /// outside the group, past the straight run out of the port, and says whether the ports of
    /// any group are out of that order.
    fn order_ports(&mut self, nodes: &[CNode], edges: &[CEdge]) -> bool {
        let mut again = false;
        let mut seen: Vec<(f64, f64)> = Vec::new();
        for level in &mut self.levels {
            let Some(id) = level.id else {
                continue;
            };
            let group = &nodes[id];
            let vertical = matches!(level.dir, Dir::Tb | Dir::Bt);
            let end = opposite(start_side(level.dir));
            let a = usize::from(!vertical);
            seen.clear();
            for port in &mut level.ports {
                if port.side != end {
                    continue;
                }
                let route = &edges[port.e].route;
                let bx = level.x + port.px;
                let by = level.y + port.py;
                let mut at = None;
                for k in (0..route.len().saturating_sub(2)).step_by(3) {
                    if at.is_some() && !port.out {
                        break;
                    }
                    if (route[k] - bx).abs() < 0.01 && (route[k + 1] - by).abs() < 0.01 {
                        at = Some(k);
                    }
                }
                let Some(at) = at else {
                    continue;
                };
                let mut next = Some(at);
                let step = |k: usize| if port.out { k.checked_add(3) } else { k.checked_sub(3) };
                next = next.and_then(step);
                while let Some(k) = next
                    && k < route.len()
                    && (route[k + a] - route[at + a]).abs() < IN_LINE
                {
                    next = step(k);
                }
                let Some(k) = next.filter(|&k| k < route.len()) else {
                    continue;
                };
                port.hint = route[k + a] - if vertical { group.x - group.w / 2.0 } else { group.y - group.h / 2.0 };
                seen.push((if vertical { port.px } else { port.py }, port.hint));
            }
            seen.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(Ordering::Equal));
            if seen.windows(2).any(|w| w[0].1 > w[1].1) {
                again = true;
            }
        }
        again
    }
}

/// Where a layout leaves its nodes, edges, and ports, to go back to.
struct Snapshot {
    /// Each edge's label center.
    labels: Vec<(f64, f64)>,
    /// Each level's place, then each of its ports' places.
    levels: Vec<f64>,
    /// Each node's center and size.
    nodes: Vec<[f64; 4]>,
    /// Each edge's route.
    routes: Vec<Vec<f64>>,
    /// The size of the drawing.
    total: CompoundResult,
}

/// Takes a snapshot of a layout.
fn snapshot(nodes: &[CNode], edges: &[CEdge], levels: &[Level], total: CompoundResult) -> Snapshot {
    let mut places = Vec::new();
    for level in levels {
        places.extend([level.x, level.y]);
        for port in &level.ports {
            places.extend([port.px, port.py]);
        }
    }
    Snapshot {
        labels: edges.iter().map(|e| (e.label_x, e.label_y)).collect(),
        levels: places,
        nodes: nodes.iter().map(|c| [c.x, c.y, c.w, c.h]).collect(),
        routes: edges.iter().map(|e| e.route.clone()).collect(),
        total,
    }
}

/// Puts a layout back as `snapshot` took it, and returns the size of the drawing.
fn restore(s: &Snapshot, nodes: &mut [CNode], edges: &mut [CEdge], levels: &mut [Level]) -> CompoundResult {
    for (c, &[x, y, w, h]) in nodes.iter_mut().zip(&s.nodes) {
        c.x = x;
        c.y = y;
        c.w = w;
        c.h = h;
    }
    for ((e, route), &(x, y)) in edges.iter_mut().zip(&s.routes).zip(&s.labels) {
        e.route.clone_from(route);
        e.label_x = x;
        e.label_y = y;
    }
    let mut places = s.levels.iter().copied();
    for level in levels {
        level.x = places.next().unwrap_or(level.x);
        level.y = places.next().unwrap_or(level.y);
        for port in &mut level.ports {
            port.px = places.next().unwrap_or(port.px);
            port.py = places.next().unwrap_or(port.py);
        }
    }
    s.total
}

/// Sets the side of the port that edge `e` passes through on the border of group `item`.
fn set_side(levels: &mut [Level], level_of: &[Option<usize>], item: usize, e: usize, out: bool, side: Side) {
    let Some(q) = level_of.get(item).copied().flatten() else {
        return;
    };
    let Some(level) = levels.get_mut(q) else {
        return;
    };
    if let Some(&k) = level.port_of.get(&port_key(e, out))
        && let Some(port) = level.ports.get_mut(k)
    {
        port.side = side;
    }
}

/// Appends the points of `part`, moved by `ox`, `oy`, to `route`, skipping a point that
/// repeats the last one.
fn append(route: &mut Vec<f64>, part: &[f64], ox: f64, oy: f64) {
    for p in part.chunks_exact(3) {
        let x = p[0] + ox;
        let y = p[1] + oy;
        let len = route.len();
        if len >= 3 && (route[len - 3] - x).abs() < 0.01 && (route[len - 2] - y).abs() < 0.01 {
            continue;
        }
        route.extend([x, y, p[2]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;
    use crate::Options;
    use crate::Theme;
    use crate::render;

    fn options() -> LayeredOptions {
        LayeredOptions { edge_sep: 16.0, node_sep: 40.0, port_sep: 20.0, rank_sep: 48.0, ..LayeredOptions::default() }
    }

    fn edge(src: usize, dst: usize) -> CEdge {
        CEdge { dst, minlen: 1, src, ..CEdge::default() }
    }

    fn group(seq: f64) -> CNode {
        CNode {
            is_group: true,
            pad_bottom: 20.0,
            pad_top: 40.0,
            pad_x: 20.0,
            seq,
            span: f64::INFINITY,
            ..cnode(0.0, 0.0, -1)
        }
    }

    fn on_border(node: &CNode, x: f64, y: f64) -> bool {
        let dx = (x - node.x).abs();
        let dy = (y - node.y).abs();
        let inside = dx <= node.w / 2.0 + 0.5 && dy <= node.h / 2.0 + 0.5;
        inside && ((dx - node.w / 2.0).abs() <= 0.5 || (dy - node.h / 2.0).abs() <= 0.5)
    }

    /// Pele's issue #3: two chains in separate groups feed a store in a third group.
    fn fan_in() -> (Vec<CNode>, Vec<CEdge>) {
        let mut nodes = vec![group(100.0), group(101.0), group(102.0), cnode(80.0, 60.0, 2)];
        let mut edges = Vec::new();
        for g in [0, 1] {
            for i in 0..5 {
                let v = nodes.len();
                nodes.push(CNode { seq: float(v), ..cnode(40.0, 40.0, g) });
                if i > 0 {
                    edges.push(edge(v - 1, v));
                }
                edges.push(edge(v, 3));
            }
        }
        (nodes, edges)
    }

    #[test]
    fn counts_route_crossings_but_not_fans() {
        let nodes: Vec<CNode> = (0..4)
            .map(|i| CNode { x: 100.0 * f64::from(i % 2), y: 200.0 * f64::from(i / 2), ..cnode(40.0, 40.0, -1) })
            .collect();
        let mut crossed = [edge(0, 3), edge(1, 2)];
        crossed[0].route = vec![0.0, 20.0, 0.0, 100.0, 180.0, 0.0];
        crossed[1].route = vec![100.0, 20.0, 0.0, 0.0, 180.0, 0.0];
        assert_eq!(route_crossings(&nodes, &crossed, Style::Curved), 1);
        let mut fan = [edge(0, 2), edge(0, 3)];
        fan[0].route = vec![10.0, 20.0, 0.0, 0.0, 180.0, 0.0];
        fan[1].route = vec![-10.0, 20.0, 0.0, 100.0, 180.0, 0.0];
        assert_eq!(route_crossings(&nodes, &fan, Style::Curved), 0);
    }

    #[test]
    fn edges_joining_one_run_into_a_node_do_not_cross() {
        let mut nodes = vec![cnode(40.0, 40.0, -1), cnode(40.0, 40.0, -1), cnode(40.0, 40.0, -1)];
        nodes[1].x = 100.0;
        nodes[2].x = 50.0;
        nodes[2].y = 300.0;
        let mut edges = [edge(0, 2), edge(1, 2)];
        edges[0].route = vec![0.0, 20.0, 0.0, 0.0, 200.0, 0.0, 50.0, 200.0, 0.0, 50.0, 280.0, 0.0];
        edges[1].route = vec![100.0, 20.0, 0.0, 100.0, 150.0, 0.0, 50.0, 150.0, 0.0, 50.0, 280.0, 0.0];
        assert_eq!(route_crossings(&nodes, &edges, Style::Curved), 0);
        let (mut fan, mut into) = fan_in();
        compound_layout(&mut fan, &mut into, Dir::Tb, &LayeredOptions { tracks: true, ..options() });
        assert_eq!(route_crossings(&fan, &into, Style::Curved), 0);
    }

    #[test]
    fn counts_crossings_round_corners_and_at_steps() {
        let mut nodes = vec![cnode(40.0, 40.0, -1); 4];
        nodes[0].y = -20.0;
        nodes[1].x = 120.0;
        nodes[1].y = 100.0;
        nodes[2].x = 5.0;
        nodes[2].y = 170.0;
        nodes[3].x = 5.0;
        nodes[3].y = 40.0;
        // Rounding the corner clears the other edge.
        let mut corner = [edge(0, 1), edge(2, 3)];
        corner[0].route = vec![0.0, 0.0, 0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 1.0];
        corner[1].route = vec![5.0, 150.0, 0.0, 5.0, 60.0, 0.0];
        assert_eq!(route_crossings(&nodes, &corner, Style::Curved), 0);
        assert_eq!(route_crossings(&nodes, &corner, Style::Stepped), 1);
        assert_eq!(route_crossings(&nodes, &corner, Style::Linear), 1);
        let (mut fan, mut into) = fan_in();
        compound_layout(&mut fan, &mut into, Dir::Tb, &options());
        assert!(route_crossings(&fan, &into, Style::Curved) <= 4);
    }

    #[test]
    fn orders_far_side_ports_by_where_their_edges_go() {
        for dir in [Dir::Tb, Dir::Lr] {
            let mut nodes = vec![group(100.0), group(101.0), group(102.0), cnode(80.0, 40.0, 2)];
            let mut edges = Vec::new();
            for g in [0, 1] {
                for i in 0..5 {
                    let v = nodes.len();
                    nodes.push(cnode(100.0, 40.0, g));
                    if i > 0 {
                        edges.push(edge(v - 1, v));
                    }
                    edges.push(edge(v, 3));
                }
            }
            for (i, node) in nodes.iter_mut().enumerate() {
                if !node.is_group {
                    node.seq = float(i);
                    node.span = 72.0;
                }
            }
            compound_layout(&mut nodes, &mut edges, dir, &options());
            let across = usize::from(dir != Dir::Tb);
            let at = |e: &CEdge, g: usize| {
                e.route.chunks_exact(3).find(|p| on_border(&nodes[g], p[0], p[1])).map_or(f64::NAN, |p| p[across])
            };
            let fan: Vec<&CEdge> = edges.iter().filter(|e| e.dst == 3).collect();
            let leave: Vec<f64> = fan.iter().map(|e| at(e, parent_of(&nodes, e.src).unwrap())).collect();
            let enter: Vec<f64> = fan.iter().map(|e| at(e, 2)).collect();
            for i in 0..fan.len() {
                assert!(!leave[i].is_nan() && !enter[i].is_nan(), "{dir:?} edge {i}");
                for j in i + 1..fan.len() {
                    assert_eq!(
                        (leave[i] - leave[j]).signum(),
                        (enter[i] - enter[j]).signum(),
                        "{dir:?} edges {i} and {j}"
                    );
                }
            }
        }
    }

    #[test]
    fn orders_a_row_of_groups_as_one_drawing() {
        let mut nodes = vec![group(100.0), group(101.0), group(102.0)];
        let mut stage = |g: i32| {
            (0..5)
                .map(|_| {
                    let v = nodes.len();
                    nodes.push(CNode { seq: float(v + 1), ..cnode(80.0, 40.0, g) });
                    v
                })
                .collect::<Vec<_>>()
        };
        let (a, b, c) = (stage(0), stage(1), stage(2));
        let mut edges = Vec::new();
        for (i, j) in [4, 2, 0, 3, 1].into_iter().enumerate() {
            edges.push(edge(a[i], b[j]));
        }
        for (i, j) in [1, 3, 0, 4, 2].into_iter().enumerate() {
            edges.push(edge(b[i], c[j]));
        }
        compound_layout(&mut nodes, &mut edges, Dir::Lr, &options());
        assert_eq!(route_crossings(&nodes, &edges, Style::Curved), 0);
    }

    #[test]
    fn renders_nested_subgraphs_the_same_every_time() {
        let theme = Theme::monochrome(Color::rgb(0xff, 0xff, 0xff), Color::rgb(0, 0, 0));
        for dir in ["TB", "LR", "BT", "RL"] {
            let source = format!(
                "flowchart {dir}
  start --> a1
  start --> b1
  subgraph outer[Outer]
    subgraph left[Left]
      a1[A1] --> a2[A2]
      a1 --> a3[A3]
    end
    subgraph right[Right]
      b1 --> b2
      subgraph deep[Deep]
        c1 --> c2
        c1 --> c3
      end
    end
  end
  a2 --> c2
  a3 --> c1
  c3 --> b2
  b2 --> finish
  a2 --> finish
  c2 -->|label| finish
"
            );
            let first = render(&source, &Options::default(), &theme).unwrap();
            let again = render(&source, &Options::default(), &theme).unwrap();
            assert_eq!(first, again, "{dir}");
            assert!(first.width > 0.0 && first.height > 0.0, "{dir}");
            assert!(!first.svg.contains("NaN"), "{dir}: {}", first.svg);
            for id in ["Outer", "Left", "Right", "Deep"] {
                assert!(first.svg.contains(id), "{dir}: no {id}");
            }
        }
    }
}
