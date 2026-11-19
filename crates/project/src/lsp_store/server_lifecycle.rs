use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::btree_map as std_collections_btree_map;
use std::collections::btree_map;
use std::ffi::OsStr;
use std::mem;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::vec;

use anyhow::Context as _;
use anyhow::Result;
use async_macos::command;
use async_trait::async_trait;
use fs::Fs;
use futures::Future;
use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::future::Shared;
use futures::future::join_all;
use futures::select;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::PromptLevel;
use gpui::SharedString;
use gpui::Task;
use gpui::WeakEntity;
use itertools::Itertools as _;
use language::BinaryStatus;
use language::Buffer;
use language::CachedLspAdapter;
use language::DiagnosticSet;
use language::DiagnosticSourceKind;
use language::File as _;
use language::LanguageName;
use language::LanguageRegistry;
use language::LanguageServerStatusUpdate;
use language::LocalFile as _;
use language::LspAdapter;
use language::LspAdapterDelegate;
use language::ServerNotInstalled;
use lsp::LSP_REQUEST_TIMEOUT;
use lsp::LanguageServer;
use lsp::LanguageServerBinary;
use lsp::LanguageServerId;
use lsp::LanguageServerName;
use lsp::LanguageServerSelector;
use lsp::MessageActionItem;
use lsp::Uri;
use lsp::WorkDoneProgressCancelParams;
use lsp::notification::DidChangeConfiguration;
use lsp::notification::LogMessage;
use lsp::notification::LogTrace;
use lsp::notification::Progress;
use lsp::notification::PublishDiagnostics;
use lsp::notification::ShowMessage;
use lsp::notification::WorkDoneProgressCancel;
use lsp::request::ApplyWorkspaceEdit;
use lsp::request::CodeLensRefresh;
use lsp::request::InlayHintRefreshRequest;
use lsp::request::RegisterCapability;
use lsp::request::SemanticTokensRefresh;
use lsp::request::ShowDocument;
use lsp::request::ShowMessageRequest;
use lsp::request::UnregisterCapability;
use lsp::request::WorkDoneProgressCreate;
use lsp::request::WorkspaceConfiguration;
use lsp::request::WorkspaceDiagnosticRefresh;
use lsp::request::WorkspaceFoldersRequest;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use util::ResultExt as _;
use util::rel_path::RelPath;
use util::union_json_value_into;
use worktree::File;
use worktree::Worktree;
use worktree::WorktreeId;

use super::LanguageServerLogType;
use super::LanguageServerProgress;
use super::LanguageServerPromptRequest;
use super::LanguageServerSeed;
use super::LanguageServerShowDocumentRequest;
use super::LanguageServerState;
use super::LanguageServerStatus;
use super::LocalLspStore;
use super::LspBufferSnapshot;
use super::LspStore;
use super::LspStoreEvent;
use super::ProgressToken;
use super::SERVER_LAUNCHING_BEFORE_SHUTDOWN_TIMEOUT;
use super::SERVER_PROGRESS_THROTTLE_TIMEOUT;
use super::UnifiedLanguageServer;
use super::WORKSPACE_DIAGNOSTICS_TOKEN_START;
use super::diagnostics::DocumentDiagnosticsUpdate;
use super::diagnostics::lsp_workspace_diagnostics_refresh;
use super::file_watchers::RenamePathsWatchedForServer;
use super::rust_analyzer_ext;
use super::workspace_folder_uri;
use crate::ProjectPath;
use crate::environment::ProjectEnvironment;
use crate::manifest_tree::LaunchDisposition;
use crate::manifest_tree::ManifestQueryDelegate;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LanguageServerStartMode {
    Automatic,
    Explicit,
}

pub(super) fn subscribe_to_binary_statuses(languages: &Arc<LanguageRegistry>, cx: &Context<'_, LspStore>) -> Task<()> {
    let (mut server_statuses, subscription) = languages.language_server_binary_statuses();
    cx.spawn(async move |lsp_store, cx| {
        let _subscription = subscription;
        while let Some((source, server_name, binary_status)) = server_statuses.next().await {
            if lsp_store
                .update(cx, |lsp_store, cx| {
                    if source.is_some_and(|source| source != cx.entity_id())
                        || source.is_none() && !lsp_store.should_forward_untagged_binary_status(&server_name)
                    {
                        return;
                    }

                    cx.emit(LspStoreEvent::LanguageServerStatus {
                        // The binary might not have a language server id yet; receivers ignore it.
                        language_server_id: LanguageServerId(0),
                        name: Some(server_name),
                        status: LanguageServerStatusUpdate::Binary(binary_status),
                    });
                })
                .is_err()
            {
                break;
            }
        }
    })
}

pub struct LocalLspAdapterDelegate {
    lsp_store: WeakEntity<LspStore>,
    worktree: worktree::Snapshot,
    fs: Arc<dyn Fs>,
    language_registry: Arc<LanguageRegistry>,
    load_shell_env_task: Shared<Task<Option<FxHashMap<String, String>>>>,
}

impl LocalLspAdapterDelegate {
    pub fn new(
        language_registry: Arc<LanguageRegistry>,
        environment: &Entity<ProjectEnvironment>,
        lsp_store: WeakEntity<LspStore>,
        worktree: &Entity<Worktree>,
        fs: Arc<dyn Fs>,
        cx: &mut App,
    ) -> Arc<Self> {
        let load_shell_env_task = environment.update(cx, |env, cx| env.worktree_environment(worktree.clone(), cx));

        Arc::new(Self { lsp_store, worktree: worktree.read(cx).snapshot(), fs, language_registry, load_shell_env_task })
    }

    pub fn from_local_lsp(local: &LocalLspStore, worktree: &Entity<Worktree>, cx: &mut App) -> Arc<Self> {
        Self::new(
            Arc::clone(&local.languages),
            &local.environment,
            local.weak.clone(),
            worktree,
            Arc::clone(&local.fs),
            cx,
        )
    }
}

#[async_trait]
impl LspAdapterDelegate for LocalLspAdapterDelegate {
    fn worktree_id(&self) -> WorktreeId {
        self.worktree.id()
    }

    fn worktree_root_path(&self) -> &Path {
        self.worktree.abs_path().as_ref()
    }

    fn resolve_relative_path(&self, path: PathBuf) -> PathBuf {
        self.worktree.resolve_relative_path(path)
    }

    async fn shell_env(&self) -> FxHashMap<String, String> {
        let task = self.load_shell_env_task.clone();
        task.await.unwrap_or_default()
    }

    async fn which(&self, command: &OsStr) -> Option<PathBuf> {
        let mut worktree_abs_path = self.worktree_root_path().to_path_buf();
        if self.fs.is_file(&worktree_abs_path).await {
            worktree_abs_path.pop();
        }

        let env = self.shell_env().await;

        let shell_path = env.get("PATH").cloned();

        which::which_in(command, shell_path.as_ref(), worktree_abs_path).ok()
    }

    async fn try_exec(&self, command: LanguageServerBinary) -> Result<()> {
        let mut working_dir = self.worktree_root_path().to_path_buf();
        if self.fs.is_file(&working_dir).await {
            working_dir.pop();
        }
        let output = command::new_command(&command.path)
            .args(command.arguments)
            .envs(command.env.clone().unwrap_or_default())
            .current_dir(working_dir)
            .output()
            .await?;

        anyhow::ensure!(
            output.status.success(),
            "{}, stdout: {:?}, stderr: {:?}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    fn status_source_id(&self) -> gpui::EntityId {
        self.lsp_store.entity_id()
    }

    fn update_status(&self, server_name: LanguageServerName, status: language::BinaryStatus) {
        self.language_registry.update_lsp_binary_status_for_entity(self.status_source_id(), server_name, status);
    }

    fn registered_lsp_adapters(&self) -> Vec<Arc<dyn LspAdapter>> {
        self.language_registry
            .all_lsp_adapters()
            .into_iter()
            .map(|adapter| Arc::clone(&adapter.adapter) as Arc<dyn LspAdapter>)
            .sorted_by_key(|adapter| adapter.name())
            .collect()
    }

    // Reads from disk, not the worktree snapshot: servers start on project open before the scan has
    // reached `.ed/` (often gitignored), and the lint switches must still apply.
    async fn read_text_file(&self, path: &RelPath) -> Result<String> {
        self.fs.load(&self.worktree.absolutize(path)).await
    }
}

pub(super) fn include_text(server: &lsp::LanguageServer) -> Option<bool> {
    match server.capabilities().text_document_sync.as_ref()? {
        lsp::TextDocumentSyncCapability::Options(opts) => match opts.save.as_ref()? {
            // Server wants didSave but didn't specify includeText.
            lsp::TextDocumentSyncSaveOptions::Supported(true) => Some(false),
            // Server doesn't want didSave at all.
            lsp::TextDocumentSyncSaveOptions::Supported(false) => None,
            // Server provided SaveOptions.
            lsp::TextDocumentSyncSaveOptions::SaveOptions(save_options) => {
                Some(save_options.include_text.unwrap_or(false))
            }
        },
        // We do not have any save info. Kind affects didChange only.
        lsp::TextDocumentSyncCapability::Kind(_) => None,
    }
}

impl LocalLspStore {
    fn can_start_language_server(&self, name: &LanguageServerName) -> bool {
        self.language_server_start_overrides.get(name).copied().unwrap_or(self.auto_start_language_servers)
    }

    pub(super) fn get_or_insert_language_server(
        &mut self,
        worktree_handle: &Entity<Worktree>,
        delegate: Arc<LocalLspAdapterDelegate>,
        disposition: &Arc<LaunchDisposition>,
        language_name: &LanguageName,
        start_mode: LanguageServerStartMode,
        cx: &App,
    ) -> Option<LanguageServerId> {
        let key =
            LanguageServerSeed { worktree_id: worktree_handle.read(cx).id(), name: disposition.server_name.clone() };
        if start_mode == LanguageServerStartMode::Explicit {
            self.language_server_start_overrides.insert(key.name.clone(), true);
        }
        if let Some(state) = self.language_server_ids.get_mut(&key) {
            state.project_roots.insert(Arc::clone(&disposition.path.path));
            Some(state.id)
        } else {
            if start_mode == LanguageServerStartMode::Automatic && !self.can_start_language_server(&key.name) {
                return None;
            }
            let adapter = self
                .languages
                .lsp_adapters(language_name)
                .into_iter()
                .find(|adapter| adapter.name() == disposition.server_name)
                .expect("To find LSP adapter");
            let new_language_server_id =
                self.start_language_server(worktree_handle, delegate, adapter, key.clone(), language_name.clone(), cx);
            if let Some(state) = self.language_server_ids.get_mut(&key) {
                state.project_roots.insert(Arc::clone(&disposition.path.path));
            }
            Some(new_language_server_id)
        }
    }

    fn update_binary_status(&self, server_name: LanguageServerName, status: BinaryStatus) {
        self.languages.update_lsp_binary_status_for_entity(self.weak.entity_id(), server_name, status);
    }

    fn start_language_server(
        &mut self,
        worktree_handle: &Entity<Worktree>,
        delegate: Arc<LocalLspAdapterDelegate>,
        adapter: Arc<CachedLspAdapter>,
        key: LanguageServerSeed,
        language_name: LanguageName,
        cx: &App,
    ) -> LanguageServerId {
        let worktree = worktree_handle.read(cx);

        let worktree_abs_path = worktree.abs_path();

        let stderr_capture = Arc::new(Mutex::new(Some(String::new())));

        let server_id = self.languages.next_language_server_id();
        log::trace!(
            "attempting to start language server {:?}, path: {worktree_abs_path:?}, id: {server_id}",
            adapter.name.0
        );

        let binary =
            self.get_language_server_binary(Arc::clone(&adapter), Arc::<LocalLspAdapterDelegate>::clone(&delegate), cx);
        let pending_workspace_folders = Arc::<Mutex<BTreeSet<Uri>>>::default();

        let pending_server = cx.spawn({
            let adapter = Arc::clone(&adapter);
            let server_name = adapter.name.clone();
            let stderr_capture = Arc::clone(&stderr_capture);
            let pending_workspace_folders = Arc::clone(&pending_workspace_folders);
            async move |cx| {
                let binary = binary.await?;

                let code_action_kinds = adapter.code_action_kinds();
                lsp::LanguageServer::new(
                    stderr_capture,
                    server_id,
                    server_name,
                    binary,
                    &worktree_abs_path,
                    code_action_kinds,
                    Some(pending_workspace_folders),
                    cx,
                )
            }
        });

        let startup = {
            let server_name = adapter.name.0.clone();
            let delegate = delegate as Arc<dyn LspAdapterDelegate>;
            let key = key.clone();
            let adapter = Arc::clone(&adapter);
            let lsp_store = self.weak.clone();
            let pending_workspace_folders = Arc::clone(&pending_workspace_folders);
            cx.spawn(async move |cx| {
                let result = async {
                    let language_server = pending_server.await?;

                    let workspace_config =
                        Self::workspace_configuration_for_adapter(Arc::clone(&adapter.adapter), &delegate, None, cx)
                            .await?;

                    let initialization_options =
                        Self::initialization_options_for_adapter(Arc::clone(&adapter.adapter), &delegate, cx).await?;

                    let initialization_params = cx.update(|cx| {
                        let mut params = language_server.default_initialize_params();
                        params.initialization_options = initialization_options;
                        adapter.adapter.prepare_initialize_params(params, cx)
                    })?;

                    Self::setup_lsp_messages(
                        lsp_store.clone(),
                        &language_server,
                        Arc::clone(&delegate),
                        Arc::clone(&adapter),
                    );

                    let did_change_configuration_params =
                        lsp::DidChangeConfigurationParams { settings: workspace_config };
                    let language_server = cx
                        .update(|cx| {
                            let request_timeout = LSP_REQUEST_TIMEOUT;

                            language_server.initialize(
                                initialization_params,
                                Arc::new(did_change_configuration_params.clone()),
                                request_timeout,
                                cx,
                            )
                        })
                        .await
                        .inspect_err(|_| {
                            if let Some(lsp_store) = lsp_store.upgrade() {
                                lsp_store.update(cx, |lsp_store, cx| {
                                    lsp_store.cleanup_lsp_data(server_id);
                                    cx.emit(LspStoreEvent::LanguageServerRemoved(server_id));
                                });
                            }
                        })?;

                    language_server.notify::<DidChangeConfiguration>(did_change_configuration_params)?;

                    anyhow::Ok(language_server)
                }
                .await;

                match result {
                    Ok(server) => {
                        lsp_store
                            .update(cx, |lsp_store, cx| {
                                lsp_store.insert_newly_running_language_server(
                                    adapter,
                                    Arc::clone(&server),
                                    server_id,
                                    key,
                                    language_name,
                                    pending_workspace_folders,
                                    cx,
                                );
                            })
                            .ok();
                        stderr_capture.lock().take();
                        Some(server)
                    }

                    Err(err) if err.is::<ServerNotInstalled>() => {
                        delegate.update_status(adapter.name(), BinaryStatus::NotInstalled { message: err.to_string() });
                        log::info!("{err}");
                        None
                    }

                    Err(err) => {
                        let log = stderr_capture.lock().take().unwrap_or_default();
                        delegate.update_status(
                            adapter.name(),
                            BinaryStatus::Failed {
                                error: if log.is_empty() {
                                    format!("{err:#}")
                                } else {
                                    format!("{err:#}\n-- stderr --\n{log}")
                                },
                            },
                        );
                        log::error!("Failed to start language server {server_name:?}: {err:?}");
                        if !log.is_empty() {
                            log::error!("server stderr: {log}");
                        }
                        None
                    }
                }
            })
        };
        let state = LanguageServerState::Starting { startup, pending_workspace_folders };

        self.update_binary_status(adapter.name(), BinaryStatus::Starting);

        self.language_servers.insert(server_id, state);
        self.language_server_ids
            .entry(key)
            .or_insert(UnifiedLanguageServer { id: server_id, project_roots: FxHashSet::default() });
        server_id
    }

    fn get_language_server_binary(
        &self,
        adapter: Arc<CachedLspAdapter>,
        delegate: Arc<dyn LspAdapterDelegate>,
        cx: &App,
    ) -> Task<Result<LanguageServerBinary>> {
        cx.spawn(async move |cx| {
            let mut binary = adapter.find_binary(&delegate, cx).await?;
            log::info!(
                "found user-installed language server for {}. path: {:?}, arguments: {:?}",
                adapter.name(),
                binary.path,
                binary.arguments
            );

            let mut shell_env = delegate.shell_env().await;
            shell_env.extend(binary.env.take().unwrap_or_default());
            binary.env = Some(shell_env);
            Ok(binary)
        })
    }

    fn setup_lsp_messages(
        lsp_store: WeakEntity<LspStore>,
        language_server: &LanguageServer,
        delegate: Arc<dyn LspAdapterDelegate>,
        adapter: Arc<CachedLspAdapter>,
    ) {
        let name = language_server.name();
        let server_id = language_server.server_id();
        language_server
            .on_notification::<PublishDiagnostics, _>({
                let adapter = Arc::clone(&adapter);
                let this = lsp_store.clone();
                move |mut params, cx| {
                    let adapter = Arc::clone(&adapter);
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            adapter.process_diagnostics(&mut params, server_id);

                            this.merge_lsp_diagnostics(
                                DiagnosticSourceKind::Pushed,
                                vec![DocumentDiagnosticsUpdate {
                                    server_id,
                                    diagnostics: params,
                                    result_id: None,
                                    disk_based_sources: Cow::Borrowed(&adapter.disk_based_diagnostic_sources),
                                    registration_id: None,
                                }],
                                |_, diagnostic, _cx| diagnostic.source_kind == DiagnosticSourceKind::Pulled,
                                cx,
                            )
                            .log_err();
                        });
                    }
                }
            })
            .detach();
        language_server
            .on_request::<WorkspaceConfiguration, _, _>({
                let adapter = Arc::clone(&adapter.adapter);
                let delegate = Arc::clone(&delegate);
                move |params, cx| {
                    let adapter = Arc::clone(&adapter);
                    let delegate = Arc::clone(&delegate);
                    let mut cx = cx.clone();
                    async move {
                        let mut scope_uri_to_workspace_config = BTreeMap::new();
                        for item in &params.items {
                            let scope_uri = item.scope_uri.clone();
                            let std_collections_btree_map::Entry::Vacant(new_scope_uri) =
                                scope_uri_to_workspace_config.entry(scope_uri.clone())
                            else {
                                // We've already queried workspace configuration of this URI.
                                continue;
                            };
                            let workspace_config = Self::workspace_configuration_for_adapter(
                                Arc::clone(&adapter),
                                &delegate,
                                scope_uri,
                                &mut cx,
                            )
                            .await?;
                            new_scope_uri.insert(workspace_config);
                        }

                        Ok(params
                            .items
                            .into_iter()
                            .filter_map(|item| {
                                let workspace_config = scope_uri_to_workspace_config.get(&item.scope_uri)?;
                                if let Some(section) = &item.section {
                                    Some(workspace_config.get(section).cloned().unwrap_or(serde_json::Value::Null))
                                } else {
                                    Some(workspace_config.clone())
                                }
                            })
                            .collect())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<WorkspaceFoldersRequest, _, _>({
                let this = lsp_store.clone();
                move |(), cx| {
                    let this = this.clone();
                    let cx = cx.clone();
                    async move {
                        let Some(server) = this.read_with(&cx, |this, _| this.language_server_for_id(server_id))?
                        else {
                            return Ok(None);
                        };
                        let root = server.workspace_folders();
                        Ok(Some(root.into_iter().map(lsp::workspace_folder_for_uri).collect()))
                    }
                }
            })
            .detach();
        // Even though we don't have handling for these requests, respond to them to
        // avoid stalling any language server like `gopls` which waits for a response
        // to these requests when initializing.
        language_server
            .on_request::<WorkDoneProgressCreate, _, _>({
                let this = lsp_store.clone();
                move |params, cx| {
                    let this = this.clone();
                    let mut cx = cx.clone();
                    async move {
                        this.update(&mut cx, |this, _| {
                            if let Some(status) = this.language_server_statuses.get_mut(&server_id) {
                                status.progress_tokens.insert(ProgressToken::from_lsp(params.token));
                            }
                        })?;

                        Ok(())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<RegisterCapability, _, _>({
                let lsp_store = lsp_store.clone();
                move |params, cx| {
                    let lsp_store = lsp_store.clone();
                    let mut cx = cx.clone();
                    async move {
                        lsp_store
                            .update(&mut cx, |lsp_store, cx| {
                                match lsp_store.register_server_capabilities(server_id, params, cx) {
                                    Ok(()) => {}
                                    Err(e) => {
                                        log::error!("Failed to register server capabilities: {e:#}");
                                    }
                                }
                            })
                            .ok();
                        Ok(())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<UnregisterCapability, _, _>({
                let lsp_store = lsp_store.clone();
                move |params, cx| {
                    let lsp_store = lsp_store.clone();
                    let mut cx = cx.clone();
                    async move {
                        lsp_store
                            .update(&mut cx, |lsp_store, cx| {
                                match lsp_store.unregister_server_capabilities(server_id, params, cx) {
                                    Ok(()) => {}
                                    Err(e) => {
                                        log::error!("Failed to unregister server capabilities: {e:#}");
                                    }
                                }
                            })
                            .ok();
                        Ok(())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<ApplyWorkspaceEdit, _, _>({
                let this = lsp_store.clone();
                move |params, cx| {
                    let mut cx = cx.clone();
                    let this = this.clone();
                    async move { LocalLspStore::on_lsp_workspace_edit(this.clone(), params, server_id, &mut cx).await }
                }
            })
            .detach();

        language_server
            .on_request::<InlayHintRefreshRequest, _, _>({
                let lsp_store = lsp_store.clone();
                move |(), cx| {
                    let lsp_store = lsp_store.clone();
                    let mut cx = cx.clone();
                    async move {
                        lsp_store.update(&mut cx, |lsp_store, cx| {
                            lsp_store.refresh_inlay_hints(server_id, cx);
                        })?;
                        Ok(())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<CodeLensRefresh, _, _>({
                let lsp_store = lsp_store.clone();
                move |(), cx| {
                    let result = lsp_store.update(cx, |lsp_store, cx| {
                        lsp_store.refresh_code_lens(Some(server_id), cx);
                    });
                    async move { result }
                }
            })
            .detach();

        language_server
            .on_request::<SemanticTokensRefresh, _, _>({
                let lsp_store = lsp_store.clone();
                move |(), cx| {
                    let lsp_store = lsp_store.clone();
                    let mut cx = cx.clone();
                    async move {
                        lsp_store.update(&mut cx, |lsp_store, cx| {
                            lsp_store.refresh_semantic_tokens(server_id, cx);
                        })?;
                        Ok(())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<WorkspaceDiagnosticRefresh, _, _>({
                let this = lsp_store.clone();
                move |(), cx| {
                    let lsp_store = this.clone();
                    let mut cx = cx.clone();
                    async move {
                        lsp_store.update(&mut cx, |lsp_store, cx| {
                            lsp_store.pull_workspace_diagnostics(server_id);
                            // Respond before pulling: awaiting a round-trip to the same server
                            // here can deadlock servers that bound their request concurrency
                            // and await this response.
                            drop(lsp_store.pull_document_diagnostics_for_server(server_id, None, cx));
                            anyhow::Ok(())
                        })??;
                        Ok(())
                    }
                }
            })
            .detach();

        language_server
            .on_request::<ShowMessageRequest, _, _>({
                let lsp_store = lsp_store.clone();
                let name = name.to_string();
                let adapter = Arc::clone(&adapter);
                move |params, cx| {
                    let lsp_store = lsp_store.clone();
                    let name = name.clone();
                    let adapter = Arc::clone(&adapter);
                    let mut cx = cx.clone();
                    async move {
                        let actions = params.actions.unwrap_or_default();
                        let message = params.message.clone();
                        let (tx, rx) = async_channel::bounded::<MessageActionItem>(1);
                        let level = match params.typ {
                            lsp::MessageType::ERROR => PromptLevel::Critical,
                            lsp::MessageType::WARNING => PromptLevel::Warning,
                            _ => PromptLevel::Info,
                        };
                        let request =
                            LanguageServerPromptRequest::new(level, params.message, actions, name.clone(), tx);

                        lsp_store.update(&mut cx, |_, cx| {
                            cx.emit(LspStoreEvent::LanguageServerPrompt(request));
                        })?;
                        let response = rx.recv().await.ok();
                        if let Some(ref selected_action) = response {
                            let context =
                                language::PromptResponseContext { message, selected_action: selected_action.clone() };
                            adapter.process_prompt_response(&context, &mut cx);
                        }

                        Ok(response)
                    }
                }
            })
            .detach();

        language_server
            .on_request::<ShowDocument, _, _>({
                let lsp_store = lsp_store.clone();
                move |params, cx| {
                    let lsp_store = lsp_store.clone();
                    let mut cx = cx.clone();
                    async move {
                        let (tx, rx) = async_channel::bounded(1);
                        let request = LanguageServerShowDocumentRequest {
                            uri: params.uri,
                            external: params.external.unwrap_or(false),
                            take_focus: params.take_focus.unwrap_or(false),
                            selection: params.selection,
                            response_channel: tx,
                        };
                        lsp_store.update(&mut cx, |_, cx| {
                            cx.emit(LspStoreEvent::LanguageServerShowDocument(request));
                        })?;
                        let success = rx.recv().await.unwrap_or(false);
                        Ok(lsp::ShowDocumentResult { success })
                    }
                }
            })
            .detach();

        language_server
            .on_notification::<ShowMessage, _>({
                let this = lsp_store.clone();
                let name = name.to_string();
                move |params, cx| {
                    let this = this.clone();
                    let name = name.clone();
                    let mut cx = cx.clone();

                    let (tx, _) = async_channel::bounded(1);
                    let level = match params.typ {
                        lsp::MessageType::ERROR => PromptLevel::Critical,
                        lsp::MessageType::WARNING => PromptLevel::Warning,
                        _ => PromptLevel::Info,
                    };
                    let request = LanguageServerPromptRequest::new(level, params.message, vec![], name, tx);

                    this.update(&mut cx, |_, cx| {
                        cx.emit(LspStoreEvent::LanguageServerPrompt(request));
                    })
                    .ok();
                }
            })
            .detach();

        let disk_based_diagnostics_progress_token = adapter.disk_based_diagnostics_progress_token.clone();

        language_server
            .on_notification::<Progress, _>({
                let this = lsp_store.clone();
                move |params, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            this.on_lsp_progress(params, server_id, disk_based_diagnostics_progress_token.clone(), cx);
                        });
                    }
                }
            })
            .detach();

        language_server
            .on_notification::<LogMessage, _>({
                let this = lsp_store.clone();
                move |params, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |_, cx| {
                            cx.emit(LspStoreEvent::LanguageServerLog(
                                server_id,
                                LanguageServerLogType::Log(params.typ),
                                params.message,
                            ));
                        });
                    }
                }
            })
            .detach();

        language_server
            .on_notification::<LogTrace, _>({
                let this = lsp_store.clone();
                move |params, cx| {
                    let mut cx = cx.clone();
                    if let Some(this) = this.upgrade() {
                        this.update(&mut cx, |_, cx| {
                            cx.emit(LspStoreEvent::LanguageServerLog(
                                server_id,
                                LanguageServerLogType::Trace { verbose_info: params.verbose },
                                params.message,
                            ));
                        });
                    }
                }
            })
            .detach();

        rust_analyzer_ext::register_notifications(lsp_store, language_server);
    }

    pub(super) fn shutdown_language_servers_on_quit(&mut self) -> impl Future<Output = ()> + use<> {
        let shutdown_futures = self
            .language_servers
            .drain()
            .map(|(_, server_state)| Self::shutdown_server(server_state))
            .collect::<Vec<_>>();

        async move {
            join_all(shutdown_futures).await;
        }
    }

    async fn shutdown_server(server_state: LanguageServerState) -> anyhow::Result<()> {
        match server_state {
            LanguageServerState::Running { server, .. } => {
                if let Some(shutdown) = server.shutdown() {
                    shutdown.await;
                }
            }
            LanguageServerState::Starting { startup, .. } => {
                if let Some(server) = startup.await
                    && let Some(shutdown) = server.shutdown()
                {
                    shutdown.await;
                }
            }
        }
        Ok(())
    }

    async fn initialization_options_for_adapter(
        adapter: Arc<dyn LspAdapter>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        cx: &mut AsyncApp,
    ) -> Result<Option<serde_json::Value>> {
        let mut initialization_config = Arc::clone(&adapter).initialization_options(delegate, cx).await?;

        for other_adapter in delegate.registered_lsp_adapters() {
            if other_adapter.name() == adapter.name() {
                continue;
            }
            if let Some(target_config) = Arc::clone(&other_adapter)
                .additional_initialization_options(adapter.name(), delegate)
                .await
                .with_context(|| {
                    format!(
                        "getting additional initialization options for {} from {}",
                        adapter.name(),
                        other_adapter.name()
                    )
                })
                .log_err()
                .flatten()
            {
                union_json_value_into(
                    target_config,
                    initialization_config.get_or_insert_with(|| serde_json::json!({})),
                );
            }
        }

        Ok(initialization_config)
    }

    async fn workspace_configuration_for_adapter(
        adapter: Arc<dyn LspAdapter>,
        delegate: &Arc<dyn LspAdapterDelegate>,
        requested_uri: Option<Uri>,
        cx: &mut AsyncApp,
    ) -> Result<serde_json::Value> {
        let mut workspace_config = Arc::clone(&adapter).workspace_configuration(delegate, requested_uri, cx).await?;

        for other_adapter in delegate.registered_lsp_adapters() {
            if other_adapter.name() == adapter.name() {
                continue;
            }
            if let Some(target_config) = Arc::clone(&other_adapter)
                .additional_workspace_configuration(adapter.name(), delegate, cx)
                .await
                .with_context(|| {
                    format!(
                        "getting additional workspace configuration for {} from {}",
                        adapter.name(),
                        other_adapter.name()
                    )
                })
                .log_err()
                .flatten()
            {
                union_json_value_into(target_config, &mut workspace_config);
            }
        }

        Ok(workspace_config)
    }
}

impl LspStore {
    fn refresh_server_tree(&mut self, cx: &mut Context<Self>) {
        let buffer_store = self.buffer_store.clone();
        let local = &mut self.local;
        let mut adapters = BTreeMap::default();
        let get_adapter = {
            let languages = Arc::clone(&local.languages);
            let environment = local.environment.clone();
            let weak = local.weak.clone();
            let worktree_store = local.worktree_store.clone();
            let fs = Arc::clone(&local.fs);
            move |worktree_id, cx: &mut App| {
                let worktree = worktree_store.read(cx).worktree_for_id(worktree_id, cx)?;
                Some(LocalLspAdapterDelegate::new(
                    Arc::clone(&languages),
                    &environment,
                    weak.clone(),
                    &worktree,
                    Arc::clone(&fs),
                    cx,
                ))
            }
        };

        let mut messages_to_report = Vec::new();
        let (new_tree, to_stop) = {
            let mut rebase = local.lsp_tree.rebase();
            let buffers = buffer_store
                .read(cx)
                .buffers()
                .filter_map(|buffer| {
                    let raw_buffer = buffer.read(cx);
                    if !local.registered_buffers.contains_key(&raw_buffer.buffer_id()) {
                        return None;
                    }
                    let file = File::from_dyn(raw_buffer.file()).cloned()?;
                    let language = raw_buffer.language().cloned()?;
                    Some((file, language, raw_buffer.buffer_id()))
                })
                .sorted_by_key(|(file, _, _)| Reverse(file.worktree.read(cx).is_visible()));
            for (file, language, buffer_id) in buffers {
                let worktree_id = file.worktree_id(cx);
                let Some(worktree) = local.worktree_store.read(cx).worktree_for_id(worktree_id, cx) else {
                    continue;
                };

                if let Some((_, apply)) =
                    local.reuse_existing_language_server(rebase.server_tree(), &worktree, &language.name(), cx)
                {
                    (apply)(rebase.server_tree());
                } else if let Some(lsp_delegate) =
                    adapters.entry(worktree_id).or_insert_with(|| get_adapter(worktree_id, cx)).clone()
                {
                    let delegate = Arc::new(ManifestQueryDelegate::new(worktree.read(cx).snapshot()));
                    let path = file.path().parent().map_or_else(|| Arc::clone(file.path()), Arc::from);
                    let worktree_path = ProjectPath { worktree_id, path };
                    let abs_path = file.abs_path(cx);
                    let nodes = rebase
                        .walk(worktree_path, language.name(), language.manifest(), delegate, cx)
                        .collect::<Vec<_>>();
                    for node in nodes {
                        let server_id = node.server_id_or_init(|disposition| {
                            let path = &disposition.path;
                            let workspace_folder = workspace_folder_uri(worktree.read(cx), &path.path);
                            let key = LanguageServerSeed { worktree_id, name: disposition.server_name.clone() };
                            if local.can_start_language_server(&key.name) {
                                local.language_server_ids.remove(&key);
                            }

                            let server_id = local.get_or_insert_language_server(
                                &worktree,
                                Arc::clone(&lsp_delegate),
                                disposition,
                                &language.name(),
                                LanguageServerStartMode::Automatic,
                                cx,
                            )?;
                            if let Some(state) = local.language_servers.get(&server_id)
                                && let Some(workspace_folder) = workspace_folder
                            {
                                state.add_workspace_folder(workspace_folder);
                            }
                            Some(server_id)
                        });

                        if let Some(language_server_id) = server_id {
                            messages_to_report.push(LspStoreEvent::LanguageServerRegisteredForBuffer {
                                language_server_id,
                                name: node.name(),
                                buffer_id,
                                buffer_abs_path: abs_path.clone(),
                            });
                        }
                    }
                }
            }
            rebase.finish()
        };
        for message in messages_to_report {
            cx.emit(message);
        }
        local.lsp_tree = new_tree;
        for (id, _) in to_stop {
            self.stop_local_language_server(id, cx).detach();
        }
    }

    async fn refresh_workspace_configurations(lsp_store: &WeakEntity<Self>, cx: &mut AsyncApp) {
        async move {
            let mut refreshed_servers = FxHashSet::default();
            let servers = lsp_store
                .update(cx, |lsp_store, cx| {
                    let local = &lsp_store.local;

                    let servers = local
                        .language_server_ids
                        .iter()
                        .filter_map(|(seed, state)| {
                            let worktree = lsp_store.worktree_store.read(cx).worktree_for_id(seed.worktree_id, cx);
                            let delegate: Arc<dyn LspAdapterDelegate> = worktree.map(|worktree| {
                                LocalLspAdapterDelegate::new(
                                    Arc::clone(&local.languages),
                                    &local.environment,
                                    cx.weak_entity(),
                                    &worktree,
                                    Arc::clone(&local.fs),
                                    cx,
                                )
                            })?;
                            let server_id = state.id;

                            let states = local.language_servers.get(&server_id)?;

                            match states {
                                LanguageServerState::Starting { .. } => None,
                                LanguageServerState::Running { adapter, server, .. } => {
                                    let adapter = Arc::clone(adapter);
                                    let server = Arc::clone(server);
                                    refreshed_servers.insert(server.name());
                                    Some(cx.spawn(async move |_, cx| {
                                        let settings = LocalLspStore::workspace_configuration_for_adapter(
                                            Arc::clone(&adapter.adapter),
                                            &delegate,
                                            None,
                                            cx,
                                        )
                                        .await
                                        .ok()?;
                                        server
                                            .notify::<DidChangeConfiguration>(lsp::DidChangeConfigurationParams {
                                                settings,
                                            })
                                            .ok()?;
                                        Some(())
                                    }))
                                }
                            }
                        })
                        .collect::<Vec<_>>();

                    Some(servers)
                })
                .ok()
                .flatten()?;

            log::debug!("Refreshing workspace configurations for servers {refreshed_servers:?}");
            // A server may stop while its workspace configuration is being refreshed.
            let _: Vec<Option<()>> = join_all(servers).await;

            Some(())
        }
        .await;
    }

    pub(super) fn maintain_workspace_config(
        mut external_refresh_requests: watch::Receiver<()>,
        cx: &Context<Self>,
    ) -> Task<Result<()>> {
        // Multiple things can happen when workspace settings change:
        // - We might shut down a language server if it's no longer enabled for a given language (and there are no buffers using it otherwise).
        // - We might also shut it down when the workspace configuration of all of the users of a given language server converges onto that of the other.
        // - In the same vein, we might also decide to start a new language server if the workspace configuration *diverges* from the other.
        // - In the easiest case (where we're not wrangling the lifetime of a language server anyhow), if none of the roots of a single language server diverge in their configuration,
        // but it is still different to what we had before, we're gonna send out a workspace configuration update.
        //
        // Settings-store changes reach this loop via `on_settings_changed` -> `request_workspace_config_refresh`,
        // which writes to `external_refresh_requests`. Observing `SettingsStore` here as well would cause every
        // settings change to drive the loop twice and emit duplicate `workspace/didChangeConfiguration` notifications.
        cx.spawn(async move |this, cx| {
            // Refresh once up front, then on every request.
            let mut initial = true;
            while mem::take(&mut initial) || external_refresh_requests.changed().await.is_ok() {
                this.update(cx, |this, cx| {
                    this.refresh_server_tree(cx);
                })
                .ok();

                Self::refresh_workspace_configurations(&this, cx).await;
            }

            anyhow::Ok(())
        })
    }

    fn on_lsp_progress(
        &mut self,
        progress_params: lsp::ProgressParams,
        language_server_id: LanguageServerId,
        disk_based_diagnostics_progress_token: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match progress_params.value {
            lsp::ProgressParamsValue::WorkDone(progress) => {
                self.handle_work_done_progress(
                    progress,
                    language_server_id,
                    disk_based_diagnostics_progress_token,
                    ProgressToken::from_lsp(progress_params.token),
                    cx,
                );
            }
            lsp::ProgressParamsValue::WorkspaceDiagnostic(report) => {
                let lsp::NumberOrString::String(token) = progress_params.token else {
                    return;
                };
                let registration_id = token.split_once(WORKSPACE_DIAGNOSTICS_TOKEN_START).map(|(_, id)| id.to_owned());
                if let Some(LanguageServerState::Running { workspace_diagnostics_refresh_tasks, .. }) =
                    self.local.language_servers.get_mut(&language_server_id)
                    && let Some(workspace_diagnostics) = workspace_diagnostics_refresh_tasks.get_mut(&registration_id)
                    && workspace_diagnostics.current_request_token.as_deref() == Some(token.as_str())
                {
                    workspace_diagnostics.progress_tx.try_send(()).ok();
                    self.apply_workspace_diagnostic_report(
                        language_server_id,
                        report,
                        registration_id.map(SharedString::from),
                        cx,
                    );
                }
            }
        }
    }

    fn handle_work_done_progress(
        &mut self,
        progress: lsp::WorkDoneProgress,
        language_server_id: LanguageServerId,
        disk_based_diagnostics_progress_token: Option<String>,
        token: ProgressToken,
        cx: &mut Context<Self>,
    ) {
        let Some(language_server_status) = self.language_server_statuses.get_mut(&language_server_id) else {
            return;
        };

        if !language_server_status.progress_tokens.contains(&token) {
            return;
        }

        let is_disk_based_diagnostics_progress = if let (Some(disk_based_token), ProgressToken::String(token)) =
            (&disk_based_diagnostics_progress_token, &token)
        {
            token.starts_with(disk_based_token)
        } else {
            false
        };

        match progress {
            lsp::WorkDoneProgress::Begin(report) => {
                if is_disk_based_diagnostics_progress {
                    self.disk_based_diagnostics_started(language_server_id, cx);
                }
                self.on_lsp_work_start(
                    language_server_id,
                    token.clone(),
                    LanguageServerProgress {
                        title: Some(report.title),
                        is_disk_based_diagnostics_progress,
                        is_cancellable: report.cancellable.unwrap_or(false),
                        message: report.message.clone(),
                        percentage: report.percentage.map(|p| p as usize),
                        last_update_at: cx.background_executor().now(),
                    },
                    cx,
                );
            }
            lsp::WorkDoneProgress::Report(report) => self.on_lsp_work_progress(
                language_server_id,
                token,
                LanguageServerProgress {
                    title: None,
                    is_disk_based_diagnostics_progress,
                    is_cancellable: report.cancellable.unwrap_or(false),
                    message: report.message,
                    percentage: report.percentage.map(|p| p as usize),
                    last_update_at: cx.background_executor().now(),
                },
                cx,
            ),
            lsp::WorkDoneProgress::End(_) => {
                language_server_status.progress_tokens.remove(&token);
                self.on_lsp_work_end(language_server_id, token.clone(), cx);
                if is_disk_based_diagnostics_progress {
                    self.disk_based_diagnostics_finished(language_server_id, cx);
                }
            }
        }
    }

    pub(super) fn on_lsp_work_start(
        &mut self,
        language_server_id: LanguageServerId,
        token: ProgressToken,
        progress: LanguageServerProgress,
        cx: &mut Context<Self>,
    ) {
        if let Some(status) = self.language_server_statuses.get_mut(&language_server_id) {
            status.pending_work.insert(token, progress);
            cx.notify();
        }
        cx.emit(LspStoreEvent::PendingWorkUpdated);
    }

    fn on_lsp_work_progress(
        &mut self,
        language_server_id: LanguageServerId,
        token: ProgressToken,
        progress: LanguageServerProgress,
        cx: &mut Context<Self>,
    ) {
        let mut did_update = false;
        if let Some(status) = self.language_server_statuses.get_mut(&language_server_id) {
            match status.pending_work.entry(token) {
                btree_map::Entry::Vacant(entry) => {
                    entry.insert(progress);
                    did_update = true;
                }
                btree_map::Entry::Occupied(mut entry) => {
                    let entry = entry.get_mut();
                    if (progress.last_update_at - entry.last_update_at) >= SERVER_PROGRESS_THROTTLE_TIMEOUT {
                        entry.last_update_at = progress.last_update_at;
                        if progress.message.is_some() {
                            entry.message.clone_from(&progress.message);
                        }
                        if progress.percentage.is_some() {
                            entry.percentage = progress.percentage;
                        }
                        if progress.is_cancellable != entry.is_cancellable {
                            entry.is_cancellable = progress.is_cancellable;
                        }
                        did_update = true;
                    }
                }
            }
        }

        if did_update {
            cx.emit(LspStoreEvent::PendingWorkUpdated);
        }
    }

    pub(super) fn on_lsp_work_end(
        &mut self,
        language_server_id: LanguageServerId,
        token: ProgressToken,
        cx: &mut Context<Self>,
    ) {
        if let Some(status) = self.language_server_statuses.get_mut(&language_server_id) {
            let refresh_inlay_hints =
                status.pending_work.remove(&token).is_some_and(|work| !work.is_disk_based_diagnostics_progress);
            if refresh_inlay_hints {
                self.refresh_inlay_hints_on_work_end(language_server_id, cx);
            }
            cx.notify();
        }
        cx.emit(LspStoreEvent::PendingWorkUpdated);
    }

    async fn shutdown_language_server(
        server_state: Option<LanguageServerState>,
        name: LanguageServerName,
        cx: &AsyncApp,
    ) {
        let server = match server_state {
            Some(LanguageServerState::Starting { startup, .. }) => {
                let mut timer = cx.background_executor().timer(SERVER_LAUNCHING_BEFORE_SHUTDOWN_TIMEOUT).fuse();

                select! {
                    server = startup.fuse() => server,
                    () = timer => {
                        log::info!("timeout waiting for language server {name} to finish launching before stopping");
                        None
                    },
                }
            }

            Some(LanguageServerState::Running { server, .. }) => Some(server),

            None => None,
        };

        let Some(server) = server else { return };
        if let Some(shutdown) = server.shutdown() {
            shutdown.await;
        }
    }

    // Returns a list of all of the worktrees which no longer have a language server and the root path
    // for the stopped server
    fn stop_local_language_server(&mut self, server_id: LanguageServerId, cx: &mut Context<Self>) -> Task<()> {
        let local = &mut self.local;

        // Remove this server ID from all entries in the given worktree.
        local.language_server_ids.retain(|_, state| state.id != server_id);
        self.buffer_store.update(cx, |buffer_store, cx| {
            for buffer in buffer_store.buffers() {
                buffer.update(cx, |buffer, cx| {
                    buffer.update_diagnostics(server_id, DiagnosticSet::new([], buffer), cx);
                    buffer.set_completion_triggers(server_id, BTreeSet::default(), cx);
                });
            }
        });

        let mut cleared_paths: Vec<ProjectPath> = Vec::new();
        for (worktree_id, summaries) in &mut self.diagnostic_summaries {
            summaries.retain(|path, summaries_by_server_id| {
                if summaries_by_server_id.remove(&server_id).is_some() {
                    cleared_paths.push(ProjectPath { worktree_id: *worktree_id, path: Arc::clone(path) });
                    !summaries_by_server_id.is_empty()
                } else {
                    true
                }
            });
        }
        if !cleared_paths.is_empty() {
            cx.emit(LspStoreEvent::DiagnosticsUpdated { server_id, paths: cleared_paths });
        }

        let local = &mut self.local;
        for diagnostics in local.diagnostics.values_mut() {
            diagnostics.retain(|_, diagnostics_by_server_id| {
                if let Ok(ix) = diagnostics_by_server_id.binary_search_by_key(&server_id, |e| e.0) {
                    diagnostics_by_server_id.remove(ix);
                    !diagnostics_by_server_id.is_empty()
                } else {
                    true
                }
            });
        }
        local.language_server_watched_paths.remove(&server_id);
        local.language_server_paths_watched_for_rename.remove(&server_id);
        local.language_server_dynamic_registrations.remove(&server_id);
        local.initial_server_capabilities.remove(&server_id);

        let server_state = local.language_servers.remove(&server_id);
        self.cleanup_lsp_data(server_id);
        let name = self.language_server_statuses.remove(&server_id).map(|status| status.name).or_else(|| {
            if let Some(LanguageServerState::Running { adapter, .. }) = server_state.as_ref() {
                Some(adapter.name())
            } else {
                None
            }
        });

        if let Some(name) = name {
            log::info!("stopping language server {name}");
            {
                let local = &self.local;
                local.update_binary_status(name.clone(), BinaryStatus::Stopping);
            }
            cx.notify();

            return cx.spawn(async move |lsp_store, cx| {
                Self::shutdown_language_server(server_state, name.clone(), cx).await;
                lsp_store
                    .update(cx, |lsp_store, cx| {
                        {
                            let local = &lsp_store.local;
                            local.update_binary_status(name, BinaryStatus::Stopped);
                        }
                        cx.emit(LspStoreEvent::LanguageServerRemoved(server_id));
                        cx.notify();
                    })
                    .ok();
            });
        }

        if server_state.is_some() {
            cx.emit(LspStoreEvent::LanguageServerRemoved(server_id));
        }
        Task::ready(())
    }

    pub fn stop_all_language_servers(&mut self, cx: &mut Context<Self>) {
        {
            let local = &mut self.local;
            local.auto_start_language_servers = false;
            local.language_server_start_overrides.clear();
        }
        self.shutdown_all_language_servers(cx).detach();
    }

    pub fn shutdown_all_language_servers(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let local = &mut self.local;
        local.language_server_restart_tasks.clear();
        let language_servers_to_stop = local.language_server_ids.values().map(|state| state.id).collect();
        local.lsp_tree.remove_nodes(&language_servers_to_stop);
        let tasks = language_servers_to_stop
            .into_iter()
            .map(|server| self.stop_local_language_server(server, cx))
            .collect::<Vec<_>>();
        cx.background_spawn(async move {
            join_all(tasks).await;
        })
    }

    pub fn restart_all_language_servers(&mut self, cx: &mut Context<Self>) {
        {
            let local = &mut self.local;
            local.auto_start_language_servers = true;
            local.language_server_start_overrides.clear();
        }
        let buffers = self.buffer_store.read(cx).buffers().collect();
        self.restart_language_servers_for_buffers(buffers, FxHashSet::default(), cx);
    }

    pub fn restart_language_servers_for_buffers(
        &mut self,
        buffers: Vec<Entity<Buffer>>,
        only_restart_servers: FxHashSet<LanguageServerSelector>,
        cx: &mut Context<Self>,
    ) {
        self.restart_local_language_servers_for_buffers(
            buffers,
            only_restart_servers,
            LanguageServerStartMode::Explicit,
            cx,
        );
    }

    pub fn stop_language_servers_for_buffers(
        &mut self,
        buffers: Vec<Entity<Buffer>>,
        also_stop_servers: FxHashSet<LanguageServerSelector>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let (stopped_names, task) = self.stop_local_language_servers_for_buffers(&buffers, also_stop_servers, cx);
        {
            let local = &mut self.local;
            local.language_server_start_overrides.extend(stopped_names.into_iter().map(|name| (name, false)));
        }
        cx.background_spawn(async move {
            task.await;
            Ok(())
        })
    }

    fn restart_local_language_servers_for_buffers(
        &mut self,
        buffers: Vec<Entity<Buffer>>,
        only_restart_servers: FxHashSet<LanguageServerSelector>,
        start_mode: LanguageServerStartMode,
        cx: &mut Context<Self>,
    ) {
        let local = &mut self.local;
        local.language_server_restart_tasks.retain(|task| !task.is_ready());
        let (_, stop_task) = if only_restart_servers.is_empty() {
            self.stop_local_language_servers_for_buffers(&buffers, FxHashSet::default(), cx)
        } else {
            self.stop_local_language_servers_for_buffers(&[], only_restart_servers.clone(), cx)
        };
        let stop_task = stop_task.shared();
        cx.background_spawn(stop_task.clone()).detach();
        let task = cx.spawn(async move |lsp_store, cx| {
            stop_task.await;
            lsp_store
                .update(cx, |lsp_store, cx| {
                    let local = &mut lsp_store.local;
                    for buffer in buffers {
                        if local.registered_buffers.contains_key(&buffer.read(cx).buffer_id()) {
                            local.register_buffer_with_language_servers(
                                &buffer,
                                only_restart_servers.clone(),
                                start_mode,
                                cx,
                            );
                        }
                    }
                })
                .ok();
        });
        {
            let local = &mut self.local;
            local.language_server_restart_tasks.push(task);
        }
    }

    fn stop_local_language_servers_for_buffers(
        &mut self,
        buffers: &[Entity<Buffer>],
        also_stop_servers: FxHashSet<LanguageServerSelector>,
        cx: &mut Context<Self>,
    ) -> (FxHashSet<LanguageServerName>, Task<()>) {
        let local = &mut self.local;
        let mut language_server_names_to_stop = BTreeSet::default();
        let mut language_servers_to_stop = also_stop_servers
            .into_iter()
            .filter_map(|selector| match selector {
                LanguageServerSelector::Id(id) => Some(id),
                LanguageServerSelector::Name(name) => {
                    language_server_names_to_stop.insert(name);
                    None
                }
            })
            .collect::<BTreeSet<_>>();

        let mut covered_worktrees = FxHashSet::default();
        for buffer in buffers {
            buffer.update(cx, |buffer, cx| {
                language_servers_to_stop.extend(local.language_server_ids_for_buffer(buffer, cx));
                if let Some(worktree_id) = buffer.file().map(|f| f.worktree_id(cx))
                    && covered_worktrees.insert(worktree_id)
                {
                    language_server_names_to_stop.retain(|name| {
                        let old_ids_count = language_servers_to_stop.len();
                        let all_language_servers_with_this_name = local
                            .language_server_ids
                            .iter()
                            .filter_map(|(seed, state)| seed.name.eq(name).then_some(state.id));
                        language_servers_to_stop.extend(all_language_servers_with_this_name);
                        old_ids_count == language_servers_to_stop.len()
                    });
                }
            });
        }
        for name in language_server_names_to_stop {
            language_servers_to_stop
                .extend(local.language_server_ids.iter().filter_map(|(seed, v)| seed.name.eq(&name).then_some(v.id)));
        }

        let stopped_names: FxHashSet<LanguageServerName> = language_servers_to_stop
            .iter()
            .filter_map(|id| {
                local.language_server_ids.iter().find(|(_, state)| state.id == *id).map(|(seed, _)| seed.name.clone())
            })
            .collect();

        local.lsp_tree.remove_nodes(&language_servers_to_stop);
        let tasks = language_servers_to_stop
            .into_iter()
            .map(|server| self.stop_local_language_server(server, cx))
            .collect::<Vec<_>>();

        (stopped_names, cx.background_spawn(join_all(tasks).map(|_| ())))
    }

    fn insert_newly_running_language_server(
        &mut self,
        adapter: Arc<CachedLspAdapter>,
        language_server: Arc<LanguageServer>,
        server_id: LanguageServerId,
        key: LanguageServerSeed,
        language_name: LanguageName,
        workspace_folders: Arc<Mutex<BTreeSet<Uri>>>,
        cx: &mut Context<Self>,
    ) {
        let local = &mut self.local;
        // If the language server for this key doesn't match the server id, don't store the
        // server. Which will cause it to be dropped, killing the process
        if local.language_server_ids.get(&key).is_some_and(|state| state.id != server_id) {
            return;
        }

        // Update language_servers collection with Running variant of LanguageServerState
        // indicating that the server is up and running and ready
        let workspace_folders = workspace_folders.lock().clone();
        language_server.set_workspace_folders(workspace_folders);
        let server_capabilities = language_server.capabilities();
        local.initial_server_capabilities.insert(server_id, server_capabilities.clone());

        let workspace_diagnostics_refresh_tasks = server_capabilities
            .diagnostic_provider
            .clone()
            .and_then(|provider| {
                let workspace_refresher =
                    lsp_workspace_diagnostics_refresh(None, provider, Arc::clone(&language_server), cx)?;

                Some((None, workspace_refresher))
            })
            .into_iter()
            .collect();
        local.language_servers.insert(
            server_id,
            LanguageServerState::Running {
                workspace_diagnostics_refresh_tasks,
                adapter: Arc::clone(&adapter),
                server: Arc::clone(&language_server),
                simulate_disk_based_diagnostics_completion: None,
            },
        );
        local.update_binary_status(adapter.name(), BinaryStatus::None);
        if let Some(file_ops_caps) =
            language_server.capabilities().workspace.as_ref().and_then(|ws| ws.file_operations.as_ref())
        {
            let did_rename_caps = file_ops_caps.did_rename.as_ref();
            let will_rename_caps = file_ops_caps.will_rename.as_ref();
            if did_rename_caps.or(will_rename_caps).is_some() {
                let watcher = RenamePathsWatchedForServer::default()
                    .with_did_rename_patterns(did_rename_caps)
                    .with_will_rename_patterns(will_rename_caps);
                local.language_server_paths_watched_for_rename.insert(server_id, watcher);
            }
        }

        self.language_server_statuses.insert(
            server_id,
            LanguageServerStatus {
                name: language_server.name(),
                language_name: Some(language_name),
                server_version: language_server.version(),
                server_readable_version: language_server.readable_version(),
                pending_work: BTreeMap::default(),
                has_pending_diagnostic_updates: false,
                progress_tokens: FxHashSet::default(),
                worktree: Some(key.worktree_id),
                binary: Some(language_server.binary().clone()),
                configuration: Some(language_server.configuration().clone()),
                workspace_folders: language_server.workspace_folders(),
                process_id: language_server.process_id(),
            },
        );

        cx.emit(LspStoreEvent::LanguageServerAdded(server_id, language_server.name(), Some(key.worktree_id)));

        self.lsp_server_capabilities.insert(server_id, server_capabilities);

        // Tell the language server about every open buffer in the worktree that matches the language.
        // Also check for buffers in worktrees that reused this server
        let mut worktrees_using_server = vec![key.worktree_id];
        {
            let local = &self.local;
            // Find all worktrees that have this server in their language server tree
            for (worktree_id, servers) in &local.lsp_tree.instances {
                if *worktree_id != key.worktree_id {
                    for server_map in servers.roots.values() {
                        if server_map.values().any(|(node, _)| node.id() == Some(server_id)) {
                            worktrees_using_server.push(*worktree_id);
                        }
                    }
                }
            }
        }

        let mut buffer_paths_registered = Vec::new();
        self.buffer_store.clone().update(cx, |buffer_store, cx| {
            let mut lsp_adapters = FxHashMap::default();
            for buffer_handle in buffer_store.buffers() {
                let buffer = buffer_handle.read(cx);
                let Some(file) = File::from_dyn(buffer.file()) else {
                    continue;
                };
                let Some(language) = buffer.language() else {
                    continue;
                };
                if LocalLspStore::language_server_line_length_limit_exceeded(buffer).is_some() {
                    continue;
                }

                if !worktrees_using_server.contains(&file.worktree.read(cx).id())
                    || !lsp_adapters
                        .entry(language.name())
                        .or_insert_with(|| self.languages.lsp_adapters(&language.name()))
                        .iter()
                        .any(|a| a.name == key.name)
                {
                    continue;
                }
                // didOpen
                let local = &mut self.local;

                let buffer_id = buffer.buffer_id();
                if local.registered_buffers.contains_key(&buffer_id) {
                    let abs_path = file.abs_path(cx);
                    let Ok(uri) = lsp::Uri::from_file_path(&abs_path) else {
                        log::error!("failed to convert path to URI: {abs_path:?}");
                        continue;
                    };

                    let versions = local
                        .buffer_snapshots
                        .entry(buffer_id)
                        .or_default()
                        .entry(server_id)
                        .and_modify(|_| panic!("There should not be an existing snapshot for a newly inserted buffer"))
                        .or_insert_with(|| vec![LspBufferSnapshot { version: 0, snapshot: buffer.text_snapshot() }]);

                    let snapshot = versions.last().expect("snapshot versions are never empty");
                    let version = snapshot.version;
                    let initial_snapshot = &snapshot.snapshot;
                    language_server.register_buffer(
                        uri,
                        adapter.language_id(&language.name()),
                        version,
                        initial_snapshot.text_with_line_endings(),
                    );
                    buffer_paths_registered.push((buffer_id, abs_path));
                    local.buffers_opened_in_servers.entry(buffer_id).or_default().insert(server_id);
                }
                buffer_handle.update(cx, |buffer, cx| {
                    buffer.set_completion_triggers(
                        server_id,
                        language_server
                            .capabilities()
                            .completion_provider
                            .as_ref()
                            .and_then(|provider| {
                                provider
                                    .trigger_characters
                                    .as_ref()
                                    .map(|characters| characters.iter().cloned().collect())
                            })
                            .unwrap_or_default(),
                        cx,
                    );
                });
            }
        });

        for (buffer_id, abs_path) in buffer_paths_registered {
            cx.emit(LspStoreEvent::LanguageServerRegisteredForBuffer {
                language_server_id: server_id,
                name: Some(adapter.name()),
                buffer_id,
                buffer_abs_path: abs_path,
            });
        }

        cx.notify();
    }

    pub(crate) fn cancel_language_server_work_for_buffers(
        &self,
        buffers: impl IntoIterator<Item = Entity<Buffer>>,
        cx: &mut Context<Self>,
    ) {
        {
            let local = &self.local;
            let servers = buffers
                .into_iter()
                .flat_map(|buffer| {
                    buffer.update(cx, |buffer, cx| local.language_server_ids_for_buffer(buffer, cx).into_iter())
                })
                .collect::<FxHashSet<_>>();
            for server_id in servers {
                self.cancel_language_server_work(server_id, None, cx);
            }
        }
    }

    pub(crate) fn cancel_language_server_work(
        &self,
        server_id: LanguageServerId,
        token_to_cancel: Option<ProgressToken>,
        _: &Context<Self>,
    ) {
        {
            let local = &self.local;
            let status = self.language_server_statuses.get(&server_id);
            let server = local.language_servers.get(&server_id);
            if let Some((LanguageServerState::Running { server, .. }, status)) = server.zip(status) {
                for (token, progress) in &status.pending_work {
                    if let Some(token_to_cancel) = token_to_cancel.as_ref()
                        && token != token_to_cancel
                    {
                        continue;
                    }
                    if progress.is_cancellable {
                        server
                            .notify::<WorkDoneProgressCancel>(WorkDoneProgressCancelParams { token: token.to_lsp() })
                            .ok();
                    }
                }
            }
        }
    }
}
