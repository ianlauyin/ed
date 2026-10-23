//! Diagram types and the contract each one implements.

pub(crate) mod common {
    pub(crate) mod acc_descr;
    pub(crate) mod caseless;
    pub(crate) mod fit;
    pub(crate) mod fit_text;
    pub(crate) mod fit_width;
    pub(crate) mod generics;
    pub(crate) mod icon;
    pub(crate) mod literal_label;
    pub(crate) mod outline;
    pub(crate) mod scan;
    pub(crate) mod tokens;
}

pub(crate) mod agentflow;
pub(crate) mod architecture;
pub(crate) mod block;
pub(crate) mod c4;
pub(crate) mod class;
pub(crate) mod cynefin;
pub(crate) mod er;
pub(crate) mod eventmodeling;
/// Swimlane diagrams draw through the flowchart, and agentflow reads its shapes.
pub(crate) mod flowchart;
pub(crate) mod gantt;
pub(crate) mod git;
pub(crate) mod info;
pub(crate) mod ishikawa;
pub(crate) mod journey;
pub(crate) mod kanban;
pub(crate) mod mindmap;
pub(crate) mod packet;
pub(crate) mod pie;
pub(crate) mod quadrant;
pub(crate) mod radar;
pub(crate) mod railroad;
pub(crate) mod requirement;
pub(crate) mod sankey;
pub(crate) mod sequence;
pub(crate) mod state;
pub(crate) mod swimlane;
pub(crate) mod timeline;
pub(crate) mod treemap;
pub(crate) mod treeview;
pub(crate) mod usecase;
pub(crate) mod venn;
pub(crate) mod wardley;
pub(crate) mod xychart;

use crate::error::Error;
use crate::svg::root::RenderOptions;
use crate::util::url::LinkInfo;
use crate::util::url::UrlOptions;
use crate::yaml::Value;

/// What a diagram type receives, as Pele's `Diagram.parse` does.
pub(crate) struct Input<'a> {
    /// The diagram configuration from front matter and directives, as a YAML mapping.
    pub(crate) config: &'a Value,
    /// Front matter lines removed before `source`, for types that report source positions.
    pub(crate) line_offset: u32,
    /// The most edges a flowchart-like diagram may have.
    pub(crate) max_edges: usize,
    /// Preprocessed, entity-encoded source ending in a newline. It keeps `%%` comment lines
    /// when the type sets `keep_comments`.
    pub(crate) source: &'a str,
    /// The front matter title.
    pub(crate) title: Option<&'a str>,
}

/// A drawn diagram.
#[derive(Clone, Debug)]
pub(crate) struct Rendered {
    /// Height in pixels.
    pub(crate) height: f64,
    /// Links in drawing order.
    pub(crate) links: Vec<LinkInfo>,
    /// The SVG document.
    pub(crate) svg: String,
    /// Width in pixels.
    pub(crate) width: f64,
}

impl common::fit_width::Drawn for Rendered {
    fn width(&self) -> f64 {
        self.width
    }
}

/// A diagram type, as Pele's `Diagram` describes one.
#[derive(Clone, Copy)]
pub(crate) struct Diagram {
    /// Whether the type parses source with its `%%` comment lines kept.
    pub(crate) keep_comments: bool,
    /// Parses and draws the diagram.
    pub(crate) render: fn(&Input<'_>, &RenderOptions, &UrlOptions<'_>) -> Result<Rendered, Error>,
    /// The type's section in Mermaid's config, when it is not the type's own name.
    pub(crate) section: Option<&'static str>,
}

/// The diagram type named `type` (Pele's names), when this build includes it.
pub(crate) fn find(r#type: &str) -> Option<Diagram> {
    match r#type {
        "agentflow" => Some(agentflow::DIAGRAM),
        "architecture" => Some(architecture::DIAGRAM),
        "block" => Some(block::DIAGRAM),
        "c4" => Some(c4::DIAGRAM),
        "class" => Some(class::DIAGRAM),
        "cynefin" => Some(cynefin::DIAGRAM),
        "er" => Some(er::DIAGRAM),
        "eventmodeling" => Some(eventmodeling::DIAGRAM),
        "flowchart" => Some(flowchart::DIAGRAM),
        "gantt" => Some(gantt::DIAGRAM),
        "gitGraph" => Some(git::DIAGRAM),
        "info" => Some(info::DIAGRAM),
        "ishikawa" => Some(ishikawa::DIAGRAM),
        "journey" => Some(journey::DIAGRAM),
        "kanban" => Some(kanban::DIAGRAM),
        "mindmap" => Some(mindmap::DIAGRAM),
        "packet" => Some(packet::DIAGRAM),
        "pie" => Some(pie::DIAGRAM),
        "quadrantChart" => Some(quadrant::DIAGRAM),
        "radar" => Some(radar::DIAGRAM),
        "railroad" => Some(railroad::DIAGRAM),
        "requirement" => Some(requirement::DIAGRAM),
        "sankey" => Some(sankey::DIAGRAM),
        "sequence" => Some(sequence::DIAGRAM),
        "state" => Some(state::DIAGRAM),
        "swimlane" => Some(swimlane::DIAGRAM),
        "timeline" => Some(timeline::DIAGRAM),
        "treemap" => Some(treemap::DIAGRAM),
        "treeView" => Some(treeview::DIAGRAM),
        "usecase" => Some(usecase::DIAGRAM),
        "venn" => Some(venn::DIAGRAM),
        "wardley" => Some(wardley::DIAGRAM),
        "xychart" => Some(xychart::DIAGRAM),
        _ => None,
    }
}
