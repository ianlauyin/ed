//! Draws a quadrant chart. Port of Pele's `src/diagrams/quadrant/render.ts`.

use super::db::{QuadrantDb, QuadrantText};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::cap_width;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{RADIUS, class_names, resolve_style, series_color};
use crate::text::label::{Label, layout_label};
use crate::text::measure::BOLD;
use crate::yaml::{Value, push_number};

const GAP: f64 = 4.0;
const CAREFUL: usize = 300;

/// A point drawn on the chart, before its label is placed.
struct Dot {
    cx: f64,
    cy: f64,
    reach: f64,
    text: Label,
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

fn setting<'v>(config: &'v Value, key: &str) -> Option<&'v Value> {
    property(config, "quadrantChart").and_then(|section| property(section, key))
}

fn length(config: &Value, key: &str, fallback: f64, max: f64) -> f64 {
    match setting(config, key) {
        Some(Value::Number(value)) if *value >= 0.0 => value.min(max),
        _ => fallback,
    }
}

fn hex(color: &str) -> String {
    if color.starts_with('#') {
        color.to_owned()
    } else {
        ["#", color].concat()
    }
}

/// JavaScript's `parseInt` of a width such as `2px`: its leading digits.
fn parse_int(text: &str) -> f64 {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    text.get(..digits)
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(f64::NAN)
}

/// Draws a quadrant chart at its configured size, or narrower to fit the host's width.
#[expect(clippy::too_many_lines, reason = "mirrors Pele's renderQuadrant")]
pub(crate) fn render(model: &QuadrantDb, config: &Value, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let axis_size = js_round(size * 0.875);
    let point_size = js_round(size * 0.75);
    let natural = js_round(100.0_f64.max(length(config, "chartWidth", 500.0, 4000.0)));
    let width = js_round(cap_width(options, natural, 280.0, 0.0));
    let height =
        js_round((100.0_f64.max(length(config, "chartHeight", 500.0, 4000.0)) * width) / natural);
    let outer = length(
        config,
        "quadrantPadding",
        options.padding.unwrap_or(8.0),
        200.0,
    );
    let x_pad = length(config, "xAxisLabelPadding", 5.0, 200.0);
    let y_pad = length(config, "yAxisLabelPadding", 5.0, 200.0);
    let has_points = !model.points.is_empty();

    let label = |text: Option<&QuadrantText>, font_size: f64, max: f64| {
        layout_label(
            text.map(|text| text.text.as_str()),
            text.is_some_and(|text| text.markdown),
            font_size,
            max.max(40.0),
            0,
            None,
        )
    };

    let title = layout_label(
        model.title.as_deref(),
        false,
        size,
        (width - 2.0 * outer).max(40.0),
        BOLD,
        None,
    );
    let title_pad = length(config, "titlePadding", 10.0, 200.0);
    let title_space = if title.height > 0.0 {
        title.height + title_pad
    } else {
        0.0
    };

    let filled = |text: Option<&QuadrantText>| text.is_some_and(|text| !text.text.is_empty());
    let has_x = filled(model.x_axis_left.as_ref()) || filled(model.x_axis_right.as_ref());
    let has_y = filled(model.y_axis_bottom.as_ref()) || filled(model.y_axis_top.as_ref());
    // Mermaid moves the x axis labels below the chart as soon as there are points.
    let x_below = has_points
        || matches!(setting(config, "xAxisPosition"), Some(Value::String(position)) if position == "bottom");
    let y_right = matches!(setting(config, "yAxisPosition"), Some(Value::String(position)) if position == "right");

    let side = width - 2.0 * outer;
    let x_left = label(
        model.x_axis_left.as_ref(),
        axis_size,
        if model.x_axis_right.is_some() {
            side / 2.0 - 16.0
        } else {
            side
        },
    );
    let x_right = label(model.x_axis_right.as_ref(), axis_size, side / 2.0 - 16.0);
    let x_height = x_left.height.max(x_right.height);
    let x_space = if has_x { x_height + x_pad } else { 0.0 };
    let tall = height - 2.0 * outer - title_space - x_space;
    let y_bottom = label(
        model.y_axis_bottom.as_ref(),
        axis_size,
        if model.y_axis_top.is_some() {
            tall / 2.0 - 16.0
        } else {
            tall
        },
    );
    let y_top = label(model.y_axis_top.as_ref(), axis_size, tall / 2.0 - 16.0);
    let y_height = y_bottom.height.max(y_top.height);
    let y_space = if has_y { y_height + y_pad } else { 0.0 };

    let left = outer + if y_right { 0.0 } else { y_space };
    let top = outer + title_space + if x_below { 0.0 } else { x_space };
    let w = 0.0_f64.max(side - y_space);
    let h = 0.0_f64.max(tall);
    let tile_w = 0.0_f64.max((w - GAP) / 2.0);
    let tile_h = 0.0_f64.max((h - GAP) / 2.0);

    // Boxes a point label keeps clear of: the gaps between quadrants, quadrant labels, points,
    // and the labels placed before it.
    let mut taken: Vec<f64> = vec![
        left + tile_w,
        top,
        left + w - tile_w,
        top + h,
        left,
        top + tile_h,
        left + w,
        top + h - tile_h,
    ];
    let is_clear = |taken: &[f64], from: usize, x0: f64, y0: f64, x1: f64, y1: f64| {
        taken
            .get(from..)
            .unwrap_or(&[])
            .as_chunks::<4>()
            .0
            .iter()
            .all(|&[left, top, right, bottom]| {
                !(x0 < right && x1 > left && y0 < bottom && y1 > top)
            })
    };

    let text_top = length(config, "quadrantTextTopPadding", 5.0, 200.0);
    let mut quadrants = String::new();
    for k in 0..4 {
        let x = if k == 0 || k == 3 {
            left + w - tile_w
        } else {
            left
        };
        let y = if k < 2 { top } else { top + h - tile_h };
        let text = label(
            model.quadrants.get(k).and_then(Option::as_ref),
            size,
            tile_w - 16.0,
        );
        let cx = x + tile_w / 2.0;
        let cy = if has_points {
            y + text_top + text.height / 2.0
        } else {
            y + tile_h / 2.0
        };
        taken.extend([
            cx - text.width / 2.0,
            cy - text.height / 2.0,
            cx + text.width / 2.0,
            cy + text.height / 2.0,
        ]);
        quadrants.push_str("<g class=\"ondine-quadrant\" data-id=\"");
        quadrants.push(char::from(b'1' + u8::try_from(k).unwrap_or(0)));
        quadrants.push_str("\"><rect x=\"");
        push_num(&mut quadrants, x);
        quadrants.push_str("\" y=\"");
        push_num(&mut quadrants, y);
        quadrants.push_str("\" width=\"");
        push_num(&mut quadrants, tile_w);
        quadrants.push_str("\" height=\"");
        push_num(&mut quadrants, tile_h);
        quadrants.push_str("\" rx=\"");
        quadrants.push_str(RADIUS);
        quadrants.push_str("\"/>");
        quadrants.push_str(&label_svg(
            &text,
            cx,
            cy,
            " class=\"ondine-label\" fill=\"var(--_fg)\" stroke=\"none\"",
            None,
        ));
        quadrants.push_str("</g>");
    }

    let mut axes = String::new();
    if has_x {
        let y = if x_below {
            top + h + x_pad + x_height / 2.0
        } else {
            top - x_pad - x_height / 2.0
        };
        axes.push_str(&label_svg(
            &x_left,
            if x_right.height > 0.0 {
                left + w / 4.0
            } else {
                left + x_left.width / 2.0
            },
            y,
            "",
            None,
        ));
        axes.push_str(&label_svg(&x_right, left + (w * 3.0) / 4.0, y, "", None));
    }
    if has_y {
        let x = if y_right {
            left + w + y_pad + y_height / 2.0
        } else {
            left - y_pad - y_height / 2.0
        };
        let turned = |text: &Label, y: f64| {
            let mut out = String::new();
            if text.height > 0.0 {
                out.push_str("<g transform=\"translate(");
                push_num(&mut out, x);
                out.push(',');
                push_num(&mut out, y);
                out.push_str(") rotate(-90)\">");
                out.push_str(&label_svg(text, 0.0, 0.0, "", None));
                out.push_str("</g>");
            }
            out
        };
        axes.push_str(&turned(
            &y_bottom,
            if y_top.height > 0.0 {
                top + (h * 3.0) / 4.0
            } else {
                top + h - y_bottom.width / 2.0
            },
        ));
        axes.push_str(&turned(&y_top, top + h / 4.0));
    }

    let radius = length(config, "pointRadius", 5.0, 200.0);
    let text_pad = length(config, "pointTextPadding", 5.0, 200.0);
    let fill = series_color(0);
    let mut dots: Vec<Dot> = Vec::new();
    let mut circles: Vec<String> = Vec::new();
    for point in &model.points {
        if !(point.x >= 0.0 && point.x <= 1.0 && point.y >= 0.0 && point.y <= 1.0) {
            continue;
        }
        let shared = model.class(&point.class_name);
        let r = point
            .style
            .radius
            .or_else(|| shared.and_then(|shared| shared.radius))
            .unwrap_or(radius)
            .min(2000.0);
        let color = point
            .style
            .color
            .as_ref()
            .or_else(|| shared.and_then(|shared| shared.color.as_ref()));
        let stroke_color = point
            .style
            .stroke_color
            .as_ref()
            .or_else(|| shared.and_then(|shared| shared.stroke_color.as_ref()));
        let stroke_width = point
            .style
            .stroke_width
            .as_ref()
            .or_else(|| shared.and_then(|shared| shared.stroke_width.as_ref()));
        let mut styles: Vec<String> = Vec::new();
        if let Some(color) = color.filter(|color| !color.is_empty()) {
            styles.push(["fill:", &hex(color)].concat());
        }
        let stroke_width = stroke_width.filter(|width| !width.is_empty());
        let stroke_color = stroke_color.filter(|color| !color.is_empty());
        if let Some(stroke_width) = stroke_width {
            if let Some(stroke_color) = stroke_color {
                styles.push(["stroke:", &hex(stroke_color)].concat());
            }
            styles.push(["stroke-width:", stroke_width].concat());
        }
        let cx = left + point.x * w;
        let cy = top + (1.0 - point.y) * h;
        dots.push(Dot {
            cx,
            cy,
            reach: r + stroke_width.map_or(0.0, |width| parse_int(width).min(400.0) / 2.0),
            text: label(Some(&point.text), point_size, 200.0),
        });

        let mut circle = String::from("<circle class=\"ondine-point");
        circle.push_str(&class_names(&point.class_name));
        circle.push_str("\" data-id=\"");
        circle.push_str(&esc_text(&point.text.text));
        circle.push_str("\" cx=\"");
        push_num(&mut circle, cx);
        circle.push_str("\" cy=\"");
        push_num(&mut circle, cy);
        circle.push_str("\" r=\"");
        push_num(&mut circle, r);
        circle.push('"');
        if stroke_width.is_some() && stroke_color.is_none() {
            circle.push_str(" stroke=\"");
            circle.push_str(&fill);
            circle.push('"');
        }
        circle.push_str(&resolve_style(&styles).shape);
        circle.push_str("/>");
        circles.push(circle);
    }

    // A label goes below its point, or above, right or left of it when that is the first place
    // that is inside the chart and free. A second round lets it cross the gaps between quadrants.
    // Large charts skip the search for a free place.
    let careful = dots.len() <= CAREFUL;
    if careful {
        for dot in &dots {
            taken.extend([
                dot.cx - dot.reach,
                dot.cy - dot.reach,
                dot.cx + dot.reach,
                dot.cy + dot.reach,
            ]);
        }
    }
    let mut labels = String::new();
    for dot in &dots {
        let (cx, cy, text) = (dot.cx, dot.cy, &dot.text);
        let half_w = text.width / 2.0;
        let half_h = text.size * 0.6 + (text.height - text.line_height) / 2.0;
        let away = dot.reach + text_pad;
        let spots = [
            cx,
            cy + away + half_h,
            cx,
            cy - away - half_h,
            cx + away + half_w,
            cy,
            cx - away - half_w,
            cy,
        ];
        let mut tx = cx;
        let mut ty = spots[1];
        let mut found = false;
        for k in (0..16).step_by(2) {
            let spot = k % 8;
            let at = spots.get(spot).copied().unwrap_or(f64::NAN);
            let x = if spot < 4 && text.width < w {
                at.max(left + half_w).min(left + w - half_w)
            } else {
                at
            };
            let y = spots.get(spot + 1).copied().unwrap_or(f64::NAN);
            if x - half_w < left
                || x + half_w > left + w
                || y - half_h < top
                || y + half_h > top + h
            {
                continue;
            }
            let free = !careful
                || is_clear(
                    &taken,
                    if k < 8 { 0 } else { 8 },
                    x - half_w,
                    y - half_h,
                    x + half_w,
                    y + half_h,
                );
            if !found || free {
                tx = x;
                ty = y;
                found = true;
            }
            if free {
                break;
            }
        }
        if careful {
            taken.extend([tx - half_w, ty - half_h, tx + half_w, ty + half_h]);
        }
        labels.push_str(&label_svg(text, tx, ty, " class=\"ondine-label\"", None));
    }

    // Mermaid puts each new point under the ones before it.
    let mut content = label_svg(
        &title,
        width / 2.0,
        title_pad / 2.0 + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g class=\"ondine-quadrants\" fill=\"var(--_s)\" stroke=\"var(--_b)\">");
    content.push_str(&quadrants);
    content.push_str("</g>");
    if !axes.is_empty() {
        content.push_str("<g class=\"ondine-axis-labels\" font-size=\"");
        push_number(&mut content, axis_size);
        content.push_str("\" fill=\"var(--_m)\">");
        content.push_str(&axes);
        content.push_str("</g>");
    }
    if !dots.is_empty() {
        content.push_str("<g class=\"ondine-points\" fill=\"");
        content.push_str(&fill);
        content.push_str("\">");
        for circle in circles.iter().rev() {
            content.push_str(circle);
        }
        content.push_str("</g><g class=\"ondine-point-labels\" font-size=\"");
        push_number(&mut content, point_size);
        content.push_str("\">");
        content.push_str(&labels);
        content.push_str("</g>");
    }

    Rendered {
        height,
        links: Vec::new(),
        svg: svg_document(
            "quadrantChart",
            width,
            height,
            size,
            options,
            Accessible {
                acc_descr: model.acc_descr.as_deref(),
                acc_title: model.acc_title.as_deref(),
            },
            &content,
        ),
        width,
    }
}
