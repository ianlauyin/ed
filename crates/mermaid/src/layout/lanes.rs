//! Layout for swimlanes. Port of Pele's `src/layout/lanes.ts`.
//!
//! Every top-level group is a lane: a band that runs the length of the diagram along the flow.
//! Ranks are shared by all lanes, so a step sits level with the steps that happen at the same
//! time in other lanes, and each node stays inside its own lane. Arrays are sized from the
//! validated graph, so their indices are in bounds by construction, as in the layered layout.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::layout::compound::CEdge;
use crate::layout::compound::CNode;
use crate::layout::compound::CompoundResult;
use crate::layout::compound::Dir;
use crate::layout::layered::Kind;
use crate::layout::layered::LEdge;
use crate::layout::layered::LNode;
use crate::layout::layered::LayeredOptions;
use crate::layout::layered::ledge;
use crate::layout::layered::lnode;
use crate::layout::layered::orient;
use crate::layout::layered::rank;
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
    usize::try_from(nodes[i].parent).ok().filter(|&p| p < nodes.len())
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
    let lanes: Vec<usize> = (0..n).rev().filter(|&i| nodes[i].parent < 0 && nodes[i].is_group).collect();
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
                room += nodes[t].pad_x.max(nodes[t].pad_top).max(nodes[t].pad_bottom);
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

    let mut lnodes: Vec<LNode> = real.iter().map(|&i| lnode(cross(&nodes[i]), along(&nodes[i]), Kind::Node)).collect();
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
        let (label_w, label_h) = if sideways { (e.label_h, e.label_w) } else { (e.label_w, e.label_h) };
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
            plain.iter().zip(&placed.routed).map(|(&on, &routed)| on && routed).collect()
        };
        if next == plain && placed.margins == margins {
            break;
        }
        plain = next;
        margins = std::mem::take(&mut placed.margins);
        placed = place(&ctx, nodes, edges, &plain, &margins);
        round += 1;
    }
    if plain.iter().zip(&placed.routed).any(|(&on, &routed)| on && !routed) {
        placed = place(&ctx, nodes, edges, &vec![false; m], &vec![false; lane_count]);
    }
    CompoundResult { height: placed.height, width: placed.width }
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
#[expect(clippy::many_single_char_names, reason = "names mirror Pele's lanes.ts for side-by-side review")]
fn place(ctx: &Ctx<'_>, nodes: &mut [CNode], edges: &mut [CEdge], plain: &[bool], margins: &[bool]) -> Placed {
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
    let (end_pad, flipped, lane_count, sideways, step) = (*end_pad, *flipped, *lane_count, *sideways, *step);
    let n = nodes.len();
    let m = ledges.len();

    let pad: Vec<f64> =
        lane_pads.iter().zip(margins).map(|(&room, &margin)| if margin { room.max(MARGIN) } else { room }).collect();
    let mut used = vec![false; lane_count];

    // Items: the real ones, then a dummy for every rank an edge passes through.
    let mut dummies: i64 = 0;
    let span: Vec<i64> =
        ledges.iter().map(|e| (lnodes[e.head].rank as i64 - lnodes[e.tail].rank as i64).abs()).collect();
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
            let (a, b) = if e.reversed { (e.head, e.tail) } else { (e.tail, e.head) };
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
    for pair in links.as_chunks::<2>().0 {
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
        for pair in links.as_chunks::<2>().0 {
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
            let (start, list) = if downward { (&up_start, &up) } else { (&down_start, &down) };
            for &i in row.iter() {
                let mut sum = 0.0;
                let count = start[i + 1] - start[i];
                for k in start[i]..start[i + 1] {
                    sum += pos[list[k]];
                }
                key[i] = if count > 0 { sum / count as f64 } else { pos[i] };
            }
            settle(row, &key, &mut pos);
        }
    }

    // The items of one rank that share a lane sit side by side as a run.
    let gap = |a: usize, b: usize| {
        if dummy[a] || dummy[b] { opt.edge_sep } else { opt.node_sep }
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
            let mut at = (lane_start[at_lane] + pad[at_lane] + (lane_start[at_lane + 1] - pad[at_lane])
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
            let (start, list) = if downward { (&up_start, &up) } else { (&down_start, &down) };
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
                want[i] = if count > 0 { sum / f64::from(count) } else { c[i] };
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
            let (a, b) = if e.reversed { (e.head, e.tail) } else { (e.tail, e.head) };
            if plain[k] {
                if e.label_h <= 0.0 || span[k] > step as i64 {
                    continue;
                }
                let room = full.min((e.label_h + 2.0 * TURN - depth[a].min(depth[b]) / 2.0) / span[k] as f64);
                for s in &mut sep[rank_of[a]..rank_of[b]] {
                    if room > *s {
                        *s = room;
                    }
                }
                continue;
            }
            let Some(d0) = first_dummy[k].filter(|_| span[k] > step as i64 * i64::from(e.minlen)) else {
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
    let mut bounds = vec![[f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]; n];
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
        let (a, b) = if e.reversed { (e.head, e.tail) } else { (e.tail, e.head) };
        let (ra, rb) = (rank_of[a], rank_of[b]);
        let mut pts = vec![c[a], band_end[ra] - (band_end[ra] - band_start[ra] - depth[a]) / 2.0];
        if let Some(d0) = first_dummy[k] {
            for d in d0..d0 + span[k] as usize - 1 {
                let rd = rank_of[d];
                pts.extend([c[d], band_start[rd]]);
                if band_end[rd] > band_start[rd] {
                    pts.extend([c[d], band_end[rd]]);
                }
            }
        }
        pts.extend([c[b], band_start[rb] + (band_end[rb] - band_start[rb] - depth[b]) / 2.0]);
        let mut route = Vec::with_capacity(pts.len() / 2 * 3);
        for p in pts.as_chunks::<2>().0 {
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
        let flat = if (bx - ax).abs() > (by - ay).abs() { 1.0 } else { 0.0 };
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
            .map(|e| [if sideways { e.label_y } else { e.label_x }, flip(if sideways { e.label_x } else { e.label_y })])
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
                let (w, h) = if sideways { (e.label_h, e.label_w) } else { (e.label_w, e.label_h) };
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
            let attempt = |edges: &mut [CEdge], labels: &mut [[f64; 2]], pts: &[f64], margin: f64| {
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
                        let short = if across > 0.0 { across < cw + 8.0 } else { along < ah + 8.0 };
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
                        let bounds = (cc - cw / 2.0, ca - ah / 2.0, cc + cw / 2.0, ca + ah / 2.0);
                        if !clear(edges, labels, bounds, None, ei) {
                            continue;
                        }
                        // A label moved onto a handoff must not cover a return path that keeps its
                        // ranked route. A segment's whole box is kept clear, including its curve.
                        let (lx, ly) = (px(cc, ca), py(cc, ca));
                        if crossings(edges, lx - out_w / 2.0, ly - out_h / 2.0, lx + out_w / 2.0, ly + out_h / 2.0, ei)
                            > 0
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
                            way.extend([pts[p - 2] + (dx / run) * first, pts[p - 1] + (dy / run) * first]);
                        }
                        if last != 0.0 {
                            way.extend([pts[p] - (dx / run) * last, pts[p + 1] - (dy / run) * last]);
                        }
                    }
                    way.extend([pts[p], pts[p + 1]]);
                    p += 2;
                }
                let out = &mut edges[ei];
                out.route = Vec::with_capacity(way.len() / 2 * 3);
                let mut p = 0;
                while p + 1 < way.len() {
                    let crosswise =
                        if p == 0 { way.get(2).is_none_or(|&next| way[0] != next) } else { way[p] != way[p - 2] };
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

            if attempt(edges, &mut labels, &[ac + dc * size[a] / 2.0, aa, bc, aa, bc, ba - da * depth[b] / 2.0], 2.0)
                || attempt(
                    edges,
                    &mut labels,
                    &[ac, aa + da * depth[a] / 2.0, ac, ba, bc - dc * size[b] / 2.0, ba],
                    2.0,
                )
            {
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
            let order = if lane[a] < lane[b] { [lane[b], lane[a]] } else { [lane[a], lane[b]] };
            for at_lane in order {
                // A lane that keeps no margin yet has less room there, and the handoff stays
                // inside it.
                let inset = (pad[at_lane] - LEG).max((pad[at_lane] / 2.0).min(MARGIN - LEG));
                for ch in [lane_start[at_lane] + inset, lane_start[at_lane + 1] - inset] {
                    if (ch - ac).abs() < size[a] / 2.0 + 4.0 || (ch - bc).abs() < size[b] / 2.0 + 4.0 {
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
                        let crossed = crossings(edges, x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1), ei);
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
    let sx = if dx == 0.0 { f64::INFINITY } else { hw / dx.abs() };
    let sy = if dy == 0.0 { f64::INFINITY } else { hh / dy.abs() };
    let scale = sx.min(sy);
    (from.x + dx * scale, from.y + dy * scale)
}
