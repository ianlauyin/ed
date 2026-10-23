//! Layout for swimlanes. Port of Pele's `src/layout/lanes.ts`.
//!
//! Every top-level group is a lane: a band that runs the length of the diagram along the flow.
//! Ranks are shared by all lanes, so a step sits level with the steps that happen at the same
//! time in other lanes, and each node stays inside its own lane. Arrays are sized from the
//! validated graph, so their indices are in bounds by construction, as in the layered layout.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::layout::compound::{CEdge, CNode, CompoundResult, Dir};
use crate::layout::layered::{Kind, LEdge, LNode, LayeredOptions, ledge, lnode, orient, rank};
use crate::svg::builder::js_round;

const LANE_HEAD: f64 = 34.0;
/// The room a lane keeps around its items by default.
pub(crate) const LANE_PAD: f64 = 24.0;
const SWEEPS: usize = 8;
const MAX_DUMMIES: i64 = 20000;
const MAX_HOPS: usize = 10000;
const ROUTE_BUDGET: f64 = 2_000_000.0;
/// Parallel lines closer than this are drawn as one.
const NEAR: f64 = 4.0;
const ROUNDS: usize = 3;
const RELAY: f64 = 8.0;
const TURN: f64 = 20.0;
/// The room a lane keeps beside a handoff along its margin, and how much of it is between the
/// handoff and the lane's items.
const MARGIN: f64 = 20.0;
const LEG: f64 = 14.0;
const CLOSE: f64 = 3.0;

/// `Math.sign`: zero stays zero.
fn sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        value
    }
}

/// What every placement shares: the graph's ranked items and its lanes.
struct Ctx<'a> {
    /// The ranked relations between items.
    edge_of: Vec<usize>,
    end_pad: f64,
    flipped: bool,
    /// The item each node is, when it is ranked and placed.
    item_of: Vec<Option<usize>>,
    item_lane: Vec<usize>,
    lane_count: usize,
    lane_pads: Vec<f64>,
    /// Each lane's slot, for a top-level group.
    lane_slot: Vec<Option<usize>>,
    /// Lanes in the order they were written.
    lanes: Vec<usize>,
    ledges: Vec<LEdge>,
    lnodes: Vec<LNode>,
    opt: &'a LayeredOptions,
    /// Nodes and empty groups, which get ranked and placed.
    real: Vec<usize>,
    sideways: bool,
    step: usize,
}

/// One placement's size, and what its routing found.
struct Placed {
    height: f64,
    /// The lanes whose margin a handoff runs along.
    margins: Vec<bool>,
    /// The relations given a simple route.
    routed: Vec<bool>,
    width: f64,
    work: f64,
}

/// The enclosing group of node `i`, when it names one.
fn parent(nodes: &[CNode], i: usize) -> Option<usize> {
    usize::try_from(nodes[i].parent)
        .ok()
        .filter(|&p| p < nodes.len())
}

/// Lays out `nodes` and `edges` with every top-level group as a lane, flowing in `dir`. Sets the
/// position and size of every node and group, and every edge's route and label position, and
/// returns the size of the drawing. `lane_pad` is the room a lane keeps around its items
/// ([`LANE_PAD`] in Pele). An edge whose end names no node makes the layout empty, where Pele
/// throws.
pub(crate) fn lane_layout(
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    dir: Dir,
    opt: &LayeredOptions,
    lane_pad: f64,
) -> CompoundResult {
    let n = nodes.len();
    if n == 0 || edges.iter().any(|e| e.src >= n || e.dst >= n) {
        return CompoundResult::default();
    }
    let sideways = matches!(dir, Dir::Lr | Dir::Rl);
    let flipped = matches!(dir, Dir::Bt | Dir::Rl);

    // The graph lists groups last-declared first.
    let lanes: Vec<usize> = (0..n)
        .rev()
        .filter(|&i| nodes[i].parent < 0 && nodes[i].is_group)
        .collect();
    let lane_count = lanes.len().max(1);
    let mut lane_slot = vec![None; n];
    for (k, &id) in lanes.iter().enumerate() {
        lane_slot[id] = Some(k);
    }
    let mut has_child = vec![false; n];
    for i in 0..n {
        if let Some(p) = parent(nodes, i) {
            has_child[p] = true;
        }
    }

    // Room a lane keeps around its items, so that the groups drawn around them stay inside it.
    let mut lane_pads = vec![lane_pad; lane_count];
    let mut end_pad = lane_pad;
    let mut lane_of = |i: usize| {
        let mut t = i;
        let mut room = lane_pad;
        let mut hops = 0;
        while let Some(p) = parent(nodes, t)
            && hops < MAX_HOPS
        {
            t = p;
            if lane_slot[t].is_none() {
                room += nodes[t]
                    .pad_x
                    .max(nodes[t].pad_top)
                    .max(nodes[t].pad_bottom);
            }
            hops += 1;
        }
        let lane = lane_slot[t].unwrap_or(0);
        if room > lane_pads[lane] {
            lane_pads[lane] = room;
        }
        if room > end_pad {
            end_pad = room;
        }
        lane
    };

    // What gets ranked and placed: nodes, and groups that are empty and so act as nodes.
    let mut item_of = vec![None; n];
    let mut real = Vec::new();
    for i in 0..n {
        if lane_slot[i].is_some() {
            continue;
        }
        if !nodes[i].is_group || !has_child[i] {
            item_of[i] = Some(real.len());
            real.push(i);
        }
    }
    let along = |c: &CNode| if sideways { c.w } else { c.h };
    let cross = |c: &CNode| if sideways { c.h } else { c.w };

    let mut lnodes: Vec<LNode> = real
        .iter()
        .map(|&i| lnode(cross(&nodes[i]), along(&nodes[i]), Kind::Node))
        .collect();
    let mut ledges = Vec::new();
    let mut edge_of = Vec::new();
    let mut step = 1;
    let mut seen = BTreeSet::new();
    for (ei, e) in edges.iter().enumerate() {
        let (Some(a), Some(b)) = (item_of[e.src], item_of[e.dst]) else {
            continue;
        };
        if a == b {
            continue;
        }
        let key = a.min(b) * real.len() + a.max(b);
        if e.label_w > 0.0 || seen.contains(&key) {
            step = 2;
        }
        seen.insert(key);
        let (label_w, label_h) = if sideways {
            (e.label_h, e.label_w)
        } else {
            (e.label_w, e.label_h)
        };
        ledges.push(ledge(a, b, e.minlen, label_w, label_h));
        edge_of.push(ei);
    }
    orient(&lnodes, &mut ledges);
    rank(&mut lnodes, &ledges, step);

    let m = ledges.len();
    let item_lane: Vec<usize> = real.iter().map(|&i| lane_of(i)).collect();
    let ctx = Ctx {
        edge_of,
        end_pad,
        flipped,
        item_of,
        item_lane,
        lane_count,
        lane_pads,
        lane_slot,
        lanes,
        ledges,
        lnodes,
        opt,
        real,
        sideways,
        step,
    };

    let mut plain = vec![false; m];
    let mut margins = vec![false; lane_count];
    let mut placed = place(&ctx, nodes, edges, &plain, &margins);
    // A handoff that was given a simple route needs no room kept for it through the ranks it
    // passes, so the lanes are laid out again without it. A handoff that then loses its route
    // goes back to the ranked one. Routing is most of the work, so a large graph is laid out once.
    let mut round = 0;
    while round < ROUNDS && placed.work <= ROUTE_BUDGET / RELAY {
        let next: Vec<bool> = if round == 0 {
            placed.routed.clone()
        } else {
            plain
                .iter()
                .zip(&placed.routed)
                .map(|(&on, &routed)| on && routed)
                .collect()
        };
        if next == plain && placed.margins == margins {
            break;
        }
        plain = next;
        margins = std::mem::take(&mut placed.margins);
        placed = place(&ctx, nodes, edges, &plain, &margins);
        round += 1;
    }
    if plain
        .iter()
        .zip(&placed.routed)
        .any(|(&on, &routed)| on && !routed)
    {
        placed = place(
            &ctx,
            nodes,
            edges,
            &vec![false; m],
            &vec![false; lane_count],
        );
    }
    CompoundResult {
        height: placed.height,
        width: placed.width,
    }
}

/// Lays the ranked items out and routes the relations. A `plain` relation is a handoff that takes
/// a simple route of its own, and so keeps no room through the ranks it passes. `margins` are the
/// lanes that keep room along their margin for a handoff.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "ranks, lanes, and counts are small whole numbers, as in Pele's typed arrays"
)]
#[expect(clippy::float_cmp, reason = "Pele compares route coordinates exactly")]
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's lanes.ts for side-by-side review"
)]
fn place(
    ctx: &Ctx<'_>,
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    plain: &[bool],
    margins: &[bool],
) -> Placed {
    let Ctx {
        edge_of,
        end_pad,
        flipped,
        item_of,
        item_lane,
        lane_count,
        lane_pads,
        lane_slot,
        lanes,
        ledges,
        lnodes,
        opt,
        real,
        sideways,
        step,
    } = ctx;
    let (end_pad, flipped, lane_count, sideways, step) =
        (*end_pad, *flipped, *lane_count, *sideways, *step);
    let n = nodes.len();
    let m = ledges.len();

    let pad: Vec<f64> = lane_pads
        .iter()
        .zip(margins)
        .map(|(&room, &margin)| if margin { room.max(MARGIN) } else { room })
        .collect();
    let mut used = vec![false; lane_count];

    // Items: the real ones, then a dummy for every rank an edge passes through.
    let mut dummies: i64 = 0;
    let span: Vec<i64> = ledges
        .iter()
        .map(|e| (lnodes[e.head].rank as i64 - lnodes[e.tail].rank as i64).abs())
        .collect();
    for k in 0..m {
        if !plain[k] {
            dummies += span[k] - 1;
        }
    }
    let direct = dummies > MAX_DUMMIES;
    let total = real.len() + if direct { 0 } else { dummies.max(0) as usize };
    let mut rank_of = vec![0usize; total];
    let mut lane = vec![0usize; total];
    let mut size = vec![0.0f64; total];
    let mut depth = vec![0.0f64; total];
    let mut dummy = vec![false; total];
    let mut max_rank = 0;
    for k in 0..real.len() {
        rank_of[k] = lnodes[k].rank;
        lane[k] = item_lane[k];
        size[k] = lnodes[k].w;
        depth[k] = lnodes[k].h;
        max_rank = max_rank.max(rank_of[k]);
    }
    let mut first_dummy: Vec<Option<usize>> = vec![None; m];
    let mut label_item: Vec<Option<usize>> = vec![None; m];
    let mut links: Vec<usize> = Vec::new();
    {
        let mut next = real.len();
        for (k, e) in ledges.iter().enumerate() {
            let (a, b) = if e.reversed {
                (e.head, e.tail)
            } else {
                (e.tail, e.head)
            };
            if direct || span[k] < 2 || plain[k] {
                links.extend([a, b]);
                continue;
            }
            let ra = rank_of[a];
            let rb = rank_of[b];
            let mut label_rank = -1i64;
            if e.label_w > 0.0 {
                label_rank = (ra + ((rb - ra) >> 1)) as i64;
                if step == 2 && label_rank & 1 == 0 {
                    label_rank -= 1;
                }
            }
            first_dummy[k] = Some(next);
            let mut prev = a;
            for r in ra + 1..rb {
                let d = next;
                next += 1;
                rank_of[d] = r;
                // The edge crosses from its first lane to its last one evenly along the way.
                let (la, lb) = (lane[a] as f64, lane[b] as f64);
                lane[d] = (la + js_round((lb - la) * (r - ra) as f64 / (rb - ra) as f64)) as usize;
                dummy[d] = true;
                if r as i64 == label_rank {
                    size[d] = e.label_w;
                    depth[d] = e.label_h;
                    label_item[k] = Some(d);
                }
                links.extend([prev, d]);
                prev = d;
            }
            links.extend([prev, b]);
        }
    }

    let mut up_start = vec![0usize; total + 1];
    let mut down_start = vec![0usize; total + 1];
    for pair in links.chunks_exact(2) {
        down_start[pair[0] + 1] += 1;
        up_start[pair[1] + 1] += 1;
    }
    for i in 0..total {
        up_start[i + 1] += up_start[i];
        down_start[i + 1] += down_start[i];
    }
    let mut up = vec![0usize; links.len() / 2];
    let mut down = vec![0usize; links.len() / 2];
    {
        let mut uf = up_start[..total].to_vec();
        let mut df = down_start[..total].to_vec();
        for pair in links.chunks_exact(2) {
            down[df[pair[0]]] = pair[1];
            df[pair[0]] += 1;
            up[uf[pair[1]]] = pair[0];
            uf[pair[1]] += 1;
        }
    }

    // Order within each rank: by lane first, then by where the item's neighbours sit.
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..total {
        rows[rank_of[i]].push(i);
    }
    let mut pos = vec![0.0f64; total];
    let mut key = vec![0.0f64; total];
    let settle = |row: &mut Vec<usize>, key: &[f64], pos: &mut [f64]| {
        row.sort_by(|&a, &b| {
            lane[a].cmp(&lane[b]).then_with(|| {
                let diff = key[a] - key[b];
                if diff > 0.0 {
                    Ordering::Greater
                } else if diff < 0.0 {
                    Ordering::Less
                } else {
                    a.cmp(&b)
                }
            })
        });
        for (k, &i) in row.iter().enumerate() {
            pos[i] = k as f64;
        }
    };
    for row in &mut rows {
        for (k, &i) in row.iter().enumerate() {
            key[i] = k as f64;
        }
        settle(row, &key, &mut pos);
    }
    for sweep in 0..SWEEPS {
        if max_rank == 0 {
            break;
        }
        let downward = sweep & 1 == 0;
        for s in 1..=max_rank {
            let row = &mut rows[if downward { s } else { max_rank - s }];
            let (start, list) = if downward {
                (&up_start, &up)
            } else {
                (&down_start, &down)
            };
            for &i in row.iter() {
                let mut sum = 0.0;
                let count = start[i + 1] - start[i];
                for k in start[i]..start[i + 1] {
                    sum += pos[list[k]];
                }
                key[i] = if count > 0 {
                    sum / count as f64
                } else {
                    pos[i]
                };
            }
            settle(row, &key, &mut pos);
        }
    }

    // The items of one rank that share a lane sit side by side as a run.
    let gap = |a: usize, b: usize| {
        if dummy[a] || dummy[b] {
            opt.edge_sep
        } else {
            opt.node_sep
        }
    };
    let runs = |row: &[usize]| {
        let mut out = Vec::new();
        let mut from = 0;
        while from < row.len() {
            let mut to = from;
            while to + 1 < row.len() && lane[row[to + 1]] == lane[row[from]] {
                to += 1;
            }
            out.push((from, to));
            from = to + 1;
        }
        out
    };
    let run_size = |row: &[usize], from: usize, to: usize| {
        let mut total = size[row[from]];
        for k in from..to {
            total += gap(row[k], row[k + 1]) + size[row[k + 1]];
        }
        total
    };

    // Lane breadth: enough for its fullest rank, and for its title.
    let mut breadth = vec![0.0f64; lane_count];
    let mut head = if lanes.is_empty() { 0.0 } else { LANE_HEAD };
    for (k, &id) in lanes.iter().enumerate() {
        let node = &nodes[id];
        breadth[k] = (node.min_w - 2.0 * pad[k]).max(0.0);
        if node.pad_top > head {
            head = node.pad_top;
        }
    }
    for row in &rows {
        for (from, to) in runs(row) {
            let total = run_size(row, from, to);
            let at = lane[row[from]];
            if total > breadth[at] {
                breadth[at] = total;
            }
        }
    }
    let mut lane_start = vec![0.0f64; lane_count + 1];
    for k in 0..lane_count {
        lane_start[k + 1] = lane_start[k] + breadth[k] + 2.0 * pad[k];
    }

    // Across the flow: each lane's items start centered, then line up with their neighbours in
    // the same lane as far as the lane and the items beside them allow.
    let mut c = vec![0.0f64; total];
    let mut want = vec![0.0f64; total];
    for row in &rows {
        for (from, to) in runs(row) {
            let at_lane = lane[row[from]];
            let mut at =
                (lane_start[at_lane] + pad[at_lane] + (lane_start[at_lane + 1] - pad[at_lane])
                    - run_size(row, from, to))
                    / 2.0;
            for k in from..=to {
                c[row[k]] = at + size[row[k]] / 2.0;
                if k < to {
                    at += size[row[k]] + gap(row[k], row[k + 1]);
                }
            }
        }
    }
    for pass in 0..4 {
        if max_rank == 0 {
            break;
        }
        let downward = pass & 1 == 0;
        for s in 1..=max_rank {
            let row = &rows[if downward { s } else { max_rank - s }];
            let (start, list) = if downward {
                (&up_start, &up)
            } else {
                (&down_start, &down)
            };
            for &i in row {
                let mut sum = 0.0;
                let mut count = 0;
                for k in start[i]..start[i + 1] {
                    if lane[list[k]] != lane[i] {
                        continue;
                    }
                    sum += c[list[k]];
                    count += 1;
                }
                want[i] = if count > 0 {
                    sum / f64::from(count)
                } else {
                    c[i]
                };
            }
            for (from, to) in runs(row) {
                let at_lane = lane[row[from]];
                let mut floor = lane_start[at_lane] + pad[at_lane];
                for k in from..=to {
                    let i = row[k];
                    c[i] = want[i].max(floor + size[i] / 2.0);
                    if k < to {
                        floor = c[i] + size[i] / 2.0 + gap(i, row[k + 1]);
                    }
                }
                let mut ceil = lane_start[at_lane + 1] - pad[at_lane];
                for k in (from..=to).rev() {
                    let i = row[k];
                    c[i] = c[i].min(ceil - size[i] / 2.0);
                    if k > from {
                        ceil = c[i] - size[i] / 2.0 - gap(row[k - 1], i);
                    }
                }
            }
        }
    }

    // Along the flow: one band per rank, after the lane titles.
    let mut band_start = vec![0.0f64; max_rank + 1];
    let mut band_end = vec![0.0f64; max_rank + 1];
    // The room after a rank is for the relations that turn there or are only as long as they
    // were asked to be. Where just plain handoffs and straight lines pass, the ranks close up,
    // as far as the label of a handoff between them leaves it a line to sit on.
    let mut sep = vec![opt.rank_sep / step as f64; max_rank + 1];
    if plain.iter().any(|&on| on) {
        let full = opt.rank_sep / step as f64;
        sep.fill(full / CLOSE);
        for (k, e) in ledges.iter().enumerate() {
            let (a, b) = if e.reversed {
                (e.head, e.tail)
            } else {
                (e.tail, e.head)
            };
            if plain[k] {
                if e.label_h <= 0.0 || span[k] > step as i64 {
                    continue;
                }
                let room = full
                    .min((e.label_h + 2.0 * TURN - depth[a].min(depth[b]) / 2.0) / span[k] as f64);
                for s in &mut sep[rank_of[a]..rank_of[b]] {
                    if room > *s {
                        *s = room;
                    }
                }
                continue;
            }
            let Some(d0) = first_dummy[k].filter(|_| span[k] > step as i64 * i64::from(e.minlen))
            else {
                sep[rank_of[a]..rank_of[b]].fill(full);
                continue;
            };
            let count = span[k] as usize;
            for j in 0..count {
                let u = if j == 0 { a } else { d0 + j - 1 };
                let v = if j + 1 < count { d0 + j } else { b };
                if lane[u] != lane[v] || (c[u] - c[v]).abs() >= 0.5 {
                    sep[rank_of[u]] = full;
                }
            }
        }
    }
    let mut at = head + end_pad;
    for r in 0..=max_rank {
        let mut most = 0.0f64;
        for &i in &rows[r] {
            if depth[i] > most {
                most = depth[i];
            }
        }
        band_start[r] = at;
        band_end[r] = at + most;
        at += most + if r == max_rank { 0.0 } else { sep[r] };
    }
    let length = at + end_pad;
    let breadth_total = lane_start[lane_count];
    let flip = |a: f64| if flipped { head + length - a } else { a };
    let px = |c: f64, a: f64| if sideways { flip(a) } else { c };
    let py = |c: f64, a: f64| if sideways { c } else { flip(a) };
    let axis = if sideways { 1.0 } else { 0.0 };

    for (k, &id) in real.iter().enumerate() {
        let a = f64::midpoint(band_start[rank_of[k]], band_end[rank_of[k]]);
        nodes[id].x = px(c[k], a);
        nodes[id].y = py(c[k], a);
    }
    for (k, &id) in lanes.iter().enumerate() {
        let mid = f64::midpoint(lane_start[k], lane_start[k + 1]);
        let breadth = lane_start[k + 1] - lane_start[k];
        let node = &mut nodes[id];
        node.x = if sideways { length / 2.0 } else { mid };
        node.y = if sideways { mid } else { length / 2.0 };
        node.w = if sideways { length } else { breadth };
        node.h = if sideways { breadth } else { length };
        node.pad_top = head;
    }

    // Groups inside a lane are drawn around their members, deepest first.
    let mut depth_of = vec![0usize; n];
    let mut inner = Vec::new();
    for i in 0..n {
        if !nodes[i].is_group || lane_slot[i].is_some() || item_of[i].is_some() {
            continue;
        }
        let mut d = 0;
        let mut p = parent(nodes, i);
        while let Some(at) = p
            && d < MAX_HOPS
        {
            d += 1;
            p = parent(nodes, at);
        }
        depth_of[i] = d;
        inner.push(i);
    }
    inner.sort_by(|&a, &b| depth_of[b].cmp(&depth_of[a]).then(a.cmp(&b)));
    let mut bounds = vec![
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY
        ];
        n
    ];
    let grow = |bounds: &mut [[f64; 4]], g: usize, c: &CNode| {
        let b = &mut bounds[g];
        if c.x - c.w / 2.0 < b[0] {
            b[0] = c.x - c.w / 2.0;
        }
        if c.y - c.h / 2.0 < b[1] {
            b[1] = c.y - c.h / 2.0;
        }
        if c.x + c.w / 2.0 > b[2] {
            b[2] = c.x + c.w / 2.0;
        }
        if c.y + c.h / 2.0 > b[3] {
            b[3] = c.y + c.h / 2.0;
        }
    };
    for &i in real {
        if let Some(p) = parent(nodes, i)
            && lane_slot[p].is_none()
        {
            grow(&mut bounds, p, &nodes[i]);
        }
    }
    for &g in &inner {
        let [x0, y0, x1, y1] = bounds[g];
        if x0 == f64::INFINITY {
            continue;
        }
        let group = &mut nodes[g];
        let w = (x1 - x0 + 2.0 * group.pad_x).max(group.min_w);
        group.x = f64::midpoint(x0, x1);
        group.w = w;
        group.y = (y0 - group.pad_top + y1 + group.pad_bottom) / 2.0;
        group.h = y1 - y0 + group.pad_top + group.pad_bottom;
        if let Some(p) = parent(nodes, g)
            && lane_slot[p].is_none()
        {
            grow(&mut bounds, p, &nodes[g]);
        }
    }

    for e in edges.iter_mut() {
        e.route = Vec::new();
    }
    for (k, e) in ledges.iter().enumerate() {
        let (a, b) = if e.reversed {
            (e.head, e.tail)
        } else {
            (e.tail, e.head)
        };
        let (ra, rb) = (rank_of[a], rank_of[b]);
        let mut pts = vec![
            c[a],
            band_end[ra] - (band_end[ra] - band_start[ra] - depth[a]) / 2.0,
        ];
        if let Some(d0) = first_dummy[k] {
            for d in d0..d0 + span[k] as usize - 1 {
                let rd = rank_of[d];
                pts.extend([c[d], band_start[rd]]);
                if band_end[rd] > band_start[rd] {
                    pts.extend([c[d], band_end[rd]]);
                }
            }
        }
        pts.extend([
            c[b],
            band_start[rb] + (band_end[rb] - band_start[rb] - depth[b]) / 2.0,
        ]);
        let mut route = Vec::with_capacity(pts.len() / 2 * 3);
        for p in pts.chunks_exact(2) {
            route.extend([px(p[0], p[1]), py(p[0], p[1]), axis]);
        }
        if e.reversed && route.len() >= 6 {
            let (mut i, mut j) = (0, route.len() - 3);
            while i < j {
                route.swap(i, j);
                route.swap(i + 1, j + 1);
                i += 3;
                j -= 3;
            }
        }
        let out = &mut edges[edge_of[k]];
        if let Some(label) = label_item[k] {
            let mid = f64::midpoint(band_start[rank_of[label]], band_end[rank_of[label]]);
            out.label_x = px(c[label], mid);
            out.label_y = py(c[label], mid);
        } else if let (Some(&x0), Some(&y0)) = (route.first(), route.get(1)) {
            let last = route.len() - 3;
            out.label_x = f64::midpoint(x0, route[last]);
            out.label_y = f64::midpoint(y0, route[last + 1]);
        }
        out.route = route;
    }

    // An edge that starts or ends at a lane or a group runs straight to its border.
    for e in edges.iter_mut() {
        if !e.route.is_empty() || e.src == e.dst {
            continue;
        }
        let (ax, ay) = border(&nodes[e.src], &nodes[e.dst]);
        let (bx, by) = border(&nodes[e.dst], &nodes[e.src]);
        let flat = if (bx - ax).abs() > (by - ay).abs() {
            1.0
        } else {
            0.0
        };
        e.route = vec![ax, ay, flat, bx, by, flat];
        e.label_x = f64::midpoint(ax, bx);
        e.label_y = f64::midpoint(ay, by);
    }

    // A handoff can leave towards the other lane, then arrive along the flow (or vice versa).
    // In particular, a decision's return branch must not leave through its incoming side and
    // retrace the path past the preceding step. Try the two single-corner routes, keeping the
    // ranked route when a node or another label blocks them.
    let mut routed = vec![false; m];
    let route_points: f64 = edges.iter().map(|edge| (edge.route.len() / 3) as f64).sum();
    let work = m as f64 * (real.len() as f64 + edges.len() as f64 + route_points);
    if work <= ROUTE_BUDGET {
        let amid = |i: usize| f64::midpoint(band_start[rank_of[i]], band_end[rank_of[i]]);
        let mut labels: Vec<[f64; 2]> = edges
            .iter()
            .map(|e| {
                [
                    if sideways { e.label_y } else { e.label_x },
                    flip(if sideways { e.label_x } else { e.label_y }),
                ]
            })
            .collect();
        let overlaps = |c0: f64, a0: f64, c1: f64, a1: f64, lc: f64, la: f64, cw: f64, ah: f64| {
            c1 > lc - cw / 2.0 && c0 < lc + cw / 2.0 && a1 > la - ah / 2.0 && a0 < la + ah / 2.0
        };
        let clear = |edges: &[CEdge],
                     labels: &[[f64; 2]],
                     (c0, a0, c1, a1): (f64, f64, f64, f64),
                     ends: Option<(usize, usize)>,
                     ei: usize| {
            for i in 0..real.len() {
                if ends.is_some_and(|(src, dst)| i == src || i == dst) {
                    continue;
                }
                if overlaps(c0, a0, c1, a1, c[i], amid(i), size[i] + 8.0, depth[i] + 8.0) {
                    return false;
                }
            }
            for (i, e) in edges.iter().enumerate() {
                // A step's own loop keeps its label inside the step's box, and has no place here.
                let (w, h) = if sideways {
                    (e.label_h, e.label_w)
                } else {
                    (e.label_w, e.label_h)
                };
                if i != ei
                    && e.label_w > 0.0
                    && e.src != e.dst
                    && overlaps(c0, a0, c1, a1, labels[i][0], labels[i][1], w, h)
                {
                    return false;
                }
            }
            true
        };
        // How many other relations have a stretch inside a box. A stretch's whole box counts,
        // including its curve.
        let crossings = |edges: &[CEdge], x0: f64, y0: f64, x1: f64, y1: f64, ei: usize| {
            let mut count = 0;
            for (i, e) in edges.iter().enumerate() {
                if i == ei {
                    continue;
                }
                let r = &e.route;
                let mut j = 3;
                while j < r.len() {
                    if r[j - 3].max(r[j]) > x0
                        && r[j - 3].min(r[j]) < x1
                        && r[j - 2].max(r[j + 1]) > y0
                        && r[j - 2].min(r[j + 1]) < y1
                    {
                        count += 1;
                        break;
                    }
                    j += 3;
                }
            }
            count
        };
        let meets = |edges: &[CEdge], x: f64, y: f64, ei: usize| {
            let mut count = 0;
            for (i, e) in edges.iter().enumerate() {
                let r = &e.route;
                if i == ei || r.is_empty() {
                    continue;
                }
                let last = r.len() - 3;
                if ((r[0] - x).abs() < 1.0 && (r[1] - y).abs() < 1.0)
                    || ((r[last] - x).abs() < 1.0 && (r[last + 1] - y).abs() < 1.0)
                {
                    count += 1;
                }
            }
            count
        };
        // Two relations drawn along one line read as a single path. Those that leave the same step
        // or arrive at the same step may share a trunk; any other pair must not.
        let retraces = |edges: &[CEdge], x0: f64, y0: f64, x1: f64, y1: f64, ei: usize| {
            let upright = (x1 - x0).abs() < 0.5;
            let lo = if upright { y0.min(y1) } else { x0.min(x1) };
            let hi = if upright { y0.max(y1) } else { x0.max(x1) };
            let line = if upright { x0 } else { y0 };
            let (us, vs) = if upright { (0, 1) } else { (1, 0) };
            for (i, other) in edges.iter().enumerate() {
                if i == ei || other.src == edges[ei].src || other.dst == edges[ei].dst {
                    continue;
                }
                let r = &other.route;
                let mut j = 3;
                while j < r.len() {
                    let (u0, u1) = (r[j - 3 + us], r[j + us]);
                    if (u0 - line).abs() < NEAR && (u1 - line).abs() < NEAR {
                        let (v0, v1) = (r[j - 3 + vs], r[j + vs]);
                        if hi.min(v0.max(v1)) - lo.max(v0.min(v1)) > 1.0 {
                            return true;
                        }
                    }
                    j += 3;
                }
            }
            false
        };

        for (k, e) in ledges.iter().enumerate() {
            let ei = edge_of[k];
            let (a, b) = (e.tail, e.head);
            if lane[a] == lane[b] || (c[b] - c[a]).abs() < 1.0 {
                continue;
            }
            let (ac, bc, aa, ba) = (c[a], c[b], amid(a), amid(b));
            let dc = sign(bc - ac);
            let da = sign(ba - aa);
            let attempt =
                |edges: &mut [CEdge], labels: &mut [[f64; 2]], pts: &[f64], margin: f64| {
                    let mut p = 2;
                    while p + 1 < pts.len() {
                        let bounds = (
                            pts[p - 2].min(pts[p]) - margin,
                            pts[p - 1].min(pts[p + 1]) - margin,
                            pts[p - 2].max(pts[p]) + margin,
                            pts[p - 1].max(pts[p + 1]) + margin,
                        );
                        if !clear(edges, labels, bounds, Some((a, b)), ei) {
                            return false;
                        }
                        let (x0, y0) = (px(pts[p - 2], pts[p - 1]), py(pts[p - 2], pts[p - 1]));
                        let (x1, y1) = (px(pts[p], pts[p + 1]), py(pts[p], pts[p + 1]));
                        if retraces(edges, x0, y0, x1, y1, ei) {
                            return false;
                        }
                        p += 2;
                    }

                    let (out_w, out_h) = (edges[ei].label_w, edges[ei].label_h);
                    let (mut lc, mut la, mut longest) = (0.0, 0.0, -1.0);
                    if out_w > 0.0 {
                        let (cw, ah) = (e.label_w, e.label_h);
                        let mut p = 2;
                        while p + 1 < pts.len() {
                            let cc = f64::midpoint(pts[p - 2], pts[p]);
                            let ca = f64::midpoint(pts[p - 1], pts[p + 1]);
                            let across = (pts[p] - pts[p - 2]).abs();
                            let along = (pts[p + 1] - pts[p - 1]).abs();
                            let run = across + along;
                            p += 2;
                            let short = if across > 0.0 {
                                across < cw + 8.0
                            } else {
                                along < ah + 8.0
                            };
                            if run <= longest || short {
                                continue;
                            }
                            if cc - cw / 2.0 < 0.0
                                || cc + cw / 2.0 > breadth_total
                                || ca - ah / 2.0 < 0.0
                                || ca + ah / 2.0 > length
                            {
                                continue;
                            }
                            // Include the ends here: the label must clear both shapes it connects.
                            let bounds =
                                (cc - cw / 2.0, ca - ah / 2.0, cc + cw / 2.0, ca + ah / 2.0);
                            if !clear(edges, labels, bounds, None, ei) {
                                continue;
                            }
                            // A label moved onto a handoff must not cover a return path that keeps its
                            // ranked route. A segment's whole box is kept clear, including its curve.
                            let (lx, ly) = (px(cc, ca), py(cc, ca));
                            if crossings(
                                edges,
                                lx - out_w / 2.0,
                                ly - out_h / 2.0,
                                lx + out_w / 2.0,
                                ly + out_h / 2.0,
                                ei,
                            ) > 0
                            {
                                continue;
                            }
                            lc = cc;
                            la = ca;
                            longest = run;
                        }
                        if longest < 0.0 {
                            return false;
                        }
                    }

                    // A point short of each corner and one past it keep the turn close to the corner,
                    // so that the line stays under its label and the handoff takes little room.
                    let mut way = vec![pts[0], pts[1]];
                    let mut p = 2;
                    while p + 1 < pts.len() {
                        let dx = pts[p] - pts[p - 2];
                        let dy = pts[p + 1] - pts[p - 1];
                        let run = dx.abs() + dy.abs();
                        let first = if p > 2 { TURN } else { 0.0 };
                        let last = if p + 2 < pts.len() { TURN } else { 0.0 };
                        if run > first + last + 1.0 {
                            if first != 0.0 {
                                way.extend([
                                    pts[p - 2] + (dx / run) * first,
                                    pts[p - 1] + (dy / run) * first,
                                ]);
                            }
                            if last != 0.0 {
                                way.extend([
                                    pts[p] - (dx / run) * last,
                                    pts[p + 1] - (dy / run) * last,
                                ]);
                            }
                        }
                        way.extend([pts[p], pts[p + 1]]);
                        p += 2;
                    }
                    let out = &mut edges[ei];
                    out.route = Vec::with_capacity(way.len() / 2 * 3);
                    let mut p = 0;
                    while p + 1 < way.len() {
                        let crosswise = if p == 0 {
                            way.get(2).is_none_or(|&next| way[0] != next)
                        } else {
                            way[p] != way[p - 2]
                        };
                        out.route.extend([
                            px(way[p], way[p + 1]),
                            py(way[p], way[p + 1]),
                            if crosswise { 1.0 - axis } else { axis },
                        ]);
                        p += 2;
                    }
                    if out.label_w > 0.0 {
                        out.label_x = px(lc, la);
                        out.label_y = py(lc, la);
                        labels[ei] = [lc, la];
                    }
                    true
                };

            if attempt(
                edges,
                &mut labels,
                &[
                    ac + dc * size[a] / 2.0,
                    aa,
                    bc,
                    aa,
                    bc,
                    ba - da * depth[b] / 2.0,
                ],
                2.0,
            ) || attempt(
                edges,
                &mut labels,
                &[
                    ac,
                    aa + da * depth[a] / 2.0,
                    ac,
                    ba,
                    bc - dc * size[b] / 2.0,
                    ba,
                ],
                2.0,
            ) {
                routed[k] = true;
                continue;
            }

            // With both corners blocked, as for a return past the steps between, the handoff goes
            // around: out to the margin of a lane, along it, and back in level with its end. It
            // takes the margin that crosses the fewest other relations and shares the fewest ends
            // with them, then the shortest way.
            if work > ROUTE_BUDGET / RELAY {
                continue;
            }
            let mut around: Vec<([f64; 8], f64, usize)> = Vec::new();
            let order = if lane[a] < lane[b] {
                [lane[b], lane[a]]
            } else {
                [lane[a], lane[b]]
            };
            for at_lane in order {
                // A lane that keeps no margin yet has less room there, and the handoff stays
                // inside it.
                let inset = (pad[at_lane] - LEG).max((pad[at_lane] / 2.0).min(MARGIN - LEG));
                for ch in [lane_start[at_lane] + inset, lane_start[at_lane + 1] - inset] {
                    if (ch - ac).abs() < size[a] / 2.0 + 4.0
                        || (ch - bc).abs() < size[b] / 2.0 + 4.0
                    {
                        continue;
                    }
                    let pts = [
                        ac + sign(ch - ac) * size[a] / 2.0,
                        aa,
                        ch,
                        aa,
                        ch,
                        ba,
                        bc + sign(ch - bc) * size[b] / 2.0,
                        ba,
                    ];
                    let ends = meets(edges, px(pts[0], pts[1]), py(pts[0], pts[1]), ei)
                        + meets(edges, px(pts[6], pts[7]), py(pts[6], pts[7]), ei);
                    let mut cost = f64::from(ends) * 1e6;
                    for p in [2, 4, 6] {
                        let (x0, y0) = (px(pts[p - 2], pts[p - 1]), py(pts[p - 2], pts[p - 1]));
                        let (x1, y1) = (px(pts[p], pts[p + 1]), py(pts[p], pts[p + 1]));
                        let crossed =
                            crossings(edges, x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1), ei);
                        cost += f64::from(crossed) * 1e6 + (x1 - x0).abs() + (y1 - y0).abs();
                    }
                    around.push((pts, cost, at_lane));
                }
            }
            around.sort_by(|p, q| p.1.partial_cmp(&q.1).unwrap_or(Ordering::Equal));
            for (pts, _, at_lane) in &around {
                if !attempt(edges, &mut labels, pts, 1.0) {
                    continue;
                }
                routed[k] = true;
                used[*at_lane] = true;
                break;
            }
        }
    }

    Placed {
        height: if sideways { breadth_total } else { length },
        margins: used,
        routed,
        width: if sideways { length } else { breadth_total },
        work,
    }
}

/// The point where the line from one box's center to another's leaves the first box. When the
/// other center is inside the box, the point is on the nearest side instead.
fn border(from: &CNode, to: &CNode) -> (f64, f64) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let hw = (from.w / 2.0).max(0.5);
    let hh = (from.h / 2.0).max(0.5);
    if dx.abs() < hw && dy.abs() < hh {
        let to_side = hw - dx.abs();
        let to_end = hh - dy.abs();
        return if to_side < to_end {
            (from.x + if dx < 0.0 { -hw } else { hw }, to.y)
        } else {
            (to.x, from.y + if dy < 0.0 { -hh } else { hh })
        };
    }
    let sx = if dx == 0.0 {
        f64::INFINITY
    } else {
        hw / dx.abs()
    };
    let sy = if dy == 0.0 {
        f64::INFINITY
    } else {
        hh / dy.abs()
    };
    let scale = sx.min(sy);
    (from.x + dx * scale, from.y + dy * scale)
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::float_cmp,
    reason = "test graphs hold small whole numbers, built as the Pele script builds them"
)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `laneLayout` with `bun`, never from this port: on the graphs Pele's
    //! swimlane renderer builds for the swimlane corpus, and on random graphs both sides build
    //! from the same seeded generator. Each case compares an FNV-1a hash of the exact bits of the
    //! size, every node's box, and every edge's label and route.

    use super::*;
    use crate::diagrams::common::tokens::tests::fnv;
    use crate::layout::compound::cnode;

    struct Case {
        dir: Dir,
        edges: Vec<CEdge>,
        nodes: Vec<CNode>,
        opt: LayeredOptions,
        pad: f64,
    }

    fn options(node_sep: f64, edge_sep: f64, rank_sep: f64) -> LayeredOptions {
        LayeredOptions {
            edge_sep,
            node_sep,
            port_sep: 20.0,
            rank_sep,
            ..LayeredOptions::default()
        }
    }

    fn dir(name: &str) -> Dir {
        match name {
            "BT" => Dir::Bt,
            "LR" => Dir::Lr,
            "RL" => Dir::Rl,
            _ => Dir::Tb,
        }
    }

    fn edge(src: usize, dst: usize, minlen: u32, label_w: f64, label_h: f64) -> CEdge {
        CEdge {
            dst,
            label_h,
            label_w,
            minlen,
            src,
            ..CEdge::default()
        }
    }

    /// A graph in the scripts' compact form: `dir|pad|opt|nodes|edges`.
    fn parse(text: &str) -> Case {
        let parts: Vec<&str> = text.split('|').collect();
        let numbers = |item: &str| {
            item.split(',')
                .map(|n| n.parse::<f64>().unwrap())
                .collect::<Vec<_>>()
        };
        let opt = numbers(parts[2]);
        let nodes = parts[3]
            .split(';')
            .filter(|item| !item.is_empty())
            .map(|item| {
                let v = numbers(item);
                CNode {
                    is_group: v[3] == 1.0,
                    min_w: v[7],
                    pad_bottom: v[6],
                    pad_x: v[4],
                    pad_top: v[5],
                    ..cnode(v[0], v[1], v[2] as i32)
                }
            })
            .collect();
        let edges = parts[4]
            .split(';')
            .filter(|item| !item.is_empty())
            .map(|item| {
                let v = numbers(item);
                edge(v[0] as usize, v[1] as usize, v[2] as u32, v[3], v[4])
            })
            .collect();
        Case {
            dir: dir(parts[0]),
            edges,
            nodes,
            opt: options(opt[0], opt[1], opt[2]),
            pad: parts[1].parse().unwrap(),
        }
    }

    /// The random graph for `seed`, built as the Pele script's `randomCase` builds it.
    fn random(seed: u32) -> Case {
        let mut s = seed;
        let mut int = |n: usize| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(s) / 4_294_967_296.0 * n as f64).floor() as usize
        };
        let mut nodes = Vec::new();
        let lanes = 1 + int(4);
        for _ in 0..lanes {
            let pad_top = (20 + int(30)) as f64;
            let min_w = (int(3) * 60) as f64;
            nodes.push(CNode {
                is_group: true,
                min_w,
                pad_top,
                ..cnode(0.0, 0.0, -1)
            });
        }
        let groups = int(3);
        for _ in 0..groups {
            let parent = int(nodes.len()) as i32;
            let pad_x = (8 + int(10)) as f64;
            let pad_top = (16 + int(10)) as f64;
            let min_w = (int(2) * 80) as f64;
            nodes.push(CNode {
                is_group: true,
                min_w,
                pad_bottom: 8.0,
                pad_x,
                pad_top,
                ..cnode(0.0, 0.0, parent)
            });
        }
        let count = 2 + int(14);
        for _ in 0..count {
            let w = (40 + int(80)) as f64;
            let h = (30 + int(30)) as f64;
            let parent = int(nodes.len()) as i32;
            nodes.push(cnode(w, h, parent));
        }
        let mut edges = Vec::new();
        let m = int(count * 2);
        for _ in 0..m {
            let label = int(3) == 0;
            let src = int(nodes.len());
            let dst = int(nodes.len());
            let minlen = 1 + int(2) * int(2);
            let label_w = if label { (20 + int(60)) as f64 } else { 0.0 };
            edges.push(edge(
                src,
                dst,
                minlen as u32,
                label_w,
                if label { 18.0 } else { 0.0 },
            ));
        }
        let node_sep = (30 + int(20)) as f64;
        let edge_sep = (10 + int(10)) as f64;
        let rank_sep = (40 + int(30)) as f64;
        let dir = [Dir::Tb, Dir::Lr, Dir::Bt, Dir::Rl][int(4)];
        let pad = if int(2) == 0 { LANE_PAD } else { 12.0 };
        Case {
            dir,
            edges,
            nodes,
            opt: options(node_sep, edge_sep, rank_sep),
            pad,
        }
    }

    fn bits(values: &[f64]) -> String {
        values
            .iter()
            .map(|&v| format!("{:x}", if v == 0.0 { 0.0f64 } else { v }.to_bits()))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn outcome(mut case: Case) -> u32 {
        let size = lane_layout(
            &mut case.nodes,
            &mut case.edges,
            case.dir,
            &case.opt,
            case.pad,
        );
        let mut out = vec![bits(&[size.width]), bits(&[size.height])];
        for n in &case.nodes {
            out.push(bits(&[n.x, n.y, n.w, n.h, n.pad_top]));
        }
        for e in &case.edges {
            let mut values = vec![e.label_x, e.label_y];
            values.extend(&e.route);
            out.push(bits(&values));
        }
        fnv(&out.join("\n"))
    }

    #[test]
    fn lane_layout_matches_pele_on_swimlane_corpus() {
        let cases: &[(&str, u32)] = &[
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,84.912;0,0,-1,1,20,40,20,109.808;0,0,-1,1,20,40,20,97.344;163.168,48,2,0,0,0,0,0;63.568,48,2,0,0,0,0,0;108.896,48,1,0,0,0,0,0;109.808,48,1,0,0,0,0,0;131.136,48,0,0,0,0,0,0|3,4,1,0,0;4,5,1,0,0;5,6,1,0,0;4,7,1,0,0",
                0x4dcc9821,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,84.912;0,0,-1,1,20,40,20,109.808;0,0,-1,1,20,40,20,97.344;163.168,48,2,0,0,0,0,0;63.568,48,2,0,0,0,0,0;108.896,48,1,0,0,0,0,0;109.808,48,1,0,0,0,0,0;131.136,48,0,0,0,0,0,0|3,4,1,0,0;4,5,1,0,0;5,6,1,0,0;4,7,1,0,0",
                0x4dcc9821,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,113.376;0,0,-1,1,20,40,20,84.032;0,0,-1,1,20,40,20,97.344;150.704,48,2,0,0,0,0,0;147.168,48,2,0,0,0,0,0;139.152,48,1,0,0,0,0,0;129.376,48,1,0,0,0,0,0;155.168,48,0,0,0,0,0,0;113.36,48,0,0,0,0,0,0|3,5,1,0,0;5,6,1,91.366,25;5,7,1,136.51600000000002,25;7,8,1,0,0;8,6,1,0,0;6,4,1,0,0",
                0x5d496276,
            ),
            (
                "TB|24|40,16,48|0,0,-1,1,20,40,20,68.01599999999999;120.48,48,0,0,0,0,0,0;137.392,48,0,0,0,0,0,0|",
                0x67b643f0,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,124.928;0,0,-1,1,20,40,20,108.032;120.48,48,1,0,0,0,0,0;137.392,48,1,0,0,0,0,0;132.89600000000002,48,0,0,0,0,0,0;140.064,48,0,0,0,0,0,0|2,3,1,0,0;3,4,1,0,0;4,5,1,0,0",
                0xccfd53e8,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,96.464;0,0,-1,1,20,40,20,80.44800000000001;0,0,-1,1,20,40,20,71.584;85.792,48,2,0,0,0,0,0;94.672,48,2,0,0,0,0,0;107.12,48,2,0,0,0,0,0;122.336,76.46,1,0,0,0,0,0;61.14750934437026,61.14750934437026,0,0,0,0,0,0|3,4,1,0,0;4,6,1,0,0;6,7,1,36.122,25;6,5,1,29.892,25;5,4,1,0,0",
                0xdbb54a87,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,66.24000000000001;0,0,-1,1,20,40,20,69.792;148.94400000000002,48,1,0,0,0,0,0;117.808,48,1,0,0,0,0,0;137.36,48,0,0,0,0,0,0;125.824,48,0,0,0,0,0,0|2,4,1,0,0;4,3,1,95.258,25;3,5,1,0,0",
                0x7ed65878,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,113.376;0,0,-1,1,20,40,20,84.032;0,0,-1,1,20,40,20,97.344;132.94400000000002,48,2,0,0,0,0,0;81.344,48,1,0,0,0,0,0;93.792,48,0,0,0,0,0,0|3,4,1,0,0;4,5,1,0,0",
                0xa1192cc0,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,76.88;0,0,-1,1,20,40,20,66.24000000000001;0,0,-1,1,20,40,20,97.344;127.584,48,2,0,0,0,0,0;151.56799999999998,48,2,0,0,0,0,0;123.136,48,1,0,0,0,0,0;114.24,48,1,0,0,0,0,0;149.824,48,0,0,0,0,0,0;150.704,48,0,0,0,0,0,0|3,5,1,0,0;5,6,1,0,0;6,7,1,0,0;7,8,1,0,0;8,4,1,0,0",
                0x2764150a,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,81.344;0,0,-1,1,20,40,20,94.672;0,0,-1,1,20,40,20,93.808;166.72,48,2,0,0,0,0,0;148.94400000000002,48,2,0,0,0,0,0;167.616,48,1,0,0,0,0,0;144.57600000000002,90.36,1,0,0,0,0,0;144.49599999999998,48,0,0,0,0,0,0;183.6,48,0,0,0,0,0,0|3,5,1,137.272,25;5,6,1,0,0;6,7,1,71.92,25;7,8,1,0,0;8,4,1,0,0;6,3,1,109.272,25",
                0x1834dcc5,
            ),
            (
                "TB|24|40,16,48|0,0,-1,1,20,40,20,85.77600000000001;0,0,-1,1,20,40,20,80.44800000000001;0,0,-1,1,20,40,20,71.584;142.704,48,2,0,0,0,0,0;144.49599999999998,48,2,0,0,0,0,0;146.256,48,1,0,0,0,0,0;122.336,76.46,1,0,0,0,0,0;140.928,48,0,0,0,0,0,0;142.688,48,0,0,0,0,0,0|3,4,1,0,0;4,5,1,0,0;5,6,1,0,0;6,7,1,36.122,25;7,8,1,0,0;6,3,1,29.892,25",
                0x675985e9,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,67.136;0,0,-1,1,20,40,20,106.256;151.6,48,1,0,0,0,0,0;106.24,48,1,0,0,0,0,0;149.808,48,0,0,0,0,0,0|2,3,1,0,0;3,4,1,0,0",
                0x2fde33e7,
            ),
            (
                "LR|24|40,16,48|0,0,-1,1,20,40,20,113.376;0,0,-1,1,20,40,20,83.136;0,0,-1,1,20,40,20,84.032;217.488,135.93,2,0,0,0,0,0;188.064,48,2,0,0,0,0,0;118.672,48,1,0,0,0,0,0;131.136,48,0,0,0,0,0,0|3,4,1,36.122,25;3,5,1,29.892,25;5,6,1,0,0;6,4,1,0,0",
                0xefce4d6b,
            ),
        ];
        for &(graph, hash) in cases {
            assert_eq!(outcome(parse(graph)), hash, "{graph}");
        }
    }

    #[test]
    fn lane_layout_matches_pele_on_random_graphs() {
        let cases: &[(u32, u32)] = &[
            (1, 0x363eac50),
            (2, 0xe6bfdc8),
            (3, 0x2c58df2c),
            (4, 0xefffc62f),
            (5, 0xae05cd78),
            (6, 0x59f5cd22),
            (7, 0x96dfe9d6),
            (8, 0xe0184a67),
            (9, 0xf8d3e4fd),
            (10, 0x1354893),
            (11, 0x84080dfe),
            (12, 0xa4a9d1ed),
            (13, 0x92de611d),
            (14, 0x854f7e66),
            (15, 0xb7102917),
            (16, 0x3b00d9a2),
            (17, 0x53c14461),
            (18, 0xa90ec47a),
            (19, 0xfc2b736b),
            (20, 0xce740002),
            (21, 0x340ffa04),
            (22, 0x27927704),
            (23, 0xc48b2064),
            (24, 0x2b0832a7),
            (25, 0xbb0723c1),
            (26, 0x5c39fccb),
            (27, 0x8c7253cc),
            (28, 0x2cc0aee9),
            (29, 0xd2cfb742),
            (30, 0xe6863e78),
            (31, 0x7e14d4d7),
            (32, 0x372a0fc2),
            (33, 0xa5e756c1),
            (34, 0xada6673d),
            (35, 0x9286683f),
            (36, 0xe426dd5b),
            (37, 0x8473bccb),
            (38, 0x93c54525),
            (39, 0x1fedc9a),
            (40, 0x63a5cef6),
            (41, 0x942a5caa),
            (42, 0xb0b904ea),
            (43, 0x141263f5),
            (44, 0xc15aadb),
            (45, 0x4536b27),
            (46, 0xca09173b),
            (47, 0x945dfbd6),
            (48, 0x9468b6a4),
            (49, 0x42b45aba),
            (50, 0x28d9c0f),
            (51, 0x601dfc88),
            (52, 0xa09ca0e),
            (53, 0x5034028f),
            (54, 0xfcab3a7e),
            (55, 0xa19a490),
            (56, 0x76360819),
            (57, 0x9507ecca),
            (58, 0xc5bc660),
            (59, 0x3823ea66),
            (60, 0x5eb9d3c6),
            (61, 0x59305436),
            (62, 0x6c9f6cfe),
            (63, 0x482613a2),
            (64, 0x9906478b),
            (65, 0x766a7180),
            (66, 0x1aa579fc),
            (67, 0xf13b9745),
            (68, 0x80ec9091),
            (69, 0xa76fa198),
            (70, 0x5defe181),
            (71, 0x35f45518),
            (72, 0x648a8dd7),
            (73, 0xc948ea26),
            (74, 0x564ef6c5),
            (75, 0x1aaee29b),
            (76, 0x27f782a1),
            (77, 0x18939245),
            (78, 0xe940cf59),
            (79, 0x95c19128),
            (80, 0xbf08fa0d),
            (81, 0xa79ddb79),
            (82, 0x477db143),
            (83, 0x4b53b450),
            (84, 0x5ad22b52),
            (85, 0xfcaee964),
            (86, 0xa57b980d),
            (87, 0xc65e515),
            (88, 0x2cb39e10),
            (89, 0xf63eaa1b),
            (90, 0xa4f00a89),
            (91, 0xfac5a1b6),
            (92, 0x8d6d9443),
            (93, 0xf958bd2c),
            (94, 0x352632d2),
            (95, 0x9d280255),
            (96, 0xdad400de),
            (97, 0x6c18a03c),
            (98, 0xc1b88dff),
            (99, 0x25e63059),
            (100, 0x8687974e),
            (101, 0xf808dfdd),
            (102, 0xb7305fef),
            (103, 0x6ef5178b),
            (104, 0x81886ee5),
            (105, 0xe5dc26a0),
            (106, 0x8887bd86),
            (107, 0x3f821216),
            (108, 0x4a5350e6),
            (109, 0x252ddd6a),
            (110, 0x8cc617ea),
            (111, 0x8847fe27),
            (112, 0xf6be8258),
            (113, 0xa2889d1c),
            (114, 0x831e0a1f),
            (115, 0x1ff12aa5),
            (116, 0xd14f1862),
            (117, 0xb01bb009),
            (118, 0xf46d9475),
            (119, 0x796ede47),
            (120, 0x8433c6d),
            (121, 0xa20fdac1),
            (122, 0x8b0974a8),
            (123, 0x934cda0e),
            (124, 0x3202054b),
            (125, 0x5a42ec31),
            (126, 0x5c04a804),
            (127, 0xb5af9194),
            (128, 0x23085c1f),
            (129, 0x54877b55),
            (130, 0xf78a5402),
            (131, 0x5524a63e),
            (132, 0x6a1075c0),
            (133, 0x1f0c3612),
            (134, 0x398a7eec),
            (135, 0x5ae985a9),
            (136, 0xa20ff27b),
            (137, 0xcee33b21),
            (138, 0x4bcdfedd),
            (139, 0xa189b6c2),
            (140, 0x2cd17782),
            (141, 0x49f15a0),
            (142, 0x59ff6ec8),
            (143, 0xf4afca17),
            (144, 0xd6981953),
            (145, 0xf658d792),
            (146, 0x4067d2df),
            (147, 0x2faeed61),
            (148, 0x85c42b3b),
            (149, 0x69aad717),
            (150, 0x5f0351bb),
            (151, 0xdd19b78b),
            (152, 0x18704623),
            (153, 0xdf9cb4eb),
            (154, 0x49e55aa),
            (155, 0x6b43bc08),
            (156, 0xe72669a7),
            (157, 0xd9cb3a64),
            (158, 0xeebd964e),
            (159, 0xdeb565fd),
            (160, 0x73876496),
            (161, 0xe3079d7a),
            (162, 0xb0e8dad8),
            (163, 0x3b405257),
            (164, 0x8f476d41),
            (165, 0x23ef0516),
            (166, 0xb45f5ae5),
            (167, 0x5d1cf92f),
            (168, 0x670c7d7a),
            (169, 0xe7a33d9c),
            (170, 0x9c510114),
            (171, 0x96cd8abd),
            (172, 0x80b790f9),
            (173, 0x381788fb),
            (174, 0x3892496e),
            (175, 0xa8923d80),
            (176, 0x28f60f91),
            (177, 0xda22abbf),
            (178, 0xc48da71e),
            (179, 0x24d7ab1c),
            (180, 0x9667136b),
            (181, 0x6c21f41b),
            (182, 0x4ab816ee),
            (183, 0xb6701907),
            (184, 0xe5e9a99),
            (185, 0xf39a5ba3),
            (186, 0x1bd5e1da),
            (187, 0xb88cdf6a),
            (188, 0x305e5cbd),
            (189, 0x4b61843e),
            (190, 0x63827f16),
            (191, 0xa913f52f),
            (192, 0x8ecfda91),
            (193, 0x6f6c487e),
            (194, 0xbe14c1c4),
            (195, 0xabc35336),
            (196, 0xfa18ba4a),
            (197, 0x210939c),
            (198, 0x6a069778),
            (199, 0x66296669),
            (200, 0x4a311f4e),
        ];
        for &(seed, hash) in cases {
            assert_eq!(outcome(random(seed)), hash, "seed {seed}");
        }
    }

    #[test]
    fn lane_layout_of_nothing_is_empty() {
        let size = lane_layout(
            &mut [],
            &mut [],
            Dir::Tb,
            &options(40.0, 16.0, 48.0),
            LANE_PAD,
        );
        assert_eq!((size.width, size.height), (0.0, 0.0));
    }
}
