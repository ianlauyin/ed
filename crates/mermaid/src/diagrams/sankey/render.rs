//! Draws a sankey diagram: node bars, bands between them, and labels beside the bars. Port of
//! Pele's `src/diagrams/sankey/render.ts`.

use super::db::SankeyDb;
use super::layout::{Alignment, LayoutOptions, layout_sankey};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{fit_width, title_room};
use crate::error::Error;
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{resolve_style, series_color};
use crate::text::label::layout_label;
use crate::text::measure::{self, BOLD};
use crate::yaml::{Value, push_number};

/// Space between a bar and its label.
const LABEL_GAP: f64 = 6.0;

/// Extra node padding that makes room for values.
const VALUE_GAP: f64 = 15.0;

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

/// A number option at least `min`, capped at `max`, or `fallback`.
fn number_option(conf: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match property(conf, key) {
        Some(Value::Number(value)) if *value >= min => value.min(max),
        _ => fallback,
    }
}

/// A string option, or a finite number written as JavaScript's `String` writes it, or empty.
fn text_option(conf: &Value, key: &str) -> String {
    match property(conf, key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(value)) if value.is_finite() => {
            let mut out = String::new();
            push_number(&mut out, *value);
            out
        }
        _ => String::new(),
    }
}

/// Two decimals, as Mermaid shows values; numbers too large for that are left as they are.
fn rounded(value: f64) -> f64 {
    if value.abs() < 1e15 {
        js_round(value * 100.0) / 100.0
    } else {
        value
    }
}

/// A colour from the configuration, as an inline style that overrides the series colour.
fn fill_style(color: Option<&Value>) -> String {
    let Some(Value::String(color)) = color else {
        return String::new();
    };
    let mut declaration = String::from("fill:");
    declaration.push_str(color);
    resolve_style(&[declaration]).shape
}

/// Draws `model` with the `sankey` section of `config`, narrowed to fit the host's width.
///
/// # Errors
///
/// Returns a semantic error when the links form a cycle.
pub(crate) fn render(
    model: &SankeyDb,
    config: &Value,
    options: &RenderOptions,
) -> Result<Rendered, Error> {
    let empty = Value::Object(Vec::new());
    let conf = match property(config, "sankey") {
        Some(value @ Value::Object(_)) => value,
        _ => &empty,
    };
    let width = number_option(conf, "width", 600.0, 1.0, 10000.0);
    let result = fit_width(options, width, 160.0, |chart_width, _| {
        draw(model, conf, options, chart_width)
    });
    match result {
        Drawn(Ok(rendered)) => Ok(rendered),
        Drawn(Err(error)) => Err(error),
    }
}

/// A drawing or the error that stopped it, as [`fit_width`] takes it.
struct Drawn(Result<Rendered, Error>);

impl crate::diagrams::common::fit_width::Drawn for Drawn {
    fn width(&self) -> f64 {
        self.0.as_ref().map_or(0.0, |rendered| rendered.width)
    }
}

#[expect(clippy::too_many_lines, reason = "mirrors Pele's draw")]
fn draw(model: &SankeyDb, conf: &Value, options: &RenderOptions, chart_width: f64) -> Drawn {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);

    let chart_height = number_option(conf, "height", 400.0, 1.0, 10000.0);
    let node_width = number_option(conf, "nodeWidth", 10.0, 1.0, 200.0).min(chart_width);
    let show_values = !matches!(property(conf, "showValues"), Some(Value::Bool(false)));
    let prefix = text_option(conf, "prefix");
    let suffix = text_option(conf, "suffix");
    let outlined =
        matches!(property(conf, "labelStyle"), Some(Value::String(s)) if s == "outlined");
    let alignment = match property(conf, "nodeAlignment") {
        Some(Value::String(s)) if s == "left" => Alignment::Left,
        Some(Value::String(s)) if s == "right" => Alignment::Right,
        Some(Value::String(s)) if s == "center" => Alignment::Center,
        _ => Alignment::Justify,
    };
    let link_color = match property(conf, "linkColor") {
        Some(Value::String(s)) => s.as_str(),
        _ => "gradient",
    };

    let layout = layout_sankey(
        model,
        &LayoutOptions {
            alignment,
            height: chart_height,
            label_height: small * 1.15,
            node_padding: number_option(conf, "nodePadding", 12.0, 0.0, 200.0)
                + if show_values { VALUE_GAP } else { 0.0 },
            node_width,
            width: chart_width,
        },
    );
    let (nodes, links) = match layout {
        Ok(layout) => layout,
        Err(error) => return Drawn(Err(error)),
    };

    // Configured colours by node name. Only looked up, so mapping order does not matter.
    let custom: Vec<(&str, String)> = match property(conf, "nodeColors") {
        Some(Value::Object(entries)) => entries
            .iter()
            .filter_map(|(id, color)| {
                let style = fill_style(Some(color));
                (!style.is_empty()).then_some((id.as_str(), style))
            })
            .collect(),
        _ => Vec::new(),
    };
    let node_style = |i: usize| -> &str {
        let Some(node) = model.nodes.get(i) else {
            return "";
        };
        custom
            .iter()
            .find(|(id, _)| *id == node.id)
            .map_or("", |(_, style)| style.as_str())
    };

    // Labels sit beside the bars, on the side that faces the middle of the chart.
    let mut central = 0;
    let mut most = 0.0;
    for node in &nodes {
        if node.value > most {
            most = node.value;
            central = node.layer;
        }
    }
    let mut min_x: f64 = 0.0;
    let mut max_x = chart_width;
    let mut min_y: f64 = 0.0;
    let mut max_y = chart_height;
    let mut labels_out = String::new();
    let mut nodes_out = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let id = model.nodes.get(i).map_or("", |node| node.id.as_str());
        let height = node.height.max(1.0);
        nodes_out.push_str("<rect class=\"ondine-sankey-node\" data-id=\"");
        nodes_out.push_str(&esc_text(id));
        nodes_out.push_str("\" x=\"");
        push_num(&mut nodes_out, node.x);
        nodes_out.push_str("\" y=\"");
        push_num(&mut nodes_out, node.y);
        nodes_out.push_str("\" width=\"");
        push_num(&mut nodes_out, node_width);
        nodes_out.push_str("\" height=\"");
        push_num(&mut nodes_out, height);
        nodes_out.push_str("\" rx=\"");
        push_num(&mut nodes_out, f64::min(2.0, height / 2.0));
        nodes_out.push_str("\" fill=\"");
        nodes_out.push_str(&series_color(i64::try_from(i).unwrap_or(0)));
        nodes_out.push('"');
        nodes_out.push_str(node_style(i));
        nodes_out.push_str("/>");

        let value = if show_values {
            let mut value = prefix.clone();
            push_number(&mut value, rounded(node.value));
            value.push_str(&suffix);
            value
        } else {
            String::new()
        };
        let after = if outlined {
            node.layer >= central
        } else {
            node.x < chart_width / 2.0
        };
        let text_width = if value.is_empty() {
            measure::width(id, small, 0)
        } else {
            let mut both = String::from(id);
            both.push(' ');
            both.push_str(&value);
            measure::width(&both, small, 0)
        };
        let x = if after {
            node.x + node_width + LABEL_GAP
        } else {
            node.x - LABEL_GAP
        };
        let y = node.y + node.height / 2.0;
        min_x = min_x.min(if after { x } else { x - text_width });
        max_x = max_x.max(if after { x + text_width } else { x });
        min_y = min_y.min(y - small * 0.75);
        max_y = max_y.max(y + small * 0.75);

        let mut position = String::from(" x=\"");
        push_num(&mut position, x);
        position.push_str("\" y=\"");
        push_num(&mut position, y + small * 0.35);
        position.push_str("\" text-anchor=\"");
        position.push_str(if after { "start" } else { "end" });
        position.push('"');

        labels_out.push_str("<g class=\"ondine-label\" data-id=\"");
        labels_out.push_str(&esc_text(id));
        labels_out.push_str("\">");
        if outlined {
            labels_out.push_str("<text");
            labels_out.push_str(&position);
            labels_out.push_str(
                " fill=\"none\" stroke=\"var(--_bg)\" stroke-width=\"3\" stroke-linejoin=\"round\">",
            );
            labels_out.push_str(&esc(id));
            if !value.is_empty() {
                labels_out.push(' ');
                labels_out.push_str(&esc(&value));
            }
            labels_out.push_str("</text>");
        }
        labels_out.push_str("<text");
        labels_out.push_str(&position);
        labels_out.push('>');
        labels_out.push_str(&esc(id));
        if !value.is_empty() {
            labels_out.push_str(" <tspan class=\"ondine-sankey-value\" fill=\"var(--_m)\">");
            labels_out.push_str(&esc(&value));
            labels_out.push_str("</tspan>");
        }
        labels_out.push_str("</text></g>");
    }

    let fixed = if matches!(link_color, "gradient" | "source" | "target") {
        String::new()
    } else {
        fill_style(property(conf, "linkColor"))
    };
    let mut links_out = String::new();
    for link in &links {
        // Without definitions there are no gradients, so a gradient link takes its source's colour.
        let end = if link_color == "target" {
            link.target
        } else {
            link.source
        };
        let half = link.thickness.max(1.0) / 2.0;
        let x0 = nodes.get(link.source).map_or(0.0, |node| node.x) + node_width;
        let x1 = nodes.get(link.target).map_or(0.0, |node| node.x);
        let mut mid = String::new();
        #[expect(
            clippy::manual_midpoint,
            reason = "JavaScript rounds the sum, then halves it"
        )]
        let center = (x0 + x1) / 2.0;
        push_num(&mut mid, center);
        let from = esc_text(
            model
                .nodes
                .get(link.source)
                .map_or("", |node| node.id.as_str()),
        );
        let to = esc_text(
            model
                .nodes
                .get(link.target)
                .map_or("", |node| node.id.as_str()),
        );

        let out = &mut links_out;
        out.push_str("<path class=\"ondine-sankey-link\" data-id=\"");
        out.push_str(&from);
        out.push_str("-&gt;");
        out.push_str(&to);
        out.push_str("\" d=\"M");
        push_num(out, x0);
        out.push(',');
        push_num(out, link.source_y - half);
        out.push('C');
        out.push_str(&mid);
        out.push(',');
        push_num(out, link.source_y - half);
        out.push(' ');
        out.push_str(&mid);
        out.push(',');
        push_num(out, link.target_y - half);
        out.push(' ');
        push_num(out, x1);
        out.push(',');
        push_num(out, link.target_y - half);
        out.push('L');
        push_num(out, x1);
        out.push(',');
        push_num(out, link.target_y + half);
        out.push('C');
        out.push_str(&mid);
        out.push(',');
        push_num(out, link.target_y + half);
        out.push(' ');
        out.push_str(&mid);
        out.push(',');
        push_num(out, link.source_y + half);
        out.push(' ');
        push_num(out, x0);
        out.push(',');
        push_num(out, link.source_y + half);
        out.push_str("Z\" fill=\"");
        out.push_str(&series_color(i64::try_from(end).unwrap_or(0)));
        out.push('"');
        if fixed.is_empty() {
            out.push_str(node_style(end));
        } else {
            out.push_str(&fixed);
        }
        out.push_str("><title>");
        out.push_str(&from);
        out.push_str(" → ");
        out.push_str(&to);
        out.push_str(": ");
        out.push_str(&esc_text(&prefix));
        push_number(out, rounded(link.value));
        out.push_str(&esc_text(&suffix));
        out.push_str("</title></path>");
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
        title.height + 12.0
    } else {
        0.0
    };
    let inner = f64::max(max_x - min_x, title.width);
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g transform=\"translate(");
    push_num(&mut content, ox);
    content.push(',');
    push_num(&mut content, oy);
    content.push_str(")\"><g class=\"ondine-sankey-links\" fill-opacity=\"0.4\">");
    content.push_str(&links_out);
    content.push_str("</g><g class=\"ondine-sankey-nodes\">");
    content.push_str(&nodes_out);
    content.push_str("</g><g class=\"ondine-labels\" font-size=\"");
    push_number(&mut content, small);
    content.push_str("\">");
    content.push_str(&labels_out);
    content.push_str("</g></g>");

    let accessible = Accessible {
        acc_descr: model.acc_descr.as_deref(),
        acc_title: model.acc_title.as_deref(),
    };
    let svg = svg_document("sankey", width, height, size, options, accessible, &content);

    Drawn(Ok(Rendered {
        height,
        links: Vec::new(),
        svg,
        width,
    }))
}
