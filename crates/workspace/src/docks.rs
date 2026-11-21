use std::sync::Arc;

use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::Focusable as _;
use gpui::Render;
use gpui_util::ResultExt as _;
use ui::Window;
use ui::font;
use ui::prelude::IntoElement;
use ui::prelude::Pixels;
use ui::prelude::px;

use super::Event;
use super::Workspace;
use super::WorkspaceMode;
use super::hidden_by_zoom;
use crate::dock::Dock;
use crate::dock::DockPosition;
use crate::dock::Panel;
use crate::dock::PanelHandle;
use crate::dock::RESIZE_HANDLE_SIZE;
use crate::dock::next_dock_cycle_panel;
use crate::persistence::model::DockData;
use crate::persistence::model::DockStructure;

impl Workspace {
    pub const fn left_dock(&self) -> &Entity<Dock> {
        &self.left_dock
    }

    pub const fn bottom_dock(&self) -> &Entity<Dock> {
        &self.bottom_dock
    }

    pub const fn all_docks(&self) -> [&Entity<Dock>; 2] {
        [&self.left_dock, &self.bottom_dock]
    }

    pub fn capture_dock_state(&self, _window: &Window, cx: &App) -> DockStructure {
        let left_dock = self.left_dock.read(cx);
        let left_visible = left_dock.is_open();
        let left_active_panel = left_dock.active_panel().map(|panel| panel.persistent_name().to_owned());
        // `zoomed_position` is kept in sync with individual panel zoom state
        // by the dock code in `Dock::new` and `Dock::add_panel`.
        let left_dock_zoom = self.zoomed_position == Some(DockPosition::Left);

        let bottom_dock = self.bottom_dock.read(cx);
        let bottom_visible = bottom_dock.is_open();
        let bottom_active_panel = bottom_dock.active_panel().map(|panel| panel.persistent_name().to_owned());
        let bottom_dock_zoom = self.zoomed_position == Some(DockPosition::Bottom);

        DockStructure {
            left: DockData { visible: left_visible, active_panel: left_active_panel, zoom: left_dock_zoom },
            bottom: DockData { visible: bottom_visible, active_panel: bottom_active_panel, zoom: bottom_dock_zoom },
        }
    }

    pub fn set_dock_structure(&self, docks: DockStructure, window: &mut Window, cx: &mut Context<Self>) {
        for (dock, data) in [(&self.left_dock, docks.left), (&self.bottom_dock, docks.bottom)] {
            dock.update(cx, |dock, cx| {
                dock.restore_serialized_state(data, window, cx);
            });
        }
    }

    pub fn finish_dock_restoration(&self, cx: &mut App) {
        for dock in [&self.left_dock, &self.bottom_dock] {
            dock.update(cx, |dock, _| {
                dock.finish_restoration();
            });
        }
    }

    /// Returns which dock currently has focus, or `None` if focus is in the
    /// center pane or elsewhere. Does NOT fall back to any global state.
    pub fn focused_dock_position(&self, window: &Window, cx: &App) -> Option<DockPosition> {
        [(DockPosition::Left, &self.left_dock), (DockPosition::Bottom, &self.bottom_dock)]
            .into_iter()
            .find(|(_, dock)| dock.read(cx).is_open() && dock.focus_handle(cx).contains_focused(window, cx))
            .map(|(position, _)| position)
    }

    pub const fn dock_at_position(&self, position: DockPosition) -> &Entity<Dock> {
        match position {
            DockPosition::Left => &self.left_dock,
            DockPosition::Bottom => &self.bottom_dock,
        }
    }

    fn dock_size(&self, dock: &Dock, window: &Window, cx: &App) -> Option<Pixels> {
        let panel = dock.active_panel()?;
        let size_state = dock.stored_panel_size_state(panel.as_ref()).unwrap_or_default();
        Some(size_state.size.unwrap_or_else(|| panel.default_size(window, cx)))
    }

    pub fn add_panel<T: Panel>(&mut self, panel: Entity<T>, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = panel.panel_focus_handle(cx);
        cx.on_focus_in(&focus_handle, window, Self::handle_panel_focused).detach();

        let dock_position = panel.position(window, cx);
        let dock = self.dock_at_position(dock_position);
        let any_panel = panel.to_any();

        dock.update(cx, |dock, cx| {
            dock.add_panel(panel.clone(), self.weak_self.clone(), window, cx);
        });

        cx.emit(Event::PanelAdded(any_panel));
    }

    /// Whether the dock is open and not covered by another zoomed view.
    pub fn is_dock_visible(&self, position: DockPosition, cx: &App) -> bool {
        self.dock_at_position(position).read(cx).is_open()
            && !hidden_by_zoom(self.zoomed.is_some(), self.zoomed_position, position)
    }

    pub fn toggle_dock(&mut self, dock_side: DockPosition, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.has_docks() {
            return;
        }
        let mut focus_center = false;
        let mut reveal_dock = false;

        let was_visible = self.is_dock_visible(dock_side, cx);

        let dock = self.dock_at_position(dock_side);
        dock.update(cx, |dock, cx| {
            dock.set_open(!was_visible, window, cx);

            if dock.active_panel().is_none() {
                let Some(panel_ix) = dock.first_enabled_panel_idx(cx).log_with_level(log::Level::Info) else {
                    return;
                };
                dock.activate_panel(panel_ix, window, cx);
            }

            if let Some(active_panel) = dock.active_panel() {
                if was_visible {
                    if active_panel.panel_focus_handle(cx).contains_focused(window, cx) {
                        focus_center = true;
                    }
                } else {
                    let focus_handle = active_panel.activation_focus_handle(cx);
                    window.focus(&focus_handle, cx);
                    reveal_dock = true;
                }
            }
        });

        if reveal_dock {
            self.dismiss_zoomed_items_to_reveal(Some(dock_side), window, cx);
        }

        if focus_center {
            self.active_pane.update(cx, |pane, cx| window.focus(&pane.focus_handle(cx), cx));
        }

        cx.notify();
        self.serialize_workspace(window, cx);
    }

    /// Shows the dock's next enabled panel, starting from the first when the
    /// dock is hidden, and closes the dock after the last one.
    pub fn cycle_dock(&mut self, dock_side: DockPosition, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.has_docks() {
            return;
        }
        let dock = self.dock_at_position(dock_side).clone();
        let visible_index = if self.is_dock_visible(dock_side, cx) { dock.read(cx).active_panel_index() } else { None };
        let enabled = dock.read(cx).panels_enabled(cx);

        match next_dock_cycle_panel(&enabled, visible_index) {
            Some(panel_ix) => {
                dock.update(cx, |dock, cx| dock.activate_panel(panel_ix, window, cx));
                if visible_index.is_none() {
                    self.toggle_dock(dock_side, window, cx);
                } else {
                    if let Some(panel) = dock.read(cx).active_panel() {
                        window.focus(&panel.activation_focus_handle(cx), cx);
                    }
                    cx.notify();
                    self.serialize_workspace(window, cx);
                }
            }
            None if visible_index.is_some() => self.toggle_dock(dock_side, window, cx),
            None => {}
        }
    }

    pub(super) fn active_dock(&self, window: &Window, cx: &Context<Self>) -> Option<&Entity<Dock>> {
        self.all_docks()
            .into_iter()
            .find(|&dock| dock.read(cx).is_open() && dock.focus_handle(cx).contains_focused(window, cx))
    }

    pub(super) fn close_active_dock(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(dock) = self.active_dock(window, cx).cloned() {
            dock.update(cx, |dock, cx| {
                dock.set_open(false, window, cx);
            });
            return true;
        }
        false
    }

    pub fn close_all_docks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.has_docks() {
            return;
        }
        for dock in self.all_docks() {
            dock.update(cx, |dock, cx| {
                dock.set_open(false, window, cx);
            });
        }

        cx.focus_self(window);
        cx.notify();
        self.serialize_workspace(window, cx);
    }

    /// Transfer focus to the panel of the given type.
    pub fn focus_panel<T: Panel>(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<T>> {
        let panel = self.focus_or_unfocus_panel::<T>(window, cx, &mut |_, _, _| true)?;
        panel.to_any().downcast().ok()
    }

    /// Focus the panel of the given type if it isn't already focused. If it is
    /// already focused, then transfer focus back to the workspace center.
    pub fn toggle_panel_focus<T: Panel>(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut did_focus_panel = false;
        self.focus_or_unfocus_panel::<T>(window, cx, &mut |panel, window, cx| {
            did_focus_panel = !panel.panel_focus_handle(cx).contains_focused(window, cx);
            did_focus_panel
        });
        did_focus_panel
    }

    pub fn focus_center_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = self.active_item(cx) {
            item.item_focus_handle(cx).focus(window, cx);
        } else {
            log::error!("Could not find a focus target when switching focus to the center panes");
        }
    }

    /// Focus or unfocus the given panel type, depending on the given callback.
    #[allow(clippy::type_complexity)]
    fn focus_or_unfocus_panel<T: Panel>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        should_focus: &mut dyn FnMut(&dyn PanelHandle, &mut Window, &mut Context<Dock>) -> bool,
    ) -> Option<Arc<dyn PanelHandle>> {
        if !self.mode.has_docks() {
            return None;
        }
        let mut result_panel = None;
        let mut serialize = false;
        for dock in self.all_docks() {
            if let Some(panel_index) = dock.read(cx).panel_index_for_type::<T>() {
                let mut focus_center = false;
                let panel = dock.update(cx, |dock, cx| {
                    dock.activate_panel(panel_index, window, cx);

                    let panel = dock.active_panel().cloned();
                    if let Some(panel) = panel.as_ref() {
                        if should_focus(&**panel, window, cx) {
                            dock.set_open(true, window, cx);
                            panel.activation_focus_handle(cx).focus(window, cx);
                        } else {
                            focus_center = true;
                        }
                    }
                    panel
                });

                if focus_center {
                    self.active_pane.update(cx, |pane, cx| window.focus(&pane.focus_handle(cx), cx));
                }

                result_panel = panel;
                serialize = true;
                break;
            }
        }

        if serialize {
            self.serialize_workspace(window, cx);
        }

        cx.notify();
        result_panel
    }

    /// Open the panel of the given type. Ignored in modes without docks.
    pub fn open_panel<T: Panel>(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.has_docks() {
            return;
        }
        for dock in self.all_docks() {
            if let Some(panel_index) = dock.read(cx).panel_index_for_type::<T>() {
                dock.update(cx, |dock, cx| {
                    dock.activate_panel(panel_index, window, cx);
                    dock.set_open(true, window, cx);
                });
            }
        }
    }

    /// Close the panel of the given type. Ignored in modes without docks.
    pub fn close_panel<T: Panel>(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.mode.has_docks() {
            return;
        }
        for dock in &self.all_docks() {
            dock.update(cx, |dock, cx| {
                if dock.panel::<T>().is_some() {
                    dock.set_open(false, window, cx);
                }
            });
        }
    }

    pub fn panel<T: Panel>(&self, cx: &App) -> Option<Entity<T>> {
        self.all_docks().iter().find_map(|dock| dock.read(cx).panel::<T>())
    }

    pub(super) fn dismiss_zoomed_items_to_reveal(
        &mut self,
        dock_to_reveal: Option<DockPosition>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // If a center pane is zoomed, unzoom it.
        for pane in &self.panes {
            if pane != &self.active_pane || dock_to_reveal.is_some() {
                pane.update(cx, |pane, cx| pane.set_zoomed(false, cx));
            }
        }

        // If another dock is zoomed, hide it.
        let mut focus_center = false;
        for dock in self.all_docks() {
            dock.update(cx, |dock, cx| {
                if Some(dock.position()) != dock_to_reveal
                    && let Some(panel) = dock.active_panel()
                    && panel.is_zoomed(window, cx)
                {
                    focus_center |= panel.panel_focus_handle(cx).contains_focused(window, cx);
                    dock.set_open(false, window, cx);
                }
            });
        }

        if focus_center {
            self.active_pane.update(cx, |pane, cx| window.focus(&pane.focus_handle(cx), cx));
        }

        if self.zoomed_position != dock_to_reveal {
            self.zoomed = None;
            self.zoomed_position = None;
            cx.emit(Event::ZoomChanged);
        }

        cx.notify();
    }

    fn handle_panel_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_mode(WorkspaceMode::Editor, cx);
        self.flush_deferred_saves(window, cx);
    }

    fn resize_dock(&self, dock_pos: DockPosition, new_size: Pixels, window: &mut Window, cx: &mut Context<Self>) {
        match dock_pos {
            DockPosition::Left => self.resize_left_dock(new_size, window, cx),
            DockPosition::Bottom => self.resize_bottom_dock(new_size, window, cx),
        }
    }

    pub(super) fn resize_left_dock(&self, new_size: Pixels, window: &mut Window, cx: &mut App) {
        let workspace_width = self.bounds.size.width;
        let size = new_size.min(workspace_width - RESIZE_HANDLE_SIZE);
        self.left_dock.update(cx, |left_dock, cx| {
            left_dock.resize_panel_sizes(Some(size), window, cx);
        });
    }

    pub(super) fn resize_bottom_dock(&self, new_size: Pixels, window: &mut Window, cx: &mut App) {
        let size = new_size.min(self.bounds.bottom() - RESIZE_HANDLE_SIZE - self.bounds.top());
        self.bottom_dock.update(cx, |bottom_dock, cx| {
            bottom_dock.resize_panel_sizes(Some(size), window, cx);
        });
    }
}

pub(super) fn px_with_ui_font_fallback(val: u32, cx: &Context<Workspace>) -> Pixels {
    if val == 0 { font::ui_font_size(cx) } else { px(val as f32) }
}

pub(super) fn adjust_active_dock_size_by_px(
    px: Pixels,
    workspace: &Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(active_dock) =
        workspace.all_docks().into_iter().find(|dock| dock.focus_handle(cx).contains_focused(window, cx))
    else {
        return;
    };
    let dock = active_dock.read(cx);
    let Some(panel_size) = workspace.dock_size(dock, window, cx) else {
        return;
    };
    workspace.resize_dock(dock.position(), panel_size + px, window, cx);
}

pub(super) fn adjust_open_docks_size_by_px(
    px: Pixels,
    workspace: &Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if !workspace.mode.has_docks() {
        return;
    }
    let docks = workspace
        .all_docks()
        .into_iter()
        .filter_map(|dock_entity| {
            let dock = dock_entity.read(cx);
            if dock.is_open() {
                let dock_pos = dock.position();
                let panel_size = workspace.dock_size(dock, window, cx)?;
                Some((dock_pos, panel_size + px))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    for (position, new_size) in docks {
        workspace.resize_dock(position, new_size, window, cx);
    }
}

#[derive(Clone)]
pub(crate) struct DraggedDock(pub(crate) DockPosition);

impl Render for DraggedDock {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}
