//! Lays out C4 shapes and boundaries in Mermaid's grid. Port of Pele's
//! `src/diagrams/c4/layout.ts`.

use super::db::C4Db;
use super::route::{Box, Form, HEAD, LANE};
use super::types::C4Element;
use crate::text::label::{Label, layout_label, trim};
use crate::text::measure::BOLD;

const SHAPE_W: f64 = 216.0;
const SHAPE_MIN_H: f64 = 64.0;
const PAD_X: f64 = 14.0;
const PAD_Y: f64 = 12.0;
const GAP_X: f64 = 100.0;
const GAP_Y: f64 = 88.0;
/// Room for a relation to run along the inside of a boundary.
pub(crate) const GROUP_PAD: f64 = LANE + 8.0;
const GROUP_GAP: f64 = 48.0;
/// Where a boundary's heading starts, from its top-left corner.
pub(crate) const HEAD_LEFT: f64 = 16.0;
/// Where a boundary's heading starts, from its top-left corner.
pub(crate) const HEAD_TOP: f64 = 10.0;

/// A laid out shape.
pub(crate) struct Node {
    /// Its box, in [`Layout::boxes`].
    pub(crate) id: usize,
    /// Type, name, technology, description.
    pub(crate) labels: [Label; 4],
    /// The index of its shape.
    pub(crate) shape: usize,
    /// The height of its text.
    pub(crate) text_h: f64,
}

/// A laid out boundary.
pub(crate) struct Group {
    /// The index of its boundary.
    pub(crate) boundary: usize,
    /// The height of its heading.
    pub(crate) head: f64,
    /// The width of its heading.
    pub(crate) head_w: f64,
    /// Where its heading starts, from its left edge.
    pub(crate) head_x: f64,
    /// Its box, in [`Layout::boxes`].
    pub(crate) id: usize,
    /// The groups inside it.
    pub(crate) kids: Vec<usize>,
    /// Name, type, description. Empty for the top level.
    pub(crate) labels: Vec<Label>,
    /// The nodes inside it.
    pub(crate) nodes: Vec<Node>,
}

/// A laid out diagram.
pub(crate) struct Layout {
    /// Every box: shapes and boundaries.
    pub(crate) boxes: Vec<Box>,
    /// The box of each boundary that is drawn, by boundary index.
    pub(crate) boundary_boxes: Vec<Option<usize>>,
    /// Every group, by boundary index.
    pub(crate) groups: Vec<Group>,
    /// The groups that are drawn, parents before children. The first is the unnamed top level.
    pub(crate) order: Vec<usize>,
    /// The box of each shape that is drawn, by shape index.
    pub(crate) shape_boxes: Vec<Option<usize>>,
}

/// The shared shape of a keyword Mermaid accepts in `$shape`, `$sprite` or `$tags`.
fn form_named(name: &str) -> Option<Form> {
    const FORMS: [(&str, Form); 9] = [
        ("person", Form::Person),
        ("box", Form::Rect),
        ("rounded", Form::Rect),
        ("component", Form::Rect),
        ("cylinder", Form::Cyl),
        ("database", Form::Cyl),
        ("db", Form::Cyl),
        ("queue", Form::HCyl),
        ("pipe", Form::HCyl),
    ];
    // Every keyword is ASCII, and no other character lowercases to an ASCII one of theirs.
    FORMS
        .iter()
        .find(|(own, _)| own.eq_ignore_ascii_case(name))
        .map(|&(_, form)| form)
}

fn form_of(shape: &C4Element) -> Form {
    let explicit = form_named(shape.shape.as_deref().unwrap_or(""))
        .or_else(|| form_named(shape.sprite.as_deref().unwrap_or("")));
    if let Some(form) = explicit {
        return form;
    }
    if let Some(tags) = shape.tags.as_deref().filter(|tags| !tags.is_empty()) {
        for tag in tags.split(',') {
            if let Some(form) = form_named(trim(tag)) {
                return form;
            }
        }
    }
    let kind = shape.kind.as_str();
    if kind.ends_with("person") {
        Form::Person
    } else if kind.ends_with("_db") {
        Form::Cyl
    } else if kind.ends_with("_queue") {
        Form::HCyl
    } else {
        Form::Rect
    }
}

fn measure(shape: &C4Element, size: f64, small: f64, width: f64) -> (Box, [Label; 4], f64) {
    let form = form_of(shape);
    let cap_w = if form == Form::HCyl { 36.0 } else { 0.0 };
    let cap_h = match form {
        Form::Person => HEAD,
        Form::Cyl => 24.0,
        _ => 0.0,
    };
    let inner = width - 2.0 * PAD_X - cap_w;
    let mut kind = String::from("«");
    for c in shape.kind.chars() {
        kind.push(if c == '_' { ' ' } else { c });
    }
    kind.push('»');
    let techn = shape
        .techn
        .as_deref()
        .filter(|techn| !techn.is_empty())
        .map(|techn| ["[", techn, "]"].concat());
    let labels = [
        layout_label(Some(&kind), false, small - 1.0, inner, 0, None),
        layout_label(Some(&shape.label), false, size, inner, BOLD, None),
        layout_label(techn.as_deref(), false, small, inner, 0, None),
        layout_label(shape.descr.as_deref(), false, small, inner, 0, None),
    ];
    let mut tw = 0.0_f64;
    let mut text_h = if labels[3].height > 0.0 { 4.0 } else { 0.0 };
    for label in &labels {
        tw = tw.max(label.width);
        text_h += label.height;
    }
    let shape_box = Box {
        form,
        h: SHAPE_MIN_H.max(text_h + 2.0 * PAD_Y + cap_h),
        w: width.max(tw + 2.0 * PAD_X + cap_w),
        x: 0.0,
        y: 0.0,
    };
    (shape_box, labels, text_h)
}

/// Places boxes left to right in rows of `per_row`, from (0, 0), and returns the width and
/// height the rows take. Shapes are stretched to the height of their row. Boundaries keep their
/// own.
fn rows(
    boxes: &mut [Box],
    ids: &[usize],
    per_row: f64,
    gap_x: f64,
    gap_y: f64,
    stretch: bool,
) -> (f64, f64) {
    // A row size too large to count in is as good as all of them.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive whole number, saturated"
    )]
    let per_row = (per_row as usize).max(1);
    let mut width = 0.0_f64;
    let mut y = 0.0;
    for row in ids.chunks(per_row) {
        let mut x = 0.0;
        let row_h = row
            .iter()
            .filter_map(|&id| boxes.get(id))
            .fold(0.0_f64, |h, b| h.max(b.h));
        for &id in row {
            if let Some(b) = boxes.get_mut(id) {
                b.x = x;
                b.y = y;
                if stretch {
                    b.h = row_h;
                }
                x += b.w + gap_x;
            }
        }
        width = width.max(x - gap_x);
        y += row_h + gap_y;
    }
    (width, if ids.is_empty() { 0.0 } else { y - gap_y })
}

/// How many boxes go in a row: `limit`, or fewer when the diagram must fit `room`.
fn count(
    boxes: &[Box],
    ids: &[usize],
    room: Option<f64>,
    limit: f64,
    gap: f64,
    group_pad: f64,
) -> f64 {
    let Some(room) = room else {
        return limit;
    };
    let widest = ids
        .iter()
        .filter_map(|&id| boxes.get(id))
        .fold(1.0_f64, |w, b| w.max(b.w));
    1.0_f64.max(limit.min(((room + gap - 2.0 * group_pad) / (widest + gap)).floor()))
}

/// Mermaid's grid: in each boundary, its shapes in rows of `c4ShapeInRow`, then the boundaries
/// inside it in rows of `c4BoundaryInRow`, each sized around its own content. `room`, when set,
/// is the width to fit, and `gap` the space between shapes then.
pub(crate) fn layout_c4(db: &C4Db, size: f64, small: f64, room: Option<f64>, gap: f64) -> Layout {
    let group_pad = if room.is_none() { GROUP_PAD } else { 12.0 };
    let mut boxes: Vec<Box> = Vec::new();
    let mut groups: Vec<Group> = Vec::with_capacity(db.boundaries.len());
    for (index, boundary) in db.boundaries.iter().enumerate() {
        let label_room = room.map_or(4000.0, |room| {
            80.0_f64.max(room - 2.0 * HEAD_LEFT - 2.0 * GROUP_PAD * 2.0)
        });
        let labels = if boundary.parent.is_some() {
            let r#type = boundary
                .r#type
                .as_deref()
                .filter(|t| !t.is_empty())
                .map(|t| ["[", t, "]"].concat());
            vec![
                layout_label(Some(&boundary.label), false, size, label_room, BOLD, None),
                layout_label(r#type.as_deref(), false, small, label_room, 0, None),
                layout_label(boundary.descr.as_deref(), false, small, label_room, 0, None),
            ]
        } else {
            Vec::new()
        };
        let mut head = 0.0;
        let mut head_w = 0.0_f64;
        for label in &labels {
            head += label.height;
            head_w = head_w.max(label.width);
        }
        let id = boxes.len();
        boxes.push(Box::default());
        groups.push(Group {
            boundary: index,
            head: if head > 0.0 {
                head + HEAD_TOP + 6.0 + LANE
            } else {
                group_pad
            },
            head_w,
            head_x: HEAD_LEFT,
            id,
            kids: Vec::new(),
            labels,
            nodes: Vec::new(),
        });
    }
    for index in 0..groups.len() {
        if let Some(parent) = db.boundaries.get(index).and_then(|b| b.parent)
            && let Some(group) = groups.get_mut(parent)
        {
            group.kids.push(index);
        }
    }
    // A boundary that ends up inside itself is not reachable from the top, as in Mermaid.
    let mut order: Vec<usize> = vec![0];
    let mut depths: Vec<Option<usize>> = vec![None; groups.len()];
    if let Some(depth) = depths.first_mut() {
        *depth = Some(0);
    }
    let mut i = 0;
    while let Some(&at) = order.get(i) {
        let depth = depths.get(at).copied().flatten().unwrap_or(0);
        for &kid in groups.get(at).map_or(&[][..], |g| &g.kids[..]) {
            if let Some(slot) = depths.get_mut(kid) {
                *slot = Some(depth + 1);
            }
            order.push(kid);
        }
        i += 1;
    }
    for (index, shape) in db.shapes.iter().enumerate() {
        let Some(parent) = shape.parent else {
            continue;
        };
        let depth = depths.get(parent).copied().flatten().unwrap_or(0);
        #[expect(clippy::cast_precision_loss, reason = "depths stay far below 2^52")]
        let depth = depth as f64;
        let node_width = room.map_or(SHAPE_W, |room| {
            100.0_f64.max(SHAPE_W.min(room - 2.0 * depth * group_pad - 32.0))
        });
        let Some(group) = groups.get_mut(parent) else {
            continue;
        };
        let (shape_box, labels, text_h) = measure(shape, size, small, node_width);
        let id = boxes.len();
        boxes.push(shape_box);
        group.nodes.push(Node {
            id,
            labels,
            shape: index,
            text_h,
        });
    }

    let per_row = db.shape_in_row.max(1.0);
    let groups_per_row = db.boundary_in_row.max(1.0);
    let mut kids_top: Vec<f64> = vec![0.0; groups.len()];
    for &at in order.iter().rev() {
        let Some(group) = groups.get(at) else {
            continue;
        };
        let gap_x = if room.is_none() { GAP_X } else { gap };
        let node_ids: Vec<usize> = group.nodes.iter().map(|node| node.id).collect();
        let kid_ids: Vec<usize> = group
            .kids
            .iter()
            .filter_map(|&kid| groups.get(kid).map(|g| g.id))
            .collect();
        let node_count = count(&boxes, &node_ids, room, per_row, gap_x, group_pad);
        let (grid_w, grid_h) = rows(&mut boxes, &node_ids, node_count, gap_x, GAP_Y, true);
        let kid_count = count(&boxes, &kid_ids, room, groups_per_row, GROUP_GAP, group_pad);
        let (kids_w, kids_h) = rows(&mut boxes, &kid_ids, kid_count, GROUP_GAP, GROUP_GAP, false);
        let between = if grid_h > 0.0 && kids_h > 0.0 {
            GROUP_GAP
        } else {
            0.0
        };
        let frame = if at == 0 { 0.0 } else { group_pad };
        if let Some(top) = kids_top.get_mut(at) {
            *top = grid_h + between;
        }
        let (head, head_w, id) = (group.head, group.head_w, group.id);
        if let Some(b) = boxes.get_mut(id) {
            b.w = grid_w
                .max(kids_w)
                .max(head_w + 2.0 * HEAD_LEFT - 2.0 * frame)
                + 2.0 * frame;
            let head = if frame == 0.0 { 0.0 } else { head };
            b.h = head + grid_h + between + kids_h + frame;
        }
    }

    let mut shape_boxes: Vec<Option<usize>> = vec![None; db.shapes.len()];
    let mut boundary_boxes: Vec<Option<usize>> = vec![None; db.boundaries.len()];
    for &at in &order {
        let Some(group) = groups.get(at) else {
            continue;
        };
        let Some(&own) = boxes.get(group.id) else {
            continue;
        };
        let left = own.x + if at == 0 { 0.0 } else { group_pad };
        let top = own.y + if at == 0 { 0.0 } else { group.head };
        for node in &group.nodes {
            if let Some(b) = boxes.get_mut(node.id) {
                b.x += left;
                b.y += top;
            }
            if let Some(slot) = shape_boxes.get_mut(node.shape) {
                *slot = Some(node.id);
            }
        }
        let kids_top = kids_top.get(at).copied().unwrap_or(0.0);
        for &kid in &group.kids {
            if let Some(b) = groups.get(kid).and_then(|kid| boxes.get_mut(kid.id)) {
                b.x += left;
                b.y += top + kids_top;
            }
        }
        if at != 0
            && let Some(slot) = boundary_boxes.get_mut(at)
        {
            *slot = Some(group.id);
        }
    }
    Layout {
        boxes,
        boundary_boxes,
        groups,
        order,
        shape_boxes,
    }
}
