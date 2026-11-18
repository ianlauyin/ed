use std::cmp::Reverse;
use std::collections::hash_map;
use std::fmt::Debug;
use std::mem;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use gpui::App;
use gpui::AsyncApp;
use gpui::Entity;
use gpui::SharedString;
use gpui::prelude::FluentBuilder as _;
use language::Anchor;
use language::Bias;
use language::Buffer;
use language::BufferSnapshot;
use language::CachedLspAdapter;
use language::CharKind;
use language::CharScopeContext;
use language::OffsetRangeExt as _;
use language::PointUtf16;
use language::ToOffset as _;
use language::ToPointUtf16 as _;
use language::Transaction;
use language::Unclipped;
use language::language_settings::InlayHintKind;
use language::language_settings::LanguageSettings;
use language::lsp_to_symbol_kind;
use language::point_from_lsp;
use language::point_to_lsp;
use language::range_from_lsp;
use language::range_to_lsp;
use lsp::AdapterServerCapabilities;
use lsp::CodeActionKind;
use lsp::CodeActionOptions;
use lsp::CompletionContext;
use lsp::CompletionListItemDefaultsEditRange;
use lsp::LanguageServer;
use lsp::LanguageServerId;
use lsp::LinkedEditingRangeServerCapabilities;
use lsp::OneOf;
use lsp::RenameOptions;
use lsp::ServerCapabilities;
use lsp::request;
use lsp::request::CodeActionRequest;
use lsp::request::Completion;
use lsp::request::DocumentHighlightRequest;
use lsp::request::DocumentSymbolRequest;
use lsp::request::GotoDeclaration;
use lsp::request::GotoDefinition;
use lsp::request::GotoImplementation;
use lsp::request::GotoTypeDefinition;
use lsp::request::HoverRequest;
use lsp::request::PrepareRenameRequest;
use lsp::request::References;
use lsp::request::Rename;
use lsp::request::Request;
use rustc_hash::FxHashMap;
use text::LineEnding;
use util::ResultExt as _;

use crate::CodeAction;
use crate::CompletionSource;
use crate::CoreCompletion;
use crate::CoreCompletionResponse;
use crate::DocumentColor;
use crate::DocumentHighlight;
use crate::DocumentSymbol;
use crate::Hover;
use crate::HoverBlock;
use crate::HoverBlockKind;
use crate::InlayHint;
use crate::InlayHintLabel;
use crate::InlayHintLabelPart;
use crate::InlayHintLabelPartTooltip;
use crate::InlayHintTextEdits;
use crate::InlayHintTooltip;
use crate::Location;
use crate::LocationLink;
use crate::LspAction;
use crate::LspPullDiagnostics;
use crate::MarkupContent;
use crate::PrepareRenameResponse;
use crate::ProjectTransaction;
use crate::PulledDiagnostics;
use crate::ResolveState;
use crate::lsp_store;
use crate::lsp_store::LanguageServerToQuery;
use crate::lsp_store::LocalLspStore;
use crate::lsp_store::LspDocumentLink;
use crate::lsp_store::LspFoldingRange;
use crate::lsp_store::LspStore;

fn code_action_kind_matches(requested: &lsp::CodeActionKind, actual: &lsp::CodeActionKind) -> bool {
    let requested_str = requested.as_str();
    let actual_str = actual.as_str();

    // Exact match or hierarchical match
    actual_str == requested_str || actual_str.strip_prefix(requested_str).is_some_and(|suffix| suffix.starts_with('.'))
}

pub fn lsp_formatting_options(settings: &LanguageSettings) -> lsp::FormattingOptions {
    lsp::FormattingOptions {
        tab_size: settings.tab_size.into(),
        insert_spaces: !settings.hard_tabs,
        ..lsp::FormattingOptions::default()
    }
}

pub fn file_path_to_lsp_url(path: &Path) -> Result<lsp::Uri> {
    match lsp::Uri::from_file_path(path) {
        Ok(url) => Ok(url),
        Err(()) => anyhow::bail!("Invalid file path provided to LSP request: {path:?}"),
    }
}

pub(crate) fn make_text_document_identifier(path: &Path) -> Result<lsp::TextDocumentIdentifier> {
    Ok(lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(path)? })
}

pub(crate) fn make_lsp_text_document_position(
    path: &Path,
    position: PointUtf16,
) -> Result<lsp::TextDocumentPositionParams> {
    Ok(lsp::TextDocumentPositionParams {
        text_document: make_text_document_identifier(path)?,
        position: point_to_lsp(position),
    })
}

// Futures are polled on the foreground executor, so no `Send` bound is wanted
#[allow(async_fn_in_trait)]
pub trait LspCommand: 'static + Sized + Send + Debug {
    type Response: 'static + Default + Send + Debug;
    type LspRequest: 'static + Send + Request;

    fn display_name(&self) -> &str;

    fn status(&self) -> Option<String> {
        None
    }

    fn language_server_to_query(&self) -> LanguageServerToQuery {
        LanguageServerToQuery::FirstCapable
    }

    /// Returns whether the given static or dynamic capability supports this request.
    fn check_capabilities(&self, _: AdapterServerCapabilities<'_>) -> bool;

    fn response_without_request<'a, I>(&self, _applicable_capabilities: I) -> Option<Self::Response>
    where
        I: Iterator<Item = AdapterServerCapabilities<'a>>,
    {
        None
    }

    fn to_lsp(
        &self,
        path: &Path,
        buffer: &Buffer,
        language_server: &Arc<LanguageServer>,
        cx: &App,
    ) -> Result<<Self::LspRequest as Request>::Params>;

    async fn response_from_lsp(
        self,
        message: <Self::LspRequest as Request>::Result,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Self::Response>;
}

#[derive(Debug)]
pub(crate) struct PrepareRename {
    pub position: PointUtf16,
}

#[derive(Debug)]
pub(crate) struct PerformRename {
    pub position: PointUtf16,
    pub new_name: String,
    pub push_to_history: bool,
    pub language_server_id: Option<LanguageServerId>,
}

#[derive(Debug, Clone, Copy)]
pub struct GetDefinitions {
    pub position: PointUtf16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GetDeclarations {
    pub position: PointUtf16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GetTypeDefinitions {
    pub position: PointUtf16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GetImplementations {
    pub position: PointUtf16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GetReferences {
    pub position: PointUtf16,
}

#[derive(Debug)]
pub(crate) struct GetDocumentHighlights {
    pub position: PointUtf16,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct GetDocumentSymbols;

#[derive(Clone, Debug)]
pub(crate) struct GetHover {
    pub position: PointUtf16,
}

#[derive(Debug)]
pub(crate) struct GetCompletions {
    pub position: PointUtf16,
    pub context: CompletionContext,
}

#[derive(Clone, Debug)]
pub(crate) struct GetCodeActions {
    pub range: Range<Anchor>,
    pub kinds: Option<Vec<lsp::CodeActionKind>>,
}

#[derive(Debug)]
pub(crate) struct OnTypeFormatting {
    pub position: PointUtf16,
    pub trigger: String,
    pub options: lsp::FormattingOptions,
    pub push_to_history: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct InlayHints {
    pub range: Range<Anchor>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SemanticTokensFull;

#[derive(Debug, Clone)]
pub(crate) struct SemanticTokensDelta {
    pub previous_result_id: SharedString,
}

#[derive(Debug)]
pub(crate) enum SemanticTokensResponse {
    Full { data: Vec<u32>, result_id: Option<SharedString> },
    Delta { edits: Vec<SemanticTokensEdit>, result_id: Option<SharedString> },
}

impl Default for SemanticTokensResponse {
    fn default() -> Self {
        Self::Delta { edits: Vec::new(), result_id: None }
    }
}

#[derive(Debug)]
pub(crate) struct SemanticTokensEdit {
    pub start: u32,
    pub delete_count: u32,
    pub data: Vec<u32>,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct GetCodeLens;

#[derive(Debug, Copy, Clone)]
pub(crate) struct GetDocumentColor;

#[derive(Debug, Copy, Clone)]
pub(crate) struct GetFoldingRanges;

#[derive(Debug, Copy, Clone)]
pub(crate) struct GetDocumentLinks;

impl GetCodeLens {
    pub(crate) fn can_resolve_lens(capabilities: &ServerCapabilities) -> bool {
        capabilities
            .code_lens_provider
            .as_ref()
            .and_then(|code_lens_options| code_lens_options.resolve_provider)
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub(crate) struct LinkedEditingRange {
    pub position: Anchor,
}

#[derive(Clone, Debug)]
pub struct GetDocumentDiagnostics {
    /// We cannot blindly rely on server's capabilities.diagnostic_provider, as they're a singular field, whereas
    /// a server can register multiple diagnostic providers post-mortem.
    pub registration_id: Option<SharedString>,
    pub identifier: Option<SharedString>,
    pub previous_result_id: Option<SharedString>,
}

impl LspCommand for PrepareRename {
    type Response = PrepareRenameResponse;
    type LspRequest = PrepareRenameRequest;

    fn display_name(&self) -> &'static str {
        "Prepare rename"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.rename_provider.as_ref().is_some_and(|capability| match capability {
            OneOf::Left(enabled) => *enabled,
            OneOf::Right(_) => true,
        })
    }

    fn response_without_request<'a, I>(&self, mut applicable_capabilities: I) -> Option<Self::Response>
    where
        I: Iterator<Item = AdapterServerCapabilities<'a>>,
    {
        (!applicable_capabilities.any(|capabilities| {
            matches!(
                capabilities.server_capabilities.rename_provider.as_ref(),
                Some(lsp::OneOf::Right(RenameOptions { prepare_provider: Some(true), .. }))
            )
        }))
        .then_some(PrepareRenameResponse::OnlyUnpreparedRenameSupported)
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::TextDocumentPositionParams> {
        make_lsp_text_document_position(path, self.position)
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::PrepareRenameResponse>,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<PrepareRenameResponse> {
        buffer.read_with(&cx, |buffer, _| match message {
            Some(
                lsp::PrepareRenameResponse::Range(range)
                | lsp::PrepareRenameResponse::RangeWithPlaceholder { range, .. },
            ) => {
                let Range { start, end } = range_from_lsp(range);
                if buffer.clip_point_utf16(start, Bias::Left) == start.0
                    && buffer.clip_point_utf16(end, Bias::Left) == end.0
                {
                    Ok(PrepareRenameResponse::Success {
                        range: buffer.anchor_after(start)..buffer.anchor_before(end),
                        language_server_id: Some(server_id),
                    })
                } else {
                    Ok(PrepareRenameResponse::InvalidPosition)
                }
            }
            Some(lsp::PrepareRenameResponse::DefaultBehavior { .. }) => {
                let snapshot = buffer.snapshot();
                let (range, _) = snapshot.surrounding_word(self.position, None);
                let range = snapshot.anchor_after(range.start)..snapshot.anchor_before(range.end);
                Ok(PrepareRenameResponse::Success { range, language_server_id: Some(server_id) })
            }
            None => Ok(PrepareRenameResponse::InvalidPosition),
        })
    }
}

impl LspCommand for PerformRename {
    type Response = ProjectTransaction;
    type LspRequest = Rename;

    fn display_name(&self) -> &'static str {
        "Rename"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.rename_provider.as_ref().is_some_and(|capability| match capability {
            OneOf::Left(enabled) => *enabled,
            OneOf::Right(_) => true,
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::RenameParams> {
        Ok(lsp::RenameParams {
            text_document_position: make_lsp_text_document_position(path, self.position)?,
            new_name: self.new_name.clone(),
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::WorkspaceEdit>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        mut cx: AsyncApp,
    ) -> Result<ProjectTransaction> {
        if let Some(edit) = message {
            let (_, lsp_server) = language_server_for_buffer(&lsp_store, &buffer, server_id, &mut cx)?;
            LocalLspStore::deserialize_workspace_edit(lsp_store, edit, self.push_to_history, lsp_server, &mut cx).await
        } else {
            Ok(ProjectTransaction::default())
        }
    }
}

impl LspCommand for GetDefinitions {
    type Response = Vec<LocationLink>;
    type LspRequest = GotoDefinition;

    fn display_name(&self) -> &'static str {
        "Get definition"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.definition_provider.as_ref().is_some_and(|capability| match capability {
            OneOf::Left(supported) => *supported,
            OneOf::Right(_options) => true,
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::GotoDefinitionParams> {
        Ok(lsp::GotoDefinitionParams {
            text_document_position_params: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::GotoDefinitionResponse>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<LocationLink>> {
        location_links_from_lsp(message, lsp_store, buffer, server_id, cx).await
    }
}

impl LspCommand for GetDeclarations {
    type Response = Vec<LocationLink>;
    type LspRequest = GotoDeclaration;

    fn display_name(&self) -> &'static str {
        "Get declaration"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.declaration_provider.as_ref().is_some_and(|capability| match capability {
            lsp::DeclarationCapability::Simple(supported) => *supported,
            lsp::DeclarationCapability::RegistrationOptions(..) => true,
            lsp::DeclarationCapability::Options(..) => true,
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::GotoDeclarationParams> {
        Ok(lsp::GotoDeclarationParams {
            text_document_position_params: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::GotoDeclarationResponse>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<LocationLink>> {
        location_links_from_lsp(message, lsp_store, buffer, server_id, cx).await
    }
}

impl LspCommand for GetImplementations {
    type Response = Vec<LocationLink>;
    type LspRequest = GotoImplementation;

    fn display_name(&self) -> &'static str {
        "Get implementation"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.implementation_provider.as_ref().is_some_and(|capability| match capability {
            lsp::ImplementationProviderCapability::Simple(enabled) => *enabled,
            lsp::ImplementationProviderCapability::Options(_options) => true,
        })
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::GotoImplementationParams> {
        Ok(lsp::GotoImplementationParams {
            text_document_position_params: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::GotoImplementationResponse>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<LocationLink>> {
        location_links_from_lsp(message, lsp_store, buffer, server_id, cx).await
    }
}

impl LspCommand for GetTypeDefinitions {
    type Response = Vec<LocationLink>;
    type LspRequest = GotoTypeDefinition;

    fn display_name(&self) -> &'static str {
        "Get type definition"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        !matches!(
            &capabilities.server_capabilities.type_definition_provider,
            None | Some(lsp::TypeDefinitionProviderCapability::Simple(false))
        )
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::GotoTypeDefinitionParams> {
        Ok(lsp::GotoTypeDefinitionParams {
            text_document_position_params: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::GotoTypeDefinitionResponse>,
        project: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<LocationLink>> {
        location_links_from_lsp(message, project, buffer, server_id, cx).await
    }
}

fn language_server_for_buffer(
    lsp_store: &Entity<LspStore>,
    buffer: &Entity<Buffer>,
    server_id: LanguageServerId,
    cx: &mut AsyncApp,
) -> Result<(Arc<CachedLspAdapter>, Arc<LanguageServer>)> {
    lsp_store
        .update(cx, |lsp_store, cx| {
            buffer.update(cx, |buffer, cx| {
                lsp_store
                    .language_server_for_local_buffer(buffer, server_id, cx)
                    .map(|(adapter, server)| (Arc::clone(adapter), Arc::clone(server)))
            })
        })
        .context("no language server found for buffer")
}

pub async fn location_links_from_lsp(
    message: Option<lsp::GotoDefinitionResponse>,
    lsp_store: Entity<LspStore>,
    buffer: Entity<Buffer>,
    server_id: LanguageServerId,
    mut cx: AsyncApp,
) -> Result<Vec<LocationLink>> {
    let unresolved_links = definition_locations_from_lsp(message);

    let (_, language_server) = language_server_for_buffer(&lsp_store, &buffer, server_id, &mut cx)?;
    let mut definitions = Vec::new();
    for (origin_range, target_uri, target_range) in unresolved_links {
        let target_buffer_handle = lsp_store
            .update(&mut cx, |this, cx| this.open_local_buffer_via_lsp(target_uri, language_server.server_id(), cx))
            .await?;

        cx.update(|cx| {
            let origin_location = origin_range.map(|origin_range| {
                let origin_buffer = buffer.read(cx);
                let origin_range = range_from_lsp(origin_range);
                let origin_start = origin_buffer.clip_point_utf16(origin_range.start, Bias::Left);
                let origin_end = origin_buffer.clip_point_utf16(origin_range.end, Bias::Left);
                Location {
                    buffer: buffer.clone(),
                    range: origin_buffer.anchor_after(origin_start)..origin_buffer.anchor_before(origin_end),
                }
            });

            let target_buffer = target_buffer_handle.read(cx);
            let target_range = range_from_lsp(target_range);
            let target_start = target_buffer.clip_point_utf16(target_range.start, Bias::Left);
            let target_end = target_buffer.clip_point_utf16(target_range.end, Bias::Left);
            let target_location = Location {
                buffer: target_buffer_handle,
                range: target_buffer.anchor_after(target_start)..target_buffer.anchor_before(target_end),
            };

            definitions.push(LocationLink { origin: origin_location, target: target_location });
        });
    }
    Ok(definitions)
}

fn definition_locations_from_lsp(
    message: Option<lsp::GotoDefinitionResponse>,
) -> Vec<(Option<lsp::Range>, lsp::Uri, lsp::Range)> {
    let Some(message) = message else {
        return Vec::new();
    };

    let mut locations = Vec::new();
    match message {
        lsp::GotoDefinitionResponse::Scalar(location) => {
            locations.push((None, location.uri, location.range));
        }

        lsp::GotoDefinitionResponse::Array(locations_from_lsp) => {
            locations.extend(locations_from_lsp.into_iter().map(|location| (None, location.uri, location.range)));
        }

        lsp::GotoDefinitionResponse::Link(links) => {
            locations.extend(
                links
                    .into_iter()
                    .map(|link| (link.origin_selection_range, link.target_uri, link.target_selection_range)),
            );
        }
    }
    locations
}

pub async fn location_link_from_lsp(
    link: lsp::LocationLink,
    lsp_store: &Entity<LspStore>,
    buffer: &Entity<Buffer>,
    server_id: LanguageServerId,
    cx: &mut AsyncApp,
) -> Result<LocationLink> {
    let (_, language_server) = language_server_for_buffer(lsp_store, buffer, server_id, cx)?;

    let (origin_range, target_uri, target_range) =
        (link.origin_selection_range, link.target_uri, link.target_selection_range);

    let target_buffer_handle = lsp_store
        .update(cx, |lsp_store, cx| lsp_store.open_local_buffer_via_lsp(target_uri, language_server.server_id(), cx))
        .await?;

    Ok(cx.update(|cx| {
        let origin_location = origin_range.map(|origin_range| {
            let origin_buffer = buffer.read(cx);
            let origin_range = range_from_lsp(origin_range);
            let origin_start = origin_buffer.clip_point_utf16(origin_range.start, Bias::Left);
            let origin_end = origin_buffer.clip_point_utf16(origin_range.end, Bias::Left);
            Location {
                buffer: buffer.clone(),
                range: origin_buffer.anchor_after(origin_start)..origin_buffer.anchor_before(origin_end),
            }
        });

        let target_buffer = target_buffer_handle.read(cx);
        let target_range = range_from_lsp(target_range);
        let target_start = target_buffer.clip_point_utf16(target_range.start, Bias::Left);
        let target_end = target_buffer.clip_point_utf16(target_range.end, Bias::Left);
        let target_location = Location {
            buffer: target_buffer_handle,
            range: target_buffer.anchor_after(target_start)..target_buffer.anchor_before(target_end),
        };

        LocationLink { origin: origin_location, target: target_location }
    }))
}

impl LspCommand for GetReferences {
    type Response = Vec<Location>;
    type LspRequest = References;

    fn display_name(&self) -> &'static str {
        "Find all references"
    }

    fn status(&self) -> Option<String> {
        Some("Finding references...".to_owned())
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        match &capabilities.server_capabilities.references_provider {
            Some(OneOf::Left(has_support)) => *has_support,
            Some(OneOf::Right(_)) => true,
            None => false,
        }
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::ReferenceParams> {
        Ok(lsp::ReferenceParams {
            text_document_position: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
            context: lsp::ReferenceContext { include_declaration: true },
        })
    }

    async fn response_from_lsp(
        self,
        locations: Option<Vec<lsp::Location>>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        mut cx: AsyncApp,
    ) -> Result<Vec<Location>> {
        let mut references = Vec::new();
        let (_, language_server) = language_server_for_buffer(&lsp_store, &buffer, server_id, &mut cx)?;

        if let Some(locations) = locations {
            for lsp_location in locations {
                let target_buffer_handle = lsp_store
                    .update(&mut cx, |lsp_store, cx| {
                        lsp_store.open_local_buffer_via_lsp(lsp_location.uri, language_server.server_id(), cx)
                    })
                    .await?;

                target_buffer_handle.clone().read_with(&cx, |target_buffer, _| {
                    let range = range_from_lsp(lsp_location.range);
                    let target_start = target_buffer.clip_point_utf16(range.start, Bias::Left);
                    let target_end = target_buffer.clip_point_utf16(range.end, Bias::Left);
                    references.push(Location {
                        buffer: target_buffer_handle,
                        range: target_buffer.anchor_after(target_start)..target_buffer.anchor_before(target_end),
                    });
                });
            }
        }

        Ok(references)
    }
}

impl LspCommand for GetDocumentHighlights {
    type Response = Vec<DocumentHighlight>;
    type LspRequest = DocumentHighlightRequest;

    fn display_name(&self) -> &'static str {
        "Get document highlights"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.document_highlight_provider.as_ref().is_some_and(|capability| match capability
        {
            OneOf::Left(supported) => *supported,
            OneOf::Right(_options) => true,
        })
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::DocumentHighlightParams> {
        Ok(lsp::DocumentHighlightParams {
            text_document_position_params: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        lsp_highlights: Option<Vec<lsp::DocumentHighlight>>,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        _: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<DocumentHighlight>> {
        Ok(buffer.read_with(&cx, |buffer, _| {
            let mut lsp_highlights = lsp_highlights.unwrap_or_default();
            lsp_highlights.sort_unstable_by_key(|h| (h.range.start, Reverse(h.range.end)));
            lsp_highlights
                .into_iter()
                .map(|lsp_highlight| {
                    let range = range_from_lsp(lsp_highlight.range);
                    let start = buffer.clip_point_utf16(range.start, Bias::Left);
                    let end = buffer.clip_point_utf16(range.end, Bias::Left);
                    DocumentHighlight {
                        range: buffer.anchor_after(start)..buffer.anchor_before(end),
                        kind: lsp_highlight.kind.unwrap_or(lsp::DocumentHighlightKind::READ),
                    }
                })
                .collect()
        }))
    }
}

impl LspCommand for GetDocumentSymbols {
    type Response = Vec<DocumentSymbol>;
    type LspRequest = DocumentSymbolRequest;

    fn display_name(&self) -> &'static str {
        "Get document symbols"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.document_symbol_provider.as_ref().is_some_and(|capability| match capability {
            OneOf::Left(supported) => *supported,
            OneOf::Right(_options) => true,
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::DocumentSymbolParams> {
        Ok(lsp::DocumentSymbolParams {
            text_document: make_text_document_identifier(path)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        lsp_symbols: Option<lsp::DocumentSymbolResponse>,
        _: Entity<LspStore>,
        _: Entity<Buffer>,
        _: LanguageServerId,
        _: AsyncApp,
    ) -> Result<Vec<DocumentSymbol>> {
        let Some(lsp_symbols) = lsp_symbols else {
            return Ok(Vec::new());
        };

        let symbols = match lsp_symbols {
            lsp::DocumentSymbolResponse::Flat(symbol_information) => symbol_information
                .into_iter()
                .map(|lsp_symbol| DocumentSymbol {
                    name: lsp_symbol.name,
                    kind: lsp_to_symbol_kind(lsp_symbol.kind),
                    range: range_from_lsp(lsp_symbol.location.range),
                    selection_range: range_from_lsp(lsp_symbol.location.range),
                    children: Vec::new(),
                })
                .collect(),
            lsp::DocumentSymbolResponse::Nested(nested_responses) => {
                fn convert_symbol(lsp_symbol: lsp::DocumentSymbol) -> DocumentSymbol {
                    DocumentSymbol {
                        name: lsp_symbol.name,
                        kind: lsp_to_symbol_kind(lsp_symbol.kind),
                        range: range_from_lsp(lsp_symbol.range),
                        selection_range: range_from_lsp(lsp_symbol.selection_range),
                        children: lsp_symbol
                            .children
                            .map_or_default(|children| children.into_iter().map(convert_symbol).collect::<Vec<_>>()),
                    }
                }
                nested_responses.into_iter().map(convert_symbol).collect()
            }
        };
        Ok(symbols)
    }
}

impl LspCommand for GetHover {
    type Response = Option<Hover>;
    type LspRequest = HoverRequest;

    fn display_name(&self) -> &'static str {
        "Get hover"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        match capabilities.server_capabilities.hover_provider.as_ref() {
            Some(lsp::HoverProviderCapability::Simple(enabled)) => *enabled,
            Some(lsp::HoverProviderCapability::Options(_)) => true,
            None => false,
        }
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::HoverParams> {
        Ok(lsp::HoverParams {
            text_document_position_params: make_lsp_text_document_position(path, self.position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::Hover>,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        _: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Self::Response> {
        let Some(hover) = message else {
            return Ok(None);
        };

        let (language, range) = buffer.read_with(&cx, |buffer, _| {
            (
                buffer.language().cloned(),
                hover.range.map(|range| {
                    let range = range_from_lsp(range);
                    let token_start = buffer.clip_point_utf16(range.start, Bias::Left);
                    let token_end = buffer.clip_point_utf16(range.end, Bias::Left);
                    buffer.anchor_after(token_start)..buffer.anchor_before(token_end)
                }),
            )
        });

        fn hover_blocks_from_marked_string(marked_string: lsp::MarkedString) -> Option<HoverBlock> {
            let block = match marked_string {
                lsp::MarkedString::String(content) => HoverBlock { text: content, kind: HoverBlockKind::Markdown },
                lsp::MarkedString::LanguageString(lsp::LanguageString { language, value }) => {
                    HoverBlock { text: value, kind: HoverBlockKind::Code { language } }
                }
            };
            if block.text.is_empty() { None } else { Some(block) }
        }

        let contents = match hover.contents {
            lsp::HoverContents::Scalar(marked_string) => {
                hover_blocks_from_marked_string(marked_string).into_iter().collect()
            }
            lsp::HoverContents::Array(marked_strings) => {
                marked_strings.into_iter().filter_map(hover_blocks_from_marked_string).collect()
            }
            lsp::HoverContents::Markup(markup_content) => vec![HoverBlock {
                text: markup_content.value,
                kind: if markup_content.kind == lsp::MarkupKind::Markdown {
                    HoverBlockKind::Markdown
                } else {
                    HoverBlockKind::PlainText
                },
            }],
        };

        Ok(Some(Hover { contents, range, language }))
    }
}

impl GetCompletions {
    pub(crate) fn can_resolve_completions(capabilities: &lsp::ServerCapabilities) -> bool {
        capabilities.completion_provider.as_ref().and_then(|options| options.resolve_provider).unwrap_or(false)
    }
}

impl LspCommand for GetCompletions {
    type Response = CoreCompletionResponse;
    type LspRequest = Completion;

    fn display_name(&self) -> &'static str {
        "Get completion"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.completion_provider.is_some()
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::CompletionParams> {
        Ok(lsp::CompletionParams {
            text_document_position: make_lsp_text_document_position(path, self.position)?,
            context: Some(self.context.clone()),
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        completions: Option<lsp::CompletionResponse>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        mut cx: AsyncApp,
    ) -> Result<Self::Response> {
        let mut response_list = None;
        let (mut completions, mut is_incomplete) = if let Some(completions) = completions {
            match completions {
                lsp::CompletionResponse::Array(completions) => (completions, false),
                lsp::CompletionResponse::List(mut list) => {
                    let is_incomplete = list.is_incomplete;
                    let items = mem::take(&mut list.items);
                    response_list = Some(list);
                    (items, is_incomplete)
                }
            }
        } else {
            (Vec::new(), false)
        };

        let unfiltered_completions_count = completions.len();

        let language_server_adapter = lsp_store
            .read_with(&cx, |lsp_store, _| lsp_store.language_server_adapter_for_id(server_id))
            .with_context(|| format!("no language server with id {server_id}"))?;

        let lsp_defaults = response_list.as_ref().and_then(|list| list.item_defaults.clone()).map(Arc::new);

        let mut completion_edits = Vec::new();
        buffer.update(&mut cx, |buffer, _cx| {
            let snapshot = buffer.snapshot();
            let clipped_position = buffer.clip_point_utf16(Unclipped(self.position), Bias::Left);

            let mut range_for_token = None;
            completions.retain(|lsp_completion| {
                let lsp_edit = lsp_completion.text_edit.clone().or_else(|| {
                    let default_text_edit = lsp_defaults.as_deref()?.edit_range.as_ref()?;
                    let new_text = lsp_completion.text_edit_text.as_ref().unwrap_or(&lsp_completion.label).clone();
                    match default_text_edit {
                        CompletionListItemDefaultsEditRange::Range(range) => {
                            Some(lsp::CompletionTextEdit::Edit(lsp::TextEdit { range: *range, new_text }))
                        }
                        CompletionListItemDefaultsEditRange::InsertAndReplace { insert, replace } => {
                            Some(lsp::CompletionTextEdit::InsertAndReplace(lsp::InsertReplaceEdit {
                                new_text,
                                insert: *insert,
                                replace: *replace,
                            }))
                        }
                    }
                });

                // If the language server provides a range to overwrite, then
                // check that the range is valid.
                let edit = if let Some(completion_text_edit) = lsp_edit {
                    let Some(edit) = parse_completion_text_edit(&completion_text_edit, &snapshot) else {
                        return false;
                    };
                    edit
                } else {
                    // If the language server does not provide a range, then infer
                    // the range based on the syntax tree.
                    if self.position != clipped_position {
                        log::info!("completion out of expected range ");
                        return false;
                    }

                    let range = range_for_token
                        .get_or_insert_with(|| {
                            let offset = self.position.to_offset(&snapshot);
                            let (range, kind) = snapshot.surrounding_word(offset, Some(CharScopeContext::Completion));
                            let range = if kind == Some(CharKind::Word) { range } else { offset..offset };

                            snapshot.anchor_before(range.start)..snapshot.anchor_after(range.end)
                        })
                        .clone();

                    // We already know text_edit is None here
                    let text = lsp_completion.insert_text.as_ref().unwrap_or(&lsp_completion.label).clone();

                    let insert_range = Some(range.start..snapshot.anchor_after(self.position));
                    ParsedCompletionEdit { replace_range: range, insert_range, new_text: text }
                };

                completion_edits.push(edit);
                true
            });
        });

        // If completions were filtered out due to errors that may be transient, mark the result
        // incomplete so that it is re-queried.
        if unfiltered_completions_count != completions.len() {
            is_incomplete = true;
        }

        language_server_adapter.process_completions(&mut completions).await;

        let completions = completions
            .into_iter()
            .zip(completion_edits)
            .map(|(mut lsp_completion, mut edit)| {
                LineEnding::normalize(&mut edit.new_text);
                if lsp_completion.data.is_none()
                    && let Some(default_data) =
                        lsp_defaults.as_ref().and_then(|item_defaults| item_defaults.data.clone())
                {
                    // Servers (e.g. JDTLS) prefer unchanged completions, when resolving the items later,
                    // so we do not insert the defaults here, but `data` is needed for resolving, so this is an exception.
                    lsp_completion.data = Some(default_data);
                }
                CoreCompletion {
                    replace_range: edit.replace_range,
                    new_text: edit.new_text,
                    source: CompletionSource::Lsp {
                        insert_range: edit.insert_range,
                        server_id,
                        lsp_completion: Box::new(lsp_completion),
                        lsp_defaults: lsp_defaults.clone(),
                        resolved: false,
                    },
                }
            })
            .collect();

        Ok(CoreCompletionResponse { completions, is_incomplete })
    }
}

pub struct ParsedCompletionEdit {
    pub replace_range: Range<Anchor>,
    pub insert_range: Option<Range<Anchor>>,
    pub new_text: String,
}

pub(crate) fn parse_completion_text_edit(
    edit: &lsp::CompletionTextEdit,
    snapshot: &BufferSnapshot,
) -> Option<ParsedCompletionEdit> {
    let (replace_range, insert_range, new_text) = match edit {
        lsp::CompletionTextEdit::Edit(edit) => (edit.range, None, &edit.new_text),
        lsp::CompletionTextEdit::InsertAndReplace(edit) => (edit.replace, Some(edit.insert), &edit.new_text),
    };

    let replace_range = {
        let range = range_from_lsp(replace_range);
        let start = snapshot.clip_point_utf16(range.start, Bias::Left);
        let end = snapshot.clip_point_utf16(range.end, Bias::Left);
        if start != range.start.0 || end != range.end.0 {
            log::info!("completion out of expected range, start: {start:?}, end: {end:?}, range: {range:?}");
            return None;
        }
        snapshot.anchor_before(start)..snapshot.anchor_after(end)
    };

    let insert_range = match insert_range {
        None => None,
        Some(insert_range) => {
            let range = range_from_lsp(insert_range);
            let start = snapshot.clip_point_utf16(range.start, Bias::Left);
            let end = snapshot.clip_point_utf16(range.end, Bias::Left);
            if start != range.start.0 || end != range.end.0 {
                log::info!("completion (insert) out of expected range");
                return None;
            }
            Some(snapshot.anchor_before(start)..snapshot.anchor_after(end))
        }
    };

    Some(ParsedCompletionEdit { insert_range, replace_range, new_text: new_text.clone() })
}

impl LspCommand for GetCodeActions {
    type Response = Vec<CodeAction>;
    type LspRequest = CodeActionRequest;

    fn display_name(&self) -> &'static str {
        "Get code actions"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        match &capabilities.server_capabilities.code_action_provider {
            None => false,
            Some(lsp::CodeActionProviderCapability::Simple(false)) => false,
            _ => {
                // If we do know that we want specific code actions AND we know that
                // the server only supports specific code actions, then we want to filter
                // down to the ones that are supported.
                if let Some((requested, supported)) =
                    self.kinds.as_ref().zip(Self::supported_code_action_kinds(capabilities))
                {
                    requested.iter().any(|requested_kind| {
                        supported.iter().any(|supported_kind| code_action_kind_matches(requested_kind, supported_kind))
                    })
                } else {
                    true
                }
            }
        }
    }

    fn to_lsp(
        &self,
        path: &Path,
        buffer: &Buffer,
        language_server: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::CodeActionParams> {
        let text_document = make_text_document_identifier(path)?;
        let snapshot = buffer.snapshot();
        let mut relevant_diagnostics = Vec::new();
        let target_server_id = language_server.server_id();
        for (source_server_id, entry) in snapshot.diagnostic_entries_in_range_with_server_id(self.range.clone(), false)
        {
            let downgrade_markup = source_server_id != target_server_id && entry.diagnostic.message.has_lsp_markup();
            let entry = entry
                .clone()
                .map_coordinates(|range| range.start.to_point_utf16(&snapshot)..range.end.to_point_utf16(&snapshot));
            let mut diagnostic = entry.to_lsp_diagnostic_stub(&text_document.uri)?;
            if downgrade_markup {
                diagnostic.message = lsp::DiagnosticMessage::from(entry.diagnostic.message.as_str());
            }
            relevant_diagnostics.push(diagnostic);
        }

        Ok(lsp::CodeActionParams {
            text_document: make_text_document_identifier(path)?,
            range: range_to_lsp(self.range.to_point_utf16(buffer))?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
            context: lsp::CodeActionContext {
                diagnostics: relevant_diagnostics,
                only: self.kinds.clone(),
                ..lsp::CodeActionContext::default()
            },
        })
    }

    async fn response_from_lsp(
        self,
        actions: Option<lsp::CodeActionResponse>,
        lsp_store: Entity<LspStore>,
        _: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<CodeAction>> {
        let requested_kinds = self.kinds.as_ref();

        let language_server = cx.update(|cx| {
            lsp_store
                .read(cx)
                .language_server_for_id(server_id)
                .with_context(|| format!("Missing the language server that just returned a response {server_id}"))
        })?;

        let server_capabilities = language_server.capabilities();
        let available_commands =
            server_capabilities.execute_command_provider.as_ref().map_or_default(|options| options.commands.as_slice());
        Ok(actions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                let (lsp_action, resolved) = match entry {
                    lsp::CodeActionOrCommand::CodeAction(lsp_action) => {
                        if let Some(command) = lsp_action.command.as_ref()
                            && !available_commands.contains(&command.command)
                        {
                            return None;
                        }
                        (LspAction::Action(Box::new(lsp_action)), false)
                    }
                    lsp::CodeActionOrCommand::Command(command) => {
                        if available_commands.contains(&command.command) {
                            (LspAction::Command(command), true)
                        } else {
                            return None;
                        }
                    }
                };

                if let Some((kinds, kind)) = requested_kinds.zip(lsp_action.action_kind())
                    && !kinds.iter().any(|requested_kind| code_action_kind_matches(requested_kind, &kind))
                {
                    return None;
                }

                Some(CodeAction { server_id, range: self.range.clone(), lsp_action, resolved })
            })
            .collect())
    }
}

impl GetCodeActions {
    fn supported_code_action_kinds(capabilities: AdapterServerCapabilities<'_>) -> Option<&[CodeActionKind]> {
        match capabilities.server_capabilities.code_action_provider.as_ref() {
            Some(lsp::CodeActionProviderCapability::Options(CodeActionOptions {
                code_action_kinds: Some(supported_action_kinds),
                ..
            })) => Some(supported_action_kinds),
            _ => capabilities.code_action_kinds,
        }
    }

    pub(crate) fn can_resolve_actions(capabilities: &ServerCapabilities) -> bool {
        capabilities
            .code_action_provider
            .as_ref()
            .and_then(|options| match options {
                lsp::CodeActionProviderCapability::Simple(_is_supported) => None,
                lsp::CodeActionProviderCapability::Options(options) => options.resolve_provider,
            })
            .unwrap_or(false)
    }
}

impl OnTypeFormatting {
    pub(crate) fn supports_on_type_formatting(trigger: &str, capabilities: &ServerCapabilities) -> bool {
        let Some(on_type_formatting_options) = &capabilities.document_on_type_formatting_provider else {
            return false;
        };
        on_type_formatting_options.first_trigger_character.contains(trigger)
            || on_type_formatting_options.more_trigger_character.iter().flatten().any(|chars| chars.contains(trigger))
    }
}

impl LspCommand for OnTypeFormatting {
    type Response = Option<Transaction>;
    type LspRequest = request::OnTypeFormatting;

    fn display_name(&self) -> &'static str {
        "Formatting on typing"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        Self::supports_on_type_formatting(&self.trigger, capabilities.server_capabilities)
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::DocumentOnTypeFormattingParams> {
        Ok(lsp::DocumentOnTypeFormattingParams {
            text_document_position: make_lsp_text_document_position(path, self.position)?,
            ch: self.trigger.clone(),
            options: self.options.clone(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<Vec<lsp::TextEdit>>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        mut cx: AsyncApp,
    ) -> Result<Option<Transaction>> {
        if let Some(edits) = message {
            let (lsp_adapter, lsp_server) = language_server_for_buffer(&lsp_store, &buffer, server_id, &mut cx)?;
            LocalLspStore::deserialize_text_edits(
                lsp_store,
                buffer,
                edits,
                self.push_to_history,
                lsp_adapter,
                lsp_server,
                &mut cx,
            )
            .await
        } else {
            Ok(None)
        }
    }
}

impl InlayHints {
    fn project_hint_kind(kind: Option<lsp::InlayHintKind>) -> Option<InlayHintKind> {
        let kind = kind?;
        match kind {
            lsp::InlayHintKind::TYPE => Some(InlayHintKind::Type),
            lsp::InlayHintKind::PARAMETER => Some(InlayHintKind::Parameter),
            _ => None,
        }
    }

    /// Returns the clipped position and the bias to use for this hint in isolation.
    fn hint_position_and_bias(lsp_hint: &lsp::InlayHint, snapshot: &BufferSnapshot) -> (PointUtf16, Bias) {
        let position = snapshot.clip_point_utf16(point_from_lsp(lsp_hint.position), Bias::Left);
        let bias = match Self::project_hint_kind(lsp_hint.kind) {
            Some(InlayHintKind::Type) => Bias::Right,
            Some(InlayHintKind::Parameter) => Bias::Left,
            // `None`-kinded hints can go either way: rust-analyzer's `Lifetime` before `str`
            // in `&str` is a prefix, while `ClosingBrace` after `}` is a suffix. Asymmetric
            // padding is a reliable signal: space before the hint (`padding_left`) means it is
            // a suffix attached to the left → Right; space after (`padding_right`) means it is
            // a prefix attached to the right → Left. When padding is ambiguous, fall back to
            // `surrounding_word`, which uses the greater of `prev` and `next` as the word-kind
            // level: if the previous character's kind is greater than or equal to the next
            // character's kind, the hint is a suffix → Right; otherwise, it is a prefix → Left.
            // `None` covers both directions.
            None => match (lsp_hint.padding_left.unwrap_or(false), lsp_hint.padding_right.unwrap_or(false)) {
                (true, false) => Bias::Right,
                (false, true) => Bias::Left,
                _ => {
                    let offset = position.to_offset(snapshot);
                    let (range, _) = snapshot.surrounding_word(offset, None);
                    if range.start < offset { Bias::Right } else { Bias::Left }
                }
            },
        };
        (position, bias)
    }

    pub(crate) fn lsp_to_project_hint(
        lsp_hint: lsp::InlayHint,
        position: Anchor,
        snapshot: &BufferSnapshot,
        server_id: LanguageServerId,
        resolve_state: ResolveState,
        force_no_type_left_padding: bool,
    ) -> anyhow::Result<InlayHint> {
        let kind = lsp_hint.kind.and_then(|kind| match kind {
            lsp::InlayHintKind::TYPE => Some(InlayHintKind::Type),
            lsp::InlayHintKind::PARAMETER => Some(InlayHintKind::Parameter),
            _ => None,
        });

        let text_edits = lsp_hint
            .text_edits
            .map(|edits| {
                let mut edits = edits
                    .into_iter()
                    .map(|edit| {
                        let range = range_from_lsp(edit.range);
                        let start = snapshot.clip_point_utf16(range.start, Bias::Left);
                        let end = snapshot.clip_point_utf16(range.end, Bias::Left);
                        anyhow::ensure!(start <= end, "invalid inlay hint text edit range: {:?}", edit.range);
                        Ok((start..end, edit.new_text))
                    })
                    .collect::<Result<Vec<_>>>()?;
                edits.sort_by_key(|(range, _)| (range.start, range.end));
                anyhow::ensure!(
                    edits.iter().zip(edits.iter().skip(1)).all(|(a, b)| a.0.end <= b.0.start),
                    "overlapping inlay hint text edits"
                );
                edits.dedup_by(|(range, new_text), (previous_range, previous_text)| {
                    let adjacent = previous_range.end == range.start;
                    if adjacent {
                        previous_range.end = range.end;
                        previous_text.push_str(new_text);
                    }
                    adjacent
                });
                let edits = edits
                    .into_iter()
                    .map(|(range, mut new_text)| {
                        LineEnding::normalize(&mut new_text);
                        let start = snapshot.anchor_after(range.start);
                        let end = if range.is_empty() { start } else { snapshot.anchor_before(range.end) };
                        (start..end, new_text)
                    })
                    .collect();
                anyhow::Ok(InlayHintTextEdits { edits, buffer_version: snapshot.version().clone() })
            })
            .transpose()?;
        let label = Self::lsp_inlay_label_to_project(lsp_hint.label, server_id);
        let padding_left = if force_no_type_left_padding && kind == Some(InlayHintKind::Type) {
            false
        } else {
            lsp_hint.padding_left.unwrap_or(false)
        };

        Ok(InlayHint {
            position,
            text_edits,
            padding_left,
            padding_right: lsp_hint.padding_right.unwrap_or(false),
            label,
            kind,
            tooltip: lsp_hint.tooltip.map(|tooltip| match tooltip {
                lsp::InlayHintTooltip::String(s) => InlayHintTooltip::String(s),
                lsp::InlayHintTooltip::MarkupContent(markup_content) => {
                    InlayHintTooltip::MarkupContent(MarkupContent {
                        kind: match markup_content.kind {
                            lsp::MarkupKind::PlainText => HoverBlockKind::PlainText,
                            lsp::MarkupKind::Markdown => HoverBlockKind::Markdown,
                        },
                        value: markup_content.value,
                    })
                }
            }),
            resolve_state,
        })
    }

    fn lsp_inlay_label_to_project(lsp_label: lsp::InlayHintLabel, server_id: LanguageServerId) -> InlayHintLabel {
        match lsp_label {
            lsp::InlayHintLabel::String(s) => InlayHintLabel::String(s),
            lsp::InlayHintLabel::LabelParts(lsp_parts) => {
                let mut parts = Vec::with_capacity(lsp_parts.len());
                for lsp_part in lsp_parts {
                    parts.push(InlayHintLabelPart {
                        value: lsp_part.value,
                        tooltip: lsp_part.tooltip.map(|tooltip| match tooltip {
                            lsp::InlayHintLabelPartTooltip::String(s) => InlayHintLabelPartTooltip::String(s),
                            lsp::InlayHintLabelPartTooltip::MarkupContent(markup_content) => {
                                InlayHintLabelPartTooltip::MarkupContent(MarkupContent {
                                    kind: match markup_content.kind {
                                        lsp::MarkupKind::PlainText => HoverBlockKind::PlainText,
                                        lsp::MarkupKind::Markdown => HoverBlockKind::Markdown,
                                    },
                                    value: markup_content.value,
                                })
                            }
                        }),
                        location: Some(server_id).zip(lsp_part.location),
                        command: Some(server_id).zip(lsp_part.command),
                    });
                }
                InlayHintLabel::LabelParts(parts)
            }
        }
    }

    pub(crate) fn project_to_lsp_hint(hint: InlayHint, snapshot: &BufferSnapshot) -> Result<lsp::InlayHint> {
        anyhow::ensure!(snapshot.can_resolve(&hint.position), "invalid inlay hint position anchor");
        Ok(lsp::InlayHint {
            position: point_to_lsp(hint.position.to_point_utf16(snapshot)),
            kind: hint.kind.map(|kind| match kind {
                InlayHintKind::Type => lsp::InlayHintKind::TYPE,
                InlayHintKind::Parameter => lsp::InlayHintKind::PARAMETER,
            }),
            text_edits: hint
                .text_edits
                .map(|text_edits| {
                    text_edits
                        .edits
                        .into_iter()
                        .map(|(range, new_text)| {
                            anyhow::ensure!(
                                snapshot.can_resolve(&range.start),
                                "invalid inlay hint text edit start anchor"
                            );
                            anyhow::ensure!(
                                snapshot.can_resolve(&range.end),
                                "invalid inlay hint text edit end anchor"
                            );
                            let start = range.start.to_point_utf16(snapshot);
                            let end = range.end.to_point_utf16(snapshot);
                            anyhow::ensure!(start <= end, "reversed inlay hint text edit range");
                            Ok(lsp::TextEdit {
                                range: lsp::Range::new(point_to_lsp(start), point_to_lsp(end)),
                                new_text,
                            })
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?,
            tooltip: hint.tooltip.and_then(|tooltip| {
                Some(match tooltip {
                    InlayHintTooltip::String(s) => lsp::InlayHintTooltip::String(s),
                    InlayHintTooltip::MarkupContent(markup_content) => {
                        lsp::InlayHintTooltip::MarkupContent(lsp::MarkupContent {
                            kind: match markup_content.kind {
                                HoverBlockKind::PlainText => lsp::MarkupKind::PlainText,
                                HoverBlockKind::Markdown => lsp::MarkupKind::Markdown,
                                HoverBlockKind::Code { .. } => return None,
                            },
                            value: markup_content.value,
                        })
                    }
                })
            }),
            label: match hint.label {
                InlayHintLabel::String(s) => lsp::InlayHintLabel::String(s),
                InlayHintLabel::LabelParts(label_parts) => lsp::InlayHintLabel::LabelParts(
                    label_parts
                        .into_iter()
                        .map(|part| lsp::InlayHintLabelPart {
                            value: part.value,
                            tooltip: part.tooltip.and_then(|tooltip| {
                                Some(match tooltip {
                                    InlayHintLabelPartTooltip::String(s) => lsp::InlayHintLabelPartTooltip::String(s),
                                    InlayHintLabelPartTooltip::MarkupContent(markup_content) => {
                                        lsp::InlayHintLabelPartTooltip::MarkupContent(lsp::MarkupContent {
                                            kind: match markup_content.kind {
                                                HoverBlockKind::PlainText => lsp::MarkupKind::PlainText,
                                                HoverBlockKind::Markdown => lsp::MarkupKind::Markdown,
                                                HoverBlockKind::Code { .. } => return None,
                                            },
                                            value: markup_content.value,
                                        })
                                    }
                                })
                            }),
                            location: part.location.map(|(_, location)| location),
                            command: part.command.map(|(_, command)| command),
                        })
                        .collect(),
                ),
            },
            padding_left: Some(hint.padding_left),
            padding_right: Some(hint.padding_right),
            data: match hint.resolve_state {
                ResolveState::CanResolve(_, data) => data,
                ResolveState::Resolving | ResolveState::Resolved => None,
            },
        })
    }

    pub(crate) fn can_resolve_inlays(capabilities: &ServerCapabilities) -> bool {
        capabilities
            .inlay_hint_provider
            .as_ref()
            .and_then(|options| match options {
                OneOf::Left(_is_supported) => None,
                OneOf::Right(capabilities) => match capabilities {
                    lsp::InlayHintServerCapabilities::Options(o) => o.resolve_provider,
                    lsp::InlayHintServerCapabilities::RegistrationOptions(o) => o.inlay_hint_options.resolve_provider,
                },
            })
            .unwrap_or(false)
    }

    pub(crate) fn check_capabilities(capabilities: &ServerCapabilities) -> bool {
        capabilities.inlay_hint_provider.as_ref().is_some_and(|inlay_hint_provider| match inlay_hint_provider {
            lsp::OneOf::Left(enabled) => *enabled,
            lsp::OneOf::Right(_) => true,
        })
    }
}

impl LspCommand for InlayHints {
    type Response = Vec<InlayHint>;
    type LspRequest = lsp::InlayHintRequest;

    fn display_name(&self) -> &'static str {
        "Inlay hints"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        Self::check_capabilities(capabilities.server_capabilities)
    }

    fn to_lsp(&self, path: &Path, buffer: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::InlayHintParams> {
        Ok(lsp::InlayHintParams {
            text_document: lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(path)? },
            range: range_to_lsp(self.range.to_point_utf16(buffer))?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<Vec<lsp::InlayHint>>,
        lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        mut cx: AsyncApp,
    ) -> anyhow::Result<Vec<InlayHint>> {
        let (lsp_adapter, lsp_server) = language_server_for_buffer(&lsp_store, &buffer, server_id, &mut cx)?;
        // TypeScript servers may add padding to the left for type hints, turning
        // `const foo: boolean` into `const foo : boolean` which looks odd.
        // `rust-analyzer` does not have the padding for this case, and we have to accommodate both.
        //
        // We could trim the whole string, but being pessimistic on par with the situation above,
        // there might be a hint with multiple whitespaces at the end(s) which we need to display properly.
        // Hence let's use a heuristic first to handle the most awkward case and look for more.
        let force_no_type_left_padding = lsp_adapter.name.0.as_ref() == "tsgo";
        let can_resolve = lsp_store.update(&mut cx, |lsp_store, cx| {
            lsp_store.text_document_capability_matches_for_server(
                &buffer,
                server_id,
                "textDocument/inlayHint",
                |capabilities| InlayHints::can_resolve_inlays(capabilities.server_capabilities),
                cx,
            )
        });

        let snapshot = buffer.read_with(&cx, |buffer, _| buffer.snapshot());
        let last_row = snapshot.max_point().row;
        let hints = message
            .unwrap_or_default()
            .into_iter()
            .filter(|lsp_hint| lsp_hint.position.line <= last_row)
            .filter_map(|lsp_hint| {
                let resolve_state = if can_resolve {
                    ResolveState::CanResolve(lsp_server.server_id(), lsp_hint.data.clone())
                } else {
                    ResolveState::Resolved
                };
                let (position, bias) = InlayHints::hint_position_and_bias(&lsp_hint, &snapshot);
                InlayHints::lsp_to_project_hint(
                    lsp_hint,
                    snapshot.anchor_at(position, bias),
                    &snapshot,
                    server_id,
                    resolve_state,
                    force_no_type_left_padding,
                )
                .context("lsp to project inlay hint conversion")
                .log_err()
            })
            .collect();

        Ok(hints)
    }
}

impl LspCommand for SemanticTokensFull {
    type Response = SemanticTokensResponse;
    type LspRequest = lsp::SemanticTokensFullRequest;

    fn display_name(&self) -> &'static str {
        "Semantic tokens full"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.semantic_tokens_provider.as_ref().is_some_and(|semantic_tokens_provider| {
            let options = match semantic_tokens_provider {
                lsp::SemanticTokensServerCapabilities::SemanticTokensOptions(opts) => opts,
                lsp::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(opts) => {
                    &opts.semantic_tokens_options
                }
            };

            match options.full {
                Some(lsp::SemanticTokensFullOptions::Bool(is_supported)) => is_supported,
                Some(lsp::SemanticTokensFullOptions::Delta { .. }) => true,
                None => false,
            }
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::SemanticTokensParams> {
        Ok(lsp::SemanticTokensParams {
            text_document: lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(path)? },
            partial_result_params: lsp::PartialResultParams::default(),
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::SemanticTokensResult>,
        _: Entity<LspStore>,
        _: Entity<Buffer>,
        _: LanguageServerId,
        _: AsyncApp,
    ) -> anyhow::Result<SemanticTokensResponse> {
        match message {
            Some(lsp::SemanticTokensResult::Tokens(tokens)) => Ok(SemanticTokensResponse::Full {
                data: tokens.data,
                result_id: tokens.result_id.map(SharedString::new),
            }),
            Some(lsp::SemanticTokensResult::Partial(_)) => {
                anyhow::bail!("Unexpected semantic tokens response with partial result for inlay hints")
            }
            None => Ok(SemanticTokensResponse::default()),
        }
    }
}

impl LspCommand for SemanticTokensDelta {
    type Response = SemanticTokensResponse;
    type LspRequest = lsp::SemanticTokensFullDeltaRequest;

    fn display_name(&self) -> &'static str {
        "Semantic tokens delta"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.semantic_tokens_provider.as_ref().is_some_and(|semantic_tokens_provider| {
            let options = match semantic_tokens_provider {
                lsp::SemanticTokensServerCapabilities::SemanticTokensOptions(opts) => opts,
                lsp::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(opts) => {
                    &opts.semantic_tokens_options
                }
            };

            match options.full {
                Some(lsp::SemanticTokensFullOptions::Delta { delta }) => delta.unwrap_or(false),
                // `full: true` (instead of `full: { delta: true }`) means no support for delta.
                _ => false,
            }
        })
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::SemanticTokensDeltaParams> {
        Ok(lsp::SemanticTokensDeltaParams {
            text_document: lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(path)? },
            previous_result_id: self.previous_result_id.clone().map(|s| s.to_string()),
            partial_result_params: lsp::PartialResultParams::default(),
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::SemanticTokensFullDeltaResult>,
        _: Entity<LspStore>,
        _: Entity<Buffer>,
        _: LanguageServerId,
        _: AsyncApp,
    ) -> anyhow::Result<SemanticTokensResponse> {
        match message {
            Some(lsp::SemanticTokensFullDeltaResult::Tokens(tokens)) => Ok(SemanticTokensResponse::Full {
                data: tokens.data,
                result_id: tokens.result_id.map(SharedString::new),
            }),
            Some(lsp::SemanticTokensFullDeltaResult::TokensDelta(delta)) => Ok(SemanticTokensResponse::Delta {
                edits: delta
                    .edits
                    .into_iter()
                    .map(|e| SemanticTokensEdit {
                        start: e.start,
                        delete_count: e.delete_count,
                        data: e.data.unwrap_or_default(),
                    })
                    .collect(),
                result_id: delta.result_id.map(SharedString::new),
            }),
            Some(lsp::SemanticTokensFullDeltaResult::PartialTokensDelta { .. }) => {
                anyhow::bail!("Unexpected semantic tokens response with partial result for inlay hints")
            }
            None => Ok(SemanticTokensResponse::default()),
        }
    }
}

impl LspCommand for GetCodeLens {
    type Response = Vec<CodeAction>;
    type LspRequest = lsp::CodeLensRequest;

    fn display_name(&self) -> &'static str {
        "Code Lens"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.code_lens_provider.is_some()
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::CodeLensParams> {
        Ok(lsp::CodeLensParams {
            text_document: lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(path)? },
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<Vec<lsp::CodeLens>>,
        _lsp_store: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> anyhow::Result<Vec<CodeAction>> {
        let snapshot = buffer.read_with(&cx, |buffer, _| buffer.snapshot());
        let code_lenses = message.unwrap_or_default();

        Ok(code_lenses
            .into_iter()
            .map(|code_lens| {
                let code_lens_range = range_from_lsp(code_lens.range);
                let start = snapshot.clip_point_utf16(code_lens_range.start, Bias::Left);
                let end = snapshot.clip_point_utf16(code_lens_range.end, Bias::Right);
                let range = snapshot.anchor_before(start)..snapshot.anchor_after(end);
                let resolved = code_lens.command.is_some();
                CodeAction { server_id, range, lsp_action: LspAction::CodeLens(code_lens), resolved }
            })
            .collect())
    }
}

impl LinkedEditingRange {
    pub(crate) const fn check_server_capabilities(capabilities: &ServerCapabilities) -> bool {
        let Some(linked_editing_options) = capabilities.linked_editing_range_provider.as_ref() else {
            return false;
        };
        !matches!(linked_editing_options, LinkedEditingRangeServerCapabilities::Simple(false))
    }
}

impl LspCommand for LinkedEditingRange {
    type Response = Vec<Range<Anchor>>;
    type LspRequest = request::LinkedEditingRange;

    fn display_name(&self) -> &'static str {
        "Linked editing range"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        Self::check_server_capabilities(capabilities.server_capabilities)
    }

    fn to_lsp(
        &self,
        path: &Path,
        buffer: &Buffer,
        _server: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::LinkedEditingRangeParams> {
        let position = self.position.to_point_utf16(&buffer.snapshot());
        Ok(lsp::LinkedEditingRangeParams {
            text_document_position_params: make_lsp_text_document_position(path, position)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<lsp::LinkedEditingRanges>,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        _server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Vec<Range<Anchor>>> {
        if let Some(lsp::LinkedEditingRanges { mut ranges, .. }) = message {
            ranges.sort_by_key(|range| range.start);

            Ok(buffer.read_with(&cx, |buffer, _| {
                ranges
                    .into_iter()
                    .map(|range| {
                        let range = range_from_lsp(range);
                        let start = buffer.clip_point_utf16(range.start, Bias::Left);
                        let end = buffer.clip_point_utf16(range.end, Bias::Left);
                        buffer.anchor_before(start)..buffer.anchor_after(end)
                    })
                    .collect()
            }))
        } else {
            Ok(vec![])
        }
    }
}

impl GetDocumentDiagnostics {
    pub fn deserialize_workspace_diagnostics_report(
        report: lsp::WorkspaceDiagnosticReportResult,
        server_id: LanguageServerId,
        registration_id: Option<SharedString>,
    ) -> Vec<WorkspaceLspPullDiagnostics> {
        let mut pulled_diagnostics = FxHashMap::default();
        match report {
            lsp::WorkspaceDiagnosticReportResult::Report(workspace_diagnostic_report) => {
                for report in workspace_diagnostic_report.items {
                    match report {
                        lsp::WorkspaceDocumentDiagnosticReport::Full(report) => {
                            process_full_workspace_diagnostics_report(
                                &mut pulled_diagnostics,
                                server_id,
                                report,
                                registration_id.clone(),
                            );
                        }
                        lsp::WorkspaceDocumentDiagnosticReport::Unchanged(report) => {
                            process_unchanged_workspace_diagnostics_report(
                                &mut pulled_diagnostics,
                                server_id,
                                report,
                                registration_id.clone(),
                            );
                        }
                    }
                }
            }
            lsp::WorkspaceDiagnosticReportResult::Partial(workspace_diagnostic_report_partial_result) => {
                for report in workspace_diagnostic_report_partial_result.items {
                    match report {
                        lsp::WorkspaceDocumentDiagnosticReport::Full(report) => {
                            process_full_workspace_diagnostics_report(
                                &mut pulled_diagnostics,
                                server_id,
                                report,
                                registration_id.clone(),
                            );
                        }
                        lsp::WorkspaceDocumentDiagnosticReport::Unchanged(report) => {
                            process_unchanged_workspace_diagnostics_report(
                                &mut pulled_diagnostics,
                                server_id,
                                report,
                                registration_id.clone(),
                            );
                        }
                    }
                }
            }
        }
        pulled_diagnostics.into_values().collect()
    }
}

#[derive(Debug)]
pub struct WorkspaceLspPullDiagnostics {
    pub version: Option<i32>,
    pub diagnostics: LspPullDiagnostics,
}

fn process_full_workspace_diagnostics_report(
    diagnostics: &mut FxHashMap<lsp::Uri, WorkspaceLspPullDiagnostics>,
    server_id: LanguageServerId,
    report: lsp::WorkspaceFullDocumentDiagnosticReport,
    registration_id: Option<SharedString>,
) {
    let mut new_diagnostics = FxHashMap::default();
    process_full_diagnostics_report(
        &mut new_diagnostics,
        server_id,
        report.uri,
        report.full_document_diagnostic_report,
        registration_id,
    );
    diagnostics.extend(new_diagnostics.into_iter().map(|(uri, diagnostics)| {
        (uri, WorkspaceLspPullDiagnostics { version: report.version.map(|v| v as i32), diagnostics })
    }));
}

fn process_unchanged_workspace_diagnostics_report(
    diagnostics: &mut FxHashMap<lsp::Uri, WorkspaceLspPullDiagnostics>,
    server_id: LanguageServerId,
    report: lsp::WorkspaceUnchangedDocumentDiagnosticReport,
    registration_id: Option<SharedString>,
) {
    let mut new_diagnostics = FxHashMap::default();
    process_unchanged_diagnostics_report(
        &mut new_diagnostics,
        server_id,
        report.uri,
        report.unchanged_document_diagnostic_report,
        registration_id,
    );
    diagnostics.extend(new_diagnostics.into_iter().map(|(uri, diagnostics)| {
        (uri, WorkspaceLspPullDiagnostics { version: report.version.map(|v| v as i32), diagnostics })
    }));
}

impl LspCommand for GetDocumentDiagnostics {
    type Response = Vec<LspPullDiagnostics>;
    type LspRequest = request::DocumentDiagnosticRequest;

    fn display_name(&self) -> &'static str {
        "Get diagnostics"
    }

    fn check_capabilities(&self, capabilities: AdapterServerCapabilities<'_>) -> bool {
        capabilities.server_capabilities.diagnostic_provider.is_some()
    }

    fn to_lsp(
        &self,
        path: &Path,
        _: &Buffer,
        _: &Arc<LanguageServer>,
        _: &App,
    ) -> Result<lsp::DocumentDiagnosticParams> {
        Ok(lsp::DocumentDiagnosticParams {
            text_document: lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(path)? },
            identifier: self.identifier.as_ref().map(ToString::to_string),
            previous_result_id: self.previous_result_id.as_ref().map(ToString::to_string),
            partial_result_params: lsp::PartialResultParams::default(),
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: lsp::DocumentDiagnosticReportResult,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        server_id: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Self::Response> {
        let url = buffer.read_with(&cx, |buffer, cx| {
            buffer
                .file()
                .and_then(|file| file.as_local())
                .map(|file| {
                    let abs_path = file.abs_path(cx);
                    file_path_to_lsp_url(&abs_path)
                })
                .transpose()?
                .with_context(|| format!("missing url on buffer {}", buffer.buffer_id()))
        })?;

        let mut pulled_diagnostics = FxHashMap::default();
        match message {
            lsp::DocumentDiagnosticReportResult::Report(report) => match report {
                lsp::DocumentDiagnosticReport::Full(report) => {
                    if let Some(related_documents) = report.related_documents {
                        process_related_documents(
                            &mut pulled_diagnostics,
                            server_id,
                            related_documents,
                            self.registration_id.clone(),
                        );
                    }
                    process_full_diagnostics_report(
                        &mut pulled_diagnostics,
                        server_id,
                        url,
                        report.full_document_diagnostic_report,
                        self.registration_id,
                    );
                }
                lsp::DocumentDiagnosticReport::Unchanged(report) => {
                    if let Some(related_documents) = report.related_documents {
                        process_related_documents(
                            &mut pulled_diagnostics,
                            server_id,
                            related_documents,
                            self.registration_id.clone(),
                        );
                    }
                    process_unchanged_diagnostics_report(
                        &mut pulled_diagnostics,
                        server_id,
                        url,
                        report.unchanged_document_diagnostic_report,
                        self.registration_id,
                    );
                }
            },
            lsp::DocumentDiagnosticReportResult::Partial(report) => {
                if let Some(related_documents) = report.related_documents {
                    process_related_documents(
                        &mut pulled_diagnostics,
                        server_id,
                        related_documents,
                        self.registration_id,
                    );
                }
            }
        }

        Ok(pulled_diagnostics.into_values().collect())
    }
}

impl LspCommand for GetDocumentColor {
    type Response = Vec<DocumentColor>;
    type LspRequest = request::DocumentColor;

    fn display_name(&self) -> &'static str {
        "Document color"
    }

    fn check_capabilities(&self, server_capabilities: AdapterServerCapabilities<'_>) -> bool {
        server_capabilities.server_capabilities.color_provider.as_ref().is_some_and(|capability| match capability {
            lsp::ColorProviderCapability::Simple(supported) => *supported,
            lsp::ColorProviderCapability::ColorProvider(..) => true,
            lsp::ColorProviderCapability::Options(..) => true,
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::DocumentColorParams> {
        Ok(lsp::DocumentColorParams {
            text_document: make_text_document_identifier(path)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Vec<lsp::ColorInformation>,
        _: Entity<LspStore>,
        _: Entity<Buffer>,
        _: LanguageServerId,
        _: AsyncApp,
    ) -> Result<Self::Response> {
        Ok(message
            .into_iter()
            .map(|color| DocumentColor {
                lsp_range: color.range,
                color: color.color,
                resolved: false,
                color_presentations: Vec::new(),
            })
            .collect())
    }
}

impl LspCommand for GetFoldingRanges {
    type Response = Vec<LspFoldingRange>;
    type LspRequest = request::FoldingRangeRequest;

    fn display_name(&self) -> &'static str {
        "Folding ranges"
    }

    fn check_capabilities(&self, server_capabilities: AdapterServerCapabilities<'_>) -> bool {
        server_capabilities.server_capabilities.folding_range_provider.as_ref().is_some_and(|capability| {
            match capability {
                lsp::FoldingRangeProviderCapability::Simple(supported) => *supported,
                lsp::FoldingRangeProviderCapability::FoldingProvider(..)
                | lsp::FoldingRangeProviderCapability::Options(..) => true,
            }
        })
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::FoldingRangeParams> {
        Ok(lsp::FoldingRangeParams {
            text_document: make_text_document_identifier(path)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<Vec<lsp::FoldingRange>>,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        _: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Self::Response> {
        let snapshot = buffer.read_with(&cx, |buffer, _| buffer.snapshot());
        let max_point = snapshot.max_point_utf16();
        Ok(message
            .unwrap_or_default()
            .into_iter()
            .filter(|range| range.start_line < range.end_line)
            .filter(|range| range.start_line <= max_point.row && range.end_line <= max_point.row)
            .map(|folding_range| {
                let start_col = folding_range.start_character.unwrap_or(u32::MAX);
                let end_col = folding_range.end_character.unwrap_or(u32::MAX);
                let start = snapshot
                    .clip_point_utf16(Unclipped(PointUtf16::new(folding_range.start_line, start_col)), Bias::Right);
                let end =
                    snapshot.clip_point_utf16(Unclipped(PointUtf16::new(folding_range.end_line, end_col)), Bias::Left);
                let start = snapshot.anchor_after(start);
                let end = snapshot.anchor_before(end);
                let collapsed_text = folding_range
                    .collapsed_text
                    .filter(|t| !t.is_empty())
                    .map(|t| SharedString::from(lsp_store::collapse_newlines(&t, " ")));
                LspFoldingRange { range: start..end, collapsed_text }
            })
            .collect())
    }
}

impl LspCommand for GetDocumentLinks {
    type Response = Vec<LspDocumentLink>;
    type LspRequest = request::DocumentLinkRequest;

    fn display_name(&self) -> &'static str {
        "Document links"
    }

    fn check_capabilities(&self, server_capabilities: AdapterServerCapabilities<'_>) -> bool {
        server_capabilities.server_capabilities.document_link_provider.is_some()
    }

    fn to_lsp(&self, path: &Path, _: &Buffer, _: &Arc<LanguageServer>, _: &App) -> Result<lsp::DocumentLinkParams> {
        Ok(lsp::DocumentLinkParams {
            text_document: make_text_document_identifier(path)?,
            work_done_progress_params: lsp::WorkDoneProgressParams::default(),
            partial_result_params: lsp::PartialResultParams::default(),
        })
    }

    async fn response_from_lsp(
        self,
        message: Option<Vec<lsp::DocumentLink>>,
        _: Entity<LspStore>,
        buffer: Entity<Buffer>,
        _: LanguageServerId,
        cx: AsyncApp,
    ) -> Result<Self::Response> {
        let snapshot = buffer.read_with(&cx, |buffer, _| buffer.snapshot());
        Ok(message
            .unwrap_or_default()
            .into_iter()
            .map(|link| {
                let start = snapshot.clip_point_utf16(
                    Unclipped(PointUtf16::new(link.range.start.line, link.range.start.character)),
                    Bias::Left,
                );
                let end = snapshot.clip_point_utf16(
                    Unclipped(PointUtf16::new(link.range.end.line, link.range.end.character)),
                    Bias::Right,
                );
                LspDocumentLink {
                    range: snapshot.anchor_after(start)..snapshot.anchor_before(end),
                    target: link.target.map(|url| url.to_string().into()),
                    tooltip: link.tooltip.map(SharedString::from),
                    data: link.data,
                    resolved: false,
                }
            })
            .collect())
    }
}

fn process_related_documents(
    diagnostics: &mut FxHashMap<lsp::Uri, LspPullDiagnostics>,
    server_id: LanguageServerId,
    documents: impl IntoIterator<Item = (lsp::Uri, lsp::DocumentDiagnosticReportKind)>,
    registration_id: Option<SharedString>,
) {
    for (url, report_kind) in documents {
        match report_kind {
            lsp::DocumentDiagnosticReportKind::Full(report) => {
                process_full_diagnostics_report(diagnostics, server_id, url, report, registration_id.clone());
            }
            lsp::DocumentDiagnosticReportKind::Unchanged(report) => {
                process_unchanged_diagnostics_report(diagnostics, server_id, url, report, registration_id.clone());
            }
        }
    }
}

fn process_unchanged_diagnostics_report(
    diagnostics: &mut FxHashMap<lsp::Uri, LspPullDiagnostics>,
    server_id: LanguageServerId,
    uri: lsp::Uri,
    report: lsp::UnchangedDocumentDiagnosticReport,
    registration_id: Option<SharedString>,
) {
    let result_id = SharedString::new(report.result_id);
    match diagnostics.entry(uri.clone()) {
        hash_map::Entry::Occupied(mut o) => match o.get_mut() {
            LspPullDiagnostics::Default => {
                o.insert(LspPullDiagnostics::Response {
                    server_id,
                    uri,
                    diagnostics: PulledDiagnostics::Unchanged { result_id },
                    registration_id,
                });
            }
            LspPullDiagnostics::Response {
                server_id: existing_server_id,
                uri: existing_uri,
                diagnostics: existing_diagnostics,
                ..
            } => {
                if server_id != *existing_server_id || &uri != existing_uri {
                    log::error!("Unexpected state: file {uri} has two different sets of diagnostics reported");
                }
                match existing_diagnostics {
                    PulledDiagnostics::Unchanged { .. } => {
                        *existing_diagnostics = PulledDiagnostics::Unchanged { result_id };
                    }
                    PulledDiagnostics::Changed { .. } => {}
                }
            }
        },
        hash_map::Entry::Vacant(v) => {
            v.insert(LspPullDiagnostics::Response {
                server_id,
                uri,
                diagnostics: PulledDiagnostics::Unchanged { result_id },
                registration_id,
            });
        }
    }
}

fn process_full_diagnostics_report(
    diagnostics: &mut FxHashMap<lsp::Uri, LspPullDiagnostics>,
    server_id: LanguageServerId,
    uri: lsp::Uri,
    report: lsp::FullDocumentDiagnosticReport,
    registration_id: Option<SharedString>,
) {
    let result_id = report.result_id.map(SharedString::new);
    match diagnostics.entry(uri.clone()) {
        hash_map::Entry::Occupied(mut o) => match o.get_mut() {
            LspPullDiagnostics::Default => {
                o.insert(LspPullDiagnostics::Response {
                    server_id,
                    uri,
                    diagnostics: PulledDiagnostics::Changed { result_id, diagnostics: report.items },
                    registration_id,
                });
            }
            LspPullDiagnostics::Response {
                server_id: existing_server_id,
                uri: existing_uri,
                diagnostics: existing_diagnostics,
                ..
            } => {
                if server_id != *existing_server_id || &uri != existing_uri {
                    log::error!("Unexpected state: file {uri} has two different sets of diagnostics reported");
                }
                match existing_diagnostics {
                    PulledDiagnostics::Unchanged { .. } => {
                        *existing_diagnostics = PulledDiagnostics::Changed { result_id, diagnostics: report.items };
                    }
                    PulledDiagnostics::Changed { result_id: existing_result_id, diagnostics: existing_diagnostics } => {
                        if result_id.is_some() {
                            *existing_result_id = result_id;
                        }
                        existing_diagnostics.extend(report.items);
                    }
                }
            }
        },
        hash_map::Entry::Vacant(v) => {
            v.insert(LspPullDiagnostics::Response {
                server_id,
                uri,
                diagnostics: PulledDiagnostics::Changed { result_id, diagnostics: report.items },
                registration_id,
            });
        }
    }
}
