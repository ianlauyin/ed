use gpui::App;
use gpui::Axis;
use gpui::Context;
use gpui::Div;
use gpui::DragMoveEvent;
use gpui::Entity;
use gpui::IntoElement;
use gpui::Stateful;
use gpui::canvas;
use gpui::relative;
use settings::CenteredPaddingSettings;
use settings::Settings;
use ui::Window;
use ui::prelude::*;

use crate::BottomDockLayout;
use crate::CARD_GAP;
use crate::DraggedDock;
use crate::PaneRenderContext;
use crate::Workspace;
use crate::WorkspaceSettings;
use crate::dock::Dock;
use crate::dock::DockPosition;

impl Workspace {
    pub(super) fn render_editor_mode(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let pad_center_pane = self.centered_layout && self.center.panes().len() == 1 && self.active_item(cx).is_some();
        let pad_zoomed_pane = self.centered_layout && self.zoomed.is_some() && self.zoomed_position.is_none();
        let render_padding = |size| (size > 0.0).then(|| div().h_full().w(relative(size)));
        let render_centered_paddings = |enabled: bool| {
            if !enabled {
                return (None, None);
            }

            let settings = WorkspaceSettings::get_global(cx).centered_layout;
            (
                render_padding(Self::adjust_padding(settings.left_padding.map(|padding| padding.0))),
                render_padding(Self::adjust_padding(settings.right_padding.map(|padding| padding.0))),
            )
        };
        let centered_paddings = render_centered_paddings(pad_center_pane);
        let zoomed_paddings = render_centered_paddings(pad_zoomed_pane);
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
                            div().flex().flex_col().flex_1().overflow_hidden().child(
                                h_flex()
                                    .flex_1()
                                    .when_some(centered_paddings.0, |this, p| this.child(p.border_r_1()))
                                    .child(self.render_center(&pane_render_context, window, cx))
                                    .when_some(centered_paddings.1, |this, p| this.child(p.border_l_1())),
                            ),
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
                                .child(
                                    div().flex().flex_col().flex_1().overflow_hidden().child(
                                        h_flex()
                                            .flex_1()
                                            .when_some(centered_paddings.0, |this, p| this.child(p.border_r_1()))
                                            .child(self.render_center(&pane_render_context, window, cx))
                                            .when_some(centered_paddings.1, |this, p| this.child(p.border_l_1())),
                                    ),
                                ),
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
                                .child(
                                    div().flex().flex_col().flex_1().overflow_hidden().child(
                                        h_flex()
                                            .flex_1()
                                            .when_some(centered_paddings.0, |this, p| this.child(p.border_r_1()))
                                            .child(self.render_center(&pane_render_context, window, cx))
                                            .when_some(centered_paddings.1, |this, p| this.child(p.border_l_1())),
                                    ),
                                )
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
                        .child(
                            h_flex()
                                .flex_1()
                                .when_some(centered_paddings.0, |this, p| this.child(p.border_r_1()))
                                .child(self.render_center(&pane_render_context, window, cx))
                                .when_some(centered_paddings.1, |this, p| this.child(p.border_l_1())),
                        )
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
            .when(self.zoomed.is_none(), |this| {
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
                let zoomed_element = match zoomed_paddings {
                    (None, None) => zoomed_view.into_any_element(),
                    (left, right) => h_flex()
                        .size_full()
                        .when_some(left, |this, padding| this.child(padding.border_r_1()))
                        .child(div().size_full().child(zoomed_view))
                        .when_some(right, |this, padding| this.child(padding.border_l_1()))
                        .into_any_element(),
                };
                // Whatever is zoomed (editor or any dock) covers exactly the card
                // area, so every zoomed view gets the same size.
                Some(
                    div()
                        .occlude()
                        .absolute()
                        .top_0()
                        .left(CARD_GAP)
                        .right(CARD_GAP)
                        .bottom(if self.status_bar_visible(cx) { px(0.) } else { CARD_GAP })
                        .bg(background)
                        .child(zoomed_element)
                        .shadow_lg(),
                )
            }))
    }

    fn adjust_padding(padding: Option<f32>) -> f32 {
        padding
            .unwrap_or(CenteredPaddingSettings::default().0)
            .clamp(CenteredPaddingSettings::MIN_PADDING, CenteredPaddingSettings::MAX_PADDING)
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
        let mut container = div().id(dock_element_id).flex().overflow_hidden().flex_none().child(dock.clone());

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
