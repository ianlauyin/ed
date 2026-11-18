use std::collections::BTreeSet;

use gpui::App;
use gpui::Entity;
use language::Buffer;
use language::CachedLspAdapter;
use language::LanguageName;
use lsp::AdapterServerCapabilities;
use lsp::LanguageServer;
use lsp::LanguageServerId;
use lsp::request::Request;
use rustc_hash::FxHashSet;

use super::LanguageServerState;
use super::LocalLspStore;
use super::LspStore;
use super::dynamic_registration;
use crate::lsp_command::LspCommand;

#[derive(Clone, Debug)]
pub(super) struct DocumentSelectorContext {
    language_id: String,
    scheme: &'static str,
}

fn document_selector_context_for_buffer(
    buffer: &Buffer,
    adapter: &CachedLspAdapter,
) -> Option<DocumentSelectorContext> {
    let language = buffer.language()?;
    Some(document_selector_context_for_language(&language.name(), adapter))
}

pub(super) fn document_selector_context_for_language(
    language: &LanguageName,
    adapter: &CachedLspAdapter,
) -> DocumentSelectorContext {
    DocumentSelectorContext { language_id: adapter.language_id(language), scheme: "file" }
}

pub(super) fn document_selector_matches(
    document_selector: Option<&lsp::DocumentSelector>,
    context: &DocumentSelectorContext,
) -> bool {
    let Some(document_selector) = document_selector else {
        return true;
    };

    document_selector.iter().any(|filter| {
        if let Some(language) = &filter.language
            && language != &context.language_id
        {
            return false;
        }

        if let Some(scheme) = &filter.scheme
            && !scheme.eq_ignore_ascii_case(context.scheme)
        {
            return false;
        }

        true
    })
}

pub(super) fn dynamic_text_document_registration_allows_buffer(
    registration: &dynamic_registration::DynamicTextDocumentRegistration,
    buffer: &Buffer,
    adapter: &CachedLspAdapter,
) -> bool {
    let Some(context) = document_selector_context_for_buffer(buffer, adapter) else {
        return true;
    };

    document_selector_matches(registration.document_selector.as_ref(), &context)
}

fn text_document_registration_method(method: &str) -> &str {
    // Some related requests have distinct methods but share one registration method.
    match method {
        "textDocument/prepareRename" => "textDocument/rename",
        "textDocument/semanticTokens/full"
        | "textDocument/semanticTokens/full/delta"
        | "textDocument/semanticTokens/range" => "textDocument/semanticTokens",
        _ => method,
    }
}

pub(super) fn text_document_capabilities_for_buffer<'a>(
    local: &'a LocalLspStore,
    method: &str,
    buffer: &'a Buffer,
    adapter: &'a CachedLspAdapter,
    server: &'a LanguageServer,
) -> impl Iterator<Item = AdapterServerCapabilities<'a>> {
    let code_action_kinds = server.code_action_kinds();
    let static_capabilities = local
        .initial_server_capabilities
        .get(&server.server_id())
        .map(move |server_capabilities| AdapterServerCapabilities { server_capabilities, code_action_kinds });

    let registration_method = text_document_registration_method(method);
    let dynamic_capabilities = local
        .language_server_dynamic_registrations
        .get(&server.server_id())
        .and_then(|registrations| registrations.text_documents.get(registration_method))
        .into_iter()
        .flat_map(|registrations| registrations.values())
        .filter(move |registration| dynamic_text_document_registration_allows_buffer(registration, buffer, adapter))
        .map(move |registration| AdapterServerCapabilities {
            server_capabilities: &registration.server_capabilities,
            code_action_kinds,
        });

    static_capabilities.into_iter().chain(dynamic_capabilities)
}

pub(super) fn applicable_lsp_command_capabilities_for_buffer<'a, R>(
    local: &'a LocalLspStore,
    request: &'a R,
    buffer: &'a Buffer,
    adapter: &'a CachedLspAdapter,
    server: &'a LanguageServer,
) -> impl Iterator<Item = AdapterServerCapabilities<'a>>
where
    R: LspCommand,
{
    text_document_capabilities_for_buffer(local, <R::LspRequest as Request>::METHOD, buffer, adapter, server)
        .filter(move |capabilities| request.check_capabilities(*capabilities))
}

pub(super) fn lsp_command_allowed_for_buffer<R>(
    local: &LocalLspStore,
    request: &R,
    buffer: &Buffer,
    adapter: &CachedLspAdapter,
    server: &LanguageServer,
) -> bool
where
    R: LspCommand,
{
    applicable_lsp_command_capabilities_for_buffer(local, request, buffer, adapter, server).next().is_some()
}

pub(super) fn completion_trigger_characters_for_buffer(
    local: &LocalLspStore,
    server_id: LanguageServerId,
    adapter: &CachedLspAdapter,
    buffer: &Buffer,
) -> BTreeSet<String> {
    fn extend_triggers(triggers: &mut BTreeSet<String>, options: &lsp::CompletionOptions) {
        if let Some(characters) = &options.trigger_characters {
            triggers.extend(characters.iter().cloned());
        }
    }

    let mut triggers = BTreeSet::new();
    if let Some(options) = local
        .initial_server_capabilities
        .get(&server_id)
        .and_then(|capabilities| capabilities.completion_provider.as_ref())
    {
        extend_triggers(&mut triggers, options);
    }
    if let Some(registrations) = local
        .language_server_dynamic_registrations
        .get(&server_id)
        .and_then(|registrations| registrations.text_documents.get("textDocument/completion"))
    {
        for registration in registrations.values() {
            if dynamic_text_document_registration_allows_buffer(registration, buffer, adapter)
                && let Some(options) = registration.server_capabilities.completion_provider.as_ref()
            {
                extend_triggers(&mut triggers, options);
            }
        }
    }
    triggers
}

impl LspStore {
    pub fn relevant_server_ids_for_capability_check(
        &self,
        buffer: &Entity<Buffer>,
        cx: &App,
    ) -> FxHashSet<LanguageServerId> {
        let buffer_id = buffer.read(cx).buffer_id();
        self.local.buffers_opened_in_servers.get(&buffer_id).into_iter().flatten().copied().collect()
    }

    pub(crate) fn text_document_capability_matches_for_server(
        &self,
        buffer: &Entity<Buffer>,
        server_id: LanguageServerId,
        method: &str,
        mut check: impl FnMut(AdapterServerCapabilities<'_>) -> bool,
        cx: &mut App,
    ) -> bool {
        let local = &self.local;
        buffer.update(cx, |buffer, cx| {
            let Some((adapter, server)) =
                local.language_servers_for_buffer(buffer, cx).find(|(_, server)| server.server_id() == server_id)
            else {
                return false;
            };
            text_document_capabilities_for_buffer(local, method, buffer, adapter, server).any(&mut check)
        })
    }

    pub(super) fn check_if_any_relevant_text_document_server_matches<F>(
        &self,
        buffer: &Entity<Buffer>,
        method: &str,
        mut check: F,
        cx: &App,
    ) -> bool
    where
        F: FnMut(&lsp::LanguageServerName, AdapterServerCapabilities<'_>) -> bool,
    {
        let local = &self.local;
        let buffer = buffer.read(cx);
        local
            .buffers_opened_in_servers
            .get(&buffer.buffer_id())
            .into_iter()
            .flatten()
            .filter_map(|server_id| match local.language_servers.get(server_id)? {
                LanguageServerState::Running { adapter, server, .. } => Some((adapter, server)),
                LanguageServerState::Starting { .. } => None,
            })
            .any(|(adapter, server)| {
                let server_name = server.name();
                text_document_capabilities_for_buffer(local, method, buffer, adapter, server)
                    .any(|capabilities| check(&server_name, capabilities))
            })
    }
}
