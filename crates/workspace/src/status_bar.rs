use std::ops::Deref;

use gpui::AnyView;
use gpui::App;
use gpui::Context;
use gpui::Decorations;
use gpui::Entity;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Render;
use gpui::Styled;
use gpui::Subscription;
use gpui::Window;
use theme::CLIENT_SIDE_DECORATION_ROUNDING;
use ui::prelude::*;

use crate::ItemHandle;
use crate::Pane;
use crate::WorkspaceMode;

pub trait StatusItemView: Render {
    /// Event callback that is triggered when the active pane item changes.
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn crate::ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

trait StatusItemViewHandle: Send {
    fn to_any(&self) -> AnyView;
    fn set_active_pane_item(&self, active_pane_item: Option<&dyn ItemHandle>, window: &mut Window, cx: &mut App);
}

/// The colour of a status bar button that opens a pane item: accent while
/// that item is the active one, so every such button reads the same way.
pub const fn status_item_color(active: bool) -> Color {
    if active { Color::Accent } else { Color::Default }
}

/// The workspace modes a status bar item is shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusItemModes {
    Editor,
    GitHistory,
    All,
}

impl StatusItemModes {
    fn includes(self, mode: WorkspaceMode) -> bool {
        match self {
            StatusItemModes::Editor => mode == WorkspaceMode::Editor,
            StatusItemModes::GitHistory => mode == WorkspaceMode::GitHistory,
            StatusItemModes::All => true,
        }
    }
}

struct StatusItem {
    view: Box<dyn StatusItemViewHandle>,
    modes: StatusItemModes,
}

impl Deref for StatusItem {
    type Target = dyn StatusItemViewHandle;

    fn deref(&self) -> &Self::Target {
        self.view.as_ref()
    }
}

pub struct StatusBar {
    left_items: Vec<StatusItem>,
    right_items: Vec<StatusItem>,
    active_pane: Entity<Pane>,
    mode: WorkspaceMode,
    focus_handle: FocusHandle,
    _observe_active_pane: Subscription,
}

impl Focusable for StatusBar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for StatusBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("status-bar")
            .track_focus(&self.focus_handle)
            .key_context("StatusBar")
            // The controls inside form a tab group so region navigation can
            // reach them: it lands on the first control, Tab steps through them,
            // and arrow keys move between them once focus is inside.
            .tab_group()
            .on_key_down(cx.listener(|status_bar, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "right" => {
                        status_bar.move_item_focus(true, window, cx);
                        cx.stop_propagation();
                    }
                    "left" => {
                        status_bar.move_item_focus(false, window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .w_full()
            .justify_between()
            .gap(DynamicSpacing::Base08.rems())
            // Even gap above and below, matching the floating card gap.
            .px(crate::CARD_GAP)
            .py(crate::CARD_GAP - px(2.))
            .bg(cx.theme().colors().status_bar_background)
            .map(|el| match window.window_decorations() {
                Decorations::Server => el,
                Decorations::Client { tiling, .. } => el
                    .when(!(tiling.bottom || tiling.right), |el| el.rounded_br(CLIENT_SIDE_DECORATION_ROUNDING))
                    .when(!(tiling.bottom || tiling.left), |el| el.rounded_bl(CLIENT_SIDE_DECORATION_ROUNDING))
                    // This border is to avoid a transparent gap in the rounded corners
                    .mb(px(-1.))
                    .mt({
                        let needs_gap_fix = false;
                        if needs_gap_fix { px(-1.) } else { px(0.) }
                    })
                    .border_b(px(1.0))
                    .border_color(cx.theme().colors().status_bar_background),
            })
            .child(self.render_left_tools())
            .child(self.render_right_tools())
    }
}

impl StatusBar {
    fn render_left_tools(&self) -> impl IntoElement {
        h_flex()
            .gap_1()
            .min_w_0()
            .overflow_x_hidden()
            .children(self.shown_items(&self.left_items).map(|item| item.to_any()))
    }

    fn render_right_tools(&self) -> impl IntoElement {
        h_flex()
            .flex_shrink_0()
            .gap_1()
            .overflow_x_hidden()
            .children(self.shown_items(&self.right_items).rev().map(|item| item.to_any()))
    }
}

impl StatusBar {
    pub fn new(active_pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let this = Self {
            left_items: Vec::default(),
            right_items: Vec::default(),
            active_pane: active_pane.clone(),
            mode: WorkspaceMode::default(),
            focus_handle: cx.focus_handle(),
            _observe_active_pane: cx
                .observe_in(active_pane, window, |this, _, window, cx| this.update_active_pane_item(window, cx)),
        };
        this.update_active_pane_item(window, cx);
        this
    }

    pub fn set_mode(&mut self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }

    fn shown_items<'a>(&self, items: &'a [StatusItem]) -> impl DoubleEndedIterator<Item = &'a StatusItem> {
        let mode = self.mode;
        items.iter().filter(move |item| item.modes.includes(mode))
    }

    /// Adds an item shown only in Workspace mode.
    pub fn add_left_item<T>(&mut self, item: Entity<T>, window: &mut Window, cx: &mut Context<Self>)
    where
        T: 'static + StatusItemView,
    {
        self.add_left_item_in(item, StatusItemModes::Editor, window, cx);
    }

    pub fn add_left_item_in<T>(
        &mut self,
        item: Entity<T>,
        modes: StatusItemModes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        T: 'static + StatusItemView,
    {
        let active_pane_item = self.active_pane.read(cx).active_item();
        item.set_active_pane_item(active_pane_item.as_deref(), window, cx);

        self.left_items.push(StatusItem { view: Box::new(item), modes });
        cx.notify();
    }

    pub fn item_of_type<T: StatusItemView>(&self) -> Option<Entity<T>> {
        self.left_items.iter().chain(&self.right_items).find_map(|item| item.to_any().downcast().ok())
    }

    /// Adds an item shown only in Workspace mode.
    pub fn add_right_item<T>(&mut self, item: Entity<T>, window: &mut Window, cx: &mut Context<Self>)
    where
        T: 'static + StatusItemView,
    {
        self.add_right_item_in(item, StatusItemModes::Editor, window, cx);
    }

    pub fn add_right_item_in<T>(
        &mut self,
        item: Entity<T>,
        modes: StatusItemModes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        T: 'static + StatusItemView,
    {
        let active_pane_item = self.active_pane.read(cx).active_item();
        item.set_active_pane_item(active_pane_item.as_deref(), window, cx);

        self.right_items.push(StatusItem { view: Box::new(item), modes });
        cx.notify();
    }

    pub fn set_active_pane(&mut self, active_pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        self.active_pane = active_pane.clone();
        self._observe_active_pane =
            cx.observe_in(active_pane, window, |this, _, window, cx| this.update_active_pane_item(window, cx));
        self.update_active_pane_item(window, cx);
    }

    fn update_active_pane_item(&self, window: &mut Window, cx: &mut Context<Self>) {
        let active_pane_item = self.active_pane.read(cx).active_item();
        for item in self.left_items.iter().chain(&self.right_items) {
            item.set_active_pane_item(active_pane_item.as_deref(), window, cx);
        }
    }

    /// Moves focus between the interactive controls within the status bar in
    /// response to arrow keys. Navigation is clamped to the status bar so
    /// arrows move between items and stop at the ends;
    /// Tab is still used to leave the toolbar.
    fn move_item_focus(&self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let previous = window.focused(cx);
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        let landed_in_status_bar = window.focused(cx).is_some_and(|handle| self.focus_handle.contains(&handle, window));
        if !landed_in_status_bar && let Some(previous) = previous {
            window.focus(&previous, cx);
        }
        cx.notify();
    }
}

impl<T: StatusItemView> StatusItemViewHandle for Entity<T> {
    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn set_active_pane_item(&self, active_pane_item: Option<&dyn ItemHandle>, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_active_pane_item(active_pane_item, window, cx));
    }
}

impl From<&dyn StatusItemViewHandle> for AnyView {
    fn from(val: &dyn StatusItemViewHandle) -> Self {
        val.to_any()
    }
}

#[cfg(test)]
mod tests {
    use ui::Color;

    use super::StatusItemModes;
    use super::status_item_color;
    use crate::WorkspaceMode;

    #[test]
    fn test_status_item_color() {
        assert_eq!(status_item_color(true), Color::Accent);
        assert_eq!(status_item_color(false), Color::Default);
    }

    #[test]
    fn test_status_item_modes_includes() {
        assert!(StatusItemModes::Editor.includes(WorkspaceMode::Editor));
        assert!(!StatusItemModes::Editor.includes(WorkspaceMode::GitHistory));
        assert!(!StatusItemModes::GitHistory.includes(WorkspaceMode::Editor));
        assert!(StatusItemModes::GitHistory.includes(WorkspaceMode::GitHistory));
        assert!(StatusItemModes::All.includes(WorkspaceMode::Editor));
        assert!(StatusItemModes::All.includes(WorkspaceMode::GitHistory));
    }
}
