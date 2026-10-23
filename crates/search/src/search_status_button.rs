use editor::EditorSettings;
use gpui::App;
use gpui::FocusHandle;
use settings::Settings as _;
use ui::ButtonCommon;
use ui::Clickable;
use ui::Context;
use ui::Render;
use ui::Tooltip;
use ui::Window;
use ui::prelude::*;
use workspace::HideStatusItem;
use workspace::ItemHandle;
use workspace::StatusItemView;

pub const SEARCH_ICON: IconName = IconName::MagnifyingGlass;

pub struct SearchButton {
    pane_item_focus_handle: Option<FocusHandle>,
}

impl SearchButton {
    pub const fn new() -> Self {
        Self { pane_item_focus_handle: None }
    }
}

impl Default for SearchButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for SearchButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl ui::IntoElement {
        let button = div();

        if !EditorSettings::get_global(cx).search.button {
            return button.hidden();
        }

        let focus_handle = self.pane_item_focus_handle.clone();
        button.child(
            IconButton::new("project-search-indicator", SEARCH_ICON)
                .icon_size(IconSize::Small)
                .tab_index(0isize)
                .tooltip(move |_window, cx| {
                    if let Some(focus_handle) = &focus_handle {
                        Tooltip::for_action_in("Project Search", &workspace::DeploySearch::default(), focus_handle, cx)
                    } else {
                        Tooltip::for_action("Project Search", &workspace::DeploySearch::default(), cx)
                    }
                })
                .on_click(cx.listener(|_this, _, window, cx| {
                    window.dispatch_action(Box::new(workspace::DeploySearch::default()), cx);
                })),
        )
    }
}

impl StatusItemView for SearchButton {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane_item_focus_handle = active_pane_item.map(|item| item.item_focus_handle(cx));
    }

    fn hide_setting(&self, _: &App) -> Option<HideStatusItem> {
        Some(HideStatusItem::new(|settings| {
            settings.editor.search.get_or_insert_default().button = Some(false);
        }))
    }
}
