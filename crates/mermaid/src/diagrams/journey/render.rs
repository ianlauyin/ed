//! Draws a user journey: task cards over a chart of their scores, with an actor legend, or a
//! column of steps when that is narrower. Port of Pele's `src/diagrams/journey/render.ts`.

use super::db::{JourneyDb, journey_actors};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit::{fit_label, plain_label};
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{ROUNDED, series_color};
use crate::text::label::{Label, trim};
use crate::text::measure::BOLD;
use crate::yaml::push_number;

const PAD_X: f64 = 10.0;
const PAD_Y: f64 = 8.0;
const MIN_TEXT: f64 = 124.0;
const MAX_TEXT: f64 = 200.0;
const GAP: f64 = 12.0;
const BAND_PAD: f64 = 6.0;
const GUTTER: f64 = 24.0;
const LEVEL: f64 = 26.0;
const FACE: f64 = 11.0;
const DOT: f64 = 5.0;
const DOT_STEP: f64 = 13.0;
const LEGEND_GAP: f64 = 20.0;

const EYES: &str =
    "<circle cx=\"-3.6\" cy=\"-2.8\" r=\"1.3\"/><circle cx=\"3.6\" cy=\"-2.8\" r=\"1.3\"/>";

/// The mood a score shows: above 3 is happy, below is sad, and the rest (NaN too) neutral.
fn mood(score: f64) -> (&'static str, &'static str) {
    if score > 3.0 {
        ("happy", "M-5,2A6,6 0 0 0 5,2")
    } else if score < 3.0 {
        ("sad", "M-5,6.5A6,6 0 0 1 5,6.5")
    } else {
        ("neutral", "M-4.5,4.5H4.5")
    }
}

fn push_face(out: &mut String, x: f64, y: f64, score: f64) {
    let (name, mouth) = mood(score);
    out.push_str("<g class=\"ondine-face ondine-face-");
    out.push_str(name);
    out.push_str("\" transform=\"translate(");
    push_num(out, x);
    out.push(',');
    push_num(out, y);
    out.push_str(")\"><circle r=\"");
    push_number(out, FACE);
    out.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_l)\"/><g fill=\"var(--_l)\">");
    out.push_str(EYES);
    out.push_str("</g><path d=\"");
    out.push_str(mouth);
    out.push_str("\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\"/></g>");
}

/// The series color of an actor's index.
fn series(index: usize) -> String {
    series_color(i64::try_from(index).unwrap_or(0))
}

/// The ` font-size` attribute for `size`.
fn font_size(size: f64) -> String {
    let mut out = String::from(" font-size=\"");
    push_number(&mut out, size);
    out.push('"');
    out
}

/// Draws `model`, running down when across does not fit.
pub(crate) fn render(model: &JourneyDb, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |down| {
        if down {
            vertical(model, options)
        } else {
            draw(model, options)
        }
    })
}

/// A legend entry.
struct Entry<'a> {
    h: f64,
    label: Label,
    name: &'a str,
    x: f64,
    y: f64,
}

/// A run of tasks that share a section.
struct Run {
    count: usize,
    label: Option<Label>,
    start: usize,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "counts and indexes are far below 2^52"
)]
fn draw(model: &JourneyDb, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let tiny = js_round(size * 0.75);
    let pad = options.padding.unwrap_or(8.0);
    let tasks = &model.tasks;
    let n = tasks.len();

    // `Task: 5:` names an actor with no name; Mermaid gives it a dot and an empty legend row.
    let names: Vec<&str> = journey_actors(tasks)
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    let actor = |name: &str| names.iter().position(|own| *own == name).unwrap_or(0);

    // Cards widen, up to a point, for a word that would not fit; past that the word is cut.
    let mut text_w = MIN_TEXT;
    let mut labels: Vec<Label> = tasks
        .iter()
        .map(|task| plain_label(Some(&task.task), small, text_w, 0))
        .collect();
    let widest = labels.iter().fold(0.0, |w: f64, label| w.max(label.width));
    if widest > text_w {
        text_w = widest.ceil().min(MAX_TEXT);
        labels = tasks
            .iter()
            .map(|task| fit_label(Some(&task.task), small, text_w, 0))
            .collect();
    }

    let card_w = text_w + 2.0 * PAD_X;
    let pitch = card_w + GAP;
    let plot_w = if n > 0 { n as f64 * pitch - GAP } else { 0.0 };

    let line_h = js_round(small * 1.5);
    let limit = plot_w.max(240.0);
    let mut legend: Vec<Entry<'_>> = Vec::new();
    let mut legend_w: f64 = 0.0;
    let mut legend_h: f64 = 0.0;
    let mut lx = 0.0;
    for &name in &names {
        let label = fit_label(Some(name), small, limit - 2.0 * DOT - 6.0, 0);
        let w = 2.0 * DOT + 6.0 + label.width;
        if lx > 0.0 && lx + w > limit {
            lx = 0.0;
            legend_h += 4.0;
        }
        let y = if lx > 0.0 {
            legend.last().map_or(0.0, |entry| entry.y)
        } else {
            legend_h
        };
        let h = label.height.max(line_h);
        legend.push(Entry {
            h,
            label,
            name,
            x: lx,
            y,
        });
        lx += w;
        legend_w = legend_w.max(lx);
        legend_h = legend_h.max(y + h);
        lx += LEGEND_GAP;
    }

    let body_w = if n > 0 { GUTTER + plot_w } else { 0.0 };
    let title = fit_label(model.title.as_deref(), size, body_w.max(320.0), BOLD);
    let title_h = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let width = body_w
        .max(title.width)
        .max(if n > 0 { GUTTER + legend_w } else { 0.0 });
    let left = pad + (width - body_w) / 2.0 + GUTTER;
    let top = pad + title_h;

    // A band covers a run of tasks that share a section, as in Mermaid; a section with no tasks
    // draws nothing.
    let mut runs: Vec<Run> = Vec::new();
    let mut band_h: f64 = 0.0;
    let mut i = 0;
    while let Some(first) = tasks.get(i) {
        let mut end = i + 1;
        while tasks
            .get(end)
            .is_some_and(|task| task.section == first.section)
        {
            end += 1;
        }
        let mut label = None;
        if !first.section.is_empty() {
            let own = fit_label(
                Some(&first.section),
                small,
                (end - i) as f64 * pitch - GAP - 2.0 * PAD_X,
                BOLD,
            );
            band_h = band_h.max(own.height.max(line_h) + 2.0 * BAND_PAD);
            label = Some(own);
        }
        runs.push(Run {
            count: end - i,
            label,
            start: i,
        });
        i = end;
    }

    let text_h = labels.iter().fold(line_h, |h, label| h.max(label.height));
    let dots_h = if names.is_empty() {
        0.0
    } else {
        2.0 * DOT + 6.0
    };
    let card_h = text_h + dots_h + 2.0 * PAD_Y;
    let cards_y = top + if band_h > 0.0 { band_h + BAND_PAD } else { 0.0 };
    let chart_top = cards_y + card_h + FACE + 16.0;
    let chart_bottom = chart_top + 4.0 * LEVEL;
    let right = left + plot_w;
    let small_size = font_size(small);

    let mut body = String::new();
    let mut bottom = top;
    if n > 0 {
        let mut grid = String::new();
        let mut scale = String::new();
        for score in (1..=5).rev() {
            let y = chart_top + f64::from(5 - score) * LEVEL;
            grid.push('M');
            push_num(&mut grid, left - 6.0);
            grid.push(',');
            push_num(&mut grid, y);
            grid.push('H');
            push_num(&mut grid, right);
            scale.push_str("<text x=\"");
            push_num(&mut scale, left - 12.0);
            scale.push_str("\" y=\"");
            push_num(&mut scale, y + tiny * 0.35);
            scale.push_str("\">");
            push_number(&mut scale, f64::from(score));
            scale.push_str("</text>");
        }
        body.push_str("<path class=\"ondine-grid\" d=\"");
        body.push_str(&grid);
        body.push_str("\" fill=\"none\" stroke=\"var(--_a)\"/><g class=\"ondine-scale\"");
        body.push_str(&font_size(tiny));
        body.push_str(" text-anchor=\"end\" fill=\"var(--_m)\">");
        body.push_str(&scale);
        body.push_str("</g>");

        let mut line = String::new();
        let mut stems = String::new();
        let mut faces = String::new();
        let mut cards: Vec<String> = Vec::with_capacity(n);
        for (i, task) in tasks.iter().enumerate() {
            let x = left + i as f64 * pitch;
            let cx = x + card_w / 2.0;
            let score = task.score;
            let level = if score.is_nan() {
                3.0
            } else {
                score.clamp(1.0, 5.0)
            };
            let y = chart_top + (5.0 - level) * LEVEL;
            line.push(if i == 0 { 'M' } else { 'L' });
            push_num(&mut line, cx);
            line.push(',');
            push_num(&mut line, y);
            stems.push('M');
            push_num(&mut stems, cx);
            stems.push(',');
            push_num(&mut stems, cards_y + card_h);
            stems.push('V');
            push_num(&mut stems, y - FACE);
            push_face(&mut faces, cx, y, score);

            let mut card = String::from("<g class=\"ondine-node ondine-task\" data-id=\"");
            card.push_str(&esc_text(trim(&task.task)));
            card.push_str("\"><rect x=\"");
            push_num(&mut card, x);
            card.push_str("\" y=\"");
            push_num(&mut card, cards_y);
            card.push_str("\" width=\"");
            push_num(&mut card, card_w);
            card.push_str("\" height=\"");
            push_num(&mut card, card_h);
            card.push('"');
            card.push_str(ROUNDED);
            card.push_str(" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>");
            if let Some(label) = labels.get(i) {
                card.push_str(&label_svg(
                    label,
                    cx,
                    cards_y + PAD_Y + text_h / 2.0,
                    &small_size,
                    None,
                ));
            }
            let people: Vec<&String> = task.people.iter().filter(|name| !name.is_empty()).collect();
            let spread = people.len() as f64 - 1.0;
            let step = DOT_STEP.min((card_w - 2.0 * PAD_X - 2.0 * DOT) / spread.max(1.0));
            for (k, name) in people.iter().enumerate() {
                card.push_str("<circle class=\"ondine-actor\" cx=\"");
                push_num(&mut card, cx + (k as f64 - spread / 2.0) * step);
                card.push_str("\" cy=\"");
                push_num(&mut card, cards_y + card_h - PAD_Y - DOT);
                card.push_str("\" r=\"");
                push_number(&mut card, DOT);
                card.push_str("\" fill=\"");
                card.push_str(&series(actor(name)));
                card.push_str("\" stroke=\"var(--_s)\"><title>");
                card.push_str(&esc_text(name));
                card.push_str("</title></circle>");
            }
            card.push_str("</g>");
            cards.push(card);
        }
        body.push_str("<path class=\"ondine-stems\" d=\"");
        body.push_str(&stems);
        body.push_str("\" fill=\"none\" stroke=\"var(--_b)\" stroke-dasharray=\"2 3\"/>");
        if n > 1 {
            body.push_str("<path class=\"ondine-edge\" d=\"");
            body.push_str(&line);
            body.push_str("\" fill=\"none\" stroke=\"var(--_l)\" stroke-linejoin=\"round\"/>");
        }
        body.push_str(&faces);

        for run in &runs {
            let members = cards
                .get(run.start..run.start + run.count)
                .unwrap_or(&[])
                .concat();
            let Some(label) = &run.label else {
                body.push_str(&members);
                continue;
            };
            let x = left + run.start as f64 * pitch;
            let w = run.count as f64 * pitch - GAP;
            body.push_str("<g class=\"ondine-cluster ondine-section\" data-id=\"");
            body.push_str(&esc_text(trim(
                tasks
                    .get(run.start)
                    .map_or("", |task| task.section.as_str()),
            )));
            body.push_str("\"><rect x=\"");
            push_num(&mut body, x);
            body.push_str("\" y=\"");
            push_num(&mut body, top);
            body.push_str("\" width=\"");
            push_num(&mut body, w);
            body.push_str("\" height=\"");
            push_num(&mut body, band_h);
            body.push('"');
            body.push_str(ROUNDED);
            body.push_str(" fill=\"var(--_a)\"/>");
            body.push_str(&label_svg(
                label,
                x + w / 2.0,
                top + band_h / 2.0,
                &[small_size.as_str(), " font-weight=\"var(--_hw)\""].concat(),
                None,
            ));
            body.push_str(&members);
            body.push_str("</g>");
        }
        bottom = chart_bottom + FACE;

        if !legend.is_empty() {
            let x0 = left + (plot_w - legend_w) / 2.0;
            let y0 = bottom + 16.0;
            let mut items = String::new();
            for item in &legend {
                let cy = y0 + item.y + item.h / 2.0;
                items.push_str("<g class=\"ondine-legend-item\" data-id=\"");
                items.push_str(&esc_text(item.name));
                items.push_str("\"><circle cx=\"");
                push_num(&mut items, x0 + item.x + DOT);
                items.push_str("\" cy=\"");
                push_num(&mut items, cy);
                items.push_str("\" r=\"");
                push_number(&mut items, DOT);
                items.push_str("\" fill=\"");
                items.push_str(&series(actor(item.name)));
                items.push_str("\"/>");
                items.push_str(&label_svg(
                    &item.label,
                    x0 + item.x + 2.0 * DOT + 6.0 + item.label.width / 2.0,
                    cy,
                    "",
                    None,
                ));
                items.push_str("</g>");
            }
            body.push_str("<g class=\"ondine-legend\"");
            body.push_str(&small_size);
            body.push('>');
            body.push_str(&items);
            body.push_str("</g>");
            bottom = y0 + legend_h;
        }
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom.max(pad) + pad).ceil();
    let mut content = label_svg(
        &title,
        pad + width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&body);
    document(model, options, size, total_width, total_height, &content)
}

/// Wraps `content` in the root document.
fn document(
    model: &JourneyDb,
    options: &RenderOptions,
    size: f64,
    width: f64,
    height: f64,
    content: &str,
) -> Rendered {
    Rendered {
        height,
        links: Vec::new(),
        svg: svg_document(
            "journey",
            width,
            height,
            size,
            options,
            Accessible {
                acc_descr: model.acc_descr.as_deref(),
                acc_title: model.acc_title.as_deref(),
            },
            content,
        ),
        width,
    }
}

/// Draws the journey as a column of steps.
#[expect(clippy::cast_precision_loss, reason = "indexes are far below 2^52")]
fn vertical(model: &JourneyDb, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let pad = options.padding.unwrap_or(8.0);
    let room = 140.0_f64.max(options.max_width.unwrap_or(320.0) - 2.0 * pad);
    let title = fit_label(model.title.as_deref(), size, room, BOLD);
    let names: Vec<&str> = journey_actors(&model.tasks)
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    let mut y = pad
        + if title.height == 0.0 {
            0.0
        } else {
            title.height + 16.0
        };
    let mut section: Option<&str> = None;
    let mut body = String::new();
    let mut width = title.width;
    let small_size = font_size(small);
    let count = model.tasks.len();
    for (index, task) in model.tasks.iter().enumerate() {
        if section != Some(task.section.as_str()) {
            section = Some(&task.section);
            let head = fit_label(Some(&task.section), size, room - 16.0, BOLD);
            if head.height != 0.0 {
                body.push_str(&label_svg(
                    &head,
                    pad + 8.0 + head.width / 2.0,
                    y + head.height / 2.0,
                    " class=\"ondine-cluster-label\" font-weight=\"var(--_hw)\"",
                    None,
                ));
                width = width.max(head.width + 16.0);
                y += head.height + 8.0;
            }
        }
        let label = fit_label(Some(&task.task), small, room - 72.0, 0);
        let people: Vec<(&str, Label)> = task
            .people
            .iter()
            .filter(|name| !name.is_empty())
            .map(|name| (name.as_str(), fit_label(Some(name), small, room - 44.0, 0)))
            .collect();
        let text_h = label.height.max(42.0);
        let mut h = 2.0 * PAD_Y + text_h;
        for (_, person) in &people {
            h += person.height.max(16.0) + 4.0;
        }
        let w = room.max(label.width + 72.0);
        body.push_str("<g class=\"ondine-node ondine-journey-step\" data-id=\"");
        push_number(&mut body, index as f64);
        body.push_str("\"><rect x=\"");
        push_num(&mut body, pad);
        body.push_str("\" y=\"");
        push_num(&mut body, y);
        body.push_str("\" width=\"");
        push_num(&mut body, w);
        body.push_str("\" height=\"");
        push_num(&mut body, h);
        body.push('"');
        body.push_str(ROUNDED);
        body.push_str(" fill=\"var(--_s)\" stroke=\"var(--_b)\"/>");
        body.push_str(&label_svg(
            &label,
            pad + PAD_X + label.width / 2.0,
            y + PAD_Y + label.height / 2.0,
            " class=\"ondine-label\"",
            None,
        ));
        let cx = pad + w - 28.0;
        push_face(&mut body, cx, y + PAD_Y + FACE, task.score);
        if !task.score.is_nan() {
            body.push_str("<text class=\"ondine-journey-score\" x=\"");
            push_num(&mut body, cx);
            body.push_str("\" y=\"");
            push_num(&mut body, y + PAD_Y + 2.0 * FACE + small);
            body.push('"');
            body.push_str(&small_size);
            body.push_str(" text-anchor=\"middle\">");
            push_num(&mut body, task.score.clamp(1.0, 5.0));
            body.push_str("</text>");
        }
        let mut py = y + PAD_Y + text_h + 4.0;
        for (name, person) in &people {
            let ph = person.height.max(16.0);
            body.push_str("<circle class=\"ondine-actor\" cx=\"");
            push_num(&mut body, pad + 16.0);
            body.push_str("\" cy=\"");
            push_num(&mut body, py + ph / 2.0);
            body.push_str("\" r=\"");
            push_number(&mut body, DOT);
            body.push_str("\" fill=\"");
            body.push_str(&series(
                names.iter().position(|own| own == name).unwrap_or(0),
            ));
            body.push_str("\"/>");
            body.push_str(&label_svg(
                person,
                pad + 28.0 + person.width / 2.0,
                py + ph / 2.0,
                &[" class=\"ondine-journey-actor\"", small_size.as_str()].concat(),
                None,
            ));
            py += ph + 4.0;
        }
        body.push_str("</g>");
        y += h;
        if index + 1 < count {
            body.push_str("<path class=\"ondine-edge\" d=\"M");
            push_num(&mut body, pad + w / 2.0);
            body.push(',');
            push_num(&mut body, y);
            body.push_str("v16\" fill=\"none\" stroke=\"var(--_l)\"/>");
        }
        y += 16.0;
        width = width.max(w);
    }
    let total_width = (width + 2.0 * pad).ceil();
    let tail = if count > 0 { 16.0 } else { 0.0 };
    let height = ((pad + title.height).max(y - tail) + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&body);
    document(model, options, size, total_width, height, &content)
}
