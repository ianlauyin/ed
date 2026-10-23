//! Lays out nested groups, each in its own direction, on top of the layered layout.
//! Port of Pele's `src/layout/compound.ts`.
//!
//! Each group is laid out on its own and placed in its parent as a single box. An edge that
//! crosses a group's border passes through a port on the side facing its other end.

use std::collections::BTreeMap;

use crate::layout::layered::Kind;
use crate::layout::layered::LEdge;
use crate::layout::layered::LNode;
use crate::layout::layered::LayeredOptions;
use crate::layout::layered::layered;
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
/// `sized`, every node is a point, which is enough to rank them.
fn build(
    levels: &[Level],
    level_of: &[Option<usize>],
    q: usize,
    nodes: &[CNode],
    edges: &[CEdge],
    local: &mut [usize],
    sized: bool,
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
            lnodes.push(lnode(0.0, 0.0, kind));
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
                level.ports.push(PortEdge { e: ei, inner, out, px: 0.0, py: 0.0, route: Vec::new(), side: Side::Top });
            }
        }
    }

    let mut local = vec![0usize; n];

    // Top-down: rank each level to learn which side of a group every crossing edge leaves from.
    for q in 0..levels.len() {
        for k in 0..levels[q].ports.len() {
            let port = &levels[q].ports[k];
            let (inner, e, out, side) = (port.inner, port.e, port.out, port.side);
            if nodes[inner].is_group && endpoint_below(inner, &edges[e], out) {
                set_side(&mut levels, &level_of, inner, e, out, side);
            }
        }
        if levels[q].edges.is_empty() {
            continue;
        }
        let needed = levels[q].edges.iter().any(|le| {
            let e = &edges[le.e];
            (nodes[le.a].is_group && e.src != le.a) || (nodes[le.b].is_group && e.dst != le.b)
        });
        if !needed {
            continue;
        }
        let (mut lnodes, mut ledges, _) = build(&levels, &level_of, q, nodes, edges, &mut local, false);
        orient(&lnodes, &mut ledges);
        rank(&mut lnodes, &ledges, 1);
        let start = start_side(levels[q].dir);
        let count = levels[q].edges.len();
        for (i, ledge) in ledges.iter().enumerate().take(count) {
            let LevelEdge { a, b, e: ei } = levels[q].edges[i];
            let e = &edges[ei];
            let a_above = !ledge.reversed;
            if nodes[a].is_group && e.src != a {
                let side = if a_above { opposite(start) } else { start };
                set_side(&mut levels, &level_of, a, ei, true, side);
            }
            if nodes[b].is_group && e.dst != b {
                let side = if a_above { start } else { opposite(start) };
                set_side(&mut levels, &level_of, b, ei, false, side);
            }
        }
    }

    // Bottom-up: lay out each level now that the sizes of its groups are known.
    let mut routes: Vec<Vec<f64>> = vec![Vec::new(); edges.len()];
    let mut total = CompoundResult::default();
    for q in (0..levels.len()).rev() {
        let group = levels[q].id;
        let dir = levels[q].dir;
        let vertical = matches!(dir, Dir::Tb | Dir::Bt);
        let flip = matches!(dir, Dir::Bt | Dir::Rl);
        let (mut lnodes, mut ledges, dummies) = build(&levels, &level_of, q, nodes, edges, &mut local, true);
        let size = layered(&mut lnodes, &mut ledges, opt);
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

        for (i, &item) in levels[q].items.iter().enumerate() {
            nodes[item].x = tx(lnodes[i].x, lnodes[i].y);
            nodes[item].y = ty(lnodes[i].x, lnodes[i].y);
        }
        for (i, le) in levels[q].edges.iter().enumerate() {
            let edge = &ledges[i];
            routes[le.e] = convert(&edge.points);
            let e = &mut edges[le.e];
            if e.label_w > 0.0 {
                e.label_x = tx(edge.label_x, edge.label_y);
                e.label_y = ty(edge.label_x, edge.label_y);
            }
        }
        let mut k = levels[q].edges.len();
        for (i, dummy) in dummies.iter().enumerate() {
            let port = &levels[q].ports[i];
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
                let (bx, by) = border(route.get(at).copied().unwrap_or(0.0), route.get(at + 1).copied().unwrap_or(0.0));
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
                    if let Some(p) = port_of(&levels, &level_of, inner, ei, out) {
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
            let port = &mut levels[q].ports[i];
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
    for q in 0..levels.len() {
        let (ox, oy) = (levels[q].x, levels[q].y);
        for k in 0..levels[q].items.len() {
            let item = levels[q].items[k];
            let node = &mut nodes[item];
            node.x += ox;
            node.y += oy;
            if node.is_group
                && let Some(child) = level_of[item]
            {
                let (x, y) = (node.x - node.w / 2.0, node.y - node.h / 2.0);
                levels[child].x = x;
                levels[child].y = y;
            }
        }
    }

    let mut outward = Vec::new();
    let mut inward = Vec::new();
    for level in &levels {
        for le in &level.edges {
            let e = &mut edges[le.e];
            let mut route = Vec::new();
            outward.clear();
            let mut item = e.src;
            while let Some(p) = parent_of(nodes, item)
                && Some(p) != level.id
            {
                outward.extend(level_of[p]);
                item = p;
            }
            for &lv in &outward {
                let lv = &levels[lv];
                if let Some(&k) = lv.port_of.get(&port_key(le.e, true)) {
                    append(&mut route, &lv.ports[k].route, lv.x, lv.y);
                }
            }
            append(&mut route, &routes[le.e], level.x, level.y);
            inward.clear();
            let mut item = e.dst;
            while let Some(p) = parent_of(nodes, item)
                && Some(p) != level.id
            {
                inward.extend(level_of[p]);
                item = p;
            }
            for &lv in inward.iter().rev() {
                let lv = &levels[lv];
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
