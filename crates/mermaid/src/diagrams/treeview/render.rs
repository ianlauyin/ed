//! Draws a tree view: one row per entry, indented under its folder, with guide lines. Port of
//! Pele's `src/diagrams/treeview/render.ts`.

use super::icons::{BUILTIN_PACK, get_node_icon, glyph, icon_config};
use super::model::{NodeType, TreeViewModel};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::diagrams::common::literal_label::{left_label_svg, literal_label};
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{RADIUS, class_names};
use crate::text::entities::decode;
use crate::text::label::{Label, layout_label};
use crate::text::measure::{self, BOLD, ITALIC};
use crate::util::chars::is_space;
use crate::yaml::Value;

/// Space between an icon and its label.
const ICON_GAP: f64 = 6.0;
/// Space between the widest label and the column of descriptions.
const DESC_GAP: f64 = 16.0;
/// Where a guide line runs, right of its folder's start.
const GUIDE_INSET: f64 = 7.0;
/// The radius of a guide line's last turn.
const ELBOW: f64 = 4.0;
/// How far a highlight reaches past its row.
const HIGHLIGHT_OVERHANG: f64 = 4.0;

/// The `treeView` option `key` of `config` when it is a number from `min` to `max`, or
/// `fallback`.
fn setting(config: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    let Value::Object(entries) = config else {
        return fallback;
    };
    let value = entries
        .iter()
        .find(|(own, _)| own == "treeView")
        .and_then(|(_, section)| match section {
            Value::Object(entries) => entries
                .iter()
                .find(|(own, _)| own == key)
                .map(|(_, value)| value),
            _ => None,
        });
    match value {
        Some(Value::Number(value)) if *value >= min && *value <= max => *value,
        _ => fallback,
    }
}

/// Mermaid's layout options, with Pele's defaults: Pele's `treeSettings`.
struct Settings {
    /// The width of guide lines.
    line_thickness: f64,
    /// Space between a guide's tick and the entries it leads to.
    padding_x: f64,
    /// Space above and below each row's text.
    padding_y: f64,
    /// How far each level is indented, besides `padding_x`.
    row_indent: f64,
}

/// The layout options of `config`.
fn tree_settings(config: &Value) -> Settings {
    Settings {
        line_thickness: setting(config, "lineThickness", 1.0, 0.25, 12.0),
        padding_x: setting(config, "paddingX", 5.0, 0.0, 100.0),
        padding_y: setting(config, "paddingY", 2.0, 0.0, 100.0),
        row_indent: setting(config, "rowIndent", 15.0, 0.0, 400.0),
    }
}

/// A drawn row.
struct Row {
    /// The middle of the row's first line, from the top of the rows.
    cy: f64,
    /// The description, laid out when drawing tightly.
    description: Option<Label>,
    /// The row's height.
    height: f64,
    /// Whether the entry has the `highlight` class.
    highlighted: bool,
    /// The qualified icon name, when the entry shows one.
    icon: Option<String>,
    /// The name, laid out when drawing tightly.
    label: Option<Label>,
    /// Where the name starts.
    label_x: f64,
    /// The entry's index in the model.
    node: usize,
    /// The name, with entities decoded.
    text: String,
    /// The row's top, from the top of the rows.
    top: f64,
    /// Where the row starts, by its depth.
    x: f64,
}

/// Draws `model` with the `treeView` section of `config`.
pub(crate) fn render(model: &TreeViewModel, config: &Value, options: &RenderOptions) -> Rendered {
    tighten(options, |tight| draw(model, config, options, tight))
}

/// Draws `model` with room to spare, or `tight`ly within the host's width.
fn draw(model: &TreeViewModel, config: &Value, options: &RenderOptions, tight: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let pad = options.padding.unwrap_or(8.0);
    let max_width = options.max_width.unwrap_or(320.0);
    let icons = icon_config(config);

    let Settings {
        line_thickness,
        padding_x,
        padding_y,
        row_indent,
    } = tree_settings(config);
    let mut max_depth = 0usize;
    if tight {
        let mut depths: Vec<(usize, usize)> = model.roots().iter().map(|&node| (node, 0)).collect();
        let mut i = 0;
        while let Some(&(node, depth)) = depths.get(i) {
            max_depth = max_depth.max(depth);
            if let Some(node) = model.node(node) {
                depths.extend(node.children.iter().map(|&child| (child, depth + 1)));
            }
            i += 1;
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "depths stay far below 2^52")]
    let step = if tight {
        (row_indent + padding_x).min(2.0_f64.max((max_width - 120.0) / (max_depth.max(1) as f64)))
    } else {
        row_indent + padding_x
    };
    let row_height = js_round(size * 1.5) + 2.0 * padding_y;
    let icon_size = size;

    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        if tight { title_room(options) } else { 4000.0 },
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };

    // Siblings line up: when one entry of a folder has an icon, the others leave room for one.
    let has_icon = |list: &[usize]| {
        list.iter().any(|&child| {
            model
                .node(child)
                .is_some_and(|child| get_node_icon(child, &icons).is_some())
        })
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut row_of = vec![usize::MAX; model.nodes.len()];
    let mut pending: Vec<(usize, usize, bool)> = Vec::new();
    let push = |pending: &mut Vec<(usize, usize, bool)>, list: &[usize], depth: usize| {
        let slot = has_icon(list);
        for &node in list.iter().rev() {
            pending.push((node, depth, slot));
        }
    };
    push(&mut pending, model.roots(), 0);
    let mut label_right: f64 = 0.0;
    let mut any_highlight = false;
    let mut rows_height = 0.0;
    while let Some((index, depth, slot)) = pending.pop() {
        let Some(node) = model.node(index) else {
            continue;
        };
        let directory = node.node_type == NodeType::Directory;
        let style = if directory { BOLD } else { 0 };
        #[expect(clippy::cast_precision_loss, reason = "depths stay far below 2^52")]
        let x = depth as f64 * step;
        let text = decode(&node.name).into_owned();
        let label_x = x + if slot { icon_size + ICON_GAP } else { 0.0 };
        let room = 40.0_f64.max(max_width - 2.0 * pad - 2.0 * HIGHLIGHT_OVERHANG - label_x);
        let label = tight.then(|| literal_label(&text, size, room, style));
        let description = node
            .description
            .as_deref()
            .filter(|_| tight)
            .map(|description| literal_label(&decode(description), size, room, ITALIC));
        let width = label
            .as_ref()
            .map_or_else(|| measure::width(&text, size, style), |label| label.width);
        if label_x + width > label_right {
            label_right = label_x + width;
        }
        let highlighted = node.css_class.as_deref().is_some_and(|class| {
            class
                .split(|c: char| is_space(u32::from(c)))
                .any(|word| word == "highlight")
        });
        any_highlight |= highlighted;
        let label_height = label.as_ref().map_or(0.0, |label| label.height);
        let height = if tight {
            row_height.max(
                label_height
                    + description
                        .as_ref()
                        .map_or(0.0, |description| description.height)
                    + 2.0 * padding_y,
            )
        } else {
            row_height
        };
        let cy = rows_height
            + if tight {
                padding_y + icon_size.max(label_height) / 2.0
            } else {
                height / 2.0
            };
        if let Some(slot) = row_of.get_mut(index) {
            *slot = rows.len();
        }
        rows.push(Row {
            cy,
            description,
            height,
            highlighted,
            icon: get_node_icon(node, &icons),
            label,
            label_x,
            node: index,
            text,
            top: rows_height,
            x,
        });
        rows_height += height;
        push(&mut pending, &node.children, depth + 1);
    }

    let desc_x = label_right + DESC_GAP;
    let mut width = label_right;
    for row in &rows {
        let Some(description) = model
            .node(row.node)
            .and_then(|node| node.description.as_deref())
        else {
            continue;
        };
        let w = row.description.as_ref().map_or_else(
            || measure::width(&decode(description), size, ITALIC),
            |label| label.width,
        );
        width = width.max(if tight { row.label_x } else { desc_x } + w);
    }
    width = width.max(title.width);
    let inset = pad
        + if any_highlight {
            HIGHLIGHT_OVERHANG
        } else {
            0.0
        };
    let top = pad + title_height;
    let total_width = (width + 2.0 * inset).ceil();
    let total_height = (top + rows_height + pad).ceil();

    let cy_of = |node: usize| {
        row_of
            .get(node)
            .and_then(|&row| rows.get(row))
            .map_or(f64::NAN, |row| row.cy)
    };
    let mut guides = String::new();
    for row in &rows {
        let Some(node) = model.node(row.node) else {
            continue;
        };
        let Some(&last_child) = node.children.last() else {
            continue;
        };
        let gx = inset + row.x + GUIDE_INSET;
        let tick = gx.max(inset + row.x + step - padding_x);
        let last = top + cy_of(last_child);
        let r = ELBOW.min(tick - gx);
        guides.push_str("<path d=\"M");
        push_num(&mut guides, gx);
        guides.push(',');
        push_num(&mut guides, top + row.top + row.height - 2.0);
        guides.push('V');
        push_num(&mut guides, last - r);
        if r > 0.0 {
            guides.push('Q');
            push_num(&mut guides, gx);
            guides.push(',');
            push_num(&mut guides, last);
            guides.push(' ');
            push_num(&mut guides, gx + r);
            guides.push(',');
            push_num(&mut guides, last);
            guides.push('H');
            push_num(&mut guides, tick);
        }
        if tick > gx {
            for &child in node.children.iter().take(node.children.len() - 1) {
                guides.push('M');
                push_num(&mut guides, gx);
                guides.push(',');
                push_num(&mut guides, top + cy_of(child));
                guides.push('H');
                push_num(&mut guides, tick);
            }
        }
        guides.push_str("\"/>");
    }

    let baseline = size * 0.35;
    let mut body = String::new();
    for row in &rows {
        let Some(node) = model.node(row.node) else {
            continue;
        };
        let cy = top + row.cy;
        let directory = node.node_type == NodeType::Directory;
        let mut inner = String::new();
        if row.highlighted {
            let x = inset + row.x - HIGHLIGHT_OVERHANG;
            inner.push_str("<rect class=\"ondine-tree-highlight\" x=\"");
            push_num(&mut inner, x);
            inner.push_str("\" y=\"");
            push_num(&mut inner, top + row.top + 1.0);
            inner.push_str("\" width=\"");
            push_num(&mut inner, total_width - inset + HIGHLIGHT_OVERHANG - x);
            inner.push_str("\" height=\"");
            push_num(&mut inner, row.height - 2.0);
            inner.push_str("\" rx=\"");
            inner.push_str(RADIUS);
            inner.push_str("\" fill=\"var(--_c)\" fill-opacity=\"0.14\"/>");
        }
        if let Some(icon) = &row.icon {
            let name = icon
                .strip_prefix(BUILTIN_PACK)
                .and_then(|rest| rest.strip_prefix(':'))
                .unwrap_or("");
            let path = glyph(name)
                .or_else(|| glyph(if directory { "folder" } else { "file" }))
                .unwrap_or("");
            inner.push_str("<svg class=\"ondine-icon\" data-icon=\"");
            inner.push_str(&esc_text(icon));
            inner.push_str("\" x=\"");
            push_num(&mut inner, inset + row.x);
            inner.push_str("\" y=\"");
            push_num(&mut inner, cy - icon_size / 2.0);
            inner.push_str("\" width=\"");
            push_num(&mut inner, icon_size);
            inner.push_str("\" height=\"");
            push_num(&mut inner, icon_size);
            inner.push_str("\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"");
            inner.push_str(path);
            inner.push_str("\" stroke=\"var(--_m)\" stroke-width=\"1.5\"/></svg>");
        }
        let weight = if directory {
            " font-weight=\"var(--_hw)\""
        } else {
            ""
        };
        if let Some(label) = &row.label {
            let mut attrs = String::from(" class=\"ondine-label\"");
            attrs.push_str(weight);
            inner.push_str(&left_label_svg(
                label,
                inset + row.label_x,
                top + row.top + padding_y,
                &attrs,
            ));
        } else if !row.text.is_empty() {
            inner.push_str("<text class=\"ondine-label\" x=\"");
            push_num(&mut inner, inset + row.label_x);
            inner.push_str("\" y=\"");
            push_num(&mut inner, cy + baseline);
            inner.push('"');
            inner.push_str(weight);
            inner.push_str(" xml:space=\"preserve\">");
            inner.push_str(&esc(&row.text));
            inner.push_str("</text>");
        }
        if let Some(description) = &row.description {
            inner.push_str(&left_label_svg(
                description,
                inset + row.label_x,
                top + row.top + padding_y + row.label.as_ref().map_or(0.0, |label| label.height),
                " class=\"ondine-tree-description\" font-style=\"italic\" fill=\"var(--_m)\"",
            ));
        } else if let Some(description) = &node.description {
            inner.push_str("<text class=\"ondine-tree-description\" x=\"");
            push_num(&mut inner, inset + desc_x);
            inner.push_str("\" y=\"");
            push_num(&mut inner, cy + baseline);
            inner.push_str("\" font-style=\"italic\" fill=\"var(--_m)\" xml:space=\"preserve\">");
            inner.push_str(&esc(&decode(description)));
            inner.push_str("</text>");
        }
        body.push_str("<g class=\"ondine-node ondine-tree-");
        body.push_str(if directory { "dir" } else { "file" });
        body.push_str(&class_names(node.css_class.as_deref().unwrap_or("")));
        body.push_str("\" data-id=\"");
        body.push_str(&esc_text(&node.name));
        body.push_str("\">");
        body.push_str(&inner);
        body.push_str("</g>");
    }

    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !guides.is_empty() {
        content.push_str(
            "<g class=\"ondine-tree-guides\" fill=\"none\" stroke=\"var(--_b)\" stroke-width=\"",
        );
        push_num(&mut content, line_thickness);
        content.push_str("\">");
        content.push_str(&guides);
        content.push_str("</g>");
    }
    content.push_str(&body);

    Rendered {
        height: total_height,
        links: Vec::new(),
        svg: svg_document(
            "treeView",
            total_width,
            total_height,
            size,
            options,
            Accessible {
                acc_descr: model.acc_descr.as_deref(),
                acc_title: model.acc_title.as_deref(),
            },
            &content,
        ),
        width: total_width,
    }
}
