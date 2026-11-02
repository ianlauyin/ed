use editor::Editor;
use editor::EditorEvent;
use gpui::Action;
use gpui::AnyView;
use gpui::AppContext;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::Task;
use serde::Deserialize;
use ui::SharedString;
use ui::prelude::*;
use workspace::Item;
use workspace::Pane;
use workspace::Workspace;

use crate::parser::EditorState;
pub use crate::table_view::CellSelection;
pub use crate::table_view::TableView;

mod parser;
mod preview_provider;
mod renderer;
mod settings;
mod table_data_engine;
mod table_view;
pub mod types;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NavigationDirection {
    #[default]
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Deserialize, PartialEq, Action)]
#[action(namespace = tabular_data)]
pub struct MoveFocusedCell {
    pub direction: NavigationDirection,
}

/// Editor-backed adapter: watches an [`Editor`], parses its buffer into a [`crate::types::TableLikeContent`],
/// and feeds the result to an embedded [`TableView`] that owns all grid rendering.
pub struct TabularDataPreviewPane {
    /// The reusable tabular viewer this adapter drives.
    pub(crate) table: Entity<TableView>,
    active_editor_state: EditorState,
    pub(crate) parsing_task: Option<Task<anyhow::Result<()>>>,
    pub(crate) is_parsing: bool,
    pub(crate) parse_error: Option<SharedString>,
    /// Time when the last parsing operation ended, used for smart debouncing
    pub(crate) last_parse_end_time: Option<std::time::Instant>,
    /// Forwards the table's notifications so observers of this pane (e.g. `cx.condition` in
    /// tests, or anything watching for the mapping/list-state to settle) see them too.
    _table_subscription: gpui::Subscription,
}

pub fn init(cx: &mut App) {
    workspace::preview::register_preview_provider(preview_provider::PROVIDER, cx);
}

impl TabularDataPreviewPane {
    pub fn open_preview_in_pane(
        editor: Entity<Editor>,
        pane: Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::activate_or_add_preview(editor, pane, true, window, cx);
    }

    pub fn open_preview_to_the_side_of_pane(
        workspace: &mut Workspace,
        editor: Entity<Editor>,
        origin_pane: Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let target_pane = workspace.adjacent_pane_of(&origin_pane, window, cx);
        Self::activate_or_add_preview(editor.clone(), target_pane, false, window, cx);
        editor.focus_handle(cx).focus(window, cx);
    }

    fn activate_or_add_preview(
        editor: Entity<Editor>,
        pane: Entity<Pane>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let existing_view_idx = Self::find_existing_preview_item_idx(pane.read(cx), &editor, cx);
        if let Some(existing_view_idx) = existing_view_idx {
            pane.update(cx, |pane, cx| {
                pane.activate_item(existing_view_idx, focus, focus, window, cx);
            });
        } else {
            let preview_pane = Self::new(&editor, window, cx);
            pane.update(cx, |pane, cx| {
                pane.add_item(Box::new(preview_pane), focus, focus, None, window, cx);
            });
        }
        cx.notify();
    }

    fn find_existing_preview_item_idx(pane: &Pane, editor: &Entity<Editor>, cx: &App) -> Option<usize> {
        pane.items_of_type::<TabularDataPreviewPane>()
            .find(|view| &view.read(cx).active_editor_state.editor == editor)
            .and_then(|view| pane.index_for_item(&view))
    }

    fn new(editor: &Entity<Editor>, window: &Window, cx: &mut Context<Workspace>) -> Entity<Self> {
        cx.new(|cx| {
            let subscription =
                cx.subscribe(editor, |this: &mut TabularDataPreviewPane, _editor, event: &EditorEvent, cx| {
                    match event {
                        EditorEvent::Edited { .. } | EditorEvent::DirtyChanged => {
                            this.parse_from_active_editor(true, cx);
                        }
                        _ => {}
                    };
                });

            let table = cx.new(|cx| TableView::new(window, cx));
            let table_subscription = cx.observe(&table, |_, _, cx| cx.notify());

            let mut view = TabularDataPreviewPane {
                active_editor_state: EditorState { editor: editor.clone(), _subscription: subscription },
                table,
                parsing_task: None,
                is_parsing: false,
                parse_error: None,
                last_parse_end_time: None,
                _table_subscription: table_subscription,
            };

            view.parse_from_active_editor(false, cx);
            view
        })
    }

    pub(crate) const fn editor_state(&self) -> &EditorState {
        &self.active_editor_state
    }

    pub fn is_tabular_data_file(editor: &Entity<Editor>, cx: &App) -> bool {
        parser::TabularFormat::from_editor(editor, cx).is_some()
    }
}

impl Focusable for TabularDataPreviewPane {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.table.read(cx).focus_handle(cx)
    }
}

impl Render for TabularDataPreviewPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(match &self.parse_error {
            Some(error) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .text_color(cx.theme().status().error)
                .child(error.clone())
                .into_any_element(),
            None => self.table.clone().into_any_element(),
        })
    }
}

impl EventEmitter<()> for TabularDataPreviewPane {}

impl Item for TabularDataPreviewPane {
    type Event = ();

    /// A preview of a Git History revision keeps that commit's sidebar.
    fn side_card(&self, cx: &App) -> Option<AnyView> {
        Item::side_card(self.active_editor_state.editor.read(cx), cx)
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Table))
    }

    fn tab_content_text(&self, _detail: usize, cx: &App) -> SharedString {
        self.editor_state()
            .editor
            .read(cx)
            .buffer()
            .read(cx)
            .as_singleton()
            .and_then(|b| {
                let file = b.read(cx).file()?;
                let local_file = file.as_local()?;
                local_file.abs_path(cx).file_name().map(|name| format!("Preview {}", name.to_string_lossy()).into())
            })
            .unwrap_or_else(|| SharedString::from("Tabular Data Preview"))
    }
}
