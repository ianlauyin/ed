use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;
use futures::future;
use gpui::AsyncWindowContext;
use gpui::Axis;
use gpui::Entity;
use gpui::WeakEntity;
use project::Project;
use serde::Deserialize;
use serde::Serialize;
use util::ResultExt as _;

use crate::Member;
use crate::Pane;
use crate::PaneAxis;
use crate::Workspace;
use crate::item::ItemHandle;
use crate::item_registry::SerializableItemRegistry;
use crate::path_list::PathList;

/// A project that was open when ed last quit.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SessionWorkspace {
    pub paths: PathList,
}

/// Contents of a project's `.ed/workspace.json`.
#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SerializedWorkspace {
    #[serde(default)]
    pub(crate) center_group: SerializedPaneGroup,
    #[serde(default)]
    pub(crate) docks: DockStructure,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recent_navigation_history: Vec<PathBuf>,
    /// Terminal mode's layout, if it has any terminals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) terminal: Option<SerializedTerminalGroup>,
    /// The window's border colour, if one is picked: an index into the
    /// theme's accent colours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) border_color: Option<u32>,
}

/// Terminal mode's splits and each terminal's working directory. Shells are
/// started afresh in those directories; output and processes aren't kept.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SerializedTerminalGroup {
    Group {
        axis: Axis,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flexes: Option<Vec<f32>>,
        children: Vec<SerializedTerminalGroup>,
    },
    Pane(SerializedTerminalPane),
}

#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SerializedTerminalPane {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) active: bool,
    #[serde(default)]
    pub(crate) active_tab: usize,
    #[serde(default)]
    pub(crate) tabs: Vec<SerializedTerminalTab>,
    /// Whether this pane is zoomed over the others.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) zoomed: bool,
}

#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SerializedTerminalTab {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) working_directory: Option<PathBuf>,
}

impl SerializedTerminalGroup {
    /// Drops panes without tabs and groups left with a single child, so a
    /// restored layout has no empty splits. `None` when no tab is left.
    pub(crate) fn prune(self) -> Option<SerializedTerminalGroup> {
        match self {
            SerializedTerminalGroup::Pane(pane) => {
                (!pane.tabs.is_empty()).then_some(SerializedTerminalGroup::Pane(pane))
            }
            SerializedTerminalGroup::Group { axis, flexes, children } => {
                let flexes = flexes.filter(|flexes| flexes.len() == children.len());
                let mut kept_children = Vec::new();
                let mut kept_flexes = Vec::new();
                for (index, child) in children.into_iter().enumerate() {
                    if let Some(child) = child.prune() {
                        kept_children.push(child);
                        if let Some(flexes) = &flexes {
                            kept_flexes.push(flexes[index]);
                        }
                    }
                }
                match kept_children.len() {
                    0 => None,
                    1 => kept_children.pop(),
                    _ => Some(SerializedTerminalGroup::Group {
                        axis,
                        flexes: flexes.map(|_| kept_flexes),
                        children: kept_children,
                    }),
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
pub struct DockStructure {
    #[serde(default)]
    pub left: DockData,
    #[serde(default)]
    pub bottom: DockData,
}

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
pub struct DockData {
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub active_panel: Option<String>,
    #[serde(default)]
    pub zoom: bool,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SerializedPaneGroup {
    Group {
        axis: Axis,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flexes: Option<Vec<f32>>,
        children: Vec<SerializedPaneGroup>,
    },
    Pane(SerializedPane),
}

impl Default for SerializedPaneGroup {
    fn default() -> SerializedPaneGroup {
        SerializedPaneGroup::Pane(SerializedPane { active: true, children: Vec::new(), pinned_count: 0, zoomed: false })
    }
}

impl SerializedPaneGroup {
    pub(crate) async fn deserialize(
        self,
        project: &Entity<Project>,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Option<(Member, Option<Entity<Pane>>, Vec<Option<Box<dyn ItemHandle>>>)> {
        match self {
            SerializedPaneGroup::Group { axis, children, flexes } => {
                let mut current_active_pane = None;
                let mut members = Vec::new();
                let mut items = Vec::new();
                for child in children {
                    if let Some((new_member, active_pane, new_items)) =
                        Box::pin(child.deserialize(project, workspace.clone(), cx)).await
                    {
                        members.push(new_member);
                        items.extend(new_items);
                        current_active_pane = current_active_pane.or(active_pane);
                    }
                }

                if members.is_empty() {
                    return None;
                }

                if members.len() == 1 {
                    return Some((members.remove(0), current_active_pane, items));
                }

                Some((Member::Axis(PaneAxis::load(axis, members, flexes)), current_active_pane, items))
            }
            SerializedPaneGroup::Pane(serialized_pane) => {
                let pane = workspace
                    .update_in(cx, |workspace, window, cx| workspace.add_pane(window, cx).downgrade())
                    .log_err()?;
                let active = serialized_pane.active;
                let new_items = serialized_pane
                    .deserialize_to(project, &pane, workspace.clone(), cx)
                    .await
                    .context("Could not deserialize pane)")
                    .log_err()?;

                if pane.read_with(cx, |pane, _| pane.items_len() != 0).log_err()? {
                    let pane = pane.upgrade()?;
                    if serialized_pane.zoomed {
                        pane.update(cx, |pane, cx| pane.set_zoomed(true, cx));
                    }
                    Some((Member::Pane(pane.clone()), active.then_some(pane), new_items))
                } else {
                    let pane = pane.upgrade()?;
                    workspace
                        .update_in(cx, |workspace, window, cx| workspace.force_remove_pane(&pane, None, window, cx))
                        .log_err()?;
                    None
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone, Serialize, Deserialize)]
pub(crate) struct SerializedPane {
    #[serde(default)]
    pub(crate) active: bool,
    #[serde(default, rename = "items")]
    pub(crate) children: Vec<SerializedItem>,
    #[serde(default)]
    pub(crate) pinned_count: usize,
    /// Whether this pane is zoomed over the others.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) zoomed: bool,
}

impl SerializedPane {
    pub(crate) const fn new(children: Vec<SerializedItem>, active: bool, pinned_count: usize, zoomed: bool) -> Self {
        SerializedPane { active, children, pinned_count, zoomed }
    }

    pub(crate) async fn deserialize_to(
        &self,
        project: &Entity<Project>,
        pane: &WeakEntity<Pane>,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Result<Vec<Option<Box<dyn ItemHandle>>>> {
        let mut item_tasks = Vec::new();
        let mut active_item_index = None;
        let mut preview_item_index = None;
        for (index, item) in self.children.iter().enumerate() {
            let project = project.clone();
            item_tasks.push(pane.update_in(cx, |_, window, cx| {
                SerializableItemRegistry::deserialize(
                    &item.kind,
                    project,
                    workspace.clone(),
                    item.state.clone(),
                    window,
                    cx,
                )
            })?);
            if item.active {
                active_item_index = Some(index);
            }
            if item.preview {
                preview_item_index = Some(index);
            }
        }

        let mut items = Vec::new();
        for item_handle in future::join_all(item_tasks).await {
            let item_handle = item_handle.log_err();
            items.push(item_handle.clone());

            if let Some(item_handle) = item_handle {
                pane.update_in(cx, |pane, window, cx| {
                    pane.add_item(item_handle.clone(), true, true, None, window, cx);
                })?;
            }
        }

        if let Some(active_item) = active_item_index.and_then(|index| items.get(index)?.clone()) {
            pane.update_in(cx, |pane, window, cx| {
                if let Some(index) = pane.index_for_item(active_item.as_ref()) {
                    pane.activate_item(index, false, false, window, cx);
                }
            })?;
        }

        if let Some(preview_item) = preview_item_index.and_then(|index| items.get(index)?.clone()) {
            pane.update(cx, |pane, _| pane.set_preview_item_id(Some(preview_item.item_id())))?;
        }

        // `items` keeps a `None` for every item that failed to deserialize, and those
        // were never added to the pane. Counting them would leave the pinned count
        // pointing past the pinned tabs and pin unpinned ones in their place.
        let pinned_count = items.iter().take(self.pinned_count).filter(|item| item.is_some()).count();
        pane.update(cx, |pane, _| {
            pane.set_pinned_count(pinned_count);
        })?;

        anyhow::Ok(items)
    }
}

/// A tab: its kind, which picks the item type that restores it, and the state
/// that item type saved.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub(crate) struct SerializedItem {
    pub kind: String,
    #[serde(default)]
    pub state: serde_json::Value,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview: bool,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::Axis;

    use super::DockData;
    use super::DockStructure;
    use super::SerializedItem;
    use super::SerializedPane;
    use super::SerializedPaneGroup;
    use super::SerializedTerminalGroup;
    use super::SerializedTerminalPane;
    use super::SerializedTerminalTab;
    use super::SerializedWorkspace;
    use crate::persistence::parse_workspace_state;

    #[test]
    fn serialized_workspace_round_trip() {
        let workspace = SerializedWorkspace {
            center_group: SerializedPaneGroup::Group {
                axis: Axis::Horizontal,
                flexes: Some(vec![0.5, 0.5]),
                children: vec![
                    SerializedPaneGroup::Pane(SerializedPane::new(
                        vec![SerializedItem {
                            kind: "Editor".into(),
                            state: serde_json::json!({"abs_path": "/p/a.rs"}),
                            active: true,
                            preview: false,
                        }],
                        true,
                        1,
                        true,
                    )),
                    SerializedPaneGroup::default(),
                ],
            },
            docks: DockStructure {
                left: DockData { visible: true, active_panel: Some("ProjectPanel".into()), zoom: false },
                ..DockStructure::default()
            },
            recent_navigation_history: vec![PathBuf::from("/p/a.rs")],
            terminal: Some(SerializedTerminalGroup::Group {
                axis: Axis::Vertical,
                flexes: None,
                children: vec![
                    terminal_pane(&[Some("/p")], true),
                    SerializedTerminalGroup::Pane(SerializedTerminalPane {
                        active: false,
                        active_tab: 0,
                        tabs: vec![SerializedTerminalTab::default()],
                        zoomed: true,
                    }),
                ],
            }),
            border_color: Some(6),
        };
        let json = serde_json::to_string_pretty(&workspace).unwrap();
        assert!(json.contains(r#""border_color": 6"#));
        assert_eq!(parse_workspace_state(&json), Some(workspace));
    }

    #[test]
    fn border_color_is_written_only_when_set() {
        assert!(!serde_json::to_string(&SerializedWorkspace::default()).unwrap().contains("border_color"));
        assert_eq!(parse_workspace_state(r#"{"border_color": 2}"#).unwrap().border_color, Some(2));
    }

    #[test]
    fn parse_workspace_state_tolerates_missing_fields() {
        assert_eq!(parse_workspace_state("{}"), Some(SerializedWorkspace::default()));
        assert_eq!(parse_workspace_state("garbage"), None);
        let workspace =
            parse_workspace_state(r#"{"center_group": {"type": "pane", "items": [{"kind": "Editor"}]}}"#).unwrap();
        let SerializedPaneGroup::Pane(pane) = workspace.center_group else {
            panic!("expected a pane");
        };
        assert_eq!(pane.children[0].state, serde_json::Value::Null);
        assert!(!pane.children[0].active);
        assert!(!pane.zoomed);

        let workspace = parse_workspace_state(r#"{"terminal": {"type": "pane", "tabs": [{}]}}"#).unwrap();
        let Some(SerializedTerminalGroup::Pane(terminal_pane)) = workspace.terminal else {
            panic!("expected a terminal pane");
        };
        assert!(!terminal_pane.zoomed);
    }

    #[test]
    fn zoomed_is_written_only_when_set() {
        let pane = SerializedPane::new(Vec::new(), true, 0, false);
        assert!(!serde_json::to_string(&pane).unwrap().contains("zoomed"));
        let pane = SerializedPane::new(Vec::new(), true, 0, true);
        assert!(serde_json::to_string(&pane).unwrap().contains(r#""zoomed":true"#));

        let terminal = SerializedTerminalPane { zoomed: true, ..SerializedTerminalPane::default() };
        assert!(serde_json::to_string(&terminal).unwrap().contains(r#""zoomed":true"#));
        assert!(!serde_json::to_string(&SerializedTerminalPane::default()).unwrap().contains("zoomed"));
    }

    fn terminal_pane(working_directories: &[Option<&str>], active: bool) -> SerializedTerminalGroup {
        SerializedTerminalGroup::Pane(SerializedTerminalPane {
            active,
            active_tab: 0,
            tabs: working_directories
                .iter()
                .map(|dir| SerializedTerminalTab { working_directory: dir.map(PathBuf::from) })
                .collect(),
            zoomed: false,
        })
    }

    #[test]
    fn prune_terminal_group() {
        assert_eq!(terminal_pane(&[], true).prune(), None);
        assert_eq!(terminal_pane(&[None], true).prune(), Some(terminal_pane(&[None], true)));

        // An empty split goes, and its flex with it.
        let group = SerializedTerminalGroup::Group {
            axis: Axis::Horizontal,
            flexes: Some(vec![0.2, 0.3, 0.5]),
            children: vec![
                terminal_pane(&[Some("/a")], true),
                terminal_pane(&[], false),
                terminal_pane(&[None], false),
            ],
        };
        assert_eq!(
            group.prune(),
            Some(SerializedTerminalGroup::Group {
                axis: Axis::Horizontal,
                flexes: Some(vec![0.2, 0.5]),
                children: vec![terminal_pane(&[Some("/a")], true), terminal_pane(&[None], false)],
            })
        );

        // A group left with one child collapses into it, nested ones too.
        let group = SerializedTerminalGroup::Group {
            axis: Axis::Horizontal,
            flexes: None,
            children: vec![
                terminal_pane(&[], false),
                SerializedTerminalGroup::Group {
                    axis: Axis::Vertical,
                    flexes: Some(vec![0.5, 0.5]),
                    children: vec![terminal_pane(&[Some("/b")], true), terminal_pane(&[], false)],
                },
            ],
        };
        assert_eq!(group.prune(), Some(terminal_pane(&[Some("/b")], true)));

        // Nothing left.
        let group = SerializedTerminalGroup::Group {
            axis: Axis::Vertical,
            flexes: None,
            children: vec![terminal_pane(&[], false), terminal_pane(&[], true)],
        };
        assert_eq!(group.prune(), None);
    }

    #[test]
    fn parse_terminal_layout() {
        let workspace = parse_workspace_state(
            r#"{"terminal": {"type": "pane", "active": true, "tabs": [{"working_directory": "/p"}, {}]}}"#,
        )
        .unwrap();
        assert_eq!(workspace.terminal, Some(terminal_pane(&[Some("/p"), None], true)));
    }
}
