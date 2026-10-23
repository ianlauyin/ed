//! Draws a radar chart: a graticule, axes, one closed curve per series, and a legend. Port of
//! Pele's `src/diagrams/radar/render.ts`.

use std::f64::consts::PI;

use super::model::RadarModel;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{fit_width, title_room};
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::series_color;
use crate::text::label::{Label, layout_label};
use crate::text::measure::BOLD;
use crate::yaml::{Value, push_number};

/// The side of a legend swatch.
const SWATCH: f64 = 12.0;
/// The height of a legend row.
const ROW: f64 = 26.0;
/// The space between the chart and its legend.
const GAP: f64 = 24.0;

/// A point of the chart, around its center.
#[derive(Clone, Copy, Debug)]
struct Point {
    x: f64,
    y: f64,
}

/// The `radar` option `key` of `config` when it is a number at least `min`, capped at `max`, or
/// `fallback`.
fn option(config: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match property(config, "radar").and_then(|radar| property(radar, key)) {
        Some(Value::Number(value)) if *value >= min => value.min(max),
        _ => fallback,
    }
}

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

/// The distance from the center of `value` between `min` at the center and `max` at `radius`.
fn relative_radius(value: f64, min: f64, max: f64, radius: f64) -> f64 {
    let clipped = value.max(min).min(max);
    (radius * (clipped - min)) / (max - min)
}

/// Appends `x,y`.
fn push_point(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// A closed Catmull-Rom spline through the points, as cubic Bézier segments.
fn closed_round_curve(points: &[Point], tension: f64) -> String {
    let n = points.len();
    let mut d = String::from("M");
    let Some(first) = points.first() else {
        return d;
    };
    push_point(&mut d, first.x, first.y);
    let at = |i: usize| points.get(i % n).copied().unwrap_or(*first);
    for i in 0..n {
        let p0 = at(i + n - 1);
        let p1 = at(i);
        let p2 = at(i + 1);
        let p3 = at(i + 2);
        d.push_str(" C");
        push_point(
            &mut d,
            p1.x + (p2.x - p0.x) * tension,
            p1.y + (p2.y - p0.y) * tension,
        );
        d.push(' ');
        push_point(
            &mut d,
            p2.x - (p3.x - p1.x) * tension,
            p2.y - (p3.y - p1.y) * tension,
        );
        d.push(' ');
        push_point(&mut d, p2.x, p2.y);
    }
    d.push_str(" Z");
    d
}

/// The `points` attribute of a polygon through `points`.
fn polygon(points: &[Point]) -> String {
    let mut out = String::new();
    for p in points {
        if !out.is_empty() {
            out.push(' ');
        }
        push_point(&mut out, p.x, p.y);
    }
    out
}

/// Draws `model` with the `radar` section of `config`, narrowed to fit the host's width.
pub(crate) fn render(model: &RadarModel, config: &Value, options: &RenderOptions) -> Rendered {
    let natural = option(config, "width", 320.0, 1.0, 4000.0);
    fit_width(options, natural, 160.0, |width, tight| {
        let height = (option(config, "height", 320.0, 1.0, 4000.0) * width) / natural;
        draw(model, config, options, width, height, tight)
    })
}

/// `tight` is set when the chart does not fit the width it has with its legend beside it.
fn draw(
    model: &RadarModel,
    config: &Value,
    options: &RenderOptions,
    width: f64,
    height: f64,
    tight: bool,
) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);

    let axis_scale = option(config, "axisScaleFactor", 1.0, 0.0, 10.0);
    let label_factor = option(config, "axisLabelFactor", 1.05, 0.0, 10.0);
    let tension = option(config, "curveTension", 0.17, 0.0, 1.0);
    let radius = width.min(height) / 2.0;

    let axes = &model.axes;
    let curves = &model.curves;
    let ticks = model.options.ticks;
    let circle = model.options.graticule == "circle";
    let min = model.options.min;
    #[expect(clippy::cast_precision_loss, reason = "axis counts are small")]
    let count = axes.len() as f64;
    let unit: Vec<Point> = (0..axes.len())
        .map(|i| {
            #[expect(clippy::cast_precision_loss, reason = "axis counts are small")]
            let angle = (2.0 * PI * i as f64) / count - PI / 2.0;
            Point {
                x: angle.cos(),
                y: angle.sin(),
            }
        })
        .collect();

    let max = model.options.max.unwrap_or_else(|| {
        let mut max = f64::NEG_INFINITY;
        for value in curves.iter().flat_map(|curve| &curve.entries) {
            if *value > max {
                max = *value;
            }
        }
        max
    });

    // The chart is drawn around the origin. Margins reserve space; labels and the legend widen it.
    let mut x0 = -width / 2.0 - option(config, "marginLeft", 16.0, 0.0, 2000.0);
    let mut x1 = width / 2.0 + option(config, "marginRight", 16.0, 0.0, 2000.0);
    let mut y0 = -height / 2.0 - option(config, "marginTop", 16.0, 0.0, 2000.0);
    let mut y1 = height / 2.0 + option(config, "marginBottom", 16.0, 0.0, 2000.0);

    let mut grid = String::new();
    let mut i = 0.0;
    while i < ticks {
        let r = radius.min((radius * (i + 1.0)) / ticks);
        if circle {
            grid.push_str("<circle r=\"");
            push_num(&mut grid, r);
            grid.push_str("\"/>");
        } else {
            let points: Vec<Point> = unit
                .iter()
                .map(|u| Point {
                    x: u.x * r,
                    y: u.y * r,
                })
                .collect();
            grid.push_str("<polygon points=\"");
            grid.push_str(&polygon(&points));
            grid.push_str("\"/>");
        }
        i += 1.0;
    }

    let mut spokes = String::new();
    let mut labels = String::new();
    for (u, axis) in unit.iter().zip(axes) {
        let reach = radius * axis_scale;
        spokes.push_str("M0,0L");
        push_point(&mut spokes, u.x * reach, u.y * reach);
        let label = layout_label(Some(&axis.label), false, small, 4000.0, 0, None);
        let at = radius * label_factor + 6.0;
        // Each label hangs off its axis on the side away from the center.
        let away = |along: f64, extent: f64| {
            if along > 0.01 {
                extent / 2.0
            } else if along < -0.01 {
                -extent / 2.0
            } else {
                0.0
            }
        };
        let cx = u.x * at + away(u.x, label.width);
        let cy = u.y * at + away(u.y, label.height);
        labels.push_str("<g class=\"ondine-axis-label\" data-id=\"");
        labels.push_str(&esc_text(&axis.name));
        labels.push_str("\">");
        labels.push_str(&label_svg(&label, cx, cy, "", None));
        labels.push_str("</g>");
        x0 = x0.min(cx - label.width / 2.0);
        x1 = x1.max(cx + label.width / 2.0);
        y0 = y0.min(cy - label.height / 2.0);
        y1 = y1.max(cy + label.height / 2.0);
    }

    let mut shapes = String::new();
    for (index, curve) in curves.iter().enumerate() {
        // Mermaid leaves out a curve that does not have a value for every axis.
        if curve.entries.len() != axes.len() {
            continue;
        }
        let points: Vec<Point> = curve
            .entries
            .iter()
            .zip(&unit)
            .map(|(&value, u)| {
                let r = relative_radius(value, min, max, radius);
                let safe = if r.is_finite() { r } else { 0.0 };
                Point {
                    x: u.x * safe,
                    y: u.y * safe,
                }
            })
            .collect();
        let color = series_color(i64::try_from(index).unwrap_or(0));
        let mut attrs = String::from(" class=\"ondine-curve\" data-id=\"");
        attrs.push_str(&esc_text(&curve.name));
        attrs.push_str("\" fill=\"");
        attrs.push_str(&color);
        attrs.push_str("\" stroke=\"");
        attrs.push_str(&color);
        attrs.push('"');
        if circle {
            shapes.push_str("<path");
            shapes.push_str(&attrs);
            shapes.push_str(" d=\"");
            shapes.push_str(&closed_round_curve(&points, tension));
        } else {
            shapes.push_str("<polygon");
            shapes.push_str(&attrs);
            shapes.push_str(" points=\"");
            shapes.push_str(&polygon(&points));
        }
        shapes.push_str("\"/>");
    }

    let legend: Vec<Label> = if model.options.show_legend {
        curves
            .iter()
            .map(|curve| layout_label(Some(&curve.label), false, small, 4000.0, 0, None))
            .collect()
    } else {
        Vec::new()
    };
    let mut legend_width: f64 = 0.0;
    for label in &legend {
        legend_width = legend_width.max(label.width);
    }
    #[expect(clippy::cast_precision_loss, reason = "curve counts are small")]
    let legend_height = legend.len() as f64 * ROW;
    let legend_x = if tight {
        -(SWATCH + 8.0 + legend_width) / 2.0
    } else {
        x1 + GAP
    };
    let legend_top = if tight {
        y1 + GAP / 2.0
    } else {
        -legend_height / 2.0
    };
    if !legend.is_empty() && tight {
        x0 = x0.min(legend_x);
        x1 = x1.max(-legend_x);
        y1 = legend_top + legend_height;
    } else if !legend.is_empty() {
        x1 = legend_x + SWATCH + 8.0 + legend_width;
        y0 = y0.min(-legend_height / 2.0);
        y1 = y1.max(legend_height / 2.0);
    }
    let mut legend_out = String::new();
    for (index, (label, curve)) in legend.iter().zip(curves).enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "curve counts are small")]
        let y = legend_top + index as f64 * ROW + ROW / 2.0;
        let color = series_color(i64::try_from(index).unwrap_or(0));
        legend_out.push_str("<g class=\"ondine-legend-item\" data-id=\"");
        legend_out.push_str(&esc_text(&curve.name));
        legend_out.push_str("\"><rect x=\"");
        push_num(&mut legend_out, legend_x);
        legend_out.push_str("\" y=\"");
        push_num(&mut legend_out, y - SWATCH / 2.0);
        legend_out.push_str("\" width=\"12\" height=\"12\" rx=\"2\" fill=\"");
        legend_out.push_str(&color);
        legend_out.push_str("\" fill-opacity=\"0.4\" stroke=\"");
        legend_out.push_str(&color);
        legend_out.push_str("\"/>");
        legend_out.push_str(&label_svg(
            label,
            legend_x + SWATCH + 8.0 + label.width / 2.0,
            y,
            "",
            None,
        ));
        legend_out.push_str("</g>");
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
        title.height + 8.0
    } else {
        0.0
    };
    let body_width = (x1 - x0).max(title.width);
    let total_width = (body_width + 2.0 * pad).ceil();
    let total_height = (y1 - y0 + title_height + 2.0 * pad).ceil();

    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g transform=\"translate(");
    push_point(
        &mut content,
        pad + (body_width - (x1 - x0)) / 2.0 - x0,
        pad + title_height - y0,
    );
    content.push_str(")\">");
    if !grid.is_empty() {
        content.push_str("<g class=\"ondine-graticule\" fill=\"none\" stroke=\"var(--_a)\">");
        content.push_str(&grid);
        content.push_str("</g>");
    }
    if !spokes.is_empty() {
        content.push_str("<path class=\"ondine-axes\" d=\"");
        content.push_str(&spokes);
        content.push_str("\" fill=\"none\" stroke=\"var(--_b)\"/>");
    }
    if !shapes.is_empty() {
        content.push_str(
            "<g class=\"ondine-curves\" fill-opacity=\"0.2\" stroke-width=\"1.5\" stroke-linejoin=\"round\">",
        );
        content.push_str(&shapes);
        content.push_str("</g>");
    }
    if !labels.is_empty() {
        content.push_str("<g class=\"ondine-axis-labels\" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\">");
        content.push_str(&labels);
        content.push_str("</g>");
    }
    if !legend_out.is_empty() {
        content.push_str("<g class=\"ondine-legend\" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\">");
        content.push_str(&legend_out);
        content.push_str("</g>");
    }
    content.push_str("</g>");

    let svg = svg_document(
        "radar",
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
