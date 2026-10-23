//! Draws a timeline: periods along an axis with their events, grouped by section, across or
//! running down. Port of Pele's `src/diagrams/timeline/render.ts`.

use super::db::TimelineDb;
use crate::diagrams::Rendered;
use crate::diagrams::common::fit::{fit_label, plain_label};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::edges::{ARROW, marker};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{ROUNDED, series_color};
use crate::text::label::{Label, trim};
use crate::text::measure::BOLD;
use crate::yaml::{Value, push_number};

const PAD_X: f64 = 12.0;
const PAD_Y: f64 = 8.0;
const MIN_TEXT: f64 = 152.0;
const MAX_TEXT: f64 = 216.0;
const EVENT_TEXT_TD: f64 = 264.0;
const GAP: f64 = 20.0;
const AXIS: f64 = 20.0;
const SPINE: f64 = 14.0;
const EVENT_GAP: f64 = 8.0;
const SECTION_GAP: f64 = 8.0;
const TAIL: f64 = 12.0;
const ON_COLOR: &str = " fill=\"var(--_bg)\"";
const LINE: &str = " fill=\"none\" stroke=\"var(--_b)\"";

/// A section, or the periods before the first one, with its periods' indexes.
struct Group {
    periods: Vec<usize>,
    /// The section's index, or `None` for the periods before the first section.
    section: Option<usize>,
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

/// The series color of a section or period index.
fn series(index: usize) -> String {
    series_color(i64::try_from(index).unwrap_or(0))
}

/// Appends a box with the themed corner radius.
fn push_box(out: &mut String, x: f64, y: f64, w: f64, h: f64, attrs: &str) {
    out.push_str("<rect x=\"");
    push_num(out, x);
    out.push_str("\" y=\"");
    push_num(out, y);
    out.push_str("\" width=\"");
    push_num(out, w);
    out.push_str("\" height=\"");
    push_num(out, h);
    out.push('"');
    out.push_str(ROUNDED);
    out.push_str(attrs);
    out.push_str("/>");
}

/// Draws `model` with the `timeline` section of `config`. A `TD` timeline always runs down;
/// another runs down when across does not fit.
pub(crate) fn render(model: &TimelineDb, config: &Value, options: &RenderOptions) -> Rendered {
    if model.direction == "TD" {
        return draw(model, config, options, true);
    }
    turn_to_fit(options, |down| draw(model, config, options, down))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "counts and indexes are far below 2^52"
)]
fn draw(model: &TimelineDb, config: &Value, options: &RenderOptions, vertical: bool) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);
    let multicolor = !matches!(
        property(config, "timeline").and_then(|conf| property(conf, "disableMulticolor")),
        Some(Value::Bool(true))
    );
    let periods = &model.periods;
    let has_sections = !model.sections.is_empty();

    // Mermaid draws nothing for periods that come before the first section; here they lead, in a
    // neutral color.
    let mut groups: Vec<Group> = (0..model.sections.len())
        .map(|section| Group {
            periods: Vec::new(),
            section: Some(section),
        })
        .collect();
    let mut leading = Group {
        periods: Vec::new(),
        section: None,
    };
    for (i, period) in periods.iter().enumerate() {
        let group = usize::try_from(period.section_index)
            .ok()
            .and_then(|at| groups.get_mut(at))
            .unwrap_or(&mut leading);
        group.periods.push(i);
    }
    if !leading.periods.is_empty() {
        groups.insert(0, leading);
    }

    // Columns widen, up to a point, for a word that would not fit; past that the word is cut.
    // Running down, the two columns share the width the host has.
    let room = match options.max_width {
        Some(width) if vertical && width > 0.0 => width - 2.0 * pad,
        _ => f64::INFINITY,
    };
    let taken = AXIS + SPINE * 2.0 + 4.0 * PAD_X;
    let period_text = MIN_TEXT.min(80.0_f64.max(js_round((room - taken) * 0.38)));
    let event_text = EVENT_TEXT_TD.min(110.0_f64.max(room - taken - period_text));
    let mut text_w = period_text;
    let event_text_w = |text_w: f64| if vertical { event_text } else { text_w };
    let layout = |fit: bool, text_w: f64| -> (Vec<Label>, Vec<Vec<Label>>) {
        let label = |raw: &str, size: f64, max: f64| {
            if fit {
                fit_label(Some(raw), size, max, 0)
            } else {
                plain_label(Some(raw), size, max, 0)
            }
        };
        let period_labels = periods
            .iter()
            .map(|period| label(&period.text, size, text_w))
            .collect();
        let event_labels = periods
            .iter()
            .map(|period| {
                period
                    .events
                    .iter()
                    .map(|event| label(event, small, event_text_w(text_w)))
                    .collect()
            })
            .collect();
        (period_labels, event_labels)
    };
    let (mut period_labels, mut event_labels) = layout(false, text_w);
    let mut widest: f64 = period_labels
        .iter()
        .fold(0.0, |w, label| w.max(label.width));
    if !vertical {
        widest = event_labels
            .iter()
            .flatten()
            .fold(widest, |w, label| w.max(label.width));
    }
    let mut overflow = widest > text_w;
    if vertical {
        overflow = overflow
            || event_labels
                .iter()
                .flatten()
                .any(|label| label.width > event_text);
    }
    if overflow {
        let cap = if room == f64::INFINITY {
            MAX_TEXT
        } else {
            period_text
        };
        text_w = widest.ceil().max(period_text).min(cap);
        (period_labels, event_labels) = layout(true, text_w);
    }

    let line_height = js_round(size * 1.5);
    let col_w = text_w + 2.0 * PAD_X;
    let event_w = event_text_w(text_w) + 2.0 * PAD_X;
    let chip_height = |label: &Label| label.height.max(line_height) + 2.0 * PAD_Y;
    let event_height = |label: &Label| label.height.max(js_round(small * 1.5)) + 2.0 * PAD_Y;
    let color_of = |group: &Group, period: usize| match group.section {
        Some(section) => series(section),
        None if has_sections => "var(--_m)".to_owned(),
        None => series(if multicolor { period } else { 0 }),
    };

    let has_events = event_labels.iter().any(|labels| !labels.is_empty());
    let slots: usize = groups.iter().map(|group| group.periods.len().max(1)).sum();

    let pitch = col_w + GAP;
    let drawn = !periods.is_empty();
    let body_w = if vertical {
        col_w
            + if has_events {
                AXIS + SPINE * 2.0 + event_w
            } else if drawn {
                AXIS + TAIL
            } else {
                0.0
            }
    } else if slots > 0 {
        slots as f64 * pitch - GAP + if drawn { TAIL + ARROW } else { 0.0 }
    } else {
        0.0
    };
    let title = fit_label(model.title.as_deref(), size, body_w.max(320.0), BOLD);
    let title_h = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let width = body_w.max(title.width);
    let left = pad + (width - body_w) / 2.0;
    let top = pad + title_h;

    let section_name = |group: &Group| {
        group
            .section
            .and_then(|section| model.sections.get(section))
            .map(String::as_str)
    };
    let section_label =
        |group: &Group, w: f64| fit_label(section_name(group), size, w - 2.0 * PAD_X, BOLD);
    let header =
        |out: &mut String, section: usize, label: &Label, x: f64, y: f64, w: f64, h: f64| {
            push_box(
                out,
                x,
                y,
                w,
                h,
                &[" fill=\"", &series(section), "\""].concat(),
            );
            out.push_str(&label_svg(
                label,
                x + w / 2.0,
                y + h / 2.0,
                &[" font-weight=\"var(--_hw)\"", ON_COLOR].concat(),
                None,
            ));
        };
    let period_svg = |out: &mut String, i: usize, color: &str, x: f64, y: f64, h: f64| {
        push_box(out, x, y, col_w, h, &[" fill=\"", color, "\""].concat());
        if let Some(label) = period_labels.get(i) {
            out.push_str(&label_svg(
                label,
                x + col_w / 2.0,
                y + h / 2.0,
                ON_COLOR,
                None,
            ));
        }
    };
    let mut font_size = String::from(" font-size=\"");
    push_number(&mut font_size, small);
    font_size.push('"');
    let event = |out: &mut String, label: &Label, color: &str, x: f64, y: f64, h: f64| {
        out.push_str("<g class=\"ondine-node ondine-event\">");
        push_box(
            out,
            x,
            y,
            event_w,
            h,
            &[" fill=\"", color, "\" fill-opacity=\"0.16\""].concat(),
        );
        out.push_str(&label_svg(
            label,
            x + event_w / 2.0,
            y + h / 2.0,
            &font_size,
            None,
        ));
        out.push_str("</g>");
    };
    let dot = |out: &mut String, x: f64, y: f64, color: &str| {
        out.push_str("<circle class=\"ondine-dot\" cx=\"");
        push_num(out, x);
        out.push_str("\" cy=\"");
        push_num(out, y);
        out.push_str("\" r=\"4\" fill=\"");
        out.push_str(color);
        out.push_str("\" stroke=\"var(--_bg)\" stroke-width=\"2\"/>");
    };
    let open = |out: &mut String, group: &Group| {
        if let Some(name) = section_name(group) {
            out.push_str("<g class=\"ondine-cluster ondine-section\" data-id=\"");
            out.push_str(&esc_text(trim(name)));
            out.push_str("\">");
        }
    };
    let open_period = |out: &mut String, i: usize| {
        out.push_str("<g class=\"ondine-node ondine-period\" data-id=\"");
        out.push_str(&esc_text(trim(
            periods.get(i).map_or("", |p| p.text.as_str()),
        )));
        out.push_str("\">");
    };
    let no_labels: &[Label] = &[];
    let events_of = |i: usize| event_labels.get(i).map_or(no_labels, Vec::as_slice);

    let mut body = String::new();
    let height;

    if vertical {
        let axis_x = left + col_w + AXIS;
        let spine_x = axis_x + SPINE;
        let events_x = spine_x + SPINE;
        let mut y = top;
        for group in &groups {
            open(&mut body, group);
            if let Some(section) = group.section {
                let label = section_label(group, body_w);
                let h = chip_height(&label);
                header(&mut body, section, &label, left, y, body_w, h);
                y += h + SECTION_GAP + 4.0;
            }
            for &i in &group.periods {
                let color = color_of(group, i);
                let h = period_labels.get(i).map_or(0.0, chip_height);
                let cy = y + h / 2.0;
                let mut d = String::from("M");
                push_num(&mut d, left + col_w);
                d.push(',');
                push_num(&mut d, cy);
                d.push('H');
                push_num(&mut d, axis_x);
                let mut cards = String::new();
                let mut ey = y;
                let mut last = cy;
                for label in events_of(i) {
                    let eh = event_height(label);
                    last = ey + eh / 2.0;
                    d.push('M');
                    push_num(&mut d, spine_x);
                    d.push(',');
                    push_num(&mut d, last);
                    d.push('H');
                    push_num(&mut d, events_x);
                    event(&mut cards, label, &color, events_x, ey, eh);
                    ey += eh + EVENT_GAP;
                }
                if !cards.is_empty() {
                    d.push('M');
                    push_num(&mut d, axis_x);
                    d.push(',');
                    push_num(&mut d, cy);
                    d.push('H');
                    push_num(&mut d, spine_x);
                    d.push('V');
                    push_num(&mut d, last);
                }
                open_period(&mut body, i);
                period_svg(&mut body, i, &color, left, y, h);
                body.push_str("<path class=\"ondine-edge\" d=\"");
                body.push_str(&d);
                body.push('"');
                body.push_str(LINE);
                body.push_str("/>");
                dot(&mut body, axis_x, cy, &color);
                body.push_str(&cards);
                body.push_str("</g>");
                y = (y + h).max(ey - EVENT_GAP) + GAP;
            }
            if group.periods.is_empty() {
                y += GAP - SECTION_GAP - 4.0;
            }
            if group.section.is_some() {
                body.push_str("</g>");
            }
        }
        if !groups.is_empty() {
            y -= GAP;
        }
        if drawn {
            let tip = y + TAIL + ARROW;
            let mut axis = String::from("<path class=\"ondine-axis\" d=\"M");
            push_num(&mut axis, axis_x);
            axis.push(',');
            push_num(&mut axis, top);
            axis.push('V');
            push_num(&mut axis, tip - ARROW + 1.0);
            axis.push_str("\" fill=\"none\" stroke=\"var(--_l)\"/>");
            axis.push_str(&marker("arrow_point", axis_x, tip, 0.0, 1.0, "var(--_l)"));
            axis.push_str(&body);
            body = axis;
            y = tip;
        }
        height = y + pad;
    } else {
        let mut header_h: f64 = 0.0;
        let section_labels: Vec<Option<Label>> = groups
            .iter()
            .map(|group| {
                group.section?;
                let span = group.periods.len().max(1) as f64;
                let label = section_label(group, span * pitch - GAP);
                header_h = header_h.max(chip_height(&label));
                Some(label)
            })
            .collect();
        let chip_h = period_labels
            .iter()
            .fold(0.0, |h: f64, label| h.max(chip_height(label)));
        let periods_y = top
            + if has_sections {
                header_h + SECTION_GAP
            } else {
                0.0
            };
        let axis_y = periods_y + chip_h + if chip_h > 0.0 { AXIS } else { 0.0 };
        let events_y = axis_y + AXIS;
        let mut bottom = if drawn { axis_y + TAIL } else { top + header_h };
        let mut slot = 0;
        for (group, label) in groups.iter().zip(&section_labels) {
            let span = group.periods.len().max(1);
            let gx = left + slot as f64 * pitch;
            open(&mut body, group);
            if let (Some(label), Some(section)) = (label, group.section) {
                header(
                    &mut body,
                    section,
                    label,
                    gx,
                    top,
                    span as f64 * pitch - GAP,
                    header_h,
                );
            }
            for (k, &i) in group.periods.iter().enumerate() {
                let color = color_of(group, i);
                let x = gx + k as f64 * pitch;
                let cx = x + col_w / 2.0;
                let events = events_of(i);
                let mut d = String::from("M");
                push_num(&mut d, cx);
                d.push(',');
                push_num(&mut d, periods_y + chip_h);
                d.push('V');
                push_num(&mut d, if events.is_empty() { axis_y } else { events_y });
                let mut cards = String::new();
                let mut ey = events_y;
                for label in events {
                    let eh = event_height(label);
                    if ey > events_y {
                        d.push('M');
                        push_num(&mut d, cx);
                        d.push(',');
                        push_num(&mut d, ey - EVENT_GAP);
                        d.push('V');
                        push_num(&mut d, ey);
                    }
                    event(&mut cards, label, &color, x, ey, eh);
                    ey += eh + EVENT_GAP;
                }
                bottom = bottom.max(ey - EVENT_GAP);
                open_period(&mut body, i);
                period_svg(&mut body, i, &color, x, periods_y, chip_h);
                body.push_str("<path class=\"ondine-edge\" d=\"");
                body.push_str(&d);
                body.push('"');
                body.push_str(LINE);
                body.push_str("/>");
                dot(&mut body, cx, axis_y, &color);
                body.push_str(&cards);
                body.push_str("</g>");
            }
            if group.section.is_some() {
                body.push_str("</g>");
            }
            slot += span;
        }
        if drawn {
            let tip = left + body_w;
            let mut axis = String::from("<path class=\"ondine-axis\" d=\"M");
            push_num(&mut axis, left);
            axis.push(',');
            push_num(&mut axis, axis_y);
            axis.push('H');
            push_num(&mut axis, tip - ARROW + 1.0);
            axis.push_str("\" fill=\"none\" stroke=\"var(--_l)\"/>");
            axis.push_str(&marker("arrow_point", tip, axis_y, 1.0, 0.0, "var(--_l)"));
            axis.push_str(&body);
            body = axis;
        }
        height = bottom + pad;
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = height.max(pad * 2.0).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&body);
    Rendered {
        height: total_height,
        links: Vec::new(),
        svg: svg_document(
            "timeline",
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
