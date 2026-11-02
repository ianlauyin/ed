use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use gpui::App;
use gpui::Context;
use gpui::Div;
use gpui::DragMoveEvent;
use gpui::Entity;
use gpui::Focusable;
use gpui::Pixels;
use gpui::WeakEntity;
use project::Project;
use ui::Window;
use ui::prelude::*;
use util::ResultExt;

use crate::CARD_GAP;
use crate::Event;
use crate::NavHistory;
use crate::Pane;
use crate::PaneRenderContext;
use crate::PaneSearchBarCallbacks;
use crate::SplitDirection;
use crate::SplitMode;
use crate::Workspace;
use crate::WorkspaceMode;
use crate::pane;

pub(crate) const GIT_HISTORY_SIDEBAR_WIDTH: Pixels = px(320.);
const GIT_HISTORY_SIDEBAR_MIN_WIDTH: Pixels = px(240.);

#[derive(Clone)]
struct DraggedGitHistorySidebar;

/// Keeps the git history sidebar between its minimum and 60% of the window.
fn clamp_git_history_sidebar_width(width: Pixels, available: Pixels) -> Pixels {
    width.min(available * 0.6).max(GIT_HISTORY_SIDEBAR_MIN_WIDTH)
}

impl Workspace {
    pub(super) fn render_git_history(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let pane = self.active_git_history_pane.clone();
        let zoomed_pane = self.zoomed_pane(WorkspaceMode::GitHistory, cx);
        let side_card = self.git_history_side_card(cx);
        let panes = match zoomed_pane {
            Some(zoomed_pane) => zoomed_pane.into_any_element(),
            None => {
                let weak_handle = self.weak_handle();
                let render_cx = PaneRenderContext { active_pane: &pane, workspace: &weak_handle };
                self.git_history_center.render(None, None, &render_cx, window, cx).into_any_element()
            }
        };
        let content = h_flex()
            .size_full()
            .on_drag_move(cx.listener(|workspace, e: &DragMoveEvent<DraggedGitHistorySidebar>, _, cx| {
                workspace.git_history_sidebar_width =
                    clamp_git_history_sidebar_width(e.event.position.x - e.bounds.left(), e.bounds.size.width);
                cx.notify();
            }))
            .when_some(side_card, |this, side_card| {
                this.child(
                    div()
                        .id("git-history-sidebar")
                        .relative()
                        .flex_none()
                        .h_full()
                        .w(self.git_history_sidebar_width)
                        .pr(CARD_GAP)
                        .flex()
                        .flex_col()
                        .child(self.render_sidebar_header(cx))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .w_full()
                                .overflow_hidden()
                                .bg(cx.theme().colors().panel_background)
                                .child(side_card),
                        )
                        .child(
                            div()
                                .id("git-history-sidebar-resize-handle")
                                .absolute()
                                .top_0()
                                .right_0()
                                .h_full()
                                .w(CARD_GAP)
                                .cursor_col_resize()
                                .on_click(cx.listener(|workspace, event: &gpui::ClickEvent, _, cx| {
                                    if event.click_count() == 2 {
                                        workspace.git_history_sidebar_width = GIT_HISTORY_SIDEBAR_WIDTH;
                                        cx.notify();
                                    }
                                }))
                                .on_drag(DraggedGitHistorySidebar, |_, _, _, cx| cx.new(|_| gpui::Empty)),
                        ),
                )
            })
            .child(div().id("git-history-panes").flex().flex_1().min_w_0().h_full().child(panes));
        match cx.try_global::<PaneSearchBarCallbacks>() {
            Some(callbacks) => (callbacks.wrap_div_with_search_actions)(content, pane),
            None => content,
        }
    }

    /// What the history sidebar shows: the active history item's side card.
    /// A zoomed history pane fills the area alone, without the sidebar, as
    /// zooming hides docks and other panes in Workspace mode.
    pub(crate) fn git_history_side_card(&self, cx: &App) -> Option<gpui::AnyView> {
        if self.has_zoomed(WorkspaceMode::GitHistory, cx) {
            return None;
        }
        self.active_git_history_pane.read(cx).active_item().and_then(|item| item.side_card(cx))
    }

    /// The active history pane, where the graph and commit views open.
    pub const fn git_history_pane(&self) -> &Entity<Pane> {
        &self.active_git_history_pane
    }

    pub fn git_history_panes(&self) -> Vec<&Entity<Pane>> {
        self.git_history_center.panes()
    }

    /// Whether no history pane has any tabs.
    pub fn git_history_is_empty(&self, cx: &App) -> bool {
        self.git_history_panes().iter().all(|pane| pane.read(cx).items_len() == 0)
    }

    pub(crate) fn new_git_history_pane(
        workspace: WeakEntity<Self>,
        project: Entity<Project>,
        pane_history_timestamp: Arc<AtomicUsize>,
        nav_history: NavHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        let pane = cx.new(|cx| {
            let mut pane = Pane::new(
                workspace,
                project,
                pane_history_timestamp,
                None,
                Box::new(gpui::NoAction),
                false,
                window,
                cx,
            );
            pane.set_should_display_tab_bar(|_, _| true);
            pane.set_can_split(Some(Arc::new(|_, _, _, _| true)));
            pane.set_read_only_tabs(true);
            pane.set_nav_history(nav_history);
            pane
        });
        cx.subscribe_in(&pane, window, Self::handle_git_history_pane_event).detach();
        cx.emit(Event::PaneAdded(pane.clone()));
        pane
    }

    pub(crate) fn add_git_history_pane(&self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Pane> {
        Self::new_git_history_pane(
            self.weak_handle(),
            self.project.clone(),
            self.pane_history_timestamp.clone(),
            self.nav_history.clone(),
            window,
            cx,
        )
    }

    fn split_git_history_pane(
        &mut self,
        pane: Entity<Pane>,
        direction: SplitDirection,
        mode: SplitMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The new pane would be hidden behind a zoomed one.
        self.zoom_git_history_pane(None, cx);
        match mode {
            SplitMode::EmptyPane => {
                self.split_git_history_pane_empty(&pane, direction, window, cx);
            }
            // A tab lives in one pane, so splitting moves it and needs another tab left behind.
            SplitMode::MovePane | SplitMode::ClonePane => {
                if pane.read(cx).items_len() < 2 {
                    return;
                }
                let Some(item) = pane.update(cx, |pane, cx| pane.take_active_item(window, cx)) else {
                    return;
                };
                let new_pane = self.add_git_history_pane(window, cx);
                new_pane.update(cx, |pane, cx| pane.add_item(item, true, true, None, window, cx));
                self.git_history_center.split(&pane, &new_pane, direction, cx);
                self.set_active_git_history_pane(&new_pane, window, cx);
            }
        }
        self.notify_mode(WorkspaceMode::GitHistory, cx);
    }

    fn split_git_history_pane_empty(
        &mut self,
        pane: &Entity<Pane>,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        let new_pane = self.add_git_history_pane(window, cx);
        self.git_history_center.split(pane, &new_pane, direction, cx);
        self.set_active_git_history_pane(&new_pane, window, cx);
        window.focus(&new_pane.focus_handle(cx), cx);
        new_pane
    }

    /// Drops an emptied history pane, unless it is the last one.
    fn remove_git_history_pane(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.git_history_center.remove(pane, cx).log_err().unwrap_or(false) {
            return;
        }
        if self.active_git_history_pane == *pane {
            let next_pane = self.git_history_center.first_pane();
            self.set_active_git_history_pane(&next_pane, window, cx);
            if self.mode == WorkspaceMode::GitHistory {
                window.focus(&next_pane.focus_handle(cx), cx);
            }
        }
        self.notify_mode(WorkspaceMode::GitHistory, cx);
    }

    pub fn set_active_git_history_pane(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_git_history_pane == *pane {
            return;
        }
        self.active_git_history_pane = pane.clone();
        if self.mode == WorkspaceMode::GitHistory {
            self.status_bar.update(cx, |status_bar, cx| status_bar.set_active_pane(pane, window, cx));
        }
        self.notify_mode(WorkspaceMode::GitHistory, cx);
    }

    /// Zooms `pane` (unzooming any other history pane), or unzooms all for `None`.
    pub(crate) fn zoom_git_history_pane(&self, pane: Option<&Entity<Pane>>, cx: &mut Context<Self>) {
        for history_pane in self.git_history_center.panes() {
            let zoomed = pane == Some(history_pane);
            if history_pane.read(cx).is_zoomed() != zoomed {
                history_pane.update(cx, |history_pane, cx| history_pane.set_zoomed(zoomed, cx));
            }
        }
        self.notify_mode(WorkspaceMode::GitHistory, cx);
    }

    fn handle_git_history_pane_event(
        &mut self,
        pane: &Entity<Pane>,
        event: &pane::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            pane::Event::AddItem { item } => {
                item.added_to_pane(self, pane.clone(), window, cx);
                cx.emit(Event::GitHistoryItemAdded { item: item.boxed_clone() });
            }
            pane::Event::Split { direction, mode } => {
                self.split_git_history_pane(pane.clone(), *direction, *mode, window, cx);
            }
            pane::Event::Remove { .. } => self.remove_git_history_pane(pane, window, cx),
            pane::Event::RemovedItem { .. } if self.git_history_is_empty(cx) => {
                self.zoom_git_history_pane(None, cx);
                self.set_mode(WorkspaceMode::Editor, window, cx)
            }
            pane::Event::ZoomIn => self.zoom_git_history_pane(Some(pane), cx),
            pane::Event::ZoomOut => self.zoom_git_history_pane(None, cx),
            pane::Event::Focus => {
                self.content_pane_focused(pane, window, cx);
                self.set_active_git_history_pane(pane, window, cx);
                self.switch_mode(WorkspaceMode::GitHistory, window, cx);
            }
            pane::Event::ChangeItemTitle | pane::Event::ActivateItem { .. } | pane::Event::RemovedItem { .. } => {
                self.notify_mode(WorkspaceMode::GitHistory, cx)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::GIT_HISTORY_SIDEBAR_MIN_WIDTH;
    use super::clamp_git_history_sidebar_width;

    #[test]
    fn test_clamp_git_history_sidebar_width() {
        assert_eq!(clamp_git_history_sidebar_width(px(400.), px(1000.)), px(400.));
        assert_eq!(clamp_git_history_sidebar_width(px(900.), px(1000.)), px(600.));
        assert_eq!(clamp_git_history_sidebar_width(px(100.), px(1000.)), GIT_HISTORY_SIDEBAR_MIN_WIDTH);
        assert_eq!(clamp_git_history_sidebar_width(px(300.), px(200.)), GIT_HISTORY_SIDEBAR_MIN_WIDTH);
    }
}
