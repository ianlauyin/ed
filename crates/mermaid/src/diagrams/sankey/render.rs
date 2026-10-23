//! Draws a sankey diagram: node bars, bands between them, and labels beside the bars. Port of
//! Pele's `src/diagrams/sankey/render.ts`.

use std::collections::HashMap;

use super::db::SankeyDb;
use super::layout::Alignment;
use super::layout::LayoutOptions;
use super::layout::layout_sankey;
use crate::Options;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::fit_width;
use crate::diagrams::common::fit_width::title_room;
use crate::error::Error;
use crate::svg::builder::esc;
use crate::svg::builder::esc_text;
use crate::svg::builder::js_round;
use crate::svg::builder::label_svg;
use crate::svg::builder::push_num;
use crate::svg::root::svg_document;
use crate::svg::theme::resolve_style;
use crate::svg::theme::series_color;
use crate::text::entities::decode;
use crate::text::label::layout_label;
use crate::text::measure::BOLD;
use crate::text::measure::{self};
use crate::yaml::Value;
use crate::yaml::push_number;

/// Space between a bar and its label.
const LABEL_GAP: f64 = 6.0;

/// Extra node padding that makes room for values.
const VALUE_GAP: f64 = 15.0;

/// About how many lines of label room each bar of the most crowded column keeps.
const LABEL_ROOM: f64 = 1.8;

/// The most nodes whose labels are kept apart; past it, checking costs too much.
const CAREFUL: usize = 300;

/// A property of a YAML mapping, as JavaScript's `value.key` reads it.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries.iter().find(|(own, _)| own == key).map(|(_, value)| value)
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
    if value.abs() < 1e15 { js_round(value * 100.0) / 100.0 } else { value }
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
pub(crate) fn render(model: &SankeyDb, config: &Value, options: &Options) -> Result<Rendered, Error> {
    let empty = Value::Object(Vec::new());
    let conf = match property(config, "sankey") {
        Some(value @ Value::Object(_)) => value,
        _ => &empty,
    };
    let width = number_option(conf, "width", 600.0, 1.0, 10000.0);
    let result = fit_width(options, width, 160.0, |chart_width, _| draw(model, conf, options, chart_width, width));
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
/// Where a node's label goes: its value text, width, side, left edge, and middle.
struct Placed {
    value: String,
    width: f64,
    after: bool,
    left: f64,
    y: f64,
}

/// The band of label middles `y` falls in, `line_height` tall.
#[expect(clippy::cast_possible_truncation, reason = "bands of a chart at most 10000 tall")]
fn band(y: f64, line_height: f64) -> i64 {
    (y / line_height).floor() as i64
}

#[expect(clippy::too_many_lines, reason = "mirrors Pele's draw")]
fn draw(model: &SankeyDb, conf: &Value, options: &Options, chart_width: f64, natural: f64) -> Drawn {
    let size = options.font_size;
    let least = js_round(size * 0.625);
    // Labels shrink with a chart narrowed to fit, as they would if the whole picture were scaled
    // down.
    let mut small = least.max(js_round(size * 0.875 * f64::min(1.0, chart_width / natural)));
    let pad = options.padding;

    let chart_height = number_option(conf, "height", 400.0, 1.0, 10000.0);
    let node_width = number_option(conf, "nodeWidth", 10.0, 1.0, 200.0).min(chart_width);
    let show_values = !matches!(property(conf, "showValues"), Some(Value::Bool(false)));
    let prefix = text_option(conf, "prefix");
    let suffix = text_option(conf, "suffix");
    let outlined = matches!(property(conf, "labelStyle"), Some(Value::String(s)) if s == "outlined");
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

    let lay = |label_size: f64| {
        layout_sankey(
            model,
            &LayoutOptions {
                alignment,
                height: chart_height,
                label_height: label_size * 1.15,
                node_padding: number_option(conf, "nodePadding", 12.0, 0.0, 200.0)
                    + if show_values { VALUE_GAP } else { 0.0 },
                node_width,
                width: chart_width,
            },
        )
    };
    let (mut nodes, mut links) = match lay(small) {
        Ok(layout) => layout,
        Err(error) => return Drawn(Err(error)),
    };
    // Labels also shrink with the most crowded column, so each keeps about LABEL_ROOM lines.
    let mut rows: Vec<usize> = Vec::new();
    let mut fullest = 1;
    for node in &nodes {
        if rows.len() <= node.layer {
            rows.resize(node.layer + 1, 0);
        }
        if let Some(count) = rows.get_mut(node.layer) {
            *count += 1;
            fullest = fullest.max(*count);
        }
    }
    let crowded =
        least.max(js_round(chart_height / f64::from(u32::try_from(fullest).unwrap_or(u32::MAX)) / LABEL_ROOM));
    if crowded < small {
        small = crowded;
        (nodes, links) = match lay(small) {
            Ok(layout) => layout,
            Err(error) => return Drawn(Err(error)),
        };
    }
    let line_height = small * 1.5;

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
        custom.iter().find(|(id, _)| *id == node.id).map_or("", |(_, style)| style.as_str())
    };
    // The whole fill, which keeps the semicolons of escaped entities.
    let node_color = |i: usize| -> String {
        node_style(i)
            .strip_prefix(" style=\"fill:")
            .and_then(|rest| rest.strip_suffix(";\""))
            .map_or_else(|| series_color(i64::try_from(i).unwrap_or(0)), str::to_owned)
    };

    // Labels sit beside the bars, on the side that faces the middle of the chart unless that
    // side is crowded.
    let mut central = 0;
    let mut most = 0.0;
    for node in &nodes {
        if node.value > most {
            most = node.value;
            central = node.layer;
        }
    }
    let names: Vec<String> =
        (0..nodes.len()).map(|i| decode(model.nodes.get(i).map_or("", |node| node.id.as_str())).into_owned()).collect();
    let mut min_x: f64 = 0.0;
    let mut max_x = chart_width;
    let mut min_y: f64 = 0.0;
    let mut max_y = chart_height;
    let mut labels: Vec<Placed> = Vec::with_capacity(nodes.len());
    for (node, name) in nodes.iter().zip(&names) {
        let value = if show_values {
            let mut value = prefix.clone();
            push_number(&mut value, rounded(node.value));
            value.push_str(&suffix);
            value
        } else {
            String::new()
        };
        let width = if value.is_empty() {
            measure::width(name, small, 0)
        } else {
            let mut both = name.clone();
            both.push(' ');
            both.push_str(&value);
            measure::width(&both, small, 0)
        };
        let after = if outlined { node.layer >= central } else { node.x < chart_width / 2.0 };
        let x = if after { node.x + node_width + LABEL_GAP } else { node.x - LABEL_GAP };
        let y = node.y + node.height / 2.0;
        min_x = min_x.min(if after { x } else { x - width });
        max_x = max_x.max(if after { x + width } else { x });
        min_y = min_y.min(y - small * 0.75);
        max_y = max_y.max(y + small * 0.75);
        labels.push(Placed { value, width, after, left: if after { x } else { x - width }, y });
    }
    // A label moves at most one line, so each band also holds the labels two bands either side.
    let careful = nodes.len() <= CAREFUL;
    let mut bands: HashMap<i64, Vec<usize>> = HashMap::new();
    if careful {
        for (i, label) in labels.iter().enumerate() {
            let at = band(label.y, line_height);
            for key in at - 2..=at + 2 {
                bands.entry(key).or_default().push(i);
            }
        }
    }

    let mut labels_out = String::new();
    let mut nodes_out = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let id = model.nodes.get(i).map_or("", |node| node.id.as_str());
        let name = names.get(i).map_or("", String::as_str);
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

        let Some(label) = labels.get(i) else {
            continue;
        };
        let label_width = label.width;
        let overlaps_label = |left: f64, y: f64| {
            bands.get(&band(y, line_height)).is_some_and(|nearby| {
                nearby.iter().filter(|&&j| j != i).filter_map(|&j| labels.get(j)).any(|other| {
                    (other.y - y).abs() < line_height
                        && left < other.left + other.width + LABEL_GAP
                        && left + label_width > other.left - LABEL_GAP
                })
            })
        };
        // Try the other side, then an adjacent line when both sides are crowded.
        let mut moved = None;
        if careful && overlaps_label(label.left, label.y) {
            'place: for dy in [0.0, -line_height, line_height] {
                for after in [label.after, !label.after] {
                    let left = if after { node.x + node_width + LABEL_GAP } else { node.x - LABEL_GAP - label_width };
                    let y = label.y + dy;
                    if overlaps_label(left, y)
                        || left < min_x
                        || left + label_width > max_x
                        || y - small * 0.75 < min_y
                        || y + small * 0.75 > max_y
                    {
                        continue;
                    }
                    let covers_bar = nodes.iter().enumerate().any(|(j, bar)| {
                        j != i
                            && left < bar.x + node_width + LABEL_GAP
                            && left + label_width > bar.x - LABEL_GAP
                            && y - small * 0.75 < bar.y + bar.height
                            && y + small * 0.75 > bar.y
                    });
                    if covers_bar {
                        continue;
                    }
                    // A label stays nearer its own bar than any other in the column, so it can't
                    // name that one.
                    let own = f64::max(0.0, f64::max(node.y - y, y - node.y - node.height));
                    #[expect(clippy::float_cmp, reason = "bars of one column share their x exactly")]
                    let nearer = nodes.iter().enumerate().any(|(j, bar)| {
                        j != i && bar.x == node.x && f64::max(0.0, f64::max(bar.y - y, y - bar.y - bar.height)) <= own
                    });
                    if nearer {
                        continue;
                    }
                    moved = Some((left, y, after));
                    break 'place;
                }
            }
        }
        if let Some((left, y, after)) = moved
            && let Some(label) = labels.get_mut(i)
        {
            label.left = left;
            label.y = y;
            label.after = after;
        }
        let Some(label) = labels.get(i) else {
            continue;
        };
        let (value, after, y) = (&label.value, label.after, label.y);
        let x = if after { node.x + node_width + LABEL_GAP } else { node.x - LABEL_GAP };

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
            labels_out.push_str(" fill=\"none\" stroke=\"var(--_bg)\" stroke-width=\"3\" stroke-linejoin=\"round\">");
            labels_out.push_str(&esc(name));
            if !value.is_empty() {
                labels_out.push(' ');
                labels_out.push_str(&esc(value));
            }
            labels_out.push_str("</text>");
        }
        labels_out.push_str("<text");
        labels_out.push_str(&position);
        labels_out.push('>');
        labels_out.push_str(&esc(name));
        if !value.is_empty() {
            labels_out.push_str(" <tspan class=\"ondine-sankey-value\" fill=\"var(--_m)\">");
            labels_out.push_str(&esc(value));
            labels_out.push_str("</tspan>");
        }
        labels_out.push_str("</text></g>");
    }

    let fixed = if matches!(link_color, "gradient" | "source" | "target") {
        String::new()
    } else {
        fill_style(property(conf, "linkColor"))
    };
    // Each SVG is rasterized on its own, so gradient ids can't clash with another diagram's.
    let gradient = link_color == "gradient";
    let mut gradients: Vec<String> = Vec::new();
    let mut links_out = String::new();
    for link in &links {
        let end = if link_color == "target" { link.target } else { link.source };
        let half = link.thickness.max(1.0) / 2.0;
        let x0 = nodes.get(link.source).map_or(0.0, |node| node.x) + node_width;
        let x1 = nodes.get(link.target).map_or(0.0, |node| node.x);
        let mut paint = String::from("fill=\"");
        if gradient {
            let mut stops = String::from("x1=\"");
            push_num(&mut stops, x0);
            stops.push_str("\" x2=\"");
            push_num(&mut stops, x1);
            stops.push_str("\"><stop offset=\"0\" stop-color=\"");
            stops.push_str(&node_color(link.source));
            stops.push_str("\"/><stop offset=\"1\" stop-color=\"");
            stops.push_str(&node_color(link.target));
            stops.push_str("\"/>");
            let at = gradients.iter().position(|own| *own == stops).unwrap_or_else(|| {
                gradients.push(stops);
                gradients.len() - 1
            });
            paint.push_str("url(#ondine-sankey-");
            push_number(&mut paint, f64::from(u32::try_from(at).unwrap_or(u32::MAX)));
            paint.push_str(")\"");
        } else {
            paint.push_str(&series_color(i64::try_from(end).unwrap_or(0)));
            paint.push('"');
            if fixed.is_empty() {
                paint.push_str(node_style(end));
            } else {
                paint.push_str(&fixed);
            }
        }
        let mut mid = String::new();
        #[expect(clippy::manual_midpoint, reason = "JavaScript rounds the sum, then halves it")]
        let center = (x0 + x1) / 2.0;
        push_num(&mut mid, center);
        let from = esc_text(model.nodes.get(link.source).map_or("", |node| node.id.as_str()));
        let to = esc_text(model.nodes.get(link.target).map_or("", |node| node.id.as_str()));

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
        out.push_str("Z\" ");
        out.push_str(&paint);
        out.push_str("/>");
    }
    let mut defs = String::new();
    if !gradients.is_empty() {
        defs.push_str("<defs>");
        for (at, stops) in gradients.iter().enumerate() {
            defs.push_str("<linearGradient id=\"ondine-sankey-");
            push_number(&mut defs, f64::from(u32::try_from(at).unwrap_or(u32::MAX)));
            defs.push_str("\" gradientUnits=\"userSpaceOnUse\" ");
            defs.push_str(stops);
            defs.push_str("</linearGradient>");
        }
        defs.push_str("</defs>");
    }

    let title = layout_label(model.title.as_deref(), false, size, title_room(options), BOLD, None);
    let title_height = if title.height > 0.0 { title.height + 12.0 } else { 0.0 };
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
    content.push_str(")\">");
    content.push_str(&defs);
    content.push_str("<g class=\"ondine-sankey-links\" fill-opacity=\"0.4\">");
    content.push_str(&links_out);
    content.push_str("</g><g class=\"ondine-sankey-nodes\">");
    content.push_str(&nodes_out);
    content.push_str("</g><g class=\"ondine-labels\" font-size=\"");
    push_number(&mut content, small);
    content.push_str("\">");
    content.push_str(&labels_out);
    content.push_str("</g></g>");
    let svg = svg_document("sankey", width, height, size, &content);

    Drawn(Ok(Rendered { height, svg, width }))
}
