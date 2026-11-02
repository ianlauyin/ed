use workspace::preview::PreviewProvider;

use crate::svg_preview_view::SvgPreviewView;

/// Previews SVG files, detected by file name.
pub(crate) const PROVIDER: PreviewProvider = PreviewProvider {
    label: |item, cx| {
        item.act_as::<multi_buffer::MultiBuffer>(cx)
            .filter(|buffer| SvgPreviewView::is_svg_file(buffer, cx))
            .map(|_| "Preview SVG")
    },
    open: |workspace, item, pane, to_the_side, window, cx| {
        let Some(buffer) =
            item.act_as::<multi_buffer::MultiBuffer>(cx).filter(|buffer| SvgPreviewView::is_svg_file(buffer, cx))
        else {
            return;
        };
        if to_the_side {
            SvgPreviewView::open_preview_to_the_side_of_pane(workspace, buffer, pane, window, cx);
        } else {
            SvgPreviewView::open_preview_in_pane(workspace, buffer, pane, window, cx);
        }
    },
};
