//! Grid placement of architecture nodes and groups. Port of Pele's
//! `src/diagrams/architecture/layout.ts`.
//!
//! Mermaid hands the edge sides to a force-directed solver as relative placement constraints.
//! Pele applies them directly: every service and junction takes a cell, an edge `a:R -- L:b` puts
//! b in the cell to the right of a, and a group is a solid rectangle of cells, so a group never
//! encloses a node that is not in it. Each container (the diagram, then every group) is laid out
//! on its own, innermost first, and a finished group is placed in its parent as one block.

use std::collections::BTreeMap;

use super::db::{ArchEdge, ArchGroup, ArchLayoutHint, ArchNode};
use super::parser::{Direction, Side};

/// Cells of the nodes and groups.
#[derive(Clone, Debug, Default)]
pub(crate) struct Grid {
    /// Column of each node, in the order of the nodes passed in.
    pub(crate) col: Vec<i64>,
    /// Column of each group's top left cell.
    pub(crate) group_col: Vec<i64>,
    /// Width of each group in cells. A group with nothing in it takes one cell.
    pub(crate) group_cols: Vec<i64>,
    /// Whether each group has nothing in it.
    pub(crate) group_empty: Vec<bool>,
    /// Parent group of each group, or -1.
    pub(crate) group_parent: Vec<i64>,
    /// Row of each group's top left cell.
    pub(crate) group_row: Vec<i64>,
    /// Height of each group in cells.
    pub(crate) group_rows: Vec<i64>,
    /// Parent group of each node, or -1.
    pub(crate) node_parent: Vec<i64>,
    /// Row of each node.
    pub(crate) row: Vec<i64>,
}

/// Step across for each side index: left, right, top, bottom.
const DX: [i64; 4] = [-1, 1, 0, 0];
/// Step down for each side index.
const DY: [i64; 4] = [0, 0, -1, 1];

/// The index of `side` in [`DX`] and [`DY`].
pub(crate) fn side_index(side: Side) -> usize {
    match side {
        Side::L => 0,
        Side::R => 1,
        Side::T => 2,
        Side::B => 3,
    }
}

/// Blocks up to this many cells are found through the cell map; larger ones are compared one by
/// one.
const SMALL: i64 = 64;
const OFFSET: i64 = 1 << 25;
const SPAN: i64 = 1 << 26;

fn at<T: Copy + Default>(values: &[T], i: usize) -> T {
    values.get(i).copied().unwrap_or_default()
}

fn set<T>(values: &mut [T], i: usize, value: T) {
    if let Some(slot) = values.get_mut(i) {
        *slot = value;
    }
}

fn cell_key(cx: i64, cy: i64) -> i64 {
    (cx + OFFSET) * SPAN + cy + OFFSET
}

/// The links that place items beside each other, as parallel arrays.
#[derive(Default)]
struct Links {
    item_a: Vec<usize>,
    item_b: Vec<usize>,
    link_a: Vec<usize>,
    link_b: Vec<usize>,
    side_a: Vec<usize>,
    side_b: Vec<usize>,
}

/// The state of one placement run.
struct Placer {
    cells: BTreeMap<i64, usize>,
    edge_links: Vec<Vec<usize>>,
    first: usize,
    h: Vec<i64>,
    hints: Vec<Vec<usize>>,
    large: Vec<usize>,
    links: Links,
    n: usize,
    order: Vec<usize>,
    parent: Vec<usize>,
    placed: Vec<bool>,
    reach: BTreeMap<[i64; 6], i64>,
    w: Vec<i64>,
    x: Vec<i64>,
    y: Vec<i64>,
}

impl Placer {
    /// The position of `node` relative to its ancestor `item`.
    fn anchor(&self, node: usize, item: usize) -> (i64, i64) {
        let (mut ax, mut ay) = (0, 0);
        let mut cur = node;
        while cur != item && cur < self.x.len() {
            ax += at(&self.x, cur);
            ay += at(&self.y, cur);
            cur = self.n + at(&self.parent, cur);
        }
        (ax, ay)
    }

    fn put(&mut self, item: usize, px: i64, py: i64) {
        set(&mut self.x, item, px);
        set(&mut self.y, item, py);
        set(&mut self.placed, item, true);
        self.order.push(item);
        let (w, h) = (at(&self.w, item), at(&self.h, item));
        if w * h > SMALL {
            self.large.push(item);
            return;
        }
        for cy in py..py + h {
            for cx in px..px + w {
                self.cells.insert(cell_key(cx, cy), item);
            }
        }
    }

    fn overlaps(&self, item: usize, rx: i64, ry: i64, rw: i64, rh: i64) -> bool {
        let (x, y) = (at(&self.x, item), at(&self.y, item));
        x < rx + rw && rx < x + at(&self.w, item) && y < ry + rh && ry < y + at(&self.h, item)
    }

    fn collide(&self, rx: i64, ry: i64, rw: i64, rh: i64) -> Option<usize> {
        if rw * rh > SMALL {
            return self
                .order
                .get(self.first..)
                .unwrap_or(&[])
                .iter()
                .copied()
                .find(|&item| self.overlaps(item, rx, ry, rw, rh));
        }
        for cy in ry..ry + rh {
            for cx in rx..rx + rw {
                if let Some(&hit) = self.cells.get(&cell_key(cx, cy)) {
                    return Some(hit);
                }
            }
        }
        self.large
            .iter()
            .copied()
            .find(|&item| self.overlaps(item, rx, ry, rw, rh))
    }

    /// How far a block must slide along (`sx`, `sy`) to be clear. Each step jumps past what it
    /// hit, and the result is kept, because a later block starting from the same place cannot
    /// stop sooner.
    fn slide(&mut self, rx: i64, ry: i64, rw: i64, rh: i64, sx: i64, sy: i64) -> i64 {
        let key = [rx, ry, rw, rh, sx, sy];
        let mut k = self.reach.get(&key).copied().unwrap_or(0);
        while let Some(hit) = self.collide(rx + k * sx, ry + k * sy, rw, rh) {
            let (hx, hy, hw, hh) = (
                at(&self.x, hit),
                at(&self.y, hit),
                at(&self.w, hit),
                at(&self.h, hit),
            );
            k = if sx > 0 {
                hx + hw - rx
            } else if sx < 0 {
                rx + rw - hx
            } else if sy > 0 {
                hy + hh - ry
            } else {
                ry + rh - hy
            };
        }
        self.reach.insert(key, k);
        k
    }

    fn follow(&mut self, from: usize, k: usize) {
        let l = &self.links;
        let forward = at(&l.item_a, k) == from;
        let to = if forward {
            at(&l.item_b, k)
        } else {
            at(&l.item_a, k)
        };
        if at(&self.placed, to) {
            return;
        }
        let (sp, sq) = if forward {
            (at(&l.side_a, k), at(&l.side_b, k))
        } else {
            (at(&l.side_b, k), at(&l.side_a, k))
        };
        let (near_link, far_link) = if forward {
            (at(&l.link_a, k), at(&l.link_b, k))
        } else {
            (at(&l.link_b, k), at(&l.link_a, k))
        };
        let (pax, pay) = self.anchor(near_link, from);
        let (ax, ay) = self.anchor(far_link, to);
        let mut dx = at(&DX, sp);
        let mut dy = at(&DY, sp);
        let mut shift_x = 0;
        let mut shift_y = 0;
        let facing = at(&DX, sp) + at(&DX, sq) == 0 && at(&DY, sp) + at(&DY, sq) == 0;
        if sp == sq {
            // Like sides: the edge loops around, so the nodes stack across the direction it
            // leaves in.
            dx = i64::from(at(&DX, sp) == 0);
            dy = 1 - dx;
        } else if !facing {
            // A bend: one step along the side the edge leaves, one step back from the side it
            // enters.
            shift_x = -at(&DX, sq);
            shift_y = -at(&DY, sq);
        }

        let (xf, yf, wf, hf) = (
            at(&self.x, from),
            at(&self.y, from),
            at(&self.w, from),
            at(&self.h, from),
        );
        let (wt, ht) = (at(&self.w, to), at(&self.h, to));
        let (mut bx, mut by);
        if dx == 0 {
            by = if dy > 0 { yf + hf } else { yf - ht };
            bx = xf + pax + shift_x - ax;
        } else {
            bx = if dx > 0 { xf + wf } else { xf - wt };
            by = yf + pay + shift_y - ay;
        }

        if facing {
            // When several placed nodes reach this one through the same pair of sides, line it
            // up with the middle one, not with whichever happened to be visited first.
            if let Some(list) = self.edge_links.get(to)
                && list.len() > 1
            {
                let mut wanted: Vec<i64> = Vec::new();
                for &j in list {
                    let l = &self.links;
                    let to_is_b = at(&l.item_b, j) == to;
                    let other = if to_is_b {
                        at(&l.item_a, j)
                    } else {
                        at(&l.item_b, j)
                    };
                    let (other_side, own_side) = if to_is_b {
                        (at(&l.side_a, j), at(&l.side_b, j))
                    } else {
                        (at(&l.side_b, j), at(&l.side_a, j))
                    };
                    if !at(&self.placed, other) || other_side != sp || own_side != sq {
                        continue;
                    }
                    let (other_link, own_link) = if to_is_b {
                        (at(&l.link_a, j), at(&l.link_b, j))
                    } else {
                        (at(&l.link_b, j), at(&l.link_a, j))
                    };
                    let (oax, oay) = self.anchor(other_link, other);
                    let spot = if dx == 0 {
                        at(&self.x, other) + oax
                    } else {
                        at(&self.y, other) + oay
                    };
                    let (tax, tay) = self.anchor(own_link, to);
                    wanted.push(spot - if dx == 0 { tax } else { tay });
                }
                if wanted.len() > 1 {
                    wanted.sort_unstable();
                    let middle = at(&wanted, (wanted.len() - 1) >> 1);
                    if dx == 0 {
                        bx = middle;
                    } else {
                        by = middle;
                    }
                }
            }
            // Taken: move sideways, to whichever side is nearer, so the edge can still leave
            // straight.
            let px = i64::from(dx == 0);
            let py = 1 - px;
            let forth = self.slide(bx, by, wt, ht, px, py);
            let back = if forth == 0 {
                0
            } else {
                self.slide(bx, by, wt, ht, -px, -py)
            };
            let near = if forth <= back { forth } else { -back };
            bx += px * near;
            by += py * near;
        } else {
            let far = self.slide(bx, by, wt, ht, dx, dy);
            bx += dx * far;
            by += dy * far;
        }
        self.put(to, bx, by);
    }

    /// Lays out the children of container `c` and, for a group, records its size.
    fn layout(&mut self, c: usize, items: &[usize], groups: usize) {
        if items.is_empty() {
            return;
        }
        self.order.clear();
        let mut starts: Vec<usize> = Vec::new();
        for &start in items {
            if at(&self.placed, start) {
                continue;
            }
            self.first = self.order.len();
            starts.push(self.first);
            self.cells.clear();
            self.large.clear();
            self.reach.clear();
            self.put(start, 0, 0);
            // Declared alignments are followed as soon as a node is placed, ahead of any edge.
            let mut hint_at = self.first;
            let mut edge_at = self.first;
            let mut link_at = 0;
            loop {
                if hint_at < self.order.len() {
                    let from = at(&self.order, hint_at);
                    hint_at += 1;
                    let count = self.hints.get(from).map_or(0, Vec::len);
                    for j in 0..count {
                        let k = self.hints.get(from).map_or(0, |list| at(list, j));
                        self.follow(from, k);
                    }
                    continue;
                }
                if edge_at >= self.order.len() {
                    break;
                }
                let from = at(&self.order, edge_at);
                let next = self
                    .edge_links
                    .get(from)
                    .and_then(|list| list.get(link_at))
                    .copied();
                let Some(k) = next else {
                    edge_at += 1;
                    link_at = 0;
                    continue;
                };
                link_at += 1;
                self.follow(from, k);
            }
        }
        starts.push(self.order.len());

        // Unconnected parts go side by side, wrapping into rows that keep the whole roughly
        // square.
        let count = starts.len() - 1;
        let mut min_x = vec![0; count];
        let mut min_y = vec![0; count];
        let mut part_w = vec![0; count];
        let mut part_h = vec![0; count];
        let mut area = 0;
        let mut widest = 0;
        for p in 0..count {
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for i in at(&starts, p)..at(&starts, p + 1) {
                let item = at(&self.order, i);
                x0 = x0.min(at(&self.x, item));
                y0 = y0.min(at(&self.y, item));
                x1 = x1.max(at(&self.x, item) + at(&self.w, item));
                y1 = y1.max(at(&self.y, item) + at(&self.h, item));
            }
            set(&mut min_x, p, x0);
            set(&mut min_y, p, y0);
            set(&mut part_w, p, x1 - x0);
            set(&mut part_h, p, y1 - y0);
            area += (x1 - x0) * (y1 - y0);
            widest = widest.max(x1 - x0);
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "cell counts are small integers"
        )]
        let limit = widest.max(((area * 2) as f64).sqrt().ceil() as i64);
        let (mut cx, mut cy, mut shelf, mut width) = (0, 0, 0, 0);
        for p in 0..count {
            let pw = at(&part_w, p);
            if cx > 0 && cx + pw > limit {
                cy += shelf;
                cx = 0;
                shelf = 0;
            }
            for i in at(&starts, p)..at(&starts, p + 1) {
                let item = at(&self.order, i);
                let (x, y) = (at(&self.x, item), at(&self.y, item));
                set(&mut self.x, item, x + cx - at(&min_x, p));
                set(&mut self.y, item, y + cy - at(&min_y, p));
            }
            cx += pw;
            shelf = shelf.max(at(&part_h, p));
            width = width.max(cx);
        }
        if c < groups {
            set(&mut self.w, self.n + c, width);
            set(&mut self.h, self.n + c, cy + shelf);
        }
    }
}

/// Places `nodes` and `groups` on a grid so each edge's sides and each alignment hold where they
/// can.
pub(crate) fn place_on_grid(
    nodes: &[&ArchNode],
    groups: &[&ArchGroup],
    edges: &[ArchEdge],
    hints: &[ArchLayoutHint],
) -> Grid {
    let n = nodes.len();
    let g_count = groups.len();
    let root = g_count;
    let mut node_index: BTreeMap<&str, usize> = BTreeMap::new();
    let mut group_index: BTreeMap<&str, usize> = BTreeMap::new();
    // Items are nodes (0..N-1) then groups (N..N+G-1). Containers are groups (0..G-1) then the
    // root.
    let mut parent = vec![root; n + g_count];
    let mut depth = vec![0_i64; g_count + 1];
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); g_count + 1];
    for (g, group) in groups.iter().enumerate() {
        group_index.insert(&group.id, g);
        let p = group
            .r#in
            .as_deref()
            .and_then(|id| group_index.get(id).copied());
        if let Some(p) = p.filter(|&p| p != g) {
            set(&mut parent, n + g, p);
            let d = at(&depth, p) + 1;
            set(&mut depth, g, d);
        } else {
            set(&mut depth, g, 1);
        }
    }
    for (i, node) in nodes.iter().enumerate() {
        node_index.insert(&node.id, i);
        if let Some(p) = node
            .r#in
            .as_deref()
            .and_then(|id| group_index.get(id).copied())
        {
            set(&mut parent, i, p);
        }
        if let Some(list) = kids.get_mut(at(&parent, i)) {
            list.push(i);
        }
    }
    for g in 0..g_count {
        if let Some(list) = kids.get_mut(at(&parent, n + g)) {
            list.push(n + g);
        }
    }

    // A link asks for one node to sit beside another. It belongs to the innermost container
    // that holds both ends, and there it relates the two children of that container the ends
    // are in.
    let mut links = Links::default();
    let mut hint_links: Vec<Vec<usize>> = vec![Vec::new(); n + g_count];
    let mut edge_links: Vec<Vec<usize>> = vec![Vec::new(); n + g_count];
    let mut add_link = |a: usize, b: usize, sa: usize, sb: usize, lists: &mut Vec<Vec<usize>>| {
        if a == b {
            return;
        }
        let (mut ia, mut ib) = (a, b);
        let (mut ca, mut cb) = (at(&parent, a), at(&parent, b));
        while ca != cb {
            if at(&depth, ca) >= at(&depth, cb) {
                ia = n + ca;
                ca = at(&parent, ia);
            } else {
                ib = n + cb;
                cb = at(&parent, ib);
            }
        }
        let k = links.link_a.len();
        links.link_a.push(a);
        links.link_b.push(b);
        links.side_a.push(sa);
        links.side_b.push(sb);
        links.item_a.push(ia);
        links.item_b.push(ib);
        if let Some(list) = lists.get_mut(ia) {
            list.push(k);
        }
        if let Some(list) = lists.get_mut(ib) {
            list.push(k);
        }
    };
    for hint in hints {
        let row = hint.direction == Direction::Row;
        for pair in hint.members.windows(2) {
            let [prev, cur] = pair else { continue };
            let a = node_index.get(prev.as_str()).copied();
            let b = node_index.get(cur.as_str()).copied();
            if let (Some(a), Some(b)) = (a, b) {
                let (sa, sb) = if row { (1, 0) } else { (3, 2) };
                add_link(a, b, sa, sb, &mut hint_links);
            }
        }
    }
    // An edge between two like sides says least about where its ends go, so those come last.
    for like_sides in [false, true] {
        for edge in edges {
            if (edge.lhs_dir == edge.rhs_dir) != like_sides {
                continue;
            }
            let a = node_index.get(edge.lhs_id.as_str()).copied();
            let b = node_index.get(edge.rhs_id.as_str()).copied();
            if let (Some(a), Some(b)) = (a, b) {
                add_link(
                    a,
                    b,
                    side_index(edge.lhs_dir),
                    side_index(edge.rhs_dir),
                    &mut edge_links,
                );
            }
        }
    }

    // Position of each item in its container, and its size in cells.
    let mut placer = Placer {
        cells: BTreeMap::new(),
        edge_links,
        first: 0,
        h: vec![1; n + g_count],
        hints: hint_links,
        large: Vec::new(),
        links,
        n,
        order: Vec::new(),
        parent,
        placed: vec![false; n + g_count],
        reach: BTreeMap::new(),
        w: vec![1; n + g_count],
        x: vec![0; n + g_count],
        y: vec![0; n + g_count],
    };
    for c in (0..g_count).rev() {
        let items = kids.get(c).cloned().unwrap_or_default();
        placer.layout(c, &items, g_count);
    }
    let items = kids.get(root).cloned().unwrap_or_default();
    placer.layout(root, &items, g_count);

    let mut grid = Grid {
        col: vec![0; n],
        group_col: vec![0; g_count],
        group_cols: placer.w.get(n..).unwrap_or(&[]).to_vec(),
        group_empty: vec![false; g_count],
        group_parent: vec![0; g_count],
        group_row: vec![0; g_count],
        group_rows: placer.h.get(n..).unwrap_or(&[]).to_vec(),
        node_parent: vec![0; n],
        row: vec![0; n],
    };
    let signed = |p: usize| i64::try_from(p).unwrap_or(-1);
    for g in 0..g_count {
        let p = at(&placer.parent, n + g);
        let (base_col, base_row) = if p == root {
            (0, 0)
        } else {
            (at(&grid.group_col, p), at(&grid.group_row, p))
        };
        set(
            &mut grid.group_parent,
            g,
            if p == root { -1 } else { signed(p) },
        );
        set(&mut grid.group_col, g, at(&placer.x, n + g) + base_col);
        set(&mut grid.group_row, g, at(&placer.y, n + g) + base_row);
        if kids.get(g).is_none_or(Vec::is_empty) {
            set(&mut grid.group_empty, g, true);
        }
    }
    for i in 0..n {
        let p = at(&placer.parent, i);
        let (base_col, base_row) = if p == root {
            (0, 0)
        } else {
            (at(&grid.group_col, p), at(&grid.group_row, p))
        };
        set(
            &mut grid.node_parent,
            i,
            if p == root { -1 } else { signed(p) },
        );
        set(&mut grid.col, i, at(&placer.x, i) + base_col);
        set(&mut grid.row, i, at(&placer.y, i) + base_row);
    }
    grid
}
