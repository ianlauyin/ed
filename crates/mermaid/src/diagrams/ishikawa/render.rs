//! Draws an Ishikawa diagram. Port of Pele's `src/diagrams/ishikawa/render.ts`.

use super::db::IshikawaDb;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{title_room, turn_to_fit};
use crate::diagrams::common::literal_label::break_label;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::edges::{ARROW, marker};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::RADIUS;
use crate::text::label::{Label, layout_label};
use crate::text::measure::BOLD;
use crate::yaml::push_number;

/// How far a bone moves toward the tail for each unit it rises from the spine.
const SLANT: f64 = 0.5;
const FIRST_ROW: f64 = 14.0;
const ROW_GAP: f64 = 6.0;
const BONE_END: f64 = 8.0;
const TWIG_STEP: f64 = 22.0;
const STUB: f64 = 18.0;
const TEXT_GAP: f64 = 5.0;
const PAIR_GAP: f64 = 26.0;
const HEAD_GAP: f64 = 34.0;
const TAIL: f64 = 28.0;
const BOX_PAD_X: f64 = 10.0;
const BOX_PAD_Y: f64 = 5.0;

/// Pele's `Math.hypot(SLANT, 1)`: V8 sums the squares of each value over the larger, 1 here.
fn length() -> f64 {
    (SLANT * SLANT + 1.0).sqrt()
}

/// A cause below a category. Bones at even depths run level, from the slanted bone above them
/// toward the tail. Bones at odd depths slant away from the spine, from the level bone above them.
struct Bone {
    /// Where it meets its parent, measured level from the category's bone: 0 on it, negative
    /// toward the tail.
    at: f64,
    depth: usize,
    kids: usize,
    label: Label,
    parent: Option<usize>,
    /// How far its row is from the spine.
    rise: f64,
    /// Its place among its siblings, counted from the spine outward.
    slot: usize,
    /// Where its text ends.
    text_end: f64,
}

struct Category<'a> {
    bones: Vec<Bone>,
    box_h: f64,
    box_w: f64,
    label: Label,
    /// How far its causes reach toward the tail, measured level from its bone.
    reach: f64,
    /// The length of its bone, as distance from the spine.
    rise: f64,
    text: &'a str,
}

struct Frame {
    /// Set when the bone was made and only its row is still to come.
    bone: Option<usize>,
    depth: usize,
    node: usize,
    parent: Option<usize>,
    slot: usize,
}

fn usize_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// Gives every cause of a category a row of its own, so that no two texts can meet, and measures
/// how far the whole of it reaches. A level bone takes the row nearest the spine and its twigs
/// the rows beyond. A slanted twig takes the row beyond everything that hangs off it. The outline
/// may be nested as deep as its indentation goes, so the walk keeps a stack of its own.
fn arrange(model: &IshikawaDb, node: usize, up: bool, size: f64, small: f64) -> Category<'_> {
    let text = model.nodes.get(node).map_or("", |n| n.text.as_str());
    let label = layout_label(Some(text), false, size, 180.0, 0, None);
    let line_height = js_round(small * 1.5);
    let mut bones: Vec<Bone> = Vec::new();
    let mut rows: Vec<usize> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();

    // Above the spine the first cause goes farthest out, so that the causes read from the top down.
    let push = |stack: &mut Vec<Frame>, node: usize, depth: usize, parent: Option<usize>| {
        let children = model.nodes.get(node).map_or(&[][..], |n| &n.children[..]);
        let n = children.len();
        for slot in (0..n).rev() {
            let child = if up { n - 1 - slot } else { slot };
            if let Some(&child) = children.get(child) {
                stack.push(Frame {
                    bone: None,
                    depth,
                    node: child,
                    parent,
                    slot,
                });
            }
        }
    };
    push(&mut stack, node, 2, None);
    while let Some(mut frame) = stack.pop() {
        if let Some(bone) = frame.bone {
            rows.push(bone);
            continue;
        }
        let index = bones.len();
        let own = model.nodes.get(frame.node);
        bones.push(Bone {
            at: 0.0,
            depth: frame.depth,
            kids: own.map_or(0, |n| n.children.len()),
            label: layout_label(own.map(|n| n.text.as_str()), false, small, 200.0, 0, None),
            parent: frame.parent,
            rise: 0.0,
            slot: frame.slot,
            text_end: 0.0,
        });
        let (child, depth) = (frame.node, frame.depth);
        if depth % 2 == 0 {
            rows.push(index);
        } else {
            frame.bone = Some(index);
            stack.push(frame);
        }
        push(&mut stack, child, depth + 1, Some(index));
    }

    let mut rise = FIRST_ROW;
    for &index in &rows {
        if let Some(bone) = bones.get_mut(index) {
            let height = bone.label.height.max(line_height) + ROW_GAP;
            bone.rise = rise + height / 2.0;
            rise += height;
        }
    }
    let box_w = label.width + 2.0 * BOX_PAD_X;
    let mut reach = box_w / 2.0;
    for index in 0..bones.len() {
        let parent = bones
            .get(index)
            .and_then(|bone| bone.parent)
            .and_then(|parent| bones.get(parent))
            .map(|parent| (parent.at, parent.kids));
        let Some(bone) = bones.get_mut(index) else {
            continue;
        };
        if bone.depth % 2 == 0 {
            bone.at = parent.map_or(0.0, |(at, _)| at);
            bone.text_end = bone.at
                - if bone.kids > 0 {
                    usize_f64(bone.kids + 1) * TWIG_STEP
                } else {
                    STUB
                }
                - TEXT_GAP;
        } else {
            // Twigs stand along their level bone, the one nearest the spine nearest the tail.
            let (at, kids) = parent.unwrap_or((0.0, 0));
            bone.at = at - (usize_f64(kids) - usize_f64(bone.slot)) * TWIG_STEP;
            bone.text_end = bone.at - TEXT_GAP;
        }
        reach = reach.max(bone.label.width - bone.text_end);
    }
    Category {
        bones,
        box_h: label.height + 2.0 * BOX_PAD_Y,
        box_w,
        label,
        reach,
        rise: rise + BONE_END,
        text,
    }
}

/// Draws an Ishikawa diagram, turned to run down when it does not fit the host's width.
pub(crate) fn render(model: &IshikawaDb, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |down| draw(model, options, down))
}

fn draw(model: &IshikawaDb, options: &RenderOptions, down: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);
    if down {
        return vertical_causes(model, options);
    }
    let title = layout_label(model.title.as_deref(), false, size, 4000.0, BOLD, None);
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };

    let mut body = String::new();
    let mut min_x = 0.0_f64;
    let mut max_x = 0.0_f64;
    let mut min_y = 0.0_f64;
    let mut max_y = 0.0_f64;
    if let Some(root) = model.root() {
        let head = layout_label(Some(&root.text), false, size, 200.0, BOLD, None);
        let head_h = (head.height + 24.0).max(48.0);
        let head_w = head.width + 24.0 + head_h / 2.0;
        let mut categories: Vec<Category<'_>> = root
            .children
            .iter()
            .enumerate()
            .map(|(i, &node)| arrange(model, node, i % 2 == 0, size, small))
            .collect();
        // Even a category without causes has a bone long enough to carry its name clear of the head.
        for category in &mut categories {
            category.rise = category.rise.max(head_h / 2.0 + 14.0);
        }

        // Categories meet the spine in pairs, one above and one below, each pair as far toward the
        // tail from the one before as the wider of the two needs.
        let mut xs: Vec<f64> = Vec::new();
        let mut x = -HEAD_GAP;
        let mut i = 0;
        while i < categories.len() {
            if i > 0 {
                let mut room = 0.0_f64;
                for side in 0..2 {
                    let before = categories.get(i - 2 + side);
                    let next = categories.get(i + side);
                    if let Some(before) = before {
                        room = room.max(before.reach + next.map_or(0.0, |next| next.box_w / 2.0));
                    }
                }
                x -= room + PAIR_GAP;
            }
            xs.push(x);
            xs.push(x);
            i += 2;
        }
        let tail = if categories.is_empty() {
            -HEAD_GAP
        } else {
            x - TAIL
        };

        max_x = head_w;
        min_x = tail;
        min_y = -head_h / 2.0;
        max_y = head_h / 2.0;
        let length = length();
        let mut groups = String::new();
        for (i, category) in categories.iter().enumerate() {
            let sign = if i % 2 == 0 { -1.0 } else { 1.0 };
            let ax = xs.get(i).copied().unwrap_or(0.0);
            let px = |at: f64, rise: f64| ax + at - rise * SLANT;
            let end_x = px(0.0, category.rise);
            let end_y = sign * category.rise;
            // The bone stops short of the spine to leave its arrowhead a clean point.
            let trim = ARROW - 1.0;
            let mut d = String::from("M");
            push_num(&mut d, ax - (SLANT / length) * trim);
            d.push(',');
            push_num(&mut d, (sign * trim) / length);
            d.push('L');
            push_num(&mut d, end_x);
            d.push(',');
            push_num(&mut d, end_y);
            let mut texts = String::new();
            for bone in &category.bones {
                let y = sign * bone.rise;
                if bone.depth % 2 == 0 {
                    d.push('M');
                    push_num(&mut d, px(bone.at, bone.rise));
                    d.push(',');
                    push_num(&mut d, y);
                    d.push('H');
                    push_num(&mut d, px(bone.text_end + TEXT_GAP, bone.rise));
                } else {
                    let from = bone
                        .parent
                        .and_then(|parent| category.bones.get(parent))
                        .map_or(f64::NAN, |parent| parent.rise);
                    d.push('M');
                    push_num(&mut d, px(bone.at, from));
                    d.push(',');
                    push_num(&mut d, sign * from);
                    d.push('L');
                    push_num(&mut d, px(bone.at, bone.rise));
                    d.push(',');
                    push_num(&mut d, y);
                }
                let right = px(bone.text_end, bone.rise);
                texts.push_str(&label_svg(
                    &bone.label,
                    right - bone.label.width / 2.0,
                    y,
                    if bone.depth > 2 {
                        " fill=\"var(--_m)\""
                    } else {
                        ""
                    },
                    None,
                ));
                min_x = min_x.min(right - bone.label.width);
            }
            let box_y = end_y + sign * (category.box_h / 2.0);
            min_x = min_x.min(end_x - category.box_w / 2.0);
            max_x = max_x.max(end_x + category.box_w / 2.0);
            min_y = min_y.min(box_y - category.box_h / 2.0);
            max_y = max_y.max(box_y + category.box_h / 2.0);

            groups.push_str("<g class=\"ondine-ishikawa-category\" data-id=\"");
            groups.push_str(&esc_text(category.text));
            groups.push_str("\"><path class=\"ondine-ishikawa-bones\" d=\"");
            groups.push_str(&d);
            groups.push_str("\" fill=\"none\" stroke=\"var(--_l)\"/>");
            groups.push_str(&marker(
                "arrow_point",
                ax,
                0.0,
                SLANT / length,
                -sign / length,
                "var(--_l)",
            ));
            groups.push_str("<rect x=\"");
            push_num(&mut groups, end_x - category.box_w / 2.0);
            groups.push_str("\" y=\"");
            push_num(&mut groups, box_y - category.box_h / 2.0);
            groups.push_str("\" width=\"");
            push_num(&mut groups, category.box_w);
            groups.push_str("\" height=\"");
            push_num(&mut groups, category.box_h);
            groups.push_str("\" rx=\"");
            groups.push_str(RADIUS);
            groups.push_str("\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>");
            groups.push_str(&label_svg(
                &category.label,
                end_x,
                box_y,
                " class=\"ondine-label\"",
                None,
            ));
            if !texts.is_empty() {
                groups.push_str("<g class=\"ondine-ishikawa-causes\" font-size=\"");
                push_number(&mut groups, small);
                groups.push_str("\">");
                groups.push_str(&texts);
                groups.push_str("</g>");
            }
            groups.push_str("</g>");
        }

        let r = head_h / 2.0;
        body.push_str("<path class=\"ondine-ishikawa-spine\" d=\"M");
        push_num(&mut body, tail);
        body.push_str(",0H");
        push_num(&mut body, -(ARROW - 1.0));
        body.push_str("\" fill=\"none\" stroke=\"var(--_l)\"/>");
        body.push_str(&marker("arrow_point", 0.0, 0.0, 1.0, 0.0, "var(--_l)"));
        body.push_str(&groups);
        body.push_str("<g class=\"ondine-ishikawa-head\" data-id=\"");
        body.push_str(&esc_text(&root.text));
        body.push_str("\"><path d=\"M0,");
        push_num(&mut body, -r);
        body.push('H');
        push_num(&mut body, head_w - r);
        body.push('A');
        push_num(&mut body, r);
        body.push(',');
        push_num(&mut body, r);
        body.push_str(" 0 0 1 ");
        push_num(&mut body, head_w - r);
        body.push(',');
        push_num(&mut body, r);
        body.push_str("H0Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/>");
        body.push_str(&label_svg(
            &head,
            12.0 + head.width / 2.0,
            0.0,
            " class=\"ondine-label\" font-weight=\"var(--_hw)\"",
            None,
        ));
        body.push_str("</g>");
    }

    let inner = (max_x - min_x).max(title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = pad + (inner - (max_x - min_x)) / 2.0 - min_x;
    let oy = pad + title_height - min_y;
    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !body.is_empty() {
        content.push_str("<g transform=\"translate(");
        push_num(&mut content, ox);
        content.push(',');
        push_num(&mut content, oy);
        content.push_str(")\">");
        content.push_str(&body);
        content.push_str("</g>");
    }
    let svg = svg_document(
        "ishikawa",
        width,
        height,
        size,
        options,
        Accessible::default(),
        &content,
    );
    Rendered {
        height,
        links: Vec::new(),
        svg,
        width,
    }
}

/// A line of the outline drawn running down.
struct Entry {
    depth: usize,
    h: f64,
    label: Label,
    node: usize,
    parent: Option<usize>,
    w: f64,
    x: f64,
    y: f64,
}

fn vertical_causes(model: &IshikawaDb, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);
    let room = 140.0_f64.max(options.max_width.unwrap_or(320.0) - 2.0 * pad);
    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        title_room(options),
        BOLD,
        None,
    );

    let mut order: Vec<Entry> = Vec::new();
    let mut pending: Vec<(usize, usize, Option<usize>)> = if model.root().is_some() {
        vec![(0, 0, None)]
    } else {
        Vec::new()
    };
    let mut depth = 0;
    while let Some((node, own, parent)) = pending.pop() {
        let index = order.len();
        order.push(Entry {
            depth: own,
            h: 0.0,
            label: Label::default(),
            node,
            parent,
            w: 0.0,
            x: 0.0,
            y: 0.0,
        });
        depth = depth.max(own);
        if let Some(entry) = model.nodes.get(node) {
            for &child in entry.children.iter().rev() {
                pending.push((child, own + 1, Some(index)));
            }
        }
    }

    let indent = 20.0_f64.min(2.0_f64.max((room - 120.0) / usize_f64(depth.max(1))));
    let mut y = pad
        + if title.height > 0.0 {
            title.height + 12.0
        } else {
            0.0
        };
    let mut width = title.width;
    let mut nodes = String::new();
    let mut edges = String::new();
    for index in 0..order.len() {
        let Some(entry) = order.get_mut(index) else {
            continue;
        };
        let text = model.nodes.get(entry.node).map_or("", |n| n.text.as_str());
        let depth = usize_f64(entry.depth);
        entry.x = pad + depth * indent;
        let label_room = 60.0_f64.max(room - depth * indent - 20.0);
        let (font, style) = if entry.depth < 2 {
            (size, BOLD)
        } else {
            (small, 0)
        };
        entry.label = break_label(
            &layout_label(Some(text), false, font, label_room, style, None),
            label_room,
        );
        entry.w = entry.label.width + 20.0;
        entry.h = entry.label.height + 12.0;
        entry.y = y;
        y += entry.h + 12.0;
        width = width.max(entry.x - pad + entry.w);

        let framed = entry.depth < 2;
        nodes.push_str("<g class=\"ondine-node ");
        nodes.push_str(match entry.depth {
            0 => "ondine-ishikawa-head",
            1 => "ondine-ishikawa-category",
            _ => "ondine-ishikawa-cause",
        });
        nodes.push_str("\" data-id=\"");
        nodes.push_str(&esc_text(text));
        nodes.push_str("\">");
        if framed {
            nodes.push_str("<rect x=\"");
            push_num(&mut nodes, entry.x);
            nodes.push_str("\" y=\"");
            push_num(&mut nodes, entry.y);
            nodes.push_str("\" width=\"");
            push_num(&mut nodes, entry.w);
            nodes.push_str("\" height=\"");
            push_num(&mut nodes, entry.h);
            nodes.push_str(if entry.depth == 0 {
                "\" rx=\"12\""
            } else {
                "\" rx=\"4\""
            });
            nodes.push_str(" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>");
        }
        let mut attrs = String::from(" class=\"ondine-label\"");
        if entry.depth < 2 {
            attrs.push_str(" font-weight=\"var(--_hw)\"");
        }
        if entry.depth > 1 {
            attrs.push_str(" font-size=\"");
            push_number(&mut attrs, small);
            attrs.push('"');
        }
        nodes.push_str(&label_svg(
            &entry.label,
            entry.x + entry.w / 2.0,
            entry.y + entry.h / 2.0,
            &attrs,
            None,
        ));
        nodes.push_str("</g>");

        let (x, top, h) = (entry.x, entry.y, entry.h);
        if let Some(parent) = entry.parent.and_then(|parent| order.get(parent)) {
            let gx = parent.x + indent / 2.0;
            edges.push_str("<path class=\"ondine-edge\" d=\"M");
            push_num(&mut edges, gx);
            edges.push(',');
            push_num(&mut edges, parent.y + parent.h);
            edges.push('V');
            push_num(&mut edges, top + h / 2.0);
            edges.push('H');
            push_num(&mut edges, x);
            edges.push_str("\" fill=\"none\" stroke=\"var(--_b)\"/>");
        }
    }

    let total_width = (width + 2.0 * pad).ceil();
    let height = ((pad + title.height).max(y - 12.0) + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&edges);
    content.push_str(&nodes);
    Rendered {
        height,
        links: Vec::new(),
        svg: svg_document(
            "ishikawa",
            total_width,
            height,
            size,
            options,
            Accessible::default(),
            &content,
        ),
        width: total_width,
    }
}
