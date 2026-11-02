use gpui::AnyElement;
use gpui::App;
use gpui::Context;
use gpui::Entity;
use gpui::Global;
use gpui::Window;
use gpui::actions;
use ui::IconButton;
use ui::IconName;
use ui::IconSize;
use ui::Tooltip;
use ui::prelude::*;

use crate::ItemHandle;
use crate::Pane;
use crate::Workspace;

actions!(
    preview,
    [
        /// Opens a preview of the current file (Markdown/HTML, SVG or tabular data).
        OpenPreview,
        /// Opens a preview of the current file in a split pane.
        OpenPreviewToTheSide,
    ]
);

/// A kind of file preview, registered by the crate that renders it. The first
/// registered provider that can preview an item wins.
pub struct PreviewProvider {
    /// The tab bar button's tooltip for `item`, `None` when it can't be previewed.
    pub label: fn(&dyn ItemHandle, &App) -> Option<&'static str>,
    /// Opens the preview of `item` in `pane`, or beside `pane` when `to_the_side`.
    pub open: OpenPreviewFn,
}

pub type OpenPreviewFn = fn(&mut Workspace, &dyn ItemHandle, Entity<Pane>, bool, &mut Window, &mut Context<Workspace>);

#[derive(Default)]
struct PreviewProviders(Vec<PreviewProvider>);

impl Global for PreviewProviders {}

pub fn register_preview_provider(provider: PreviewProvider, cx: &mut App) {
    cx.default_global::<PreviewProviders>().0.push(provider);
}

fn find_provider<'a>(item: &dyn ItemHandle, cx: &'a App) -> Option<(&'a PreviewProvider, &'static str)> {
    let providers = cx.try_global::<PreviewProviders>()?;
    providers.0.iter().find_map(|provider| (provider.label)(item, cx).map(|label| (provider, label)))
}

/// Whether `item` has a preview. Panes only handle the preview actions then,
/// so menus can check `Window::is_action_available`.
pub(crate) fn has_preview(item: &dyn ItemHandle, cx: &App) -> bool {
    find_provider(item, cx).is_some()
}

pub(crate) fn open_preview(
    workspace: &mut Workspace,
    item: &dyn ItemHandle,
    pane: Entity<Pane>,
    to_the_side: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some((provider, _)) = find_provider(item, cx) {
        let open = provider.open;
        open(workspace, item, pane, to_the_side, window, cx);
    }
}

/// The tab bar's eye button for an item that has a preview; Option-click opens
/// it in a split.
pub(crate) fn render_tab_bar_preview_button(pane: &Pane, cx: &App) -> Option<AnyElement> {
    let item = pane.active_item()?;
    let (_, label) = find_provider(item.as_ref(), cx)?;
    let workspace = pane.workspace.clone();
    Some(
        IconButton::new("toggle-preview", IconName::Eye)
            .icon_size(IconSize::Small)
            .tooltip(move |_window, cx| {
                Tooltip::with_meta(
                    label,
                    Some(&OpenPreview),
                    split_hint(&ui::text_for_keystroke(&gpui::Modifiers::alt(), "click", cx)),
                    cx,
                )
            })
            .on_click(move |_, window, cx| {
                let to_the_side = window.modifiers().alt;
                workspace
                    .update(cx, |workspace, cx| {
                        if let Some(pane) = workspace.pane_for(item.as_ref()) {
                            open_preview(workspace, item.as_ref(), pane, to_the_side, window, cx);
                        }
                    })
                    .ok();
            })
            .into_any_element(),
    )
}

fn split_hint(alt_click: &str) -> String {
    format!("{alt_click} to open in a split")
}

#[cfg(test)]
mod tests {
    use super::split_hint;

    #[test]
    fn test_split_hint() {
        assert_eq!(split_hint("⌥-click"), "⌥-click to open in a split");
    }
}
