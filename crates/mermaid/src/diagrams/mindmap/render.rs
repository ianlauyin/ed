//! Draws a mindmap: branches spreading to both sides of the root, or an indented outline running
//! down when that is narrower. Port of Pele's `src/diagrams/mindmap/render.ts`.

use super::db::{MindmapDb, MindmapNode};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::diagrams::common::icon::icon_svg;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::shapes::{draw_shape, shape_inset, shape_size};
use crate::svg::theme::{class_names, series_color};
use crate::text::icons::{IconResolver, icon_markup};
use crate::text::label::{Label, layout_label};
use crate::text::measure::BOLD;
use crate::yaml::Value;

/// Indexed by Mermaid's node type. A node without delimiters is text on a line, except the root.
const SHAPES: [&str; 7] = ["", "rounded", "rect", "circle", "cloud", "bang", "hex"];
const ROOT_SHAPE: &str = "stadium";

const GAP_X: f64 = 36.0;
const GAP_Y: f64 = 8.0;
/// Between two subtrees, of which at least one has children of its own.
const GROUP_GAP: f64 = 18.0;
const BRANCH_GAP_X: f64 = 52.0;
const BRANCH_GAP_Y: f64 = 28.0;
const TEXT_PAD: f64 = 6.0;
const PILL_PAD: f64 = 12.0;
const TINT: &str = "0.22";
const ICON_GAP: f64 = 6.0;
/// Running down, each level is set in by this much, and links leave a node this far from its left
/// edge.
const INDENT: f64 = 28.0;
const STEM: f64 = 12.0;
const BEND: f64 = 8.0;
const ROW_GAP: f64 = 10.0;

const LABEL_ATTRS: &str = " class=\"ondine-label\"";

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// JavaScript's truthiness of a number, as `a || b` reads it.
fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

/// A label with each line starting at `left`, for an outline, where centered lines would look
/// ragged.
#[expect(clippy::cast_precision_loss, reason = "line counts are far below 2^52")]
fn left_label(label: &Label, left: f64, cy: f64, icons: Option<IconResolver<'_>>) -> String {
    if label.lines.len() < 2 {
        return label_svg(label, left + label.width / 2.0, cy, LABEL_ATTRS, icons);
    }
    let mut out = String::new();
    let count = label.lines.len() as f64;
    for (k, (line, &width)) in label.lines.iter().zip(&label.widths).enumerate() {
        let single = Label {
            height: label.line_height,
            line_height: label.line_height,
            lines: vec![line.clone()],
            size: label.size,
            width,
            widths: vec![width],
        };
        let y = cy + (k as f64 - (count - 1.0) / 2.0) * label.line_height;
        out.push_str(&label_svg(
            &single,
            left + width / 2.0,
            y,
            LABEL_ATTRS,
            icons,
        ));
    }
    out
}

/// Draws `model` with the `mindmap` section of `config`, running down when across does not fit.
pub(crate) fn render(
    model: &MindmapDb,
    config: &Value,
    options: &RenderOptions,
    icons: Option<IconResolver<'_>>,
) -> Rendered {
    turn_to_fit(options, |down| draw(model, config, options, icons, down))
}

/// Stacks the subtrees `list[from..to]` top to bottom, setting their offsets in `rel`. Returns
/// the height at which their parent's anchor belongs and the stacked height.
fn stack(
    nodes: &[MindmapNode],
    list: &[usize],
    gap: f64,
    rel: &mut [f64],
    sizes: (&[f64], &[f64], &[f64]),
) -> (f64, f64) {
    let (extent, own, anchor) = sizes;
    let at = |values: &[f64], i: usize| values.get(i).copied().unwrap_or(0.0);
    let has_children = |c: usize| nodes.get(c).is_some_and(|node| !node.children.is_empty());
    let mut y = 0.0;
    let mut last = 0.0;
    for (k, &c) in list.iter().enumerate() {
        if let Some(slot) = rel.get_mut(c) {
            *slot = y;
        }
        let grouped = has_children(c) || list.get(k + 1).is_some_and(|&next| has_children(next));
        last = if grouped { gap.max(GROUP_GAP) } else { gap };
        y += at(extent, c) + last;
    }
    let stacked = y - last;
    let (Some(&a), Some(&b)) = (list.first(), list.last()) else {
        return (f64::NAN, stacked);
    };
    let middle =
        (at(rel, a) + at(own, a) + at(anchor, a) + at(rel, b) + at(own, b) + at(anchor, b)) / 2.0;
    (middle, stacked)
}

/// Draws the map. `down` draws it as an outline: the root at the top, and each level set in under
/// its parent. It is far narrower than branches spreading to both sides, and as tall as it needs
/// to be.
#[expect(
    clippy::cast_precision_loss,
    reason = "depths and counts are far below 2^52"
)]
#[expect(clippy::many_single_char_names, reason = "Pele's names")]
fn draw(
    model: &MindmapDb,
    config: &Value,
    options: &RenderOptions,
    icons: Option<IconResolver<'_>>,
    down: bool,
) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let pad = options.padding.unwrap_or(8.0);
    let wrap = match property(config, "mindmap").and_then(|conf| property(conf, "maxNodeWidth")) {
        Some(Value::Number(width)) if *width > 0.0 => width.min(2000.0),
        _ => 200.0,
    };
    let glyph = size * 1.25;

    let nodes = &model.nodes;
    let n = nodes.len();
    let mut parent = vec![0usize; n];
    let mut depth = vec![0usize; n];
    for (i, node) in nodes.iter().enumerate() {
        let own = depth.get(i).copied().unwrap_or(0);
        for &child in &node.children {
            if let Some(slot) = parent.get_mut(child) {
                *slot = i;
            }
            if let Some(slot) = depth.get_mut(child) {
                *slot = own + 1;
            }
        }
    }
    let at = |values: &[f64], i: usize| values.get(i).copied().unwrap_or(0.0);

    // An outline has the width the host gives it, less what its level is set in.
    let room = match options.max_width {
        Some(width) if down && width > 0.0 => width - 2.0 * pad,
        _ => f64::INFINITY,
    };
    let mut labels = Vec::with_capacity(n);
    let mut shapes = Vec::with_capacity(n);
    let mut w = vec![0.0; n];
    let mut h = vec![0.0; n];
    let mut dy = vec![0.0; n];
    // Where links meet the node, measured down from its top: its middle.
    let mut anchor = vec![0.0; n];
    // Whether the host has the node's icon. One it does not have takes no room.
    let mut drawn = vec![false; n];

    for (i, node) in nodes.iter().enumerate() {
        let has_icon = node
            .icon
            .as_deref()
            .is_some_and(|icon| !icon_markup(icons, icon).is_empty());
        let icon_room = if has_icon { glyph + ICON_GAP } else { 0.0 };
        let level = depth.get(i).copied().unwrap_or(0) as f64;
        let fit = wrap.min(72.0_f64.max(room - level * INDENT - 2.0 * TEXT_PAD - icon_room));
        let label = layout_label(Some(&node.descr), true, size, fit, 0, icons);
        let tw = label.width
            + if has_icon {
                glyph + if label.width > 0.0 { ICON_GAP } else { 0.0 }
            } else {
                0.0
            };
        let th = label.height.max(js_round(size * 1.5));
        let mut shape = match SHAPES.get(usize::from(node.kind)).copied() {
            Some(shape) if !shape.is_empty() => shape,
            _ if i == 0 => ROOT_SHAPE,
            _ => "",
        };
        // In an outline the root heads a column, and the trunk drops from under its left end. A
        // circle or a cloud would stand wide of that, so the root is a pill there unless it is a
        // box already.
        if down && i == 0 && shape != "rect" && shape != "rounded" {
            shape = ROOT_SHAPE;
        }
        if let (Some(wi), Some(hi), Some(ai), Some(di)) =
            (w.get_mut(i), h.get_mut(i), anchor.get_mut(i), dy.get_mut(i))
        {
            if shape.is_empty() {
                // A node with no shape of its own is plain text when it ends a branch, and text
                // in a tinted pill when the branch goes on from it, which sets the levels apart.
                let pill = !node.children.is_empty();
                *wi = tw + 2.0 * if pill { PILL_PAD } else { TEXT_PAD };
                *hi = th + if pill { 6.0 } else { 0.0 };
                *ai = *hi / 2.0;
            } else {
                let s = shape_size(shape, tw, th);
                *wi = s.w;
                *hi = s.h;
                *di = s.dy;
                *ai = s.h / 2.0;
            }
        }
        if let Some(slot) = drawn.get_mut(i) {
            *slot = has_icon;
        }
        labels.push(label);
        shapes.push(shape);
    }

    // Each subtree takes a band of height `extent`. `rel` is a subtree's offset among its
    // siblings, `own` the node's offset inside its band, and `kids` the offset of its children's
    // bands.
    let mut extent = vec![0.0; n];
    let mut rel = vec![0.0; n];
    let mut own = vec![0.0; n];
    let mut kids = vec![0.0; n];

    // Nodes are in depth-first order, so going backwards visits children before their parent.
    for i in (1..n).rev() {
        let Some(node) = nodes.get(i) else { continue };
        let hi = at(&h, i);
        if node.children.is_empty() {
            if let Some(slot) = extent.get_mut(i) {
                *slot = hi;
            }
            continue;
        }
        let (middle, stacked) = stack(
            nodes,
            &node.children,
            GAP_Y,
            &mut rel,
            (&extent, &own, &anchor),
        );
        let top = middle - at(&anchor, i);
        let low = top.min(0.0);
        if let (Some(e), Some(o), Some(k)) = (extent.get_mut(i), own.get_mut(i), kids.get_mut(i)) {
            *e = stacked.max(top + hi) - low;
            *o = top - low;
            *k = -low;
        }
    }

    let mut x = vec![0.0; n];
    let mut y = vec![0.0; n];
    let mut band = vec![0.0; n];
    let mut side = vec![0.0; n];
    // Running down: how far the trunk under each node has been drawn.
    let mut trunk = vec![0.0; n];

    if down {
        // Nodes are in depth-first order, which is the order of an outline's rows.
        let mut row = 0.0;
        for i in 0..n {
            if i > 0 && parent.get(i) == Some(&0) {
                row += ROW_GAP;
            }
            if let (Some(xi), Some(yi)) = (x.get_mut(i), y.get_mut(i)) {
                *xi = depth.get(i).copied().unwrap_or(0) as f64 * INDENT + at(&w, i) / 2.0;
                *yi = row;
            }
            row += at(&h, i) + ROW_GAP;
        }
    } else if let Some(root) = nodes.first() {
        // The first branches go right and the rest left, split where the two sides are closest
        // in height.
        let branches = &root.children;
        let count = branches.len();
        let total: f64 = branches.iter().map(|&c| at(&extent, c)).sum();
        let mut split = count;
        let mut best = f64::INFINITY;
        let mut right = 0.0;
        for k in 1..count {
            right += branches.get(k - 1).map_or(0.0, |&c| at(&extent, c));
            let diff = (2.0 * right - total + (2.0 * k as f64 - count as f64) * BRANCH_GAP_Y).abs();
            if diff < best {
                best = diff;
                split = k;
            }
        }
        for (range, dir) in [(0..split, 1.0), (split..count, -1.0)] {
            let Some(list) = branches.get(range) else {
                continue;
            };
            if list.is_empty() {
                continue;
            }
            let (middle, _) = stack(
                nodes,
                list,
                BRANCH_GAP_Y,
                &mut rel,
                (&extent, &own, &anchor),
            );
            for &c in list {
                if let (Some(b), Some(s)) = (band.get_mut(c), side.get_mut(c)) {
                    *b = at(&rel, c) - middle;
                    *s = dir;
                }
            }
        }
        if let Some(y0) = y.get_mut(0) {
            *y0 = -at(&h, 0) / 2.0;
        }
        for (i, node) in nodes.iter().enumerate() {
            if i > 0 {
                let value = at(&band, i) + at(&own, i);
                if let Some(slot) = y.get_mut(i) {
                    *slot = value;
                }
            }
            for &c in &node.children {
                if i > 0 {
                    let (s, b) = (at(&side, i), at(&band, i) + at(&kids, i) + at(&rel, c));
                    if let (Some(sc), Some(bc)) = (side.get_mut(c), band.get_mut(c)) {
                        *sc = s;
                        *bc = b;
                    }
                }
                let gap = if i > 0 { GAP_X } else { BRANCH_GAP_X };
                let value = at(&x, i) + at(&side, c) * (at(&w, i) / 2.0 + gap + at(&w, c) / 2.0);
                if let Some(slot) = x.get_mut(c) {
                    *slot = value;
                }
            }
        }
    }

    let mut min_x: f64 = 0.0;
    let mut max_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    let mut out = String::new();
    let mut edges = String::new();
    let mut body = String::new();
    let mut color = String::new();
    let flush = |out: &mut String,
                 edges: &mut String,
                 body: &mut String,
                 color: &str,
                 section: Option<usize>| {
        if body.is_empty() {
            return;
        }
        out.push_str("<g class=\"ondine-branch ondine-section-");
        push_num(out, section.unwrap_or(0) as f64);
        out.push_str("\"><g class=\"ondine-edges\" fill=\"none\" stroke=\"");
        out.push_str(color);
        out.push_str("\" stroke-width=\"1.5\">");
        out.push_str(edges);
        out.push_str("</g>");
        out.push_str(body);
        out.push_str("</g>");
        edges.clear();
        body.clear();
    };

    // The root is drawn last, on top of the links that leave it.
    for k in 1..=n {
        let i = k % n;
        let (Some(node), Some(&shape), Some(label)) = (nodes.get(i), shapes.get(i), labels.get(i))
        else {
            continue;
        };
        let (wi, hi) = (at(&w, i), at(&h, i));
        let cx = at(&x, i);
        let top = at(&y, i);
        min_x = min_x.min(cx - wi / 2.0);
        max_x = max_x.max(cx + wi / 2.0);
        min_y = min_y.min(top);
        max_y = max_y.max(top + hi);

        let p = parent.get(i).copied().unwrap_or(0);
        if i == 0 || p == 0 {
            let section = nodes.get(k - 1).and_then(|node| node.section);
            flush(&mut out, &mut edges, &mut body, &color, section);
        }
        if i > 0 && p == 0 {
            color = series_color(i64::try_from(node.section.unwrap_or(0)).unwrap_or(0));
        }
        let (wp, hp) = (at(&w, p), at(&h, p));
        if i > 0 && down {
            // The link drops from under its parent and turns in to the node's left. It starts
            // where the link to the sibling above turned in, so each stretch of a shared trunk
            // has one color. Under the root's rounded end, the trunk starts where that end has
            // come down to the bottom.
            let stem = if p == 0 {
                STEM.max(at(&h, 0) / 2.0).min(INDENT - BEND)
            } else {
                STEM
            };
            let x1 = at(&x, p) - wp / 2.0 + stem.min(wp / 2.0);
            let drawn_to = at(&trunk, p);
            let y1 = if truthy(drawn_to) {
                drawn_to
            } else {
                at(&y, p) + hp
            };
            let left = cx - wi / 2.0;
            let x2 = left
                + if shape.is_empty() {
                    0.0
                } else {
                    shape_inset(shape, wi, hi, 3, 0.0)
                };
            let y2 = top + at(&anchor, i);
            let bend = BEND.min(x2 - x1).min(y2 - y1).max(0.0);
            // The next link carries on from where this one leaves the trunk, so the trunk has no
            // gap.
            if let Some(slot) = trunk.get_mut(p) {
                *slot = y2 - bend;
            }
            edges.push_str("<path class=\"ondine-edge\" d=\"M");
            push_num(&mut edges, x1);
            edges.push(',');
            push_num(&mut edges, y1);
            edges.push('V');
            push_num(&mut edges, y2 - bend);
            edges.push('Q');
            push_num(&mut edges, x1);
            edges.push(',');
            push_num(&mut edges, y2);
            edges.push(' ');
            push_num(&mut edges, x1 + bend);
            edges.push(',');
            push_num(&mut edges, y2);
            edges.push('H');
            push_num(&mut edges, x2);
            edges.push_str("\"/>");
        } else if i > 0 {
            let dir = at(&side, i);
            let from = if dir > 0.0 { 1 } else { 3 };
            let parent_shape = shapes.get(p).copied().unwrap_or("");
            let x1 = at(&x, p)
                + dir
                    * (wp / 2.0
                        - if parent_shape.is_empty() {
                            0.0
                        } else {
                            shape_inset(parent_shape, wp, hp, from, 0.0)
                        });
            let y1 = at(&y, p) + at(&anchor, p);
            let near = cx - (dir * wi) / 2.0;
            let x2 = near
                + if shape.is_empty() {
                    0.0
                } else {
                    dir * shape_inset(shape, wi, hi, 4 - from, 0.0)
                };
            let y2 = top + at(&anchor, i);
            let mid = f64::midpoint(x1, x2);
            edges.push_str("<path class=\"ondine-edge\" d=\"M");
            push_num(&mut edges, x1);
            edges.push(',');
            push_num(&mut edges, y1);
            edges.push('C');
            push_num(&mut edges, mid);
            edges.push(',');
            push_num(&mut edges, y1);
            edges.push(' ');
            push_num(&mut edges, mid);
            edges.push(',');
            push_num(&mut edges, y2);
            edges.push(' ');
            push_num(&mut edges, x2);
            edges.push(',');
            push_num(&mut edges, y2);
            edges.push_str("\"/>");
        }

        let pill = shape.is_empty() && !node.children.is_empty();
        // Every node off the root is filled with a tint of its branch's color and outlined in
        // it, whether its shape was given in the source or is the pill of a node that branches.
        // The root is neutral.
        let tint = [
            " fill=\"",
            &color,
            "\" fill-opacity=\"",
            TINT,
            "\" stroke=\"",
            &color,
            "\"",
        ]
        .concat();
        let mut inner = String::new();
        if !shape.is_empty() {
            if i > 0 {
                let line = [" stroke=\"", &color, "\""].concat();
                inner = draw_shape(shape, wi, hi, &tint, &line);
            } else {
                inner = draw_shape(
                    shape,
                    wi,
                    hi,
                    " fill=\"var(--_s)\" stroke=\"var(--_b)\"",
                    " stroke=\"var(--_b)\"",
                );
            }
        } else if pill {
            inner.push_str("<rect x=\"");
            push_num(&mut inner, -wi / 2.0);
            inner.push_str("\" y=\"");
            push_num(&mut inner, -hi / 2.0);
            inner.push_str("\" width=\"");
            push_num(&mut inner, wi);
            inner.push_str("\" height=\"");
            push_num(&mut inner, hi);
            inner.push_str("\" rx=\"");
            push_num(&mut inner, hi / 2.0);
            inner.push('"');
            inner.push_str(&tint);
            inner.push_str("/>");
        }
        let mut shift = 0.0;
        if let Some(icon) = node.icon.as_deref().filter(|icon| !icon.is_empty())
            && drawn.get(i) == Some(&true)
        {
            shift = f64::midpoint(glyph, if label.width > 0.0 { ICON_GAP } else { 0.0 });
            inner.push_str(&icon_svg(
                icon,
                -shift - label.width / 2.0,
                at(&dy, i) - glyph / 2.0,
                glyph,
                icons,
            ));
        }

        let group = if i > 0 { &mut body } else { &mut out };
        group.push_str("<g class=\"ondine-node ondine-shape-");
        group.push_str(if !shape.is_empty() {
            shape
        } else if pill {
            "branch"
        } else {
            "text"
        });
        if i == 0 {
            group.push_str(" ondine-root");
        }
        group.push_str(&class_names(node.class.as_deref().unwrap_or("")));
        group.push_str("\" data-id=\"");
        group.push_str(&esc_text(&node.node_id));
        group.push_str("\" transform=\"translate(");
        push_num(group, cx);
        group.push(',');
        push_num(group, top + hi / 2.0);
        group.push_str(")\">");
        group.push_str(&inner);
        if down && shape.is_empty() {
            group.push_str(&left_label(
                label,
                shift - label.width / 2.0,
                at(&dy, i),
                icons,
            ));
        } else {
            group.push_str(&label_svg(label, shift, at(&dy, i), LABEL_ATTRS, icons));
        }
        group.push_str("</g>");
    }

    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        title_room(options),
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let width = (max_x - min_x).max(title.width);
    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g transform=\"translate(");
    push_num(
        &mut content,
        pad - min_x + f64::midpoint(width - max_x, min_x),
    );
    content.push(',');
    push_num(&mut content, pad + title_height - min_y);
    content.push_str(")\">");
    content.push_str(&out);
    content.push_str("</g>");

    Rendered {
        height: total_height,
        links: Vec::new(),
        svg: svg_document(
            "mindmap",
            total_width,
            total_height,
            size,
            options,
            Accessible::default(),
            &content,
        ),
        width: total_width,
    }
}
