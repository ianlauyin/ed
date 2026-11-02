//! The `language` crate provides a large chunk of ed's language-related
//! features (the other big contributors being project and lsp crates that revolve around LSP features).
//! Namely, this crate:
//! - Provides [`Language`], [`Grammar`] and [`LanguageRegistry`] types that
//!   use Tree-sitter to provide syntax highlighting to the editor; note though that `language` doesn't perform the highlighting by itself. It only maps ranges in a buffer to colors. Treesitter is also used for buffer outlines (lists of symbols in a buffer)
//! - Exposes [`LanguageConfig`] that describes how constructs (like brackets or line comments) should be handled by the editor for a source file of a particular language.
//!
//! Notably we do *not* assign a single language to a single file; in real world a single file can consist of multiple programming languages - HTML is a good example of that - and `language` crate tends to reflect that status quo in its API.
mod available_languages;
mod buffer;
mod diagnostic;
mod diagnostic_set;
mod fallback_highlight;
mod language_registry;

pub mod language_settings;
mod library;
mod manifest;
pub mod modeline;
mod outline;
pub mod proto;
mod runnable;
mod syntax_map;
mod task_context;
mod text_diff;

use std::ffi::OsStr;
use std::fmt::Debug;
use std::hash::Hash;
use std::iter;
use std::mem;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::Result;
use async_trait::async_trait;
pub use available_languages::AvailableLanguage;
pub use buffer::Operation;
pub use buffer::*;
use collections::HashMap;
use collections::HashSet;
pub use diagnostic::Diagnostic;
pub use diagnostic::DiagnosticMessage;
pub use diagnostic::DiagnosticSourceKind;
pub use diagnostic::RelatedInformation;
pub use diagnostic::RelatedLocation;
pub use diagnostic_set::DiagnosticEntry;
pub use diagnostic_set::DiagnosticEntryRef;
pub use diagnostic_set::DiagnosticGroup;
pub use fallback_highlight::FallbackHighlighter;
use gpui::App;
use gpui::AsyncApp;
use gpui::Entity;
use gpui::EntityId;
use gpui::SharedString;
pub use language_core::BlockCommentConfig;
pub use language_core::BracketPair;
pub use language_core::BracketPairConfig;
pub use language_core::BracketPairContent;
pub use language_core::BracketsConfig;
pub use language_core::BracketsPatternConfig;
pub use language_core::CodeLabel;
pub use language_core::CodeLabelBuilder;
pub use language_core::DecreaseIndentConfig;
pub use language_core::Grammar;
pub use language_core::GrammarId;
pub use language_core::HighlightsConfig;
pub use language_core::IndentConfig;
pub use language_core::InjectionConfig;
pub use language_core::InjectionPatternConfig;
pub use language_core::JsxTagAutoCloseConfig;
pub use language_core::LanguageConfig;
pub use language_core::LanguageConfigOverride;
pub use language_core::LanguageId;
pub use language_core::LanguageMatcher;
pub use language_core::OrderedListConfig;
pub use language_core::OutlineConfig;
pub use language_core::Override;
pub use language_core::OverrideConfig;
pub use language_core::OverrideEntry;
pub use language_core::RunnableCapture;
pub use language_core::RunnableConfig;
pub use language_core::SoftWrap;
pub use language_core::Symbol;
pub use language_core::SymbolKind;
pub use language_core::TaskListConfig;
pub use language_core::TextObject;
pub use language_core::TextObjectConfig;
pub use language_core::WrapCharactersConfig;
pub use language_core::deserialize_regex;
pub use language_core::deserialize_regex_vec;
use language_core::highlight_cache::MAX_TEXT_HIGHLIGHT_ENTRY_BYTES;
pub use language_core::highlight_cache::ResolvedHighlights;
use language_core::highlight_cache::TextHighlightKey;
pub use language_core::highlight_map::CaptureId;
use language_core::highlight_map::HighlightCaptureRef;
pub use language_core::highlight_map::HighlightId;
pub use language_core::highlight_map::HighlightMap;
pub use language_core::serialize_regex;
pub use language_registry::BinaryStatus;
pub use language_registry::LanguageLoader;
pub use language_registry::LanguageName;
pub use language_registry::LanguageNotFound;
pub use language_registry::LanguageQueries;
pub use language_registry::LanguageRegistry;
pub use language_registry::LanguageServerStatusUpdate;
pub use language_registry::LoadedLanguage;
pub use language_registry::QueryFile;
pub use language_registry::QueryFileContents;
pub use language_registry::QueryFiles;
pub use language_registry::ServerHealth;
pub use library::LibraryProvider;
pub use library::LibraryRoot;
use lsp::CodeActionKind;
use lsp::InitializeParams;
use lsp::LanguageServerBinary;
pub use lsp::LanguageServerId;
pub use lsp::LanguageServerName;
use lsp::Uri;
pub use manifest::ManifestDelegate;
pub use manifest::ManifestName;
pub use manifest::ManifestProvider;
pub use manifest::ManifestQuery;
pub use modeline::ModelineSettings;
pub use modeline::parse_modeline;
pub use outline::*;
use parking_lot::Mutex;
use regex::Regex;
pub use runnable::ResolvedRunnable;
pub use runnable::RunnableMatchCapture;
pub use runnable::RunnableRange;
pub use runnable::RunnableResolver;
use serde_json::Value;
use settings::WorktreeId;
pub use syntax_map::OwnedSyntaxLayer;
use syntax_map::QueryCursorHandle;
pub use syntax_map::SyntaxLayer;
pub use syntax_map::SyntaxMapMatches;
use syntax_map::TextProvider;
pub use syntax_map::ToTreeSitterPoint;
pub use syntax_map::TreeSitterOptions;
use syntax_map::flatten_capture_regions;
use task::RunnableTag;
pub use task_context::ContextLocation;
pub use task_context::ContextProvider;
pub use text::AnchorRangeExt;
pub use text::LineEnding;
pub use text_diff::DiffOptions;
pub use text_diff::apply_diff_patch;
pub use text_diff::apply_reversed_diff_patch;
pub use text_diff::char_diff;
pub use text_diff::line_diff;
pub use text_diff::text_diff;
pub use text_diff::text_diff_with_options;
pub use text_diff::unified_diff;
pub use text_diff::unified_diff_with_context;
pub use text_diff::unified_diff_with_offsets;
pub use text_diff::word_diff_ranges;
use theme::SyntaxTheme;
pub use tree_sitter::Node;
pub use tree_sitter::Parser;
pub use tree_sitter::QueryCapture;
use tree_sitter::QueryCursor;
use tree_sitter::StreamingIterator as _;
pub use tree_sitter::Tree;
pub use tree_sitter::TreeCursor;
use tree_sitter::{self};
use util::rel_path::RelPath;

pub use crate::language_settings::AutoIndentMode;
pub use crate::language_settings::IndentGuideSettings;

pub(crate) const fn to_settings_soft_wrap(value: language_core::SoftWrap) -> settings::SoftWrap {
    match value {
        language_core::SoftWrap::None => settings::SoftWrap::None,
        language_core::SoftWrap::PreferLine => settings::SoftWrap::PreferLine,
        language_core::SoftWrap::EditorWidth => settings::SoftWrap::EditorWidth,
        language_core::SoftWrap::Bounded => settings::SoftWrap::Bounded,
    }
}

static QUERY_CURSORS: Mutex<Vec<QueryCursor>> = Mutex::new(vec![]);
static PARSERS: Mutex<Vec<Parser>> = Mutex::new(vec![]);

pub fn with_parser<F, R>(func: F) -> R
where
    F: FnOnce(&mut Parser) -> R,
{
    let mut parser = PARSERS.lock().pop().unwrap_or_default();
    // Tree-sitter auto-resets the parser at the end of a successful parse,
    // but the cancellation paths (progress callback returning `Break`,
    // cancelled balancing) leave outstanding state on the parser. The next
    // call to `parse_with_options` would then *resume* that cancelled parse
    // instead of starting fresh.
    parser.reset();
    parser.set_included_ranges(&[]).unwrap();
    let result = func(&mut parser);
    PARSERS.lock().push(parser);
    result
}

pub fn with_query_cursor<F, R>(func: F) -> R
where
    F: FnOnce(&mut QueryCursor) -> R,
{
    let mut cursor = QueryCursorHandle::new();
    func(&mut cursor)
}

/// A shared grammar for plain text, exposed for reuse by downstream crates.
pub static PLAIN_TEXT: LazyLock<Arc<Language>> = LazyLock::new(|| {
    Arc::new(Language::new(
        LanguageConfig {
            name: "Plain Text".into(),
            soft_wrap: Some(SoftWrap::EditorWidth),
            autoclose_before: ")]}".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec![SharedString::new_static("txt")],
                first_line_pattern: None,
                modeline_aliases: vec![SharedString::new_static("text"), SharedString::new_static("txt")],
            })
            .into(),
            brackets: BracketPairConfig {
                pairs: vec![
                    BracketPair {
                        start: "(".to_string(),
                        end: ")".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "[".to_string(),
                        end: "]".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "{".to_string(),
                        end: "}".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "\"".to_string(),
                        end: "\"".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                    BracketPair {
                        start: "'".to_string(),
                        end: "'".to_string(),
                        close: true,
                        surround: true,
                        newline: false,
                    },
                ],
                disabled_scopes_by_bracket_ix: Vec::default(),
            },
            ..Default::default()
        },
        None,
    ))
});

pub const fn lsp_to_symbol_kind(kind: lsp::SymbolKind) -> SymbolKind {
    match kind {
        lsp::SymbolKind::FILE => SymbolKind::File,
        lsp::SymbolKind::MODULE => SymbolKind::Module,
        lsp::SymbolKind::NAMESPACE => SymbolKind::Namespace,
        lsp::SymbolKind::PACKAGE => SymbolKind::Package,
        lsp::SymbolKind::CLASS => SymbolKind::Class,
        lsp::SymbolKind::METHOD => SymbolKind::Method,
        lsp::SymbolKind::PROPERTY => SymbolKind::Property,
        lsp::SymbolKind::FIELD => SymbolKind::Field,
        lsp::SymbolKind::CONSTRUCTOR => SymbolKind::Constructor,
        lsp::SymbolKind::ENUM => SymbolKind::Enum,
        lsp::SymbolKind::INTERFACE => SymbolKind::Interface,
        lsp::SymbolKind::FUNCTION => SymbolKind::Function,
        lsp::SymbolKind::VARIABLE => SymbolKind::Variable,
        lsp::SymbolKind::CONSTANT => SymbolKind::Constant,
        lsp::SymbolKind::STRING => SymbolKind::String,
        lsp::SymbolKind::NUMBER => SymbolKind::Number,
        lsp::SymbolKind::BOOLEAN => SymbolKind::Boolean,
        lsp::SymbolKind::ARRAY => SymbolKind::Array,
        lsp::SymbolKind::OBJECT => SymbolKind::Object,
        lsp::SymbolKind::KEY => SymbolKind::Key,
        lsp::SymbolKind::NULL => SymbolKind::Null,
        lsp::SymbolKind::ENUM_MEMBER => SymbolKind::EnumMember,
        lsp::SymbolKind::STRUCT => SymbolKind::Struct,
        lsp::SymbolKind::EVENT => SymbolKind::Event,
        lsp::SymbolKind::OPERATOR => SymbolKind::Operator,
        lsp::SymbolKind::TYPE_PARAMETER => SymbolKind::TypeParameter,
        _ => SymbolKind::Null,
    }
}

/// Commands that the client (editor) handles locally rather than forwarding
/// to the language server. Servers embed these in code lens and code action
/// responses when they want the editor to perform a well-known UI action.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    /// Open a location list (references panel / peek view).
    ShowLocations,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub buffer: Entity<Buffer>,
    pub range: Range<Anchor>,
}

/// Context provided to LSP adapters when a user responds to a ShowMessageRequest prompt.
/// This allows adapters to intercept preference selections (like "Always" or "Never")
/// and potentially persist them to ed's settings.
#[derive(Debug, Clone)]
pub struct PromptResponseContext {
    /// The original message shown to the user
    pub message: String,
    /// The action (button) the user selected
    pub selected_action: lsp::MessageActionItem,
}

/// Represents a Language Server, with certain cached sync properties.
/// Uses [`LspAdapter`] under the hood, but calls all 'static' methods
/// once at startup, and caches the results.
pub struct CachedLspAdapter {
    pub name: LanguageServerName,
    pub disk_based_diagnostic_sources: Vec<String>,
    pub disk_based_diagnostics_progress_token: Option<String>,
    language_ids: HashMap<LanguageName, String>,
    pub adapter: Arc<dyn LspAdapter>,
}

impl Debug for CachedLspAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedLspAdapter")
            .field("name", &self.name)
            .field("disk_based_diagnostic_sources", &self.disk_based_diagnostic_sources)
            .field("disk_based_diagnostics_progress_token", &self.disk_based_diagnostics_progress_token)
            .field("language_ids", &self.language_ids)
            .finish_non_exhaustive()
    }
}

impl CachedLspAdapter {
    pub fn new(adapter: Arc<dyn LspAdapter>) -> Arc<Self> {
        let name = adapter.name();
        let disk_based_diagnostic_sources = adapter.disk_based_diagnostic_sources();
        let disk_based_diagnostics_progress_token = adapter.disk_based_diagnostics_progress_token();
        let language_ids = adapter.language_ids();

        Arc::new(CachedLspAdapter {
            name,
            disk_based_diagnostic_sources,
            disk_based_diagnostics_progress_token,
            language_ids,
            adapter,
        })
    }

    pub fn name(&self) -> LanguageServerName {
        self.adapter.name()
    }

    pub async fn find_binary(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        cx: &AsyncApp,
    ) -> Result<LanguageServerBinary> {
        self.adapter.find_binary(delegate, cx).await
    }

    pub fn code_action_kinds(&self) -> Option<Vec<CodeActionKind>> {
        self.adapter.code_action_kinds()
    }

    pub fn process_diagnostics(&self, params: &mut lsp::PublishDiagnosticsParams, server_id: LanguageServerId) {
        self.adapter.process_diagnostics(params, server_id)
    }

    pub fn diagnostic_message_to_markdown(&self, message: &str) -> Option<String> {
        self.adapter.diagnostic_message_to_markdown(message)
    }

    pub async fn process_completions(&self, completion_items: &mut [lsp::CompletionItem]) {
        self.adapter.process_completions(completion_items).await
    }

    pub async fn labels_for_completions(
        &self,
        completion_items: &[lsp::CompletionItem],
        language: &Arc<Language>,
    ) -> Result<Vec<Option<CodeLabel>>> {
        self.adapter.clone().labels_for_completions(completion_items, language).await
    }

    pub async fn labels_for_symbols(
        &self,
        symbols: &[Symbol],
        language: &Arc<Language>,
    ) -> Result<Vec<Option<CodeLabel>>> {
        self.adapter.clone().labels_for_symbols(symbols, language).await
    }

    pub fn language_id(&self, language_name: &LanguageName) -> String {
        self.language_ids.get(language_name).cloned().unwrap_or_else(|| language_name.lsp_id())
    }

    pub fn process_prompt_response(&self, context: &PromptResponseContext, cx: &mut AsyncApp) {
        self.adapter.process_prompt_response(context, cx)
    }
}

/// [`LspAdapterDelegate`] allows [`LspAdapter]` implementations to interface with the application.
// async_trait marks its boxed futures `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait LspAdapterDelegate: Send + Sync {
    fn worktree_id(&self) -> WorktreeId;
    fn worktree_root_path(&self) -> &Path;
    fn resolve_relative_path(&self, path: PathBuf) -> PathBuf;
    fn status_source_id(&self) -> EntityId;
    fn update_status(&self, language: LanguageServerName, status: BinaryStatus);
    fn registered_lsp_adapters(&self) -> Vec<Arc<dyn LspAdapter>>;

    async fn which(&self, command: &OsStr) -> Option<PathBuf>;
    async fn shell_env(&self) -> HashMap<String, String>;
    async fn read_text_file(&self, path: &RelPath) -> Result<String>;
    async fn try_exec(&self, binary: LanguageServerBinary) -> Result<()>;
}

// async_trait marks its boxed futures `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait(?Send)]
pub trait LspAdapter: 'static + Send + Sync {
    fn name(&self) -> LanguageServerName;

    /// Locates the language server binary installed by the user (on `PATH` or in the
    /// project). Language servers are never installed by the editor,
    /// so returning `None` means the server is not available.
    async fn check_if_user_installed(
        &self,
        _delegate: &Arc<dyn LspAdapterDelegate>,
        _cx: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        None
    }

    /// The executable [`Self::check_if_user_installed`] looks for, named in the error
    /// when the server is not installed.
    fn binary_name(&self) -> String {
        self.name().0.to_string()
    }

    /// Finds the binary to start the server with. Adapters override this instead of
    /// [`Self::check_if_user_installed`] to explain why the server can't start.
    async fn find_binary(&self, delegate: &Arc<dyn LspAdapterDelegate>, cx: &AsyncApp) -> Result<LanguageServerBinary> {
        let Some(binary) = self.check_if_user_installed(delegate, cx).await else {
            anyhow::bail!(
                "language server {} is not installed: `{}` was not found on your PATH",
                self.name(),
                self.binary_name(),
            );
        };
        Ok(binary)
    }

    fn process_diagnostics(&self, _: &mut lsp::PublishDiagnosticsParams, _: LanguageServerId) {}

    /// Post-processes completions provided by the language server.
    async fn process_completions(&self, _: &mut [lsp::CompletionItem]) {}

    fn diagnostic_message_to_markdown(&self, _message: &str) -> Option<String> {
        None
    }

    async fn labels_for_completions(
        self: Arc<Self>,
        completions: &[lsp::CompletionItem],
        language: &Arc<Language>,
    ) -> Result<Vec<Option<CodeLabel>>> {
        let mut labels = Vec::new();
        for (ix, completion) in completions.iter().enumerate() {
            let label = self.label_for_completion(completion, language).await;
            if let Some(label) = label {
                labels.resize(ix + 1, None);
                *labels.last_mut().unwrap() = Some(label);
            }
        }
        Ok(labels)
    }

    async fn label_for_completion(&self, _: &lsp::CompletionItem, _: &Arc<Language>) -> Option<CodeLabel> {
        None
    }

    async fn labels_for_symbols(
        self: Arc<Self>,
        symbols: &[Symbol],
        language: &Arc<Language>,
    ) -> Result<Vec<Option<CodeLabel>>> {
        let mut labels = Vec::new();
        for (ix, symbol) in symbols.iter().enumerate() {
            let label = self.label_for_symbol(symbol, language).await;
            if let Some(label) = label {
                labels.resize(ix + 1, None);
                *labels.last_mut().unwrap() = Some(label);
            }
        }
        Ok(labels)
    }

    async fn label_for_symbol(&self, _symbol: &Symbol, _language: &Arc<Language>) -> Option<CodeLabel> {
        None
    }

    /// Returns initialization options that are going to be sent to a LSP server as a part of [`lsp::InitializeParams`]
    async fn initialization_options(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _cx: &mut AsyncApp,
    ) -> Result<Option<Value>> {
        Ok(None)
    }

    async fn workspace_configuration(
        self: Arc<Self>,
        _: &Arc<dyn LspAdapterDelegate>,
        _: Option<Uri>,
        _cx: &mut AsyncApp,
    ) -> Result<Value> {
        Ok(serde_json::json!({}))
    }

    async fn additional_initialization_options(
        self: Arc<Self>,
        _target_language_server_id: LanguageServerName,
        _: &Arc<dyn LspAdapterDelegate>,
    ) -> Result<Option<Value>> {
        Ok(None)
    }

    async fn additional_workspace_configuration(
        self: Arc<Self>,
        _target_language_server_id: LanguageServerName,
        _: &Arc<dyn LspAdapterDelegate>,
        _cx: &mut AsyncApp,
    ) -> Result<Option<Value>> {
        Ok(None)
    }

    /// Returns a list of code actions supported by a given LspAdapter
    fn code_action_kinds(&self) -> Option<Vec<CodeActionKind>> {
        None
    }

    fn disk_based_diagnostic_sources(&self) -> Vec<String> {
        Vec::default()
    }

    fn disk_based_diagnostics_progress_token(&self) -> Option<String> {
        None
    }

    fn language_ids(&self) -> HashMap<LanguageName, String> {
        HashMap::default()
    }

    /// Support custom initialize params.
    fn prepare_initialize_params(&self, original: InitializeParams, _: &App) -> Result<InitializeParams> {
        Ok(original)
    }

    fn client_command(&self, _command_name: &str, _arguments: &[serde_json::Value]) -> Option<ClientCommand> {
        None
    }

    /// Called when a user responds to a ShowMessageRequest from this language server.
    /// This allows adapters to intercept preference selections (like "Always" or "Never")
    /// for settings that should be persisted to ed's settings file.
    fn process_prompt_response(&self, _context: &PromptResponseContext, _cx: &mut AsyncApp) {}
}

/// Represents a language for the given range. Some languages (e.g. HTML)
/// interleave several languages together, thus a single buffer might actually contain
/// several nested scopes.
#[derive(Clone, Debug)]
pub struct LanguageScope {
    language: Arc<Language>,
    override_id: Option<u32>,
}

pub struct Language {
    pub(crate) id: LanguageId,
    pub(crate) config: LanguageConfig,
    pub(crate) grammar: Option<Arc<Grammar>>,
    pub(crate) context_provider: Option<Arc<dyn ContextProvider>>,
    pub(crate) manifest_name: Option<ManifestName>,
}

impl Language {
    pub fn new(config: LanguageConfig, ts_language: Option<tree_sitter::Language>) -> Self {
        Self::new_with_id(LanguageId::new(), config, ts_language)
    }

    pub const fn id(&self) -> LanguageId {
        self.id
    }

    fn new_with_id(id: LanguageId, config: LanguageConfig, ts_language: Option<tree_sitter::Language>) -> Self {
        Self {
            id,
            config,
            grammar: ts_language.map(|ts_language| Arc::new(Grammar::new(ts_language))),
            context_provider: None,
            manifest_name: None,
        }
    }

    pub fn with_context_provider(mut self, provider: Option<Arc<dyn ContextProvider>>) -> Self {
        self.context_provider = provider;
        self
    }

    pub fn with_manifest(mut self, name: Option<ManifestName>) -> Self {
        self.manifest_name = name;
        self
    }

    pub fn with_queries(mut self, queries: LanguageQueries) -> Result<Self> {
        if let Some(grammar) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar).map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            let grammar = grammar.with_queries(queries, &mut self.config)?;
            self.grammar = Some(Arc::new(grammar));
        }
        Ok(self)
    }

    pub fn with_highlights_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query(|grammar| grammar.with_highlights_query(source))
    }

    pub fn with_runnable_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query(|grammar| grammar.with_runnable_query(source))
    }

    pub fn with_outline_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_outline_query(source, name))
    }

    pub fn with_text_object_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_text_object_query(source, name))
    }

    pub fn with_brackets_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_brackets_query(source, name))
    }

    pub fn with_indents_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_indents_query(source, name))
    }

    pub fn with_injection_query(self, source: &str) -> Result<Self> {
        self.with_grammar_query_and_name(|grammar, name| grammar.with_injection_query(source, name))
    }

    pub fn with_override_query(mut self, source: &str) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc).map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            let grammar = grammar.with_override_query(
                source,
                &self.config.name,
                &self.config.overrides,
                &mut self.config.brackets,
            )?;
            self.grammar = Some(Arc::new(grammar));
        }
        Ok(self)
    }

    fn with_grammar_query(mut self, build: impl FnOnce(Grammar) -> Result<Grammar>) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc).map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            self.grammar = Some(Arc::new(build(grammar)?));
        }
        Ok(self)
    }

    fn with_grammar_query_and_name(
        mut self,
        build: impl FnOnce(Grammar, &LanguageName) -> Result<Grammar>,
    ) -> Result<Self> {
        if let Some(grammar_arc) = self.grammar.take() {
            let grammar = Arc::try_unwrap(grammar_arc).map_err(|_| anyhow::anyhow!("cannot mutate grammar"))?;
            self.grammar = Some(Arc::new(build(grammar, &self.config.name)?));
        }
        Ok(self)
    }

    pub fn name(&self) -> LanguageName {
        self.config.name.clone()
    }
    pub const fn manifest(&self) -> Option<&ManifestName> {
        self.manifest_name.as_ref()
    }

    pub fn code_fence_block_name(&self) -> Arc<str> {
        self.config.code_fence_block_name.clone().unwrap_or_else(|| self.config.name.as_ref().to_lowercase().into())
    }

    pub fn context_provider(&self) -> Option<Arc<dyn ContextProvider>> {
        self.context_provider.clone()
    }

    pub fn highlight_text<Source: HighlightSource + ?Sized>(
        self: &Arc<Self>,
        text: &Source,
        range: Range<usize>,
    ) -> impl Iterator<Item = (Range<usize>, HighlightId)> + 'static + use<Source> {
        self.highlight_text_resolved(text, 0..text.source_len()).runs_in_range(range)
    }

    pub fn highlight_text_resolved(
        self: &Arc<Self>,
        text: &(impl HighlightSource + ?Sized),
        range: Range<usize>,
    ) -> ResolvedHighlights {
        let Some(grammar) = &self.grammar else {
            return ResolvedHighlights::default();
        };
        let Some(highlights_config) = &grammar.highlights_config else {
            return ResolvedHighlights::default();
        };
        let text_len = text.source_len();
        let highlights = if text_len > MAX_TEXT_HIGHLIGHT_ENTRY_BYTES {
            Self::compute_resolved_highlights(grammar, &highlights_config.query, text)
        } else {
            let key = TextHighlightKey::new(text.source_chunks(), text_len);
            match highlights_config.text_highlight_cache.get(&key, text.source_chunks()) {
                Some(highlights) => highlights,
                None => {
                    let mut cached_text = String::with_capacity(text_len);
                    cached_text.extend(text.source_chunks());
                    highlights_config.text_highlight_cache.insert(
                        key,
                        cached_text,
                        Self::compute_resolved_highlights(grammar, &highlights_config.query, text),
                    )
                }
            }
        };
        if range.start == 0 && range.end >= text_len {
            return highlights;
        }
        ResolvedHighlights { sources: highlights.sources.clone(), runs: highlights.runs_in_range(range).collect() }
    }

    fn compute_resolved_highlights(
        grammar: &Arc<Grammar>,
        query: &tree_sitter::Query,
        text: &(impl HighlightSource + ?Sized),
    ) -> ResolvedHighlights {
        let highlight_map = grammar.highlight_map();
        let text_len = text.source_len();
        let tree = text.parse(grammar);
        let regions = with_query_cursor(|cursor| {
            cursor.set_byte_range(0..text_len);
            let mut captures = cursor.captures(query, tree.root_node(), text.text_provider());
            let capture_refs = iter::from_fn(move || {
                let (query_match, capture_index) = captures.next()?;
                let capture = query_match.captures()[*capture_index];
                Some((
                    capture.node.byte_range(),
                    HighlightCaptureRef { grammar_index: 0, capture_id: CaptureId(capture.index) },
                ))
            });
            flatten_capture_regions(0..text_len, capture_refs)
        });
        let mut runs = Vec::<(Range<usize>, HighlightId)>::new();
        for region in regions {
            let highlight_id = region.stack.iter().rev().find_map(|capture| highlight_map.get(capture.capture_id));
            let Some(highlight_id) = highlight_id else {
                continue;
            };
            match runs.last_mut() {
                Some((last_range, last_highlight_id))
                    if *last_highlight_id == highlight_id && last_range.end == region.range.start =>
                {
                    last_range.end = region.range.end;
                }
                _ => runs.push((region.range, highlight_id)),
            }
        }
        ResolvedHighlights { sources: [(Arc::clone(grammar), highlight_map)].into_iter().collect(), runs: runs.into() }
    }

    pub fn path_suffixes(&self) -> &[SharedString] {
        &self.config.matcher.path_suffixes
    }

    pub fn should_autoclose_before(&self, c: char) -> bool {
        c.is_whitespace() || self.config.autoclose_before.contains(c)
    }

    pub fn set_theme(&self, theme: &SyntaxTheme) {
        if let Some(grammar) = self.grammar.as_ref()
            && let Some(highlights_config) = &grammar.highlights_config
        {
            *grammar.highlight_map.lock() = build_highlight_map(highlights_config.query.capture_names(), theme);
        }
    }

    pub const fn grammar(&self) -> Option<&Arc<Grammar>> {
        self.grammar.as_ref()
    }

    pub fn default_scope(self: &Arc<Self>) -> LanguageScope {
        LanguageScope { language: self.clone(), override_id: None }
    }

    pub fn lsp_id(&self) -> String {
        self.config.name.lsp_id()
    }

    pub fn snippet_scope_id(&self) -> String {
        self.config.name.snippet_scope_id()
    }

    pub const fn config(&self) -> &LanguageConfig {
        &self.config
    }
}

#[inline]
pub fn build_highlight_map(capture_names: &[&str], theme: &SyntaxTheme) -> HighlightMap {
    HighlightMap::from_ids(
        capture_names.iter().map(|capture_name| theme.highlight_id(capture_name).map(HighlightId::new)),
    )
}

impl LanguageScope {
    pub fn path_suffixes(&self) -> &[SharedString] {
        self.language.path_suffixes()
    }

    pub fn language_name(&self) -> LanguageName {
        self.language.config.name.clone()
    }

    pub fn collapsed_placeholder(&self) -> &str {
        self.language.config.collapsed_placeholder.as_ref()
    }

    /// Returns line prefix that is inserted in e.g. line continuations or
    /// in `toggle comments` action.
    pub fn line_comment_prefixes(&self) -> &[Arc<str>] {
        Override::as_option(self.config_override().map(|o| &o.line_comments), Some(&self.language.config.line_comments))
            .map_or([].as_slice(), |e| e.as_slice())
    }

    /// Config for block comments for this language.
    pub fn block_comment(&self) -> Option<&BlockCommentConfig> {
        Override::as_option(
            self.config_override().map(|o| &o.block_comment),
            self.language.config.block_comment.as_ref(),
        )
    }

    /// Config for documentation-style block comments for this language.
    pub fn documentation_comment(&self) -> Option<&BlockCommentConfig> {
        self.language.config.documentation_comment.as_ref()
    }

    /// Returns list markers that are inserted unchanged on newline (e.g., `- `, `* `, `+ `).
    pub fn unordered_list(&self) -> &[Arc<str>] {
        &self.language.config.unordered_list
    }

    /// Returns configuration for ordered lists with auto-incrementing numbers (e.g., `1. ` becomes `2. `).
    pub fn ordered_list(&self) -> &[OrderedListConfig] {
        &self.language.config.ordered_list
    }

    /// Returns configuration for task list continuation, if any (e.g., `- [x] ` continues as `- [ ] `).
    pub fn task_list(&self) -> Option<&TaskListConfig> {
        self.language.config.task_list.as_ref()
    }

    /// Returns additional regex patterns that act as prefix markers for creating
    /// boundaries during rewrapping.
    ///
    /// By default, ed treats as paragraph and comment prefixes as boundaries.
    pub fn rewrap_prefixes(&self) -> &[Regex] {
        &self.language.config.rewrap_prefixes
    }

    /// Returns a list of language-specific word characters.
    ///
    /// By default, ed treats alphanumeric characters (and '_') as word characters for
    /// the purpose of actions like 'move to next word end` or whole-word search.
    /// It additionally accounts for language's additional word characters.
    pub fn word_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override().map(|o| &o.word_characters),
            Some(&self.language.config.word_characters),
        )
    }

    /// Returns a list of language-specific characters that are considered part of
    /// a completion query.
    pub fn completion_query_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override().map(|o| &o.completion_query_characters),
            Some(&self.language.config.completion_query_characters),
        )
    }

    /// Returns a list of language-specific characters that are considered part of
    /// identifiers during linked editing operations.
    pub fn linked_edit_characters(&self) -> Option<&HashSet<char>> {
        Override::as_option(
            self.config_override().map(|o| &o.linked_edit_characters),
            Some(&self.language.config.linked_edit_characters),
        )
    }

    /// Returns whether to prefer snippet `label` over `new_text` to replace text when
    /// completion is accepted.
    ///
    /// In cases like when cursor is in string or renaming existing function,
    /// you don't want to expand function signature instead just want function name
    /// to replace existing one.
    pub fn prefers_label_for_snippet_in_completion(&self) -> bool {
        self.config_override().and_then(|o| o.prefer_label_for_snippet).unwrap_or(false)
    }

    /// Returns a list of bracket pairs for a given language with an additional
    /// piece of information about whether the particular bracket pair is currently active for a given language.
    pub fn brackets(&self) -> impl Iterator<Item = (&BracketPair, bool)> {
        let mut disabled_ids = self.config_override().map_or(&[] as _, |o| o.disabled_bracket_ixs.as_slice());
        self.language.config.brackets.pairs.iter().enumerate().map(move |(ix, bracket)| {
            let mut is_enabled = true;
            if let Some(next_disabled_ix) = disabled_ids.first()
                && ix == *next_disabled_ix as usize
            {
                disabled_ids = &disabled_ids[1..];
                is_enabled = false;
            }
            (bracket, is_enabled)
        })
    }

    pub fn should_autoclose_before(&self, c: char) -> bool {
        c.is_whitespace() || self.language.config.autoclose_before.contains(c)
    }

    pub fn override_name(&self) -> Option<&str> {
        let id = self.override_id?;
        let grammar = self.language.grammar.as_ref()?;
        let override_config = grammar.override_config.as_ref()?;
        override_config.values.get(&id).map(|e| e.name.as_str())
    }

    fn config_override(&self) -> Option<&LanguageConfigOverride> {
        let id = self.override_id?;
        let grammar = self.language.grammar.as_ref()?;
        let override_config = grammar.override_config.as_ref()?;
        override_config.values.get(&id).map(|e| &e.value)
    }
}

impl Hash for Language {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for Language {}

impl Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Language").field("name", &self.config.name).finish()
    }
}

pub trait HighlightSource {
    fn source_len(&self) -> usize;
    fn source_chunks(&self) -> impl Iterator<Item = &str>;
    fn parse(&self, grammar: &Grammar) -> Tree;
    fn text_provider(&self) -> impl tree_sitter::TextProvider<&[u8]> + '_;
}

impl HighlightSource for str {
    fn source_len(&self) -> usize {
        self.len()
    }

    fn source_chunks(&self) -> impl Iterator<Item = &str> {
        iter::once(self)
    }

    fn parse(&self, grammar: &Grammar) -> Tree {
        with_parser(|parser| {
            parser.set_language(&grammar.ts_language).expect("incompatible grammar");
            parser.parse(self, None).unwrap()
        })
    }

    fn text_provider(&self) -> impl tree_sitter::TextProvider<&[u8]> + '_ {
        self.as_bytes()
    }
}

impl HighlightSource for Rope {
    fn source_len(&self) -> usize {
        self.len()
    }

    fn source_chunks(&self) -> impl Iterator<Item = &str> {
        self.chunks()
    }

    fn parse(&self, grammar: &Grammar) -> Tree {
        parse_text(grammar, self, None)
    }

    fn text_provider(&self) -> impl tree_sitter::TextProvider<&[u8]> + '_ {
        TextProvider(self)
    }
}

pub(crate) fn parse_text(grammar: &Grammar, text: &Rope, old_tree: Option<Tree>) -> Tree {
    with_parser(|parser| {
        parser.set_language(&grammar.ts_language).expect("incompatible grammar");
        let mut chunks = text.chunks_in_range(0..text.len());
        parser
            .parse_with_options(
                &mut move |offset, _| {
                    chunks.seek(offset);
                    chunks.peek_bytes().unwrap_or_default()
                },
                old_tree.as_ref(),
                None,
            )
            .unwrap()
    })
}

pub trait CodeLabelExt {
    fn fallback_for_completion(item: &lsp::CompletionItem, language: Option<&Language>) -> CodeLabel;
}

impl CodeLabelExt for CodeLabel {
    fn fallback_for_completion(item: &lsp::CompletionItem, language: Option<&Language>) -> CodeLabel {
        let highlight_id = item.kind.and_then(|kind| {
            let grammar = language?.grammar()?;
            use lsp::CompletionItemKind as Kind;
            match kind {
                Kind::CLASS => grammar.highlight_id_for_name("type"),
                Kind::CONSTANT => grammar.highlight_id_for_name("constant"),
                Kind::CONSTRUCTOR => grammar.highlight_id_for_name("constructor"),
                Kind::ENUM => grammar.highlight_id_for_name("enum").or_else(|| grammar.highlight_id_for_name("type")),
                Kind::ENUM_MEMBER => {
                    grammar.highlight_id_for_name("variant").or_else(|| grammar.highlight_id_for_name("property"))
                }
                Kind::FIELD => grammar.highlight_id_for_name("property"),
                Kind::FUNCTION => grammar.highlight_id_for_name("function"),
                Kind::INTERFACE => grammar.highlight_id_for_name("type"),
                Kind::METHOD => grammar
                    .highlight_id_for_name("function.method")
                    .or_else(|| grammar.highlight_id_for_name("function")),
                Kind::OPERATOR => grammar.highlight_id_for_name("operator"),
                Kind::PROPERTY => grammar.highlight_id_for_name("property"),
                Kind::STRUCT => grammar.highlight_id_for_name("type"),
                Kind::VARIABLE => grammar.highlight_id_for_name("variable"),
                Kind::KEYWORD => grammar.highlight_id_for_name("keyword"),
                _ => None,
            }
        });

        let label = &item.label;
        let label_length = label.len();
        let runs = highlight_id.map(|highlight_id| vec![(0..label_length, highlight_id)]).unwrap_or_default();
        let text = if let Some(detail) = item.detail.as_deref().filter(|detail| detail != label) {
            format!("{label} {detail}")
        } else if let Some(description) = item
            .label_details
            .as_ref()
            .and_then(|label_details| label_details.description.as_deref())
            .filter(|description| description != label)
        {
            format!("{label} {description}")
        } else {
            label.clone()
        };
        let filter_range = item
            .filter_text
            .as_deref()
            .and_then(|filter| text.find(filter).map(|ix| ix..ix + filter.len()))
            .unwrap_or(0..label_length);
        CodeLabel { text, runs, filter_range }
    }
}

pub const fn point_to_lsp(point: PointUtf16) -> lsp::Position {
    lsp::Position::new(point.row, point.column)
}

pub const fn point_from_lsp(point: lsp::Position) -> Unclipped<PointUtf16> {
    Unclipped(PointUtf16::new(point.line, point.character))
}

pub fn range_to_lsp(range: Range<PointUtf16>) -> Result<lsp::Range> {
    anyhow::ensure!(
        range.start <= range.end,
        "Inverted range provided to an LSP request: {:?}-{:?}",
        range.start,
        range.end
    );
    Ok(lsp::Range { start: point_to_lsp(range.start), end: point_to_lsp(range.end) })
}

pub fn range_from_lsp(range: lsp::Range) -> Range<Unclipped<PointUtf16>> {
    let mut start = point_from_lsp(range.start);
    let mut end = point_from_lsp(range.end);
    if start > end {
        // Not logged: completions produce enough of these to hang the main thread on log writes (#36223).
        mem::swap(&mut start, &mut end);
    }
    start..end
}
