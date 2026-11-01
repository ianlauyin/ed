//! What fills the window between the title bar and the status bar: one view
//! per [`WorkspaceMode`], with each mode's rendering in its own module.

mod editor;
mod git_history;

pub(crate) use git_history::GIT_HISTORY_SIDEBAR_WIDTH;
use gpui::AnyElement;
use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::IntoElement;
use gpui::Render;
use gpui::Subscription;
use gpui::WeakEntity;
use gpui::Window;
use gpui::div;
use ui::prelude::*;

use crate::CARD_GAP;
use crate::Workspace;
use crate::WorkspaceMode;
use crate::dock::Dock;

/// Renders one mode's content, everything between the title bar and the
/// status bar. The workspace draws only the shown mode's view, cached, so a
/// mode is laid out only while shown and redraws of the shared chrome reuse it.
pub(crate) struct ModeView {
    workspace: WeakEntity<Workspace>,
    mode: WorkspaceMode,
    _subscriptions: Vec<Subscription>,
}

impl ModeView {
    /// A cached view re-renders only when it or a view inside it notifies, so
    /// it also follows the workspace state it reads, and `followed` entities
    /// that aren't always drawn inside it.
    pub(crate) fn new(
        workspace: &Entity<Workspace>,
        mode: WorkspaceMode,
        followed: &[Entity<Dock>],
        cx: &mut Context<ModeView>,
    ) -> ModeView {
        let mut subscriptions = vec![cx.observe(workspace, |mode_view: &mut ModeView, _, cx| {
            mode_view.notify_if_shown(cx);
        })];
        subscriptions.extend(
            followed
                .iter()
                .map(|entity| cx.observe(entity, |mode_view: &mut ModeView, _, cx| mode_view.notify_if_shown(cx))),
        );
        ModeView { workspace: workspace.downgrade(), mode, _subscriptions: subscriptions }
    }

    fn notify_if_shown(&self, cx: &mut Context<ModeView>) {
        if self.workspace.upgrade().is_some_and(|workspace| workspace.read(cx).mode() == self.mode) {
            cx.notify();
        }
    }
}

impl Render for ModeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<ModeView>) -> impl IntoElement {
        let mode = self.mode;
        self.workspace
            .update(cx, |workspace: &mut Workspace, cx| workspace.render_mode(mode, window, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Workspace {
    /// What fills the window below the title bar in `mode`, rendered by that
    /// mode's [`ModeView`].
    fn render_mode(&self, mode: WorkspaceMode, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match mode {
            WorkspaceMode::Editor => self.render_editor_mode(window, cx),
            WorkspaceMode::GitHistory => self.mode_container(cx).child(self.render_git_history(window, cx)),
        }
        .into_any_element()
    }

    fn mode_container(&self, cx: &App) -> Div {
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .px(CARD_GAP)
            // The status bar pads itself to center between the card and window edge.
            .when(!self.status_bar_visible(cx), |this| this.pb(CARD_GAP))
    }

    pub(crate) const fn mode_view(&self, mode: WorkspaceMode) -> &Entity<ModeView> {
        match mode {
            WorkspaceMode::Editor => &self.editor_view,
            WorkspaceMode::GitHistory => &self.git_history_view,
        }
    }

    /// Redraws after a change to `mode`'s content. A hidden mode isn't drawn,
    /// and re-renders when it is shown again, so its changes redraw nothing.
    pub(crate) fn notify_mode(&self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            cx.notify();
        }
    }
}
