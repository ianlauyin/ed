//! Draws an event model: frames as boxes in swimlanes with time running across, or as cards
//! running down when that fits the host better. Port of Pele's
//! `src/diagrams/eventmodeling/render.ts`.

use super::model::{EmBox, EntityKind, EventModelingModel};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::diagrams::common::literal_label::{break_label, left_label_svg, literal_label};
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num};
use crate::svg::edges::{marker, marker_trim};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{FONT_MONO, RADIUS, series_color};
use crate::text::entities::decode;
use crate::text::label::{Label, layout_label};
use crate::text::measure::{self, BOLD, MONO};
use crate::yaml::push_number;

/// The series slot of each kind of entity. A UI has none and is drawn plain.
fn series(kind: EntityKind) -> Option<i64> {
    match kind {
        EntityKind::Command => Some(0),
        EntityKind::Event => Some(1),
        EntityKind::Readmodel => Some(2),
        EntityKind::Processor => Some(6),
        EntityKind::Ui => None,
    }
}

/// Space inside a box beside its text.
const BOX_PAD_X: f64 = 12.0;
/// Space inside a box above and below its text.
const BOX_PAD_Y: f64 = 10.0;
/// The narrowest box.
const BOX_MIN_WIDTH: f64 = 96.0;
/// Space between boxes in one lane.
const BOX_GAP: f64 = 16.0;
/// How far a box in a new lane may start before the end of the box before it.
const STAGGER: f64 = 80.0;
/// Space inside a lane around its boxes.
const LANE_PAD: f64 = 12.0;
/// Space between lanes.
const LANE_GAP: f64 = 8.0;
/// The widest line of a box's data, in characters.
const DATA_COLUMNS: usize = 48;
/// The widest line of a note or specification, in characters.
const NOTE_COLUMNS: usize = 80;
/// Space between cards.
const CARD_GAP: f64 = 8.0;
/// Space between rails beside the cards when running down.
const RAIL_GAP: f64 = 8.0;
/// The narrowest card when running down.
const CARD_MIN: f64 = 120.0;
/// Room a lane heading leaves for an arrow passing beside it.
const ARROW_ROOM: f64 = 20.0;

/// Splits text into lines of at most `columns` UTF-16 units, keeping its own line breaks.
fn hard_wrap(text: &str, columns: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in decode(text).split('\n') {
        let units = line.encode_utf16().count();
        if units <= columns {
            out.push(line.to_owned());
            continue;
        }

        // ponytail: a character split by a column boundary goes whole to the earlier line,
        // where Pele splits its UTF-16 pair. Rows and widths match; the text differs only there.
        let rows = out.len() + units.div_ceil(columns);
        let mut piece = String::new();
        let mut at = 0;
        let mut next = columns;
        for c in line.chars() {
            if at >= next {
                out.push(std::mem::take(&mut piece));
                next += columns;
            }
            piece.push(c);
            at += c.len_utf16();
        }
        out.push(piece);
        out.resize(rows, String::new());
    }
    out
}

/// One `<text>` with a `<tspan>` per non-empty row, `step` apart from `y` down.
fn lines(rows: &[String], x: f64, y: f64, step: f64, attrs: &str) -> String {
    let mut body = String::new();
    for (i, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        #[expect(clippy::cast_precision_loss, reason = "row counts stay far below 2^52")]
        let line = i as f64;
        body.push_str("<tspan x=\"");
        push_num(&mut body, x);
        body.push_str("\" y=\"");
        push_num(&mut body, y + line * step);
        body.push_str("\">");
        body.push_str(&esc(row));
        body.push_str("</tspan>");
    }
    if body.is_empty() {
        return body;
    }
    let mut out = String::from("<text");
    out.push_str(attrs);
    out.push_str(" xml:space=\"preserve\">");
    out.push_str(&body);
    out.push_str("</text>");
    out
}

/// The text of each line of `label`.
fn label_rows(label: &Label) -> Vec<String> {
    label
        .lines
        .iter()
        .map(|spans| spans.iter().map(|span| span.text.as_str()).collect())
        .collect()
}

/// The widest of `rows` at `size`.
fn widest(rows: &[String], size: f64, style: u8) -> f64 {
    rows.iter().fold(0.0, |width: f64, row| {
        width.max(measure::width(row, size, style))
    })
}

/// ` font-size="{size}"`, with the size as JavaScript writes a number.
fn font_size(size: f64) -> String {
    let mut out = String::from(" font-size=\"");
    push_number(&mut out, size);
    out.push('"');
    out
}

/// The heading of a note or specification card.
fn card_heading(model: &EventModelingModel, heading: &str, frame: &str) -> String {
    let mut text = String::from(heading);
    text.push_str(" · ");
    text.push_str(frame);
    if let Some(found) = model.boxes.iter().rev().find(|item| item.id == frame) {
        text.push(' ');
        text.push_str(&found.name);
    }
    text
}

/// The rows of a specification card, before wrapping.
fn specification_rows(given: &[String], when: &[String], then: &[String]) -> Vec<String> {
    let row = |head: &str, names: &[String]| {
        let mut text = String::from(head);
        text.push_str(&names.join(", "));
        text
    };
    let mut rows = vec![row("Given  ", given)];
    if !when.is_empty() {
        rows.push(row("When  ", when));
    }
    rows.push(row("Then  ", then));
    rows
}

/// The `class` and `data-id` of a box's group, and its opening tag.
fn push_box_open(out: &mut String, item: &EmBox) {
    out.push_str("<g class=\"ondine-node ondine-em-");
    out.push_str(item.kind.as_str());
    if item.reset {
        out.push_str(" ondine-em-reset");
    }
    out.push_str("\" data-id=\"");
    out.push_str(&esc_text(&item.id));
    out.push_str("\">");
}

/// A box's frame attributes: position, size, and corner radius.
fn frame_attrs(x: f64, y: f64, w: f64, h: f64) -> String {
    let mut out = String::from("x=\"");
    push_num(&mut out, x);
    out.push_str("\" y=\"");
    push_num(&mut out, y);
    out.push_str("\" width=\"");
    push_num(&mut out, w);
    out.push_str("\" height=\"");
    push_num(&mut out, h);
    out.push_str("\" rx=\"");
    out.push_str(RADIUS);
    out.push('"');
    out
}

/// A rectangle in a series color, over the box's background.
fn push_series_rect(out: &mut String, frame: &str, slot: i64) {
    let color = series_color(slot);
    out.push_str("<rect ");
    out.push_str(frame);
    out.push_str(" fill=\"");
    out.push_str(&color);
    out.push_str("\" fill-opacity=\"0.22\" stroke=\"");
    out.push_str(&color);
    out.push_str("\"/>");
}

/// An edge's group: its path and arrowhead.
fn push_edge(
    out: &mut String,
    model: &EventModelingModel,
    relation: (usize, usize),
    d: &str,
    head: &str,
) {
    let id = |index: usize| model.boxes.get(index).map_or("", |item| item.id.as_str());
    out.push_str("<g class=\"ondine-edge\" data-id=\"");
    out.push_str(&esc_text(id(relation.0)));
    out.push('-');
    out.push_str(&esc_text(id(relation.1)));
    out.push_str("\"><path d=\"");
    out.push_str(d);
    out.push_str("\" fill=\"none\" stroke=\"var(--_l)\"/>");
    out.push_str(head);
    out.push_str("</g>");
}

/// Appends `M{x},{y}`.
fn push_move(out: &mut String, x: f64, y: f64) {
    out.push('M');
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Draws `model`, running down when across does not fit the host.
pub(crate) fn render(model: &EventModelingModel, options: &RenderOptions) -> Rendered {
    turn_to_fit(options, |down| {
        if down {
            vertical(model, options)
        } else {
            horizontal(model, options)
        }
    })
}

/// The document around drawn content.
fn document(
    model: &EventModelingModel,
    options: &RenderOptions,
    width: f64,
    height: f64,
    size: f64,
    content: &str,
) -> Rendered {
    Rendered {
        height,
        links: Vec::new(),
        svg: svg_document(
            "eventmodeling",
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

/// A box placed in its lane.
struct Placed {
    /// Wrapped data lines.
    data: Vec<String>,
    /// Height.
    h: f64,
    /// The name, laid out.
    name: Label,
    /// Width.
    w: f64,
    /// Left edge.
    x: f64,
    /// Top edge.
    y: f64,
}

/// A lane's band across the drawing.
struct Band {
    /// Height.
    h: f64,
    /// The heading, laid out.
    label: Label,
    /// Where the last box in the lane ends.
    right: f64,
    /// Top edge.
    y: f64,
}

/// Draws `model` with time running across and one band per lane.
fn horizontal(model: &EventModelingModel, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let name_size = js_round(size * 0.875);
    let small = js_round(size * 0.8125);
    let mono = js_round(size * 0.75);
    let mono_step = js_round(mono * 1.4);
    let small_step = js_round(small * 1.5);
    let pad = options.padding.unwrap_or(8.0);

    let mut bands: Vec<Band> = Vec::new();
    let mut label_column: f64 = 0.0;
    for lane in &model.lanes {
        let label = layout_label(Some(&lane.label), false, small, 4000.0, BOLD, None);
        label_column = label_column.max(label.width);
        bands.push(Band {
            h: 0.0,
            label,
            right: f64::NEG_INFINITY,
            y: 0.0,
        });
    }
    label_column += 2.0 * LANE_PAD;

    // Time runs left to right. A frame in a new lane starts under the end of the one before it,
    // so the arrow between them can drop straight down; a frame in the same lane follows on.
    let mut placed: Vec<Placed> = Vec::new();
    let mut previous: Option<usize> = None;
    for item in &model.boxes {
        let name = layout_label(Some(&item.name), false, name_size, 4000.0, BOLD, None);
        let data = item
            .data
            .as_deref()
            .filter(|data| !data.is_empty())
            .map_or_else(Vec::new, |data| hard_wrap(data, DATA_COLUMNS));
        let w = BOX_MIN_WIDTH.max(name.width.max(widest(&data, mono, MONO)) + 2.0 * BOX_PAD_X);
        #[expect(
            clippy::cast_precision_loss,
            reason = "line counts stay far below 2^52"
        )]
        let h = 2.0 * BOX_PAD_Y
            + name.height
            + if data.is_empty() {
                0.0
            } else {
                6.0 + data.len() as f64 * mono_step
            };
        let right = bands
            .get(item.lane)
            .map_or(f64::NEG_INFINITY, |band| band.right);
        let x = match previous.and_then(|i| placed.get(i).zip(model.boxes.get(i))) {
            None => 0.0,
            Some((before, before_box)) => {
                let stagger = before.x + (before.w / 2.0).max(before.w - STAGGER);
                if before_box.lane == item.lane {
                    right + BOX_GAP
                } else {
                    stagger.max(right + BOX_GAP)
                }
            }
        };
        if let Some(band) = bands.get_mut(item.lane) {
            band.right = x + w;
            band.h = band.h.max(h);
        }
        previous = Some(placed.len());
        placed.push(Placed {
            data,
            h,
            name,
            w,
            x,
            y: 0.0,
        });
    }

    let content_right = placed
        .iter()
        .fold(0.0, |right: f64, item| right.max(item.x + item.w));
    let lane_width = label_column + content_right + LANE_PAD;

    let title = layout_label(model.title.as_deref(), false, size, 4000.0, BOLD, None);
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let left = pad;
    let top = pad + title_height;

    let mut y = top;
    let mut lanes = String::new();
    let mut attrs = String::from(" class=\"ondine-cluster-label\"");
    attrs.push_str(&font_size(small));
    attrs.push_str(" font-weight=\"var(--_hw)\" fill=\"var(--_m)\"");
    for (band, lane) in bands.iter_mut().zip(&model.lanes) {
        band.y = y;
        band.h += 2.0 * LANE_PAD;
        lanes.push_str("<g class=\"ondine-cluster ondine-em-lane\" data-id=\"");
        lanes.push_str(&esc_text(&lane.label));
        lanes.push_str("\"><rect x=\"");
        push_num(&mut lanes, left);
        lanes.push_str("\" y=\"");
        push_num(&mut lanes, y);
        lanes.push_str("\" width=\"");
        push_num(&mut lanes, lane_width);
        lanes.push_str("\" height=\"");
        push_num(&mut lanes, band.h);
        lanes.push_str("\" rx=\"");
        lanes.push_str(RADIUS);
        lanes.push_str("\" fill=\"var(--_s)\"/>");
        lanes.push_str(&label_svg(
            &band.label,
            left + LANE_PAD + band.label.width / 2.0,
            y + band.h / 2.0,
            &attrs,
            None,
        ));
        lanes.push_str("</g>");
        y += band.h + LANE_GAP;
    }
    let mut bottom = if bands.is_empty() { top } else { y - LANE_GAP };

    let mut nodes = String::new();
    let mut name_attrs = String::from(" class=\"ondine-label\"");
    name_attrs.push_str(&font_size(name_size));
    name_attrs.push_str(" font-weight=\"var(--_hw)\"");
    let mut data_attrs = String::from(" class=\"ondine-em-data\" font-family=\"");
    data_attrs.push_str(FONT_MONO);
    data_attrs.push('"');
    data_attrs.push_str(&font_size(mono));
    for (item, info) in placed.iter_mut().zip(&model.boxes) {
        let Some(band) = bands.get(info.lane) else {
            continue;
        };
        item.x += left + label_column;
        item.y = band.y + (band.h - item.h) / 2.0;
        let frame = frame_attrs(item.x, item.y, item.w, item.h);
        push_box_open(&mut nodes, info);
        nodes.push_str("<rect ");
        nodes.push_str(&frame);
        match series(info.kind) {
            None => nodes.push_str(" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/>"),
            Some(slot) => {
                nodes.push_str(" fill=\"var(--_bg)\"/>");
                push_series_rect(&mut nodes, &frame, slot);
            }
        }
        let name_y = item.y + BOX_PAD_Y + item.name.height / 2.0;
        nodes.push_str(&label_svg(
            &item.name,
            item.x + item.w / 2.0,
            name_y,
            &name_attrs,
            None,
        ));
        nodes.push_str(&lines(
            &item.data,
            item.x + BOX_PAD_X,
            item.y + BOX_PAD_Y + item.name.height + 6.0 + mono_step / 2.0 + mono * 0.35,
            mono_step,
            &data_attrs,
        ));
        nodes.push_str("</g>");
    }

    let mut edges = String::new();
    let trim = marker_trim("arrow_point");
    for &relation in &model.relations {
        let (Some(s), Some(t)) = (placed.get(relation.0), placed.get(relation.1)) else {
            continue;
        };
        if relation.0 == relation.1 {
            continue;
        }
        let same_lane = model.boxes.get(relation.0).map(|item| item.lane)
            == model.boxes.get(relation.1).map(|item| item.lane);
        let mut d = String::new();
        let head = if same_lane {
            // Along the lane, from the side of one box to the side of the other.
            let forward = t.x >= s.x;
            let sx = if forward { s.x + s.w } else { s.x };
            let tx = if forward { t.x } else { t.x + t.w };
            let sy = s.y + s.h / 2.0;
            let ty = t.y + t.h / 2.0;
            let dir = if tx >= sx { 1.0 } else { -1.0 };
            push_move(&mut d, sx, sy);
            d.push('L');
            push_num(&mut d, tx - dir * trim);
            d.push(',');
            push_num(&mut d, ty);
            marker("arrow_point", tx, ty, dir, 0.0, "var(--_l)")
        } else {
            // Between lanes, straight down or up where the boxes overlap, else in an S.
            let down = t.y > s.y;
            let from = s.x.max(t.x);
            let to = (s.x + s.w).min(t.x + t.w);
            let inset = |w: f64| 24.0_f64.min(w / 4.0);
            let overlap = to - from >= 16.0;
            let sx = if overlap {
                f64::midpoint(from, to)
            } else if t.x > s.x {
                s.x + s.w - inset(s.w)
            } else {
                s.x + inset(s.w)
            };
            let tx = if overlap {
                sx
            } else if t.x > s.x {
                t.x + inset(t.w)
            } else {
                t.x + t.w - inset(t.w)
            };
            let sy = if down { s.y + s.h } else { s.y };
            let ty = if down { t.y } else { t.y + t.h };
            let dir = if down { 1.0 } else { -1.0 };
            let end = ty - dir * trim;
            let mid = f64::midpoint(sy, end);
            push_move(&mut d, sx, sy);
            #[expect(clippy::float_cmp, reason = "Pele compares the two exactly")]
            if sx == tx {
                d.push('V');
                push_num(&mut d, end);
            } else {
                d.push('C');
                push_num(&mut d, sx);
                d.push(',');
                push_num(&mut d, mid);
                d.push(' ');
                push_num(&mut d, tx);
                d.push(',');
                push_num(&mut d, mid);
                d.push(' ');
                push_num(&mut d, tx);
                d.push(',');
                push_num(&mut d, end);
            }
            marker("arrow_point", tx, ty, 0.0, dir, "var(--_l)")
        };
        push_edge(&mut edges, model, relation, &d, &head);
    }

    // Notes and given/when/then specifications, listed under the lanes with the frame they
    // belong to.
    let mut cards = String::new();
    let mut cards_right: f64 = 0.0;
    let mut small_attrs = font_size(small);
    small_attrs.push_str(" font-weight=\"var(--_hw)\"");
    let row_attrs = font_size(small);
    let mut card = |kind: &str, heading: &str, frame: &str, rows: &[String]| {
        let head = layout_label(
            Some(&card_heading(model, heading, frame)),
            false,
            small,
            4000.0,
            BOLD,
            None,
        );
        let w = head.width.max(widest(rows, small, 0)) + 2.0 * BOX_PAD_X;
        #[expect(clippy::cast_precision_loss, reason = "row counts stay far below 2^52")]
        let h = 2.0 * BOX_PAD_Y + head.height + rows.len() as f64 * small_step;
        let cy = bottom
            + if cards.is_empty() && bands.is_empty() {
                0.0
            } else {
                CARD_GAP
            };
        cards.push_str("<g class=\"ondine-em-");
        cards.push_str(kind);
        cards.push_str("\" data-id=\"");
        cards.push_str(&esc_text(frame));
        cards.push_str("\"><rect x=\"");
        push_num(&mut cards, left);
        cards.push_str("\" y=\"");
        push_num(&mut cards, cy);
        cards.push_str("\" width=\"");
        push_num(&mut cards, w);
        cards.push_str("\" height=\"");
        push_num(&mut cards, h);
        cards.push_str("\" rx=\"");
        cards.push_str(RADIUS);
        cards.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/>");
        cards.push_str(&label_svg(
            &head,
            left + BOX_PAD_X + head.width / 2.0,
            cy + BOX_PAD_Y + head.height / 2.0,
            &small_attrs,
            None,
        ));
        cards.push_str(&lines(
            rows,
            left + BOX_PAD_X,
            cy + BOX_PAD_Y + head.height + small_step / 2.0 + small * 0.35,
            small_step,
            &row_attrs,
        ));
        cards.push_str("</g>");
        bottom = cy + h;
        cards_right = cards_right.max(w);
    };
    for note in &model.notes {
        card(
            "note",
            "Note",
            &note.frame,
            &hard_wrap(&note.text, NOTE_COLUMNS),
        );
    }
    for spec in &model.specifications {
        let rows: Vec<String> = specification_rows(&spec.given, &spec.when, &spec.then)
            .iter()
            .flat_map(|row| hard_wrap(row, NOTE_COLUMNS))
            .collect();
        card("specification", "Given / When / Then", &spec.frame, &rows);
    }

    let width = (if bands.is_empty() { 0.0 } else { lane_width })
        .max(cards_right)
        .max(title.width);
    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&lanes);
    content.push_str(&edges);
    content.push_str(&nodes);
    content.push_str(&cards);
    document(model, options, total_width, total_height, size, &content)
}

/// A card placed when running down.
#[derive(Clone, Copy)]
struct Card {
    /// Height.
    h: f64,
    /// The card's position in frame order.
    index: usize,
    /// The width of its lane heading.
    lane: f64,
    /// Width.
    w: f64,
    /// Top edge.
    y: f64,
}

/// Draws `model` as cards running down, with relations that skip cards on rails beside them.
fn vertical(model: &EventModelingModel, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let mono = js_round(size * 0.75);
    let pad = options.padding.unwrap_or(8.0);
    let room = 160.0_f64.max(options.max_width.unwrap_or(320.0) - 2.0 * pad);

    // A relation that skips a card runs down a rail beside the cards. Relations into one card
    // share a rail; others whose runs overlap take the next rail out, so that no two follow the
    // same line.
    let mut runs: Vec<(usize, [usize; 2])> = Vec::new();
    // A relation between neighbouring cards passes the lower card's lane heading.
    let mut passed = vec![false; model.boxes.len()];
    for &(s, t) in &model.relations {
        if s == t || s >= model.boxes.len() || t >= model.boxes.len() {
            continue;
        }
        if s.abs_diff(t) < 2 {
            if let Some(flag) = passed.get_mut(s.max(t)) {
                *flag = true;
            }
            continue;
        }
        match runs.iter_mut().find(|(target, _)| *target == t) {
            Some((_, run)) => {
                run[0] = run[0].min(s);
                run[1] = run[1].max(s);
            }
            None => runs.push((t, [s.min(t), s.max(t)])),
        }
    }
    runs.sort_by_key(|&(_, [from, _])| from);
    let mut rail_of: Vec<(usize, usize)> = Vec::new();
    let mut rail_ends: Vec<usize> = Vec::new();
    for &(target, [from, to]) in &runs {
        let rail = rail_ends
            .iter()
            .position(|&end| end < from)
            .unwrap_or(rail_ends.len());
        match rail_ends.get_mut(rail) {
            Some(end) => *end = to,
            None => rail_ends.push(to),
        }
        rail_of.push((target, rail));
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "rail counts stay far below 2^52"
    )]
    let gutter = 24.0 + RAIL_GAP * rail_ends.len().saturating_sub(1) as f64;
    // Rails are never shared to save room: past what the width allows, the drawing is wider.
    let card_width = CARD_MIN.max(room - gutter);
    let text_room = card_width - 2.0 * BOX_PAD_X;
    let title = layout_label(model.title.as_deref(), false, size, room, BOLD, None);
    let mut y = pad
        + if title.height > 0.0 {
            title.height + 12.0
        } else {
            0.0
        };
    let mut bottom = pad + title.height;
    let mut width = title.width.max(card_width + gutter);
    let mut nodes = String::new();
    let mut edges = String::new();
    let mut cards = String::new();
    let mut placed: Vec<Card> = Vec::with_capacity(model.boxes.len());
    let text =
        |raw: &str, size: f64, style: u8| literal_label(&decode(raw), size, text_room, style);

    let mut lane_attrs = String::from(" class=\"ondine-cluster-label\"");
    lane_attrs.push_str(&font_size(small));
    lane_attrs.push_str(" font-weight=\"var(--_hw)\" fill=\"var(--_m)\"");
    let mut name_attrs = String::from(" class=\"ondine-label\"");
    name_attrs.push_str(&font_size(small));
    name_attrs.push_str(" font-weight=\"var(--_hw)\"");
    let mut data_attrs = String::from(" class=\"ondine-em-data\" font-family=\"");
    data_attrs.push_str(FONT_MONO);
    data_attrs.push('"');
    data_attrs.push_str(&font_size(mono));
    for (index, item) in model.boxes.iter().enumerate() {
        let lane_label = model
            .lanes
            .get(item.lane)
            .map_or("", |lane| lane.label.as_str());
        let lane_room = if passed.get(index).copied().unwrap_or(false) {
            text_room - ARROW_ROOM
        } else {
            text_room
        };
        let lane = break_label(
            &layout_label(Some(lane_label), false, small, lane_room, BOLD, None),
            lane_room,
        );
        let name = layout_label(Some(&item.name), false, small, text_room - 36.0, BOLD, None);
        let data = item
            .data
            .as_deref()
            .filter(|data| !data.is_empty())
            .map(|data| text(data, mono, MONO));
        let w = card_width
            .max(lane.width + 2.0 * BOX_PAD_X)
            .max(name.width + 2.0 * BOX_PAD_X + 36.0)
            .max(data.as_ref().map_or(0.0, |data| data.width) + 2.0 * BOX_PAD_X);
        let head_h = lane.height + 12.0;
        let h = 2.0 * BOX_PAD_Y + name.height + data.as_ref().map_or(0.0, |data| data.height + 6.0);
        let cy = y + head_h;
        let frame = frame_attrs(pad, cy, w, h);
        nodes.push_str("<g class=\"ondine-cluster ondine-em-lane\" data-id=\"");
        nodes.push_str(&esc_text(lane_label));
        nodes.push_str("\">");
        nodes.push_str(&left_label_svg(&lane, pad + BOX_PAD_X, y, &lane_attrs));
        push_box_open(&mut nodes, item);
        nodes.push_str("<rect ");
        nodes.push_str(&frame);
        nodes.push_str(" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/>");
        if let Some(slot) = series(item.kind) {
            push_series_rect(&mut nodes, &frame, slot);
        }
        nodes.push_str(&label_svg(
            &name,
            pad + BOX_PAD_X + name.width / 2.0,
            cy + BOX_PAD_Y + name.height / 2.0,
            &name_attrs,
            None,
        ));
        nodes.push_str("<text class=\"ondine-em-frame\" x=\"");
        push_num(&mut nodes, pad + w - BOX_PAD_X);
        nodes.push_str("\" y=\"");
        push_num(&mut nodes, cy + BOX_PAD_Y + small);
        nodes.push_str("\" text-anchor=\"end\"");
        nodes.push_str(&font_size(mono));
        nodes.push_str(" fill=\"var(--_m)\">");
        nodes.push_str(&esc(&item.id));
        nodes.push_str("</text>");
        if let Some(data) = &data {
            nodes.push_str(&lines(
                &label_rows(data),
                pad + BOX_PAD_X,
                cy + BOX_PAD_Y + name.height + 6.0 + data.line_height / 2.0 + mono * 0.35,
                data.line_height,
                &data_attrs,
            ));
        }
        nodes.push_str("</g></g>");
        placed.push(Card {
            h,
            index,
            lane: lane.width,
            w,
            y: cy,
        });
        width = width.max(w + gutter);
        bottom = cy + h;
        y = bottom + 28.0;
    }

    let trim = marker_trim("arrow_point");
    for &relation in &model.relations {
        let (Some(&s), Some(&t)) = (placed.get(relation.0), placed.get(relation.1)) else {
            continue;
        };
        if relation.0 == relation.1 {
            continue;
        }
        let down = t.y > s.y;
        let dir = if down { 1.0 } else { -1.0 };
        let sy = if down { s.y + s.h } else { s.y };
        let ty = if down { t.y } else { t.y + t.h };
        let mut d = String::new();
        let head = if s.index.abs_diff(t.index) == 1 {
            // The lower card's lane heading sits in the gap, and wraps to leave the arrow room
            // beside it.
            let lower = if down { t } else { s };
            let cx = pad + (s.w.min(t.w) / 2.0).max(BOX_PAD_X + lower.lane + 16.0);
            push_move(&mut d, cx, sy);
            d.push('V');
            push_num(&mut d, ty - dir * trim);
            marker("arrow_point", cx, ty, 0.0, dir, "var(--_l)")
        } else {
            let slot = rail_of
                .iter()
                .find(|&&(target, _)| target == relation.1)
                .map_or(0, |&(_, rail)| rail);
            #[expect(
                clippy::cast_precision_loss,
                reason = "rail counts stay far below 2^52"
            )]
            let rail = pad + width - gutter + 12.0 + RAIL_GAP * slot as f64;
            // A card can send one relation and receive another: they leave above and arrive
            // below its middle.
            let from = s.y + s.h / 2.0 - 5.0;
            let to = t.y + t.h / 2.0 + 5.0;
            push_move(&mut d, pad + s.w, from);
            d.push('H');
            push_num(&mut d, rail);
            d.push('V');
            push_num(&mut d, to);
            d.push('H');
            push_num(&mut d, pad + t.w + trim);
            marker("arrow_point", pad + t.w, to, -1.0, 0.0, "var(--_l)")
        };
        push_edge(&mut edges, model, relation, &d, &head);
    }

    let mut head_attrs = font_size(small);
    head_attrs.push_str(" font-weight=\"var(--_hw)\"");
    let row_attrs = font_size(small);
    let mut card = |kind: &str, heading: &str, frame: &str, raw: &str| {
        let head = layout_label(
            Some(&card_heading(model, heading, frame)),
            false,
            small,
            text_room,
            BOLD,
            None,
        );
        let body = text(raw, small, 0);
        let w = card_width
            .max(head.width + 2.0 * BOX_PAD_X)
            .max(body.width + 2.0 * BOX_PAD_X);
        let h = 2.0 * BOX_PAD_Y + head.height + body.height;
        cards.push_str("<g class=\"ondine-em-");
        cards.push_str(kind);
        cards.push_str("\" data-id=\"");
        cards.push_str(&esc_text(frame));
        cards.push_str("\"><rect x=\"");
        push_num(&mut cards, pad);
        cards.push_str("\" y=\"");
        push_num(&mut cards, y);
        cards.push_str("\" width=\"");
        push_num(&mut cards, w);
        cards.push_str("\" height=\"");
        push_num(&mut cards, h);
        cards.push_str("\" rx=\"");
        cards.push_str(RADIUS);
        cards.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"/>");
        cards.push_str(&label_svg(
            &head,
            pad + BOX_PAD_X + head.width / 2.0,
            y + BOX_PAD_Y + head.height / 2.0,
            &head_attrs,
            None,
        ));
        cards.push_str(&lines(
            &label_rows(&body),
            pad + BOX_PAD_X,
            y + BOX_PAD_Y + head.height + body.line_height / 2.0 + small * 0.35,
            body.line_height,
            &row_attrs,
        ));
        cards.push_str("</g>");
        bottom = y + h;
        y = bottom + CARD_GAP;
        width = width.max(w);
    };
    for note in &model.notes {
        card("note", "Note", &note.frame, &note.text);
    }
    for spec in &model.specifications {
        card(
            "specification",
            "Given / When / Then",
            &spec.frame,
            &specification_rows(&spec.given, &spec.when, &spec.then).join("\n"),
        );
    }

    let total_width = (width + 2.0 * pad).ceil();
    let height = (bottom + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str(&edges);
    content.push_str(&nodes);
    content.push_str(&cards);
    document(model, options, total_width, height, size, &content)
}
