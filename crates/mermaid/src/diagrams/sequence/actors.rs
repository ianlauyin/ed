//! Participant shapes. Port of Pele's `src/diagrams/sequence/actors.ts`.

use crate::svg::builder::{label_svg, push_num};
use crate::svg::theme::RADIUS;
use crate::text::label::Label;

const PAD_X: f64 = 16.0;
const PAD_Y: f64 = 10.0;
const GLYPH: f64 = 34.0;
const GLYPH_GAP: f64 = 4.0;
const STACK: f64 = 5.0;
const CAP: f64 = 6.0;
const SHAPE: &str = " fill=\"var(--_s)\" stroke=\"var(--_b)\"";
const LINE: &str = " fill=\"none\" stroke=\"var(--_b)\"";

const KINDS: [&str; 8] = [
    "participant",
    "actor",
    "boundary",
    "control",
    "entity",
    "database",
    "collections",
    "queue",
];

/// A participant's size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ActorSize {
    /// The height.
    pub(crate) h: f64,
    /// The width.
    pub(crate) w: f64,
}

/// The drawn kind of a participant type: the type when Pele draws it, and `participant`
/// otherwise.
pub(crate) fn actor_kind(r#type: &str) -> &'static str {
    KINDS
        .into_iter()
        .find(|kind| *kind == r#type)
        .unwrap_or("participant")
}

/// Whether `kind` is a figure, which stands above its label. The other kinds hold the label
/// inside.
pub(crate) fn is_figure(kind: &str) -> bool {
    matches!(
        kind,
        "actor" | "boundary" | "control" | "entity" | "database"
    )
}

/// The size of a participant of `kind` with `label`, at least `min_w` by `min_h` for the kinds
/// that hold their label.
pub(crate) fn actor_size(
    kind: &str,
    label: &Label,
    min_w: f64,
    min_h: f64,
    padding_x: Option<f64>,
) -> ActorSize {
    if is_figure(kind) {
        return ActorSize {
            h: GLYPH
                + if label.height > 0.0 {
                    GLYPH_GAP + label.height
                } else {
                    0.0
                },
            w: 40.0_f64.max(label.width + 8.0),
        };
    }
    let padding_x = padding_x.unwrap_or(PAD_X);
    let extra = match kind {
        "collections" => STACK,
        "queue" => 2.0 * CAP,
        _ => 0.0,
    };
    ActorSize {
        h: min_h.max(label.height + 2.0 * PAD_Y) + if kind == "collections" { STACK } else { 0.0 },
        w: min_w.max(label.width + 2.0 * padding_x) + extra,
    }
}

/// Appends `x,y`.
fn point(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Appends an attribute `name="value"` with a number value.
fn attr(out: &mut String, name: &str, value: f64) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    push_num(out, value);
    out.push('"');
}

fn circle(out: &mut String, cx: f64, cy: f64, r: f64) {
    out.push_str("<circle");
    attr(out, "cx", cx);
    attr(out, "cy", cy);
    attr(out, "r", r);
    out.push_str(SHAPE);
    out.push_str("/>");
}

fn figure(out: &mut String, kind: &str, cx: f64, top: f64) {
    match kind {
        "actor" => {
            circle(out, cx, top + 7.0, 6.0);
            out.push_str("<path d=\"M");
            point(out, cx, top + 13.0);
            out.push('V');
            push_num(out, top + 24.0);
            out.push('M');
            point(out, cx - 10.0, top + 17.0);
            out.push('H');
            push_num(out, cx + 10.0);
            out.push('M');
            point(out, cx - 8.0, top + 34.0);
            out.push('L');
            point(out, cx, top + 24.0);
            out.push('L');
            point(out, cx + 8.0, top + 34.0);
            out.push('"');
            out.push_str(LINE);
            out.push_str("/>");
        }
        "boundary" => {
            out.push_str("<path d=\"M");
            point(out, cx - 19.0, top + 6.0);
            out.push('V');
            push_num(out, top + 28.0);
            out.push('M');
            point(out, cx - 19.0, top + 17.0);
            out.push('H');
            push_num(out, cx - 8.0);
            out.push('"');
            out.push_str(LINE);
            out.push_str("/>");
            circle(out, cx + 3.0, top + 17.0, 11.0);
        }
        "control" => {
            circle(out, cx, top + 21.0, 11.0);
            out.push_str("<path d=\"M");
            point(out, cx + 5.0, top + 5.0);
            out.push('L');
            point(out, cx - 1.0, top + 10.0);
            out.push('L');
            point(out, cx + 5.0, top + 15.0);
            out.push('"');
            out.push_str(LINE);
            out.push_str("/>");
        }
        "entity" => {
            circle(out, cx, top + 15.0, 11.0);
            out.push_str("<path d=\"M");
            point(out, cx - 11.0, top + 30.0);
            out.push('H');
            push_num(out, cx + 11.0);
            out.push('"');
            out.push_str(LINE);
            out.push_str("/>");
        }
        _ => {
            let (l, r) = (cx - 14.0, cx + 14.0);
            out.push_str("<path d=\"M");
            point(out, l, top + 7.0);
            out.push('V');
            push_num(out, top + 27.0);
            out.push_str("A14,5 0 0 0 ");
            point(out, r, top + 27.0);
            out.push('V');
            push_num(out, top + 7.0);
            out.push_str("A14,5 0 0 0 ");
            point(out, l, top + 7.0);
            out.push_str("Z\"");
            out.push_str(SHAPE);
            out.push_str("/><path d=\"M");
            point(out, l, top + 7.0);
            out.push_str("A14,5 0 0 0 ");
            point(out, r, top + 7.0);
            out.push('"');
            out.push_str(LINE);
            out.push_str("/>");
        }
    }
}

fn rect(out: &mut String, x: f64, y: f64, w: f64, h: f64) {
    out.push_str("<rect");
    attr(out, "x", x);
    attr(out, "y", y);
    attr(out, "width", w);
    attr(out, "height", h);
    out.push_str(" rx=\"");
    out.push_str(RADIUS);
    out.push('"');
    out.push_str(SHAPE);
    out.push_str("/>");
}

/// Draws a participant whose bounding box has its top center at (`cx`, `top`).
pub(crate) fn draw_actor(
    out: &mut String,
    kind: &str,
    cx: f64,
    top: f64,
    size: ActorSize,
    label: &Label,
) {
    let ActorSize { h, w } = size;
    let text = " class=\"ondine-label\"";
    if is_figure(kind) {
        figure(out, kind, cx, top);
        out.push_str(&label_svg(
            label,
            cx,
            top + GLYPH + GLYPH_GAP + label.height / 2.0,
            text,
            None,
        ));
        return;
    }
    let left = cx - w / 2.0;
    if kind == "collections" {
        let bw = w - STACK;
        let bh = h - STACK;
        rect(out, left + STACK, top, bw, bh);
        rect(out, left, top + STACK, bw, bh);
        out.push_str(&label_svg(
            label,
            left + bw / 2.0,
            top + STACK + bh / 2.0,
            text,
            None,
        ));
        return;
    }
    if kind == "queue" {
        let l = left + CAP;
        let r = left + w - CAP;
        let b = top + h;
        let ry = h / 2.0;
        let arc = |out: &mut String, sweep: &str, x: f64, y: f64| {
            out.push('A');
            push_num(out, CAP);
            out.push(',');
            push_num(out, ry);
            out.push_str(" 0 0 ");
            out.push_str(sweep);
            out.push(' ');
            point(out, x, y);
        };
        out.push_str("<path d=\"M");
        point(out, l, top);
        out.push('H');
        push_num(out, r);
        arc(out, "1", r, b);
        out.push('H');
        push_num(out, l);
        arc(out, "1", l, top);
        out.push_str("Z\"");
        out.push_str(SHAPE);
        out.push_str("/><path d=\"M");
        point(out, r, top);
        arc(out, "0", r, b);
        out.push('"');
        out.push_str(LINE);
        out.push_str("/>");
        out.push_str(&label_svg(label, cx - CAP / 2.0, top + h / 2.0, text, None));
        return;
    }
    rect(out, left, top, w, h);
    out.push_str(&label_svg(label, cx, top + h / 2.0, text, None));
}
