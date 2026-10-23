//! Draws a treemap: sections with headers and leaves with names and values. Port of Pele's
//! `src/diagrams/treemap/render.ts`.

use super::format::{format, formatter};
use super::layout::{Insets, layout_treemap};
use super::model::TreemapModel;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_text::fit_text;
use crate::diagrams::common::fit_width::{cap_width, title_room};
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{RADIUS, class_names, resolve_style, series_color};
use crate::text::entities::decode;
use crate::text::label::{layout_label, trim};
use crate::text::measure::{self, BOLD};
use crate::yaml::{Value, push_number};

/// The height of a section header.
const HEADER: f64 = 28.0;
/// The space a section keeps around its children below its header.
const SIDE: f64 = 6.0;
/// Line height of leaf text, as a multiple of its size.
const LINE: f64 = 1.25;

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

/// The `treemap` option `key` of `config` when it is a number at least `min`, capped at `max`, or
/// `fallback`.
fn option(config: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match property(config, "treemap").and_then(|treemap| property(treemap, key)) {
        Some(Value::Number(value)) if *value >= min => value.min(max),
        _ => fallback,
    }
}

/// Draws `model` with the `treemap` section of `config`.
pub(crate) fn render(model: &TreemapModel, config: &Value, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let tiny = js_round(size * 0.75);
    let least = js_round(size * 0.5625);
    let treemap = property(config, "treemap");
    let pad = options
        .padding
        .unwrap_or_else(|| option(config, "diagramPadding", 8.0, 0.0, 2000.0));

    // Mermaid sizes the diagram as ten times `nodeWidth` by ten times `nodeHeight`.
    let width = js_round(cap_width(
        options,
        option(config, "nodeWidth", 64.0, 1.0, 400.0) * 10.0,
        200.0,
        2.0 * pad,
    ));
    let height = js_round(option(config, "nodeHeight", 40.0, 1.0, 400.0) * 10.0);
    let show_values = !matches!(
        treemap.and_then(|treemap| property(treemap, "showValues")),
        Some(Value::Bool(false))
    );
    let spec = match treemap.and_then(|treemap| property(treemap, "valueFormat")) {
        Some(Value::String(spec)) => formatter(spec),
        _ => formatter(","),
    };
    let (boxes, placed) = layout_treemap(
        &model.roots,
        width,
        height,
        Insets {
            gap: option(config, "padding", 4.0, 0.0, 200.0),
            header: HEADER,
            side: SIDE,
        },
    );

    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        title_room(options),
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 8.0
    } else {
        0.0
    };
    let left = pad + 0.0_f64.max((title.width - width) / 2.0);
    let top = pad + title_height;

    let mut sections = String::new();
    let mut leaves = String::new();
    for b in placed.iter().filter_map(|&k| boxes.get(k)) {
        let node = b.node;
        let style = resolve_style(node.css_compiled_styles.as_deref().unwrap_or(&[]));
        let color = series_color(i64::try_from(b.series).unwrap_or(0));
        let x = left + b.x;
        let y = top + b.y;
        let decoded = decode(&node.name);
        let name = trim(&decoded);
        let value = if show_values {
            format(&spec, b.value)
        } else {
            String::new()
        };
        let classes = class_names(node.class_selector.as_deref().unwrap_or(""));

        let mut rect = String::from("data-id=\"");
        rect.push_str(&esc_text(&node.name));
        rect.push_str("\"><rect x=\"");
        push_num(&mut rect, x);
        rect.push_str("\" y=\"");
        push_num(&mut rect, y);
        rect.push_str("\" width=\"");
        push_num(&mut rect, b.w);
        rect.push_str("\" height=\"");
        push_num(&mut rect, b.h);
        rect.push_str("\" rx=\"");
        rect.push_str(RADIUS);
        rect.push_str("\" fill=\"");
        rect.push_str(&color);
        rect.push('"');

        if node.children.is_some() {
            let mut text = String::new();
            if b.h >= HEADER {
                let room = b.w - 2.0 * SIDE - 4.0;
                let value_width = if value.is_empty() {
                    0.0
                } else {
                    measure::width(&value, tiny, 0)
                };
                let with_value = value_width > 0.0 && room - value_width - 12.0 >= 3.0 * small;
                let label = fit_text(
                    name,
                    if with_value {
                        room - value_width - 12.0
                    } else {
                        room
                    },
                    small,
                    small,
                    BOLD,
                );
                let baseline = y + HEADER / 2.0 + 1.0;
                if let Some(label) = label {
                    text.push_str("<text x=\"");
                    push_num(&mut text, x + SIDE + 2.0);
                    text.push_str("\" y=\"");
                    push_num(&mut text, baseline + small * 0.35);
                    text.push_str("\" font-weight=\"var(--_hw)\"");
                    text.push_str(&style.text);
                    text.push('>');
                    text.push_str(&esc(&label.text));
                    text.push_str("</text>");
                }
                if with_value {
                    text.push_str("<text class=\"ondine-value\" x=\"");
                    push_num(&mut text, x + b.w - SIDE - 2.0);
                    text.push_str("\" y=\"");
                    push_num(&mut text, baseline + tiny * 0.35);
                    text.push_str("\" font-size=\"");
                    push_number(&mut text, tiny);
                    text.push_str("\" text-anchor=\"end\" fill=\"var(--_m)\"");
                    text.push_str(&style.text);
                    text.push('>');
                    text.push_str(&esc(&value));
                    text.push_str("</text>");
                }
            }
            // A section is a tint of its color, unless a class gives it a fill of its own.
            let own_fill = style.shape.contains("\"fill:") || style.shape.contains(";fill:");
            sections.push_str("<g class=\"ondine-node ondine-section");
            sections.push_str(&classes);
            sections.push_str("\" ");
            sections.push_str(&rect);
            if !own_fill {
                sections.push_str(" fill-opacity=\"0.12\"");
            }
            sections.push_str(" stroke=\"");
            sections.push_str(&color);
            sections.push('"');
            sections.push_str(&style.shape);
            sections.push_str("/>");
            sections.push_str(&text);
            sections.push_str("</g>");
            continue;
        }

        let mut text = String::new();
        let room = b.w - 8.0;
        let tallest = size.min(((b.h - 4.0) / LINE).floor());
        let label = if tallest >= least {
            fit_text(name, room, tallest, least, 0)
        } else {
            None
        };
        if let Some(label) = label {
            let mut cx = String::new();
            push_num(&mut cx, x + b.w / 2.0);
            let cy = y + b.h / 2.0;
            let value_size = least.max(js_round(label.size * 0.8));
            let lines = (label.size + value_size) * LINE;
            let push_text = |text: &mut String, class: &str, y: f64, size: f64, body: &str| {
                text.push_str("<text");
                text.push_str(class);
                text.push_str(" x=\"");
                text.push_str(&cx);
                text.push_str("\" y=\"");
                push_num(text, y + size * 0.35);
                text.push_str("\" font-size=\"");
                push_num(text, size);
                text.push('"');
                text.push_str(&style.text);
                text.push('>');
                text.push_str(&esc(body));
                text.push_str("</text>");
            };
            if !value.is_empty()
                && lines <= b.h - 4.0
                && measure::width(&value, value_size, 0) <= room
            {
                let first = cy - lines / 2.0 + (label.size * LINE) / 2.0;
                let second = cy + lines / 2.0 - (value_size * LINE) / 2.0;
                push_text(&mut text, "", first, label.size, &label.text);
                push_text(
                    &mut text,
                    " class=\"ondine-value\"",
                    second,
                    value_size,
                    &value,
                );
            } else {
                push_text(&mut text, "", cy, label.size, &label.text);
            }
        }
        leaves.push_str("<g class=\"ondine-node ondine-leaf");
        leaves.push_str(&classes);
        leaves.push_str("\" ");
        leaves.push_str(&rect);
        leaves.push_str(&style.shape);
        leaves.push_str("/>");
        leaves.push_str(&text);
        leaves.push_str("</g>");
    }

    let total_width = (width.max(title.width) + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !sections.is_empty() {
        content.push_str("<g class=\"ondine-sections\" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\">");
        content.push_str(&sections);
        content.push_str("</g>");
    }
    if !leaves.is_empty() {
        content.push_str("<g class=\"ondine-leaves\" text-anchor=\"middle\" fill=\"var(--_bg)\">");
        content.push_str(&leaves);
        content.push_str("</g>");
    }

    let svg = svg_document(
        "treemap",
        total_width,
        total_height,
        size,
        options,
        Accessible {
            acc_descr: model.acc_descr.as_deref(),
            acc_title: model.acc_title.as_deref(),
        },
        &content,
    );
    Rendered {
        height: total_height,
        links: Vec::new(),
        svg,
        width: total_width,
    }
}
