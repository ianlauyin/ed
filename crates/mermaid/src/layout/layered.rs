//! Layered (Sugiyama-style) layout of one flat graph. Port of Pele's `src/layout/layered.ts`.
//!
//! Ranks run down the y axis; callers rotate the result for other directions. What the layout of
//! edges aims for, most important first, with where each is carried out:
//!
//! 1. Lines can be told apart: ends on one side of a node have places of their own (`spread`),
//!    and curves that would pass within a few pixels turn at levels of their own
//!    (`assign_tracks`).
//! 2. Lines cross as little as the order of the nodes allows (`reduce_crossings`).
//! 3. An edge takes the shortest and simplest way (`align_labels`, `straighten`, `settle`,
//!    `snap`).
//! 4. An edge meets the middle of a side unless an aim above moves it (`settle`).
//! 5. The drawing is balanced (`head_room` in `layered`, the crossed gaps in `assign_tracks`).
//! 6. Nodes keep the order they were written in where nothing above says otherwise.
//! 7. The drawing stays compact.
//!
//! Bounds: every array a pass builds is sized from the graph it indexes (node, segment, or rank
//! count), and every index stored in one comes from that graph. `layered`, `orient`, and `rank`
//! check the caller's edge endpoints once on entry, so indexing inside the passes stays in
//! bounds. Floating-point expressions keep Pele's operation order, since output depends on it.

#![expect(
    clippy::needless_range_loop,
    reason = "index loops mirror Pele's typed-array loops for side-by-side review"
)]

use std::cmp::Ordering;
use std::collections::BTreeSet;

/// What a layout node stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A bend of an edge in a rank it passes.
    Dummy,
    /// A port on the end border of a group, which edges leave the group through.
    EndPort,
    /// The label of an edge, in a rank it passes.
    Label,
    /// A node the caller placed.
    #[default]
    Node,
    /// A port on the start border of a group, which edges enter the group through.
    StartPort,
}

/// A node of a layered layout. Mirrors Pele's `LNode`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LNode {
    /// Height along the flow.
    pub(crate) h: f64,
    /// What the node stands for.
    pub(crate) kind: Kind,
    /// Keeps the node first (negative) or last (positive) in its rank; 0 leaves it free.
    pub(crate) pin: f64,
    /// Rank after layout.
    pub(crate) rank: usize,
    /// How much of the side that edges meet may be shared out between them; 0 keeps them all
    /// at its middle.
    pub(crate) span: f64,
    /// Width across the flow.
    pub(crate) w: f64,
    /// Center x after layout.
    pub(crate) x: f64,
    /// Center y after layout.
    pub(crate) y: f64,
}

/// An edge of a layered layout. Mirrors Pele's `LEdge`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LEdge {
    /// Index of the head node.
    pub(crate) head: usize,
    /// Offset of the head end from the middle of its node, across the flow.
    pub(crate) head_dx: f64,
    /// Whether the head end is held at its offset and takes no part in sharing out its side.
    pub(crate) head_fixed: bool,
    /// Label height.
    pub(crate) label_h: f64,
    /// Label width; 0 for no label.
    pub(crate) label_w: f64,
    /// Label center x after layout.
    pub(crate) label_x: f64,
    /// Label center y after layout.
    pub(crate) label_y: f64,
    /// Least number of ranks the edge spans; 0 counts as 1.
    pub(crate) minlen: u32,
    /// Route after layout, as `x, y` pairs from tail to head.
    pub(crate) points: Vec<f64>,
    /// Whether the edge runs against the flow, set by `orient`.
    pub(crate) reversed: bool,
    /// Index of the tail node.
    pub(crate) tail: usize,
    /// Offset of the tail end from the middle of its node, across the flow.
    pub(crate) tail_dx: f64,
    /// Whether the tail end is held at its offset and takes no part in sharing out its side.
    pub(crate) tail_fixed: bool,
}

/// Options of a layered layout. Mirrors Pele's `LayeredOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LayeredOptions {
    /// The caller sets the ends of the edges on one side of a node apart itself, so twin edges
    /// need no bend to keep them from being drawn as one.
    pub(crate) apart: bool,
    /// Room between edges and bends.
    pub(crate) edge_sep: f64,
    /// The room the marker at the head of an edge takes. A rank of labels whose edges all head
    /// one way sits half of this nearer their tails.
    pub(crate) head_room: f64,
    /// Room between nodes in a rank.
    pub(crate) node_sep: f64,
    /// Room under a rank that holds only group border ports.
    pub(crate) port_sep: f64,
    /// Room between ranks.
    pub(crate) rank_sep: f64,
    /// Draw every edge straight from point to point: none is given a level to turn onto.
    pub(crate) straight: bool,
    /// Route edges at right angles, each on a track of its own across the gap between ranks.
    pub(crate) tracks: bool,
}

/// The size of a layered layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LayeredResult {
    /// Height along the flow.
    pub(crate) height: f64,
    /// Width across the flow.
    pub(crate) width: f64,
}

/// A node of the given size and kind, as Pele's `lnode` creates it.
pub(crate) fn lnode(w: f64, h: f64, kind: Kind) -> LNode {
    LNode {
        h,
        kind,
        w,
        ..LNode::default()
    }
}

/// An edge from `tail` to `head`, as Pele's `ledge` creates it.
pub(crate) fn ledge(tail: usize, head: usize, minlen: u32, label_w: f64, label_h: f64) -> LEdge {
    LEdge {
        head,
        label_h,
        label_w,
        minlen,
        tail,
        ..LEdge::default()
    }
}

/// The most crossing-reduction sweeps per attempt.
const MAX_SWEEPS: usize = 24;

/// Past this many bends, the longest edges are drawn as one direct curve instead.
const MAX_DUMMIES: usize = 20000;

/// The room between two edges that meet one side of a node.
const PORT_PITCH: f64 = 16.0;

/// The least room that keeps two arrowheads apart.
const MIN_PITCH: f64 = 8.0;

/// Where edges get no levels, a gap grows to keep its edges at least this steep.
const MIN_SLOPE: f64 = 0.125;

/// A gap grows to at most this many times its usual height.
const MAX_GAP: f64 = 3.0;

/// Room kept clear between a rank and the nearest track.
const TRACK_MARGIN: f64 = 18.0;

/// Room between two tracks when the gap has to grow to hold them.
const TRACK_PITCH: f64 = 10.0;

/// The most room between two tracks when the gap has room to spare.
const TRACK_SPREAD: f64 = 14.0;

/// A curve that runs more than this many times as far across as its gap is high is wide.
const LEVEL_WIDE: f64 = 2.0;

/// Room kept between a gap's levels and its ranks.
const LEVEL_MARGIN: f64 = 28.0;

/// Room between two levels.
const LEVEL_PITCH: f64 = 12.0;

/// The room a plain curve is taken to keep straight at each end of its gap.
const LEVEL_LEAD: f64 = 12.0;

/// Two curves, one of them wide, nearer than this turn at levels of their own.
const LEVEL_CLEAR: f64 = 8.0;

/// Two plain curves nearer than this turn at levels of their own.
const LEVEL_TOUCH: f64 = 4.0;

/// A gap whose curves mostly cross is this many times as tall as its widest curve runs across.
const CROSSED_SLOPE: f64 = 0.5;

/// How far out of line two points may be and still count as in line.
const IN_LINE: f64 = 0.5;

/// The farthest a node moves to bring one more of its edges into line.
const SNAP: f64 = 12.0;

/// An end that leans keeps this far from the ends of the part of its side that edges spread
/// along.
const LEAN_CLEAR: f64 = 40.0;

/// Past this many edges across one gap, tracks are given out in a plain order.
const MAX_WEIGHED: usize = 400;

/// Points sampled along a curve to tell how near two curves pass.
const SAMPLES: usize = 16;

/// What a segment's end is to the node it meets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum End {
    /// The edge arrives at the node.
    Arrives,
    /// Held where its offset puts it.
    #[default]
    Held,
    /// The edge leaves the node.
    Leaves,
}

/// Whether a kind is a bend or label of an edge.
fn is_dummy(kind: Kind) -> bool {
    kind == Kind::Dummy || kind == Kind::Label
}

/// Orders two numbers as a JavaScript comparator returning `a - b` does: equal values, and
/// `NaN`, tie.
fn cmp(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// An index or count as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "indices and counts stay far below 2^52"
)]
fn float(i: usize) -> f64 {
    i as f64
}

/// An index as a signed number, for arithmetic that may go below zero.
#[expect(
    clippy::cast_possible_wrap,
    reason = "indices stay below isize::MAX, since they index allocations"
)]
fn signed(i: usize) -> isize {
    i as isize
}

/// `Math.max(lo, Math.min(hi, x))`, which returns `lo` when `lo > hi` where `f64::clamp`
/// panics.
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    lo.max(hi.min(x))
}

/// V8's `Math.hypot` for two values: a compensated sum of squares scaled by the larger value.
/// It differs from `f64::hypot` in the last bit, and the result feeds comparisons.
fn hypot(x: f64, y: f64) -> f64 {
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

/// Whether every edge endpoint names one of `count` nodes.
fn valid(count: usize, edges: &[LEdge]) -> bool {
    edges.iter().all(|e| e.tail < count && e.head < count)
}

/// Insertion-sorts `list[from..to]` by `key`, then by value.
fn sort_range(list: &mut [usize], from: usize, to: usize, key: &[usize]) {
    for i in from + 1..to {
        let item = list[i];
        let k = key[item];
        let mut j = i;
        while j > from && (key[list[j - 1]] > k || (key[list[j - 1]] == k && list[j - 1] > item)) {
            list[j] = list[j - 1];
            j -= 1;
        }
        list[j] = item;
    }
}

/// Lays out a flat graph in ranks down the y axis. Sets each node's `rank`, `x`, and `y`, and
/// each edge's `reversed`, `points`, and label position, and moves `tail_dx` and `head_dx`
/// where edges spread along their nodes. Returns the size of the drawing.
///
/// An edge whose endpoint names no node makes the whole layout empty, where Pele throws.
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's layered.ts for side-by-side review"
)]
pub(crate) fn layered(
    nodes: &mut [LNode],
    edges: &mut [LEdge],
    opt: &LayeredOptions,
) -> LayeredResult {
    let real = nodes.len();
    if real == 0 || !valid(real, edges) {
        return LayeredResult::default();
    }
    let m = edges.len();

    // Every edge gets a middle rank of its own when labels or parallel edges need the room.
    let mut step = 1;
    if m > 0 {
        let mut seen = BTreeSet::new();
        for e in edges.iter() {
            let key = e.tail.min(e.head) as u64 * real as u64 + e.tail.max(e.head) as u64;
            if e.label_w > 0.0 || !seen.insert(key) {
                step = 2;
                break;
            }
        }
    }

    orient(nodes, edges);
    rank(nodes, edges, step);

    let span: Vec<usize> = edges
        .iter()
        .map(|e| nodes[e.head].rank.abs_diff(nodes[e.tail].rank))
        .collect();
    let mut dummies: usize = span.iter().filter(|&&s| s > 1).map(|&s| s - 1).sum();
    let mut direct = vec![false; m];
    if dummies > MAX_DUMMIES {
        let mut longest: Vec<usize> = (0..m).filter(|&ei| span[ei] > 1).collect();
        longest.sort_by(|&a, &b| span[b].cmp(&span[a]).then(a.cmp(&b)));
        for ei in longest {
            if dummies <= MAX_DUMMIES {
                break;
            }
            direct[ei] = true;
            dummies -= span[ei] - 1;
        }
    }

    let n = real + dummies;
    let mut w = vec![0.0; n];
    let mut h = vec![0.0; n];
    let mut kind = vec![Kind::Node; n];
    let mut ranks = vec![0usize; n];
    let mut max_rank = 0;
    for (i, node) in nodes.iter().enumerate() {
        w[i] = node.w;
        h[i] = node.h;
        kind[i] = node.kind;
        ranks[i] = node.rank;
        max_rank = max_rank.max(node.rank);
    }

    // The graph that gets ordered and positioned: every edge cut into rank-to-rank segments.
    // An edge whose head ranks above its tail (only around a self-loop) counts bends it never
    // gets, as in Pele, and those slots stay zero.
    let segments: usize = (0..m).filter(|&ei| !direct[ei]).map(|ei| span[ei]).sum();
    let mut ef = vec![0usize; segments];
    let mut et = vec![0usize; segments];
    let mut efd = vec![0.0; segments];
    let mut etd = vec![0.0; segments];
    let mut first_dummy: Vec<Option<usize>> = vec![None; m];
    // Which way the edges with a label in each rank head: 1 down, 2 up, 3 both.
    let mut heads = vec![0u8; max_rank + 1];
    let mut label_node: Vec<Option<usize>> = vec![None; m];
    let spans = nodes.iter().any(|v| v.span > 0.0 && v.kind == Kind::Node);
    // What each segment's end is to the node it meets, for the ends that may move along a side.
    let ends = if spans { segments } else { 0 };
    let mut efk = vec![End::Held; ends];
    let mut etk = vec![End::Held; ends];
    let mut first_seg = vec![0usize; m];
    let mut last_seg = vec![0usize; m];
    let mut offsets = false;
    {
        let mut next = real;
        let mut ce = 0;
        for (ei, e) in edges.iter().enumerate() {
            if direct[ei] {
                continue;
            }
            let (a, b, a_dx, b_dx) = if e.reversed {
                (e.head, e.tail, e.head_dx, e.tail_dx)
            } else {
                (e.tail, e.head, e.tail_dx, e.head_dx)
            };
            if a_dx != 0.0 || b_dx != 0.0 {
                offsets = true;
            }
            let ra = ranks[a];
            let rb = ranks[b];
            // The label goes halfway along: in a rank of nodes beside them for a long edge, the
            // nearer its start of the two either side of halfway, or in the rank between two
            // neighbouring ranks. Only an edge that gets bends can hold it.
            let label_rank = (e.label_w > 0.0 && rb > ra + 1).then(|| {
                let mut r = ra + ((rb - ra) >> 1);
                if step == 2 && rb - ra >= 4 {
                    if r & 1 == 1 {
                        r = if e.reversed { r + 1 } else { r - 1 };
                    }
                } else if step == 2 && r & 1 == 0 {
                    r = if e.reversed { r + 1 } else { r - 1 };
                }
                r
            });
            let mut prev = a;
            if rb > ra + 1 {
                first_dummy[ei] = Some(next);
            }
            first_seg[ei] = ce;
            if spans
                && !(if e.reversed {
                    e.head_fixed
                } else {
                    e.tail_fixed
                })
            {
                efk[ce] = if e.reversed {
                    End::Arrives
                } else {
                    End::Leaves
                };
            }
            for r in ra + 1..rb {
                let d = next;
                next += 1;
                ranks[d] = r;
                if label_rank == Some(r) {
                    w[d] = e.label_w;
                    h[d] = e.label_h;
                    kind[d] = Kind::Label;
                    label_node[ei] = Some(d);
                    heads[r] |= if e.reversed { 2 } else { 1 };
                } else {
                    kind[d] = Kind::Dummy;
                }
                ef[ce] = prev;
                et[ce] = d;
                efd[ce] = if prev == a { a_dx } else { 0.0 };
                ce += 1;
                prev = d;
            }
            ef[ce] = prev;
            et[ce] = b;
            efd[ce] = if prev == a { a_dx } else { 0.0 };
            etd[ce] = b_dx;
            if spans
                && !(if e.reversed {
                    e.tail_fixed
                } else {
                    e.head_fixed
                })
            {
                etk[ce] = if e.reversed {
                    End::Leaves
                } else {
                    End::Arrives
                };
            }
            last_seg[ei] = ce;
            ce += 1;
        }
    }

    let mut succ_start = vec![0usize; n + 1];
    let mut pred_start = vec![0usize; n + 1];
    for e in 0..segments {
        succ_start[ef[e] + 1] += 1;
        pred_start[et[e] + 1] += 1;
    }
    for i in 0..n {
        succ_start[i + 1] += succ_start[i];
        pred_start[i + 1] += pred_start[i];
    }
    let mut succ_e = vec![0usize; segments];
    let mut pred_e = vec![0usize; segments];
    {
        let mut sf = succ_start[..n].to_vec();
        let mut pf = pred_start[..n].to_vec();
        for e in 0..segments {
            succ_e[sf[ef[e]]] = e;
            sf[ef[e]] += 1;
            pred_e[pf[et[e]]] = e;
            pf[et[e]] += 1;
        }
    }

    let mut layer_start = vec![0usize; max_rank + 2];
    for &r in &ranks {
        layer_start[r + 1] += 1;
    }
    for r in 0..=max_rank {
        layer_start[r + 1] += layer_start[r];
    }
    let mut order = vec![0usize; n];

    // Initial order: depth-first from the top so that subtrees start out contiguous.
    {
        let mut by_rank = vec![0usize; n];
        let mut fill = layer_start[..=max_rank].to_vec();
        for i in 0..n {
            by_rank[fill[ranks[i]]] = i;
            fill[ranks[i]] += 1;
        }
        fill.copy_from_slice(&layer_start[..=max_rank]);
        let mut visited = vec![false; n];
        let mut stack = Vec::new();
        for &first in &by_rank {
            if visited[first] {
                continue;
            }
            stack.push(first);
            while let Some(v) = stack.pop() {
                if visited[v] {
                    continue;
                }
                visited[v] = true;
                order[fill[ranks[v]]] = v;
                fill[ranks[v]] += 1;
                for j in (succ_start[v]..succ_start[v + 1]).rev() {
                    let w = et[succ_e[j]];
                    if !visited[w] {
                        stack.push(w);
                    }
                }
            }
        }
    }

    if nodes.iter().any(|v| v.pin != 0.0) {
        for r in 0..=max_rank {
            let pin = |v: usize| if v < real { nodes[v].pin } else { 0.0 };
            order[layer_start[r]..layer_start[r + 1]].sort_by(|&a, &b| cmp(pin(a), pin(b)));
        }
    }

    let mut g = Graph {
        ef,
        efd,
        et,
        etd,
        kind,
        layer_start,
        max_rank,
        n,
        offsets,
        order,
        pos: vec![0; n],
        pred_e,
        pred_start,
        rank: ranks,
        real,
        succ_e,
        succ_start,
        w,
    };
    g.set_positions();

    if segments > 0 && max_rank > 0 {
        reduce_crossings(&mut g, nodes);
        g.set_positions();
    }

    if spans {
        spread(&mut g, nodes, &efk, &etk);
    }

    let mut xs = position(&mut g, opt);

    let empty = empty_ranks(&g);
    let empty = empty.as_deref();
    if spans {
        let free = free_ends(&g, &efk, &etk);
        align_labels(&g, nodes, &mut xs, &free, opt, empty);
        settle(&mut g, nodes, &xs, &efk, &etk, &free, empty);
        snap(&g, &mut xs, empty, opt);
        for (ei, e) in edges.iter_mut().enumerate() {
            if direct[ei] {
                continue;
            }
            let a_dx = g.efd[first_seg[ei]];
            let b_dx = g.etd[last_seg[ei]];
            e.tail_dx = if e.reversed { b_dx } else { a_dx };
            e.head_dx = if e.reversed { a_dx } else { b_dx };
        }
    }

    // A rank that holds nothing but bends is only there because some other edge has a label or
    // a twin. Its bends move onto the straight line between the ends of their edges, and one
    // that gets there is left out of the route.
    let mut on_line = vec![false; n];
    if let Some(empty) = empty
        && !opt.tracks
    {
        let sep = if opt.apart { 0.0 } else { opt.edge_sep };
        straighten(&g, &mut xs, empty, &first_dummy, &span, sep, &mut on_line);
    }
    if !opt.tracks {
        past_labels(&g, &mut xs, &first_dummy, &span, opt.edge_sep, &mut on_line);
    }

    let is_empty = |r: usize| empty.is_some_and(|empty| empty[r]);
    // An edge is cut where it passes a rank that holds something, and each piece crosses one
    // gap. With `tracks`, every piece that does not run straight gets a track; otherwise, where
    // edges spread along their nodes, a piece gets one only if it needs a level to turn onto.
    let mut piece_start = vec![0usize; m + 1];
    let mut pieces = (opt.tracks || (spans && !opt.straight)).then(Pieces::default);
    let mut track_count = vec![0usize; max_rank + 1];
    if let Some(pieces) = pieces.as_mut() {
        for ei in 0..m {
            piece_start[ei] = pieces.from.len();
            if direct[ei] {
                continue;
            }
            let bends = first_dummy[ei].map_or(0..0, |d0| d0..d0 + span[ei] - 1);
            // An edge that keeps a bend to stay clear of its twin is drawn through the bend, as
            // it is, and so is one that has a bend left out in a rank of labels.
            let twin = !opt.tracks && bends.clone().any(|d| is_empty(g.rank[d]) != on_line[d]);
            if twin {
                continue;
            }
            let first = first_seg[ei];
            let mut x = xs[g.ef[first]] + g.efd[first];
            let mut rank = g.rank[g.ef[first]];
            for d in bends {
                if is_empty(g.rank[d]) {
                    continue;
                }
                pieces.from.push(x);
                pieces.to.push(xs[d]);
                pieces.rank.push(rank);
                pieces.lower.push(Some(d));
                x = xs[d];
                rank = g.rank[d];
            }
            let last = last_seg[ei];
            pieces.from.push(x);
            pieces.to.push(xs[g.et[last]] + g.etd[last]);
            pieces.rank.push(rank);
            pieces.lower.push(None);
        }
        piece_start[m] = pieces.from.len();
    }

    // Ranks that hold only border ports or their pass-through dummies need little room.
    let port_only = |r: usize, port: Kind| {
        let mut found = false;
        for &v in &g.order[g.layer_start[r]..g.layer_start[r + 1]] {
            if g.kind[v] == port {
                found = true;
            } else if g.kind[v] != Kind::Dummy {
                return false;
            }
        }
        found
    };
    let start_ranks = if port_only(0, Kind::StartPort) {
        step
    } else {
        0
    };
    let end_ranks = if port_only(max_rank, Kind::EndPort) {
        step
    } else {
        0
    };
    // The longest way across that any edge travels in the gap below each rank.
    let mut run = vec![0.0; max_rank + 1];
    for e in 0..segments {
        let across = (xs[g.ef[e]] + g.efd[e] - xs[g.et[e]] - g.etd[e]).abs();
        let r = g.rank[g.ef[e]];
        if across > run[r] {
            run[r] = across;
        }
    }
    let step_f = float(step);
    let usual = |r: usize| {
        let sep = if r < start_ranks || r + end_ranks >= max_rank {
            opt.port_sep
        } else {
            opt.rank_sep
        };
        sep / step_f
    };
    // The room under each rank. Edges that are not cut into pieces need more where they run
    // far across.
    let mut gap_under = vec![0.0; max_rank + 1];
    for r in 0..max_rank {
        gap_under[r] = if pieces.is_some() {
            usual(r)
        } else {
            usual(r).max(((MAX_GAP * opt.rank_sep) / step_f).min(run[r] * MIN_SLOPE))
        };
    }
    // A rank between two ranks of nodes, which holds labels and bends only.
    let mut between = vec![true; max_rank + 1];
    for &r in &g.rank[..real] {
        between[r] = false;
    }
    for r in 1..max_rank {
        if opt.tracks || !between[r] || (heads[r] != 1 && heads[r] != 2) {
            continue;
        }
        let down = heads[r] == 1;
        let shift = (opt.head_room / 2.0).min(gap_under[if down { r - 1 } else { r }] / 2.0);
        gap_under[r - 1] += if down { -shift } else { shift };
        gap_under[r] += if down { shift } else { -shift };
    }
    // The rank that the pieces leaving each rank arrive at, past any that hold only bends, and
    // the room between the two, which grows to hold the gap's tracks or levels.
    let margin = if opt.tracks {
        TRACK_MARGIN
    } else {
        LEVEL_MARGIN
    };
    let least_pitch = if opt.tracks { TRACK_PITCH } else { LEVEL_PITCH };
    let most_pitch = if opt.tracks {
        TRACK_SPREAD
    } else {
        LEVEL_PITCH
    };
    let mut below = vec![0usize; max_rank + 1];
    if let Some(pieces) = pieces.as_mut() {
        let mut room = vec![0.0; max_rank + 1];
        for r in 0..max_rank {
            if is_empty(r) {
                continue;
            }
            let mut next = r + 1;
            room[r] = gap_under[r];
            while next < max_rank && is_empty(next) {
                room[r] += gap_under[next];
                next += 1;
            }
            below[r] = next;
        }
        for k in 0..pieces.from.len() {
            let wide = (pieces.from[k] - pieces.to[k]).abs() > LEVEL_WIDE * room[pieces.rank[k]];
            pieces.wide.push(wide);
        }
        let mut tall = vec![0.0; max_rank + 1];
        assign_tracks(pieces, &mut track_count, !opt.tracks, &room, &mut tall);
        for r in 0..max_rank {
            if track_count[r] > 0 {
                let need = 2.0 * margin + float(track_count[r] - 1) * least_pitch - room[r];
                gap_under[r] += need.max(0.0);
            } else if tall[r] > 0.0 {
                gap_under[r] += ((MAX_GAP * opt.rank_sep).min(tall[r]) - room[r]).max(0.0);
            }
        }
    }
    let mut band_top = vec![0.0; max_rank + 1];
    let mut band_bottom = vec![0.0; max_rank + 1];
    let mut y = 0.0;
    for r in 0..=max_rank {
        let mut tallest = 0.0;
        for &v in &g.order[g.layer_start[r]..g.layer_start[r + 1]] {
            if h[v] > tallest {
                tallest = h[v];
            }
        }
        band_top[r] = y;
        band_bottom[r] = y + tallest;
        y += tallest + gap_under[r];
    }
    let height = y;
    // The part of its rank that an edge runs straight through at a bend or label. A bare bend
    // beside labels keeps to the middle half.
    let inset = |d: usize| {
        let r = g.rank[d];
        if g.kind[d] == Kind::Dummy && between[r] && !opt.tracks {
            (band_bottom[r] - band_top[r]) / 4.0
        } else {
            0.0
        }
    };
    // Where a piece runs across its gap on a track of its group: its tracks sit in the middle
    // of the gap, the first at the top.
    let track_y = |pieces: &Pieces, piece: usize, on_track: isize| {
        let r = pieces.rank[piece];
        let top = band_bottom[r];
        let bottom = band_top[below[r]];
        let count = if opt.tracks {
            track_count[r]
        } else {
            pieces.levels[piece]
        };
        let pitch = if count > 1 {
            most_pitch.min((bottom - top - 2.0 * margin) / float(count - 1))
        } else {
            0.0
        };
        let on_track = float(usize::try_from(on_track).unwrap_or(0));
        f64::midpoint(top, bottom) + (on_track - float(count.saturating_sub(1)) / 2.0) * pitch
    };

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for i in 0..n {
        let half = g.w[i] / 2.0;
        if xs[i] - half < min_x {
            min_x = xs[i] - half;
        }
        if xs[i] + half > max_x {
            max_x = xs[i] + half;
        }
    }
    // Where edges spread along their nodes, a rank whose nodes only send edges on stands on one
    // line, and a rank whose nodes only receive them hangs from one. Other ranks are centered.
    let mut arrives = vec![false; max_rank + 1];
    let mut leaves = vec![false; max_rank + 1];
    for i in 0..real {
        if g.kind[i] != Kind::Node || !spans {
            continue;
        }
        if g.pred_start[i + 1] > g.pred_start[i] {
            arrives[g.rank[i]] = true;
        }
        if g.succ_start[i + 1] > g.succ_start[i] {
            leaves[g.rank[i]] = true;
        }
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        let r = node.rank;
        node.x = xs[i] - min_x;
        node.y = if g.kind[i] == Kind::Node && leaves[r] && !arrives[r] {
            band_bottom[r] - node.h / 2.0
        } else if g.kind[i] == Kind::Node && arrives[r] && !leaves[r] {
            band_top[r] + node.h / 2.0
        } else {
            f64::midpoint(band_top[r], band_bottom[r])
        };
    }

    for (ei, e) in edges.iter_mut().enumerate() {
        let first = &nodes[if e.reversed { e.head } else { e.tail }];
        let last = &nodes[if e.reversed { e.tail } else { e.head }];
        let first_dx = if e.reversed { e.head_dx } else { e.tail_dx };
        let mut pts = vec![first.x + first_dx, first.y + first.h / 2.0];
        match (pieces.as_ref(), first_dummy[ei]) {
            (Some(pieces), _) if piece_start[ei + 1] > piece_start[ei] => {
                for k in piece_start[ei]..piece_start[ei + 1] {
                    let track = pieces.track[k];
                    if track >= 0 && (opt.tracks || pieces.wide[k]) {
                        let across = track_y(pieces, k, track);
                        pts.extend([pieces.from[k] - min_x, across, pieces.to[k] - min_x, across]);
                    } else if track > 0 {
                        // A short piece among levels runs straight until it is past the level
                        // before its own, and turns in one S from there.
                        pts.extend([pieces.from[k] - min_x, track_y(pieces, k, track - 1)]);
                    }
                    let Some(d) = pieces.lower[k] else {
                        continue;
                    };
                    let r = g.rank[d];
                    pts.extend([xs[d] - min_x, band_top[r] + inset(d)]);
                    if band_bottom[r] > band_top[r] {
                        pts.extend([xs[d] - min_x, band_bottom[r] - inset(d)]);
                    }
                }
            }
            (_, Some(d0)) => {
                for d in d0..d0 + span[ei] - 1 {
                    if on_line[d] {
                        continue;
                    }
                    let x = xs[d] - min_x;
                    let top = band_top[g.rank[d]];
                    let bottom = band_bottom[g.rank[d]];
                    pts.extend([x, top + inset(d)]);
                    if bottom > top {
                        pts.extend([x, bottom - inset(d)]);
                    }
                }
            }
            _ => {}
        }
        let last_dx = if e.reversed { e.tail_dx } else { e.head_dx };
        pts.extend([last.x + last_dx, last.y - last.h / 2.0]);
        if e.reversed {
            let pairs = pts.len() / 2;
            for i in 0..pairs / 2 {
                let j = pairs - 1 - i;
                pts.swap(2 * i, 2 * j);
                pts.swap(2 * i + 1, 2 * j + 1);
            }
        }
        if let Some(ln) = label_node[ei] {
            e.label_x = xs[ln] - min_x;
            e.label_y = f64::midpoint(band_top[g.rank[ln]], band_bottom[g.rank[ln]]);
        } else if e.label_w > 0.0 {
            let len = pts.len();
            e.label_x = f64::midpoint(pts[0], pts[len - 2]);
            e.label_y = f64::midpoint(pts[1], pts[len - 1]);
        }
        e.points = pts;
    }

    LayeredResult {
        height,
        width: max_x - min_x,
    }
}

/// The pieces of edges that each cross one gap. Where only some pieces get a level to turn
/// onto, `wide` marks those that run far across, `levels` is how many tracks the group a piece
/// belongs to takes (0 for none), and each track counts from its group's first.
#[derive(Default)]
struct Pieces {
    /// Where each piece leaves the rank above.
    from: Vec<f64>,
    /// How many tracks each piece's group takes.
    levels: Vec<usize>,
    /// The bend each piece ends at, or `None` at the edge's last node.
    lower: Vec<Option<usize>>,
    /// The rank above each piece.
    rank: Vec<usize>,
    /// Where each piece reaches the rank below.
    to: Vec<f64>,
    /// The track each piece runs across on, or -1.
    track: Vec<isize>,
    /// Whether each piece runs far across its gap.
    wide: Vec<bool>,
}

/// The graph that gets ordered and positioned. Mirrors Pele's `Graph`.
struct Graph {
    /// Segment tails.
    ef: Vec<usize>,
    /// Where each segment attaches on its tail, from the middle.
    efd: Vec<f64>,
    /// Segment heads.
    et: Vec<usize>,
    /// Where each segment attaches on its head, from the middle.
    etd: Vec<f64>,
    /// Each node's kind.
    kind: Vec<Kind>,
    /// Where each rank starts in `order`, with one more entry for the end.
    layer_start: Vec<usize>,
    /// The last rank.
    max_rank: usize,
    /// Node count, bends and labels included.
    n: usize,
    /// Whether any edge end is offset from the middle of its node.
    offsets: bool,
    /// Nodes rank by rank, each rank in order across.
    order: Vec<usize>,
    /// Each node's place within its rank.
    pos: Vec<usize>,
    /// Segments into each node, grouped by `pred_start`.
    pred_e: Vec<usize>,
    /// Where each node's incoming segments start in `pred_e`.
    pred_start: Vec<usize>,
    /// Each node's rank.
    rank: Vec<usize>,
    /// Count of the caller's nodes, which come first.
    real: usize,
    /// Segments out of each node, grouped by `succ_start`.
    succ_e: Vec<usize>,
    /// Where each node's outgoing segments start in `succ_e`.
    succ_start: Vec<usize>,
    /// Each node's width.
    w: Vec<f64>,
}

impl Graph {
    /// Sets `pos` from `order`.
    fn set_positions(&mut self) {
        for r in 0..=self.max_rank {
            let s = self.layer_start[r];
            for i in s..self.layer_start[r + 1] {
                self.pos[self.order[i]] = i - s;
            }
        }
    }
}

/// Reverses the edges that close a cycle, found by depth-first search in declaration order.
/// Edges into a start port or out of an end port are reversed outright.
pub(crate) fn orient(nodes: &[LNode], edges: &mut [LEdge]) {
    let n = nodes.len();
    if !valid(n, edges) {
        return;
    }
    let mut start = vec![0usize; n + 1];
    for e in edges.iter_mut() {
        e.reversed = nodes[e.head].kind == Kind::StartPort || nodes[e.tail].kind == Kind::EndPort;
        if !e.reversed {
            start[e.tail + 1] += 1;
        }
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut out = vec![0usize; start[n]];
    {
        let mut fill = start[..n].to_vec();
        for (i, e) in edges.iter().enumerate() {
            if !e.reversed {
                out[fill[e.tail]] = i;
                fill[e.tail] += 1;
            }
        }
    }
    let mut state = vec![0u8; n];
    let mut cursor = start[..n].to_vec();
    let mut stack = Vec::new();
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        stack.push(root);
        state[root] = 1;
        while let Some(&v) = stack.last() {
            if cursor[v] < start[v + 1] {
                let e = &mut edges[out[cursor[v]]];
                cursor[v] += 1;
                let w = e.head;
                if state[w] == 1 {
                    e.reversed = true;
                } else if state[w] == 0 {
                    state[w] = 1;
                    stack.push(w);
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
}

/// Ranks the nodes by longest path along the oriented edges, each edge spanning at least
/// `minlen * step` ranks, then pulls loosely attached nodes toward the side with more edges.
/// Start ports go one step above every other node and end ports one step below.
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's layered.ts for side-by-side review"
)]
pub(crate) fn rank(nodes: &mut [LNode], edges: &[LEdge], step: usize) {
    let n = nodes.len();
    if !valid(n, edges) {
        return;
    }
    let m = edges.len();
    let step = i64::from(u32::try_from(step).unwrap_or(u32::MAX));
    let mut from = vec![0usize; m];
    let mut to = vec![0usize; m];
    let mut len = vec![0i64; m];
    let mut out_start = vec![0usize; n + 1];
    let mut in_start = vec![0usize; n + 1];
    for (i, e) in edges.iter().enumerate() {
        let (a, b) = if e.reversed {
            (e.head, e.tail)
        } else {
            (e.tail, e.head)
        };
        from[i] = a;
        to[i] = b;
        len[i] = i64::from(e.minlen.max(1)) * step;
        out_start[a + 1] += 1;
        in_start[b + 1] += 1;
    }
    for i in 0..n {
        out_start[i + 1] += out_start[i];
        in_start[i + 1] += in_start[i];
    }
    let mut out_e = vec![0usize; m];
    let mut in_e = vec![0usize; m];
    {
        let mut of = out_start[..n].to_vec();
        let mut inf = in_start[..n].to_vec();
        for i in 0..m {
            out_e[of[from[i]]] = i;
            of[from[i]] += 1;
            in_e[inf[to[i]]] = i;
            inf[to[i]] += 1;
        }
    }

    let mut r = vec![0i64; n];
    let mut indeg: Vec<usize> = (0..n).map(|i| in_start[i + 1] - in_start[i]).collect();
    let mut topo = Vec::with_capacity(n);
    topo.extend((0..n).filter(|&i| indeg[i] == 0));
    let mut q = 0;
    while q < topo.len() {
        let v = topo[q];
        q += 1;
        for &ei in &out_e[out_start[v]..out_start[v + 1]] {
            let w = to[ei];
            if r[v] + len[ei] > r[w] {
                r[w] = r[v] + len[ei];
            }
            indeg[w] -= 1;
            if indeg[w] == 0 {
                topo.push(w);
            }
        }
    }

    // Longest-path ranking leaves loosely attached nodes far from their neighbours. Move each
    // node toward the side that has more edges.
    for _ in 0..8 {
        let mut changed = false;
        for &v in topo.iter().rev() {
            let ins = in_start[v + 1] - in_start[v];
            let outs = out_start[v + 1] - out_start[v];
            if ins == 0 && outs == 0 {
                continue;
            }
            let lo = in_e[in_start[v]..in_start[v + 1]]
                .iter()
                .map(|&ei| r[from[ei]] + len[ei])
                .max();
            let hi = out_e[out_start[v]..out_start[v + 1]]
                .iter()
                .map(|&ei| r[to[ei]] - len[ei])
                .min();
            let next = match (lo, hi) {
                (_, Some(hi)) if outs > ins => hi,
                (Some(lo), _) if ins > outs => lo,
                _ => r[v],
            };
            if next != r[v] {
                r[v] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut min = None::<i64>;
    let mut max = None::<i64>;
    let mut start_ports = false;
    for (i, node) in nodes.iter().enumerate() {
        if node.kind == Kind::StartPort {
            start_ports = true;
        }
        if node.kind == Kind::StartPort || node.kind == Kind::EndPort {
            continue;
        }
        min = Some(min.map_or(r[i], |min| min.min(r[i])));
        max = Some(max.map_or(r[i], |max| max.max(r[i])));
    }
    let mut min = min.unwrap_or(0);
    let max = max.unwrap_or(0);
    for (i, node) in nodes.iter().enumerate() {
        if node.kind == Kind::StartPort {
            r[i] = min - step;
        } else if node.kind == Kind::EndPort {
            r[i] = max + step;
        }
    }
    if start_ports {
        min -= step;
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        node.rank = usize::try_from(r[i] - min).unwrap_or(0);
    }
}

/// The ends of edges that may move along one side of a node (0 the side its edges arrive at, 1
/// the other) into `ids`, in the order of the nodes at their other ends. `room` receives the
/// least distance between each end and the one before it, and the result is their sum. With
/// too little room, ends that leave side by side move closer, then neighbours that run the same
/// way share a place.
fn side_ends(
    g: &Graph,
    v: usize,
    side: usize,
    span: f64,
    kinds: &[End],
    ids: &mut Vec<usize>,
    room: &mut Vec<f64>,
) -> f64 {
    let (start, list, other) = if side == 0 {
        (&g.pred_start, &g.pred_e, &g.ef)
    } else {
        (&g.succ_start, &g.succ_e, &g.et)
    };
    ids.clear();
    ids.extend(
        list[start[v]..start[v + 1]]
            .iter()
            .copied()
            .filter(|&k| kinds[k] != End::Held),
    );
    if ids.len() < 2 {
        return 0.0;
    }
    ids.sort_by(|&a, &b| g.pos[other[a]].cmp(&g.pos[other[b]]).then(a.cmp(&b)));
    let fill = |room: &mut Vec<f64>, mode: u8| {
        let mut total = 0.0;
        room.clear();
        room.push(0.0);
        for k in 1..ids.len() {
            let same = kinds[ids[k]] == kinds[ids[k - 1]];
            let share = match mode {
                1 if same && kinds[ids[k]] == End::Leaves => 0.5,
                2 if same => 0.0,
                _ => 1.0,
            };
            room.push(share);
            total += share;
        }
        total
    };
    let mut total = fill(room, 0);
    if span / total < MIN_PITCH {
        total = fill(room, 1);
    }
    if span / total < MIN_PITCH {
        total = fill(room, 2);
    }
    if total == 0.0 {
        return 0.0;
    }
    let pitch = PORT_PITCH.min(span / total);
    for value in room.iter_mut().skip(1) {
        *value *= pitch;
    }
    total * pitch
}

/// Gives the edges that meet one side of a node places of their own along it, around its
/// middle.
fn spread(g: &mut Graph, nodes: &[LNode], efk: &[End], etk: &[End]) {
    let mut ids = Vec::new();
    let mut room = Vec::new();
    for v in 0..g.real {
        let span = nodes[v].span.min(g.w[v]);
        if span <= 0.0 || nodes[v].kind != Kind::Node {
            continue;
        }
        for side in 0..2 {
            let kinds = if side == 0 { etk } else { efk };
            let total = side_ends(g, v, side, span, kinds, &mut ids, &mut room);
            if total == 0.0 {
                continue;
            }
            let dx = if side == 0 { &mut g.etd } else { &mut g.efd };
            let mut at = -total / 2.0;
            for (k, &id) in ids.iter().enumerate() {
                at += room[k];
                dx[id] = at;
            }
        }
    }
}

/// The segment of an edge that reaches the next point held in place, on the way up from
/// segment `e` (side 0) or down: past the bends in ranks that hold nothing else.
fn follow(g: &Graph, empty: Option<&[bool]>, mut e: usize, side: usize) -> usize {
    let Some(empty) = empty else {
        return e;
    };
    for _ in 0..100_000 {
        let u = if side == 0 { g.ef[e] } else { g.et[e] };
        if g.kind[u] != Kind::Dummy || !empty[g.rank[u]] {
            break;
        }
        e = if side == 0 {
            g.pred_e[g.pred_start[u]]
        } else {
            g.succ_e[g.succ_start[u]]
        };
    }
    e
}

/// How many ends that may move along it each node has on the side its edges arrive at, and on
/// the other.
fn free_ends(g: &Graph, efk: &[End], etk: &[End]) -> [Vec<u32>; 2] {
    let mut free = [vec![0u32; g.n], vec![0u32; g.n]];
    for e in 0..g.ef.len() {
        if etk[e] != End::Held {
            free[0][g.et[e]] += 1;
        }
        if efk[e] != End::Held {
            free[1][g.ef[e]] += 1;
        }
    }
    free
}

/// How far along the flow the handles of a plain S reach, as a share of the way it travels along
/// it. Half gives an even S; one that runs far across turns sooner, down to a quarter. The
/// layout uses this to tell how near two curves will pass, and the drawing to draw them.
pub(crate) fn ease(across: f64, along: f64) -> f64 {
    // `Math.abs(across) || 1` also replaces NaN.
    let across = if across == 0.0 || across.is_nan() {
        1.0
    } else {
        across.abs()
    };
    (4.0 * along.abs() / across).clamp(0.25, 0.5)
}

/// Where segment `e` ends on the way up (side 0) or down, and how many ends share that side
/// of the node there: none for a point that is not on a node.
fn segment_end(
    g: &Graph,
    nodes: &[LNode],
    xs: &[f64],
    free: &[Vec<u32>; 2],
    e: usize,
    side: usize,
) -> (f64, u32) {
    let (u, x) = if side == 0 {
        (g.ef[e], xs[g.ef[e]] + g.efd[e])
    } else {
        (g.et[e], xs[g.et[e]] + g.etd[e])
    };
    let shared = if u < g.real && nodes[u].kind == Kind::Node {
        free[1 - side][u]
    } else {
        0
    };
    (x, shared)
}

/// The room the neighbours in its rank leave a label or bend: a full gap from a node or
/// between two labels, half of one between a label and a line that passes it.
fn label_room(g: &Graph, xs: &[f64], opt: &LayeredOptions, v: usize) -> (f64, f64) {
    let sep = opt.edge_sep;
    let s = g.layer_start[g.rank[v]];
    let i = s + g.pos[v];
    let clear = |other: usize| {
        let extra = if g.kind[other] == Kind::Node {
            opt.node_sep / 2.0 + sep / 2.0
        } else if g.kind[other] == g.kind[v] {
            sep
        } else {
            sep / 2.0
        };
        g.w[other] / 2.0 + g.w[v] / 2.0 + extra
    };
    let lo = if i > s {
        xs[g.order[i - 1]] + clear(g.order[i - 1])
    } else {
        f64::NEG_INFINITY
    };
    let hi = if i + 1 < g.layer_start[g.rank[v] + 1] {
        xs[g.order[i + 1]] - clear(g.order[i + 1])
    } else {
        f64::INFINITY
    };
    (lo, hi)
}

/// The place between `lo` and `hi` that is in line with one of two ends, or else nearest the
/// first choice.
fn choose(x: f64, up: (f64, u32), down: (f64, u32), lo: f64, hi: f64) -> f64 {
    let up_first = if up.1 == down.1 {
        (up.0 - x).abs() <= (down.0 - x).abs()
    } else {
        up.1 < down.1
    };
    let choices = if up_first {
        [up.0, down.0]
    } else {
        [down.0, up.0]
    };
    choices
        .into_iter()
        .find(|&choice| choice >= lo && choice <= hi)
        .unwrap_or_else(|| clamp(choices[0], lo, hi))
}

/// Brings the labels and bends of each edge into line with an end of the edge, so that it runs
/// straight from that end and turns once, at the other: the end with fewer edges beside it, or
/// else the other, as near as the neighbours in their ranks leave room. Bends in ranks that hold
/// nothing else are left to `straighten`. An edge with no one line that fits brings each label
/// and bend into line with the nearest held point above or below.
fn align_labels(
    g: &Graph,
    nodes: &[LNode],
    xs: &mut [f64],
    free: &[Vec<u32>; 2],
    opt: &LayeredOptions,
    empty: Option<&[bool]>,
) {
    let placed = |v: usize| {
        g.kind[v] == Kind::Label
            || (g.kind[v] == Kind::Dummy && !empty.is_some_and(|empty| empty[g.rank[v]]))
    };
    let first_pred = |v: usize| g.pred_e[g.pred_start[v]];
    let first_succ = |v: usize| g.succ_e[g.succ_start[v]];
    let mut done = vec![false; g.n];
    let mut chain = Vec::new();
    for r in 1..g.max_rank {
        for i in g.layer_start[r]..g.layer_start[r + 1] {
            let v = g.order[i];
            if !placed(v) || done[v] {
                continue;
            }
            // The edge's labels and bends from its first node to its last, and the room they
            // share.
            let mut top = v;
            while is_dummy(g.kind[g.ef[first_pred(top)]]) {
                top = g.ef[first_pred(top)];
            }
            let mut bottom = top;
            let mut lo = f64::NEG_INFINITY;
            let mut hi = f64::INFINITY;
            chain.clear();
            let mut d = top;
            while is_dummy(g.kind[d]) {
                bottom = d;
                if placed(d) {
                    done[d] = true;
                    chain.push(d);
                    let (least, most) = label_room(g, xs, opt, d);
                    if least > lo {
                        lo = least;
                    }
                    if most < hi {
                        hi = most;
                    }
                }
                d = g.et[first_succ(d)];
            }
            if lo <= hi {
                let up = segment_end(g, nodes, xs, free, first_pred(top), 0);
                let down = segment_end(g, nodes, xs, free, first_succ(bottom), 1);
                let x = choose(xs[chain[0]], up, down, lo, hi);
                for &d in &chain {
                    xs[d] = x;
                }
                continue;
            }
            for &d in &chain {
                let (least, most) = label_room(g, xs, opt, d);
                if least > most {
                    continue;
                }
                let up = segment_end(g, nodes, xs, free, follow(g, empty, first_pred(d), 0), 0);
                let down = segment_end(g, nodes, xs, free, follow(g, empty, first_succ(d), 1), 1);
                xs[d] = choose(xs[d], up, down, least, most);
            }
        }
    }
}

/// Where the end of segment `e` on node `v` should be, from the middle of its side. The edge
/// is followed up (side 0) or down past bends in ranks that hold nothing else, to where it next
/// comes to a held point or another node's side.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors Pele's closure, which reads these from its scope"
)]
fn wanted(
    g: &Graph,
    nodes: &[LNode],
    xs: &[f64],
    kinds: (&[End], &[End]),
    free: &[Vec<u32>; 2],
    empty: Option<&[bool]>,
    e: usize,
    side: usize,
    v: usize,
    span: f64,
    again: bool,
) -> f64 {
    if free[side][v] < 2 {
        return 0.0;
    }
    let e = follow(g, empty, e, side);
    let (u, point) = if side == 0 {
        (g.ef[e], xs[g.ef[e]] + g.efd[e])
    } else {
        (g.et[e], xs[g.et[e]] + g.etd[e])
    };
    // Once the other end has its place, this one comes into line with it if it can.
    if again && (point - xs[v]).abs() <= span / 2.0 + IN_LINE {
        return (-span / 2.0).max((span / 2.0).min(point - xs[v]));
    }
    // The part of the other side that its end may move along, if it may move.
    let mut middle = point;
    let mut width = 0.0;
    let other = if side == 0 { kinds.0 } else { kinds.1 };
    if u < g.real && nodes[u].kind == Kind::Node && other[e] != End::Held && free[1 - side][u] >= 2
    {
        width = nodes[u].span.min(g.w[u]).max(0.0);
        if width > 0.0 {
            middle = xs[u];
        }
    }
    let lo = (xs[v] - span / 2.0).max(middle - width / 2.0);
    let hi = (xs[v] + span / 2.0).min(middle + width / 2.0);
    // Two sides with no part in line: the end leans towards the other, if its side is long
    // enough.
    if lo > hi + IN_LINE {
        let lean = (span / 2.0 - LEAN_CLEAR).max(0.0);
        return (-lean).max(lean.min(point - xs[v]));
    }
    // Both ends settle on one place in the part the two sides share: nearer the middle of the
    // narrower.
    let place = (xs[v] * width + middle * span) / (width + span);
    clamp(place, lo, hi) - xs[v]
}

/// Once the nodes have their places, moves an end along its side when that lets its edge run
/// straight. Any other end leans towards the point its edge runs to, and an end with a side to
/// itself stays in the middle. On a group, the ends keep clear of held ends.
fn settle(
    g: &mut Graph,
    nodes: &[LNode],
    xs: &[f64],
    efk: &[End],
    etk: &[End],
    free: &[Vec<u32>; 2],
    empty: Option<&[bool]>,
) {
    let mut ids = Vec::new();
    let mut room = Vec::new();
    let mut all = Vec::new();
    let mut sum: Vec<f64> = Vec::new();
    let mut count: Vec<usize> = Vec::new();
    // The ends of an edge follow each other, so the nodes are gone over until none moves.
    let mut moved = true;
    let mut pass = 0;
    while moved && pass < 6 {
        moved = false;
        for v in 0..g.real {
            let span = nodes[v].span.min(g.w[v]);
            if span <= 0.0 || nodes[v].kind != Kind::Node {
                continue;
            }
            for side in 0..2 {
                let kinds = if side == 0 { etk } else { efk };
                side_ends(g, v, side, span, kinds, &mut ids, &mut room);
                if ids.is_empty() {
                    continue;
                }
                if room.len() < ids.len() {
                    room.resize(ids.len(), 0.0);
                }
                if ids.len() == 1 {
                    room[0] = 0.0;
                }
                // Every end on the side in order; the held ones divide the rest into runs.
                all.clear();
                {
                    let (start, list, other) = if side == 0 {
                        (&g.pred_start, &g.pred_e, &g.ef)
                    } else {
                        (&g.succ_start, &g.succ_e, &g.et)
                    };
                    all.extend_from_slice(&list[start[v]..start[v + 1]]);
                    all.sort_by(|&a: &usize, &b: &usize| {
                        g.pos[other[a]].cmp(&g.pos[other[b]]).then(a.cmp(&b))
                    });
                }
                let dx = |g: &Graph, k: usize| if side == 0 { g.etd[k] } else { g.efd[k] };
                let mut k = 0;
                let mut lo = -span / 2.0;
                for i in 0..=all.len() {
                    let held = i < all.len() && kinds[all[i]] == End::Held;
                    if i < all.len() && !held {
                        continue;
                    }
                    // The run of free ends before this point is ids[k..], as many as were passed
                    // since the last held end.
                    let size = all[..i]
                        .iter()
                        .rev()
                        .take_while(|&&a| kinds[a] != End::Held)
                        .count();
                    let hi = if held {
                        dx(g, all[i]) - PORT_PITCH
                    } else {
                        span / 2.0
                    };
                    if size > 0 {
                        // The places nearest the wanted ones that keep the order and the room
                        // between neighbours: an isotonic fit, pooling neighbours that would
                        // otherwise be too close.
                        sum.clear();
                        count.clear();
                        let mut before = 0.0;
                        for j in 0..size {
                            if j > 0 {
                                before += room[k + j];
                            }
                            let want = wanted(
                                g,
                                nodes,
                                xs,
                                (efk, etk),
                                free,
                                empty,
                                ids[k + j],
                                side,
                                v,
                                span,
                                pass > 0,
                            );
                            sum.push(want - before);
                            count.push(1);
                            // Ends that share a place for want of room stay together.
                            let mut together = j > 0 && room[k + j] == 0.0;
                            while let [.., s2, s1] = sum[..]
                                && let [.., c2, c1] = count[..]
                                && (together || s2 / float(c2) > s1 / float(c1))
                            {
                                together = false;
                                sum.pop();
                                count.pop();
                                if let (Some(s), Some(c)) = (sum.last_mut(), count.last_mut()) {
                                    *s += s1;
                                    *c += c1;
                                }
                            }
                        }
                        let total = before;
                        let mut j = 0;
                        before = 0.0;
                        for b in 0..sum.len() {
                            // With too little room between two held ends, the run sits in the
                            // middle of what there is.
                            let level = if hi - total < lo {
                                (lo + hi - total) / 2.0
                            } else {
                                lo.max((hi - total).min(sum[b] / float(count[b])))
                            };
                            for _ in 0..count[b] {
                                if j > 0 {
                                    before += room[k + j];
                                }
                                let id = ids[k + j];
                                if (dx(g, id) - level - before).abs() > 0.01 {
                                    moved = true;
                                }
                                if side == 0 {
                                    g.etd[id] = level + before;
                                } else {
                                    g.efd[id] = level + before;
                                }
                                j += 1;
                            }
                        }
                        k += size;
                    }
                    if held {
                        lo = dx(g, all[i]) + PORT_PITCH;
                    }
                }
            }
        }
        pass += 1;
    }
}

/// Brings edges that are nearly straight into line, by moving a node a little across the flow
/// along with everything already in line with it, where the nodes beside them leave room.
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's layered.ts for side-by-side review"
)]
fn snap(g: &Graph, xs: &mut [f64], empty: Option<&[bool]>, opt: &LayeredOptions) {
    let passed = |u: usize| g.kind[u] == Kind::Dummy && empty.is_some_and(|empty| empty[g.rank[u]]);
    // Each edge from one held point to the next, past the bends in ranks that hold nothing
    // else.
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut da = Vec::new();
    let mut db = Vec::new();
    for e in 0..g.ef.len() {
        if passed(g.ef[e]) {
            continue;
        }
        let f = follow(g, empty, e, 1);
        a.push(g.ef[e]);
        b.push(g.et[f]);
        da.push(g.efd[e]);
        db.push(g.etd[f]);
    }
    let off = |xs: &[f64], k: usize| xs[b[k]] + db[k] - xs[a[k]] - da[k];
    // The nodes in line with each other move as one.
    let mut block: Vec<usize> = (0..g.n).collect();
    let mut members: Vec<Vec<usize>> = (0..g.n).map(|i| vec![i]).collect();
    let join = |block: &mut Vec<usize>, members: &mut Vec<Vec<usize>>, x: usize, y: usize| {
        let mut keep = block[x];
        let mut drop = block[y];
        if keep == drop {
            return;
        }
        if members[keep].len() < members[drop].len() {
            std::mem::swap(&mut keep, &mut drop);
        }
        let moving = std::mem::take(&mut members[drop]);
        for &v in &moving {
            block[v] = keep;
        }
        members[keep].extend(moving);
    };
    let mut near = Vec::new();
    for k in 0..a.len() {
        let d = off(xs, k).abs();
        if d < 0.01 {
            join(&mut block, &mut members, a[k], b[k]);
        } else if d <= SNAP {
            near.push(k);
        }
    }
    if near.is_empty() {
        return;
    }
    near.sort_by(|&x, &y| cmp(off(xs, x).abs(), off(xs, y).abs()).then(x.cmp(&y)));
    let gap = |v: usize| half_room(g, opt, v);
    // Whether every node of a block can move by `shift` and keep its room from those beside it.
    let free = |xs: &[f64], block: &[usize], members: &[Vec<usize>], id: usize, shift: f64| {
        for &v in &members[id] {
            let s = g.layer_start[g.rank[v]];
            let end = g.layer_start[g.rank[v] + 1];
            let i = s + g.pos[v];
            if i > s {
                let left = g.order[i - 1];
                if block[left] != id && xs[v] + shift - xs[left] < gap(v) + gap(left) - 0.01 {
                    return false;
                }
            }
            if i + 1 < end {
                let right = g.order[i + 1];
                if block[right] != id && xs[right] - xs[v] - shift < gap(v) + gap(right) - 0.01 {
                    return false;
                }
            }
        }
        true
    };
    for k in near {
        let d = off(xs, k);
        if d.abs() < 0.01 {
            join(&mut block, &mut members, a[k], b[k]);
            continue;
        }
        if d.abs() > SNAP || block[a[k]] == block[b[k]] {
            continue;
        }
        // The smaller of the two blocks moves, if it can; else the other.
        let upper = block[a[k]];
        let lower = block[b[k]];
        let first = if members[upper].len() <= members[lower].len() {
            upper
        } else {
            lower
        };
        let second = if first == upper { lower } else { upper };
        for id in [first, second] {
            let shift = if id == upper { d } else { -d };
            if !free(xs, &block, &members, id, shift) {
                continue;
            }
            for &v in &members[id] {
                xs[v] += shift;
            }
            join(&mut block, &mut members, a[k], b[k]);
            break;
        }
    }
}

/// Marks the ranks that hold nothing but the bends of edges, if there are any.
fn empty_ranks(g: &Graph) -> Option<Vec<bool>> {
    let mut empty = vec![false; g.max_rank + 1];
    let mut any = false;
    for r in 1..g.max_rank {
        let layer = &g.order[g.layer_start[r]..g.layer_start[r + 1]];
        if !layer.is_empty() && layer.iter().all(|&v| g.kind[v] == Kind::Dummy) {
            empty[r] = true;
            any = true;
        }
    }
    any.then_some(empty)
}

/// Where the pieces of one gap begin and end across the flow, worked out once since they are
/// compared pair by pair.
struct Bounds<'a> {
    /// Where each piece leaves the rank above.
    from: &'a [f64],
    /// The right end of each piece.
    hi: Vec<f64>,
    /// The left end of each piece.
    lo: Vec<f64>,
    /// Where each piece reaches the rank below.
    to: &'a [f64],
}

impl Bounds<'_> {
    /// 1 when `x` lies across piece `k`, else 0.
    fn within(&self, x: f64, k: usize) -> usize {
        usize::from(x >= self.lo[k] - IN_LINE && x <= self.hi[k] + IN_LINE)
    }

    /// Whether `a` and `b` leave from or arrive at the same point.
    fn mates(&self, a: usize, b: usize) -> bool {
        (self.from[a] - self.from[b]).abs() < IN_LINE || (self.to[a] - self.to[b]).abs() < IN_LINE
    }

    /// Whether `a` and `b` overlap across the flow.
    fn overlap(&self, a: usize, b: usize) -> bool {
        self.lo[a] < self.hi[b] - IN_LINE && self.lo[b] < self.hi[a] - IN_LINE
    }

    /// Whether `a` and `b` need tracks of their own.
    fn clash(&self, a: usize, b: usize) -> bool {
        self.overlap(a, b) && !self.mates(a, b)
    }

    /// Whether one of `a` and `b` could cross the other, depending on which is above.
    fn meet(&self, a: usize, b: usize) -> bool {
        self.overlap(a, b)
            || self.within(self.to[a], b)
                + self.within(self.from[b], a)
                + self.within(self.to[b], a)
                + self.within(self.from[a], b)
                > 0
    }

    /// How often `a` and `b` cross with `a` on the higher track.
    fn crossings(&self, a: usize, b: usize) -> usize {
        self.within(self.to[a], b) + self.within(self.from[b], a)
    }

    /// Points along piece `k` as a plain curve across a gap of height `h`, into `out`.
    #[expect(
        clippy::many_single_char_names,
        reason = "names mirror Pele's layered.ts for side-by-side review"
    )]
    fn sample(&self, k: usize, h: f64, out: &mut [f64; 2 * SAMPLES + 2]) {
        let c = ease(self.to[k] - self.from[k], h);
        for i in 0..=SAMPLES {
            let t = float(i) / float(SAMPLES);
            let u = 1.0 - t;
            out[2 * i] = self.from[k] + (self.to[k] - self.from[k]) * t * t * (3.0 - 2.0 * t);
            out[2 * i + 1] = h * (3.0 * c * t * u * u + 3.0 * (1.0 - c) * t * t * u + t * t * t);
        }
    }
}

/// The least distance from the points of one sampled curve to the polyline of another.
fn reach(points: &[f64; 2 * SAMPLES + 2], line: &[f64; 2 * SAMPLES + 2]) -> f64 {
    let mut least = f64::INFINITY;
    for i in 0..=SAMPLES {
        let px = points[2 * i];
        let py = points[2 * i + 1];
        for j in 0..SAMPLES {
            let dx = line[2 * j + 2] - line[2 * j];
            let dy = line[2 * j + 3] - line[2 * j + 1];
            let mut den = dx * dx + dy * dy;
            if den == 0.0 || den.is_nan() {
                den = 1.0;
            }
            let t = clamp(
                ((px - line[2 * j]) * dx + (py - line[2 * j + 1]) * dy) / den,
                0.0,
                1.0,
            );
            let d = hypot(px - line[2 * j] - t * dx, py - line[2 * j + 1] - t * dy);
            if d < least {
                least = d;
            }
        }
    }
    least
}

/// Gives the pieces of one gap in `list` their tracks, and each the number of tracks that the
/// pieces it has to do with take between them, counting its own from the first of those when
/// `curved`. Returns the number of tracks. The pieces come without tracks.
fn lay(
    bounds: &Bounds<'_>,
    track: &mut [isize],
    levels: &mut [usize],
    list: &mut [usize],
    curved: bool,
) -> usize {
    let (lo, hi) = (&bounds.lo, &bounds.hi);
    // A plain order to start from: the pieces that reach furthest go first, so that those of one
    // fan nest.
    list.sort_by(|&a, &b| {
        cmp(hi[b] - lo[b], hi[a] - lo[a])
            .then(cmp(lo[a], lo[b]))
            .then(a.cmp(&b))
    });
    let count = list.len();
    let mut above: Vec<Vec<usize>> = Vec::new();
    let mut group: Vec<usize> = (0..count).collect();
    let find = |group: &mut Vec<usize>, mut i: usize| {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    };
    let mut sequence: Vec<usize> = (0..count).collect();
    if count <= MAX_WEIGHED {
        // above[j] lists the pieces that should be above list[j]. A piece is placed once all
        // that should be above it are, and when none is free, the one with the fewest waiting.
        let mut waiting = vec![0usize; count];
        let mut after: Vec<Vec<usize>> = vec![Vec::new(); count];
        above = vec![Vec::new(); count];
        for i in 0..count {
            for j in i + 1..count {
                let a = list[i];
                let b = list[j];
                if !bounds.meet(a, b) {
                    continue;
                }
                let root = find(&mut group, i);
                group[root] = find(&mut group, j);
                let ab = bounds.crossings(a, b);
                let ba = bounds.crossings(b, a);
                if ab == ba {
                    continue;
                }
                let (first, second) = if ab < ba { (i, j) } else { (j, i) };
                above[second].push(first);
                after[first].push(second);
                waiting[second] += 1;
            }
        }
        let mut placed = vec![false; count];
        sequence.clear();
        for _ in 0..count {
            let mut pick = None::<usize>;
            for i in 0..count {
                if placed[i] {
                    continue;
                }
                if pick.is_none_or(|p| waiting[i] < waiting[p]) {
                    pick = Some(i);
                }
                if waiting[i] == 0 {
                    break;
                }
            }
            let Some(pick) = pick else {
                break;
            };
            placed[pick] = true;
            sequence.push(pick);
            for &j in &after[pick] {
                waiting[j] -= 1;
            }
        }
    }
    // Each piece takes the first track below those it should be under that has room for it.
    // `sequence` holds places in `list`.
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for &i in &sequence {
        let k = list[i];
        let mut t: isize = 0;
        if let Some(over) = above.get(i) {
            for &o in over {
                if track[list[o]] >= t {
                    t = track[list[o]] + 1;
                }
            }
        }
        loop {
            let slot = usize::try_from(t).unwrap_or(0);
            if rows.len() <= slot {
                rows.resize(slot + 1, Vec::new());
            }
            if rows[slot].iter().all(|&other| !bounds.clash(k, other)) {
                rows[slot].push(k);
                break;
            }
            t += 1;
        }
        track[k] = t;
    }
    let mut first = vec![isize::MAX; count];
    let mut last = vec![isize::MIN; count];
    for (i, &k) in list.iter().enumerate() {
        let root = find(&mut group, i);
        first[root] = first[root].min(track[k]);
        last[root] = last[root].max(track[k]);
    }
    for (i, &k) in list.iter().enumerate() {
        let root = find(&mut group, i);
        levels[k] = usize::try_from(last[root] - first[root] + 1).unwrap_or(0);
        if curved {
            track[k] -= first[root];
        }
    }
    rows.len()
}

/// Gives every piece that does not run straight down a track across its gap, and counts the
/// tracks of each gap. Two pieces share a track only where they cannot be mistaken for each
/// other, and which goes above the other is whichever makes them cross less. With `curved`, the
/// tracks are levels for curves to turn onto, given only to pieces that would run along or too
/// near another. `room` is the height of the gap under each rank, and `tall` receives the
/// height that a gap whose curves mostly cross should have instead of levels.
fn assign_tracks(
    pieces: &mut Pieces,
    track_count: &mut [usize],
    curved: bool,
    room: &[f64],
    tall: &mut [f64],
) {
    let Pieces {
        from,
        levels,
        rank,
        to,
        track,
        wide,
        ..
    } = pieces;
    let total = from.len();
    // Pieces by rank, ranks in order of their first piece.
    let mut slot: Vec<Option<usize>> = vec![None; track_count.len()];
    let mut by_rank: Vec<(usize, Vec<usize>)> = Vec::new();
    for k in 0..total {
        track.push(-1);
        levels.push(0);
        if (from[k] - to[k]).abs() < IN_LINE {
            continue;
        }
        let r = rank[k];
        if let Some(s) = slot[r] {
            by_rank[s].1.push(k);
        } else {
            slot[r] = Some(by_rank.len());
            by_rank.push((r, vec![k]));
        }
    }
    let bounds = Bounds {
        from,
        hi: (0..total).map(|k| from[k].max(to[k])).collect(),
        lo: (0..total).map(|k| from[k].min(to[k])).collect(),
        to,
    };
    let mut line_a = [0.0; 2 * SAMPLES + 2];
    let mut line_b = [0.0; 2 * SAMPLES + 2];
    // The least distance between a and b as plain curves across a gap that leaves them
    // `height` to turn in.
    let mut nearest = |a: usize, b: usize, height: f64| {
        let h = (height / 2.0).max(height - 2.0 * LEVEL_LEAD);
        bounds.sample(a, h, &mut line_a);
        bounds.sample(b, h, &mut line_b);
        reach(&line_a, &line_b).min(reach(&line_b, &line_a))
    };
    let mut in_level = vec![false; total];
    let mut turning = Vec::new();

    for (r, mut all) in by_rank {
        if !curved {
            track_count[r] = lay(&bounds, track, levels, &mut all, curved);
            continue;
        }
        if all.len() > MAX_WEIGHED {
            continue;
        }
        // The pieces that turn onto levels: first those that run far across and have another
        // such piece to be told apart from, then any two that would pass too near each other.
        let mut far: Vec<usize> = all.iter().copied().filter(|&k| wide[k]).collect();
        lay(&bounds, track, levels, &mut far, curved);
        turning.clear();
        for &k in &far {
            if levels[k] > 1 {
                in_level[k] = true;
                turning.push(k);
            }
        }
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                let a = all[i];
                let b = all[j];
                if (in_level[a] && in_level[b])
                    || !bounds.clash(a, b)
                    || (from[a] < to[a]) != (from[b] < to[b])
                {
                    continue;
                }
                let limit = if wide[a] || wide[b] {
                    LEVEL_CLEAR
                } else {
                    LEVEL_TOUCH
                };
                if nearest(a, b, room[r]) < limit {
                    for k in [a, b] {
                        if !in_level[k] {
                            in_level[k] = true;
                            turning.push(k);
                        }
                    }
                }
            }
        }
        for &k in &far {
            track[k] = -1;
            levels[k] = 0;
        }
        if turning.is_empty() {
            continue;
        }
        let mut crossing = 0;
        let mut alongside = 0;
        for i in 0..turning.len() {
            for j in i + 1..turning.len() {
                let a = turning[i];
                let b = turning[j];
                if !bounds.overlap(a, b) {
                    continue;
                }
                if (from[a] < to[a]) != (from[b] < to[b]) {
                    crossing += 1;
                } else if !bounds.mates(a, b) {
                    alongside += 1;
                }
            }
        }
        if crossing > alongside {
            for &k in &all {
                let height = (bounds.hi[k] - bounds.lo[k]) * CROSSED_SLOPE;
                if height > tall[r] {
                    tall[r] = height;
                }
            }
        } else {
            // With them go the short pieces that could cross one of them, so that each has its
            // place among the levels and crosses none.
            let mut members: Vec<usize> = all
                .iter()
                .copied()
                .filter(|&k| {
                    in_level[k] || (!wide[k] && turning.iter().any(|&j| bounds.meet(k, j)))
                })
                .collect();
            lay(&bounds, track, levels, &mut members, curved);
            track_count[r] = members.iter().map(|&k| levels[k]).max().unwrap_or(0);
        }
        for &k in &turning {
            in_level[k] = false;
        }
    }
}

/// Moves the bends in ranks that hold only bends onto the straight line between the nearest
/// held points of their edges, and marks the ones that can be left out of the route. `sep` is
/// the room kept between the bends of twin edges, or 0 to leave those out as well.
fn straighten(
    g: &Graph,
    xs: &mut [f64],
    empty: &[bool],
    first_dummy: &[Option<usize>],
    span: &[usize],
    sep: f64,
    on_line: &mut [bool],
) {
    let mut want = vec![0.0; xs.len()];
    // The points that each bend's edge runs between, across the ranks that hold only bends.
    let mut from = vec![0.0; xs.len()];
    let mut to = vec![0.0; xs.len()];
    for (ei, d0) in first_dummy.iter().enumerate() {
        let Some(d0) = *d0 else {
            continue;
        };
        let end = d0 + span[ei] - 1;
        // A bend's rank is its place in the run, counted from the rank after the edge's first
        // node.
        let base = g.rank[d0];
        let mut d = d0;
        while d < end {
            if !empty[base + d - d0] {
                d += 1;
                continue;
            }
            let mut last = d;
            while last + 1 < end && empty[base + last + 1 - d0] {
                last += 1;
            }
            let above = g.pred_e[g.pred_start[d]];
            let below = g.succ_e[g.succ_start[last]];
            let x0 = xs[g.ef[above]] + g.efd[above];
            let x1 = xs[g.et[below]] + g.etd[below];
            let steps = float(last - d + 2);
            for k in d..=last {
                want[k] = x0 + ((x1 - x0) * float(k - d + 1)) / steps;
                from[k] = x0;
                to[k] = x1;
            }
            d = last + 1;
        }
    }

    // Nothing in such a rank is in a bend's way, so each one goes onto its line and out of the
    // route. Only twins stay, a little apart around the line.
    let mut twins = Vec::new();
    let mut row = Vec::new();
    for r in 1..g.max_rank {
        if !empty[r] {
            continue;
        }
        row.clear();
        row.extend_from_slice(&g.order[g.layer_start[r]..g.layer_start[r + 1]]);
        row.sort_by(|&a, &b| {
            cmp(from[a], from[b])
                .then(cmp(to[a], to[b]))
                .then(a.cmp(&b))
        });
        let mut i = 0;
        while i < row.len() {
            twins.clear();
            let mut j = i;
            while j < row.len()
                && (from[row[j]] - from[row[i]]).abs() < 1.0
                && (to[row[j]] - to[row[i]]).abs() < 1.0
            {
                twins.push(row[j]);
                j += 1;
            }
            // The twins keep the order the rank gave them, so two edges do not swap sides.
            twins.sort_by_key(|&d| g.pos[d]);
            let middle = float(twins.len() - 1) / 2.0;
            for (k, &d) in twins.iter().enumerate() {
                xs[d] = want[d] + (float(k) - middle) * sep;
                if twins.len() == 1 || sep == 0.0 {
                    on_line[d] = true;
                }
            }
            i = j;
        }
    }
}

/// In a rank that holds labels and bends but no nodes, leaves out each bend whose edge would
/// pass clear of every label anyway, so that the edge turns in the whole height between the
/// ranks on either side.
fn past_labels(
    g: &Graph,
    xs: &mut [f64],
    first_dummy: &[Option<usize>],
    span: &[usize],
    clear: f64,
    on_line: &mut [bool],
) {
    let mut labelled = vec![false; g.max_rank + 1];
    let mut any = false;
    for r in 1..g.max_rank {
        let mut labels = false;
        let mut others = false;
        for &v in &g.order[g.layer_start[r]..g.layer_start[r + 1]] {
            if g.kind[v] == Kind::Label {
                labels = true;
            } else if g.kind[v] != Kind::Dummy {
                others = true;
            }
        }
        if labels && !others {
            labelled[r] = true;
            any = true;
        }
    }
    if !any {
        return;
    }
    for (ei, d0) in first_dummy.iter().enumerate() {
        let Some(d0) = *d0 else {
            continue;
        };
        let end = d0 + span[ei] - 1;
        for d in d0..end {
            if g.kind[d] != Kind::Dummy || !labelled[g.rank[d]] {
                continue;
            }
            // The points on either side that the edge is held at, past bends already left out.
            let mut first = d;
            let mut last = d;
            while first > d0 && on_line[first - 1] {
                first -= 1;
            }
            while last + 1 < end && on_line[last + 1] {
                last += 1;
            }
            let above = g.pred_e[g.pred_start[first]];
            let below = g.succ_e[g.succ_start[last]];
            let from = xs[g.ef[above]] + g.efd[above];
            let to = xs[g.et[below]] + g.etd[below];
            let lo = from.min(to) - clear;
            let hi = from.max(to) + clear;
            let r = g.rank[d];
            let free = g.order[g.layer_start[r]..g.layer_start[r + 1]]
                .iter()
                .all(|&label| {
                    g.kind[label] != Kind::Label
                        || xs[label] + g.w[label] / 2.0 <= lo
                        || xs[label] - g.w[label] / 2.0 >= hi
                });
            if !free {
                continue;
            }
            on_line[d] = true;
            // The bends left out between the two points lie on the line between them.
            let steps = float(last - first + 2);
            for k in first..=last {
                xs[k] = from + ((to - from) * float(k - first + 1)) / steps;
            }
        }
    }
}

/// Reusable buffers for counting crossings.
struct Counter {
    /// Segments of one node, for sorting.
    ids: Vec<usize>,
    /// With offsets, each segment's place along its head's side.
    place: Option<Vec<usize>>,
    /// Each segment's head place, rank by rank.
    targets: Vec<usize>,
    /// Accumulation tree.
    tree: Vec<u32>,
}

/// Counts the crossings between every two neighbouring ranks.
#[expect(clippy::float_cmp, reason = "Pele compares offsets exactly")]
fn count_crossings(g: &Graph, c: &mut Counter) -> u64 {
    let Counter {
        ids,
        place,
        targets,
        tree,
    } = c;
    let ls = &g.layer_start;
    let mut total = 0u64;
    for r in 0..g.max_rank {
        let mut south_len = ls[r + 2] - ls[r + 1];
        if let Some(place) = place.as_mut() {
            let mut places = 0;
            for &v in &g.order[ls[r + 1]..ls[r + 2]] {
                let from = g.pred_start[v];
                let to = g.pred_start[v + 1];
                let mixed = (from + 1..to).any(|k| g.etd[g.pred_e[k]] != g.etd[g.pred_e[from]]);
                if mixed {
                    ids.clear();
                    ids.extend_from_slice(&g.pred_e[from..to]);
                    ids.sort_by(|&a, &b| cmp(g.etd[a], g.etd[b]));
                    for k in 0..ids.len() {
                        if k > 0 && g.etd[ids[k]] != g.etd[ids[k - 1]] {
                            places += 1;
                        }
                        place[ids[k]] = places;
                    }
                } else {
                    for &e in &g.pred_e[from..to] {
                        place[e] = places;
                    }
                }
                places += 1;
            }
            south_len = places;
        }
        if south_len < 2 {
            continue;
        }
        let mut t = 0;
        for &v in &g.order[ls[r]..ls[r + 1]] {
            let from = g.succ_start[v];
            let to = g.succ_start[v + 1];
            let t0 = t;
            for &e in &g.succ_e[from..to] {
                targets[t] = place.as_ref().map_or(g.pos[g.et[e]], |place| place[e]);
                t += 1;
            }
            if t - t0 < 2 {
                continue;
            }
            let mixed = place.is_some()
                && (from + 1..to).any(|k| g.efd[g.succ_e[k]] != g.efd[g.succ_e[from]]);
            if let Some(place) = place.as_ref()
                && mixed
            {
                ids.clear();
                ids.extend_from_slice(&g.succ_e[from..to]);
                ids.sort_by(|&a, &b| cmp(g.efd[a], g.efd[b]).then(place[a].cmp(&place[b])));
                for (k, &id) in ids.iter().enumerate() {
                    targets[t0 + k] = place[id];
                }
            } else {
                targets[t0..t].sort_unstable();
            }
        }
        if t < 2 {
            continue;
        }
        let leaves = south_len.next_power_of_two();
        tree[..2 * leaves].fill(0);
        for &target in &targets[..t] {
            let mut i = target + leaves;
            tree[i] += 1;
            while i > 1 {
                if i & 1 == 0 {
                    total += u64::from(tree[i + 1]);
                }
                i >>= 1;
                tree[i] += 1;
            }
        }
    }
    total
}

/// One barycenter sweep down (from the ranks above) or up, ties broken left or right.
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's layered.ts for side-by-side review"
)]
#[expect(
    clippy::float_cmp,
    reason = "Pele marks nodes without neighbours with exactly -1"
)]
fn sweep(
    g: &mut Graph,
    nodes: &[LNode],
    bary: &mut [f64],
    moving: &mut [usize],
    down: bool,
    bias_right: bool,
) {
    for step in 1..=g.max_rank {
        let r = if down { step } else { g.max_rank - step };
        let s = g.layer_start[r];
        let e = g.layer_start[r + 1];
        let mut movable = 0;
        for i in s..e {
            let v = g.order[i];
            let pin = if v < g.real { nodes[v].pin } else { 0.0 };
            if pin != 0.0 {
                bary[v] = pin * 1e9;
                moving[movable] = v;
                movable += 1;
                continue;
            }
            let (start, list, other, other_dx) = if down {
                (&g.pred_start, &g.pred_e, &g.ef, &g.efd)
            } else {
                (&g.succ_start, &g.succ_e, &g.et, &g.etd)
            };
            let from = start[v];
            let to = start[v + 1];
            if from == to {
                bary[v] = -1.0;
                continue;
            }
            let mut sum = 0.0;
            for &k in &list[from..to] {
                let u = other[k];
                sum += float(g.pos[u]);
                if g.offsets && g.w[u] > 0.0 {
                    sum += other_dx[k] / g.w[u];
                }
            }
            bary[v] = sum / float(to - from);
            moving[movable] = v;
            movable += 1;
        }
        if movable < 2 {
            continue;
        }
        let pos = &g.pos;
        moving[..movable].sort_by(|&a, &b| {
            let tie = if bias_right {
                pos[b].cmp(&pos[a])
            } else {
                pos[a].cmp(&pos[b])
            };
            cmp(bary[a], bary[b]).then(tie)
        });
        let mut k = 0;
        for i in s..e {
            if bary[g.order[i]] != -1.0 {
                g.order[i] = moving[k];
                k += 1;
            }
        }
        for i in s..e {
            g.pos[g.order[i]] = i - s;
        }
    }
}

/// How many pairs of nodes in one rank are not in the order they were given.
fn out_of_order(g: &Graph) -> usize {
    let mut total = 0;
    for r in 0..=g.max_rank {
        let end = g.layer_start[r + 1];
        for i in g.layer_start[r]..end {
            if g.order[i] >= g.real {
                continue;
            }
            total += g.order[i + 1..end]
                .iter()
                .filter(|&&o| o < g.order[i])
                .count();
        }
    }
    total
}

/// Orders each rank to reduce crossings by barycenter sweeps, keeping the best order found.
/// Small graphs try two sweep orders and a mirror image, keeping the one closest to the order
/// the nodes were given among those with the fewest crossings.
fn reduce_crossings(g: &mut Graph, nodes: &[LNode]) {
    let segments = g.ef.len();
    let widest = (0..=g.max_rank)
        .map(|r| g.layer_start[r + 1] - g.layer_start[r])
        .max()
        .unwrap_or(0);
    // With offsets, edges that reach one node at different offsets take separate places.
    let place = g.offsets.then(|| vec![0usize; segments]);
    let size = if place.is_some() {
        widest + segments
    } else {
        widest
    }
    .next_power_of_two();
    let mut counter = Counter {
        ids: Vec::new(),
        place,
        targets: vec![0; segments],
        tree: vec![0; 2 * size],
    };

    let mut best = count_crossings(g, &mut counter);
    if best == 0 {
        return;
    }
    let mut best_order = g.order.clone();
    let mut bary = vec![0.0; g.n];
    let mut moving = vec![0usize; widest];

    // The sweeps run twice from the same start, going down first and going up first. When both
    // cross as little, the one that keeps more nodes in their given order wins.
    let initial = g.order.clone();
    let start = best;
    let small = g.n <= 600;
    let mut chosen = best_order.clone();
    let mut chosen_out = usize::MAX;
    for attempt in 0..if small { 2 } else { 1 } {
        if attempt > 0 {
            g.order.copy_from_slice(&initial);
            g.set_positions();
        }
        let mut found = start;
        let mut found_order = initial.clone();
        let mut stale = 0;
        let mut i = 0;
        while stale < 4 && i < MAX_SWEEPS {
            sweep(
                g,
                nodes,
                &mut bary,
                &mut moving,
                (i + attempt).is_multiple_of(2),
                i % 4 >= 2,
            );
            let c = count_crossings(g, &mut counter);
            if c < found {
                found = c;
                found_order.copy_from_slice(&g.order);
                stale = 0;
                if c == 0 {
                    break;
                }
            }
            i += 1;
            stale += 1;
        }
        g.order.copy_from_slice(&found_order);
        let out = if small { out_of_order(g) } else { 0 };
        if attempt == 0 || found < best || (found == best && out < chosen_out) {
            best = found;
            chosen = found_order;
            chosen_out = out;
        }
    }
    g.order.copy_from_slice(&chosen);
    g.set_positions();
    best_order.copy_from_slice(&chosen);

    // A drawing and its mirror image cross as often too. Of those two, likewise.
    if !small || nodes.iter().any(|v| v.pin != 0.0) || chosen_out == 0 {
        return;
    }
    for r in 0..=g.max_rank {
        g.order[g.layer_start[r]..g.layer_start[r + 1]].reverse();
    }
    g.set_positions();
    if count_crossings(g, &mut counter) > best || out_of_order(g) >= chosen_out {
        g.order.copy_from_slice(&best_order);
    }
}

/// Half the room a node, bend, or port takes across the flow, with its share of the gap beside
/// it.
fn half_room(g: &Graph, opt: &LayeredOptions, v: usize) -> f64 {
    let sep = if g.kind[v] == Kind::Node {
        opt.node_sep
    } else {
        opt.edge_sep
    };
    g.w[v] / 2.0 + sep / 2.0
}

/// Brandes-Köpf horizontal coordinate assignment: four aligned layouts, balanced. Sorts each
/// node's segments by the place of their other ends, which later passes rely on.
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's layered.ts for side-by-side review"
)]
fn position(g: &mut Graph, opt: &LayeredOptions) -> Vec<f64> {
    let n = g.n;
    let max_rank = g.max_rank;
    let segments = g.ef.len();
    let gap: Vec<f64> = (0..n).map(|i| half_room(g, opt, i)).collect();

    // Neighbours in left-to-right order, so the medians can be read off directly.
    let mut key = vec![0usize; segments];
    for e in 0..segments {
        key[e] = g.pos[g.et[e]];
    }
    for v in 0..n {
        if g.succ_start[v + 1] - g.succ_start[v] > 1 {
            sort_range(&mut g.succ_e, g.succ_start[v], g.succ_start[v + 1], &key);
        }
    }
    for e in 0..segments {
        key[e] = g.pos[g.ef[e]];
    }
    for v in 0..n {
        if g.pred_start[v + 1] - g.pred_start[v] > 1 {
            sort_range(&mut g.pred_e, g.pred_start[v], g.pred_start[v + 1], &key);
        }
    }
    let g = &*g;

    // An edge between two real nodes may not cross a dummy-to-dummy segment; mark those that
    // would.
    let mut conflict = vec![false; segments];
    for r in 1..=max_rank {
        let prev_len = g.layer_start[r] - g.layer_start[r - 1];
        let s = g.layer_start[r];
        let end = g.layer_start[r + 1];
        let mut k0 = 0;
        let mut scan = s;
        for i in s..end {
            let v = g.order[i];
            let mut inner = None;
            if is_dummy(g.kind[v]) {
                let u = g.ef[g.pred_e[g.pred_start[v]]];
                if is_dummy(g.kind[u]) {
                    inner = Some(u);
                }
            }
            let k1 = inner.map_or(prev_len, |u| g.pos[u]);
            if inner.is_some() || i == end - 1 {
                for &w in &g.order[scan..=i] {
                    let w_dummy = is_dummy(g.kind[w]);
                    for &e in &g.pred_e[g.pred_start[w]..g.pred_start[w + 1]] {
                        let up = g.pos[g.ef[e]];
                        if (up < k0 || k1 < up) && !(w_dummy && is_dummy(g.kind[g.ef[e]])) {
                            conflict[e] = true;
                        }
                    }
                }
                scan = i + 1;
                k0 = k1;
            }
        }
    }

    let mut results: Vec<Vec<f64>> = Vec::with_capacity(4);
    let mut root = vec![0usize; n];
    let mut align = vec![0usize; n];
    // Offset of each node from its block's root, so that aligned edges meet at their
    // attachment points.
    let mut shift = vec![0.0; n];
    let mut e_from = vec![0usize; n];
    let mut e_to = vec![0usize; n];
    let mut e_w = vec![0.0; n];
    let mut indeg = vec![0usize; n];
    let mut out_start = vec![0usize; n + 1];
    let mut out_e = vec![0usize; n];
    let mut fill = vec![0usize; n];
    let mut topo = vec![0usize; n];

    for variant in 0..4 {
        let up = variant < 2;
        let right = variant & 1 == 1;
        let (start, list, other, own_dx, other_dx) = if up {
            (&g.pred_start, &g.pred_e, &g.ef, &g.etd, &g.efd)
        } else {
            (&g.succ_start, &g.succ_e, &g.et, &g.efd, &g.etd)
        };
        for i in 0..n {
            root[i] = i;
            align[i] = i;
            shift[i] = 0.0;
        }

        for step in 0..=max_rank {
            let r = if up { step } else { max_rank - step };
            let s = g.layer_start[r];
            let end = g.layer_start[r + 1];
            let adjacent = if up {
                r.checked_sub(1)
            } else {
                Some(r + 1).filter(|&a| a <= max_rank)
            };
            let adjacent_last =
                adjacent.map_or(0, |a| signed(g.layer_start[a + 1] - g.layer_start[a]) - 1);
            let mut prev_idx: isize = -1;
            for o in 0..end - s {
                let i = if right { end - 1 - o } else { s + o };
                let v = g.order[i];
                let from = start[v];
                let d = start[v + 1] - from;
                if d == 0 {
                    continue;
                }
                let lo = from + ((d - 1) >> 1);
                let hi = from + (d >> 1);
                for pass in 0..2 {
                    if pass == 1 && lo == hi {
                        break;
                    }
                    let e = list[if (pass == 0) == right { hi } else { lo }];
                    let w = other[e];
                    let idx = if right {
                        adjacent_last - signed(g.pos[w])
                    } else {
                        signed(g.pos[w])
                    };
                    if align[v] == v && prev_idx < idx && !conflict[e] {
                        align[w] = v;
                        root[v] = root[w];
                        align[v] = root[v];
                        shift[v] = shift[w] + other_dx[e] - own_dx[e];
                        prev_idx = idx;
                    }
                }
            }
        }

        let mut block_edges = 0;
        indeg.fill(0);
        out_start.fill(0);
        for r in 0..=max_rank {
            let s = g.layer_start[r];
            let end = g.layer_start[r + 1];
            for i in s + 1..end {
                let (a, b) = if right {
                    (g.order[i], g.order[i - 1])
                } else {
                    (g.order[i - 1], g.order[i])
                };
                let sep = gap[a] + gap[b];
                e_from[block_edges] = root[a];
                e_to[block_edges] = root[b];
                e_w[block_edges] = if right {
                    sep - shift[a] + shift[b]
                } else {
                    sep + shift[a] - shift[b]
                };
                indeg[root[b]] += 1;
                out_start[root[a] + 1] += 1;
                block_edges += 1;
            }
        }
        for i in 0..n {
            out_start[i + 1] += out_start[i];
        }
        fill.copy_from_slice(&out_start[..n]);
        for e in 0..block_edges {
            out_e[fill[e_from[e]]] = e;
            fill[e_from[e]] += 1;
        }

        let mut x = vec![0.0; n];
        let mut count = 0;
        for i in 0..n {
            if root[i] == i && indeg[i] == 0 {
                topo[count] = i;
                count += 1;
            }
        }
        let mut q = 0;
        while q < count {
            let v = topo[q];
            q += 1;
            for &e in &out_e[out_start[v]..out_start[v + 1]] {
                let w = e_to[e];
                if x[v] + e_w[e] > x[w] {
                    x[w] = x[v] + e_w[e];
                }
                indeg[w] -= 1;
                if indeg[w] == 0 {
                    topo[count] = w;
                    count += 1;
                }
            }
        }
        for &v in topo[..count].iter().rev() {
            let mut min = f64::INFINITY;
            for &e in &out_e[out_start[v]..out_start[v + 1]] {
                let cand = x[e_to[e]] - e_w[e];
                if cand < min {
                    min = cand;
                }
            }
            if min != f64::INFINITY && min > x[v] {
                x[v] = min;
            }
        }
        results.push(
            (0..n)
                .map(|i| (if right { -x[root[i]] } else { x[root[i]] }) + shift[i])
                .collect(),
        );
    }

    let mut narrowest = 0;
    let mut best_width = f64::INFINITY;
    let mut mins = [0.0; 4];
    let mut maxs = [0.0; 4];
    for (k, xs) in results.iter().enumerate() {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for i in 0..n {
            let half = g.w[i] / 2.0;
            if xs[i] - half < lo {
                lo = xs[i] - half;
            }
            if xs[i] + half > hi {
                hi = xs[i] + half;
            }
        }
        mins[k] = lo;
        maxs[k] = hi;
        if hi - lo < best_width {
            best_width = hi - lo;
            narrowest = k;
        }
    }
    for (k, xs) in results.iter_mut().enumerate() {
        let delta = if k & 1 == 0 {
            mins[narrowest] - mins[k]
        } else {
            maxs[narrowest] - maxs[k]
        };
        if delta != 0.0 {
            for x in xs.iter_mut() {
                *x += delta;
            }
        }
    }

    (0..n)
        .map(|i| {
            let mut a = results[0][i];
            let mut b = results[1][i];
            let mut c = results[2][i];
            let mut d = results[3][i];
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            if c > d {
                std::mem::swap(&mut c, &mut d);
            }
            if a > c {
                std::mem::swap(&mut a, &mut c);
            }
            if b > d {
                std::mem::swap(&mut b, &mut d);
            }
            if b > c {
                std::mem::swap(&mut b, &mut c);
            }
            f64::midpoint(b, c)
        })
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::many_single_char_names,
    clippy::unreadable_literal,
    reason = "fixtures print Pele's numbers exactly and name ends as Pele does"
)]
mod tests {
    //! End-to-end checks against Pele. Every expected number comes from running Pele's TypeScript
    //! (`obsidianmd/pele` at `458322a`) on the same input with `npx tsx`, never from this port.
    //! Large graphs compare checksums that both sides compute in the same order.

    use super::*;

    /// `w, h, kind, span, pin`, with kinds numbered as in Pele.
    type N = (f64, f64, u8, f64, f64);
    /// `tail, head, minlen, label_w, label_h, tail_dx, head_dx, tail_fixed, head_fixed`.
    type E = (usize, usize, u32, f64, f64, f64, f64, bool, bool);

    /// An edge as Pele lays it out.
    struct Want {
        head_dx: f64,
        label: (f64, f64),
        points: &'static [f64],
        reversed: bool,
        tail_dx: f64,
    }

    const OPT: LayeredOptions = LayeredOptions {
        apart: false,
        edge_sep: 16.0,
        head_room: 6.0,
        node_sep: 40.0,
        port_sep: 20.0,
        rank_sep: 48.0,
        straight: false,
        tracks: false,
    };

    fn close(actual: f64, expected: f64, what: &str) {
        assert!(
            (actual - expected).abs() <= 1e-6 * expected.abs().max(1.0),
            "{what}: {actual} != {expected}"
        );
    }

    fn run(
        nodes: &[N],
        edges: &[E],
        opt: &LayeredOptions,
    ) -> (Vec<LNode>, Vec<LEdge>, LayeredResult) {
        let kinds = [
            Kind::Node,
            Kind::Dummy,
            Kind::Label,
            Kind::StartPort,
            Kind::EndPort,
        ];
        let mut ln: Vec<LNode> = nodes
            .iter()
            .map(|&(w, h, kind, span, pin)| LNode {
                pin,
                span,
                ..lnode(w, h, kinds[usize::from(kind)])
            })
            .collect();
        let mut le: Vec<LEdge> = edges
            .iter()
            .map(|&(t, h, minlen, lw, lh, tdx, hdx, tf, hf)| LEdge {
                head_dx: hdx,
                head_fixed: hf,
                tail_dx: tdx,
                tail_fixed: tf,
                ..ledge(t, h, minlen, lw, lh)
            })
            .collect();
        let size = layered(&mut ln, &mut le, opt);
        (ln, le, size)
    }

    fn check(
        nodes: &[N],
        edges: &[E],
        opt: &LayeredOptions,
        size: (f64, f64),
        want_nodes: &[(usize, f64, f64)],
        want_edges: &[Want],
    ) {
        let (ln, le, got) = run(nodes, edges, opt);
        close(got.width, size.0, "width");
        close(got.height, size.1, "height");
        for (i, (node, want)) in ln.iter().zip(want_nodes).enumerate() {
            assert_eq!(node.rank, want.0, "node {i} rank");
            close(node.x, want.1, &format!("node {i} x"));
            close(node.y, want.2, &format!("node {i} y"));
        }
        for (i, (edge, want)) in le.iter().zip(want_edges).enumerate() {
            assert_eq!(edge.reversed, want.reversed, "edge {i} reversed");
            close(edge.tail_dx, want.tail_dx, &format!("edge {i} tail_dx"));
            close(edge.head_dx, want.head_dx, &format!("edge {i} head_dx"));
            assert_eq!(
                edge.points.len(),
                want.points.len(),
                "edge {i} points {:?}",
                edge.points
            );
            for (k, (&p, &q)) in edge.points.iter().zip(want.points).enumerate() {
                close(p, q, &format!("edge {i} point {k}"));
            }
            close(edge.label_x, want.label.0, &format!("edge {i} label x"));
            close(edge.label_y, want.label.1, &format!("edge {i} label y"));
        }
    }

    /// Pele's oracle graph generator: a Lehmer sequence picks sizes, ends, and labels.
    fn random(seed: u64, n: usize, m: usize, span: f64, labels: u64) -> (Vec<N>, Vec<E>) {
        let mut s = seed;
        let mut rnd = || {
            s = s * 48271 % 2_147_483_647;
            s
        };
        let size = |v: u64| f64::from(u32::try_from(v).unwrap());
        let nodes = (0..n)
            .map(|_| {
                (
                    20.0 + size(rnd() % 60),
                    20.0 + size(rnd() % 30),
                    0,
                    span,
                    0.0,
                )
            })
            .collect();
        let mut edges = Vec::new();
        let count = n as u64;
        while edges.len() < m {
            let t = usize::try_from(rnd() % count).unwrap();
            let h = usize::try_from(rnd() % count).unwrap();
            let l = rnd() % 100 < labels;
            if t == h {
                continue;
            }
            let (lw, lh) = if l { (30.0, 14.0) } else { (0.0, 0.0) };
            edges.push((t, h, 1, lw, lh, 0.0, 0.0, false, false));
        }
        (nodes, edges)
    }

    fn check_sum(
        nodes: &[N],
        edges: &[E],
        opt: &LayeredOptions,
        size: (f64, f64),
        sum: (f64, f64, usize),
    ) {
        let (ln, le, got) = run(nodes, edges, opt);
        close(got.width, size.0, "width");
        close(got.height, size.1, "height");
        let mut node_sum = 0.0;
        for v in &ln {
            node_sum += v.x + 2.0 * v.y;
        }
        let mut points = 0.0;
        let mut count = 0;
        for e in &le {
            for (i, p) in e.points.iter().enumerate() {
                points += p * f64::from(u8::try_from(i % 7).unwrap() + 1);
            }
            count += e.points.len();
            points += e.label_x + e.label_y;
        }
        close(node_sum, sum.0, "node checksum");
        close(points, sum.1, "point checksum");
        assert_eq!(count, sum.2, "point count");
    }

    fn square(count: usize, w: f64, h: f64, span: f64) -> Vec<N> {
        vec![(w, h, 0, span, 0.0); count]
    }

    #[test]
    fn empty() {
        check(&[], &[], &OPT, (0.0, 0.0), &[], &[]);
    }

    #[test]
    fn single() {
        check(
            &[(40.0, 30.0, 0, 0.0, 0.0)],
            &[],
            &OPT,
            (40.0, 30.0),
            &[(0, 20.0, 15.0)],
            &[],
        );
    }

    #[test]
    fn chain() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (40.0, 186.0),
            &[(0, 20.0, 15.0), (1, 20.0, 93.0), (2, 20.0, 171.0)],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn diamond() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (120.0, 30.0, 0, 0.0, 0.0),
                (30.0, 60.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (190.0, 216.0),
            &[
                (0, 117.5, 15.0),
                (1, 60.0, 108.0),
                (1, 175.0, 108.0),
                (2, 117.5, 201.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[117.5, 30.0, 60.0, 93.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[117.5, 30.0, 175.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[60.0, 123.0, 117.5, 186.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[175.0, 138.0, 117.5, 186.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn cycle() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 0, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (68.0, 264.0),
            &[
                (0, 44.0, 15.0),
                (1, 20.0, 93.0),
                (2, 44.0, 171.0),
                (3, 44.0, 249.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[44.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 108.0, 44.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[44.0, 156.0, 68.0, 108.0, 68.0, 78.0, 44.0, 30.0],
                    reversed: true,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[44.0, 186.0, 44.0, 234.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn twins() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (88.0, 186.0),
            &[(0, 20.0, 15.0), (4, 20.0, 171.0), (2, 68.0, 93.0)],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 4.0, 78.0, 4.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0, 20.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 68.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[68.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn twins_apart() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions { apart: true, ..OPT },
            (88.0, 186.0),
            &[(0, 20.0, 15.0), (4, 20.0, 171.0), (2, 68.0, 93.0)],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 4.0, 78.0, 4.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0, 20.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 68.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[68.0, 108.0, 20.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn labels() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 50.0, 14.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 20.0, 14.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (153.0, 214.0),
            &[
                (0, 105.0, 15.0),
                (2, 25.0, 107.0),
                (2, 105.0, 107.0),
                (4, 105.0, 199.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (25.0, 58.0),
                    points: &[105.0, 30.0, 25.0, 51.0, 25.0, 65.0, 25.0, 92.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[105.0, 30.0, 105.0, 92.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[25.0, 122.0, 25.0, 146.5, 25.0, 153.5, 105.0, 184.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[
                        105.0, 30.0, 153.0, 92.0, 153.0, 122.0, 153.0, 146.5, 153.0, 153.5, 105.0,
                        184.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (105.0, 150.0),
                    points: &[105.0, 122.0, 105.0, 143.0, 105.0, 157.0, 105.0, 184.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn long_edges() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 40.0, 14.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (148.0, 342.0),
            &[
                (0, 106.0, 15.0),
                (2, 24.0, 93.0),
                (4, 48.0, 171.0),
                (6, 82.0, 249.0),
                (8, 82.0, 327.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[106.0, 30.0, 24.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[24.0, 108.0, 48.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[48.0, 186.0, 82.0, 234.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[82.0, 264.0, 82.0, 312.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[
                        106.0, 30.0, 148.0, 78.0, 148.0, 108.0, 148.0, 156.0, 148.0, 186.0, 148.0,
                        234.0, 148.0, 264.0, 82.0, 312.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[
                        24.0, 108.0, 0.0, 156.0, 0.0, 186.0, 0.0, 234.0, 0.0, 264.0, 82.0, 312.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (106.0, 93.0),
                    points: &[
                        106.0, 30.0, 106.0, 78.0, 106.0, 108.0, 106.0, 156.0, 106.0, 186.0, 82.0,
                        234.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn wide_and_tall() {
        check(
            &[
                (300.0, 20.0, 0, 0.0, 0.0),
                (20.0, 200.0, 0, 0.0, 0.0),
                (80.0, 80.0, 0, 0.0, 0.0),
                (10.0, 10.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (300.0, 326.0),
            &[
                (0, 150.0, 10.0),
                (1, 60.0, 168.0),
                (1, 150.0, 168.0),
                (2, 150.0, 321.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[150.0, 20.0, 60.0, 68.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[150.0, 20.0, 150.0, 128.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[150.0, 20.0, 218.0, 68.0, 218.0, 268.0, 150.0, 316.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[60.0, 268.0, 150.0, 316.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn crossings() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (200.0, 108.0),
            &[
                (0, 20.0, 15.0),
                (0, 180.0, 15.0),
                (0, 100.0, 15.0),
                (1, 100.0, 93.0),
                (1, 180.0, 93.0),
                (1, 20.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn pins() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 1.0),
                (40.0, 30.0, 0, 0.0, -1.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (200.0, 108.0),
            &[
                (0, 100.0, 15.0),
                (1, 180.0, 93.0),
                (1, 20.0, 93.0),
                (1, 100.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn offsets() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, -10.0, 5.0, true, false),
                (0, 2, 1, 0.0, 0.0, 10.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, -8.0, false, true),
                (2, 3, 1, 0.0, 0.0, 0.0, 8.0, false, false),
            ],
            &OPT,
            (120.0, 186.0),
            &[
                (0, 62.5, 15.0),
                (1, 20.0, 93.0),
                (1, 100.0, 93.0),
                (2, 60.0, 171.0),
            ],
            &[
                Want {
                    head_dx: 5.0,
                    label: (0.0, 0.0),
                    points: &[52.5, 30.0, 25.0, 78.0],
                    reversed: false,
                    tail_dx: -10.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[72.5, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 10.0,
                },
                Want {
                    head_dx: -8.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 108.0, 52.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 8.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 108.0, 68.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn minlen() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 3, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 0, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (68.0, 312.0),
            &[
                (0, 24.0, 15.0),
                (3, 24.0, 219.0),
                (1, 48.0, 93.0),
                (4, 24.0, 297.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[24.0, 30.0, 0.0, 78.0, 0.0, 108.0, 24.0, 204.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[24.0, 30.0, 48.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[48.0, 108.0, 24.0, 204.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[24.0, 234.0, 24.0, 282.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn disconnected() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (200.0, 108.0),
            &[
                (0, 20.0, 15.0),
                (1, 20.0, 93.0),
                (0, 100.0, 15.0),
                (0, 180.0, 15.0),
                (1, 180.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn spans_fan() {
        check(
            &[
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (440.0, 186.0),
            &[
                (0, 220.0, 15.0),
                (1, 60.0, 93.0),
                (1, 220.0, 93.0),
                (1, 380.0, 93.0),
                (2, 220.0, 171.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[204.0, 30.0, 60.0, 78.0],
                    reversed: false,
                    tail_dx: -16.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[220.0, 30.0, 220.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[236.0, 30.0, 380.0, 78.0],
                    reversed: false,
                    tail_dx: 16.0,
                },
                Want {
                    head_dx: -16.0,
                    label: (0.0, 0.0),
                    points: &[60.0, 108.0, 204.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[220.0, 108.0, 220.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 16.0,
                    label: (0.0, 0.0),
                    points: &[380.0, 108.0, 236.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn spans_labels() {
        check(
            &[
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
            ],
            &[
                (0, 1, 1, 40.0, 14.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 30.0, 14.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (5, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (4, 0, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (424.0, 292.0),
            &[
                (0, 266.4, 15.0),
                (2, 50.0, 107.0),
                (2, 190.0, 107.0),
                (4, 190.0, 199.0),
                (6, 346.0, 277.0),
                (2, 346.0, 107.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (50.0, 58.0),
                    points: &[235.99999999999997, 30.0, 50.0, 51.0, 50.0, 65.0, 50.0, 92.0],
                    reversed: false,
                    tail_dx: -30.4,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[251.99999999999997, 30.0, 190.0, 92.0],
                    reversed: false,
                    tail_dx: -14.399999999999999,
                },
                Want {
                    head_dx: -16.0,
                    label: (0.0, 0.0),
                    points: &[50.0, 122.0, 50.0, 146.5, 50.0, 153.5, 174.0, 184.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (190.0, 150.0),
                    points: &[190.0, 122.0, 190.0, 143.0, 190.0, 157.0, 190.0, 184.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 16.0,
                    label: (0.0, 0.0),
                    points: &[
                        268.0, 30.0, 268.0, 92.0, 268.0, 122.0, 268.0, 146.5, 268.0, 153.5, 206.0,
                        184.0,
                    ],
                    reversed: false,
                    tail_dx: 1.6000000000000014,
                },
                Want {
                    head_dx: -16.0,
                    label: (0.0, 0.0),
                    points: &[190.0, 214.0, 330.0, 262.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[284.0, 30.0, 346.0, 92.0],
                    reversed: false,
                    tail_dx: 17.6,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[346.0, 122.0, 346.0, 184.0, 346.0, 214.0, 346.0, 262.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 33.6,
                    label: (0.0, 0.0),
                    points: &[
                        362.0, 262.0, 424.0, 214.0, 424.0, 184.0, 424.0, 122.0, 424.0, 92.0, 300.0,
                        30.0,
                    ],
                    reversed: true,
                    tail_dx: 16.0,
                },
            ],
        );
    }

    #[test]
    fn spans_twins() {
        check(
            &[
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (268.0, 206.0),
            &[
                (0, 179.0, 15.0),
                (2, 50.0, 113.0),
                (2, 190.0, 113.0),
                (4, 190.0, 191.0),
            ],
            &[
                Want {
                    head_dx: -8.0,
                    label: (0.0, 0.0),
                    points: &[158.0, 30.0, 158.0, 58.0, 42.0, 58.0, 42.0, 98.0],
                    reversed: false,
                    tail_dx: -21.0,
                },
                Want {
                    head_dx: 8.0,
                    label: (0.0, 0.0),
                    points: &[174.0, 30.0, 174.0, 70.0, 58.0, 70.0, 58.0, 98.0],
                    reversed: false,
                    tail_dx: -5.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[190.0, 30.0, 190.0, 98.0],
                    reversed: false,
                    tail_dx: 11.0,
                },
                Want {
                    head_dx: -16.0,
                    label: (0.0, 0.0),
                    points: &[50.0, 128.0, 174.0, 176.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[190.0, 128.0, 190.0, 176.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 16.0,
                    label: (0.0, 0.0),
                    points: &[206.0, 30.0, 268.0, 98.0, 268.0, 128.0, 206.0, 176.0],
                    reversed: false,
                    tail_dx: 27.0,
                },
            ],
        );
    }

    #[test]
    fn spans_narrow() {
        check(
            &[
                (30.0, 30.0, 0, 20.0, 0.0),
                (30.0, 30.0, 0, 20.0, 0.0),
                (30.0, 30.0, 0, 20.0, 0.0),
                (30.0, 30.0, 0, 20.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 0, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (144.0, 186.0),
            &[
                (0, 109.0, 15.0),
                (2, 15.0, 93.0),
                (2, 85.0, 93.0),
                (4, 114.5, 171.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[99.0, 30.0, 15.0, 78.0],
                    reversed: false,
                    tail_dx: -10.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[104.0, 30.0, 85.0, 78.0],
                    reversed: false,
                    tail_dx: -5.0,
                },
                Want {
                    head_dx: -6.0,
                    label: (0.0, 0.0),
                    points: &[109.0, 30.0, 128.0, 78.0, 128.0, 108.0, 108.5, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: -6.0,
                    label: (0.0, 0.0),
                    points: &[15.0, 108.0, 108.5, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: -6.0,
                    label: (0.0, 0.0),
                    points: &[85.0, 108.0, 108.5, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 10.0,
                    label: (0.0, 0.0),
                    points: &[124.5, 156.0, 144.0, 108.0, 144.0, 78.0, 119.0, 30.0],
                    reversed: true,
                    tail_dx: 10.0,
                },
            ],
        );
    }

    #[test]
    fn spans_fixed() {
        check(
            &[
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
                (120.0, 30.0, 0, 100.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, -30.0, 0.0, true, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 30.0, 0.0, true, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, true),
            ],
            &OPT,
            (324.0, 186.0),
            &[
                (0, 261.0, 15.0),
                (2, 60.0, 93.0),
                (2, 220.0, 93.0),
                (4, 264.0, 171.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[231.0, 30.0, 60.0, 78.0],
                    reversed: false,
                    tail_dx: -30.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[247.0, 30.0, 220.0, 78.0],
                    reversed: false,
                    tail_dx: -14.0,
                },
                Want {
                    head_dx: 35.0,
                    label: (0.0, 0.0),
                    points: &[291.0, 30.0, 324.0, 78.0, 324.0, 108.0, 299.0, 156.0],
                    reversed: false,
                    tail_dx: 30.0,
                },
                Want {
                    head_dx: 19.0,
                    label: (0.0, 0.0),
                    points: &[275.0, 30.0, 308.0, 78.0, 308.0, 108.0, 283.0, 156.0],
                    reversed: false,
                    tail_dx: 14.0,
                },
                Want {
                    head_dx: -16.0,
                    label: (0.0, 0.0),
                    points: &[60.0, 108.0, 248.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[220.0, 108.0, 264.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn levels_wide() {
        check(
            &[
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (400.0, 30.0, 0, 380.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
            ],
            &[
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (655.0, 140.0),
            &[
                (0, 35.0, 15.0),
                (0, 115.0, 15.0),
                (0, 635.0, 15.0),
                (0, 375.0, 15.0),
                (1, 111.0, 125.0),
                (1, 20.0, 125.0),
                (1, 378.0, 125.0),
            ],
            &[
                Want {
                    head_dx: -15.0,
                    label: (0.0, 0.0),
                    points: &[35.0, 30.0, 35.0, 70.0, 96.0, 110.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 110.0],
                    reversed: false,
                    tail_dx: -15.0,
                },
                Want {
                    head_dx: -15.0,
                    label: (0.0, 0.0),
                    points: &[50.0, 30.0, 50.0, 70.0, 363.0, 70.0, 363.0, 110.0],
                    reversed: false,
                    tail_dx: 15.0,
                },
                Want {
                    head_dx: -5.0,
                    label: (0.0, 0.0),
                    points: &[121.0, 30.0, 121.0, 58.0, 373.0, 58.0, 373.0, 110.0],
                    reversed: false,
                    tail_dx: 6.0,
                },
                Want {
                    head_dx: 15.0,
                    label: (0.0, 0.0),
                    points: &[635.0, 30.0, 393.0, 110.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 15.0,
                    label: (0.0, 0.0),
                    points: &[225.0, 30.0, 225.0, 82.0, 126.0, 82.0, 126.0, 110.0],
                    reversed: false,
                    tail_dx: -150.0,
                },
                Want {
                    head_dx: 5.0,
                    label: (0.0, 0.0),
                    points: &[383.0, 30.0, 383.0, 110.0],
                    reversed: false,
                    tail_dx: 8.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[105.0, 30.0, 111.0, 110.0],
                    reversed: false,
                    tail_dx: -10.0,
                },
            ],
        );
    }

    #[test]
    fn levels_crossed() {
        check(
            &[
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
                (40.0, 30.0, 0, 30.0, 0.0),
            ],
            &[
                (0, 7, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 7, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (280.0, 108.0),
            &[
                (0, 100.0, 15.0),
                (0, 20.0, 15.0),
                (0, 260.0, 15.0),
                (0, 180.0, 15.0),
                (1, 180.0, 93.0),
                (1, 260.0, 93.0),
                (1, 20.0, 93.0),
                (1, 100.0, 93.0),
            ],
            &[
                Want {
                    head_dx: -8.0,
                    label: (0.0, 0.0),
                    points: &[92.0, 30.0, 92.0, 78.0],
                    reversed: false,
                    tail_dx: -8.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[260.0, 30.0, 260.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 8.0,
                    label: (0.0, 0.0),
                    points: &[188.0, 30.0, 188.0, 78.0],
                    reversed: false,
                    tail_dx: 8.0,
                },
                Want {
                    head_dx: -8.0,
                    label: (0.0, 0.0),
                    points: &[108.0, 30.0, 172.0, 78.0],
                    reversed: false,
                    tail_dx: 8.0,
                },
                Want {
                    head_dx: 8.0,
                    label: (0.0, 0.0),
                    points: &[172.0, 30.0, 108.0, 78.0],
                    reversed: false,
                    tail_dx: -8.0,
                },
            ],
        );
    }

    #[test]
    fn tracks() {
        check(
            &[
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
            ],
            &[
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 5, 1, 30.0, 14.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions {
                tracks: true,
                ..OPT
            },
            (317.0, 222.0),
            &[
                (0, 229.0, 15.0),
                (0, 84.0, 15.0),
                (2, 130.0, 93.0),
                (2, 30.0, 93.0),
                (2, 287.0, 93.0),
                (4, 159.0, 207.0),
            ],
            &[
                Want {
                    head_dx: 8.0,
                    label: (0.0, 0.0),
                    points: &[213.0, 30.0, 213.0, 60.0, 38.0, 60.0, 38.0, 78.0],
                    reversed: false,
                    tail_dx: -16.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[245.0, 30.0, 245.0, 48.0, 287.0, 48.0, 287.0, 78.0],
                    reversed: false,
                    tail_dx: 16.0,
                },
                Want {
                    head_dx: 24.0,
                    label: (0.0, 0.0),
                    points: &[
                        229.0, 30.0, 229.0, 78.0, 229.0, 108.0, 229.0, 132.0, 229.0, 146.0, 229.0,
                        174.0, 183.0, 174.0, 183.0, 192.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: -8.0,
                    label: (0.0, 0.0),
                    points: &[76.0, 30.0, 76.0, 48.0, 22.0, 48.0, 22.0, 78.0],
                    reversed: false,
                    tail_dx: -8.0,
                },
                Want {
                    head_dx: -8.0,
                    label: (0.0, 0.0),
                    points: &[
                        130.0, 108.0, 130.0, 132.0, 130.0, 146.0, 130.0, 164.0, 151.0, 164.0,
                        151.0, 192.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 8.0,
                    label: (0.0, 0.0),
                    points: &[
                        92.0, 30.0, 92.0, 48.0, 188.0, 48.0, 188.0, 78.0, 188.0, 108.0, 188.0,
                        132.0, 188.0, 146.0, 188.0, 164.0, 167.0, 164.0, 167.0, 192.0,
                    ],
                    reversed: false,
                    tail_dx: 8.0,
                },
                Want {
                    head_dx: -24.0,
                    label: (30.0, 139.0),
                    points: &[
                        30.0, 108.0, 30.0, 132.0, 30.0, 146.0, 30.0, 174.0, 135.0, 174.0, 135.0,
                        192.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn tracks_plain() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions {
                tracks: true,
                ..OPT
            },
            (200.0, 108.0),
            &[
                (0, 100.0, 15.0),
                (0, 180.0, 15.0),
                (1, 100.0, 93.0),
                (1, 20.0, 93.0),
                (1, 180.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 48.0, 20.0, 48.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 180.0, 60.0, 100.0, 60.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 48.0, 180.0, 48.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn straight() {
        check(
            &[
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
                (100.0, 30.0, 0, 80.0, 0.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 30.0, 14.0, 0.0, 0.0, false, false),
                (4, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions {
                straight: true,
                ..OPT
            },
            (380.0, 200.0),
            &[
                (0, 190.0, 15.0),
                (2, 50.0, 107.0),
                (2, 190.0, 107.0),
                (4, 190.0, 185.0),
                (2, 330.0, 107.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[174.0, 30.0, 50.0, 92.0],
                    reversed: false,
                    tail_dx: -16.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[190.0, 30.0, 190.0, 92.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: -16.0,
                    label: (0.0, 0.0),
                    points: &[50.0, 122.0, 174.0, 170.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[190.0, 122.0, 190.0, 170.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (330.0, 58.0),
                    points: &[206.0, 30.0, 330.0, 51.0, 330.0, 65.0, 330.0, 92.0],
                    reversed: false,
                    tail_dx: 16.0,
                },
                Want {
                    head_dx: 16.0,
                    label: (0.0, 0.0),
                    points: &[330.0, 122.0, 206.0, 170.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn head_room() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 1, 1, 30.0, 14.0, 0.0, 0.0, false, false),
                (0, 2, 1, 30.0, 14.0, 0.0, 0.0, false, false),
                (1, 3, 1, 30.0, 14.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 30.0, 14.0, 0.0, 0.0, false, false),
                (4, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions {
                head_room: 10.0,
                ..OPT
            },
            (118.5, 384.0),
            &[
                (0, 55.5, 15.0),
                (2, 24.0, 107.0),
                (8, 48.0, 369.0),
                (4, 24.0, 199.0),
                (6, 48.0, 291.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (24.0, 56.0),
                    points: &[55.5, 30.0, 24.0, 49.0, 24.0, 63.0, 24.0, 92.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (103.5, 199.0),
                    points: &[
                        55.5, 30.0, 103.5, 92.0, 103.5, 122.0, 103.5, 184.0, 103.5, 214.0, 103.5,
                        276.0, 103.5, 306.0, 48.0, 354.0,
                    ],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (24.0, 148.0),
                    points: &[24.0, 122.0, 24.0, 141.0, 24.0, 155.0, 24.0, 184.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[
                        48.0, 354.0, 0.0, 306.0, 0.0, 276.0, 0.0, 243.5, 0.0, 236.5, 24.0, 214.0,
                    ],
                    reversed: true,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (48.0, 240.0),
                    points: &[24.0, 214.0, 48.0, 233.0, 48.0, 247.0, 48.0, 276.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[48.0, 306.0, 48.0, 354.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn ports() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (0.0, 0.0, 3, 0.0, 0.0),
                (0.0, 0.0, 4, 0.0, 0.0),
                (0.0, 0.0, 3, 0.0, 0.0),
            ],
            &[
                (2, 0, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (4, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions {
                port_sep: 10.0,
                ..OPT
            },
            (68.0, 128.0),
            &[
                (2, 20.0, 25.0),
                (4, 44.0, 103.0),
                (0, 20.0, 0.0),
                (6, 44.0, 128.0),
                (0, 68.0, 0.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 0.0, 12.0, 5.0, 20.0, 10.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 40.0, 44.0, 88.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[44.0, 118.0, 44.0, 128.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[68.0, 0.0, 68.0, 10.0, 68.0, 40.0, 44.0, 88.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 10.0, 28.0, 5.0, 20.0, 0.0],
                    reversed: true,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn mirror_a() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (200.0, 108.0),
            &[
                (0, 20.0, 15.0),
                (0, 100.0, 15.0),
                (0, 180.0, 15.0),
                (1, 180.0, 93.0),
                (1, 100.0, 93.0),
                (1, 20.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn mirror_b() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (280.0, 108.0),
            &[
                (0, 100.0, 15.0),
                (0, 20.0, 15.0),
                (0, 260.0, 15.0),
                (0, 180.0, 15.0),
                (1, 220.0, 93.0),
                (1, 60.0, 93.0),
                (1, 140.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 140.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 60.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[260.0, 30.0, 220.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 220.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 60.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 140.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn mirror_c() {
        check(
            &[
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
                (40.0, 30.0, 0, 0.0, 0.0),
            ],
            &[
                (0, 7, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (3, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 6, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (7, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (280.0, 186.0),
            &[
                (0, 20.0, 15.0),
                (0, 100.0, 15.0),
                (0, 180.0, 15.0),
                (1, 260.0, 93.0),
                (2, 140.0, 171.0),
                (1, 180.0, 93.0),
                (1, 100.0, 93.0),
                (1, 20.0, 93.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 20.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[180.0, 30.0, 180.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[260.0, 108.0, 140.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 30.0, 100.0, 78.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[20.0, 108.0, 140.0, 156.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn crossed_wide() {
        check(
            &[
                (200.0, 30.0, 0, 180.0, 0.0),
                (200.0, 30.0, 0, 180.0, 0.0),
                (200.0, 30.0, 0, 180.0, 0.0),
                (200.0, 30.0, 0, 180.0, 0.0),
            ],
            &[
                (0, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 2, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (440.0, 130.0),
            &[
                (0, 100.0, 15.0),
                (0, 340.0, 15.0),
                (1, 100.0, 115.0),
                (1, 340.0, 115.0),
            ],
            &[
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[100.0, 30.0, 100.0, 100.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: -50.0,
                    label: (0.0, 0.0),
                    points: &[150.0, 30.0, 290.0, 100.0],
                    reversed: false,
                    tail_dx: 50.0,
                },
                Want {
                    head_dx: 50.0,
                    label: (0.0, 0.0),
                    points: &[290.0, 30.0, 150.0, 100.0],
                    reversed: false,
                    tail_dx: -50.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[340.0, 30.0, 340.0, 100.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
            ],
        );
    }

    #[test]
    fn crossed_wide_three() {
        check(
            &[
                (160.0, 30.0, 0, 140.0, 0.0),
                (160.0, 30.0, 0, 140.0, 0.0),
                (160.0, 30.0, 0, 140.0, 0.0),
                (160.0, 30.0, 0, 140.0, 0.0),
                (160.0, 30.0, 0, 140.0, 0.0),
                (160.0, 30.0, 0, 140.0, 0.0),
            ],
            &[
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &OPT,
            (744.0, 176.0),
            &[
                (0, 80.0, 15.0),
                (0, 372.0, 15.0),
                (0, 664.0, 15.0),
                (1, 372.0, 161.0),
                (1, 664.0, 161.0),
                (1, 80.0, 161.0),
            ],
            &[
                Want {
                    head_dx: -30.0,
                    label: (0.0, 0.0),
                    points: &[110.0, 30.0, 342.0, 146.0],
                    reversed: false,
                    tail_dx: 30.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[80.0, 30.0, 80.0, 146.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: -30.0,
                    label: (0.0, 0.0),
                    points: &[402.0, 30.0, 634.0, 146.0],
                    reversed: false,
                    tail_dx: 30.0,
                },
                Want {
                    head_dx: 30.0,
                    label: (0.0, 0.0),
                    points: &[634.0, 30.0, 402.0, 146.0],
                    reversed: false,
                    tail_dx: -30.0,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[664.0, 30.0, 664.0, 146.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 30.0,
                    label: (0.0, 0.0),
                    points: &[342.0, 30.0, 110.0, 146.0],
                    reversed: false,
                    tail_dx: -30.0,
                },
            ],
        );
    }

    #[test]
    fn tracks_roomy() {
        check(
            &[
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
                (60.0, 30.0, 0, 50.0, 0.0),
            ],
            &[
                (0, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 4, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (0, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 3, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (2, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
                (1, 5, 1, 0.0, 0.0, 0.0, 0.0, false, false),
            ],
            &LayeredOptions {
                rank_sep: 120.0,
                tracks: true,
                ..OPT
            },
            (286.6666666666667, 180.0),
            &[
                (0, 51.33333333333333, 15.0),
                (0, 151.33333333333331, 15.0),
                (0, 256.6666666666667, 15.0),
                (1, 135.33333333333331, 165.0),
                (1, 29.999999999999996, 165.0),
                (1, 235.33333333333331, 165.0),
            ],
            &[
                Want {
                    head_dx: -5.33203125,
                    label: (0.0, 0.0),
                    points: &[
                        46.0,
                        30.0,
                        46.0,
                        104.0,
                        130.00130208333331,
                        104.0,
                        130.00130208333331,
                        150.0,
                    ],
                    reversed: false,
                    tail_dx: -5.333333333333332,
                },
                Want {
                    head_dx: 0.0,
                    label: (0.0, 0.0),
                    points: &[29.999999999999996, 30.0, 29.999999999999996, 150.0],
                    reversed: false,
                    tail_dx: -21.333333333333332,
                },
                Want {
                    head_dx: -10.666666666666666,
                    label: (0.0, 0.0),
                    points: &[
                        62.0,
                        30.0,
                        62.0,
                        90.0,
                        224.66666666666669,
                        90.0,
                        224.66666666666666,
                        150.0,
                    ],
                    reversed: false,
                    tail_dx: 10.666666666666668,
                },
                Want {
                    head_dx: 10.66796875,
                    label: (0.0, 0.0),
                    points: &[
                        140.66927083333331,
                        30.0,
                        140.66927083333331,
                        76.0,
                        146.00130208333331,
                        76.0,
                        146.00130208333331,
                        150.0,
                    ],
                    reversed: false,
                    tail_dx: -10.6640625,
                },
                Want {
                    head_dx: 21.333333333333336,
                    label: (0.0, 0.0),
                    points: &[256.6666666666667, 30.0, 256.66666666666663, 150.0],
                    reversed: false,
                    tail_dx: 0.0,
                },
                Want {
                    head_dx: 5.333333333333334,
                    label: (0.0, 0.0),
                    points: &[
                        156.66927083333331,
                        30.0,
                        156.66927083333331,
                        76.0,
                        240.66666666666669,
                        76.0,
                        240.66666666666666,
                        150.0,
                    ],
                    reversed: false,
                    tail_dx: 5.3359375,
                },
            ],
        );
    }

    #[test]
    fn ports_only() {
        check(
            &[(0.0, 0.0, 3, 0.0, 0.0), (0.0, 0.0, 4, 0.0, 0.0)],
            &[(0, 1, 1, 0.0, 0.0, 0.0, 0.0, false, false)],
            &OPT,
            (0.0, 40.0),
            &[(0, 0.0, 0.0), (2, 0.0, 40.0)],
            &[Want {
                head_dx: 0.0,
                label: (0.0, 0.0),
                points: &[0.0, 0.0, 0.0, 40.0],
                reversed: false,
                tail_dx: 0.0,
            }],
        );
    }

    #[test]
    fn random_spans() {
        let (nodes, edges) = random(7, 120, 200, 60.0, 10);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (4387.5, 3737.0),
            (704389.25, 32464139.51282051, 4416),
        );
    }

    #[test]
    fn random_plain() {
        let (nodes, edges) = random(11, 80, 160, 0.0, 15);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (3245.0, 5389.609375),
            (532974.3125, 41447693.234375, 5060),
        );
    }

    #[test]
    fn random_large() {
        let (nodes, edges) = random(13, 650, 700, 0.0, 0);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (23533.25, 6135.0),
            (9319369.75, 256514427.5, 10320),
        );
    }

    #[test]
    fn random_tracks() {
        let (nodes, edges) = random(17, 60, 110, 50.0, 10);
        check_sum(
            &nodes,
            &edges,
            &LayeredOptions {
                tracks: true,
                ..OPT
            },
            (2387.5, 2372.0),
            (218697.1660839161, 15679987.962826645, 3460),
        );
    }

    #[test]
    fn small_curved_23() {
        let (nodes, edges) = random(23, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (907.5, 922.0),
            (33684.0, 648061.5833333334, 384),
        );
    }

    #[test]
    fn small_curved_29() {
        let (nodes, edges) = random(29, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (894.75, 803.0),
            (29479.5, 548352.6666666667, 356),
        );
    }

    #[test]
    fn small_curved_31() {
        let (nodes, edges) = random(31, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (755.6666666666666, 1229.3333333333333),
            (39182.883333333324, 807365.3166666664, 434),
        );
    }

    #[test]
    fn small_curved_37() {
        let (nodes, edges) = random(37, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (626.8333333333333, 1348.2916666666665),
            (38013.03140096618, 880943.7737922706, 480),
        );
    }

    #[test]
    fn small_curved_41() {
        let (nodes, edges) = random(41, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (592.9166666666666, 1215.0),
            (36689.22222222222, 669711.9800187114, 424),
        );
    }

    #[test]
    fn small_curved_43() {
        let (nodes, edges) = random(43, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (765.5, 1237.0),
            (31899.666666666668, 1013979.3128156568, 550),
        );
    }

    #[test]
    fn small_curved_47() {
        let (nodes, edges) = random(47, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (857.25, 980.0),
            (32779.66666666667, 801337.2664473683, 470),
        );
    }

    #[test]
    fn small_curved_53() {
        let (nodes, edges) = random(53, 24, 40, 100.0, 20);
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (853.0833333333333, 1254.4305555555557),
            (38944.32222222223, 1037218.5499999996, 538),
        );
    }

    #[test]
    fn random_straight() {
        let (nodes, edges) = random(19, 60, 110, 50.0, 10);
        check_sum(
            &nodes,
            &edges,
            &LayeredOptions {
                straight: true,
                ..OPT
            },
            (2421.5, 3159.838541666667),
            (253828.10763888896, 10360698.010416664, 2148),
        );
    }

    #[test]
    fn max_dummies() {
        let nodes = square(60, 40.0, 30.0, 0.0);
        let mut edges: Vec<E> = (0..59)
            .map(|i| (i, i + 1, 1, 0.0, 0.0, 0.0, 0.0, false, false))
            .collect();
        edges.extend((0..200).map(|k| (k % 5, 55 + k % 5, 1, 0.0, 0.0, 0.0, 0.0, false, false)));
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (2976.0, 4954.0),
            (435258.0, 318716981.0, 40348),
        );
    }

    #[test]
    fn weighed_tracks() {
        let nodes = square(42, 40.0, 30.0, 0.0);
        let edges: Vec<E> = (0..21)
            .flat_map(|a| (21..42).map(move |b| (a, b, 1, 0.0, 0.0, 0.0, 0.0, false, false)))
            .collect();
        check_sum(
            &nodes,
            &edges,
            &LayeredOptions {
                tracks: true,
                ..OPT
            },
            (1640.0, 466.0),
            (54012.0, 6572484.0, 3444),
        );
    }

    #[test]
    fn weighed_levels() {
        let nodes = square(42, 40.0, 30.0, 30.0);
        let edges: Vec<E> = (0..21)
            .flat_map(|a| (21..42).map(move |b| (a, b, 1, 0.0, 0.0, 0.0, 0.0, false, false)))
            .collect();
        check_sum(
            &nodes,
            &edges,
            &OPT,
            (1640.0, 108.0),
            (38976.0, 1610532.0, 1764),
        );
    }
}
