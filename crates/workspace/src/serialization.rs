use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use gpui::App;
use gpui::AppContext;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::Task;
use gpui::WindowHandle;
use gpui_util::ResultExt as _;
use project::ProjectPath;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use ui::Window;

use super::AppState;
use super::Event;
use super::OpenMode;
use super::Pane;
use super::SERIALIZATION_THROTTLE_TIME;
use super::Workspace;
use super::persistence;
use super::restored_active_pane;
use crate::item::ItemHandle;
use crate::mode::TerminalLayout;
use crate::pane_group::Member;
use crate::pane_group::PaneAxis;
use crate::pane_group::PaneGroup;
use crate::path_list::PathList;
use crate::persistence::ProjectLocation;
use crate::persistence::ProjectsStore;
use crate::persistence::model::SerializedItem;
use crate::persistence::model::SerializedPane;
use crate::persistence::model::SerializedPaneGroup;
use crate::persistence::model::SerializedWorkspace;
use crate::persistence::model::SessionWorkspace;
use crate::persistence::recent_project_workspaces;

impl Workspace {
    /// Where this workspace's project lives, or `None` for a workspace without
    /// root folders, which isn't persisted.
    pub fn project_location(&self, cx: &App) -> Option<ProjectLocation> {
        let paths = PathList::new(&self.root_paths(cx));
        (!paths.is_empty()).then(|| ProjectLocation::new(&paths))
    }

    /// Bypass the serialization throttle and write the workspace state now.
    /// Returns a task the caller can await to ensure the write completes before
    /// the process exits.
    pub fn flush_serialization(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        self._schedule_serialize_workspace.take();
        self._serialize_workspace_task.take();
        self.serialize_workspace_internal(window, cx)
    }

    pub fn root_paths(&self, cx: &App) -> Vec<Arc<Path>> {
        let project = self.project().read(cx);
        project.visible_worktrees(cx).map(|worktree| worktree.read(cx).abs_path()).collect::<Vec<_>>()
    }

    pub(crate) fn serialize_workspace(&mut self, window: &Window, cx: &Context<Self>) {
        if self._schedule_serialize_workspace.is_none() {
            self._schedule_serialize_workspace = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(SERIALIZATION_THROTTLE_TIME).await;
                this.update_in(cx, |this, window, cx| {
                    this._serialize_workspace_task = Some(this.serialize_workspace_internal(window, cx));
                    this._schedule_serialize_workspace.take();
                })
                .log_err();
            }));
        }
    }

    /// Writes this workspace's state to `.ed/workspace.json` in its first root
    /// folder. Workspaces without root folders aren't persisted.
    fn serialize_workspace_internal(&mut self, window: &Window, cx: &Context<Self>) -> Task<()> {
        if self.restoring_workspace {
            return Task::ready(());
        }
        let Some(location) = self.project_location(cx) else {
            return Task::ready(());
        };
        let Some(root) = location.paths.into_iter().next() else {
            return Task::ready(());
        };

        fn build_serialized_pane_group(
            pane_group: &Member,
            workspace: &Workspace,
            window: &Window,
            cx: &App,
        ) -> SerializedPaneGroup {
            match pane_group {
                Member::Axis(PaneAxis { axis, members, flexes, bounding_boxes: _ }) => SerializedPaneGroup::Group {
                    axis: *axis,
                    children: members
                        .iter()
                        .map(|member| build_serialized_pane_group(member, workspace, window, cx))
                        .collect::<Vec<_>>(),
                    flexes: Some(flexes.lock().clone()),
                },
                Member::Pane(pane_handle) => {
                    SerializedPaneGroup::Pane(serialize_pane_handle(pane_handle, workspace, window, cx))
                }
            }
        }

        let serialized_workspace = SerializedWorkspace {
            center_group: build_serialized_pane_group(&self.center.root, self, window, cx),
            docks: self.capture_dock_state(window, cx),
            recent_navigation_history: self.persisted_recent_navigation_history.clone(),
            terminal: self.terminal_layout_to_save(cx),
            border_color: self.border_color,
        };
        let Some(json) = serde_json::to_string_pretty(&serialized_workspace).log_err() else {
            return Task::ready(());
        };
        if self.last_serialized_state.as_ref() == Some(&json) {
            return Task::ready(());
        }
        self.last_serialized_state = Some(json.clone());

        let write = persistence::write_workspace_state(&self.project, &root, json, cx);
        cx.background_spawn(async move {
            write.await.with_context(|| format!("saving workspace state for {root:?}")).log_err();
        })
    }

    pub(super) fn update_history(&self, cx: &App) {
        let Some(location) = self.project_location(cx) else {
            return;
        };
        if let Some(store) = ProjectsStore::try_global(cx) {
            store.record_recent(location, cx);
        }
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn load_workspace(
        serialized_workspace: SerializedWorkspace,
        paths_to_open: Vec<Option<ProjectPath>>,
        window: &Window,
        cx: &Context<Workspace>,
    ) -> Task<Result<Vec<Option<Box<dyn ItemHandle>>>>> {
        cx.spawn_in(window, async move |workspace, cx| {
            let recent_navigation_history = serialized_workspace.recent_navigation_history.clone();
            let terminal_layout = serialized_workspace.terminal.clone();
            let border_color = serialized_workspace.border_color;
            workspace.update(cx, |workspace, cx| {
                workspace.border_color = border_color;
                cx.notify();
                workspace.persisted_recent_navigation_history = recent_navigation_history;
                if matches!(workspace.terminal_layout, TerminalLayout::Pending(_)) {
                    workspace.terminal_layout = TerminalLayout::Pending(terminal_layout);
                }
                workspace.restoring_workspace = true;
            })?;
            let project = workspace.read_with(cx, |workspace, _| workspace.project().clone())?;

            let mut center_group = None;
            let mut center_items = None;

            // Traverse the splits tree and add to things
            if let Some((group, active_pane, items)) =
                serialized_workspace.center_group.deserialize(&project, workspace.clone(), cx).await
            {
                center_items = Some(items);
                center_group = Some((group, active_pane));
            }

            let mut items_by_project_path = FxHashMap::default();
            cx.update(|_, cx| {
                for item in center_items.unwrap_or_default().into_iter().flatten() {
                    if let Some(project_path) = item.project_path(cx) {
                        items_by_project_path.insert(project_path, item);
                    }
                }
            })?;

            let opened_items = paths_to_open
                .into_iter()
                .map(|path_to_open| {
                    let path_to_open = path_to_open?;
                    items_by_project_path.remove(&path_to_open)
                })
                .collect::<Vec<_>>();

            // Remove old panes from workspace panes list
            workspace.update_in(cx, |workspace, window, cx| {
                if let Some((center_group, active_pane)) = center_group {
                    workspace.remove_panes(workspace.center.root.clone(), window, cx);

                    // Swap workspace center group
                    workspace.center = PaneGroup::with_root(center_group);
                    workspace.center.set_is_center(true);
                    workspace.center.mark_positions(cx);

                    // Only one pane stays zoomed, and it covers the others, so
                    // it is the one to activate.
                    let zoomed_pane =
                        workspace.center.panes().into_iter().find(|pane| pane.read(cx).is_zoomed()).cloned();
                    for pane in workspace.center.panes() {
                        if Some(pane) != zoomed_pane.as_ref() && pane.read(cx).is_zoomed() {
                            pane.update(cx, |pane, cx| pane.set_zoomed(false, cx));
                        }
                    }
                    if let Some(zoomed_pane) = &zoomed_pane {
                        workspace.zoomed = Some(zoomed_pane.downgrade().into());
                        workspace.zoomed_position = None;
                        cx.emit(Event::ZoomChanged);
                    }

                    if let Some(active_pane) = restored_active_pane(active_pane, zoomed_pane) {
                        workspace.set_active_pane(&active_pane, window, cx);
                        cx.focus_self(window);
                    } else {
                        workspace.set_active_pane(&workspace.center.first_pane(), window, cx);
                    }
                }

                let docks = serialized_workspace.docks;

                for (dock, serialized_dock) in
                    &mut [(&mut workspace.left_dock, docks.left), (&mut workspace.bottom_dock, docks.bottom)]
                {
                    dock.update(cx, |dock, cx| {
                        dock.restore_serialized_state(serialized_dock.clone(), window, cx);
                    });
                }

                workspace.restoring_workspace = false;
                cx.notify();
            })?;

            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.serialize_workspace(window, cx);

                    // Ensure that we mark the window as edited if we did load dirty items
                    workspace.update_window_edited(window, cx);
                })
                .ok();

            Ok(opened_items)
        })
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn open_items(
    serialized_workspace: Option<SerializedWorkspace>,
    mut project_paths_to_open: Vec<(PathBuf, Option<ProjectPath>)>,
    window: &Window,
    cx: &Context<Workspace>,
) -> impl 'static + Future<Output = Result<Vec<Option<Result<Box<dyn ItemHandle>>>>>> + use<> {
    let restored_items = serialized_workspace.map(|serialized_workspace| {
        Workspace::load_workspace(
            serialized_workspace,
            project_paths_to_open.iter().map(|(_, project_path)| project_path).cloned().collect(),
            window,
            cx,
        )
    });

    cx.spawn_in(window, async move |workspace, cx| {
        let mut opened_items = Vec::with_capacity(project_paths_to_open.len());

        if let Some(restored_items) = restored_items {
            let restored_items = restored_items.await?;

            let restored_project_paths = restored_items
                .iter()
                .filter_map(|item| cx.update(|_, cx| item.as_ref()?.project_path(cx)).ok().flatten())
                .collect::<FxHashSet<_>>();

            for restored_item in restored_items {
                opened_items.push(restored_item.map(Ok));
            }

            for (_, project_path) in &mut project_paths_to_open {
                if let Some(project_path_to_open) = project_path
                    && restored_project_paths.contains(project_path_to_open)
                {
                    *project_path = None;
                }
            }
        } else {
            for _ in 0..project_paths_to_open.len() {
                opened_items.push(None);
            }
        }
        assert!(opened_items.len() == project_paths_to_open.len());

        let tasks = project_paths_to_open.into_iter().enumerate().map(|(ix, (abs_path, project_path))| {
            let workspace = workspace.clone();
            cx.spawn(async move |cx| {
                let file_project_path = project_path?;
                let abs_path_task = workspace.update(cx, |workspace, cx| {
                    workspace
                        .project()
                        .update(cx, |project, cx| project.resolve_abs_path(abs_path.to_string_lossy().as_ref(), cx))
                });

                // We only want to open file paths here. If one of the items
                // here is a directory, it was already opened further above
                // with a `find_or_create_worktree`.
                if let Ok(task) = abs_path_task
                    && task.await.is_none_or(|p| p.is_file())
                {
                    return Some((
                        ix,
                        workspace
                            .update_in(cx, |workspace, window, cx| {
                                workspace.open_path(file_project_path, None, true, window, cx)
                            })
                            .log_err()?
                            .await,
                    ));
                }
                None
            })
        });

        let tasks = tasks.collect::<Vec<_>>();

        let tasks = futures::future::join_all(tasks);
        for (ix, path_open_result) in tasks.await.into_iter().flatten() {
            opened_items[ix] = Some(path_open_result);
        }

        Ok(opened_items)
    })
}

/// The most recently opened project, if its folders still exist.
pub async fn last_opened_workspace_location(fs: &dyn fs::Fs, cx: &AsyncApp) -> Option<SessionWorkspace> {
    let recent = cx.update(|cx| ProjectsStore::global(cx).recent());
    let workspace = recent_project_workspaces(recent, fs).await.into_iter().next()?;
    Some(SessionWorkspace { paths: workspace.paths })
}

pub async fn restore_workspace(
    session_workspace: SessionWorkspace,
    app_state: Arc<AppState>,
    cx: &mut AsyncApp,
) -> anyhow::Result<WindowHandle<Workspace>> {
    let window_handle = cx
        .update(|cx| {
            Workspace::new_local(
                session_workspace.paths.ordered_paths().cloned().collect(),
                Arc::clone(&app_state),
                None,
                None,
                OpenMode::NewWindow,
                cx,
            )
        })
        .await?
        .window;

    window_handle
        .update(cx, |_, window, _cx| {
            window.activate_window();
        })
        .ok();

    Ok(window_handle)
}

fn serialize_pane_handle(
    pane_handle: &Entity<Pane>,
    workspace: &Workspace,
    window: &Window,
    cx: &App,
) -> SerializedPane {
    let (items, active, pinned_count, zoomed) = {
        let pane = pane_handle.read(cx);
        let active_item_id = pane.active_item().map(|item| item.item_id());
        // Pinned tabs are the leading tabs of a pane, so the pinned count has to
        // shrink along with every pinned item that is dropped here. Otherwise a
        // tab that was not pinned would take the dropped item's slot and come
        // back pinned on the next restore.
        let pinned_region = 0..pane.pinned_count();
        let mut pinned_count = pane.pinned_count();
        let items = pane
            .items()
            .enumerate()
            .filter_map(|(index, handle)| {
                let in_state_dir = handle
                    .project_path(cx)
                    .is_some_and(|project_path| persistence::is_in_workspace_state_dir(&project_path.path));
                let serializable = handle.to_serializable_item_handle(cx).filter(|_| !in_state_dir);
                let serialized = serializable.and_then(|serializable| {
                    Some(SerializedItem {
                        kind: serializable.serialized_item_kind().to_owned(),
                        state: serializable.serialize(workspace, cx)?,
                        active: Some(handle.item_id()) == active_item_id,
                        preview: pane.is_active_preview_item(handle.item_id()),
                    })
                });
                if serialized.is_none() && pinned_region.contains(&index) {
                    pinned_count -= 1;
                }
                serialized
            })
            .collect::<Vec<_>>();

        (items, pane.has_focus(window, cx), pinned_count, pane.is_zoomed())
    };

    SerializedPane::new(items, active, pinned_count, zoomed)
}

pub async fn flush_windows_serialization(workspace_windows: &[WindowHandle<Workspace>], cx: &mut AsyncApp) {
    let flush_tasks = collect_flush_tasks(workspace_windows, cx);
    futures::future::join_all(flush_tasks).await;
}

pub(super) fn flush_windows_serialization_on_quit(cx: &mut App) -> impl Future<Output = ()> + use<> {
    let workspace_windows =
        cx.windows().into_iter().filter_map(|window| window.downcast::<Workspace>()).collect::<Vec<_>>();
    let flush_tasks = collect_flush_tasks(&workspace_windows, cx);
    async move {
        futures::future::join_all(flush_tasks).await;
    }
}

fn collect_flush_tasks(workspace_windows: &[WindowHandle<Workspace>], cx: &mut impl AppContext) -> Vec<Task<()>> {
    let mut flush_tasks = Vec::new();
    for window in workspace_windows {
        window
            .update(cx, |workspace, window, cx| {
                flush_tasks.push(workspace.flush_serialization(window, cx));
            })
            .with_context(|| format!("flushing pending serialization for window {:?}", window.window_id()))
            .log_err();
    }
    flush_tasks
}
