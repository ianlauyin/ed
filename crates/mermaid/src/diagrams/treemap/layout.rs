//! Squarified treemap layout. Port of Pele's `src/diagrams/treemap/layout.ts`.

use super::model::TreemapNode;
use crate::svg::builder::js_round;

/// A node's rectangle. Boxes link to their parent and children by index into one list.
#[derive(Clone, Debug)]
pub(crate) struct Layout<'a> {
    /// Indices of the child boxes.
    pub(crate) children: Vec<usize>,
    /// Height.
    pub(crate) h: f64,
    /// The node.
    pub(crate) node: &'a TreemapNode,
    /// Index of the parent box.
    pub(crate) parent: Option<usize>,
    /// Position among the top-level nodes of the branch this box belongs to, which picks its
    /// color.
    pub(crate) series: usize,
    /// A leaf's own value, or the sum of the leaves under a section.
    pub(crate) value: f64,
    /// Width.
    pub(crate) w: f64,
    /// Left edge.
    pub(crate) x: f64,
    /// Top edge.
    pub(crate) y: f64,
}

/// Space around boxes: Pele's `Insets`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Insets {
    /// Space between boxes.
    pub(crate) gap: f64,
    /// Space a section keeps for its header.
    pub(crate) header: f64,
    /// Space a section keeps around its children on the other three sides.
    pub(crate) side: f64,
}

/// A box for `node`, valued by its own value when it is a positive leaf.
fn new_box(node: &TreemapNode, parent: Option<usize>, series: usize) -> Layout<'_> {
    let value = match (&node.children, node.value) {
        (None, Some(value)) if value.is_finite() && value > 0.0 => value,
        _ => 0.0,
    };
    Layout {
        children: Vec::new(),
        h: 0.0,
        node,
        parent,
        series,
        value,
        w: 0.0,
        x: 0.0,
        y: 0.0,
    }
}

/// Lays out the boxes `shown` in the rectangle as one squarified treemap: rows are added along the
/// shorter side, and a row stops growing when another box would make its boxes less square.
#[expect(
    clippy::many_single_char_names,
    reason = "the rectangle's coordinates, as in Pele"
)]
fn squarify(boxes: &mut [Layout<'_>], shown: &[usize], total: f64, rect: [f64; 4], gap: f64) {
    let [mut x, mut y, mut w, mut h] = rect;
    let value = |boxes: &[Layout<'_>], i: usize| {
        shown
            .get(i)
            .and_then(|&k| boxes.get(k))
            .map_or(0.0, |b| b.value)
    };
    let mut rest = total;
    let mut from = 0;
    while from < shown.len() {
        let scale = (w * h) / rest;
        let side = w.min(h);
        let mut sum = value(boxes, from) * scale;
        let largest = sum;
        let mut worst =
            ((side * side * largest) / (sum * sum)).max((sum * sum) / (side * side * largest));
        let mut to = from + 1;
        while to < shown.len() {
            let smallest = value(boxes, to) * scale;
            let next = sum + smallest;
            let ratio = ((side * side * largest) / (next * next))
                .max((next * next) / (side * side * smallest));
            if ratio > worst {
                break;
            }
            worst = ratio;
            sum = next;
            to += 1;
        }

        let thickness = sum / side;
        let mut offset = 0.0;
        for i in from..to {
            let Some(b) = shown.get(i).and_then(|&k| boxes.get_mut(k)) else {
                continue;
            };
            let length = if i == to - 1 {
                side - offset
            } else {
                (b.value * scale) / thickness
            };
            let bx = if w >= h { x } else { x + offset };
            let by = if w >= h { y + offset } else { y };
            let x0 = js_round(bx + gap / 2.0);
            let y0 = js_round(by + gap / 2.0);
            b.x = x0;
            b.y = y0;
            b.w = js_round(bx + if w >= h { thickness } else { length } - gap / 2.0) - x0;
            b.h = js_round(by + if w >= h { length } else { thickness } - gap / 2.0) - y0;
            offset += length;
            rest -= b.value;
        }
        if w >= h {
            x += thickness;
            w -= thickness;
        } else {
            y += thickness;
            h -= thickness;
        }
        from = to;
    }
}

/// Lays out the children `children` of a box in the rectangle, adding the boxes that get a size
/// to `placed`.
fn place(
    boxes: &mut [Layout<'_>],
    placed: &mut Vec<usize>,
    children: &[usize],
    rect: [f64; 4],
    gap: f64,
) {
    let [x, y, w, h] = rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut shown: Vec<usize> = children
        .iter()
        .copied()
        .filter(|&k| boxes.get(k).is_some_and(|b| b.value > 0.0))
        .collect();
    let value = |k: usize| boxes.get(k).map_or(0.0, |b| b.value);
    shown.sort_by(|&a, &b| value(b).total_cmp(&value(a)));
    let mut total = 0.0;
    for &k in &shown {
        total += value(k);
    }
    if shown.is_empty() || !total.is_finite() {
        return;
    }

    squarify(
        boxes,
        &shown,
        total,
        [x - gap / 2.0, y - gap / 2.0, w + gap, h + gap],
        gap,
    );
    for k in shown {
        if boxes.get(k).is_some_and(|b| b.w > 0.0 && b.h > 0.0) {
            placed.push(k);
        }
    }
}

/// Lays out `roots` in a `width` by `height` rectangle. Returns every box and the indices of
/// those with a size, parents before their children.
pub(crate) fn layout_treemap(
    roots: &[TreemapNode],
    width: f64,
    height: f64,
    insets: Insets,
) -> (Vec<Layout<'_>>, Vec<usize>) {
    let mut boxes: Vec<Layout<'_>> = roots
        .iter()
        .enumerate()
        .map(|(index, node)| new_box(node, None, index))
        .collect();
    let top: Vec<usize> = (0..boxes.len()).collect();
    let mut i = 0;
    while let Some(parent) = boxes.get(i) {
        let node = parent.node;
        let series = parent.series;
        for child in node.children.iter().flatten() {
            let k = boxes.len();
            boxes.push(new_box(child, Some(i), series));
            if let Some(parent) = boxes.get_mut(i) {
                parent.children.push(k);
            }
        }
        i += 1;
    }
    for k in (0..boxes.len()).rev() {
        let Some(b) = boxes.get(k) else {
            continue;
        };
        let (parent, value) = (b.parent, b.value);
        if let Some(parent) = parent.and_then(|parent| boxes.get_mut(parent)) {
            parent.value += value;
        }
    }

    let Insets { gap, header, side } = insets;
    let mut placed = Vec::new();
    place(
        &mut boxes,
        &mut placed,
        &top,
        [0.0, 0.0, width, height],
        gap,
    );
    let mut i = 0;
    while let Some(&k) = placed.get(i) {
        if let Some(b) = boxes.get(k)
            && !b.children.is_empty()
        {
            let children = b.children.clone();
            let rect = [
                b.x + side,
                b.y + header,
                b.w - 2.0 * side,
                b.h - header - side,
            ];
            place(&mut boxes, &mut placed, &children, rect, gap);
        }
        i += 1;
    }
    (boxes, placed)
}
