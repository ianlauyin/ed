//! LSP store provides unified access to the language server protocol.
//! The consumers of LSP store can interact with language servers without knowing exactly which language server they're interacting with.
//!
//! [`LocalLspStore`] manages the lifecycle of language servers; [`LspStore`] is the interface consumers use.
//! The actual implementation of the LSP protocol is handled by [`lsp`] crate.
use std::fmt;
use std::fmt::Display;

use lsp::notification::DidChangeTextDocument;
use lsp::notification::DidSaveTextDocument;
use lsp::request::Request;
use lsp::request::WorkspaceSymbolRequest;
mod capabilities;
mod code_actions;
pub mod code_lens;
mod completions;
mod diagnostics;
mod document_colors;
mod document_links;
mod document_symbols;
mod dynamic_registration;
mod file_watchers;
mod folding_ranges;
mod inlay_hints;
pub mod log_store;
pub mod lsp_ext_command;
mod on_type_format;
pub mod rust_analyzer_ext;
mod semantic_tokens;
mod server_lifecycle;
mod workspace_edit;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::convert::TryInto as _;
use std::iter;
use std::mem;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use std::time::Instant;
use std::vec;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use capabilities::applicable_lsp_command_capabilities_for_buffer;
use capabilities::completion_trigger_characters_for_buffer;
use capabilities::document_selector_context_for_language;
use capabilities::document_selector_matches;
use capabilities::lsp_command_allowed_for_buffer;
use clock::Global;
pub use completions::CompletionDocumentation;
pub use completions::ensure_uniform_list_compatible_label;
pub use diagnostics::DiagnosticSummary;
pub use diagnostics::DocumentDiagnostics;
pub use diagnostics::DocumentDiagnosticsUpdate;
pub use diagnostics::WorkspaceRefreshTask;
use diagnostics::lsp_workspace_diagnostics_refresh;
pub use document_colors::DocumentColors;
pub use document_links::BufferDocumentLinks;
pub use document_links::DocumentLinkId;
pub use document_links::LspDocumentLink;
pub use document_links::ResolvedDocumentLink;
use file_watchers::LanguageServerWatchedPaths;
use file_watchers::RenamePathsWatchedForServer;
pub use file_watchers::glob_literal_prefix;
pub use folding_ranges::LspFoldingRange;
use fs::Fs;
use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::future::Shared;
use futures::future::join_all;
use futures::stream::FuturesUnordered;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::PromptLevel;
use gpui::SharedString;
use gpui::Task;
use gpui::WeakEntity;
use itertools::Itertools as _;
use language::Buffer;
use language::BufferRow;
use language::CachedLspAdapter;
use language::CodeLabel;
use language::DiagnosticEntry;
use language::DiagnosticSet;
use language::DiagnosticSourceKind;
use language::File as _;
use language::Language;
use language::LanguageName;
use language::LanguageRegistry;
use language::LanguageServerStatusUpdate;
use language::LocalFile as _;
use language::Location;
use language::ManifestDelegate;
use language::ManifestName;
use language::PointUtf16;
use language::TextBufferSnapshot;
use language::Unclipped;
use language::lsp_to_symbol_kind;
use language::point_to_lsp;
use language::range_from_lsp;
use lsp::LSP_REQUEST_TIMEOUT;
use lsp::LanguageServer;
use lsp::LanguageServerBinary;
use lsp::LanguageServerId;
use lsp::LanguageServerName;
use lsp::LanguageServerSelector;
use lsp::LspRequestFuture as _;
use lsp::MessageActionItem;
use lsp::MessageType;
use lsp::OneOf;
use lsp::Uri;
pub use lsp_store::inlay_hints::CacheInlayHints;
pub use lsp_store::inlay_hints::CacheInlayHintsTask;
pub use lsp_store::inlay_hints::InvalidationStrategy;
use parking_lot::Mutex;
use rand::prelude::RngExt as _;
use rand::prelude::StdRng;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
pub use semantic_tokens::BufferSemanticToken;
pub use semantic_tokens::BufferSemanticTokens;
pub use semantic_tokens::SemanticTokenStylizer;
pub use semantic_tokens::TokenType;
use serde::Serialize;
use serde_json::Value;
use server_lifecycle::LanguageServerStartMode;
pub use server_lifecycle::LocalLspAdapterDelegate;
use server_lifecycle::include_text;
use server_lifecycle::subscribe_to_binary_statuses;
use sha2::Digest as _;
use sha2::Sha256;
use snippet::Snippet;
use sum_tree::Dimensions;
use text::Anchor;
use text::BufferId;
use text::OffsetRangeExt as _;
use util::ResultExt as _;
use util::defer;
use util::rel_path::RelPath;
use util::rel_path::RelPathBuf;
use worktree::File;
use worktree::ProjectEntryId;
use worktree::Worktree;
use worktree::WorktreeId;

use self::code_lens::CodeLensData;
use self::document_colors::DocumentColorData;
use self::document_links::DocumentLinksData;
use self::document_symbols::DocumentSymbolsData;
use self::dynamic_registration::DynamicRegistrations;
use self::inlay_hints::BufferInlayHints;
use crate::Hover;
use crate::InlayHint;
use crate::InlayId;
use crate::LocationLink;
use crate::ManifestProvidersStore;
use crate::ProjectPath;
use crate::ProjectTransaction;
use crate::ResolveState;
use crate::Symbol;
use crate::buffer_store::BufferStore;
use crate::buffer_store::BufferStoreEvent;
use crate::environment::ProjectEnvironment;
use crate::lsp_command::GetDeclarations;
use crate::lsp_command::GetDefinitions;
use crate::lsp_command::GetHover;
use crate::lsp_command::GetImplementations;
use crate::lsp_command::GetReferences;
use crate::lsp_command::GetTypeDefinitions;
use crate::lsp_command::InlayHints;
use crate::lsp_command::LinkedEditingRange;
use crate::lsp_command::LspCommand;
use crate::lsp_command::file_path_to_lsp_url;
use crate::lsp_store;
use crate::lsp_store::folding_ranges::FoldingRangeData;
use crate::lsp_store::semantic_tokens::SemanticTokenConfig;
use crate::lsp_store::semantic_tokens::SemanticTokensData;
use crate::manifest_tree::LanguageServerTree;
use crate::manifest_tree::LanguageServerTreeNode;
use crate::manifest_tree::ManifestQueryDelegate;
use crate::manifest_tree::ManifestTree;
use crate::worktree_store::WorktreeStore;
use crate::worktree_store::WorktreeStoreEvent;

const SERVER_LAUNCHING_BEFORE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
pub const SERVER_PROGRESS_THROTTLE_TIMEOUT: Duration = Duration::from_millis(100);
const WORKSPACE_DIAGNOSTICS_TOKEN_START: &str = "id:";
const WORKSPACE_DIAGNOSTICS_REPULL_DELAY: Duration = Duration::from_secs(2);
const CROSS_BUFFER_DIAGNOSTICS_PULL_DELAY: Duration = Duration::from_millis(100);
const DOCUMENT_DIAGNOSTICS_RETRIGGER_LIMIT: usize = 3;
const DOCUMENT_DIAGNOSTICS_RETRIGGER_DELAY: Duration = Duration::from_millis(100);
/// Buffers with a longer line skip language server features entirely.
const MAX_LSP_BUFFER_LINE_LENGTH: u32 = 20_000;
static NEXT_PROMPT_REQUEST_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ProgressToken {
    Number(i32),
    String(SharedString),
}

impl Display for ProgressToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(f, "{number}"),
            Self::String(string) => write!(f, "{string}"),
        }
    }
}

impl ProgressToken {
    fn from_lsp(value: lsp::NumberOrString) -> Self {
        match value {
            lsp::NumberOrString::Number(number) => Self::Number(number),
            lsp::NumberOrString::String(string) => Self::String(SharedString::new(string)),
        }
    }

    fn to_lsp(&self) -> lsp::NumberOrString {
        match self {
            Self::Number(number) => lsp::NumberOrString::Number(*number),
            Self::String(string) => lsp::NumberOrString::String(string.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpenLspBufferHandle(Entity<OpenLspBuffer>);

struct OpenLspBuffer(Entity<Buffer>);

#[derive(Clone)]
struct UnifiedLanguageServer {
    id: LanguageServerId,
    project_roots: FxHashSet<Arc<RelPath>>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct LanguageServerSeed {
    worktree_id: WorktreeId,
    name: LanguageServerName,
}

pub struct LocalLspStore {
    weak: WeakEntity<LspStore>,
    pub worktree_store: Entity<WorktreeStore>,
    environment: Entity<ProjectEnvironment>,
    fs: Arc<dyn Fs>,
    languages: Arc<LanguageRegistry>,
    language_server_ids: FxHashMap<LanguageServerSeed, UnifiedLanguageServer>,
    pub language_servers: FxHashMap<LanguageServerId, LanguageServerState>,
    last_workspace_edits_by_language_server: FxHashMap<LanguageServerId, ProjectTransaction>,
    language_server_watched_paths: FxHashMap<LanguageServerId, LanguageServerWatchedPaths>,
    watched_manifest_filenames: FxHashSet<ManifestName>,
    language_server_paths_watched_for_rename: FxHashMap<LanguageServerId, RenamePathsWatchedForServer>,
    language_server_dynamic_registrations: FxHashMap<LanguageServerId, DynamicRegistrations>,
    initial_server_capabilities: FxHashMap<LanguageServerId, lsp::ServerCapabilities>,
    next_diagnostic_group_id: usize,
    #[allow(clippy::type_complexity)]
    diagnostics: FxHashMap<
        WorktreeId,
        FxHashMap<Arc<RelPath>, Vec<(LanguageServerId, Vec<DiagnosticEntry<Unclipped<PointUtf16>>>)>>,
    >,
    buffer_snapshots: FxHashMap<BufferId, FxHashMap<LanguageServerId, Vec<LspBufferSnapshot>>>, // buffer_id -> server_id -> vec of snapshots
    _subscription: gpui::Subscription,
    _binary_status_task: Task<()>,
    lsp_tree: LanguageServerTree,
    registered_buffers: FxHashMap<BufferId, usize>,
    buffers_opened_in_servers: FxHashMap<BufferId, FxHashSet<LanguageServerId>>,
    #[allow(clippy::type_complexity)]
    buffer_pull_diagnostics_result_ids:
        FxHashMap<LanguageServerId, FxHashMap<Option<SharedString>, FxHashMap<PathBuf, Option<SharedString>>>>,
    #[allow(clippy::type_complexity)]
    workspace_pull_diagnostics_result_ids:
        FxHashMap<LanguageServerId, FxHashMap<Option<SharedString>, FxHashMap<PathBuf, Option<SharedString>>>>,
    auto_start_language_servers: bool,
    language_server_start_overrides: FxHashMap<LanguageServerName, bool>,
    language_server_restart_tasks: Vec<Task<()>>,

    buffers_to_refresh_hash_set: FxHashSet<BufferId>,
    buffers_to_refresh_queue: VecDeque<BufferId>,
    _background_diagnostics_worker: Shared<Task<()>>,
}

fn workspace_folder_uri(worktree: &Worktree, path: &RelPath) -> Option<Uri> {
    if !worktree.entry_for_path(path)?.is_dir() {
        return None;
    }

    let absolute_path = worktree.absolutize(path);
    if let Ok(uri) = Uri::from_file_path(&absolute_path) {
        Some(uri)
    } else {
        log::error!("{absolute_path:?} is not a valid URI");
        None
    }
}

impl LocalLspStore {
    fn language_servers_for_worktree(&self, worktree_id: WorktreeId) -> impl Iterator<Item = &Arc<LanguageServer>> {
        self.language_server_ids.iter().filter_map(move |(seed, state)| {
            if seed.worktree_id != worktree_id {
                return None;
            }

            if let Some(LanguageServerState::Running { server, .. }) = self.language_servers.get(&state.id) {
                Some(server)
            } else {
                None
            }
        })
    }

    fn language_server_ids_for_project_path(
        &self,
        project_path: ProjectPath,
        language: &Language,
        cx: &mut App,
    ) -> Vec<LanguageServerId> {
        let Some(worktree) = self.worktree_store.read(cx).worktree_for_id(project_path.worktree_id, cx) else {
            return Vec::new();
        };
        let delegate: Arc<dyn ManifestDelegate> = Arc::new(ManifestQueryDelegate::new(worktree.read(cx).snapshot()));

        self.lsp_tree.get(project_path, language.name(), language.manifest(), &delegate, cx).collect::<Vec<_>>()
    }

    fn language_server_ids_for_buffer(&self, buffer: &Buffer, cx: &mut App) -> Vec<LanguageServerId> {
        if let Some((file, language)) = File::from_dyn(buffer.file()).zip(buffer.language()) {
            let worktree_id = file.worktree_id(cx);

            let path: Arc<RelPath> = file.path().parent().map_or_else(|| Arc::clone(file.path()), Arc::from);
            let worktree_path = ProjectPath { worktree_id, path };
            self.language_server_ids_for_project_path(worktree_path, language, cx)
        } else {
            Vec::new()
        }
    }

    fn language_servers_for_buffer<'a>(
        &'a self,
        buffer: &'a Buffer,
        cx: &'a mut App,
    ) -> impl Iterator<Item = (&'a Arc<CachedLspAdapter>, &'a Arc<LanguageServer>)> {
        self.language_server_ids_for_buffer(buffer, cx).into_iter().filter_map(|server_id| {
            match self.language_servers.get(&server_id)? {
                LanguageServerState::Running { adapter, server, .. } => Some((adapter, server)),
                LanguageServerState::Starting { .. } => None,
            }
        })
    }

    fn initialize_buffer(&mut self, buffer_handle: &Entity<Buffer>, cx: &mut Context<LspStore>) {
        let buffer = buffer_handle.read(cx);

        let file = buffer.file().cloned();

        let Some(file) = File::from_dyn(file.as_ref()) else {
            return;
        };
        let path = ProjectPath::from_file(file, cx);
        let worktree_id = file.worktree_id(cx);
        let language = buffer.language().cloned();

        if let Some(diagnostics) = self.diagnostics.get(&worktree_id) {
            for (server_id, diagnostics) in diagnostics.get(file.path()).cloned().unwrap_or_default() {
                self.update_buffer_diagnostics(buffer_handle, server_id, None, Vec::new(), diagnostics, cx).log_err();
            }
        }
        let Some(language) = language else {
            return;
        };
        let Some(snapshot) =
            self.worktree_store.read(cx).worktree_for_id(worktree_id, cx).map(|worktree| worktree.read(cx).snapshot())
        else {
            return;
        };
        let delegate: Arc<dyn ManifestDelegate> = Arc::new(ManifestQueryDelegate::new(snapshot));

        for server_id in self.lsp_tree.get(path, language.name(), language.manifest(), &delegate, cx) {
            let Some((server, adapter)) = self.language_servers.get(&server_id).and_then(|server_state| {
                if let LanguageServerState::Running { server, adapter, .. } = server_state {
                    Some((Arc::clone(server), Arc::clone(adapter)))
                } else {
                    None
                }
            }) else {
                continue;
            };

            buffer_handle.update(cx, |buffer, cx| {
                buffer.set_completion_triggers(
                    server.server_id(),
                    completion_trigger_characters_for_buffer(self, server.server_id(), &adapter, buffer),
                    cx,
                );
            });
        }
    }

    pub(crate) fn reset_buffer(&self, buffer: &Entity<Buffer>, old_file: &File, cx: &mut App) {
        buffer.update(cx, |buffer, cx| {
            let Some(language) = buffer.language() else {
                return;
            };
            let path = ProjectPath { worktree_id: old_file.worktree_id(cx), path: Arc::clone(&old_file.path) };
            for server_id in self.language_server_ids_for_project_path(path, language, cx) {
                buffer.update_diagnostics(server_id, DiagnosticSet::new([], buffer), cx);
                buffer.set_completion_triggers(server_id, BTreeSet::default(), cx);
            }
        });
    }

    fn register_language_server_for_invisible_worktree(
        &mut self,
        worktree: &Entity<Worktree>,
        language_server_id: LanguageServerId,
        cx: &App,
    ) {
        let worktree = worktree.read(cx);
        let worktree_id = worktree.id();
        let Some(mut origin_seed) = self
            .language_server_ids
            .iter()
            .find_map(|(seed, state)| (state.id == language_server_id).then(|| seed.clone()))
        else {
            return;
        };
        origin_seed.worktree_id = worktree_id;
        self.language_server_ids
            .entry(origin_seed)
            .or_insert_with(|| UnifiedLanguageServer { id: language_server_id, project_roots: FxHashSet::default() });
    }

    fn language_server_line_length_limit_exceeded(buffer: &Buffer) -> Option<(u32, u32)> {
        let longest_line_length = buffer.text_snapshot().text_summary().longest_row_chars;
        (longest_line_length > MAX_LSP_BUFFER_LINE_LENGTH).then_some((longest_line_length, MAX_LSP_BUFFER_LINE_LENGTH))
    }

    fn register_buffer_with_language_servers(
        &mut self,
        buffer_handle: &Entity<Buffer>,
        only_register_servers: FxHashSet<LanguageServerSelector>,
        start_mode: LanguageServerStartMode,
        cx: &mut Context<LspStore>,
    ) {
        let buffer = buffer_handle.read(cx);
        let buffer_id = buffer.buffer_id();

        let Some(file) = File::from_dyn(buffer.file()) else {
            return;
        };

        let abs_path = file.abs_path(cx);
        if let Some((longest_line_length, maximum_line_length)) =
            Self::language_server_line_length_limit_exceeded(buffer)
        {
            log::debug!(
                "not registering {} with language servers because its longest line has {} characters, exceeding the configured limit of {}",
                abs_path.display(),
                longest_line_length,
                maximum_line_length,
            );
            return;
        }
        let Some(uri) = file_path_to_lsp_url(&abs_path).log_err() else {
            return;
        };
        let initial_snapshot = buffer.text_snapshot();
        let worktree_id = file.worktree_id(cx);

        let Some(language) = buffer.language().cloned() else {
            return;
        };
        let path: Arc<RelPath> = file.path().parent().map_or_else(|| Arc::clone(file.path()), Arc::from);
        let Some(worktree) = self.worktree_store.read(cx).worktree_for_id(worktree_id, cx) else {
            return;
        };
        let language_name = language.name();
        let (delegate, servers) = self
            .reuse_existing_language_server(&self.lsp_tree, &worktree, &language_name, cx)
            .map(|(delegate, apply)| (delegate, apply(&mut self.lsp_tree)))
            .unwrap_or_else(|| {
                let lsp_delegate = LocalLspAdapterDelegate::from_local_lsp(self, &worktree, cx);
                let delegate: Arc<dyn ManifestDelegate> =
                    Arc::new(ManifestQueryDelegate::new(worktree.read(cx).snapshot()));

                let servers = self
                    .lsp_tree
                    .walk(ProjectPath { worktree_id, path }, language.name(), language.manifest(), &delegate, cx)
                    .collect::<Vec<_>>();
                (lsp_delegate, servers)
            });
        let servers_and_adapters = servers
            .into_iter()
            .filter_map(|server_node| {
                if !only_register_servers.is_empty() {
                    if let Some(server_id) = server_node.server_id()
                        && !only_register_servers.contains(&LanguageServerSelector::Id(server_id))
                    {
                        return None;
                    }
                    if let Some(name) = server_node.name()
                        && !only_register_servers.contains(&LanguageServerSelector::Name(name))
                    {
                        return None;
                    }
                }

                let server_id = server_node.server_id_or_init(|disposition| {
                    let path = &disposition.path;

                    {
                        let workspace_folder = workspace_folder_uri(worktree.read(cx), &path.path);

                        let server_id = self.get_or_insert_language_server(
                            &worktree,
                            Arc::clone(&delegate),
                            disposition,
                            &language_name,
                            start_mode,
                            cx,
                        )?;

                        if let Some(state) = self.language_servers.get(&server_id)
                            && let Some(workspace_folder) = workspace_folder
                        {
                            state.add_workspace_folder(workspace_folder);
                        }
                        Some(server_id)
                    }
                })?;
                let server_state = self.language_servers.get(&server_id)?;
                if let LanguageServerState::Running { server, adapter, .. } = server_state {
                    Some((Arc::clone(server), Arc::clone(adapter)))
                } else {
                    // A server that never starts (not installed, failed) is still listed for the buffer.
                    cx.emit(LspStoreEvent::LanguageServerRegisteredForBuffer {
                        language_server_id: server_id,
                        name: server_node.name(),
                        buffer_id,
                        buffer_abs_path: abs_path.clone(),
                    });
                    None
                }
            })
            .collect::<Vec<_>>();
        for (server, adapter) in servers_and_adapters {
            buffer_handle.update(cx, |buffer, cx| {
                buffer.set_completion_triggers(
                    server.server_id(),
                    completion_trigger_characters_for_buffer(self, server.server_id(), &adapter, buffer),
                    cx,
                );
            });

            let snapshot = LspBufferSnapshot { version: 0, snapshot: initial_snapshot.clone() };

            let mut registered = false;
            self.buffer_snapshots.entry(buffer_id).or_default().entry(server.server_id()).or_insert_with(|| {
                registered = true;
                server.register_buffer(
                    uri.clone(),
                    adapter.language_id(&language.name()),
                    0,
                    initial_snapshot.text_with_line_endings(),
                );

                vec![snapshot]
            });

            self.buffers_opened_in_servers.entry(buffer_id).or_default().insert(server.server_id());
            if registered {
                cx.emit(LspStoreEvent::LanguageServerRegisteredForBuffer {
                    language_server_id: server.server_id(),
                    name: None,
                    buffer_id,
                    buffer_abs_path: abs_path.clone(),
                });
            }
        }
    }

    fn reuse_existing_language_server<'lang_name>(
        &self,
        server_tree: &LanguageServerTree,
        worktree: &Entity<Worktree>,
        language_name: &'lang_name LanguageName,
        cx: &mut App,
    ) -> Option<(
        Arc<LocalLspAdapterDelegate>,
        impl FnOnce(&mut LanguageServerTree) -> Vec<LanguageServerTreeNode> + use<'lang_name>,
    )> {
        if worktree.read(cx).is_visible() {
            return None;
        }

        let worktree_store = self.worktree_store.read(cx);
        let servers = server_tree
            .instances
            .iter()
            .filter(|(worktree_id, _)| {
                worktree_store.worktree_for_id(**worktree_id, cx).is_some_and(|worktree| worktree.read(cx).is_visible())
            })
            .flat_map(|(worktree_id, servers)| {
                servers
                    .roots
                    .values()
                    .flatten()
                    .map(move |(_, (server_node, server_languages))| (worktree_id, server_node, server_languages))
                    .filter(|(_, node, server_languages)| {
                        server_languages.contains(language_name)
                            && node.id().is_some_and(|id| self.language_servers.contains_key(&id))
                    })
                    .map(|(worktree_id, server_node, _)| {
                        (*worktree_id, LanguageServerTreeNode::from(Arc::downgrade(server_node)))
                    })
            })
            .fold(FxHashMap::default(), |mut acc, (worktree_id, server_node)| {
                acc.entry(worktree_id).or_insert_with(Vec::new).push(server_node);
                acc
            })
            .into_values()
            .max_by_key(Vec::len)?;

        let worktree_id = worktree.read(cx).id();
        let apply = move |tree: &mut LanguageServerTree| {
            for server_node in &servers {
                tree.register_reused(worktree_id, language_name.clone(), server_node.clone());
            }
            servers
        };

        let delegate = LocalLspAdapterDelegate::from_local_lsp(self, worktree, cx);
        Some((delegate, apply))
    }

    pub(crate) fn unregister_old_buffer_from_language_servers(
        &mut self,
        buffer: &Entity<Buffer>,
        old_file: &File,
        cx: &mut App,
    ) -> Vec<LanguageServerId> {
        let old_path = old_file.abs_path(cx);

        let Ok(file_url) = lsp::Uri::from_file_path(old_path.as_path()) else {
            return Vec::new();
        };
        self.unregister_buffer_from_language_servers(buffer, &file_url, cx)
    }

    pub(crate) fn unregister_buffer_from_language_servers(
        &mut self,
        buffer: &Entity<Buffer>,
        file_url: &lsp::Uri,
        cx: &mut App,
    ) -> Vec<LanguageServerId> {
        let abs_path = file_url.to_file_path().ok();
        buffer.update(cx, |buffer, cx| {
            let buffer_id = buffer.buffer_id();
            let mut snapshots = self.buffer_snapshots.remove(&buffer_id);

            let mut detached_servers = Vec::new();
            for (_, language_server) in self.language_servers_for_buffer(buffer, cx) {
                let server_id = language_server.server_id();
                if snapshots.as_mut().is_some_and(|map| map.remove(&server_id).is_some()) {
                    language_server.unregister_buffer(file_url.clone());
                    detached_servers.push(server_id);
                }
            }

            for &server_id in &detached_servers {
                buffer.set_completion_triggers(server_id, BTreeSet::default(), cx);
                if let Some(opened_in_servers) = self.buffers_opened_in_servers.get_mut(&buffer_id) {
                    opened_in_servers.remove(&server_id);
                }
                if let Some(abs_path) = &abs_path
                    && let Some(result_ids) = self.buffer_pull_diagnostics_result_ids.get_mut(&server_id)
                {
                    for result_ids in result_ids.values_mut() {
                        result_ids.remove(abs_path);
                    }
                }
            }

            detached_servers
        })
    }

    fn buffer_snapshot_for_lsp_version(
        &mut self,
        buffer: &Entity<Buffer>,
        server_id: LanguageServerId,
        version: Option<i32>,
        cx: &App,
    ) -> Result<TextBufferSnapshot> {
        const OLD_VERSIONS_TO_RETAIN: i32 = 10;

        if let Some(version) = version {
            let buffer_id = buffer.read(cx).buffer_id();
            let Some(snapshots) = self.buffer_snapshots.get_mut(&buffer_id).and_then(|m| m.get_mut(&server_id)) else {
                // Some language servers report version 0 even if the buffer hasn't been opened yet.
                // We detect this case and treat it as if the version was `None`.
                if version == 0 {
                    return Ok(buffer.read(cx).text_snapshot());
                }
                anyhow::bail!("no snapshots found for buffer {buffer_id} and server {server_id}");
            };

            let found_snapshot = snapshots
                .binary_search_by_key(&version, |e| e.version)
                .map(|ix| snapshots[ix].snapshot.clone())
                .ok()
                .with_context(|| {
                    format!("snapshot not found for buffer {buffer_id} server {server_id} at version {version}")
                })?;

            snapshots.retain(|snapshot| snapshot.version + OLD_VERSIONS_TO_RETAIN >= version);
            Ok(found_snapshot)
        } else {
            Ok((buffer.read(cx)).text_snapshot())
        }
    }

    fn remove_worktree(&mut self, id_to_remove: WorktreeId, cx: &mut Context<LspStore>) -> Vec<LanguageServerId> {
        self.diagnostics.remove(&id_to_remove);

        let mut servers_to_remove = BTreeSet::default();
        let mut servers_to_preserve = FxHashSet::default();
        for (seed, state) in &self.language_server_ids {
            if seed.worktree_id == id_to_remove {
                servers_to_remove.insert(state.id);
            } else {
                servers_to_preserve.insert(state.id);
            }
        }
        servers_to_remove.retain(|server_id| !servers_to_preserve.contains(server_id));
        self.language_server_ids
            .retain(|seed, state| seed.worktree_id != id_to_remove && !servers_to_remove.contains(&state.id));
        self.lsp_tree.instances.remove(&id_to_remove);
        for server_id_to_remove in &servers_to_remove {
            self.language_server_watched_paths.remove(server_id_to_remove);
            self.language_server_paths_watched_for_rename.remove(server_id_to_remove);
            self.last_workspace_edits_by_language_server.remove(server_id_to_remove);
            self.language_servers.remove(server_id_to_remove);
            self.language_server_dynamic_registrations.remove(server_id_to_remove);
            self.initial_server_capabilities.remove(server_id_to_remove);
            self.buffer_pull_diagnostics_result_ids.remove(server_id_to_remove);
            self.workspace_pull_diagnostics_result_ids.remove(server_id_to_remove);
            for buffer_servers in self.buffers_opened_in_servers.values_mut() {
                buffer_servers.remove(server_id_to_remove);
            }
            cx.emit(LspStoreEvent::LanguageServerRemoved(*server_id_to_remove));
        }
        servers_to_remove.into_iter().collect()
    }

    fn language_server_for_id(&self, id: LanguageServerId) -> Option<Arc<LanguageServer>> {
        if let Some(LanguageServerState::Running { server, .. }) = self.language_servers.get(&id) {
            Some(Arc::clone(server))
        } else {
            None
        }
    }
}

/// Prunes per-server cached LSP data of servers no longer relevant for the buffer, and
/// returns the relevant servers that were not fetched for yet.
/// `None` means the cache covers all relevant servers and can be returned as is.
///
/// `fetched_servers` tracks the servers a completed fetch has queried, not the ones that
/// answered: a server that returned no data (not capable or errored) would otherwise be
/// considered perpetually missing and every call would re-fetch.
///
/// An empty `current_servers` set does not prune: data of removed servers is cleaned up
/// via the `remove_server_data` methods.
fn missing_servers_to_query<T>(
    cached: &mut FxHashMap<LanguageServerId, T>,
    fetched_servers: &mut FxHashSet<LanguageServerId>,
    current_servers: &FxHashSet<LanguageServerId>,
) -> Option<FxHashSet<LanguageServerId>> {
    if current_servers.is_empty() {
        return None;
    }
    cached.retain(|server_id, _| current_servers.contains(server_id));
    fetched_servers.retain(|server_id| current_servers.contains(server_id));
    let missing_servers = current_servers.difference(fetched_servers).copied().collect::<FxHashSet<_>>();
    if missing_servers.is_empty() { None } else { Some(missing_servers) }
}

fn next_lsp_fetch_id() -> u64 {
    static NEXT_LSP_FETCH_ID: AtomicUsize = AtomicUsize::new(1);
    NEXT_LSP_FETCH_ID.fetch_add(1, atomic::Ordering::Relaxed) as u64
}

#[derive(Debug)]
struct RunningFetch<T> {
    id: u64,
    version: Global,
    servers: FxHashSet<LanguageServerId>,
    task: T,
}

impl<T> RunningFetch<T> {
    /// Unregisters the completed fetch and reports whether its results are still
    /// relevant: an eviction, a buffer version reset, or a superseding fetch (whose
    /// server set covers this one's) all clear or replace the registration, in which
    /// case the results must be discarded.
    fn take_finished(update: &mut Option<Self>, fetch_id: u64) -> bool {
        if update.as_ref().is_some_and(|running| running.id == fetch_id) {
            *update = None;
            true
        } else {
            false
        }
    }

    /// Unregisters a fetch that has queried the given server, so that, when the server
    /// is removed, a still-running fetch cannot re-insert the removed server's data
    /// after its cache entries were cleaned up.
    fn discard_if_queried(update: &mut Option<Self>, server_id: LanguageServerId) {
        if update.as_ref().is_some_and(|running| running.servers.contains(&server_id)) {
            *update = None;
        }
    }
}

enum ChunkFetch {
    Cached(CacheInlayHints),
    Running(CacheInlayHintsTask),
}

pub struct LspStore {
    local: LocalLspStore,
    nonce: u128,
    buffer_store: Entity<BufferStore>,
    worktree_store: Entity<WorktreeStore>,
    pub languages: Arc<LanguageRegistry>,
    pub language_server_statuses: BTreeMap<LanguageServerId, LanguageServerStatus>,
    active_entry: Option<ProjectEntryId>,
    _maintain_workspace_config: (Task<Result<()>>, watch::Sender<()>),
    _maintain_buffer_languages: Task<()>,
    diagnostic_summaries:
        FxHashMap<WorktreeId, FxHashMap<Arc<RelPath>, FxHashMap<LanguageServerId, DiagnosticSummary>>>,
    pub lsp_server_capabilities: FxHashMap<LanguageServerId, lsp::ServerCapabilities>,
    semantic_token_config: SemanticTokenConfig,
    lsp_data: FxHashMap<BufferId, BufferLspData>,
    buffer_reload_tasks: FxHashMap<BufferId, Task<anyhow::Result<()>>>,
    next_hint_id: Arc<AtomicUsize>,
}

#[derive(Debug)]
pub struct BufferLspData {
    buffer_version: Global,
    document_colors: Option<DocumentColorData>,
    code_lens: Option<CodeLensData>,
    semantic_tokens: Option<SemanticTokensData>,
    folding_ranges: Option<FoldingRangeData>,
    document_links: Option<DocumentLinksData>,
    document_symbols: Option<DocumentSymbolsData>,
    inlay_hints: BufferInlayHints,
}

impl BufferLspData {
    fn new(buffer: &Entity<Buffer>, cx: &App) -> Self {
        Self {
            buffer_version: buffer.read(cx).version(),
            document_colors: None,
            code_lens: None,
            semantic_tokens: None,
            folding_ranges: None,
            document_links: None,
            document_symbols: None,
            inlay_hints: BufferInlayHints::new(buffer, cx),
        }
    }

    fn remove_server_data(&mut self, for_server: LanguageServerId) {
        if let Some(document_colors) = &mut self.document_colors {
            document_colors.remove_server_data(for_server);
        }

        if let Some(code_lens) = &mut self.code_lens {
            code_lens.remove_server_data(for_server);
        }

        self.inlay_hints.remove_server_data(for_server);

        if let Some(semantic_tokens) = &mut self.semantic_tokens {
            semantic_tokens.remove_server_data(for_server);
        }

        if let Some(folding_ranges) = &mut self.folding_ranges {
            folding_ranges.remove_server_data(for_server);
        }

        if let Some(document_links) = &mut self.document_links {
            document_links.remove_server_data(for_server);
        }

        if let Some(document_symbols) = &mut self.document_symbols {
            document_symbols.remove_server_data(for_server);
        }
    }

    fn reset_for_newer_buffer_version(&mut self, buffer: &Entity<Buffer>, cx: &App) {
        // To send delta requests for semantic tokens, the previous tokens
        // need to be kept between buffer changes.
        let semantic_tokens = self.semantic_tokens.take();
        *self = Self::new(buffer, cx);
        self.semantic_tokens = semantic_tokens;
    }
}

#[derive(Debug)]
pub enum LspStoreEvent {
    LanguageServerAdded(LanguageServerId, LanguageServerName, Option<WorktreeId>),
    LanguageServerRemoved(LanguageServerId),
    LanguageServerStatus {
        language_server_id: LanguageServerId,
        name: Option<LanguageServerName>,
        status: LanguageServerStatusUpdate,
    },
    /// The pending work of some language server changed.
    PendingWorkUpdated,
    LanguageServerRegisteredForBuffer {
        language_server_id: LanguageServerId,
        name: Option<LanguageServerName>,
        buffer_id: BufferId,
        buffer_abs_path: PathBuf,
    },
    LanguageServerLog(LanguageServerId, LanguageServerLogType, String),
    LanguageServerPrompt(LanguageServerPromptRequest),
    LanguageDetected {
        buffer: Entity<Buffer>,
        new_language: Option<Arc<Language>>,
    },
    Notification(String),
    LanguageServerShowDocument(LanguageServerShowDocumentRequest),
    RefreshInlayHints {
        server_id: LanguageServerId,
    },
    RefreshSemanticTokens {
        server_id: LanguageServerId,
    },
    RefreshCodeLens {
        server_id: Option<LanguageServerId>,
    },
    RefreshDocumentColors {
        server_id: Option<LanguageServerId>,
    },
    RefreshDocumentLinks {
        server_id: Option<LanguageServerId>,
    },
    RefreshDocumentHighlights {
        server_id: Option<LanguageServerId>,
    },
    RefreshFoldingRanges {
        server_id: Option<LanguageServerId>,
    },
    RefreshDocumentSymbols {
        server_id: Option<LanguageServerId>,
    },
    DiagnosticsUpdated {
        server_id: LanguageServerId,
        paths: Vec<ProjectPath>,
    },
    DiskBasedDiagnosticsStarted {
        language_server_id: LanguageServerId,
    },
    DiskBasedDiagnosticsFinished {
        language_server_id: LanguageServerId,
    },
    SnippetEdit {
        buffer_id: BufferId,
        edits: Vec<(lsp::Range, Snippet)>,
    },
    WorkspaceEditApplied(ProjectTransaction),
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageServerStatus {
    pub name: LanguageServerName,
    pub language_name: Option<LanguageName>,
    pub server_version: Option<SharedString>,
    pub server_readable_version: Option<SharedString>,
    pub pending_work: BTreeMap<ProgressToken, LanguageServerProgress>,
    pub has_pending_diagnostic_updates: bool,
    pub progress_tokens: FxHashSet<ProgressToken>,
    pub worktree: Option<WorktreeId>,
    pub binary: Option<LanguageServerBinary>,
    pub configuration: Option<Value>,
    pub workspace_folders: BTreeSet<Uri>,
    pub process_id: Option<u32>,
}

#[derive(Clone, Debug)]
struct CoreSymbol {
    pub language_server_name: LanguageServerName,
    pub source_worktree_id: WorktreeId,
    pub source_language_server_id: LanguageServerId,
    pub path: SymbolLocation,
    pub name: String,
    pub kind: language::SymbolKind,
    pub range: Range<Unclipped<PointUtf16>>,
    pub container_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolLocation {
    InProject(ProjectPath),
    OutsideProject { abs_path: Arc<Path>, signature: [u8; 32] },
}

impl SymbolLocation {
    fn file_name(&self) -> Option<&str> {
        match self {
            Self::InProject(path) => path.path.file_name(),
            Self::OutsideProject { abs_path, .. } => abs_path.file_name()?.to_str(),
        }
    }
}

fn should_log_lsp_request_failure(message: &str) -> bool {
    // "content modified" and "server cancelled the request" are noisy failure
    // modes of rust-analyzer where requests are denied before it has loaded a
    // project.
    !(message.ends_with("content modified") || message.ends_with("server cancelled the request"))
}

fn should_log_lsp_response_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<lsp::ResponseError>().is_none_or(|response_error| !response_error.is_request_denied())
}

fn lsp_pull_cancelled_with_retrigger(error: &anyhow::Error) -> bool {
    error.downcast_ref::<lsp::ResponseError>().is_some_and(lsp::ResponseError::should_retrigger)
}

impl LspStore {
    // todo: always `Some`; inline `self.local` at call sites
    pub fn new_local(
        buffer_store: Entity<BufferStore>,
        worktree_store: Entity<WorktreeStore>,
        environment: Entity<ProjectEnvironment>,
        manifest_tree: Entity<ManifestTree>,
        languages: Arc<LanguageRegistry>,
        fs: Arc<dyn Fs>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&buffer_store, Self::on_buffer_store_event).detach();
        cx.subscribe(&worktree_store, Self::on_worktree_store_event).detach();
        let binary_status_task = subscribe_to_binary_statuses(&languages, cx);

        let _maintain_workspace_config = {
            let (sender, receiver) = watch::channel(());
            (Self::maintain_workspace_config(receiver, cx), sender)
        };

        Self {
            local: LocalLspStore {
                weak: cx.weak_entity(),
                worktree_store: worktree_store.clone(),

                languages: Arc::clone(&languages),
                language_server_ids: FxHashMap::default(),
                language_servers: FxHashMap::default(),
                last_workspace_edits_by_language_server: FxHashMap::default(),
                language_server_watched_paths: FxHashMap::default(),
                language_server_paths_watched_for_rename: FxHashMap::default(),
                language_server_dynamic_registrations: FxHashMap::default(),
                initial_server_capabilities: FxHashMap::default(),
                buffers_to_refresh_hash_set: FxHashSet::default(),
                buffers_to_refresh_queue: VecDeque::new(),
                _background_diagnostics_worker: Task::ready(()).shared(),
                buffer_snapshots: FxHashMap::default(),
                environment,
                fs,
                next_diagnostic_group_id: Default::default(),
                diagnostics: FxHashMap::default(),
                _subscription: cx.on_app_quit(|this, _| this.local.shutdown_language_servers_on_quit()),
                _binary_status_task: binary_status_task,
                lsp_tree: LanguageServerTree::new(manifest_tree, Arc::clone(&languages)),
                registered_buffers: FxHashMap::default(),
                buffers_opened_in_servers: FxHashMap::default(),
                buffer_pull_diagnostics_result_ids: FxHashMap::default(),
                workspace_pull_diagnostics_result_ids: FxHashMap::default(),
                auto_start_language_servers: true,
                language_server_start_overrides: FxHashMap::default(),
                language_server_restart_tasks: Vec::new(),
                watched_manifest_filenames: ManifestProvidersStore::global(cx).manifest_file_names(),
            },
            buffer_store,
            worktree_store,
            languages: Arc::clone(&languages),
            language_server_statuses: BTreeMap::default(),
            nonce: rand::make_rng::<StdRng>().random(),
            diagnostic_summaries: FxHashMap::default(),
            lsp_server_capabilities: FxHashMap::default(),
            semantic_token_config: SemanticTokenConfig::new(),
            lsp_data: FxHashMap::default(),
            buffer_reload_tasks: FxHashMap::default(),
            next_hint_id: Arc::default(),
            active_entry: None,
            _maintain_workspace_config,
            _maintain_buffer_languages: Self::maintain_buffer_languages(languages, cx),
        }
    }

    pub fn cached_inlay_hint(&self, buffer_id: BufferId, id: InlayId) -> Option<&InlayHint> {
        self.lsp_data.get(&buffer_id)?.inlay_hints.cached_hint(id)
    }

    fn on_buffer_store_event(&mut self, _: Entity<BufferStore>, event: &BufferStoreEvent, cx: &mut Context<Self>) {
        match event {
            BufferStoreEvent::BufferAdded(buffer) => {
                self.on_buffer_added(buffer, cx);
            }
            BufferStoreEvent::BufferChangedFilePath { buffer, old_file } => {
                let buffer_id = buffer.read(cx).buffer_id();
                let local = &mut self.local;
                if let Some(old_file) = File::from_dyn(old_file.as_ref()) {
                    local.reset_buffer(buffer, old_file, cx);

                    if local.registered_buffers.contains_key(&buffer_id) {
                        local.unregister_old_buffer_from_language_servers(buffer, old_file, cx);
                    }
                }

                self.detect_language_for_buffer(buffer, cx);
                {
                    let local = &mut self.local;
                    local.initialize_buffer(buffer, cx);
                    if local.registered_buffers.contains_key(&buffer_id) {
                        local.register_buffer_with_language_servers(
                            buffer,
                            FxHashSet::default(),
                            LanguageServerStartMode::Automatic,
                            cx,
                        );
                    }
                }
            }
            BufferStoreEvent::BufferDropped(_) => {}
        }
    }

    fn on_worktree_store_event(
        &mut self,
        _: Entity<WorktreeStore>,
        event: &WorktreeStoreEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            WorktreeStoreEvent::WorktreeAdded(worktree) => cx
                .subscribe(worktree, |this, worktree, event, cx| match event {
                    worktree::Event::UpdatedEntries(changes) => {
                        this.update_local_worktree_language_servers(&worktree, changes, cx);
                    }
                    worktree::Event::UpdatedGitRepositories(_)
                    | worktree::Event::DeletedEntry(_)
                    | worktree::Event::Deleted => {}
                })
                .detach(),
            WorktreeStoreEvent::WorktreeRemoved(_, id) => self.remove_worktree(*id, cx),
            WorktreeStoreEvent::WorktreeUpdatedEntries(worktree_id, changes) => {
                self.invalidate_diagnostic_summaries_for_removed_entries(*worktree_id, changes, cx);
            }
            _ => {}
        }
    }

    fn request_workspace_config_refresh(&mut self) {
        self._maintain_workspace_config.1.send(()).ok();
    }

    fn on_buffer_event(&mut self, buffer: Entity<Buffer>, event: &language::BufferEvent, cx: &mut Context<Self>) {
        match event {
            language::BufferEvent::Edited => {
                self.on_buffer_edited(buffer, cx);
            }

            language::BufferEvent::LineEndingChanged => {
                self.on_buffer_edited(buffer, cx);
            }

            language::BufferEvent::Saved => {
                self.on_buffer_saved(buffer, cx);
            }

            language::BufferEvent::Reloaded => {
                self.on_buffer_reloaded(buffer, cx);
            }

            _ => {}
        }
    }

    fn on_buffer_added(&mut self, buffer: &Entity<Buffer>, cx: &mut Context<Self>) {
        buffer.read(cx).set_language_registry(Arc::clone(&self.languages));

        cx.subscribe(buffer, |this, buffer, event, cx| {
            this.on_buffer_event(buffer, event, cx);
        })
        .detach();

        self.detect_language_for_buffer(buffer, cx);
        {
            let local = &mut self.local;
            local.initialize_buffer(buffer, cx);
        }
    }

    fn on_buffer_reloaded(&mut self, buffer: Entity<Buffer>, cx: &mut Context<Self>) {
        let buffer_id = buffer.read(cx).buffer_id();
        let task = self.pull_diagnostics_for_buffer(buffer, cx);
        self.buffer_reload_tasks.insert(buffer_id, task);
    }

    pub(crate) fn register_buffer_with_language_servers(
        &mut self,
        buffer: &Entity<Buffer>,
        only_register_servers: FxHashSet<LanguageServerSelector>,
        cx: &mut Context<Self>,
    ) -> OpenLspBufferHandle {
        let buffer_id = buffer.read(cx).buffer_id();
        let handle = OpenLspBufferHandle(cx.new(|_| OpenLspBuffer(buffer.clone())));
        {
            let local = &mut self.local;
            let refcount = local.registered_buffers.entry(buffer_id).or_insert(0);
            *refcount += 1;

            // We run early exits on non-existing buffers AFTER we mark the buffer as registered in order to handle buffer saving.
            // When a new unnamed buffer is created and saved, we will start loading it's language. Once the language is loaded, we go over all "language-less" buffers and try to fit that new language
            // with them. However, we do that only for the buffers that we think are open in at least one editor; thus, we need to keep tab of unnamed buffers as well, even though they're not actually registered with any language
            // servers in practice (we don't support non-file URI schemes in our LSP impl).
            if File::from_dyn(buffer.read(cx).file()).is_none() {
                return handle;
            }

            if *refcount == 1 {
                local.register_buffer_with_language_servers(
                    buffer,
                    only_register_servers,
                    LanguageServerStartMode::Automatic,
                    cx,
                );
            }
            cx.observe_release(&handle.0, move |lsp_store, buffer, cx| {
                let refcount = {
                    let local = &mut lsp_store.local;
                    let Some(refcount) = local.registered_buffers.get_mut(&buffer_id) else {
                        log::error!("bad refcounting");
                        return;
                    };

                    *refcount -= 1;
                    *refcount
                };
                if refcount == 0 {
                    lsp_store.lsp_data.remove(&buffer_id);
                    lsp_store.buffer_reload_tasks.remove(&buffer_id);
                    let local = &mut lsp_store.local;
                    local.registered_buffers.remove(&buffer_id);

                    local.buffers_opened_in_servers.remove(&buffer_id);
                    if let Some(file) = File::from_dyn(buffer.0.read(cx).file()).cloned() {
                        local.unregister_old_buffer_from_language_servers(&buffer.0, &file, cx);

                        let buffer_abs_path = file.abs_path(cx);
                        for buffer_pull_diagnostics_result_ids in local.buffer_pull_diagnostics_result_ids.values_mut()
                        {
                            buffer_pull_diagnostics_result_ids.retain(|_, buffer_result_ids| {
                                buffer_result_ids.remove(&buffer_abs_path);
                                !buffer_result_ids.is_empty()
                            });
                        }

                        // Closing the buffer does not change which servers are relevant
                        // to its path, so pushed diagnostics stay: a running server may
                        // still describe the file. Pulled document diagnostics need an
                        // open document to be refreshed, so they are dropped, except for
                        // servers whose workspace pull still owns the path's verdict.
                        let servers_to_clear = local
                            .language_servers
                            .keys()
                            .copied()
                            .filter(|server_id| !local.server_pulls_workspace_diagnostics(*server_id))
                            .collect::<Vec<_>>();
                        lsp_store.clear_path_diagnostics_for_servers(
                            buffer_abs_path,
                            servers_to_clear,
                            |_, diagnostic, _| diagnostic.source_kind != DiagnosticSourceKind::Pulled,
                            cx,
                        );
                    }
                }
            })
            .detach();
        }
        handle
    }

    fn maintain_buffer_languages(languages: Arc<LanguageRegistry>, cx: &Context<Self>) -> Task<()> {
        let mut subscription = languages.subscribe();
        let mut prev_reload_count = languages.reload_count();
        cx.spawn(async move |this, cx| {
            // Run once up front, then on every registry change.
            let mut initial = true;
            while mem::take(&mut initial) || subscription.changed().await.is_ok() {
                if let Some(this) = this.upgrade() {
                    // If the language registry has been reloaded, then remove and
                    // re-assign the languages on all open buffers.
                    let reload_count = languages.reload_count();
                    if reload_count > prev_reload_count {
                        prev_reload_count = reload_count;
                        this.update(cx, |this, cx| {
                            this.buffer_store.clone().update(cx, |buffer_store, cx| {
                                for buffer in buffer_store.buffers() {
                                    if let Some(f) = File::from_dyn(buffer.read(cx).file()).cloned() {
                                        // Detach before the language is unassigned: the
                                        // detachment resolves the buffer's servers through
                                        // its current language, and with the language gone
                                        // it skips didClose and leaks the per-server state
                                        // while still dropping the snapshots.
                                        let local = &mut this.local;
                                        if local.registered_buffers.contains_key(&buffer.read(cx).buffer_id())
                                            && let Some(file_url) = file_path_to_lsp_url(&f.abs_path(cx)).log_err()
                                        {
                                            local.unregister_buffer_from_language_servers(&buffer, &file_url, cx);
                                        }
                                        buffer.update(cx, |buffer, cx| buffer.set_language_async(None, cx));
                                    }
                                }
                            });
                        });
                    }

                    this.update(cx, |this, cx| {
                        let mut buffers_to_detect = Vec::new();
                        let mut buffers_with_language = Vec::new();
                        let mut buffers_with_unknown_injections = Vec::new();
                        for handle in this.buffer_store.read(cx).buffers() {
                            let buffer = handle.read(cx);
                            if buffer.language().is_none() || buffer.language() == Some(&*language::PLAIN_TEXT) {
                                buffers_to_detect.push(handle);
                            } else {
                                if buffer.contains_unknown_injections() {
                                    buffers_with_unknown_injections.push(handle.clone());
                                }
                                buffers_with_language.push(handle);
                            }
                        }

                        // Deprioritize the invisible worktrees so main worktrees' language servers can be started first,
                        // and reused later in the invisible worktrees.
                        buffers_to_detect.sort_by_key(|buffer| {
                            Reverse(
                                File::from_dyn(buffer.read(cx).file()).map(|file| file.worktree.read(cx).is_visible()),
                            )
                        });

                        for buffer in buffers_to_detect {
                            this.detect_language_for_buffer(&buffer, cx);
                            {
                                let local = &mut this.local;
                                local.initialize_buffer(&buffer, cx);
                                if local.registered_buffers.contains_key(&buffer.read(cx).buffer_id()) {
                                    local.register_buffer_with_language_servers(
                                        &buffer,
                                        FxHashSet::default(),
                                        LanguageServerStartMode::Automatic,
                                        cx,
                                    );
                                }
                            }
                        }

                        // Also register buffers that already have a language with
                        // any newly-available language servers (e.g., from extensions
                        // that finished loading after buffers were restored).
                        {
                            let local = &mut this.local;
                            for buffer in buffers_with_language {
                                if local.registered_buffers.contains_key(&buffer.read(cx).buffer_id()) {
                                    local.register_buffer_with_language_servers(
                                        &buffer,
                                        FxHashSet::default(),
                                        LanguageServerStartMode::Automatic,
                                        cx,
                                    );
                                }
                            }
                        }

                        for buffer in buffers_with_unknown_injections {
                            buffer.update(cx, |buffer, cx| buffer.reparse(cx, false));
                        }
                    });
                }
            }
        })
    }

    fn detect_language_for_buffer(&mut self, buffer_handle: &Entity<Buffer>, cx: &mut Context<Self>) {
        // If the buffer has a language, set it and start the language server if we haven't already.
        let buffer = buffer_handle.read(cx);
        let Some(file) = buffer.file() else {
            return;
        };
        let content = buffer.as_rope();
        if let Some(language_id) = self.languages.language_for_file(file, Some(content), cx) {
            if let Some(Ok(Ok(new_language))) = self.languages.load_language(language_id).now_or_never() {
                self.set_language_for_buffer(buffer_handle, new_language, cx);
            }
        } else {
            cx.emit(LspStoreEvent::LanguageDetected { buffer: buffer_handle.clone(), new_language: None });
        }
    }

    pub(crate) fn set_language_for_buffer(
        &mut self,
        buffer_entity: &Entity<Buffer>,
        new_language: Arc<Language>,
        cx: &mut Context<Self>,
    ) {
        let buffer = buffer_entity.read(cx);
        let buffer_file = buffer.file().cloned();
        let buffer_id = buffer.buffer_id();
        let mut detached_servers = Vec::new();
        let mut detached_abs_path = None;
        let local_store = &mut self.local;
        if local_store.registered_buffers.contains_key(&buffer_id)
            && let Some(abs_path) = File::from_dyn(buffer_file.as_ref()).map(|file| file.abs_path(cx))
            && let Some(file_url) = file_path_to_lsp_url(&abs_path).log_err()
        {
            detached_servers = local_store.unregister_buffer_from_language_servers(buffer_entity, &file_url, cx);
            detached_abs_path = Some(abs_path);
        }
        buffer_entity.update(cx, |buffer, cx| {
            if buffer.language().is_none_or(|old_language| !Arc::ptr_eq(old_language, &new_language)) {
                buffer.set_language_async(Some(Arc::clone(&new_language)), cx);
            }
        });

        let buffer_file = File::from_dyn(buffer_file.as_ref());

        let local = &mut self.local;
        if buffer_file.is_some() && local.registered_buffers.contains_key(&buffer_id) {
            local.register_buffer_with_language_servers(
                buffer_entity,
                FxHashSet::default(),
                LanguageServerStartMode::Automatic,
                cx,
            );
        }

        // A detached server keeps running and may legitimately re-publish for this path,
        // since publishes are applied by path with no regard for registration. So only
        // drop what it already said: clear the (path, server) diagnostics everywhere,
        // exempting servers whose workspace-diagnostics pull owns the verdict, mirroring
        // the buffer-close path. Anything a live server still asserts comes back on its
        // next publish; a verdict from a server that stopped tracking the file has no
        // other way to ever go away.
        if let Some(abs_path) = detached_abs_path
            && !detached_servers.is_empty()
        {
            let servers_to_clear = {
                let local = &self.local;
                let reattached_servers =
                    buffer_entity.update(cx, |buffer, cx| local.language_server_ids_for_buffer(buffer, cx));
                detached_servers
                    .into_iter()
                    .filter(|server_id| !reattached_servers.contains(server_id))
                    .filter(|server_id| !local.server_pulls_workspace_diagnostics(*server_id))
                    .collect::<Vec<_>>()
            };
            if let Some(lsp_data) = self.lsp_data.get_mut(&buffer_id) {
                for server_id in &servers_to_clear {
                    lsp_data.remove_server_data(*server_id);
                }
            }
            self.clear_path_diagnostics_for_servers(abs_path, servers_to_clear, |_, _, _| false, cx);
        }

        cx.emit(LspStoreEvent::LanguageDetected { buffer: buffer_entity.clone(), new_language: Some(new_language) });
    }

    pub fn buffer_store(&self) -> Entity<BufferStore> {
        self.buffer_store.clone()
    }

    pub const fn set_active_entry(&mut self, active_entry: Option<ProjectEntryId>) {
        self.active_entry = active_entry;
    }

    fn notify_server_capabilities_updated(&mut self, server: &LanguageServer) {
        let server_id = server.server_id();
        self.lsp_server_capabilities.insert(server_id, server.capabilities());
        if let Some(status) = self.language_server_statuses.get_mut(&server_id) {
            status.configuration = Some(server.configuration().clone());
            status.workspace_folders = server.workspace_folders();
        }
    }

    fn refresh_document_highlights(&self, for_server: Option<LanguageServerId>, cx: &mut Context<Self>) {
        cx.emit(LspStoreEvent::RefreshDocumentHighlights { server_id: for_server });
    }

    pub fn request_lsp<R>(
        &mut self,
        buffer: Entity<Buffer>,
        server: LanguageServerToQuery,
        request: R,
        cx: &mut Context<Self>,
    ) -> Task<Result<R::Response>>
    where
        R: LspCommand,
        <R::LspRequest as Request>::Result: Send,
        <R::LspRequest as Request>::Params: Send,
    {
        let query_outcome = buffer.update(cx, |buffer, cx| {
            let mut early_response = None;
            {
                let local = &self.local;
                for (adapter, language_server) in local.language_servers_for_buffer(buffer, cx) {
                    if let LanguageServerToQuery::Other(id) = server
                        && language_server.server_id() != id
                    {
                        continue;
                    }
                    let mut applicable_capabilities = applicable_lsp_command_capabilities_for_buffer(
                        local,
                        &request,
                        buffer,
                        adapter,
                        language_server,
                    )
                    .peekable();
                    if applicable_capabilities.peek().is_some() {
                        match request.response_without_request(&mut applicable_capabilities) {
                            None => {
                                return LanguageServerQueryOutcome::Query(Arc::clone(language_server));
                            }
                            Some(response) => {
                                if early_response.is_none() {
                                    early_response = Some(response);
                                }
                            }
                        }
                    }
                    if let LanguageServerToQuery::Other(_) = server {
                        break;
                    }
                }
            }
            LanguageServerQueryOutcome::Respond(early_response.unwrap_or_default())
        });
        let language_server = match query_outcome {
            LanguageServerQueryOutcome::Query(language_server) => language_server,
            LanguageServerQueryOutcome::Respond(response) => return Task::ready(Ok(response)),
        };

        let Some(file) = File::from_dyn(buffer.read(cx).file()) else {
            return Task::ready(Ok(Default::default()));
        };

        let lsp_params = match request.to_lsp(&file.abs_path(cx), buffer.read(cx), &language_server, cx) {
            Ok(lsp_params) => lsp_params,
            Err(err) => {
                let err = err.context(format!("{} via {} failed", request.display_name(), language_server.name()));
                let message = format!("{err:#}");
                if should_log_lsp_request_failure(&message) {
                    log::warn!("{message}");
                }
                return Task::ready(Err(err));
            }
        };

        let status = request.status();
        let request_timeout = LSP_REQUEST_TIMEOUT;

        cx.spawn(async move |this, cx| {
            let lsp_request = language_server.request::<R::LspRequest>(lsp_params, request_timeout);

            let id = lsp_request.id();
            let _cleanup = status.is_some().then(|| {
                cx.update(|cx| {
                    this.update(cx, |this, cx| {
                        this.on_lsp_work_start(
                            language_server.server_id(),
                            ProgressToken::Number(id),
                            LanguageServerProgress {
                                is_disk_based_diagnostics_progress: false,
                                is_cancellable: false,
                                title: None,
                                message: status.clone(),
                                percentage: None,
                                last_update_at: cx.background_executor().now(),
                            },
                            cx,
                        );
                    })
                })
                .log_err();

                defer(|| {
                    cx.update(|cx| {
                        this.update(cx, |this, cx| {
                            this.on_lsp_work_end(language_server.server_id(), ProgressToken::Number(id), cx);
                        })
                    })
                    .log_err();
                })
            });

            let result = lsp_request.await.into_response();

            let response = result.map_err(|err| {
                let context = format!("{} via {} failed", request.display_name(), language_server.name());
                if should_log_lsp_response_error(&err) {
                    let message = format!("{context}: {err}");
                    if should_log_lsp_request_failure(&message) {
                        log::warn!("{message}");
                    }
                }
                err.context(context)
            })?;

            request
                .response_from_lsp(
                    response,
                    this.upgrade().context("no app context")?,
                    buffer,
                    language_server.server_id(),
                    cx.clone(),
                )
                .await
        })
    }

    pub fn resolved_hint(&mut self, buffer_id: BufferId, id: InlayId, cx: &mut Context<Self>) -> Option<ResolvedHint> {
        let buffer = self.buffer_store.read(cx).get(buffer_id)?;

        let lsp_data = self.lsp_data.get_mut(&buffer_id)?;
        let buffer_lsp_hints = &mut lsp_data.inlay_hints;
        let hint = buffer_lsp_hints.hint_for_id(id)?.clone();
        let (server_id, resolve_data) = match &hint.resolve_state {
            ResolveState::Resolved => return Some(ResolvedHint::Resolved(hint)),
            ResolveState::Resolving => {
                return Some(ResolvedHint::Resolving(buffer_lsp_hints.hint_resolves.get(&id)?.clone()));
            }
            ResolveState::CanResolve(server_id, resolve_data)
                if !buffer.read(cx).has_edits_since(&lsp_data.buffer_version) =>
            {
                (*server_id, resolve_data.clone())
            }
            ResolveState::CanResolve(_, _) => return None,
        };

        let resolve_task = self.resolve_inlay_hint(hint, buffer, server_id, cx);
        let resolve_task = cx
            .spawn(async move |lsp_store, cx| {
                let resolved_hint = resolve_task.await;
                lsp_store
                    .update(cx, |lsp_store, _| {
                        let buffer_lsp_hints = &mut lsp_store.lsp_data.get_mut(&buffer_id)?.inlay_hints;
                        buffer_lsp_hints.hint_resolves.remove(&id);
                        let old_inlay_hint = buffer_lsp_hints.hint_for_id(id)?;
                        match resolved_hint {
                            Ok(resolved_hint) => {
                                *old_inlay_hint = resolved_hint.clone();
                                Some(resolved_hint)
                            }
                            Err(error) => {
                                old_inlay_hint.resolve_state = ResolveState::CanResolve(server_id, resolve_data);
                                log::error!("Inlay hint resolve failed: {error:#}");
                                None
                            }
                        }
                    })
                    .ok()
                    .flatten()
            })
            .shared();
        let buffer_lsp_hints = &mut self.lsp_data.get_mut(&buffer_id)?.inlay_hints;
        buffer_lsp_hints.hint_resolves.insert(id, resolve_task.clone());
        buffer_lsp_hints.hint_for_id(id)?.resolve_state = ResolveState::Resolving;
        Some(ResolvedHint::Resolving(resolve_task))
    }

    pub(crate) fn linked_edits(
        &mut self,
        buffer: &Entity<Buffer>,
        position: Anchor,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<Range<Anchor>>>> {
        let local = &self.local;
        let Some(server_id) = buffer
            .update(cx, |buffer, cx| {
                local
                    .language_servers_for_buffer(buffer, cx)
                    .filter(|(_, server)| LinkedEditingRange::check_server_capabilities(&server.capabilities()))
                    .map(|(_, server)| LanguageServerToQuery::Other(server.server_id()))
                    .next()
            })
            .filter(|_| buffer.read(cx).language_at(position).is_some())
        else {
            return Task::ready(Ok(Vec::new()));
        };

        self.request_lsp(buffer.clone(), server_id, LinkedEditingRange { position }, cx)
    }

    pub fn definitions(
        &mut self,
        buffer: &Entity<Buffer>,
        position: PointUtf16,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        let definitions_task = self.request_multiple_lsp_locally(buffer, GetDefinitions { position }, cx);
        cx.background_spawn(async move {
            Ok(Some(definitions_task.await.into_iter().flat_map(|(_, definitions)| definitions).dedup().collect()))
        })
    }

    pub fn declarations(
        &mut self,
        buffer: &Entity<Buffer>,
        position: PointUtf16,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        let declarations_task = self.request_multiple_lsp_locally(buffer, GetDeclarations { position }, cx);
        cx.background_spawn(async move {
            Ok(Some(declarations_task.await.into_iter().flat_map(|(_, declarations)| declarations).dedup().collect()))
        })
    }

    pub fn type_definitions(
        &mut self,
        buffer: &Entity<Buffer>,
        position: PointUtf16,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        let type_definitions_task = self.request_multiple_lsp_locally(buffer, GetTypeDefinitions { position }, cx);
        cx.background_spawn(async move {
            Ok(Some(
                type_definitions_task
                    .await
                    .into_iter()
                    .flat_map(|(_, type_definitions)| type_definitions)
                    .dedup()
                    .collect(),
            ))
        })
    }

    pub fn implementations(
        &mut self,
        buffer: &Entity<Buffer>,
        position: PointUtf16,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<LocationLink>>>> {
        let implementations_task = self.request_multiple_lsp_locally(buffer, GetImplementations { position }, cx);
        cx.background_spawn(async move {
            Ok(Some(
                implementations_task
                    .await
                    .into_iter()
                    .flat_map(|(_, implementations)| implementations)
                    .dedup()
                    .collect(),
            ))
        })
    }

    pub fn references(
        &mut self,
        buffer: &Entity<Buffer>,
        position: PointUtf16,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Vec<Location>>>> {
        let references_task = self.request_multiple_lsp_locally(buffer, GetReferences { position }, cx);
        cx.background_spawn(async move {
            Ok(Some(references_task.await.into_iter().flat_map(|(_, references)| references).dedup().collect()))
        })
    }

    pub fn applicable_inlay_chunks(
        &mut self,
        buffer: &Entity<Buffer>,
        ranges: &[Range<text::Anchor>],
        cx: &mut Context<Self>,
    ) -> Vec<Range<BufferRow>> {
        let buffer_snapshot = buffer.read(cx).snapshot();
        let ranges = ranges.iter().map(|range| range.to_point(&buffer_snapshot)).collect::<Vec<_>>();

        self.latest_lsp_data(buffer, cx)
            .inlay_hints
            .applicable_chunks(ranges.as_slice())
            .map(|chunk| chunk.row_range())
            .collect()
    }

    pub fn invalidate_inlay_hints<'a>(&'a mut self, for_buffers: impl IntoIterator<Item = &'a BufferId> + 'a) {
        for buffer_id in for_buffers {
            if let Some(lsp_data) = self.lsp_data.get_mut(buffer_id) {
                lsp_data.inlay_hints.clear();
            }
        }
    }

    pub fn inlay_hints(
        &mut self,
        invalidate: InvalidationStrategy,
        buffer: Entity<Buffer>,
        ranges: Vec<Range<text::Anchor>>,
        known_chunks: Option<(clock::Global, FxHashSet<Range<BufferRow>>)>,
        cx: &mut Context<Self>,
    ) -> FxHashMap<Range<BufferRow>, Task<Result<CacheInlayHints>>> {
        let next_hint_id = Arc::clone(&self.next_hint_id);
        let capability_probe =
            InlayHints { range: text::Anchor::min_max_range_for_buffer(buffer.read(cx).buffer_id()) };
        let current_servers = self.language_server_ids_for_request(&buffer, &capability_probe, cx);
        let lsp_data = self.latest_lsp_data(&buffer, cx);
        let query_version = lsp_data.buffer_version.clone();
        if let InvalidationStrategy::RefreshRequested { server_id } = invalidate {
            lsp_data.inlay_hints.invalidate_for_server_refresh(server_id);
        }
        let existing_inlay_hints = &mut lsp_data.inlay_hints;
        existing_inlay_hints.fetched_servers.retain(|server_id| current_servers.contains(server_id));
        let missing_servers =
            current_servers.difference(&existing_inlay_hints.fetched_servers).copied().collect::<FxHashSet<_>>();
        let known_chunks = known_chunks
            .filter(|(known_version, _)| !query_version.changed_since(known_version))
            .map_or_default(|(_, known_chunks)| known_chunks);

        let buffer_snapshot = buffer.read(cx).snapshot();
        let ranges = ranges.iter().map(|range| range.to_point(&buffer_snapshot)).collect::<Vec<_>>();

        let applicable_chunks = existing_inlay_hints
            .applicable_chunks(ranges.as_slice())
            .filter(|chunk| !known_chunks.contains(&chunk.row_range()))
            .collect::<Vec<_>>();
        if applicable_chunks.is_empty() {
            return FxHashMap::default();
        }

        let mut chunk_hint_tasks = FxHashMap::default();
        for chunk in applicable_chunks {
            let cached_hints = existing_inlay_hints.cached_hints(&chunk).cloned();
            let running_fetch = existing_inlay_hints.fetched_hints(&chunk).clone();
            let servers_to_query = if cached_hints.is_none() && running_fetch.is_none() {
                Some(None)
            } else if missing_servers.is_empty() {
                None
            } else {
                Some(Some(missing_servers.clone()))
            };

            let Some(servers_to_query) = servers_to_query else {
                match running_fetch {
                    Some(running_fetch) => {
                        chunk_hint_tasks.insert(chunk.row_range(), ChunkFetch::Running(running_fetch))
                    }
                    None => {
                        chunk_hint_tasks.insert(chunk.row_range(), ChunkFetch::Cached(cached_hints.unwrap_or_default()))
                    }
                };
                continue;
            };

            existing_inlay_hints.fetched_servers.extend(servers_to_query.iter().flatten().copied());
            if servers_to_query.is_none() {
                existing_inlay_hints.fetched_servers.extend(current_servers.iter().copied());
            }
            let queried_servers = match &servers_to_query {
                Some(servers) => servers.clone(),
                None => current_servers.clone(),
            };

            let next_hint_id = Arc::clone(&next_hint_id);
            let buffer = buffer.clone();
            let query_version = query_version.clone();
            let range_to_query = chunk.anchor_range();
            let new_inlay_hints = cx
                .spawn(async move |lsp_store, cx| {
                    // Let the previous fetch merge its hints into the cache first, so
                    // that the chunk state read below contains its servers' hints too.
                    if let Some(running_fetch) = running_fetch {
                        running_fetch.await.ok();
                    }
                    let new_fetch_task = lsp_store.update(cx, |lsp_store, cx| {
                        lsp_store.fetch_inlay_hints(servers_to_query, &buffer, range_to_query, cx)
                    })?;
                    match new_fetch_task.await {
                        Ok(new_hints_by_server) => lsp_store
                            .update(cx, |lsp_store, cx| {
                                let lsp_data = lsp_store.latest_lsp_data(&buffer, cx);
                                // `chunk` indexes the chunks of the buffer version this fetch
                                // was started for. If the buffer changed meanwhile, the hint
                                // cache has been rebuilt for the new version and may hold fewer
                                // chunks, so the stale id must not be used to index it.
                                if lsp_data.buffer_version != query_version {
                                    return CacheInlayHints::default();
                                }
                                if new_hints_by_server.is_empty() {
                                    lsp_data.inlay_hints.invalidate_for_chunk(chunk);
                                } else {
                                    for (server_id, new_hints) in new_hints_by_server {
                                        let new_hints = new_hints
                                            .into_iter()
                                            .map(|new_hint| {
                                                (
                                                    InlayId::Hint(next_hint_id.fetch_add(1, atomic::Ordering::AcqRel)),
                                                    new_hint,
                                                )
                                            })
                                            .collect::<Vec<_>>();
                                        lsp_data.inlay_hints.insert_new_hints(chunk, server_id, new_hints);
                                    }
                                }
                                lsp_data.inlay_hints.cached_hints(&chunk).cloned().unwrap_or_default()
                            })
                            .map_err(Arc::new),
                        Err(e) => {
                            lsp_store
                                .update(cx, |lsp_store, cx| {
                                    let lsp_data = lsp_store.latest_lsp_data(&buffer, cx);
                                    if lsp_data.buffer_version == query_version {
                                        for server_id in &queried_servers {
                                            lsp_data.inlay_hints.fetched_servers.remove(server_id);
                                        }
                                    }
                                })
                                .ok();
                            Err(Arc::new(e))
                        }
                    }
                })
                .shared();

            *existing_inlay_hints.fetched_hints(&chunk) = Some(new_inlay_hints.clone());
            chunk_hint_tasks.insert(chunk.row_range(), ChunkFetch::Running(new_inlay_hints));
        }

        chunk_hint_tasks
            .into_iter()
            .map(|(row_range, chunk_fetch)| {
                let task = match chunk_fetch {
                    ChunkFetch::Cached(hints) => Task::ready(Ok(hints)),
                    ChunkFetch::Running(hints_fetch) => {
                        cx.spawn(async move |_, _| hints_fetch.await.map_err(|e| anyhow!("{e:#}")))
                    }
                };
                (row_range, task)
            })
            .collect()
    }

    fn fetch_inlay_hints(
        &mut self,
        for_servers: Option<FxHashSet<LanguageServerId>>,
        buffer: &Entity<Buffer>,
        range: Range<Anchor>,
        cx: &mut Context<Self>,
    ) -> Task<Result<FxHashMap<LanguageServerId, Vec<InlayHint>>>> {
        let request = InlayHints { range: range.clone() };
        let inlay_hints_task = self.request_filtered_lsp_locally(buffer, request, for_servers.as_ref(), cx);
        let buffer = buffer.clone();
        cx.spawn(async move |_, cx| {
            let inlay_hints = inlay_hints_task.await;
            // Response conversion can anchor hints to edits made while the request was pending.
            let buffer_snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
            cx.background_spawn(async move {
                Ok(inlay_hints
                    .into_iter()
                    .map(|(server_id, new_hints)| {
                        (server_id, inlay_hints::hints_in_range(new_hints, &range, &buffer_snapshot))
                    })
                    .collect())
            })
            .await
        })
    }

    pub fn hover(
        &mut self,
        buffer: &Entity<Buffer>,
        position: PointUtf16,
        cx: &mut Context<Self>,
    ) -> Task<Option<Vec<Hover>>> {
        let all_actions_task = self.request_multiple_lsp_locally(buffer, GetHover { position }, cx);
        cx.background_spawn(async move {
            Some(deduplicate_hovers(
                all_actions_task
                    .await
                    .into_iter()
                    .filter_map(|(_, hover)| remove_empty_hover_blocks(hover?))
                    .collect::<Vec<Hover>>(),
            ))
        })
    }

    pub fn symbols(&self, query: &str, cx: &mut Context<Self>) -> Task<Result<Vec<Symbol>>> {
        let language_registry = Arc::clone(&self.languages);

        {
            let local = &self.local;
            struct WorkspaceSymbolsResult {
                server_id: LanguageServerId,
                lsp_adapter: Arc<CachedLspAdapter>,
                worktree: WeakEntity<Worktree>,
                lsp_symbols: Vec<(String, language::SymbolKind, lsp::Location, Option<String>)>,
            }

            let mut requests = Vec::new();
            let mut requested_servers = BTreeSet::new();
            let request_timeout = LSP_REQUEST_TIMEOUT;

            for (seed, state) in &local.language_server_ids {
                let Some(worktree_handle) = self.worktree_store.read(cx).worktree_for_id(seed.worktree_id, cx) else {
                    continue;
                };

                let worktree = worktree_handle.read(cx);
                if !worktree.is_visible() {
                    continue;
                }

                if !requested_servers.insert(state.id) {
                    continue;
                }

                let (lsp_adapter, server) = match local.language_servers.get(&state.id) {
                    Some(LanguageServerState::Running { adapter, server, .. }) => (Arc::clone(adapter), server),

                    _ => continue,
                };

                let supports_workspace_symbol_request = match server.capabilities().workspace_symbol_provider {
                    Some(OneOf::Left(supported)) => supported,
                    Some(OneOf::Right(_)) => true,
                    None => false,
                };

                if !supports_workspace_symbol_request {
                    continue;
                }

                let worktree_handle = worktree_handle.clone();
                let server_id = server.server_id();
                requests.push(
                    server
                        .request::<WorkspaceSymbolRequest>(
                            lsp::WorkspaceSymbolParams { query: query.to_owned(), ..Default::default() },
                            request_timeout,
                        )
                        .map(move |response| {
                            let lsp_symbols = response
                                .into_response()
                                .context("workspace symbols request")
                                .log_err()
                                .flatten()
                                .map_or_default(|symbol_response| match symbol_response {
                                    lsp::WorkspaceSymbolResponse::Flat(flat_responses) => flat_responses
                                        .into_iter()
                                        .map(|lsp_symbol| {
                                            (
                                                lsp_symbol.name,
                                                lsp_to_symbol_kind(lsp_symbol.kind),
                                                lsp_symbol.location,
                                                lsp_symbol.container_name,
                                            )
                                        })
                                        .collect::<Vec<_>>(),
                                    lsp::WorkspaceSymbolResponse::Nested(nested_responses) => nested_responses
                                        .into_iter()
                                        .filter_map(|lsp_symbol| {
                                            let location = match lsp_symbol.location {
                                                OneOf::Left(location) => location,
                                                OneOf::Right(_) => {
                                                    log::error!(
                                                        "Unexpected: client capabilities \
                                                            forbid symbol resolutions in \
                                                            workspace.symbol.resolveSupport"
                                                    );
                                                    return None;
                                                }
                                            };
                                            Some((
                                                lsp_symbol.name,
                                                lsp_to_symbol_kind(lsp_symbol.kind),
                                                location,
                                                lsp_symbol.container_name,
                                            ))
                                        })
                                        .collect::<Vec<_>>(),
                                });

                            WorkspaceSymbolsResult {
                                server_id,
                                lsp_adapter,
                                worktree: worktree_handle.downgrade(),
                                lsp_symbols,
                            }
                        }),
                );
            }

            cx.spawn(async move |this, cx| {
                let responses = join_all(requests).await;
                let Some(this) = this.upgrade() else {
                    return Ok(Vec::new());
                };

                let mut symbols = Vec::new();
                for result in responses {
                    let core_symbols = this.update(cx, |this, cx| {
                        result
                            .lsp_symbols
                            .into_iter()
                            .filter_map(|(symbol_name, symbol_kind, symbol_location, container_name)| {
                                let abs_path = symbol_location.uri.to_file_path().ok()?;
                                let source_worktree = result.worktree.upgrade()?;
                                let source_worktree_id = source_worktree.read(cx).id();

                                let path = if let Some((tree, rel_path)) =
                                    this.worktree_store.read(cx).find_worktree(&abs_path, cx)
                                {
                                    let worktree_id = tree.read(cx).id();
                                    SymbolLocation::InProject(ProjectPath { worktree_id, path: rel_path })
                                } else {
                                    SymbolLocation::OutsideProject {
                                        signature: this.symbol_signature(&abs_path),
                                        abs_path: abs_path.into(),
                                    }
                                };

                                Some(CoreSymbol {
                                    source_language_server_id: result.server_id,
                                    language_server_name: result.lsp_adapter.name.clone(),
                                    source_worktree_id,
                                    path,
                                    kind: symbol_kind,
                                    name: collapse_newlines(&symbol_name, "↵ "),
                                    range: range_from_lsp(symbol_location.range),
                                    container_name: container_name.map(|c| collapse_newlines(&c, "↵ ")),
                                })
                            })
                            .collect::<Vec<_>>()
                    });

                    populate_labels_for_symbols(
                        core_symbols,
                        &language_registry,
                        Some(result.lsp_adapter),
                        &mut symbols,
                    )
                    .await;
                }

                Ok(symbols)
            })
        }
    }

    pub fn on_buffer_edited(&mut self, buffer: Entity<Buffer>, cx: &mut Context<Self>) -> Option<()> {
        let language_servers: Vec<_> = buffer.update(cx, |buffer, cx| {
            Some(self.local.language_servers_for_buffer(buffer, cx).map(|i| Arc::clone(i.1)).collect())
        })?;

        let buffer = buffer.read(cx);
        let file = File::from_dyn(buffer.file())?;
        let abs_path = file.abs_path(cx);
        let uri = lsp::Uri::from_file_path(&abs_path)
            .ok()
            .with_context(|| format!("Failed to convert path to URI: {}", abs_path.display()))
            .log_err()?;
        let next_snapshot = buffer.text_snapshot();
        let line_ending = next_snapshot.line_ending();

        for language_server in language_servers {
            let language_server = language_server;

            let buffer_snapshots = self
                .local
                .buffer_snapshots
                .get_mut(&buffer.buffer_id())
                .and_then(|m| m.get_mut(&language_server.server_id()))?;
            let previous_snapshot = buffer_snapshots.last()?;

            // If the line ending differs from what this server was last sent, the LF-normalized
            // rope is byte-identical so `edits_since` yields no diffs. We must resync the whole
            // document, otherwise the server keeps the stale line endings indefinitely.
            let line_ending_changed = line_ending != previous_snapshot.snapshot.line_ending();

            let build_full_change = || {
                vec![lsp::TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: next_snapshot.text_with_line_endings(),
                }]
            };

            let build_incremental_change = || {
                buffer
                    .edits_since::<Dimensions<PointUtf16, usize>>(previous_snapshot.snapshot.version())
                    .map(|edit| {
                        let edit_start = edit.new.start.0;
                        let edit_end = edit_start + (edit.old.end.0 - edit.old.start.0);
                        let new_text = next_snapshot.text_for_range(edit.new.start.1..edit.new.end.1).collect();
                        lsp::TextDocumentContentChangeEvent {
                            range: Some(lsp::Range::new(point_to_lsp(edit_start), point_to_lsp(edit_end))),
                            range_length: None,
                            text: line_ending.apply(new_text),
                        }
                    })
                    .collect()
            };

            let document_sync_kind =
                language_server.capabilities().text_document_sync.as_ref().and_then(|sync| match sync {
                    lsp::TextDocumentSyncCapability::Kind(kind) => Some(*kind),
                    lsp::TextDocumentSyncCapability::Options(options) => options.change,
                });

            let build_change = || {
                if line_ending_changed { build_full_change() } else { build_incremental_change() }
            };

            let content_changes: Vec<_> = match document_sync_kind {
                Some(lsp::TextDocumentSyncKind::FULL) => build_full_change(),
                Some(lsp::TextDocumentSyncKind::INCREMENTAL) => build_change(),
                _ => {
                    continue;
                }
            };

            let next_version = previous_snapshot.version + 1;
            buffer_snapshots.push(LspBufferSnapshot { version: next_version, snapshot: next_snapshot.clone() });

            language_server
                .notify::<DidChangeTextDocument>(lsp::DidChangeTextDocumentParams {
                    text_document: lsp::VersionedTextDocumentIdentifier::new(uri.clone(), next_version),
                    content_changes,
                })
                .ok();
            self.pull_workspace_diagnostics(language_server.server_id());
        }

        None
    }

    pub fn on_buffer_saved(&mut self, buffer: Entity<Buffer>, cx: &mut Context<Self>) -> Option<()> {
        if File::from_dyn(buffer.read(cx).file()).is_some_and(|file| is_workspace_state_file(file.path())) {
            let buffers = self.buffer_store.read(cx).buffers().collect();
            self.restart_language_servers_for_buffers(buffers, lint_server_selectors(), cx);
        }
        let file = File::from_dyn(buffer.read(cx).file())?;
        let worktree_id = file.worktree_id(cx);
        let abs_path = file.abs_path(cx);
        let text_document = lsp::TextDocumentIdentifier { uri: file_path_to_lsp_url(&abs_path).log_err()? };
        let local = &self.local;

        for server in local.language_servers_for_worktree(worktree_id) {
            if let Some(include_text) = include_text(server.as_ref()) {
                let text = include_text.then(|| buffer.read(cx).text());
                server
                    .notify::<DidSaveTextDocument>(lsp::DidSaveTextDocumentParams {
                        text_document: text_document.clone(),
                        text,
                    })
                    .ok();
            }
        }

        let language_servers = buffer.update(cx, |buffer, cx| local.language_server_ids_for_buffer(buffer, cx));
        for language_server_id in language_servers {
            self.simulate_disk_based_diagnostics_events_if_needed(language_server_id, cx);
        }

        None
    }

    pub fn running_language_servers_for_local_buffer<'a>(
        &'a self,
        buffer: &Buffer,
        cx: &mut App,
    ) -> impl Iterator<Item = (&'a Arc<CachedLspAdapter>, &'a Arc<LanguageServer>)> {
        let local = &self.local;
        let language_server_ids = local.language_server_ids_for_buffer(buffer, cx);

        language_server_ids.into_iter().filter_map(move |server_id| match local.language_servers.get(&server_id)? {
            LanguageServerState::Running { adapter, server, .. } => Some((adapter, server)),
            LanguageServerState::Starting { .. } => None,
        })
    }

    pub fn language_server_ids_for_opened_buffer(&self, buffer_id: BufferId) -> Option<&FxHashSet<LanguageServerId>> {
        self.local.buffers_opened_in_servers.get(&buffer_id)
    }

    pub fn language_server_for_local_buffer<'a>(
        &'a self,
        buffer: &'a Buffer,
        server_id: LanguageServerId,
        cx: &'a mut App,
    ) -> Option<(&'a Arc<CachedLspAdapter>, &'a Arc<LanguageServer>)> {
        self.local.language_servers_for_buffer(buffer, cx).find(|(_, s)| s.server_id() == server_id)
    }

    pub(crate) fn language_server_capable_of_lsp_request<R: LspCommand>(
        &self,
        buffer: &Entity<Buffer>,
        server_id: LanguageServerId,
        request: &R,
        cx: &mut App,
    ) -> bool {
        self.text_document_capability_matches_for_server(
            buffer,
            server_id,
            <R::LspRequest as Request>::METHOD,
            |capabilities| request.check_capabilities(capabilities),
            cx,
        )
    }

    fn remove_worktree(&mut self, id_to_remove: WorktreeId, cx: &mut Context<Self>) {
        self.diagnostic_summaries.remove(&id_to_remove);
        {
            let local = &mut self.local;
            let to_remove = local.remove_worktree(id_to_remove, cx);
            for server in to_remove {
                self.language_server_statuses.remove(&server);
                self.cleanup_lsp_data(server);
            }
        }
    }

    pub fn open_buffer_for_symbol(&mut self, symbol: &Symbol, cx: &mut Context<Self>) -> Task<Result<Entity<Buffer>>> {
        {
            let local = &self.local;
            let is_valid = local.language_server_ids.iter().any(|(seed, state)| {
                seed.worktree_id == symbol.source_worktree_id
                    && state.id == symbol.source_language_server_id
                    && symbol.language_server_name == seed.name
            });
            if !is_valid {
                return Task::ready(Err(anyhow!("language server for worktree and language not found")));
            }

            let symbol_abs_path = match &symbol.path {
                SymbolLocation::InProject(project_path) => {
                    self.worktree_store.read(cx).absolutize(project_path, cx).context("no such worktree")
                }
                SymbolLocation::OutsideProject { abs_path, signature: _ } => Ok(abs_path.to_path_buf()),
            };
            let symbol_abs_path = match symbol_abs_path {
                Ok(abs_path) => abs_path,
                Err(err) => return Task::ready(Err(err)),
            };
            let Ok(symbol_uri) = lsp::Uri::from_file_path(symbol_abs_path) else {
                return Task::ready(Err(anyhow!("invalid symbol path")));
            };

            self.open_local_buffer_via_lsp(symbol_uri, symbol.source_language_server_id, cx)
        }
    }

    pub(crate) fn open_local_buffer_via_lsp(
        &self,
        abs_path: lsp::Uri,
        language_server_id: LanguageServerId,
        cx: &Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        cx.spawn(async move |lsp_store, cx| {
            let abs_path = abs_path.to_file_path().map_err(|()| anyhow!("can't convert URI to path"))?;
            let worktree = find_worktree_for_lsp_path(&lsp_store, &abs_path, cx).await?;
            let (worktree, relative_path) = if let Some((worktree, relative_path)) = worktree {
                (worktree, relative_path)
            } else {
                let worktree = lsp_store
                    .update(cx, |lsp_store, cx| {
                        lsp_store
                            .worktree_store
                            .update(cx, |worktree_store, cx| worktree_store.create_worktree(&abs_path, false, cx))
                    })?
                    .await?;
                let worktree_root = worktree.read_with(cx, |worktree, _| worktree.abs_path());
                {
                    lsp_store
                        .update(cx, |lsp_store, cx| {
                            let local = &mut lsp_store.local;
                            local.register_language_server_for_invisible_worktree(&worktree, language_server_id, cx);
                        })
                        .ok();
                }
                let relative_path = RelPath::new(abs_path.strip_prefix(worktree_root)?)?.into_arc();
                (worktree, relative_path)
            };
            let project_path =
                ProjectPath { worktree_id: worktree.read_with(cx, |worktree, _| worktree.id()), path: relative_path };
            lsp_store
                .update(cx, |lsp_store, cx| {
                    lsp_store.buffer_store().update(cx, |buffer_store, cx| buffer_store.open_buffer(project_path, cx))
                })?
                .await
        })
    }

    fn language_server_ids_for_request<R>(
        &self,
        buffer: &Entity<Buffer>,
        request: &R,
        cx: &mut App,
    ) -> FxHashSet<LanguageServerId>
    where
        R: LspCommand,
    {
        let local = &self.local;
        let buffer_id = buffer.read(cx).buffer_id();
        buffer.update(cx, |buffer, cx| {
            local
                .language_servers_for_buffer(buffer, cx)
                .filter(|(adapter, server)| lsp_command_allowed_for_buffer(local, request, buffer, adapter, server))
                .map(|(_, server)| server.server_id())
                .filter(|server_id| {
                    local.buffers_opened_in_servers.get(&buffer_id).is_some_and(|servers| servers.contains(server_id))
                })
                .collect()
        })
    }

    fn request_multiple_lsp_locally<R>(
        &mut self,
        buffer: &Entity<Buffer>,
        request: R,
        cx: &mut Context<Self>,
    ) -> Task<Vec<(LanguageServerId, R::Response)>>
    where
        R: LspCommand + Clone,
        <R::LspRequest as Request>::Result: Send,
        <R::LspRequest as Request>::Params: Send,
    {
        self.request_filtered_lsp_locally(buffer, request, None, cx)
    }

    fn request_filtered_lsp_locally<R>(
        &mut self,
        buffer: &Entity<Buffer>,
        request: R,
        only_servers: Option<&FxHashSet<LanguageServerId>>,
        cx: &mut Context<Self>,
    ) -> Task<Vec<(LanguageServerId, R::Response)>>
    where
        R: LspCommand + Clone,
        <R::LspRequest as Request>::Result: Send,
        <R::LspRequest as Request>::Params: Send,
    {
        let local = &self.local;

        let server_ids = buffer.update(cx, |buffer, cx| {
            local
                .language_servers_for_buffer(buffer, cx)
                .filter(|(adapter, server)| lsp_command_allowed_for_buffer(local, &request, buffer, adapter, server))
                .map(|(_, server)| server.server_id())
                .filter(|server_id| only_servers.is_none_or(|only_servers| only_servers.contains(server_id)))
                .filter(|server_id| {
                    let local = &self.local;
                    local
                        .buffers_opened_in_servers
                        .get(&buffer.buffer_id())
                        .is_some_and(|servers| servers.contains(server_id))
                })
                .collect::<Vec<_>>()
        });

        let mut response_results = server_ids
            .into_iter()
            .map(|server_id| {
                let task =
                    self.request_lsp(buffer.clone(), LanguageServerToQuery::Other(server_id), request.clone(), cx);
                async move { (server_id, task.await) }
            })
            .collect::<FuturesUnordered<_>>();

        cx.background_spawn(async move {
            let mut responses = Vec::with_capacity(response_results.len());
            while let Some((server_id, response_result)) = response_results.next().await {
                match response_result {
                    Ok(response) => responses.push((server_id, response)),
                    // rust-analyzer likes to error with this when its still loading up
                    Err(e)
                        if !should_log_lsp_response_error(&e) || !should_log_lsp_request_failure(&format!("{e:#}")) => {
                    }
                    Err(e) => log::error!("Error handling response for request {request:?}: {e:#}"),
                }
            }
            responses
        })
    }

    pub fn language_server_statuses(
        &self,
    ) -> impl DoubleEndedIterator<Item = (LanguageServerId, &LanguageServerStatus)> {
        self.language_server_statuses.iter().map(|(key, value)| (*key, value))
    }

    fn should_forward_untagged_binary_status(&self, server_name: &LanguageServerName) -> bool {
        {
            let local = &self.local;
            local.language_server_ids.keys().any(|seed| &seed.name == server_name)
                || self.language_server_statuses.values().any(|status| &status.name == server_name)
        }
    }

    pub fn language_server_for_id(&self, id: LanguageServerId) -> Option<Arc<LanguageServer>> {
        self.local.language_server_for_id(id)
    }

    fn symbol_signature(&self, abs_path: &Path) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(abs_path.to_string_lossy().as_bytes());
        hasher.update(self.nonce.to_be_bytes());
        hasher.finalize().as_slice().try_into().expect("sha256 digest is 32 bytes")
    }

    pub fn language_server_adapter_for_id(&self, id: LanguageServerId) -> Option<Arc<CachedLspAdapter>> {
        match self.local.language_servers.get(&id)? {
            LanguageServerState::Running { adapter, .. } => Some(Arc::clone(adapter)),
            LanguageServerState::Starting { .. } => None,
        }
    }

    fn cleanup_lsp_data(&mut self, for_server: LanguageServerId) {
        self.lsp_server_capabilities.remove(&for_server);
        self.semantic_token_config.remove_server_data(for_server);
        for lsp_data in self.lsp_data.values_mut() {
            lsp_data.remove_server_data(for_server);
        }
        {
            let local = &mut self.local;
            local.buffer_pull_diagnostics_result_ids.remove(&for_server);
            local.workspace_pull_diagnostics_result_ids.remove(&for_server);
            for buffer_servers in local.buffers_opened_in_servers.values_mut() {
                buffer_servers.remove(&for_server);
            }
        }
    }

    pub fn worktree_store(&self) -> Entity<WorktreeStore> {
        self.worktree_store.clone()
    }

    /// Gets what's stored in the LSP data for the given buffer.
    pub fn current_lsp_data(&mut self, buffer_id: BufferId) -> Option<&mut BufferLspData> {
        self.lsp_data.get_mut(&buffer_id)
    }

    /// Gets the most recent LSP data for the given buffer: if the data is absent or out of date,
    /// new [`BufferLspData`] will be created to replace the previous state.
    pub fn latest_lsp_data(&mut self, buffer: &Entity<Buffer>, cx: &mut App) -> &mut BufferLspData {
        let (buffer_id, buffer_version) = buffer.read_with(cx, |buffer, _| (buffer.buffer_id(), buffer.version()));
        let lsp_data = self.lsp_data.entry(buffer_id).or_insert_with(|| BufferLspData::new(buffer, cx));
        if buffer_version.changed_since(&lsp_data.buffer_version) {
            lsp_data.reset_for_newer_buffer_version(buffer, cx);
        }
        lsp_data
    }
}

async fn find_worktree_for_lsp_path(
    lsp_store: &WeakEntity<LspStore>,
    abs_path: &Path,
    cx: &AsyncApp,
) -> Result<Option<(Entity<Worktree>, Arc<RelPath>)>> {
    let (fs, worktree) = lsp_store.read_with(cx, |lsp_store, cx| {
        (Some(Arc::clone(&lsp_store.local.fs)), lsp_store.worktree_store.read(cx).find_worktree(abs_path, cx))
    })?;
    if let Some((worktree, relative_path)) = worktree {
        let relative_path = normalize_lsp_relative_path(&worktree, abs_path, relative_path, cx).await?;
        Ok(Some((worktree, relative_path)))
    } else {
        let Some(fs) = fs else {
            return Ok(None);
        };
        let Ok(canonical_path) = fs.canonicalize(abs_path).await else {
            return Ok(None);
        };
        let (worktree, mut scans) = lsp_store.read_with(cx, |lsp_store, cx| {
            let worktree_store = lsp_store.worktree_store.read(cx);
            if let Some(worktree) = worktree_store.find_worktree(&canonical_path, cx) {
                return (Some(worktree), FuturesUnordered::new());
            }
            let scans = worktree_store
                .worktrees()
                .map(|worktree| {
                    let scan_complete = worktree.read(cx).scan_complete();
                    async move {
                        scan_complete.await;
                        worktree
                    }
                })
                .collect::<FuturesUnordered<_>>();
            (None, scans)
        })?;
        if worktree.is_some() {
            return Ok(worktree);
        }
        // Language servers may report files in symlinked external
        // directories by their canonical path. These directories are
        // known only after they are scanned. `scan_complete` resolves
        // immediately for worktrees that are not scanning.
        while let Some(worktree) = scans.next().await {
            let relative_path =
                worktree.read_with(cx, |worktree, _| worktree.relative_path_for_external_abs_path(&canonical_path));
            if let Some(relative_path) = relative_path {
                return Ok(Some((worktree, Arc::from(relative_path))));
            }
        }
        Ok(None)
    }
}

async fn normalize_lsp_relative_path(
    worktree: &Entity<Worktree>,
    abs_path: &Path,
    relative_path: Arc<RelPath>,
    cx: &AsyncApp,
) -> Result<Arc<RelPath>> {
    let Some((fs, snapshot, worktree_root)) = worktree.read_with(cx, |worktree, _| {
        if worktree.fs_is_case_sensitive() {
            return None;
        }
        Some((Arc::clone(worktree.fs()), worktree.snapshot(), worktree.abs_path()))
    }) else {
        return Ok(relative_path);
    };
    if snapshot.entry_for_path(&relative_path).is_some() {
        return Ok(relative_path);
    }

    let mut normalized_path = RelPathBuf::new();
    let mut components = relative_path.components();
    while let Some(component) = components.next() {
        let mut exact_path = normalized_path.clone();
        exact_path.push_component(component)?;
        if snapshot.entry_for_path(&exact_path).is_some() {
            normalized_path = exact_path;
            continue;
        }
        let case_insensitive_match = snapshot
            .child_entries(&normalized_path)
            .find(|entry| entry.path.file_name().is_some_and(|name| name.eq_ignore_ascii_case(component)))
            .map(|entry| entry.path.to_rel_path_buf());
        let Some(matched_path) = case_insensitive_match else {
            if let Ok(canonical_path) = fs.canonicalize(abs_path).await
                && let Ok(canonical_root) = fs.canonicalize(&worktree_root).await
                && let Ok(canonical_relative_path) = canonical_path.strip_prefix(&canonical_root)
                && let Ok(canonical_relative_path) = RelPath::new(canonical_relative_path)
            {
                return Ok(canonical_relative_path.into_arc());
            }
            exact_path.push(components.rest());
            return Ok(Arc::from(exact_path));
        };
        normalized_path = matched_path;
    }
    Ok(Arc::from(normalized_path))
}

impl EventEmitter<LspStoreEvent> for LspStore {}

fn remove_empty_hover_blocks(mut hover: Hover) -> Option<Hover> {
    hover.contents.retain(|hover_block| !hover_block.text.trim().is_empty());
    if hover.contents.is_empty() { None } else { Some(hover) }
}

fn deduplicate_hovers(hovers: Vec<Hover>) -> Vec<Hover> {
    let mut unique_hovers = Vec::with_capacity(hovers.len());
    for hover in hovers {
        if !unique_hovers.iter().any(|unique: &Hover| unique.contents == hover.contents) {
            unique_hovers.push(hover);
        }
    }
    unique_hovers
}

#[derive(Debug)]
pub enum LanguageServerToQuery {
    /// Query language servers in order of users preference, up until one capable of handling the request is found.
    FirstCapable,
    /// Query a specific language server.
    Other(LanguageServerId),
}

enum LanguageServerQueryOutcome<T> {
    Query(Arc<LanguageServer>),
    Respond(T),
}

struct LspBufferSnapshot {
    version: i32,
    snapshot: TextBufferSnapshot,
}

/// A prompt requested by LSP server.
#[derive(Clone, Debug)]
pub struct LanguageServerPromptRequest {
    pub id: usize,
    pub level: PromptLevel,
    pub message: String,
    pub actions: Vec<MessageActionItem>,
    pub lsp_name: String,
    pub(crate) response_channel: async_channel::Sender<MessageActionItem>,
}

impl LanguageServerPromptRequest {
    pub fn new(
        level: PromptLevel,
        message: String,
        actions: Vec<MessageActionItem>,
        lsp_name: String,
        response_channel: async_channel::Sender<MessageActionItem>,
    ) -> Self {
        let id = NEXT_PROMPT_REQUEST_ID.fetch_add(1, atomic::Ordering::AcqRel);
        LanguageServerPromptRequest { id, level, message, actions, lsp_name, response_channel }
    }
    pub async fn respond(self, index: usize) -> Option<()> {
        if let Some(response) = self.actions.into_iter().nth(index) {
            self.response_channel.send(response).await.ok()
        } else {
            None
        }
    }
}
impl PartialEq for LanguageServerPromptRequest {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message && self.actions == other.actions
    }
}

#[derive(Clone, Debug)]
pub struct LanguageServerShowDocumentRequest {
    pub uri: lsp::Uri,
    pub external: bool,
    pub take_focus: bool,
    pub selection: Option<lsp::Range>,
    pub(crate) response_channel: async_channel::Sender<bool>,
}

impl LanguageServerShowDocumentRequest {
    pub fn respond(self, success: bool) {
        self.response_channel.try_send(success).ok();
    }
}

impl PartialEq for LanguageServerShowDocumentRequest {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri
            && self.external == other.external
            && self.take_focus == other.take_focus
            && self.selection == other.selection
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguageServerLogType {
    Log(MessageType),
    Trace { verbose_info: Option<String> },
}

pub enum LanguageServerState {
    Starting {
        startup: Task<Option<Arc<LanguageServer>>>,
        /// List of language servers that will be added to the workspace once it's initialization completes.
        pending_workspace_folders: Arc<Mutex<BTreeSet<Uri>>>,
    },

    Running {
        adapter: Arc<CachedLspAdapter>,
        server: Arc<LanguageServer>,
        simulate_disk_based_diagnostics_completion: Option<Task<()>>,
        workspace_diagnostics_refresh_tasks: FxHashMap<Option<String>, WorkspaceRefreshTask>,
    },
}

impl LanguageServerState {
    fn add_workspace_folder(&self, uri: Uri) {
        match self {
            LanguageServerState::Starting { pending_workspace_folders, .. } => {
                pending_workspace_folders.lock().insert(uri);
            }
            LanguageServerState::Running { server, .. } => {
                server.add_workspace_folder(uri);
            }
        }
    }
    fn _remove_workspace_folder(&self, uri: Uri) {
        match self {
            LanguageServerState::Starting { pending_workspace_folders, .. } => {
                pending_workspace_folders.lock().remove(&uri);
            }
            LanguageServerState::Running { server, .. } => server.remove_workspace_folder(uri),
        }
    }
}

impl fmt::Debug for LanguageServerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LanguageServerState::Starting { .. } => f.debug_struct("LanguageServerState::Starting").finish(),
            LanguageServerState::Running { .. } => f.debug_struct("LanguageServerState::Running").finish(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageServerProgress {
    pub is_disk_based_diagnostics_progress: bool,
    pub is_cancellable: bool,
    pub title: Option<String>,
    pub message: Option<String>,
    pub percentage: Option<usize>,
    #[serde(skip_serializing)]
    pub last_update_at: Instant,
}

#[allow(clippy::large_enum_variant)]
pub enum ResolvedHint {
    Resolved(InlayHint),
    Resolving(Shared<Task<Option<InlayHint>>>),
}

async fn populate_labels_for_symbols(
    symbols: Vec<CoreSymbol>,
    language_registry: &Arc<LanguageRegistry>,
    lsp_adapter: Option<Arc<CachedLspAdapter>>,
    output: &mut Vec<Symbol>,
) {
    #[allow(clippy::mutable_key_type)]
    let mut symbols_by_language = FxHashMap::<Option<Arc<Language>>, Vec<CoreSymbol>>::default();

    let mut unknown_paths = BTreeSet::<Arc<str>>::new();
    for symbol in symbols {
        let Some(file_name) = symbol.path.file_name() else {
            continue;
        };
        let language = language_registry.load_language_for_file_path(Path::new(file_name)).await.ok().or_else(|| {
            unknown_paths.insert(file_name.into());
            None
        });
        symbols_by_language.entry(language).or_default().push(symbol);
    }

    for unknown_path in unknown_paths {
        log::info!("no language found for symbol in file {unknown_path:?}");
    }

    let mut label_params = Vec::new();
    for (language, mut symbols) in symbols_by_language {
        label_params.clear();
        label_params.extend(symbols.iter_mut().map(|symbol| language::Symbol {
            name: mem::take(&mut symbol.name),
            kind: symbol.kind,
            container_name: symbol.container_name.take(),
        }));

        let mut labels = Vec::new();
        if let Some(language) = language {
            let lsp_adapter =
                lsp_adapter.clone().or_else(|| language_registry.lsp_adapters(&language.name()).first().cloned());
            if let Some(lsp_adapter) = lsp_adapter {
                labels = lsp_adapter.labels_for_symbols(&label_params, &language).await.log_err().unwrap_or_default();
            }
        }

        for ((symbol, language::Symbol { name, container_name, .. }), label) in
            symbols.into_iter().zip(label_params.drain(..)).zip(labels.into_iter().chain(iter::repeat(None)))
        {
            output.push(Symbol {
                language_server_name: symbol.language_server_name,
                source_worktree_id: symbol.source_worktree_id,
                source_language_server_id: symbol.source_language_server_id,
                path: symbol.path,
                label: label.unwrap_or_else(|| CodeLabel::plain(name.clone(), None)),
                name,
                kind: symbol.kind,
                range: symbol.range,
                container_name,
            });
        }
    }
}

pub(crate) fn collapse_newlines(text: &str, separator: &str) -> String {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).join(separator)
}

/// Whether `path` is the worktree's `.ed/workspace.json`, whose `lint` switches servers read on start.
fn is_workspace_state_file(path: &RelPath) -> bool {
    path.as_unix_str() == language::WORKSPACE_STATE_FILE
}

/// The linter servers to restart after the `lint` switches are saved. Saving restarts them even when
/// the section is unchanged: it's a rare, hand-made edit.
fn lint_server_selectors() -> FxHashSet<LanguageServerSelector> {
    language::LINT_SERVERS
        .iter()
        .map(|name| LanguageServerSelector::Name(LanguageServerName::new_static(name)))
        .collect()
}

#[cfg(test)]
mod lint_switch_tests {
    use lsp::LanguageServerName;
    use lsp::LanguageServerSelector;
    use util::rel_path::RelPath;

    use super::is_workspace_state_file;
    use super::lint_server_selectors;

    #[test]
    fn only_the_root_workspace_state_file_matches() {
        assert!(is_workspace_state_file(RelPath::from_unix_str(".ed/workspace.json").unwrap()));
        assert!(!is_workspace_state_file(RelPath::from_unix_str("sub/.ed/workspace.json").unwrap()));
        assert!(!is_workspace_state_file(RelPath::from_unix_str("workspace.json").unwrap()));
    }

    #[test]
    fn restarts_every_lint_server() {
        let selectors = lint_server_selectors();
        for name in ["rust-analyzer", "stylelint", "biome"] {
            assert!(selectors.contains(&LanguageServerSelector::Name(LanguageServerName::new_static(name))));
        }
        assert_eq!(selectors.len(), 3);
    }
}
