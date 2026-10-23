use std::ops::Range;
use std::slice::ChunksExact;
use std::sync::Arc;

use anyhow::Result;
use clock::Global;
use collections::HashMap;
use collections::HashSet;
use futures::FutureExt as _;
use futures::future::Shared;
use futures::future::join_all;
use gpui::App;
use gpui::AppContext;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::ReadGlobal as _;
use gpui::SharedString;
use gpui::Task;
use language::Buffer;
use language::LanguageName;
use language::language_settings::all_language_settings;
use lsp::LanguageServerId;
use rpc::TypedEnvelope;
use rpc::proto;
use settings::DefaultSemanticTokenRules;
use settings::SemanticTokenRule;
use settings::SemanticTokenRules;
use settings::Settings as _;
use settings::SettingsStore;
use smol::future::yield_now;
use text::Anchor;
use text::Bias;
use text::OffsetUtf16;
use text::PointUtf16;
use text::Unclipped;
use util::ResultExt as _;

use crate::LanguageServerToQuery;
use crate::LspStore;
use crate::LspStoreEvent;
use crate::lsp_command::LspCommand;
use crate::lsp_command::SemanticTokensDelta;
use crate::lsp_command::SemanticTokensEdit;
use crate::lsp_command::SemanticTokensFull;
use crate::lsp_command::SemanticTokensResponse;
use crate::lsp_store::LanguageServerState;
use crate::lsp_store::document_selector_context_for_language;
use crate::lsp_store::document_selector_matches;
use crate::lsp_store::missing_servers_to_query;
use crate::project_settings::ProjectSettings;

type StylizerKey = (LanguageServerId, Option<LanguageName>);

pub(super) struct SemanticTokenConfig {
    stylizers: HashMap<StylizerKey, (lsp::SemanticTokensLegend, SemanticTokenStylizer)>,
    rules: SemanticTokenRules,
    global_mode: settings::SemanticTokens,
}

impl SemanticTokenConfig {
    pub(super) fn new(cx: &App) -> Self {
        Self {
            stylizers: HashMap::default(),
            rules: ProjectSettings::get_global(cx).global_lsp_settings.semantic_token_rules.clone(),
            global_mode: all_language_settings(None, cx).defaults.semantic_tokens,
        }
    }

    pub(super) fn remove_server_data(&mut self, server_id: LanguageServerId) {
        self.stylizers.retain(|&(id, _), _| id != server_id);
    }

    pub(super) fn update_rules(&mut self, new_rules: SemanticTokenRules) -> bool {
        if new_rules != self.rules {
            self.rules = new_rules;
            self.stylizers.clear();
            true
        } else {
            false
        }
    }

    /// Clears all cached stylizers.
    ///
    /// This is called when settings change to ensure that any modifications to
    /// language-specific semantic token rules
    /// are picked up. Stylizers are recreated lazily on next use.
    pub(super) fn clear_stylizers(&mut self) {
        self.stylizers.clear();
    }

    pub(super) fn update_global_mode(&mut self, new_mode: settings::SemanticTokens) -> bool {
        if new_mode != self.global_mode {
            self.global_mode = new_mode;
            true
        } else {
            false
        }
    }
}

impl LspStore {
    pub fn semantic_tokens(&mut self, buffer: Entity<Buffer>, cx: &mut Context<Self>) -> SemanticTokensTask {
        let version_queried_for = buffer.read(cx).version();
        let capability_probe = SemanticTokensFull { for_server: None };
        let current_servers = self.language_server_ids_for_request(&buffer, &capability_probe, cx);
        let empty_current_servers_are_authoritative = self.as_local().is_some();
        let latest_lsp_data = self.latest_lsp_data(&buffer, cx);
        let semantic_tokens_data = latest_lsp_data.semantic_tokens.get_or_insert_default();
        let pending_refreshes = std::mem::take(&mut semantic_tokens_data.pending_refreshes);
        let current_servers_are_authoritative = !current_servers.is_empty() || empty_current_servers_are_authoritative;
        let server_set_shrank = current_servers_are_authoritative
            && (semantic_tokens_data.raw_tokens.servers.keys().any(|server_id| !current_servers.contains(server_id))
                || semantic_tokens_data.fetched_servers.iter().any(|server_id| !current_servers.contains(server_id)));
        let cache_invalidated = !pending_refreshes.is_empty() || server_set_shrank;
        if cache_invalidated {
            semantic_tokens_data.update = None;
            semantic_tokens_data.generation += 1;
        }
        for refreshed_server in &pending_refreshes {
            semantic_tokens_data.raw_tokens.servers.remove(refreshed_server);
            semantic_tokens_data.fetched_servers.remove(refreshed_server);
        }
        let refreshed_servers = pending_refreshes
            .into_iter()
            .filter(|server_id| current_servers.contains(server_id))
            .collect::<HashSet<_>>();
        let missing_servers = if current_servers.is_empty() && current_servers_are_authoritative {
            semantic_tokens_data.raw_tokens.servers.clear();
            semantic_tokens_data.fetched_servers.clear();
            HashSet::default()
        } else {
            missing_servers_to_query(
                &mut semantic_tokens_data.raw_tokens.servers,
                &mut semantic_tokens_data.fetched_servers,
                &current_servers,
            )
            .unwrap_or_default()
        };
        if !missing_servers.is_empty() {
            semantic_tokens_data.update = None;
        }
        let query_generation = semantic_tokens_data.generation;

        if let Some((updating_for, task)) = &semantic_tokens_data.update
            && !version_queried_for.changed_since(updating_for)
        {
            return task.clone();
        }

        let mut servers_to_fetch = refreshed_servers;
        servers_to_fetch.extend(missing_servers);
        let rebuild_from_cache_only = cache_invalidated && servers_to_fetch.is_empty();
        let for_server = if servers_to_fetch.len() == 1 {
            servers_to_fetch.iter().next().copied()
        } else {
            // With multiple servers refreshed, query all of them instead of fanning out
            // filtered queries — the non-refreshed ones kept their tokens and answer
            // with cheap deltas.
            None
        };
        if !rebuild_from_cache_only {
            semantic_tokens_data.fetched_servers.extend(match for_server {
                Some(server_id) => HashSet::from_iter([server_id]),
                None => current_servers,
            });
        }
        let new_tokens = if rebuild_from_cache_only {
            Task::ready(None)
        } else {
            self.fetch_semantic_tokens_for_buffer(&buffer, for_server, cx)
        };

        let task_buffer = buffer.clone();
        let task_version_queried_for = version_queried_for.clone();
        let task = cx
            .spawn(async move |lsp_store, cx| {
                let buffer = task_buffer;
                let version_queried_for = task_version_queried_for;
                let res = if let Some(new_tokens) = new_tokens.await {
                    let (raw_tokens, buffer_snapshot) = lsp_store
                        .update(cx, |lsp_store, cx| {
                            let lsp_data = lsp_store.latest_lsp_data(&buffer, cx);
                            let semantic_tokens_data = lsp_data.semantic_tokens.get_or_insert_default();

                            if version_queried_for == lsp_data.buffer_version
                                && semantic_tokens_data.generation == query_generation
                            {
                                for (server_id, new_tokens_response) in new_tokens {
                                    match new_tokens_response {
                                        SemanticTokensResponse::Full { data, result_id } => {
                                            semantic_tokens_data.raw_tokens.servers.insert(
                                                server_id,
                                                Arc::new(ServerSemanticTokens::from_full(data, result_id)),
                                            );
                                        }
                                        SemanticTokensResponse::Delta { edits, result_id } => {
                                            if let Some(tokens) =
                                                semantic_tokens_data.raw_tokens.servers.get_mut(&server_id)
                                            {
                                                let tokens = Arc::make_mut(tokens);
                                                tokens.result_id = result_id;
                                                tokens.apply(&edits);
                                            }
                                        }
                                    }
                                }
                            }
                            let buffer_snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
                            (semantic_tokens_data.raw_tokens.clone(), buffer_snapshot)
                        })
                        .map_err(Arc::new)?;
                    Some(
                        cx.background_spawn(raw_to_buffer_semantic_tokens(raw_tokens, buffer_snapshot.text.clone()))
                            .await,
                    )
                } else {
                    let remaining_tokens = lsp_store.update(cx, |lsp_store, cx| {
                        let mut remaining_tokens = None;
                        if let Some(current_lsp_data) = lsp_store.current_lsp_data(buffer.read(cx).remote_id())
                            && current_lsp_data.buffer_version == version_queried_for
                            && let Some(semantic_tokens) = current_lsp_data.semantic_tokens.as_mut()
                            && semantic_tokens.generation == query_generation
                        {
                            if rebuild_from_cache_only {
                                let buffer_snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
                                remaining_tokens = Some((semantic_tokens.raw_tokens.clone(), buffer_snapshot));
                            } else if for_server.is_none() || semantic_tokens.raw_tokens.servers.is_empty() {
                                semantic_tokens.evict_all();
                            } else {
                                // A targeted fetch that sent no requests (e.g. the server
                                // lost the capability or does not serve this buffer) must
                                // not drop the other servers' cached tokens.
                                let buffer_snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
                                remaining_tokens = Some((semantic_tokens.raw_tokens.clone(), buffer_snapshot));
                            }
                        }
                        remaining_tokens
                    })?;
                    match remaining_tokens {
                        Some((raw_tokens, buffer_snapshot)) => Some(
                            cx.background_spawn(raw_to_buffer_semantic_tokens(
                                raw_tokens,
                                buffer_snapshot.text.clone(),
                            ))
                            .await,
                        ),
                        None => None,
                    }
                };
                Ok(BufferSemanticTokens { tokens: res })
            })
            .shared();

        self.latest_lsp_data(&buffer, cx).semantic_tokens.get_or_insert_default().update =
            Some((version_queried_for, task.clone()));

        task
    }

    pub(super) fn fetch_semantic_tokens_for_buffer(
        &mut self,
        buffer: &Entity<Buffer>,
        for_server: Option<LanguageServerId>,
        cx: &mut Context<Self>,
    ) -> Task<Option<HashMap<LanguageServerId, SemanticTokensResponse>>> {
        if let Some((client, upstream_project_id)) = self.upstream_client() {
            let request = SemanticTokensFull { for_server };
            if !self.is_capable_for_proto_request(buffer, &request, cx) {
                return Task::ready(None);
            }

            let request_timeout = ProjectSettings::get_global(cx).global_lsp_settings.get_request_timeout();
            let request_task = client.request_lsp(
                upstream_project_id,
                None,
                request_timeout,
                cx.background_executor().clone(),
                request.to_proto(upstream_project_id, buffer.read(cx)),
            );
            let buffer = buffer.clone();
            cx.spawn(async move |weak_lsp_store, cx| {
                let lsp_store = weak_lsp_store.upgrade()?;
                let tokens = join_all(
                    request_task
                        .await
                        .log_err()
                        .flatten()
                        .map(|response| response.payload)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|response| {
                            let server_id = LanguageServerId::from_proto(response.server_id);
                            let response = request.response_from_proto(
                                response.response,
                                lsp_store.clone(),
                                buffer.clone(),
                                cx.clone(),
                            );
                            async move {
                                match response.await {
                                    Ok(tokens) => Some((server_id, tokens)),
                                    Err(e) => {
                                        log::error!(
                                            "Failed to query remote semantic tokens for server {server_id:?}: {e:#}"
                                        );
                                        None
                                    }
                                }
                            }
                        }),
                )
                .await
                .into_iter()
                .flatten()
                .collect();
                Some(tokens)
            })
        } else {
            let full_request = SemanticTokensFull { for_server: None };
            let token_tasks = self
                .language_server_ids_for_request(buffer, &full_request, cx)
                .into_iter()
                .filter(|&server_id| for_server.is_none_or(|for_server_id| for_server_id == server_id))
                .map(|server_id| {
                    let request_task = match self.semantic_tokens_result_id(server_id, buffer, cx) {
                        Some(result_id) => {
                            let delta_request = SemanticTokensDelta { previous_result_id: result_id };
                            if self.language_server_ids_for_request(buffer, &delta_request, cx).contains(&server_id) {
                                self.request_lsp(
                                    buffer.clone(),
                                    LanguageServerToQuery::Other(server_id),
                                    delta_request,
                                    cx,
                                )
                            } else {
                                self.request_lsp(
                                    buffer.clone(),
                                    LanguageServerToQuery::Other(server_id),
                                    SemanticTokensFull { for_server: Some(server_id) },
                                    cx,
                                )
                            }
                        }
                        None => self.request_lsp(
                            buffer.clone(),
                            LanguageServerToQuery::Other(server_id),
                            SemanticTokensFull { for_server: Some(server_id) },
                            cx,
                        ),
                    };
                    async move { (server_id, request_task.await) }
                })
                .collect::<Vec<_>>();
            if token_tasks.is_empty() {
                return Task::ready(None);
            }

            cx.background_spawn(async move {
                Some(
                    join_all(token_tasks)
                        .await
                        .into_iter()
                        .flat_map(|(server_id, response)| match response {
                            Ok(tokens) => Some((server_id, tokens)),
                            Err(e) => {
                                log::error!("Failed to query remote semantic tokens for server {server_id:?}: {e:#}");
                                None
                            }
                        })
                        .collect(),
                )
            })
        }
    }

    /// Marks the server's semantic tokens as refresh-pending in every buffer, to be
    /// invalidated by the next query, and notifies the observers.
    pub(crate) fn refresh_semantic_tokens(&mut self, server_id: LanguageServerId, cx: &mut Context<Self>) {
        for lsp_data in self.lsp_data.values_mut() {
            if let Some(semantic_tokens) = &mut lsp_data.semantic_tokens {
                semantic_tokens.pending_refreshes.insert(server_id);
            }
        }
        cx.emit(LspStoreEvent::RefreshSemanticTokens { server_id });
        if let Some((client, project_id)) = self.downstream_client.as_ref() {
            client
                .send(proto::RefreshSemanticTokens {
                    project_id: *project_id,
                    server_id: server_id.to_proto(),
                    request_id: Some(super::next_wire_refresh_request_id()),
                })
                .log_err();
        }
    }

    pub(crate) async fn handle_refresh_semantic_tokens(
        lsp_store: Entity<Self>,
        envelope: TypedEnvelope<proto::RefreshSemanticTokens>,
        mut cx: AsyncApp,
    ) -> Result<proto::Ack> {
        lsp_store.update(&mut cx, |lsp_store, cx| {
            lsp_store.refresh_semantic_tokens(LanguageServerId::from_proto(envelope.payload.server_id), cx);
        });
        Ok(proto::Ack {})
    }

    fn semantic_tokens_result_id(
        &mut self,
        server_id: LanguageServerId,
        buffer: &Entity<Buffer>,
        cx: &mut App,
    ) -> Option<SharedString> {
        self.latest_lsp_data(buffer, cx).semantic_tokens.as_ref()?.raw_tokens.servers.get(&server_id)?.result_id.clone()
    }

    pub fn get_or_create_token_stylizer(
        &mut self,
        server_id: LanguageServerId,
        language: Option<&LanguageName>,
        cx: &mut App,
    ) -> Option<&SemanticTokenStylizer> {
        let key = (server_id, language.cloned());
        let legend = self.semantic_tokens_legend(server_id, language)?;
        let up_to_date =
            self.semantic_token_config.stylizers.get(&key).is_some_and(|(cached_legend, _)| cached_legend == legend);
        if !up_to_date {
            let legend = legend.clone();
            let language_rules = language
                .and_then(|language| SettingsStore::global(cx).language_semantic_token_rules(language.as_ref()));
            let stylizer = SemanticTokenStylizer::new(server_id, &legend, language_rules, cx);
            self.semantic_token_config.stylizers.insert(key.clone(), (legend, stylizer));
        }
        self.semantic_token_config.stylizers.get(&key).map(|(_, stylizer)| stylizer)
    }

    fn semantic_tokens_legend(
        &self,
        server_id: LanguageServerId,
        language: Option<&LanguageName>,
    ) -> Option<&lsp::SemanticTokensLegend> {
        if let Some(local) = self.as_local()
            && let Some(language) = language
            && let Some(LanguageServerState::Running { adapter, .. }) = local.language_servers.get(&server_id)
        {
            let context = document_selector_context_for_language(language, adapter);
            if let Some(registrations) = local
                .language_server_dynamic_registrations
                .get(&server_id)
                .and_then(|registrations| registrations.text_documents.get("textDocument/semanticTokens"))
            {
                for registration in registrations.values().rev() {
                    if document_selector_matches(registration.document_selector.as_ref(), &context)
                        && let Some(provider) = registration.server_capabilities.semantic_tokens_provider.as_ref()
                    {
                        return Some(semantic_tokens_provider_legend(provider));
                    }
                }
            }
            if let Some(provider) = local
                .initial_server_capabilities
                .get(&server_id)
                .and_then(|capabilities| capabilities.semantic_tokens_provider.as_ref())
            {
                return Some(semantic_tokens_provider_legend(provider));
            }
        }

        if let Some(initial_capabilities) = self.lsp_server_initial_capabilities.get(&server_id) {
            let context = self.remote_document_selector_context(server_id, language);
            if let Some(registrations) = self
                .lsp_server_text_document_registrations
                .get(&server_id)
                .and_then(|registrations| registrations.get("textDocument/semanticTokens"))
            {
                // Token indices are only meaningful with the provider's legend, so do not
                // guess when the client cannot reconstruct the remote server's selector context.
                for registration in registrations.values().rev() {
                    let matches = match context.as_ref() {
                        Some(context) => document_selector_matches(registration.document_selector.as_ref(), context),
                        None if registration.document_selector.is_none() => true,
                        None => return None,
                    };
                    if matches
                        && let Some(provider) = registration.server_capabilities.semantic_tokens_provider.as_ref()
                    {
                        return Some(semantic_tokens_provider_legend(provider));
                    }
                }
            }

            let provider = initial_capabilities.semantic_tokens_provider.as_ref()?;
            return Some(semantic_tokens_provider_legend(provider));
        }

        let tokens_provider = self.lsp_server_capabilities.get(&server_id)?.semantic_tokens_provider.as_ref()?;
        Some(semantic_tokens_provider_legend(tokens_provider))
    }
}

fn semantic_tokens_provider_legend(provider: &lsp::SemanticTokensServerCapabilities) -> &lsp::SemanticTokensLegend {
    match provider {
        lsp::SemanticTokensServerCapabilities::SemanticTokensOptions(options) => &options.legend,
        lsp::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(options) => {
            &options.semantic_tokens_options.legend
        }
    }
}

pub type SemanticTokensTask = Shared<Task<std::result::Result<BufferSemanticTokens, Arc<anyhow::Error>>>>;

#[derive(Debug, Default, Clone)]
pub struct BufferSemanticTokens {
    pub tokens: Option<HashMap<LanguageServerId, Arc<[BufferSemanticToken]>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenType(pub u32);

#[derive(Debug, Clone)]
pub struct BufferSemanticToken {
    /// The range of the token in the buffer.
    ///
    /// Guaranteed to contain a buffer id.
    pub range: Range<Anchor>,
    pub token_type: TokenType,
    pub token_modifiers: u32,
}

pub struct SemanticTokenStylizer {
    server_id: LanguageServerId,
    rules_by_token_type: HashMap<TokenType, Vec<SemanticTokenRule>>,
    token_type_names: HashMap<TokenType, SharedString>,
    modifier_mask: HashMap<SharedString, u32>,
}

impl SemanticTokenStylizer {
    pub fn new(
        server_id: LanguageServerId,
        legend: &lsp::SemanticTokensLegend,
        language_rules: Option<&SemanticTokenRules>,
        cx: &App,
    ) -> Self {
        let token_types: HashMap<TokenType, SharedString> = legend
            .token_types
            .iter()
            .enumerate()
            .map(|(i, token_type)| (TokenType(i as u32), SharedString::from(token_type.as_str().to_string())))
            .collect();
        let modifier_mask: HashMap<SharedString, u32> = legend
            .token_modifiers
            .iter()
            .enumerate()
            .map(|(i, modifier)| (SharedString::from(modifier.as_str().to_string()), 1 << i))
            .collect();

        let global_rules = &ProjectSettings::get_global(cx).global_lsp_settings.semantic_token_rules;
        let default_rules = cx.global::<DefaultSemanticTokenRules>();

        let rules_by_token_type = token_types
            .iter()
            .map(|(index, token_type_name)| {
                let filter = |rule: &&SemanticTokenRule| {
                    rule.token_type.as_ref().is_none_or(|rule_token_type| rule_token_type == token_type_name.as_ref())
                };
                let matching_rules: Vec<SemanticTokenRule> = global_rules
                    .rules
                    .iter()
                    .chain(language_rules.into_iter().flat_map(|lr| &lr.rules))
                    .chain(default_rules.0.rules.iter())
                    .rev()
                    .filter(filter)
                    .cloned()
                    .collect();
                (*index, matching_rules)
            })
            .collect();

        SemanticTokenStylizer { server_id, rules_by_token_type, token_type_names: token_types, modifier_mask }
    }

    pub fn server_id(&self) -> LanguageServerId {
        self.server_id
    }

    pub fn token_type_name(&self, token_type: TokenType) -> Option<&SharedString> {
        self.token_type_names.get(&token_type)
    }

    pub fn has_modifier(&self, token_modifiers: u32, modifier: &str) -> bool {
        let Some(mask) = self.modifier_mask.get(modifier) else {
            return false;
        };
        (token_modifiers & mask) != 0
    }

    pub fn token_modifiers(&self, token_modifiers: u32) -> Option<String> {
        let modifiers: Vec<&str> = self
            .modifier_mask
            .iter()
            .filter(|(_, mask)| (token_modifiers & *mask) != 0)
            .map(|(name, _)| name.as_ref())
            .collect();
        if modifiers.is_empty() { None } else { Some(modifiers.join(", ")) }
    }

    pub fn rules_for_token(&self, token_type: TokenType) -> Option<&[SemanticTokenRule]> {
        self.rules_by_token_type.get(&token_type).map(|v| v.as_slice())
    }
}

async fn raw_to_buffer_semantic_tokens(
    raw_tokens: RawSemanticTokens,
    buffer_snapshot: text::BufferSnapshot,
) -> HashMap<LanguageServerId, Arc<[BufferSemanticToken]>> {
    let mut res = HashMap::default();
    for (&server_id, server_tokens) in &raw_tokens.servers {
        let mut last = 0;
        // We don't do `collect` here due to the filter map not pre-allocating
        // we'd rather over allocate here than not since we have to re-allocate into an arc slice anyways
        let mut buffer_tokens = Vec::with_capacity(server_tokens.data.len() / 5);
        let mut tokens = server_tokens.tokens();
        // 5000 was chosen by profiling, on a decent machine this will take about 1ms per chunk
        // This is to avoid blocking the main thread for hundreds of milliseconds at a time for very big files
        // If we every change the below code to not query the underlying rope 6 times per token we can bump this up
        const CHUNK_LEN: usize = 5000;
        loop {
            let mut changed = false;
            let chunk = tokens.by_ref().take(CHUNK_LEN).inspect(|_| changed = true).filter_map(|token| {
                let start = Unclipped(PointUtf16::new(token.line, token.start));
                let clipped_start = buffer_snapshot.clip_point_utf16(start, Bias::Left);
                let start_offset = buffer_snapshot.as_rope().point_utf16_to_offset_utf16(clipped_start);
                let end_offset = start_offset + OffsetUtf16(token.length as usize);

                let start = buffer_snapshot.as_rope().offset_utf16_to_offset(start_offset);
                if start < last {
                    return None;
                }

                let end = buffer_snapshot.as_rope().offset_utf16_to_offset(end_offset);
                last = end;

                if start == end {
                    return None;
                }

                Some(BufferSemanticToken {
                    range: buffer_snapshot.anchor_range_inside(start..end),
                    token_type: token.token_type,
                    token_modifiers: token.token_modifiers,
                })
            });
            buffer_tokens.extend(chunk);

            if !changed {
                break;
            }
            yield_now().await;
        }

        res.insert(server_id, buffer_tokens.into());
    }
    res
}

#[derive(Default, Debug)]
pub struct SemanticTokensData {
    pub(super) raw_tokens: RawSemanticTokens,
    pub(super) pending_refreshes: HashSet<LanguageServerId>,
    fetched_servers: HashSet<LanguageServerId>,
    update: Option<(Global, SemanticTokensTask)>,
    /// Bumped on every eviction so that fetches started against the evicted state
    /// cannot write their stale results into the new one.
    generation: u64,
}

impl SemanticTokensData {
    pub(super) fn remove_server_data(&mut self, server_id: LanguageServerId) {
        self.raw_tokens.servers.remove(&server_id);
        self.pending_refreshes.remove(&server_id);
        self.fetched_servers.remove(&server_id);
        self.update = None;
        self.generation += 1;
    }

    fn evict_all(&mut self) {
        self.raw_tokens.servers.clear();
        self.fetched_servers.clear();
        self.update = None;
        self.generation += 1;
    }
}

/// All the semantic token tokens for a buffer.
///
/// This aggregates semantic tokens from multiple language servers in a specific order.
/// Semantic tokens later in the list will override earlier ones in case of overlap.
#[derive(Default, Debug, Clone)]
pub(super) struct RawSemanticTokens {
    pub servers: HashMap<lsp::LanguageServerId, Arc<ServerSemanticTokens>>,
}

/// All the semantic tokens for a buffer, from a single language server.
#[derive(Debug, Clone)]
pub struct ServerSemanticTokens {
    /// Each value is:
    /// data[5*i] - deltaLine: token line number, relative to the start of the previous token
    /// data[5*i+1] - deltaStart: token start character, relative to the start of the previous token (relative to 0 or the previous token’s start if they are on the same line)
    /// data[5*i+2] - length: the length of the token.
    /// data[5*i+3] - tokenType: will be looked up in SemanticTokensLegend.tokenTypes. We currently ask that tokenType < 65536.
    /// data[5*i+4] - tokenModifiers: each set bit will be looked up in SemanticTokensLegend.tokenModifiers
    ///
    /// See https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/ for more.
    data: Vec<u32>,

    pub(crate) result_id: Option<SharedString>,
}

pub struct SemanticTokensIter<'a> {
    prev: Option<(u32, u32)>,
    data: ChunksExact<'a, u32>,
}

// A single item from `data`.
struct SemanticTokenValue {
    delta_line: u32,
    delta_start: u32,
    length: u32,
    token_type: TokenType,
    token_modifiers: u32,
}

/// A semantic token, independent of its position.
#[derive(Debug, PartialEq, Eq)]
pub struct SemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub token_type: TokenType,
    pub token_modifiers: u32,
}

impl ServerSemanticTokens {
    pub fn from_full(data: Vec<u32>, result_id: Option<SharedString>) -> Self {
        ServerSemanticTokens { data, result_id }
    }

    pub(crate) fn apply(&mut self, edits: &[SemanticTokensEdit]) {
        for edit in edits {
            let start = (edit.start as usize).min(self.data.len());
            let end = (start + edit.delete_count as usize).min(self.data.len());
            self.data.splice(start..end, edit.data.iter().copied());
        }
    }

    pub fn tokens(&self) -> SemanticTokensIter<'_> {
        SemanticTokensIter { prev: None, data: self.data.chunks_exact(5) }
    }
}

impl Iterator for SemanticTokensIter<'_> {
    type Item = SemanticToken;

    fn next(&mut self) -> Option<Self::Item> {
        let chunk = self.data.next()?;
        let token = SemanticTokenValue {
            delta_line: chunk[0],
            delta_start: chunk[1],
            length: chunk[2],
            token_type: TokenType(chunk[3]),
            token_modifiers: chunk[4],
        };

        let (line, start) = if let Some((last_line, last_start)) = self.prev {
            let line = last_line + token.delta_line;
            let start = if token.delta_line == 0 { last_start + token.delta_start } else { token.delta_start };
            (line, start)
        } else {
            (token.delta_line, token.delta_start)
        };

        self.prev = Some((line, start));

        Some(SemanticToken {
            line,
            start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers: token.token_modifiers,
        })
    }
}
