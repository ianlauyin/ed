use editor::Editor;
use workspace::preview::PreviewProvider;

use crate::TabularDataPreviewPane;

/// Previews CSV-like files, detected by file extension.
pub(crate) const PROVIDER: PreviewProvider = PreviewProvider {
    label: |item, cx| {
        item.act_as::<Editor>(cx)
            .filter(|editor| TabularDataPreviewPane::is_tabular_data_file(editor, cx))
            .map(|_| "Preview Tabular Data")
    },
    open: |workspace, item, pane, to_the_side, window, cx| {
        let Some(editor) =
            item.act_as::<Editor>(cx).filter(|editor| TabularDataPreviewPane::is_tabular_data_file(editor, cx))
        else {
            return;
        };
        if to_the_side {
            TabularDataPreviewPane::open_preview_to_the_side_of_pane(workspace, editor, pane, window, cx);
        } else {
            TabularDataPreviewPane::open_preview_in_pane(editor, pane, window, cx);
        }
    },
};
