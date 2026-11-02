use gpui::App;
use gpui::Axis;
use gpui::Context;
use gpui::Div;
use gpui::DragMoveEvent;
use gpui::Entity;
use gpui::IntoElement;
use gpui::Stateful;
use gpui::canvas;
use settings::Settings;
use ui::Window;
use ui::prelude::*;

use crate::BottomDockLayout;
use crate::CARD_GAP;
use crate::DraggedDock;
use crate::PaneRenderContext;
use crate::Workspace;
use crate::WorkspaceMode;
use crate::WorkspaceSettings;
use crate::dock::Dock;
use crate::dock::DockPosition;

impl Workspace {
    pub(super) fn render_editor_mode(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let background = cx.theme().colors().background;
        let bottom_dock_layout = WorkspaceSettings::get_global(cx).bottom_dock_layout;
        let pane_render_context = PaneRenderContext { active_pane: &self.active_pane, workspace: &self.weak_self };

        let layout = match bottom_dock_layout {
            BottomDockLayout::Full => div()
                .flex()
                .flex_col()
                .h_full()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .overflow_hidden()
                        .children(self.render_dock(DockPosition::Left, &self.left_dock, window, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .overflow_hidden()
                                .child(h_flex().flex_1().child(self.render_center(&pane_render_context, window, cx))),
                        )
                        .children(self.render_dock(DockPosition::Right, &self.right_dock, window, cx)),
                )
                .child(div().w_full().children(self.render_dock(DockPosition::Bottom, &self.bottom_dock, window, cx))),

            BottomDockLayout::LeftAligned => div()
                .flex()
                .flex_row()
                .h_full()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
                                .children(self.render_dock(DockPosition::Left, &self.left_dock, window, cx))
                                .child(div().flex().flex_col().flex_1().overflow_hidden().child(
                                    h_flex().flex_1().child(self.render_center(&pane_render_context, window, cx)),
                                )),
                        )
                        .child(div().w_full().children(self.render_dock(
                            DockPosition::Bottom,
                            &self.bottom_dock,
                            window,
                            cx,
                        ))),
                )
                .children(self.render_dock(DockPosition::Right, &self.right_dock, window, cx)),
            BottomDockLayout::RightAligned => div()
                .flex()
                .flex_row()
                .h_full()
                .children(self.render_dock(DockPosition::Left, &self.left_dock, window, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
                                .child(div().flex().flex_col().flex_1().overflow_hidden().child(
                                    h_flex().flex_1().child(self.render_center(&pane_render_context, window, cx)),
                                ))
                                .children(self.render_dock(DockPosition::Right, &self.right_dock, window, cx)),
                        )
                        .child(div().w_full().children(self.render_dock(
                            DockPosition::Bottom,
                            &self.bottom_dock,
                            window,
                            cx,
                        ))),
                ),
            BottomDockLayout::Contained => div()
                .flex()
                .flex_row()
                .h_full()
                .children(self.render_dock(DockPosition::Left, &self.left_dock, window, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .overflow_hidden()
                        .child(h_flex().flex_1().child(self.render_center(&pane_render_context, window, cx)))
                        .children(self.render_dock(DockPosition::Bottom, &self.bottom_dock, window, cx)),
                )
                .children(self.render_dock(DockPosition::Right, &self.right_dock, window, cx)),
        };

        self.mode_container(cx)
            .child({
                let this = cx.entity();
                canvas(
                    move |bounds, window, cx| {
                        this.update(cx, |this, cx| {
                            let bounds_changed = this.bounds != bounds;
                            this.bounds = bounds;

                            if bounds_changed {
                                this.left_dock
                                    .update(cx, |dock, cx| dock.clamp_panel_size(bounds.size.width, window, cx));

                                this.right_dock
                                    .update(cx, |dock, cx| dock.clamp_panel_size(bounds.size.width, window, cx));

                                this.bottom_dock
                                    .update(cx, |dock, cx| dock.clamp_panel_size(bounds.size.height, window, cx));
                            }
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .when(!self.has_zoomed(WorkspaceMode::Editor, cx), |this| {
                this.on_drag_move(cx.listener(move |workspace, e: &DragMoveEvent<DraggedDock>, window, cx| {
                    if workspace.previous_dock_drag_coordinates != Some(e.event.position) {
                        workspace.previous_dock_drag_coordinates = Some(e.event.position);

                        match e.drag(cx).0 {
                            DockPosition::Left => {
                                workspace.resize_left_dock(e.event.position.x - workspace.bounds.left(), window, cx);
                            }
                            DockPosition::Right => {
                                workspace.resize_right_dock(workspace.bounds.right() - e.event.position.x, window, cx);
                            }
                            DockPosition::Bottom => {
                                workspace.resize_bottom_dock(
                                    workspace.bounds.bottom() - e.event.position.y,
                                    window,
                                    cx,
                                );
                            }
                        };
                        workspace.serialize_workspace(window, cx);
                    }
                }))
            })
            .child(layout)
            .children(self.zoomed.as_ref().and_then(|view| {
                let zoomed_view = view.upgrade()?;
                // Whatever is zoomed (editor or any dock) covers exactly the card
                // area, so every zoomed view gets the same size.
                // A zoomed dock covers the pane holding the window header, so it gets its own.
                Some(
                    v_flex()
                        .occlude()
                        .absolute()
                        .top_0()
                        .left(CARD_GAP)
                        .right(CARD_GAP)
                        .bottom(if self.status_bar_visible(cx) { px(0.) } else { CARD_GAP })
                        .bg(background)
                        .when(self.zoomed_position.is_some(), |this| this.child(self.render_zoomed_dock_header(cx)))
                        .child(div().flex_1().min_h_0().child(zoomed_view))
                        .shadow_lg(),
                )
            }))
    }

    fn render_dock(
        &self,
        position: DockPosition,
        dock: &Entity<Dock>,
        window: &Window,
        cx: &App,
    ) -> Option<Stateful<Div>> {
        if self.zoomed_position == Some(position) {
            return None;
        }

        let dock_element_id = match position {
            DockPosition::Left => "left-dock",
            DockPosition::Right => "right-dock",
            DockPosition::Bottom => "bottom-dock",
        };
        // The left dock is the sidebar: the window header sits above it.
        let has_header = position == DockPosition::Left
            && !self.has_zoomed(WorkspaceMode::Editor, cx)
            && !self.window_header_in_tab_bar(cx);
        let mut container = div()
            .id(dock_element_id)
            .flex()
            .overflow_hidden()
            .flex_none()
            .when(has_header, |this| this.flex_col().child(self.render_sidebar_header(cx)))
            // The right dock keeps the card gap from the window's top edge, like top-row tab bars,
            // so a panel's header row lines up with the tabs.
            .when(position == DockPosition::Right && dock.read(cx).visible_panel().is_some(), |this| {
                this.flex_col()
                    .child(ui::utils::window_drag_area(div().id("right-dock-top-gap")).flex_none().w_full().h(CARD_GAP))
            })
            .child(div().flex().flex_1().min_h_0().size_full().child(dock.clone()));

        // Apply sizing only when the dock is open. When closed the dock is still
        // included in the element tree so its focus handle remains mounted — without
        // this, toggle_panel_focus cannot focus the panel when the dock is closed.
        let dock = dock.read(cx);
        if let Some(panel) = dock.visible_panel() {
            // Separate the open dock from the center cards.
            container = match position {
                DockPosition::Left => container.pr(CARD_GAP),
                DockPosition::Right => container.pl(CARD_GAP),
                DockPosition::Bottom => container.pt(CARD_GAP),
            };
            let size_state = dock.stored_panel_size_state(panel.as_ref());
            let min_size = panel.min_size(window, cx);
            if position.axis() == Axis::Horizontal {
                let size = size_state.and_then(|state| state.size).unwrap_or_else(|| panel.default_size(window, cx));
                container = container.w(size);
                // Allow the dock to shrink when there isn't enough space
                // (e.g. when the sidebar is open). The stored size is
                // preserved so the dock expands back when space becomes
                // available.
                let style = container.style();
                style.flex_shrink = Some(1.0);
                if let Some(min) = min_size {
                    container = container.min_w(min);
                }
            } else {
                let size = size_state.and_then(|state| state.size).unwrap_or_else(|| panel.default_size(window, cx));
                container = container.h(size);
            }
        }

        Some(container)
    }

    /// Renders the center pane group.
    fn render_center(&self, render_cx: &PaneRenderContext, window: &mut Window, cx: &mut App) -> impl IntoElement {
        div().id("editor-region").size_full().child(self.center.render(
            self.zoomed.as_ref(),
            self.maximized_pane.as_ref(),
            render_cx,
            window,
            cx,
        ))
    }
}
