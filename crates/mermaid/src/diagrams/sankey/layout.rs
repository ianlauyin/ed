//! Sankey layout: columns by distance from a source, node heights by flow, and a few relaxation
//! passes that let bands cross less. Port of Pele's `src/diagrams/sankey/layout.ts`.

use std::cmp::Ordering;

use super::db::SankeyDb;
use crate::error::{Code, Error};

/// Which column a node goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alignment {
    /// Sources pulled next to their first target.
    Center,
    /// Sinks in the last column, everything else by depth.
    Justify,
    /// By depth from a source.
    Left,
    /// By distance to a sink, from the right.
    Right,
}

/// What the layout is fitted to.
pub(crate) struct LayoutOptions {
    /// Column placement.
    pub(crate) alignment: Alignment,
    /// Chart height.
    pub(crate) height: f64,
    /// Height of a label line; thin nodes are kept at least this far apart where there is room.
    pub(crate) label_height: f64,
    /// Space between nodes of a column.
    pub(crate) node_padding: f64,
    /// Width of a node bar.
    pub(crate) node_width: f64,
    /// Chart width.
    pub(crate) width: f64,
}

/// A placed node.
#[derive(Clone, Debug, Default)]
pub(crate) struct PlacedNode {
    /// Bar height.
    pub(crate) height: f64,
    /// Column index.
    pub(crate) layer: usize,
    /// Total flow through the node, in the diagram's units.
    pub(crate) value: f64,
    /// Left edge.
    pub(crate) x: f64,
    /// Top edge.
    pub(crate) y: f64,
}

/// A placed link.
#[derive(Clone, Debug)]
pub(crate) struct PlacedLink {
    /// Source node index.
    pub(crate) source: usize,
    /// Vertical center of the band where it leaves its source.
    pub(crate) source_y: f64,
    /// Target node index.
    pub(crate) target: usize,
    /// Vertical center of the band where it enters its target.
    pub(crate) target_y: f64,
    /// Band thickness.
    pub(crate) thickness: f64,
    /// The flow, or 0 when it is not a positive finite number.
    pub(crate) value: f64,
}

/// Relaxation passes.
const PASSES: i32 = 6;

/// Sorts as Pele's `sort` does: insertion sort for short lists, the built-in stable sort otherwise.
/// Comparators here never tie, so both orders agree with JavaScript's.
fn sort(list: &mut [usize], compare: impl Fn(usize, usize) -> f64) {
    if list.len() > 8 {
        list.sort_by(|&a, &b| {
            let d = compare(a, b);
            if d > 0.0 {
                Ordering::Greater
            } else if d < 0.0 {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });
        return;
    }
    for i in 1..list.len() {
        let Some(&item) = list.get(i) else {
            continue;
        };
        let mut j = i;
        while j > 0
            && let Some(&previous) = list.get(j - 1)
            && compare(previous, item) > 0.0
        {
            if let Some(slot) = list.get_mut(j) {
                *slot = previous;
            }
            j -= 1;
        }
        if let Some(slot) = list.get_mut(j) {
            *slot = item;
        }
    }
}

/// `x || fallback` for a JavaScript number: `fallback` when `x` is 0 or `NaN`.
fn or(x: f64, fallback: f64) -> f64 {
    if x == 0.0 || x.is_nan() { fallback } else { x }
}

#[expect(clippy::cast_precision_loss, reason = "indexes are small")]
fn index_diff(a: usize, b: usize) -> f64 {
    a as f64 - b as f64
}

/// Places nodes in columns by their distance from a source, sizes them by their flow, and then
/// lets each column settle next to its neighbours so that the bands cross less.
///
/// # Errors
///
/// Returns a semantic error naming a node on a cycle.
#[expect(clippy::cast_precision_loss, reason = "counts are small")]
pub(crate) fn layout_sankey(
    model: &SankeyDb,
    options: &LayoutOptions,
) -> Result<(Vec<PlacedNode>, Vec<PlacedLink>), Error> {
    let n = model.nodes.len();
    let mut links: Vec<PlacedLink> = model
        .links
        .iter()
        .map(|link| PlacedLink {
            source: link.source as usize,
            source_y: 0.0,
            target: link.target as usize,
            target_y: 0.0,
            thickness: 0.0,
            value: if link.value.is_finite() && link.value > 0.0 {
                link.value
            } else {
                0.0
            },
        })
        .collect();
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, link) in links.iter().enumerate() {
        if let Some(list) = outgoing.get_mut(link.source) {
            list.push(k);
        }
        if let Some(list) = incoming.get_mut(link.target) {
            list.push(k);
        }
    }
    let source_of = |k: usize| links.get(k).map_or(0, |link| link.source);
    let target_of = |k: usize| links.get(k).map_or(0, |link| link.target);

    // Longest distance from a source, in topological order; what is left over lies on a cycle.
    let mut depth = vec![0usize; n];
    let mut pending: Vec<usize> = incoming.iter().map(Vec::len).collect();
    let mut order: Vec<usize> = (0..n).filter(|&i| pending.get(i) == Some(&0)).collect();
    let mut q = 0;
    while let Some(&i) = order.get(q) {
        let d = depth.get(i).copied().unwrap_or(0);
        for &k in outgoing.get(i).map_or(&[][..], Vec::as_slice) {
            let t = target_of(k);
            if let Some(slot) = depth.get_mut(t)
                && *slot < d + 1
            {
                *slot = d + 1;
            }
            if let Some(slot) = pending.get_mut(t) {
                *slot = slot.saturating_sub(1);
                if *slot == 0 {
                    order.push(t);
                }
            }
        }
        q += 1;
    }
    if order.len() < n {
        // Every node left over has a predecessor that is left over too; walking back finds the loop.
        let mut stuck = pending.iter().position(|&p| p != 0).unwrap_or(0);
        let mut seen = vec![false; n];
        while seen.get(stuck) == Some(&false) {
            if let Some(slot) = seen.get_mut(stuck) {
                *slot = true;
            }
            for &k in incoming.get(stuck).map_or(&[][..], Vec::as_slice) {
                let s = source_of(k);
                if pending.get(s).is_some_and(|&p| p > 0) {
                    stuck = s;
                    break;
                }
            }
        }
        let id = model.nodes.get(stuck).map_or("", |node| node.id.as_str());
        let mut message = String::from("Sankey diagrams cannot have circular links, and \"");
        message.push_str(id);
        message.push_str("\" is part of one.");
        let mut error = Error::new(Code::Semantic, message);
        error.r#type = Some("sankey");
        return Err(error);
    }
    let layers = depth.iter().map(|&d| d + 1).max().unwrap_or(1).max(1);
    let mut reach = vec![0usize; n];
    for &i in order.iter().rev() {
        for &k in outgoing.get(i).map_or(&[][..], Vec::as_slice) {
            let r = reach.get(target_of(k)).copied().unwrap_or(0) + 1;
            if let Some(slot) = reach.get_mut(i)
                && *slot < r
            {
                *slot = r;
            }
        }
    }

    // Flows are scaled to the largest one, so that sums of huge values stay finite.
    let largest = links.iter().fold(0.0_f64, |largest, link| {
        if link.value > largest {
            link.value
        } else {
            largest
        }
    });
    let unit = if largest > 0.0 { 1.0 / largest } else { 0.0 };

    let mut nodes: Vec<PlacedNode> = Vec::with_capacity(n);
    let mut flow = vec![0.0_f64; n];
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); layers];
    let step = if layers > 1 {
        (options.width - options.node_width) / (layers - 1) as f64
    } else {
        0.0
    };
    let empty: &[usize] = &[];
    for i in 0..n {
        let ins = incoming.get(i).map_or(empty, Vec::as_slice);
        let outs = outgoing.get(i).map_or(empty, Vec::as_slice);
        let d = depth.get(i).copied().unwrap_or(0);
        let layer = match options.alignment {
            Alignment::Left => d,
            Alignment::Right => (layers - 1).saturating_sub(reach.get(i).copied().unwrap_or(0)),
            Alignment::Center => {
                if ins.is_empty() && !outs.is_empty() {
                    let mut layer = layers;
                    for &k in outs {
                        let t = depth.get(target_of(k)).copied().unwrap_or(0);
                        if t.saturating_sub(1) < layer {
                            layer = t.saturating_sub(1);
                        }
                    }
                    layer
                } else {
                    d
                }
            }
            Alignment::Justify => {
                if outs.is_empty() {
                    layers - 1
                } else {
                    d
                }
            }
        };
        let mut into = 0.0;
        let mut out = 0.0;
        let mut value = 0.0;
        let mut value_out = 0.0;
        for &k in ins {
            let v = links.get(k).map_or(0.0, |link| link.value);
            into += v * unit;
            value += v;
        }
        for &k in outs {
            let v = links.get(k).map_or(0.0, |link| link.value);
            out += v * unit;
            value_out += v;
        }
        if let Some(slot) = flow.get_mut(i) {
            *slot = f64::max(into, out);
        }
        nodes.push(PlacedNode {
            height: 0.0,
            layer,
            value: f64::max(value, value_out),
            x: layer as f64 * step,
            y: 0.0,
        });
        if let Some(column) = columns.get_mut(layer) {
            column.push(i);
        }
    }
    let flow_of = |i: usize| flow.get(i).copied().unwrap_or(0.0);

    let tallest = columns.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let gap = if tallest > 1 {
        f64::min(options.node_padding, options.height / (tallest - 1) as f64)
    } else {
        options.node_padding
    };
    let mut scale = f64::INFINITY;
    for column in &columns {
        let sum: f64 = column.iter().map(|&i| flow_of(i)).sum();
        if sum > 0.0 {
            scale = f64::min(
                scale,
                (options.height - (column.len() as f64 - 1.0) * gap) / sum,
            );
        }
    }
    if !scale.is_finite() || scale < 0.0 {
        scale = 0.0;
    }

    // A thin node still needs room for its label. Where every column can afford it, each node
    // claims at least a label's height, and the flows are scaled down to make that fit.
    let mut claim = f64::max(0.0, options.label_height - gap);
    let fits = |s: f64, claim: f64| {
        columns.iter().all(|column| {
            let mut extent = (column.len() as f64 - 1.0) * gap;
            for &i in column {
                extent += f64::max(flow_of(i) * s, claim);
            }
            extent <= options.height + 1e-6
        })
    };
    if claim > 0.0 && !fits(scale, claim) {
        if fits(0.0, claim) {
            let mut low = 0.0;
            for _ in 0..40 {
                #[expect(
                    clippy::manual_midpoint,
                    reason = "JavaScript rounds the sum, then halves it"
                )]
                let mid = (low + scale) / 2.0;
                if fits(mid, claim) {
                    low = mid;
                } else {
                    scale = mid;
                }
            }
            scale = low;
        } else {
            claim = 0.0;
        }
    }

    // The room a node keeps clear above and below itself.
    let mut margin = vec![0.0_f64; n];
    for link in &mut links {
        link.thickness = link.value * unit * scale;
    }
    for column in &columns {
        let mut y = 0.0;
        for &i in column {
            if let (Some(node), Some(m)) = (nodes.get_mut(i), margin.get_mut(i)) {
                node.height = flow_of(i) * scale;
                *m = f64::max(0.0, claim - node.height) / 2.0;
                node.y = y + *m;
                y += node.height + 2.0 * *m + gap;
            }
        }
        // Spare room is shared out between the nodes of a column.
        let spare = (options.height - y + gap) / (column.len() + 1) as f64;
        for (k, &i) in column.iter().enumerate() {
            if let Some(node) = nodes.get_mut(i) {
                node.y += spare * (k + 1) as f64;
            }
        }
    }

    let mut state = Settle {
        gap,
        height: options.height,
        incoming,
        links: &links,
        margin,
        nodes,
        outgoing,
        source_offset: vec![0.0; links.len()],
        target_offset: vec![0.0; links.len()],
    };

    state.stack_all();
    for pass in 0..PASSES {
        let pull = 0.99_f64.powf(f64::from(pass));
        for l in (0..layers.saturating_sub(1)).rev() {
            if let Some(column) = columns.get_mut(l) {
                state.settle(column, pull, false);
            }
        }
        state.stack_all();
        for l in 1..layers {
            if let Some(column) = columns.get_mut(l) {
                state.settle(column, pull, true);
            }
        }
        state.stack_all();
    }

    let Settle {
        nodes,
        source_offset,
        target_offset,
        ..
    } = state;
    let mut placed = links.clone();
    for (k, link) in placed.iter_mut().enumerate() {
        link.source_y = nodes.get(link.source).map_or(0.0, |node| node.y)
            + source_offset.get(k).copied().unwrap_or(0.0);
        link.target_y = nodes.get(link.target).map_or(0.0, |node| node.y)
            + target_offset.get(k).copied().unwrap_or(0.0);
    }

    Ok((nodes, placed))
}

/// The relaxation state: node positions and where each band meets its nodes.
struct Settle<'l> {
    gap: f64,
    height: f64,
    incoming: Vec<Vec<usize>>,
    links: &'l [PlacedLink],
    margin: Vec<f64>,
    nodes: Vec<PlacedNode>,
    outgoing: Vec<Vec<usize>>,
    /// Offsets of each band's middle from the top of its source node.
    source_offset: Vec<f64>,
    /// Offsets of each band's middle from the top of its target node.
    target_offset: Vec<f64>,
}

impl Settle<'_> {
    fn center(nodes: &[PlacedNode], i: usize) -> f64 {
        nodes.get(i).map_or(0.0, |node| node.y + node.height / 2.0)
    }

    fn source(&self, k: usize) -> usize {
        self.links.get(k).map_or(0, |link| link.source)
    }

    fn target(&self, k: usize) -> usize {
        self.links.get(k).map_or(0, |link| link.target)
    }

    fn thick(&self, k: usize) -> f64 {
        self.links.get(k).map_or(0.0, |link| link.thickness)
    }

    /// Bands leave a node in the order of where they go, so they do not cross at it.
    fn stack_out(&mut self, i: usize) {
        let mut list = std::mem::take(self.outgoing.get_mut(i).unwrap_or(&mut Vec::new()));
        let nodes = &self.nodes;
        let links = self.links;
        let target = |k: usize| links.get(k).map_or(0, |link| link.target);
        sort(&mut list, |a, b| {
            or(
                Self::center(nodes, target(a)) - Self::center(nodes, target(b)),
                index_diff(a, b),
            )
        });
        let mut y = 0.0;
        for &k in &list {
            let t = self.thick(k);
            if let Some(slot) = self.source_offset.get_mut(k) {
                *slot = y + t / 2.0;
            }
            y += t;
        }
        if let Some(slot) = self.outgoing.get_mut(i) {
            *slot = list;
        }
    }

    /// Bands enter a node in the order of where they come from.
    fn stack_in(&mut self, i: usize) {
        let mut list = std::mem::take(self.incoming.get_mut(i).unwrap_or(&mut Vec::new()));
        let nodes = &self.nodes;
        let links = self.links;
        let source = |k: usize| links.get(k).map_or(0, |link| link.source);
        sort(&mut list, |a, b| {
            or(
                Self::center(nodes, source(a)) - Self::center(nodes, source(b)),
                index_diff(a, b),
            )
        });
        let mut y = 0.0;
        for &k in &list {
            let t = self.thick(k);
            if let Some(slot) = self.target_offset.get_mut(k) {
                *slot = y + t / 2.0;
            }
            y += t;
        }
        if let Some(slot) = self.incoming.get_mut(i) {
            *slot = list;
        }
    }

    fn stack_all(&mut self) {
        for i in 0..self.nodes.len() {
            self.stack_out(i);
            self.stack_in(i);
        }
    }

    /// Moves each node of a column towards where its bands would run level, then restores the
    /// order and the gaps. `forward` looks at the bands coming in, otherwise at those going out.
    #[expect(clippy::cast_precision_loss, reason = "counts are small")]
    fn settle(&mut self, column: &mut [usize], pull: f64, forward: bool) {
        for &i in column.iter() {
            let empty = if forward {
                self.incoming.get(i).is_none_or(Vec::is_empty)
            } else {
                self.outgoing.get(i).is_none_or(Vec::is_empty)
            };
            if empty {
                continue;
            }
            if forward {
                self.stack_in(i);
            } else {
                self.stack_out(i);
            }
            let list = if forward {
                self.incoming.get(i)
            } else {
                self.outgoing.get(i)
            }
            .map_or(&[][..], Vec::as_slice);
            let height = self.nodes.get(i).map_or(0.0, |node| node.height);
            let mut weighted = 0.0;
            let mut weight = 0.0;
            let mut plain = 0.0;
            for &k in list {
                let y_of = |j: usize| self.nodes.get(j).map_or(0.0, |node| node.y);
                let so = self.source_offset.get(k).copied().unwrap_or(0.0);
                let to = self.target_offset.get(k).copied().unwrap_or(0.0);
                let level = if forward {
                    y_of(self.source(k)) + so - to
                } else {
                    y_of(self.target(k)) + to - so
                };
                let t = self.thick(k);
                weighted += level * t;
                weight += t;
                let other = if forward {
                    self.source(k)
                } else {
                    self.target(k)
                };
                plain += Self::center(&self.nodes, other) - height / 2.0;
            }
            let wanted = if weight > 0.0 {
                weighted / weight
            } else {
                plain / list.len() as f64
            };
            if let Some(node) = self.nodes.get_mut(i) {
                node.y += (wanted - node.y) * pull;
            }
        }

        let nodes = &self.nodes;
        sort(column, |a, b| {
            or(
                Self::center(nodes, a) - Self::center(nodes, b),
                index_diff(a, b),
            )
        });
        let mut y = 0.0;
        for &i in column.iter() {
            let m = self.margin.get(i).copied().unwrap_or(0.0);
            if let Some(node) = self.nodes.get_mut(i) {
                if node.y - m < y {
                    node.y = y + m;
                }
                y = node.y + node.height + m + self.gap;
            }
        }
        y = self.height;
        for &i in column.iter().rev() {
            let m = self.margin.get(i).copied().unwrap_or(0.0);
            if let Some(node) = self.nodes.get_mut(i) {
                if node.y + node.height + m > y {
                    node.y = y - node.height - m;
                }
                y = node.y - m - self.gap;
            }
        }
    }
}
