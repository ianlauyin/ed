//! Wraps the disconnected parts of a laid out drawing into rows. Port of Pele's
//! `src/layout/components.ts`.

use crate::layout::compound::CEdge;
use crate::layout::compound::CNode;
use crate::layout::compound::CompoundResult;

/// A disconnected part: its nodes and edges, by index, and its bounds.
struct Part {
    nodes: Vec<usize>,
    edges: Vec<usize>,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Part {
    /// Grows the bounds to hold a box of `w` by `h` centered on (`x`, `y`).
    fn include(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.left = self.left.min(x - w / 2.0);
        self.top = self.top.min(y - h / 2.0);
        self.right = self.right.max(x + w / 2.0);
        self.bottom = self.bottom.max(y + h / 2.0);
    }
}

/// The representative of `i`'s set, halving the path to it.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&up) = parent.get(i)
        && up != i
    {
        let grand = parent.get(up).copied().unwrap_or(up);
        if let Some(slot) = parent.get_mut(i) {
            *slot = grand;
        }
        i = grand;
    }
    i
}

/// Moves disconnected parts into rows `width` wide, `gap` apart, without changing the layout
/// inside a part. Returns the new size, or `None` when there is one part or rows would not make
/// the drawing narrower.
pub(crate) fn wrap_components(
    nodes: &mut [CNode],
    edges: &mut [CEdge],
    width: f64,
    gap: f64,
) -> Option<CompoundResult> {
    /// Puts `a`'s set into `b`'s.
    fn join(parent: &mut [usize], a: usize, b: usize) {
        let (ra, rb) = (root(parent, a), root(parent, b));
        if let Some(slot) = parent.get_mut(ra) {
            *slot = rb;
        }
    }

    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    for (i, c) in nodes.iter().enumerate() {
        if let Ok(up) = usize::try_from(c.parent) {
            join(&mut parent, i, up);
        }
    }
    for e in edges.iter() {
        join(&mut parent, e.src, e.dst);
    }

    // Parts in the order their first node comes, as Pele's map keeps them.
    let mut parts: Vec<Part> = Vec::new();
    let mut part_of: Vec<Option<usize>> = vec![None; nodes.len()];
    for (i, c) in nodes.iter().enumerate() {
        let id = root(&mut parent, i);
        let at = if let Some(at) = part_of.get(id).copied().flatten() {
            at
        } else {
            parts.push(Part {
                nodes: Vec::new(),
                edges: Vec::new(),
                left: f64::INFINITY,
                top: f64::INFINITY,
                right: f64::NEG_INFINITY,
                bottom: f64::NEG_INFINITY,
            });
            if let Some(slot) = part_of.get_mut(id) {
                *slot = Some(parts.len() - 1);
            }
            parts.len() - 1
        };
        if let Some(part) = parts.get_mut(at) {
            part.nodes.push(i);
            part.include(c.x, c.y, c.w, c.h);
        }
    }
    if parts.len() < 2 {
        return None;
    }
    for (k, e) in edges.iter().enumerate() {
        let id = root(&mut parent, e.src);
        let Some(part) = part_of.get(id).copied().flatten().and_then(|at| parts.get_mut(at)) else {
            continue;
        };
        part.edges.push(k);
        for point in e.route.chunks_exact(3) {
            if let [x, y, _] = point {
                part.include(*x, *y, 0.0, 0.0);
            }
        }
        if e.label_w > 0.0 {
            part.include(e.label_x, e.label_y, e.label_w, e.label_h);
        }
    }

    let (mut x, mut y, mut row_height, mut right) = (0.0, 0.0, 0.0_f64, 0.0_f64);
    let (mut from, mut to) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut shifts = Vec::with_capacity(parts.len());
    for part in &parts {
        let (w, h) = (part.right - part.left, part.bottom - part.top);
        if x > 0.0 && x + w > width {
            y += row_height + gap;
            x = 0.0;
            row_height = 0.0;
        }
        shifts.push((x - part.left, y - part.top));
        from = from.min(part.left);
        to = to.max(part.right);
        right = right.max(x + w);
        row_height = row_height.max(h);
        x += w + gap;
    }
    if right >= to - from {
        return None;
    }
    for (part, (ox, oy)) in parts.iter().zip(shifts) {
        for &i in &part.nodes {
            if let Some(c) = nodes.get_mut(i) {
                c.x += ox;
                c.y += oy;
            }
        }
        for &k in &part.edges {
            if let Some(e) = edges.get_mut(k) {
                for point in e.route.chunks_exact_mut(3) {
                    if let [x, y, _] = point {
                        *x += ox;
                        *y += oy;
                    }
                }
                e.label_x += ox;
                e.label_y += oy;
            }
        }
    }
    Some(CompoundResult { height: y + row_height, width: right })
}
