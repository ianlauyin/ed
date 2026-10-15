use crate::available_languages::AvailableLanguage;
use crate::{
    CachedLspAdapter, File, Language, LanguageConfig, LanguageId, LanguageMatcher,
    LanguageServerName, LspAdapter, ManifestName, PLAIN_TEXT, ToolchainLister,
    available_languages::AvailableLanguages, language_settings::all_language_settings,
    task_context::ContextProvider,
};
use anyhow::{Result, anyhow};
use collections::{FxHashMap, HashMap, HashSet, hash_map};
pub use language_core::{
    BinaryStatus, LanguageName, LanguageQueries, LanguageServerStatusUpdate, QueryFile,
    QueryFileContents, QueryFiles, ServerHealth,
};
use settings::{AllLanguageSettingsContent, LanguageSettingsContent};

use futures::{
    Future,
    channel::{mpsc, oneshot},
    future::{BoxFuture, FutureExt as _},
};
use globset::GlobSet;
use gpui::{App, BackgroundExecutor, EntityId, Subscription};
use lsp::LanguageServerId;
use parking_lot::{Mutex, RwLock};
use postage::watch;
use smallvec::SmallVec;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use text::Rope;
use theme::Theme;

use util::post_inc;

pub struct LanguageRegistry {
    state: RwLock<LanguageRegistryState>,
    language_server_download_dir: Option<Arc<Path>>,
    executor: BackgroundExecutor,
    lsp_binary_status_tx: ServerStatusSender,
}

struct LanguageRegistryState {
    next_language_server_id: usize,
    languages: Vec<Arc<Language>>,
    language_settings: AllLanguageSettingsContent,
    available_languages: AvailableLanguages,
    grammars: HashMap<Arc<str>, tree_sitter::Language>,
    lsp_adapters: HashMap<LanguageName, Vec<Arc<CachedLspAdapter>>>,
    all_lsp_adapters: HashMap<LanguageServerName, Arc<CachedLspAdapter>>,
    available_lsp_adapters:
        HashMap<LanguageServerName, Arc<dyn Fn() -> Arc<CachedLspAdapter> + 'static + Send + Sync>>,
    loading_languages: HashMap<LanguageId, Vec<oneshot::Sender<Result<Arc<Language>>>>>,
    subscription: (watch::Sender<()>, watch::Receiver<()>),
    theme: Option<Arc<Theme>>,
    version: usize,
    reload_count: usize,
}

#[derive(Debug)]
pub struct LanguageNotFound;

impl std::fmt::Display for LanguageNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "language not found")
    }
}

type ServerStatus = (Option<EntityId>, LanguageServerName, BinaryStatus);

#[derive(Clone, Default)]
struct ServerStatusSender {
    state: Arc<Mutex<ServerStatusSenderState>>,
}

#[derive(Default)]
struct ServerStatusSenderState {
    next_subscription_id: usize,
    txs: HashMap<usize, mpsc::UnboundedSender<ServerStatus>>,
}

pub struct LoadedLanguage {
    pub config: LanguageConfig,
    pub queries: LanguageQueries,
    pub context_provider: Option<Arc<dyn ContextProvider>>,
    pub toolchain_provider: Option<Arc<dyn ToolchainLister>>,
    pub manifest_name: Option<ManifestName>,
}

pub type LanguageLoader =
    Arc<dyn Fn() -> BoxFuture<'static, Result<LoadedLanguage>> + Send + Sync + 'static>;

impl LanguageRegistry {
    pub fn new(executor: BackgroundExecutor) -> Self {
        let this = Self {
            state: RwLock::new(LanguageRegistryState {
                next_language_server_id: 0,
                languages: Vec::new(),
                available_languages: AvailableLanguages::default(),
                grammars: Default::default(),
                language_settings: Default::default(),
                loading_languages: Default::default(),
                lsp_adapters: Default::default(),
                all_lsp_adapters: Default::default(),
                available_lsp_adapters: HashMap::default(),
                subscription: watch::channel(),
                theme: Default::default(),
                version: 0,
                reload_count: 0,
            }),
            language_server_download_dir: None,
            lsp_binary_status_tx: Default::default(),
            executor,
        };
        this.add(PLAIN_TEXT.clone());
        this
    }

    /// Clears out all of the loaded languages and reload them from scratch.
    pub fn reload(&self) {
        self.state.write().reload();
    }

    /// Reorders the list of language servers for the given language.
    ///
    /// Uses the provided list of ordered [`CachedLspAdapters`] as the desired order.
    ///
    /// Any existing language servers not present in `ordered_lsp_adapters` will be
    /// appended to the end.
    pub fn reorder_language_servers(
        &self,
        language: &LanguageName,
        ordered_lsp_adapters: Vec<Arc<CachedLspAdapter>>,
    ) {
        self.state
            .write()
            .reorder_language_servers(language, ordered_lsp_adapters);
    }

    /// Registers an available language server adapter.
    ///
    /// The language server is registered under the language server name, but
    /// not bound to a particular language.
    ///
    /// When a language wants to load this particular language server, it will
    /// invoke the `load` function.
    pub fn register_available_lsp_adapter(
        &self,
        name: LanguageServerName,
        adapter: Arc<dyn LspAdapter>,
    ) {
        let mut state = self.state.write();

        state.available_lsp_adapters.insert(
            name,
            Arc::new(move || CachedLspAdapter::new(adapter.clone())),
        );
    }

    /// Loads the language server adapter for the language server with the given name.
    pub fn load_available_lsp_adapter(
        &self,
        name: &LanguageServerName,
    ) -> Option<Arc<CachedLspAdapter>> {
        let state = self.state.read();
        let load_lsp_adapter = state.available_lsp_adapters.get(name)?;

        Some(load_lsp_adapter())
    }

    /// Checks if a language server adapter with the given name is available to be loaded.
    pub fn is_lsp_adapter_available(&self, name: &LanguageServerName) -> bool {
        let state = self.state.read();
        state.available_lsp_adapters.contains_key(name)
    }

    /// Returns the names of all available LSP adapters (registered via `register_available_lsp_adapter`).
    /// These are adapters that are not bound to a specific language but can be enabled via settings.
    pub fn available_lsp_adapter_names(&self) -> Vec<LanguageServerName> {
        self.state
            .read()
            .available_lsp_adapters
            .keys()
            .cloned()
            .collect()
    }

    pub fn register_lsp_adapter(&self, language_name: LanguageName, adapter: Arc<dyn LspAdapter>) {
        let mut state = self.state.write();

        let cached = CachedLspAdapter::new(adapter);
        state
            .lsp_adapters
            .entry(language_name)
            .or_default()
            .push(cached.clone());
        state
            .all_lsp_adapters
            .insert(cached.name.clone(), cached.clone());
        state.version += 1;
        *state.subscription.0.borrow_mut() = ();
    }

    /// Adds a language to the registry, which can be loaded if needed.
    pub fn register_language(
        &self,
        name: LanguageName,
        grammar_name: Option<Arc<str>>,
        matcher: Arc<LanguageMatcher>,
        hidden: bool,
        manifest_name: Option<ManifestName>,
        load: LanguageLoader,
    ) {
        let state = &mut *self.state.write();

        let was_loaded = state.available_languages.register(
            name.clone(),
            grammar_name,
            matcher,
            hidden,
            manifest_name,
            load,
        );
        if was_loaded {
            state.languages.retain(|language| language.name() != name);
        }

        state.version += 1;
        state.reload_count += 1;
        *state.subscription.0.borrow_mut() = ();
    }

    /// Adds grammars to the registry. Language configurations reference a grammar by name. The
    /// grammar controls how the source code is parsed.
    pub fn register_native_grammars(
        &self,
        grammars: impl IntoIterator<Item = (impl Into<Arc<str>>, impl Into<tree_sitter::Language>)>,
    ) {
        self.state.write().grammars.extend(
            grammars
                .into_iter()
                .map(|(name, grammar)| (name.into(), grammar.into())),
        );
    }

    pub fn language_settings(&self) -> AllLanguageSettingsContent {
        self.state.read().language_settings.clone()
    }

    pub fn language_names(&self) -> Vec<LanguageName> {
        let state = self.state.read();
        let mut result = state.available_languages.unloaded_language_names();
        result.extend(
            state
                .languages
                .iter()
                .map(|language| language.config.name.clone()),
        );
        result.sort_unstable_by_key(|language_name| language_name.as_ref().to_lowercase());
        result
    }

    pub fn grammar_names(&self) -> Vec<Arc<str>> {
        let state = self.state.read();
        let mut result = state.grammars.keys().cloned().collect::<Vec<_>>();
        result.sort_unstable_by_key(|grammar_name| grammar_name.to_lowercase());
        result
    }

    /// Add a pre-loaded language to the registry.
    pub fn add(&self, language: Arc<Language>) {
        let mut state = self.state.write();
        state.available_languages.add(AvailableLanguage {
            id: language.id,
            name: language.name(),
            grammar: language.config.grammar.clone(),
            matcher: language.config.matcher.clone(),
            hidden: language.config.hidden,
            manifest_name: None,
            load: Arc::new(|| async { Err(anyhow!("already loaded")) }.boxed()),
            loaded: true,
        });
        state.add(language);
    }

    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.state.read().subscription.1.clone()
    }

    /// Returns the number of times that the registry has been changed,
    /// by adding languages or reloading.
    pub fn version(&self) -> usize {
        self.state.read().version
    }

    /// Returns the number of times that the registry has been reloaded.
    pub fn reload_count(&self) -> usize {
        self.state.read().reload_count
    }

    pub fn set_theme(&self, theme: Arc<Theme>) {
        let mut state = self.state.write();
        state.theme = Some(theme.clone());
        for language in &state.languages {
            language.set_theme(theme.syntax());
        }
    }

    pub fn set_language_server_download_dir(&mut self, path: impl Into<Arc<Path>>) {
        self.language_server_download_dir = Some(path.into());
    }

    pub fn language_for_name(
        self: &Arc<Self>,
        name: &str,
    ) -> impl Future<Output = Result<Arc<Language>>> + use<> {
        let language_id = self.state.read().available_languages.find_by_name(name);
        let rx = self.get_or_load_language(language_id);
        async move { rx.await? }
    }

    pub fn language_name_for_extension(self: &Arc<Self>, extension: &str) -> Option<LanguageName> {
        self.state
            .try_read()
            .and_then(|state| state.available_languages.find_name_by_extension(extension))
    }

    pub fn language_for_name_or_extension(
        self: &Arc<Self>,
        string: &str,
    ) -> impl Future<Output = Result<Arc<Language>>> {
        let language_id = self
            .state
            .read()
            .available_languages
            .find_by_name_or_extension(string);
        let rx = self.get_or_load_language(language_id);
        async move { rx.await? }
    }

    pub fn available_language_for_name(self: &Arc<Self>, name: &str) -> Option<AvailableLanguage> {
        let state = self.state.read();
        state.available_languages.find_by_exact_name(name)
    }

    /// Look up a language by its modeline name (vim filetype or emacs mode).
    ///
    /// This performs a case-insensitive match against:
    /// 1. Explicit modeline aliases defined in the language config
    /// 2. The language's grammar name
    /// 3. The language name itself
    pub fn available_language_for_modeline_name(
        self: &Arc<Self>,
        modeline_name: &str,
    ) -> Option<AvailableLanguage> {
        self.state
            .read()
            .available_languages
            .find_by_modeline_name(modeline_name)
    }

    pub fn language_for_file(
        self: &Arc<Self>,
        file: &Arc<dyn File>,
        content: Option<&Rope>,
        cx: &App,
    ) -> Option<LanguageId> {
        let user_file_types = all_language_settings(Some(file), cx);
        let filename = file
            .path()
            .file_name()
            .unwrap_or_else(|| file.file_name(cx));
        let paths = [Some(file.full_path(cx)), file.file_system_abs_path(cx)];
        let path_style = file.path_style(cx);
        let normalized_paths = paths.each_ref().map(|path| {
            if path_style.is_windows()
                && let Some(path) = path.as_deref().and_then(Path::to_str)
                && path.contains('\\')
            {
                Some(PathBuf::from(path.replace('\\', "/")))
            } else {
                None
            }
        });
        let paths = paths
            .iter()
            .filter_map(Option::as_deref)
            .chain(normalized_paths.iter().filter_map(Option::as_deref))
            .collect::<SmallVec<[_; 4]>>();
        self.language_for_file_internal(
            Some(filename),
            &paths,
            content,
            Some(&user_file_types.file_types),
        )
    }

    pub fn language_for_file_path(self: &Arc<Self>, path: &Path) -> Option<LanguageId> {
        self.language_for_file_internal(
            path.file_name().and_then(|filename| filename.to_str()),
            &[path],
            None,
            None,
        )
    }

    pub fn load_language_for_file_path<'a>(
        self: &Arc<Self>,
        path: &'a Path,
    ) -> impl Future<Output = Result<Arc<Language>>> + 'a {
        let language = self.language_for_file_path(path);

        let this = self.clone();
        async move {
            if let Some(language_id) = language {
                this.load_language(language_id).await?
            } else {
                Err(anyhow!(LanguageNotFound))
            }
        }
    }

    fn language_for_file_internal(
        self: &Arc<Self>,
        filename: Option<&str>,
        paths: &[&Path],
        content: Option<&Rope>,
        user_file_types: Option<&FxHashMap<Arc<str>, (GlobSet, Vec<String>)>>,
    ) -> Option<LanguageId> {
        self.state.read().available_languages.find_for_file(
            filename,
            paths,
            content,
            user_file_types,
        )
    }

    pub fn load_language(
        self: &Arc<Self>,
        language_id: LanguageId,
    ) -> oneshot::Receiver<Result<Arc<Language>>> {
        let (tx, rx) = oneshot::channel();

        let mut state = self.state.write();

        // If the language is already loaded, resolve with it immediately.
        for loaded_language in state.languages.iter() {
            if loaded_language.id == language_id {
                tx.send(Ok(loaded_language.clone())).unwrap();
                return rx;
            }
        }

        let Some(available_language) = state.available_languages.get_language(language_id) else {
            tx.send(Err(anyhow!(LanguageNotFound))).ok();
            return rx;
        };

        let (language_name, language_load) = (
            available_language.name.clone(),
            available_language.load.clone(),
        );

        match state.loading_languages.entry(language_id) {
            // If the language is already being loaded, then add this
            // channel to a list that will be sent to when the load completes.
            hash_map::Entry::Occupied(mut entry) => entry.get_mut().push(tx),

            // Otherwise, start loading the language.
            hash_map::Entry::Vacant(entry) => {
                let this = self.clone();

                self.executor
                    .spawn(async move {
                        let language = async {
                            let loaded_language = (language_load)().await?;
                            if let Some(grammar) = loaded_language.config.grammar.clone() {
                                let grammar = Some(this.get_or_load_grammar(grammar).await?);

                                Language::new_with_id(language_id, loaded_language.config, grammar)
                                    .with_context_provider(loaded_language.context_provider)
                                    .with_toolchain_lister(loaded_language.toolchain_provider)
                                    .with_manifest(loaded_language.manifest_name)
                                    .with_queries(loaded_language.queries)
                            } else {
                                Ok(
                                    Language::new_with_id(
                                        language_id,
                                        loaded_language.config,
                                        None,
                                    )
                                    .with_context_provider(loaded_language.context_provider)
                                    .with_manifest(loaded_language.manifest_name)
                                    .with_toolchain_lister(loaded_language.toolchain_provider),
                                )
                            }
                        }
                        .await;

                        let language = language.map(Arc::new);
                        if let Err(error) = &language {
                            log::error!("failed to load language {language_name}:\n{error:?}");
                        }
                        let stale_txs = {
                            let mut state = this.state.write();
                            let is_current = state
                                .available_languages
                                .get_language(language_id)
                                .is_some();
                            if is_current {
                                if let Ok(language) = &language {
                                    state.add(language.clone());
                                }
                                state.mark_language_loaded(language_id);
                                if let Some(txs) = state.loading_languages.remove(&language_id) {
                                    for tx in txs {
                                        let _ = tx.send(match &language {
                                            Ok(language) => Ok(language.clone()),
                                            Err(error) => Err(anyhow!(
                                                "failed to load language {language_name}: {error}"
                                            )),
                                        });
                                    }
                                }
                                None
                            } else {
                                let txs = state
                                    .loading_languages
                                    .remove(&language_id)
                                    .unwrap_or_default();
                                let replacement_id = state
                                    .available_languages
                                    .find_by_exact_name(language_name.0.as_ref())
                                    .map(|language| language.id());
                                Some((txs, replacement_id))
                            }
                        };
                        if let Some((txs, replacement_id)) = stale_txs
                            && !txs.is_empty()
                        {
                            match replacement_id {
                                Some(replacement_id) => {
                                    let result = match this.load_language(replacement_id).await {
                                        Ok(result) => result,
                                        Err(_) => Err(anyhow!(LanguageNotFound)),
                                    };
                                    match result {
                                        Ok(replacement_language) => {
                                            for tx in txs {
                                                let _ = tx.send(Ok(replacement_language.clone()));
                                            }
                                        }
                                        Err(error) => {
                                            let message = format!("{error:#}");
                                            for tx in txs {
                                                let _ = tx.send(Err(anyhow!(
                                                    "failed to load language {language_name}: {message}"
                                                )));
                                            }
                                        }
                                    }
                                }
                                None => {
                                    for tx in txs {
                                        let _ = tx.send(Err(anyhow!(LanguageNotFound)));
                                    }
                                }
                            }
                        }
                    })
                    .detach();

                entry.insert(vec![tx]);
            }
        }

        drop(state);
        rx
    }

    fn get_or_load_language(
        self: &Arc<Self>,
        language_id: Option<LanguageId>,
    ) -> oneshot::Receiver<Result<Arc<Language>>> {
        let Some(language_id) = language_id else {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Err(anyhow!(LanguageNotFound)));
            return rx;
        };

        self.load_language(language_id)
    }

    fn get_or_load_grammar(
        self: &Arc<Self>,
        name: Arc<str>,
    ) -> impl Future<Output = Result<tree_sitter::Language>> {
        let grammar = self
            .state
            .read()
            .grammars
            .get(name.as_ref())
            .cloned()
            .ok_or_else(|| anyhow!("no such grammar {name}"));
        async move { grammar }
    }

    pub fn to_vec(&self) -> Vec<Arc<Language>> {
        self.state.read().languages.to_vec()
    }

    pub fn lsp_adapters(&self, language_name: &LanguageName) -> Vec<Arc<CachedLspAdapter>> {
        self.state
            .read()
            .lsp_adapters
            .get(language_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn lsp_adapter(
        &self,
        language_name: &LanguageName,
        server_name: &LanguageServerName,
    ) -> Option<Arc<CachedLspAdapter>> {
        self.state
            .read()
            .lsp_adapters
            .get(language_name)?
            .iter()
            .find(|adapter| adapter.name() == *server_name)
            .cloned()
    }

    pub fn all_lsp_adapters(&self) -> Vec<Arc<CachedLspAdapter>> {
        self.state
            .read()
            .all_lsp_adapters
            .values()
            .cloned()
            .collect()
    }

    pub fn adapter_for_name(&self, name: &LanguageServerName) -> Option<Arc<CachedLspAdapter>> {
        self.state.read().all_lsp_adapters.get(name).cloned()
    }

    pub fn update_lsp_binary_status(&self, server_name: LanguageServerName, status: BinaryStatus) {
        self.lsp_binary_status_tx.send(None, server_name, status);
    }

    pub fn update_lsp_binary_status_for_entity(
        &self,
        source: EntityId,
        server_name: LanguageServerName,
        status: BinaryStatus,
    ) {
        self.lsp_binary_status_tx
            .send(Some(source), server_name, status);
    }

    pub fn next_language_server_id(&self) -> LanguageServerId {
        self.state.write().next_language_server_id()
    }

    pub fn language_server_download_dir(&self, name: &LanguageServerName) -> Option<Arc<Path>> {
        self.language_server_download_dir
            .as_ref()
            .map(|dir| Arc::from(dir.join(name.0.as_ref())))
    }

    pub fn language_server_binary_statuses(
        &self,
    ) -> (
        mpsc::UnboundedReceiver<(Option<EntityId>, LanguageServerName, BinaryStatus)>,
        Subscription,
    ) {
        self.lsp_binary_status_tx.subscribe()
    }
}

impl LanguageRegistryState {
    fn next_language_server_id(&mut self) -> LanguageServerId {
        LanguageServerId(post_inc(&mut self.next_language_server_id))
    }

    fn add(&mut self, language: Arc<Language>) {
        if let Some(theme) = self.theme.as_ref() {
            language.set_theme(theme.syntax());
        }
        self.language_settings.languages.0.insert(
            language.name().0.to_string(),
            LanguageSettingsContent {
                tab_size: language.config.tab_size,
                hard_tabs: language.config.hard_tabs,
                soft_wrap: language.config.soft_wrap.map(crate::to_settings_soft_wrap),
                auto_indent_on_paste: language.config.auto_indent_on_paste,
                ..Default::default()
            },
        );
        self.languages.push(language);
        self.version += 1;
        *self.subscription.0.borrow_mut() = ();
    }

    fn reload(&mut self) {
        self.languages.clear();
        self.version += 1;
        self.reload_count += 1;
        self.available_languages.mark_all_unloaded();
        *self.subscription.0.borrow_mut() = ();
    }

    /// Reorders the list of language servers for the given language.
    ///
    /// Uses the provided list of ordered [`CachedLspAdapters`] as the desired order.
    ///
    /// Any existing language servers not present in `ordered_lsp_adapters` will be
    /// appended to the end.
    fn reorder_language_servers(
        &mut self,
        language_name: &LanguageName,
        ordered_lsp_adapters: Vec<Arc<CachedLspAdapter>>,
    ) {
        let Some(lsp_adapters) = self.lsp_adapters.get_mut(language_name) else {
            return;
        };

        let ordered_lsp_adapter_ids = ordered_lsp_adapters
            .iter()
            .map(|lsp_adapter| lsp_adapter.name.clone())
            .collect::<HashSet<_>>();

        let mut new_lsp_adapters = ordered_lsp_adapters;
        for adapter in lsp_adapters.iter() {
            if !ordered_lsp_adapter_ids.contains(&adapter.name) {
                new_lsp_adapters.push(adapter.clone());
            }
        }

        *lsp_adapters = new_lsp_adapters;
    }

    /// Mark the given language as having been loaded, so that the
    /// language registry won't try to load it again.
    fn mark_language_loaded(&mut self, id: LanguageId) {
        self.available_languages.mark_loaded(id);
    }
}

impl ServerStatusSender {
    fn subscribe(&self) -> (mpsc::UnboundedReceiver<ServerStatus>, Subscription) {
        let (tx, rx) = mpsc::unbounded();
        let subscription_id = {
            let mut state = self.state.lock();
            let subscription_id = post_inc(&mut state.next_subscription_id);
            state.txs.insert(subscription_id, tx);
            subscription_id
        };
        let state = self.state.clone();
        let subscription = Subscription::new(move || {
            state.lock().txs.remove(&subscription_id);
        });
        (rx, subscription)
    }

    fn send(&self, source: Option<EntityId>, name: LanguageServerName, status: BinaryStatus) {
        let mut state = self.state.lock();
        state.txs.retain(|_, tx| {
            tx.unbounded_send((source, name.clone(), status.clone()))
                .is_ok()
        });
    }
}
