use gpui::Context;
use gpui::Point;
use gpui::Window;

use crate::Autoscroll;
use crate::Editor;
use crate::EditorMode;
use crate::NextScreen;
use crate::NextScrollCursorCenterTopBottom;
use crate::SCROLL_CENTER_TOP_BOTTOM_DEBOUNCE_TIMEOUT;
use crate::ScrollCursorBottom;
use crate::ScrollCursorCenter;
use crate::ScrollCursorCenterTopBottom;
use crate::ScrollCursorTop;
use crate::display_map::DisplayRow;
use crate::scroll::ScrollOffset;

impl Editor {
    pub fn next_screen(&mut self, _: &NextScreen, window: &mut Window, cx: &mut Context<Editor>) {
        if self.take_rename(true, window, cx).is_some() {
            return;
        }

        if self.mouse_context_menu.is_some() {
            return;
        }

        if matches!(self.mode, EditorMode::SingleLine) {
            cx.propagate();
            return;
        }
        self.request_autoscroll(Autoscroll::Next, cx);
    }

    pub fn scroll(&mut self, scroll_position: Point<ScrollOffset>, window: &mut Window, cx: &mut Context<Self>) {
        self.set_scroll_position(scroll_position, window, cx);
    }

    pub fn scroll_cursor_center_top_bottom(
        &mut self,
        _: &ScrollCursorCenterTopBottom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.next_scroll_position {
            NextScrollCursorCenterTopBottom::Center => {
                self.scroll_cursor_center(&ScrollCursorCenter, window, cx);
            }
            NextScrollCursorCenterTopBottom::Top => {
                self.scroll_cursor_top(&ScrollCursorTop, window, cx);
            }
            NextScrollCursorCenterTopBottom::Bottom => {
                self.scroll_cursor_bottom(&ScrollCursorBottom, window, cx);
            }
        }

        self.next_scroll_position = self.next_scroll_position.next();
        self._scroll_cursor_center_top_bottom_task = cx.spawn(async move |editor, cx| {
            cx.background_executor().timer(SCROLL_CENTER_TOP_BOTTOM_DEBOUNCE_TIMEOUT).await;
            editor
                .update(cx, |editor, _| {
                    editor.next_scroll_position = NextScrollCursorCenterTopBottom::default();
                })
                .ok();
        });
    }

    pub fn scroll_cursor_top(&mut self, _: &ScrollCursorTop, window: &mut Window, cx: &mut Context<Editor>) {
        let display_snapshot = self.display_snapshot(cx);
        let scroll_margin_rows = self.vertical_scroll_margin() as u32;
        let selection_head = self.selections.newest_display(&display_snapshot).head();

        let new_screen_top = selection_head.row().0;
        let header_offset =
            if display_snapshot.buffer_snapshot().show_headers() { display_snapshot.buffer_header_height() } else { 0 };

        let adjustment = scroll_margin_rows + header_offset;
        let new_screen_top = new_screen_top.saturating_sub(adjustment);
        self.set_scroll_top_row(DisplayRow(new_screen_top), window, cx);
    }

    pub fn scroll_cursor_center(&mut self, _: &ScrollCursorCenter, window: &mut Window, cx: &mut Context<Editor>) {
        let Some(visible_rows) = self.visible_line_count().map(|count| count as u32) else {
            return;
        };
        let new_screen_top = self.selections.newest_display(&self.display_snapshot(cx)).head().row().0;
        let new_screen_top = new_screen_top.saturating_sub(visible_rows / 2);
        self.set_scroll_top_row(DisplayRow(new_screen_top), window, cx);
    }

    pub fn scroll_cursor_bottom(&mut self, _: &ScrollCursorBottom, window: &mut Window, cx: &mut Context<Editor>) {
        let scroll_margin_rows = self.vertical_scroll_margin() as u32;
        let Some(visible_rows) = self.visible_line_count().map(|count| count as u32) else {
            return;
        };
        let new_screen_top = self.selections.newest_display(&self.display_snapshot(cx)).head().row().0;
        let new_screen_top = new_screen_top.saturating_sub(visible_rows.saturating_sub(scroll_margin_rows));
        self.set_scroll_top_row(DisplayRow(new_screen_top), window, cx);
    }
}
