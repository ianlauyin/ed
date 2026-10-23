//! Lays out and draws a sequence diagram. Port of Pele's `src/diagrams/sequence/render.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::actors::{ActorSize, actor_kind, actor_size, draw_actor, is_figure};
use super::db::SeqDb;
use super::types::{MessageText, SeqActor, SeqMessage, line, placement};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::diagrams::common::literal_label::break_label;
use crate::svg::builder::{esc, esc_text, js_round, label_svg, num, push_num};
use crate::svg::edges::{ARROW, marker};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{RADIUS, class_names, resolve_style};
use crate::text::label::{Label, layout_label};
use crate::text::measure::{BOLD, width as measure};
use crate::yaml::{Value, push_number};

/// The head drawn at a message's end.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Head {
    None,
    Arrow,
    Cross,
    Open,
    HalfSolid,
    HalfStick,
}

/// How a message line is drawn.
#[derive(Clone, Copy)]
#[expect(clippy::struct_excessive_bools, reason = "mirrors Pele's LineStyle")]
struct LineStyle {
    /// Whether a head is at the sender: the reverse arrows and the bidirectional ones.
    at_start: bool,
    /// Whether a head is at the receiver.
    at_end: bool,
    bottom: bool,
    dotted: bool,
    head: Head,
}

/// The line style of a message kind, or `None` for what is not a message line.
fn style_of(r#type: u8) -> Option<LineStyle> {
    // `ends` is 1 for a head at the receiver, 0 at the sender only, and 2 at both.
    let style = |dotted: bool, head: Head, ends: u8, bottom: bool| LineStyle {
        at_start: ends != 1,
        at_end: ends != 0,
        bottom,
        dotted,
        head,
    };
    Some(match r#type {
        line::SOLID => style(false, Head::Arrow, 1, false),
        line::DOTTED => style(true, Head::Arrow, 1, false),
        line::SOLID_CROSS => style(false, Head::Cross, 1, false),
        line::DOTTED_CROSS => style(true, Head::Cross, 1, false),
        line::SOLID_OPEN => style(false, Head::None, 1, false),
        line::DOTTED_OPEN => style(true, Head::None, 1, false),
        line::SOLID_POINT => style(false, Head::Open, 1, false),
        line::DOTTED_POINT => style(true, Head::Open, 1, false),
        line::BIDIRECTIONAL_SOLID => style(false, Head::Arrow, 2, false),
        line::BIDIRECTIONAL_DOTTED => style(true, Head::Arrow, 2, false),
        41..=48 | 51..=58 => {
            let dotted = r#type > 50;
            match (r#type - if dotted { 10 } else { 0 }) - line::SOLID_TOP {
                0 => style(dotted, Head::HalfSolid, 1, false),
                1 => style(dotted, Head::HalfSolid, 1, true),
                2 => style(dotted, Head::HalfStick, 1, false),
                3 => style(dotted, Head::HalfStick, 1, true),
                4 => style(dotted, Head::HalfSolid, 0, false),
                5 => style(dotted, Head::HalfSolid, 0, true),
                6 => style(dotted, Head::HalfStick, 0, false),
                _ => style(dotted, Head::HalfStick, 0, true),
            }
        }
        _ => return None,
    })
}

fn frame_kind(r#type: u8) -> Option<&'static str> {
    Some(match r#type {
        line::LOOP_START => "loop",
        line::ALT_START => "alt",
        line::OPT_START => "opt",
        line::PAR_START | line::PAR_OVER_START => "par",
        line::CRITICAL_START => "critical",
        line::BREAK_START => "break",
        line::RECT_START => "rect",
        _ => return None,
    })
}

const MSG_PAD: f64 = 12.0;
const SELF_W: f64 = 28.0;
const SELF_H: f64 = 22.0;
const ACT_W: f64 = 10.0;
const ACT_STEP: f64 = 4.0;
const ACT_MIN: f64 = 14.0;
const NOTE_GAP: f64 = 10.0;
const NOTE_OVER: f64 = 16.0;
const FRAME_PAD: f64 = 20.0;
const FRAME_NEST: f64 = 10.0;
const TAB_H: f64 = 20.0;
const FRAME_TEXT: f64 = 240.0;
const BADGE_H: f64 = 16.0;
const CENTRAL_R: f64 = 4.0;
const DASH: &str = " stroke-dasharray=\"4 4\"";

/// The numbers of the message lines.
pub(crate) struct Numbering {
    /// The number each message line carries, whether or not it is shown. NaN for everything else.
    pub(crate) index: Vec<f64>,
    /// Whether each message's number is shown.
    pub(crate) shown: Vec<bool>,
    /// Whether numbers are on after the last statement.
    pub(crate) visible: bool,
}

/// Whether a message kind is drawn as a line.
pub(crate) fn is_line_message(r#type: Option<u8>) -> bool {
    r#type.and_then(style_of).is_some()
}

/// Counts message lines the way Mermaid's renderer does while it draws.
pub(crate) fn number_messages(messages: &[SeqMessage], always: bool) -> Numbering {
    let mut index = vec![f64::NAN; messages.len()];
    let mut shown = vec![false; messages.len()];
    let mut current = 1.0;
    let mut step = 1.0;
    let mut visible = false;
    for (i, m) in messages.iter().enumerate() {
        if m.r#type == Some(line::AUTONUMBER) {
            if let MessageText::Numbering(spec) = &m.message {
                // `spec.start || current`: zero and NaN keep the current value.
                if let Some(start) = spec.start.filter(|s| *s != 0.0 && !s.is_nan()) {
                    current = start;
                }
                if let Some(next) = spec.step.filter(|s| *s != 0.0 && !s.is_nan()) {
                    step = next;
                }
                visible = spec.visible;
            }
        } else if is_line_message(m.r#type) {
            if let Some(slot) = index.get_mut(i) {
                *slot = current;
            }
            if let Some(slot) = shown.get_mut(i) {
                *slot = visible || always;
            }
            current = js_round((current + step) * 100.0) / 100.0;
        }
    }
    Numbering {
        index,
        shown,
        visible,
    }
}

/// A participant's column.
struct Column<'d> {
    activations: Vec<f64>,
    actor: &'d SeqActor,
    create_at: Option<usize>,
    destroy_at: Option<usize>,
    id: &'d str,
    kind: &'static str,
    label: Label,
    life_end: Option<f64>,
    life_start: f64,
    size: ActorSize,
    top: f64,
    x: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum V {
    Line,
    SelfLoop,
    Note,
}

/// A message or note with the columns it joins.
struct View {
    a: usize,
    b: usize,
    kind: V,
    label: Label,
    w: f64,
    x: f64,
}

/// A block's frame.
struct Frame {
    divider_labels: Vec<Label>,
    divider_y: Vec<f64>,
    dividers: Vec<usize>,
    fill: String,
    kind: &'static str,
    tab_w: f64,
    title: Label,
    top: f64,
    x0: f64,
    x1: f64,
}

/// Consecutive columns of one box.
struct BoxRun {
    r#box: usize,
    first: usize,
    grow: f64,
    last: usize,
    title: Label,
}

/// The horizontal extent of everything drawn.
struct Bounds {
    max_x: f64,
    min_x: f64,
}

impl Bounds {
    fn extend(&mut self, x0: f64, x1: f64) {
        if x0 < self.min_x {
            self.min_x = x0;
        }
        if x1 > self.max_x {
            self.max_x = x1;
        }
    }
}

/// The value under `key` of a YAML mapping.
fn get<'v>(value: Option<&'v Value>, key: &str) -> Option<&'v Value> {
    match value {
        Some(Value::Object(entries)) => entries.iter().find(|(own, _)| own == key).map(|(_, v)| v),
        _ => None,
    }
}

fn option(conf: Option<&Value>, key: &str, fallback: f64) -> f64 {
    match get(conf, key) {
        Some(Value::Number(value)) if *value >= 0.0 && *value <= 2000.0 => *value,
        _ => fallback,
    }
}

fn half(out: &mut String, x: f64, y: f64, dir: f64, up: f64, stick: bool) {
    let bx = x - dir * ARROW;
    out.push_str("<path class=\"ondine-marker\" d=\"M");
    push_num(out, x);
    out.push(',');
    push_num(out, y);
    out.push('L');
    push_num(out, bx);
    out.push(',');
    if stick {
        push_num(out, y + up * 5.0);
        out.push_str("\"/>");
        return;
    }
    push_num(out, y + up * 4.5);
    out.push('V');
    push_num(out, y);
    out.push_str("Z\" fill=\"var(--_l)\" stroke=\"none\"/>");
}

/// Draws the head whose tip is at (`x`, `y`) and points along `dir`. `up` is the side a half head
/// sits on.
fn head(out: &mut String, r#type: Head, x: f64, y: f64, dir: f64, up: f64) {
    match r#type {
        Head::Arrow => out.push_str(&marker("arrow_point", x, y, dir, 0.0, "var(--_l)")),
        Head::Cross => out.push_str(&marker("arrow_cross", x, y, dir, 0.0, "var(--_l)")),
        Head::Open => {
            out.push_str("<path class=\"ondine-marker\" d=\"M");
            push_num(out, x - dir * ARROW);
            out.push(',');
            push_num(out, y - 4.5);
            out.push('L');
            push_num(out, x);
            out.push(',');
            push_num(out, y);
            out.push('L');
            push_num(out, x - dir * ARROW);
            out.push(',');
            push_num(out, y + 4.5);
            out.push_str("\" stroke-linejoin=\"round\"/>");
        }
        Head::HalfSolid => half(out, x, y, dir, up, false),
        Head::HalfStick => half(out, x, y, dir, up, true),
        Head::None => {}
    }
}

/// A message's text, unless it is a numbering entry.
fn message_text(m: &SeqMessage) -> Option<&str> {
    match &m.message {
        MessageText::Text(text) => Some(text),
        MessageText::Numbering(_) => None,
    }
}

fn trim_of(r#type: Head) -> f64 {
    match r#type {
        Head::Arrow => ARROW - 1.0,
        Head::Cross => 4.0,
        _ => 0.0,
    }
}

/// Draws a sequence diagram, tightening its spacing when it does not fit the host's width.
pub(crate) fn render(db: &SeqDb, config: &Value, options: &RenderOptions) -> Rendered {
    tighten(options, |tight| draw(db, config, options, tight))
}

#[expect(
    clippy::many_single_char_names,
    reason = "mirrors the names of Pele's draw"
)]
fn draw(db: &SeqDb, config: &Value, options: &RenderOptions, tight: bool) -> Rendered {
    let conf = get(Some(config), "sequence");
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let tiny = js_round(size * 0.75);
    let pad = options.padding.unwrap_or(8.0);
    let mirror = get(conf, "mirrorActors") != Some(&Value::Bool(false));
    let right_angles = get(conf, "rightAngles") == Some(&Value::Bool(true));
    let loose = |value: f64, tight_value: f64| if tight { value.min(tight_value) } else { value };
    let actor_margin = loose(option(conf, "actorMargin", 40.0), 8.0);
    let min_w = loose(option(conf, "width", 64.0), 48.0);
    let min_h = option(conf, "height", js_round(size * 1.5) + 20.0);
    #[expect(clippy::cast_precision_loss, reason = "a participant count")]
    let actor_count = db.actors.len() as f64;
    let wrap_w = loose(
        option(conf, "width", 160.0),
        32.0_f64.max(
            (options.max_width.unwrap_or(320.0) - 2.0 * pad) / actor_count.max(1.0)
                - actor_margin
                - 24.0,
        ),
    );
    // Mermaid's default of 35 corresponds to Pele's default gap. Other values move it by the
    // difference.
    let msg_gap = 4.0_f64.max(14.0 + option(conf, "messageMargin", 35.0) - 35.0);
    let note_pad = loose(option(conf, "noteMargin", 10.0), 6.0);
    let box_pad = loose(option(conf, "boxMargin", 10.0), 6.0);
    let align = match get(conf, "messageAlign") {
        Some(Value::String(a)) if a == "left" => -1,
        Some(Value::String(a)) if a == "right" => 1,
        _ => 0,
    };

    let msg_pad = if tight { 6.0 } else { MSG_PAD };
    let note_over = if tight { 8.0 } else { NOTE_OVER };
    let frame_pad = if tight { 12.0 } else { FRAME_PAD };
    let frame_nest = if tight { 6.0 } else { FRAME_NEST };
    let messages = &db.messages;
    let count = messages.len();
    let numbering = number_messages(
        messages,
        get(conf, "showSequenceNumbers") == Some(&Value::Bool(true)),
    );
    let text = |raw: Option<&str>, font_size: f64, wrap: bool, width: f64, base: u8| -> Label {
        let label = layout_label(
            raw,
            false,
            font_size,
            if tight || wrap { width } else { f64::INFINITY },
            base,
            None,
        );
        if tight {
            break_label(&label, width.max(120.0))
        } else {
            label
        }
    };

    let used: Option<BTreeSet<&str>> =
        (get(conf, "hideUnusedParticipants") == Some(&Value::Bool(true))).then(|| {
            messages
                .iter()
                .flat_map(|m| [m.from.as_deref(), m.to.as_deref()])
                .flatten()
                .collect()
        });

    let mut next_line: Vec<Option<usize>> = vec![None; count + 1];
    for i in (0..count).rev() {
        let next = next_line.get(i + 1).copied().flatten();
        if let Some(slot) = next_line.get_mut(i) {
            *slot = if messages.get(i).is_some_and(|m| is_line_message(m.r#type)) {
                Some(i)
            } else {
                next
            };
        }
    }
    let line_after = |at: usize| next_line.get(at.min(count)).copied().flatten();

    let mut cols: Vec<Column> = Vec::new();
    let mut col_of: BTreeMap<&str, usize> = BTreeMap::new();
    for actor in &db.actors {
        let id = actor.name.as_str();
        if used.as_ref().is_some_and(|used| !used.contains(id)) {
            continue;
        }
        let kind = actor_kind(&actor.r#type);
        let label = text(Some(&actor.description), size, actor.wrap, wrap_w, 0);
        let created = db
            .created_actors
            .iter()
            .find(|(own, _)| own == id)
            .map(|&(_, at)| at);
        let destroyed = db
            .destroyed_actors
            .iter()
            .find(|(own, _)| own == id)
            .map(|&(_, at)| at);
        let create_at = created.and_then(line_after).filter(|&at| {
            messages
                .get(at)
                .is_some_and(|m| m.to.as_deref() == Some(id))
        });
        let destroy_at = destroyed.and_then(line_after).filter(|&at| {
            messages
                .get(at)
                .is_some_and(|m| m.to.as_deref() == Some(id) || m.from.as_deref() == Some(id))
        });
        col_of.insert(id, cols.len());
        let size = actor_size(kind, &label, min_w, min_h, tight.then_some(8.0));
        cols.push(Column {
            activations: Vec::new(),
            actor,
            create_at,
            destroy_at,
            id,
            kind,
            label,
            life_end: None,
            life_start: 0.0,
            size,
            top: 0.0,
            x: 0.0,
        });
    }
    let n = cols.len();

    // Widths between lifelines. Every message and note says how far apart the lifelines it
    // touches must be.
    let mut needs: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    let mut need = |lo: Option<usize>, hi: usize, width: f64| {
        let Some(lo) = lo else {
            return;
        };
        if hi >= n {
            return;
        }
        let slot = needs.entry((lo, hi)).or_insert(0.0);
        if *slot < width {
            *slot = width;
        }
    };
    let badge_width = |i: usize| {
        if numbering.shown.get(i).copied().unwrap_or(false) {
            let mut label = String::new();
            push_number(
                &mut label,
                numbering.index.get(i).copied().unwrap_or(f64::NAN),
            );
            BADGE_H.max(measure(&label, tiny, 0) + 8.0)
        } else {
            0.0
        }
    };

    let mut views: Vec<Option<View>> = Vec::with_capacity(count);
    for (i, m) in messages.iter().enumerate() {
        let a = m.from.as_deref().and_then(|id| col_of.get(id).copied());
        let b = m.to.as_deref().and_then(|id| col_of.get(id).copied());
        let (Some(a), Some(b), Some(raw)) = (a, b, message_text(m)) else {
            views.push(None);
            continue;
        };
        if m.r#type == Some(line::NOTE) {
            let label = text(Some(raw), small, m.wrap, wrap_w, 0);
            let w = label.width + 2.0 * note_pad;
            if m.placement == Some(placement::LEFT_OF) {
                need(a.checked_sub(1), a, w + 2.0 * NOTE_GAP + ACT_W);
            } else if m.placement == Some(placement::RIGHT_OF) {
                need(Some(a), a + 1, w + 2.0 * NOTE_GAP + ACT_W);
            } else if a == b {
                need(a.checked_sub(1), a, w / 2.0 + NOTE_GAP);
                need(Some(a), a + 1, w / 2.0 + NOTE_GAP);
            } else {
                need(Some(a.min(b)), a.max(b), w - 2.0 * note_over);
            }
            views.push(Some(View {
                a,
                b,
                kind: V::Note,
                label,
                w,
                x: 0.0,
            }));
        } else if is_line_message(m.r#type) {
            let label = text(Some(raw), small, m.wrap, wrap_w, 0);
            let badge = badge_width(i);
            if a == b {
                need(
                    Some(a),
                    a + 1,
                    SELF_W + 8.0 + label.width + 16.0 + ACT_W + badge,
                );
                views.push(Some(View {
                    a,
                    b,
                    kind: V::SelfLoop,
                    label,
                    w: 0.0,
                    x: 0.0,
                }));
            } else {
                let created = cols
                    .get(b)
                    .filter(|col| col.create_at == Some(i))
                    .map_or(0.0, |col| col.size.w / 2.0);
                let central = if m.central_connection.is_some_and(|c| c != 0) {
                    4.0 * CENTRAL_R
                } else {
                    0.0
                };
                let width = if tight {
                    label.width.min(16.0_f64.max(wrap_w - 16.0))
                } else {
                    label.width
                };
                need(
                    Some(a.min(b)),
                    a.max(b),
                    width + 2.0 * msg_pad + ACT_W + badge + created + central,
                );
                views.push(Some(View {
                    a,
                    b,
                    kind: V::Line,
                    label,
                    w: 0.0,
                    x: 0.0,
                }));
            }
        } else {
            views.push(None);
        }
    }

    let mut runs: Vec<BoxRun> = Vec::new();
    for (i, col) in cols.iter().enumerate() {
        let Some(at) = col.actor.r#box else {
            continue;
        };
        if let Some(run) = runs.last_mut()
            && run.r#box == at
            && run.last + 1 == i
        {
            run.last = i;
            continue;
        }
        let r#box = db.boxes.get(at);
        runs.push(BoxRun {
            r#box: at,
            first: i,
            grow: 0.0,
            last: i,
            title: text(
                r#box.and_then(|b| b.name.as_deref()),
                small,
                r#box.is_some_and(|b| b.wrap),
                wrap_w,
                0,
            ),
        });
    }

    let mut gaps: Vec<f64> = Vec::with_capacity(n.saturating_sub(1));
    for i in 1..n {
        let (Some(left), Some(right)) = (cols.get(i - 1), cols.get(i)) else {
            continue;
        };
        let (lb, rb) = (left.actor.r#box, right.actor.r#box);
        let boxed = if lb == rb {
            0.0
        } else {
            (if lb.is_some() { box_pad } else { 0.0 }) + (if rb.is_some() { box_pad } else { 0.0 })
        };
        gaps.push(
            (left.size.w / 2.0 + right.size.w / 2.0 + actor_margin + boxed)
                .max(needs.get(&(i - 1, i)).copied().unwrap_or(0.0)),
        );
    }
    // A message across several lifelines widens each gap it crosses by an equal share of what is
    // missing.
    let mut spans: Vec<(usize, usize, f64)> = needs
        .iter()
        .filter(|&(&(lo, hi), _)| hi - lo > 1)
        .map(|(&(lo, hi), &width)| (lo, hi, width))
        .collect();
    spans.sort_by_key(|p| (p.1 - p.0, p.0));
    for (lo, hi, width) in spans {
        let total: f64 = gaps.get(lo..hi).map_or(0.0, |g| g.iter().sum());
        if total >= width {
            continue;
        }
        #[expect(clippy::cast_precision_loss, reason = "a column count")]
        let share = (width - total) / (hi - lo) as f64;
        for gap in gaps.get_mut(lo..hi).into_iter().flatten() {
            *gap += share;
        }
    }
    let place = |cols: &mut [Column], gaps: &[f64]| {
        for i in 1..cols.len() {
            let x = cols.get(i - 1).map_or(0.0, |c| c.x) + gaps.get(i - 1).copied().unwrap_or(0.0);
            if let Some(col) = cols.get_mut(i) {
                col.x = x;
            }
        }
    };
    place(&mut cols, &gaps);
    let col_x = |cols: &[Column], i: usize| cols.get(i).map_or(0.0, |c| c.x);
    let col_w = |cols: &[Column], i: usize| cols.get(i).map_or(0.0, |c| c.size.w);
    for run in &mut runs {
        let inner = col_x(&cols, run.last) + col_w(&cols, run.last) / 2.0
            - (col_x(&cols, run.first) - col_w(&cols, run.first) / 2.0);
        run.grow = 0.0_f64.max((run.title.width - inner) / 2.0);
        if run.grow == 0.0 {
            continue;
        }
        if run.first > 0
            && let Some(gap) = gaps.get_mut(run.first - 1)
        {
            *gap += run.grow;
        }
        if run.last + 1 < n
            && let Some(gap) = gaps.get_mut(run.last)
        {
            *gap += run.grow;
        }
    }
    place(&mut cols, &gaps);

    let mut bounds = Bounds {
        max_x: f64::NEG_INFINITY,
        min_x: f64::INFINITY,
    };
    for col in &cols {
        bounds.extend(col.x - col.size.w / 2.0, col.x + col.size.w / 2.0);
    }
    for run in &runs {
        bounds.extend(
            col_x(&cols, run.first) - col_w(&cols, run.first) / 2.0 - box_pad - run.grow,
            col_x(&cols, run.last) + col_w(&cols, run.last) / 2.0 + box_pad + run.grow,
        );
    }

    // Horizontal extents of every block, from what it holds.
    let mut frames: Vec<Frame> = Vec::new();
    let mut frame_at: Vec<Option<usize>> = vec![None; count];
    let mut open: Vec<usize> = Vec::new();
    let span_all = |cols: &[Column]| {
        if n == 0 {
            (0.0, 120.0)
        } else {
            (col_x(cols, 0) - frame_pad, col_x(cols, n - 1) + frame_pad)
        }
    };
    for (i, m) in messages.iter().enumerate() {
        let r#type = m.r#type;
        if let Some(kind) = r#type.and_then(frame_kind) {
            let fill = match (&m.message, kind) {
                (MessageText::Text(text), "rect") => text.clone(),
                _ => String::new(),
            };
            if let Some(slot) = frame_at.get_mut(i) {
                *slot = Some(frames.len());
            }
            open.push(frames.len());
            frames.push(Frame {
                divider_labels: Vec::new(),
                divider_y: Vec::new(),
                dividers: vec![i],
                fill,
                kind,
                tab_w: 0.0,
                title: Label::default(),
                top: 0.0,
                x0: f64::INFINITY,
                x1: f64::NEG_INFINITY,
            });
        } else if matches!(
            r#type,
            Some(line::ALT_ELSE | line::PAR_AND | line::CRITICAL_OPTION)
        ) {
            if let Some(frame) = open.last().and_then(|&at| frames.get_mut(at)) {
                frame.dividers.push(i);
            }
        } else if matches!(
            r#type,
            Some(
                line::LOOP_END
                    | line::ALT_END
                    | line::OPT_END
                    | line::PAR_END
                    | line::CRITICAL_END
                    | line::BREAK_END
                    | line::RECT_END
            )
        ) {
            let Some(at) = open.pop() else {
                continue;
            };
            if let Some(slot) = frame_at.get_mut(i) {
                *slot = Some(at);
            }
            let Some(frame) = frames.get_mut(at) else {
                continue;
            };
            if frame.x0 > frame.x1 {
                (frame.x0, frame.x1) = span_all(&cols);
            }
            if frame.kind != "rect" {
                frame.tab_w = measure(frame.kind, tiny, BOLD) + 14.0;
                // A block grows to the right to give its texts up to FRAME_TEXT of width before
                // they wrap.
                for k in 0..frame.dividers.len() {
                    let raw = frame
                        .dividers
                        .get(k)
                        .and_then(|&d| messages.get(d))
                        .and_then(message_text)
                        .unwrap_or("");
                    let tab = if k == 0 { frame.tab_w + 8.0 } else { 0.0 };
                    let room = frame.x1 - frame.x0 - 16.0 - tab;
                    let label = if raw.is_empty() {
                        Label::default()
                    } else {
                        let least = if tight {
                            FRAME_TEXT.min(wrap_w)
                        } else {
                            FRAME_TEXT
                        };
                        text(
                            Some(&["[", raw, "]"].concat()),
                            small,
                            true,
                            room.max(least),
                            0,
                        )
                    };
                    if label.width > room {
                        frame.x1 += label.width - room;
                    }
                    if k == 0 {
                        frame.title = label;
                    } else {
                        frame.divider_labels.push(label);
                    }
                }
            }
            let (x0, x1) = (frame.x0, frame.x1);
            bounds.extend(x0, x1);
            if let Some(parent) = open.last().and_then(|&at| frames.get_mut(at)) {
                parent.x0 = parent.x0.min(x0 - frame_nest);
                parent.x1 = parent.x1.max(x1 + frame_nest);
            }
        } else if let Some(view) = views.get_mut(i).and_then(Option::as_mut) {
            let (Some(a), Some(b)) = (cols.get(view.a), cols.get(view.b)) else {
                continue;
            };
            let (x0, x1) = match view.kind {
                V::Note => {
                    if m.placement == Some(placement::LEFT_OF) {
                        view.x = a.x - NOTE_GAP - view.w;
                    } else if m.placement == Some(placement::RIGHT_OF) {
                        view.x = a.x + NOTE_GAP;
                    } else if view.a == view.b {
                        view.x = a.x - view.w / 2.0;
                    } else {
                        let lo = a.x.min(b.x) - note_over;
                        let hi = a.x.max(b.x) + note_over;
                        if tight || m.wrap {
                            view.label = text(
                                message_text(m),
                                small,
                                true,
                                wrap_w.max(hi - lo - 2.0 * note_pad),
                                0,
                            );
                        }
                        view.w = (hi - lo).max(view.label.width + 2.0 * note_pad);
                        view.x = (lo + hi - view.w) / 2.0;
                    }
                    (view.x, view.x + view.w)
                }
                V::SelfLoop => (
                    a.x,
                    a.x + ACT_W + SELF_W + 8.0 + view.label.width + badge_width(i),
                ),
                V::Line => {
                    let mut x0 = a.x.min(b.x);
                    let mut x1 = a.x.max(b.x);
                    if tight || m.wrap {
                        view.label = text(
                            message_text(m),
                            small,
                            true,
                            wrap_w.max(x1 - x0 - 2.0 * msg_pad - ACT_W),
                            0,
                        );
                    }
                    if b.create_at == Some(i) {
                        if b.x > a.x {
                            x1 += b.size.w / 2.0;
                        } else {
                            x0 -= b.size.w / 2.0;
                        }
                    }
                    (x0, x1)
                }
            };
            bounds.extend(x0, x1);
            if let Some(frame) = open.last().and_then(|&at| frames.get_mut(at)) {
                frame.x0 = frame.x0.min(x0 - frame_pad);
                frame.x1 = frame.x1.max(x1 + frame_pad);
            }
        }
    }

    // Vertical placement, top to bottom, drawing as it goes.
    let mut box_title_h: f64 = 0.0;
    for run in &runs {
        box_title_h = box_title_h.max(run.title.height);
    }
    let box_top = if runs.is_empty() {
        0.0
    } else {
        box_pad
            + if box_title_h > 0.0 {
                box_title_h + 4.0
            } else {
                0.0
            }
    };
    let mut row_h: f64 = 0.0;
    for col in &cols {
        if col.create_at.is_none() {
            row_h = row_h.max(col.size.h);
        }
    }
    let head_bottom = box_top + row_h;
    for col in &mut cols {
        col.top = head_bottom - col.size.h;
        col.life_start = head_bottom;
    }

    let xs: Vec<f64> = cols.iter().map(|col| col.x).collect();
    // Whether a lifeline runs between two x positions, where text would be struck through.
    let crosses = |x0: f64, x1: f64| {
        let at = xs.partition_point(|&x| x <= x0);
        xs.get(at).is_some_and(|&x| x < x1)
    };
    let backed = |label: &Label, cx: f64, cy: f64, attrs: &str| -> String {
        if label.width == 0.0 {
            return String::new();
        }
        let l = cx - label.width / 2.0;
        let mut out = String::new();
        if crosses(l, l + label.width) {
            out.push_str("<rect x=\"");
            push_num(&mut out, l - 3.0);
            out.push_str("\" y=\"");
            push_num(&mut out, cy - label.height / 2.0 + 2.0);
            out.push_str("\" width=\"");
            push_num(&mut out, label.width + 6.0);
            out.push_str("\" height=\"");
            push_num(&mut out, label.height - 4.0);
            out.push_str("\" rx=\"3\" fill=\"var(--_bg)\" stroke=\"none\"/>");
        }
        out.push_str(&label_svg(label, cx, cy, attrs, None));
        out
    };
    let left_edge = |col: &Column, extra: usize| {
        if col.activations.len() + extra > 0 {
            col.x - ACT_W / 2.0
        } else {
            col.x
        }
    };
    let right_edge = |col: &Column, extra: usize| {
        let depth = col.activations.len() + extra;
        if depth > 0 {
            #[expect(clippy::cast_precision_loss, reason = "a nesting depth")]
            let steps = (depth - 1) as f64;
            col.x + ACT_W / 2.0 + steps * ACT_STEP
        } else {
            col.x
        }
    };

    // Shaded blocks and activations close from the inside out. They are written in reverse, so
    // that the inner ones end up on top.
    let mut rects_out: Vec<String> = Vec::new();
    let mut activations_out: Vec<String> = Vec::new();
    let mut frames_out = String::new();
    let mut edges_out = String::new();
    let mut labels_out = String::new();
    let mut notes_out = String::new();
    let mut actors_out = String::new();
    let mut crosses_out = String::new();
    let end_activation = |col: &mut Column,
                          end: f64,
                          bounds: &mut Bounds,
                          activations_out: &mut Vec<String>|
     -> f64 {
        let Some(start) = col.activations.pop() else {
            return end;
        };
        let bottom = end.max(start + ACT_MIN);
        #[expect(clippy::cast_precision_loss, reason = "a nesting depth")]
        let depth = col.activations.len() as f64;
        let x = col.x - ACT_W / 2.0 + depth * ACT_STEP;
        bounds.extend(x, x + ACT_W);
        let mut out = String::from("<rect class=\"ondine-activation\" data-id=\"");
        out.push_str(&esc_text(col.id));
        out.push_str("\" x=\"");
        push_num(&mut out, x);
        out.push_str("\" y=\"");
        push_num(&mut out, start);
        out.push_str("\" width=\"");
        push_num(&mut out, ACT_W);
        out.push_str("\" height=\"");
        push_num(&mut out, bottom - start);
        out.push_str("\" rx=\"2\"/>");
        activations_out.push(out);
        bottom
    };
    // The lifeline of a destroyed participant ends in a cross a little below the message that
    // ends it.
    let destroy = |col: &mut Column, line_y: f64, crosses_out: &mut String| -> f64 {
        let cx = col.x;
        let cy = line_y + 16.0;
        col.life_end = Some(cy);
        crosses_out.push_str("<path class=\"ondine-destroy\" data-id=\"");
        crosses_out.push_str(&esc_text(col.id));
        crosses_out.push_str("\" d=\"M");
        for (k, (x, y)) in [
            (cx - 6.0, cy - 6.0),
            (cx + 6.0, cy + 6.0),
            (cx + 6.0, cy - 6.0),
            (cx - 6.0, cy + 6.0),
        ]
        .into_iter()
        .enumerate()
        {
            if k > 0 {
                crosses_out.push(if k == 2 { 'M' } else { 'L' });
            }
            push_num(crosses_out, x);
            crosses_out.push(',');
            push_num(crosses_out, y);
        }
        crosses_out.push_str("\"/>");
        cy + 6.0
    };
    let actor_svg = |col: &Column, top: f64, footer: bool| -> String {
        let mut out = String::from("<g class=\"ondine-node ondine-actor ondine-actor-");
        out.push_str(col.kind);
        if footer {
            out.push_str(" ondine-actor-footer");
        }
        if let Some((_, Value::String(cls))) =
            col.actor.properties.iter().find(|(key, _)| key == "class")
        {
            out.push_str(&class_names(cls));
        }
        out.push_str("\" data-id=\"");
        out.push_str(&esc_text(col.id));
        out.push_str("\">");
        draw_actor(&mut out, col.kind, col.x, top, col.size, &col.label);
        out.push_str("</g>");
        out
    };
    let badge = |i: usize, cx: f64, cy: f64, w: f64| -> String {
        let mut out = String::from("<g class=\"ondine-sequence-number\"><rect x=\"");
        push_num(&mut out, cx - w / 2.0);
        out.push_str("\" y=\"");
        push_num(&mut out, cy - BADGE_H / 2.0);
        out.push_str("\" width=\"");
        push_num(&mut out, w);
        out.push_str("\" height=\"");
        push_num(&mut out, BADGE_H);
        out.push_str("\" rx=\"");
        push_num(&mut out, BADGE_H / 2.0);
        out.push_str("\" fill=\"var(--_l)\" stroke=\"none\"/><text x=\"");
        push_num(&mut out, cx);
        out.push_str("\" y=\"");
        push_num(&mut out, cy + tiny * 0.35);
        out.push_str("\" text-anchor=\"middle\" font-size=\"");
        push_number(&mut out, tiny);
        out.push_str("\" fill=\"var(--_bg)\" stroke=\"none\">");
        let mut number = String::new();
        push_number(
            &mut number,
            numbering.index.get(i).copied().unwrap_or(f64::NAN),
        );
        out.push_str(&esc(&number));
        out.push_str("</text></g>");
        out
    };
    let frame_label_attrs = " class=\"ondine-frame-label\" fill=\"var(--_m)\" stroke=\"none\"";

    let mut y = head_bottom + 6.0;
    let mut stack: Vec<usize> = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        let r#type = m.r#type;
        let frame = frame_at.get(i).copied().flatten();
        let col_at = |id: Option<&str>| id.and_then(|id| col_of.get(id).copied());

        if r#type == Some(line::ACTIVE_START) {
            if let Some(col) = col_at(m.from.as_deref()).and_then(|c| cols.get_mut(c)) {
                col.activations.push(y);
            }
        } else if r#type == Some(line::ACTIVE_END) {
            if let Some(col) = col_at(m.from.as_deref()).and_then(|c| cols.get_mut(c))
                && !col.activations.is_empty()
            {
                y = y.max(end_activation(col, y, &mut bounds, &mut activations_out));
            }
        } else if let Some(at) = frame
            && frames
                .get(at)
                .is_some_and(|f| f.dividers.first() == Some(&i))
        {
            stack.push(at);
            if let Some(frame) = frames.get_mut(at) {
                if frame.kind == "rect" {
                    frame.top = y + 8.0;
                    y = frame.top;
                } else {
                    frame.top = y + 12.0;
                    y = frame.top + TAB_H.max(frame.title.height + 2.0);
                }
            }
        } else if let Some(at) = frame {
            stack.pop();
            let Some(frame) = frames.get(at) else {
                continue;
            };
            let bottom = y + 12.0;
            let x = num(frame.x0);
            let w = num(frame.x1 - frame.x0);
            let h = num(bottom - frame.top);
            let top = num(frame.top);
            if frame.kind == "rect" {
                let fill = if frame.fill.is_empty() {
                    String::new()
                } else {
                    resolve_style(&[["fill:", &frame.fill].concat()]).shape
                };
                rects_out.push(
                    [
                        "<rect class=\"ondine-rect\" x=\"",
                        &x,
                        "\" y=\"",
                        &top,
                        "\" width=\"",
                        &w,
                        "\" height=\"",
                        &h,
                        "\" rx=\"",
                        RADIUS,
                        "\"",
                        if fill.is_empty() {
                            " fill-opacity=\"0.5\""
                        } else {
                            ""
                        },
                        &fill,
                        "/>",
                    ]
                    .concat(),
                );
            } else {
                let mut inner = [
                    "<rect x=\"",
                    &x,
                    "\" y=\"",
                    &top,
                    "\" width=\"",
                    &w,
                    "\" height=\"",
                    &h,
                    "\" rx=\"",
                    RADIUS,
                    "\" fill=\"none\"/>",
                ]
                .concat();
                for (k, &dy) in frame.divider_y.iter().enumerate() {
                    let empty = Label::default();
                    let label = frame.divider_labels.get(k).unwrap_or(&empty);
                    inner.push_str("<path d=\"M");
                    inner.push_str(&x);
                    inner.push(',');
                    push_num(&mut inner, dy);
                    inner.push('H');
                    push_num(&mut inner, frame.x1);
                    inner.push('"');
                    inner.push_str(DASH);
                    inner.push_str("/>");
                    inner.push_str(&backed(
                        label,
                        frame.x0 + 8.0 + label.width / 2.0,
                        dy + 2.0 + label.height / 2.0,
                        frame_label_attrs,
                    ));
                }
                inner.push_str("<rect x=\"");
                inner.push_str(&x);
                inner.push_str("\" y=\"");
                inner.push_str(&top);
                inner.push_str("\" width=\"");
                push_num(&mut inner, frame.tab_w);
                inner.push_str("\" height=\"");
                push_num(&mut inner, TAB_H);
                inner.push_str("\" rx=\"");
                inner.push_str(RADIUS);
                inner.push_str("\" fill=\"var(--_s)\"/><text class=\"ondine-frame-kind\" x=\"");
                push_num(&mut inner, frame.x0 + frame.tab_w / 2.0);
                inner.push_str("\" y=\"");
                push_num(&mut inner, frame.top + TAB_H / 2.0 + tiny * 0.35);
                inner.push_str("\" text-anchor=\"middle\" font-size=\"");
                push_number(&mut inner, tiny);
                inner.push_str("\" font-weight=\"var(--_hw)\" fill=\"var(--_m)\" stroke=\"none\">");
                inner.push_str(frame.kind);
                inner.push_str("</text>");
                inner.push_str(&backed(
                    &frame.title,
                    frame.x0 + frame.tab_w + 8.0 + frame.title.width / 2.0,
                    frame.top + TAB_H.max(frame.title.height) / 2.0,
                    frame_label_attrs,
                ));
                frames_out.push_str("<g class=\"ondine-frame ondine-frame-");
                frames_out.push_str(frame.kind);
                frames_out.push_str("\">");
                frames_out.push_str(&inner);
                frames_out.push_str("</g>");
            }
            y = bottom;
        } else if matches!(
            r#type,
            Some(line::ALT_ELSE | line::PAR_AND | line::CRITICAL_OPTION)
        ) {
            if let Some(current) = stack.last().and_then(|&at| frames.get_mut(at)) {
                let height = current
                    .divider_labels
                    .get(current.divider_y.len())
                    .map_or(0.0, |label| label.height);
                current.divider_y.push(y + 12.0);
                y += 12.0 + if height > 0.0 { height } else { 0.0 };
            }
        } else if let Some(view) = views.get(i).and_then(Option::as_ref) {
            if view.kind == V::Note {
                let Some(a) = cols.get(view.a) else {
                    continue;
                };
                let h = view.label.height + 2.0 * 4.0_f64.max(note_pad - 4.0);
                let top = y + 10.0;
                let mut x = view.x;
                if m.placement == Some(placement::RIGHT_OF) {
                    x += right_edge(a, 0) - a.x;
                } else if m.placement == Some(placement::LEFT_OF) {
                    x += left_edge(a, 0) - a.x;
                }
                bounds.extend(x, x + view.w);
                notes_out.push_str("<g class=\"ondine-note\" data-id=\"i");
                notes_out.push_str(&esc_text(&m.id));
                notes_out.push_str("\"><rect x=\"");
                push_num(&mut notes_out, x);
                notes_out.push_str("\" y=\"");
                push_num(&mut notes_out, top);
                notes_out.push_str("\" width=\"");
                push_num(&mut notes_out, view.w);
                notes_out.push_str("\" height=\"");
                push_num(&mut notes_out, h);
                notes_out.push_str("\" rx=\"");
                notes_out.push_str(RADIUS);
                notes_out.push_str("\"/>");
                notes_out.push_str(&label_svg(
                    &view.label,
                    x + view.w / 2.0,
                    top + h / 2.0,
                    " class=\"ondine-label\" stroke=\"none\" fill=\"var(--_fg)\"",
                    None,
                ));
                notes_out.push_str("</g>");
                y = top + h;
                continue;
            }

            let Some(st) = r#type.and_then(style_of) else {
                continue;
            };
            let label = &view.label;
            let bw = badge_width(i);
            // The activation a `+` starts belongs to this message, so the arrow stops at its edge.
            let mut starting = 0;
            for next in messages.iter().skip(i + 1) {
                let t = next.r#type;
                if t == Some(line::ACTIVE_START) {
                    if next.from == m.to {
                        starting += 1;
                    }
                } else if !matches!(
                    t,
                    Some(
                        line::ACTIVE_END
                            | line::CENTRAL_CONNECTION
                            | line::CENTRAL_CONNECTION_REVERSE
                    )
                ) {
                    break;
                }
            }
            let up = if st.bottom { 1.0 } else { -1.0 };
            let attrs = if st.dotted { DASH } else { "" };
            let cc = m.central_connection;
            let circle_to = matches!(
                cc,
                Some(line::CENTRAL_CONNECTION | line::CENTRAL_CONNECTION_DUAL)
            );
            let circle_from = matches!(
                cc,
                Some(line::CENTRAL_CONNECTION_REVERSE | line::CENTRAL_CONNECTION_DUAL)
            );
            let circle = |out: &mut String, cx: f64, cy: f64| {
                out.push_str("<circle class=\"ondine-central\" cx=\"");
                push_num(out, cx);
                out.push_str("\" cy=\"");
                push_num(out, cy);
                out.push_str("\" r=\"");
                push_num(out, CENTRAL_R);
                out.push_str("\" fill=\"var(--_l)\" stroke=\"none\"/>");
            };
            let tail_at_end = st.at_start && !st.at_end;
            let head_trim = trim_of(st.head);
            let mut extra = String::new();
            let line_y;
            let mut path = String::new();
            let mut heads = String::new();
            let (a_at, b_at) = (view.a, view.b);

            if view.kind == V::SelfLoop {
                let Some(a) = cols.get(a_at) else {
                    continue;
                };
                let top = y + msg_gap + 0.0_f64.max((label.height - SELF_H) / 2.0);
                let bottom = top + SELF_H;
                let mut x0 = if circle_from {
                    a.x + CENTRAL_R
                } else {
                    right_edge(a, 0)
                };
                let mut x1 = if circle_to {
                    a.x + CENTRAL_R
                } else {
                    right_edge(a, starting)
                };
                let out = x0.max(x1) + SELF_W + bw / 2.0;
                if circle_from {
                    circle(&mut extra, a.x, top);
                }
                if circle_to {
                    circle(&mut extra, a.x, bottom);
                }
                if bw > 0.0 {
                    extra.push_str(&badge(
                        i,
                        if tail_at_end { x1 } else { x0 },
                        if tail_at_end { bottom } else { top },
                        bw,
                    ));
                    if tail_at_end {
                        x1 += bw / 2.0;
                    } else {
                        x0 += bw / 2.0;
                    }
                }
                // The loop leaves to the right and comes back heading left, which turns a half
                // head over.
                if st.at_start {
                    head(&mut heads, st.head, x0, top, -1.0, up);
                }
                if st.at_end {
                    head(&mut heads, st.head, x1, bottom, -1.0, -up);
                }
                let from = x0 + if st.at_start { head_trim } else { 0.0 };
                let to = x1 + if st.at_end { head_trim } else { 0.0 };
                path.push('M');
                push_num(&mut path, from);
                path.push(',');
                push_num(&mut path, top);
                path.push('H');
                if right_angles {
                    push_num(&mut path, out);
                    path.push('V');
                    push_num(&mut path, bottom);
                } else {
                    push_num(&mut path, out - 6.0);
                    path.push_str("A6,6 0 0 1 ");
                    push_num(&mut path, out);
                    path.push(',');
                    push_num(&mut path, top + 6.0);
                    path.push('V');
                    push_num(&mut path, bottom - 6.0);
                    path.push_str("A6,6 0 0 1 ");
                    push_num(&mut path, out - 6.0);
                    path.push(',');
                    push_num(&mut path, bottom);
                }
                path.push('H');
                push_num(&mut path, to);
                bounds.extend(a.x, out + 8.0 + label.width);
                if label.width > 0.0 {
                    labels_out.push_str("<g class=\"ondine-edge-label\" data-id=\"i");
                    labels_out.push_str(&esc_text(&m.id));
                    labels_out.push_str("\">");
                    labels_out.push_str(&label_svg(
                        label,
                        out + 8.0 + label.width / 2.0,
                        f64::midpoint(top, bottom),
                        "",
                        None,
                    ));
                    labels_out.push_str("</g>");
                }
                line_y = bottom;
            } else {
                let (Some(a), Some(b)) = (cols.get(a_at), cols.get(b_at)) else {
                    continue;
                };
                let dir = if b.x > a.x { 1.0 } else { -1.0 };
                let creating = b.create_at == Some(i);
                line_y = y
                    + msg_gap
                    + (if label.height > 0.0 {
                        label.height + 2.0
                    } else {
                        6.0
                    })
                    .max(if creating {
                        b.size.h / 2.0 - msg_gap + 6.0
                    } else {
                        0.0
                    });
                let mut x0 = if dir > 0.0 {
                    right_edge(a, 0)
                } else {
                    left_edge(a, 0)
                };
                let mut x1 = if dir > 0.0 {
                    left_edge(b, starting)
                } else {
                    right_edge(b, starting)
                };
                let (b_x, b_size, b_kind) = (b.x, b.size, b.kind);
                let a_x = a.x;
                if creating {
                    if let Some(b) = cols.get_mut(b_at) {
                        b.top = line_y - b_size.h / 2.0;
                        b.life_start = b.top + b_size.h;
                    }
                    x1 = b_x
                        - dir
                            * if is_figure(b_kind) {
                                18.0
                            } else {
                                b_size.w / 2.0 + 1.0
                            };
                }
                if circle_from {
                    circle(&mut extra, a_x, line_y);
                    x0 = a_x + dir * CENTRAL_R;
                }
                if circle_to {
                    circle(&mut extra, b_x, line_y);
                    x1 = b_x - dir * CENTRAL_R;
                }
                if bw > 0.0 {
                    // The number sits on the lifeline at the tail of the arrow, past a connection
                    // circle if there is one.
                    if tail_at_end {
                        let cx = if circle_to {
                            x1 - dir * (bw / 2.0 + 2.0)
                        } else {
                            x1
                        };
                        extra.push_str(&badge(i, cx, line_y, bw));
                        x1 = cx - (dir * bw) / 2.0;
                    } else {
                        let cx = if circle_from {
                            x0 + dir * (bw / 2.0 + 2.0)
                        } else {
                            x0
                        };
                        extra.push_str(&badge(i, cx, line_y, bw));
                        x0 = cx + (dir * bw) / 2.0;
                    }
                }
                if st.at_start {
                    head(&mut heads, st.head, x0, line_y, -dir, up * dir);
                }
                if st.at_end {
                    head(&mut heads, st.head, x1, line_y, dir, up * dir);
                }
                let from = x0 + if st.at_start { dir * head_trim } else { 0.0 };
                let to = x1 - if st.at_end { dir * head_trim } else { 0.0 };
                path.push('M');
                push_num(&mut path, from);
                path.push(',');
                push_num(&mut path, line_y);
                path.push('H');
                push_num(&mut path, to);
                if label.width > 0.0 {
                    let l = x0.min(x1);
                    let r = x0.max(x1);
                    let cx = match align {
                        -1 => l + msg_pad + label.width / 2.0,
                        1 => r - msg_pad - label.width / 2.0,
                        _ => f64::midpoint(l, r),
                    };
                    bounds.extend(cx - label.width / 2.0, cx + label.width / 2.0);
                    labels_out.push_str("<g class=\"ondine-edge-label\" data-id=\"i");
                    labels_out.push_str(&esc_text(&m.id));
                    labels_out.push_str("\">");
                    labels_out.push_str(&backed(label, cx, line_y - 4.0 - label.height / 2.0, ""));
                    labels_out.push_str("</g>");
                }
                if creating {
                    y = line_y + b_size.h / 2.0 - 6.0;
                }
            }
            edges_out.push_str("<g class=\"ondine-edge ondine-message\" data-id=\"i");
            edges_out.push_str(&esc_text(&m.id));
            edges_out.push_str("\"><path d=\"");
            edges_out.push_str(&path);
            edges_out.push('"');
            edges_out.push_str(attrs);
            edges_out.push_str("/>");
            edges_out.push_str(&heads);
            edges_out.push_str(&extra);
            edges_out.push_str("</g>");
            y = y.max(line_y);
            if cols.get(a_at).is_some_and(|a| a.destroy_at == Some(i))
                && let Some(a) = cols.get_mut(a_at)
            {
                y = y.max(destroy(a, line_y, &mut crosses_out));
            }
            if b_at != a_at
                && cols.get(b_at).is_some_and(|b| b.destroy_at == Some(i))
                && let Some(b) = cols.get_mut(b_at)
            {
                y = y.max(destroy(b, line_y, &mut crosses_out));
            }
        }
    }

    y += 20.0;
    for col in &mut cols {
        while !col.activations.is_empty() {
            end_activation(col, y, &mut bounds, &mut activations_out);
        }
    }
    let mut bottom = y;
    if mirror {
        let mut foot_h: f64 = 0.0;
        for col in &cols {
            if col.life_end.is_some() {
                continue;
            }
            foot_h = foot_h.max(col.size.h);
            actors_out.push_str(&actor_svg(col, y, true));
        }
        bottom = y + foot_h;
    }
    let mut lifelines = String::new();
    for col in &cols {
        actors_out.push_str(&actor_svg(col, col.top, false));
        let end = col.life_end.unwrap_or(y);
        if end > col.life_start {
            lifelines.push_str("<path class=\"ondine-lifeline\" data-id=\"");
            lifelines.push_str(&esc_text(col.id));
            lifelines.push_str("\" d=\"M");
            push_num(&mut lifelines, col.x);
            lifelines.push(',');
            push_num(&mut lifelines, col.life_start);
            lifelines.push('V');
            push_num(&mut lifelines, end);
            lifelines.push_str("\"/>");
        }
    }

    let mut boxes_out = String::new();
    for run in &runs {
        let x0 = col_x(&cols, run.first) - col_w(&cols, run.first) / 2.0 - box_pad - run.grow;
        let x1 = col_x(&cols, run.last) + col_w(&cols, run.last) / 2.0 + box_pad + run.grow;
        let r#box = db.boxes.get(run.r#box);
        let fill = match r#box {
            Some(b) if b.fill != "transparent" => {
                resolve_style(&[["fill:", &b.fill].concat()]).shape
            }
            _ => String::new(),
        };
        boxes_out.push_str("<g class=\"ondine-cluster ondine-box\"");
        if let Some(name) = r#box
            .and_then(|b| b.name.as_deref())
            .filter(|name| !name.is_empty())
        {
            boxes_out.push_str(" data-id=\"");
            boxes_out.push_str(&esc_text(name));
            boxes_out.push('"');
        }
        boxes_out.push_str("><rect x=\"");
        push_num(&mut boxes_out, x0);
        boxes_out.push_str("\" y=\"0\" width=\"");
        push_num(&mut boxes_out, x1 - x0);
        boxes_out.push_str("\" height=\"");
        push_num(&mut boxes_out, bottom + box_pad);
        boxes_out.push_str("\" rx=\"");
        boxes_out.push_str(RADIUS);
        boxes_out.push_str("\" fill=\"var(--_a)\" stroke=\"var(--_b)\"");
        if fill.is_empty() {
            boxes_out.push_str(" fill-opacity=\"0.5\"");
        }
        boxes_out.push_str(&fill);
        boxes_out.push_str("/>");
        boxes_out.push_str(&label_svg(
            &run.title,
            f64::midpoint(x0, x1),
            box_pad / 2.0 + 2.0 + run.title.height / 2.0,
            " class=\"ondine-cluster-label\" fill=\"var(--_m)\"",
            None,
        ));
        boxes_out.push_str("</g>");
    }
    if !runs.is_empty() {
        bottom += box_pad;
    }

    let mut min_y = 0.0;
    let mut title_out = String::new();
    let title = text(
        db.title.as_deref(),
        size,
        false,
        if tight { title_room(options) } else { wrap_w },
        BOLD,
    );
    if bounds.min_x > bounds.max_x {
        bounds.extend(0.0, title.width.max(40.0));
    }
    if title.width > 0.0 {
        let cx = f64::midpoint(bounds.min_x, bounds.max_x);
        bounds.extend(cx - title.width / 2.0, cx + title.width / 2.0);
        min_y = -title.height - 12.0;
        title_out = label_svg(
            &title,
            cx,
            min_y + title.height / 2.0,
            " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
            None,
        );
    }

    let width = (bounds.max_x - bounds.min_x + 2.0 * pad).ceil();
    let height = (bottom - min_y + 2.0 * pad).ceil();
    let mut content = String::from("<g transform=\"translate(");
    push_num(&mut content, pad - bounds.min_x);
    content.push(',');
    push_num(&mut content, pad - min_y);
    content.push_str(")\">");
    content.push_str(&title_out);
    let group = |content: &mut String, open: &str, inner: &str| {
        if !inner.is_empty() {
            content.push_str(open);
            content.push_str(inner);
            content.push_str("</g>");
        }
    };
    let mut small_attr = String::new();
    push_number(&mut small_attr, small);
    group(
        &mut content,
        &["<g class=\"ondine-boxes\" font-size=\"", &small_attr, "\">"].concat(),
        &boxes_out,
    );
    rects_out.reverse();
    group(
        &mut content,
        "<g class=\"ondine-rects\" fill=\"var(--_a)\">",
        &rects_out.concat(),
    );
    group(
        &mut content,
        "<g class=\"ondine-lifelines\" fill=\"none\" stroke=\"var(--_b)\">",
        &lifelines,
    );
    activations_out.reverse();
    group(
        &mut content,
        "<g class=\"ondine-activations\" fill=\"var(--_s)\" stroke=\"var(--_b)\">",
        &activations_out.concat(),
    );
    group(
        &mut content,
        &[
            "<g class=\"ondine-frames\" font-size=\"",
            &small_attr,
            "\" stroke=\"var(--_b)\">",
        ]
        .concat(),
        &frames_out,
    );
    group(
        &mut content,
        "<g class=\"ondine-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">",
        &edges_out,
    );
    group(
        &mut content,
        "<g class=\"ondine-destroys\" fill=\"none\" stroke=\"var(--_l)\" stroke-width=\"1.5\" stroke-linecap=\"round\">",
        &crosses_out,
    );
    group(
        &mut content,
        &[
            "<g class=\"ondine-edge-labels\" font-size=\"",
            &small_attr,
            "\">",
        ]
        .concat(),
        &labels_out,
    );
    group(
        &mut content,
        &[
            "<g class=\"ondine-notes\" font-size=\"",
            &small_attr,
            "\" fill=\"var(--_a)\" stroke=\"var(--_b)\">",
        ]
        .concat(),
        &notes_out,
    );
    group(&mut content, "<g class=\"ondine-nodes\">", &actors_out);
    content.push_str("</g>");

    let accessible = Accessible {
        acc_descr: db.acc_descr.as_deref(),
        acc_title: db.acc_title.as_deref(),
    };
    let svg = svg_document(
        "sequence", width, height, size, options, accessible, &content,
    );
    Rendered {
        height,
        links: Vec::new(),
        svg,
        width,
    }
}
