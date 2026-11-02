use gpui::Context;
use gpui::Render;
use gpui::Window;
use ui::Tooltip;
use ui::prelude::*;
use workspace::ItemHandle;
use workspace::StatusItemView;
use workspace::status_item_color;

use crate::git_graph::GitGraph;
use crate::git_graph::Open;

/// The Git History mode status bar button: returns to the git graph, and is
/// accent-coloured while a graph is the active history tab.
pub struct GitHistoryButton {
    graph_active: bool,
}

impl GitHistoryButton {
    pub const fn new() -> Self {
        Self { graph_active: false }
    }
}

impl Default for GitHistoryButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for GitHistoryButton {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        IconButton::new("git-history-graph", IconName::GitGraph)
            .icon_size(IconSize::Small)
            .icon_color(status_item_color(self.graph_active))
            .tab_index(0isize)
            .tooltip(|_window, cx| Tooltip::for_action("Git History", &Open, cx))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(Open), cx))
    }
}

impl StatusItemView for GitHistoryButton {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.graph_active = active_pane_item.is_some_and(|item| item.downcast::<GitGraph>().is_some());
        cx.notify();
    }
}
