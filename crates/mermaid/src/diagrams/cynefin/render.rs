//! Draws a Cynefin diagram: four quadrants and the central confusion ellipse with their items,
//! the boundaries between them, and transitions. Port of Pele's
//! `src/diagrams/cynefin/render.ts`.

use super::boundaries::{
    FOLD, HORIZON, WaveOptions, backward, cliff, curve_path, ellipse_path, forward, resolve_seed,
    wave,
};
use super::model::CynefinModel;
use crate::diagrams::Rendered;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{RADIUS, series_color};
use crate::text::label::{Label, layout_label};
use crate::text::measure::{BOLD, ITALIC};
use crate::yaml::{Value, push_number};

/// `[domain, column, row, series color]`, in drawing order.
const QUADRANTS: [(&str, f64, f64, i64); 4] = [
    ("complex", 0.0, 0.0, 0),
    ("complicated", 1.0, 0.0, 1),
    ("chaotic", 0.0, 1.0, 3),
    ("clear", 1.0, 1.0, 2),
];
/// The series color of the confusion ellipse.
const CONFUSION_COLOR: i64 = 4;

/// `[name, decision model, kind of practice]` of each domain.
fn meta(name: &str) -> [&'static str; 3] {
    match name {
        "complex" => ["Complex", "Probe → Sense → Respond", "Emergent Practices"],
        "complicated" => ["Complicated", "Sense → Analyse → Respond", "Good Practices"],
        "clear" => ["Clear", "Sense → Categorise → Respond", "Best Practices"],
        "chaotic" => ["Chaotic", "Act → Sense → Respond", "Novel Practices"],
        _ => ["Confusion", "", "Disorder"],
    }
}

/// The most confusion items drawn before a `+n more` badge.
const MAX_CONFUSION_ITEMS: usize = 3;
/// The width at which item and transition labels wrap.
const ITEM_MAX_WIDTH: f64 = 200.0;
/// Horizontal padding inside a badge.
const ITEM_PAD_X: f64 = 10.0;
/// Vertical padding inside a badge.
const ITEM_PAD_Y: f64 = 4.0;
/// Space between badges.
const ITEM_GAP: f64 = 6.0;
/// Space between a heading and the first badge.
const HEAD_GAP: f64 = 10.0;
/// Space around a block inside its quadrant.
const MARGIN: f64 = 20.0;
/// Space between a block and the ellipse.
const CLEARANCE: f64 = 10.0;
/// Samples along a transition when finding where it leaves and enters blocks.
const STEPS: u32 = 48;

/// An item drawn as a badge.
struct Badge {
    h: f64,
    label: Label,
    overflow: bool,
    text: String,
    w: f64,
}

/// A domain's heading and badges, centered on (`cx`, `cy`).
struct Block {
    badges: Vec<Badge>,
    cx: f64,
    cy: f64,
    h: f64,
    name: &'static str,
    subtitles: Vec<Label>,
    title: Label,
    w: f64,
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

/// The `cynefin` option `key` of `config` when it is a number from `min` to `max`, or
/// `fallback`.
fn setting(config: &Value, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match property(config, "cynefin").and_then(|cynefin| property(cynefin, key)) {
        Some(Value::Number(value)) if *value >= min && *value <= max => *value,
        _ => fallback,
    }
}

/// V8's `Math.hypot` for two values: a compensated sum of squares scaled by the larger value.
/// It differs from `f64::hypot` in the last bit.
fn hypot(x: f64, y: f64) -> f64 {
    let x = x.abs();
    let y = y.abs();
    if x == f64::INFINITY || y == f64::INFINITY {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let max = if y > x { y } else { x };
    if max == 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in [x, y] {
        let n = value / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `value || 1`.
fn or_one(value: f64) -> f64 {
    if value == 0.0 || value.is_nan() {
        1.0
    } else {
        value
    }
}

/// Appends `x,y`.
fn push_pair(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Draws `model` with the `cynefin` section of `config`.
pub(crate) fn render(model: &CynefinModel, config: &Value, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.8125);
    let item_size = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);
    let show_descriptions = !matches!(
        property(config, "cynefin").and_then(|cynefin| property(cynefin, "showDomainDescriptions")),
        Some(Value::Bool(false))
    );
    let amplitude = setting(config, "boundaryAmplitude", 8.0, 0.0, 50.0);
    // Pele seeds from its default prefix. Ondine keeps that seed, so the default boundaries match.
    let seed = resolve_seed(
        setting(config, "seed", 0.0, -1e15, 1e15),
        options.id_prefix.as_deref().unwrap_or("pele"),
    );
    let item_line = js_round(item_size * 1.5);

    let badge = |text: String, overflow: bool| {
        let label = layout_label(Some(&text), false, item_size, ITEM_MAX_WIDTH, 0, None);
        Badge {
            h: label.height.max(item_line) + 2.0 * ITEM_PAD_Y,
            overflow,
            text,
            w: label.width + 2.0 * ITEM_PAD_X,
            label,
        }
    };

    let block = |name: &'static str| {
        let [heading, model_text, practice] = meta(name);
        let title = layout_label(Some(heading), false, size, 4000.0, BOLD, None);
        let mut subtitles = Vec::new();
        if show_descriptions {
            for text in [model_text, practice] {
                if !text.is_empty() {
                    subtitles.push(layout_label(Some(text), false, small, 4000.0, ITALIC, None));
                }
            }
        }
        let items = model.items(name);
        let shown = if name == "confusion" && items.len() > MAX_CONFUSION_ITEMS {
            MAX_CONFUSION_ITEMS
        } else {
            items.len()
        };
        let mut badges: Vec<Badge> = items
            .iter()
            .take(shown)
            .map(|item| badge(item.clone(), false))
            .collect();
        if shown < items.len() {
            let mut text = String::from("+");
            #[expect(clippy::cast_precision_loss, reason = "item counts are small")]
            push_number(&mut text, (items.len() - shown) as f64);
            text.push_str(" more");
            badges.push(badge(text, true));
        }
        let mut w = title.width;
        let mut h = title.height;
        for s in &subtitles {
            w = w.max(s.width);
            h += s.height;
        }
        if !badges.is_empty() {
            h += HEAD_GAP - ITEM_GAP;
        }
        for b in &badges {
            w = w.max(b.w);
            h += ITEM_GAP + b.h;
        }
        Block {
            badges,
            cx: 0.0,
            cy: 0.0,
            h,
            name,
            subtitles,
            title,
            w,
        }
    };

    let mut quadrants: Vec<Block> = QUADRANTS.iter().map(|&(name, ..)| block(name)).collect();
    let mut confusion = block("confusion");
    let rx = 88.0_f64.max(confusion.w * 0.71 + 8.0);
    let ry = 64.0_f64.max(confusion.h * 0.71 + 6.0);

    let mut quad_w = setting(config, "width", 640.0, 100.0, 10000.0) / 2.0;
    let mut quad_h = setting(config, "height", 440.0, 100.0, 10000.0) / 2.0;
    for b in &quadrants {
        quad_w = quad_w.max(b.w + 2.0 * MARGIN);
        quad_h = quad_h.max(b.h + 2.0 * MARGIN);
    }
    // A block sits in the middle of its quadrant, so its inner corner must stay clear of the
    // ellipse.
    for b in &quadrants {
        let dx = (quad_w - b.w) / 2.0;
        let dy = (quad_h - b.h) / 2.0;
        let ex = dx / (rx + CLEARANCE);
        let ey = dy / (ry + CLEARANCE);
        if ex * ex + ey * ey >= 1.0 {
            continue;
        }
        let wider = b.w + 2.0 * (rx + CLEARANCE);
        let taller = b.h + 2.0 * (ry + CLEARANCE);
        if wider * quad_h <= quad_w * taller {
            quad_w = quad_w.max(wider);
        } else {
            quad_h = quad_h.max(taller);
        }
    }

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, BOLD, None);
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let width = 2.0 * quad_w;
    let height = 2.0 * quad_h;
    let x0 = pad;
    let y0 = pad + title_height;
    let cx = x0 + quad_w;
    let cy = y0 + quad_h;
    let right = x0 + width;
    let bottom = y0 + height;

    let fold = wave(
        y0,
        cy,
        cx,
        seed,
        amplitude,
        WaveOptions {
            pin_end: true,
            segments: 4,
            ..FOLD
        },
    );
    let west = wave(
        x0,
        cx,
        cy,
        seed + 100.0,
        amplitude,
        WaveOptions {
            pin_end: true,
            segments: 4,
            ..HORIZON
        },
    );
    let east = wave(
        cx,
        right,
        cy,
        seed + 200.0,
        amplitude,
        WaveOptions {
            pin_start: true,
            segments: 4,
            ..HORIZON
        },
    );
    let drop = cliff(cx, cy, bottom, width * 0.03);
    let at = |out: &mut String, curve: &[f64], end: bool| {
        let k = if end {
            curve.len().saturating_sub(2)
        } else {
            0
        };
        push_pair(
            out,
            curve.get(k).copied().unwrap_or(0.0),
            curve.get(k + 1).copied().unwrap_or(0.0),
        );
    };

    let region = |name: &str| {
        let mut d = String::from("M");
        match name {
            "complex" => {
                push_pair(&mut d, x0, y0);
                d.push('L');
                at(&mut d, &fold, false);
                forward(&mut d, &fold);
                backward(&mut d, &west);
            }
            "complicated" => {
                at(&mut d, &fold, false);
                d.push('L');
                push_pair(&mut d, right, y0);
                d.push('L');
                at(&mut d, &east, true);
                backward(&mut d, &east);
                backward(&mut d, &fold);
            }
            "chaotic" => {
                at(&mut d, &west, false);
                forward(&mut d, &west);
                forward(&mut d, &drop);
                d.push('L');
                push_pair(&mut d, x0, bottom);
            }
            _ => {
                at(&mut d, &east, false);
                forward(&mut d, &east);
                d.push('L');
                push_pair(&mut d, right, bottom);
                d.push('L');
                at(&mut d, &drop, true);
                backward(&mut d, &drop);
            }
        }
        d.push('Z');
        d
    };

    let mut fills = String::new();
    for (name, _, _, color) in QUADRANTS {
        fills.push_str("<path class=\"ondine-cynefin-region\" data-id=\"");
        fills.push_str(name);
        fills.push_str("\" d=\"");
        fills.push_str(&region(name));
        fills.push_str("\" fill=\"");
        fills.push_str(&series_color(color));
        fills.push_str("\" fill-opacity=\"0.14\"/>");
    }
    let mut boundaries = String::from(
        "<g class=\"ondine-cynefin-boundaries\" fill=\"none\" stroke=\"var(--_b)\" stroke-dasharray=\"6 3\">",
    );
    for curve in [&fold, &west, &east] {
        boundaries.push_str("<path d=\"");
        boundaries.push_str(&curve_path(curve));
        boundaries.push_str("\"/>");
    }
    boundaries.push_str("<path class=\"ondine-cynefin-cliff\" d=\"");
    boundaries.push_str(&curve_path(&drop));
    boundaries.push_str(
        "\" stroke=\"var(--_l)\" stroke-width=\"2\" stroke-dasharray=\"none\" stroke-linecap=\"round\"/></g>",
    );
    let oval = ellipse_path(cx, cy, rx, ry);
    let mut ellipse = String::from("<path d=\"");
    ellipse.push_str(&oval);
    ellipse.push_str(
        "\" fill=\"var(--_bg)\"/><path class=\"ondine-cynefin-region\" data-id=\"confusion\" d=\"",
    );
    ellipse.push_str(&oval);
    ellipse.push_str("\" fill=\"");
    ellipse.push_str(&series_color(CONFUSION_COLOR));
    ellipse.push_str("\" fill-opacity=\"0.18\" stroke=\"var(--_b)\" stroke-dasharray=\"4 2\"/>");

    let draw_block = |b: &Block| {
        let mut y = b.cy - b.h / 2.0;
        let mut out = label_svg(
            &b.title,
            b.cx,
            y + b.title.height / 2.0,
            " class=\"ondine-label\" font-weight=\"var(--_hw)\"",
            None,
        );
        y += b.title.height;
        let mut attrs = String::from(" class=\"ondine-cynefin-subtitle\" font-size=\"");
        push_number(&mut attrs, small);
        attrs.push_str("\" font-style=\"italic\" fill=\"var(--_m)\"");
        for s in &b.subtitles {
            out.push_str(&label_svg(s, b.cx, y + s.height / 2.0, &attrs, None));
            y += s.height;
        }
        y += HEAD_GAP - ITEM_GAP;
        for item in &b.badges {
            y += ITEM_GAP;
            out.push_str("<g class=\"ondine-node");
            if item.overflow {
                out.push_str(" ondine-cynefin-overflow\"");
            } else {
                out.push_str("\" data-id=\"");
                out.push_str(&esc_text(&item.text));
                out.push('"');
            }
            out.push_str("><rect x=\"");
            push_num(&mut out, b.cx - item.w / 2.0);
            out.push_str("\" y=\"");
            push_num(&mut out, y);
            out.push_str("\" width=\"");
            push_num(&mut out, item.w);
            out.push_str("\" height=\"");
            push_num(&mut out, item.h);
            out.push_str("\" rx=\"");
            out.push_str(RADIUS);
            out.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"");
            if item.overflow {
                out.push_str(" stroke-dasharray=\"3 2\"");
            }
            out.push_str("/>");
            let mut attrs = String::from(" class=\"ondine-label\" font-size=\"");
            push_number(&mut attrs, item_size);
            attrs.push('"');
            if item.overflow {
                attrs.push_str(" fill=\"var(--_m)\"");
            }
            out.push_str(&label_svg(
                &item.label,
                b.cx,
                y + item.h / 2.0,
                &attrs,
                None,
            ));
            out.push_str("</g>");
            y += item.h;
        }
        let mut group = String::from("<g class=\"ondine-cynefin-domain\" data-id=\"");
        group.push_str(b.name);
        group.push_str("\">");
        group.push_str(&out);
        group.push_str("</g>");
        group
    };

    let mut domains = String::new();
    for (b, &(_, col, row, _)) in quadrants.iter_mut().zip(&QUADRANTS) {
        b.cx = x0 + quad_w * (col + 0.5);
        b.cy = y0 + quad_h * (row + 0.5);
        domains.push_str(&draw_block(b));
    }
    confusion.cx = cx;
    confusion.cy = cy;
    domains.push_str(&draw_block(&confusion));

    let find = |name: &str| {
        if name == "confusion" {
            return Some(&confusion);
        }
        quadrants.iter().find(|b| b.name == name)
    };
    let inside = |b: &Block, x: f64, y: f64| {
        if b.name == "confusion" {
            let ex = (x - cx) / (rx + 4.0);
            let ey = (y - cy) / (ry + 4.0);
            return ex * ex + ey * ey <= 1.0;
        }
        (x - b.cx).abs() <= b.w / 2.0 + 8.0 && (y - b.cy).abs() <= b.h / 2.0 + 8.0
    };

    let mut edges = String::new();
    let mut seen: Vec<(&str, &str, f64)> = Vec::new();
    for transition in &model.transitions {
        let (Some(from), Some(to)) = (find(transition.from), find(transition.to)) else {
            continue;
        };
        if from.name == to.name {
            continue;
        }
        let key = if transition.from < transition.to {
            (transition.from, transition.to)
        } else {
            (transition.to, transition.from)
        };
        let repeat = if let Some(entry) = seen.iter_mut().find(|(a, b, _)| (*a, *b) == key) {
            entry.2 += 1.0;
            entry.2 - 1.0
        } else {
            seen.push((key.0, key.1, 1.0));
            0.0
        };

        let dx = to.cx - from.cx;
        let dy = to.cy - from.cy;
        let len = or_one(hypot(dx, dy));
        let mx = f64::midpoint(from.cx, to.cx);
        let my = f64::midpoint(from.cy, to.cy);
        // Curves bow away from the middle of the diagram, which keeps them off the ellipse.
        let outward = if -dy * (mx - cx) + dx * (my - cy) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let nx = (-dy / len) * outward;
        let ny = (dx / len) * outward;
        // A diagonal runs through the middle, so it bows far enough to pass the ellipse.
        let diagonal =
            from.name != "confusion" && to.name != "confusion" && dx.abs() > 1.0 && dy.abs() > 1.0;
        let bow = if diagonal {
            2.0 * (hypot(rx * nx, ry * ny) + 16.0)
        } else {
            len * 0.15
        } + repeat * 56.0;
        let px = mx + nx * bow;
        let py = my + ny * bow;
        let point = |t: f64| {
            let u = 1.0 - t;
            (
                u * u * from.cx + 2.0 * u * t * px + t * t * to.cx,
                u * u * from.cy + 2.0 * u * t * py + t * t * to.cy,
            )
        };
        let crossing = |b: &Block, mut lo: f64, mut hi: f64, leaving: bool| {
            for _ in 0..16 {
                let mid = f64::midpoint(lo, hi);
                let (x, y) = point(mid);
                if inside(b, x, y) == leaving {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            f64::midpoint(lo, hi)
        };
        let step = |i: u32| f64::from(i) / f64::from(STEPS);
        let mut i = 1;
        while i <= STEPS && {
            let (x, y) = point(step(i));
            inside(from, x, y)
        } {
            i += 1;
        }
        if i > STEPS {
            continue;
        }
        let t0 = crossing(from, step(i - 1), step(i), true);
        while i <= STEPS && {
            let (x, y) = point(step(i));
            !inside(to, x, y)
        } {
            i += 1;
        }
        let t1 = if i > STEPS {
            1.0
        } else {
            crossing(to, step(i - 1), step(i), false)
        };

        let (sx, sy) = point(t0);
        let (mut ex, mut ey) = point(t1);
        let blossom = |a: f64, b: f64, c: f64| {
            (1.0 - t0) * (1.0 - t1) * a + ((1.0 - t0) * t1 + t0 * (1.0 - t1)) * b + t0 * t1 * c
        };
        let qx = blossom(from.cx, px, to.cx);
        let qy = blossom(from.cy, py, to.cy);
        let el = or_one(hypot(ex - qx, ey - qy));
        let ux = (ex - qx) / el;
        let uy = (ey - qy) / el;
        let head = marker("arrow_point", ex, ey, ux, uy, "var(--_l)");
        ex -= ux * marker_trim("arrow_point");
        ey -= uy * marker_trim("arrow_point");

        let mut text = String::new();
        if let Some(label) = &transition.label {
            let label = layout_label(Some(label), false, small, ITEM_MAX_WIDTH, 0, None);
            let (lx, ly) = point(f64::midpoint(t0, t1));
            let away = (nx.abs() * label.width) / 2.0 + (ny.abs() * label.height) / 2.0 + 6.0;
            let half = label.width / 2.0 + 4.0;
            let tx = (lx + nx * away)
                .max(x0 + half)
                .min((right - half).max(x0 + half));
            let mut attrs = String::from(" class=\"ondine-edge-label\" font-size=\"");
            push_number(&mut attrs, small);
            attrs.push('"');
            text = label_svg(&label, tx, ly + ny * away, &attrs, None);
        }
        edges.push_str("<g class=\"ondine-edge\" data-id=\"");
        edges.push_str(transition.from);
        edges.push('-');
        edges.push_str(transition.to);
        edges.push_str("\"><path d=\"M");
        push_pair(&mut edges, sx, sy);
        edges.push('Q');
        push_pair(&mut edges, qx, qy);
        edges.push(' ');
        push_pair(&mut edges, ex, ey);
        edges.push_str("\" fill=\"none\" stroke=\"var(--_l)\"/>");
        edges.push_str(&head);
        edges.push_str(&text);
        edges.push_str("</g>");
    }

    let total_width = (width.max(title.width) + 2.0 * pad).ceil();
    let total_height = (height + title_height + 2.0 * pad).ceil();
    let mut content = label_svg(
        &title,
        x0 + width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g class=\"ondine-cynefin-regions\">");
    content.push_str(&fills);
    content.push_str("</g>");
    content.push_str(&boundaries);
    content.push_str(&ellipse);
    content.push_str(&domains);
    if !edges.is_empty() {
        content.push_str("<g class=\"ondine-edges\">");
        content.push_str(&edges);
        content.push_str("</g>");
    }

    let svg = svg_document(
        "cynefin",
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
