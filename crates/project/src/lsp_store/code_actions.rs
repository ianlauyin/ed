use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;
use std::vec;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::Task;
use gpui::WeakEntity;
use language::Buffer;
use lsp::CodeActionKind;
use lsp::LSP_REQUEST_TIMEOUT;
use lsp::LanguageServer;
use lsp::LanguageServerId;
use lsp::request::CodeActionResolveRequest;
use lsp::request::CodeLensResolve;
use lsp::request::ExecuteCommand;
use rustc_hash::FxHashSet;
use text::Anchor;

use super::LanguageServerToQuery;
use super::LocalLspStore;
use super::LspStore;
use crate::CodeAction;
use crate::LspAction;
use crate::ProjectTransaction;
use crate::lsp_command::GetCodeActions;
use crate::lsp_command::GetCodeLens;

impl LocalLspStore {
    async fn execute_code_action_kind_locally(
        lsp_store: WeakEntity<LspStore>,
        buffers: Vec<Entity<Buffer>>,
        kind: CodeActionKind,
        push_to_history: bool,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<ProjectTransaction> {
        let mut project_transaction = ProjectTransaction::default();

        for buffer in &buffers {
            let adapters_and_servers = lsp_store.update(cx, |lsp_store, cx| {
                buffer.update(cx, |buffer, cx| {
                    lsp_store
                        .local
                        .language_servers_for_buffer(buffer, cx)
                        .map(|(adapter, lsp)| (Arc::clone(adapter), Arc::clone(lsp)))
                        .collect::<Vec<_>>()
                })
            })?;
            for (_, language_server) in &adapters_and_servers {
                let actions = Self::get_server_code_actions_from_action_kinds(
                    &lsp_store,
                    language_server.server_id(),
                    vec![kind.clone()],
                    buffer,
                    cx,
                )
                .await?;
                Self::execute_code_actions_on_server(
                    &lsp_store,
                    language_server,
                    buffer,
                    actions,
                    push_to_history,
                    &mut project_transaction,
                    cx,
                )
                .await?;
            }
        }
        Ok(project_transaction)
    }

    pub(super) async fn try_resolve_code_action(
        lang_server: &LanguageServer,
        action: &mut CodeAction,
        can_resolve: bool,
        request_timeout: Duration,
    ) -> anyhow::Result<()> {
        match &mut action.lsp_action {
            LspAction::Action(lsp_action) => {
                if !action.resolved
                    && can_resolve
                    && lsp_action.data.is_some()
                    && (lsp_action.command.is_none() || lsp_action.edit.is_none())
                {
                    **lsp_action = lang_server
                        .request::<CodeActionResolveRequest>(*lsp_action.clone(), request_timeout)
                        .await
                        .into_response()?;
                }
            }
            LspAction::CodeLens(lens) => {
                if !action.resolved && can_resolve {
                    *lens =
                        lang_server.request::<CodeLensResolve>(lens.clone(), request_timeout).await.into_response()?;
                }
            }
            LspAction::Command(_) => {}
        }

        action.resolved = true;
        anyhow::Ok(())
    }

    pub(super) async fn get_server_code_actions_from_action_kinds(
        lsp_store: &WeakEntity<LspStore>,
        language_server_id: LanguageServerId,
        code_action_kinds: Vec<lsp::CodeActionKind>,
        buffer: &Entity<Buffer>,
        cx: &mut AsyncApp,
    ) -> Result<Vec<CodeAction>> {
        let actions = lsp_store
            .update(cx, move |this, cx| {
                let request = GetCodeActions {
                    range: text::Anchor::min_max_range_for_buffer(buffer.read(cx).buffer_id()),
                    kinds: Some(code_action_kinds),
                };
                let server = LanguageServerToQuery::Other(language_server_id);
                this.request_lsp(buffer.clone(), server, request, cx)
            })?
            .await?;
        Ok(actions)
    }

    pub async fn execute_code_actions_on_server(
        lsp_store: &WeakEntity<LspStore>,
        language_server: &Arc<LanguageServer>,
        buffer: &Entity<Buffer>,
        actions: Vec<CodeAction>,
        push_to_history: bool,
        project_transaction: &mut ProjectTransaction,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        let request_timeout = LSP_REQUEST_TIMEOUT;
        let server_id = language_server.server_id();
        let can_resolve_actions = lsp_store.update(cx, |lsp_store, cx| {
            lsp_store.text_document_capability_matches_for_server(
                buffer,
                server_id,
                "textDocument/codeAction",
                |capabilities| GetCodeActions::can_resolve_actions(capabilities.server_capabilities),
                cx,
            )
        })?;

        for mut action in actions {
            Self::try_resolve_code_action(language_server, &mut action, can_resolve_actions, request_timeout)
                .await
                .context("resolving a formatting code action")?;

            if let Some(edit) = action.lsp_action.edit() {
                if edit.changes.is_none() && edit.document_changes.is_none() {
                    continue;
                }

                let new = Self::deserialize_workspace_edit(
                    lsp_store.upgrade().context("project dropped")?,
                    edit.clone(),
                    push_to_history,
                    Arc::clone(language_server),
                    cx,
                )
                .await?;
                project_transaction.0.extend(new.0);
            }

            let Some(command) = action.lsp_action.command() else {
                continue;
            };

            let server_capabilities = language_server.capabilities();
            let available_commands = server_capabilities
                .execute_command_provider
                .as_ref()
                .map_or_default(|options| options.commands.as_slice());
            if !available_commands.contains(&command.command) {
                log::warn!(
                    "Cannot execute a command {} not listed in the language server capabilities",
                    command.command
                );
                continue;
            }

            lsp_store.update(cx, |lsp_store, _| {
                lsp_store.local.last_workspace_edits_by_language_server.remove(&language_server.server_id());
            })?;

            language_server
                .request::<ExecuteCommand>(
                    lsp::ExecuteCommandParams {
                        command: command.command.clone(),
                        arguments: command.arguments.clone().unwrap_or_default(),
                        ..Default::default()
                    },
                    request_timeout,
                )
                .await
                .into_response()
                .context("execute command")?;

            lsp_store.update(cx, |this, _| {
                project_transaction.0.extend(
                    this.local
                        .last_workspace_edits_by_language_server
                        .remove(&language_server.server_id())
                        .unwrap_or_default()
                        .0,
                );
            })?;
        }
        Ok(())
    }
}

impl LspStore {
    fn can_resolve_lsp_action_for_buffer(&self, buffer: &Entity<Buffer>, action: &CodeAction, cx: &mut App) -> bool {
        match &action.lsp_action {
            LspAction::Action(_) => self.text_document_capability_matches_for_server(
                buffer,
                action.server_id,
                "textDocument/codeAction",
                |capabilities| GetCodeActions::can_resolve_actions(capabilities.server_capabilities),
                cx,
            ),
            LspAction::CodeLens(_) => self.text_document_capability_matches_for_server(
                buffer,
                action.server_id,
                "textDocument/codeLens",
                |capabilities| GetCodeLens::can_resolve_lens(capabilities.server_capabilities),
                cx,
            ),
            LspAction::Command(_) => false,
        }
    }

    pub fn apply_code_action(
        &self,
        buffer_handle: Entity<Buffer>,
        mut action: CodeAction,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        let can_resolve = self.can_resolve_lsp_action_for_buffer(&buffer_handle, &action, cx);
        let Some((_, lang_server, request_timeout)) = buffer_handle.update(cx, |buffer, cx| {
            let request_timeout = LSP_REQUEST_TIMEOUT;
            self.language_server_for_local_buffer(buffer, action.server_id, cx)
                .map(|(adapter, server)| (Arc::clone(adapter), Arc::clone(server), request_timeout))
        }) else {
            return Task::ready(Ok(ProjectTransaction::default()));
        };

        cx.spawn(async move |this, cx| {
            LocalLspStore::try_resolve_code_action(&lang_server, &mut action, can_resolve, request_timeout)
                .await
                .context("resolving a code action")?;
            if let Some(edit) = action.lsp_action.edit()
                && (edit.changes.is_some() || edit.document_changes.is_some())
            {
                return LocalLspStore::deserialize_workspace_edit(
                    this.upgrade().context("no app present")?,
                    edit.clone(),
                    push_to_history,
                    Arc::clone(&lang_server),
                    cx,
                )
                .await;
            }

            let Some(command) = action.lsp_action.command() else {
                return Ok(ProjectTransaction::default());
            };

            let server_capabilities = lang_server.capabilities();
            let available_commands = server_capabilities
                .execute_command_provider
                .as_ref()
                .map_or_default(|options| options.commands.as_slice());

            if !available_commands.contains(&command.command) {
                log::debug!(
                    "Skipping executeCommand for {}, not listed in language server capabilities",
                    command.command
                );
                return Ok(ProjectTransaction::default());
            }

            let request_timeout = LSP_REQUEST_TIMEOUT;

            this.update(cx, |this, _| {
                this.local.last_workspace_edits_by_language_server.remove(&lang_server.server_id());
            })?;

            let _result = lang_server
                .request::<ExecuteCommand>(
                    lsp::ExecuteCommandParams {
                        command: command.command.clone(),
                        arguments: command.arguments.clone().unwrap_or_default(),
                        ..lsp::ExecuteCommandParams::default()
                    },
                    request_timeout,
                )
                .await
                .into_response()
                .context("execute command")?;

            this.update(cx, |this, _| {
                this.local.last_workspace_edits_by_language_server.remove(&lang_server.server_id()).unwrap_or_default()
            })
        })
    }

    pub fn execute_lsp_command(
        &self,
        server_id: LanguageServerId,
        command: String,
        arguments: Vec<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<serde_json::Value>>> {
        let Some(server) = self.language_server_for_id(server_id) else {
            return Task::ready(Err(anyhow!("language server {server_id} not found")));
        };
        let available_commands =
            server.capabilities().execute_command_provider.as_ref().map_or_default(|options| options.commands.clone());
        if !available_commands.contains(&command) {
            return Task::ready(Err(anyhow!("command {command} is not advertised by the language server")));
        }
        let request_timeout = LSP_REQUEST_TIMEOUT;
        cx.background_spawn(async move {
            server
                .request::<ExecuteCommand>(
                    lsp::ExecuteCommandParams { command, arguments, ..lsp::ExecuteCommandParams::default() },
                    request_timeout,
                )
                .await
                .into_response()
        })
    }

    pub fn apply_code_action_kind(
        &mut self,
        buffers: FxHashSet<Entity<Buffer>>,
        kind: CodeActionKind,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<ProjectTransaction>> {
        {
            cx.spawn(async move |lsp_store, cx| {
                let buffers = buffers.into_iter().collect::<Vec<_>>();
                LocalLspStore::execute_code_action_kind_locally(lsp_store, buffers, kind, push_to_history, cx).await
            })
        }
    }

    pub fn code_actions(
        &mut self,
        buffer: &Entity<Buffer>,
        range: Range<Anchor>,
        kinds: Option<Vec<CodeActionKind>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<CodeAction>>>> {
        let all_actions_task = self.request_multiple_lsp_locally(buffer, GetCodeActions { range, kinds }, cx);
        cx.background_spawn(async move {
            Ok(Some(all_actions_task.await.into_iter().flat_map(|(_, actions)| actions).collect()))
        })
    }
}
