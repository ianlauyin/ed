mod header;
mod minimap;
mod mouse;

use std::any::TypeId;
use std::borrow::Cow;
use std::cell::Cell;
use std::cmp::Ordering;
use std::cmp::{self};
use std::fmt::Write;
use std::fmt::{self};
use std::iter;
use std::mem;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use buffer_diff::DiffHunkStatus;
use buffer_diff::DiffHunkStatusKind;
use collections::BTreeMap;
use collections::BTreeSet;
use collections::HashMap;
use collections::HashSet;
use git::Oid;
use git::blame::BlameEntry;
use gpui::Action;
use gpui::Along;
use gpui::AnyElement;
use gpui::App;
use gpui::AvailableSpace;
use gpui::Axis as ScrollbarAxis;
use gpui::BorderStyle;
use gpui::Bounds;
use gpui::ClipboardItem;
use gpui::ContentMask;
use gpui::Context;
use gpui::Corners;
use gpui::CursorStyle;
use gpui::DispatchPhase;
use gpui::Edges;
use gpui::Element;
use gpui::ElementInputHandler;
use gpui::Entity;
use gpui::Focusable as _;
use gpui::Font;
use gpui::FontId;
use gpui::FontWeight;
use gpui::GlobalElementId;
use gpui::Hitbox;
use gpui::HitboxBehavior;
use gpui::Hsla;
use gpui::InteractiveElement;
use gpui::IntoElement;
use gpui::IsZero;
use gpui::ModifiersChangedEvent;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::MouseMoveEvent;
use gpui::MouseUpEvent;
use gpui::ParentElement;
use gpui::Pixels;
use gpui::ScaledPixels;
use gpui::ShapedLine;
use gpui::SharedString;
use gpui::Size;
use gpui::StatefulInteractiveElement;
use gpui::Style;
use gpui::Styled;
use gpui::StyledText;
use gpui::TaskExt;
use gpui::TextAlign;
use gpui::TextRun;
use gpui::TextStyleRefinement;
use gpui::UnderlineStyle;
use gpui::Window;
use gpui::div;
use gpui::fill;
use gpui::outline;
use gpui::pattern_slash;
use gpui::point;
use gpui::px;
use gpui::quad;
use gpui::relative;
use gpui::size;
use gpui::solid_background;
use gpui::transparent_black;
use gpui::underline_y_offset;
pub use header::file_status_label_color;
pub(crate) use header::header_jump_data;
pub(crate) use header::render_buffer_header;
use itertools::Itertools;
use language::Diagnostic;
use language::HighlightedText;
use language::IndentGuideSettings;
use language::LanguageAwareStyling;
use language::language_settings::ShowWhitespaceSetting;
use minimap::MinimapLayout;
use multi_buffer::Anchor;
use multi_buffer::ExcerptRange;
use multi_buffer::ExpandExcerptDirection;
use multi_buffer::ExpandInfo;
use multi_buffer::MultiBufferOffset;
use multi_buffer::MultiBufferRow;
use multi_buffer::MultiBufferSnapshot;
use multi_buffer::RowInfo;
use multi_buffer::ToOffset;
use project::project_settings::ProjectSettings;
use settings::GitGutterSetting;
use settings::GitHunkStyleSetting;
use settings::IndentGuideBackgroundColoring;
use settings::IndentGuideColoring;
use settings::Settings;
use smallvec::SmallVec;
use smallvec::smallvec;
use sum_tree::Bias;
use text::BufferId;
use text::ToPoint as _;
use theme::ActiveTheme;
use theme::Appearance;
use theme::PlayerColor;
use theme_settings::BufferLineHeight;
use ui::ButtonLike;
use ui::POPOVER_Y_PADDING;
use ui::Tooltip;
use ui::prelude::*;
use ui::utils::ensure_highlight_contrast;
use ui::utils::merge_highlight_backgrounds;
use unicode_segmentation::UnicodeSegmentation;
use util::ResultExt;
use util::debug_panic;
use workspace::ItemHandle;
use workspace::Workspace;
use workspace::item::Item;
use workspace::item::ItemBufferKind;

use crate::BUFFER_HEADER_PADDING;
use crate::BlockId;
use crate::ChunkRendererContext;
use crate::ChunkReplacement;
use crate::CodeActionSource;
use crate::ConflictsOurs;
use crate::ConflictsOursMarker;
use crate::ConflictsOuter;
use crate::ConflictsTheirs;
use crate::ConflictsTheirsMarker;
use crate::ContextMenuPlacement;
use crate::CursorShape;
use crate::CustomBlockId;
use crate::DisplayDiffHunk;
use crate::DisplayPoint;
use crate::DisplayRow;
use crate::Editor;
use crate::EditorMode;
use crate::EditorSettings;
use crate::EditorSnapshot;
use crate::EditorStyle;
use crate::FILE_HEADER_HEIGHT;
use crate::FocusedBlock;
use crate::GutterDimensions;
use crate::HalfPageDown;
use crate::HalfPageUp;
use crate::HandleInput;
use crate::InlayHintRefreshReason;
use crate::LineDown;
use crate::LineHighlight;
use crate::LineUp;
use crate::MAX_LINE_LEN;
use crate::MIN_LINE_NUMBER_DIGITS;
use crate::PageDown;
use crate::PageUp;
use crate::Point;
use crate::RowExt;
use crate::RowRangeExt;
use crate::Selection;
use crate::SelectionDragState;
use crate::SizingBehavior;
use crate::SoftWrap;
use crate::ToPoint;
use crate::code_context_menus::CodeActionsMenu;
use crate::code_context_menus::MENU_ASIDE_MAX_WIDTH;
use crate::code_context_menus::MENU_ASIDE_MIN_WIDTH;
use crate::code_context_menus::MENU_GAP;
use crate::column_pixels;
use crate::cursor_animation::CursorViewport;
use crate::cursor_animation::LogicalCursorPosition;
use crate::cursor_animation::animated_corners_overlap_target;
use crate::display_map::Block;
use crate::display_map::BlockContext;
use crate::display_map::BlockStyle;
use crate::display_map::ChunkRendererId;
use crate::display_map::DisplaySnapshot;
use crate::display_map::EditorMargins;
use crate::display_map::HighlightKey;
use crate::display_map::HighlightedChunk;
use crate::display_map::ToDisplayPoint;
use crate::editor_settings::CurrentLineHighlight;
use crate::editor_settings::DocumentColorsRenderMode;
use crate::editor_settings::ScrollBeyondLastLine;
use crate::git::blame::BlameRenderer;
use crate::git::blame::GitBlame;
use crate::git::blame::GlobalBlameRenderer;
use crate::hover_popover::HOVER_POPOVER_GAP;
use crate::hover_popover::MIN_POPOVER_CHARACTER_WIDTH;
use crate::hover_popover::MIN_POPOVER_LINE_HEIGHT;
use crate::hover_popover::POPOVER_RIGHT_OFFSET;
use crate::hover_popover::{self};
use crate::inlay_hint_settings;
use crate::scroll::ActiveScrollbarState;
use crate::scroll::ScrollOffset;
use crate::scroll::ScrollPixelOffset;
use crate::scroll::ScrollbarThumbState;
use crate::scroll::scroll_amount::ScrollAmount;

/// Determines what kinds of highlights should be applied to a lines background.
#[derive(Clone, Copy, Default)]
struct LineHighlightSpec {
    selection: bool,
}

enum LineNumberStyle {
    DiffAdded,
    DiffDeleted,
    Active,
    Inactive,
}

impl LineNumberStyle {
    fn new(is_active: bool, diff_status: Option<DiffHunkStatus>) -> Self {
        match (is_active, diff_status.map(|status| status.kind)) {
            (true, _) => Self::Active,
            (_, Some(DiffHunkStatusKind::Added)) => Self::DiffAdded,
            (_, Some(DiffHunkStatusKind::Deleted)) => Self::DiffDeleted,
            (_, _) => Self::Inactive,
        }
    }

    const fn color(self, colors: &theme::ThemeColors) -> Hsla {
        match self {
            Self::DiffAdded => colors.version_control_added,
            Self::DiffDeleted => colors.version_control_deleted,
            Self::Active => colors.editor_active_line_number,
            Self::Inactive => colors.editor_line_number,
        }
    }
}

#[derive(Debug)]
struct SelectionLayout {
    id: usize,
    head: DisplayPoint,
    cursor_shape: CursorShape,
    is_newest: bool,
    is_local: bool,
    range: Range<DisplayPoint>,
    active_rows: Range<DisplayRow>,
    user_name: Option<SharedString>,
}

#[derive(Debug, Clone, Copy)]
struct PointDiagnostic {
    column: u32,
    underline: UnderlineStyle,
    severity: lsp::DiagnosticSeverity,
}

struct DiagnosticUnderline {
    origin: gpui::Point<Pixels>,
    width: Pixels,
    style: UnderlineStyle,
    bounds: Bounds<ScaledPixels>,
    span: Range<ScaledPixels>,
    severity: lsp::DiagnosticSeverity,
    is_point: bool,
}

impl DiagnosticUnderline {
    fn point(diagnostic: &PointDiagnostic, origin: gpui::Point<Pixels>, width: Pixels, window: &Window) -> Self {
        let bounds = window.underline_bounds(origin, width, &diagnostic.underline);
        Self {
            origin,
            width,
            style: diagnostic.underline,
            bounds,
            span: bounds.left()..bounds.right(),
            severity: diagnostic.severity,
            is_point: true,
        }
    }

    fn paint(&self, span: Range<ScaledPixels>, window: &mut Window) {
        window.paint_underline_with_exclusions(
            self.origin,
            self.width,
            &self.style,
            &[self.bounds.left()..span.start, span.end..self.bounds.right()],
        );
    }

    fn paint_all(underlines: &[Self], window: &mut Window) {
        if underlines.is_empty() {
            return;
        }
        let mask = window.content_mask().bounds;
        let scale = window.scale_factor();
        let left = ScaledPixels((f32::from(mask.left()) * scale).floor());
        let top = ScaledPixels((f32::from(mask.top()) * scale).floor());
        let right = ScaledPixels((f32::from(mask.right()) * scale).ceil()).max(left);
        let bottom = ScaledPixels((f32::from(mask.bottom()) * scale).ceil()).max(top);
        let mask = Bounds::from_corners(point(left, top), point(right, bottom));
        let mut events = SmallVec::<[_; 8]>::new();
        for (index, underline) in underlines.iter().enumerate() {
            let bounds = underline.bounds.intersect(&mask);
            let start = underline.span.start.max(bounds.left());
            let end = underline.span.end.min(bounds.right());
            if !bounds.is_empty() && start < end {
                events.push((start, true, index));
                events.push((end, false, index));
            }
        }
        events.sort_unstable();

        let mut active = BTreeSet::new();
        let mut previous = None;
        let mut visible: Option<(usize, Range<ScaledPixels>)> = None;
        for (position, is_start, index) in events {
            if let Some(start) = previous
                && start < position
                && let Some(&(_, _, winner)) = active.first()
            {
                if let Some((current, span)) = visible.as_mut()
                    && *current == winner
                    && span.end == start
                {
                    span.end = position;
                } else {
                    if let Some((current, span)) = visible.take() {
                        underlines[current].paint(span, window);
                    }
                    visible = Some((winner, start..position));
                }
            }
            let underline = &underlines[index];
            let key = (underline.severity, underline.is_point, index);
            if is_start {
                active.insert(key);
            } else {
                active.remove(&key);
            }
            previous = Some(position);
        }
        if let Some((winner, span)) = visible {
            underlines[winner].paint(span, window);
        }
    }
}

impl SelectionLayout {
    fn new<T: ToPoint + ToDisplayPoint + Clone>(
        selection: Selection<T>,
        line_mode: bool,
        cursor_offset: bool,
        cursor_shape: CursorShape,
        map: &DisplaySnapshot,
        is_newest: bool,
        is_local: bool,
        user_name: Option<SharedString>,
    ) -> Self {
        let id = selection.id;
        let buffer_snapshot = map.buffer_snapshot();
        let point_selection = selection.map(|p| p.to_point(buffer_snapshot));
        let display_selection = point_selection.map(|p| p.to_display_point(map));
        let mut range = display_selection.range();
        let mut head = display_selection.head();
        if !line_mode {
            let offset_range =
                point_selection.start.to_offset(buffer_snapshot)..point_selection.end.to_offset(buffer_snapshot);
            if let Some(contiguous_range) = map.contiguous_display_point_range_for_buffer_range(offset_range) {
                range = contiguous_range;
                // Keep the cursor attached to the highlight boundary; the
                // anchor-bias display position may sit on the far side of a
                // boundary inlay the highlight excludes.
                head = if selection.reversed { range.start } else { range.end };
            }
        }
        let mut active_rows =
            map.prev_line_boundary(point_selection.start).1.row()..map.next_line_boundary(point_selection.end).1.row();

        // vim visual line mode
        if line_mode {
            let point_range = map.expand_to_line(point_selection.range());
            range = point_range.start.to_display_point(map)..point_range.end.to_display_point(map);
        }

        // any vim visual mode (including line mode)
        if cursor_offset && !range.is_empty() && !selection.reversed {
            if head.column() > 0 {
                head = map.clip_point(DisplayPoint::new(head.row(), head.column() - 1), Bias::Left);
            } else if head.row().0 > 0 {
                head = map.clip_point(
                    DisplayPoint::new(head.row().previous_row(), map.line_len(head.row().previous_row())),
                    Bias::Left,
                );
                // updating range.end is a no-op unless you're cursor is
                // on the newline containing a multi-buffer divider
                // in which case the clip_point may have moved the head up
                // an additional row.
                range.end = DisplayPoint::new(head.row().next_row(), 0);
                active_rows.end = head.row();
            }
        }

        Self { id, head, cursor_shape, is_newest, is_local, range, active_rows, user_name }
    }
}

#[derive(Default)]
struct RenderBlocksOutput {
    // We store spacer blocks separately because they paint in a different order
    // (spacers -> indent guides -> non-spacers)
    non_spacer_blocks: Vec<BlockLayout>,
    spacer_blocks: Vec<BlockLayout>,
    row_block_types: HashMap<DisplayRow, bool>,
    resized_blocks: Option<HashMap<CustomBlockId, u32>>,
}

pub struct EditorElement {
    editor: Entity<Editor>,
    style: EditorStyle,
    split_side: Option<SplitSide>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSide {
    Left,
    Right,
}

impl EditorElement {
    pub(crate) const SCROLLBAR_WIDTH: Pixels = ui::SCROLLBAR_TRACK_WIDTH;

    pub fn new(editor: &Entity<Editor>, style: EditorStyle) -> Self {
        Self { editor: editor.clone(), style, split_side: None }
    }

    pub const fn set_split_side(&mut self, side: SplitSide) {
        self.split_side = Some(side);
    }

    fn register_actions(&self, window: &mut Window, cx: &mut App) {
        let editor = &self.editor;
        editor.update(cx, |editor, cx| {
            for action in editor.editor_actions.borrow().values() {
                (action)(editor, window, cx)
            }
        });

        crate::rust_analyzer_ext::apply_related_actions(editor, window, cx);

        register_action(editor, window, Editor::open_context_menu);
        register_action(editor, window, Editor::move_left);
        register_action(editor, window, Editor::move_right);
        register_action(editor, window, Editor::move_down);
        register_action(editor, window, Editor::move_down_by_lines);
        register_action(editor, window, Editor::select_down_by_lines);
        register_action(editor, window, Editor::move_up);
        register_action(editor, window, Editor::move_up_by_lines);
        register_action(editor, window, Editor::select_up_by_lines);
        register_action(editor, window, Editor::select_page_down);
        register_action(editor, window, Editor::select_page_up);
        register_action(editor, window, Editor::cancel);
        register_action(editor, window, Editor::next_snippet_tabstop);
        register_action(editor, window, Editor::previous_snippet_tabstop);
        register_action(editor, window, Editor::copy);
        register_action(editor, window, Editor::copy_and_trim);
        register_action(editor, window, Editor::diff_clipboard_with_selection);
        register_action(editor, window, Editor::move_page_up);
        register_action(editor, window, Editor::move_page_down);
        register_action(editor, window, Editor::next_screen);
        register_action(editor, window, Editor::scroll_cursor_top);
        register_action(editor, window, Editor::scroll_cursor_center);
        register_action(editor, window, Editor::scroll_cursor_bottom);
        register_action(editor, window, Editor::scroll_cursor_center_top_bottom);
        register_action(editor, window, |editor, _: &LineDown, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Line(1.), window, cx)
        });
        register_action(editor, window, |editor, _: &LineUp, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Line(-1.), window, cx)
        });
        register_action(editor, window, |editor, _: &HalfPageDown, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(0.5), window, cx)
        });
        register_action(editor, window, |editor, _: &HalfPageUp, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(-0.5), window, cx)
        });
        register_action(editor, window, |editor, _: &PageDown, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(1.), window, cx)
        });
        register_action(editor, window, |editor, _: &PageUp, window, cx| {
            editor.scroll_screen_with_cursor_margin(&ScrollAmount::Page(-1.), window, cx)
        });
        register_action(editor, window, Editor::move_to_previous_word_start);
        register_action(editor, window, Editor::move_to_previous_subword_start);
        register_action(editor, window, Editor::move_to_next_word_end);
        register_action(editor, window, Editor::move_to_next_subword_end);
        register_action(editor, window, Editor::move_to_beginning_of_line);
        register_action(editor, window, Editor::move_to_end_of_line);
        register_action(editor, window, Editor::move_to_start_of_paragraph);
        register_action(editor, window, Editor::move_to_end_of_paragraph);
        register_action(editor, window, Editor::move_to_next_comment_paragraph);
        register_action(editor, window, Editor::move_to_previous_comment_paragraph);
        register_action(editor, window, Editor::move_to_beginning);
        register_action(editor, window, Editor::move_to_end);
        register_action(editor, window, Editor::move_to_start_of_excerpt);
        register_action(editor, window, Editor::move_to_start_of_next_excerpt);
        register_action(editor, window, Editor::move_to_end_of_excerpt);
        register_action(editor, window, Editor::move_to_end_of_previous_excerpt);
        register_action(editor, window, Editor::select_up);
        register_action(editor, window, Editor::select_down);
        register_action(editor, window, Editor::select_left);
        register_action(editor, window, Editor::select_right);
        register_action(editor, window, Editor::select_to_previous_word_start);
        register_action(editor, window, Editor::select_to_previous_subword_start);
        register_action(editor, window, Editor::select_to_next_word_end);
        register_action(editor, window, Editor::select_to_next_subword_end);
        register_action(editor, window, Editor::select_to_beginning_of_line);
        register_action(editor, window, Editor::select_to_end_of_line);
        register_action(editor, window, Editor::select_to_start_of_paragraph);
        register_action(editor, window, Editor::select_to_end_of_paragraph);
        register_action(editor, window, Editor::select_to_start_of_excerpt);
        register_action(editor, window, Editor::select_to_start_of_next_excerpt);
        register_action(editor, window, Editor::select_to_end_of_excerpt);
        register_action(editor, window, Editor::select_to_end_of_previous_excerpt);
        register_action(editor, window, Editor::select_to_beginning);
        register_action(editor, window, Editor::select_to_end);
        register_action(editor, window, Editor::select_all);
        register_action(editor, window, |editor, action, window, cx| {
            editor.select_all_matches(action, window, cx).log_err();
        });
        register_action(editor, window, Editor::select_line);
        register_action(editor, window, Editor::split_selection_into_lines);
        register_action(editor, window, Editor::add_selection_above);
        register_action(editor, window, Editor::add_selection_below);
        register_action(editor, window, Editor::insert_snippet_at_selections);
        register_action(editor, window, |editor, action, window, cx| {
            editor.select_next(action, window, cx).log_err();
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.select_previous(action, window, cx).log_err();
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.find_next_match(action, window, cx).log_err();
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.find_previous_match(action, window, cx).log_err();
        });
        register_action(editor, window, Editor::select_larger_syntax_node);
        register_action(editor, window, Editor::select_smaller_syntax_node);
        register_action(editor, window, Editor::select_next_syntax_node);
        register_action(editor, window, Editor::select_prev_syntax_node);
        register_action(editor, window, Editor::select_to_start_of_larger_syntax_node);
        register_action(editor, window, Editor::select_to_end_of_larger_syntax_node);
        register_action(editor, window, Editor::move_to_start_of_larger_syntax_node);
        register_action(editor, window, Editor::move_to_end_of_larger_syntax_node);
        register_action(editor, window, Editor::select_enclosing_symbol);
        register_action(editor, window, Editor::move_to_enclosing_bracket);
        register_action(editor, window, Editor::select_inside_delimiters);
        register_action(editor, window, Editor::select_around_delimiters);
        register_action(editor, window, Editor::undo_selection);
        register_action(editor, window, Editor::redo_selection);
        if editor.read(cx).buffer_kind(cx) == ItemBufferKind::Multibuffer {
            register_action(editor, window, Editor::expand_excerpts);
            register_action(editor, window, Editor::expand_excerpts_up);
            register_action(editor, window, Editor::expand_excerpts_down);
        }
        register_action(editor, window, Editor::go_to_diagnostic);
        register_action(editor, window, Editor::go_to_prev_diagnostic);
        register_action(editor, window, Editor::go_to_next_hunk);
        register_action(editor, window, Editor::go_to_prev_hunk);
        register_action(editor, window, Editor::go_to_next_document_highlight);
        register_action(editor, window, Editor::go_to_prev_document_highlight);
        if editor.read(cx).lsp_data_enabled() {
            register_action(editor, window, Editor::open_definition_locations);
        }
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_definition(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_definition_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_declaration(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_declaration_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_implementation(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_implementation_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_type_definition(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, |editor, action, window, cx| {
            editor.go_to_type_definition_split(action, window, cx).detach_and_log_err(cx);
        });
        register_action(editor, window, Editor::open_url);
        register_action(editor, window, Editor::open_selected_filename);
        register_action(editor, window, Editor::fold);
        register_action(editor, window, Editor::fold_at_level);
        register_action(editor, window, Editor::fold_at_level_1);
        register_action(editor, window, Editor::fold_at_level_2);
        register_action(editor, window, Editor::fold_at_level_3);
        register_action(editor, window, Editor::fold_at_level_4);
        register_action(editor, window, Editor::fold_at_level_5);
        register_action(editor, window, Editor::fold_at_level_6);
        register_action(editor, window, Editor::fold_at_level_7);
        register_action(editor, window, Editor::fold_at_level_8);
        register_action(editor, window, Editor::fold_at_level_9);
        register_action(editor, window, Editor::fold_all);
        register_action(editor, window, Editor::fold_function_bodies);
        register_action(editor, window, Editor::fold_recursive);
        register_action(editor, window, Editor::toggle_fold);
        register_action(editor, window, Editor::toggle_fold_recursive);
        register_action(editor, window, Editor::toggle_fold_all);
        register_action(editor, window, Editor::unfold_lines);
        register_action(editor, window, Editor::unfold_recursive);
        register_action(editor, window, Editor::unfold_all);
        register_action(editor, window, Editor::fold_selected_ranges);
        register_action(editor, window, Editor::set_mark);
        register_action(editor, window, Editor::save_location);
        register_action(editor, window, Editor::swap_selection_ends);
        register_action(editor, window, Editor::show_completions);
        register_action(editor, window, Editor::show_word_completions);
        register_action(editor, window, Editor::toggle_code_actions);
        register_action(editor, window, Editor::open_excerpts);
        register_action(editor, window, Editor::open_excerpts_in_split);
        register_action(editor, window, Editor::toggle_soft_wrap);
        register_action(editor, window, Editor::toggle_tab_bar);
        register_action(editor, window, Editor::toggle_breadcrumb);
        register_action(editor, window, Editor::toggle_indent_guides);

        if editor.read(cx).lsp_data_enabled() {
            register_action(editor, window, Editor::toggle_inlay_hints);
            if editor.read(cx).can_accept_inlay_hint(cx) {
                register_action(editor, window, Editor::accept_inlay_hint);
            }
            register_action(editor, window, Editor::toggle_semantic_highlights);
            register_action(editor, window, Editor::toggle_diagnostics);
        }
        if editor.read(cx).inline_diagnostics_enabled() {
            register_action(editor, window, Editor::toggle_inline_diagnostics);
        }
        if editor.read(cx).supports_minimap(cx) {
            register_action(editor, window, Editor::toggle_minimap);
        }
        register_action(editor, window, hover_popover::hover);
        register_action(editor, window, Editor::reveal_in_finder);
        register_action(editor, window, Editor::copy_path);
        register_action(editor, window, Editor::copy_relative_path);
        register_action(editor, window, Editor::copy_file_name);
        register_action(editor, window, Editor::copy_file_name_without_extension);
        register_action(editor, window, Editor::copy_highlight_json);
        register_action(editor, window, Editor::copy_file_location);
        register_action(editor, window, Editor::toggle_git_blame);
        if editor.read(cx).blame().is_some() {
            register_action(editor, window, Editor::open_git_blame_commit);
            if editor.update(cx, |editor, cx| editor.blame_revision_target(window, cx).is_some()) {
                register_action(editor, window, Editor::blame_revision);
            }
            if editor.update(cx, |editor, cx| editor.blame_previous_revision_target(window, cx).is_some()) {
                register_action(editor, window, Editor::blame_previous_revision);
            }
        }
        register_action(editor, window, Editor::toggle_selected_diff_hunks);
        register_action(editor, window, Editor::toggle_staged_selected_diff_hunks);
        register_action(editor, window, Editor::stage_and_next);
        register_action(editor, window, Editor::unstage_and_next);
        register_action(editor, window, Editor::expand_all_diff_hunks);
        register_action(editor, window, Editor::collapse_all_diff_hunks);
        register_action(editor, window, Editor::toggle_all_diff_hunks);
        register_action(editor, window, Editor::go_to_previous_change);
        register_action(editor, window, Editor::go_to_next_change);
        register_action(editor, window, Editor::go_to_prev_reference);
        register_action(editor, window, Editor::go_to_next_reference);
        register_action(editor, window, Editor::go_to_previous_symbol);
        register_action(editor, window, Editor::go_to_next_symbol);
        register_action(editor, window, Editor::restart_language_server);
        register_action(editor, window, Editor::stop_language_server);
        register_action(editor, window, Editor::show_character_palette);
        register_action(editor, window, |editor, action, window, cx| {
            if let Some(task) = editor.compose_completion(action, window, cx) {
                editor.detach_and_notify_err(task, window, cx);
            } else {
                cx.propagate();
            }
        });
        register_action(editor, window, |editor, action, window, cx| {
            if let Some(task) = editor.find_all_references(action, window, cx) {
                task.detach_and_log_err(cx);
            } else {
                cx.propagate();
            }
        });

        register_action(editor, window, Editor::context_menu_first);
        register_action(editor, window, Editor::context_menu_prev);
        register_action(editor, window, Editor::context_menu_next);
        register_action(editor, window, Editor::context_menu_last);
        register_action(editor, window, Editor::open_active_item_in_terminal);
        register_action(editor, window, Editor::spawn_nearest_task);
        register_action(editor, window, Editor::open_selections_in_multibuffer);
        register_action(editor, window, Editor::toggle_read_only);
        register_action(editor, window, Editor::reload_file);

        if !editor.read(cx).read_only(cx) {
            register_action(editor, window, Editor::newline);
            register_action(editor, window, Editor::newline_above);
            register_action(editor, window, Editor::newline_below);
            register_action(editor, window, Editor::backspace);
            register_action(editor, window, Editor::delete);
            register_action(editor, window, Editor::tab);
            register_action(editor, window, Editor::backtab);
            register_action(editor, window, Editor::indent);
            register_action(editor, window, Editor::outdent);
            register_action(editor, window, Editor::autoindent);
            register_action(editor, window, Editor::delete_line);
            register_action(editor, window, Editor::join_lines);
            register_action(editor, window, Editor::sort_lines_by_length);
            register_action(editor, window, Editor::sort_lines_case_sensitive);
            register_action(editor, window, Editor::sort_lines_case_insensitive);
            register_action(editor, window, Editor::unique_lines_case_insensitive);
            register_action(editor, window, Editor::unique_lines_case_sensitive);
            register_action(editor, window, Editor::reverse_lines);
            register_action(editor, window, Editor::shuffle_lines);
            register_action(editor, window, Editor::rotate_selections_forward);
            register_action(editor, window, Editor::rotate_selections_backward);
            register_action(editor, window, Editor::convert_indentation_to_spaces);
            register_action(editor, window, Editor::convert_indentation_to_tabs);
            register_action(editor, window, Editor::convert_to_upper_case);
            register_action(editor, window, Editor::convert_to_lower_case);
            register_action(editor, window, Editor::convert_to_title_case);
            register_action(editor, window, Editor::convert_to_snake_case);
            register_action(editor, window, Editor::convert_to_kebab_case);
            register_action(editor, window, Editor::convert_to_upper_camel_case);
            register_action(editor, window, Editor::convert_to_lower_camel_case);
            register_action(editor, window, Editor::convert_to_opposite_case);
            register_action(editor, window, Editor::convert_to_sentence_case);
            register_action(editor, window, Editor::toggle_case);
            register_action(editor, window, Editor::convert_to_rot13);
            register_action(editor, window, Editor::convert_to_rot47);
            register_action(editor, window, Editor::convert_to_base64);
            register_action(editor, window, Editor::convert_from_base64);
            register_action(editor, window, Editor::delete_to_previous_word_start);
            register_action(editor, window, Editor::delete_to_previous_subword_start);
            register_action(editor, window, Editor::delete_to_next_word_end);
            register_action(editor, window, Editor::delete_to_next_subword_end);
            register_action(editor, window, Editor::delete_to_beginning_of_line);
            register_action(editor, window, Editor::delete_to_end_of_line);
            register_action(editor, window, Editor::cut_to_end_of_line);
            register_action(editor, window, Editor::duplicate_line_up);
            register_action(editor, window, Editor::duplicate_line_down);
            register_action(editor, window, Editor::duplicate_selection);
            register_action(editor, window, Editor::move_line_up);
            register_action(editor, window, Editor::move_line_down);
            register_action(editor, window, Editor::transpose);
            register_action(editor, window, |editor, _: &crate::Rewrap, _, cx| {
                editor.rewrap(crate::RewrapOptions::default(), cx);
            });
            register_action(editor, window, Editor::cut);
            register_action(editor, window, Editor::kill_ring_cut);
            register_action(editor, window, Editor::kill_ring_yank);
            register_action(editor, window, Editor::paste);
            register_action(editor, window, Editor::undo);
            register_action(editor, window, Editor::redo);
            register_action(editor, window, Editor::toggle_comments);
            register_action(editor, window, Editor::toggle_block_comments);
            register_action(editor, window, Editor::toggle_markdown_block_quote);
            register_action(editor, window, Editor::unwrap_syntax_node);

            register_action(editor, window, Editor::restore_file);
            register_action(editor, window, Editor::git_restore);
            register_action(editor, window, Editor::restore_and_next);
            register_action(editor, window, Editor::apply_all_diff_hunks);
            register_action(editor, window, Editor::apply_selected_diff_hunks);
            register_action(editor, window, Editor::insert_uuid_v4);
            register_action(editor, window, Editor::insert_uuid_v7);
            register_action(editor, window, Editor::align_selections);
            if editor.read(cx).enable_wrap_selections_in_tag(cx) {
                register_action(editor, window, Editor::wrap_selections_in_tag);
            }
            register_action(editor, window, |editor, HandleInput(text): &HandleInput, window, cx| {
                if text.is_empty() {
                    return;
                }
                editor.handle_input(text, window, cx);
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.format(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            if editor.read(cx).can_format_selections(cx) {
                register_action(editor, window, |editor, action, window, cx| {
                    if let Some(task) = editor.format_selections(action, window, cx) {
                        editor.detach_and_notify_err(task, window, cx);
                    } else {
                        cx.propagate();
                    }
                });
            }
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.organize_imports(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_completion(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_completion_replace(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_completion_insert(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_code_action(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.rename(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
            register_action(editor, window, |editor, action, window, cx| {
                if let Some(task) = editor.confirm_rename(action, window, cx) {
                    editor.detach_and_notify_err(task, window, cx);
                } else {
                    cx.propagate();
                }
            });
        }
    }

    fn register_key_listeners(&self, window: &mut Window, _: &mut App, layout: &EditorLayout) {
        let position_map = layout.position_map.clone();
        window.on_key_event({
            let editor = self.editor.clone();
            move |event: &ModifiersChangedEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                editor.update(cx, |editor, cx| {
                    let inlay_hint_settings = inlay_hint_settings(
                        editor.selections.newest_anchor().head(),
                        &editor.buffer.read(cx).snapshot(cx),
                        cx,
                    );

                    if let Some(inlay_modifiers) =
                        inlay_hint_settings.toggle_on_modifiers_press.as_ref().filter(|modifiers| modifiers.modified())
                    {
                        editor.refresh_inlay_hints(
                            InlayHintRefreshReason::ModifiersChanged(inlay_modifiers == &event.modifiers),
                            cx,
                        );
                    }

                    if editor.hover_state.focused(window, cx) {
                        return;
                    }

                    editor.handle_modifiers_changed(event.modifiers, &position_map, window, cx);
                })
            }
        });
    }

    fn layout_selections(
        &self,
        start_anchor: Anchor,
        end_anchor: Anchor,
        local_selections: &[Selection<Point>],
        snapshot: &EditorSnapshot,
        start_row: DisplayRow,
        end_row: DisplayRow,
        window: &Window,
        cx: &mut App,
    ) -> (Vec<(PlayerColor, Vec<SelectionLayout>)>, BTreeMap<DisplayRow, LineHighlightSpec>, Option<DisplayPoint>) {
        let mut selections: Vec<(PlayerColor, Vec<SelectionLayout>)> = Vec::new();
        let mut active_rows = BTreeMap::new();
        let mut newest_selection_head = None;

        let Some(editor_with_selections) = self.editor_with_selections(cx) else {
            return (selections, active_rows, newest_selection_head);
        };

        editor_with_selections.update(cx, |editor, cx| {
            if editor.show_local_selections {
                let mut layouts = Vec::new();
                let newest = editor.selections.newest(&editor.display_snapshot(cx));
                for selection in local_selections.iter().cloned() {
                    let is_empty = selection.start == selection.end;
                    let is_newest = selection == newest;

                    let layout = SelectionLayout::new(
                        selection,
                        editor.selections.line_mode(),
                        editor.cursor_offset_on_selection,
                        editor.cursor_shape,
                        &snapshot.display_snapshot,
                        is_newest,
                        true,
                        None,
                    );
                    if is_newest {
                        newest_selection_head = Some(layout.head);
                    }

                    for row in cmp::max(layout.active_rows.start.0, start_row.0)
                        ..=cmp::min(layout.active_rows.end.0, end_row.0)
                    {
                        let contains_non_empty_selection =
                            active_rows.entry(DisplayRow(row)).or_insert_with(LineHighlightSpec::default);
                        contains_non_empty_selection.selection |= !is_empty;
                    }
                    layouts.push(layout);
                }

                let mut player = editor.current_user_player_color(cx);
                if !editor.is_focused(window) {
                    const UNFOCUS_EDITOR_SELECTION_OPACITY: f32 = 0.5;
                    player.selection = player.selection.opacity(UNFOCUS_EDITOR_SELECTION_OPACITY);
                }
                selections.push((player, layouts));

                if let SelectionDragState::Dragging { ref selection, ref drop_cursor, ref hide_drop_cursor } =
                    editor.selection_drag_state
                    && !hide_drop_cursor
                    && (drop_cursor.start.cmp(&selection.start, snapshot.buffer_snapshot()).eq(&Ordering::Less)
                        || drop_cursor.end.cmp(&selection.end, snapshot.buffer_snapshot()).eq(&Ordering::Greater))
                {
                    let drag_cursor_layout = SelectionLayout::new(
                        *drop_cursor,
                        false,
                        editor.cursor_offset_on_selection,
                        CursorShape::Bar,
                        &snapshot.display_snapshot,
                        false,
                        false,
                        None,
                    );
                    let mut drop_color = cx.theme().player();
                    drop_color.cursor = drop_color.cursor.opacity(0.5);
                    selections.push((drop_color, vec![drag_cursor_layout]));
                }
            }

            if !editor.is_focused(window) && editor.show_cursor_when_unfocused {
                let cursor_offset_on_selection = editor.cursor_offset_on_selection;

                let layouts = snapshot
                    .buffer_snapshot()
                    .selections_in_range(&(start_anchor..end_anchor), true)
                    .map(move |(_, line_mode, cursor_shape, selection)| {
                        SelectionLayout::new(
                            selection,
                            line_mode,
                            cursor_offset_on_selection,
                            cursor_shape,
                            &snapshot.display_snapshot,
                            false,
                            false,
                            None,
                        )
                    })
                    .collect::<Vec<_>>();
                let player = editor.current_user_player_color(cx);
                selections.push((player, layouts));
            }
        });

        #[cfg(debug_assertions)]
        Self::layout_debug_ranges(&mut selections, start_anchor..end_anchor, &snapshot.display_snapshot, cx);

        (selections, active_rows, newest_selection_head)
    }

    fn collect_cursors(&self, snapshot: &EditorSnapshot, cx: &App) -> Vec<(DisplayPoint, Hsla)> {
        let editor = self.editor.read(cx);
        let mut cursors = Vec::new();
        let color = cx.theme().player().cursor;
        let mut add_cursor = |anchor: Anchor| {
            cursors.push((anchor.to_display_point(&snapshot.display_snapshot), color));
        };
        editor.selections.disjoint_anchors().iter().for_each(|selection| add_cursor(selection.head()));
        if let Some(selection) = editor.selections.pending_anchor() {
            add_cursor(selection.head());
        }
        cursors
    }

    fn layout_visible_cursors(
        &self,
        snapshot: &EditorSnapshot,
        selections: &[(PlayerColor, Vec<SelectionLayout>)],
        row_block_types: &HashMap<DisplayRow, bool>,
        visible_display_row_range: Range<DisplayRow>,
        line_layouts: &[LineWithInvisibles],
        text_hitbox: &Hitbox,
        content_origin: gpui::Point<Pixels>,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        line_height: Pixels,
        em_width: Pixels,
        em_advance: Pixels,
        autoscroll_containing_element: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<CursorLayout> {
        let mut autoscroll_bounds = None;
        let cursor_layouts = self.editor.update(cx, |editor, cx| {
            let mut cursors = Vec::new();
            let mut handled_animation_cursors = HashSet::default();
            let mut request_animation_frame = false;

            let show_local_cursors = editor.show_local_cursors(window, cx);
            let animation_settings = EditorSettings::get_global(cx).cursor_animation;
            let animation_enabled = animation_settings.enabled && !cx.reduce_motion();
            let animation_context = animation_enabled.then(|| {
                (
                    CursorViewport::new(
                        content_origin,
                        text_hitbox.bounds,
                        scroll_position,
                        scroll_pixel_position,
                        line_height,
                        em_advance,
                    ),
                    Instant::now(),
                )
            });

            if animation_enabled {
                let newest_animation_selection_id =
                    if true && cursor_shape_supports_cursor_animation(editor.cursor_shape) {
                        Some(editor.selections.newest_anchor().id)
                    } else {
                        None
                    };

                editor.cursor_animations.reconcile_newest_selection(newest_animation_selection_id);
            } else {
                editor.cursor_animations.clear();
            }

            for (player_color, selections) in selections {
                for selection in selections {
                    let cursor_position = selection.head;

                    let in_range = visible_display_row_range.contains(&cursor_position.row());
                    if (selection.is_local && !show_local_cursors)
                        || !in_range
                        || row_block_types.get(&cursor_position.row()) == Some(&true)
                    {
                        continue;
                    }

                    let cursor_row_layout =
                        &line_layouts[cursor_position.row().minus(visible_display_row_range.start) as usize];
                    let cursor_column = cursor_position.column() as usize;

                    let cursor_character_x = cursor_row_layout.x_for_index(cursor_column)
                        + cursor_row_layout.alignment_offset(self.style.text.text_align, text_hitbox.size.width);
                    let cursor_next_x = cursor_row_layout.x_for_index(cursor_column + 1)
                        + cursor_row_layout.alignment_offset(self.style.text.text_align, text_hitbox.size.width);
                    let mut cell_width = cursor_next_x - cursor_character_x;
                    if cell_width == Pixels::ZERO {
                        cell_width = em_advance;
                    }

                    let mut block_width = cell_width;
                    let mut block_text = None;

                    if selection.cursor_shape == CursorShape::Block
                        && let Some(text) = snapshot.grapheme_at(cursor_position).or_else(|| {
                            if snapshot.is_empty() {
                                snapshot
                                    .placeholder_text()
                                    .and_then(|s| s.graphemes(true).next().map(|s| s.to_string().into()))
                            } else {
                                None
                            }
                        })
                    {
                        let is_ascii_whitespace_only = text.as_ref().chars().all(|c| c.is_ascii_whitespace());
                        let len = text.len();

                        let mut font = cursor_row_layout
                            .font_id_for_index(cursor_column)
                            .and_then(|cursor_font_id| window.text_system().get_font_for_id(cursor_font_id))
                            .unwrap_or(self.style.text.font());
                        font.features = self.style.text.font_features.clone();

                        // Invert the text color for the block cursor. Ensure that the text
                        // color is opaque enough to be visible against the background color.
                        //
                        // 0.75 is an arbitrary threshold to determine if the background color is
                        // opaque enough to use as a text color.
                        //
                        // TODO: In the future we should ensure themes have a `text_inverse` color.
                        let color = if cx.theme().colors().editor_background.a < 0.75 {
                            match cx.theme().appearance {
                                Appearance::Dark => Hsla::black(),
                                Appearance::Light => Hsla::white(),
                            }
                        } else {
                            cx.theme().colors().editor_background
                        };

                        let shaped = window.text_system().shape_line(
                            text,
                            cursor_row_layout.font_size,
                            &[TextRun { len, font, color, ..Default::default() }],
                            None,
                        );
                        if !is_ascii_whitespace_only {
                            block_width = block_width.max(shaped.width);
                        }
                        block_text = Some(shaped);
                    }

                    let x = cursor_character_x - scroll_pixel_position.x.into();
                    let y = ((cursor_position.row().as_f64() - scroll_position.y)
                        * ScrollPixelOffset::from(line_height))
                    .into();
                    if selection.is_newest {
                        editor.pixel_position_of_newest_cursor = Some(point(
                            text_hitbox.origin.x + x + block_width / 2.,
                            text_hitbox.origin.y + y + line_height / 2.,
                        ));

                        if autoscroll_containing_element {
                            let top = text_hitbox.origin.y
                                + ((cursor_position.row().as_f64() - scroll_position.y - 3.).max(0.)
                                    * ScrollPixelOffset::from(line_height))
                                .into();
                            let left = text_hitbox.origin.x
                                + ((cursor_position.column() as ScrollOffset - scroll_position.x - 3.).max(0.)
                                    * ScrollPixelOffset::from(em_width))
                                .into();

                            let bottom = text_hitbox.origin.y
                                + ((cursor_position.row().as_f64() - scroll_position.y + 4.)
                                    * ScrollPixelOffset::from(line_height))
                                .into();
                            let right = text_hitbox.origin.x
                                + ((cursor_position.column() as ScrollOffset - scroll_position.x + 4.)
                                    * ScrollPixelOffset::from(em_width))
                                .into();

                            autoscroll_bounds = Some(Bounds::from_corners(point(left, top), point(right, bottom)))
                        }
                    }

                    let mut cursor = CursorLayout {
                        color: player_color.cursor,
                        block_width,
                        origin: point(x, y),
                        line_height,
                        shape: selection.cursor_shape,
                        block_text,
                        cursor_name: None,
                        animated_corners: None,
                    };
                    let cursor_name = selection.user_name.clone().map(|name| CursorName {
                        string: name,
                        color: self.style.background,
                        is_top_row: cursor_position.row().0 == 0,
                    });
                    cursor.layout(content_origin, cursor_name, window, cx);
                    if selection.is_local && cursor_shape_supports_cursor_animation(selection.cursor_shape) {
                        if let Some((cursor_viewport, animation_now)) = animation_context {
                            handled_animation_cursors.insert(selection.id);
                            let target_bounds = window.pixel_snap_bounds(cursor.bounds(content_origin));
                            cursor.animated_corners = editor.cursor_animations.update(
                                selection.id,
                                LogicalCursorPosition {
                                    row: cursor_position.row().0,
                                    column: cursor_position.column(),
                                },
                                target_bounds,
                                cursor_viewport,
                                animation_now,
                            );
                            request_animation_frame |= cursor.animated_corners.is_some();
                        }
                    } else if animation_enabled && selection.is_local {
                        editor.cursor_animations.remove(selection.id);
                    }
                    cursors.push(cursor);
                }
            }

            if animation_enabled {
                editor.cursor_animations.capture_newest_state();
                editor.cursor_animations.retain(|selection_id| handled_animation_cursors.contains(&selection_id));
                if request_animation_frame {
                    window.request_animation_frame();
                }
            }

            cursors
        });

        if let Some(bounds) = autoscroll_bounds {
            window.request_autoscroll(bounds);
        }

        cursor_layouts
    }

    fn layout_navigation_overlays(
        &self,
        snapshot: &EditorSnapshot,
        visible_display_row_range: Range<DisplayRow>,
        line_layouts: &[LineWithInvisibles],
        text_hitbox: &Hitbox,
        content_origin: gpui::Point<Pixels>,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<NavigationOverlayPaintCommand> {
        let mut overlay_sets = self
            .editor
            .read(cx)
            .navigation_overlay_sets()
            .iter()
            .map(|(key, overlays)| (*key, overlays.clone()))
            .collect::<Vec<_>>();
        if overlay_sets.is_empty() {
            return Vec::new();
        }
        overlay_sets.sort_by_key(|(key, _)| *key);

        let layout_context = NavigationOverlayLayoutContext {
            display_snapshot: &snapshot.display_snapshot,
            visible_display_row_range: &visible_display_row_range,
            line_layouts,
            text_align: self.style.text.text_align,
            content_width: text_hitbox.size.width,
            content_origin,
            scroll_position,
            scroll_pixel_position,
            line_height,
            editor_font: self.style.text.font(),
            editor_font_size: self.style.text.font_size.to_pixels(window.rem_size()),
        };
        let mut navigation_overlay_paint_commands = Vec::new();

        for (_, overlays) in overlay_sets {
            for overlay in overlays.as_ref() {
                Self::layout_navigation_label(
                    overlay,
                    &layout_context,
                    window,
                    cx,
                    &mut navigation_overlay_paint_commands,
                );
            }
        }

        navigation_overlay_paint_commands
    }

    fn layout_navigation_label(
        overlay: &crate::NavigationTargetOverlay,
        context: &NavigationOverlayLayoutContext<'_>,
        window: &mut Window,
        cx: &mut App,
        paint_commands: &mut Vec<NavigationOverlayPaintCommand>,
    ) {
        let label = &overlay.label;
        let label_display_point = overlay.target_range.start.to_display_point(context.display_snapshot);
        let label_row = label_display_point.row();
        if !context.visible_display_row_range.contains(&label_row) {
            return;
        }

        let row_index = label_row.minus(context.visible_display_row_range.start) as usize;
        let row_layout = &context.line_layouts[row_index];
        let label_column = label_display_point.column().min(row_layout.len as u32) as usize;
        let label_x = row_layout.x_for_index(label_column)
            + row_layout.alignment_offset(context.text_align, context.content_width)
            - context.scroll_pixel_position.x.into()
            + label.x_offset;
        let label_y =
            ((label_row.as_f64() - context.scroll_position.y) * ScrollPixelOffset::from(context.line_height)).into();
        let label_text_size = (context.editor_font_size * label.scale_factor.max(0.0)).max(px(1.0));
        let origin = context.content_origin + point(label_x, label_y);

        let mut element = div()
            .block_mouse_except_scroll()
            .font(context.editor_font.clone())
            .text_size(label_text_size)
            .text_color(label.text_color)
            .line_height(context.line_height)
            .child(label.text.clone())
            .into_any_element();
        element.prepaint_as_root(origin, AvailableSpace::min_size(), window, cx);

        paint_commands.push(NavigationOverlayPaintCommand::Label(NavigationLabelLayout { element, origin }));
    }

    fn layout_scrollbars(
        &self,
        snapshot: &EditorSnapshot,
        scrollbar_layout_information: &ScrollbarLayoutInformation,
        content_offset: gpui::Point<Pixels>,
        scroll_position: gpui::Point<ScrollOffset>,
        non_visible_cursors: bool,
        right_margin: Pixels,
        editor_width: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<EditorScrollbars> {
        if !self.editor.read(cx).show_horizontal_scrollbar || self.style.scrollbar_width.is_zero() {
            return None;
        }

        // If a drag took place after we started dragging the scrollbar,
        // cancel the scrollbar drag.
        if cx.has_active_drag() {
            self.editor.update(cx, |editor, cx| editor.scroll_manager.reset_scrollbar_state(cx));
        }

        let show_scrollbars = {
            let editor = self.editor.read(cx);
            let is_singleton = editor.buffer_kind(cx) == ItemBufferKind::Singleton;
            let supports_git_diff_markers = is_singleton || editor.allow_git_diff_scrollbar_markers;
            // Git
            (supports_git_diff_markers && ProjectSettings::get_global(cx).git.enabled.diff && snapshot.buffer_snapshot().has_diff_hunks())
            ||
            // Buffer Search Results
            (is_singleton && editor.has_background_highlights(HighlightKey::BufferSearchHighlights))
            ||
            // Selected Text Occurrences
            (is_singleton && editor.has_background_highlights(HighlightKey::SelectedTextHighlight))
            ||
            // Selected Symbol Occurrences
            (is_singleton && (editor.has_background_highlights(HighlightKey::DocumentHighlightRead) || editor.has_background_highlights(HighlightKey::DocumentHighlightWrite)))
            ||
            // Diagnostics
            (is_singleton && snapshot.buffer_snapshot().has_diagnostics())
            ||
            // Cursors out of sight
            non_visible_cursors
            ||
            // Scrollmanager
            editor.scroll_manager.scrollbars_visible()
        };

        // The horizontal scrollbar is usually slightly offset to align nicely with
        // indent guides. However, this offset is not needed if indent guides are
        // disabled for the current editor.
        let content_offset = if self.editor.read(cx).show_indent_guides.is_none_or(|should_show| should_show) {
            content_offset
        } else {
            Default::default()
        };

        Some(EditorScrollbars::new(
            scrollbar_layout_information,
            content_offset,
            scroll_position,
            self.style.scrollbar_width,
            right_margin,
            editor_width,
            show_scrollbars,
            self.editor.read(cx).scroll_manager.active_scrollbar_state(),
            window,
        ))
    }

    fn prepaint_crease_toggles(
        &self,
        crease_toggles: &mut [Option<AnyElement>],
        line_height: Pixels,
        gutter_dimensions: &GutterDimensions,
        scroll_position: gpui::Point<ScrollOffset>,
        start_row: DisplayRow,
        gutter_hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (ix, crease_toggle) in crease_toggles.iter_mut().enumerate() {
            if let Some(crease_toggle) = crease_toggle {
                let available_space = size(AvailableSpace::MinContent, AvailableSpace::Definite(line_height * 0.55));
                let crease_toggle_size = crease_toggle.layout_as_root(available_space, window, cx);

                let display_row = DisplayRow(start_row.0 + ix as u32);
                let position = point(
                    gutter_dimensions.width - gutter_dimensions.right_padding,
                    line_height * (display_row.as_f64() - scroll_position.y) as f32,
                );
                let centering_offset = point(
                    (gutter_dimensions.fold_area_width() - crease_toggle_size.width) / 2.,
                    (line_height - crease_toggle_size.height) / 2.,
                );
                let origin = gutter_hitbox.origin + position + centering_offset;
                crease_toggle.prepaint_as_root(origin, available_space, window, cx);
            }
        }
    }

    fn prepaint_expand_toggles(
        &self,
        expand_toggles: &mut [Option<(AnyElement, gpui::Point<Pixels>)>],
        window: &mut Window,
        cx: &mut App,
    ) {
        for (expand_toggle, origin) in expand_toggles.iter_mut().flatten() {
            let available_space = size(AvailableSpace::MinContent, AvailableSpace::MinContent);
            expand_toggle.layout_as_root(available_space, window, cx);
            expand_toggle.prepaint_as_root(*origin, available_space, window, cx);
        }
    }

    fn prepaint_crease_trailers(
        &self,
        trailers: Vec<Option<AnyElement>>,
        lines: &[LineWithInvisibles],
        line_height: Pixels,
        content_origin: gpui::Point<Pixels>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        scroll_position: gpui::Point<ScrollOffset>,
        start_row: DisplayRow,
        em_width: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<Option<CreaseTrailerLayout>> {
        trailers
            .into_iter()
            .enumerate()
            .map(|(ix, element)| {
                let mut element = element?;
                let available_space = size(AvailableSpace::MinContent, AvailableSpace::Definite(line_height));
                let size = element.layout_as_root(available_space, window, cx);

                let line = &lines[ix];
                let padding = if line.width == Pixels::ZERO { Pixels::ZERO } else { 4. * em_width };
                let position = point(
                    Pixels::from(scroll_pixel_position.x) + line.width + padding,
                    line_height * (DisplayRow(start_row.0 + ix as u32).as_f64() - scroll_position.y) as f32,
                );
                let centering_offset = point(px(0.), (line_height - size.height) / 2.);
                let origin = content_origin + position + centering_offset;
                element.prepaint_as_root(origin, available_space, window, cx);
                Some(CreaseTrailerLayout { element, bounds: Bounds::new(origin, size) })
            })
            .collect()
    }

    // Folds contained in a hunk are ignored apart from shrinking visual size
    // If a fold contains any hunks then that fold line is marked as modified
    fn layout_gutter_diff_hunks(
        &self,
        line_height: Pixels,
        gutter_hitbox: &Hitbox,
        display_rows: Range<DisplayRow>,
        snapshot: &EditorSnapshot,
        scroll_position: gpui::Point<ScrollOffset>,
        window: &mut Window,
        cx: &App,
    ) -> Vec<(DisplayDiffHunk, Option<Hitbox>)> {
        let folded_buffers = self.editor.read(cx).folded_buffers(cx);
        let mut display_hunks = snapshot
            .display_diff_hunks_for_rows(display_rows, folded_buffers)
            .map(|hunk| (hunk, None))
            .collect::<Vec<_>>();
        let git_gutter_setting = ProjectSettings::get_global(cx).git.git_gutter;
        if let GitGutterSetting::TrackedFiles = git_gutter_setting {
            for (hunk, hitbox) in &mut display_hunks {
                if matches!(hunk, DisplayDiffHunk::Unfolded { .. }) {
                    let hunk_bounds =
                        Self::diff_hunk_bounds(scroll_position, line_height, gutter_hitbox.bounds, hunk, snapshot);
                    *hitbox = Some(window.insert_hitbox(hunk_bounds, HitboxBehavior::BlockMouse));
                }
            }
        }

        display_hunks
    }

    fn layout_inline_diagnostics(
        &self,
        line_layouts: &[LineWithInvisibles],
        crease_trailers: &[Option<CreaseTrailerLayout>],
        row_block_types: &HashMap<DisplayRow, bool>,
        content_origin: gpui::Point<Pixels>,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        start_row: DisplayRow,
        end_row: DisplayRow,
        line_height: Pixels,
        em_width: Pixels,
        style: &EditorStyle,
        window: &mut Window,
        cx: &mut App,
    ) -> HashMap<DisplayRow, AnyElement> {
        let max_severity = match self
            .editor
            .read(cx)
            .inline_diagnostics_enabled()
            .then(|| {
                ProjectSettings::get_global(cx)
                    .diagnostics
                    .inline
                    .max_severity
                    .unwrap_or_else(|| self.editor.read(cx).diagnostics_max_severity)
                    .into_lsp()
            })
            .flatten()
        {
            Some(max_severity) => max_severity,
            None => return HashMap::default(),
        };

        let active_diagnostics_group = self.editor.read(cx).active_diagnostic_group_id();

        let diagnostics_by_rows = self.editor.update(cx, |editor, cx| {
            let snapshot = editor.snapshot(window, cx);
            editor
                .inline_diagnostics
                .iter()
                .filter(|(_, diagnostic)| diagnostic.severity <= max_severity)
                .filter(|(_, diagnostic)| match active_diagnostics_group {
                    Some(active_diagnostics_group) => {
                        // Active diagnostics are all shown in the editor already, no need to display them inline
                        diagnostic.group_id != active_diagnostics_group
                    }
                    None => true,
                })
                .map(|(point, diag)| (point.to_display_point(&snapshot), diag.clone()))
                .skip_while(|(point, _)| point.row() < start_row)
                .take_while(|(point, _)| point.row() < end_row)
                .filter(|(point, _)| !row_block_types.contains_key(&point.row()))
                .fold(HashMap::default(), |mut acc, (point, diagnostic)| {
                    acc.entry(point.row()).or_insert_with(Vec::new).push(diagnostic);
                    acc
                })
        });

        if diagnostics_by_rows.is_empty() {
            return HashMap::default();
        }

        let severity_to_color = |sev: &lsp::DiagnosticSeverity| match *sev {
            lsp::DiagnosticSeverity::ERROR => Color::Error,
            lsp::DiagnosticSeverity::WARNING => Color::Warning,
            lsp::DiagnosticSeverity::INFORMATION => Color::Info,
            lsp::DiagnosticSeverity::HINT => Color::Hint,
            _ => Color::Error,
        };

        let padding = ProjectSettings::get_global(cx).diagnostics.inline.padding as f32 * em_width;
        let min_x =
            column_pixels(&self.style, ProjectSettings::get_global(cx).diagnostics.inline.min_column as usize, window);

        let mut elements = HashMap::default();
        for (row, mut diagnostics) in diagnostics_by_rows {
            diagnostics.sort_by_key(|diagnostic| {
                (
                    diagnostic.severity,
                    std::cmp::Reverse(diagnostic.is_primary),
                    diagnostic.start.row,
                    diagnostic.start.column,
                )
            });

            let Some(diagnostic_to_render) =
                diagnostics.iter().find(|diagnostic| diagnostic.is_primary).or_else(|| diagnostics.first())
            else {
                continue;
            };

            let pos_y = content_origin.y + line_height * (row.0 as f64 - scroll_position.y) as f32;

            let window_ix = row.0.saturating_sub(start_row.0) as usize;
            let pos_x = {
                let crease_trailer_layout = &crease_trailers[window_ix];
                let line_layout = &line_layouts[window_ix];

                let line_end = if let Some(crease_trailer) = crease_trailer_layout {
                    crease_trailer.bounds.right()
                } else {
                    Pixels::from(
                        ScrollPixelOffset::from(content_origin.x + line_layout.width) - scroll_pixel_position.x,
                    )
                };

                let padded_line = line_end + padding;
                let min_start =
                    Pixels::from(ScrollPixelOffset::from(content_origin.x + min_x) - scroll_pixel_position.x);

                cmp::max(padded_line, min_start)
            };

            let mut element = h_flex()
                .id(("diagnostic", row.0))
                .h(line_height)
                .w_full()
                .px_1()
                .rounded_xs()
                .bg(severity_to_color(&diagnostic_to_render.severity).color(cx).opacity(0.05))
                .text_color(severity_to_color(&diagnostic_to_render.severity).color(cx))
                .text_sm()
                .font(style.text.font())
                .child(diagnostic_to_render.message.clone())
                .into_any();

            element.prepaint_as_root(point(pos_x, pos_y), AvailableSpace::min_size(), window, cx);

            elements.insert(row, element);
        }

        elements
    }

    fn layout_inline_code_actions(
        &self,
        display_point: DisplayPoint,
        content_origin: gpui::Point<Pixels>,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        line_height: Pixels,
        snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        // Don't show code actions in split diff view
        if self.split_side.is_some() {
            return None;
        }

        if !snapshot.show_code_actions.unwrap_or(EditorSettings::get_global(cx).inline_code_actions) {
            return None;
        }

        let icon_size = ui::IconSize::XSmall;
        let mut button = self.editor.update(cx, |editor, cx| {
            if !editor.has_available_code_actions_for_selection() {
                return None;
            }
            let active = editor
                .context_menu
                .borrow()
                .as_ref()
                .and_then(|menu| {
                    if let crate::CodeContextMenu::CodeActions(CodeActionsMenu { deployed_from, .. }) = menu {
                        deployed_from.as_ref()
                    } else {
                        None
                    }
                })
                .is_some_and(|source| matches!(source, CodeActionSource::Indicator(..)));
            Some(editor.render_inline_code_actions(icon_size, display_point.row(), active, cx))
        })?;

        let buffer_point = display_point.to_point(&snapshot.display_snapshot);
        let new_display_row = snapshot.display_row_for_inline_code_action(buffer_point)?;

        let start_y = content_origin.y
            + (((new_display_row.as_f64() - scroll_position.y) as f32) * line_height)
            + (line_height / 2.0)
            - (icon_size.square(window, cx) / 2.);
        let start_x = (ScrollPixelOffset::from(content_origin.x) - scroll_pixel_position.x
            + ScrollPixelOffset::from(window.rem_size() * 0.1))
        .into();

        let absolute_offset = gpui::point(start_x, start_y);
        button.layout_as_root(gpui::AvailableSpace::min_size(), window, cx);
        button.prepaint_as_root(absolute_offset, gpui::AvailableSpace::min_size(), window, cx);
        Some(button)
    }

    fn layout_blame_entries(
        &self,
        buffer_rows: &[RowInfo],
        em_width: Pixels,
        scroll_position: gpui::Point<ScrollOffset>,
        start_row: DisplayRow,
        line_height: Pixels,
        gutter_hitbox: &Hitbox,
        max_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Vec<AnyElement>> {
        if !self.editor.update(cx, |editor, cx| editor.render_git_blame_gutter(cx)) {
            return None;
        }

        let blame = self.editor.read(cx).blame.clone()?;
        let workspace = self.editor.read(cx).workspace()?;
        let blamed_rows: Vec<_> = blame.update(cx, |blame, cx| blame.blame_for_rows(buffer_rows, cx).collect());

        let width = if let Some(max_width) = max_width {
            AvailableSpace::Definite(max_width)
        } else {
            AvailableSpace::MaxContent
        };
        let start_x = em_width;

        let mut last_used_color: Option<(Hsla, Oid)> = None;
        let blame_renderer = cx.global::<GlobalBlameRenderer>().0.clone();

        let shaped_lines = blamed_rows
            .into_iter()
            .enumerate()
            .flat_map(|(ix, blame_entry)| {
                let (buffer_id, blame_entry) = blame_entry?;
                let mut element = render_blame_entry(
                    ix,
                    &blame,
                    blame_entry,
                    &self.style,
                    &mut last_used_color,
                    self.editor.clone(),
                    workspace.clone(),
                    buffer_id,
                    &*blame_renderer,
                    window,
                    cx,
                )?;

                let start_y = line_height * (DisplayRow(start_row.0 + ix as u32).as_f64() - scroll_position.y) as f32;
                let absolute_offset = gutter_hitbox.origin + point(start_x, start_y);

                element.prepaint_as_root(absolute_offset, size(width, AvailableSpace::MinContent), window, cx);

                Some(element)
            })
            .collect();

        Some(shaped_lines)
    }

    fn layout_indent_guides(
        &self,
        content_origin: gpui::Point<Pixels>,
        text_origin: gpui::Point<Pixels>,
        visible_buffer_range: Range<MultiBufferRow>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        line_height: Pixels,
        snapshot: &DisplaySnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Vec<IndentGuideLayout>> {
        let indent_guides =
            self.editor.update(cx, |editor, cx| editor.indent_guides(visible_buffer_range, snapshot, cx))?;

        let active_indent_guide_indices = self.editor.update(cx, |editor, cx| {
            editor.find_active_indent_guide_indices(&indent_guides, snapshot, window, cx).unwrap_or_default()
        });

        Some(
            indent_guides
                .into_iter()
                .enumerate()
                .filter_map(|(i, indent_guide)| {
                    let single_indent_width = column_pixels(&self.style, indent_guide.tab_size as usize, window);
                    let total_width = single_indent_width * indent_guide.depth as f32;
                    let start_x =
                        Pixels::from(ScrollOffset::from(content_origin.x + total_width) - scroll_pixel_position.x);
                    if start_x >= text_origin.x {
                        let (offset_y, length, display_row_range) = Self::calculate_indent_guide_bounds(
                            indent_guide.start_row..indent_guide.end_row,
                            line_height,
                            snapshot,
                        );

                        let start_y =
                            Pixels::from(ScrollOffset::from(content_origin.y) + offset_y - scroll_pixel_position.y);

                        Some(IndentGuideLayout {
                            origin: point(start_x, start_y),
                            length,
                            single_indent_width,
                            display_row_range,
                            depth: indent_guide.depth,
                            active: active_indent_guide_indices.contains(&i),
                            settings: indent_guide.settings,
                        })
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }

    fn depth_zero_indent_guide_padding_for_row(indent_guides: &[IndentGuideLayout], row: DisplayRow) -> Pixels {
        indent_guides
            .iter()
            .find(|guide| guide.depth == 0 && guide.display_row_range.contains(&row))
            .and_then(|guide| guide.settings.visible_line_width(guide.active).map(|width| px(width as f32 * 2.0)))
            .unwrap_or(px(0.0))
    }

    fn layout_wrap_guides(
        &self,
        em_advance: Pixels,
        scroll_position: gpui::Point<f64>,
        content_origin: gpui::Point<Pixels>,
        hitbox: &Hitbox,
        window: &Window,
        cx: &App,
    ) -> SmallVec<[(Pixels, bool); 2]> {
        let scroll_left = scroll_position.x as f32 * em_advance;
        let content_origin = content_origin.x;
        let horizontal_offset = content_origin - scroll_left;

        self.editor
            .read(cx)
            .wrap_guides(cx)
            .into_iter()
            .flat_map(|(guide, active)| {
                let wrap_position = column_pixels(&self.style, guide, window);
                let wrap_guide_x = wrap_position + horizontal_offset;
                let display_wrap_guide = wrap_guide_x >= content_origin && wrap_guide_x <= hitbox.bounds.right();

                display_wrap_guide.then_some((wrap_guide_x, active))
            })
            .collect()
    }

    fn calculate_indent_guide_bounds(
        row_range: Range<MultiBufferRow>,
        line_height: Pixels,
        snapshot: &DisplaySnapshot,
    ) -> (f64, gpui::Pixels, Range<DisplayRow>) {
        let start_point = Point::new(row_range.start.0, 0);
        let end_point = Point::new(row_range.end.0, 0);

        let mut row_range = start_point.to_display_point(snapshot).row()..end_point.to_display_point(snapshot).row();

        let mut prev_line = start_point;
        prev_line.row = prev_line.row.saturating_sub(1);
        let prev_line = prev_line.to_display_point(snapshot).row();

        let mut cons_line = end_point;
        cons_line.row += 1;
        let cons_line = cons_line.to_display_point(snapshot).row();

        let mut offset_y = row_range.start.as_f64() * f64::from(line_height);
        let mut length = (cons_line.0.saturating_sub(row_range.start.0)) as f32 * line_height;

        // If we are at the end of the buffer, ensure that the indent guide extends to the end of the line.
        if row_range.end == cons_line {
            length += line_height;
        }

        // If there is a block (e.g. diagnostic) in between the start of the indent guide and the line above,
        // we want to extend the indent guide to the start of the block.
        let mut block_height = 0;
        let mut block_offset = 0;
        let mut found_excerpt_header = false;
        for (_, block) in snapshot.blocks_in_range(prev_line..row_range.start) {
            if matches!(block, Block::ExcerptBoundary { .. } | Block::BufferHeader { .. }) {
                found_excerpt_header = true;
                break;
            }
            block_offset += block.height();
            block_height += block.height();
        }
        if !found_excerpt_header {
            offset_y -= block_offset as f64 * f64::from(line_height);
            length += block_height as f32 * line_height;
            row_range = DisplayRow(row_range.start.0.saturating_sub(block_offset))..row_range.end;
        }

        // If there is a block (e.g. diagnostic) at the end of an multibuffer excerpt,
        // we want to ensure that the indent guide stops before the excerpt header.
        let mut block_height = 0;
        let mut found_excerpt_header = false;
        for (_, block) in snapshot.blocks_in_range(row_range.end..cons_line) {
            if matches!(block, Block::ExcerptBoundary { .. } | Block::BufferHeader { .. }) {
                found_excerpt_header = true;
            }
            block_height += block.height();
        }
        if found_excerpt_header {
            length -= block_height as f32 * line_height;
        } else {
            row_range = row_range.start..cons_line;
        }

        (offset_y, length, row_range)
    }

    fn layout_gutter_hover_button(
        &self,
        gutter: &Gutter,
        row: DisplayRow,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if self.split_side == Some(SplitSide::Left) {
            return None;
        }

        self.editor.update(cx, |editor, cx| {
            gutter.layout_item_skipping_folds(
                row,
                |cx, _| editor.render_gutter_hover_button(row, cx).into_any_element(),
                window,
                cx,
            )
        })
    }

    fn layout_run_indicators(
        &self,
        gutter: &Gutter,
        run_indicators: &HashSet<DisplayRow>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        if self.split_side == Some(SplitSide::Left) {
            return Vec::new();
        }

        self.editor.update(cx, |editor, cx| {
            let active_task_indicator_row =
                // TODO: add edit button on the right side of each row in the context menu
                if let Some(crate::CodeContextMenu::CodeActions(CodeActionsMenu {
                    deployed_from,
                    actions,
                    ..
                })) = editor.context_menu.borrow().as_ref()
                {
                    actions
                        .tasks()
                        .map(|tasks| tasks.position.to_display_point(gutter.snapshot).row())
                        .or(match deployed_from {
                            Some(CodeActionSource::Indicator(row)) => Some(*row),
                            _ => None,
                        })
                } else {
                    None
                };

            run_indicators
                .iter()
                .filter_map(|display_row| {
                    let task_status = gutter
                        .row_infos
                        .get((display_row.0.saturating_sub(gutter.range.start.0)) as usize)
                        .and_then(|row_info| Some((row_info.buffer_id?, row_info.buffer_row?)))
                        .and_then(|(buffer_id, buffer_row)| editor.runnable_task_status(buffer_id, buffer_row));

                    gutter.layout_item(
                        *display_row,
                        |cx, _| {
                            editor
                                .render_run_indicator(
                                    &self.style,
                                    Some(*display_row) == active_task_indicator_row,
                                    task_status,
                                    *display_row,
                                    cx,
                                )
                                .into_any_element()
                        },
                        window,
                        cx,
                    )
                })
                .collect_vec()
        })
    }

    fn layout_expand_toggles(
        &self,
        gutter_hitbox: &Hitbox,
        gutter_dimensions: GutterDimensions,
        em_width: Pixels,
        line_height: Pixels,
        scroll_position: gpui::Point<ScrollOffset>,
        start_row: DisplayRow,
        buffer_rows: &[RowInfo],
        window: &Window,
        cx: &App,
    ) -> Vec<Option<(AnyElement, gpui::Point<Pixels>)>> {
        if self.editor.read(cx).disable_expand_excerpt_buttons {
            return vec![];
        }

        let editor_font_size = self.style.text.font_size.to_pixels(window.rem_size()) * 1.2;

        let max_line_number_length =
            self.editor.read(cx).buffer().read(cx).snapshot(cx).widest_line_number().ilog10() + 1;

        let git_gutter_width =
            Self::gutter_strip_width(line_height) + gutter_dimensions.git_blame_entries_width.unwrap_or_default();
        let available_width = gutter_dimensions.left_padding - git_gutter_width;

        buffer_rows
            .iter()
            .enumerate()
            .map(|(ix, row_info)| {
                let ExpandInfo { direction, start_anchor } = row_info.expand_info?;

                let icon_name = match direction {
                    ExpandExcerptDirection::Up => IconName::ExpandUp,
                    ExpandExcerptDirection::Down => IconName::ExpandDown,
                    ExpandExcerptDirection::UpAndDown => IconName::ExpandVertical,
                };

                let editor = self.editor.clone();
                let is_wide = max_line_number_length >= MIN_LINE_NUMBER_DIGITS as u32
                    && row_info.buffer_row.is_some_and(|row| (row + 1).ilog10() + 1 == max_line_number_length)
                    || gutter_dimensions.right_padding == px(0.);

                let width = if is_wide { available_width - px(5.) } else { available_width + em_width - px(5.) };

                let toggle = IconButton::new(("expand", ix), icon_name)
                    .icon_color(Color::Custom(cx.theme().colors().editor_line_number))
                    .icon_size(IconSize::Custom(rems(editor_font_size / window.rem_size())))
                    .width(width)
                    .on_click(move |_, window, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.expand_excerpt(start_anchor, direction, window, cx);
                        });
                    })
                    .tooltip(Tooltip::for_action_title("Expand Excerpt", &crate::actions::ExpandExcerpts::default()))
                    .into_any_element();

                let position = point(
                    git_gutter_width + px(1.),
                    line_height * (DisplayRow(start_row.0 + ix as u32).as_f64() - scroll_position.y) as f32 + px(1.),
                );
                let origin = gutter_hitbox.origin + position;

                Some((toggle, origin))
            })
            .collect()
    }

    fn layout_line_numbers(
        &self,
        gutter: &Gutter<'_>,
        active_rows: &BTreeMap<DisplayRow, LineHighlightSpec>,
        window: &mut Window,
        cx: &App,
    ) -> Arc<HashMap<MultiBufferRow, LineNumberLayout>> {
        let include_line_numbers = gutter.snapshot.show_line_numbers;
        if !include_line_numbers {
            return Arc::default();
        }

        let mut line_number = String::new();
        let segments = gutter.row_infos.iter().enumerate().flat_map(|(ix, row_info)| {
            let display_row = DisplayRow(gutter.range.start.0 + ix as u32);
            line_number.clear();
            let number = row_info.buffer_row? + 1;
            if !gutter.snapshot.number_deleted_lines && row_info.diff_status.is_some_and(|status| status.is_deleted()) {
                return None;
            }

            write!(&mut line_number, "{number}").unwrap();

            let spec = active_rows.get(&display_row);
            let color = LineNumberStyle::new(spec.is_some(), row_info.diff_status).color(cx.theme().colors());

            let shaped_line = self.shape_line_number(SharedString::from(&line_number), color, window);
            let scroll_top = gutter.scroll_position.y * ScrollPixelOffset::from(gutter.line_height);
            let line_origin = gutter.hitbox.origin
                + point(
                    gutter.hitbox.size.width - shaped_line.width - gutter.dimensions.right_padding,
                    ix as f32 * gutter.line_height
                        - Pixels::from(scroll_top % ScrollPixelOffset::from(gutter.line_height)),
                );

            let hitbox = Some(window.insert_hitbox(
                Bounds::new(line_origin, size(shaped_line.width, gutter.line_height)),
                HitboxBehavior::Normal,
            ));

            let segment = LineNumberSegment { shaped_line, hitbox };

            let buffer_row = DisplayPoint::new(display_row, 0).to_point(gutter.snapshot).row;
            let multi_buffer_row = MultiBufferRow(buffer_row);

            Some((multi_buffer_row, segment))
        });

        let mut line_numbers: HashMap<MultiBufferRow, LineNumberLayout> = HashMap::default();
        for (buffer_row, segment) in segments {
            line_numbers
                .entry(buffer_row)
                .or_insert_with(|| LineNumberLayout { segments: Default::default() })
                .segments
                .push(segment);
        }
        Arc::new(line_numbers)
    }

    fn layout_crease_toggles(
        &self,
        rows: Range<DisplayRow>,
        row_infos: &[RowInfo],
        active_rows: &BTreeMap<DisplayRow, LineHighlightSpec>,
        snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<Option<AnyElement>> {
        let include_fold_statuses = snapshot.mode.is_full()
            && snapshot.display_snapshot.companion_snapshot().is_none()
            && self.editor.read(cx).buffer_kind(cx) == ItemBufferKind::Singleton;
        if include_fold_statuses {
            row_infos
                .iter()
                .enumerate()
                .map(|(ix, info)| {
                    if info.expand_info.is_some() {
                        return None;
                    }
                    let row = info.multibuffer_row?;
                    let display_row = DisplayRow(rows.start.0 + ix as u32);
                    let active = active_rows.contains_key(&display_row);

                    snapshot.render_crease_toggle(row, active, self.editor.clone(), window, cx)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    fn layout_crease_trailers(
        &self,
        buffer_rows: impl IntoIterator<Item = RowInfo>,
        snapshot: &EditorSnapshot,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<Option<AnyElement>> {
        buffer_rows
            .into_iter()
            .map(|row_info| {
                if row_info.expand_info.is_some() {
                    return None;
                }
                if let Some(row) = row_info.multibuffer_row {
                    snapshot.render_crease_trailer(row, window, cx)
                } else {
                    None
                }
            })
            .collect()
    }

    fn bg_segments_per_row(
        rows: Range<DisplayRow>,
        selections: &[(PlayerColor, Vec<SelectionLayout>)],
        highlight_ranges: impl IntoIterator<Item = (Range<DisplayPoint>, Hsla)>,
        base_background: Hsla,
    ) -> Vec<Vec<(Range<usize>, Hsla)>> {
        if rows.start >= rows.end {
            return Vec::new();
        }
        if !base_background.is_opaque() {
            // We don't actually know what color is behind this editor.
            return Vec::new();
        }
        let highlight_iter = highlight_ranges.into_iter();
        let selection_iter = selections.iter().flat_map(|(player_color, layouts)| {
            let color = player_color.selection;
            layouts.iter().filter_map(move |selection_layout| {
                if selection_layout.range.start != selection_layout.range.end {
                    Some((selection_layout.range.clone(), color))
                } else {
                    None
                }
            })
        });
        let mut per_row_map = vec![Vec::new(); rows.len()];
        for (range, color) in highlight_iter.chain(selection_iter) {
            let covered_rows = if range.end.column() == 0 {
                cmp::max(range.start.row(), rows.start)..cmp::min(range.end.row(), rows.end)
            } else {
                cmp::max(range.start.row(), rows.start)..cmp::min(range.end.row().next_row(), rows.end)
            };
            for row in covered_rows.iter_rows() {
                let seg_start = if row == range.start.row() { range.start } else { DisplayPoint::new(row, 0) };
                let seg_end = if row == range.end.row() && range.end.column() != 0 {
                    range.end
                } else {
                    DisplayPoint::new(row, u32::MAX)
                };
                let ix = row.minus(rows.start) as usize;
                debug_assert!(row >= rows.start && row < rows.end);
                debug_assert!(ix < per_row_map.len());
                per_row_map[ix].push((seg_start..seg_end, color));
            }
        }
        for row_segments in per_row_map.iter_mut() {
            if row_segments.is_empty() {
                continue;
            }
            *row_segments = merge_highlight_backgrounds(mem::take(row_segments), base_background);
        }
        per_row_map
            .into_iter()
            .map(|row_segments| {
                row_segments
                    .into_iter()
                    .map(|(range, color)| (range.start.column() as usize..range.end.column() as usize, color))
                    .collect()
            })
            .collect()
    }

    fn layout_lines(
        rows: Range<DisplayRow>,
        snapshot: &EditorSnapshot,
        style: &EditorStyle,
        editor_width: Pixels,
        is_row_soft_wrapped: impl Copy + Fn(usize) -> bool,
        bg_segments_per_row: &[Vec<(Range<usize>, Hsla)>],
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<LineWithInvisibles> {
        if rows.start >= rows.end {
            return Vec::new();
        }

        // Show the placeholder when the editor is empty
        if snapshot.is_empty() {
            let font_size = style.text.font_size.to_pixels(window.rem_size());
            let placeholder_color = cx.theme().colors().text_placeholder;
            let placeholder_text = snapshot.placeholder_text();

            let placeholder_lines =
                placeholder_text.as_ref().map_or(Vec::new(), |text| text.split('\n').collect::<Vec<_>>());

            let placeholder_line_count = placeholder_lines.len();

            placeholder_lines
                .into_iter()
                .skip(rows.start.0 as usize)
                .chain(iter::repeat(""))
                .take(cmp::max(rows.len(), placeholder_line_count))
                .map(move |line| {
                    let run = TextRun {
                        len: line.len(),
                        font: style.text.font(),
                        color: placeholder_color,
                        ..Default::default()
                    };
                    let line = window.text_system().shape_line(SharedString::new(line), font_size, &[run], None);

                    LineWithInvisibles {
                        width: line.width,
                        len: line.len,
                        fragments: smallvec![LineFragment::Text(line)],
                        invisibles: Vec::new(),
                        diagnostic_underline_severity_ranges: Vec::new(),
                        point_diagnostics: Vec::new(),
                        font_size,
                    }
                })
                .collect()
        } else {
            let use_tree_sitter =
                !snapshot.semantic_tokens_enabled || snapshot.use_tree_sitter_for_syntax(rows.start, cx);
            let language_aware = LanguageAwareStyling {
                tree_sitter: use_tree_sitter,
                diagnostics: snapshot.display_snapshot.diagnostics_visible(),
            };
            let chunks = snapshot.highlighted_chunks(rows.clone(), language_aware, style);
            LineWithInvisibles::from_chunks(
                chunks,
                style,
                MAX_LINE_LEN,
                rows.len(),
                &snapshot.mode,
                editor_width,
                is_row_soft_wrapped,
                bg_segments_per_row,
                window,
                cx,
            )
        }
    }

    fn prepaint_lines(
        &self,
        start_row: DisplayRow,
        line_layouts: &mut [LineWithInvisibles],
        line_height: Pixels,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        content_origin: gpui::Point<Pixels>,
        content_width: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> SmallVec<[AnyElement; 1]> {
        let mut line_elements = SmallVec::new();
        for (ix, line) in line_layouts.iter_mut().enumerate() {
            let row = start_row + DisplayRow(ix as u32);
            line.prepaint(
                line_height,
                scroll_position,
                scroll_pixel_position,
                row,
                content_origin,
                self.style.text.text_align,
                content_width,
                &mut line_elements,
                window,
                cx,
            );
        }
        line_elements
    }

    fn render_block(
        &self,
        block: &Block,
        available_width: AvailableSpace,
        block_id: BlockId,
        block_row_start: DisplayRow,
        snapshot: &EditorSnapshot,
        text_x: Pixels,
        rows: &Range<DisplayRow>,
        line_layouts: &[LineWithInvisibles],
        editor_margins: &EditorMargins,
        line_height: Pixels,
        em_width: Pixels,
        text_hitbox: &Hitbox,
        editor_width: Pixels,
        scroll_width: &Pixels,
        resized_blocks: &mut HashMap<CustomBlockId, u32>,
        row_block_types: &mut HashMap<DisplayRow, bool>,
        selections: &[Selection<Point>],
        selected_buffer_ids: &[BufferId],
        latest_selection_anchors: &HashMap<BufferId, Anchor>,
        is_row_soft_wrapped: impl Copy + Fn(usize) -> bool,
        sticky_header_excerpt_id: Option<BufferId>,
        indent_guides: &Option<Vec<IndentGuideLayout>>,
        block_resize_offset: &mut i32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(AnyElement, Size<Pixels>, DisplayRow, Pixels)> {
        let mut x_position = None;
        let mut element = match block {
            Block::Custom(custom) => {
                let block_start = custom.start().to_point(snapshot.buffer_snapshot());
                let block_end = custom.end().to_point(snapshot.buffer_snapshot());
                if block.place_near() && snapshot.is_line_folded(MultiBufferRow(block_start.row)) {
                    return None;
                }
                let align_to = block_start.to_display_point(snapshot);
                let x_and_width = |layout: &LineWithInvisibles| {
                    (text_x + layout.x_for_index(align_to.column() as usize), text_x + layout.width)
                };
                let line_ix = align_to.row().0.checked_sub(rows.start.0);
                let custom_block_x_position = if let Some(layout) = line_ix.and_then(|ix| line_layouts.get(ix as usize))
                {
                    x_and_width(layout)
                } else {
                    x_and_width(&layout_line(
                        align_to.row(),
                        snapshot,
                        &self.style,
                        editor_width,
                        is_row_soft_wrapped,
                        window,
                        cx,
                    ))
                };

                let anchor_x = custom_block_x_position.0;
                x_position = Some(custom_block_x_position);

                let selected = selections
                    .binary_search_by(|selection| {
                        if selection.end <= block_start {
                            Ordering::Less
                        } else if selection.start >= block_end {
                            Ordering::Greater
                        } else {
                            Ordering::Equal
                        }
                    })
                    .is_ok();

                div()
                    .size_full()
                    .child(
                        custom.render(&mut BlockContext {
                            window,
                            app: cx,
                            anchor_x,
                            margins: editor_margins,
                            line_height,
                            em_width,
                            block_id,
                            height: custom.height.unwrap_or(1),
                            selected,
                            max_width: text_hitbox.size.width.max(*scroll_width),
                            editor_style: &self.style,
                            indent_guide_padding: indent_guides
                                .as_ref()
                                .map(|guides| Self::depth_zero_indent_guide_padding_for_row(guides, block_row_start))
                                .unwrap_or(px(0.0)),
                        }),
                    )
                    .into_any()
            }

            Block::FoldedBuffer { first_excerpt, height, .. } => {
                let mut result = v_flex().id(block_id).w_full().pr(editor_margins.right);

                if self.should_show_buffer_headers() {
                    let selected = selected_buffer_ids.contains(&first_excerpt.buffer_id());
                    let jump_data = header::header_jump_data(
                        snapshot,
                        block_row_start,
                        *height,
                        first_excerpt,
                        latest_selection_anchors,
                    );
                    result = result.child(header::render_buffer_header(
                        &self.editor,
                        first_excerpt,
                        true,
                        selected,
                        false,
                        jump_data,
                        window,
                        cx,
                    ));
                } else {
                    result = result.child(div().h(FILE_HEADER_HEIGHT as f32 * window.line_height()));
                }

                result.into_any_element()
            }

            Block::ExcerptBoundary { .. } => {
                let color = cx.theme().colors().clone();
                let mut result = v_flex().id(block_id).w_full();

                result = result.child(
                    h_flex()
                        .relative()
                        .child(div().top(line_height / 2.).absolute().w_full().h_px().bg(color.border_variant)),
                );

                result.into_any()
            }

            Block::BufferHeader { excerpt, height } => {
                let mut result = v_flex().id(block_id).w_full();

                if self.should_show_buffer_headers() {
                    let jump_data =
                        header::header_jump_data(snapshot, block_row_start, *height, excerpt, latest_selection_anchors);

                    if sticky_header_excerpt_id != Some(excerpt.buffer_id()) {
                        let selected = selected_buffer_ids.contains(&excerpt.buffer_id());

                        result = result.child(div().pr(editor_margins.right).child(header::render_buffer_header(
                            &self.editor,
                            excerpt,
                            false,
                            selected,
                            false,
                            jump_data,
                            window,
                            cx,
                        )));
                    } else {
                        result = result.child(div().h(FILE_HEADER_HEIGHT as f32 * window.line_height()));
                    }
                } else {
                    result = result.child(div().h(FILE_HEADER_HEIGHT as f32 * window.line_height()));
                }

                result.into_any()
            }

            Block::Spacer { height, .. } => {
                let indent_guide_padding = indent_guides
                    .as_ref()
                    .map(|guides| Self::depth_zero_indent_guide_padding_for_row(guides, block_row_start))
                    .unwrap_or(px(0.0));
                Self::render_spacer_block(block_id, *height, line_height, indent_guide_padding, window, cx)
            }
        };

        // Discover the element's content height, then round up to the nearest multiple of line height.
        let preliminary_size = element.layout_as_root(size(available_width, AvailableSpace::MinContent), window, cx);
        let quantized_height = (preliminary_size.height / line_height).ceil() * line_height;
        let final_size = if preliminary_size.height == quantized_height {
            preliminary_size
        } else {
            element.layout_as_root(size(available_width, quantized_height.into()), window, cx)
        };
        let mut element_height_in_lines = ((final_size.height / line_height).ceil() as u32).max(1);

        let effective_row_start = block_row_start.0 as i32 + *block_resize_offset;
        debug_assert!(effective_row_start >= 0);
        let mut row = DisplayRow(effective_row_start.max(0) as u32);

        let mut x_offset = px(0.);
        let mut is_block = true;

        if let BlockId::Custom(custom_block_id) = block_id
            && block.has_height()
        {
            if block.place_near()
                && let Some((x_target, line_width)) = x_position
            {
                let margin = em_width * 2;
                if line_width + final_size.width + margin < editor_width + editor_margins.gutter.full_width()
                    && !row_block_types.contains_key(&(row - 1))
                    && element_height_in_lines == 1
                {
                    // Render inline at end of line (for diagnostic blocks that fit)
                    x_offset = line_width + margin;
                    row = row - 1;
                    is_block = false;
                    element_height_in_lines = 0;
                    row_block_types.insert(row, is_block);
                } else {
                    let max_offset = editor_width + editor_margins.gutter.full_width() - final_size.width;
                    let min_offset = (x_target + em_width - final_size.width).max(editor_margins.gutter.full_width());
                    x_offset = x_target.min(max_offset).max(min_offset);
                }
            };
            if element_height_in_lines != block.height() {
                *block_resize_offset += element_height_in_lines as i32 - block.height() as i32;
                resized_blocks.insert(custom_block_id, element_height_in_lines);
            }
        }
        for i in 0..element_height_in_lines {
            row_block_types.insert(row + i, is_block);
        }

        Some((element, final_size, row, x_offset))
    }

    /// The spacer pattern period must be an even factor of the line height, so
    /// that two consecutive spacer blocks can render contiguously without an
    /// obvious break in the pattern.
    ///
    /// Two consecutive spacers can appear when the other side has a diff hunk
    /// and a custom block next to each other (e.g. merge conflict buttons).
    fn spacer_pattern_period(line_height: f32, target_height: f32) -> f32 {
        let k_approx = line_height / (2.0 * target_height);
        let k_floor = (k_approx.floor() as u32).max(1);
        let k_ceil = (k_approx.ceil() as u32).max(1);

        let size_floor = line_height / (2 * k_floor) as f32;
        let size_ceil = line_height / (2 * k_ceil) as f32;

        if (size_floor - target_height).abs() <= (size_ceil - target_height).abs() { size_floor } else { size_ceil }
    }

    pub fn render_spacer_block(
        block_id: BlockId,
        block_height: u32,
        line_height: Pixels,
        indent_guide_padding: Pixels,
        window: &mut Window,
        cx: &App,
    ) -> AnyElement {
        let target_size = 16.0;
        let scale = window.scale_factor();
        let pattern_size = Self::spacer_pattern_period(f32::from(line_height) * scale, target_size * scale);
        let color = cx.theme().colors().panel_background;
        let background = pattern_slash(color, 2.0, pattern_size - 2.0);

        div()
            .id(block_id)
            .cursor(CursorStyle::Arrow)
            .w_full()
            .h((block_height as f32) * line_height)
            .flex()
            .flex_row()
            .child(div().flex_shrink_0().w(indent_guide_padding).h_full())
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .relative()
                    .overflow_x_hidden()
                    .child(div().absolute().top_0().bottom_0().right_0().left(-indent_guide_padding).bg(background)),
            )
            .into_any()
    }

    fn render_blocks(
        &self,
        rows: Range<DisplayRow>,
        snapshot: &EditorSnapshot,
        hitbox: &Hitbox,
        text_hitbox: &Hitbox,
        editor_width: Pixels,
        scroll_width: &mut Pixels,
        editor_margins: &EditorMargins,
        em_width: Pixels,
        text_x: Pixels,
        line_height: Pixels,
        line_layouts: &[LineWithInvisibles],
        selections: &[Selection<Point>],
        selected_buffer_ids: &[BufferId],
        latest_selection_anchors: &HashMap<BufferId, Anchor>,
        is_row_soft_wrapped: impl Copy + Fn(usize) -> bool,
        sticky_header_excerpt_id: Option<BufferId>,
        indent_guides: &Option<Vec<IndentGuideLayout>>,
        window: &mut Window,
        cx: &mut App,
    ) -> RenderBlocksOutput {
        let (fixed_blocks, non_fixed_blocks) = snapshot
            .blocks_in_range(rows.clone())
            .partition::<Vec<_>, _>(|(_, block)| block.style() == BlockStyle::Fixed);

        let mut focused_block = self.editor.update(cx, |editor, _| editor.take_focused_block());
        let mut fixed_block_max_width = Pixels::ZERO;
        let mut blocks = Vec::new();
        let mut spacer_blocks = Vec::new();
        let mut resized_blocks = HashMap::default();
        let mut row_block_types = HashMap::default();
        let mut block_resize_offset: i32 = 0;

        for (row, block) in fixed_blocks {
            let block_id = block.id();

            if focused_block.as_ref().is_some_and(|b| b.id == block_id) {
                focused_block = None;
            }

            if let Some((element, element_size, row, x_offset)) = self.render_block(
                block,
                AvailableSpace::MinContent,
                block_id,
                row,
                snapshot,
                text_x,
                &rows,
                line_layouts,
                editor_margins,
                line_height,
                em_width,
                text_hitbox,
                editor_width,
                scroll_width,
                &mut resized_blocks,
                &mut row_block_types,
                selections,
                selected_buffer_ids,
                latest_selection_anchors,
                is_row_soft_wrapped,
                sticky_header_excerpt_id,
                indent_guides,
                &mut block_resize_offset,
                window,
                cx,
            ) {
                fixed_block_max_width = fixed_block_max_width.max(element_size.width + em_width);
                blocks.push(BlockLayout {
                    id: block_id,
                    x_offset,
                    row: Some(row),
                    element,
                    available_space: size(AvailableSpace::MinContent, element_size.height.into()),
                    style: BlockStyle::Fixed,
                    overlaps_gutter: true,
                    is_buffer_header: block.is_buffer_header(),
                });
            }
        }

        for (row, block) in non_fixed_blocks {
            let style = block.style();
            let width = match (style, block.place_near()) {
                (_, true) => AvailableSpace::MinContent,
                (BlockStyle::Sticky, _) => hitbox.size.width.into(),
                (BlockStyle::Flex, _) => hitbox
                    .size
                    .width
                    .max(fixed_block_max_width)
                    .max(editor_margins.gutter.width + *scroll_width + editor_margins.extended_right)
                    .into(),
                (BlockStyle::Spacer, _) => hitbox
                    .size
                    .width
                    .max(fixed_block_max_width)
                    .max(*scroll_width + editor_margins.extended_right)
                    .into(),
                (BlockStyle::Fixed, _) => unreachable!(),
            };
            let block_id = block.id();

            if focused_block.as_ref().is_some_and(|b| b.id == block_id) {
                focused_block = None;
            }

            if let Some((element, element_size, row, x_offset)) = self.render_block(
                block,
                width,
                block_id,
                row,
                snapshot,
                text_x,
                &rows,
                line_layouts,
                editor_margins,
                line_height,
                em_width,
                text_hitbox,
                editor_width,
                scroll_width,
                &mut resized_blocks,
                &mut row_block_types,
                selections,
                selected_buffer_ids,
                latest_selection_anchors,
                is_row_soft_wrapped,
                sticky_header_excerpt_id,
                indent_guides,
                &mut block_resize_offset,
                window,
                cx,
            ) {
                let layout = BlockLayout {
                    id: block_id,
                    x_offset,
                    row: Some(row),
                    element,
                    available_space: size(width, element_size.height.into()),
                    style,
                    overlaps_gutter: !block.place_near() && style != BlockStyle::Spacer,
                    is_buffer_header: block.is_buffer_header(),
                };
                if style == BlockStyle::Spacer {
                    spacer_blocks.push(layout);
                } else {
                    blocks.push(layout);
                }
            }
        }

        if let Some(focused_block) = focused_block
            && let Some(focus_handle) = focused_block.focus_handle.upgrade()
            && focus_handle.is_focused(window)
            && let Some(block) = snapshot.block_for_id(focused_block.id)
        {
            let style = block.style();
            let width = match style {
                BlockStyle::Fixed => AvailableSpace::MinContent,
                BlockStyle::Flex => AvailableSpace::Definite(
                    hitbox
                        .size
                        .width
                        .max(fixed_block_max_width)
                        .max(editor_margins.gutter.width + *scroll_width + editor_margins.extended_right),
                ),
                BlockStyle::Spacer => AvailableSpace::Definite(
                    hitbox.size.width.max(fixed_block_max_width).max(*scroll_width + editor_margins.extended_right),
                ),
                BlockStyle::Sticky => AvailableSpace::Definite(hitbox.size.width),
            };

            if let Some((element, element_size, _, x_offset)) = self.render_block(
                &block,
                width,
                focused_block.id,
                rows.end,
                snapshot,
                text_x,
                &rows,
                line_layouts,
                editor_margins,
                line_height,
                em_width,
                text_hitbox,
                editor_width,
                scroll_width,
                &mut resized_blocks,
                &mut row_block_types,
                selections,
                selected_buffer_ids,
                latest_selection_anchors,
                is_row_soft_wrapped,
                sticky_header_excerpt_id,
                indent_guides,
                &mut block_resize_offset,
                window,
                cx,
            ) {
                blocks.push(BlockLayout {
                    id: block.id(),
                    x_offset,
                    row: None,
                    element,
                    available_space: size(width, element_size.height.into()),
                    style,
                    overlaps_gutter: true,
                    is_buffer_header: block.is_buffer_header(),
                });
            }
        }

        if resized_blocks.is_empty() {
            *scroll_width = (*scroll_width).max(fixed_block_max_width - editor_margins.gutter.width);
        }

        RenderBlocksOutput {
            non_spacer_blocks: blocks,
            spacer_blocks,
            row_block_types,
            resized_blocks: (!resized_blocks.is_empty()).then_some(resized_blocks),
        }
    }

    fn layout_blocks(
        &self,
        blocks: &mut Vec<BlockLayout>,
        hitbox: &Hitbox,
        gutter_hitbox: &Hitbox,
        line_height: Pixels,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        editor_margins: &EditorMargins,
        window: &mut Window,
        cx: &mut App,
    ) {
        for block in blocks {
            let mut origin = if let Some(row) = block.row {
                hitbox.origin
                    + point(
                        block.x_offset,
                        Pixels::from((row.as_f64() - scroll_position.y) * ScrollPixelOffset::from(line_height)),
                    )
            } else {
                // Position the block outside the visible area
                hitbox.origin + point(Pixels::ZERO, hitbox.size.height)
            };

            if block.style == BlockStyle::Spacer {
                origin += point(gutter_hitbox.size.width + editor_margins.gutter.margin, Pixels::ZERO);
            }

            if !matches!(block.style, BlockStyle::Sticky) {
                origin += point(Pixels::from(-scroll_pixel_position.x), Pixels::ZERO);
            }

            let focus_handle = block.element.prepaint_as_root(origin, block.available_space, window, cx);

            if let Some(focus_handle) = focus_handle {
                self.editor.update(cx, |editor, _cx| {
                    editor.set_focused_block(FocusedBlock { id: block.id, focus_handle: focus_handle.downgrade() });
                });
            }
        }
    }

    fn layout_cursor_popovers(
        &self,
        line_height: Pixels,
        text_hitbox: &Hitbox,
        content_origin: gpui::Point<Pixels>,
        right_margin: Pixels,
        start_row: DisplayRow,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        line_layouts: &[LineWithInvisibles],
        cursor: DisplayPoint,
        _cursor_point: Point,
        _style: &EditorStyle,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<ContextMenuLayout> {
        let mut min_menu_height = Pixels::ZERO;
        let mut max_menu_height = Pixels::ZERO;
        let height_above_menu = Pixels::ZERO;
        let height_below_menu = Pixels::ZERO;
        let mut context_menu_visible = false;
        let context_menu_placement;

        {
            let editor = self.editor.read(cx);
            if editor.context_menu_visible()
                && let Some(crate::ContextMenuOrigin::Cursor) = editor.context_menu_origin()
            {
                let (min_height_in_lines, max_height_in_lines) = editor
                    .context_menu_options
                    .as_ref()
                    .map_or((3, 12), |options| (options.min_entries_visible, options.max_entries_visible));

                min_menu_height += line_height * min_height_in_lines as f32 + POPOVER_Y_PADDING;
                max_menu_height += line_height * max_height_in_lines as f32 + POPOVER_Y_PADDING;
                context_menu_visible = true;
            }
            context_menu_placement = editor.context_menu_options.as_ref().and_then(|options| options.placement.clone());
        }

        let visible = context_menu_visible;
        if !visible {
            return None;
        }

        let cursor_row_layout = &line_layouts[cursor.row().minus(start_row) as usize];
        let target_position = content_origin
            + gpui::Point {
                x: cmp::max(
                    px(0.),
                    Pixels::from(
                        ScrollPixelOffset::from(cursor_row_layout.x_for_index(cursor.column() as usize))
                            - scroll_pixel_position.x,
                    ),
                ),
                y: cmp::max(
                    px(0.),
                    Pixels::from(
                        cursor.row().next_row().as_f64() * ScrollPixelOffset::from(line_height)
                            - scroll_pixel_position.y,
                    ),
                ),
            };

        let viewport_bounds = Bounds::new(Default::default(), window.viewport_size())
            .extend(Edges { right: -right_margin - MENU_GAP, ..Default::default() });

        let min_height = height_above_menu + min_menu_height + height_below_menu;
        let max_height = height_above_menu + max_menu_height + height_below_menu;
        let (laid_out_popovers, y_flipped) = self.layout_popovers_above_or_below_line(
            target_position,
            line_height,
            min_height,
            max_height,
            context_menu_placement,
            text_hitbox,
            viewport_bounds,
            window,
            cx,
            |height, max_width_for_stable_x, y_flipped, window, cx| {
                // First layout the menu to get its size - others can be at least this wide.
                let context_menu = if context_menu_visible {
                    let menu_height = if y_flipped { height - height_below_menu } else { height - height_above_menu };
                    let mut element = self
                        .render_context_menu(line_height, menu_height, window, cx)
                        .expect("Visible context menu should always render.");
                    let size = element.layout_as_root(AvailableSpace::min_size(), window, cx);
                    Some((CursorPopoverType::CodeContextMenu, element, size))
                } else {
                    None
                };
                let _min_width = context_menu.as_ref().map_or(px(0.), |(_, _, size)| size.width);
                let _max_width =
                    max_width_for_stable_x.max(context_menu.as_ref().map_or(px(0.), |(_, _, size)| size.width));

                context_menu.into_iter().collect::<Vec<_>>()
            },
        )?;

        let (menu_ix, (_, menu_bounds)) =
            laid_out_popovers.iter().find_position(|(x, _)| matches!(x, CursorPopoverType::CodeContextMenu))?;
        let last_ix = laid_out_popovers.len() - 1;
        let menu_is_last = menu_ix == last_ix;
        let first_popover_bounds = laid_out_popovers[0].1;
        let last_popover_bounds = laid_out_popovers[last_ix].1;

        // Bounds to layout the aside around. When y_flipped, the aside goes either above or to the
        // right, and otherwise it goes below or to the right.
        let mut target_bounds = Bounds::from_corners(first_popover_bounds.origin, last_popover_bounds.bottom_right());
        target_bounds.size.width = menu_bounds.size.width;

        // Like `target_bounds`, but with the max height it could occupy. Choosing an aside position
        // based on this is preferred for layout stability.
        let mut max_target_bounds = target_bounds;
        max_target_bounds.size.height = max_height;
        if y_flipped {
            max_target_bounds.origin.y -= max_height - target_bounds.size.height;
        }

        // Add spacing around `target_bounds` and `max_target_bounds`.
        let mut extend_amount = Edges::all(MENU_GAP);
        if y_flipped {
            extend_amount.bottom = line_height;
        } else {
            extend_amount.top = line_height;
        }
        let target_bounds = target_bounds.extend(extend_amount);
        let max_target_bounds = max_target_bounds.extend(extend_amount);

        let must_place_above_or_below = if y_flipped && !menu_is_last && menu_bounds.size.height < max_menu_height {
            laid_out_popovers[menu_ix + 1..]
                .iter()
                .any(|(_, popover_bounds)| popover_bounds.size.width > menu_bounds.size.width)
        } else {
            false
        };

        let aside_bounds = self.layout_context_menu_aside(
            y_flipped,
            *menu_bounds,
            target_bounds,
            max_target_bounds,
            max_menu_height,
            must_place_above_or_below,
            text_hitbox,
            viewport_bounds,
            window,
            cx,
        );

        if let Some(menu_bounds) = laid_out_popovers.iter().find_map(|(popover_type, bounds)| {
            if matches!(popover_type, CursorPopoverType::CodeContextMenu) { Some(*bounds) } else { None }
        }) {
            let bounds =
                if let Some(aside_bounds) = aside_bounds { menu_bounds.union(&aside_bounds) } else { menu_bounds };
            return Some(ContextMenuLayout { y_flipped, bounds });
        }

        None
    }

    fn layout_gutter_menu(
        &self,
        line_height: Pixels,
        text_hitbox: &Hitbox,
        content_origin: gpui::Point<Pixels>,
        right_margin: Pixels,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        gutter_overshoot: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.read(cx);
        if !editor.context_menu_visible() {
            return;
        }
        let Some(crate::ContextMenuOrigin::GutterIndicator(gutter_row)) = editor.context_menu_origin() else {
            return;
        };
        // Context menu was spawned via a click on a gutter. Ensure it's a bit closer to the
        // indicator than just a plain first column of the text field.
        let target_position = content_origin
            + gpui::Point {
                x: -gutter_overshoot,
                y: Pixels::from(
                    gutter_row.next_row().as_f64() * ScrollPixelOffset::from(line_height) - scroll_pixel_position.y,
                ),
            };

        let (min_height_in_lines, max_height_in_lines) = editor
            .context_menu_options
            .as_ref()
            .map_or((3, 12), |options| (options.min_entries_visible, options.max_entries_visible));

        let min_height = line_height * min_height_in_lines as f32 + POPOVER_Y_PADDING;
        let max_height = line_height * max_height_in_lines as f32 + POPOVER_Y_PADDING;
        let viewport_bounds = Bounds::new(Default::default(), window.viewport_size())
            .extend(Edges { right: -right_margin - MENU_GAP, ..Default::default() });
        self.layout_popovers_above_or_below_line(
            target_position,
            line_height,
            min_height,
            max_height,
            editor.context_menu_options.as_ref().and_then(|options| options.placement.clone()),
            text_hitbox,
            viewport_bounds,
            window,
            cx,
            move |height, _max_width_for_stable_x, _, window, cx| {
                let mut element = self
                    .render_context_menu(line_height, height, window, cx)
                    .expect("Visible context menu should always render.");
                let size = element.layout_as_root(AvailableSpace::min_size(), window, cx);
                vec![(CursorPopoverType::CodeContextMenu, element, size)]
            },
        );
    }

    fn layout_popovers_above_or_below_line(
        &self,
        target_position: gpui::Point<Pixels>,
        line_height: Pixels,
        min_height: Pixels,
        max_height: Pixels,
        placement: Option<ContextMenuPlacement>,
        text_hitbox: &Hitbox,
        viewport_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
        make_sized_popovers: impl FnOnce(
            Pixels,
            Pixels,
            bool,
            &mut Window,
            &mut App,
        ) -> Vec<(CursorPopoverType, AnyElement, Size<Pixels>)>,
    ) -> Option<(Vec<(CursorPopoverType, Bounds<Pixels>)>, bool)> {
        let text_style = TextStyleRefinement {
            line_height: Some(DefiniteLength::Fraction(BufferLineHeight::Comfortable.value())),
            ..Default::default()
        };
        window.with_text_style(Some(text_style), |window| {
            // If the max height won't fit below and there is more space above, put it above the line.
            let bottom_y_when_flipped = target_position.y - line_height;
            let available_above = bottom_y_when_flipped - text_hitbox.top();
            let available_below = text_hitbox.bottom() - target_position.y;
            let y_overflows_below = max_height > available_below;
            let mut y_flipped = match placement {
                Some(ContextMenuPlacement::Above) => true,
                Some(ContextMenuPlacement::Below) => false,
                None => y_overflows_below && available_above > available_below,
            };
            let mut height = cmp::min(max_height, if y_flipped { available_above } else { available_below });

            // If the min height doesn't fit within text bounds, instead fit within the window.
            if height < min_height {
                let available_above = bottom_y_when_flipped;
                let available_below = viewport_bounds.bottom() - target_position.y;
                let (y_flipped_override, height_override) = match placement {
                    Some(ContextMenuPlacement::Above) => (true, cmp::min(available_above, min_height)),
                    Some(ContextMenuPlacement::Below) => (false, cmp::min(available_below, min_height)),
                    None => {
                        if available_below > min_height {
                            (false, min_height)
                        } else if available_above > min_height {
                            (true, min_height)
                        } else if available_above > available_below {
                            (true, available_above)
                        } else {
                            (false, available_below)
                        }
                    }
                };
                y_flipped = y_flipped_override;
                height = height_override;
            }

            let max_width_for_stable_x = viewport_bounds.right() - target_position.x;

            // TODO: Use viewport_bounds.width as a max width so that it doesn't get clipped on the left
            // for very narrow windows.
            let popovers = make_sized_popovers(height, max_width_for_stable_x, y_flipped, window, cx);
            if popovers.is_empty() {
                return None;
            }

            let max_width = popovers.iter().map(|(_, _, size)| size.width).max().unwrap_or_default();

            let mut current_position = gpui::Point {
                // Snap the right edge of the list to the right edge of the window if its horizontal bounds
                // overflow. Include space for the scrollbar.
                x: target_position.x.min((viewport_bounds.right() - max_width).max(Pixels::ZERO)),
                y: if y_flipped { bottom_y_when_flipped } else { target_position.y },
            };

            let mut laid_out_popovers = popovers
                .into_iter()
                .map(|(popover_type, element, size)| {
                    if y_flipped {
                        current_position.y -= size.height;
                    }
                    let position = current_position;
                    window.defer_draw(element, current_position, 1, None);
                    if !y_flipped {
                        current_position.y += size.height + MENU_GAP;
                    } else {
                        current_position.y -= MENU_GAP;
                    }
                    (popover_type, Bounds::new(position, size))
                })
                .collect::<Vec<_>>();

            if y_flipped {
                laid_out_popovers.reverse();
            }

            Some((laid_out_popovers, y_flipped))
        })
    }

    fn layout_context_menu_aside(
        &self,
        y_flipped: bool,
        menu_bounds: Bounds<Pixels>,
        target_bounds: Bounds<Pixels>,
        max_target_bounds: Bounds<Pixels>,
        max_height: Pixels,
        must_place_above_or_below: bool,
        text_hitbox: &Hitbox,
        viewport_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let available_within_viewport = target_bounds.space_within(&viewport_bounds);
        let positioned_aside = if available_within_viewport.right >= MENU_ASIDE_MIN_WIDTH && !must_place_above_or_below
        {
            let max_width = cmp::min(available_within_viewport.right - px(1.), MENU_ASIDE_MAX_WIDTH);
            let mut aside =
                self.render_context_menu_aside(size(max_width, max_height - POPOVER_Y_PADDING), window, cx)?;
            let size = aside.layout_as_root(AvailableSpace::min_size(), window, cx);
            let right_position = point(target_bounds.right(), menu_bounds.origin.y);
            Some((aside, right_position, size))
        } else {
            let max_size = size(
                // TODO(mgsloan): Once the menu is bounded by viewport width the bound on viewport
                // won't be needed here.
                cmp::min(cmp::max(menu_bounds.size.width - px(2.), MENU_ASIDE_MIN_WIDTH), viewport_bounds.right()),
                cmp::min(max_height, cmp::max(available_within_viewport.top, available_within_viewport.bottom))
                    - POPOVER_Y_PADDING,
            );
            let mut aside = self.render_context_menu_aside(max_size, window, cx)?;
            let actual_size = aside.layout_as_root(AvailableSpace::min_size(), window, cx);

            let top_position = point(menu_bounds.origin.x, target_bounds.top() - actual_size.height);
            let bottom_position = point(menu_bounds.origin.x, target_bounds.bottom());

            let fit_within = |available: Edges<Pixels>, wanted: Size<Pixels>| {
                // Prefer to fit on the same side of the line as the menu, then on the other side of
                // the line.
                if !y_flipped && wanted.height < available.bottom {
                    Some(bottom_position)
                } else if wanted.height < available.top {
                    Some(top_position)
                } else if y_flipped && wanted.height < available.bottom {
                    Some(bottom_position)
                } else {
                    None
                }
            };

            // Prefer choosing a direction using max sizes rather than actual size for stability.
            let available_within_text = max_target_bounds.space_within(&text_hitbox.bounds);
            let wanted = size(MENU_ASIDE_MAX_WIDTH, max_height);
            let aside_position = fit_within(available_within_text, wanted)
                // Fallback: fit max size in window.
                .or_else(|| fit_within(max_target_bounds.space_within(&viewport_bounds), wanted))
                // Fallback: fit actual size in window.
                .or_else(|| fit_within(available_within_viewport, actual_size));

            aside_position.map(|position| (aside, position, actual_size))
        };

        // Skip drawing if it doesn't fit anywhere.
        if let Some((aside, position, size)) = positioned_aside {
            let aside_bounds = Bounds::new(position, size);
            window.defer_draw(aside, position, 2, None);
            return Some(aside_bounds);
        }

        None
    }

    fn render_context_menu(
        &self,
        line_height: Pixels,
        height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let max_height_in_lines = ((height - POPOVER_Y_PADDING) / line_height).floor() as u32;
        self.editor.update(cx, |editor, cx| editor.render_context_menu(max_height_in_lines, window, cx))
    }

    fn render_context_menu_aside(
        &self,
        max_size: Size<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if max_size.width < px(100.) || max_size.height < px(12.) {
            None
        } else {
            self.editor.update(cx, |editor, cx| editor.render_context_menu_aside(max_size, window, cx))
        }
    }

    fn layout_hover_popovers(
        &self,
        snapshot: &EditorSnapshot,
        hitbox: &Hitbox,
        visible_display_row_range: Range<DisplayRow>,
        content_origin: gpui::Point<Pixels>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        line_layouts: &[LineWithInvisibles],
        line_height: Pixels,
        em_width: Pixels,
        context_menu_layout: Option<ContextMenuLayout>,
        window: &mut Window,
        cx: &mut App,
    ) {
        struct MeasuredHoverPopover {
            element: AnyElement,
            size: Size<Pixels>,
            horizontal_offset: Pixels,
        }

        let max_size = size(
            (120. * em_width) // Default size
                .min(hitbox.size.width / 2.) // Shrink to half of the editor width
                .max(MIN_POPOVER_CHARACTER_WIDTH * em_width), // Apply minimum width of 20 characters
            (16. * line_height) // Default size
                .min(hitbox.size.height / 2.) // Shrink to half of the editor height
                .max(MIN_POPOVER_LINE_HEIGHT * line_height), // Apply minimum height of 4 lines
        );

        // Don't show hover popovers when context menu is open to avoid overlap
        let has_context_menu = self.editor.read(cx).mouse_context_menu.is_some();
        if has_context_menu {
            return;
        }

        let hover_popovers = self.editor.update(cx, |editor, cx| {
            editor.hover_state.render(
                snapshot,
                visible_display_row_range.clone(),
                max_size,
                &editor.text_layout_details(window, cx),
                window,
                cx,
            )
        });
        let Some((popover_position, hover_popovers)) = hover_popovers else {
            return;
        };

        // This is safe because we check on layout whether the required row is available
        let hovered_row_layout = &line_layouts[popover_position.row().minus(visible_display_row_range.start) as usize];

        // Compute Hovered Point
        let x =
            hovered_row_layout.x_for_index(popover_position.column() as usize) - Pixels::from(scroll_pixel_position.x);
        let y = Pixels::from(
            popover_position.row().as_f64() * ScrollPixelOffset::from(line_height) - scroll_pixel_position.y,
        );
        let hovered_point = content_origin + point(x, y);

        let mut overall_height = Pixels::ZERO;

        let measured_hover_popovers = hover_popovers
            .into_iter()
            .with_position()
            .map(|(position, mut hover_popover)| {
                let size = hover_popover.layout_as_root(AvailableSpace::min_size(), window, cx);
                let horizontal_offset =
                    (hitbox.top_right().x - POPOVER_RIGHT_OFFSET - (hovered_point.x + size.width)).min(Pixels::ZERO);
                match position {
                    itertools::Position::Middle | itertools::Position::Last => overall_height += HOVER_POPOVER_GAP,
                    _ => {}
                }
                overall_height += size.height;
                MeasuredHoverPopover { element: hover_popover, size, horizontal_offset }
            })
            .collect::<Vec<_>>();

        fn draw_occluder(width: Pixels, origin: gpui::Point<Pixels>, window: &mut Window, cx: &mut App) {
            let mut occlusion =
                div().size_full().occlude().on_mouse_move(|_, _, cx| cx.stop_propagation()).into_any_element();
            occlusion.layout_as_root(size(width, HOVER_POPOVER_GAP).into(), window, cx);
            window.defer_draw(occlusion, origin, 2, None);
        }

        fn place_popovers_above(
            hovered_point: gpui::Point<Pixels>,
            measured_hover_popovers: Vec<MeasuredHoverPopover>,
            window: &mut Window,
            cx: &mut App,
        ) {
            let mut current_y = hovered_point.y;
            for (position, popover) in measured_hover_popovers.into_iter().with_position() {
                let size = popover.size;
                let popover_origin = point(hovered_point.x + popover.horizontal_offset, current_y - size.height);

                window.defer_draw(popover.element, popover_origin, 2, None);
                if position != itertools::Position::Last {
                    let origin = point(popover_origin.x, popover_origin.y - HOVER_POPOVER_GAP);
                    draw_occluder(size.width, origin, window, cx);
                }

                current_y = popover_origin.y - HOVER_POPOVER_GAP;
            }
        }

        fn place_popovers_below(
            hovered_point: gpui::Point<Pixels>,
            measured_hover_popovers: Vec<MeasuredHoverPopover>,
            line_height: Pixels,
            window: &mut Window,
            cx: &mut App,
        ) {
            let mut current_y = hovered_point.y + line_height;
            for (position, popover) in measured_hover_popovers.into_iter().with_position() {
                let size = popover.size;
                let popover_origin = point(hovered_point.x + popover.horizontal_offset, current_y);

                window.defer_draw(popover.element, popover_origin, 2, None);
                if position != itertools::Position::Last {
                    let origin = point(popover_origin.x, popover_origin.y + size.height);
                    draw_occluder(size.width, origin, window, cx);
                }

                current_y = popover_origin.y + size.height + HOVER_POPOVER_GAP;
            }
        }

        let intersects_menu = |bounds: Bounds<Pixels>| -> bool {
            context_menu_layout.as_ref().is_some_and(|menu| bounds.intersects(&menu.bounds))
        };

        let can_place_above = {
            let mut current_y = hovered_point.y;
            measured_hover_popovers.iter().all(|popover| {
                let size = popover.size;
                let popover_origin = point(hovered_point.x + popover.horizontal_offset, current_y - size.height);
                let bounds = Bounds::new(popover_origin, size);
                current_y = popover_origin.y - HOVER_POPOVER_GAP;
                bounds.is_contained_within(hitbox) && !intersects_menu(bounds)
            })
        };

        let can_place_below = || {
            let mut current_y = hovered_point.y + line_height;
            measured_hover_popovers.iter().all(|popover| {
                let size = popover.size;
                let popover_origin = point(hovered_point.x + popover.horizontal_offset, current_y);
                let bounds = Bounds::new(popover_origin, size);
                current_y = popover_origin.y + size.height + HOVER_POPOVER_GAP;
                bounds.is_contained_within(hitbox) && !intersects_menu(bounds)
            })
        };

        if can_place_above {
            // try placing above hovered point
            place_popovers_above(hovered_point, measured_hover_popovers, window, cx);
        } else if can_place_below() {
            // try placing below hovered point
            place_popovers_below(hovered_point, measured_hover_popovers, line_height, window, cx);
        } else {
            // try to place popovers around the context menu
            let origin_surrounding_menu = context_menu_layout.as_ref().and_then(|menu| {
                let total_width = measured_hover_popovers.iter().map(|p| p.size.width).max().unwrap_or(Pixels::ZERO);
                let y_for_horizontal_positioning =
                    if menu.y_flipped { menu.bounds.bottom() - overall_height } else { menu.bounds.top() };
                let possible_origins = [
                    // left of context menu
                    point(menu.bounds.left() - total_width - HOVER_POPOVER_GAP, y_for_horizontal_positioning),
                    // right of context menu
                    point(menu.bounds.right() + HOVER_POPOVER_GAP, y_for_horizontal_positioning),
                    // top of context menu
                    point(menu.bounds.left(), menu.bounds.top() - overall_height - HOVER_POPOVER_GAP),
                    // bottom of context menu
                    point(menu.bounds.left(), menu.bounds.bottom() + HOVER_POPOVER_GAP),
                ];
                possible_origins
                    .into_iter()
                    .find(|&origin| Bounds::new(origin, size(total_width, overall_height)).is_contained_within(hitbox))
            });
            if let Some(origin) = origin_surrounding_menu {
                let mut current_y = origin.y;
                for (position, popover) in measured_hover_popovers.into_iter().with_position() {
                    let size = popover.size;
                    let popover_origin = point(origin.x, current_y);

                    window.defer_draw(popover.element, popover_origin, 2, None);
                    if position != itertools::Position::Last {
                        let origin = point(popover_origin.x, popover_origin.y + size.height);
                        draw_occluder(size.width, origin, window, cx);
                    }

                    current_y = popover_origin.y + size.height + HOVER_POPOVER_GAP;
                }
            } else {
                // fallback to existing above/below cursor logic
                // this might overlap menu or overflow in rare case
                if can_place_above {
                    place_popovers_above(hovered_point, measured_hover_popovers, window, cx);
                } else {
                    place_popovers_below(hovered_point, measured_hover_popovers, line_height, window, cx);
                }
            }
        }
    }

    fn layout_word_diff_highlights(
        display_hunks: &[(DisplayDiffHunk, Option<Hitbox>)],
        row_infos: &[RowInfo],
        start_row: DisplayRow,
        snapshot: &EditorSnapshot,
        highlighted_ranges: &mut Vec<(Range<DisplayPoint>, Hsla)>,
        cx: &App,
    ) {
        let colors = cx.theme().colors();

        let visible_start = DisplayPoint::new(start_row, 0).to_offset(&snapshot.display_snapshot, Bias::Left);
        let visible_end = DisplayPoint::new(DisplayRow(start_row.0 + row_infos.len() as u32), 0)
            .to_offset(&snapshot.display_snapshot, Bias::Right);

        // Gather the word diffs that intersect the viewport. A hunk stores the
        // word diffs for its entire range, so without this filter a large hunk
        // that is only partially scrolled into view would cost work
        // proportional to its whole size every frame.
        let mut visible_word_diffs: Vec<&Range<MultiBufferOffset>> = display_hunks
            .iter()
            .filter_map(|(hunk, _)| match hunk {
                DisplayDiffHunk::Unfolded { word_diffs, status, .. } if status.is_modified() => Some(word_diffs),
                _ => None,
            })
            .flatten()
            .filter(|word_diff| word_diff.start < visible_end && word_diff.end > visible_start)
            .collect();

        // The converter walks each display-map layer with a forward-only cursor,
        // so it must receive ranges in non-decreasing order. Word diffs are
        // disjoint, so sorting by offset yields a monotonic sequence.
        visible_word_diffs.sort_unstable_by_key(|word_diff| (word_diff.start, word_diff.end));

        let mut converter = snapshot.display_snapshot.display_point_converter();
        for word_diff in visible_word_diffs {
            for range in converter.map(word_diff.start..word_diff.end) {
                let start_row_offset = range.start.row().0.saturating_sub(start_row.0) as usize;

                let Some(diff_status) = row_infos.get(start_row_offset).and_then(|row_info| row_info.diff_status)
                else {
                    continue;
                };

                let background_color = match diff_status.kind {
                    DiffHunkStatusKind::Added => colors.version_control_word_added,
                    DiffHunkStatusKind::Deleted => colors.version_control_word_deleted,
                    DiffHunkStatusKind::Modified => {
                        debug_panic!("modified diff status for row info");
                        continue;
                    }
                };

                highlighted_ranges.push((range, background_color));
            }
        }
    }

    fn layout_diff_hunk_controls(
        &self,
        row_range: Range<DisplayRow>,
        row_infos: &[RowInfo],
        text_hitbox: &Hitbox,
        newest_cursor_row: Option<DisplayRow>,
        line_height: Pixels,
        right_margin: Pixels,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        sticky_header_height: Pixels,
        display_hunks: &[(DisplayDiffHunk, Option<Hitbox>)],
        highlighted_rows: &BTreeMap<DisplayRow, LineHighlight>,
        editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut App,
    ) -> (Vec<AnyElement>, Vec<(DisplayRow, Bounds<Pixels>)>) {
        let diff_hunk_renderer = editor.read(cx).diff_hunk_renderer();
        let hovered_diff_hunk_row = editor.read(cx).hovered_diff_hunk_row;
        let sticky_top = text_hitbox.bounds.top() + sticky_header_height;

        let mut controls = vec![];
        let mut control_bounds = vec![];

        let active_rows = [hovered_diff_hunk_row, newest_cursor_row];

        for (hunk, _) in display_hunks {
            if let DisplayDiffHunk::Unfolded {
                display_row_range, multi_buffer_range, status, is_created_file, ..
            } = &hunk
            {
                if display_row_range.start >= row_range.end {
                    // hunk is fully below the viewport
                    continue;
                }
                if display_row_range.end <= row_range.start {
                    // hunk is fully above the viewport
                    continue;
                }
                let row_ix = display_row_range.start.0.saturating_sub(row_range.start.0);
                if row_infos.get(row_ix as usize).and_then(|row_info| row_info.diff_status).is_none() {
                    continue;
                }
                if highlighted_rows.get(&display_row_range.start).and_then(|highlight| highlight.type_id).is_some_and(
                    |type_id| {
                        [
                            TypeId::of::<ConflictsOuter>(),
                            TypeId::of::<ConflictsOursMarker>(),
                            TypeId::of::<ConflictsOurs>(),
                            TypeId::of::<ConflictsTheirs>(),
                            TypeId::of::<ConflictsTheirsMarker>(),
                        ]
                        .contains(&type_id)
                    },
                ) {
                    continue;
                }

                if active_rows.iter().any(|row| row.is_some_and(|row| display_row_range.contains(&row))) {
                    let hunk_start_y: Pixels = (display_row_range.start.as_f64()
                        * ScrollPixelOffset::from(line_height)
                        + ScrollPixelOffset::from(text_hitbox.bounds.top())
                        - scroll_pixel_position.y)
                        .into();

                    let y: Pixels = if hunk_start_y >= sticky_top {
                        hunk_start_y
                    } else {
                        let hunk_end_y: Pixels = hunk_start_y
                            + (display_row_range.len() as f64 * ScrollPixelOffset::from(line_height)).into();
                        let max_y = hunk_end_y - line_height;
                        sticky_top.min(max_y)
                    };

                    let mut element = diff_hunk_renderer.render_hunk_controls(
                        display_row_range.start.0,
                        status,
                        multi_buffer_range.clone(),
                        *is_created_file,
                        line_height,
                        &editor,
                        window,
                        cx,
                    );
                    let size = element.layout_as_root(size(px(100.0), line_height).into(), window, cx);

                    let x = text_hitbox.bounds.right() - right_margin - px(10.) - size.width;

                    if x < text_hitbox.bounds.left() {
                        continue;
                    }

                    let bounds = Bounds::new(gpui::Point::new(x, y), size);
                    control_bounds.push((display_row_range.start, bounds));

                    window.with_absolute_element_offset(gpui::Point::new(x, y), |window| element.prepaint(window, cx));
                    controls.push(element);
                }
            }
        }

        (controls, control_bounds)
    }

    fn paint_background(&self, layout: &EditorLayout, window: &mut Window, cx: &App) {
        window.paint_layer(layout.hitbox.bounds, |window| {
            let scroll_top = layout.position_map.scroll_position.y;
            let gutter_bg = cx.theme().colors().editor_gutter_background;
            window.paint_quad(fill(layout.gutter_hitbox.bounds, gutter_bg));
            window.paint_quad(fill(layout.position_map.text_hitbox.bounds, self.style.background));

            if matches!(layout.mode, EditorMode::Full { .. } | EditorMode::Minimap { .. }) {
                let show_active_line_background = match layout.mode {
                    EditorMode::Full { show_active_line_background, .. } => show_active_line_background,
                    EditorMode::Minimap { .. } => true,
                    _ => false,
                };
                let mut active_rows = layout.active_rows.iter().peekable();
                while let Some((start_row, contains_non_empty_selection)) = active_rows.next() {
                    let mut end_row = start_row.0;
                    while active_rows.peek().is_some_and(|(active_row, has_selection)| {
                        active_row.0 == end_row + 1 && has_selection.selection == contains_non_empty_selection.selection
                    }) {
                        active_rows.next().unwrap();
                        end_row += 1;
                    }

                    if show_active_line_background && !contains_non_empty_selection.selection {
                        let highlight_h_range = match layout.position_map.snapshot.current_line_highlight {
                            CurrentLineHighlight::Gutter => {
                                Some(Range { start: layout.hitbox.left(), end: layout.gutter_hitbox.right() })
                            }
                            CurrentLineHighlight::Line => Some(Range {
                                start: layout.position_map.text_hitbox.bounds.left(),
                                end: layout.position_map.text_hitbox.bounds.right(),
                            }),
                            CurrentLineHighlight::All => {
                                Some(Range { start: layout.hitbox.left(), end: layout.hitbox.right() })
                            }
                            CurrentLineHighlight::None => None,
                        };
                        if let Some(range) = highlight_h_range {
                            let active_line_bg = cx.theme().colors().editor_active_line_background;
                            let bounds = Bounds {
                                origin: point(
                                    range.start,
                                    layout.hitbox.origin.y
                                        + Pixels::from(
                                            (start_row.as_f64() - scroll_top)
                                                * ScrollPixelOffset::from(layout.position_map.line_height),
                                        ),
                                ),
                                size: size(
                                    range.end - range.start,
                                    layout.position_map.line_height * (end_row - start_row.0 + 1) as f32,
                                ),
                            };
                            window.paint_quad(fill(bounds, active_line_bg));
                        }
                    }
                }

                let mut paint_highlight = |highlight_row_start: DisplayRow,
                                           highlight_row_end: DisplayRow,
                                           highlight: crate::LineHighlight,
                                           edges| {
                    let mut origin_x = layout.hitbox.left();
                    let mut width = layout.hitbox.size.width;
                    if !highlight.include_gutter {
                        origin_x += layout.gutter_hitbox.size.width;
                        width -= layout.gutter_hitbox.size.width;
                    }

                    let origin = point(
                        origin_x,
                        layout.hitbox.origin.y
                            + Pixels::from(
                                (highlight_row_start.as_f64() - scroll_top)
                                    * ScrollPixelOffset::from(layout.position_map.line_height),
                            ),
                    );
                    let size = size(
                        width,
                        layout.position_map.line_height
                            * highlight_row_end.next_row().minus(highlight_row_start) as f32,
                    );
                    let mut quad = fill(Bounds { origin, size }, highlight.background);
                    if let Some(border_color) = highlight.border {
                        quad.border_color = border_color;
                        quad.border_widths = edges
                    }
                    window.paint_quad(quad);
                };

                let mut current_paint: Option<(LineHighlight, Range<DisplayRow>, Edges<Pixels>)> = None;
                for (&new_row, &new_background) in &layout.highlighted_rows {
                    match &mut current_paint {
                        &mut Some((current_background, ref mut current_range, mut edges)) => {
                            let new_range_started =
                                current_background != new_background || current_range.end.next_row() != new_row;
                            if new_range_started {
                                if current_range.end.next_row() == new_row {
                                    edges.bottom = px(0.);
                                };
                                paint_highlight(current_range.start, current_range.end, current_background, edges);
                                let edges = Edges {
                                    top: if current_range.end.next_row() != new_row { px(1.) } else { px(0.) },
                                    bottom: px(1.),
                                    ..Default::default()
                                };
                                current_paint = Some((new_background, new_row..new_row, edges));
                                continue;
                            } else {
                                current_range.end = current_range.end.next_row();
                            }
                        }
                        None => {
                            let edges = Edges { top: px(1.), bottom: px(1.), ..Default::default() };
                            current_paint = Some((new_background, new_row..new_row, edges))
                        }
                    };
                }
                if let Some((color, range, edges)) = current_paint {
                    paint_highlight(range.start, range.end, color, edges);
                }

                for (guide_x, active) in layout.wrap_guides.iter() {
                    let color = if *active {
                        cx.theme().colors().editor_active_wrap_guide
                    } else {
                        cx.theme().colors().editor_wrap_guide
                    };
                    window.paint_quad(fill(
                        window.pixel_snap_bounds(Bounds {
                            origin: point(*guide_x, layout.position_map.text_hitbox.origin.y),
                            size: size(px(1.), layout.position_map.text_hitbox.size.height),
                        }),
                        color,
                    ));
                }
            }
        })
    }

    fn paint_indent_guides(&self, layout: &EditorLayout, window: &mut Window, cx: &App) {
        let Some(indent_guides) = &layout.indent_guides else {
            return;
        };

        let faded_color = |color: Hsla, alpha: f32| {
            let mut faded = color;
            faded.a = alpha;
            faded
        };

        for indent_guide in indent_guides {
            let indent_accent_colors = cx.theme().accents().color_for_index(indent_guide.depth);
            let settings = &indent_guide.settings;

            // TODO fixed for now, expose them through themes later
            const INDENT_AWARE_ALPHA: f32 = 0.2;
            const INDENT_AWARE_ACTIVE_ALPHA: f32 = 0.4;
            const INDENT_AWARE_BACKGROUND_ALPHA: f32 = 0.1;
            const INDENT_AWARE_BACKGROUND_ACTIVE_ALPHA: f32 = 0.2;

            let line_color = match (settings.coloring, indent_guide.active) {
                (IndentGuideColoring::Disabled, _) => None,
                (IndentGuideColoring::Fixed, false) => Some(cx.theme().colors().editor_indent_guide),
                (IndentGuideColoring::Fixed, true) => Some(cx.theme().colors().editor_indent_guide_active),
                (IndentGuideColoring::IndentAware, false) => {
                    Some(faded_color(indent_accent_colors, INDENT_AWARE_ALPHA))
                }
                (IndentGuideColoring::IndentAware, true) => {
                    Some(faded_color(indent_accent_colors, INDENT_AWARE_ACTIVE_ALPHA))
                }
            };

            let background_color = match (settings.background_coloring, indent_guide.active) {
                (IndentGuideBackgroundColoring::Disabled, _) => None,
                (IndentGuideBackgroundColoring::IndentAware, false) => {
                    Some(faded_color(indent_accent_colors, INDENT_AWARE_BACKGROUND_ALPHA))
                }
                (IndentGuideBackgroundColoring::IndentAware, true) => {
                    Some(faded_color(indent_accent_colors, INDENT_AWARE_BACKGROUND_ACTIVE_ALPHA))
                }
            };

            let mut line_indicator_width = 0.;
            if let Some(requested_line_width) = settings.visible_line_width(indent_guide.active)
                && let Some(color) = line_color
            {
                window.paint_quad(fill(
                    window.pixel_snap_bounds(Bounds {
                        origin: indent_guide.origin,
                        size: size(px(requested_line_width as f32), indent_guide.length),
                    }),
                    color,
                ));
                line_indicator_width = requested_line_width as f32;
            }

            if let Some(color) = background_color {
                let width = indent_guide.single_indent_width - px(line_indicator_width);
                window.paint_quad(fill(
                    window.pixel_snap_bounds(Bounds {
                        origin: point(indent_guide.origin.x + px(line_indicator_width), indent_guide.origin.y),
                        size: size(width, indent_guide.length),
                    }),
                    color,
                ));
            }
        }
    }

    fn paint_line_numbers(&self, layout: &EditorLayout, window: &mut Window, cx: &mut App) {
        let is_singleton = self.editor.read(cx).buffer_kind(cx) == ItemBufferKind::Singleton;

        let line_height = layout.position_map.line_height;
        window.set_cursor_style(CursorStyle::Arrow, &layout.gutter_hitbox);

        for line_layout in layout.line_numbers.values() {
            for LineNumberSegment { shaped_line, hitbox } in &line_layout.segments {
                let Some(hitbox) = hitbox else {
                    continue;
                };

                let Some(()) = (if !is_singleton && hitbox.is_hovered(window) {
                    let color = cx.theme().colors().editor_hover_line_number;

                    let line = self.shape_line_number(shaped_line.text.clone(), color, window);
                    line.paint(hitbox.origin, line_height, TextAlign::Left, None, window, cx).log_err()
                } else {
                    shaped_line.paint(hitbox.origin, line_height, TextAlign::Left, None, window, cx).log_err()
                }) else {
                    continue;
                };

                // In singleton buffers, we select corresponding lines on the line number click, so use | -like cursor.
                // In multi buffers, we open file at the line number clicked, so use a pointing hand cursor.
                if is_singleton {
                    window.set_cursor_style(CursorStyle::IBeam, hitbox);
                } else {
                    window.set_cursor_style(CursorStyle::PointingHand, hitbox);
                }
            }
        }
    }

    fn paint_gutter_diff_hunks(
        &self,
        layout: &EditorLayout,
        split_side: Option<SplitSide>,
        window: &mut Window,
        cx: &App,
    ) {
        if layout.display_hunks.is_empty() {
            return;
        }

        let line_height = layout.position_map.line_height;
        window.paint_layer(layout.gutter_hitbox.bounds, |window| {
            for (hunk, hitbox) in &layout.display_hunks {
                let hunk_to_paint = match hunk {
                    DisplayDiffHunk::Folded { .. } => {
                        let hunk_bounds = Self::diff_hunk_bounds(
                            layout.position_map.scroll_position,
                            line_height,
                            layout.gutter_hitbox.bounds,
                            hunk,
                            &layout.position_map.snapshot,
                        );
                        Some((
                            hunk_bounds,
                            cx.theme().colors().version_control_modified,
                            Corners::all(px(0.)),
                            DiffHunkStatus::modified_none(),
                        ))
                    }
                    DisplayDiffHunk::Unfolded { status, display_row_range, .. } => hitbox.as_ref().map(|hunk_hitbox| {
                        let color = match split_side {
                            Some(SplitSide::Left) => cx.theme().colors().version_control_deleted,
                            Some(SplitSide::Right) => cx.theme().colors().version_control_added,
                            None => match status.kind {
                                DiffHunkStatusKind::Added => cx.theme().colors().version_control_added,
                                DiffHunkStatusKind::Modified => cx.theme().colors().version_control_modified,
                                DiffHunkStatusKind::Deleted => cx.theme().colors().version_control_deleted,
                            },
                        };
                        match status.kind {
                            DiffHunkStatusKind::Deleted if display_row_range.is_empty() => (
                                Bounds::new(
                                    point(hunk_hitbox.origin.x - hunk_hitbox.size.width, hunk_hitbox.origin.y),
                                    size(hunk_hitbox.size.width * 2., hunk_hitbox.size.height),
                                ),
                                color,
                                Corners::all(1. * line_height),
                                *status,
                            ),
                            _ => (hunk_hitbox.bounds, color, Corners::all(px(0.)), *status),
                        }
                    }),
                };

                if let Some((hunk_bounds, background_color, corner_radii, status)) = hunk_to_paint {
                    // Flatten the background color with the editor color to prevent
                    // elements below transparent hunks from showing through
                    let flattened_background_color = cx.theme().colors().editor_background.blend(background_color);

                    if !self.diff_hunk_hollow(status, cx) {
                        window.paint_quad(quad(
                            hunk_bounds,
                            corner_radii,
                            flattened_background_color,
                            Edges::default(),
                            transparent_black(),
                            BorderStyle::default(),
                        ));
                    } else {
                        let flattened_unstaged_background_color =
                            cx.theme().colors().editor_background.blend(background_color.opacity(0.3));

                        window.paint_quad(quad(
                            hunk_bounds,
                            corner_radii,
                            flattened_unstaged_background_color,
                            Edges::all(px(1.0)),
                            flattened_background_color,
                            BorderStyle::Solid,
                        ));
                    }
                }
            }
        });
    }

    const DEFAULT_STRIP_WIDTH_RATIO: f32 = 0.275;
    const DELETED_MARKER_WIDTH_RATIO: f32 = 0.35 / Self::DEFAULT_STRIP_WIDTH_RATIO;

    fn gutter_strip_width(line_height: Pixels) -> Pixels {
        (Self::DEFAULT_STRIP_WIDTH_RATIO * line_height).floor()
    }

    fn deleted_marker_base_width(line_height: Pixels) -> Pixels {
        (Self::DEFAULT_STRIP_WIDTH_RATIO * line_height * Self::DELETED_MARKER_WIDTH_RATIO).floor()
    }

    fn diff_hunk_bounds(
        scroll_position: gpui::Point<ScrollOffset>,
        line_height: Pixels,
        gutter_bounds: Bounds<Pixels>,
        hunk: &DisplayDiffHunk,
        snapshot: &EditorSnapshot,
    ) -> Bounds<Pixels> {
        let scroll_top = scroll_position.y * ScrollPixelOffset::from(line_height);
        let gutter_strip_width = Self::gutter_strip_width(line_height);

        match hunk {
            DisplayDiffHunk::Folded { display_row, .. } => {
                let start_y = (display_row.as_f64() * ScrollPixelOffset::from(line_height) - scroll_top).into();
                let end_y = start_y + line_height;
                let highlight_origin = gutter_bounds.origin + point(px(0.), start_y);
                let highlight_size = size(gutter_strip_width, end_y - start_y);
                Bounds::new(highlight_origin, highlight_size)
            }
            DisplayDiffHunk::Unfolded { display_row_range, status, .. } => {
                if status.is_deleted() && display_row_range.is_empty() {
                    let row = display_row_range.start;

                    let offset = ScrollPixelOffset::from(line_height / 2.);
                    let start_y = (row.as_f64() * ScrollPixelOffset::from(line_height) - offset - scroll_top).into();
                    let end_y = start_y + line_height;

                    let width = Self::deleted_marker_base_width(line_height);
                    let highlight_origin = gutter_bounds.origin + point(px(0.), start_y);
                    let highlight_size = size(width, end_y - start_y);
                    Bounds::new(highlight_origin, highlight_size)
                } else {
                    let start_row = display_row_range.start;
                    let end_row = display_row_range.end;
                    // If we're in a multibuffer, row range span might include an
                    // excerpt header, so if we were to draw the marker straight away,
                    // the hunk might include the rows of that header.
                    // Making the range inclusive doesn't quite cut it, as we rely on the exclusivity for the soft wrap.
                    // Instead, we simply check whether the range we're dealing with includes
                    // any excerpt headers and if so, we stop painting the diff hunk on the first row of that header.
                    let end_row_in_current_excerpt = snapshot
                        .blocks_in_range(start_row..end_row)
                        .find_map(|(start_row, block)| {
                            if matches!(block, Block::ExcerptBoundary { .. } | Block::BufferHeader { .. }) {
                                Some(start_row)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(end_row);

                    let start_y = (start_row.as_f64() * ScrollPixelOffset::from(line_height) - scroll_top).into();
                    let end_y = Pixels::from(
                        end_row_in_current_excerpt.as_f64() * ScrollPixelOffset::from(line_height) - scroll_top,
                    );

                    let highlight_origin = gutter_bounds.origin + point(px(0.), start_y);
                    let highlight_size = size(gutter_strip_width, end_y - start_y);
                    Bounds::new(highlight_origin, highlight_size)
                }
            }
        }
    }

    fn paint_gutter_indicators(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        window.paint_layer(layout.gutter_hitbox.bounds, |window| {
            window.with_element_namespace("crease_toggles", |window| {
                for crease_toggle in layout.crease_toggles.iter_mut().flatten() {
                    crease_toggle.paint(window, cx);
                }
            });

            window.with_element_namespace("expand_toggles", |window| {
                for (expand_toggle, _) in layout.expand_toggles.iter_mut().flatten() {
                    expand_toggle.paint(window, cx);
                }
            });

            for test_indicator in layout.test_indicators.iter_mut() {
                test_indicator.paint(window, cx);
            }

            for gutter_hover_button in layout.gutter_hover_buttons.iter_mut() {
                gutter_hover_button.paint(window, cx);
            }
        });
    }

    fn paint_gutter_highlights(&self, layout: &EditorLayout, window: &mut Window, cx: &App) {
        for (_, hunk_hitbox) in &layout.display_hunks {
            if let Some(hunk_hitbox) = hunk_hitbox
                && !self.editor.read(cx).buffer().read(cx).all_diff_hunks_expanded()
            {
                window.set_cursor_style(CursorStyle::PointingHand, hunk_hitbox);
            }
        }

        let show_git_gutter = layout.position_map.snapshot.show_git_diff_gutter.unwrap_or_else(|| {
            matches!(ProjectSettings::get_global(cx).git.git_gutter, GitGutterSetting::TrackedFiles)
        });
        if show_git_gutter {
            self.paint_gutter_diff_hunks(layout, self.split_side, window, cx)
        }

        let highlight_width = 0.275 * layout.position_map.line_height;
        let highlight_corner_radii = Corners::all(0.05 * layout.position_map.line_height);
        window.paint_layer(layout.gutter_hitbox.bounds, |window| {
            for (range, color) in &layout.highlighted_gutter_ranges {
                let start_row = if range.start.row() < layout.visible_display_row_range.start {
                    layout.visible_display_row_range.start - DisplayRow(1)
                } else {
                    range.start.row()
                };
                let end_row = if range.end.row() > layout.visible_display_row_range.end {
                    layout.visible_display_row_range.end + DisplayRow(1)
                } else {
                    range.end.row()
                };

                let start_y = layout.gutter_hitbox.top()
                    + Pixels::from(
                        start_row.0 as f64 * ScrollPixelOffset::from(layout.position_map.line_height)
                            - layout.position_map.scroll_pixel_position.y,
                    );
                let end_y = layout.gutter_hitbox.top()
                    + Pixels::from(
                        (end_row.0 + 1) as f64 * ScrollPixelOffset::from(layout.position_map.line_height)
                            - layout.position_map.scroll_pixel_position.y,
                    );
                let bounds = Bounds::from_corners(
                    point(layout.gutter_hitbox.left(), start_y),
                    point(layout.gutter_hitbox.left() + highlight_width, end_y),
                );
                window.paint_quad(fill(bounds, *color).corner_radii(highlight_corner_radii));
            }
        });
    }

    fn paint_blamed_display_rows(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        let Some(blamed_display_rows) = layout.blamed_display_rows.take() else {
            return;
        };

        window.paint_layer(layout.gutter_hitbox.bounds, |window| {
            for mut blame_element in blamed_display_rows.into_iter() {
                blame_element.paint(window, cx);
            }
        })
    }

    fn populate_point_diagnostics(
        &self,
        snapshot: &EditorSnapshot,
        visible_rows: Range<DisplayRow>,
        line_layouts: &mut [LineWithInvisibles],
    ) {
        if !snapshot.display_snapshot.diagnostics_visible() || visible_rows.is_empty() {
            return;
        }

        let display_snapshot = &snapshot.display_snapshot;
        let buffer_snapshot = snapshot.buffer_snapshot();

        let query_start = display_snapshot.display_point_to_point(DisplayPoint::new(visible_rows.start, 0), Bias::Left);
        let query_end_display =
            display_snapshot.clip_ignoring_line_ends(DisplayPoint::new(visible_rows.end, 0), Bias::Right);
        let query_end = display_snapshot.display_point_to_point(query_end_display, Bias::Right);

        for (point, diagnostic) in Self::point_diagnostics_in_range(buffer_snapshot, query_start..query_end) {
            if display_snapshot.intersects_fold(point) {
                continue;
            }

            let Some(underline) = display_snapshot.diagnostic_underline_style(
                diagnostic.severity,
                diagnostic.underline,
                diagnostic.is_unnecessary,
                &self.style,
            ) else {
                continue;
            };

            let display_point = point.to_display_point(display_snapshot);
            if !visible_rows.contains(&display_point.row()) {
                continue;
            }

            let line_ix = display_point.row().minus(visible_rows.start) as usize;
            let Some(line_layout) = line_layouts.get_mut(line_ix) else {
                continue;
            };
            let point_diagnostic =
                PointDiagnostic { column: display_point.column(), underline, severity: diagnostic.severity };

            line_layout.add_point_diagnostic(point_diagnostic);
        }
    }

    fn point_diagnostic_anchor(
        snapshot: &MultiBufferSnapshot,
        excerpt: &ExcerptRange<text::Anchor>,
        text_anchor: text::Anchor,
    ) -> Option<Anchor> {
        let buffer = snapshot.buffer_for_id(text_anchor.buffer_id)?;
        if !excerpt.contains(&text_anchor, buffer) {
            return None;
        }
        let anchor = text_anchor.bias_right(buffer);
        let anchor = if excerpt.context.start.cmp(&anchor, buffer).is_gt() {
            excerpt.context.start
        } else if excerpt.context.end.cmp(&anchor, buffer).is_lt() {
            excerpt.context.end
        } else {
            anchor
        };
        snapshot.anchor_in_buffer(anchor)
    }

    fn add_point_diagnostic<'a>(
        point_diagnostics: &mut Vec<(Point, &'a Diagnostic)>,
        seen: &mut HashSet<(Point, *const Diagnostic)>,
        range: &Range<Point>,
        point: Point,
        diagnostic: &'a Diagnostic,
    ) {
        if point >= range.start && point <= range.end && seen.insert((point, std::ptr::from_ref(diagnostic))) {
            point_diagnostics.push((point, diagnostic));
        }
    }

    fn point_diagnostics_in_range(snapshot: &MultiBufferSnapshot, range: Range<Point>) -> Vec<(Point, &Diagnostic)> {
        let mut point_diagnostics = Vec::new();
        let mut seen = HashSet::default();

        for (buffer, buffer_range, excerpt) in snapshot.range_to_buffer_ranges(range.clone()) {
            for entry in buffer
                .diagnostic_entries_in_range(buffer_range, false)
                .filter(|entry| entry.range.start == entry.range.end)
            {
                let Some(anchor) = Self::point_diagnostic_anchor(snapshot, &excerpt, entry.range.start) else {
                    continue;
                };
                Self::add_point_diagnostic(
                    &mut point_diagnostics,
                    &mut seen,
                    &range,
                    anchor.to_point(snapshot),
                    &entry.diagnostic,
                );
            }
        }

        for (_, _, deleted_hunk_anchor) in snapshot.range_to_buffer_ranges_with_deleted_hunks(range.clone()) {
            let Some(deleted_hunk_anchor) = deleted_hunk_anchor else {
                continue;
            };
            let Some((live_anchor, buffer)) = snapshot.anchor_to_buffer_anchor(deleted_hunk_anchor) else {
                continue;
            };
            let Some((_, excerpt)) = snapshot.excerpt_containing(deleted_hunk_anchor..deleted_hunk_anchor) else {
                continue;
            };
            let live_point = live_anchor.to_point(buffer);

            for entry in buffer
                .diagnostic_entries_in_range(live_point..live_point, false)
                .filter(|entry| entry.range.start == entry.range.end)
            {
                if entry.range.start.to_point(buffer) != live_point {
                    continue;
                }
                let Some(anchor) = Self::point_diagnostic_anchor(snapshot, &excerpt, entry.range.start) else {
                    continue;
                };
                Self::add_point_diagnostic(
                    &mut point_diagnostics,
                    &mut seen,
                    &range,
                    anchor.to_point(snapshot),
                    &entry.diagnostic,
                );
            }
        }

        point_diagnostics
    }

    fn paint_text(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        window.with_content_mask(Some(ContentMask { bounds: layout.position_map.text_hitbox.bounds }), |window| {
            let editor = self.editor.read(cx);
            if let SelectionDragState::ReadyToDrag { mouse_down_time, .. } = &editor.selection_drag_state {
                let drag_and_drop_delay =
                    Duration::from_millis(EditorSettings::get_global(cx).drag_and_drop_selection.delay.0);
                if mouse_down_time.elapsed() >= drag_and_drop_delay {
                    window.set_cursor_style(CursorStyle::DragCopy, &layout.position_map.text_hitbox);
                }
            } else if matches!(editor.selection_drag_state, SelectionDragState::Dragging { .. }) {
                window.set_cursor_style(CursorStyle::DragCopy, &layout.position_map.text_hitbox);
            } else if editor
                .hovered_link_state
                .as_ref()
                .is_some_and(|hovered_link_state| !hovered_link_state.links.is_empty())
            {
                window.set_cursor_style(CursorStyle::PointingHand, &layout.position_map.text_hitbox);
            } else if !window.modifiers().modified()
                && let Some(hovered_command) = editor.hovered_inlay_hint_command()
                && hovered_command.contains_glyph(
                    &layout.position_map.snapshot,
                    layout.position_map.inlay_hint_glyph_for_position(window.mouse_position()),
                )
            {
                window.set_cursor_style(CursorStyle::PointingHand, &layout.position_map.text_hitbox);
            } else {
                window.set_cursor_style(CursorStyle::IBeam, &layout.position_map.text_hitbox);
            };

            self.paint_lines_background(layout, window, cx);
            let invisible_display_ranges = self.paint_highlights(layout, window, cx);
            self.paint_document_colors(layout, window);
            self.paint_lines(&invisible_display_ranges, layout, window, cx);
            self.paint_navigation_overlays(layout, window, cx);
            self.paint_cursors(layout, window, cx);
            self.paint_inline_diagnostics(layout, window, cx);
            self.paint_inline_code_actions(layout, window, cx);
            self.paint_diff_hunk_controls(layout, window, cx);
            window.with_element_namespace("crease_trailers", |window| {
                for trailer in layout.crease_trailers.iter_mut().flatten() {
                    trailer.element.paint(window, cx);
                }
            });
        })
    }

    fn paint_highlights(
        &self,
        layout: &EditorLayout,
        window: &mut Window,
        cx: &App,
    ) -> SmallVec<[Range<DisplayPoint>; 32]> {
        window.paint_layer(layout.position_map.text_hitbox.bounds, |window| {
            let mut invisible_display_ranges = SmallVec::<[Range<DisplayPoint>; 32]>::new();
            let line_end_overshoot = 0.15 * layout.position_map.line_height;
            for (range, color) in &layout.highlighted_ranges {
                self.paint_highlighted_range(
                    range.clone(),
                    true,
                    *color,
                    Pixels::ZERO,
                    line_end_overshoot,
                    layout,
                    window,
                );
            }

            let corner_radius = if EditorSettings::get_global(cx).rounded_selection {
                0.15 * layout.position_map.line_height
            } else {
                Pixels::ZERO
            };

            for (player_color, selections) in &layout.selections {
                for selection in selections.iter() {
                    self.paint_highlighted_range(
                        selection.range.clone(),
                        true,
                        player_color.selection,
                        corner_radius,
                        corner_radius * 2.,
                        layout,
                        window,
                    );

                    if selection.is_local && !selection.range.is_empty() {
                        invisible_display_ranges.push(selection.range.clone());
                    }
                }
            }
            invisible_display_ranges
        })
    }

    fn paint_lines(
        &self,
        invisible_display_ranges: &[Range<DisplayPoint>],
        layout: &mut EditorLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        let whitespace_setting = self.editor.read(cx).buffer.read(cx).language_settings(cx).show_whitespaces;

        for (ix, line_with_invisibles) in layout.position_map.line_layouts.iter().enumerate() {
            let row = DisplayRow(layout.visible_display_row_range.start.0 + ix as u32);
            line_with_invisibles.draw(
                layout,
                row,
                layout.content_origin,
                whitespace_setting,
                invisible_display_ranges,
                window,
                cx,
            )
        }

        for line_element in &mut layout.line_elements {
            line_element.paint(window, cx);
        }
    }

    fn paint_lines_background(&self, layout: &EditorLayout, window: &mut Window, cx: &mut App) {
        for (ix, line_with_invisibles) in layout.position_map.line_layouts.iter().enumerate() {
            let row = DisplayRow(layout.visible_display_row_range.start.0 + ix as u32);
            line_with_invisibles.draw_background(layout, row, layout.content_origin, window, cx);
        }
    }

    fn paint_navigation_overlays(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        window.with_element_namespace("navigation_overlays", |window| {
            for command in &mut layout.navigation_overlay_paint_commands {
                let NavigationOverlayPaintCommand::Label(label) = command;
                label.element.paint(window, cx);
            }
        });
    }

    fn paint_document_colors(&self, layout: &EditorLayout, window: &mut Window) {
        let Some((colors_render_mode, image_colors)) = &layout.document_colors else {
            return;
        };
        if image_colors.is_empty()
            || colors_render_mode == &DocumentColorsRenderMode::None
            || colors_render_mode == &DocumentColorsRenderMode::Inlay
        {
            return;
        }

        let line_end_overshoot = layout.line_end_overshoot();

        for (range, color) in image_colors {
            match colors_render_mode {
                DocumentColorsRenderMode::Inlay | DocumentColorsRenderMode::None => return,
                DocumentColorsRenderMode::Background => {
                    self.paint_highlighted_range(
                        range.clone(),
                        true,
                        *color,
                        Pixels::ZERO,
                        line_end_overshoot,
                        layout,
                        window,
                    );
                }
                DocumentColorsRenderMode::Border => {
                    self.paint_highlighted_range(
                        range.clone(),
                        false,
                        *color,
                        Pixels::ZERO,
                        line_end_overshoot,
                        layout,
                        window,
                    );
                }
            }
        }
    }

    fn paint_cursors(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        for cursor in &mut layout.visible_cursors {
            cursor.paint(layout.content_origin, window, cx);
        }
    }

    fn paint_scrollbars(&self, layout: &mut EditorLayout, window: &mut Window, cx: &App) {
        let Some(scrollbars_layout) = layout.scrollbars_layout.take() else {
            return;
        };
        let any_scrollbar_dragged = self.editor.read(cx).scroll_manager.any_scrollbar_dragged();

        // Painted like the `ui` scrollbars (e.g. the project panel): no track, and a
        // thin rounded thumb inset from the larger hitbox.
        for (scrollbar_layout, _) in scrollbars_layout.iter_scrollbars() {
            let hitbox = &scrollbar_layout.hitbox;
            if !scrollbars_layout.visible {
                continue;
            }
            let Some(thumb_bounds) = scrollbar_layout.thumb_bounds else {
                continue;
            };
            let scrollbar_thumb_color = match scrollbar_layout.thumb_state {
                ScrollbarThumbState::Dragging => cx.theme().colors().scrollbar_thumb_active_background,
                ScrollbarThumbState::Hovered => cx.theme().colors().scrollbar_thumb_hover_background,
                ScrollbarThumbState::Idle => cx.theme().colors().scrollbar_thumb_background,
            };
            let thumb_bounds = thumb_bounds.dilate(-ui::SCROLLBAR_PADDING);
            window.paint_layer(hitbox.bounds, |window| {
                window.paint_quad(quad(
                    thumb_bounds,
                    Corners::all(Pixels::MAX).clamp_radii_for_quad_size(thumb_bounds.size),
                    scrollbar_thumb_color,
                    Edges::default(),
                    Hsla::transparent_black(),
                    BorderStyle::default(),
                ));
            });

            if any_scrollbar_dragged {
                window.set_window_cursor_style(CursorStyle::Arrow);
            } else {
                window.set_cursor_style(CursorStyle::Arrow, hitbox);
            }
        }

        window.on_mouse_event({
            let editor = self.editor.clone();
            let scrollbars_layout = scrollbars_layout.clone();

            let mut mouse_position = window.mouse_position();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    return;
                }

                editor.update(cx, |editor, cx| {
                    if let Some((scrollbar_layout, axis)) = event
                        .pressed_button
                        .filter(|button| *button == MouseButton::Left)
                        .and(editor.scroll_manager.dragging_scrollbar_axis())
                        .and_then(|axis| scrollbars_layout.iter_scrollbars().find(|(_, a)| *a == axis))
                    {
                        let ScrollbarLayout { hitbox, text_unit_size, .. } = scrollbar_layout;

                        let old_position = mouse_position.along(axis);
                        let new_position = event.position.along(axis);
                        if (hitbox.origin.along(axis)..hitbox.bottom_right().along(axis)).contains(&old_position) {
                            let position = editor.scroll_position(cx).apply_along(axis, |p| {
                                (p + ScrollOffset::from((new_position - old_position) / *text_unit_size)).max(0.)
                            });
                            editor.set_scroll_position(position, window, cx);
                        }

                        editor.scroll_manager.show_scrollbars(window, cx);
                        cx.stop_propagation();
                    } else if let Some((layout, axis)) =
                        scrollbars_layout.get_hovered_axis(window).filter(|_| !event.dragging())
                    {
                        if layout.thumb_hovered(&event.position) {
                            editor.scroll_manager.set_hovered_scroll_thumb_axis(axis, cx);
                        } else {
                            editor.scroll_manager.reset_scrollbar_state(cx);
                        }

                        editor.scroll_manager.show_scrollbars(window, cx);
                    } else {
                        editor.scroll_manager.reset_scrollbar_state(cx);
                    }

                    mouse_position = event.position;
                })
            }
        });

        if any_scrollbar_dragged {
            window.on_mouse_event({
                let editor = self.editor.clone();
                move |_: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        return;
                    }

                    editor.update(cx, |editor, cx| {
                        if let Some((_, axis)) = scrollbars_layout.get_hovered_axis(window) {
                            editor.scroll_manager.set_hovered_scroll_thumb_axis(axis, cx);
                        } else {
                            editor.scroll_manager.reset_scrollbar_state(cx);
                        }
                        cx.stop_propagation();
                    });
                }
            });
        } else {
            window.on_mouse_event({
                let editor = self.editor.clone();

                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        return;
                    }
                    let Some((scrollbar_layout, axis)) = scrollbars_layout.get_hovered_axis(window) else {
                        return;
                    };

                    let ScrollbarLayout { hitbox, visible_range, text_unit_size, thumb_bounds, .. } = scrollbar_layout;

                    let Some(thumb_bounds) = thumb_bounds else {
                        return;
                    };

                    editor.update(cx, |editor, cx| {
                        editor.scroll_manager.set_dragged_scroll_thumb_axis(axis, cx);

                        let event_position = event.position.along(axis);

                        if event_position < thumb_bounds.origin.along(axis)
                            || thumb_bounds.bottom_right().along(axis) < event_position
                        {
                            let center_position =
                                ((event_position - hitbox.origin.along(axis)) / *text_unit_size).round() as u32;
                            let start_position =
                                center_position.saturating_sub((visible_range.end - visible_range.start) as u32 / 2);

                            let position =
                                editor.scroll_position(cx).apply_along(axis, |_| start_position as ScrollOffset);

                            editor.set_scroll_position(position, window, cx);
                        } else {
                            editor.scroll_manager.show_scrollbars(window, cx);
                        }

                        cx.stop_propagation();
                    });
                }
            });
        }
    }

    fn paint_highlighted_range(
        &self,
        range: Range<DisplayPoint>,
        fill: bool,
        color: Hsla,
        corner_radius: Pixels,
        line_end_overshoot: Pixels,
        layout: &EditorLayout,
        window: &mut Window,
    ) {
        let start_row = layout.visible_display_row_range.start;
        let end_row = layout.visible_display_row_range.end;
        if range.start != range.end {
            let row_range = if range.end.column() == 0 {
                cmp::max(range.start.row(), start_row)..cmp::min(range.end.row(), end_row)
            } else {
                cmp::max(range.start.row(), start_row)..cmp::min(range.end.row().next_row(), end_row)
            };

            let highlighted_range = HighlightedRange {
                color,
                line_height: layout.position_map.line_height,
                corner_radius,
                start_y: layout.content_origin.y
                    + Pixels::from(
                        (row_range.start.as_f64() - layout.position_map.scroll_position.y)
                            * ScrollOffset::from(layout.position_map.line_height),
                    ),
                lines: row_range
                    .iter_rows()
                    .map(|row| {
                        let line_layout = &layout.position_map.line_layouts[row.minus(start_row) as usize];
                        let alignment_offset = line_layout.alignment_offset(layout.text_align, layout.content_width);
                        HighlightedRangeLine {
                            start_x: if row == range.start.row() {
                                layout.content_origin.x
                                    + Pixels::from(
                                        ScrollPixelOffset::from(
                                            line_layout.x_for_index(range.start.column() as usize) + alignment_offset,
                                        ) - layout.position_map.scroll_pixel_position.x,
                                    )
                            } else {
                                layout.content_origin.x + alignment_offset
                                    - Pixels::from(layout.position_map.scroll_pixel_position.x)
                            },
                            end_x: if row == range.end.row() {
                                layout.content_origin.x
                                    + Pixels::from(
                                        ScrollPixelOffset::from(
                                            line_layout.x_for_index(range.end.column() as usize) + alignment_offset,
                                        ) - layout.position_map.scroll_pixel_position.x,
                                    )
                            } else {
                                Pixels::from(
                                    ScrollPixelOffset::from(
                                        layout.content_origin.x
                                            + line_layout.width
                                            + alignment_offset
                                            + line_end_overshoot,
                                    ) - layout.position_map.scroll_pixel_position.x,
                                )
                            },
                        }
                    })
                    .collect(),
            };

            highlighted_range.paint(fill, layout.position_map.text_hitbox.bounds, window);
        }
    }

    fn paint_inline_diagnostics(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        for mut inline_diagnostic in layout.inline_diagnostics.drain() {
            inline_diagnostic.1.paint(window, cx);
        }
    }

    fn paint_inline_code_actions(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        if let Some(mut inline_code_actions) = layout.inline_code_actions.take() {
            window.paint_layer(layout.position_map.text_hitbox.bounds, |window| {
                inline_code_actions.paint(window, cx);
            })
        }
    }

    fn paint_diff_hunk_controls(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        for mut diff_hunk_control in layout.diff_hunk_controls.drain(..) {
            diff_hunk_control.paint(window, cx);
        }
    }

    fn paint_spacer_blocks(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        for mut block in layout.spacer_blocks.drain(..) {
            let mut bounds = layout.hitbox.bounds;
            bounds.origin.x += layout.gutter_hitbox.bounds.size.width;
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                block.element.paint(window, cx);
            })
        }
    }

    fn paint_non_spacer_blocks(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        for mut block in layout.blocks.drain(..) {
            if block.overlaps_gutter {
                block.element.paint(window, cx);
            } else {
                let mut bounds = layout.hitbox.bounds;
                bounds.origin.x += layout.gutter_hitbox.bounds.size.width;
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    block.element.paint(window, cx);
                })
            }
        }
    }

    fn paint_mouse_context_menu(&self, layout: &mut EditorLayout, window: &mut Window, cx: &mut App) {
        if let Some(mouse_context_menu) = layout.mouse_context_menu.as_mut() {
            mouse_context_menu.paint(window, cx);
        }
    }

    fn shape_line_number(&self, text: SharedString, color: Hsla, window: &Window) -> ShapedLine {
        let run = TextRun { len: text.len(), font: self.style.text.font(), color, ..Default::default() };
        window.text_system().shape_line(text, self.style.text.font_size.to_pixels(window.rem_size()), &[run], None)
    }

    fn diff_hunk_hollow(&self, status: DiffHunkStatus, cx: &App) -> bool {
        let unstaged = !self.editor.read(cx).diff_hunk_renderer().render_hunk_as_staged(&status, cx);
        let unstaged_hollow =
            matches!(ProjectSettings::get_global(cx).git.hunk_style, GitHunkStyleSetting::UnstagedHollow);

        unstaged == unstaged_hollow
    }

    #[cfg(debug_assertions)]
    fn layout_debug_ranges(
        selections: &mut Vec<(PlayerColor, Vec<SelectionLayout>)>,
        anchor_range: Range<Anchor>,
        display_snapshot: &DisplaySnapshot,
        cx: &App,
    ) {
        let theme = cx.theme();
        text::debug::GlobalDebugRanges::with_locked(|debug_ranges| {
            if debug_ranges.ranges.is_empty() {
                return;
            }
            let buffer_snapshot = &display_snapshot.buffer_snapshot();
            for (excerpt_buffer_snapshot, buffer_range, _) in
                buffer_snapshot.range_to_buffer_ranges(anchor_range.start..anchor_range.end)
            {
                let buffer_range = excerpt_buffer_snapshot.anchor_after(buffer_range.start)
                    ..excerpt_buffer_snapshot.anchor_before(buffer_range.end);
                selections.extend(debug_ranges.ranges.iter().flat_map(|debug_range| {
                    debug_range.ranges.iter().filter_map(|range| {
                        let accent = theme.accents().color_for_index(debug_range.occurrence_index as u32 + 1);
                        let player_color = PlayerColor { cursor: accent, selection: accent.opacity(0.25) };
                        if range.start.buffer_id != excerpt_buffer_snapshot.remote_id() {
                            return None;
                        }
                        let clipped_start = range.start.max(&buffer_range.start, excerpt_buffer_snapshot);
                        let clipped_end = range.end.min(&buffer_range.end, excerpt_buffer_snapshot);
                        let range =
                            buffer_snapshot.buffer_anchor_range_to_anchor_range(*clipped_start..*clipped_end)?;
                        let start = range.start.to_display_point(display_snapshot);
                        let end = range.end.to_display_point(display_snapshot);
                        let selection_layout = SelectionLayout {
                            id: 0,
                            head: start,
                            range: start..end,
                            cursor_shape: CursorShape::Bar,
                            is_newest: false,
                            is_local: false,
                            active_rows: start.row()..end.row(),
                            user_name: Some(SharedString::from(debug_range.value.clone())),
                        };
                        Some((player_color, vec![selection_layout]))
                    })
                }));
            }
        });
    }
}

struct Gutter<'a> {
    line_height: Pixels,
    range: Range<DisplayRow>,
    scroll_position: gpui::Point<ScrollOffset>,
    dimensions: &'a GutterDimensions,
    hitbox: &'a Hitbox,
    snapshot: &'a EditorSnapshot,
    row_infos: &'a [RowInfo],
}

impl Gutter<'_> {
    fn layout_item_skipping_folds(
        &self,
        display_row: DisplayRow,
        render_item: impl Fn(&mut Context<'_, Editor>, &mut Window) -> AnyElement,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> Option<AnyElement> {
        let row = MultiBufferRow(DisplayPoint::new(display_row, 0).to_point(self.snapshot).row);
        if self.snapshot.is_line_folded(row) {
            return None;
        }

        self.layout_item(display_row, render_item, window, cx)
    }

    fn layout_item(
        &self,
        display_row: DisplayRow,
        render_item: impl Fn(&mut Context<'_, Editor>, &mut Window) -> AnyElement,
        window: &mut Window,
        cx: &mut Context<'_, Editor>,
    ) -> Option<AnyElement> {
        if !self.range.contains(&display_row) {
            return None;
        }

        if self.row_infos.get((display_row.0.saturating_sub(self.range.start.0)) as usize).is_some_and(|row_info| {
            row_info.expand_info.is_some() || row_info.diff_status.is_some_and(|status| status.is_deleted())
        }) {
            return None;
        }

        let button = self.prepaint_button(render_item(cx, window), display_row, window, cx);
        Some(button)
    }

    fn prepaint_button(
        &self,
        mut button: AnyElement,
        row: DisplayRow,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let available_space = size(AvailableSpace::MinContent, AvailableSpace::Definite(self.line_height));
        let indicator_size = button.layout_as_root(available_space, window, cx);
        let git_gutter_width = EditorElement::gutter_strip_width(self.line_height)
            + self.dimensions.git_blame_entries_width.unwrap_or_default();

        // center in the slot between the git strip and the line numbers, so the gap to the
        // numbers stays even once they fill their column (4+ digit files)
        let slack = self.dimensions.left_padding - git_gutter_width - indicator_size.width;
        let x = git_gutter_width + (slack / 2.).max(Pixels::ZERO);

        let mut y = Pixels::from((row.as_f64() - self.scroll_position.y) * ScrollPixelOffset::from(self.line_height));
        y += (self.line_height - indicator_size.height) / 2.;

        button.prepaint_as_root(self.hitbox.origin + point(x, y), available_space, window, cx);
        button
    }
}

pub fn render_breadcrumb_text(
    mut segments: Vec<HighlightedText>,
    breadcrumb_font: Option<Font>,
    prefix: Option<gpui::AnyElement>,
    active_item: &dyn ItemHandle,
    multibuffer_header: bool,
    window: &mut Window,
    cx: &App,
) -> gpui::AnyElement {
    const MAX_SEGMENTS: usize = 12;

    let element = h_flex().flex_grow_1().text_ui(cx);

    let prefix_end_ix = cmp::min(segments.len(), MAX_SEGMENTS / 2);
    let suffix_start_ix = cmp::max(prefix_end_ix, segments.len().saturating_sub(MAX_SEGMENTS / 2));

    if suffix_start_ix > prefix_end_ix {
        segments.splice(prefix_end_ix..suffix_start_ix, Some(HighlightedText { text: "⋯".into(), highlights: vec![] }));
    }

    let highlighted_segments = segments.into_iter().enumerate().map(|(index, segment)| {
        let mut text_style = window.text_style();
        if let Some(font) = &breadcrumb_font {
            text_style.font_family = font.family.clone();
            text_style.font_features = font.features.clone();
            text_style.font_style = font.style;
            text_style.font_weight = font.weight;
        }
        text_style.color = Color::Muted.color(cx);

        if index == 0
            && !workspace::TabBarSettings::get_global(cx).show
            && active_item.is_dirty(cx)
            && let Some(styled_element) = apply_dirty_filename_style(&segment, &text_style, cx)
        {
            return styled_element;
        }

        StyledText::new(segment.text.replace('\n', " "))
            .with_default_highlights(&text_style, segment.highlights)
            .into_any()
    });

    let breadcrumbs = Itertools::intersperse_with(highlighted_segments, || {
        Label::new("›").color(Color::Placeholder).into_any_element()
    });

    let breadcrumbs_stack = h_flex()
        .gap_1()
        .when(multibuffer_header, |this| this.pl_2().border_l_1().border_color(cx.theme().colors().border.opacity(0.6)))
        .children(breadcrumbs);

    let breadcrumbs = if let Some(prefix) = prefix {
        h_flex().gap_1p5().child(prefix).child(breadcrumbs_stack)
    } else {
        breadcrumbs_stack
    };

    let editor = active_item.downcast::<Editor>().map(|editor| editor.downgrade());

    let has_project_path = active_item.project_path(cx).is_some();

    match editor {
        Some(editor) => element
            .id("breadcrumb_container")
            .when(!multibuffer_header, |this| this.overflow_x_scroll())
            .child(
                ButtonLike::new("toggle outline view")
                    .child(breadcrumbs)
                    .when(multibuffer_header, |this| this.style(ButtonStyle::Transparent))
                    .when(!multibuffer_header, |this| {
                        let focus_handle = editor.upgrade().unwrap().focus_handle(cx);

                        this.tooltip(Tooltip::element(move |_window, cx| {
                            v_flex()
                                .gap_1()
                                .child(
                                    h_flex().gap_1().justify_between().child(Label::new("Show Symbol Outline")).child(
                                        ui::KeyBinding::for_action_in(
                                            &zed_actions::outline::ToggleOutline,
                                            &focus_handle,
                                            cx,
                                        ),
                                    ),
                                )
                                .when(has_project_path, |this| {
                                    this.child(
                                        h_flex()
                                            .gap_1()
                                            .justify_between()
                                            .pt_1()
                                            .border_t_1()
                                            .border_color(cx.theme().colors().border_variant)
                                            .child(Label::new("Right-Click to Copy Path")),
                                    )
                                })
                                .into_any_element()
                        }))
                        .on_click({
                            let editor = editor.clone();
                            move |_, window, cx| {
                                if let Some((editor, callback)) =
                                    editor.upgrade().zip(zed_actions::outline::TOGGLE_OUTLINE.get())
                                {
                                    callback(editor.to_any_view(), window, cx);
                                }
                            }
                        })
                        .when(has_project_path, |this| {
                            this.on_right_click({
                                let editor = editor.clone();
                                move |_, _, cx| {
                                    if let Some(abs_path) = editor.upgrade().and_then(|editor| {
                                        editor.update(cx, |editor, cx| editor.target_file_abs_path(cx))
                                    }) && let Some(path_str) = abs_path.to_str()
                                    {
                                        cx.write_to_clipboard(ClipboardItem::new_string(path_str.to_string()));
                                    }
                                }
                            })
                        })
                    }),
            )
            .into_any_element(),
        None => element
            .h(rems_from_px(22_f32)) // Match the height and padding of the `ButtonLike` in the other arm.
            .pl_1()
            .child(breadcrumbs)
            .into_any_element(),
    }
}

fn apply_dirty_filename_style(
    segment: &HighlightedText,
    text_style: &gpui::TextStyle,
    cx: &App,
) -> Option<gpui::AnyElement> {
    let text = segment.text.replace('\n', " ");

    let filename_position = std::path::Path::new(segment.text.as_ref()).file_name().and_then(|f| {
        let filename_str = f.to_string_lossy();
        segment.text.rfind(filename_str.as_ref())
    })?;

    let bold_weight = FontWeight::BOLD;
    let default_color = Color::Default.color(cx);

    if filename_position == 0 {
        let mut filename_style = text_style.clone();
        filename_style.font_weight = bold_weight;
        filename_style.color = default_color;

        return Some(StyledText::new(text).with_default_highlights(&filename_style, []).into_any());
    }

    let highlight_style =
        gpui::HighlightStyle { font_weight: Some(bold_weight), color: Some(default_color), ..Default::default() };

    let highlight = vec![(filename_position..text.len(), highlight_style)];
    Some(StyledText::new(text).with_default_highlights(text_style, highlight).into_any())
}

fn render_blame_entry(
    ix: usize,
    blame: &Entity<GitBlame>,
    blame_entry: BlameEntry,
    style: &EditorStyle,
    last_used_color: &mut Option<(Hsla, Oid)>,
    editor: Entity<Editor>,
    workspace: Entity<Workspace>,
    buffer: BufferId,
    renderer: &dyn BlameRenderer,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let index: u32 = blame_entry.sha.into();
    let mut sha_color = cx.theme().accents().color_for_index(index);

    // If the last color we used is the same as the one we get for this line, but
    // the commit SHAs are different, then we try again to get a different color.
    if let Some((color, sha)) = *last_used_color
        && sha != blame_entry.sha
        && color == sha_color
    {
        sha_color = cx.theme().accents().color_for_index(index + 1);
    }
    last_used_color.replace((sha_color, blame_entry.sha));

    let blame = blame.read(cx);
    let details = blame.details_for_entry(buffer, &blame_entry);
    let tag_names = blame.tag_names_for_entry(buffer, &blame_entry);
    let repository = blame.repository(cx, buffer)?;
    renderer.render_blame_entry(
        &style.text,
        blame_entry,
        details,
        tag_names,
        repository,
        workspace.downgrade(),
        editor,
        ix,
        sha_color,
        window,
        cx,
    )
}

#[derive(Debug)]
pub(crate) struct LineWithInvisibles {
    fragments: SmallVec<[LineFragment; 1]>,
    invisibles: Vec<Invisible>,
    diagnostic_underline_severity_ranges: Vec<(Range<usize>, lsp::DiagnosticSeverity)>,
    point_diagnostics: Vec<PointDiagnostic>,
    len: usize,
    pub(crate) width: Pixels,
    font_size: Pixels,
}

enum LineFragment {
    Text(ShapedLine),
    Element { id: ChunkRendererId, element: Option<AnyElement>, size: Size<Pixels>, len: usize },
}

impl fmt::Debug for LineFragment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LineFragment::Text(line) => f.debug_tuple("Text").field(line).finish(),
            LineFragment::Element { size, len, .. } => {
                f.debug_struct("Element").field("size", size).field("len", len).finish()
            }
        }
    }
}

impl LineWithInvisibles {
    fn from_chunks<'a>(
        chunks: impl Iterator<Item = HighlightedChunk<'a>>,
        editor_style: &EditorStyle,
        max_line_len: usize,
        max_line_count: usize,
        editor_mode: &EditorMode,
        text_width: Pixels,
        is_row_soft_wrapped: impl Copy + Fn(usize) -> bool,
        bg_segments_per_row: &[Vec<(Range<usize>, Hsla)>],
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<Self> {
        let text_style = &editor_style.text;
        let mut layouts = Vec::with_capacity(max_line_count);
        let mut fragments: SmallVec<[LineFragment; 1]> = SmallVec::new();
        let mut line = String::new();
        // Byte offset into the logical line used to position invisible markers.
        // Unlike `line`, this is not cleared when we flush `shape_line` for
        // mid-line inlays/replacements, so marker offsets stay correct in that case.
        let mut line_byte_offset: usize = 0;
        let mut invisibles = Vec::new();
        let mut diagnostic_underline_severity_ranges = Vec::new();
        let mut width = Pixels::ZERO;
        let mut len = 0;
        let mut styles = Vec::new();
        let mut non_whitespace_added = false;
        let mut row = 0;
        let mut line_exceeded_max_len = false;
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let min_contrast = EditorSettings::get_global(cx).minimum_contrast_for_highlights;

        let ellipsis = SharedString::from("⋯");

        for highlighted_chunk in chunks.chain([HighlightedChunk {
            text: "\n",
            style: None,
            diagnostic_underline_severity: None,
            is_tab: false,
            is_inlay: false,
            replacement: None,
        }]) {
            if let Some(replacement) = highlighted_chunk.replacement {
                if line_exceeded_max_len {
                    continue;
                }

                if len + line.len() + highlighted_chunk.text.len() > max_line_len {
                    line_exceeded_max_len = true;
                    continue;
                }

                if !line.is_empty() {
                    let segments = bg_segments_per_row.get(row).map(|v| &v[..]).unwrap_or(&[]);
                    let text_runs: &[TextRun] = if segments.is_empty() {
                        &styles
                    } else {
                        &ensure_highlight_contrast(&styles, segments, min_contrast, len)
                    };
                    let shaped_line = window.text_system().shape_line(line.as_str().into(), font_size, text_runs, None);
                    width += shaped_line.width;
                    len += shaped_line.len;

                    fragments.push(LineFragment::Text(shaped_line));
                    line.clear();
                    styles.clear();
                }

                match replacement {
                    ChunkReplacement::Renderer(renderer) => {
                        let available_width = if renderer.constrain_width {
                            let chunk = if highlighted_chunk.text == ellipsis.as_ref() {
                                ellipsis.clone()
                            } else {
                                SharedString::from(Arc::from(highlighted_chunk.text))
                            };
                            let shaped_line = window.text_system().shape_line(
                                chunk,
                                font_size,
                                &[text_style.to_run(highlighted_chunk.text.len())],
                                None,
                            );
                            AvailableSpace::Definite(shaped_line.width)
                        } else {
                            AvailableSpace::MinContent
                        };

                        let mut element =
                            (renderer.render)(&mut ChunkRendererContext { context: cx, window, max_width: text_width });
                        let line_height = text_style.line_height_in_pixels(window.rem_size());
                        let size = element.layout_as_root(
                            size(available_width, AvailableSpace::Definite(line_height)),
                            window,
                            cx,
                        );

                        width += size.width;
                        len += highlighted_chunk.text.len();
                        line_byte_offset += highlighted_chunk.text.len();
                        fragments.push(LineFragment::Element {
                            id: renderer.id,
                            element: Some(element),
                            size,
                            len: highlighted_chunk.text.len(),
                        });
                    }
                    ChunkReplacement::Str(x) => {
                        let text_style = if let Some(style) = highlighted_chunk.style {
                            Cow::Owned(text_style.clone().highlight(style))
                        } else {
                            Cow::Borrowed(text_style)
                        };

                        let run = TextRun {
                            len: x.len(),
                            font: text_style.font(),
                            color: text_style.color,
                            background_color: text_style.background_color,
                            underline: text_style.underline,
                            strikethrough: text_style.strikethrough,
                        };

                        let line_layout = window
                            .text_system()
                            .shape_line(x, font_size, &[run], None)
                            .with_len(highlighted_chunk.text.len());

                        if let Some(severity) = highlighted_chunk.diagnostic_underline_severity {
                            diagnostic_underline_severity_ranges
                                .push((line_byte_offset..line_byte_offset + highlighted_chunk.text.len(), severity));
                        }
                        width += line_layout.width;
                        len += highlighted_chunk.text.len();
                        line_byte_offset += highlighted_chunk.text.len();
                        fragments.push(LineFragment::Text(line_layout))
                    }
                }
            } else {
                for (ix, mut line_chunk) in highlighted_chunk.text.split('\n').enumerate() {
                    if ix > 0 {
                        let segments = bg_segments_per_row.get(row).map(|v| &v[..]).unwrap_or(&[]);
                        let text_runs = if segments.is_empty() {
                            &styles
                        } else {
                            &ensure_highlight_contrast(&styles, segments, min_contrast, len)
                        };
                        let shaped_line =
                            window.text_system().shape_line(line.clone().into(), font_size, text_runs, None);
                        width += shaped_line.width;
                        len += shaped_line.len;

                        fragments.push(LineFragment::Text(shaped_line));
                        layouts.push(Self {
                            width: mem::take(&mut width),
                            len: mem::take(&mut len),
                            fragments: mem::take(&mut fragments),
                            invisibles: std::mem::take(&mut invisibles),
                            diagnostic_underline_severity_ranges: mem::take(&mut diagnostic_underline_severity_ranges),
                            point_diagnostics: Vec::new(),
                            font_size,
                        });

                        line.clear();
                        line_byte_offset = 0;
                        styles.clear();
                        row += 1;
                        line_exceeded_max_len = false;
                        non_whitespace_added = false;
                        if row == max_line_count {
                            return layouts;
                        }
                    }

                    if !line_chunk.is_empty() && !line_exceeded_max_len {
                        let text_style = if let Some(style) = highlighted_chunk.style {
                            Cow::Owned(text_style.clone().highlight(style))
                        } else {
                            Cow::Borrowed(text_style)
                        };

                        let current_line_len = len + line.len();
                        if current_line_len + line_chunk.len() > max_line_len {
                            let mut chunk_len = max_line_len - current_line_len;
                            while !line_chunk.is_char_boundary(chunk_len) {
                                chunk_len -= 1;
                            }
                            line_chunk = &line_chunk[..chunk_len];
                            line_exceeded_max_len = true;
                        }

                        if line_chunk.is_empty() {
                            continue;
                        }

                        styles.push(TextRun {
                            len: line_chunk.len(),
                            font: text_style.font(),
                            color: text_style.color,
                            background_color: text_style.background_color,
                            underline: text_style.underline,
                            strikethrough: text_style.strikethrough,
                        });

                        if let Some(severity) = highlighted_chunk.diagnostic_underline_severity {
                            diagnostic_underline_severity_ranges
                                .push((line_byte_offset..line_byte_offset + line_chunk.len(), severity));
                        }

                        if editor_mode.is_full() && !highlighted_chunk.is_inlay {
                            // Line wrap pads its contents with fake whitespaces,
                            // avoid printing them
                            let is_soft_wrapped = is_row_soft_wrapped(row);
                            if highlighted_chunk.is_tab {
                                if non_whitespace_added || !is_soft_wrapped {
                                    invisibles.push(Invisible::Tab {
                                        line_start_offset: line_byte_offset,
                                        line_end_offset: line_byte_offset + line_chunk.len(),
                                    });
                                }
                            } else {
                                invisibles.extend(line_chunk.char_indices().filter_map(|(index, c)| {
                                    let is_whitespace = c.is_whitespace();
                                    non_whitespace_added |= !is_whitespace;
                                    if is_whitespace && (non_whitespace_added || !is_soft_wrapped) {
                                        Some(Invisible::Whitespace {
                                            line_start_offset: line_byte_offset + index,
                                            line_end_offset: line_byte_offset + index + c.len_utf8(),
                                        })
                                    } else {
                                        None
                                    }
                                }))
                            }
                        }

                        line.push_str(line_chunk);
                        line_byte_offset += line_chunk.len();
                    }
                }
            }
        }

        layouts
    }

    fn add_point_diagnostic(&mut self, point_diagnostic: PointDiagnostic) {
        if let Some(existing) =
            self.point_diagnostics.iter_mut().find(|existing| existing.column == point_diagnostic.column)
        {
            if point_diagnostic.severity < existing.severity {
                *existing = point_diagnostic;
            }
        } else {
            self.point_diagnostics.push(point_diagnostic);
        }
    }

    /// Takes text runs and non-overlapping left-to-right background ranges with color.
    /// Returns new text runs with adjusted contrast as per background ranges.
    fn prepaint(
        &mut self,
        line_height: Pixels,
        scroll_position: gpui::Point<ScrollOffset>,
        scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
        row: DisplayRow,
        content_origin: gpui::Point<Pixels>,
        text_align: TextAlign,
        content_width: Pixels,
        line_elements: &mut SmallVec<[AnyElement; 1]>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let line_y = f32::from(line_height) * Pixels::from(row.as_f64() - scroll_position.y);
        let mut fragment_origin = content_origin
            + point(self.alignment_offset(text_align, content_width) - Pixels::from(scroll_pixel_position.x), line_y);
        for fragment in &mut self.fragments {
            match fragment {
                LineFragment::Text(line) => {
                    fragment_origin.x += line.width;
                }
                LineFragment::Element { element, size, .. } => {
                    let mut element = element.take().expect("you can't prepaint LineWithInvisibles twice");

                    // Center the element vertically within the line.
                    let mut element_origin = fragment_origin;
                    element_origin.y += (line_height - size.height) / 2.;
                    element.prepaint_at(element_origin, window, cx);
                    line_elements.push(element);

                    fragment_origin.x += size.width;
                }
            }
        }
    }

    fn draw(
        &self,
        layout: &EditorLayout,
        row: DisplayRow,
        content_origin: gpui::Point<Pixels>,
        whitespace_setting: ShowWhitespaceSetting,
        selection_ranges: &[Range<DisplayPoint>],
        window: &mut Window,
        cx: &mut App,
    ) {
        let line_y = layout.position_map.line_height * (row.as_f64() - layout.position_map.scroll_position.y) as f32;
        let line_height = layout.position_map.line_height;
        let mut fragment_origin = content_origin
            + point(
                self.alignment_offset(layout.text_align, layout.content_width)
                    - Pixels::from(layout.position_map.scroll_pixel_position.x),
                line_y,
            );
        let mut points = self
            .point_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.column as usize <= self.len)
            .collect::<SmallVec<[_; 4]>>();
        points.sort_unstable_by_key(|diagnostic| diagnostic.column);
        let has_points = !points.is_empty();
        let mut points = points.into_iter().peekable();
        let mut underlines = SmallVec::<[DiagnosticUnderline; 4]>::new();
        let mut diagnostic_ranges = self.diagnostic_underline_severity_ranges.iter().peekable();
        let mut fragment_start = 0;
        let mut end_underline_offset = layout.point_diagnostic_underline_offset;

        for fragment in &self.fragments {
            let (fragment_len, fragment_width, line) = match fragment {
                LineFragment::Text(line) => {
                    if has_points {
                        let mut glyphs = line.runs.iter().flat_map(|run| &run.glyphs).peekable();
                        line.paint_with_underline_handler(
                            fragment_origin,
                            line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                            |range, origin, width, style, window| {
                                let bounds = window.underline_bounds(origin, width, style);
                                if bounds.is_empty() {
                                    return;
                                }
                                let source_range = if line.text.len() == line.len() {
                                    fragment_start + range.start..fragment_start + range.end
                                } else {
                                    fragment_start..fragment_start + line.len()
                                };
                                if source_range.is_empty() {
                                    window.paint_underline(origin, width, style);
                                    return;
                                }
                                let mut start = source_range.start;
                                let mut span_start = bounds.left();
                                while start < source_range.end {
                                    while diagnostic_ranges.peek().is_some_and(|(range, _)| range.end <= start) {
                                        diagnostic_ranges.next();
                                    }
                                    let (end, severity) = match diagnostic_ranges.peek().copied() {
                                        Some((range, severity)) if range.start <= start => {
                                            (range.end.min(source_range.end), Some(*severity))
                                        }
                                        Some((range, _)) => (range.start.min(source_range.end), None),
                                        None => (source_range.end, None),
                                    };
                                    let span_end = if end == source_range.end {
                                        bounds.right()
                                    } else {
                                        let index = end - fragment_start;
                                        while glyphs.peek().is_some_and(|glyph| glyph.index < index) {
                                            glyphs.next();
                                        }
                                        let x = fragment_origin.x
                                            + glyphs.peek().map_or(line.width, |glyph| glyph.position.x);
                                        window
                                            .underline_bounds(point(x, origin.y), Pixels::ZERO, style)
                                            .left()
                                            .clamp(span_start, bounds.right())
                                    };
                                    if span_start < span_end {
                                        if let Some(severity) = severity {
                                            underlines.push(DiagnosticUnderline {
                                                origin,
                                                width,
                                                style: *style,
                                                bounds,
                                                span: span_start..span_end,
                                                severity,
                                                is_point: false,
                                            });
                                        } else {
                                            window.paint_underline_with_exclusions(
                                                origin,
                                                width,
                                                style,
                                                &[bounds.left()..span_start, span_end..bounds.right()],
                                            );
                                        }
                                    }
                                    start = end;
                                    span_start = span_end;
                                }
                            },
                        )
                        .log_err();
                    } else {
                        line.paint(fragment_origin, line_height, TextAlign::Left, None, window, cx).log_err();
                    }
                    (line.len(), line.width, Some(line))
                }
                LineFragment::Element { size, len, .. } => (*len, size.width, None),
            };
            let fragment_end = fragment_start + fragment_len;
            if has_points {
                let mut glyphs = line.into_iter().flat_map(|line| &line.runs).flat_map(|run| &run.glyphs).peekable();
                let has_glyphs = glyphs.peek().is_some();
                let underline_offset =
                    line.filter(|_| has_glyphs).map_or(layout.point_diagnostic_underline_offset, |line| {
                        underline_y_offset(line_height, line.ascent, line.descent)
                    });
                if fragment_len > 0 || fragment_width != Pixels::ZERO || has_glyphs {
                    end_underline_offset = underline_offset;
                }
                while points.peek().is_some_and(|diagnostic| (diagnostic.column as usize) < fragment_end) {
                    let Some(diagnostic) = points.next() else {
                        break;
                    };
                    let index = diagnostic.column as usize - fragment_start;
                    let x = if let Some(line) = line {
                        let index = if line.text.len() == line.len() { index } else { 0 };
                        while glyphs.peek().is_some_and(|glyph| glyph.index < index) {
                            glyphs.next();
                        }
                        glyphs.peek().map_or(line.width, |glyph| glyph.position.x)
                    } else {
                        Pixels::ZERO
                    };
                    underlines.push(DiagnosticUnderline::point(
                        diagnostic,
                        fragment_origin + point(x, underline_offset),
                        layout.position_map.em_advance,
                        window,
                    ));
                }
            }
            fragment_origin.x += fragment_width;
            fragment_start = fragment_end;
        }
        for diagnostic in points {
            underlines.push(DiagnosticUnderline::point(
                diagnostic,
                fragment_origin + point(Pixels::ZERO, end_underline_offset),
                layout.position_map.em_advance,
                window,
            ));
        }

        self.draw_invisibles(
            selection_ranges,
            layout,
            content_origin,
            line_y,
            row,
            line_height,
            whitespace_setting,
            window,
            cx,
        );
        DiagnosticUnderline::paint_all(&underlines, window);
    }

    fn draw_background(
        &self,
        layout: &EditorLayout,
        row: DisplayRow,
        content_origin: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let line_height = layout.position_map.line_height;
        let line_y = line_height * (row.as_f64() - layout.position_map.scroll_position.y) as f32;

        let mut fragment_origin = content_origin
            + point(
                self.alignment_offset(layout.text_align, layout.content_width)
                    - Pixels::from(layout.position_map.scroll_pixel_position.x),
                line_y,
            );

        for fragment in &self.fragments {
            match fragment {
                LineFragment::Text(line) => {
                    line.paint_background(fragment_origin, line_height, TextAlign::Left, None, window, cx).log_err();
                    fragment_origin.x += line.width;
                }
                LineFragment::Element { size, .. } => {
                    fragment_origin.x += size.width;
                }
            }
        }
    }

    fn draw_invisibles(
        &self,
        selection_ranges: &[Range<DisplayPoint>],
        layout: &EditorLayout,
        content_origin: gpui::Point<Pixels>,
        line_y: Pixels,
        row: DisplayRow,
        line_height: Pixels,
        whitespace_setting: ShowWhitespaceSetting,
        window: &mut Window,
        cx: &mut App,
    ) {
        let line_origin = content_origin
            + point(
                self.alignment_offset(layout.text_align, layout.content_width)
                    - Pixels::from(layout.position_map.scroll_pixel_position.x),
                line_y,
            );
        let extract_whitespace_info = |invisible: &Invisible| {
            let (token_offset, token_end_offset, invisible_symbol) = match invisible {
                Invisible::Tab { line_start_offset, line_end_offset } => {
                    (*line_start_offset, *line_end_offset, &layout.tab_invisible)
                }
                Invisible::Whitespace { line_start_offset, line_end_offset } => {
                    (*line_start_offset, *line_end_offset, &layout.space_invisible)
                }
            };

            let token_x = self.x_for_index(token_offset);
            // Center the marker inside the actual glyph's width so it lines up with
            // proportional fonts instead of assuming a monospace `em_width` cell.
            let glyph_width = (self.x_for_index(token_end_offset) - token_x).max(Pixels::ZERO);
            let invisible_offset = (glyph_width - invisible_symbol.width).max(Pixels::ZERO) / 2.0;
            let origin = line_origin + point(token_x + invisible_offset, Pixels::ZERO);

            (
                [token_offset, token_end_offset],
                Box::new(move |window: &mut Window, cx: &mut App| {
                    invisible_symbol.paint(origin, line_height, TextAlign::Left, None, window, cx).log_err();
                }),
            )
        };

        let invisible_iter = self.invisibles.iter().map(extract_whitespace_info);
        match whitespace_setting {
            ShowWhitespaceSetting::None => (),
            ShowWhitespaceSetting::All => invisible_iter.for_each(|(_, paint)| paint(window, cx)),
            ShowWhitespaceSetting::Selection => invisible_iter.for_each(|([start, _], paint)| {
                let invisible_point = DisplayPoint::new(row, start as u32);
                if !selection_ranges
                    .iter()
                    .any(|region| region.start <= invisible_point && invisible_point < region.end)
                {
                    return;
                }

                paint(window, cx);
            }),

            ShowWhitespaceSetting::Trailing => {
                let mut previous_start = self.len;
                for ([start, end], paint) in invisible_iter.rev() {
                    if previous_start != end {
                        break;
                    }
                    previous_start = start;
                    paint(window, cx);
                }
            }

            // For a whitespace to be on a boundary, any of the following conditions need to be met:
            // - It is a tab
            // - It is adjacent to an edge (start or end)
            // - It is adjacent to a whitespace (left or right)
            ShowWhitespaceSetting::Boundary => {
                // We'll need to keep track of the last invisible we've seen and then check if we are adjacent to it for some of
                // the above cases.
                // Note: We zip in the original `invisibles` to check for tab equality
                let mut last_seen: Option<(bool, usize, Box<dyn Fn(&mut Window, &mut App)>)> = None;
                for (([start, end], paint), invisible) in invisible_iter.zip_eq(self.invisibles.iter()) {
                    let should_render = match (&last_seen, invisible) {
                        (_, Invisible::Tab { .. }) => true,
                        (Some((_, last_end, _)), _) => *last_end == start,
                        _ => false,
                    };

                    if should_render || start == 0 || end == self.len {
                        paint(window, cx);

                        // Since we are scanning from the left, we will skip over the first available whitespace that is part
                        // of a boundary between non-whitespace segments, so we correct by manually redrawing it if needed.
                        if let Some((should_render_last, last_end, paint_last)) = last_seen {
                            // Note that we need to make sure that the last one is actually adjacent
                            if !should_render_last && last_end == start {
                                paint_last(window, cx);
                            }
                        }
                    }

                    // Manually render anything within a selection
                    let invisible_point = DisplayPoint::new(row, start as u32);
                    if selection_ranges
                        .iter()
                        .any(|region| region.start <= invisible_point && invisible_point < region.end)
                    {
                        paint(window, cx);
                    }

                    last_seen = Some((should_render, end, paint));
                }
            }
        }
    }

    pub fn x_for_index(&self, index: usize) -> Pixels {
        let mut fragment_start_x = Pixels::ZERO;
        let mut fragment_start_index = 0;

        for fragment in &self.fragments {
            match fragment {
                LineFragment::Text(shaped_line) => {
                    let fragment_end_index = fragment_start_index + shaped_line.len;
                    if index < fragment_end_index {
                        return fragment_start_x + shaped_line.x_for_index(index - fragment_start_index);
                    }
                    fragment_start_x += shaped_line.width;
                    fragment_start_index = fragment_end_index;
                }
                LineFragment::Element { len, size, .. } => {
                    let fragment_end_index = fragment_start_index + len;
                    if index < fragment_end_index {
                        return fragment_start_x;
                    }
                    fragment_start_x += size.width;
                    fragment_start_index = fragment_end_index;
                }
            }
        }

        fragment_start_x
    }

    pub fn index_for_x(&self, x: Pixels) -> Option<usize> {
        let mut fragment_start_x = Pixels::ZERO;
        let mut fragment_start_index = 0;

        for fragment in &self.fragments {
            match fragment {
                LineFragment::Text(shaped_line) => {
                    let fragment_end_x = fragment_start_x + shaped_line.width;
                    if x < fragment_end_x {
                        return Some(fragment_start_index + shaped_line.index_for_x(x - fragment_start_x)?);
                    }
                    fragment_start_x = fragment_end_x;
                    fragment_start_index += shaped_line.len;
                }
                LineFragment::Element { len, size, .. } => {
                    let fragment_end_x = fragment_start_x + size.width;
                    if x < fragment_end_x {
                        return Some(fragment_start_index);
                    }
                    fragment_start_index += len;
                    fragment_start_x = fragment_end_x;
                }
            }
        }

        None
    }

    pub fn font_id_for_index(&self, index: usize) -> Option<FontId> {
        let mut fragment_start_index = 0;

        for fragment in &self.fragments {
            match fragment {
                LineFragment::Text(shaped_line) => {
                    let fragment_end_index = fragment_start_index + shaped_line.len;
                    if index < fragment_end_index {
                        return shaped_line.font_id_for_index(index - fragment_start_index);
                    }
                    fragment_start_index = fragment_end_index;
                }
                LineFragment::Element { len, .. } => {
                    let fragment_end_index = fragment_start_index + len;
                    if index < fragment_end_index {
                        return None;
                    }
                    fragment_start_index = fragment_end_index;
                }
            }
        }

        None
    }

    pub fn alignment_offset(&self, text_align: TextAlign, content_width: Pixels) -> Pixels {
        let line_width = self.width;
        match text_align {
            TextAlign::Left => px(0.0),
            TextAlign::Center => (content_width - line_width) / 2.0,
            TextAlign::Right => content_width - line_width,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Invisible {
    /// A tab character
    ///
    /// A tab character is internally represented by spaces (configured by the user's tab width)
    /// aligned to the nearest column, so it's necessary to store the start and end offset for
    /// adjacency checks.
    Tab { line_start_offset: usize, line_end_offset: usize },
    /// A whitespace character (ASCII space or any other Unicode whitespace).
    ///
    /// Storing both offsets correctly accounts for multi-byte whitespace characters
    /// such as U+00A0 NO-BREAK SPACE, keeping adjacency checks correct.
    Whitespace { line_start_offset: usize, line_end_offset: usize },
}

impl EditorElement {
    /// Returns the rem size to use when rendering the [`EditorElement`].
    ///
    /// This allows UI elements to scale based on the `buffer_font_size`.
    fn rem_size(&self, cx: &App) -> Option<Pixels> {
        match self.editor.read(cx).mode {
            EditorMode::Full { scale_ui_elements_with_buffer_font_size: true, .. } | EditorMode::Minimap { .. } => {
                let buffer_font_size = self.style.text.font_size;
                match buffer_font_size {
                    AbsoluteLength::Pixels(pixels) => {
                        let rem_size_scale = {
                            // Our default UI font size is 14px on a 16px base scale.
                            // This means the default UI font size is 0.875rems.
                            let default_font_size_scale = 14. / ui::BASE_REM_SIZE_IN_PX;

                            // We then determine the delta between a single rem and the default font
                            // size scale.
                            let default_font_size_delta = 1. - default_font_size_scale;

                            // Finally, we add this delta to 1rem to get the scale factor that
                            // should be used to scale up the UI.
                            1. + default_font_size_delta
                        };

                        Some(pixels * rem_size_scale)
                    }
                    AbsoluteLength::Rems(rems) => Some(rems.to_pixels(ui::BASE_REM_SIZE_IN_PX.into())),
                }
            }
            // We currently use single-line and auto-height editors in UI contexts,
            // so we don't want to scale everything with the buffer font size, as it
            // ends up looking off.
            _ => None,
        }
    }

    fn editor_with_selections(&self, cx: &App) -> Option<Entity<Editor>> {
        if let EditorMode::Minimap { parent } = self.editor.read(cx).mode() {
            parent.upgrade()
        } else {
            Some(self.editor.clone())
        }
    }
}

#[derive(Default)]
pub struct EditorRequestLayoutState {
    // We use prepaint depth to limit the number of times prepaint is
    // called recursively. We need this so that we can update stale
    // data for e.g. block heights in block map.
    prepaint_depth: Rc<Cell<usize>>,
}

impl EditorRequestLayoutState {
    // In ideal conditions we only need one more subsequent prepaint call for resize to take effect.
    // i.e. MAX_PREPAINT_DEPTH = 2, but placing near blocks can expose more lines from below, and
    // we end up querying blocks for those lines too in subsequent renders.
    // Setting MAX_PREPAINT_DEPTH = 3, passes all tests. Just to be on the safe side we set it to 5, so
    // that subsequent shrinking does not lead to incorrect block placing.
    const MAX_PREPAINT_DEPTH: usize = 5;

    fn increment_prepaint_depth(&self) -> EditorPrepaintGuard {
        let depth = self.prepaint_depth.get();
        self.prepaint_depth.set(depth + 1);
        EditorPrepaintGuard { prepaint_depth: self.prepaint_depth.clone() }
    }

    fn has_remaining_prepaint_depth(&self) -> bool {
        self.prepaint_depth.get() < Self::MAX_PREPAINT_DEPTH
    }
}

struct EditorPrepaintGuard {
    prepaint_depth: Rc<Cell<usize>>,
}

impl Drop for EditorPrepaintGuard {
    fn drop(&mut self) {
        let depth = self.prepaint_depth.get();
        self.prepaint_depth.set(depth.saturating_sub(1));
    }
}

impl Element for EditorElement {
    type RequestLayoutState = EditorRequestLayoutState;
    type PrepaintState = EditorLayout;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let rem_size = self.rem_size(cx);
        window.with_rem_size(rem_size, |window| {
            self.editor.update(cx, |editor, cx| {
                editor.set_style(self.style.clone(), window, cx);

                let layout_id = match editor.mode {
                    EditorMode::SingleLine => {
                        let rem_size = window.rem_size();
                        let height = self.style.text.line_height_in_pixels(rem_size);
                        let mut style = Style::default();
                        style.size.height = height.into();
                        style.size.width = relative(1.).into();
                        window.request_layout(style, None, cx)
                    }
                    EditorMode::AutoHeight { min_lines, max_lines } => {
                        let editor_handle = cx.entity();
                        window.request_measured_layout(
                            Style::default(),
                            move |known_dimensions, available_space, window, cx| {
                                editor_handle
                                    .update(cx, |editor, cx| {
                                        compute_auto_height_layout(
                                            editor,
                                            min_lines,
                                            max_lines,
                                            known_dimensions,
                                            available_space.width,
                                            window,
                                            cx,
                                        )
                                    })
                                    .unwrap_or_default()
                            },
                        )
                    }
                    EditorMode::Minimap { .. } => {
                        let mut style = Style::default();
                        style.size.width = relative(1.).into();
                        style.size.height = relative(1.).into();
                        window.request_layout(style, None, cx)
                    }
                    EditorMode::Full { sizing_behavior, .. } => {
                        let mut style = Style::default();
                        style.size.width = relative(1.).into();
                        if sizing_behavior == SizingBehavior::SizeByContent {
                            let snapshot = editor.snapshot(window, cx);
                            let line_height = self.style.text.line_height_in_pixels(window.rem_size());
                            let scroll_height = (snapshot.max_point().row().next_row().0 as f32) * line_height;
                            style.size.height = scroll_height.into();
                        } else {
                            style.size.height = relative(1.).into();
                        }
                        window.request_layout(style, None, cx)
                    }
                };

                (layout_id, EditorRequestLayoutState::default())
            })
        })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let _prepaint_depth_guard = request_layout.increment_prepaint_depth();
        let text_style = TextStyleRefinement {
            font_size: Some(self.style.text.font_size),
            line_height: Some(self.style.text.line_height),
            ..Default::default()
        };

        let is_minimap = self.editor.read(cx).mode.is_minimap();

        if !is_minimap {
            let focus_handle = self.editor.focus_handle(cx);
            window.set_view_id(self.editor.entity_id());
            window.set_focus_handle(&focus_handle, cx);
        }

        let rem_size = self.rem_size(cx);
        window.with_rem_size(rem_size, |window| {
            window.with_text_style(Some(text_style), |window| {
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let (mut snapshot, is_read_only) =
                        self.editor.update(cx, |editor, cx| (editor.snapshot(window, cx), editor.read_only(cx)));
                    let style = &self.style;

                    let rem_size = window.rem_size();
                    let font_id = window.text_system().resolve_font(&style.text.font());
                    let font_size = style.text.font_size.to_pixels(rem_size);
                    let line_height = style.text.line_height_in_pixels(rem_size);
                    let ascent = window.text_system().ascent(font_id, font_size);
                    let descent = window.text_system().descent(font_id, font_size).abs();
                    let point_diagnostic_underline_offset = underline_y_offset(line_height, ascent, descent);
                    let em_width = window.text_system().em_width(font_id, font_size).unwrap();
                    let em_advance = window.text_system().em_advance(font_id, font_size).unwrap();
                    let em_layout_width = window.text_system().em_layout_width(font_id, font_size);
                    let glyph_grid_cell = size(em_advance, line_height);

                    let gutter_dimensions = snapshot.gutter_dimensions(font_id, font_size, style, window, cx);
                    let text_width = bounds.size.width - gutter_dimensions.width;

                    let settings = EditorSettings::get_global(cx);
                    let minimap_width = self
                        .get_minimap_width(&settings.minimap, text_width, em_width, font_size, rem_size, cx)
                        .unwrap_or_default();

                    let right_margin = minimap_width;

                    let extended_right = 2 * em_width + right_margin;
                    let editor_width = text_width - gutter_dimensions.margin - extended_right;
                    let editor_margins =
                        EditorMargins { gutter: gutter_dimensions, right: right_margin, extended_right };

                    snapshot = self.editor.update(cx, |editor, cx| {
                        editor.last_bounds = Some(bounds);
                        editor.gutter_dimensions = gutter_dimensions;
                        editor.set_visible_line_count((bounds.size.height / line_height) as f64, window, cx);
                        editor.set_visible_column_count(f64::from(editor_width / em_advance));

                        if matches!(editor.mode, EditorMode::AutoHeight { .. } | EditorMode::Minimap { .. }) {
                            snapshot
                        } else {
                            let wrap_width =
                                calculate_wrap_width(editor.soft_wrap_mode(cx), editor_width, em_layout_width);

                            if editor.set_wrap_width(wrap_width, cx) { editor.snapshot(window, cx) } else { snapshot }
                        }
                    });

                    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                    let gutter_hitbox =
                        window.insert_hitbox(gutter_bounds(bounds, gutter_dimensions), HitboxBehavior::Normal);
                    let text_hitbox = window.insert_hitbox(
                        Bounds { origin: gutter_hitbox.top_right(), size: size(text_width, bounds.size.height) },
                        HitboxBehavior::Normal,
                    );

                    // Offset the content_bounds from the text_bounds by the gutter margin (which
                    // is roughly half a character wide) to make hit testing work more like how we want.
                    let content_offset = point(editor_margins.gutter.margin, Pixels::ZERO);
                    let content_origin = text_hitbox.origin + content_offset;

                    let height_in_lines = f64::from(bounds.size.height / line_height);
                    let max_row = snapshot.max_point().row().as_f64();

                    // Calculate how much of the editor is clipped by parent containers (e.g., List).
                    // This allows us to only render lines that are actually visible, which is
                    // critical for performance when large content-sized editors are inside Lists.
                    let visible_bounds = window.content_mask().bounds;
                    let visible_top = bounds.top().max(visible_bounds.top());
                    let visible_bottom = bounds.bottom().min(visible_bounds.bottom());
                    let clipped_top = (visible_top - bounds.top()).max(px(0.));
                    let visible_height = (visible_bottom - visible_top).max(px(0.));
                    let clipped_top_in_lines = f64::from(clipped_top / line_height);
                    let visible_height_in_lines = f64::from(visible_height / line_height);

                    // The max scroll position for the top of the window
                    let scroll_beyond_last_line = self.editor.read(cx).scroll_beyond_last_line(cx);
                    let max_scroll_top = match scroll_beyond_last_line {
                        ScrollBeyondLastLine::OnePage => max_row,
                        ScrollBeyondLastLine::Off => (max_row - height_in_lines + 1.).max(0.),
                        ScrollBeyondLastLine::VerticalScrollMargin => {
                            let settings = EditorSettings::get_global(cx);
                            (max_row - height_in_lines + 1. + settings.vertical_scroll_margin).max(0.)
                        }
                    };

                    let (autoscroll_request, autoscroll_containing_element, needs_horizontal_autoscroll) =
                        self.editor.update(cx, |editor, cx| {
                            let autoscroll_request = editor.scroll_manager.take_autoscroll_request();

                            let autoscroll_containing_element =
                                autoscroll_request.is_some() || editor.has_pending_selection();

                            let (needs_horizontal_autoscroll, was_scrolled) = editor.autoscroll_vertically(
                                bounds,
                                line_height,
                                max_scroll_top,
                                autoscroll_request,
                                window,
                                cx,
                            );
                            if was_scrolled.0 {
                                snapshot = editor.snapshot(window, cx);
                            }
                            (autoscroll_request, autoscroll_containing_element, needs_horizontal_autoscroll)
                        });

                    let mut scroll_position = snapshot.scroll_position();
                    if !line_height.is_zero() {
                        scroll_position.y =
                            window.pixel_snap_f64(scroll_position.y * f64::from(line_height)) / f64::from(line_height);
                    }
                    // The scroll position is a fractional point, the whole number of which represents
                    // the top of the window in terms of display rows.
                    // We add clipped_top_in_lines to skip rows that are clipped by parent containers,
                    // but we don't modify scroll_position itself since the parent handles positioning.
                    let max_row = snapshot.max_point().row();
                    let start_row =
                        cmp::min(DisplayRow((scroll_position.y + clipped_top_in_lines).floor() as u32), max_row);
                    let end_row = cmp::min(
                        (scroll_position.y + clipped_top_in_lines + visible_height_in_lines).ceil() as u32,
                        max_row.next_row().0,
                    );
                    let end_row = DisplayRow(end_row);

                    let row_infos = snapshot // note we only get the visual range
                        .row_infos(start_row)
                        .take((start_row..end_row).len())
                        .collect::<Vec<RowInfo>>();
                    let is_row_soft_wrapped =
                        |row: usize| row_infos.get(row).is_none_or(|info| info.buffer_row.is_none());

                    let start_anchor = if start_row == Default::default() {
                        Anchor::Min
                    } else {
                        snapshot
                            .buffer_snapshot()
                            .anchor_before(DisplayPoint::new(start_row, 0).to_offset(&snapshot, Bias::Left))
                    };
                    let end_anchor = if end_row > max_row {
                        Anchor::Max
                    } else {
                        snapshot
                            .buffer_snapshot()
                            .anchor_before(DisplayPoint::new(end_row, 0).to_offset(&snapshot, Bias::Right))
                    };

                    let mut highlighted_rows = self.editor.read(cx).highlighted_display_rows_in_range(
                        start_anchor..end_anchor,
                        start_row..end_row,
                        &snapshot.display_snapshot,
                        cx,
                    );

                    let mut highlighted_ranges = self
                        .editor_with_selections(cx)
                        .map(|editor| {
                            if editor == self.editor {
                                editor.read(cx).background_highlights_in_range(
                                    start_anchor..end_anchor,
                                    &snapshot.display_snapshot,
                                    cx.theme(),
                                )
                            } else {
                                editor.update(cx, |editor, cx| {
                                    let snapshot = editor.snapshot(window, cx);
                                    let start_anchor = if start_row == Default::default() {
                                        Anchor::Min
                                    } else {
                                        snapshot.buffer_snapshot().anchor_before(
                                            DisplayPoint::new(start_row, 0).to_offset(&snapshot, Bias::Left),
                                        )
                                    };
                                    let end_anchor = if end_row > max_row {
                                        Anchor::Max
                                    } else {
                                        snapshot.buffer_snapshot().anchor_before(
                                            DisplayPoint::new(end_row, 0).to_offset(&snapshot, Bias::Right),
                                        )
                                    };

                                    editor.background_highlights_in_range(
                                        start_anchor..end_anchor,
                                        &snapshot.display_snapshot,
                                        cx.theme(),
                                    )
                                })
                            }
                        })
                        .unwrap_or_default();

                    struct DiffHunkHighlightColors {
                        filled_background: Hsla,
                        hollow_background: Hsla,
                        hollow_border: Hsla,
                    }

                    let colors = cx.theme().colors();
                    let added_diff_hunk_colors = DiffHunkHighlightColors {
                        filled_background: colors.editor_diff_hunk_added_background,
                        hollow_background: colors.editor_diff_hunk_added_hollow_background,
                        hollow_border: colors.editor_diff_hunk_added_hollow_border,
                    };
                    let deleted_diff_hunk_colors = DiffHunkHighlightColors {
                        filled_background: colors.editor_diff_hunk_deleted_background,
                        hollow_background: colors.editor_diff_hunk_deleted_hollow_background,
                        hollow_border: colors.editor_diff_hunk_deleted_hollow_border,
                    };

                    for (ix, row_info) in row_infos.iter().enumerate() {
                        let Some(diff_status) = row_info.diff_status else {
                            continue;
                        };

                        let diff_hunk_colors = match diff_status.kind {
                            DiffHunkStatusKind::Added => &added_diff_hunk_colors,
                            DiffHunkStatusKind::Deleted => &deleted_diff_hunk_colors,
                            DiffHunkStatusKind::Modified => {
                                debug_panic!("modified diff status for row info");
                                continue;
                            }
                        };

                        let hollow_highlight = LineHighlight {
                            background: diff_hunk_colors.hollow_background.into(),
                            border: Some(diff_hunk_colors.hollow_border),
                            include_gutter: true,
                            type_id: None,
                        };

                        let filled_highlight = LineHighlight {
                            background: solid_background(diff_hunk_colors.filled_background),
                            border: None,
                            include_gutter: true,
                            type_id: None,
                        };

                        let background =
                            if self.diff_hunk_hollow(diff_status, cx) { hollow_highlight } else { filled_highlight };

                        let base_display_point = DisplayPoint::new(start_row + DisplayRow(ix as u32), 0);

                        highlighted_rows.entry(base_display_point.row()).or_insert(background);
                    }

                    let highlighted_gutter_ranges = self.editor.read(cx).gutter_highlights_in_range(
                        start_anchor..end_anchor,
                        &snapshot.display_snapshot,
                        cx,
                    );

                    let document_colors =
                        self.editor.read(cx).colors.as_ref().map(|colors| colors.editor_display_highlights(&snapshot));

                    let (local_selections, selected_buffer_ids, latest_selection_anchors): (
                        Vec<Selection<Point>>,
                        Vec<BufferId>,
                        HashMap<BufferId, Anchor>,
                    ) = self
                        .editor_with_selections(cx)
                        .map(|editor| {
                            editor.update(cx, |editor, cx| {
                                let is_singleton = editor.buffer_kind(cx) == ItemBufferKind::Singleton;

                                // Singleton buffers only need the newest selection anchor here.
                                let selected_buffer_ids = if is_singleton {
                                    Vec::new()
                                } else {
                                    let all_selections = editor.selections.all::<Point>(&snapshot.display_snapshot);
                                    let mut selected_buffer_ids = Vec::with_capacity(all_selections.len());

                                    for selection in all_selections {
                                        for buffer_id in
                                            snapshot.buffer_snapshot().buffer_ids_for_range(selection.range())
                                        {
                                            if selected_buffer_ids.last() != Some(&buffer_id) {
                                                selected_buffer_ids.push(buffer_id);
                                            }
                                        }
                                    }

                                    selected_buffer_ids
                                };

                                let mut selections = editor
                                    .selections
                                    .disjoint_in_row_range(start_anchor..end_anchor, &snapshot.display_snapshot);
                                selections.extend(editor.selections.pending(&snapshot.display_snapshot));

                                let latest_selection_anchors: HashMap<BufferId, Anchor> = if is_singleton {
                                    let head = editor.selections.newest_anchor().head();
                                    snapshot
                                        .buffer_snapshot()
                                        .anchor_to_buffer_anchor(head)
                                        .map(|(text_anchor, _)| (text_anchor.buffer_id, head))
                                        .into_iter()
                                        .collect()
                                } else {
                                    let all_anchor_selections =
                                        editor.selections.all_anchors(&snapshot.display_snapshot);
                                    let mut anchors_by_buffer: HashMap<BufferId, (usize, Anchor)> = HashMap::default();
                                    for selection in all_anchor_selections.iter() {
                                        let head = selection.head();
                                        if let Some((text_anchor, _)) =
                                            snapshot.buffer_snapshot().anchor_to_buffer_anchor(head)
                                        {
                                            anchors_by_buffer
                                                .entry(text_anchor.buffer_id)
                                                .and_modify(|(latest_id, latest_anchor)| {
                                                    if selection.id > *latest_id {
                                                        *latest_id = selection.id;
                                                        *latest_anchor = head;
                                                    }
                                                })
                                                .or_insert((selection.id, head));
                                        }
                                    }
                                    anchors_by_buffer
                                        .into_iter()
                                        .map(|(buffer_id, (_, anchor))| (buffer_id, anchor))
                                        .collect()
                                };

                                (selections, selected_buffer_ids, latest_selection_anchors)
                            })
                        })
                        .unwrap_or_else(|| (Vec::new(), Vec::new(), HashMap::default()));

                    let (selections, active_rows, newest_selection_head) = self.layout_selections(
                        start_anchor,
                        end_anchor,
                        &local_selections,
                        &snapshot,
                        start_row,
                        end_row,
                        window,
                        cx,
                    );

                    let current_selection_head = self.editor.update(cx, |editor, cx| {
                        (editor.selections.count() != 0).then(|| {
                            let newest = editor.selections.newest::<Point>(&editor.display_snapshot(cx));

                            SelectionLayout::new(
                                newest,
                                editor.selections.line_mode(),
                                editor.cursor_offset_on_selection,
                                editor.cursor_shape,
                                &snapshot,
                                true,
                                true,
                                None,
                            )
                            .head
                            .row()
                        })
                    });

                    let run_indicator_rows = self
                        .editor
                        .update(cx, |editor, cx| editor.active_run_indicators(start_row..end_row, window, cx));

                    let gutter = Gutter {
                        line_height,
                        range: start_row..end_row,
                        scroll_position,
                        dimensions: &gutter_dimensions,
                        hitbox: &gutter_hitbox,
                        snapshot: &snapshot,
                        row_infos: &row_infos,
                    };

                    let line_numbers = self.layout_line_numbers(&gutter, &active_rows, window, cx);

                    let mut expand_toggles = window.with_element_namespace("expand_toggles", |window| {
                        self.layout_expand_toggles(
                            &gutter_hitbox,
                            gutter_dimensions,
                            em_width,
                            line_height,
                            scroll_position,
                            start_row,
                            &row_infos,
                            window,
                            cx,
                        )
                    });

                    let mut crease_toggles = window.with_element_namespace("crease_toggles", |window| {
                        self.layout_crease_toggles(start_row..end_row, &row_infos, &active_rows, &snapshot, window, cx)
                    });
                    let crease_trailers = window.with_element_namespace("crease_trailers", |window| {
                        self.layout_crease_trailers(row_infos.iter().cloned(), &snapshot, window, cx)
                    });

                    let display_hunks = self.layout_gutter_diff_hunks(
                        line_height,
                        &gutter_hitbox,
                        start_row..end_row,
                        &snapshot,
                        scroll_position,
                        window,
                        cx,
                    );

                    Self::layout_word_diff_highlights(
                        &display_hunks,
                        &row_infos,
                        start_row,
                        &snapshot,
                        &mut highlighted_ranges,
                        cx,
                    );

                    let bg_segments_per_row = Self::bg_segments_per_row(
                        start_row..end_row,
                        &selections,
                        highlighted_ranges
                            .iter()
                            .cloned()
                            .chain(document_colors.iter().flat_map(|(_, colors)| colors.iter().cloned())),
                        self.style.background,
                    );

                    let mut line_layouts = Self::layout_lines(
                        start_row..end_row,
                        &snapshot,
                        &self.style,
                        editor_width,
                        is_row_soft_wrapped,
                        &bg_segments_per_row,
                        window,
                        cx,
                    );
                    let new_renderer_widths = (!is_minimap).then(|| {
                        line_layouts.iter().flat_map(|layout| &layout.fragments).filter_map(|fragment| {
                            if let LineFragment::Element { id, size, .. } = fragment {
                                Some((*id, size.width))
                            } else {
                                None
                            }
                        })
                    });
                    let renderer_widths_changed = request_layout.has_remaining_prepaint_depth()
                        && new_renderer_widths.is_some_and(|new_renderer_widths| {
                            self.editor.update(cx, |editor, cx| editor.update_renderer_widths(new_renderer_widths, cx))
                        });
                    if renderer_widths_changed {
                        return self.prepaint(None, bounds, request_layout, window, cx);
                    }

                    let longest_line_width = layout_line(
                        snapshot.longest_row(),
                        &snapshot,
                        style,
                        editor_width,
                        is_row_soft_wrapped,
                        window,
                        cx,
                    )
                    .width;

                    let scrollbar_layout_information = ScrollbarLayoutInformation::new(
                        text_hitbox.bounds,
                        glyph_grid_cell,
                        size(longest_line_width, Pixels::from(max_row.as_f64() * f64::from(line_height))),
                        EditorSettings::get_global(cx),
                        scroll_beyond_last_line,
                    );

                    let mut scroll_width = scrollbar_layout_information.scroll_range.width;

                    let sticky_header_excerpt = if snapshot.buffer_snapshot().show_headers() {
                        snapshot.sticky_header_excerpt(scroll_position.y)
                    } else {
                        None
                    };
                    let sticky_header_excerpt_id = sticky_header_excerpt.as_ref().map(|top| top.excerpt.buffer_id());

                    let buffer = snapshot.buffer_snapshot();
                    let start_buffer_row = MultiBufferRow(start_anchor.to_point(buffer).row);
                    let end_buffer_row = MultiBufferRow(end_anchor.to_point(buffer).row);

                    let preliminary_scroll_pixel_position = point(
                        scroll_position.x * f64::from(em_layout_width),
                        scroll_position.y * f64::from(line_height),
                    );
                    let indent_guides = self.layout_indent_guides(
                        content_origin,
                        text_hitbox.origin,
                        start_buffer_row..end_buffer_row,
                        preliminary_scroll_pixel_position,
                        line_height,
                        &snapshot,
                        window,
                        cx,
                    );
                    let indent_guides_for_spacers = indent_guides.clone();

                    let blocks = if !is_minimap {
                        {
                            window.with_element_namespace("blocks", |window| {
                                self.render_blocks(
                                    start_row..end_row,
                                    &snapshot,
                                    &hitbox,
                                    &text_hitbox,
                                    editor_width,
                                    &mut scroll_width,
                                    &editor_margins,
                                    em_width,
                                    gutter_dimensions.full_width(),
                                    line_height,
                                    &line_layouts,
                                    &local_selections,
                                    &selected_buffer_ids,
                                    &latest_selection_anchors,
                                    is_row_soft_wrapped,
                                    sticky_header_excerpt_id,
                                    &indent_guides_for_spacers,
                                    window,
                                    cx,
                                )
                            })
                        }
                    } else {
                        Default::default()
                    };
                    let RenderBlocksOutput {
                        non_spacer_blocks: mut blocks,
                        mut spacer_blocks,
                        row_block_types,
                        resized_blocks,
                    } = blocks;
                    if let Some(resized_blocks) = resized_blocks {
                        if request_layout.has_remaining_prepaint_depth() {
                            self.editor.update(cx, |editor, cx| {
                                editor.resize_blocks(
                                    resized_blocks,
                                    autoscroll_request.map(|(autoscroll, _)| autoscroll),
                                    cx,
                                )
                            });
                            return self.prepaint(None, bounds, request_layout, window, cx);
                        } else {
                            debug_panic!(
                                "dropping block resize because prepaint depth \
                                 limit was reached"
                            );
                        }
                    }

                    let sticky_buffer_header = if self.should_show_buffer_headers() {
                        sticky_header_excerpt.map(|sticky_header_excerpt| {
                            window.with_element_namespace("blocks", |window| {
                                self.layout_sticky_buffer_header(
                                    sticky_header_excerpt,
                                    scroll_position,
                                    line_height,
                                    right_margin,
                                    &snapshot,
                                    &hitbox,
                                    &selected_buffer_ids,
                                    &blocks,
                                    &latest_selection_anchors,
                                    window,
                                    cx,
                                )
                            })
                        })
                    } else {
                        None
                    };

                    let scroll_max: gpui::Point<ScrollPixelOffset> = point(
                        ScrollPixelOffset::from(((scroll_width - editor_width) / em_layout_width).max(0.0)),
                        max_scroll_top,
                    );

                    self.editor.update(cx, |editor, cx| {
                        if editor.scroll_manager.clamp_scroll_left(scroll_max.x, cx) {
                            scroll_position.x = scroll_max.x.min(scroll_position.x);
                        }

                        if needs_horizontal_autoscroll.0
                            && let Some(new_scroll_position) = editor.autoscroll_horizontally(
                                start_row,
                                editor_width,
                                scroll_width,
                                em_advance,
                                &line_layouts,
                                autoscroll_request,
                                window,
                                cx,
                            )
                        {
                            scroll_position.x = new_scroll_position.x;
                        }
                    });

                    if !em_layout_width.is_zero() {
                        scroll_position.x = window.pixel_snap_f64(scroll_position.x * f64::from(em_layout_width))
                            / f64::from(em_layout_width);
                    }

                    let scroll_pixel_position = point(
                        scroll_position.x * f64::from(em_layout_width),
                        scroll_position.y * f64::from(line_height),
                    );
                    let indent_guides = if scroll_pixel_position != preliminary_scroll_pixel_position {
                        self.layout_indent_guides(
                            content_origin,
                            text_hitbox.origin,
                            start_buffer_row..end_buffer_row,
                            scroll_pixel_position,
                            line_height,
                            &snapshot,
                            window,
                            cx,
                        )
                    } else {
                        indent_guides
                    };

                    let crease_trailers = window.with_element_namespace("crease_trailers", |window| {
                        self.prepaint_crease_trailers(
                            crease_trailers,
                            &line_layouts,
                            line_height,
                            content_origin,
                            scroll_pixel_position,
                            scroll_position,
                            start_row,
                            em_width,
                            window,
                            cx,
                        )
                    });

                    let inline_diagnostics = self.layout_inline_diagnostics(
                        &line_layouts,
                        &crease_trailers,
                        &row_block_types,
                        content_origin,
                        scroll_position,
                        scroll_pixel_position,
                        start_row,
                        end_row,
                        line_height,
                        em_width,
                        style,
                        window,
                        cx,
                    );

                    let mut inline_code_actions = None;
                    if let Some(newest_selection_head) = newest_selection_head {
                        let display_row = newest_selection_head.row();
                        if (start_row..end_row).contains(&display_row) && !row_block_types.contains_key(&display_row) {
                            inline_code_actions = self.layout_inline_code_actions(
                                newest_selection_head,
                                content_origin,
                                scroll_position,
                                scroll_pixel_position,
                                line_height,
                                &snapshot,
                                window,
                                cx,
                            );
                        }
                    }

                    let blamed_display_rows = self.layout_blame_entries(
                        &row_infos,
                        em_width,
                        scroll_position,
                        start_row,
                        line_height,
                        &gutter_hitbox,
                        gutter_dimensions.git_blame_entries_width,
                        window,
                        cx,
                    );

                    let line_elements = self.prepaint_lines(
                        start_row,
                        &mut line_layouts,
                        line_height,
                        scroll_position,
                        scroll_pixel_position,
                        content_origin,
                        text_hitbox.size.width,
                        window,
                        cx,
                    );

                    window.with_element_namespace("blocks", |window| {
                        self.layout_blocks(
                            &mut blocks,
                            &hitbox,
                            &gutter_hitbox,
                            line_height,
                            scroll_position,
                            scroll_pixel_position,
                            &editor_margins,
                            window,
                            cx,
                        );
                        self.layout_blocks(
                            &mut spacer_blocks,
                            &hitbox,
                            &gutter_hitbox,
                            line_height,
                            scroll_position,
                            scroll_pixel_position,
                            &editor_margins,
                            window,
                            cx,
                        );
                    });

                    let cursors = self.collect_cursors(&snapshot, cx);
                    let visible_row_range = start_row..end_row;
                    let non_visible_cursors = cursors.iter().any(|c| !visible_row_range.contains(&c.0.row()));

                    let visible_cursors = self.layout_visible_cursors(
                        &snapshot,
                        &selections,
                        &row_block_types,
                        start_row..end_row,
                        &line_layouts,
                        &text_hitbox,
                        content_origin,
                        scroll_position,
                        scroll_pixel_position,
                        line_height,
                        em_width,
                        em_advance,
                        autoscroll_containing_element,
                        window,
                        cx,
                    );
                    let navigation_overlay_paint_commands = self.layout_navigation_overlays(
                        &snapshot,
                        start_row..end_row,
                        &line_layouts,
                        &text_hitbox,
                        content_origin,
                        scroll_position,
                        scroll_pixel_position,
                        line_height,
                        window,
                        cx,
                    );

                    let frozen_scroll_state = if self.editor.read(cx).search_results_hold.is_some() {
                        let is_rewrapping = self.editor.read(cx).display_map.read(cx).is_rewrapping(cx);
                        self.editor.update(cx, |editor, _| {
                            editor.frozen_scroll_range(
                                is_rewrapping,
                                scrollbar_layout_information.scroll_range,
                                editor_width,
                                scrollbar_layout_information.editor_bounds.size,
                            )
                        })
                    } else {
                        None
                    };
                    let effective_scrollbar_layout_information =
                        frozen_scroll_state.map_or(scrollbar_layout_information, |settled| {
                            ScrollbarLayoutInformation { scroll_range: settled.range, ..scrollbar_layout_information }
                        });
                    let effective_editor_width =
                        frozen_scroll_state.map_or(editor_width, |settled| settled.editor_width);

                    let scrollbars_layout = self.layout_scrollbars(
                        &snapshot,
                        &effective_scrollbar_layout_information,
                        content_offset,
                        scroll_position,
                        non_visible_cursors,
                        right_margin,
                        effective_editor_width,
                        window,
                        cx,
                    );

                    let context_menu_layout = if let Some(newest_selection_head) = newest_selection_head {
                        let newest_selection_point = newest_selection_head.to_point(&snapshot.display_snapshot);
                        if (start_row..end_row).contains(&newest_selection_head.row()) {
                            self.layout_cursor_popovers(
                                line_height,
                                &text_hitbox,
                                content_origin,
                                right_margin,
                                start_row,
                                scroll_pixel_position,
                                &line_layouts,
                                newest_selection_head,
                                newest_selection_point,
                                style,
                                window,
                                cx,
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    self.layout_gutter_menu(
                        line_height,
                        &text_hitbox,
                        content_origin,
                        right_margin,
                        scroll_pixel_position,
                        gutter_dimensions.width - gutter_dimensions.left_padding,
                        window,
                        cx,
                    );

                    let test_indicators = self.layout_run_indicators(&gutter, &run_indicator_rows, window, cx);

                    let editor = self.editor.read(cx);
                    let copied_row = editor.gutter_copied_row.0;
                    let hovered_row =
                        editor.gutter_hover_button.0.filter(|button| button.is_active).map(|button| button.display_row);
                    // Keep the "copied" check visible after the mouse leaves its row.
                    let gutter_hover_buttons = copied_row
                        .into_iter()
                        .chain(hovered_row.filter(|row| Some(*row) != copied_row))
                        .filter(|row| !run_indicator_rows.contains(row))
                        .filter_map(|row| self.layout_gutter_hover_button(&gutter, row, window, cx))
                        .collect::<Vec<_>>();

                    if !cx.has_active_drag() {
                        self.layout_hover_popovers(
                            &snapshot,
                            &hitbox,
                            start_row..end_row,
                            content_origin,
                            scroll_pixel_position,
                            &line_layouts,
                            line_height,
                            em_width,
                            context_menu_layout,
                            window,
                            cx,
                        );
                    }

                    let mouse_context_menu =
                        self.layout_mouse_context_menu(&snapshot, start_row..end_row, content_origin, window, cx);

                    window.with_element_namespace("crease_toggles", |window| {
                        self.prepaint_crease_toggles(
                            &mut crease_toggles,
                            line_height,
                            &gutter_dimensions,
                            scroll_position,
                            start_row,
                            &gutter_hitbox,
                            window,
                            cx,
                        )
                    });

                    window.with_element_namespace("expand_toggles", |window| {
                        self.prepaint_expand_toggles(&mut expand_toggles, window, cx)
                    });

                    let wrap_guides =
                        self.layout_wrap_guides(em_advance, scroll_position, content_origin, &hitbox, window, cx);

                    let minimap = window.with_element_namespace("minimap", |window| {
                        self.layout_minimap(
                            &snapshot,
                            minimap_width,
                            scroll_position,
                            &effective_scrollbar_layout_information,
                            scrollbars_layout.as_ref(),
                            window,
                            cx,
                        )
                    });

                    let invisible_symbol_font_size = font_size / 2.;
                    let whitespace_map = &self.editor.read(cx).buffer.read(cx).language_settings(cx).whitespace_map;

                    let tab_char = whitespace_map.tab.clone();
                    let tab_len = tab_char.len();
                    let tab_invisible = window.text_system().shape_line(
                        tab_char,
                        invisible_symbol_font_size,
                        &[TextRun {
                            len: tab_len,
                            font: self.style.text.font(),
                            color: cx.theme().colors().editor_invisible,
                            ..Default::default()
                        }],
                        None,
                    );

                    let space_char = whitespace_map.space.clone();
                    let space_len = space_char.len();
                    let space_invisible = window.text_system().shape_line(
                        space_char,
                        invisible_symbol_font_size,
                        &[TextRun {
                            len: space_len,
                            font: self.style.text.font(),
                            color: cx.theme().colors().editor_invisible,
                            ..Default::default()
                        }],
                        None,
                    );

                    let mode = snapshot.mode.clone();

                    let has_sticky_buffer_header = sticky_buffer_header.is_some() || sticky_header_excerpt_id.is_some();
                    let sticky_header_height = if has_sticky_buffer_header {
                        let full_height = FILE_HEADER_HEIGHT as f32 * line_height;
                        let display_row = blocks
                            .iter()
                            .filter(|block| block.is_buffer_header)
                            .find_map(|block| block.row.filter(|row| row.0 > scroll_position.y as u32));
                        let offset = match display_row {
                            Some(display_row) => {
                                let max_row = display_row.0.saturating_sub(FILE_HEADER_HEIGHT);
                                let offset = (scroll_position.y - max_row as f64).max(0.0);
                                let slide_up = Pixels::from(offset * ScrollPixelOffset::from(line_height));

                                (full_height - slide_up).max(Pixels::ZERO)
                            }
                            None => full_height,
                        };
                        let header_bottom_padding = BUFFER_HEADER_PADDING.to_pixels(window.rem_size());
                        offset - header_bottom_padding
                    } else {
                        Pixels::ZERO
                    };

                    let (diff_hunk_controls, diff_hunk_control_bounds) =
                        if is_read_only && self.editor.read(cx).diff_hunk_renderer.is_none() {
                            (vec![], vec![])
                        } else {
                            self.layout_diff_hunk_controls(
                                start_row..end_row,
                                &row_infos,
                                &text_hitbox,
                                current_selection_head,
                                line_height,
                                right_margin,
                                scroll_pixel_position,
                                sticky_header_height,
                                &display_hunks,
                                &highlighted_rows,
                                self.editor.clone(),
                                window,
                                cx,
                            )
                        };

                    self.populate_point_diagnostics(&snapshot, start_row..end_row, &mut line_layouts);

                    let position_map = Rc::new(PositionMap {
                        size: bounds.size,
                        visible_row_range,
                        scroll_position,
                        scroll_pixel_position,
                        scroll_max,
                        line_layouts,
                        line_height,
                        em_advance,
                        em_layout_width,
                        snapshot,
                        text_align: self.style.text.text_align,
                        content_width: text_hitbox.size.width,
                        content_origin,
                        gutter_hitbox: gutter_hitbox.clone(),
                        text_hitbox: text_hitbox.clone(),
                        display_hunks: display_hunks.clone(),
                        diff_hunk_control_bounds,
                    });

                    let visible_horizontal_scrollbar = scrollbars_layout.as_ref().is_some_and(|scrollbars_layout| {
                        scrollbars_layout.visible && scrollbars_layout.horizontal.is_some()
                    });

                    self.editor.update(cx, |editor, _| {
                        editor.last_position_map = Some(position_map.clone());
                        editor.last_right_margin = right_margin;
                        editor.last_horizontal_scrollbar_visible = visible_horizontal_scrollbar;
                    });

                    EditorLayout {
                        mode,
                        position_map,
                        visible_display_row_range: start_row..end_row,
                        wrap_guides,
                        indent_guides,
                        hitbox,
                        gutter_hitbox,
                        display_hunks,
                        content_origin,
                        scrollbars_layout,
                        minimap,
                        active_rows,
                        highlighted_rows,
                        highlighted_ranges,
                        highlighted_gutter_ranges,
                        document_colors,
                        line_elements,
                        line_numbers,
                        blamed_display_rows,
                        inline_diagnostics,
                        point_diagnostic_underline_offset,
                        inline_code_actions,
                        blocks,
                        spacer_blocks,
                        cursors,
                        visible_cursors,
                        navigation_overlay_paint_commands,
                        selections,
                        diff_hunk_controls,
                        mouse_context_menu,
                        test_indicators,
                        gutter_hover_buttons,
                        crease_toggles,
                        crease_trailers,
                        tab_invisible,
                        space_invisible,
                        sticky_buffer_header,
                        expand_toggles,
                        text_align: self.style.text.text_align,
                        content_width: text_hitbox.size.width,
                    }
                })
            })
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        bounds: Bounds<gpui::Pixels>,
        _: &mut Self::RequestLayoutState,
        layout: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !layout.mode.is_minimap() {
            let focus_handle = self.editor.focus_handle(cx);
            let key_context = self.editor.update(cx, |editor, cx| editor.key_context(window, cx));

            window.set_key_context(key_context);
            window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.editor.clone()), cx);
            self.register_actions(window, cx);
            self.register_key_listeners(window, cx, layout);
        }

        let text_style = TextStyleRefinement {
            font_size: Some(self.style.text.font_size),
            line_height: Some(self.style.text.line_height),
            ..Default::default()
        };
        let rem_size = self.rem_size(cx);
        window.with_rem_size(rem_size, |window| {
            window.with_text_style(Some(text_style), |window| {
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    self.paint_mouse_listeners(layout, window, cx);

                    self.paint_background(layout, window, cx);

                    self.paint_indent_guides(layout, window, cx);

                    if layout.gutter_hitbox.size.width > Pixels::ZERO {
                        self.paint_blamed_display_rows(layout, window, cx);
                        self.paint_line_numbers(layout, window, cx);
                    }

                    self.paint_text(layout, window, cx);

                    if !layout.spacer_blocks.is_empty() {
                        window.with_element_namespace("blocks", |window| {
                            self.paint_spacer_blocks(layout, window, cx);
                        });
                    }

                    if layout.gutter_hitbox.size.width > Pixels::ZERO {
                        self.paint_gutter_highlights(layout, window, cx);
                        self.paint_gutter_indicators(layout, window, cx);
                    }

                    if !layout.blocks.is_empty() {
                        window.with_element_namespace("blocks", |window| {
                            self.paint_non_spacer_blocks(layout, window, cx);
                        });
                    }

                    window.with_element_namespace("blocks", |window| {
                        if let Some(mut sticky_header) = layout.sticky_buffer_header.take() {
                            sticky_header.paint(window, cx)
                        }
                    });

                    self.paint_minimap(layout, window, cx);
                    self.paint_scrollbars(layout, window, cx);
                    self.paint_mouse_context_menu(layout, window, cx);
                });
            })
        })
    }
}

pub(super) const fn gutter_bounds(
    editor_bounds: Bounds<Pixels>,
    gutter_dimensions: GutterDimensions,
) -> Bounds<Pixels> {
    Bounds { origin: editor_bounds.origin, size: size(gutter_dimensions.width, editor_bounds.size.height) }
}

#[derive(Clone, Copy)]
struct ContextMenuLayout {
    y_flipped: bool,
    bounds: Bounds<Pixels>,
}

/// Holds information required for layouting the editor scrollbars.
#[derive(Clone, Copy)]
struct ScrollbarLayoutInformation {
    /// The bounds of the editor area (excluding the content offset).
    editor_bounds: Bounds<Pixels>,
    /// The available range to scroll within the document.
    scroll_range: Size<Pixels>,
    /// The space available for one glyph in the editor.
    glyph_grid_cell: Size<Pixels>,
}

impl ScrollbarLayoutInformation {
    pub fn new(
        editor_bounds: Bounds<Pixels>,
        glyph_grid_cell: Size<Pixels>,
        document_size: Size<Pixels>,
        settings: &EditorSettings,
        scroll_beyond_last_line: ScrollBeyondLastLine,
    ) -> Self {
        let vertical_overscroll = match scroll_beyond_last_line {
            ScrollBeyondLastLine::OnePage => editor_bounds.size.height,
            ScrollBeyondLastLine::Off => glyph_grid_cell.height,
            ScrollBeyondLastLine::VerticalScrollMargin => {
                (1.0 + settings.vertical_scroll_margin) as f32 * glyph_grid_cell.height
            }
        };

        let overscroll = size(Pixels::ZERO, vertical_overscroll);

        ScrollbarLayoutInformation { editor_bounds, scroll_range: document_size + overscroll, glyph_grid_cell }
    }
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub struct EditorLayout {
    position_map: Rc<PositionMap>,
    hitbox: Hitbox,
    gutter_hitbox: Hitbox,
    content_origin: gpui::Point<Pixels>,
    scrollbars_layout: Option<EditorScrollbars>,
    minimap: Option<MinimapLayout>,
    mode: EditorMode,
    wrap_guides: SmallVec<[(Pixels, bool); 2]>,
    indent_guides: Option<Vec<IndentGuideLayout>>,
    visible_display_row_range: Range<DisplayRow>,
    active_rows: BTreeMap<DisplayRow, LineHighlightSpec>,
    highlighted_rows: BTreeMap<DisplayRow, LineHighlight>,
    line_elements: SmallVec<[AnyElement; 1]>,
    line_numbers: Arc<HashMap<MultiBufferRow, LineNumberLayout>>,
    display_hunks: Vec<(DisplayDiffHunk, Option<Hitbox>)>,
    blamed_display_rows: Option<Vec<AnyElement>>,
    inline_diagnostics: HashMap<DisplayRow, AnyElement>,
    point_diagnostic_underline_offset: Pixels,
    inline_code_actions: Option<AnyElement>,
    blocks: Vec<BlockLayout>,
    spacer_blocks: Vec<BlockLayout>,
    highlighted_ranges: Vec<(Range<DisplayPoint>, Hsla)>,
    highlighted_gutter_ranges: Vec<(Range<DisplayPoint>, Hsla)>,
    cursors: Vec<(DisplayPoint, Hsla)>,
    visible_cursors: Vec<CursorLayout>,
    navigation_overlay_paint_commands: Vec<NavigationOverlayPaintCommand>,
    selections: Vec<(PlayerColor, Vec<SelectionLayout>)>,
    test_indicators: Vec<AnyElement>,
    gutter_hover_buttons: Vec<AnyElement>,
    crease_toggles: Vec<Option<AnyElement>>,
    expand_toggles: Vec<Option<(AnyElement, gpui::Point<Pixels>)>>,
    diff_hunk_controls: Vec<AnyElement>,
    crease_trailers: Vec<Option<CreaseTrailerLayout>>,
    mouse_context_menu: Option<AnyElement>,
    tab_invisible: ShapedLine,
    space_invisible: ShapedLine,
    sticky_buffer_header: Option<AnyElement>,
    document_colors: Option<(DocumentColorsRenderMode, Vec<(Range<DisplayPoint>, Hsla)>)>,
    text_align: TextAlign,
    content_width: Pixels,
}

impl EditorLayout {
    fn line_end_overshoot(&self) -> Pixels {
        0.15 * self.position_map.line_height
    }
}

#[derive(Debug)]
struct LineNumberSegment {
    shaped_line: ShapedLine,
    hitbox: Option<Hitbox>,
}

#[derive(Debug)]
struct LineNumberLayout {
    segments: SmallVec<[LineNumberSegment; 1]>,
}

struct ColoredRange<T> {
    start: T,
    end: T,
    color: Hsla,
}

/// Only the horizontal axis gets a scrollbar; vertical scrolling and its
/// markers live on the minimap.
#[derive(Clone)]
struct EditorScrollbars {
    pub horizontal: Option<ScrollbarLayout>,
    pub visible: bool,
}

impl EditorScrollbars {
    pub fn new(
        layout_information: &ScrollbarLayoutInformation,
        content_offset: gpui::Point<Pixels>,
        scroll_position: gpui::Point<f64>,
        scrollbar_width: Pixels,
        right_margin: Pixels,
        editor_width: Pixels,
        show_scrollbars: bool,
        scrollbar_state: Option<&ActiveScrollbarState>,
        window: &mut Window,
    ) -> Self {
        const AXIS: ScrollbarAxis = ScrollbarAxis::Horizontal;
        let ScrollbarLayoutInformation { editor_bounds, scroll_range, glyph_grid_cell } = layout_information;

        let horizontal = (scroll_range.width > editor_width).then(|| {
            let bounds = Bounds::from_anchor_and_size(
                gpui::Anchor::BottomLeft,
                editor_bounds.bottom_left(),
                size(
                    // The horizontal viewport size differs from the space available for the
                    // horizontal scrollbar, so we have to manually stitch it together here.
                    editor_bounds.size.width - right_margin,
                    scrollbar_width,
                ),
            );
            ScrollbarLayout::new(
                window.insert_hitbox(bounds, HitboxBehavior::Normal),
                editor_width,
                scroll_range.width,
                glyph_grid_cell.along(AXIS),
                content_offset.along(AXIS),
                scroll_position.along(AXIS),
                show_scrollbars,
                AXIS,
            )
            .with_thumb_state(scrollbar_state.and_then(|state| state.thumb_state_for_axis(AXIS)))
        });

        Self { horizontal, visible: show_scrollbars }
    }

    pub fn iter_scrollbars(&self) -> impl Iterator<Item = (&ScrollbarLayout, ScrollbarAxis)> + '_ {
        self.horizontal.iter().map(|scrollbar| (scrollbar, ScrollbarAxis::Horizontal))
    }

    /// Returns the currently hovered scrollbar axis, if any.
    pub fn get_hovered_axis(&self, window: &Window) -> Option<(&ScrollbarLayout, ScrollbarAxis)> {
        self.iter_scrollbars().find(|s| s.0.hitbox.is_hovered(window))
    }
}

#[derive(Clone)]
struct ScrollbarLayout {
    hitbox: Hitbox,
    visible_range: Range<ScrollOffset>,
    text_unit_size: Pixels,
    thumb_bounds: Option<Bounds<Pixels>>,
    thumb_state: ScrollbarThumbState,
}

impl ScrollbarLayout {
    const BORDER_WIDTH: Pixels = px(1.0);
    const MIN_THUMB_SIZE: Pixels = px(25.0);

    fn new(
        scrollbar_track_hitbox: Hitbox,
        viewport_size: Pixels,
        scroll_range: Pixels,
        glyph_space: Pixels,
        content_offset: Pixels,
        scroll_position: ScrollOffset,
        show_thumb: bool,
        axis: ScrollbarAxis,
    ) -> Self {
        let track_bounds = scrollbar_track_hitbox.bounds;
        // The length of the track available to the scrollbar thumb. We deliberately
        // exclude the content size here so that the thumb aligns with the content.
        let track_length = track_bounds.size.along(axis) - content_offset;

        Self::new_with_hitbox_and_track_length(
            scrollbar_track_hitbox,
            track_length,
            viewport_size,
            scroll_range.into(),
            glyph_space,
            content_offset.into(),
            scroll_position,
            show_thumb,
            axis,
        )
    }

    fn for_minimap(
        minimap_track_hitbox: Hitbox,
        visible_lines: f64,
        total_editor_lines: f64,
        minimap_line_height: Pixels,
        scroll_position: ScrollOffset,
        minimap_scroll_top: ScrollOffset,
        show_thumb: bool,
    ) -> Self {
        // The scrollbar thumb size is calculated as
        // (visible_content/total_content) × scrollbar_track_length.
        //
        // For the minimap's thumb layout, we leverage this by setting the
        // scrollbar track length to the entire document size (using minimap line
        // height). This creates a thumb that exactly represents the editor
        // viewport scaled to minimap proportions.
        //
        // We adjust the thumb position relative to `minimap_scroll_top` to
        // accommodate for the deliberately oversized track.
        //
        // This approach ensures that the minimap thumb accurately reflects the
        // editor's current scroll position whilst nicely synchronizing the minimap
        // thumb and scrollbar thumb.
        let scroll_range = total_editor_lines * f64::from(minimap_line_height);
        let viewport_size = visible_lines * f64::from(minimap_line_height);

        let track_top_offset = -minimap_scroll_top * f64::from(minimap_line_height);

        Self::new_with_hitbox_and_track_length(
            minimap_track_hitbox,
            Pixels::from(scroll_range),
            Pixels::from(viewport_size),
            scroll_range,
            minimap_line_height,
            track_top_offset,
            scroll_position,
            show_thumb,
            ScrollbarAxis::Vertical,
        )
    }

    fn new_with_hitbox_and_track_length(
        scrollbar_track_hitbox: Hitbox,
        track_length: Pixels,
        viewport_size: Pixels,
        scroll_range: f64,
        glyph_space: Pixels,
        content_offset: ScrollOffset,
        scroll_position: ScrollOffset,
        show_thumb: bool,
        axis: ScrollbarAxis,
    ) -> Self {
        let text_units_per_page = viewport_size.to_f64() / glyph_space.to_f64();
        let visible_range = scroll_position..scroll_position + text_units_per_page;
        let total_text_units = scroll_range / glyph_space.to_f64();

        let thumb_percentage = text_units_per_page / total_text_units;
        let thumb_size = Pixels::from(ScrollOffset::from(track_length) * thumb_percentage)
            .max(ScrollbarLayout::MIN_THUMB_SIZE)
            .min(track_length);

        let text_unit_divisor = (total_text_units - text_units_per_page).max(0.);

        let content_larger_than_viewport = text_unit_divisor > 0.;

        let text_unit_size = if content_larger_than_viewport {
            Pixels::from(ScrollOffset::from(track_length - thumb_size) / text_unit_divisor)
        } else {
            glyph_space
        };

        let thumb_bounds = (show_thumb && content_larger_than_viewport).then(|| {
            Self::thumb_bounds(
                &scrollbar_track_hitbox,
                content_offset,
                visible_range.start,
                text_unit_size,
                thumb_size,
                axis,
            )
        });

        ScrollbarLayout {
            hitbox: scrollbar_track_hitbox,
            visible_range,
            text_unit_size,
            thumb_bounds,
            thumb_state: Default::default(),
        }
    }

    const fn with_thumb_state(self, thumb_state: Option<ScrollbarThumbState>) -> Self {
        if let Some(thumb_state) = thumb_state { Self { thumb_state, ..self } } else { self }
    }

    fn thumb_bounds(
        scrollbar_track: &Hitbox,
        content_offset: f64,
        visible_range_start: f64,
        text_unit_size: Pixels,
        thumb_size: Pixels,
        axis: ScrollbarAxis,
    ) -> Bounds<Pixels> {
        let thumb_origin = scrollbar_track.origin.apply_along(axis, |origin| {
            origin + Pixels::from(content_offset + visible_range_start * ScrollOffset::from(text_unit_size))
        });
        Bounds::new(thumb_origin, scrollbar_track.size.apply_along(axis, |_| thumb_size))
    }

    fn thumb_hovered(&self, position: &gpui::Point<Pixels>) -> bool {
        self.thumb_bounds.is_some_and(|bounds| bounds.contains(position))
    }
}

struct CreaseTrailerLayout {
    element: AnyElement,
    bounds: Bounds<Pixels>,
}

pub(crate) struct PositionMap {
    pub size: Size<Pixels>,
    pub line_height: Pixels,
    pub scroll_position: gpui::Point<ScrollOffset>,
    pub scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
    pub scroll_max: gpui::Point<ScrollOffset>,
    pub em_advance: Pixels,
    pub em_layout_width: Pixels,
    pub visible_row_range: Range<DisplayRow>,
    pub line_layouts: Vec<LineWithInvisibles>,
    pub snapshot: EditorSnapshot,
    pub text_align: TextAlign,
    pub content_width: Pixels,
    pub content_origin: gpui::Point<Pixels>,
    pub text_hitbox: Hitbox,
    pub gutter_hitbox: Hitbox,
    pub display_hunks: Vec<(DisplayDiffHunk, Option<Hitbox>)>,
    pub diff_hunk_control_bounds: Vec<(DisplayRow, Bounds<Pixels>)>,
}

#[derive(Debug, Copy, Clone)]
pub struct PointForPosition {
    pub previous_valid: DisplayPoint,
    pub next_valid: DisplayPoint,
    pub nearest_valid: DisplayPoint,
    pub exact_unclipped: DisplayPoint,
    pub column_overshoot_after_line_end: u32,
}

impl PointForPosition {
    pub fn as_valid(&self) -> Option<DisplayPoint> {
        if self.previous_valid == self.exact_unclipped && self.next_valid == self.exact_unclipped {
            Some(self.previous_valid)
        } else {
            None
        }
    }

    pub fn intersects_selection(&self, selection: &Selection<DisplayPoint>) -> bool {
        let Some(valid_point) = self.as_valid() else {
            return false;
        };
        let range = selection.range();

        let candidate_row = valid_point.row();
        let candidate_col = valid_point.column();

        let start_row = range.start.row();
        let start_col = range.start.column();
        let end_row = range.end.row();
        let end_col = range.end.column();

        if candidate_row < start_row || candidate_row > end_row {
            false
        } else if start_row == end_row {
            candidate_col >= start_col && candidate_col < end_col
        } else if candidate_row == start_row {
            candidate_col >= start_col
        } else if candidate_row == end_row {
            candidate_col < end_col
        } else {
            true
        }
    }
}

impl PositionMap {
    pub(crate) fn inlay_hint_glyph_for_position(&self, position: gpui::Point<Pixels>) -> Option<DisplayPoint> {
        let position = position - self.content_origin;
        if position.y < Pixels::ZERO {
            return None;
        }
        let row = ((position.y / self.line_height) as f64 + self.scroll_position.y) as u32;
        let line_index = row.checked_sub(self.visible_row_range.start.0)?;
        let line = self.line_layouts.get(line_index as usize)?;
        let x = position.x + (self.scroll_position.x as f32 * self.em_layout_width)
            - line.alignment_offset(self.text_align, self.content_width);
        if x < Pixels::ZERO {
            return None;
        }
        let glyph = DisplayPoint::new(DisplayRow(row), line.index_for_x(x)? as u32);
        self.snapshot.inlay_hint_at(glyph).map(|_| glyph)
    }

    pub(crate) fn point_for_position(&self, position: gpui::Point<Pixels>) -> PointForPosition {
        let text_bounds = self.text_hitbox.bounds;
        let scroll_position = self.scroll_position;
        let position = position - text_bounds.origin;
        let y = position.y.max(px(0.)).min(self.size.height);
        let x = position.x + (scroll_position.x as f32 * self.em_layout_width);
        let row = ((y / self.line_height) as f64 + scroll_position.y) as u32;

        let (column, x_overshoot_after_line_end) = if let Some(line_index) =
            row.checked_sub(self.visible_row_range.start.0)
            && let Some(line) = self.line_layouts.get(line_index as usize)
        {
            let alignment_offset = line.alignment_offset(self.text_align, self.content_width);
            let x_relative_to_text = x - alignment_offset;
            if let Some(ix) = line.index_for_x(x_relative_to_text) {
                (ix as u32, px(0.))
            } else {
                (line.len as u32, px(0.).max(x_relative_to_text - line.width))
            }
        } else {
            (0, x)
        };

        let mut exact_unclipped = DisplayPoint::new(DisplayRow(row), column);
        let previous_valid = self.snapshot.clip_point(exact_unclipped, Bias::Left);
        let next_valid = self.snapshot.clip_point(exact_unclipped, Bias::Right);

        let nearest_valid = if previous_valid == next_valid {
            previous_valid
        } else {
            match self.snapshot.inlay_bias_at(exact_unclipped) {
                Some(Bias::Left) => next_valid,
                Some(Bias::Right) => previous_valid,
                None => previous_valid,
            }
        };

        let column_overshoot_after_line_end = (x_overshoot_after_line_end / self.em_layout_width) as u32;
        *exact_unclipped.column_mut() += column_overshoot_after_line_end;
        PointForPosition { previous_valid, next_valid, nearest_valid, exact_unclipped, column_overshoot_after_line_end }
    }
}

pub(crate) struct BlockLayout {
    pub(crate) id: BlockId,
    pub(crate) x_offset: Pixels,
    pub(crate) row: Option<DisplayRow>,
    pub(crate) element: AnyElement,
    pub(crate) available_space: Size<AvailableSpace>,
    pub(crate) style: BlockStyle,
    pub(crate) overlaps_gutter: bool,
    pub(crate) is_buffer_header: bool,
}

pub fn layout_line(
    row: DisplayRow,
    snapshot: &EditorSnapshot,
    style: &EditorStyle,
    text_width: Pixels,
    is_row_soft_wrapped: impl Copy + Fn(usize) -> bool,
    window: &mut Window,
    cx: &mut App,
) -> LineWithInvisibles {
    let use_tree_sitter = !snapshot.semantic_tokens_enabled || snapshot.use_tree_sitter_for_syntax(row, cx);
    let language_aware = LanguageAwareStyling {
        tree_sitter: use_tree_sitter,
        diagnostics: snapshot.display_snapshot.diagnostics_visible(),
    };
    let chunks = snapshot.highlighted_chunks(row..row + DisplayRow(1), language_aware, style);
    LineWithInvisibles::from_chunks(
        chunks,
        style,
        MAX_LINE_LEN,
        1,
        &snapshot.mode,
        text_width,
        is_row_soft_wrapped,
        &[],
        window,
        cx,
    )
    .pop()
    .unwrap()
}

#[derive(Debug, Clone)]
pub struct IndentGuideLayout {
    origin: gpui::Point<Pixels>,
    length: Pixels,
    single_indent_width: Pixels,
    display_row_range: Range<DisplayRow>,
    depth: u32,
    active: bool,
    settings: IndentGuideSettings,
}

enum NavigationOverlayPaintCommand {
    Label(NavigationLabelLayout),
}

struct NavigationLabelLayout {
    element: AnyElement,
    #[allow(dead_code)]
    origin: gpui::Point<Pixels>,
}

struct NavigationOverlayLayoutContext<'a> {
    display_snapshot: &'a DisplaySnapshot,
    visible_display_row_range: &'a Range<DisplayRow>,
    line_layouts: &'a [LineWithInvisibles],
    text_align: TextAlign,
    content_width: Pixels,
    content_origin: gpui::Point<Pixels>,
    scroll_position: gpui::Point<ScrollOffset>,
    scroll_pixel_position: gpui::Point<ScrollPixelOffset>,
    line_height: Pixels,
    editor_font: Font,
    editor_font_size: Pixels,
}

const LABEL_LINE_HEIGHT_PADDING_PX: f32 = 2.0;

pub struct CursorLayout {
    origin: gpui::Point<Pixels>,
    block_width: Pixels,
    line_height: Pixels,
    color: Hsla,
    shape: CursorShape,
    block_text: Option<ShapedLine>,
    cursor_name: Option<AnyElement>,
    animated_corners: Option<[gpui::Point<Pixels>; 4]>,
}

#[derive(Debug)]
pub struct CursorName {
    string: SharedString,
    color: Hsla,
    is_top_row: bool,
}

impl CursorLayout {
    pub const fn new(
        origin: gpui::Point<Pixels>,
        block_width: Pixels,
        line_height: Pixels,
        color: Hsla,
        shape: CursorShape,
        block_text: Option<ShapedLine>,
    ) -> CursorLayout {
        CursorLayout {
            origin,
            block_width,
            line_height,
            color,
            shape,
            block_text,
            cursor_name: None,
            animated_corners: None,
        }
    }

    pub fn bounding_rect(&self, origin: gpui::Point<Pixels>) -> Bounds<Pixels> {
        Bounds { origin: self.origin + origin, size: size(self.block_width, self.line_height) }
    }

    fn bounds(&self, origin: gpui::Point<Pixels>) -> Bounds<Pixels> {
        match self.shape {
            CursorShape::Bar => Bounds { origin: self.origin + origin, size: size(px(2.0), self.line_height) },
            CursorShape::Block | CursorShape::Hollow => {
                Bounds { origin: self.origin + origin, size: size(self.block_width, self.line_height) }
            }
            CursorShape::Underline => Bounds {
                origin: self.origin + origin + gpui::Point::new(Pixels::ZERO, self.line_height - px(2.0)),
                size: size(self.block_width, px(2.0)),
            },
        }
    }

    pub fn layout(
        &mut self,
        origin: gpui::Point<Pixels>,
        cursor_name: Option<CursorName>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(cursor_name) = cursor_name {
            let bounds = self.bounds(origin);
            let text_size = self.line_height / 1.5;

            let name_origin = if cursor_name.is_top_row {
                point(bounds.right() - px(1.), bounds.top())
            } else {
                match self.shape {
                    CursorShape::Bar => point(bounds.right() - px(2.), bounds.top() - text_size / 2. - px(1.)),
                    _ => point(bounds.right() - px(1.), bounds.top() - text_size / 2. - px(1.)),
                }
            };
            let mut name_element = div()
                .bg(self.color)
                .text_size(text_size)
                .px_0p5()
                .line_height(text_size + px(LABEL_LINE_HEIGHT_PADDING_PX))
                .text_color(cursor_name.color)
                .child(cursor_name.string)
                .into_any_element();

            name_element.prepaint_as_root(name_origin, AvailableSpace::min_size(), window, cx);

            self.cursor_name = Some(name_element);
        }
    }

    pub fn paint(&mut self, origin: gpui::Point<Pixels>, window: &mut Window, cx: &mut App) {
        let bounds = window.pixel_snap_bounds(self.bounds(origin));

        if let Some(corners) = self.animated_corners {
            let mut builder = gpui::PathBuilder::fill();
            builder.add_polygon(&corners, true);
            if let Ok(path) = builder.build() {
                if let Some(name) = &mut self.cursor_name {
                    name.paint(window, cx);
                }
                window.paint_path(path, self.color);

                if !animated_corners_overlap_target(bounds, &corners) {
                    return;
                }
            }
        } else {
            //Draw background or border quad
            let cursor = if matches!(self.shape, CursorShape::Hollow) {
                outline(bounds, self.color, BorderStyle::Solid)
            } else {
                fill(bounds, self.color)
            };

            if let Some(name) = &mut self.cursor_name {
                name.paint(window, cx);
            }

            window.paint_quad(cursor);
        }

        if let Some(block_text) = &self.block_text {
            block_text.paint(self.origin + origin, self.line_height, TextAlign::Left, None, window, cx).log_err();
        }
    }

    pub const fn shape(&self) -> CursorShape {
        self.shape
    }
}

const fn cursor_shape_supports_cursor_animation(shape: CursorShape) -> bool {
    matches!(shape, CursorShape::Bar | CursorShape::Block)
}

#[derive(Debug)]
pub struct HighlightedRange {
    pub start_y: Pixels,
    pub line_height: Pixels,
    pub lines: Vec<HighlightedRangeLine>,
    pub color: Hsla,
    pub corner_radius: Pixels,
}

#[derive(Debug)]
pub struct HighlightedRangeLine {
    pub start_x: Pixels,
    pub end_x: Pixels,
}

impl HighlightedRange {
    pub fn paint(&self, fill: bool, bounds: Bounds<Pixels>, window: &mut Window) {
        if self.lines.len() >= 2 && self.lines[0].start_x > self.lines[1].end_x {
            self.paint_lines(self.start_y, &self.lines[0..1], fill, bounds, window);
            self.paint_lines(self.start_y + self.line_height, &self.lines[1..], fill, bounds, window);
        } else {
            self.paint_lines(self.start_y, &self.lines, fill, bounds, window);
        }
    }

    fn paint_lines(
        &self,
        start_y: Pixels,
        lines: &[HighlightedRangeLine],
        fill: bool,
        _bounds: Bounds<Pixels>,
        window: &mut Window,
    ) {
        if lines.is_empty() {
            return;
        }

        let first_line = lines.first().unwrap();
        let last_line = lines.last().unwrap();

        let first_top_left = point(first_line.start_x, start_y);
        let first_top_right = point(first_line.end_x, start_y);

        let curve_height = point(Pixels::ZERO, self.corner_radius);
        let curve_width = |start_x: Pixels, end_x: Pixels| {
            let max = (end_x - start_x) / 2.;
            let width = if max < self.corner_radius { max } else { self.corner_radius };

            point(width, Pixels::ZERO)
        };

        let top_curve_width = curve_width(first_line.start_x, first_line.end_x);
        let mut builder = if fill { gpui::PathBuilder::fill() } else { gpui::PathBuilder::stroke(px(1.)) };
        builder.move_to(first_top_right - top_curve_width);
        builder.curve_to(first_top_right + curve_height, first_top_right);

        let mut iter = lines.iter().enumerate().peekable();
        while let Some((ix, line)) = iter.next() {
            let bottom_right = point(line.end_x, start_y + (ix + 1) as f32 * self.line_height);

            if let Some((_, next_line)) = iter.peek() {
                let next_top_right = point(next_line.end_x, bottom_right.y);

                match next_top_right.x.partial_cmp(&bottom_right.x).unwrap() {
                    Ordering::Equal => {
                        builder.line_to(bottom_right);
                    }
                    Ordering::Less => {
                        let curve_width = curve_width(next_top_right.x, bottom_right.x);
                        builder.line_to(bottom_right - curve_height);
                        if self.corner_radius > Pixels::ZERO {
                            builder.curve_to(bottom_right - curve_width, bottom_right);
                        }
                        builder.line_to(next_top_right + curve_width);
                        if self.corner_radius > Pixels::ZERO {
                            builder.curve_to(next_top_right + curve_height, next_top_right);
                        }
                    }
                    Ordering::Greater => {
                        let curve_width = curve_width(bottom_right.x, next_top_right.x);
                        builder.line_to(bottom_right - curve_height);
                        if self.corner_radius > Pixels::ZERO {
                            builder.curve_to(bottom_right + curve_width, bottom_right);
                        }
                        builder.line_to(next_top_right - curve_width);
                        if self.corner_radius > Pixels::ZERO {
                            builder.curve_to(next_top_right + curve_height, next_top_right);
                        }
                    }
                }
            } else {
                let curve_width = curve_width(line.start_x, line.end_x);
                builder.line_to(bottom_right - curve_height);
                if self.corner_radius > Pixels::ZERO {
                    builder.curve_to(bottom_right - curve_width, bottom_right);
                }

                let bottom_left = point(line.start_x, bottom_right.y);
                builder.line_to(bottom_left + curve_width);
                if self.corner_radius > Pixels::ZERO {
                    builder.curve_to(bottom_left - curve_height, bottom_left);
                }
            }
        }

        if first_line.start_x > last_line.start_x {
            let curve_width = curve_width(last_line.start_x, first_line.start_x);
            let second_top_left = point(last_line.start_x, start_y + self.line_height);
            builder.line_to(second_top_left + curve_height);
            if self.corner_radius > Pixels::ZERO {
                builder.curve_to(second_top_left + curve_width, second_top_left);
            }
            let first_bottom_left = point(first_line.start_x, second_top_left.y);
            builder.line_to(first_bottom_left - curve_width);
            if self.corner_radius > Pixels::ZERO {
                builder.curve_to(first_bottom_left - curve_height, first_bottom_left);
            }
        }

        builder.line_to(first_top_left + curve_height);
        if self.corner_radius > Pixels::ZERO {
            builder.curve_to(first_top_left + top_curve_width, first_top_left);
        }
        builder.line_to(first_top_right - top_curve_width);

        if let Ok(path) = builder.build() {
            window.paint_path(path, self.color);
        }
    }
}

enum CursorPopoverType {
    CodeContextMenu,
}

pub fn register_action<T: Action>(
    editor: &Entity<Editor>,
    window: &mut Window,
    listener: impl Fn(&mut Editor, &T, &mut Window, &mut Context<Editor>) + 'static,
) {
    register_action_erased(
        editor,
        window,
        TypeId::of::<T>(),
        Box::new(move |editor, action, window, cx| listener(editor, action.downcast_ref().unwrap(), window, cx)),
    )
}

fn register_action_erased(
    editor: &Entity<Editor>,
    window: &mut Window,
    action_type: TypeId,
    listener: Box<dyn Fn(&mut Editor, &dyn std::any::Any, &mut Window, &mut Context<Editor>)>,
) {
    let editor = editor.clone();
    window.on_action(action_type, move |action, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            editor.update(cx, |editor, cx| {
                listener(editor, action, window, cx);
            })
        }
    })
}

/// Shared between `prepaint` and `compute_auto_height_layout` to ensure
/// both full and auto-height editors compute wrap widths consistently.
fn calculate_wrap_width(soft_wrap: SoftWrap, editor_width: Pixels, em_width: Pixels) -> Option<Pixels> {
    let wrap_width_for = |column: u32| (column as f32 * em_width).ceil();

    match soft_wrap {
        SoftWrap::GitDiff => None,
        SoftWrap::None => Some(wrap_width_for(MAX_LINE_LEN as u32 / 2)),
        SoftWrap::EditorWidth => Some(editor_width),
        SoftWrap::Bounded(column) => Some(editor_width.min(wrap_width_for(column))),
    }
}

fn compute_auto_height_layout(
    editor: &mut Editor,
    min_lines: usize,
    max_lines: Option<usize>,
    known_dimensions: Size<Option<Pixels>>,
    available_width: AvailableSpace,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<Size<Pixels>> {
    let width = known_dimensions.width.or({
        if let AvailableSpace::Definite(available_width) = available_width { Some(available_width) } else { None }
    })?;
    if let Some(height) = known_dimensions.height {
        return Some(size(width, height));
    }

    let style = editor.style.as_ref().unwrap();
    let font_id = window.text_system().resolve_font(&style.text.font());
    let font_size = style.text.font_size.to_pixels(window.rem_size());
    let line_height = style.text.line_height_in_pixels(window.rem_size());
    let em_width = window.text_system().em_width(font_id, font_size).unwrap();

    let mut snapshot = editor.snapshot(window, cx);
    let gutter_dimensions = snapshot.gutter_dimensions(font_id, font_size, style, window, cx);

    editor.gutter_dimensions = gutter_dimensions;
    let text_width = width - gutter_dimensions.width;
    let overscroll = size(em_width, px(0.));

    let editor_width = text_width - gutter_dimensions.margin - overscroll.width - em_width;
    let wrap_width =
        calculate_wrap_width(editor.soft_wrap_mode(cx), editor_width, em_width).map(|width| width.min(editor_width));
    if wrap_width.is_some() && editor.set_wrap_width(wrap_width, cx) {
        snapshot = editor.snapshot(window, cx);
    }

    let scroll_height = (snapshot.max_point().row().next_row().0 as f32) * line_height;

    let min_height = line_height * min_lines as f32;
    let content_height = scroll_height.max(min_height);

    let final_height = if let Some(max_lines) = max_lines {
        let max_height = line_height * max_lines as f32;
        content_height.min(max_height)
    } else {
        content_height
    };

    Some(size(width, final_height))
}
