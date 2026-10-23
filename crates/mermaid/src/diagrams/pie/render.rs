//! Draws a pie chart: slices, percentages, and a legend. Port of Pele's
//! `src/diagrams/pie/render.ts`.

use std::f64::consts::PI;

use super::model::PieModel;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{fit_width, title_room};
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::series_color;
use crate::text::label::{Label, layout_label};
use crate::text::measure::{self, BOLD};
use crate::yaml::{Value, push_number};

/// The pie radius before fitting.
const RADIUS: f64 = 120.0;
/// The side of a legend swatch.
const SWATCH: f64 = 12.0;
/// The height of a legend row.
const ROW: f64 = 26.0;
/// The space between the pie and a legend beside it.
const GAP: f64 = 32.0;
/// The margin a percentage outside the rim needs.
const OUTSIDE: f64 = 28.0;

/// The `pie` option `key` of `config`.
fn option<'v>(config: &'v Value, key: &str) -> Option<&'v Value> {
    let property = |value: &'v Value, key: &str| match value {
        Value::Object(entries) => entries
            .iter()
            .find(|(own, _)| own == key)
            .map(|(_, value)| value),
        _ => None,
    };
    property(property(config, "pie")?, key)
}

/// Appends the point at `angle` (clockwise from the top) and `radius`.
fn push_point(out: &mut String, angle: f64, radius: f64) {
    push_num(out, angle.sin() * radius);
    out.push(',');
    push_num(out, -angle.cos() * radius);
}

/// Appends an arc command of `radius` up to its end point.
fn push_arc(out: &mut String, radius: f64, large: bool, sweep: bool) {
    out.push('A');
    push_num(out, radius);
    out.push(',');
    push_num(out, radius);
    out.push_str(if large { " 0 1 " } else { " 0 0 " });
    out.push_str(if sweep { "1 " } else { "0 " });
}

/// Appends a full circle of radius `r` as two arcs.
fn push_ring(out: &mut String, r: f64, sweep: bool) {
    out.push_str("M0,");
    push_num(out, -r);
    push_arc(out, r, true, sweep);
    out.push_str("0,");
    push_num(out, r);
    push_arc(out, r, true, sweep);
    out.push_str("0,");
    push_num(out, -r);
    out.push('Z');
}

/// The path of a slice from angle `start` to `end`, a ring when `inner` is positive.
fn slice(start: f64, end: f64, outer: f64, inner: f64) -> String {
    let mut out = String::new();
    if end - start >= PI * 2.0 - 1e-9 {
        push_ring(&mut out, outer, true);
        if inner > 0.0 {
            push_ring(&mut out, inner, false);
        }
        return out;
    }

    let large = end - start > PI;
    if inner <= 0.0 {
        out.push_str("M0,0L");
        push_point(&mut out, start, outer);
        push_arc(&mut out, outer, large, true);
        push_point(&mut out, end, outer);
        out.push('Z');
        return out;
    }
    out.push('M');
    push_point(&mut out, start, outer);
    push_arc(&mut out, outer, large, true);
    push_point(&mut out, end, outer);
    out.push('L');
    push_point(&mut out, end, inner);
    push_arc(&mut out, inner, large, false);
    push_point(&mut out, start, inner);
    out.push('Z');
    out
}

/// `value.toFixed(0)` for a finite value that is not negative.
fn fixed0(value: f64) -> String {
    let mut out = String::new();
    push_number(&mut out, js_round(value));
    out
}

/// Draws `model` with the `pie` section of `config`, narrowed to fit the host's width.
pub(crate) fn render(model: &PieModel, config: &Value, options: &RenderOptions) -> Rendered {
    fit_width(options, RADIUS * 2.0, 140.0, |diameter, tight| {
        draw(model, config, options, diameter / 2.0, tight)
    })
}

/// A percentage label placed outside the rim.
struct Outside {
    width: f64,
    x: f64,
    y: f64,
}

/// `tight` is set when the chart does not fit the width it has with its legend beside it.
fn draw(
    model: &PieModel,
    config: &Value,
    options: &RenderOptions,
    radius: f64,
    tight: bool,
) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);

    let text_position = match option(config, "textPosition") {
        Some(Value::Number(position)) if (0.0..=1.0).contains(position) => *position,
        _ => 0.75,
    };
    let inner = match option(config, "donutHole") {
        Some(Value::Number(hole)) if *hole > 0.0 && *hole <= 0.9 => hole * radius,
        _ => 0.0,
    };
    let placed = match option(config, "legendPosition") {
        None | Some(Value::Null) => String::from("right"),
        Some(value) => {
            let mut out = String::new();
            value.push_js_string(&mut out);
            out
        }
    };
    let legend_position = if tight && (placed == "left" || placed == "right") {
        "bottom"
    } else {
        placed.as_str()
    };

    let entries = &model.sections;
    let mut sum = 0.0;
    for (_, value) in entries {
        sum += value;
    }

    let legend: Vec<Label> = entries
        .iter()
        .map(|(label, value)| {
            let text = if model.show_data {
                let mut text = label.clone();
                text.push_str(" [");
                push_number(&mut text, *value);
                text.push(']');
                text
            } else {
                label.clone()
            };
            layout_label(Some(&text), false, small, 4000.0, 0, None)
        })
        .collect();
    let mut legend_width: f64 = 0.0;
    for label in &legend {
        legend_width = legend_width.max(label.width);
    }
    legend_width += if legend.is_empty() { 0.0 } else { SWATCH + 8.0 };
    #[expect(clippy::cast_precision_loss, reason = "slice counts are small")]
    let legend_height = legend.len() as f64 * ROW;

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

    // A slice too thin for its percentage gets the label outside the rim, which needs a margin.
    let shares: Vec<f64> = entries
        .iter()
        .map(|(_, value)| {
            let share = if sum > 0.0 { value / sum } else { 0.0 };
            if share * 100.0 >= 1.0 && fixed0(share * 100.0) != "0" {
                share
            } else {
                0.0
            }
        })
        .collect();
    let narrow: Vec<bool> = shares
        .iter()
        .map(|&share| {
            if share == 0.0 || share >= 0.5 {
                return false;
            }
            let chord = 2.0 * radius * text_position * (share * PI).sin();
            let mut text = fixed0(share * 100.0);
            text.push('%');
            chord < measure::width(&text, small, 0) + 6.0
        })
        .collect();
    let margin = if narrow.contains(&true) { OUTSIDE } else { 0.0 };

    let side = match legend_position {
        "left" | "right" | "center" => legend_position,
        _ => "",
    };
    let stacked = legend_position == "top" || legend_position == "bottom";
    let diameter = (radius + margin) * 2.0;
    let mut width = diameter;
    let mut height = diameter;
    if side == "left" || side == "right" {
        width += if legend_width > 0.0 {
            GAP + legend_width
        } else {
            0.0
        };
        height = height.max(legend_height);
    } else if stacked {
        width = width.max(legend_width);
        height += if legend_height > 0.0 {
            20.0 + legend_height
        } else {
            0.0
        };
    }
    width = width.max(title.width);

    let cx = pad
        + match side {
            "left" => legend_width + GAP + diameter / 2.0,
            "right" => diameter / 2.0,
            _ => width / 2.0,
        };
    let body_top = pad + title_height;
    let cy = body_top
        + if legend_position == "top" {
            legend_height + 20.0 + diameter / 2.0
        } else if stacked {
            diameter / 2.0
        } else {
            height / 2.0
        };

    let mut slices = String::new();
    let mut labels = String::new();
    let mut outside = String::new();
    let mut angle = 0.0;
    let rim = radius + OUTSIDE / 2.0;
    let mut placed_outside: Vec<Outside> = Vec::new();
    let clear = |placed: &[Outside], x: f64, y: f64, width: f64| {
        placed.iter().all(|other| {
            (x - other.x).abs() >= f64::midpoint(width, other.width) + 2.0
                || (y - other.y).abs() >= small + 2.0
        })
    };
    for (index, ((label, _), &share)) in entries.iter().zip(&shares).enumerate() {
        if share == 0.0 {
            continue;
        }
        let end = angle + share * PI * 2.0;
        slices.push_str("<path class=\"ondine-slice\" data-id=\"");
        slices.push_str(&esc_text(label));
        slices.push_str("\" d=\"");
        slices.push_str(&slice(angle, end, radius, inner));
        slices.push_str("\" fill=\"");
        slices.push_str(&series_color(i64::try_from(index).unwrap_or(0)));
        slices.push_str("\"/>");

        let mid = f64::midpoint(angle, end);
        let mut text = fixed0(share * 100.0);
        text.push('%');
        if narrow.get(index).copied().unwrap_or(false) {
            let width = measure::width(&text, small, 0);
            let mut at = mid;
            let mut step = 0;
            while step < 360 && !clear(&placed_outside, at.sin() * rim, -at.cos() * rim, width) {
                at += 2.0 / rim;
                step += 1;
            }
            let x = at.sin() * rim;
            let y = -at.cos() * rim;
            placed_outside.push(Outside { width, x, y });
            outside.push_str("<text x=\"");
            push_num(&mut outside, x);
            outside.push_str("\" y=\"");
            push_num(&mut outside, y + small * 0.35);
            outside.push_str("\">");
            outside.push_str(&text);
            outside.push_str("</text>");
        } else {
            let r = radius * text_position;
            labels.push_str("<text x=\"");
            push_num(&mut labels, mid.sin() * r);
            labels.push_str("\" y=\"");
            push_num(&mut labels, -mid.cos() * r + small * 0.35);
            labels.push_str("\">");
            labels.push_str(&text);
            labels.push_str("</text>");
        }
        angle = end;
    }
    if slices.is_empty() {
        slices.push_str("<circle class=\"ondine-slice\" r=\"");
        push_number(&mut slices, radius);
        slices.push_str("\" fill=\"none\" stroke=\"var(--_b)\"/>");
    }

    let (lx, ly) = match side {
        "right" => (
            cx + diameter / 2.0 + GAP,
            body_top + (height - legend_height) / 2.0,
        ),
        "left" => (pad, body_top + (height - legend_height) / 2.0),
        "center" => (cx - legend_width / 2.0, cy - legend_height / 2.0),
        _ => (
            pad + (width - legend_width) / 2.0,
            if legend_position == "top" {
                body_top
            } else {
                body_top + diameter + 20.0
            },
        ),
    };
    let mut legend_out = String::new();
    for (index, (label, (name, _))) in legend.iter().zip(entries).enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "slice counts are small")]
        let y = ly + index as f64 * ROW + ROW / 2.0;
        legend_out.push_str("<g class=\"ondine-legend-item\" data-id=\"");
        legend_out.push_str(&esc_text(name));
        legend_out.push_str("\"><rect x=\"");
        push_num(&mut legend_out, lx);
        legend_out.push_str("\" y=\"");
        push_num(&mut legend_out, y - SWATCH / 2.0);
        legend_out.push_str("\" width=\"12\" height=\"12\" rx=\"2\" fill=\"");
        legend_out.push_str(&series_color(i64::try_from(index).unwrap_or(0)));
        legend_out.push_str("\"/>");
        legend_out.push_str(&label_svg(
            label,
            lx + SWATCH + 8.0 + label.width / 2.0,
            y,
            "",
            None,
        ));
        legend_out.push_str("</g>");
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut translate = String::from(" transform=\"translate(");
    push_num(&mut translate, cx);
    translate.push(',');
    push_num(&mut translate, cy);
    translate.push_str(")\"");

    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g class=\"ondine-slices\"");
    content.push_str(&translate);
    content.push_str(" stroke=\"var(--_bg)\" stroke-width=\"1\" stroke-linejoin=\"round\">");
    content.push_str(&slices);
    content.push_str("</g>");
    if !labels.is_empty() {
        content.push_str("<g class=\"ondine-slice-labels\"");
        content.push_str(&translate);
        content.push_str(" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\" text-anchor=\"middle\" fill=\"var(--_bg)\">");
        content.push_str(&labels);
        content.push_str("</g>");
    }
    if !outside.is_empty() {
        content.push_str("<g class=\"ondine-slice-labels\"");
        content.push_str(&translate);
        content.push_str(" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\" text-anchor=\"middle\">");
        content.push_str(&outside);
        content.push_str("</g>");
    }
    if !legend_out.is_empty() {
        content.push_str("<g class=\"ondine-legend\" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\">");
        content.push_str(&legend_out);
        content.push_str("</g>");
    }

    let svg = svg_document(
        "pie",
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
