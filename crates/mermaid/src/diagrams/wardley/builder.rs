//! Collects a Wardley map's nodes, links, and decorations as Mermaid's database does. Port of
//! Pele's `src/diagrams/wardley/builder.ts`.

use std::collections::BTreeMap;

/// Which way a link carries flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Both ways.
    Bidirectional,
    /// From target to source.
    Backward,
    /// From source to target.
    Forward,
}

impl Flow {
    /// The flow as Pele names it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Backward => "backward",
            Self::Bidirectional => "bidirectional",
            Self::Forward => "forward",
        }
    }
}

/// A node. Coordinates are percentages: `x` is evolution, `y` is visibility.
#[derive(Clone, Debug, Default)]
pub(crate) struct WardleyNode {
    /// `anchor`, `component`, or `pipeline-component`.
    pub(crate) class_name: Option<&'static str>,
    /// The id.
    pub(crate) id: String,
    /// Whether the node is a pipeline component.
    pub(crate) in_pipeline: bool,
    /// Whether `inertia` was given, when a statement said.
    pub(crate) inertia: Option<bool>,
    /// Whether the node owns a pipeline.
    pub(crate) is_pipeline_parent: bool,
    /// The label.
    pub(crate) label: String,
    /// The label's horizontal offset.
    pub(crate) label_offset_x: Option<f64>,
    /// The label's vertical offset.
    pub(crate) label_offset_y: Option<f64>,
    /// `build`, `buy`, `outsource`, or `market`.
    pub(crate) source_strategy: Option<String>,
    /// Evolution, in percent. Pele allows it to be missing, but every node the parser adds has
    /// one, so the `build` check for it is left out.
    pub(crate) x: f64,
    /// Visibility, in percent.
    pub(crate) y: f64,
}

/// What [`WardleyBuilder::add_node`] sets. `None` in the doubled options keeps the existing
/// value, as a key missing from Pele's object does.
#[derive(Clone, Debug)]
#[expect(
    clippy::option_option,
    reason = "Pele tells a missing key from an `undefined` one"
)]
pub(crate) struct NodeInput {
    /// The class name.
    pub(crate) class_name: &'static str,
    /// The id.
    pub(crate) id: String,
    /// The inertia, when the statement sets it.
    pub(crate) inertia: Option<Option<bool>>,
    /// The label.
    pub(crate) label: String,
    /// The label's horizontal offset.
    pub(crate) label_offset_x: Option<f64>,
    /// The label's vertical offset.
    pub(crate) label_offset_y: Option<f64>,
    /// The source strategy, when the statement sets it.
    pub(crate) source_strategy: Option<Option<String>>,
    /// Evolution.
    pub(crate) x: f64,
    /// Visibility.
    pub(crate) y: f64,
}

/// A link.
#[derive(Clone, Debug)]
pub(crate) struct WardleyLink {
    /// Whether the line is dashed.
    pub(crate) dashed: bool,
    /// The flow.
    pub(crate) flow: Option<Flow>,
    /// The label.
    pub(crate) label: Option<String>,
    /// The source node id.
    pub(crate) source: String,
    /// The target node id.
    pub(crate) target: String,
}

/// Where a component is heading.
#[derive(Clone, Debug)]
pub(crate) struct WardleyTrend {
    /// The node id.
    pub(crate) node_id: String,
    /// The target evolution.
    pub(crate) target_x: f64,
    /// The target visibility.
    pub(crate) target_y: f64,
}

/// A pipeline.
#[derive(Clone, Debug)]
pub(crate) struct WardleyPipeline {
    /// The component ids.
    pub(crate) component_ids: Vec<String>,
    /// The parent node id.
    pub(crate) node_id: String,
}

/// A numbered annotation.
#[derive(Clone, Debug)]
pub(crate) struct WardleyAnnotation {
    /// Places the number is drawn, as `(x, y)`.
    pub(crate) coordinates: Vec<(f64, f64)>,
    /// The number.
    pub(crate) number: f64,
    /// The text.
    pub(crate) text: Option<String>,
}

/// A note.
#[derive(Clone, Debug)]
pub(crate) struct WardleyNote {
    /// The text.
    pub(crate) text: String,
    /// Evolution.
    pub(crate) x: f64,
    /// Visibility.
    pub(crate) y: f64,
}

/// An accelerator or deaccelerator.
#[derive(Clone, Debug)]
pub(crate) struct WardleyForce {
    /// The name.
    pub(crate) name: String,
    /// Evolution.
    pub(crate) x: f64,
    /// Visibility.
    pub(crate) y: f64,
}

/// The axes. Pele's `xLabel` and `yLabel` are never set by the parser, so they are left out.
#[derive(Clone, Debug, Default)]
pub(crate) struct WardleyAxes {
    /// The right edge of each stage, from 0 to 1.
    pub(crate) stage_boundaries: Option<Vec<f64>>,
    /// The stage names.
    pub(crate) stages: Option<Vec<String>>,
}

/// What [`WardleyBuilder::build`] returns: Pele's `WardleyBuildResult`.
#[derive(Clone, Debug, Default)]
pub(crate) struct WardleyBuildResult {
    /// Accelerators.
    pub(crate) accelerators: Vec<WardleyForce>,
    /// Annotations.
    pub(crate) annotations: Vec<WardleyAnnotation>,
    /// Where the annotation list goes, as `(x, y)`.
    pub(crate) annotations_box: Option<(f64, f64)>,
    /// The axes.
    pub(crate) axes: WardleyAxes,
    /// Deaccelerators.
    pub(crate) deaccelerators: Vec<WardleyForce>,
    /// Links.
    pub(crate) links: Vec<WardleyLink>,
    /// Nodes, in the order they were first added.
    pub(crate) nodes: Vec<WardleyNode>,
    /// Notes.
    pub(crate) notes: Vec<WardleyNote>,
    /// Pipelines, in the order their parents first started one.
    pub(crate) pipelines: Vec<WardleyPipeline>,
    /// The size, as `(width, height)`.
    pub(crate) size: Option<(f64, f64)>,
    /// Trends, in the order their nodes first got one.
    pub(crate) trends: Vec<WardleyTrend>,
}

/// Mermaid's Wardley database. Pele's `Map`s become vectors in insertion order with an index by
/// id.
#[derive(Debug, Default)]
pub(crate) struct WardleyBuilder {
    /// Node index by id.
    ids: BTreeMap<String, usize>,
    /// The first node id of each label, built on demand.
    labels: Option<BTreeMap<String, String>>,
    /// Pipeline index by parent id.
    pipeline_ids: BTreeMap<String, usize>,
    /// What `build` returns.
    result: WardleyBuildResult,
    /// Trend index by node id.
    trend_ids: BTreeMap<String, usize>,
}

impl WardleyBuilder {
    /// Adds a node, or updates one with the same id. An update keeps the class and label offsets
    /// unless the new node sets them.
    pub(crate) fn add_node(&mut self, node: NodeInput) {
        let nodes = &mut self.result.nodes;
        let at = *self.ids.entry(node.id.clone()).or_insert_with(|| {
            nodes.push(WardleyNode::default());
            nodes.len() - 1
        });
        if let Some(existing) = nodes.get_mut(at) {
            existing.id = node.id;
            existing.label = node.label;
            existing.x = node.x;
            existing.y = node.y;
            existing.class_name = Some(node.class_name);
            if node.label_offset_x.is_some() {
                existing.label_offset_x = node.label_offset_x;
            }
            if node.label_offset_y.is_some() {
                existing.label_offset_y = node.label_offset_y;
            }
            if let Some(inertia) = node.inertia {
                existing.inertia = inertia;
            }
            if let Some(strategy) = node.source_strategy {
                existing.source_strategy = strategy;
            }
        }
        self.labels = None;
    }

    /// Adds a link.
    pub(crate) fn add_link(&mut self, link: WardleyLink) {
        self.result.links.push(link);
    }

    /// Sets the trend of a node, replacing an earlier one in its place.
    pub(crate) fn add_trend(&mut self, trend: WardleyTrend) {
        if let Some(&at) = self.trend_ids.get(&trend.node_id)
            && let Some(slot) = self.result.trends.get_mut(at)
        {
            *slot = trend;
            return;
        }
        self.trend_ids
            .insert(trend.node_id.clone(), self.result.trends.len());
        self.result.trends.push(trend);
    }

    /// Starts an empty pipeline owned by a node, replacing an earlier one in its place.
    pub(crate) fn start_pipeline(&mut self, node_id: &str) {
        let pipeline = WardleyPipeline {
            component_ids: Vec::new(),
            node_id: node_id.to_owned(),
        };
        if let Some(&at) = self.pipeline_ids.get(node_id)
            && let Some(slot) = self.result.pipelines.get_mut(at)
        {
            *slot = pipeline;
        } else {
            self.pipeline_ids
                .insert(node_id.to_owned(), self.result.pipelines.len());
            self.result.pipelines.push(pipeline);
        }
        if let Some(node) = self.node_mut(node_id) {
            node.is_pipeline_parent = true;
        }
    }

    /// Adds a component to a pipeline.
    pub(crate) fn add_pipeline_component(&mut self, pipeline_node_id: &str, component_id: &str) {
        if let Some(&at) = self.pipeline_ids.get(pipeline_node_id)
            && let Some(pipeline) = self.result.pipelines.get_mut(at)
        {
            pipeline.component_ids.push(component_id.to_owned());
        }
        if let Some(node) = self.node_mut(component_id) {
            node.in_pipeline = true;
        }
    }

    /// Adds an annotation.
    pub(crate) fn add_annotation(&mut self, annotation: WardleyAnnotation) {
        self.result.annotations.push(annotation);
    }

    /// Adds a note.
    pub(crate) fn add_note(&mut self, note: WardleyNote) {
        self.result.notes.push(note);
    }

    /// Adds an accelerator.
    pub(crate) fn add_accelerator(&mut self, accelerator: WardleyForce) {
        self.result.accelerators.push(accelerator);
    }

    /// Adds a deaccelerator.
    pub(crate) fn add_deaccelerator(&mut self, deaccelerator: WardleyForce) {
        self.result.deaccelerators.push(deaccelerator);
    }

    /// Places the annotation list.
    pub(crate) fn set_annotations_box(&mut self, x: f64, y: f64) {
        self.result.annotations_box = Some((x, y));
    }

    /// Sets the axes.
    pub(crate) fn set_axes(&mut self, axes: WardleyAxes) {
        self.result.axes = axes;
    }

    /// Sets the size.
    pub(crate) fn set_size(&mut self, width: f64, height: f64) {
        self.result.size = Some((width, height));
    }

    /// The node with `id`.
    pub(crate) fn get_node(&self, id: &str) -> Option<&WardleyNode> {
        self.result.nodes.get(*self.ids.get(id)?)
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut WardleyNode> {
        self.result.nodes.get_mut(*self.ids.get(id)?)
    }

    /// A name is a node id, or else the label of the first node that has it. Pipeline components
    /// have ids like `Parent_Child` and are linked by label.
    pub(crate) fn resolve_node_id(&mut self, name: &str) -> String {
        if self.ids.contains_key(name) {
            return name.to_owned();
        }
        let nodes = &self.result.nodes;
        let labels = self.labels.get_or_insert_with(|| {
            let mut labels = BTreeMap::new();
            for node in nodes {
                labels
                    .entry(node.label.clone())
                    .or_insert_with(|| node.id.clone());
            }
            labels
        });
        labels.get(name).cloned().unwrap_or_else(|| name.to_owned())
    }

    /// The collected map.
    pub(crate) fn build(self) -> WardleyBuildResult {
        self.result
    }
}
