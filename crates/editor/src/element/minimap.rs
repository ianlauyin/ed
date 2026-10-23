//! The minimap: layout, painting and the markers (cursors, git hunks,
//! highlights, diagnostics) drawn on it. It doubles as the editor's vertical
//! scroll widget, since there's no vertical scrollbar.

use std::sync::Arc;

use buffer_diff::DiffHunkStatusKind;
use gpui::AbsoluteLength;
use gpui::Along;
use gpui::AnyElement;
use gpui::App;
use gpui::AppContext as _;
use gpui::Axis as ScrollbarAxis;
use gpui::BorderStyle;
use gpui::Bounds;
use gpui::ContentMask;
use gpui::Corners;
use gpui::CursorStyle;
use gpui::DispatchPhase;
use gpui::Edges;
use gpui::HitboxBehavior;
use gpui::Hsla;
use gpui::IntoElement;
use gpui::IsZero as _;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::MouseMoveEvent;
use gpui::MouseUpEvent;
use gpui::PaintQuad;
use gpui::ParentElement;
use gpui::Pixels;
use gpui::Styled;
use gpui::Window;
use gpui::div;
use gpui::point;
use gpui::px;
use gpui::quad;
use gpui::size;
use itertools::Itertools;
use multi_buffer::MultiBufferPoint;
use project::project_settings::ProjectSettings;
use settings::Settings;
use theme::ActiveTheme;
use workspace::item::Item as _;
use workspace::item::ItemBufferKind;

use super::ColoredRange;
use super::EditorElement;
use super::EditorLayout;
use super::EditorScrollbars;
use super::ScrollbarLayout;
use super::ScrollbarLayoutInformation;
use crate::DisplayRow;
use crate::Editor;
use crate::EditorSettings;
use crate::EditorSnapshot;
use crate::MINIMAP_FONT_SIZE;
use crate::Point;
use crate::RowExt as _;
use crate::display_map::HighlightKey;
use crate::display_map::ToDisplayPoint;
use crate::editor_settings::Minimap;
use crate::editor_settings::MinimapThumb;
use crate::editor_settings::MinimapThumbBorder;
use crate::editor_settings::ShowMinimap;
use crate::scroll::ScrollOffset;
use crate::scroll::ScrollPixelOffset;
use crate::scroll::ScrollbarThumbState;

impl EditorElement {
    pub(super) fn layout_minimap(
        &self,
        snapshot: &EditorSnapshot,
        minimap_width: Pixels,
        scroll_position: gpui::Point<f64>,
        scrollbar_layout_information: &ScrollbarLayoutInformation,
        scrollbar_layout: Option<&EditorScrollbars>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<MinimapLayout> {
        let minimap_editor = self.editor.read(cx).minimap().cloned()?;

        let minimap_settings = EditorSettings::get_global(cx).minimap;

        if minimap_settings.on_active_editor() {
            let active_editor =
                self.editor.read(cx).workspace().and_then(|ws| {
                    ws.read(cx).active_pane().read(cx).active_item().and_then(|i| i.act_as::<Editor>(cx))
                });
            if active_editor.is_some_and(|e| e != self.editor) {
                return None;
            }
        }

        if !snapshot.mode.is_full()
            || minimap_width.is_zero()
            || matches!(
                minimap_settings.show,
                ShowMinimap::Auto if scrollbar_layout.is_none_or(|layout| !layout.visible)
            )
        {
            return None;
        }

        const MINIMAP_AXIS: ScrollbarAxis = ScrollbarAxis::Vertical;

        let ScrollbarLayoutInformation { editor_bounds, scroll_range, glyph_grid_cell } = scrollbar_layout_information;

        let line_height = glyph_grid_cell.height;
        let scroll_position = scroll_position.along(MINIMAP_AXIS);

        let top_right_anchor = editor_bounds.top_right();

        let thumb_state = self.editor.read_with(cx, |editor, _| editor.scroll_manager.minimap_thumb_state());

        let show_thumb = match minimap_settings.thumb {
            MinimapThumb::Always => true,
            MinimapThumb::Hover => thumb_state.is_some(),
        };

        let minimap_bounds = Bounds::from_anchor_and_size(
            gpui::Anchor::TopRight,
            top_right_anchor,
            size(minimap_width, editor_bounds.size.height),
        );
        let minimap_line_height = self.get_minimap_line_height(
            minimap_editor
                .read(cx)
                .text_style_refinement
                .as_ref()
                .and_then(|refinement| refinement.font_size)
                .unwrap_or(MINIMAP_FONT_SIZE),
            window,
            cx,
        );
        let minimap_height = minimap_bounds.size.height;

        let visible_editor_lines = (editor_bounds.size.height / line_height) as f64;
        let total_editor_lines = (scroll_range.height / line_height) as f64;
        let minimap_lines = (minimap_height / minimap_line_height) as f64;

        let minimap_scroll_top = MinimapLayout::calculate_minimap_top_offset(
            total_editor_lines,
            visible_editor_lines,
            minimap_lines,
            scroll_position,
        );

        let layout = ScrollbarLayout::for_minimap(
            window.insert_hitbox(minimap_bounds, HitboxBehavior::Normal),
            visible_editor_lines,
            total_editor_lines,
            minimap_line_height,
            scroll_position,
            minimap_scroll_top,
            show_thumb,
        )
        .with_thumb_state(thumb_state);

        let thumb_pixels_per_editor_line =
            MinimapLayout::thumb_pixels_per_editor_line(&layout, total_editor_lines, minimap_line_height);

        minimap_editor.update(cx, |editor, cx| editor.set_scroll_position(point(0., minimap_scroll_top), window, cx));

        // Required for the drop shadow to be visible
        const PADDING_OFFSET: Pixels = px(4.);

        let mut minimap = div().size_full().shadow_xs().px(PADDING_OFFSET).child(minimap_editor).into_any_element();

        let extended_bounds =
            minimap_bounds.extend(Edges { right: PADDING_OFFSET, left: PADDING_OFFSET, ..Default::default() });
        minimap.layout_as_root(extended_bounds.size.into(), window, cx);
        window.with_absolute_element_offset(extended_bounds.origin, |window| minimap.prepaint(window, cx));

        Some(MinimapLayout {
            minimap,
            thumb_layout: layout,
            thumb_border_style: minimap_settings.thumb_border,
            minimap_line_height,
            minimap_scroll_top,
            max_scroll_top: total_editor_lines,
            thumb_pixels_per_editor_line,
        })
    }

    fn get_minimap_line_height(&self, font_size: AbsoluteLength, window: &Window, cx: &App) -> Pixels {
        let rem_size = self.rem_size(cx).unwrap_or(window.rem_size());
        let mut text_style = self.style.text.clone();
        text_style.font_size = font_size;
        text_style.line_height_in_pixels(rem_size)
    }

    pub(super) fn get_minimap_width(
        &self,
        minimap_settings: &Minimap,
        text_width: Pixels,
        em_width: Pixels,
        font_size: Pixels,
        rem_size: Pixels,
        cx: &App,
    ) -> Option<Pixels> {
        let minimap_font_size = self.editor.read_with(cx, |editor, cx| {
            editor.minimap().map(|minimap_editor| {
                minimap_editor
                    .read(cx)
                    .text_style_refinement
                    .as_ref()
                    .and_then(|refinement| refinement.font_size)
                    .unwrap_or(MINIMAP_FONT_SIZE)
            })
        })?;

        let minimap_em_width = em_width * (minimap_font_size.to_pixels(rem_size) / font_size);

        let minimap_width = (text_width * MinimapLayout::MINIMAP_WIDTH_PCT)
            .min(minimap_em_width * minimap_settings.max_width_columns.get() as f32);

        (minimap_width >= minimap_em_width * MinimapLayout::MINIMAP_MIN_WIDTH_COLUMNS).then_some(minimap_width)
    }

    /// Markers (cursors, git hunks, highlights, diagnostics) drawn on top of the
    /// minimap, aligned with the minimap row of the line they belong to.
    fn minimap_marker_quads(
        &self,
        layout: &EditorLayout,
        minimap: &MinimapLayout,
        window: &Window,
        cx: &mut App,
    ) -> Vec<PaintQuad> {
        let bounds = minimap.thumb_layout.hitbox.bounds;
        let track = MinimapMarkerTrack { width: bounds.size.width, line_height: minimap.minimap_line_height };
        let fast_markers = self.collect_fast_minimap_markers(layout, track);
        // Refresh slow markers in the background. Below, we paint whatever
        // markers have already been computed.
        self.refresh_slow_minimap_markers(layout, track, window, cx);

        // Marker quads are laid out for the unscrolled minimap, so cached ones stay
        // valid while scrolling; shift them by the minimap's scroll offset here.
        let origin = bounds.origin
            - point(Pixels::ZERO, Pixels::from(minimap.minimap_scroll_top * f64::from(track.line_height)));
        let slow_markers = self.editor.read(cx).scrollbar_marker_state.markers.clone();
        slow_markers
            .iter()
            .chain(&fast_markers)
            .filter_map(|marker| {
                let mut marker = marker.clone();
                marker.bounds.origin += origin;
                marker.bounds.intersects(&bounds).then_some(marker)
            })
            .collect()
    }

    fn collect_fast_minimap_markers(&self, layout: &EditorLayout, track: MinimapMarkerTrack) -> Vec<PaintQuad> {
        const LIMIT: usize = 100;
        if layout.cursors.len() > LIMIT {
            return vec![];
        }
        let cursor_ranges = layout
            .cursors
            .iter()
            .map(|(point, color)| ColoredRange {
                start: point.row(),
                end: point.row(),
                color: color.opacity(MinimapMarkerTrack::CURSOR_LINE_OPACITY),
            })
            .collect_vec();
        track.quads_for_ranges(cursor_ranges, MinimapMarkerLane::Cursor)
    }

    fn refresh_slow_minimap_markers(
        &self,
        layout: &EditorLayout,
        track: MinimapMarkerTrack,
        window: &Window,
        cx: &mut App,
    ) {
        self.editor.update(cx, |editor, cx| {
            let is_singleton = editor.buffer_kind(cx) == ItemBufferKind::Singleton;
            let show_git_diff_markers = ProjectSettings::get_global(cx).git.enabled.diff
                && (is_singleton || editor.allow_git_diff_scrollbar_markers);
            if !is_singleton && !show_git_diff_markers {
                editor.scrollbar_marker_state.dirty = true;
                editor.scrollbar_marker_state.markers = Default::default();
                editor.scrollbar_marker_state.pending_refresh = None;
                return;
            }
            let track_size = size(track.width, track.line_height);
            if !editor.scrollbar_marker_state.should_refresh(track_size) {
                return;
            }

            let background_highlights = editor.background_highlights.clone();
            let snapshot = layout.position_map.snapshot.clone();
            let theme = cx.theme().clone();

            editor.scrollbar_marker_state.dirty = false;
            editor.scrollbar_marker_state.pending_refresh = Some(cx.spawn_in(window, async move |editor, cx| {
                let scrollbar_markers = cx
                    .background_spawn(async move {
                        let max_point = snapshot.display_snapshot.buffer_snapshot().max_point();
                        let mut marker_quads = Vec::new();
                        if show_git_diff_markers {
                            let marker_row_ranges = snapshot.buffer_snapshot().diff_hunks().map(|hunk| {
                                let start_display_row = MultiBufferPoint::new(hunk.row_range.start.0, 0)
                                    .to_display_point(&snapshot.display_snapshot)
                                    .row();
                                let mut end_display_row = MultiBufferPoint::new(hunk.row_range.end.0, 0)
                                    .to_display_point(&snapshot.display_snapshot)
                                    .row();
                                if end_display_row != start_display_row {
                                    end_display_row.0 -= 1;
                                }
                                let color = match &hunk.status().kind {
                                    DiffHunkStatusKind::Added => theme.colors().version_control_added,
                                    DiffHunkStatusKind::Modified => theme.colors().version_control_modified,
                                    DiffHunkStatusKind::Deleted => theme.colors().version_control_deleted,
                                };
                                ColoredRange { start: start_display_row, end: end_display_row, color }
                            });

                            marker_quads.extend(track.quads_for_ranges(marker_row_ranges, MinimapMarkerLane::Git));
                        }

                        for (background_highlight_id, (_, background_ranges)) in
                            background_highlights.iter().filter(|_| is_singleton)
                        {
                            let is_search_highlights = *background_highlight_id == HighlightKey::BufferSearchHighlights;
                            let is_text_highlights = *background_highlight_id == HighlightKey::SelectedTextHighlight;
                            let is_symbol_occurrences = *background_highlight_id == HighlightKey::DocumentHighlightRead
                                || *background_highlight_id == HighlightKey::DocumentHighlightWrite;
                            if is_search_highlights || is_text_highlights || is_symbol_occurrences {
                                let mut color = theme.status().info;
                                if is_symbol_occurrences {
                                    color.fade_out(0.5);
                                }
                                let marker_row_ranges = background_ranges.iter().map(|range| {
                                    let display_start = range.start.to_display_point(&snapshot.display_snapshot);
                                    let display_end = range.end.to_display_point(&snapshot.display_snapshot);
                                    ColoredRange { start: display_start.row(), end: display_end.row(), color }
                                });
                                marker_quads
                                    .extend(track.quads_for_ranges(marker_row_ranges, MinimapMarkerLane::Highlight));
                            }
                        }

                        if is_singleton {
                            let diagnostics = snapshot
                                .buffer_snapshot()
                                .diagnostics_in_range::<Point>(Point::zero()..max_point)
                                // We want to sort by severity, in order to paint the most severe diagnostics last.
                                .sorted_by_key(|diagnostic| std::cmp::Reverse(diagnostic.diagnostic.severity));

                            let marker_row_ranges = diagnostics.into_iter().map(|diagnostic| {
                                let start_display = diagnostic.range.start.to_display_point(&snapshot.display_snapshot);
                                let end_display = diagnostic.range.end.to_display_point(&snapshot.display_snapshot);
                                let color = match diagnostic.diagnostic.severity {
                                    lsp::DiagnosticSeverity::ERROR => theme.status().error,
                                    lsp::DiagnosticSeverity::WARNING => theme.status().warning,
                                    lsp::DiagnosticSeverity::INFORMATION => theme.status().info,
                                    _ => theme.status().hint,
                                };
                                ColoredRange { start: start_display.row(), end: end_display.row(), color }
                            });
                            marker_quads
                                .extend(track.quads_for_ranges(marker_row_ranges, MinimapMarkerLane::Diagnostic));
                        }

                        Arc::from(marker_quads)
                    })
                    .await;

                editor.update(cx, |editor, cx| {
                    editor.scrollbar_marker_state.markers = scrollbar_markers;
                    editor.scrollbar_marker_state.track_size = track_size;
                    editor.scrollbar_marker_state.pending_refresh = None;
                    cx.notify();
                })?;

                Ok(())
            }));
        });
    }

    pub(super) fn paint_minimap(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        let markers = layout
            .minimap
            .as_ref()
            .map(|minimap| self.minimap_marker_quads(layout, minimap, window, cx))
            .unwrap_or_default();
        if let Some(mut layout) = layout.minimap.take() {
            let minimap_hitbox = layout.thumb_layout.hitbox.clone();
            let dragging_minimap = self.editor.read(cx).scroll_manager.is_dragging_minimap();

            window.paint_layer(layout.thumb_layout.hitbox.bounds, |window| {
                window.with_element_namespace("minimap", |window| {
                    layout.minimap.paint(window, cx);
                    if let Some(thumb_bounds) = layout.thumb_layout.thumb_bounds {
                        let minimap_thumb_color = match layout.thumb_layout.thumb_state {
                            ScrollbarThumbState::Idle => cx.theme().colors().minimap_thumb_background,
                            ScrollbarThumbState::Hovered => cx.theme().colors().minimap_thumb_hover_background,
                            ScrollbarThumbState::Dragging => cx.theme().colors().minimap_thumb_active_background,
                        };
                        let minimap_thumb_border = match layout.thumb_border_style {
                            MinimapThumbBorder::Full => Edges::all(ScrollbarLayout::BORDER_WIDTH),
                            MinimapThumbBorder::LeftOnly => {
                                Edges { left: ScrollbarLayout::BORDER_WIDTH, ..Default::default() }
                            }
                            MinimapThumbBorder::LeftOpen => Edges {
                                right: ScrollbarLayout::BORDER_WIDTH,
                                top: ScrollbarLayout::BORDER_WIDTH,
                                bottom: ScrollbarLayout::BORDER_WIDTH,
                                ..Default::default()
                            },
                            MinimapThumbBorder::RightOpen => Edges {
                                left: ScrollbarLayout::BORDER_WIDTH,
                                top: ScrollbarLayout::BORDER_WIDTH,
                                bottom: ScrollbarLayout::BORDER_WIDTH,
                                ..Default::default()
                            },
                            MinimapThumbBorder::None => Default::default(),
                        };

                        // The minimap's bottom edge sits against the pane card's rounded
                        // corner, so the thumb's bottom-right corner is concentric with it,
                        // curving in as the thumb approaches the bottom.
                        let concentric_radius = workspace::CARD_RADIUS - workspace::CARD_INSET;
                        let gap_to_bottom = minimap_hitbox.bounds.bottom() - thumb_bounds.bottom();
                        let thumb_corners = Corners {
                            bottom_right: (concentric_radius - gap_to_bottom).max(Pixels::ZERO),
                            ..Default::default()
                        }
                        .clamp_radii_for_quad_size(thumb_bounds.size);

                        window.paint_layer(minimap_hitbox.bounds, |window| {
                            window.paint_quad(quad(
                                thumb_bounds,
                                thumb_corners,
                                minimap_thumb_color,
                                minimap_thumb_border,
                                cx.theme().colors().minimap_thumb_border,
                                BorderStyle::Solid,
                            ));
                        });
                    }
                    // Above the thumb, so its overlay doesn't wash out marker colors.
                    window.paint_layer(minimap_hitbox.bounds, |window| {
                        window.with_content_mask(Some(ContentMask { bounds: minimap_hitbox.bounds }), |window| {
                            for marker in markers {
                                window.paint_quad(marker);
                            }
                        });
                    });
                });
            });

            if dragging_minimap {
                window.set_window_cursor_style(CursorStyle::Arrow);
            } else {
                window.set_cursor_style(CursorStyle::Arrow, &minimap_hitbox);
            }

            let minimap_axis = ScrollbarAxis::Vertical;

            window.on_mouse_event({
                let editor = self.editor.clone();

                let minimap_hitbox = minimap_hitbox.clone();

                let mut mouse_position = window.mouse_position();

                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        return;
                    }

                    editor.update(cx, |editor, cx| {
                        if event.pressed_button == Some(MouseButton::Left)
                            && editor.scroll_manager.is_dragging_minimap()
                        {
                            let old_position = mouse_position.along(minimap_axis);
                            let new_position = event.position.along(minimap_axis);
                            if let Some(pixels_per_line) = layout.thumb_pixels_per_editor_line
                                && (minimap_hitbox.origin.along(minimap_axis)
                                    ..minimap_hitbox.bottom_right().along(minimap_axis))
                                    .contains(&old_position)
                            {
                                let position = editor.scroll_position(cx).apply_along(minimap_axis, |p| {
                                    (p + ScrollPixelOffset::from((new_position - old_position) / pixels_per_line))
                                        .max(0.)
                                });

                                editor.set_scroll_position(position, window, cx);
                            }
                            cx.stop_propagation();
                        } else if minimap_hitbox.is_hovered(window) {
                            editor.scroll_manager.set_is_hovering_minimap_thumb(
                                !event.dragging()
                                    && layout
                                        .thumb_layout
                                        .thumb_bounds
                                        .is_some_and(|bounds| bounds.contains(&event.position)),
                                cx,
                            );

                            // Stop hover events from propagating to the
                            // underlying editor if the minimap hitbox is hovered
                            if !event.dragging() {
                                cx.stop_propagation();
                            }
                        } else {
                            editor.scroll_manager.hide_minimap_thumb(cx);
                        }
                        mouse_position = event.position;
                    });
                }
            });

            if dragging_minimap {
                window.on_mouse_event({
                    let editor = self.editor.clone();
                    move |event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            return;
                        }

                        editor.update(cx, |editor, cx| {
                            if minimap_hitbox.is_hovered(window) {
                                editor.scroll_manager.set_is_hovering_minimap_thumb(
                                    layout
                                        .thumb_layout
                                        .thumb_bounds
                                        .is_some_and(|bounds| bounds.contains(&event.position)),
                                    cx,
                                );
                            } else {
                                editor.scroll_manager.hide_minimap_thumb(cx);
                            }
                            cx.stop_propagation();
                        });
                    }
                });
            } else {
                window.on_mouse_event({
                    let editor = self.editor.clone();

                    move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture || !minimap_hitbox.is_hovered(window) {
                            return;
                        }

                        let event_position = event.position;

                        let Some(thumb_bounds) = layout.thumb_layout.thumb_bounds else {
                            return;
                        };

                        editor.update(cx, |editor, cx| {
                            if !thumb_bounds.contains(&event_position) {
                                let click_position = event_position.relative_to(&minimap_hitbox.origin).y;

                                let top_position =
                                    (click_position - thumb_bounds.size.along(minimap_axis) / 2.0).max(Pixels::ZERO);

                                let scroll_offset = (layout.minimap_scroll_top
                                    + ScrollPixelOffset::from(top_position / layout.minimap_line_height))
                                .min(layout.max_scroll_top);

                                let scroll_position =
                                    editor.scroll_position(cx).apply_along(minimap_axis, |_| scroll_offset);
                                editor.set_scroll_position(scroll_position, window, cx);
                            }

                            editor.scroll_manager.set_is_dragging_minimap(cx);
                            cx.stop_propagation();
                        });
                    }
                });
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MinimapMarkerLane {
    /// Full-width translucent band, like a current line highlight.
    Cursor,
    /// Thin bar on the minimap's left edge.
    Git,
    /// Search / selection / symbol occurrences, right edge, inner lane.
    Highlight,
    /// Diagnostics, right edge, outer lane.
    Diagnostic,
}

/// Marker geometry on the minimap. Rows map to minimap lines of the unscrolled
/// minimap; [`EditorElement::minimap_marker_quads`] applies the scroll offset.
#[derive(Clone, Copy)]
struct MinimapMarkerTrack {
    width: Pixels,
    line_height: Pixels,
}

impl MinimapMarkerTrack {
    const GIT_LANE_WIDTH: Pixels = px(2.);
    const RIGHT_LANE_WIDTH: Pixels = px(3.);
    const MIN_MARKER_HEIGHT: Pixels = px(2.);
    const CURSOR_LINE_OPACITY: f32 = 0.25;

    fn quads_for_ranges(
        &self,
        row_ranges: impl IntoIterator<Item = ColoredRange<DisplayRow>>,
        lane: MinimapMarkerLane,
    ) -> Vec<PaintQuad> {
        let x_range = match lane {
            MinimapMarkerLane::Cursor => Pixels::ZERO..self.width,
            MinimapMarkerLane::Git => Pixels::ZERO..Self::GIT_LANE_WIDTH,
            MinimapMarkerLane::Highlight => {
                self.width - Self::RIGHT_LANE_WIDTH * 2.0..self.width - Self::RIGHT_LANE_WIDTH
            }
            MinimapMarkerLane::Diagnostic => self.width - Self::RIGHT_LANE_WIDTH..self.width,
        };

        let row_to_y = |row: DisplayRow| row.as_f64() as f32 * self.line_height;
        let mut pixel_ranges = row_ranges
            .into_iter()
            .map(|range| ColoredRange {
                start: row_to_y(range.start),
                end: row_to_y(range.end) + self.line_height.max(Self::MIN_MARKER_HEIGHT),
                color: range.color,
            })
            .peekable();

        let mut quads = Vec::new();
        while let Some(mut pixel_range) = pixel_ranges.next() {
            while let Some(next_pixel_range) = pixel_ranges.peek() {
                if pixel_range.end >= next_pixel_range.start && pixel_range.color == next_pixel_range.color {
                    pixel_range.end = next_pixel_range.end.max(pixel_range.end);
                    pixel_ranges.next();
                } else {
                    break;
                }
            }

            let bounds =
                Bounds::from_corners(point(x_range.start, pixel_range.start), point(x_range.end, pixel_range.end));
            quads.push(quad(
                bounds,
                Corners::default(),
                pixel_range.color,
                Edges::default(),
                Hsla::transparent_black(),
                BorderStyle::default(),
            ));
        }

        quads
    }
}

pub(super) struct MinimapLayout {
    pub minimap: AnyElement,
    pub thumb_layout: ScrollbarLayout,
    pub minimap_scroll_top: ScrollOffset,
    pub minimap_line_height: Pixels,
    pub thumb_border_style: MinimapThumbBorder,
    pub max_scroll_top: ScrollOffset,
    pub thumb_pixels_per_editor_line: Option<Pixels>,
}

impl MinimapLayout {
    /// The minimum width of the minimap in columns. If the minimap is smaller than this, it will be hidden.
    const MINIMAP_MIN_WIDTH_COLUMNS: f32 = 20.;
    /// The minimap width as a percentage of the editor width.
    const MINIMAP_WIDTH_PCT: f32 = 0.15;
    /// Calculates the scroll top offset the minimap editor has to have based on the
    /// current scroll progress.
    fn calculate_minimap_top_offset(
        document_lines: f64,
        visible_editor_lines: f64,
        visible_minimap_lines: f64,
        scroll_position: f64,
    ) -> ScrollOffset {
        let non_visible_document_lines = (document_lines - visible_editor_lines).max(0.);
        if non_visible_document_lines == 0. {
            0.
        } else {
            let scroll_percentage = (scroll_position / non_visible_document_lines).clamp(0., 1.);
            scroll_percentage * (document_lines - visible_minimap_lines).max(0.)
        }
    }

    /// How far the thumb moves on screen per scrolled editor line.
    ///
    /// The thumb moves along its oversized track, but the minimap content scrolls underneath
    /// it at the same time (see [`Self::calculate_minimap_top_offset`]), so what we actually
    /// see is the difference of the two. Returns `None` if the thumb can't move at all, e.g.
    /// because the whole document fits into the editor or the thumb covers the entire minimap
    /// (extremely unlikely, but may happen for very small editors that we currently allow)
    fn thumb_pixels_per_editor_line(
        thumb_layout: &ScrollbarLayout,
        document_lines: f64,
        minimap_line_height: Pixels,
    ) -> Option<Pixels> {
        let visible_editor_lines = thumb_layout.visible_range.end - thumb_layout.visible_range.start;
        let visible_minimap_lines = (thumb_layout.hitbox.size.height / minimap_line_height) as f64;
        let scrollable_editor_lines = document_lines - visible_editor_lines;
        if scrollable_editor_lines <= 0. {
            return None;
        }

        let minimap_scroll_per_editor_line = (document_lines - visible_minimap_lines).max(0.) / scrollable_editor_lines;
        let pixels_per_line = Pixels::from(
            ScrollOffset::from(thumb_layout.text_unit_size)
                - minimap_scroll_per_editor_line * ScrollOffset::from(minimap_line_height),
        );
        (pixels_per_line > Pixels::ZERO).then_some(pixels_per_line)
    }
}
