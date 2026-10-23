//! Draws a venn diagram. Port of Pele's `src/diagrams/venn/render.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::db::{VennDb, normalize_text};
use super::layout::{Circle, Spot, find_spot, hypot, layout_circles, region_path};
use crate::diagrams::Rendered;
use crate::svg::builder::{esc_text, js_round, label_svg, push_num};
use crate::svg::root::{Accessible, RenderOptions, svg_document};
use crate::svg::theme::{ResolvedStyle, resolve_style, series_color};
use crate::text::label::{Label, layout_label};
use crate::text::measure::{BOLD, ITALIC};
use crate::yaml::push_number;

const RADIUS: f64 = 100.0;
const MAX_GROWTH: f64 = 1.6;

/// A line written in a region: its label first, then its texts.
struct Line {
    base: u8,
    /// The id of a text, or `None` for the region's own label.
    id: Option<String>,
    label: Label,
    raw: String,
    size: f64,
}

/// A set or stated overlap, where it is written, and how it looks.
struct Region {
    inside: Vec<usize>,
    lines: Vec<Line>,
    sets: Vec<String>,
    spot: Spot,
    style: ResolvedStyle,
}

/// The declarations for each list of targets, merged in insertion order, later ones winning.
type Styles = BTreeMap<Vec<String>, Vec<(String, String)>>;

fn style_of(styles: &Styles, key: &[String]) -> ResolvedStyle {
    let Some(declared) = styles.get(key) else {
        return ResolvedStyle::default();
    };
    let list: Vec<String> = declared
        .iter()
        .map(|(property, value)| [property.as_str(), ":", value].concat())
        .collect();
    resolve_style(&list)
}

/// Draws a venn diagram.
#[expect(
    clippy::float_cmp,
    clippy::too_many_lines,
    reason = "mirrors Pele's renderVenn, which compares sizes exactly"
)]
pub(crate) fn render(model: &VennDb, options: &RenderOptions) -> Rendered {
    let size = options.font_size.unwrap_or(16.0);
    let small = js_round(size * 0.875);
    let tiny = js_round(size * 0.8125);
    let pad = options.padding.unwrap_or(8.0);

    let circles = layout_circles(&model.subsets);
    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, c) in circles.iter().enumerate() {
        index.insert(&c.id, i);
    }

    // Styles for the same targets add up, later declarations winning.
    let mut styles: Styles = BTreeMap::new();
    for style in &model.styles {
        let merged = styles.entry(style.targets.clone()).or_default();
        for (property, value) in &style.styles {
            match merged.iter_mut().find(|(own, _)| own == property) {
                Some(entry) => entry.1.clone_from(value),
                None => merged.push((property.clone(), value.clone())),
            }
        }
    }

    // Circles that touch each circle. Only those can cut into a region of it.
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); circles.len()];
    for (a, ca) in circles.iter().enumerate() {
        for (b, cb) in circles.iter().enumerate().skip(a + 1) {
            if hypot(ca.x - cb.x, ca.y - cb.y) < ca.r + cb.r {
                if let Some(list) = neighbors.get_mut(a) {
                    list.push(b);
                }
                if let Some(list) = neighbors.get_mut(b) {
                    list.push(a);
                }
            }
        }
    }
    let radius = |i: usize| circles.get(i).map_or(0.0, |c| c.r);

    // One region for each set and each stated overlap. A later statement about the same sets
    // replaces an earlier one.
    let mut stated: Vec<usize> = Vec::new();
    let mut stated_at: BTreeMap<&[String], usize> = BTreeMap::new();
    for (i, subset) in model.subsets.iter().enumerate() {
        if let Some(&at) = stated_at.get(subset.sets.as_slice()) {
            if let Some(slot) = stated.get_mut(at) {
                *slot = i;
            }
        } else {
            stated_at.insert(&subset.sets, stated.len());
            stated.push(i);
        }
    }
    let mut regions: Vec<Region> = Vec::new();
    let mut region_at: BTreeMap<&[String], usize> = BTreeMap::new();
    for &i in &stated {
        let Some(subset) = model.subsets.get(i) else {
            continue;
        };
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let sets: Vec<String> = subset
            .sets
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .cloned()
            .collect();
        let inside: Vec<usize> = sets
            .iter()
            .map(|id| index.get(id.as_str()).copied().unwrap_or(0))
            .collect();
        let mut smallest = inside.first().copied().unwrap_or(0);
        for &i in &inside {
            if radius(i) < radius(smallest) {
                smallest = i;
            }
        }
        let style = style_of(&styles, &subset.sets);
        let only = match subset.sets.as_slice() {
            [only] => Some(only.as_str()),
            _ => None,
        };
        let text = subset.label.as_deref().or(only);
        let base = (if style.bold { BOLD } else { 0 }) | (if style.italic { ITALIC } else { 0 });
        let outside: Vec<usize> = neighbors
            .get(smallest)
            .map(|list| {
                list.iter()
                    .copied()
                    .filter(|i| !inside.contains(i))
                    .collect()
            })
            .unwrap_or_default();
        let mut spot = find_spot(&circles, &inside, &outside);
        // A region that other circles cover, or all but cover, is written across them instead.
        if spot.margin < radius(smallest) * 0.2 {
            spot = find_spot(&circles, &inside, &[]);
        }
        let mut lines = Vec::new();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            let font_size = if sets.len() == 1 { size } else { small };
            lines.push(Line {
                base,
                id: None,
                label: layout_label(Some(text), false, font_size, 4000.0, base, None),
                raw: text.to_owned(),
                size: font_size,
            });
        }
        region_at.insert(&subset.sets, regions.len());
        regions.push(Region {
            inside,
            lines,
            sets,
            spot,
            style,
        });
    }
    for node in &model.text_nodes {
        let raw = node.label.as_deref().unwrap_or(&node.id);
        if let Some(region) = region_at
            .get(node.sets.as_slice())
            .and_then(|&at| regions.get_mut(at))
        {
            region.lines.push(Line {
                base: 0,
                id: Some(node.id.clone()),
                label: layout_label(Some(raw), false, tiny, 4000.0, 0, None),
                raw: raw.to_owned(),
                size: tiny,
            });
        }
    }

    // Lengths so far are in units of the layout. The scale gives the largest circle a set radius,
    // and grows, within limits, until every region holds what is written in it.
    let mut largest = 0.0;
    for c in &circles {
        if c.r > largest {
            largest = c.r;
        }
    }
    let mut scale = if largest > 0.0 { RADIUS / largest } else { 1.0 };
    let mut growth: f64 = 1.0;
    for region in &regions {
        if region.spot.margin <= 0.0 {
            continue;
        }
        let mut width: f64 = 0.0;
        let mut height = 0.0;
        for line in &region.lines {
            width = width.max(line.label.width);
            height += line.label.height;
        }
        let need = hypot(width, height) / 2.0 + 4.0;
        growth = growth.max((need / (region.spot.margin * scale)).min(MAX_GROWTH));
    }
    scale *= growth;

    // What is still too wide for its region is wrapped to it.
    for region in &mut regions {
        let room = (region.spot.margin * scale * 1.8).max(72.0);
        for line in &mut region.lines {
            if line.label.width > room {
                line.label = layout_label(Some(&line.raw), false, line.size, room, line.base, None);
            }
        }
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for c in &circles {
        min_x = min_x.min((c.x - c.r) * scale);
        min_y = min_y.min((c.y - c.r) * scale);
        max_x = max_x.max((c.x + c.r) * scale);
        max_y = max_y.max((c.y + c.r) * scale);
    }
    for region in &regions {
        let mut wide: f64 = 0.0;
        let mut tall = 0.0;
        for line in &region.lines {
            wide = wide.max(line.label.width);
            tall += line.label.height;
        }
        min_x = min_x.min(region.spot.x * scale - wide / 2.0);
        max_x = max_x.max(region.spot.x * scale + wide / 2.0);
        min_y = min_y.min(region.spot.y * scale - tall / 2.0);
        max_y = max_y.max(region.spot.y * scale + tall / 2.0);
    }
    if circles.is_empty() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 0.0;
        max_y = 0.0;
    }

    let title = layout_label(
        model.title.as_deref().map(normalize_text),
        false,
        size,
        4000.0,
        BOLD,
        None,
    );
    let title_height = if title.height > 0.0 {
        title.height + 12.0
    } else {
        0.0
    };
    let inner = (max_x - min_x).max(title.width);
    let ox = pad + (inner - (max_x - min_x)) / 2.0 - min_x;
    let oy = pad + title_height - min_y;
    let width = (inner + 2.0 * pad).ceil();
    let height = (max_y - min_y + title_height + 2.0 * pad).ceil();
    let placed: Vec<Circle> = circles
        .iter()
        .map(|c| Circle {
            id: c.id.clone(),
            r: c.r * scale,
            x: c.x * scale + ox,
            y: c.y * scale + oy,
        })
        .collect();

    let mut sets = String::new();
    for (i, c) in placed.iter().enumerate() {
        let color = series_color(i64::try_from(i).unwrap_or(0));
        let style = region_at
            .get([c.id.clone()].as_slice())
            .and_then(|&at| regions.get(at))
            .map_or("", |region| region.style.shape.as_str());
        sets.push_str("<circle class=\"ondine-venn-set\" data-id=\"");
        sets.push_str(&esc_text(&c.id));
        sets.push_str("\" cx=\"");
        push_num(&mut sets, c.x);
        sets.push_str("\" cy=\"");
        push_num(&mut sets, c.y);
        sets.push_str("\" r=\"");
        push_num(&mut sets, c.r);
        sets.push_str("\" fill=\"");
        sets.push_str(&color);
        sets.push_str("\" fill-opacity=\"0.2\" stroke=\"");
        sets.push_str(&color);
        sets.push('"');
        sets.push_str(style);
        sets.push_str("/>");
    }

    let mut overlaps = String::new();
    let mut labels = String::new();
    for region in &regions {
        let id = esc_text(&region.sets.join("|")).into_owned();
        // An overlap is only drawn when a style gives it a look of its own. Otherwise the circles
        // show it.
        if region.sets.len() > 1 && !region.style.shape.is_empty() {
            let shared: Vec<Circle> = region
                .inside
                .iter()
                .filter_map(|&i| placed.get(i).cloned())
                .collect();
            let d = region_path(&shared);
            if !d.is_empty() {
                overlaps.push_str("<path class=\"ondine-venn-overlap\" data-id=\"");
                overlaps.push_str(&id);
                overlaps.push_str("\" d=\"");
                overlaps.push_str(&d);
                overlaps.push_str("\" fill=\"none\"");
                overlaps.push_str(&region.style.shape);
                overlaps.push_str("/>");
            }
        }
        let total = region
            .lines
            .iter()
            .fold(0.0, |total, line| total + line.label.height);
        if total == 0.0 {
            continue;
        }
        let x = region.spot.x * scale + ox;
        let mut y = region.spot.y * scale + oy - total / 2.0;
        let mut content = String::new();
        for line in &region.lines {
            let mut attrs = String::new();
            match &line.id {
                None => {
                    attrs.push_str(" class=\"ondine-label\"");
                    if line.size != size {
                        attrs.push_str(" font-size=\"");
                        push_number(&mut attrs, line.size);
                        attrs.push('"');
                    }
                    attrs.push_str(&region.style.text);
                }
                Some(id) => {
                    attrs.push_str(" class=\"ondine-venn-text\" data-id=\"");
                    attrs.push_str(&esc_text(id));
                    attrs.push_str("\" font-size=\"");
                    push_number(&mut attrs, tiny);
                    attrs.push_str("\" fill=\"var(--_m)\"");
                    attrs.push_str(&style_of(&styles, std::slice::from_ref(id)).text);
                }
            }
            content.push_str(&label_svg(
                &line.label,
                x,
                y + line.label.height / 2.0,
                &attrs,
                None,
            ));
            y += line.label.height;
        }
        labels.push_str("<g class=\"ondine-venn-region\" data-id=\"");
        labels.push_str(&id);
        labels.push_str("\">");
        labels.push_str(&content);
        labels.push_str("</g>");
    }

    let mut content = label_svg(
        &title,
        width / 2.0,
        pad + title.height / 2.0,
        " class=\"ondine-title\" font-weight=\"var(--_tw)\"",
        None,
    );
    content.push_str("<g class=\"ondine-venn-sets\">");
    content.push_str(&sets);
    content.push_str("</g>");
    if !overlaps.is_empty() {
        content.push_str("<g class=\"ondine-venn-overlaps\">");
        content.push_str(&overlaps);
        content.push_str("</g>");
    }
    if !labels.is_empty() {
        content.push_str("<g class=\"ondine-venn-labels\">");
        content.push_str(&labels);
        content.push_str("</g>");
    }

    let svg = svg_document(
        "venn",
        width,
        height,
        size,
        options,
        Accessible::default(),
        &content,
    );
    Rendered {
        height,
        links: Vec::new(),
        svg,
        width,
    }
}
