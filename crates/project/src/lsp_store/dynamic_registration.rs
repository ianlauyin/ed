//! Handling of `client/registerCapability` and `client/unregisterCapability` requests,
//! which let language servers toggle their capabilities at runtime.
//!
//! Multiple registrations of the same capability may coexist: the most recent one is
//! "active" and mirrored into the server's [`lsp::ServerCapabilities`], while the rest
//! (including the statically declared capability, if any) are kept to be restored when
//! the active registration is unregistered.

use anyhow::Context as _;
use anyhow::Result;
use collections::HashMap;
use collections::HashSet;
use collections::IndexMap;
use gpui::Context;
use gpui::SharedString;
use lsp::DiagnosticServerCapabilities;
use lsp::LanguageServer;
use lsp::LanguageServerId;
use lsp::OneOf;
use lsp::TextDocumentSyncSaveOptions;

use crate::lsp_store::LanguageServerState;
use crate::lsp_store::LspStore;
use crate::lsp_store::RenamePathsWatchedForServer;
use crate::lsp_store::completion_trigger_characters_for_buffer;
use crate::lsp_store::lsp_workspace_diagnostics_refresh;

#[derive(Debug)]
pub(super) enum RegistrationSource {
    Static,
    Dynamic(String),
}

impl RegistrationSource {
    pub(super) fn registration_id(&self) -> Option<&str> {
        match self {
            RegistrationSource::Static => None,
            RegistrationSource::Dynamic(id) => Some(id),
        }
    }
}

pub(super) type CapabilityRegistrations<T> = Vec<(RegistrationSource, T)>;

#[derive(Debug, Clone, Copy)]
struct CapabilityRegistrationChange {
    active_capability_changed: bool,
    text_document_registration_changed: bool,
    registration_changed: bool,
}

impl CapabilityRegistrationChange {
    const fn applicability_may_have_changed(self) -> bool {
        self.active_capability_changed || self.text_document_registration_changed
    }

    const fn ordered_capability_may_have_changed(self) -> bool {
        self.active_capability_changed || self.registration_changed
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct DynamicTextDocumentRegistration {
    pub(super) document_selector: Option<lsp::DocumentSelector>,
    pub(super) server_capabilities: lsp::ServerCapabilities,
}

pub(super) type TextDocumentRegistrations = HashMap<String, IndexMap<String, DynamicTextDocumentRegistration>>;

#[derive(Default, Debug)]
pub(super) struct DynamicRegistrations {
    pub(super) did_change_watched_files: HashSet<String>,
    pub(super) text_documents: TextDocumentRegistrations,
    workspace_folders: CapabilityRegistrations<lsp::WorkspaceFoldersServerCapabilities>,
    workspace_symbol: CapabilityRegistrations<OneOf<bool, lsp::WorkspaceSymbolOptions>>,
    file_operations: CapabilityRegistrations<lsp::WorkspaceFileOperationsServerCapabilities>,
    execute_command: CapabilityRegistrations<lsp::ExecuteCommandOptions>,
}

impl LspStore {
    fn register_dynamic_text_document_capability<T: Clone + PartialEq>(
        &mut self,
        server: &LanguageServer,
        method: &str,
        registration_id: String,
        options: T,
        document_selector: Option<lsp::DocumentSelector>,
        cx: &mut Context<Self>,
        capability_of: impl Fn(&mut lsp::ServerCapabilities) -> &mut Option<T>,
    ) -> anyhow::Result<CapabilityRegistrationChange> {
        let server_id = server.server_id();
        let local = self.as_local_mut().context("Expected LSP Store to be local")?;
        let mut server_capabilities = lsp::ServerCapabilities::default();
        *capability_of(&mut server_capabilities) = Some(options);
        let new_registration = DynamicTextDocumentRegistration { document_selector, server_capabilities };
        let registrations = local
            .language_server_dynamic_registrations
            .entry(server_id)
            .or_default()
            .text_documents
            .entry(method.to_owned())
            .or_default();
        let Some(applicable_set_changed) =
            insert_text_document_registration(registrations, method, registration_id, new_registration)
        else {
            return Ok(CapabilityRegistrationChange {
                active_capability_changed: false,
                text_document_registration_changed: false,
                registration_changed: false,
            });
        };
        let new_active = active_text_document_capability(
            registrations,
            local.initial_server_capabilities.get(&server_id),
            &capability_of,
        );
        let active_changed = update_active_capability(server, new_active, &capability_of);
        // Remote clients track registrations by id, so any stored change is notified.
        self.notify_server_capabilities_updated(server, cx);
        Ok(CapabilityRegistrationChange {
            active_capability_changed: active_changed,
            text_document_registration_changed: applicable_set_changed,
            registration_changed: true,
        })
    }

    fn unregister_dynamic_text_document_capability<T: Clone + PartialEq>(
        &mut self,
        server: &LanguageServer,
        unregistration: &lsp::Unregistration,
        cx: &mut Context<Self>,
        capability_of: impl Fn(&mut lsp::ServerCapabilities) -> &mut Option<T>,
    ) -> anyhow::Result<Option<CapabilityRegistrationChange>> {
        let server_id = server.server_id();
        let local = self.as_local_mut().context("Expected LSP Store to be local")?;
        let Some((registrations, applicable_set_changed)) = local
            .language_server_dynamic_registrations
            .get_mut(&server_id)
            .and_then(|dynamic_registrations| dynamic_registrations.text_documents.get_mut(&unregistration.method))
            .and_then(|registrations| {
                let applicable_set_changed = remove_text_document_registration(registrations, &unregistration.id)?;
                Some((registrations, applicable_set_changed))
            })
        else {
            log_unknown_unregistration(unregistration);
            return Ok(None);
        };
        let new_active = active_text_document_capability(
            registrations,
            local.initial_server_capabilities.get(&server_id),
            &capability_of,
        );
        let active_changed = update_active_capability(server, new_active, &capability_of);
        self.notify_server_capabilities_updated(server, cx);
        Ok(Some(CapabilityRegistrationChange {
            active_capability_changed: active_changed,
            text_document_registration_changed: applicable_set_changed,
            registration_changed: true,
        }))
    }

    /// Returns `true` when the registration changed the server's active capability value:
    /// duplicate-ID replacements of non-active registrations and (re-)registrations with
    /// options identical to the active ones do not.
    fn register_dynamic_capability<T: Clone + PartialEq>(
        &mut self,
        server: &LanguageServer,
        method: &str,
        registration_id: String,
        options: T,
        cx: &mut Context<Self>,
        registrations_of: impl FnOnce(&mut DynamicRegistrations) -> &mut CapabilityRegistrations<T>,
        capability_of: impl Fn(&mut lsp::ServerCapabilities) -> &mut Option<T>,
    ) -> anyhow::Result<bool> {
        let server_id = server.server_id();
        let local = self.as_local_mut().context("Expected LSP Store to be local")?;
        let dynamic_registrations = local.language_server_dynamic_registrations.entry(server_id).or_default();
        let registrations = registrations_of(dynamic_registrations);
        if registrations.is_empty() {
            let mut initial_capabilities = server.capabilities();
            if let Some(static_options) = capability_of(&mut initial_capabilities).take() {
                registrations.push((RegistrationSource::Static, static_options));
            }
        }
        if !insert_capability_registration(registrations, method, registration_id, options) {
            return Ok(false);
        }
        let active = registrations.last().map(|(_, options)| options.clone());
        server.update_capabilities(|capabilities| *capability_of(capabilities) = active);
        self.notify_server_capabilities_updated(server, cx);
        Ok(true)
    }

    fn unregister_dynamic_capability<T: Clone + PartialEq>(
        &mut self,
        server: &LanguageServer,
        unregistration: &lsp::Unregistration,
        cx: &mut Context<Self>,
        registrations_of: impl FnOnce(&mut DynamicRegistrations) -> &mut CapabilityRegistrations<T>,
        capability_of: impl FnOnce(&mut lsp::ServerCapabilities) -> &mut Option<T>,
    ) -> anyhow::Result<Option<bool>> {
        let server_id = server.server_id();
        let local = self.as_local_mut().context("Expected LSP Store to be local")?;
        let Some((registrations, active_changed)) =
            local.language_server_dynamic_registrations.get_mut(&server_id).map(registrations_of).and_then(
                |registrations| {
                    let active_changed = remove_capability_registration(registrations, &unregistration.id)?;
                    Some((registrations, active_changed))
                },
            )
        else {
            log_unknown_unregistration(unregistration);
            return Ok(None);
        };
        if !active_changed {
            return Ok(Some(false));
        }
        let restored = registrations.last().map(|(_, options)| options.clone());
        server.update_capabilities(|capabilities| *capability_of(capabilities) = restored);
        self.notify_server_capabilities_updated(server, cx);
        Ok(Some(true))
    }

    fn update_paths_watched_for_rename(&mut self, server: &LanguageServer) {
        let Some(local) = self.as_local_mut() else {
            return;
        };
        let watcher = server.capabilities().workspace.and_then(|workspace| workspace.file_operations).and_then(
            |file_operations| {
                let did_rename_caps = file_operations.did_rename.as_ref();
                let will_rename_caps = file_operations.will_rename.as_ref();
                did_rename_caps.or(will_rename_caps)?;
                Some(
                    RenamePathsWatchedForServer::default()
                        .with_did_rename_patterns(did_rename_caps)
                        .with_will_rename_patterns(will_rename_caps),
                )
            },
        );
        match watcher {
            Some(watcher) => {
                local.language_server_paths_watched_for_rename.insert(server.server_id(), watcher);
            }
            None => {
                local.language_server_paths_watched_for_rename.remove(&server.server_id());
            }
        }
    }

    fn apply_completion_triggers(&self, server_id: LanguageServerId, cx: &mut Context<Self>) {
        let Some(local) = self.as_local() else {
            return;
        };
        let Some(LanguageServerState::Running { adapter, .. }) = local.language_servers.get(&server_id) else {
            return;
        };
        let mut buffers_with_language_server = Vec::new();
        for handle in self.buffer_store.read(cx).buffers() {
            let buffer_id = handle.read(cx).remote_id();
            if local.buffers_opened_in_servers.get(&buffer_id).filter(|servers| servers.contains(&server_id)).is_some()
            {
                buffers_with_language_server.push(handle);
            }
        }
        for handle in buffers_with_language_server {
            handle.update(cx, |buffer, cx| {
                let triggers = completion_trigger_characters_for_buffer(local, server_id, adapter, buffer);
                buffer.set_completion_triggers(server_id, triggers, cx);
            });
        }
    }

    pub(super) fn register_server_capabilities(
        &mut self,
        server_id: LanguageServerId,
        params: lsp::RegistrationParams,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let server = self.language_server_for_id(server_id).with_context(|| format!("no server {server_id} found"))?;
        for reg in params.registrations {
            match reg.method.as_str() {
                "workspace/didChangeWatchedFiles" => {
                    if let Some(options) = reg.register_options {
                        let notify = if let Some(local_lsp_store) = self.as_local_mut() {
                            let caps = serde_json::from_value(options)?;
                            local_lsp_store.on_lsp_did_change_watched_files(server_id, &reg.id, caps, cx);
                            true
                        } else {
                            false
                        };
                        if notify {
                            self.notify_server_capabilities_updated(&server, cx);
                        }
                    }
                }
                "workspace/didChangeConfiguration" => {
                    // Ignore payload since we notify clients of setting changes unconditionally, relying on them pulling the latest settings.
                }
                "workspace/didChangeWorkspaceFolders" => {
                    // In this case register options is an empty object, we can ignore it
                    let caps = lsp::WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Right(reg.id.clone())),
                    };
                    self.register_dynamic_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        caps,
                        cx,
                        |registrations| &mut registrations.workspace_folders,
                        |capabilities| &mut capabilities.workspace.get_or_insert_default().workspace_folders,
                    )?;
                }
                "workspace/symbol" => {
                    let options = parse_register_capabilities(reg.register_options)?;
                    self.register_dynamic_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        options,
                        cx,
                        |registrations| &mut registrations.workspace_symbol,
                        |capabilities| &mut capabilities.workspace_symbol_provider,
                    )?;
                }
                "workspace/fileOperations" => {
                    if let Some(options) = reg.register_options {
                        let caps = serde_json::from_value(options)?;
                        if self.register_dynamic_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            caps,
                            cx,
                            |registrations| &mut registrations.file_operations,
                            |capabilities| &mut capabilities.workspace.get_or_insert_default().file_operations,
                        )? {
                            self.update_paths_watched_for_rename(&server);
                        }
                    }
                }
                "workspace/executeCommand" => {
                    if let Some(options) = reg.register_options {
                        let options = serde_json::from_value(options)?;
                        self.register_dynamic_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            options,
                            cx,
                            |registrations| &mut registrations.execute_command,
                            |capabilities| &mut capabilities.execute_command_provider,
                        )?;
                    }
                }
                "textDocument/rangeFormatting" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    self.register_dynamic_text_document_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        options,
                        document_selector,
                        cx,
                        |capabilities| &mut capabilities.document_range_formatting_provider,
                    )?;
                }
                "textDocument/onTypeFormatting" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    if let Some(options) = reg.register_options.map(serde_json::from_value).transpose()? {
                        self.register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            options,
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.document_on_type_formatting_provider,
                        )?;
                    }
                }
                "textDocument/formatting" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    self.register_dynamic_text_document_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        options,
                        document_selector,
                        cx,
                        |capabilities| &mut capabilities.document_formatting_provider,
                    )?;
                }
                "textDocument/rename" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    self.register_dynamic_text_document_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        options,
                        document_selector,
                        cx,
                        |capabilities| &mut capabilities.rename_provider,
                    )?;
                }
                "textDocument/inlayHint" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    if self
                        .register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            options,
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.inlay_hint_provider,
                        )?
                        .applicability_may_have_changed()
                    {
                        self.refresh_inlay_hints(server_id, cx);
                    }
                }
                "textDocument/documentSymbol" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    if self
                        .register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            options,
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.document_symbol_provider,
                        )?
                        .applicability_may_have_changed()
                    {
                        self.refresh_document_symbols(Some(server_id), cx);
                    }
                }
                "textDocument/codeAction" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    let provider = match options {
                        OneOf::Left(value) => lsp::CodeActionProviderCapability::Simple(value),
                        OneOf::Right(caps) => caps,
                    };
                    self.register_dynamic_text_document_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        provider,
                        document_selector,
                        cx,
                        |capabilities| &mut capabilities.code_action_provider,
                    )?;
                }
                "textDocument/definition" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    self.register_dynamic_text_document_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        options,
                        document_selector,
                        cx,
                        |capabilities| &mut capabilities.definition_provider,
                    )?;
                }
                "textDocument/documentHighlight" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    if self
                        .register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            options,
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.document_highlight_provider,
                        )?
                        .applicability_may_have_changed()
                    {
                        self.refresh_document_highlights(Some(server_id), cx);
                    }
                }
                "textDocument/completion" => {
                    if let Some(registration_options) = reg
                        .register_options
                        .map(serde_json::from_value::<lsp::CompletionRegistrationOptions>)
                        .transpose()?
                    {
                        self.register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            registration_options.completion_options,
                            registration_options.text_document_registration_options.document_selector,
                            cx,
                            |capabilities| &mut capabilities.completion_provider,
                        )?;
                        self.apply_completion_triggers(server_id, cx);
                    }
                }
                "textDocument/hover" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    let provider = match options {
                        OneOf::Left(value) => lsp::HoverProviderCapability::Simple(value),
                        OneOf::Right(caps) => caps,
                    };
                    self.register_dynamic_text_document_capability(
                        &server,
                        &reg.method,
                        reg.id,
                        provider,
                        document_selector,
                        cx,
                        |capabilities| &mut capabilities.hover_provider,
                    )?;
                }
                "textDocument/didChange" => {
                    if let Some(sync_kind) = reg
                        .register_options
                        .and_then(|opts| opts.get("syncKind").cloned())
                        .map(serde_json::from_value::<lsp::TextDocumentSyncKind>)
                        .transpose()?
                    {
                        server.update_capabilities(|capabilities| {
                            let mut sync_options = take_text_document_sync_options(capabilities);
                            sync_options.change = Some(sync_kind);
                            capabilities.text_document_sync =
                                Some(lsp::TextDocumentSyncCapability::Options(sync_options));
                        });
                        self.notify_server_capabilities_updated(&server, cx);
                    }
                }
                "textDocument/didSave" => {
                    if let Some(include_text) = reg
                        .register_options
                        .map(|opts| {
                            let transpose = opts
                                .get("includeText")
                                .cloned()
                                .map(serde_json::from_value::<Option<bool>>)
                                .transpose();
                            match transpose {
                                Ok(value) => Ok(value.flatten()),
                                Err(e) => Err(e),
                            }
                        })
                        .transpose()?
                    {
                        server.update_capabilities(|capabilities| {
                            let mut sync_options = take_text_document_sync_options(capabilities);
                            sync_options.save =
                                Some(TextDocumentSyncSaveOptions::SaveOptions(lsp::SaveOptions { include_text }));
                            capabilities.text_document_sync =
                                Some(lsp::TextDocumentSyncCapability::Options(sync_options));
                        });
                        self.notify_server_capabilities_updated(&server, cx);
                    }
                }
                "textDocument/codeLens" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    if let Some(options) = reg.register_options.map(serde_json::from_value).transpose()?
                        && self
                            .register_dynamic_text_document_capability(
                                &server,
                                &reg.method,
                                reg.id,
                                options,
                                document_selector,
                                cx,
                                |capabilities| &mut capabilities.code_lens_provider,
                            )?
                            .applicability_may_have_changed()
                    {
                        self.refresh_code_lens(Some(server_id), cx);
                    }
                }
                "textDocument/diagnostic" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    if let Some(caps) =
                        reg.register_options.map(serde_json::from_value::<DiagnosticServerCapabilities>).transpose()?
                    {
                        let registration_id = reg.id.clone();
                        self.register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            caps.clone(),
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.diagnostic_provider,
                        )?;

                        let supports_workspace_diagnostics =
                            |capabilities: &DiagnosticServerCapabilities| match capabilities {
                                DiagnosticServerCapabilities::Options(diagnostic_options) => {
                                    diagnostic_options.workspace_diagnostics
                                }
                                DiagnosticServerCapabilities::RegistrationOptions(diagnostic_registration_options) => {
                                    diagnostic_registration_options.diagnostic_options.workspace_diagnostics
                                }
                            };

                        let local = self.as_local_mut().context("Expected LSP Store to be local")?;
                        let state = local
                            .language_servers
                            .get_mut(&server_id)
                            .context("Could not obtain Language Servers state")?;
                        if let LanguageServerState::Running { workspace_diagnostics_refresh_tasks, .. } = state {
                            workspace_diagnostics_refresh_tasks.remove(&Some(registration_id.clone()));
                            if supports_workspace_diagnostics(&caps)
                                && let Some(task) = lsp_workspace_diagnostics_refresh(
                                    Some(registration_id.clone()),
                                    caps,
                                    server.clone(),
                                    cx,
                                )
                            {
                                workspace_diagnostics_refresh_tasks.insert(Some(registration_id), task);
                            }
                        }

                        drop(self.pull_document_diagnostics_for_server(server_id, None, cx));
                    }
                }
                "textDocument/documentColor" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    let provider = match options {
                        OneOf::Left(value) => lsp::ColorProviderCapability::Simple(value),
                        OneOf::Right(caps) => caps,
                    };
                    if self
                        .register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            provider,
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.color_provider,
                        )?
                        .applicability_may_have_changed()
                    {
                        self.refresh_document_colors(Some(server_id), cx);
                    }
                }
                "textDocument/foldingRange" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    let options = parse_register_capabilities(reg.register_options)?;
                    let provider = match options {
                        OneOf::Left(value) => lsp::FoldingRangeProviderCapability::Simple(value),
                        OneOf::Right(caps) => caps,
                    };
                    if self
                        .register_dynamic_text_document_capability(
                            &server,
                            &reg.method,
                            reg.id,
                            provider,
                            document_selector,
                            cx,
                            |capabilities| &mut capabilities.folding_range_provider,
                        )?
                        .applicability_may_have_changed()
                    {
                        self.refresh_folding_ranges(Some(server_id), cx);
                    }
                }
                "textDocument/documentLink" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    if let Some(caps) = reg.register_options.map(serde_json::from_value).transpose()?
                        && self
                            .register_dynamic_text_document_capability(
                                &server,
                                &reg.method,
                                reg.id,
                                caps,
                                document_selector,
                                cx,
                                |capabilities| &mut capabilities.document_link_provider,
                            )?
                            .applicability_may_have_changed()
                    {
                        self.refresh_document_links(Some(server_id), cx);
                    }
                }
                "textDocument/semanticTokens" => {
                    let document_selector = parse_text_document_registration(reg.register_options.as_ref())?;
                    if let Some(caps) = reg
                        .register_options
                        .map(serde_json::from_value::<lsp::SemanticTokensRegistrationOptions>)
                        .transpose()?
                        && self
                            .register_dynamic_text_document_capability(
                                &server,
                                &reg.method,
                                reg.id,
                                lsp::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(caps),
                                document_selector,
                                cx,
                                |capabilities| &mut capabilities.semantic_tokens_provider,
                            )?
                            .ordered_capability_may_have_changed()
                    {
                        // Re-query already-open buffers, which would otherwise keep
                        // tree-sitter-only highlighting until edited.
                        self.refresh_semantic_tokens(server_id, cx);
                    }
                }
                _ => log::warn!("unhandled capability registration: {reg:?}"),
            }
        }

        Ok(())
    }

    pub(super) fn unregister_server_capabilities(
        &mut self,
        server_id: LanguageServerId,
        params: lsp::UnregistrationParams,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let server = self.language_server_for_id(server_id).with_context(|| format!("no server {server_id} found"))?;
        for unreg in &params.unregisterations {
            match unreg.method.as_str() {
                "workspace/didChangeWatchedFiles" => {
                    let notify = if let Some(local_lsp_store) = self.as_local_mut() {
                        local_lsp_store.on_lsp_unregister_did_change_watched_files(server_id, &unreg.id, cx);
                        true
                    } else {
                        false
                    };
                    if notify {
                        self.notify_server_capabilities_updated(&server, cx);
                    }
                }
                "workspace/didChangeConfiguration" => {
                    // Ignore payload since we notify clients of setting changes unconditionally, relying on them pulling the latest settings.
                }
                "workspace/didChangeWorkspaceFolders" => {
                    self.unregister_dynamic_capability(
                        &server,
                        unreg,
                        cx,
                        |registrations| &mut registrations.workspace_folders,
                        |capabilities| &mut capabilities.workspace.get_or_insert_default().workspace_folders,
                    )?;
                }
                "workspace/symbol" => {
                    self.unregister_dynamic_capability(
                        &server,
                        unreg,
                        cx,
                        |registrations| &mut registrations.workspace_symbol,
                        |capabilities| &mut capabilities.workspace_symbol_provider,
                    )?;
                }
                "workspace/fileOperations" => {
                    if self.unregister_dynamic_capability(
                        &server,
                        unreg,
                        cx,
                        |registrations| &mut registrations.file_operations,
                        |capabilities| &mut capabilities.workspace.get_or_insert_default().file_operations,
                    )? == Some(true)
                    {
                        self.update_paths_watched_for_rename(&server);
                    }
                }
                "workspace/executeCommand" => {
                    self.unregister_dynamic_capability(
                        &server,
                        unreg,
                        cx,
                        |registrations| &mut registrations.execute_command,
                        |capabilities| &mut capabilities.execute_command_provider,
                    )?;
                }
                "textDocument/rangeFormatting" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.document_range_formatting_provider
                    })?;
                }
                "textDocument/onTypeFormatting" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.document_on_type_formatting_provider
                    })?;
                }
                "textDocument/formatting" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.document_formatting_provider
                    })?;
                }
                "textDocument/rename" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.rename_provider
                    })?;
                }
                "textDocument/codeAction" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.code_action_provider
                    })?;
                }
                "textDocument/definition" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.definition_provider
                    })?;
                }
                "textDocument/documentHighlight" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.document_highlight_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_document_highlights(Some(server_id), cx);
                    }
                }
                "textDocument/completion" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.completion_provider
                        })?
                        .is_some()
                    {
                        self.apply_completion_triggers(server_id, cx);
                    }
                }
                "textDocument/hover" => {
                    self.unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                        &mut capabilities.hover_provider
                    })?;
                }
                "textDocument/semanticTokens" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.semantic_tokens_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::ordered_capability_may_have_changed)
                    {
                        self.refresh_semantic_tokens(server_id, cx);
                    }
                }
                "textDocument/didChange" => {
                    server.update_capabilities(|capabilities| {
                        let mut sync_options = take_text_document_sync_options(capabilities);
                        sync_options.change = None;
                        capabilities.text_document_sync = Some(lsp::TextDocumentSyncCapability::Options(sync_options));
                    });
                    self.notify_server_capabilities_updated(&server, cx);
                }
                "textDocument/didSave" => {
                    server.update_capabilities(|capabilities| {
                        let mut sync_options = take_text_document_sync_options(capabilities);
                        sync_options.save = None;
                        capabilities.text_document_sync = Some(lsp::TextDocumentSyncCapability::Options(sync_options));
                    });
                    self.notify_server_capabilities_updated(&server, cx);
                }
                "textDocument/inlayHint" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.inlay_hint_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_inlay_hints(server_id, cx);
                    }
                }
                "textDocument/documentSymbol" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.document_symbol_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_document_symbols(Some(server_id), cx);
                    }
                }
                "textDocument/codeLens" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.code_lens_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_code_lens(Some(server_id), cx);
                    }
                }
                "textDocument/diagnostic" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.diagnostic_provider
                        })?
                        .is_some()
                    {
                        let local = self.as_local_mut().context("Expected LSP Store to be local")?;
                        let state = local
                            .language_servers
                            .get_mut(&server_id)
                            .context("Could not obtain Language Servers state")?;
                        if let LanguageServerState::Running { workspace_diagnostics_refresh_tasks, .. } = state {
                            workspace_diagnostics_refresh_tasks.remove(&Some(unreg.id.clone()));
                        }

                        self.clear_unregistered_diagnostics(server_id, SharedString::from(unreg.id.clone()), cx)?;
                    }
                }
                "textDocument/documentColor" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.color_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_document_colors(Some(server_id), cx);
                    }
                }
                "textDocument/foldingRange" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.folding_range_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_folding_ranges(Some(server_id), cx);
                    }
                }
                "textDocument/documentLink" => {
                    if self
                        .unregister_dynamic_text_document_capability(&server, unreg, cx, |capabilities| {
                            &mut capabilities.document_link_provider
                        })?
                        .is_some_and(CapabilityRegistrationChange::applicability_may_have_changed)
                    {
                        self.refresh_document_links(Some(server_id), cx);
                    }
                }
                _ => log::warn!("unhandled capability unregistration: {unreg:?}"),
            }
        }

        Ok(())
    }
}

// Servers like Biome unregister capabilities they never registered and re-register existing IDs
// whenever their configuration loads, so neither is worth a warning.
fn log_unknown_unregistration(unregistration: &lsp::Unregistration) {
    log::debug!(
        "Ignoring unregistration of unknown {} registration with ID {}",
        unregistration.method,
        unregistration.id
    );
}

/// Returns `None` when an identical registration with the same ID is already stored, otherwise
/// whether the set of (selector, options) values that buffer caches consume changed.
fn insert_text_document_registration(
    registrations: &mut IndexMap<String, DynamicTextDocumentRegistration>,
    method: &str,
    registration_id: String,
    new_registration: DynamicTextDocumentRegistration,
) -> Option<bool> {
    let applicable_set_changed = match registrations.get(&registration_id) {
        Some(previous_registration) if previous_registration == &new_registration => return None,
        Some(previous_registration) => {
            log::debug!("Replacing {method} registration with duplicate ID {registration_id}");
            let mut other_registrations =
                registrations.iter().filter(|(id, _)| **id != registration_id).map(|(_, registration)| registration);
            let previous_value_remains =
                other_registrations.clone().any(|registration| registration == previous_registration);
            let new_value_exists = other_registrations.any(|registration| registration == &new_registration);
            !previous_value_remains || !new_value_exists
        }
        None => !registrations.values().any(|registration| registration == &new_registration),
    };
    registrations.insert(registration_id, new_registration);
    Some(applicable_set_changed)
}

/// Returns `None` when no registration has the ID, otherwise whether the set of
/// (selector, options) values changed.
fn remove_text_document_registration(
    registrations: &mut IndexMap<String, DynamicTextDocumentRegistration>,
    registration_id: &str,
) -> Option<bool> {
    let removed = registrations.shift_remove(registration_id)?;
    Some(!registrations.values().any(|remaining| remaining == &removed))
}

/// The most recent registration wins; with none left, the statically declared capability applies.
fn active_text_document_capability<T>(
    registrations: &IndexMap<String, DynamicTextDocumentRegistration>,
    initial_capabilities: Option<&lsp::ServerCapabilities>,
    capability_of: impl Fn(&mut lsp::ServerCapabilities) -> &mut Option<T>,
) -> Option<T> {
    match registrations.last() {
        Some((_, registration)) => capability_of(&mut registration.server_capabilities.clone()).take(),
        None => initial_capabilities.cloned().and_then(|mut capabilities| capability_of(&mut capabilities).take()),
    }
}

/// Returns whether the server's capability differed from `active` and was updated.
fn update_active_capability<T: PartialEq>(
    server: &LanguageServer,
    active: Option<T>,
    capability_of: impl Fn(&mut lsp::ServerCapabilities) -> &mut Option<T>,
) -> bool {
    let mut current_capabilities = server.capabilities();
    if *capability_of(&mut current_capabilities) == active {
        return false;
    }
    server.update_capabilities(|capabilities| *capability_of(capabilities) = active);
    true
}

/// Returns whether the active (most recent) registration's options changed.
fn insert_capability_registration<T: PartialEq>(
    registrations: &mut CapabilityRegistrations<T>,
    method: &str,
    registration_id: String,
    options: T,
) -> bool {
    match registrations.iter().position(|(source, _)| source.registration_id() == Some(registration_id.as_str())) {
        Some(index) => {
            if registrations[index].1 == options {
                return false;
            }
            log::debug!("Replacing {method} registration with duplicate ID {registration_id}");
            registrations[index].1 = options;
            index + 1 == registrations.len()
        }
        None => {
            let active_changed = registrations.last().is_none_or(|(_, active)| active != &options);
            registrations.push((RegistrationSource::Dynamic(registration_id), options));
            active_changed
        }
    }
}

/// Returns `None` when no registration has the ID, otherwise whether the active options changed.
fn remove_capability_registration<T: PartialEq>(
    registrations: &mut CapabilityRegistrations<T>,
    registration_id: &str,
) -> Option<bool> {
    let index = registrations.iter().position(|(source, _)| source.registration_id() == Some(registration_id))?;
    let (_, removed) = registrations.remove(index);
    Some(index == registrations.len() && registrations.last().is_none_or(|(_, restored)| restored != &removed))
}

fn parse_text_document_registration(
    register_options: Option<&serde_json::Value>,
) -> Result<Option<lsp::DocumentSelector>> {
    Ok(register_options
        .cloned()
        .map(serde_json::from_value::<lsp::TextDocumentRegistrationOptions>)
        .transpose()?
        .and_then(|options| options.document_selector))
}

// Registration with registerOptions as null, should fallback to true.
// https://github.com/microsoft/vscode-languageserver-node/blob/d90a87f9557a0df9142cfb33e251cfa6fe27d970/client/src/common/client.ts#L2133
fn parse_register_capabilities<T: serde::de::DeserializeOwned>(
    register_options: Option<serde_json::Value>,
) -> Result<OneOf<bool, T>> {
    Ok(match register_options {
        Some(options) => OneOf::Right(serde_json::from_value::<T>(options)?),
        None => OneOf::Left(true),
    })
}

fn take_text_document_sync_options(capabilities: &mut lsp::ServerCapabilities) -> lsp::TextDocumentSyncOptions {
    match capabilities.text_document_sync.take() {
        Some(lsp::TextDocumentSyncCapability::Options(sync_options)) => sync_options,
        Some(lsp::TextDocumentSyncCapability::Kind(sync_kind)) => {
            lsp::TextDocumentSyncOptions { change: Some(sync_kind), ..Default::default() }
        }
        None => lsp::TextDocumentSyncOptions::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition_registration(document_selector: Option<lsp::DocumentSelector>) -> DynamicTextDocumentRegistration {
        DynamicTextDocumentRegistration {
            document_selector,
            server_capabilities: lsp::ServerCapabilities {
                definition_provider: Some(OneOf::Left(true)),
                ..lsp::ServerCapabilities::default()
            },
        }
    }

    fn rust_selector() -> Option<lsp::DocumentSelector> {
        Some(vec![lsp::DocumentFilter { language: Some("rust".to_owned()), scheme: None, pattern: None }])
    }

    fn capability_registrations(entries: &[(Option<&str>, i32)]) -> CapabilityRegistrations<i32> {
        entries
            .iter()
            .map(|(id, options)| {
                let source = match id {
                    Some(id) => RegistrationSource::Dynamic((*id).to_owned()),
                    None => RegistrationSource::Static,
                };
                (source, *options)
            })
            .collect()
    }

    fn capability_values(registrations: &CapabilityRegistrations<i32>) -> Vec<(Option<&str>, i32)> {
        registrations.iter().map(|(source, options)| (source.registration_id(), *options)).collect()
    }

    #[test]
    fn insert_text_document_registration_reports_applicable_set_changes() {
        let mut registrations = IndexMap::default();
        assert_eq!(
            insert_text_document_registration(&mut registrations, "m", "a".to_owned(), definition_registration(None)),
            Some(true)
        );
        // Same value under a new ID leaves the value set unchanged.
        assert_eq!(
            insert_text_document_registration(&mut registrations, "m", "b".to_owned(), definition_registration(None)),
            Some(false)
        );
        assert_eq!(registrations.len(), 2);
    }

    #[test]
    fn insert_text_document_registration_skips_identical_duplicate() {
        let mut registrations = IndexMap::default();
        insert_text_document_registration(&mut registrations, "m", "a".to_owned(), definition_registration(None));
        assert_eq!(
            insert_text_document_registration(&mut registrations, "m", "a".to_owned(), definition_registration(None)),
            None
        );
        assert_eq!(registrations.len(), 1);
    }

    #[test]
    fn insert_text_document_registration_replaces_duplicate_id() {
        let mut registrations = IndexMap::default();
        insert_text_document_registration(&mut registrations, "m", "a".to_owned(), definition_registration(None));
        insert_text_document_registration(&mut registrations, "m", "b".to_owned(), definition_registration(None));
        // "b" still holds the old value, so only the new value is an addition.
        assert_eq!(
            insert_text_document_registration(
                &mut registrations,
                "m",
                "a".to_owned(),
                definition_registration(rust_selector())
            ),
            Some(true)
        );
        assert_eq!(registrations.len(), 2);
        assert_eq!(registrations["a"], definition_registration(rust_selector()));
        // Replacing with a value that both existed before and still remains is not a set change.
        assert_eq!(
            insert_text_document_registration(&mut registrations, "m", "b".to_owned(), definition_registration(None)),
            None
        );
        insert_text_document_registration(&mut registrations, "m", "c".to_owned(), definition_registration(None));
        assert_eq!(
            insert_text_document_registration(
                &mut registrations,
                "m",
                "c".to_owned(),
                definition_registration(rust_selector())
            ),
            Some(false)
        );
    }

    #[test]
    fn remove_text_document_registration_reports_applicable_set_changes() {
        let mut registrations = IndexMap::default();
        insert_text_document_registration(&mut registrations, "m", "a".to_owned(), definition_registration(None));
        insert_text_document_registration(&mut registrations, "m", "b".to_owned(), definition_registration(None));
        assert_eq!(remove_text_document_registration(&mut registrations, "missing"), None);
        assert_eq!(remove_text_document_registration(&mut registrations, "a"), Some(false));
        assert_eq!(remove_text_document_registration(&mut registrations, "b"), Some(true));
        assert_eq!(remove_text_document_registration(&mut registrations, "b"), None);
    }

    #[test]
    fn active_text_document_capability_prefers_latest_registration() {
        fn definition_of(
            capabilities: &mut lsp::ServerCapabilities,
        ) -> &mut Option<OneOf<bool, lsp::DefinitionOptions>> {
            &mut capabilities.definition_provider
        }
        let initial = lsp::ServerCapabilities {
            definition_provider: Some(OneOf::Left(false)),
            ..lsp::ServerCapabilities::default()
        };
        let mut registrations = IndexMap::default();
        assert_eq!(active_text_document_capability(&registrations, None, definition_of), None);
        assert_eq!(
            active_text_document_capability(&registrations, Some(&initial), definition_of),
            Some(OneOf::Left(false))
        );
        registrations.insert("a".to_owned(), definition_registration(None));
        assert_eq!(
            active_text_document_capability(&registrations, Some(&initial), definition_of),
            Some(OneOf::Left(true))
        );
    }

    #[test]
    fn insert_capability_registration_tracks_active_options() {
        let mut registrations = capability_registrations(&[(None, 1)]);
        assert!(!insert_capability_registration(&mut registrations, "m", "a".to_owned(), 1));
        assert!(insert_capability_registration(&mut registrations, "m", "b".to_owned(), 2));
        assert_eq!(capability_values(&registrations), [(None, 1), (Some("a"), 1), (Some("b"), 2)]);

        let mut registrations = CapabilityRegistrations::<i32>::new();
        assert!(insert_capability_registration(&mut registrations, "m", "a".to_owned(), 1));
    }

    #[test]
    fn insert_capability_registration_handles_duplicate_ids() {
        let mut registrations = capability_registrations(&[(Some("a"), 1), (Some("b"), 2)]);
        assert!(!insert_capability_registration(&mut registrations, "m", "b".to_owned(), 2));
        assert!(!insert_capability_registration(&mut registrations, "m", "a".to_owned(), 3));
        assert!(insert_capability_registration(&mut registrations, "m", "b".to_owned(), 4));
        assert_eq!(capability_values(&registrations), [(Some("a"), 3), (Some("b"), 4)]);
    }

    #[test]
    fn remove_capability_registration_restores_previous_options() {
        let mut registrations = capability_registrations(&[(None, 1), (Some("a"), 2), (Some("b"), 2), (Some("c"), 3)]);
        assert_eq!(remove_capability_registration(&mut registrations, "missing"), None);
        assert_eq!(remove_capability_registration(&mut registrations, "a"), Some(false));
        assert_eq!(remove_capability_registration(&mut registrations, "c"), Some(true));
        assert_eq!(capability_values(&registrations), [(None, 1), (Some("b"), 2)]);
        assert_eq!(remove_capability_registration(&mut registrations, "b"), Some(true));

        let mut registrations = capability_registrations(&[(Some("a"), 1), (Some("b"), 1)]);
        assert_eq!(remove_capability_registration(&mut registrations, "b"), Some(false));
        assert_eq!(remove_capability_registration(&mut registrations, "a"), Some(true));
        assert!(registrations.is_empty());
    }
}
