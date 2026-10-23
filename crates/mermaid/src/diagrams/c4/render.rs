//! Draws a C4 diagram. Port of Pele's `src/diagrams/c4/render.ts`.

use std::collections::BTreeSet;

use super::db::{C4Db, ElementRef};
use super::layout::{HEAD_LEFT, HEAD_TOP, layout_c4};
use super::route::{Form, HEAD, LANE, Lanes, Route, crosses, cyl_ry, queue_rx, route};
use crate::diagrams::Rendered;
use crate::diagrams::common::fit_width::{Drawn, tighten, title_room};
use crate::diagrams::common::literal_label::left_label_svg;
use crate::error::{Code, Error};
use crate::svg::builder::{esc, esc_text, js_round, label_svg, push_num};
use crate::svg::edges::{ARROW, marker, marker_trim};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::shapes::draw_shape;
use crate::svg::theme::{RADIUS, ResolvedStyle, resolve_style};
use crate::text::label::layout_label;
use crate::text::measure::{BOLD, ITALIC};
use crate::util::url::{LinkInfo, UrlOptions, linked, sanitize_url};
use crate::yaml::push_number;

const PAIR_SHIFT: f64 = 7.0;
const NARROW_GAP: f64 = 36.0;
const WIDEN: f64 = 24.0;
const MAX_OFFSET: f64 = 2000.0;
/// A label may cover this much of a shape's edge, where there is no text.
const EDGE: f64 = 8.0;
/// Places along a relation to try for its label, nearest the middle first.
const SLIDE: [f64; 7] = [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8];
/// Above this many box comparisons, labels stay at the middle and lines are not steered around
/// shapes.
const CARE_BUDGET: f64 = 2_000_000.0;

fn user_style(fill: Option<&str>, stroke: Option<&str>, color: Option<&str>) -> ResolvedStyle {
    let mut declarations: Vec<String> = Vec::new();
    for (property, value) in [("fill:", fill), ("stroke:", stroke), ("color:", color)] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            declarations.push([property, value].concat());
        }
    }
    resolve_style(&declarations)
}

/// Entity encoding has hidden any `#name;` in an address. This puts it back, so that the check
/// sees what a browser would.
fn unhide(url: &str) -> String {
    if !url.contains("\u{b6}\u{df}") {
        return url.to_owned();
    }
    // Pele replaces `ﬂ°` or `ﬂ°°` first, then `¶ß`. Neither can make or break the other, so one
    // pass does both.
    let mut out = String::with_capacity(url.len());
    let mut rest = url;
    while let Some(c) = rest.chars().next() {
        if let Some(tail) = rest.strip_prefix("\u{fb02}\u{b0}") {
            out.push('#');
            rest = tail.strip_prefix('\u{b0}').unwrap_or(tail);
        } else if let Some(tail) = rest.strip_prefix("\u{b6}\u{df}") {
            out.push(';');
            rest = tail;
        } else {
            out.push(c);
            rest = rest.get(c.len_utf8()..).unwrap_or("");
        }
    }
    out
}

/// The context every link of one drawing shares.
struct Links<'a> {
    links: Vec<LinkInfo>,
    url: &'a UrlOptions<'a>,
}

impl Links<'_> {
    fn link(&mut self, body: &str, url: Option<&str>, id: &str) -> String {
        match url.filter(|url| !url.is_empty()) {
            Some(url) => linked(
                body,
                &sanitize_url(&unhide(url)),
                id,
                &mut self.links,
                self.url,
                "",
            ),
            None => body.to_owned(),
        }
    }
}

/// A drawing or the error that stopped it, as [`tighten`] takes it.
struct Drawing(Result<Rendered, Error>);

impl Drawn for Drawing {
    fn width(&self) -> f64 {
        self.0.as_ref().map_or(0.0, |rendered| rendered.width)
    }
}

/// Draws a C4 diagram, more tightly when it does not fit the host's width.
///
/// # Errors
///
/// Returns a semantic error for a relation that names an unknown shape or boundary.
pub(crate) fn render(
    db: &C4Db,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
) -> Result<Rendered, Error> {
    let result = tighten(options, |tight| {
        if !tight {
            return Drawing(draw(db, options, url, false, NARROW_GAP).map(|drawn| drawn.0));
        }
        let (mut best, mut squeezed) = match draw(db, options, url, true, NARROW_GAP) {
            Ok(drawn) => drawn,
            Err(error) => return Drawing(Err(error)),
        };
        let mut gap = NARROW_GAP + WIDEN;
        while squeezed > 0 && gap <= NARROW_GAP + 3.0 * WIDEN {
            match draw(db, options, url, true, gap) {
                Ok((wider, left)) if left < squeezed => {
                    best = wider;
                    squeezed = left;
                }
                Ok(_) => {}
                Err(error) => return Drawing(Err(error)),
            }
            gap += WIDEN;
        }
        Drawing(Ok(best))
    });
    result.0
}

#[cold]
fn unknown(from: &str, to: &str) -> Error {
    let mut error = Error::new(
        Code::Semantic,
        [
            "C4 rel \"",
            from,
            "\" -> \"",
            to,
            "\" references an unknown shape or boundary",
        ]
        .concat(),
    );
    error.r#type = Some("c4");
    error
}

#[expect(clippy::too_many_lines, reason = "mirrors Pele's draw")]
fn draw(
    db: &C4Db,
    options: &RenderOptions,
    url: &UrlOptions<'_>,
    tight: bool,
    gap: f64,
) -> Result<(Rendered, usize), Error> {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.8125);
    let pad = options.padding.unwrap_or(8.0);
    let mut links = Links {
        links: Vec::new(),
        url,
    };
    let room = tight.then(|| 80.0_f64.max(options.max_width.unwrap_or(320.0) - 2.0 * pad));
    let mut layout = layout_c4(db, size, small, room, gap);
    let root = layout
        .groups
        .first()
        .and_then(|group| layout.boxes.get(group.id))
        .copied()
        .unwrap_or_default();

    let mut min_x = 0.0_f64;
    let mut min_y = 0.0_f64;
    let mut max_x = root.w;
    let mut max_y = root.h;
    let mut grow = |x0: f64, y0: f64, x1: f64, y1: f64| {
        min_x = min_x.min(x0);
        min_y = min_y.min(y0);
        max_x = max_x.max(x1);
        max_y = max_y.max(y1);
    };

    let box_of = |at: Option<ElementRef>| match at {
        Some(ElementRef::Shape(at)) => layout.shape_boxes.get(at).copied().flatten(),
        Some(ElementRef::Boundary(at)) => layout.boundary_boxes.get(at).copied().flatten(),
        None => None,
    };
    let mut ends: Vec<(usize, usize)> = Vec::with_capacity(db.rels.len());
    let mut pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
    for rel in &db.rels {
        let from = if rel.from_attr.is_some() {
            None
        } else {
            db.get_c4_shape(&rel.from)
        };
        let to = if rel.to_attr.is_some() {
            None
        } else {
            db.get_c4_shape(&rel.to)
        };
        let (Some(a), Some(b)) = (box_of(from), box_of(to)) else {
            return Err(unknown(&rel.from, &rel.to));
        };
        ends.push((a, b));
        pairs.insert((a, b));
    }

    // What a relation's label should not cover: shapes, arrowheads, boundary headings, and the
    // labels placed before it.
    let mut nodes: Vec<usize> = Vec::new();
    let mut taken: Vec<f64> = Vec::new();
    for &at in &layout.order {
        for node in layout.groups.get(at).map_or(&[][..], |g| &g.nodes[..]) {
            nodes.push(node.id);
            if let Some(b) = layout.boxes.get(node.id) {
                taken.extend([b.x + EDGE, b.y + EDGE, b.x + b.w - EDGE, b.y + b.h - EDGE]);
            }
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "counts stay far below 2^52")]
    let careful = db.rels.len() as f64
        * (nodes.len() as f64 + layout.order.len() as f64 + 3.0 * db.rels.len() as f64)
        <= CARE_BUDGET;
    let covered = |taken: &[f64], x0: f64, y0: f64, x1: f64, y1: f64| {
        let mut area = 0.0;
        for &[left, top, right, bottom] in taken.as_chunks::<4>().0 {
            let w = x1.min(right) - x0.max(left);
            if w <= 0.0 {
                continue;
            }
            let h = y1.min(bottom) - y0.max(top);
            if h > 0.0 {
                area += w * h;
            }
        }
        area
    };
    let offset = |value: Option<f64>| match value {
        Some(value) if value != 0.0 && !value.is_nan() => value.clamp(-MAX_OFFSET, MAX_OFFSET),
        _ => 0.0,
    };

    // Every line is drawn before any label is placed, so that labels can keep off the arrowheads.
    let mut edges_out = String::new();
    let trim = marker_trim("arrow_point");
    let mut lanes = Lanes::default();
    for &at in layout.order.iter().skip(1) {
        if let Some(b) = layout.groups.get(at).and_then(|g| layout.boxes.get(g.id)) {
            lanes.frame(b);
        }
    }
    let mut routes: Vec<Route> = Vec::with_capacity(db.rels.len());
    for (rel, &(a, b)) in db.rels.iter().zip(&ends) {
        let style = user_style(None, rel.line_color.as_deref(), None);
        let color = style
            .stroke
            .as_deref()
            .filter(|stroke| !stroke.is_empty())
            .map_or_else(|| "var(--_l)".to_owned(), |stroke| esc(stroke).into_owned());
        let start_arrow = rel.r#type == "birel" || rel.r#type == "rel_b";
        let end_arrow = rel.r#type != "rel_b";
        let reverse = a != b && pairs.contains(&(b, a));
        let r = route(
            &layout.boxes,
            (a, b),
            if reverse { PAIR_SHIFT } else { 0.0 },
            careful.then_some(&nodes[..]),
            &mut lanes,
            if start_arrow { trim } else { 0.0 },
            if end_arrow { trim } else { 0.0 },
        );
        grow(
            r.x0.min(r.x1),
            r.y0.min(r.y1),
            r.x0.max(r.x1),
            r.y0.max(r.y1),
        );
        if start_arrow && careful {
            taken.extend([r.sx - ARROW, r.sy - ARROW, r.sx + ARROW, r.sy + ARROW]);
        }
        if end_arrow && careful {
            taken.extend([r.ex - ARROW, r.ey - ARROW, r.ex + ARROW, r.ey + ARROW]);
        }
        edges_out.push_str("<g class=\"ondine-edge\" data-id=\"");
        edges_out.push_str(&esc_text(&[rel.from.as_str(), "-", &rel.to].concat()));
        edges_out.push('"');
        edges_out.push_str(&style.line);
        edges_out.push_str("><path d=\"");
        edges_out.push_str(&r.d);
        edges_out.push_str("\"/>");
        if start_arrow {
            edges_out.push_str(&marker("arrow_point", r.sx, r.sy, r.sdx, r.sdy, &color));
        }
        if end_arrow {
            edges_out.push_str(&marker("arrow_point", r.ex, r.ey, r.edx, r.edy, &color));
        }
        edges_out.push_str("</g>");
        routes.push(r);
    }

    // A boundary's heading goes at the left, the middle or the right of its top edge: the first
    // of them no line crosses.
    for &at in &layout.order {
        let Some(group) = layout.groups.get_mut(at) else {
            continue;
        };
        if !careful || group.head_w == 0.0 {
            continue;
        }
        let Some(&own) = layout.boxes.get(group.id) else {
            continue;
        };
        let top = own.y + HEAD_TOP;
        let bottom = own.y + group.head - LANE;
        let mut fewest = usize::MAX;
        for x in [
            HEAD_LEFT,
            (own.w - group.head_w) / 2.0,
            own.w - HEAD_LEFT - group.head_w,
        ] {
            let count = routes
                .iter()
                .filter(|r| {
                    crosses(
                        &r.points,
                        own.x + x - 4.0,
                        top,
                        own.x + x + group.head_w + 4.0,
                        bottom,
                    )
                })
                .count();
            if count < fewest {
                fewest = count;
                group.head_x = x;
            }
            if count == 0 {
                break;
            }
        }
        taken.extend([
            own.x + group.head_x - 4.0,
            top,
            own.x + group.head_x + group.head_w + 4.0,
            bottom,
        ]);
    }

    let mut labels_out = String::new();
    for (index, (rel, r)) in db.rels.iter().zip(&routes).enumerate() {
        let id = [rel.from.as_str(), "-", &rel.to].concat();
        let style = user_style(None, None, rel.text_color.as_deref());
        let mut text = String::new();
        if db.c4_type.as_deref() == Some("C4Dynamic") {
            #[expect(clippy::cast_precision_loss, reason = "counts stay far below 2^52")]
            let number = (index + 1) as f64;
            push_number(&mut text, number);
            text.push_str(": ");
        }
        text.push_str(&rel.label);
        let label = layout_label(Some(&text), false, small, r.room, 0, None);
        let techn_text = rel
            .techn
            .as_deref()
            .filter(|techn| !techn.is_empty())
            .map(|techn| ["[", techn, "]"].concat());
        let techn = layout_label(techn_text.as_deref(), false, small, r.room, ITALIC, None);
        let lw = label.width.max(techn.width) + 8.0;
        let lh = label.height + techn.height;
        if lh == 0.0 {
            continue;
        }

        // The label sits on the line, or just beside it when another relation runs the opposite
        // way.
        let beside = r.nx.abs() * (lw / 2.0) + r.ny.abs() * (lh / 2.0) + 2.0;
        let aside = if r.lean == 0.0 {
            beside
        } else {
            0.0_f64.max(r.lean.min(beside - 14.0))
        };
        // Manual offsets are tuned to the original spacing. Reposition compact labels beside
        // their routed arrows so an offset cannot leave them floating outside a boundary.
        let mut dx = r.nx * aside + if tight { 0.0 } else { offset(rel.offset_x) };
        let mut dy = r.ny * aside + if tight { 0.0 } else { offset(rel.offset_y) };
        let mut at = 0.5;
        if careful {
            // Slide along the line to the spot that covers least. Offsets from the diagram are
            // kept unless a spot on the line itself covers less than any they lead to: they are
            // often tuned for Mermaid's spacing.
            let mut least = f64::INFINITY;
            #[expect(clippy::float_cmp, reason = "mirrors Pele's exact comparison")]
            let moved = dx != r.nx * aside || dy != r.ny * aside;
            let passes = if moved { 2 } else { 1 };
            let mut pass = 0;
            while pass < passes && least > 0.0 {
                let px = if pass == 0 { dx } else { r.nx * aside };
                let py = if pass == 0 { dy } else { r.ny * aside };
                for t in SLIDE {
                    let cx = r.x0 + (r.x1 - r.x0) * t + px;
                    let cy = r.y0 + (r.y1 - r.y0) * t + py;
                    let area = covered(
                        &taken,
                        cx - lw / 2.0,
                        cy - lh / 2.0,
                        cx + lw / 2.0,
                        cy + lh / 2.0,
                    );
                    if area < least {
                        least = area;
                        at = t;
                        dx = px;
                        dy = py;
                    }
                    if area == 0.0 {
                        break;
                    }
                }
                pass += 1;
            }
        }
        let cx = r.x0 + (r.x1 - r.x0) * at + dx;
        let top = r.y0 + (r.y1 - r.y0) * at + dy - lh / 2.0;
        if careful {
            taken.extend([cx - lw / 2.0, top, cx + lw / 2.0, top + lh]);
        }
        grow(cx - lw / 2.0, top, cx + lw / 2.0, top + lh);

        labels_out.push_str("<g class=\"ondine-edge-label\" data-id=\"");
        labels_out.push_str(&esc_text(&id));
        labels_out.push_str("\">");
        if let Some(descr) = rel.descr.as_deref().filter(|descr| !descr.is_empty()) {
            labels_out.push_str("<title>");
            labels_out.push_str(&esc_text(descr));
            labels_out.push_str("</title>");
        }
        labels_out.push_str("<rect x=\"");
        push_num(&mut labels_out, cx - lw / 2.0);
        labels_out.push_str("\" y=\"");
        push_num(&mut labels_out, top);
        labels_out.push_str("\" width=\"");
        push_num(&mut labels_out, lw);
        labels_out.push_str("\" height=\"");
        push_num(&mut labels_out, lh);
        labels_out.push_str("\" rx=\"3\" fill=\"var(--_bg)\"/>");
        let mut body = label_svg(&label, cx, top + label.height / 2.0, &style.text, None);
        let mut techn_attrs = String::from(" class=\"ondine-c4-techn\" fill=\"var(--_m)\"");
        techn_attrs.push_str(&style.text);
        body.push_str(&label_svg(
            &techn,
            cx,
            top + label.height + techn.height / 2.0,
            &techn_attrs,
            None,
        ));
        labels_out.push_str(&links.link(&body, rel.link.as_deref(), &id));
        labels_out.push_str("</g>");
    }

    let mut small_text = String::new();
    push_number(&mut small_text, small);
    let mut smaller_text = String::new();
    push_number(&mut smaller_text, small - 1.0);
    let mut clusters = String::new();
    let mut nodes_out = String::new();
    for &at in &layout.order {
        let Some(group) = layout.groups.get(at) else {
            continue;
        };
        if at != 0
            && let (Some(boundary), Some(&own)) = (
                db.boundaries.get(group.boundary),
                layout.boxes.get(group.id),
            )
        {
            let style = user_style(
                boundary.bg_color.as_deref(),
                boundary.border_color.as_deref(),
                boundary.font_color.as_deref(),
            );
            let x = own.x + group.head_x;
            let mut y = own.y + HEAD_TOP;
            let empty = crate::text::label::Label::default();
            let heading = |k: usize| group.labels.get(k).unwrap_or(&empty);
            let mut text = left_label_svg(
                heading(0),
                x,
                y,
                &[
                    " class=\"ondine-cluster-label\" font-weight=\"var(--_hw)\"",
                    &style.text,
                ]
                .concat(),
            );
            y += heading(0).height;
            text.push_str(&left_label_svg(
                heading(1),
                x,
                y,
                &[
                    " class=\"ondine-c4-type\" font-size=\"",
                    &small_text,
                    "\" fill=\"var(--_m)\"",
                    &style.text,
                ]
                .concat(),
            ));
            y += heading(1).height;
            text.push_str(&left_label_svg(
                heading(2),
                x,
                y,
                &[
                    " class=\"ondine-c4-descr\" font-size=\"",
                    &small_text,
                    "\" fill=\"var(--_m)\"",
                    &style.text,
                ]
                .concat(),
            ));
            let deployment = boundary.node_type.as_deref().is_some_and(|t| !t.is_empty());
            clusters.push_str("<g class=\"ondine-cluster ondine-c4-");
            clusters.push_str(if deployment { "node" } else { "boundary" });
            clusters.push_str("\" data-id=\"");
            clusters.push_str(&esc_text(&boundary.alias));
            clusters.push_str("\"><rect x=\"");
            push_num(&mut clusters, own.x);
            clusters.push_str("\" y=\"");
            push_num(&mut clusters, own.y);
            clusters.push_str("\" width=\"");
            push_num(&mut clusters, own.w);
            clusters.push_str("\" height=\"");
            push_num(&mut clusters, own.h);
            clusters.push_str("\" rx=\"");
            clusters.push_str(RADIUS);
            clusters.push_str("\" fill=\"none\" stroke=\"var(--_b)\"");
            if !deployment {
                clusters.push_str(" stroke-dasharray=\"6 4\"");
            }
            clusters.push_str(&style.shape);
            clusters.push_str("/>");
            clusters.push_str(&links.link(&text, boundary.link.as_deref(), &boundary.alias));
            clusters.push_str("</g>");
        }
        for node in &group.nodes {
            let (Some(shape), Some(&own)) = (db.shapes.get(node.shape), layout.boxes.get(node.id))
            else {
                continue;
            };
            let external = shape.kind.starts_with("external");
            let style = user_style(
                shape.bg_color.as_deref(),
                shape.border_color.as_deref(),
                shape.font_color.as_deref(),
            );
            let mut stroke = String::from(" stroke=\"var(--_b)\"");
            if external {
                stroke.push_str(" stroke-dasharray=\"4 3\"");
            }
            let (w, h, form, labels) = (own.w, own.h, own.form, &node.labels);
            let dx = if form == Form::HCyl {
                -queue_rx(h) / 2.0
            } else {
                0.0
            };
            let dy = match form {
                Form::Person => HEAD / 2.0,
                Form::Cyl => cyl_ry(w) / 2.0,
                _ => 0.0,
            };
            let muted = [" fill=\"var(--_m)\"", style.text.as_str()].concat();
            let mut y = dy - node.text_h / 2.0;
            let mut text = label_svg(
                &labels[0],
                dx,
                y + labels[0].height / 2.0,
                &[
                    " class=\"ondine-c4-type\" font-size=\"",
                    &smaller_text,
                    "\"",
                    &muted,
                ]
                .concat(),
                None,
            );
            y += labels[0].height;
            text.push_str(&label_svg(
                &labels[1],
                dx,
                y + labels[1].height / 2.0,
                &[
                    " class=\"ondine-label\" font-weight=\"var(--_hw)\"",
                    &style.text,
                ]
                .concat(),
                None,
            ));
            y += labels[1].height;
            text.push_str(&label_svg(
                &labels[2],
                dx,
                y + labels[2].height / 2.0,
                &[
                    " class=\"ondine-c4-techn\" font-size=\"",
                    &small_text,
                    "\"",
                    &muted,
                ]
                .concat(),
                None,
            ));
            y += labels[2].height + 4.0;
            text.push_str(&label_svg(
                &labels[3],
                dx,
                y + labels[3].height / 2.0,
                &[
                    " class=\"ondine-c4-descr\" font-size=\"",
                    &small_text,
                    "\"",
                    &style.text,
                ]
                .concat(),
                None,
            ));
            nodes_out.push_str("<g class=\"ondine-node ondine-shape-");
            nodes_out.push_str(form.name());
            nodes_out.push_str(" ondine-c4-");
            nodes_out.push_str(&shape.kind);
            if external {
                nodes_out.push_str(" ondine-c4-external");
            }
            nodes_out.push_str("\" data-id=\"");
            nodes_out.push_str(&esc_text(&shape.alias));
            nodes_out.push_str("\" transform=\"translate(");
            push_num(&mut nodes_out, own.x + w / 2.0);
            nodes_out.push(',');
            push_num(&mut nodes_out, own.y + h / 2.0);
            nodes_out.push_str(")\">");
            let fill = [
                " fill=\"var(",
                if external { "--_bg" } else { "--_s" },
                ")\"",
                &stroke,
                &style.shape,
            ]
            .concat();
            let mut body = draw_shape(
                form.name(),
                w,
                h,
                &fill,
                &[stroke.as_str(), &style.line].concat(),
            );
            body.push_str(&text);
            nodes_out.push_str(&links.link(&body, shape.link.as_deref(), &shape.alias));
            nodes_out.push_str("</g>");
        }
    }

    let title = layout_label(
        db.title.as_deref(),
        false,
        size,
        if tight { title_room(options) } else { 4000.0 },
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = (max_x - min_x).max(title.width);
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let ox = pad - min_x + (inner - (max_x - min_x)) / 2.0;
    let oy = pad + title_height - min_y;

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g transform=\"translate(");
    push_num(&mut content, ox);
    content.push(',');
    push_num(&mut content, oy);
    content.push_str(")\">");
    if !clusters.is_empty() {
        content.push_str("<g class=\"ondine-clusters\">");
        content.push_str(&clusters);
        content.push_str("</g>");
    }
    if !edges_out.is_empty() {
        content.push_str(
            "<g class=\"ondine-edges\" fill=\"none\" stroke=\"var(--_l)\" stroke-linecap=\"round\">",
        );
        content.push_str(&edges_out);
        content.push_str("</g>");
    }
    if !nodes_out.is_empty() {
        content.push_str("<g class=\"ondine-nodes\">");
        content.push_str(&nodes_out);
        content.push_str("</g>");
    }
    if !labels_out.is_empty() {
        content.push_str("<g class=\"ondine-edge-labels\" font-size=\"");
        content.push_str(&small_text);
        content.push_str("\">");
        content.push_str(&labels_out);
        content.push_str("</g>");
    }
    content.push_str("</g>");

    let svg = svg_document(
        "c4",
        width,
        height,
        size,
        options,
        Accessible {
            acc_descr: db.acc_descr.as_deref(),
            acc_title: db.acc_title.as_deref(),
        },
        &content,
    );
    Ok((
        Rendered {
            height,
            links: links.links,
            svg,
            width,
        },
        lanes.squeezed,
    ))
}
