//! The architecture model and Mermaid's checks as it is built. Port of Pele's
//! `src/diagrams/architecture/db.ts`.

use std::collections::{BTreeMap, BTreeSet};

use super::parser::{ArchitectureAst, Direction, Side};
use crate::error::{Code, Error};
use crate::util::chars::is_space;

/// What a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
    /// A point where edges meet.
    Junction,
    /// A service with an icon and a title.
    Service,
}

/// A service or junction.
#[derive(Clone, Debug)]
pub(crate) struct ArchNode {
    /// The icon name, for a service.
    pub(crate) icon: Option<String>,
    /// Text drawn in place of an icon, for a service.
    pub(crate) icon_text: Option<String>,
    /// The id.
    pub(crate) id: String,
    /// The parent group's id.
    pub(crate) r#in: Option<String>,
    /// Service or junction.
    pub(crate) kind: NodeKind,
    /// The title, for a service.
    pub(crate) title: Option<String>,
}

/// A group of nodes and groups.
#[derive(Clone, Debug)]
pub(crate) struct ArchGroup {
    /// The icon name.
    pub(crate) icon: Option<String>,
    /// The id.
    pub(crate) id: String,
    /// The parent group's id.
    pub(crate) r#in: Option<String>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// An edge between two sides of two nodes.
#[derive(Clone, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors Mermaid's edge fields"
)]
pub(crate) struct ArchEdge {
    /// Side of the left-hand node.
    pub(crate) lhs_dir: Side,
    /// Whether the left-hand end crosses its group's border.
    pub(crate) lhs_group: bool,
    /// Left-hand node id.
    pub(crate) lhs_id: String,
    /// Whether the left-hand end has an arrowhead.
    pub(crate) lhs_into: bool,
    /// Side of the right-hand node.
    pub(crate) rhs_dir: Side,
    /// Whether the right-hand end crosses its group's border.
    pub(crate) rhs_group: bool,
    /// Right-hand node id.
    pub(crate) rhs_id: String,
    /// Whether the right-hand end has an arrowhead.
    pub(crate) rhs_into: bool,
    /// The label.
    pub(crate) title: Option<String>,
}

/// An `align` directive.
#[derive(Clone, Debug)]
pub(crate) struct ArchLayoutHint {
    /// Row or column.
    pub(crate) direction: Direction,
    /// Ids of the aligned nodes, in order.
    pub(crate) members: Vec<String>,
}

/// What an id is registered as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Registered {
    Group,
    Node,
}

impl Registered {
    fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Node => "node",
        }
    }
}

/// The architecture model: Pele's `ArchitectureDb`. Nodes and groups keep insertion order.
#[derive(Clone, Debug, Default)]
pub(crate) struct ArchitectureDb {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Edges in source order.
    pub(crate) edges: Vec<ArchEdge>,
    /// Groups in insertion order.
    pub(crate) groups: Vec<ArchGroup>,
    /// Alignments in source order.
    pub(crate) layout_hints: Vec<ArchLayoutHint>,
    /// Index of each node by id.
    node_index: BTreeMap<String, usize>,
    /// Services and junctions in insertion order.
    pub(crate) nodes: Vec<ArchNode>,
    /// What each id is.
    registered: BTreeMap<String, Registered>,
    /// The title.
    pub(crate) title: Option<String>,
}

/// A semantic error whose message is `parts` joined.
#[cold]
fn fail(parts: &[&str]) -> Error {
    let mut error = Error::new(Code::Semantic, parts.concat());
    error.r#type = Some("architecture");
    error
}

impl ArchitectureDb {
    /// The node with `id`.
    pub(crate) fn node(&self, id: &str) -> Option<&ArchNode> {
        self.position(id).and_then(|i| self.nodes.get(i))
    }

    /// The index in [`Self::nodes`] of the node with `id`.
    pub(crate) fn position(&self, id: &str) -> Option<usize> {
        self.node_index.get(id).copied()
    }

    fn register(&mut self, kind: &str, id: &str, parent: Option<&str>) -> Result<(), Error> {
        if let Some(other) = self.registered.get(id) {
            return Err(fail(&[
                "The ",
                kind,
                " id [",
                id,
                "] is already in use by another ",
                other.as_str(),
            ]));
        }
        if let Some(parent) = parent {
            if id == parent {
                return Err(fail(&[
                    "The ",
                    kind,
                    " [",
                    id,
                    "] cannot be placed within itself",
                ]));
            }
            match self.registered.get(parent) {
                None => {
                    return Err(fail(&[
                        "The ",
                        kind,
                        " [",
                        id,
                        "]'s parent does not exist. Please make sure the parent is created before this ",
                        kind,
                    ]));
                }
                Some(Registered::Node) => {
                    return Err(fail(&["The ", kind, " [", id, "]'s parent is not a group"]));
                }
                Some(Registered::Group) => {}
            }
        }
        let registered = if kind == "group" {
            Registered::Group
        } else {
            Registered::Node
        };
        self.registered.insert(id.to_owned(), registered);
        Ok(())
    }

    fn add_node(&mut self, node: ArchNode) {
        self.node_index.insert(node.id.clone(), self.nodes.len());
        self.nodes.push(node);
    }

    /// Adds a service.
    pub(crate) fn add_service(
        &mut self,
        id: String,
        icon: Option<String>,
        icon_text: Option<String>,
        title: Option<String>,
        parent: Option<String>,
    ) -> Result<(), Error> {
        self.register("service", &id, parent.as_deref())?;
        self.add_node(ArchNode {
            icon,
            icon_text,
            id,
            r#in: parent,
            kind: NodeKind::Service,
            title,
        });
        Ok(())
    }

    /// Adds a junction.
    pub(crate) fn add_junction(&mut self, id: String, parent: Option<String>) -> Result<(), Error> {
        self.register("junction", &id, parent.as_deref())?;
        self.add_node(ArchNode {
            icon: None,
            icon_text: None,
            id,
            r#in: parent,
            kind: NodeKind::Junction,
            title: None,
        });
        Ok(())
    }

    /// Adds a group.
    pub(crate) fn add_group(&mut self, group: ArchGroup) -> Result<(), Error> {
        self.register("group", &group.id, group.r#in.as_deref())?;
        self.groups.push(group);
        Ok(())
    }

    /// Adds an edge.
    pub(crate) fn add_edge(&mut self, edge: ArchEdge) -> Result<(), Error> {
        let ends = [
            ("left", edge.lhs_id.as_str(), edge.lhs_group),
            ("right", edge.rhs_id.as_str(), edge.rhs_group),
        ];
        for (side, id, _) in ends {
            if !self.registered.contains_key(id) {
                return Err(fail(&[
                    "The ",
                    side,
                    "-hand id [",
                    id,
                    "] does not yet exist. Please create the service/group before declaring an edge to it.",
                ]));
            }
        }
        // Mermaid lets a group id through the check above and then fails with a TypeError.
        for (side, id, _) in ends {
            if self.node(id).is_none() {
                return Err(fail(&[
                    "The ",
                    side,
                    "-hand id [",
                    id,
                    "] is a group. Add {group} to a service inside it instead.",
                ]));
            }
        }
        let parent = self
            .node(&edge.lhs_id)
            .and_then(|node| node.r#in.as_deref());
        if parent.is_some()
            && parent
                == self
                    .node(&edge.rhs_id)
                    .and_then(|node| node.r#in.as_deref())
        {
            for (side, id, crosses) in ends {
                if crosses {
                    return Err(fail(&[
                        "The ",
                        side,
                        "-hand id [",
                        id,
                        "] is modified to traverse the group boundary, but the edge does not pass through two groups.",
                    ]));
                }
            }
        }
        self.edges.push(edge);
        Ok(())
    }

    /// Adds an alignment.
    pub(crate) fn add_layout_hint(&mut self, hint: ArchLayoutHint) -> Result<(), Error> {
        if hint.members.len() < 2 {
            let mut message =
                String::from("An align directive requires at least two members; got ");
            crate::error::push_u32(
                &mut message,
                u32::try_from(hint.members.len()).unwrap_or(u32::MAX),
            );
            return Err(fail(&[&message]));
        }
        let mut seen = BTreeSet::new();
        for id in &hint.members {
            if self.registered.get(id.as_str()) != Some(&Registered::Node) {
                return Err(fail(&[
                    "align ",
                    hint.direction.as_str(),
                    " references [",
                    id,
                    "], which is not a service or junction",
                ]));
            }
            if !seen.insert(id.as_str()) {
                return Err(fail(&[
                    "align ",
                    hint.direction.as_str(),
                    " lists [",
                    id,
                    "] more than once",
                ]));
            }
        }
        self.layout_hints.push(hint);
        Ok(())
    }
}

/// `text.replace(/\n\s+/g, '\n')`.
fn squeeze_indents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' && chars.peek().is_some_and(|&c| is_space(u32::from(c))) {
            while chars.peek().is_some_and(|&c| is_space(u32::from(c))) {
                chars.next();
            }
        }
    }
    out
}

/// Mermaid's `populateDb`: every group, then services, junctions, edges, and alignments.
///
/// # Errors
///
/// Returns a semantic error for an id used twice, a missing or invalid parent, an edge to an
/// unknown id or a group, a group crossing that crosses nothing, or an invalid alignment.
pub(crate) fn populate(ast: ArchitectureAst, db: &mut ArchitectureDb) -> Result<(), Error> {
    if let Some(descr) = ast.acc_descr.filter(|descr| !descr.is_empty()) {
        db.acc_descr = Some(squeeze_indents(&descr));
    }
    if let Some(title) = ast.acc_title.filter(|title| !title.is_empty()) {
        db.acc_title = Some(crate::text::label::trim_start(&title).to_owned());
    }
    if let Some(title) = ast.title.filter(|title| !title.is_empty()) {
        db.title = Some(title);
    }
    for group in ast.groups {
        db.add_group(ArchGroup {
            icon: group.icon,
            id: group.id,
            r#in: group.r#in,
            title: group.title,
        })?;
    }
    for service in ast.services {
        db.add_service(
            service.id,
            service.icon,
            service.icon_text,
            service.title,
            service.r#in,
        )?;
    }
    for junction in ast.junctions {
        db.add_junction(junction.id, junction.r#in)?;
    }
    for edge in ast.edges {
        db.add_edge(ArchEdge {
            lhs_dir: edge.lhs_dir,
            lhs_group: edge.lhs_group,
            lhs_id: edge.lhs_id,
            lhs_into: edge.lhs_into,
            rhs_dir: edge.rhs_dir,
            rhs_group: edge.rhs_group,
            rhs_id: edge.rhs_id,
            rhs_into: edge.rhs_into,
            title: edge.title,
        })?;
    }
    for alignment in ast.alignments {
        db.add_layout_hint(ArchLayoutHint {
            direction: alignment.direction,
            members: alignment.members,
        })?;
    }
    Ok(())
}
