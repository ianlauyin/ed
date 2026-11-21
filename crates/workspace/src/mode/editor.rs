use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use gpui::App;
use gpui::Axis;
use gpui::Bounds;
use gpui::Context;
use gpui::Div;
use gpui::DragMoveEvent;
use gpui::Entity;
use gpui::IntoElement;
use gpui::Pixels;
use gpui::Stateful;
use gpui::canvas;
use gpui::fill;
use gpui::motion;
use gpui::point;
use ui::Window;
use ui::prelude::ActiveTheme as _;
use ui::prelude::FluentBuilder as _;
use ui::prelude::InteractiveElement as _;
use ui::prelude::ParentElement as _;
use ui::prelude::Styled as _;
use ui::prelude::div;
use ui::prelude::h_flex;
use ui::prelude::px;
use ui::prelude::v_flex;

use crate::CARD_GAP;
use crate::Pane;
use crate::PaneRenderContext;
use crate::Workspace;
use crate::WorkspaceMode;
use crate::dock::Dock;
use crate::dock::DockPosition;
use crate::docks::DraggedDock;

impl Workspace {
    pub(super) fn render_editor_mode(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let background = cx.theme().colors().background;
        let pane_render_context = PaneRenderContext { active_pane: &self.active_pane, workspace: &self.weak_self };

        let layout = div()
            .relative()
            .flex()
            .flex_row()
            .h_full()
            .children(self.render_dock(DockPosition::Left, &self.left_dock, window, cx))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_hidden()
                    .child(h_flex().flex_1().child(self.render_center(&pane_render_context, window, cx)))
                    .children(self.render_dock(DockPosition::Bottom, &self.bottom_dock, window, cx))
                    .children(self.render_dock_transition(DockPosition::Bottom, &self.bottom_dock, cx)),
            )
            .children(self.render_dock_transition(DockPosition::Left, &self.left_dock, cx));

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

                                this.bottom_dock
                                    .update(cx, |dock, cx| dock.clamp_panel_size(bounds.size.height, window, cx));
                            }
                        });
                    },
                    |_, (), _, _| {},
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
                            DockPosition::Bottom => {
                                workspace.resize_bottom_dock(
                                    workspace.bounds.bottom() - e.event.position.y,
                                    window,
                                    cx,
                                );
                            }
                        }
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
                        .child(div().flex().flex_1().min_h_0().map(|this| match self.zoomed_center_pane(cx) {
                            Some(pane) => this.child(self.center.render_zoomed(&pane, cx)),
                            None => this.child(zoomed_view),
                        }))
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
            .child(div().flex().flex_1().min_h_0().size_full().child(dock.clone()));

        // Apply sizing only when the dock is open. When closed the dock is still
        // included in the element tree so its focus handle remains mounted — without
        // this, toggle_panel_focus cannot focus the panel when the dock is closed.
        let dock = dock.read(cx);
        if let Some(panel) = dock.visible_panel() {
            // Separate the open dock from the center cards.
            container = match position {
                DockPosition::Left => container.pr(CARD_GAP),
                DockPosition::Bottom => container.pt(CARD_GAP),
            };
            // Docks flex-shrink and clamp, so the stored size can differ from what's drawn.
            let motion = Rc::clone(&self.dock_motions[dock_index(position)]);
            container = container.relative().child(
                canvas(
                    move |bounds, _, _| {
                        motion.extent.set(if position == DockPosition::Bottom {
                            bounds.size.height
                        } else {
                            bounds.size.width
                        });
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
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

    /// Visual-only feedback for opening and closing a dock. Layout jumps to the final
    /// state at once; one overlay quad glides over it, so panes keep their cached
    /// frames and nothing re-lays out or resizes while it plays. Opening wipes a
    /// background curtain off the panel toward the center; closing shrinks a
    /// panel-coloured ghost back into the window edge.
    fn render_dock_transition(
        &self,
        position: DockPosition,
        dock: &Entity<Dock>,
        cx: &App,
    ) -> Option<impl IntoElement> {
        if self.zoomed_position == Some(position) {
            return None;
        }
        let open = dock.read(cx).is_open();
        // Advanced at prepaint, after the dock (an earlier sibling) recorded this frame's extent.
        let motion = Rc::clone(&self.dock_motions[dock_index(position)]);
        let colors = cx.theme().colors();
        let (curtain, panel) = (colors.background, colors.panel_background);

        Some(
            canvas(
                move |_, window, _| {
                    let (phase, active) = motion.advance(open, Instant::now());
                    if active {
                        window.request_animation_frame();
                    }
                    dock_transition_span(open, phase, motion.extent.get())
                },
                move |region, span, window, _| {
                    let Some((offset, span, overlay)) = span else {
                        return;
                    };
                    let color = match overlay {
                        DockOverlay::Curtain => curtain,
                        DockOverlay::Panel => panel,
                    };
                    window.paint_quad(fill(dock_overlay_bounds(position, region, offset, span), color));
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }

    /// Renders the center pane group, or nothing while a center pane is zoomed: the
    /// zoom card covers it and renders that pane through the group's motion.
    fn render_center(&self, render_cx: &PaneRenderContext, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let region = div().id("editor-region").relative().size_full();
        if self.zoomed_center_pane(cx).is_some() {
            return region;
        }
        region.child(self.center.render(self.zoomed.as_ref(), self.maximized_pane.as_ref(), render_cx, window, cx))
    }

    fn zoomed_center_pane(&self, cx: &App) -> Option<Entity<Pane>> {
        self.zoomed_pane(WorkspaceMode::Editor, cx).filter(|pane| self.center.panes().contains(&pane))
    }
}

const MAX_FRAME: Duration = Duration::from_millis(33);
/// The decay never arrives; stop once the overlay edge is within half a pixel.
const SETTLE_PX: f32 = 0.5;

/// A dock's open/close overlay state, advanced while painting.
#[derive(Default)]
pub(crate) struct DockMotion {
    /// Laid-out extent along the dock axis while open; kept while closed.
    extent: Cell<Pixels>,
    /// 0 closed, 1 open; `None` until first painted, so nothing plays on load.
    phase: Cell<Option<f32>>,
    last_frame: Cell<Option<Instant>>,
}

impl DockMotion {
    /// Glides the phase toward `open` (decelerating, no overshoot) and returns it
    /// with whether it is still moving. A toggle mid-flight continues from where
    /// it is.
    fn advance(&self, open: bool, now: Instant) -> (f32, bool) {
        let target = if open { 1.0 } else { 0.0 };
        let phase = match self.phase.get() {
            Some(phase) if phase != target => phase,
            _ => {
                self.phase.set(Some(target));
                self.last_frame.set(None);
                return (target, false);
            }
        };
        let elapsed = self
            .last_frame
            .get()
            .and_then(|last| now.checked_duration_since(last))
            .unwrap_or_default()
            .min(MAX_FRAME)
            .as_secs_f32();
        let mut phase = motion::GLIDE.step(phase, target, elapsed);
        let active = (phase - target).abs() * f32::from(self.extent.get()) > SETTLE_PX;
        if !active {
            phase = target;
        }
        self.phase.set(Some(phase));
        self.last_frame.set(active.then_some(now));
        (phase, active)
    }
}

const fn dock_index(position: DockPosition) -> usize {
    match position {
        DockPosition::Left => 0,
        DockPosition::Bottom => 1,
    }
}

/// The overlay quad `span` long, `offset` in from `position`'s edge of `region`.
fn dock_overlay_bounds(position: DockPosition, region: Bounds<Pixels>, offset: Pixels, span: Pixels) -> Bounds<Pixels> {
    let (left, top, right, bottom) = (region.left(), region.top(), region.right(), region.bottom());
    match position {
        DockPosition::Left => Bounds::from_corners(point(left + offset, top), point(left + offset + span, bottom)),
        DockPosition::Bottom => {
            Bounds::from_corners(point(left, bottom - offset - span), point(right, bottom - offset))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DockOverlay {
    /// Background over the part of the panel not yet revealed.
    Curtain,
    /// Panel colour where the panel appears to be but isn't laid out.
    Panel,
}

/// The overlay's `(offset from the window edge, extent, fill)` along the dock axis at
/// `phase` (0 closed, 1 open), or `None` once it has nothing to cover.
fn dock_transition_span(open: bool, phase: f32, size: Pixels) -> Option<(Pixels, Pixels, DockOverlay)> {
    let phase = phase.clamp(0.0, 1.0);
    let span = if open {
        (size * phase, size * (1.0 - phase), DockOverlay::Curtain)
    } else {
        (Pixels::ZERO, size * phase, DockOverlay::Panel)
    };
    (span.1 >= px(0.5)).then_some(span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_curtain_retreats_toward_center() {
        assert_eq!(dock_transition_span(true, 0.0, px(200.)), Some((px(0.), px(200.), DockOverlay::Curtain)));
        assert_eq!(dock_transition_span(true, 0.25, px(200.)), Some((px(50.), px(150.), DockOverlay::Curtain)));
        assert_eq!(dock_transition_span(true, -0.1, px(100.)), Some((px(0.), px(100.), DockOverlay::Curtain)));
        assert_eq!(dock_transition_span(true, 1.0, px(200.)), None);
    }

    #[test]
    fn dock_motion_starts_settled_then_glides_on_toggle() {
        let motion = DockMotion::default();
        motion.extent.set(px(300.));
        let mut now = Instant::now();
        assert_eq!(motion.advance(true, now), (1.0, false));

        // Closing: the first frame holds, then the phase decelerates toward 0.
        assert_eq!(motion.advance(false, now), (1.0, true));
        let (mut phase, mut previous_step) = (1.0_f32, f32::INFINITY);
        loop {
            now += Duration::from_millis(8);
            let (next, active) = motion.advance(false, now);
            assert!((0.0..=phase).contains(&next));
            assert!(next == 0.0 || phase - next <= previous_step);
            previous_step = phase - next;
            phase = next;
            if !active {
                break;
            }
        }
        assert_eq!(phase, 0.0);
        assert_eq!(motion.advance(false, now), (0.0, false));
    }

    #[test]
    fn dock_motion_retargets_from_current_phase() {
        let motion = DockMotion::default();
        motion.extent.set(px(300.));
        let mut now = Instant::now();
        motion.advance(false, now);
        motion.advance(true, now);
        now += Duration::from_millis(30);
        let (mid, _) = motion.advance(true, now);
        assert!(mid > 0.0 && mid < 1.0);
        now += Duration::from_millis(8);
        let (reversed, active) = motion.advance(false, now);
        assert!(active && reversed < mid);
    }

    #[test]
    fn closing_ghost_shrinks_into_edge() {
        assert_eq!(dock_transition_span(false, 1.0, px(200.)), Some((px(0.), px(200.), DockOverlay::Panel)));
        assert_eq!(dock_transition_span(false, 0.5, px(200.)), Some((px(0.), px(100.), DockOverlay::Panel)));
        assert_eq!(dock_transition_span(false, 0.0, px(200.)), None);
        assert_eq!(dock_transition_span(false, -0.1, px(200.)), None);
    }

    #[test]
    fn overlay_bounds_hug_each_edge() {
        let region = Bounds::from_corners(point(px(10.), px(20.)), point(px(1010.), px(620.)));
        let (offset, span) = (px(50.), px(100.));
        assert_eq!(
            dock_overlay_bounds(DockPosition::Left, region, offset, span),
            Bounds::from_corners(point(px(60.), px(20.)), point(px(160.), px(620.)))
        );
        assert_eq!(
            dock_overlay_bounds(DockPosition::Bottom, region, offset, span),
            Bounds::from_corners(point(px(10.), px(470.)), point(px(1010.), px(570.)))
        );
    }

    #[test]
    fn dock_indices_are_distinct() {
        let mut indices = [DockPosition::Left, DockPosition::Bottom].map(dock_index);
        indices.sort_unstable();
        assert_eq!(indices, [0, 1]);
    }

    #[test]
    fn empty_dock_has_no_overlay() {
        assert_eq!(dock_transition_span(true, 0.5, px(0.)), None);
    }
}
