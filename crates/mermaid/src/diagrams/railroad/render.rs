//! Draws a railroad diagram: each rule's name above a track through its definition. Port of
//! Pele's `src/diagrams/railroad/render.ts`.

use super::common::fold;
use super::types::{RailroadModel, RailroadNode};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{tighten, title_room};
use crate::svg::builder::{esc, esc_text, js_round, label_svg, num, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{FONT_MONO, RADIUS};
use crate::text::entities::decode;
use crate::text::label::layout_label;
use crate::text::measure::{self, BOLD, ITALIC, MONO};
use crate::yaml::{Value, push_number};

/// What a laid-out element draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Alternatives stacked between two rails.
    Choice,
    /// An element with a track over it that skips it.
    Bypass,
    /// A sequence folded into rows.
    Folded,
    /// An element with a track under it that goes back.
    Loop,
    /// A reference to another rule.
    NonTerminal,
    /// Elements one after another.
    Sequence,
    /// Text that stands for something the grammar does not spell out.
    Special,
    /// Literal text.
    Terminal,
}

/// A laid-out element. The track enters at the left edge and leaves at the right edge on one
/// baseline, with `up` above it and `down` below it.
#[derive(Clone, Debug)]
struct LaidOut {
    /// How far the element reaches below the baseline.
    down: f64,
    /// The name a non-terminal refers to.
    id: String,
    /// The element's parts, as indices into the layout arena.
    kids: Vec<usize>,
    /// What the element draws.
    kind: Kind,
    /// How far a bypass rises above the baseline, or a loop drops below it.
    reach: f64,
    /// A leaf's text, or a loop's count.
    text: String,
    /// How far the element reaches above the baseline.
    up: f64,
    /// The width.
    width: f64,
}

/// Sizes the layout shares.
struct Metrics {
    /// Corner radius of the track's turns.
    arc: f64,
    /// Space between elements in a sequence.
    gap: f64,
    /// Height of a leaf.
    height: f64,
    /// Space beside a leaf's text.
    padding: f64,
    /// Space between stacked branches.
    sep: f64,
    /// Font size of leaves.
    size: f64,
}

/// Space before and after a rule's track.
const LEAD: f64 = 12.0;
/// Space between a rule's name and its track.
const HEAD_GAP: f64 = 6.0;
/// Space between rules.
const RULE_GAP: f64 = 18.0;
/// Font size of a loop's count.
const COUNT_SIZE: f64 = 12.0;
/// Length of an arrowhead.
const ARROW: f64 = 7.0;

/// The `railroad` section of `config`, when it is a mapping.
fn section(config: &Value) -> Option<&[(String, Value)]> {
    let Value::Object(entries) = config else {
        return None;
    };
    match entries.iter().find(|(key, _)| key == "railroad") {
        Some((_, Value::Object(entries))) => Some(entries),
        _ => None,
    }
}

/// The `railroad` option `key` when it is a number at least 0, capped at `max`, or `fallback`.
fn option(config: &Value, key: &str, fallback: f64, max: f64) -> f64 {
    match section(config).and_then(|entries| entries.iter().find(|(own, _)| own == key)) {
        Some((_, Value::Number(value))) if *value >= 0.0 => value.min(max),
        _ => fallback,
    }
}

/// Text is literal here: a terminal such as "<br>" or "*" is itself, not markup. Mermaid's `#35;`
/// codes still stand for characters, and escapes that would be invisible are written out.
fn shown(text: &str) -> String {
    let decoded = if text.contains("\u{b6}\u{df}") {
        let mut escaped = String::with_capacity(text.len() + 8);
        for c in text.chars() {
            if c == '&' {
                escaped.push_str("&amp;");
            } else {
                escaped.push(c);
            }
        }
        decode(&escaped).into_owned()
    } else {
        text.to_owned()
    };
    if !decoded.contains(['\n', '\r', '\t']) {
        return decoded;
    }
    let mut out = String::with_capacity(decoded.len() + 4);
    for c in decoded.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// `Math.ceil(value / 2) * 2`.
fn even(value: f64) -> f64 {
    (value / 2.0).ceil() * 2.0
}

/// The arena of laid-out elements.
type Boxes = Vec<LaidOut>;

/// Adds `item` to `boxes`, returning its index.
fn add(boxes: &mut Boxes, item: LaidOut) -> usize {
    boxes.push(item);
    boxes.len() - 1
}

/// The element at `index`, or an empty one.
fn get(boxes: &Boxes, index: usize) -> LaidOut {
    boxes.get(index).cloned().unwrap_or(LaidOut {
        down: 0.0,
        id: String::new(),
        kids: Vec::new(),
        kind: Kind::Sequence,
        reach: 0.0,
        text: String::new(),
        up: 0.0,
        width: 0.0,
    })
}

/// A leaf of `kind` showing `text`.
fn leaf(boxes: &mut Boxes, kind: Kind, text: String, id: String, style: u8, m: &Metrics) -> usize {
    let half = m.height / 2.0;
    let width = even(measure::width(&text, m.size, style) + 2.0 * m.padding).max(m.height);
    add(
        boxes,
        LaidOut {
            down: half,
            id,
            kids: Vec::new(),
            kind,
            reach: 0.0,
            text,
            up: half,
            width,
        },
    )
}

/// The count under a loop, or empty for one or more.
fn count(min: f64, max: f64) -> String {
    if max == f64::INFINITY && min <= 1.0 {
        return String::new();
    }
    let text = |value: f64| {
        if value.is_finite() {
            let mut out = String::new();
            push_number(&mut out, value);
            out
        } else {
            String::from("∞")
        }
    };
    #[expect(clippy::float_cmp, reason = "Pele compares the counts exactly")]
    if min == max {
        let mut out = text(min);
        out.push('×');
        return out;
    }
    let mut out = String::new();
    if max == f64::INFINITY {
        out.push('≥');
        out.push_str(&text(min));
    } else if min == 0.0 {
        out.push('≤');
        out.push_str(&text(max));
    } else {
        out.push_str(&text(min));
        out.push('–');
        out.push_str(&text(max));
    }
    out.push('×');
    out
}

/// Where the next branch of a choice sits below the baseline, given the one above it.
fn below(offset: f64, above: &LaidOut, branch: &LaidOut, m: &Metrics) -> f64 {
    (offset + above.down + m.sep + branch.up).max(2.0 * m.arc)
}

/// A track over `kid` that skips it.
fn bypass(boxes: &mut Boxes, kid: usize, m: &Metrics) -> usize {
    let inner = get(boxes, kid);
    let reach = (inner.up + m.sep).max(2.0 * m.arc);
    add(
        boxes,
        LaidOut {
            down: inner.down,
            id: String::new(),
            kids: vec![kid],
            kind: Kind::Bypass,
            reach,
            text: String::new(),
            up: reach,
            width: inner.width + 4.0 * m.arc,
        },
    )
}

/// A sequence or row of `kids` with no folding.
fn row(boxes: &Boxes, kids: Vec<usize>, width: f64) -> LaidOut {
    let mut up: f64 = 0.0;
    let mut down: f64 = 0.0;
    for &kid in &kids {
        if let Some(kid) = boxes.get(kid) {
            up = up.max(kid.up);
            down = down.max(kid.down);
        }
    }
    LaidOut {
        down,
        id: String::new(),
        kids,
        kind: Kind::Sequence,
        reach: 0.0,
        text: String::new(),
        up,
        width,
    }
}

/// Lays out the definition `root` of `model` no wider than `room`, when it is finite.
fn measure(model: &RailroadModel, root: usize, m: &Metrics, room: f64, boxes: &mut Boxes) -> usize {
    let parts = |node: usize| model.nodes.get(node).map_or(&[][..], RailroadNode::parts);
    let mut budgets: Vec<(usize, f64)> = Vec::new();
    let mut pending: Vec<(usize, f64)> = vec![(root, room)];
    let mut i = 0;
    while room != f64::INFINITY
        && let Some(&(node, width)) = pending.get(i)
    {
        budgets.push((node, width));
        // A sequence that folds puts its rows inside rails, so its parts are sized for those
        // rows.
        let around = match model.nodes.get(node) {
            Some(RailroadNode::Choice(_) | RailroadNode::Optional(_)) => 4.0 * m.arc,
            Some(RailroadNode::Repetition { min, .. }) => {
                (if *min == 0.0 { 6.0 } else { 2.0 }) * m.arc
            }
            Some(RailroadNode::Sequence(_)) => 6.0 * m.arc,
            _ => 0.0,
        };
        for &child in parts(node) {
            pending.push((child, 40.0_f64.max(width - around)));
        }
        i += 1;
    }
    // Each node of a definition appears once, so the budgets are in the order nodes were met.
    budgets.sort_by_key(|&(node, _)| node);

    fold(root, parts, |node, kids: Vec<usize>| {
        let room = budgets
            .binary_search_by_key(&node, |&(node, _)| node)
            .ok()
            .and_then(|at| budgets.get(at))
            .map_or(f64::INFINITY, |&(_, width)| width);
        match model.nodes.get(node) {
            Some(RailroadNode::Terminal(value)) => {
                let text = shown(value);
                let text = if !text.is_empty() && text.bytes().all(|b| b == b' ') {
                    "␣".repeat(text.len())
                } else {
                    text
                };
                leaf(boxes, Kind::Terminal, text, String::new(), MONO, m)
            }
            Some(RailroadNode::NonTerminal(name)) => {
                leaf(boxes, Kind::NonTerminal, shown(name), name.clone(), 0, m)
            }
            Some(RailroadNode::Special(text)) => {
                let mut shown_text = String::from("? ");
                shown_text.push_str(&shown(text));
                shown_text.push_str(" ?");
                leaf(boxes, Kind::Special, shown_text, String::new(), ITALIC, m)
            }
            Some(RailroadNode::Sequence(_)) => sequence(boxes, kids, room, m),
            Some(RailroadNode::Choice(_)) => {
                let mut inner: f64 = 0.0;
                let mut offset = 0.0;
                for (i, &kid) in kids.iter().enumerate() {
                    let current = get(boxes, kid);
                    inner = inner.max(current.width);
                    if i > 0 {
                        let above = get(boxes, kids.get(i - 1).copied().unwrap_or(kid));
                        offset = below(offset, &above, &current, m);
                    }
                }
                let up = kids.first().map_or(0.0, |&kid| get(boxes, kid).up);
                let down = kids
                    .last()
                    .map_or(0.0, |&kid| offset + get(boxes, kid).down);
                add(
                    boxes,
                    LaidOut {
                        down,
                        id: String::new(),
                        kids,
                        kind: Kind::Choice,
                        reach: 0.0,
                        text: String::new(),
                        up,
                        width: inner + 4.0 * m.arc,
                    },
                )
            }
            Some(RailroadNode::Optional(_)) => {
                bypass(boxes, kids.first().copied().unwrap_or(usize::MAX), m)
            }
            Some(&RailroadNode::Repetition { min, max, .. }) => {
                let kid = kids.first().copied().unwrap_or(usize::MAX);
                #[expect(clippy::float_cmp, reason = "Pele compares the counts exactly")]
                if min == 1.0 && max == 1.0 {
                    return kid;
                }
                let inner = get(boxes, kid);
                let text = count(min, max);
                let reach = (inner.down + m.sep).max(2.0 * m.arc);
                let label = if text.is_empty() {
                    0.0
                } else {
                    even(measure::width(&text, COUNT_SIZE, 0))
                };
                let down = reach
                    + if text.is_empty() {
                        ARROW / 2.0 + 1.0
                    } else {
                        COUNT_SIZE + 6.0
                    };
                let repeated = add(
                    boxes,
                    LaidOut {
                        down,
                        id: String::new(),
                        kids: vec![kid],
                        kind: Kind::Loop,
                        reach,
                        text,
                        up: inner.up,
                        width: inner.width.max(label) + 2.0 * m.arc,
                    },
                );
                if min == 0.0 {
                    bypass(boxes, repeated, m)
                } else {
                    repeated
                }
            }
            None => usize::MAX,
        }
    })
}

/// A sequence of `kids`, folded into rows when it is wider than `room`.
fn sequence(boxes: &mut Boxes, kids: Vec<usize>, room: f64, m: &Metrics) -> usize {
    #[expect(
        clippy::cast_precision_loss,
        reason = "part counts stay far below 2^52"
    )]
    let mut width = kids.len().saturating_sub(1) as f64 * m.gap;
    for &kid in &kids {
        width += get(boxes, kid).width;
    }
    if width <= room || kids.len() < 2 {
        let whole = row(boxes, kids, width);
        return add(boxes, whole);
    }

    let mut rows: Vec<usize> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_width = 0.0;
    let limit = 40.0_f64.max(room - 6.0 * m.arc);
    for &kid in &kids {
        let kid_width = get(boxes, kid).width;
        if !current.is_empty() && current_width + m.gap + kid_width > limit {
            let full = row(boxes, std::mem::take(&mut current), current_width);
            rows.push(add(boxes, full));
            current_width = 0.0;
        }
        current_width += if current.is_empty() { 0.0 } else { m.gap } + kid_width;
        current.push(kid);
    }
    let last = row(boxes, current, current_width);
    rows.push(add(boxes, last));
    if rows.len() < 2 {
        let whole = row(boxes, kids, width);
        return add(boxes, whole);
    }

    let mut folded_width: f64 = 0.0;
    let mut offset = 0.0;
    for (i, &each) in rows.iter().enumerate() {
        let current = get(boxes, each);
        folded_width = folded_width.max(current.width);
        if i > 0 {
            let above = get(boxes, rows.get(i - 1).copied().unwrap_or(each));
            offset += above.down + m.sep.max(2.0 * m.arc) + current.up;
        }
    }
    let up = rows.first().map_or(0.0, |&first| get(boxes, first).up);
    let down = offset + rows.last().map_or(0.0, |&last| get(boxes, last).down);
    add(
        boxes,
        LaidOut {
            down,
            id: String::new(),
            kids: rows,
            kind: Kind::Folded,
            reach: 0.0,
            text: String::new(),
            up,
            width: folded_width + 6.0 * m.arc,
        },
    )
}

/// Draws `model` with the `railroad` section of `config`.
pub(crate) fn render(model: &RailroadModel, config: &Value, options: &RenderOptions) -> Rendered {
    tighten(options, |tight| draw(model, config, options, tight))
}

/// Reverses `list` from `from` to its end.
fn reverse<T>(list: &mut [T], from: usize) {
    if let Some(tail) = list.get_mut(from..) {
        tail.reverse();
    }
}

/// The paths and text drawn for one rule.
struct Drawing<'a> {
    /// Arrowheads.
    arrows: String,
    /// The layout arena.
    boxes: &'a Boxes,
    /// Leaves and counts.
    nodes: String,
    /// The track.
    track: String,
}

impl Drawing<'_> {
    /// A straight piece of track, when it has length.
    fn line(&mut self, x1: f64, y: f64, x2: f64) {
        if x2 > x1 {
            self.track.push('M');
            push_num(&mut self.track, x1);
            self.track.push(',');
            push_num(&mut self.track, y);
            self.track.push('H');
            push_num(&mut self.track, x2);
        }
    }
}

/// The turns of the track, as relative arcs.
struct Turns {
    /// From running east to running north.
    east_to_north: String,
    /// From running east to running south.
    east_to_south: String,
    /// From running east to running south, turning back west.
    loop_down: String,
    /// From running west to running north, turning back.
    loop_up: String,
    /// From running north to running east.
    north_to_east: String,
    /// From running south to running east.
    south_to_east: String,
}

impl Turns {
    /// The turns of radius `arc`.
    fn new(arc: f64) -> Self {
        let a = num(arc);
        let turn = |sweep: &str, dx: &str, dy: &str| {
            let mut out = String::from("a");
            out.push_str(&a);
            out.push(',');
            out.push_str(&a);
            out.push_str(" 0 0 ");
            out.push_str(sweep);
            out.push(' ');
            out.push_str(dx);
            out.push_str(&a);
            out.push(',');
            out.push_str(dy);
            out.push_str(&a);
            out
        };
        Self {
            east_to_north: turn("0", "", "-"),
            east_to_south: turn("1", "", ""),
            loop_down: turn("0", "-", ""),
            loop_up: turn("0", "-", "-"),
            north_to_east: turn("1", "", "-"),
            south_to_east: turn("0", "", ""),
        }
    }
}

/// Appends `M{x},{y}`.
fn push_move(out: &mut String, x: f64, y: f64) {
    out.push('M');
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

/// Appends a letter and a number, such as `H12`.
fn push_step(out: &mut String, letter: char, value: f64) {
    out.push(letter);
    push_num(out, value);
}

/// Leaf drawing settings.
struct Leaves<'a> {
    /// Half a leaf's height.
    box_top: f64,
    /// The leaf height.
    height: f64,
    /// The stroke width attribute, or empty.
    stroke: &'a str,
    /// How far text sits below the baseline.
    text_shift: f64,
}

/// Draws the laid-out element `root` with its track entering at (`left`, `baseline`). The walk
/// keeps its own stack, so deep definitions do not overflow.
fn place(
    drawing: &mut Drawing<'_>,
    root: usize,
    left: f64,
    baseline: f64,
    m: &Metrics,
    turns: &Turns,
    leaves: &Leaves<'_>,
) {
    let arc = m.arc;
    let mut stack: Vec<usize> = vec![root];
    let mut at: Vec<f64> = vec![left, baseline];
    while let Some(index) = stack.pop() {
        let (Some(y), Some(x)) = (at.pop(), at.pop()) else {
            break;
        };
        let item = get(drawing.boxes, index);
        let kids = &item.kids;
        let right = x + item.width;
        match item.kind {
            Kind::Sequence => {
                let mut kx = right;
                for (i, &kid) in kids.iter().enumerate().rev() {
                    kx -= get(drawing.boxes, kid).width;
                    stack.push(kid);
                    at.push(kx);
                    at.push(y);
                    if i > 0 {
                        drawing.line(kx - m.gap, y, kx);
                    }
                    kx -= m.gap;
                }
            }
            Kind::Folded => {
                let start = x + 2.0 * arc;
                let from = stack.len();
                let mut offset = 0.0;
                drawing.line(x, y, start);
                let step = m.sep.max(2.0 * arc);
                for (i, &kid) in kids.iter().enumerate() {
                    let current = get(drawing.boxes, kid);
                    let ky = y + offset;
                    stack.push(kid);
                    at.push(ky);
                    at.push(start);
                    if let Some(&next) = kids.get(i + 1) {
                        let next = get(drawing.boxes, next);
                        let next_offset = offset + current.down + step + next.up;
                        let mid = y + offset + current.down + step / 2.0;
                        push_move(&mut drawing.track, start + current.width, ky);
                        push_step(&mut drawing.track, 'H', right - 2.0 * arc);
                        push_step(&mut drawing.track, 'V', mid);
                        push_step(&mut drawing.track, 'H', x + arc);
                        push_step(&mut drawing.track, 'V', y + next_offset);
                        push_step(&mut drawing.track, 'H', start);
                        push_move(
                            &mut drawing.arrows,
                            start - ARROW,
                            y + next_offset - ARROW / 2.0,
                        );
                        drawing.arrows.push_str("l7,3.5l-7,3.5z");
                        offset = next_offset;
                    } else {
                        push_move(&mut drawing.track, start + current.width, ky);
                        push_step(&mut drawing.track, 'H', right - arc);
                        push_step(&mut drawing.track, 'V', y);
                        push_step(&mut drawing.track, 'H', right);
                    }
                }
                reverse(&mut stack, from);
                reverse(&mut at, 2 * from);
            }
            Kind::Choice => {
                let inner = item.width - 4.0 * arc;
                let rail_in = x + arc;
                let rail_out = right - arc;
                let from = stack.len();
                let mut offset = 0.0;
                for (i, &kid) in kids.iter().enumerate() {
                    let current = get(drawing.boxes, kid);
                    if i > 0 {
                        let above = get(drawing.boxes, kids.get(i - 1).copied().unwrap_or(kid));
                        offset = below(offset, &above, &current, m);
                    }
                    let ky = y + offset;
                    let kx = x + 2.0 * arc + (inner - current.width) / 2.0;
                    let last = i + 1 == kids.len();
                    if i == 0 {
                        drawing.line(x, y, kx);
                        drawing.line(kx + current.width, y, right);
                    } else {
                        let t = &mut drawing.track;
                        if last {
                            push_move(t, x, y);
                            t.push_str(&turns.east_to_south);
                            push_step(t, 'V', ky - arc);
                        } else {
                            push_move(t, rail_in, ky - arc);
                        }
                        t.push_str(&turns.south_to_east);
                        if kx > rail_in + arc {
                            push_step(t, 'H', kx);
                        }
                        push_move(t, kx + current.width, ky);
                        if kx > rail_in + arc {
                            push_step(t, 'H', rail_out - arc);
                        }
                        t.push_str(&turns.east_to_north);
                        if last {
                            push_step(t, 'V', y + arc);
                            t.push_str(&turns.north_to_east);
                        }
                    }
                    stack.push(kid);
                    at.push(ky);
                    at.push(kx);
                }
                // Reversed, so that the branches come off the stack top to bottom.
                reverse(&mut stack, from);
                reverse(&mut at, 2 * from);
            }
            Kind::Bypass => {
                let kid = kids.first().copied().unwrap_or(usize::MAX);
                let inner = get(drawing.boxes, kid);
                let kx = x + 2.0 * arc;
                drawing.line(x, y, kx);
                drawing.line(kx + inner.width, y, right);
                let t = &mut drawing.track;
                push_move(t, x, y);
                t.push_str(&turns.east_to_north);
                push_step(t, 'V', y - item.reach + arc);
                t.push_str(&turns.north_to_east);
                push_step(t, 'H', right - 2.0 * arc);
                t.push_str(&turns.east_to_south);
                push_step(t, 'V', y - arc);
                t.push_str(&turns.south_to_east);
                stack.push(kid);
                at.push(kx);
                at.push(y);
            }
            Kind::Loop => {
                let kid = kids.first().copied().unwrap_or(usize::MAX);
                let inner = get(drawing.boxes, kid);
                let kx = x + (item.width - inner.width) / 2.0;
                let loop_y = y + item.reach;
                let middle = x + item.width / 2.0;
                drawing.line(x, y, kx);
                drawing.line(kx + inner.width, y, right);
                let t = &mut drawing.track;
                push_move(t, x + arc, y);
                t.push_str(&turns.loop_down);
                push_step(t, 'V', loop_y - arc);
                t.push_str(&turns.south_to_east);
                push_step(t, 'H', right - arc);
                t.push_str(&turns.east_to_north);
                push_step(t, 'V', y + arc);
                t.push_str(&turns.loop_up);
                push_move(&mut drawing.arrows, middle - ARROW / 2.0, loop_y);
                drawing.arrows.push_str("l7,-3.5v7z");
                if !item.text.is_empty() {
                    drawing
                        .nodes
                        .push_str("<text class=\"ondine-edge-label ondine-count\" x=\"");
                    push_num(&mut drawing.nodes, middle);
                    drawing.nodes.push_str("\" y=\"");
                    push_num(&mut drawing.nodes, loop_y + COUNT_SIZE + 3.0);
                    drawing
                        .nodes
                        .push_str("\" font-size=\"12\" fill=\"var(--_m)\">");
                    drawing.nodes.push_str(&esc(&item.text));
                    drawing.nodes.push_str("</text>");
                }
                stack.push(kid);
                at.push(kx);
                at.push(y);
            }
            Kind::Terminal | Kind::NonTerminal | Kind::Special => {
                push_leaf(&mut drawing.nodes, &item, x, y, leaves);
            }
        }
    }
}

/// Draws a leaf whose track enters at (`x`, `y`).
fn push_leaf(out: &mut String, item: &LaidOut, x: f64, y: f64, leaves: &Leaves<'_>) {
    let text = &item.text;
    let space = if text.starts_with(' ') || text.ends_with(' ') || text.contains("  ") {
        " xml:space=\"preserve\""
    } else {
        ""
    };
    let mut shape = String::from("<rect x=\"");
    push_num(&mut shape, x);
    shape.push_str("\" y=\"");
    push_num(&mut shape, y - leaves.box_top);
    shape.push_str("\" width=\"");
    push_num(&mut shape, item.width);
    shape.push_str("\" height=\"");
    push_num(&mut shape, leaves.height);
    shape.push('"');
    let mut label = String::from("<text x=\"");
    push_num(&mut label, x + item.width / 2.0);
    label.push_str("\" y=\"");
    push_num(&mut label, y + leaves.text_shift);
    label.push('"');
    label.push_str(space);

    match item.kind {
        Kind::Terminal => {
            out.push_str("<g class=\"ondine-node ondine-terminal\">");
            out.push_str(&shape);
            out.push_str(" rx=\"");
            push_num(out, leaves.box_top);
            out.push_str("\" fill=\"var(--_s)\" stroke=\"var(--_b)\"");
            out.push_str(leaves.stroke);
            out.push_str("/>");
            out.push_str(&label);
            out.push_str(" font-family=\"");
            out.push_str(FONT_MONO);
            out.push_str("\">");
            out.push_str(&esc(text));
            out.push_str("</text></g>");
        }
        Kind::NonTerminal => {
            out.push_str("<g class=\"ondine-node ondine-nonterminal\" data-id=\"");
            out.push_str(&esc_text(&item.id));
            out.push_str("\">");
            out.push_str(&shape);
            out.push_str(" rx=\"");
            out.push_str(RADIUS);
            out.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_b)\"");
            out.push_str(leaves.stroke);
            out.push_str("/>");
            out.push_str(&label);
            out.push('>');
            out.push_str(&esc(text));
            out.push_str("</text></g>");
        }
        _ => {
            // The shown text is `? {text} ?`, with ASCII marks at both ends.
            let inner = text.get(2..text.len().saturating_sub(2)).unwrap_or("");
            out.push_str("<g class=\"ondine-node ondine-special\">");
            out.push_str(&shape);
            out.push_str(" rx=\"");
            out.push_str(RADIUS);
            out.push_str("\" fill=\"var(--_bg)\" stroke=\"var(--_b)\" stroke-dasharray=\"4 3\"");
            out.push_str(leaves.stroke);
            out.push_str("/>");
            out.push_str(&label);
            out.push_str(" font-style=\"italic\"><tspan fill=\"var(--_m)\">? </tspan>");
            out.push_str(&esc(inner));
            out.push_str("<tspan fill=\"var(--_m)\"> ?</tspan></text></g>");
        }
    }
}

/// Draws `model` with room to spare, or `tight`ly within the host's width.
fn draw(model: &RailroadModel, config: &Value, options: &RenderOptions, tight: bool) -> Rendered {
    let base = options.font_size.unwrap_or(16.0);
    let font_size = option(config, "fontSize", 0.0, 200.0);
    let size = if font_size == 0.0 {
        js_round(base * 0.875)
    } else {
        font_size
    };
    let head_size = if font_size == 0.0 { base } else { font_size };
    let padding = option(config, "padding", -1.0, 200.0);
    let line_height = js_round(size * 1.5);
    let m = Metrics {
        arc: option(config, "arcRadius", 10.0, 200.0).min(if tight { 6.0 } else { f64::INFINITY }),
        gap: option(config, "horizontalSeparation", 16.0, 500.0).min(if tight {
            12.0
        } else {
            f64::INFINITY
        }),
        height: even(line_height + if padding < 0.0 { 9.0 } else { 2.0 * padding }),
        padding: if padding < 0.0 { 12.0 } else { padding },
        sep: option(config, "verticalSeparation", 10.0, 500.0),
        size,
    };
    let hidden = section(config).is_some_and(|entries| {
        entries
            .iter()
            .any(|(key, value)| key == "showMarkers" && *value == Value::Bool(false))
    });
    let radius = if hidden {
        0.0
    } else {
        option(config, "markerRadius", 4.0, 50.0)
    };
    let stroke_width = option(config, "strokeWidth", 0.0, 20.0);
    let stroke = if stroke_width > 0.0 {
        let mut out = String::from(" stroke-width=\"");
        push_num(&mut out, stroke_width);
        out.push('"');
        out
    } else {
        String::new()
    };
    let pad = options.padding.unwrap_or(8.0);
    let turns = Turns::new(m.arc);

    let title = layout_label(
        model.title.as_deref(),
        false,
        base,
        if tight { title_room(options) } else { 4000.0 },
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let head_height = js_round(head_size * 1.5);
    let leaves = Leaves {
        box_top: m.height / 2.0,
        height: m.height,
        stroke: &stroke,
        text_shift: size * 0.35,
    };

    let mut rules = String::new();
    let mut width = title.width;
    let mut top = pad + title_height;
    let mut bottom = pad + title.height;
    for rule in &model.rules {
        let room = if tight {
            80.0_f64.max(
                options.max_width.unwrap_or(320.0) - 2.0 * pad - 4.0 * radius - 2.0 * LEAD - 1.0,
            )
        } else {
            f64::INFINITY
        };
        let mut boxes = Boxes::new();
        let root = measure(model, rule.definition, &m, room, &mut boxes);
        let laid = get(&boxes, root);
        let name = shown(&rule.name);
        let baseline = top + head_height + HEAD_GAP + laid.up;
        let start = pad + 2.0 * radius;
        let left = start + LEAD;
        let end = left + laid.width + LEAD;
        let mut drawing = Drawing {
            arrows: String::new(),
            boxes: &boxes,
            nodes: String::new(),
            track: String::new(),
        };
        drawing.line(start, baseline, left);
        drawing.line(left + laid.width, baseline, end);
        place(&mut drawing, root, left, baseline, &m, &turns, &leaves);

        rules.push_str("<g class=\"ondine-rule\" data-id=\"");
        rules.push_str(&esc_text(&rule.name));
        rules.push_str("\"><text class=\"ondine-rule-name\" x=\"");
        push_num(&mut rules, pad);
        rules.push_str("\" y=\"");
        push_num(&mut rules, top + head_height / 2.0 + head_size * 0.35);
        rules.push_str("\" font-size=\"");
        push_num(&mut rules, head_size);
        rules.push_str("\" font-weight=\"var(--_hw)\" text-anchor=\"start\">");
        rules.push_str(&esc(&name));
        rules.push_str("</text><path class=\"ondine-edge ondine-track\" d=\"");
        rules.push_str(&drawing.track);
        rules.push_str("\" fill=\"none\" stroke=\"var(--_l)\"");
        rules.push_str(&stroke);
        rules.push_str("/>");
        if !drawing.arrows.is_empty() {
            rules.push_str("<path class=\"ondine-marker\" d=\"");
            rules.push_str(&drawing.arrows);
            rules.push_str("\" fill=\"var(--_l)\"/>");
        }
        if radius > 0.0 {
            rules.push_str("<circle class=\"ondine-marker ondine-start\" cx=\"");
            push_num(&mut rules, pad + radius);
            rules.push_str("\" cy=\"");
            push_num(&mut rules, baseline);
            rules.push_str("\" r=\"");
            push_num(&mut rules, radius);
            rules.push_str(
                "\" fill=\"var(--_l)\"/><circle class=\"ondine-marker ondine-end\" cx=\"",
            );
            push_num(&mut rules, end + radius);
            rules.push_str("\" cy=\"");
            push_num(&mut rules, baseline);
            rules.push_str("\" r=\"");
            push_num(&mut rules, radius);
            rules.push_str("\" fill=\"var(--_l)\"/>");
        }
        rules.push_str(&drawing.nodes);
        rules.push_str("</g>");
        width = width
            .max(end + 2.0 * radius - pad)
            .max(measure::width(&name, head_size, BOLD));
        bottom = baseline + laid.down;
        top = bottom + m.sep + RULE_GAP;
    }

    let total_width = (width + 2.0 * pad).ceil();
    let total_height = (bottom + pad).ceil();
    let mut content = label_svg(
        &title,
        total_width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    if !rules.is_empty() {
        content.push_str("<g class=\"ondine-rules\" transform=\"translate(0.5,0.5)\" font-size=\"");
        push_num(&mut content, size);
        content.push_str("\" text-anchor=\"middle\">");
        content.push_str(&rules);
        content.push_str("</g>");
    }

    Rendered {
        height: total_height,
        links: Vec::new(),
        svg: svg_document(
            "railroad",
            total_width,
            total_height,
            base,
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
