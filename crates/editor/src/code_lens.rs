use futures::future::join_all;
use gpui::App;
use gpui::Entity;
use gpui::SharedString;
use gpui::Task;
use gpui::TaskExt;
use itertools::Itertools;
use language::Buffer;
use language::ClientCommand;
use project::CodeAction;
use project::LspAction;
use text::OffsetRangeExt as _;
use text::ToOffset as _;
use text::ToPoint as _;
use ui::Context;
use ui::Window;

use crate::Editor;
use crate::SelectionEffects;
use crate::hover_links::HoverLink;

static EMPTY_LENS_FALLBACK_TITLE: SharedString = SharedString::new_static("0 references");

/// A resolved code lens shown at the top of the hover popover.
#[derive(Clone, Debug)]
pub(crate) struct HoverCodeLens {
    pub(crate) title: SharedString,
    /// `None` for lenses the server resolved without a command: shown, but not clickable.
    pub(crate) action: Option<CodeAction>,
}

pub(super) fn try_handle_client_command(
    action: &CodeAction,
    editor: &mut Editor,
    workspace: &gpui::Entity<workspace::Workspace>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    let Some(command) = action.lsp_action.command() else {
        return false;
    };

    let arguments = command.arguments.as_deref().unwrap_or_default();
    let project = workspace.read(cx).project().clone();
    let client_command = project
        .read(cx)
        .lsp_store()
        .read(cx)
        .language_server_adapter_for_id(action.server_id)
        .and_then(|adapter| adapter.adapter.client_command(&command.command, arguments))
        .or(match command.command.as_str() {
            "editor.action.showReferences" | "editor.action.goToLocations" | "editor.action.peekLocations" => {
                Some(ClientCommand::ShowLocations)
            }
            _ => None,
        });

    match client_command {
        Some(ClientCommand::ShowLocations) => try_show_references(arguments, action, editor, window, cx),
        None => false,
    }
}

fn try_show_references(
    arguments: &[serde_json::Value],
    action: &CodeAction,
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    if arguments.len() < 3 {
        return false;
    }
    let Ok(locations) = serde_json::from_value::<Vec<lsp::Location>>(arguments[2].clone()) else {
        return false;
    };
    if locations.is_empty() {
        return false;
    }

    let server_id = action.server_id;
    let nav_entry = editor.navigation_entry(editor.selections.newest_anchor().head(), cx);
    let links = locations.into_iter().map(|location| HoverLink::LspLocation(location, server_id)).collect();
    editor.navigate_to_hover_links(None, links, nav_entry, false, window, cx).detach_and_log_err(cx);

    true
}

impl Editor {
    /// Fetches code lenses covering `position` and resolves them, for the hover popover.
    ///
    /// Lenses are anchored at definitions (e.g. an item's name), so only hovers there get any.
    pub(crate) fn hover_code_lenses(
        &self,
        buffer: &Entity<Buffer>,
        position: text::Anchor,
        cx: &mut App,
    ) -> Option<Task<Vec<HoverCodeLens>>> {
        if !self.lsp_data_enabled() {
            return None;
        }
        let lsp_store = self.project.as_ref()?.read(cx).lsp_store();
        let fetch_task = lsp_store.update(cx, |lsp_store, cx| lsp_store.code_lens_actions(buffer, cx));
        let buffer = buffer.clone();
        Some(cx.spawn(async move |cx| {
            let actions = match fetch_task.await {
                Ok(Some(actions)) => actions,
                Ok(None) => return Vec::new(),
                Err(e) => {
                    log::error!("Failed to fetch code lenses for hover: {e:#}");
                    return Vec::new();
                }
            };
            let resolves = lsp_store.update(cx, |lsp_store, cx| {
                let snapshot = buffer.read(cx).snapshot();
                let offset = position.to_offset(&snapshot);
                let row = position.to_point(&snapshot).row;
                actions
                    .into_iter()
                    .filter(|(_, action)| {
                        let range = action.range.to_offset(&snapshot);
                        if range.is_empty() {
                            range.start.to_point(&snapshot).row == row
                        } else {
                            range.start <= offset && offset <= range.end
                        }
                    })
                    .sorted_by_key(|(id, _)| *id)
                    .map(|(id, action)| {
                        let needs_resolve = !action.resolved
                            && matches!(&action.lsp_action, LspAction::CodeLens(lens) if lens.command.is_none());
                        let resolve =
                            needs_resolve.then(|| lsp_store.resolve_code_lens(&buffer, action.server_id, id, cx));
                        async move {
                            match resolve {
                                Some(task) => task.await.map(|(_, resolved)| resolved),
                                None => Some(action),
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            });
            join_all(resolves)
                .await
                .into_iter()
                .flatten()
                .filter_map(|action| {
                    let LspAction::CodeLens(lens) = &action.lsp_action else {
                        return None;
                    };
                    match lens.command.as_ref().filter(|command| !command.title.is_empty()) {
                        Some(command) => {
                            Some(HoverCodeLens { title: SharedString::from(&command.title), action: Some(action) })
                        }
                        None => action
                            .resolved
                            .then(|| HoverCodeLens { title: EMPTY_LENS_FALLBACK_TITLE.clone(), action: None }),
                    }
                })
                .collect()
        }))
    }

    pub(crate) fn run_code_lens_action(&mut self, action: CodeAction, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.buffer().read(cx).snapshot(cx);
        if let Some(position) = snapshot.anchor_in_excerpt(action.range.start) {
            self.change_selections(SelectionEffects::default(), window, cx, |s| {
                s.select_anchor_ranges([position..position]);
            });
        }
        let Some(workspace) = self.workspace() else {
            return;
        };
        if try_handle_client_command(&action, self, &workspace, window, cx) {
            return;
        }
        let project = workspace.read(cx).project().clone();
        if let Some(buffer) = self.buffer().read(cx).buffer(action.range.start.buffer_id) {
            project
                .update(cx, |project, cx| project.apply_code_action(buffer, action, true, cx))
                .detach_and_log_err(cx);
        }
    }
}
