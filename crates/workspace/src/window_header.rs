//! The window has no title bar row. The window header (room for the macOS
//! traffic lights, the project name, Go Back / Go Forward and the Workspace /
//! Git History switch) sits above the sidebar when one is shown, otherwise at
//! the start of the top-left tab bar.

use gpui::AnyElement;
use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::IntoElement;
use gpui::Pixels;
use gpui::Render;
use gpui::Subscription;
use gpui::WeakEntity;
use gpui::Window;
use gpui::div;
use project::Project;
use project::git_store::GitStoreEvent;
use project::project_settings::ProjectSettings;
use settings::Settings as _;
use ui::IconButtonShape;
use ui::Tab;
use ui::Tooltip;
use ui::prelude::*;
use ui::utils::TRAFFIC_LIGHT_PADDING;
use ui::utils::window_drag_area;
use util::ResultExt;

use crate::CARD_GAP;
use crate::GoBack;
use crate::GoForward;
use crate::Pane;
use crate::ShowEditor;
use crate::Workspace;
use crate::WorkspaceMode;
use crate::dock::Dock;

/// Where tab bar start content begins, from the window's left edge, when the
/// pane is not after a sidebar: `CARD_GAP`, the folder tab bar's 10px inset,
/// then the start slot's 6px padding.
pub(crate) const TAB_BAR_START_OFFSET: Pixels = px(24.);

/// Width of the empty room left for the traffic lights by an element that
/// starts `offset` from the window's left edge. Fullscreen windows hide them.
fn traffic_light_room(offset: Pixels, fullscreen: bool) -> Pixels {
    if fullscreen { px(0.) } else { (px(TRAFFIC_LIGHT_PADDING) - offset).max(px(0.)) }
}

/// The window header, drawn above the sidebar or at the start of the header
/// pane's tab bar. Redraws only when what it shows changes, since redrawing it
/// in a tab bar redraws the pane holding it.
pub(crate) struct WindowHeader {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    shown: WindowHeaderState,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct WindowHeaderState {
    in_tab_bar: bool,
    can_go_back: bool,
    can_go_forward: bool,
    mode: WorkspaceMode,
    has_repository: bool,
}

impl WindowHeaderState {
    fn of(workspace: &Workspace, project: &Entity<Project>, cx: &App) -> WindowHeaderState {
        WindowHeaderState {
            in_tab_bar: workspace.window_header_in_tab_bar(cx),
            can_go_back: workspace.can_go_back(),
            can_go_forward: workspace.can_go_forward(),
            mode: workspace.mode(),
            has_repository: ProjectSettings::get_global(cx).git.enabled.status
                && project.read(cx).active_repository(cx).is_some(),
        }
    }
}

impl WindowHeader {
    /// Made while the workspace is being built, so it takes the left dock and
    /// project rather than reading them from the workspace.
    pub(crate) fn new(
        workspace: &Entity<Workspace>,
        left_dock: &Entity<Dock>,
        project: &Entity<Project>,
        cx: &mut Context<WindowHeader>,
    ) -> WindowHeader {
        let git_store = project.read(cx).git_store().clone();
        let subscriptions = vec![
            cx.observe(workspace, |header: &mut WindowHeader, _, cx| header.refresh(cx)),
            cx.observe(left_dock, |header: &mut WindowHeader, _, cx| header.refresh(cx)),
            cx.subscribe(&git_store, |header: &mut WindowHeader, _, event: &GitStoreEvent, cx| {
                if matches!(event, GitStoreEvent::ActiveRepositoryChanged(_)) {
                    header.refresh(cx);
                }
            }),
        ];
        WindowHeader {
            workspace: workspace.downgrade(),
            project: project.clone(),
            shown: WindowHeaderState::default(),
            _subscriptions: subscriptions,
        }
    }

    fn refresh(&self, cx: &mut Context<WindowHeader>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if WindowHeaderState::of(workspace.read(cx), &self.project, cx) != self.shown {
            cx.notify();
        }
    }
}

impl Render for WindowHeader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<WindowHeader>) -> impl IntoElement {
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let workspace = workspace.read(cx);
        self.shown = WindowHeaderState::of(workspace, &self.project, cx);
        let fullscreen = window.is_fullscreen() || window.is_simple_fullscreen();
        let offset = if self.shown.in_tab_bar { TAB_BAR_START_OFFSET } else { CARD_GAP };

        h_flex()
            .h_full()
            .gap_1()
            .child(window_drag_area(div().id("traffic-lights").h_full()).w(traffic_light_room(offset, fullscreen)))
            .children(workspace.render_titlebar_region())
            .child(
                h_flex()
                    .gap_1()
                    // Clicks on the controls must not move the window.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(render_nav_buttons(&self.workspace, self.shown))
                    .when(self.shown.has_repository, |this| {
                        this.child(render_mode_switch(&self.workspace, self.shown.mode, cx))
                    }),
            )
            .into_any_element()
    }
}

/// Go Back / Go Forward over the workspace's one history, which spans every
/// pane in both modes.
fn render_nav_buttons(workspace: &WeakEntity<Workspace>, state: WindowHeaderState) -> impl IntoElement {
    let back_workspace = workspace.clone();
    let go_back = IconButton::new("go-back", IconName::ArrowLeft)
        .icon_size(IconSize::Small)
        .disabled(!state.can_go_back)
        .tooltip(|_window, cx| Tooltip::for_action("Go Back", &GoBack, cx))
        .on_click(move |_, window, cx| {
            back_workspace.update(cx, |workspace, cx| workspace.go_back(window, cx).detach_and_log_err(cx)).log_err();
        });

    let forward_workspace = workspace.clone();
    let go_forward = IconButton::new("go-forward", IconName::ArrowRight)
        .icon_size(IconSize::Small)
        .disabled(!state.can_go_forward)
        .tooltip(|_window, cx| Tooltip::for_action("Go Forward", &GoForward, cx))
        .on_click(move |_, window, cx| {
            forward_workspace
                .update(cx, |workspace, cx| workspace.go_forward(window, cx).detach_and_log_err(cx))
                .log_err();
        });

    h_flex().gap_0p5().child(go_back).child(go_forward)
}

/// Icon-only "Workspace" / "Git History" segmented switch, outlined like the
/// branch picker's Branches / Stashes switch: a thin border with a divider
/// between segments, and the shown mode's icon in the accent colour.
fn render_mode_switch(workspace: &WeakEntity<Workspace>, mode: WorkspaceMode, cx: &App) -> impl IntoElement {
    let border_color = cx.theme().colors().border.opacity(0.6);
    let segment = |id: &'static str, icon: IconName, shown: bool| {
        IconButton::new(id, icon)
            .icon_size(IconSize::Small)
            .icon_color(if shown { Color::Accent } else { Color::Muted })
            .shape(IconButtonShape::Wide)
    };

    let editor_workspace = workspace.clone();
    let show_editor = segment("workspace-mode-editor", IconName::Code, mode == WorkspaceMode::Editor)
        .tooltip(|_window, cx| Tooltip::for_action("Workspace", &ShowEditor, cx))
        .on_click(move |_, window, cx| {
            editor_workspace
                .update(cx, |workspace, cx| workspace.set_mode(WorkspaceMode::Editor, window, cx))
                .log_err();
        });

    let show_history = segment("workspace-mode-git-history", IconName::GitGraph, mode == WorkspaceMode::GitHistory)
        .tooltip(|_window, cx| Tooltip::for_action("Git History", &ed_actions::git_graph::Show, cx))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ed_actions::git_graph::Show), cx));

    h_flex()
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(border_color)
        .child(div().border_r_1().border_color(border_color).child(show_editor))
        .child(show_history)
}

impl Workspace {
    /// Whether the window header sits in the tab bar because no sidebar is
    /// shown to hold it.
    pub(crate) fn window_header_in_tab_bar(&self, cx: &App) -> bool {
        match self.mode {
            WorkspaceMode::Editor => self.zoomed.is_some() || self.left_dock.read(cx).visible_panel().is_none(),
            WorkspaceMode::GitHistory => self.git_history_side_card(cx).is_none(),
        }
    }

    /// The window header above a sidebar (left dock or Git History sidebar),
    /// on a strip that moves the window and is as high as a top-row tab bar
    /// with its gap, so the sidebar content lines up with the cards.
    pub(crate) fn render_sidebar_header(&self, cx: &App) -> impl IntoElement {
        window_drag_area(div().id("sidebar-header"))
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(CARD_GAP + Tab::container_height(cx))
            .pt(CARD_GAP)
            .overflow_hidden()
            .child(self.window_header.clone())
    }

    /// The title bar item (project name and alerts) inside its focus region.
    /// The region is a toolbar so region navigation (FocusNextPart) can reach
    /// its controls: it lands on the first control, Tab steps through them,
    /// and arrow keys move between them once focus is inside.
    pub(crate) fn render_titlebar_region(&self) -> Option<AnyElement> {
        let item = self.titlebar_item.clone()?;
        let workspace = self.weak_self.clone();
        Some(
            div()
                .id("titlebar-region")
                .track_focus(&self.titlebar_focus_handle)
                .tab_group()
                .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let forward = match event.keystroke.key.as_str() {
                        "right" => true,
                        "left" => false,
                        _ => return,
                    };
                    workspace
                        .update(cx, |workspace, cx| workspace.move_titlebar_item_focus(forward, window, cx))
                        .log_err();
                    cx.stop_propagation();
                })
                // Clicks on the project controls must not move the window.
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(item)
                .into_any_element(),
        )
    }

    /// The pane whose tab bar starts the window's top row in `mode`, which
    /// holds the [`WindowHeader`] when no sidebar does, or `None` while a
    /// zoomed dock covers the panes.
    fn tab_bar_header_pane(&self, mode: WorkspaceMode, cx: &App) -> Option<Entity<Pane>> {
        match mode {
            WorkspaceMode::Editor => {
                if self.zoomed_position.is_some() {
                    return None;
                }
                let zoomed_pane = self.zoomed.as_ref().and_then(|view| view.upgrade()?.downcast::<Pane>().ok());
                let maximized_pane = self.maximized_pane.as_ref().and_then(WeakEntity::upgrade);
                Some(zoomed_pane.or(maximized_pane).unwrap_or_else(|| self.center.first_pane()))
            }
            WorkspaceMode::GitHistory => Some(
                self.git_history_center
                    .panes()
                    .into_iter()
                    .find(|pane| pane.read(cx).is_zoomed())
                    .cloned()
                    .unwrap_or_else(|| self.git_history_center.first_pane()),
            ),
        }
    }

    /// Puts the [`WindowHeader`] on `mode`'s header pane when no sidebar holds it, and marks the panes
    /// whose tab bars form the window's top row, so their empty space moves
    /// the window. Panes only redraw when this changes for them.
    pub(crate) fn assign_tab_bar_chrome(&self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        let group = match mode {
            WorkspaceMode::Editor => &self.center,
            WorkspaceMode::GitHistory => &self.git_history_center,
        };
        let header_pane = self.window_header_in_tab_bar(cx).then(|| self.tab_bar_header_pane(mode, cx)).flatten();
        let top_row: Vec<Entity<Pane>> = group.top_row_panes().into_iter().cloned().collect();
        let panes: Vec<Entity<Pane>> = group.panes().into_iter().cloned().collect();
        for pane in panes {
            let has_header = header_pane.as_ref() == Some(&pane);
            let header = has_header.then(|| self.window_header.clone().into());
            let is_titlebar = has_header || top_row.contains(&pane);
            pane.update(cx, |pane, cx| pane.set_tab_bar_chrome(header, is_titlebar, cx));
        }
    }

    /// The header row above a zoomed dock, which covers the pane that would
    /// hold the [`WindowHeader`]. Starts where a tab bar's start content would.
    pub(crate) fn render_zoomed_dock_header(&self, cx: &App) -> impl IntoElement {
        h_flex()
            .flex_none()
            .w_full()
            .h(CARD_GAP + Tab::container_height(cx))
            .pt(CARD_GAP)
            .pl(TAB_BAR_START_OFFSET - CARD_GAP)
            .child(self.window_header.clone())
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;
    use ui::utils::TRAFFIC_LIGHT_PADDING;

    use super::TAB_BAR_START_OFFSET;
    use super::traffic_light_room;

    #[test]
    fn test_traffic_light_room() {
        assert_eq!(traffic_light_room(px(0.), false), px(TRAFFIC_LIGHT_PADDING));
        assert_eq!(traffic_light_room(TAB_BAR_START_OFFSET, false), px(TRAFFIC_LIGHT_PADDING) - TAB_BAR_START_OFFSET);
        assert_eq!(traffic_light_room(px(500.), false), px(0.));
        assert_eq!(traffic_light_room(TAB_BAR_START_OFFSET, true), px(0.));
    }
}
