use gpui::App;
use gpui::actions;
use workspace::Workspace;

mod preview_provider;
pub mod svg_preview_view;

actions!(
    svg,
    [
        /// Opens a following SVG preview that syncs with the editor.
        OpenFollowingPreview
    ]
);

pub fn init(cx: &mut App) {
    workspace::preview::register_preview_provider(preview_provider::PROVIDER, cx);
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        crate::svg_preview_view::SvgPreviewView::register(workspace, window, cx);
    })
    .detach();
}
