//! Draws a git graph: lanes for branches, commits along them, and connectors for merges and
//! cherry-picks. Port of Pele's `src/diagrams/git/render.ts`.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_1_SQRT_2;

use super::db::{CHERRY_PICK, GitCommit, GitModel, HIGHLIGHT, MERGE, REVERSE};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit::plain_label;
use crate::diagrams::common::fit_width::turn_to_fit;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{RADIUS, series_color};
use crate::text::label::{Label, layout_label};
use crate::text::measure::BOLD;
use crate::yaml::{Value, push_number};

/// Radius of a commit dot.
const DOT: f64 = 7.0;
/// Least distance between commits along a lane.
const STEP: f64 = 44.0;
/// Least distance between lanes.
const LANE: f64 = 44.0;
/// Length of a curve between lanes. Shorter than a step, so a curve between lanes never runs
/// through a commit on a lane it crosses.
const CURVE: f64 = 36.0;
/// Space between things that would otherwise touch.
const GAP: f64 = 8.0;
/// Space before the first commit, where branch names sit.
const LABEL_GAP: f64 = 20.0;
/// Length of a lane past its last commit.
const TAIL: f64 = 20.0;
/// Depth of a tag's notch.
const NOTCH: f64 = 7.0;
/// Space between a commit and its tags.
const TAG_GAP: f64 = 6.0;
/// The cosine of 45 degrees, for rotated ids.
const HALF: f64 = FRAC_1_SQRT_2;
/// Class suffixes by commit type.
const CLASSES: [&str; 5] = ["normal", "reverse", "highlight", "merge", "cherry-pick"];

/// A commit as drawn.
struct View<'a> {
    commit: &'a GitCommit,
    index: usize,
    label: Option<Label>,
    lane: usize,
    m: f64,
    tags: Vec<Label>,
}

/// A property of a YAML mapping.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

fn flag(config: &Value, key: &str, fallback: bool) -> bool {
    match property(config, "gitGraph").and_then(|git| property(git, key)) {
        Some(Value::Bool(value)) => *value,
        _ => fallback,
    }
}

fn tag_width(tag: &Label) -> f64 {
    NOTCH + 4.0 + tag.width + 6.0
}

fn color(lane: usize) -> String {
    series_color(i64::try_from(lane).unwrap_or(0))
}

/// Appends ` name="value"` with a formatted number.
fn attr(out: &mut String, name: &str, value: f64) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    push_num(out, value);
    out.push('"');
}

/// Appends `x,y`.
fn pair(out: &mut String, x: f64, y: f64) {
    push_num(out, x);
    out.push(',');
    push_num(out, y);
}

fn symbol(r#type: u8, x: f64, y: f64, color: &str) -> String {
    let mut out = String::new();
    if r#type == HIGHLIGHT {
        out.push_str("<rect");
        attr(&mut out, "x", x - DOT - 1.0);
        attr(&mut out, "y", y - DOT - 1.0);
        attr(&mut out, "width", 2.0 * DOT + 2.0);
        attr(&mut out, "height", 2.0 * DOT + 2.0);
        out.push_str(" rx=\"2\" fill=\"");
        out.push_str(color);
        out.push_str("\"/>");
        return out;
    }
    let at = |out: &mut String| {
        attr(out, "cx", x);
        attr(out, "cy", y);
    };
    if r#type == MERGE {
        out.push_str("<circle");
        at(&mut out);
        attr(&mut out, "r", DOT - 1.0);
        out.push_str(" fill=\"var(--_bg)\" stroke=\"");
        out.push_str(color);
        out.push_str("\" stroke-width=\"2\"/><circle");
        at(&mut out);
        out.push_str(" r=\"2.5\" fill=\"");
        out.push_str(color);
        out.push_str("\"/>");
        return out;
    }
    out.push_str("<circle");
    at(&mut out);
    attr(&mut out, "r", DOT);
    out.push_str(" fill=\"");
    out.push_str(color);
    out.push_str("\"/>");
    if r#type == REVERSE {
        out.push_str("<path d=\"M");
        pair(&mut out, x - 3.0, y - 3.0);
        out.push_str("l6,6M");
        pair(&mut out, x - 3.0, y + 3.0);
        out.push_str("l6,-6\" fill=\"none\" stroke=\"var(--_bg)\" stroke-width=\"1.5\" stroke-linecap=\"round\"/>");
    } else if r#type == CHERRY_PICK {
        out.push_str("<path d=\"M");
        pair(&mut out, x - 2.6, y + 1.0);
        out.push('L');
        pair(&mut out, x + 0.8, y - 4.2);
        out.push('L');
        pair(&mut out, x + 2.6, y + 1.0);
        out.push_str("\" fill=\"none\" stroke=\"var(--_bg)\" stroke-linejoin=\"round\"/>");
        for cx in [x - 2.6, x + 2.6] {
            out.push_str("<circle");
            attr(&mut out, "cx", cx);
            attr(&mut out, "cy", y + 2.2);
            out.push_str(" r=\"1.8\" fill=\"var(--_bg)\"/>");
        }
    }
    out
}

/// Draws `model` with the `gitGraph` section of `config`. A left-to-right graph that does not fit
/// the host's width may run down instead.
pub(crate) fn render(model: &GitModel, config: &Value, options: &RenderOptions) -> Rendered {
    if model.direction != "LR" {
        return draw(model, config, options, model.direction);
    }
    turn_to_fit(options, |down| {
        draw(model, config, options, if down { "TB" } else { "LR" })
    })
}

/// The extent of what is drawn.
struct Bounds {
    max_x: f64,
    max_y: f64,
    min_x: f64,
    min_y: f64,
}

impl Bounds {
    fn grow(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if x0 < self.min_x {
            self.min_x = x0;
        }
        if y0 < self.min_y {
            self.min_y = y0;
        }
        if x1 > self.max_x {
            self.max_x = x1;
        }
        if y1 > self.max_y {
            self.max_y = y1;
        }
    }
}

/// Room each commit needs: along the main axis on either side (`half`), the part of that its
/// tags take (`tag_half`), and across each lane towards the previous lane (`lo`) and the next one
/// (`hi`).
struct Room {
    half: Vec<f64>,
    hi: Vec<f64>,
    lo: Vec<f64>,
    tag_half: Vec<f64>,
}

fn room(views: &[View<'_>], lane_count: usize, vertical: bool, rotate: bool) -> Room {
    let mut half = vec![0.0; views.len()];
    let mut tag_half = vec![0.0; views.len()];
    let mut lo = vec![DOT; lane_count];
    let mut hi = vec![DOT; lane_count];
    for (k, view) in views.iter().enumerate() {
        let mut tags_main: f64 = 0.0;
        let mut tags_cross = 0.0;
        for tag in &view.tags {
            let w = tag_width(tag);
            if vertical {
                tags_main = tags_main.max(tag.height);
                tags_cross += w + 4.0;
            } else {
                tags_main = tags_main.max(w);
                tags_cross += tag.height + 3.0;
            }
        }
        let mut label_main = 0.0;
        let mut label_cross = 0.0;
        if let Some(label) = view.label.as_ref().filter(|label| !label.lines.is_empty()) {
            if rotate {
                label_cross = 3.0 + HALF * (label.width + label.height);
            } else {
                label_main = if vertical { label.height } else { label.width };
                label_cross = 6.0 + if vertical { label.width } else { label.height };
            }
        }
        if let Some(slot) = half.get_mut(k) {
            *slot = DOT.max(tags_main / 2.0).max(label_main / 2.0);
        }
        if let Some(slot) = tag_half.get_mut(k) {
            *slot = tags_main / 2.0;
        }
        let tag_side = DOT
            + if view.tags.is_empty() {
                0.0
            } else {
                TAG_GAP + tags_cross
            };
        let label_side = DOT + label_cross;
        if let Some(slot) = lo.get_mut(view.lane) {
            *slot = slot.max(if vertical { label_side } else { tag_side });
        }
        if let Some(slot) = hi.get_mut(view.lane) {
            *slot = slot.max(if vertical { tag_side } else { label_side });
        }
    }
    Room {
        half,
        hi,
        lo,
        tag_half,
    }
}

#[expect(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "mirrors Pele's draw"
)]
fn draw(
    model: &GitModel,
    config: &Value,
    options: &RenderOptions,
    direction: &'static str,
) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.75);
    let pad = options.padding.unwrap_or(8.0);

    let show_branches = flag(config, "showBranches", true);
    let show_commit_label = flag(config, "showCommitLabel", true);
    let rotate = flag(config, "rotateCommitLabel", true);
    let parallel = flag(config, "parallelCommits", false);
    let vertical = direction != "LR";
    let flip = direction == "BT";

    let lane_of: BTreeMap<&str, usize> = model
        .lanes
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    let lane_count = model.lanes.len();
    let medium = js_round(size * 0.875);
    // Mermaid writes branch names, tags, and commit ids as plain text, so tags in them show as
    // written.
    let names: Vec<Label> = model
        .lanes
        .iter()
        .map(|name| {
            plain_label(
                Some(if show_branches { name } else { "" }),
                medium,
                4000.0,
                0,
            )
        })
        .collect();

    let mut sorted: Vec<&GitCommit> = model.commits.iter().collect();
    sorted.sort_by_key(|commit| commit.seq);
    let mut views: Vec<View<'_>> = Vec::with_capacity(sorted.len());
    let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
    for commit in sorted {
        let labelled = show_commit_label
            && commit.r#type != CHERRY_PICK
            && (commit.r#type != MERGE || commit.custom_id == Some(true));
        let tags = commit
            .tags
            .iter()
            .map(|tag| plain_label(Some(tag), small, 4000.0, 0))
            .filter(|label| !label.lines.is_empty())
            .collect();
        by_id.insert(&commit.id, views.len());
        views.push(View {
            commit,
            index: views.len(),
            label: labelled.then(|| plain_label(Some(&commit.id), small, 4000.0, 0)),
            lane: lane_of.get(commit.branch.as_str()).copied().unwrap_or(0),
            m: 0.0,
            tags,
        });
    }

    let Room {
        half,
        mut hi,
        mut lo,
        tag_half,
    } = room(&views, lane_count, vertical, rotate);
    let at = |list: &[f64], k: usize| list.get(k).copied().unwrap_or(0.0);

    // In a left-to-right graph the tags stand over their commit. A connector that comes down from
    // a lane above joins the commit's lane this far before the commit, clear of the tags, and the
    // commit is placed far enough along to leave room for that.
    let mut lead = vec![0.0; views.len()];
    if !vertical {
        for (k, view) in views.iter().enumerate() {
            if at(&tag_half, k) == 0.0 {
                continue;
            }
            for id in &view.commit.parents {
                if let Some(parent) = by_id.get(id.as_str()).and_then(|&p| views.get(p))
                    && parent.lane < view.lane
                    && let Some(slot) = lead.get_mut(k)
                {
                    *slot = at(&tag_half, k) + 4.0;
                }
            }
        }
    }
    let space = |p: usize, k: usize| -> f64 {
        let lead_k = at(&lead, k);
        let curve = if lead_k > 0.0 {
            at(&half, p) + CURVE + lead_k
        } else {
            0.0
        };
        STEP.max(at(&half, p) + at(&half, k) + GAP).max(curve)
    };

    for k in 0..views.len() {
        let m = if parallel {
            // Mermaid's `parallelCommits`: a commit sits one step after its furthest parent, so
            // branches that grow side by side share positions instead of taking turns.
            let mut m: f64 = 0.0;
            if let Some(view) = views.get(k) {
                for id in &view.commit.parents {
                    let Some(parent) = by_id.get(id.as_str()).and_then(|&p| views.get(p)) else {
                        continue;
                    };
                    if parent.index >= k {
                        continue;
                    }
                    m = m.max(parent.m + space(parent.index, k));
                }
            }
            m
        } else if k == 0 {
            0.0
        } else {
            views.get(k - 1).map_or(0.0, |previous| previous.m) + space(k - 1, k)
        };
        if let Some(view) = views.get_mut(k) {
            view.m = m;
        }
    }
    let mut end: f64 = 0.0;
    for view in &views {
        end = end.max(view.m);
    }

    let mut cross = vec![0.0; lane_count];
    for (i, name) in names.iter().enumerate() {
        let reach = if name.lines.is_empty() {
            0.0
        } else if vertical {
            name.width / 2.0 + 8.0
        } else {
            name.height / 2.0 + 2.0
        };
        if let Some(slot) = lo.get_mut(i) {
            *slot = slot.max(reach);
        }
        if let Some(slot) = hi.get_mut(i) {
            *slot = slot.max(reach);
        }
        if i > 0 {
            let value = at(&cross, i - 1) + LANE.max(at(&hi, i - 1) + at(&lo, i) + GAP);
            if let Some(slot) = cross.get_mut(i) {
                *slot = value;
            }
        }
    }

    let px = |m: f64, c: f64| if vertical { c } else { m };
    let py = |m: f64, c: f64| {
        if vertical {
            if flip { -m } else { m }
        } else {
            c
        }
    };
    let point = |out: &mut String, m: f64, c: f64| pair(out, px(m, c), py(m, c));

    let mut bounds = Bounds {
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
    };

    let mut branches = String::new();
    if show_branches {
        for (i, name) in names.iter().enumerate() {
            let color = color(i);
            let c = at(&cross, i);
            let w = name.width + 16.0;
            let h = name.height + 4.0;
            let long = if vertical { h } else { w };
            let from = if name.lines.is_empty() {
                -LABEL_GAP
            } else {
                -LABEL_GAP - long
            };
            let cx = px(from + long / 2.0, c);
            let cy = py(from + long / 2.0, c);
            let (x, y) = (px(-LABEL_GAP, c), py(-LABEL_GAP, c));
            bounds.grow(x, y, x, y);
            let (x, y) = (px(end + TAIL, c), py(end + TAIL, c));
            bounds.grow(x, y, x, y);
            branches.push_str("<g class=\"ondine-branch\" data-id=\"");
            branches.push_str(&esc_text(model.lanes.get(i).map_or("", String::as_str)));
            branches.push_str("\"><path class=\"ondine-lane\" d=\"M");
            point(&mut branches, -LABEL_GAP, c);
            branches.push('L');
            point(&mut branches, end + TAIL, c);
            branches.push_str("\" stroke=\"");
            branches.push_str(&color);
            branches.push_str("\" stroke-dasharray=\"2 4\"/>");
            if !name.lines.is_empty() {
                bounds.grow(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
                branches.push_str("<rect");
                attr(&mut branches, "x", cx - w / 2.0);
                attr(&mut branches, "y", cy - h / 2.0);
                attr(&mut branches, "width", w);
                attr(&mut branches, "height", h);
                branches.push_str(" rx=\"");
                branches.push_str(RADIUS);
                branches.push_str("\" fill=\"");
                branches.push_str(&color);
                branches.push_str("\"/>");
                branches.push_str(&label_svg(
                    name,
                    cx,
                    cy,
                    " class=\"ondine-label\" fill=\"var(--_bg)\"",
                    None,
                ));
            }
            branches.push_str("</g>");
        }
    }

    // Positions of the commits on each lane, to tell whether a connector may run along it.
    let mut taken: Vec<Vec<f64>> = vec![Vec::new(); lane_count];
    for view in &views {
        if let Some(list) = taken.get_mut(view.lane) {
            list.push(view.m);
        }
    }
    if parallel {
        for list in &mut taken {
            list.sort_by(f64::total_cmp);
        }
    }
    let busy = |lane: usize, from: f64, to: f64| -> bool {
        let Some(list) = taken.get(lane) else {
            return false;
        };
        let low = list.partition_point(|&m| m <= from);
        list.get(low).is_some_and(|&m| m < to)
    };

    let mut curves = String::new();
    let mut lines = String::new();
    for view in &views {
        let commit = view.commit;
        let c2 = at(&cross, view.lane);
        let m2 = view.m;
        for (j, id) in commit.parents.iter().enumerate() {
            let Some(&p) = by_id.get(id.as_str()) else {
                continue;
            };
            let Some(parent) = views.get(p).filter(|_| p != view.index) else {
                continue;
            };
            let c1 = at(&cross, parent.lane);
            let m1 = parent.m;
            if parent.lane == view.lane {
                lines.push_str("<path class=\"ondine-edge\" d=\"M");
                point(&mut lines, m1, c1);
                lines.push('L');
                point(&mut lines, m2, c2);
                lines.push_str("\" stroke=\"");
                lines.push_str(&color(view.lane));
                lines.push_str("\"/>");
                continue;
            }
            let merged = j > 0 && commit.r#type == MERGE;
            let picked = j > 0 && !merged;
            let mut d = String::from("M");
            point(&mut d, m1, c1);
            if m2 <= m1 {
                let mid = f64::midpoint(m1, m2);
                d.push('C');
                point(&mut d, mid, c1);
                d.push(' ');
                point(&mut d, mid, c2);
                d.push(' ');
                point(&mut d, m2, c2);
            } else if !merged && !busy(view.lane, m1, m2) {
                // Leaves the parent at once and runs along the child's lane.
                let s = CURVE.min(m2 - m1);
                d.push('C');
                point(&mut d, m1 + s / 2.0, c1);
                d.push(' ');
                point(&mut d, m1 + s / 2.0, c2);
                d.push(' ');
                point(&mut d, m1 + s, c2);
                d.push('L');
                point(&mut d, m2, c2);
            } else {
                // Turns in just before the child, or before its tags when it comes from their
                // side.
                let lead_k = at(&lead, view.index);
                let to = if parent.lane < view.lane && m2 - lead_k > m1 {
                    m2 - lead_k
                } else {
                    m2
                };
                let mut s = CURVE.min(to - m1);
                let mut along = c1;
                if !merged || busy(parent.lane, m1, m2) {
                    // Neither lane is free to run along. Between two lanes, past the ids of one
                    // and before the tags of the next, a strip stays empty, and the connector
                    // takes that.
                    along = if c1 > c2 {
                        c2 + at(&hi, view.lane) + GAP / 2.0
                    } else {
                        c2 - at(&lo, view.lane) - GAP / 2.0
                    };
                    s = CURVE.min((to - m1) / 2.0);
                    d.push('C');
                    point(&mut d, m1 + s / 2.0, c1);
                    d.push(' ');
                    point(&mut d, m1 + s / 2.0, along);
                    d.push(' ');
                    point(&mut d, m1 + s, along);
                }
                d.push('L');
                point(&mut d, to - s, along);
                d.push('C');
                point(&mut d, to - s / 2.0, along);
                d.push(' ');
                point(&mut d, to - s / 2.0, c2);
                d.push(' ');
                point(&mut d, to, c2);
                if to < m2 {
                    d.push('L');
                    point(&mut d, m2, c2);
                }
            }
            curves.push_str("<path class=\"ondine-edge");
            if merged {
                curves.push_str(" ondine-edge-merge");
            } else if picked {
                curves.push_str(" ondine-edge-cherry-pick");
            }
            curves.push_str("\" d=\"");
            curves.push_str(&d);
            curves.push_str("\" stroke=\"");
            curves.push_str(&color(if merged { parent.lane } else { view.lane }));
            curves.push('"');
            if picked {
                curves.push_str(" stroke-dasharray=\"4 3\"");
            }
            curves.push_str("/>");
        }
    }

    let mut commits = String::new();
    for view in &views {
        let commit = view.commit;
        let c = at(&cross, view.lane);
        let x = px(view.m, c);
        let y = py(view.m, c);
        let r#type = commit.custom_type.unwrap_or(commit.r#type);
        bounds.grow(x - DOT - 1.0, y - DOT - 1.0, x + DOT + 1.0, y + DOT + 1.0);
        // Merges and cherry-picks carry a message Mermaid makes up; only a written one is worth
        // showing.
        let mut body = String::new();
        if commit.r#type < MERGE && !commit.message.is_empty() {
            body.push_str("<title>");
            body.push_str(&esc_text(&commit.message));
            body.push_str("</title>");
        }
        body.push_str(&symbol(r#type, x, y, &color(view.lane)));

        if let Some(label) = view.label.as_ref().filter(|label| !label.lines.is_empty()) {
            let w = label.width;
            let h = label.height;
            let back = |out: &mut String, cx: f64, cy: f64| {
                out.push_str("<rect");
                attr(out, "x", cx - w / 2.0 - 2.0);
                attr(out, "y", cy - h / 2.0 + 2.0);
                attr(out, "width", w + 4.0);
                attr(out, "height", h - 4.0);
                out.push_str(" rx=\"3\" fill=\"var(--_bg)\"/>");
            };
            if rotate {
                // The text ends beside the commit and trails back the way the history came.
                let k = (HALF * h) / 2.0;
                let ax = if vertical { x - DOT - 3.0 - k } else { x + 3.0 };
                let ay = if vertical { y } else { y + DOT + 3.0 + k };
                let reach = HALF * w + k;
                bounds.grow(
                    ax - reach,
                    if flip { ay - reach } else { ay - k },
                    ax + k,
                    if flip { ay + k } else { ay + reach },
                );
                body.push_str("<g class=\"ondine-commit-label\" transform=\"rotate(");
                body.push_str(if flip { "45" } else { "-45" });
                body.push(' ');
                push_num(&mut body, ax);
                body.push(' ');
                push_num(&mut body, ay);
                body.push_str(")\">");
                back(&mut body, ax - w / 2.0, ay);
                body.push_str(&label_svg(label, ax - w / 2.0, ay, "", None));
                body.push_str("</g>");
            } else {
                let cx = if vertical { x - DOT - 6.0 - w / 2.0 } else { x };
                let cy = if vertical { y } else { y + DOT + 6.0 + h / 2.0 };
                bounds.grow(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
                body.push_str("<g class=\"ondine-commit-label\">");
                back(&mut body, cx, cy);
                body.push_str(&label_svg(label, cx, cy, "", None));
                body.push_str("</g>");
            }
        }

        // Beside the lane the tags form a row; above it they stack, the first one furthest out.
        let mut offset = DOT + TAG_GAP;
        let order: Box<dyn Iterator<Item = &Label>> = if vertical {
            Box::new(view.tags.iter())
        } else {
            Box::new(view.tags.iter().rev())
        };
        for tag in order {
            let w = tag_width(tag);
            let h = tag.height;
            let left = if vertical { x + offset } else { x - w / 2.0 };
            let cy = if vertical { y } else { y - offset - h / 2.0 };
            offset += if vertical { w + 4.0 } else { h + 3.0 };
            bounds.grow(left, cy - h / 2.0, left + w, cy + h / 2.0);
            body.push_str("<g class=\"ondine-tag\"><path d=\"M");
            pair(&mut body, left, cy);
            body.push('L');
            pair(&mut body, left + NOTCH, cy - h / 2.0);
            body.push('H');
            push_num(&mut body, left + w);
            body.push('V');
            push_num(&mut body, cy + h / 2.0);
            body.push('H');
            push_num(&mut body, left + NOTCH);
            body.push_str(
                "Z\" fill=\"var(--_s)\" stroke=\"var(--_b)\" stroke-linejoin=\"round\"/><circle",
            );
            attr(&mut body, "cx", left + NOTCH);
            attr(&mut body, "cy", cy);
            body.push_str(" r=\"1.5\" fill=\"var(--_b)\"/>");
            body.push_str(&label_svg(
                tag,
                left + NOTCH + 4.0 + tag.width / 2.0,
                cy,
                " fill=\"var(--_fg)\"",
                None,
            ));
            body.push_str("</g>");
        }

        commits.push_str("<g class=\"ondine-commit ondine-commit-");
        commits.push_str(
            CLASSES
                .get(usize::from(r#type))
                .copied()
                .unwrap_or(CLASSES[0]),
        );
        if commit.r#type == MERGE && r#type != MERGE {
            commits.push_str(" ondine-commit-merge");
        }
        commits.push_str("\" data-id=\"");
        commits.push_str(&esc_text(&commit.id));
        commits.push_str("\">");
        commits.push_str(&body);
        commits.push_str("</g>");
    }

    if bounds.min_x > bounds.max_x {
        bounds.grow(0.0, 0.0, 0.0, 0.0);
    }
    let title = layout_label(model.title.as_deref(), false, size, 4000.0, BOLD, None);
    let title_height = if title.height > 0.0 {
        title.height + 16.0
    } else {
        0.0
    };
    let inner = (bounds.max_x - bounds.min_x).max(title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (bounds.max_y - bounds.min_y + title_height + 2.0 * pad).ceil();
    let dx = pad - bounds.min_x + (inner - (bounds.max_x - bounds.min_x)) / 2.0;
    let dy = pad + title_height - bounds.min_y;

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g transform=\"translate(");
    pair(&mut content, dx, dy);
    content.push_str(")\">");
    if !branches.is_empty() {
        content.push_str("<g class=\"ondine-branches\" fill=\"none\" font-size=\"");
        push_number(&mut content, medium);
        content.push_str("\">");
        content.push_str(&branches);
        content.push_str("</g>");
    }
    if !curves.is_empty() || !lines.is_empty() {
        content.push_str(
            "<g class=\"ondine-edges\" fill=\"none\" stroke-width=\"2\" stroke-linecap=\"round\">",
        );
        content.push_str(&curves);
        content.push_str(&lines);
        content.push_str("</g>");
    }
    if !commits.is_empty() {
        content.push_str("<g class=\"ondine-commits\" font-size=\"");
        push_number(&mut content, small);
        content.push_str("\" fill=\"var(--_m)\">");
        content.push_str(&commits);
        content.push_str("</g>");
    }
    content.push_str("</g>");

    let acc = Accessible {
        acc_descr: model.acc_descr.as_deref(),
        acc_title: model.acc_title.as_deref(),
    };
    Rendered {
        height,
        links: Vec::new(),
        svg: svg_document("gitGraph", width, height, size, options, acc, &content),
        width,
    }
}
