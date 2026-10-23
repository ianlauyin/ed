//! Draws a Wardley map: the evolution axis with its stages, components, links, pipelines, trends,
//! annotations, notes, and forces. Port of Pele's `src/diagrams/wardley/render.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::builder::{Flow, WardleyNode};
use super::model::WardleyModel;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::fit_width;
use crate::svg::builder::{
    esc, esc_text, js_round, label_svg, push_num, shared_style, span_style, text_style,
};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::RADIUS;
use crate::text::label::{Label, layout_label};
use crate::text::measure::BOLD;
use crate::yaml::{Value, push_number};

/// The stages when the map names none.
const DEFAULT_STAGES: [&str; 4] = ["Genesis", "Custom Built", "Product", "Commodity"];
/// Mermaid's canvas has this much room around the plot on every side.
const CANVAS_MARGIN: f64 = 48.0;
/// The radius of a component.
const R: f64 = 6.0;
/// The side of a pipeline parent's square.
const SQUARE: f64 = 10.0;
/// How far a label sits from its component.
const LABEL_OFFSET: f64 = 8.0;
/// Room in a pipeline box beside its outer components.
const PIPELINE_PAD: f64 = 15.0;
/// The length of a force arrow.
const FORCE_WIDTH: f64 = 60.0;
/// The thickness of a force arrow's body.
const FORCE_HEIGHT: f64 = 30.0;
/// The length of a force arrow's head.
const FORCE_HEAD: f64 = 20.0;
/// The color of trends.
const ACCENT: &str = "var(--_c)";
/// The color of lines.
const LINE: &str = "var(--_l)";

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

/// A number setting of the `wardley-beta` section within `min` and `max`, or `fallback`.
fn setting(config: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match property(config, "wardley-beta").and_then(|section| property(section, key)) {
        Some(Value::Number(value)) if *value >= min && *value <= max => *value,
        _ => fallback,
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// The plot's side from the map's own size, the configuration, or `fallback`.
fn dimension(config: &Value, given: Option<f64>, key: &str, fallback: f64) -> f64 {
    let value = match given {
        Some(given) if given.is_finite() => given,
        _ => setting(config, key, fallback, 1.0, 1e6),
    };
    clamp(value, 200.0, 10000.0) - 2.0 * CANVAS_MARGIN
}

/// Writes a label with its lines starting at `x` and its first baseline at `y`.
fn start_label(label: &Label, x: f64, y: f64, attrs: &str) -> String {
    let shared = shared_style(label);
    let mut body = String::new();
    let mut baseline = y;
    for line in &label.lines {
        let mut first = true;
        for span in line {
            if span.icon.is_some() {
                continue;
            }
            body.push_str("<tspan");
            if first {
                body.push_str(" x=\"");
                push_num(&mut body, x);
                body.push_str("\" y=\"");
                push_num(&mut body, baseline);
                body.push('"');
            }
            body.push_str(span_style(span.style & !shared));
            body.push('>');
            body.push_str(&esc(&span.text));
            body.push_str("</tspan>");
            first = false;
        }
        baseline += label.line_height;
    }
    if body.is_empty() {
        return body;
    }
    let mut out = String::from("<text");
    out.push_str(&text_style(attrs, shared));
    out.push_str(" xml:space=\"preserve\">");
    out.push_str(&body);
    out.push_str("</text>");
    out
}

/// Draws `model` with the `wardley-beta` section of `config`, narrowed to fit the host's width.
pub(crate) fn render(model: &WardleyModel, config: &Value, options: &RenderOptions) -> Rendered {
    let natural = dimension(config, model.build.size.map(|size| size.0), "width", 900.0);
    fit_width(options, natural, 160.0, |plot_w, tight| {
        draw(model, config, options, plot_w, natural, tight)
    })
}

/// The extent of what is drawn.
struct Bounds {
    max_x: f64,
    max_y: f64,
    min_x: f64,
    min_y: f64,
}

impl Bounds {
    fn grow(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if x0 < self.min_x {
            self.min_x = x0;
        }
        if y0 < self.min_y {
            self.min_y = y0;
        }
        if x1 > self.max_x {
            self.max_x = x1;
        }
        if y1 > self.max_y {
            self.max_y = y1;
        }
    }

    /// A label centered on (`cx`, `cy`).
    fn centered(&mut self, label: &Label, cx: f64, cy: f64, attrs: &str) -> String {
        self.grow(
            cx - label.width / 2.0,
            cy - label.height / 2.0,
            cx + label.width / 2.0,
            cy + label.height / 2.0,
        );
        label_svg(label, cx, cy, attrs, None)
    }

    /// A label starting at `x` with its first baseline at `y`.
    fn starting(&mut self, label: &Label, x: f64, y: f64, attrs: &str) -> String {
        self.grow(
            x,
            y - label.size,
            x + label.width,
            y - label.size + label.height + 4.0,
        );
        start_label(label, x, y, attrs)
    }
}

/// ` class="…" font-size="{size}"` and the rest of `tail`.
fn sized(head: &str, size: f64, tail: &str) -> String {
    let mut out = String::from(head);
    out.push_str(" font-size=\"");
    push_number(&mut out, size);
    out.push('"');
    out.push_str(tail);
    out
}

/// How far from a node's center a line to it stops, along (`ux`, `uy`).
fn reach(node: &WardleyNode, anchor_label: Option<&Label>, small: f64, ux: f64, uy: f64) -> f64 {
    if node.class_name == Some("anchor") {
        let width = anchor_label.map_or(0.0, |label| label.width);
        return 1.0 / (ux / (width / 2.0 + 5.0)).hypot(uy / (small * 0.8));
    }
    if node.is_pipeline_parent {
        return SQUARE / std::f64::consts::SQRT_2;
    }
    if node
        .source_strategy
        .as_deref()
        .is_some_and(|s| !s.is_empty())
    {
        2.0 * R
    } else {
        R
    }
}

/// The quadrant bit a link along (`ux`, `uy`) takes: 1 up and right, 2 down and right, 4 up and
/// left, 8 down and left. Nearly straight links take none.
fn quadrant(ux: f64, uy: f64) -> u8 {
    if ux.abs() < 0.25 || uy.abs() < 0.25 {
        return 0;
    }
    match (ux > 0.0, uy < 0.0) {
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 4,
        (false, false) => 8,
    }
}

#[expect(clippy::too_many_lines, reason = "mirrors Pele's draw")]
fn draw(
    model: &WardleyModel,
    config: &Value,
    options: &RenderOptions,
    plot_w: f64,
    natural_w: f64,
    tight: bool,
) -> Rendered {
    let build = &model.build;
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.8125);
    let tiny = js_round(size * 0.6875);
    let pad = options.padding.unwrap_or(8.0);

    // A narrower map is not as much shorter, so that components keep some room between them.
    let plot_h = dimension(config, build.size.map(|size| size.1), "height", 600.0)
        * 0.75_f64.max(plot_w / natural_w);
    let show_grid = matches!(
        property(config, "wardley-beta").and_then(|section| property(section, "showGrid")),
        Some(Value::Bool(true))
    );

    let mut bounds = Bounds {
        max_x: plot_w,
        max_y: plot_h,
        min_x: 0.0,
        min_y: 0.0,
    };
    let wrap = if tight {
        64.0_f64.max(120.0_f64.min(plot_w / 2.0))
    } else {
        4000.0
    };
    let text = |raw: &str, font_size: f64, max_width: f64, style: u8| {
        layout_label(Some(raw), false, font_size, max_width, style, None)
    };
    let project_x = |value: f64| (value / 100.0) * plot_w;
    let project_y = |value: f64| plot_h - (value / 100.0) * plot_h;

    let stages: Vec<&str> = match &build.axes.stages {
        Some(stages) if !stages.is_empty() => stages.iter().map(String::as_str).collect(),
        _ => DEFAULT_STAGES.to_vec(),
    };
    let boundaries = build
        .axes
        .stage_boundaries
        .as_ref()
        .filter(|boundaries| boundaries.len() == stages.len());
    #[expect(clippy::cast_precision_loss, reason = "stage counts are small")]
    let count = stages.len() as f64;
    let mut frame = String::new();
    let mut stage_labels = String::new();
    let mut stage_bottom = plot_h;
    let mut previous = 0.0;
    for (index, stage) in stages.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "stage counts are small")]
        let i = index as f64;
        let (start, end) = match boundaries {
            Some(boundaries) => (
                previous,
                clamp(boundaries.get(index).copied().unwrap_or(f64::NAN), 0.0, 1.0),
            ),
            None => (i / count, (i + 1.0) / count),
        };
        previous = end;
        if index > 0 {
            frame.push_str("<path class=\"ondine-wardley-stage\" d=\"M");
            push_num(&mut frame, start * plot_w);
            frame.push_str(",0V");
            push_num(&mut frame, plot_h);
            frame.push_str("\" stroke=\"var(--_b)\" stroke-dasharray=\"4 4\"/>");
        }
        let label = text(
            stage,
            small,
            48.0_f64.max((end - start).abs() * plot_w - 12.0),
            0,
        );
        stage_labels.push_str(&bounds.centered(
            &label,
            f64::midpoint(start, end) * plot_w,
            plot_h + 8.0 + label.height / 2.0,
            &sized(
                " class=\"ondine-wardley-stage-label\"",
                small,
                " fill=\"var(--_m)\"",
            ),
        ));
        stage_bottom = stage_bottom.max(plot_h + 8.0 + label.height);
    }
    if show_grid {
        for i in 1..4 {
            let i = f64::from(i);
            frame.push_str("<path class=\"ondine-wardley-grid\" d=\"M");
            push_num(&mut frame, (plot_w * i) / 4.0);
            frame.push_str(",0V");
            push_num(&mut frame, plot_h);
            frame.push_str("M0,");
            push_num(&mut frame, (plot_h * i) / 4.0);
            frame.push('H');
            push_num(&mut frame, plot_w);
            frame.push_str("\" stroke=\"var(--_b)\" stroke-dasharray=\"2 6\"/>");
        }
    }
    frame.push_str("<path class=\"ondine-wardley-axis\" d=\"M0,0V");
    push_num(&mut frame, plot_h);
    frame.push('H');
    push_num(&mut frame, plot_w);
    frame.push_str("\" stroke=\"");
    frame.push_str(LINE);
    frame.push_str("\"/>");
    let x_title = text("Evolution", small, wrap, 0);
    let y_title = text("Visibility", small, wrap, 0);
    let axis_attrs = sized(
        " class=\"ondine-wardley-axis-label\"",
        small,
        " font-weight=\"var(--_hw)\"",
    );
    let mut axis_labels = bounds.centered(
        &x_title,
        plot_w / 2.0,
        stage_bottom + 6.0 + x_title.height / 2.0,
        &axis_attrs,
    );
    let mut rotated = axis_attrs.clone();
    rotated.push_str(" transform=\"rotate(-90 -16 ");
    push_num(&mut rotated, plot_h / 2.0);
    rotated.push_str(")\"");
    axis_labels.push_str(&label_svg(&y_title, -16.0, plot_h / 2.0, &rotated, None));
    bounds.grow(-16.0 - y_title.height / 2.0, 0.0, 0.0, 0.0);
    let legend_top = stage_bottom + 6.0 + x_title.height + 10.0;

    let nodes = &build.nodes;
    let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
    let mut positions: Vec<(f64, f64)> = Vec::with_capacity(nodes.len());
    for (at, node) in nodes.iter().enumerate() {
        ids.insert(&node.id, at);
        positions.push((project_x(node.x), project_y(node.y)));
    }

    // Pipelines: a box around the components, with the parent as a square on its top edge.
    let mut pipelines = String::new();
    let mut members: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for pipeline in &build.pipelines {
        members.insert(
            &pipeline.node_id,
            pipeline.component_ids.iter().map(String::as_str).collect(),
        );
        let mut parts: Vec<(f64, f64)> = pipeline
            .component_ids
            .iter()
            .filter_map(|id| positions.get(*ids.get(id.as_str())?).copied())
            .collect();
        parts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let (Some(&(left, y)), Some(&(right, _))) = (parts.first(), parts.last()) else {
            continue;
        };
        let top = y - 2.0 * R;
        if let Some(&at) = ids.get(pipeline.node_id.as_str())
            && let Some(parent) = positions.get_mut(at)
        {
            *parent = (f64::midpoint(left, right), top - SQUARE / 6.0);
        }
        bounds.grow(
            left - PIPELINE_PAD,
            top,
            right + PIPELINE_PAD,
            top + 4.0 * R,
        );
        pipelines.push_str("<g class=\"ondine-wardley-pipeline\" data-id=\"");
        pipelines.push_str(&esc_text(&pipeline.node_id));
        pipelines.push_str("\"><rect x=\"");
        push_num(&mut pipelines, left - PIPELINE_PAD);
        pipelines.push_str("\" y=\"");
        push_num(&mut pipelines, top);
        pipelines.push_str("\" width=\"");
        push_num(&mut pipelines, right - left + 2.0 * PIPELINE_PAD);
        pipelines.push_str("\" height=\"");
        push_num(&mut pipelines, 4.0 * R);
        pipelines.push_str("\" rx=\"");
        pipelines.push_str(RADIUS);
        pipelines.push_str("\" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>");
        if parts.len() > 1 {
            pipelines.push_str("<path d=\"M");
            push_num(&mut pipelines, left);
            pipelines.push(',');
            push_num(&mut pipelines, y);
            pipelines.push('H');
            push_num(&mut pipelines, right);
            pipelines.push_str("\" stroke=\"");
            pipelines.push_str(LINE);
            pipelines.push_str("\" stroke-dasharray=\"4 4\"/>");
        }
        pipelines.push_str("</g>");
    }

    // Anchor labels, measured once when a line first reaches the anchor.
    let anchor_wrap = if tight {
        64.0_f64.max(plot_w / 2.0)
    } else {
        4000.0
    };
    let mut anchor_labels: Vec<Option<Label>> = vec![None; nodes.len()];
    let mut reach_of = |at: usize, ux: f64, uy: f64| -> f64 {
        let Some(node) = nodes.get(at) else {
            return R;
        };
        if node.class_name == Some("anchor")
            && let Some(slot) = anchor_labels.get_mut(at)
            && slot.is_none()
        {
            *slot = Some(text(&node.label, small, anchor_wrap, BOLD));
        }
        let label = anchor_labels.get(at).and_then(Option::as_ref);
        reach(node, label, small, ux, uy)
    };

    let mut taken: Vec<u8> = vec![0; nodes.len()];
    let mut links = String::new();
    for link in &build.links {
        let (Some(&from), Some(&to)) =
            (ids.get(link.source.as_str()), ids.get(link.target.as_str()))
        else {
            continue;
        };
        if from == to {
            continue;
        }
        // A pipeline component belongs to its parent already.
        if members
            .get(link.target.as_str())
            .is_some_and(|set| set.contains(link.source.as_str()))
        {
            continue;
        }
        let (Some(&(fx, fy)), Some(&(tx, ty))) = (positions.get(from), positions.get(to)) else {
            continue;
        };
        let dx = tx - fx;
        let dy = ty - fy;
        let distance = dx.hypot(dy);
        if distance < 0.01 {
            continue;
        }
        let ux = dx / distance;
        let uy = dy / distance;
        let start = reach_of(from, ux, uy);
        let end = reach_of(to, ux, uy);
        if start + end >= distance {
            continue;
        }
        if let Some(bits) = taken.get_mut(from) {
            *bits |= quadrant(ux, uy);
        }
        if let Some(bits) = taken.get_mut(to) {
            *bits |= quadrant(-ux, -uy);
        }
        let mut x1 = fx + ux * start;
        let mut y1 = fy + uy * start;
        let mut x2 = tx - ux * end;
        let mut y2 = ty - uy * end;
        let mut heads = String::new();
        let trim = marker_trim("arrow_point");
        if matches!(link.flow, Some(Flow::Forward | Flow::Bidirectional)) {
            heads.push_str(&marker("arrow_point", x2, y2, ux, uy, LINE));
            x2 -= ux * trim;
            y2 -= uy * trim;
        }
        if matches!(link.flow, Some(Flow::Backward | Flow::Bidirectional)) {
            heads.push_str(&marker("arrow_point", x1, y1, -ux, -uy, LINE));
            x1 += ux * trim;
            y1 += uy * trim;
        }
        let mut caption = String::new();
        if let Some(text_value) = link.label.as_deref().filter(|label| !label.is_empty()) {
            let label = text(text_value, tiny + 1.0, 200.0, 0);
            // Beside the middle of the line, on its upper side.
            let side = if ux >= 0.0 { 1.0 } else { -1.0 };
            let px = uy * side;
            let py = -ux * side;
            let away = 5.0 + (px.abs() * label.width) / 2.0 + (py.abs() * label.height) / 2.0;
            caption = bounds.centered(
                &label,
                f64::midpoint(fx, tx) + px * away,
                f64::midpoint(fy, ty) + py * away,
                &sized(
                    " class=\"ondine-edge-label\"",
                    tiny + 1.0,
                    " fill=\"var(--_m)\"",
                ),
            );
        }
        links.push_str("<g class=\"ondine-edge");
        if link.dashed {
            links.push_str(" ondine-wardley-dashed");
        }
        links.push_str("\" data-id=\"");
        links.push_str(&esc_text(&link.source));
        links.push('-');
        links.push_str(&esc_text(&link.target));
        links.push_str("\"><path d=\"M");
        push_num(&mut links, x1);
        links.push(',');
        push_num(&mut links, y1);
        links.push('L');
        push_num(&mut links, x2);
        links.push(',');
        push_num(&mut links, y2);
        links.push_str("\" stroke=\"");
        links.push_str(LINE);
        links.push('"');
        if link.dashed {
            links.push_str(" stroke-dasharray=\"6 6\"");
        }
        links.push_str("/>");
        links.push_str(&heads);
        links.push_str(&caption);
        links.push_str("</g>");
    }

    // Evolution: a dashed arrow to where the component is heading, drawn as an open circle.
    let mut trends = String::new();
    for trend in &build.trends {
        let Some(&at) = ids.get(trend.node_id.as_str()) else {
            continue;
        };
        let Some(&(ox, oy)) = positions.get(at) else {
            continue;
        };
        let tx = project_x(trend.target_x);
        let ty = project_y(trend.target_y);
        let dx = tx - ox;
        let dy = ty - oy;
        let distance = dx.hypot(dy);
        let mut arrow = String::new();
        let start = if distance > 0.0 {
            reach_of(at, dx / distance, dy / distance)
        } else {
            0.0
        };
        if distance > start + R + 4.0 {
            let ux = dx / distance;
            let uy = dy / distance;
            let ex = tx - ux * (R + 2.0);
            let ey = ty - uy * (R + 2.0);
            let trim = marker_trim("arrow_point");
            arrow.push_str("<path d=\"M");
            push_num(&mut arrow, ox + ux * start);
            arrow.push(',');
            push_num(&mut arrow, oy + uy * start);
            arrow.push('L');
            push_num(&mut arrow, ex - ux * trim);
            arrow.push(',');
            push_num(&mut arrow, ey - uy * trim);
            arrow.push_str("\" stroke=\"");
            arrow.push_str(ACCENT);
            arrow.push_str("\" stroke-dasharray=\"4 4\"/>");
            arrow.push_str(&marker("arrow_point", ex, ey, ux, uy, ACCENT));
        }
        bounds.grow(tx - R, ty - R, tx + R, ty + R);
        trends.push_str("<g class=\"ondine-wardley-trend\" data-id=\"");
        trends.push_str(&esc_text(&trend.node_id));
        trends.push_str("\">");
        trends.push_str(&arrow);
        trends.push_str("<circle cx=\"");
        push_num(&mut trends, tx);
        trends.push_str("\" cy=\"");
        push_num(&mut trends, ty);
        trends.push_str("\" r=\"6\" fill=\"var(--_bg)\" stroke=\"");
        trends.push_str(ACCENT);
        trends.push_str("\"/></g>");
    }

    let mut drawn_nodes = String::new();
    let tri = 1.2 * R;
    let dot = 0.7 * R;
    for (at, node) in nodes.iter().enumerate() {
        let Some(&(x, y)) = positions.get(at) else {
            continue;
        };
        let anchor = node.class_name == Some("anchor");
        let strategy = node
            .source_strategy
            .as_deref()
            .filter(|strategy| !strategy.is_empty());
        let mut shape = String::new();
        if let Some(strategy) = strategy {
            let fill = match strategy {
                "outsource" => "var(--_m)",
                "buy" => "var(--_a)",
                "build" => "var(--_s)",
                _ => "var(--_bg)",
            };
            shape.push_str("<circle class=\"ondine-wardley-");
            shape.push_str(strategy);
            shape.push_str("\" cx=\"");
            push_num(&mut shape, x);
            shape.push_str("\" cy=\"");
            push_num(&mut shape, y);
            shape.push_str("\" r=\"12\" fill=\"");
            shape.push_str(fill);
            shape.push_str("\" stroke=\"");
            shape.push_str(LINE);
            shape.push_str("\"/>");
        }
        if strategy == Some("market") {
            let bx = tri * (std::f64::consts::PI / 6.0).cos();
            let by = y + tri * (std::f64::consts::PI / 6.0).sin();
            shape.push_str("<path d=\"M");
            push_num(&mut shape, x);
            shape.push(',');
            push_num(&mut shape, y - tri);
            shape.push('L');
            push_num(&mut shape, x - bx);
            shape.push(',');
            push_num(&mut shape, by);
            shape.push('H');
            push_num(&mut shape, x + bx);
            shape.push_str("Z\" fill=\"none\" stroke=\"");
            shape.push_str(LINE);
            shape.push_str("\"/>");
            for (cx, cy) in [(x, y - tri), (x - bx, by), (x + bx, by)] {
                shape.push_str("<circle cx=\"");
                push_num(&mut shape, cx);
                shape.push_str("\" cy=\"");
                push_num(&mut shape, cy);
                shape.push_str("\" r=\"");
                push_num(&mut shape, dot);
                shape.push_str("\" fill=\"var(--_bg)\" stroke=\"");
                shape.push_str(LINE);
                shape.push_str("\"/>");
            }
        } else if node.is_pipeline_parent {
            shape.push_str("<rect x=\"");
            push_num(&mut shape, x - SQUARE / 2.0);
            shape.push_str("\" y=\"");
            push_num(&mut shape, y - SQUARE / 2.0);
            shape.push_str("\" width=\"10\" height=\"10\" fill=\"var(--_bg)\" stroke=\"");
            shape.push_str(LINE);
            shape.push_str("\"/>");
        } else if !anchor {
            shape.push_str("<circle cx=\"");
            push_num(&mut shape, x);
            shape.push_str("\" cy=\"");
            push_num(&mut shape, y);
            shape.push_str("\" r=\"6\" fill=\"var(--_bg)\" stroke=\"");
            shape.push_str(LINE);
            shape.push_str("\"/>");
        }
        if node.inertia == Some(true) {
            let offset = (if node.is_pipeline_parent {
                SQUARE / 2.0
            } else {
                R
            }) + 10.0
                + if strategy.is_some() { R + 4.0 } else { 0.0 };
            shape.push_str("<rect class=\"ondine-wardley-inertia\" x=\"");
            push_num(&mut shape, x + offset);
            shape.push_str("\" y=\"");
            push_num(&mut shape, y - R);
            shape.push_str("\" width=\"4\" height=\"12\" fill=\"");
            shape.push_str(LINE);
            shape.push_str("\"/>");
        }
        let ox = node
            .label_offset_x
            .map(|offset| clamp(offset, -5000.0, 5000.0));
        let oy = node
            .label_offset_y
            .map(|offset| clamp(offset, -5000.0, 5000.0));
        let caption = if anchor {
            let label = match anchor_labels.get(at).and_then(Option::as_ref) {
                Some(label) => label.clone(),
                None => text(&node.label, small, anchor_wrap, BOLD),
            };
            bounds.centered(
                &label,
                x + ox.unwrap_or(0.0),
                y + oy.unwrap_or(-3.0),
                &sized(
                    " class=\"ondine-label\"",
                    small,
                    " font-weight=\"var(--_hw)\"",
                ),
            )
        } else {
            let label = text(&node.label, small, wrap, 0);
            let attrs = sized(" class=\"ondine-label\"", small, "");
            let lift = LABEL_OFFSET + if strategy.is_some() { 10.0 } else { 0.0 };
            if ox.is_some() || oy.is_some() {
                bounds.starting(
                    &label,
                    x + ox.unwrap_or(lift),
                    y + oy.unwrap_or(-lift),
                    &attrs,
                )
            } else if node.in_pipeline {
                bounds.centered(&label, x, y + 2.0 * R + 4.0 + label.height / 2.0, &attrs)
            } else {
                // The label goes up and to the right unless a link leaves that way; then the next
                // free corner.
                let busy = taken.get(at).copied().unwrap_or(0);
                let corner = [1, 2, 4, 8]
                    .into_iter()
                    .find(|bit| busy & bit == 0)
                    .unwrap_or(1);
                let right = corner == 1 || corner == 2;
                let up = corner == 1 || corner == 4;
                bounds.starting(
                    &label,
                    if right {
                        x + lift
                    } else {
                        x - lift - label.width
                    },
                    if up {
                        y - lift
                    } else {
                        y + lift + small * 0.75
                    },
                    &attrs,
                )
            }
        };
        bounds.grow(x - 2.0 * R, y - 2.0 * R, x + 2.0 * R + 20.0, y + 2.0 * R);
        drawn_nodes.push_str("<g class=\"ondine-node ondine-wardley-");
        drawn_nodes.push_str(if anchor {
            "anchor"
        } else if node.is_pipeline_parent {
            "pipeline-parent"
        } else if node.in_pipeline {
            "pipeline-component"
        } else {
            "component"
        });
        drawn_nodes.push_str("\" data-id=\"");
        drawn_nodes.push_str(&esc_text(&node.id));
        drawn_nodes.push_str("\">");
        drawn_nodes.push_str(&shape);
        drawn_nodes.push_str(&caption);
        drawn_nodes.push_str("</g>");
    }

    let mut annotations = String::new();
    for annotation in &build.annotations {
        let mut number = String::new();
        push_number(&mut number, annotation.number);
        let label = text(&number, tiny, 4000.0, BOLD);
        let mut marks = String::new();
        for &(cx, cy) in &annotation.coordinates {
            let x = project_x(cx);
            let y = project_y(cy);
            let r = 10.0_f64.max(label.width / 2.0 + 4.0);
            bounds.grow(x - r, y - r, x + r, y + r);
            marks.push_str("<circle cx=\"");
            push_num(&mut marks, x);
            marks.push_str("\" cy=\"");
            push_num(&mut marks, y);
            marks.push_str("\" r=\"");
            push_num(&mut marks, r);
            marks.push_str("\" fill=\"var(--_bg)\" stroke=\"");
            marks.push_str(LINE);
            marks.push_str("\"/>");
            marks.push_str(&label_svg(
                &label,
                x,
                y,
                &sized("", tiny, " font-weight=\"var(--_hw)\""),
                None,
            ));
        }
        annotations.push_str("<g class=\"ondine-wardley-annotation\" data-id=\"");
        annotations.push_str(&esc_text(&number));
        annotations.push_str("\">");
        annotations.push_str(&marks);
        annotations.push_str("</g>");
    }
    let mut listed: Vec<(f64, &str)> = build
        .annotations
        .iter()
        .filter_map(|annotation| {
            let text = annotation.text.as_deref().filter(|text| !text.is_empty())?;
            Some((annotation.number, text))
        })
        .collect();
    listed.sort_by(|a, b| {
        (a.0 - b.0)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !listed.is_empty() {
        let list_wrap = if tight {
            80.0_f64.max(plot_w - 20.0)
        } else {
            320.0
        };
        let lines: Vec<Label> = listed
            .iter()
            .map(|&(number, body)| {
                let mut line = String::new();
                push_number(&mut line, number);
                line.push_str(". ");
                line.push_str(body);
                text(&line, small, list_wrap, 0)
            })
            .collect();
        let mut width: f64 = 0.0;
        let mut height = 0.0;
        for line in &lines {
            width = width.max(line.width);
            height += line.height;
        }
        width += 20.0;
        height += 14.0;
        // Without a position the list goes under the map, where it covers nothing.
        let (bx, by) = match build.annotations_box {
            Some((x, y)) => (
                clamp(project_x(x), 0.0, 0.0_f64.max(plot_w - width)),
                clamp(project_y(y), 0.0, 0.0_f64.max(plot_h - height)),
            ),
            None => (0.0, legend_top),
        };
        bounds.grow(bx, by, bx + width, by + height);
        let mut rows = String::new();
        let mut y = by + 7.0;
        let attrs = sized("", small, "");
        for line in &lines {
            rows.push_str(&start_label(
                line,
                bx + 10.0,
                y + line.line_height / 2.0 + small * 0.35,
                &attrs,
            ));
            y += line.height;
        }
        annotations.push_str("<g class=\"ondine-wardley-annotations\"><rect x=\"");
        push_num(&mut annotations, bx);
        annotations.push_str("\" y=\"");
        push_num(&mut annotations, by);
        annotations.push_str("\" width=\"");
        push_num(&mut annotations, width);
        annotations.push_str("\" height=\"");
        push_num(&mut annotations, height);
        annotations.push_str("\" rx=\"");
        annotations.push_str(RADIUS);
        annotations.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/>");
        annotations.push_str(&rows);
        annotations.push_str("</g>");
    }

    let mut notes = String::new();
    let note_attrs = sized(
        " class=\"ondine-wardley-note\"",
        small,
        " font-weight=\"var(--_hw)\"",
    );
    for note in &build.notes {
        let label = text(&note.text, small, 360.0_f64.min(plot_w * 0.6), BOLD);
        notes.push_str(&bounds.starting(&label, project_x(note.x), project_y(note.y), &note_attrs));
    }

    // Accelerators point right, deaccelerators left.
    let mut forces = String::new();
    let force_attrs = sized("", tiny + 1.0, " font-weight=\"var(--_hw)\"");
    let forward_forces = build.accelerators.iter().map(|force| (force, true));
    let backward_forces = build.deaccelerators.iter().map(|force| (force, false));
    for (force, forward) in forward_forces.chain(backward_forces) {
        let x = project_x(force.x);
        let y = project_y(force.y);
        let half = FORCE_HEIGHT / 2.0;
        let (tip, tail, neck) = if forward {
            (x + FORCE_WIDTH, x, x + FORCE_WIDTH - FORCE_HEAD)
        } else {
            (x, x + FORCE_WIDTH, x + FORCE_HEAD)
        };
        bounds.grow(x, y - half - 8.0, x + FORCE_WIDTH, y + half + 8.0);
        forces.push_str("<g class=\"ondine-wardley-");
        forces.push_str(if forward {
            "accelerator"
        } else {
            "deaccelerator"
        });
        forces.push_str("\" data-id=\"");
        forces.push_str(&esc_text(&force.name));
        forces.push_str("\"><path d=\"M");
        push_num(&mut forces, tail);
        forces.push(',');
        push_num(&mut forces, y - half);
        forces.push('H');
        push_num(&mut forces, neck);
        forces.push('V');
        push_num(&mut forces, y - half - 8.0);
        forces.push('L');
        push_num(&mut forces, tip);
        forces.push(',');
        push_num(&mut forces, y);
        forces.push('L');
        push_num(&mut forces, neck);
        forces.push(',');
        push_num(&mut forces, y + half + 8.0);
        forces.push('V');
        push_num(&mut forces, y + half);
        forces.push('H');
        push_num(&mut forces, tail);
        forces.push_str("Z\" fill=\"var(--_s)\" stroke=\"");
        forces.push_str(LINE);
        forces.push_str("\"/>");
        let label = text(&force.name, tiny + 1.0, 160.0, BOLD);
        forces.push_str(&bounds.centered(
            &label,
            x + FORCE_WIDTH / 2.0,
            y + half + 8.0 + 12.0,
            &force_attrs,
        ));
        forces.push_str("</g>");
    }

    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        if tight { 80.0_f64.max(plot_w) } else { 4000.0 },
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let left = bounds.min_x.min(plot_w / 2.0 - title.width / 2.0);
    let right = bounds.max_x.max(plot_w / 2.0 + title.width / 2.0);
    let tx = pad - left;
    let ty = pad + title_height - bounds.min_y;
    let total_width = (right - left + 2.0 * pad).ceil();
    let total_height = (bounds.max_y - bounds.min_y + title_height + 2.0 * pad).ceil();

    let mut content = label_svg(
        &title,
        tx + plot_w / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g transform=\"translate(");
    push_num(&mut content, tx);
    content.push(',');
    push_num(&mut content, ty);
    content.push_str(")\"><g class=\"ondine-wardley-frame\" fill=\"none\">");
    content.push_str(&frame);
    content.push_str("</g>");
    content.push_str(&stage_labels);
    content.push_str(&axis_labels);
    content.push_str(&pipelines);
    if !links.is_empty() {
        content.push_str("<g class=\"ondine-edges\" fill=\"none\">");
        content.push_str(&links);
        content.push_str("</g>");
    }
    if !trends.is_empty() {
        content.push_str("<g class=\"ondine-wardley-trends\" fill=\"none\">");
        content.push_str(&trends);
        content.push_str("</g>");
    }
    content.push_str(&drawn_nodes);
    content.push_str(&annotations);
    content.push_str(&notes);
    content.push_str(&forces);
    content.push_str("</g>");

    Rendered {
        height: total_height,
        links: Vec::new(),
        svg: svg_document(
            "wardley",
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
