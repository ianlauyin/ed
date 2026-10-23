//! Lays out nested groups, each in its own direction, on top of the layered layout.
//! Port of Pele's `src/layout/compound.ts`.
//!
//! Each group is laid out on its own and placed in its parent as a single box. An edge that
//! crosses a group's border passes through a port on the side facing its other end.

use std::collections::BTreeMap;

use crate::layout::layered::{
    Kind, LEdge, LNode, LayeredOptions, layered, ledge, lnode, orient, rank,
};

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
            let (w, h) = if vertical {
                (node.w, node.h)
            } else {
                (node.h, node.w)
            };
            lnode(w, h, Kind::Node)
        } else {
            lnode(0.0, 0.0, Kind::Node)
        };
        ln.span = if node.is_group {
            node.span.min(ln.w - 2.0 * node.pad_x)
        } else {
            node.span
        };
        lnodes.push(ln);
    }
    let held = |item: usize, ei: usize, out: bool| {
        nodes[item].is_group && endpoint_below(item, &edges[ei], out)
    };
    let offset = |item: usize, ei: usize, out: bool| {
        if !sized || !held(item, ei, out) {
            return 0.0;
        }
        port_of(levels, level_of, item, ei, out).map_or(0.0, |port| {
            if vertical {
                port.px - nodes[item].w / 2.0
            } else {
                port.py - nodes[item].h / 2.0
            }
        })
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
            let kind = if port.side == start {
                Kind::StartPort
            } else {
                Kind::EndPort
            };
            lnodes.push(lnode(0.0, 0.0, kind));
            dummies[i] = Some(d);
            let inner = local[port.inner];
            let mut edge = if port.out {
                ledge(inner, d, 1, 0.0, 0.0)
            } else {
                ledge(d, inner, 1, 0.0, 0.0)
            };
            if port.out {
                edge.tail_dx = offset(port.inner, port.e, true);
                edge.tail_fixed = held(port.inner, port.e, true);
            } else {
                edge.head_dx = offset(port.inner, port.e, false);
                edge.head_fixed = held(port.inner, port.e, false);
            }
            ledges.push(edge);
        } else if let Some(ln) = lnodes.get_mut(local[port.inner]) {
            ln.pin = if matches!(port.side, Side::Top | Side::Left) {
                -1.0
            } else {
                1.0
            };
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
    descend(
        &mut levels,
        &mut level_of,
        &mut depth,
        &mut placed,
        nodes,
        &children,
        0,
    );
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
            descend(
                &mut levels,
                &mut level_of,
                &mut depth,
                &mut placed,
                nodes,
                &children,
                from,
            );
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
            nodes[a]
                .seq
                .partial_cmp(&nodes[b].seq)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
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
        let (mut lnodes, mut ledges, _) =
            build(&levels, &level_of, q, nodes, edges, &mut local, false);
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
        let (mut lnodes, mut ledges, dummies) =
            build(&levels, &level_of, q, nodes, edges, &mut local, true);
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
                let at = if out {
                    route.len().saturating_sub(3)
                } else {
                    0
                };
                let (bx, by) = border(
                    route.get(at).copied().unwrap_or(0.0),
                    route.get(at + 1).copied().unwrap_or(0.0),
                );
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
                let a = if matches!(side, Side::Top | Side::Bottom) {
                    0.0
                } else {
                    1.0
                };
                let route = if out {
                    vec![ix, iy, a, bx, by, a]
                } else {
                    vec![bx, by, a, ix, iy, a]
                };
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
            total = CompoundResult {
                height: box_h,
                width: box_w,
            };
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
fn set_side(
    levels: &mut [Level],
    level_of: &[Option<usize>],
    item: usize,
    e: usize,
    out: bool,
    side: Side,
) {
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
#[expect(
    clippy::unreadable_literal,
    reason = "fixtures print Pele's numbers exactly and name ends as Pele does"
)]
mod tests {
    //! End-to-end checks against Pele. Every expected number comes from running Pele's TypeScript
    //! (`obsidianmd/pele` at `458322a`) on the same input with `npx tsx`, never from this port.
    //! Large graphs compare checksums that both sides compute in the same order.

    use super::*;

    /// `w, h, parent, is_group, dir, pad_x, pad_top, pad_bottom, min_w, seq, span`.
    type N = (
        f64,
        f64,
        i32,
        bool,
        Option<Dir>,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    );
    /// `src, dst, minlen, label_w, label_h`.
    type E = (usize, usize, u32, f64, f64);

    /// An edge as Pele lays it out.
    struct Want {
        dst: usize,
        label: (f64, f64),
        route: &'static [f64],
        src: usize,
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

    fn check(
        dir: Dir,
        nodes: &[N],
        edges: &[E],
        opt: &LayeredOptions,
        size: (f64, f64),
        want_nodes: &[(f64, f64, f64, f64, i32)],
        want_edges: &[Want],
    ) {
        let mut cn: Vec<CNode> = nodes
            .iter()
            .map(
                |&(w, h, parent, is_group, dir, pad_x, pad_top, pad_bottom, min_w, seq, span)| {
                    CNode {
                        dir,
                        is_group,
                        min_w,
                        pad_bottom,
                        pad_x,
                        pad_top,
                        seq,
                        span,
                        ..cnode(w, h, parent)
                    }
                },
            )
            .collect();
        let mut ce: Vec<CEdge> = edges
            .iter()
            .map(|&(src, dst, minlen, label_w, label_h)| CEdge {
                dst,
                label_h,
                label_w,
                minlen,
                src,
                ..CEdge::default()
            })
            .collect();
        let got = compound_layout(&mut cn, &mut ce, dir, opt);
        close(got.width, size.0, "width");
        close(got.height, size.1, "height");
        for (i, (node, want)) in cn.iter().zip(want_nodes).enumerate() {
            close(node.x, want.0, &format!("node {i} x"));
            close(node.y, want.1, &format!("node {i} y"));
            close(node.w, want.2, &format!("node {i} w"));
            close(node.h, want.3, &format!("node {i} h"));
            assert_eq!(node.parent, want.4, "node {i} parent");
        }
        for (i, (edge, want)) in ce.iter().zip(want_edges).enumerate() {
            assert_eq!((edge.src, edge.dst), (want.src, want.dst), "edge {i} ends");
            assert_eq!(
                edge.route.len(),
                want.route.len(),
                "edge {i} route {:?}",
                edge.route
            );
            for (k, (&p, &q)) in edge.route.iter().zip(want.route).enumerate() {
                close(p, q, &format!("edge {i} route {k}"));
            }
            close(edge.label_x, want.label.0, &format!("edge {i} label x"));
            close(edge.label_y, want.label.1, &format!("edge {i} label y"));
        }
    }

    #[test]
    fn shift() {
        let mut nodes = vec![cnode(10.0, 10.0, -1)];
        let mut edges = vec![CEdge {
            label_x: 5.0,
            label_y: 6.0,
            route: vec![1.0, 2.0, 0.0, 3.0, 4.0, 1.0],
            ..CEdge::default()
        }];
        shift_layout(&mut nodes, &mut edges, 10.0, 20.0);
        assert_eq!((nodes[0].x, nodes[0].y), (10.0, 20.0));
        assert_eq!(edges[0].route, [11.0, 22.0, 0.0, 13.0, 24.0, 1.0]);
        assert_eq!((edges[0].label_x, edges[0].label_y), (15.0, 26.0));
    }

    #[test]
    fn directions() {
        assert_eq!(direction(Some("BT")), Dir::Bt);
        assert_eq!(direction(Some("LR")), Dir::Lr);
        assert_eq!(direction(Some("RL")), Dir::Rl);
        assert_eq!(direction(Some("TD")), Dir::Tb);
        assert_eq!(direction(None), Dir::Tb);
    }

    #[test]
    fn flat() {
        check(
            Dir::Tb,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 1, 1, 30.0, 14.0),
                (1, 2, 1, 0.0, 0.0),
                (0, 2, 1, 0.0, 0.0),
            ],
            &OPT,
            (78.0, 200.0),
            &[
                (51.5, 15.0, 50.0, 30.0, -1),
                (25.0, 107.0, 50.0, 30.0, -1),
                (51.5, 185.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 1,
                    label: (25.0, 58.0),
                    route: &[
                        43.5, 30.0, 0.0, 25.0, 51.0, 0.0, 25.0, 65.0, 0.0, 25.0, 92.0, 0.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[25.0, 122.0, 0.0, 43.5, 170.0, 0.0],
                    src: 1,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[
                        59.5, 30.0, 0.0, 78.0, 92.0, 0.0, 78.0, 122.0, 0.0, 59.5, 170.0, 0.0,
                    ],
                    src: 0,
                },
            ],
        );
    }

    #[test]
    fn flat_lr() {
        check(
            Dir::Lr,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (80.0, 40.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 1, 1, 30.0, 14.0),
                (1, 2, 1, 0.0, 0.0),
                (0, 2, 1, 0.0, 0.0),
            ],
            &OPT,
            (306.0, 68.0),
            &[
                (25.0, 44.0, 50.0, 30.0, -1),
                (168.0, 20.0, 80.0, 40.0, -1),
                (281.0, 44.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 1,
                    label: (86.0, 20.0),
                    route: &[
                        50.0, 36.0, 1.0, 71.0, 20.0, 1.0, 101.0, 20.0, 1.0, 128.0, 20.0, 1.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[208.0, 20.0, 1.0, 256.0, 36.0, 1.0],
                    src: 1,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[
                        50.0, 52.0, 1.0, 128.0, 68.0, 1.0, 208.0, 68.0, 1.0, 256.0, 52.0, 1.0,
                    ],
                    src: 0,
                },
            ],
        );
    }

    #[test]
    fn flat_rl() {
        check(
            Dir::Rl,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[(0, 1, 1, 0.0, 0.0), (1, 2, 1, 30.0, 14.0)],
            &OPT,
            (276.0, 30.0),
            &[
                (251.0, 15.0, 50.0, 30.0, -1),
                (153.0, 15.0, 50.0, 30.0, -1),
                (25.0, 15.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 1,
                    label: (0.0, 0.0),
                    route: &[226.0, 15.0, 1.0, 178.0, 15.0, 1.0],
                    src: 0,
                },
                Want {
                    dst: 2,
                    label: (92.0, 15.0),
                    route: &[
                        128.0, 15.0, 1.0, 107.0, 15.0, 1.0, 77.0, 15.0, 1.0, 50.0, 15.0, 1.0,
                    ],
                    src: 1,
                },
            ],
        );
    }

    #[test]
    fn flat_bt() {
        check(
            Dir::Bt,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[(0, 1, 1, 0.0, 0.0), (0, 2, 1, 30.0, 14.0)],
            &OPT,
            (140.0, 122.0),
            &[
                (70.0, 107.0, 50.0, 30.0, -1),
                (25.0, 15.0, 50.0, 30.0, -1),
                (115.0, 15.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 1,
                    label: (0.0, 0.0),
                    route: &[62.0, 92.0, 0.0, 25.0, 30.0, 0.0],
                    src: 0,
                },
                Want {
                    dst: 2,
                    label: (115.0, 64.0),
                    route: &[
                        78.0, 92.0, 0.0, 115.0, 71.0, 0.0, 115.0, 57.0, 0.0, 115.0, 30.0, 0.0,
                    ],
                    src: 0,
                },
            ],
        );
    }

    #[test]
    fn group() {
        check(
            Dir::Tb,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 2, 1, 0.0, 0.0),
                (2, 3, 1, 0.0, 0.0),
                (3, 4, 1, 0.0, 0.0),
                (0, 4, 1, 0.0, 0.0),
            ],
            &OPT,
            (102.0, 346.0),
            &[
                (69.5, 15.0, 50.0, 30.0, -1),
                (37.0, 173.0, 74.0, 190.0, -1),
                (37.0, 143.0, 50.0, 30.0, 1),
                (37.0, 221.0, 50.0, 30.0, 1),
                (69.5, 331.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[
                        61.5, 30.0, 0.0, 37.0, 78.0, 0.0, 37.0, 108.0, 0.0, 37.0, 128.0, 0.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 3,
                    label: (0.0, 0.0),
                    route: &[37.0, 158.0, 0.0, 37.0, 206.0, 0.0],
                    src: 2,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[
                        37.0, 236.0, 0.0, 37.0, 256.0, 0.0, 37.0, 268.0, 0.0, 61.5, 316.0, 0.0,
                    ],
                    src: 3,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[
                        77.5, 30.0, 0.0, 102.0, 78.0, 0.0, 102.0, 268.0, 0.0, 77.5, 316.0, 0.0,
                    ],
                    src: 0,
                },
            ],
        );
    }

    #[test]
    fn group_lr_inside() {
        check(
            Dir::Tb,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    Some(Dir::Lr),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 2, 1, 0.0, 0.0),
                (2, 3, 1, 30.0, 14.0),
                (3, 4, 1, 0.0, 0.0),
                (4, 2, 1, 0.0, 0.0),
            ],
            &OPT,
            (202.0, 228.0),
            &[
                (37.0, 15.0, 50.0, 30.0, -1),
                (101.0, 114.0, 202.0, 72.0, -1),
                (37.0, 123.0, 50.0, 30.0, 1),
                (165.0, 123.0, 50.0, 30.0, 1),
                (101.0, 213.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[37.0, 30.0, 0.0, 37.0, 78.0, 0.0, 37.0, 108.0, 0.0],
                    src: 0,
                },
                Want {
                    dst: 3,
                    label: (98.0, 123.0),
                    route: &[
                        62.0, 123.0, 1.0, 83.0, 123.0, 1.0, 113.0, 123.0, 1.0, 140.0, 123.0, 1.0,
                    ],
                    src: 2,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[165.0, 138.0, 0.0, 165.0, 150.0, 0.0, 109.0, 198.0, 0.0],
                    src: 3,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[93.0, 198.0, 0.0, 37.0, 150.0, 0.0, 37.0, 138.0, 0.0],
                    src: 4,
                },
            ],
        );
    }

    #[test]
    fn nested_mixed() {
        check(
            Dir::Lr,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    Some(Dir::Tb),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (
                    0.0,
                    0.0,
                    1,
                    true,
                    Some(Dir::Rl),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 2, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 2, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    Some(Dir::Bt),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 7, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 7, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 3, 1, 0.0, 0.0),
                (3, 4, 1, 0.0, 0.0),
                (4, 5, 1, 0.0, 0.0),
                (5, 6, 1, 0.0, 0.0),
                (0, 2, 1, 0.0, 0.0),
                (2, 6, 1, 30.0, 14.0),
                (1, 7, 1, 0.0, 0.0),
                (8, 9, 1, 0.0, 0.0),
                (4, 8, 1, 0.0, 0.0),
                (9, 0, 1, 0.0, 0.0),
                (6, 1, 1, 0.0, 0.0),
            ],
            &OPT,
            (533.0, 262.5),
            &[
                (25.0, 55.25, 50.0, 30.0, -1),
                (216.0, 151.0, 236.0, 223.0, -1),
                (216.0, 121.0, 212.0, 103.0, 1),
                (265.0, 114.5, 50.0, 30.0, 2),
                (167.0, 145.5, 50.0, 30.0, 2),
                (167.0, 235.5, 50.0, 30.0, 1),
                (484.0, 220.5, 50.0, 30.0, -1),
                (496.0, 90.5, 74.0, 150.0, -1),
                (496.0, 138.5, 50.0, 30.0, 7),
                (496.0, 60.5, 50.0, 30.0, 7),
            ],
            &[
                Want {
                    dst: 3,
                    label: (0.0, 0.0),
                    route: &[
                        50.0, 55.25, 1.0, 98.0, 102.5, 1.0, 110.0, 102.5, 1.0, 122.0, 102.5, 1.0,
                        142.0, 102.5, 1.0, 192.0, 102.5, 1.0, 240.0, 102.5, 1.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[240.0, 118.5, 1.0, 192.0, 141.5, 1.0],
                    src: 3,
                },
                Want {
                    dst: 5,
                    label: (0.0, 0.0),
                    route: &[167.0, 160.5, 0.0, 167.0, 172.5, 0.0, 167.0, 220.5, 0.0],
                    src: 4,
                },
                Want {
                    dst: 6,
                    label: (0.0, 0.0),
                    route: &[192.0, 235.5, 1.0, 334.0, 235.5, 1.0, 459.0, 235.5, 1.0],
                    src: 5,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[50.0, 70.25, 1.0, 98.0, 121.0, 1.0, 110.0, 121.0, 1.0],
                    src: 0,
                },
                Want {
                    dst: 6,
                    label: (417.0, 74.25),
                    route: &[
                        322.0, 121.0, 1.0, 334.0, 121.0, 1.0, 362.0, 121.0, 1.0, 362.0, 74.25, 1.0,
                        402.0, 74.25, 1.0, 432.0, 74.25, 1.0, 459.0, 205.5, 1.0,
                    ],
                    src: 2,
                },
                Want {
                    dst: 7,
                    label: (0.0, 0.0),
                    route: &[
                        334.0, 131.25, 1.0, 362.0, 131.25, 1.0, 409.5, 90.5, 1.0, 424.5, 90.5, 1.0,
                        459.0, 90.5, 1.0,
                    ],
                    src: 1,
                },
                Want {
                    dst: 9,
                    label: (0.0, 0.0),
                    route: &[496.0, 123.5, 0.0, 496.0, 75.5, 0.0],
                    src: 8,
                },
                Want {
                    dst: 8,
                    label: (0.0, 0.0),
                    route: &[
                        192.0, 157.5, 1.0, 240.0, 157.5, 1.0, 290.0, 157.5, 1.0, 310.0, 157.5, 1.0,
                        322.0, 157.5, 1.0, 334.0, 157.5, 1.0, 459.0, 138.5, 1.0, 471.0, 138.5, 1.0,
                    ],
                    src: 4,
                },
                Want {
                    dst: 0,
                    label: (0.0, 0.0),
                    route: &[
                        471.0, 60.5, 1.0, 459.0, 60.5, 1.0, 424.5, 0.0, 1.0, 409.5, 0.0, 1.0,
                        334.0, 0.0, 1.0, 98.0, 0.0, 1.0, 50.0, 40.25, 1.0,
                    ],
                    src: 9,
                },
                Want {
                    dst: 1,
                    label: (0.0, 0.0),
                    route: &[459.0, 220.5, 1.0, 334.0, 147.25, 1.0],
                    src: 6,
                },
            ],
        );
    }

    #[test]
    fn group_edges() {
        check(
            Dir::Tb,
            &[
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 0, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 0, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 3, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 3, 1, 0.0, 0.0),
                (1, 4, 1, 0.0, 0.0),
                (5, 0, 1, 0.0, 0.0),
                (3, 6, 1, 20.0, 14.0),
                (2, 6, 1, 0.0, 0.0),
                (6, 1, 1, 0.0, 0.0),
            ],
            &OPT,
            (176.0, 402.0),
            &[
                (94.0, 124.0, 164.0, 92.0, -1),
                (49.0, 123.0, 50.0, 30.0, 0),
                (139.0, 123.0, 50.0, 30.0, 0),
                (37.0, 264.0, 74.0, 92.0, -1),
                (37.0, 283.0, 50.0, 30.0, 3),
                (94.0, 15.0, 50.0, 30.0, -1),
                (102.33333333333334, 387.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 3,
                    label: (0.0, 0.0),
                    route: &[106.0, 170.0, 0.0, 53.0, 218.0, 0.0],
                    src: 0,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[
                        37.0, 138.0, 0.0, 37.0, 158.0, 0.0, 37.0, 170.0, 0.0, 37.0, 218.0, 0.0,
                        37.0, 248.0, 0.0, 37.0, 268.0, 0.0,
                    ],
                    src: 1,
                },
                Want {
                    dst: 0,
                    label: (0.0, 0.0),
                    route: &[94.0, 30.0, 0.0, 94.0, 78.0, 0.0],
                    src: 5,
                },
                Want {
                    dst: 6,
                    label: (37.0, 338.0),
                    route: &[
                        37.0,
                        310.0,
                        0.0,
                        37.0,
                        331.0,
                        0.0,
                        37.0,
                        345.0,
                        0.0,
                        88.50000000000001,
                        372.0,
                        0.0,
                    ],
                    src: 3,
                },
                Want {
                    dst: 6,
                    label: (0.0, 0.0),
                    route: &[
                        139.0, 138.0, 0.0, 139.0, 158.0, 0.0, 139.0, 170.0, 0.0, 139.0, 218.0, 0.0,
                        139.0, 310.0, 0.0, 120.5, 372.0, 0.0,
                    ],
                    src: 2,
                },
                Want {
                    dst: 1,
                    label: (0.0, 0.0),
                    route: &[
                        104.50000000000001,
                        372.0,
                        0.0,
                        104.5,
                        310.0,
                        0.0,
                        104.5,
                        218.0,
                        0.0,
                        90.0,
                        170.0,
                        0.0,
                        90.0,
                        158.0,
                        0.0,
                        53.0,
                        138.0,
                        0.0,
                    ],
                    src: 6,
                },
            ],
        );
    }

    #[test]
    fn stray_loops_cycles() {
        check(
            Dir::Tb,
            &[
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 0, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 0, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    4,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (
                    0.0,
                    0.0,
                    3,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 3, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 99, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 1, 1, 0.0, 0.0),
                (1, 1, 1, 20.0, 14.0),
                (1, 2, 1, 0.0, 0.0),
                (5, 2, 1, 0.0, 0.0),
                (6, 7, 1, 0.0, 0.0),
                (7, 1, 1, 0.0, 0.0),
            ],
            &OPT,
            (228.0, 400.0),
            &[
                (164.5, 315.0, 102.0, 170.0, -1),
                (178.5, 295.0, 50.0, 30.0, 0),
                (152.0, 373.0, 50.0, 30.0, 0),
                (69.0, 130.0, 138.0, 104.0, -1),
                (24.0, 129.0, 24.0, 42.0, 3),
                (101.0, 135.0, 50.0, 30.0, 3),
                (203.0, 15.0, 50.0, 30.0, -1),
                (203.0, 130.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 1,
                    label: (171.5, 305.0),
                    route: &[164.5, 315.0, 0.0, 178.5, 295.0, 0.0],
                    src: 0,
                },
                Want {
                    dst: 1,
                    label: (241.5, 295.0),
                    route: &[],
                    src: 1,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[178.5, 310.0, 0.0, 160.0, 358.0, 0.0],
                    src: 1,
                },
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[
                        101.0, 150.0, 0.0, 101.0, 170.0, 0.0, 101.0, 182.0, 0.0, 125.5, 230.0, 0.0,
                        125.5, 260.0, 0.0, 125.5, 280.0, 0.0, 125.5, 310.0, 0.0, 144.0, 358.0, 0.0,
                    ],
                    src: 5,
                },
                Want {
                    dst: 7,
                    label: (0.0, 0.0),
                    route: &[203.0, 30.0, 0.0, 203.0, 115.0, 0.0],
                    src: 6,
                },
                Want {
                    dst: 1,
                    label: (0.0, 0.0),
                    route: &[
                        203.0, 145.0, 0.0, 178.5, 230.0, 0.0, 178.5, 260.0, 0.0, 178.5, 280.0, 0.0,
                    ],
                    src: 7,
                },
            ],
        );
    }

    #[test]
    fn padding_seq() {
        check(
            Dir::Tb,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 5.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    30.0,
                    40.0,
                    20.0,
                    400.0,
                    9.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 3.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 1.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 2.0, 40.0),
            ],
            &[
                (0, 2, 1, 0.0, 0.0),
                (3, 4, 1, 0.0, 0.0),
                (2, 3, 1, 0.0, 0.0),
            ],
            &OPT,
            (400.0, 364.0),
            &[
                (200.0, 15.0, 50.0, 30.0, -1),
                (200.0, 182.0, 400.0, 208.0, -1),
                (200.0, 153.0, 50.0, 30.0, 1),
                (200.0, 231.0, 50.0, 30.0, 1),
                (200.0, 349.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[
                        200.0, 30.0, 0.0, 200.0, 78.0, 0.0, 200.0, 118.0, 0.0, 200.0, 138.0, 0.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[
                        200.0, 246.0, 0.0, 200.0, 266.0, 0.0, 200.0, 286.0, 0.0, 200.0, 334.0, 0.0,
                    ],
                    src: 3,
                },
                Want {
                    dst: 3,
                    label: (0.0, 0.0),
                    route: &[200.0, 168.0, 0.0, 200.0, 216.0, 0.0],
                    src: 2,
                },
            ],
        );
    }

    #[test]
    fn group_tracks() {
        check(
            Dir::Tb,
            &[
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (0, 2, 1, 0.0, 0.0),
                (0, 3, 1, 0.0, 0.0),
                (0, 4, 1, 0.0, 0.0),
                (2, 5, 1, 0.0, 0.0),
                (3, 6, 1, 0.0, 0.0),
                (4, 5, 1, 0.0, 0.0),
                (5, 6, 1, 0.0, 0.0),
            ],
            &LayeredOptions {
                tracks: true,
                ..OPT
            },
            (254.0, 346.0),
            &[
                (127.0, 15.0, 50.0, 30.0, -1),
                (127.0, 134.0, 254.0, 112.0, -1),
                (37.0, 143.0, 50.0, 30.0, 1),
                (127.0, 143.0, 50.0, 30.0, 1),
                (217.0, 143.0, 50.0, 30.0, 1),
                (74.0, 253.0, 50.0, 30.0, -1),
                (100.5, 331.0, 50.0, 30.0, -1),
            ],
            &[
                Want {
                    dst: 2,
                    label: (0.0, 0.0),
                    route: &[
                        111.0, 30.0, 0.0, 111.0, 54.0, 0.0, 37.0, 54.0, 0.0, 37.0, 78.0, 0.0, 37.0,
                        108.0, 0.0, 37.0, 128.0, 0.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 3,
                    label: (0.0, 0.0),
                    route: &[
                        127.0, 30.0, 0.0, 127.0, 78.0, 0.0, 127.0, 108.0, 0.0, 127.0, 128.0, 0.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[
                        143.0, 30.0, 0.0, 143.0, 54.0, 0.0, 217.0, 54.0, 0.0, 217.0, 78.0, 0.0,
                        217.0, 108.0, 0.0, 217.0, 128.0, 0.0,
                    ],
                    src: 0,
                },
                Want {
                    dst: 5,
                    label: (0.0, 0.0),
                    route: &[
                        37.0, 158.0, 0.0, 37.0, 178.0, 0.0, 37.0, 190.0, 0.0, 37.0, 214.0, 0.0,
                        66.0, 214.0, 0.0, 66.0, 238.0, 0.0,
                    ],
                    src: 2,
                },
                Want {
                    dst: 6,
                    label: (0.0, 0.0),
                    route: &[
                        127.0, 158.0, 0.0, 127.0, 178.0, 0.0, 127.0, 190.0, 0.0, 127.0, 238.0, 0.0,
                        127.0, 268.0, 0.0, 127.0, 292.0, 0.0, 108.5, 292.0, 0.0, 108.5, 316.0, 0.0,
                    ],
                    src: 3,
                },
                Want {
                    dst: 5,
                    label: (0.0, 0.0),
                    route: &[
                        217.0, 158.0, 0.0, 217.0, 178.0, 0.0, 217.0, 190.0, 0.0, 217.0, 214.0, 0.0,
                        82.0, 214.0, 0.0, 82.0, 238.0, 0.0,
                    ],
                    src: 4,
                },
                Want {
                    dst: 6,
                    label: (0.0, 0.0),
                    route: &[
                        74.0, 268.0, 0.0, 74.0, 292.0, 0.0, 92.5, 292.0, 0.0, 92.5, 316.0, 0.0,
                    ],
                    src: 5,
                },
            ],
        );
    }

    #[test]
    fn deep_crossing() {
        check(
            Dir::Tb,
            &[
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (
                    0.0,
                    0.0,
                    0,
                    true,
                    Some(Dir::Lr),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (
                    0.0,
                    0.0,
                    1,
                    true,
                    Some(Dir::Tb),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 2, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, -1, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (
                    0.0,
                    0.0,
                    -1,
                    true,
                    Some(Dir::Rl),
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (
                    0.0,
                    0.0,
                    5,
                    true,
                    None,
                    12.0,
                    30.0,
                    12.0,
                    0.0,
                    0.0,
                    f64::INFINITY,
                ),
                (50.0, 30.0, 6, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
                (50.0, 30.0, 6, false, None, 0.0, 0.0, 0.0, 0.0, 0.0, 40.0),
            ],
            &[
                (3, 4, 1, 0.0, 0.0),
                (4, 7, 1, 0.0, 0.0),
                (3, 8, 1, 0.0, 0.0),
                (7, 3, 1, 0.0, 0.0),
                (8, 7, 1, 0.0, 0.0),
                (2, 6, 1, 0.0, 0.0),
            ],
            &OPT,
            (196.0, 436.0),
            &[
                (84.0, 98.0, 122.0, 196.0, -1),
                (84.0, 97.0, 98.0, 134.0, 0),
                (84.0, 106.0, 74.0, 92.0, 1),
                (84.0, 105.0, 50.0, 30.0, 2),
                (31.0, 259.0, 50.0, 30.0, -1),
                (98.0, 379.0, 196.0, 114.0, -1),
                (98.0, 388.0, 172.0, 72.0, 5),
                (49.0, 397.0, 50.0, 30.0, 6),
                (147.0, 397.0, 50.0, 30.0, 6),
            ],
            &[
                Want {
                    dst: 4,
                    label: (0.0, 0.0),
                    route: &[
                        68.0, 120.0, 0.0, 68.0, 140.0, 0.0, 68.0, 152.0, 0.0, 68.0, 164.0, 0.0,
                        68.0, 184.0, 0.0, 68.0, 196.0, 0.0, 31.0, 244.0, 0.0,
                    ],
                    src: 3,
                },
                Want {
                    dst: 7,
                    label: (0.0, 0.0),
                    route: &[
                        31.0, 274.0, 0.0, 49.0, 322.0, 0.0, 49.0, 352.0, 0.0, 49.0, 382.0, 0.0,
                    ],
                    src: 4,
                },
                Want {
                    dst: 8,
                    label: (0.0, 0.0),
                    route: &[
                        84.0, 120.0, 0.0, 84.0, 140.0, 0.0, 84.0, 152.0, 0.0, 84.0, 164.0, 0.0,
                        84.0, 184.0, 0.0, 84.0, 196.0, 0.0, 84.0, 244.0, 0.0, 84.0, 274.0, 0.0,
                        147.0, 322.0, 0.0, 147.0, 352.0, 0.0, 147.0, 382.0, 0.0,
                    ],
                    src: 3,
                },
                Want {
                    dst: 3,
                    label: (0.0, 0.0),
                    route: &[
                        49.0, 382.0, 0.0, 49.0, 352.0, 0.0, 49.0, 322.0, 0.0, 100.0, 274.0, 0.0,
                        100.0, 244.0, 0.0, 116.0, 196.0, 0.0, 116.0, 184.0, 0.0, 100.0, 164.0, 0.0,
                        100.0, 152.0, 0.0, 100.0, 140.0, 0.0, 100.0, 120.0, 0.0,
                    ],
                    src: 7,
                },
                Want {
                    dst: 7,
                    label: (0.0, 0.0),
                    route: &[122.0, 397.0, 1.0, 74.0, 397.0, 1.0],
                    src: 8,
                },
                Want {
                    dst: 6,
                    label: (0.0, 0.0),
                    route: &[
                        84.0, 152.0, 0.0, 84.0, 164.0, 0.0, 100.0, 184.0, 0.0, 100.0, 196.0, 0.0,
                        116.0, 244.0, 0.0, 116.0, 274.0, 0.0, 98.0, 322.0, 0.0, 98.0, 352.0, 0.0,
                    ],
                    src: 2,
                },
            ],
        );
    }
}
