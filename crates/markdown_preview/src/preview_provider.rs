use editor::Editor;
use gpui::Entity;
use ui::prelude::*;
use workspace::ItemHandle;
use workspace::preview::PreviewProvider;

use crate::markdown_preview_view::MarkdownPreviewView;

/// Previews Markdown and HTML files.
pub(crate) const PROVIDER: PreviewProvider = PreviewProvider {
    label: |item, cx| {
        previewable_editor(item, cx).map(|editor| tooltip(MarkdownPreviewView::is_html_file(&editor, cx)))
    },
    open: |workspace, item, pane, to_the_side, window, cx| {
        let Some(editor) = previewable_editor(item, cx) else {
            return;
        };
        if to_the_side {
            MarkdownPreviewView::open_preview_to_the_side_of_pane(workspace, editor, pane, window, cx);
        } else {
            MarkdownPreviewView::open_preview_in_pane(workspace, editor, pane, window, cx);
        }
    },
};

/// The preview acts as its source editor, so it is excluded explicitly.
fn previewable_editor(item: &dyn ItemHandle, cx: &App) -> Option<Entity<Editor>> {
    if item.act_as::<MarkdownPreviewView>(cx).is_some() {
        return None;
    }
    item.act_as::<Editor>(cx).filter(|editor| MarkdownPreviewView::is_previewable_file(editor, cx))
}

const fn tooltip(is_html: bool) -> &'static str {
    if is_html { "Preview HTML" } else { "Preview Markdown" }
}

#[cfg(test)]
mod tests {
    use super::tooltip;

    #[test]
    fn test_tooltip() {
        assert_eq!(tooltip(false), "Preview Markdown");
        assert_eq!(tooltip(true), "Preview HTML");
    }
}
