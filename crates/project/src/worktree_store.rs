use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use collections::HashMap;
use collections::HashSet;
use fs::Fs;
use fs::copy_recursive;
use futures::FutureExt;
use futures::future::Shared;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::Context;
use gpui::Entity;
use gpui::EntityId;
use gpui::EventEmitter;
use gpui::Global;
use gpui::Task;
use gpui::TaskExt;
use gpui::WeakEntity;
use itertools::Either;
use rpc::AnyProtoClient;
use rpc::ErrorExt;
use rpc::TypedEnvelope;
use rpc::proto::REMOTE_SERVER_PROJECT_ID;
use rpc::proto::{self};
use text::ReplicaId;
use util::ResultExt;
use util::path_list::PathList;
use util::paths::PathStyle;
use util::paths::RemotePathBuf;
use util::paths::SanitizedPath;
use util::rel_path::RelPath;
use worktree::CreatedEntry;
use worktree::Entry;
use worktree::ProjectEntryId;
use worktree::UpdatedEntriesSet;
use worktree::UpdatedGitRepositoriesSet;
use worktree::Worktree;
use worktree::WorktreeId;

use crate::ProjectPath;
use crate::library::library_root;
use crate::trusted_worktrees::TrustedWorktrees;

/// The current paths for a project's worktrees. Each folder path has a corresponding
/// main worktree path at the same position. The two lists are always the
/// same length and are modified together via `add_path` / `remove_main_path`.
///
/// For non-linked worktrees, the main path and folder path are identical.
/// For linked worktrees, the main path is the original repo and the folder
/// path is the linked worktree location.
#[derive(Default, Debug, Clone)]
pub struct WorktreePaths {
    paths: PathList,
    main_paths: PathList,
}

impl PartialEq for WorktreePaths {
    fn eq(&self, other: &Self) -> bool {
        self.ordered_pairs().eq(other.ordered_pairs())
    }
}

impl WorktreePaths {
    /// Build from two parallel `PathList`s that already share the same
    /// insertion order. Used for deserialization from DB.
    ///
    /// Returns an error if the two lists have different lengths, which
    /// indicates corrupted data from a prior migration bug.
    pub fn from_path_lists(main_worktree_paths: PathList, folder_paths: PathList) -> anyhow::Result<Self> {
        anyhow::ensure!(
            main_worktree_paths.paths().len() == folder_paths.paths().len(),
            "main_worktree_paths has {} entries but folder_paths has {}",
            main_worktree_paths.paths().len(),
            folder_paths.paths().len(),
        );
        Ok(Self { paths: folder_paths, main_paths: main_worktree_paths })
    }

    /// Build for non-linked worktrees where main == folder for every path.
    pub fn from_folder_paths(folder_paths: &PathList) -> Self {
        Self { paths: folder_paths.clone(), main_paths: folder_paths.clone() }
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The folder paths (for workspace matching / `threads_by_paths` index).
    pub const fn folder_path_list(&self) -> &PathList {
        &self.paths
    }

    /// The main worktree paths (for group key / `threads_by_main_paths` index).
    pub const fn main_worktree_path_list(&self) -> &PathList {
        &self.main_paths
    }

    /// Iterate the (main_worktree_path, folder_path) pairs in insertion order.
    pub fn ordered_pairs(&self) -> impl Iterator<Item = (&PathBuf, &PathBuf)> {
        self.main_paths.ordered_paths().zip(self.paths.ordered_paths())
    }

    /// Add a new path pair. If the exact (main, folder) pair already exists,
    /// this is a no-op. Rebuilds both internal `PathList`s to maintain
    /// consistent ordering.
    pub fn add_path(&mut self, main_path: &Path, folder_path: &Path) {
        let already_exists = self.ordered_pairs().any(|(m, f)| m.as_path() == main_path && f.as_path() == folder_path);
        if already_exists {
            return;
        }
        let (mut mains, mut folders): (Vec<PathBuf>, Vec<PathBuf>) =
            self.ordered_pairs().map(|(m, f)| (m.clone(), f.clone())).unzip();
        mains.push(main_path.to_path_buf());
        folders.push(folder_path.to_path_buf());
        self.main_paths = PathList::new(&mains);
        self.paths = PathList::new(&folders);
    }

    /// Remove all pairs whose main worktree path matches the given path.
    /// This removes the corresponding entries from both lists.
    pub fn remove_main_path(&mut self, main_path: &Path) {
        let (mains, folders): (Vec<PathBuf>, Vec<PathBuf>) =
            self.ordered_pairs().filter(|(m, _)| m.as_path() != main_path).map(|(m, f)| (m.clone(), f.clone())).unzip();
        self.main_paths = PathList::new(&mains);
        self.paths = PathList::new(&folders);
    }

    /// Remove all pairs whose folder path matches the given path.
    /// This removes the corresponding entries from both lists.
    pub fn remove_folder_path(&mut self, folder_path: &Path) {
        let (mains, folders): (Vec<PathBuf>, Vec<PathBuf>) = self
            .ordered_pairs()
            .filter(|(_, f)| f.as_path() != folder_path)
            .map(|(m, f)| (m.clone(), f.clone()))
            .unzip();
        self.main_paths = PathList::new(&mains);
        self.paths = PathList::new(&folders);
    }
}

enum WorktreeStoreState {
    Local { fs: Arc<dyn Fs> },
    Remote { upstream_client: AnyProtoClient, upstream_project_id: u64, path_style: PathStyle },
}

#[derive(Clone)]
pub struct WorktreeIdCounter(Arc<AtomicU64>);

impl Default for WorktreeIdCounter {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(1)))
    }
}

impl WorktreeIdCounter {
    pub fn get(cx: &mut App) -> Self {
        cx.default_global::<Self>().clone()
    }

    fn next(&self) -> u64 {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Global for WorktreeIdCounter {}

/// Summarizes worktree ownership and current snapshot sizes.
#[derive(Debug, Default)]
pub struct WorktreeStoreDiagnostics {
    pub worktree_slots: usize,
    pub live_worktrees: usize,
    pub visible_worktrees: usize,
    pub strong_handles: usize,
    pub dead_weak_handles: usize,
    pub loading_worktrees: usize,
    pub total_entries: usize,
    pub visible_entries: usize,
    pub largest_worktree: Option<LargestWorktreeDiagnostics>,
}

/// Identifies the worktree with the largest current snapshot.
#[derive(Debug)]
pub struct LargestWorktreeDiagnostics {
    pub path: PathBuf,
    pub entries: usize,
    pub visible_entries: usize,
}

pub struct WorktreeStore {
    next_entry_id: Arc<AtomicUsize>,
    next_worktree_id: WorktreeIdCounter,
    downstream_client: Option<(AnyProtoClient, u64)>,
    retain_worktrees: bool,
    worktrees: Vec<WorktreeHandle>,
    /// Library worktrees the user opened a file from. They are held strongly
    /// and listed in the project panel; other library worktrees only live as
    /// long as something (e.g. an inlay hint's location) references them.
    shown_libraries: HashSet<WorktreeId>,
    scanning_enabled: bool,
    #[allow(clippy::type_complexity)]
    loading_worktrees: HashMap<Arc<SanitizedPath>, Shared<Task<Result<Entity<Worktree>, Arc<anyhow::Error>>>>>,
    initial_scan_complete: (watch::Sender<bool>, watch::Receiver<bool>),
    state: WorktreeStoreState,
}

#[derive(Debug)]
pub enum WorktreeStoreEvent {
    WorktreeAdded(Entity<Worktree>),
    WorktreeRemoved(EntityId, WorktreeId),
    WorktreeReleased(EntityId, WorktreeId),
    WorktreeOrderChanged,
    WorktreeUpdateSent(Entity<Worktree>),
    WorktreeUpdatedEntries(WorktreeId, UpdatedEntriesSet),
    WorktreeUpdatedGitRepositories(WorktreeId, UpdatedGitRepositoriesSet),
    WorktreeDeletedEntry(WorktreeId, ProjectEntryId),
    WorktreeUpdatedRootRepoCommonDir(WorktreeId),
}

impl EventEmitter<WorktreeStoreEvent> for WorktreeStore {}

impl WorktreeStore {
    pub fn init(client: &AnyProtoClient) {
        client.add_entity_request_handler(Self::handle_create_project_entry);
        client.add_entity_request_handler(Self::handle_copy_project_entry);
        client.add_entity_request_handler(Self::handle_delete_project_entry);
        client.add_entity_request_handler(Self::handle_trash_project_entry);
        client.add_entity_request_handler(Self::handle_restore_project_entry);
        client.add_entity_request_handler(Self::handle_expand_project_entry);
        client.add_entity_request_handler(Self::handle_expand_all_for_project_entry);
    }

    pub fn init_remote(client: &AnyProtoClient) {
        client.add_entity_request_handler(Self::handle_allocate_worktree_id);
    }

    pub fn local(retain_worktrees: bool, fs: Arc<dyn Fs>, next_worktree_id: WorktreeIdCounter) -> Self {
        Self {
            next_entry_id: Arc::default(),
            next_worktree_id,
            loading_worktrees: HashMap::default(),
            downstream_client: None,
            worktrees: Vec::new(),
            shown_libraries: HashSet::default(),
            scanning_enabled: true,
            retain_worktrees,
            initial_scan_complete: watch::channel(true),
            state: WorktreeStoreState::Local { fs },
        }
    }

    pub fn remote(
        retain_worktrees: bool,
        upstream_client: AnyProtoClient,
        upstream_project_id: u64,
        path_style: PathStyle,
        next_worktree_id: WorktreeIdCounter,
    ) -> Self {
        Self {
            next_entry_id: Arc::default(),
            next_worktree_id,
            loading_worktrees: HashMap::default(),
            downstream_client: None,
            worktrees: Vec::new(),
            shown_libraries: HashSet::default(),
            scanning_enabled: true,
            retain_worktrees,
            initial_scan_complete: watch::channel(true),
            state: WorktreeStoreState::Remote { upstream_client, upstream_project_id, path_style },
        }
    }

    pub fn next_entry_id(&self) -> Arc<AtomicUsize> {
        self.next_entry_id.clone()
    }

    pub fn next_worktree_id(&self) -> impl Future<Output = Result<WorktreeId>> + use<> {
        let strategy = match (&self.state, &self.downstream_client) {
            // we are a remote server, the client is in charge of assigning worktree ids
            (WorktreeStoreState::Local { .. }, Some((client, REMOTE_SERVER_PROJECT_ID))) => {
                Either::Left(client.clone())
            }
            // we are just a local ed project, we can assign ids
            (WorktreeStoreState::Local { .. }, _) => Either::Right(self.next_worktree_id.next()),
            // we are connected to a remote server, we are in charge of assigning worktree ids
            (WorktreeStoreState::Remote { .. }, _) => Either::Right(self.next_worktree_id.next()),
        };
        async move {
            match strategy {
                Either::Left(client) => Ok(client
                    .request(proto::AllocateWorktreeId { project_id: REMOTE_SERVER_PROJECT_ID })
                    .await?
                    .worktree_id),
                Either::Right(id) => Ok(id),
            }
            .map(WorktreeId::from_proto)
        }
    }

    pub fn disable_scanner(&mut self) {
        self.scanning_enabled = false;
        self.initial_scan_complete.0.send(true).ok();
    }

    /// Returns a future that resolves when all visible worktrees have completed
    /// their initial scan (entries populated, git repos detected).
    pub fn wait_for_initial_scan(&self) -> impl Future<Output = ()> + use<> {
        let mut rx = self.initial_scan_complete.1.clone();
        async move {
            while !*watch::Receiver::borrow(&mut rx) {
                if rx.changed().await.is_err() {
                    break;
                }
            }
        }
    }

    /// Returns whether all visible worktrees have completed their initial scan.
    pub fn initial_scan_completed(&self) -> bool {
        *watch::Receiver::borrow(&mut self.initial_scan_complete.1.clone())
    }

    /// Checks whether all visible worktrees have completed their initial scan
    /// and no worktree creations are pending, and updates the watch channel accordingly.
    fn update_initial_scan_state(&mut self, cx: &App) {
        let complete = self.loading_worktrees.is_empty()
            && self.visible_worktrees(cx).all(|wt| wt.read(cx).completed_scan_id() >= 1);
        self.initial_scan_complete.0.send(complete).ok();
    }

    /// Spawns a detached task that waits for a worktree's initial scan to complete,
    /// then rechecks and updates the aggregate initial scan state.
    fn observe_worktree_scan_completion(&self, worktree: &Entity<Worktree>, cx: &mut Context<Self>) {
        let await_scan = worktree.update(cx, |worktree, _cx| worktree.wait_for_snapshot(1));
        cx.spawn(async move |this, cx| {
            await_scan.await.ok();
            this.update(cx, |this, cx| {
                this.update_initial_scan_state(cx);
            })
            .ok();
            anyhow::Ok(())
        })
        .detach();
    }

    /// Returns worktree ownership and current snapshot size diagnostics.
    pub fn diagnostics(&self, cx: &App) -> WorktreeStoreDiagnostics {
        let mut diagnostics = WorktreeStoreDiagnostics {
            worktree_slots: self.worktrees.len(),
            loading_worktrees: self.loading_worktrees.len(),
            ..WorktreeStoreDiagnostics::default()
        };

        for handle in &self.worktrees {
            let worktree = match handle {
                WorktreeHandle::Strong(worktree) => {
                    diagnostics.strong_handles += 1;
                    Some(worktree.clone())
                }
                WorktreeHandle::Weak(worktree) => {
                    let worktree = worktree.upgrade();
                    if worktree.is_none() {
                        diagnostics.dead_weak_handles += 1;
                    }
                    worktree
                }
            };
            let Some(worktree) = worktree else {
                continue;
            };

            diagnostics.live_worktrees += 1;
            let worktree = worktree.read(cx);
            diagnostics.visible_worktrees += usize::from(worktree.is_visible());
            let snapshot = worktree.snapshot();
            let entries = snapshot.entry_count();
            let visible_entries = snapshot.visible_entry_count();
            diagnostics.total_entries += entries;
            diagnostics.visible_entries += visible_entries;

            let is_largest = diagnostics.largest_worktree.as_ref().is_none_or(|largest| entries > largest.entries);
            if is_largest {
                diagnostics.largest_worktree = Some(LargestWorktreeDiagnostics {
                    path: worktree.abs_path().to_path_buf(),
                    entries,
                    visible_entries,
                });
            }
        }

        diagnostics
    }

    /// Iterates through all worktrees, including ones that don't appear in the project panel
    pub fn worktrees(&self) -> impl '_ + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktrees.iter().filter_map(move |worktree| worktree.upgrade())
    }

    /// Iterates through all user-visible worktrees, the ones that appear in the project panel.
    pub fn visible_worktrees<'a>(&'a self, cx: &'a App) -> impl 'a + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktrees().filter(|worktree| worktree.read(cx).is_visible())
    }

    /// Iterates through all user-visible worktrees (directories and files that appear in the project panel) and other, invisible single files that could appear e.g. due to drag and drop.
    pub fn visible_worktrees_and_single_files<'a>(
        &'a self,
        cx: &'a App,
    ) -> impl 'a + DoubleEndedIterator<Item = Entity<Worktree>> {
        self.worktrees().filter(|worktree| worktree.read(cx).is_visible() || worktree.read(cx).is_single_file())
    }

    pub fn worktree_for_id(&self, id: WorktreeId, cx: &App) -> Option<Entity<Worktree>> {
        self.worktrees().find(|worktree| worktree.read(cx).id() == id)
    }

    pub fn worktree_for_entry(&self, entry_id: ProjectEntryId, cx: &App) -> Option<Entity<Worktree>> {
        self.worktrees().find(|worktree| worktree.read(cx).contains_entry(entry_id))
    }

    pub fn find_worktree(&self, abs_path: impl AsRef<Path>, cx: &App) -> Option<(Entity<Worktree>, Arc<RelPath>)> {
        let abs_path = SanitizedPath::new(abs_path.as_ref());
        for tree in self.worktrees() {
            let path_style = tree.read(cx).path_style();
            if let Some(relative_path) = path_style
                .strip_prefix(abs_path.as_ref(), tree.read(cx).abs_path().as_ref())
                .ok()
                .and_then(|path| RelPath::new(path, path_style).ok())
            {
                return Some((tree.clone(), relative_path.into_arc()));
            }
        }
        None
    }

    pub fn project_path_for_absolute_path(&self, abs_path: &Path, cx: &App) -> Option<ProjectPath> {
        self.find_worktree(abs_path, cx)
            .map(|(worktree, relative_path)| ProjectPath { worktree_id: worktree.read(cx).id(), path: relative_path })
    }

    pub fn absolutize(&self, project_path: &ProjectPath, cx: &App) -> Option<PathBuf> {
        let worktree = self.worktree_for_id(project_path.worktree_id, cx)?;
        Some(worktree.read(cx).absolutize(&project_path.path))
    }

    pub const fn path_style(&self) -> PathStyle {
        match &self.state {
            WorktreeStoreState::Local { .. } => PathStyle::local(),
            WorktreeStoreState::Remote { path_style, .. } => *path_style,
        }
    }

    pub fn find_or_create_worktree(
        &mut self,
        abs_path: impl AsRef<Path>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(Entity<Worktree>, Arc<RelPath>)>> {
        let abs_path = abs_path.as_ref();
        if let Some((tree, relative_path)) = self.find_worktree(abs_path, cx) {
            Task::ready(Ok((tree, relative_path)))
        } else {
            let abs_path = abs_path.to_path_buf();
            let worktree = self.create_worktree(&abs_path, visible, cx);
            cx.spawn(async move |_, cx| {
                let worktree = worktree.await?;
                // A library worktree is rooted at the library, not at `abs_path`.
                let relative_path = worktree.read_with(cx, |worktree, _| {
                    let path_style = worktree.path_style();
                    path_style
                        .strip_prefix(&abs_path, worktree.abs_path().as_ref())
                        .ok()
                        .and_then(|path| RelPath::new(path, path_style).ok())
                        .map(|path| path.into_arc())
                        .context("path is outside of the created worktree")
                })?;
                Ok((worktree, relative_path))
            })
        }
    }

    pub fn entry_for_id<'a>(&'a self, entry_id: ProjectEntryId, cx: &'a App) -> Option<&'a Entry> {
        self.worktrees().find_map(|worktree| worktree.read(cx).entry_for_id(entry_id))
    }

    pub fn worktree_and_entry_for_id<'a>(
        &'a self,
        entry_id: ProjectEntryId,
        cx: &'a App,
    ) -> Option<(Entity<Worktree>, &'a Entry)> {
        self.worktrees().find_map(|worktree| worktree.read(cx).entry_for_id(entry_id).map(|e| (worktree.clone(), e)))
    }

    pub fn entry_for_path<'a>(&'a self, path: &ProjectPath, cx: &'a App) -> Option<&'a Entry> {
        self.worktree_for_id(path.worktree_id, cx)?.read(cx).entry_for_path(&path.path)
    }

    pub fn copy_entry(
        &mut self,
        entry_id: ProjectEntryId,
        new_project_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<Entry>>> {
        let Some(old_worktree) = self.worktree_for_entry(entry_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };
        let Some(old_entry) = old_worktree.read(cx).entry_for_id(entry_id) else {
            return Task::ready(Err(anyhow!("no such entry")));
        };
        let Some(new_worktree) = self.worktree_for_id(new_project_path.worktree_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };

        match &self.state {
            WorktreeStoreState::Local { fs } => {
                let old_abs_path = old_worktree.read(cx).absolutize(&old_entry.path);
                let new_abs_path = new_worktree.read(cx).absolutize(&new_project_path.path);
                let fs = fs.clone();
                let copy = cx.background_spawn(async move {
                    copy_recursive(fs.as_ref(), &old_abs_path, &new_abs_path, fs::CopyOptions::default()).await
                });

                cx.spawn(async move |_, cx| {
                    copy.await?;
                    new_worktree
                        .update(cx, |this, cx| {
                            this.as_local_mut().unwrap().refresh_entry(new_project_path.path, None, cx)
                        })
                        .await
                })
            }
            WorktreeStoreState::Remote { upstream_client, upstream_project_id, .. } => {
                let response = upstream_client.request(proto::CopyProjectEntry {
                    project_id: *upstream_project_id,
                    entry_id: entry_id.to_proto(),
                    new_path: new_project_path.path.as_unix_str().to_owned(),
                    new_worktree_id: new_project_path.worktree_id.to_proto(),
                });
                cx.spawn(async move |_, cx| {
                    let response = response.await?;
                    match response.entry {
                        Some(entry) => new_worktree
                            .update(cx, |worktree, cx| {
                                worktree.as_remote_mut().unwrap().insert_entry(
                                    entry,
                                    response.worktree_scan_id as usize,
                                    cx,
                                )
                            })
                            .await
                            .map(Some),
                        None => Ok(None),
                    }
                })
            }
        }
    }

    pub fn rename_entry(
        &mut self,
        entry_id: ProjectEntryId,
        new_project_path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<CreatedEntry>> {
        let Some(old_worktree) = self.worktree_for_entry(entry_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };
        let Some(old_entry) = old_worktree.read(cx).entry_for_id(entry_id).cloned() else {
            return Task::ready(Err(anyhow!("no such entry")));
        };
        let Some(new_worktree) = self.worktree_for_id(new_project_path.worktree_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };

        match &self.state {
            WorktreeStoreState::Local { fs } => {
                let abs_old_path = old_worktree.read(cx).absolutize(&old_entry.path);
                let new_worktree_ref = new_worktree.read(cx);
                let is_root_entry = new_worktree_ref.root_entry().is_some_and(|e| e.id == entry_id);
                let abs_new_path = if is_root_entry {
                    let abs_path = new_worktree_ref.abs_path();
                    let Some(root_parent_path) = abs_path.parent() else {
                        return Task::ready(Err(anyhow!("no parent for path {:?}", abs_path)));
                    };
                    root_parent_path.join(new_project_path.path.as_std_path())
                } else {
                    new_worktree_ref.absolutize(&new_project_path.path)
                };

                let fs = fs.clone();
                let case_sensitive = new_worktree.read(cx).as_local().unwrap().fs_is_case_sensitive();

                let do_rename = async move |fs: &dyn Fs, old_path: &Path, new_path: &Path, overwrite| {
                    fs.rename(old_path, new_path, fs::RenameOptions { overwrite, ..fs::RenameOptions::default() })
                        .await
                        .with_context(|| format!("renaming {old_path:?} into {new_path:?}"))
                };

                let rename = cx.background_spawn({
                    let abs_new_path = abs_new_path.clone();
                    async move {
                        // If we're on a case-insensitive FS and we're doing a case-only rename (i.e. `foobar` to `FOOBAR`)
                        // we want to overwrite, because otherwise we run into a file-already-exists error.
                        let overwrite = !case_sensitive
                            && abs_old_path != abs_new_path
                            && abs_old_path.to_str().map(|p| p.to_lowercase())
                                == abs_new_path.to_str().map(|p| p.to_lowercase());

                        // The directory we're renaming into might not exist yet
                        if let Err(e) = do_rename(fs.as_ref(), &abs_old_path, &abs_new_path, overwrite).await {
                            if let Some(err) = e.downcast_ref::<std::io::Error>()
                                && err.kind() == std::io::ErrorKind::NotFound
                                && let Some(parent) = abs_new_path.parent()
                            {
                                fs.create_dir(parent)
                                    .await
                                    .with_context(|| format!("creating parent directory {parent:?}"))?;
                                return do_rename(fs.as_ref(), &abs_old_path, &abs_new_path, overwrite).await;
                            }
                            return Err(e);
                        }
                        Ok(())
                    }
                });

                cx.spawn(async move |_, cx| {
                    rename.await?;
                    Ok(new_worktree
                        .update(cx, |this, cx| {
                            let local = this.as_local_mut().unwrap();
                            if is_root_entry {
                                // We eagerly update `abs_path` and refresh this worktree.
                                // Otherwise, the FS watcher would do it on the `RootUpdated` event,
                                // but with a noticeable delay, so we handle it proactively.
                                local.update_abs_path_and_refresh(SanitizedPath::new_arc(&abs_new_path), cx);
                                Task::ready(Ok(this.root_entry().cloned()))
                            } else {
                                // First refresh the parent directory (in case it was newly created)
                                if let Some(parent) = new_project_path.path.parent() {
                                    drop(local.refresh_entries_for_paths(vec![parent.into()]));
                                }
                                // Then refresh the new path
                                local.refresh_entry(new_project_path.path.clone(), Some(old_entry.path), cx)
                            }
                        })
                        .await?
                        .map(CreatedEntry::Included)
                        .unwrap_or_else(|| CreatedEntry::Excluded { abs_path: abs_new_path }))
                })
            }
            WorktreeStoreState::Remote { upstream_client, upstream_project_id, .. } => {
                let response = upstream_client.request(proto::RenameProjectEntry {
                    project_id: *upstream_project_id,
                    entry_id: entry_id.to_proto(),
                    new_path: new_project_path.path.as_unix_str().to_owned(),
                    new_worktree_id: new_project_path.worktree_id.to_proto(),
                });
                cx.spawn(async move |_, cx| {
                    let response = response.await?;
                    match response.entry {
                        Some(entry) => new_worktree
                            .update(cx, |worktree, cx| {
                                worktree.as_remote_mut().unwrap().insert_entry(
                                    entry,
                                    response.worktree_scan_id as usize,
                                    cx,
                                )
                            })
                            .await
                            .map(CreatedEntry::Included),
                        None => {
                            let abs_path =
                                new_worktree.read_with(cx, |worktree, _| worktree.absolutize(&new_project_path.path));
                            Ok(CreatedEntry::Excluded { abs_path })
                        }
                    }
                })
            }
        }
    }
    pub fn create_worktree(
        &mut self,
        abs_path: impl AsRef<Path>,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Worktree>>> {
        // Anything inside a library joins its library worktree, even when opened
        // as a visible path (e.g. a file passed on the command line). Opening the
        // library's root folder itself still makes a normal project root.
        let library =
            library_root(abs_path.as_ref(), cx).filter(|library| !visible || library.path != abs_path.as_ref());
        let visible = visible && library.is_none();
        let abs_path: Arc<SanitizedPath> = match &library {
            Some(library) => SanitizedPath::new_arc(&library.path),
            None => SanitizedPath::new_arc(&abs_path),
        };
        if library.is_some()
            && let Some(worktree) =
                self.worktrees().find(|worktree| worktree.read(cx).abs_path().as_ref() == abs_path.as_path())
        {
            return Task::ready(Ok(worktree));
        }
        let library_name: Option<Arc<str>> = library.map(|library| library.name.into());
        if !self.loading_worktrees.contains_key(&abs_path) {
            let task = match &self.state {
                WorktreeStoreState::Remote { upstream_client, path_style, .. } => {
                    let abs_path = RemotePathBuf::new(abs_path.to_string(), *path_style);
                    self.create_remote_worktree(upstream_client.clone(), abs_path, visible, library_name, cx)
                }
                WorktreeStoreState::Local { fs } => {
                    self.create_local_worktree(fs.clone(), abs_path.clone(), visible, library_name, cx)
                }
            };

            let loading_task = cx.spawn({
                let abs_path = abs_path.clone();
                async move |this, cx| {
                    let result = task.await;
                    this.update(cx, |this, cx| {
                        this.loading_worktrees.remove(&abs_path);
                        if !visible || !this.scanning_enabled || result.is_err() {
                            this.update_initial_scan_state(cx);
                        }
                    })
                    .ok();

                    if let Ok(worktree) = &result {
                        if let Some((trusted_worktrees, worktree_store)) = this
                            .update(cx, |_, cx| TrustedWorktrees::try_get_global(cx).zip(Some(cx.entity())))
                            .ok()
                            .flatten()
                        {
                            trusted_worktrees.update(cx, |trusted_worktrees, cx| {
                                trusted_worktrees.can_trust(&worktree_store, worktree.read(cx).id(), cx);
                            });
                        }

                        this.update(cx, |this, cx| {
                            if this.scanning_enabled && visible {
                                this.observe_worktree_scan_completion(worktree, cx);
                            }
                        })
                        .ok();
                    }

                    result
                }
            });

            self.loading_worktrees.insert(abs_path.clone(), loading_task.shared());

            if visible && self.scanning_enabled {
                self.initial_scan_complete.0.send(false).ok();
            }
        }
        let task = self.loading_worktrees.get(&abs_path).unwrap().clone();
        cx.background_spawn(async move { task.await.map_err(|error| (*error).cloned()) })
    }

    fn create_remote_worktree(
        &self,
        client: AnyProtoClient,
        abs_path: RemotePathBuf,
        visible: bool,
        library_name: Option<Arc<str>>,
        cx: &Context<Self>,
    ) -> Task<Result<Entity<Worktree>, Arc<anyhow::Error>>> {
        let path_style = abs_path.path_style();
        let mut abs_path = abs_path.to_string();
        // If we start with `/~` that means the ssh path was something like `ssh://user@host/~/home-dir-folder/`
        // in which case want to strip the leading the `/`.
        // On the host-side, the `~` will get expanded.
        // That's what git does too: https://github.com/libgit2/libgit2/issues/3345#issuecomment-127050850
        if abs_path.starts_with("/~") {
            abs_path = abs_path[1..].to_string();
        }
        if abs_path.is_empty() {
            abs_path = "~/".to_string();
        }

        cx.spawn(async move |this, cx| {
            let this = this.upgrade().context("Dropped worktree store")?;

            let path = RemotePathBuf::new(abs_path, path_style);
            let response = client
                .request(proto::AddWorktree { project_id: REMOTE_SERVER_PROJECT_ID, path: path.to_proto(), visible })
                .await?;

            if let Some(existing_worktree) =
                this.read_with(cx, |this, cx| this.worktree_for_id(WorktreeId::from_proto(response.worktree_id), cx))
            {
                return Ok(existing_worktree);
            }

            let root_path_buf = PathBuf::from(response.canonicalized_path.clone());
            let root_name = root_path_buf
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(root_path_buf.to_string_lossy().into_owned());

            let worktree = cx.update(|cx| {
                Worktree::remote(
                    REMOTE_SERVER_PROJECT_ID,
                    ReplicaId::REMOTE_SERVER,
                    proto::WorktreeMetadata {
                        id: response.worktree_id,
                        root_name,
                        visible,
                        library_name: library_name.map(|name| name.to_string()),
                        abs_path: response.canonicalized_path,
                        root_repo_common_dir: response.root_repo_common_dir,
                        root_repo_is_linked_worktree: response.root_repo_is_linked_worktree,
                    },
                    client,
                    path_style,
                    cx,
                )
            });

            this.update(cx, |this, cx| {
                this.add(&worktree, cx);
            });
            Ok(worktree)
        })
    }

    fn create_local_worktree(
        &self,
        fs: Arc<dyn Fs>,
        abs_path: Arc<SanitizedPath>,
        visible: bool,
        library_name: Option<Arc<str>>,
        cx: &Context<Self>,
    ) -> Task<Result<Entity<Worktree>, Arc<anyhow::Error>>> {
        let next_entry_id = self.next_entry_id.clone();
        let scanning_enabled = self.scanning_enabled;

        let next_worktree_id = self.next_worktree_id();

        cx.spawn(async move |this, cx| {
            let worktree_id = next_worktree_id.await?;
            let worktree = Worktree::local(
                SanitizedPath::cast_arc(abs_path.clone()),
                visible,
                fs,
                next_entry_id,
                scanning_enabled,
                worktree_id,
                cx,
            )
            .await?;
            if library_name.is_some() {
                worktree.update(cx, |worktree, _| worktree.set_library_name(library_name));
                // Files open right after this returns, and manifest lookups (e.g. the
                // crate's `Cargo.toml`) must not see a half-scanned library.
                let scan_complete =
                    worktree.read_with(cx, |worktree, _| worktree.as_local().map(|worktree| worktree.scan_complete()));
                if let Some(scan_complete) = scan_complete {
                    scan_complete.await;
                }
            }

            this.update(cx, |this, cx| this.add(&worktree, cx))?;

            if visible {
                cx.update(|cx| {
                    cx.add_recent_document(abs_path.as_path());
                });
            }

            Ok(worktree)
        })
    }

    pub fn add(&mut self, worktree: &Entity<Worktree>, cx: &mut Context<Self>) {
        let worktree_id = worktree.read(cx).id();
        debug_assert!(self.worktrees().all(|w| w.read(cx).id() != worktree_id));

        let push_strong_handle = self.retain_worktrees || worktree.read(cx).is_visible();
        let handle = if push_strong_handle {
            WorktreeHandle::Strong(worktree.clone())
        } else {
            WorktreeHandle::Weak(worktree.downgrade())
        };
        self.worktrees.push(handle);

        cx.emit(WorktreeStoreEvent::WorktreeAdded(worktree.clone()));
        self.send_project_updates(cx);

        let handle_id = worktree.entity_id();
        cx.subscribe(worktree, |_, worktree, event, cx| {
            let worktree_id = worktree.read(cx).id();
            match event {
                worktree::Event::UpdatedEntries(changes) => {
                    cx.emit(WorktreeStoreEvent::WorktreeUpdatedEntries(worktree_id, changes.clone()));
                }
                worktree::Event::UpdatedGitRepositories(set) => {
                    cx.emit(WorktreeStoreEvent::WorktreeUpdatedGitRepositories(worktree_id, set.clone()));
                }
                worktree::Event::DeletedEntry(id) => {
                    cx.emit(WorktreeStoreEvent::WorktreeDeletedEntry(worktree_id, *id))
                }
                worktree::Event::Deleted => {
                    // The worktree root itself has been deleted (for single-file worktrees)
                    // The worktree will be removed via the observe_release callback
                }
                worktree::Event::UpdatedRootRepoCommonDir { .. } => {
                    cx.emit(WorktreeStoreEvent::WorktreeUpdatedRootRepoCommonDir(worktree_id));
                }
            }
        })
        .detach();
        cx.observe_release(worktree, move |this, worktree, cx| {
            cx.emit(WorktreeStoreEvent::WorktreeReleased(handle_id, worktree.id()));
            cx.emit(WorktreeStoreEvent::WorktreeRemoved(handle_id, worktree.id()));
            this.send_project_updates(cx);
        })
        .detach();
    }

    pub fn is_library_shown(&self, worktree_id: WorktreeId) -> bool {
        self.shown_libraries.contains(&worktree_id)
    }

    /// Lists the library worktree in the project panel and keeps it alive.
    pub fn show_library(&mut self, worktree_id: WorktreeId, cx: &mut Context<Self>) {
        if self.shown_libraries.contains(&worktree_id) {
            return;
        }
        let Some(worktree) = self.worktree_for_id(worktree_id, cx) else {
            return;
        };
        if worktree.read(cx).library_name().is_none() {
            return;
        }
        self.shown_libraries.insert(worktree_id);
        for handle in &mut self.worktrees {
            if handle.upgrade().is_some_and(|handle| handle == worktree) {
                *handle = WorktreeHandle::Strong(worktree.clone());
            }
        }
        cx.emit(WorktreeStoreEvent::WorktreeOrderChanged);
    }

    pub fn remove_worktree(&mut self, id_to_remove: WorktreeId, cx: &mut Context<Self>) {
        self.shown_libraries.remove(&id_to_remove);
        let mut removed = false;
        self.worktrees.retain(|worktree| {
            if let Some(worktree) = worktree.upgrade() {
                if worktree.read(cx).id() == id_to_remove {
                    cx.emit(WorktreeStoreEvent::WorktreeRemoved(worktree.entity_id(), id_to_remove));
                    removed = true;
                    false
                } else {
                    true
                }
            } else {
                false
            }
        });
        // A remote client asks to remove a worktree whenever it releases a
        // handle to it, which can happen again after the worktree is gone.
        // Announcing the unchanged worktrees in response would make the
        // client re-create and release handles for the ones it has dropped
        // in the meantime, and send us even more removal requests.
        if !removed {
            return;
        }
        self.update_initial_scan_state(cx);
        self.send_project_updates(cx);
    }

    pub fn worktree_for_main_worktree_path(&self, path: &Path, cx: &App) -> Option<Entity<Worktree>> {
        self.visible_worktrees(cx).find(|worktree| {
            let worktree = worktree.read(cx);
            if let Some(common_dir) = worktree.root_repo_common_dir() {
                common_dir.parent() == Some(path)
            } else {
                worktree.abs_path().as_ref() == path
            }
        })
    }

    fn upstream_client(&self) -> Option<(AnyProtoClient, u64)> {
        match &self.state {
            WorktreeStoreState::Remote { upstream_client, upstream_project_id, .. } => {
                Some((upstream_client.clone(), *upstream_project_id))
            }
            WorktreeStoreState::Local { .. } => None,
        }
    }

    pub fn set_worktrees_from_proto(
        &mut self,
        worktrees: Vec<proto::WorktreeMetadata>,
        replica_id: ReplicaId,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let mut old_worktrees_by_id = self
            .worktrees
            .drain(..)
            .filter_map(|worktree| {
                let worktree = worktree.upgrade()?;
                Some((worktree.read(cx).id(), worktree))
            })
            .collect::<HashMap<_, _>>();

        let (client, project_id) = self.upstream_client().context("invalid project")?;

        for worktree in worktrees {
            if let Some(old_worktree) = old_worktrees_by_id.remove(&WorktreeId::from_proto(worktree.id)) {
                let push_strong_handle = self.retain_worktrees
                    || old_worktree.read(cx).is_visible()
                    || self.shown_libraries.contains(&old_worktree.read(cx).id());
                let handle = if push_strong_handle {
                    WorktreeHandle::Strong(old_worktree.clone())
                } else {
                    WorktreeHandle::Weak(old_worktree.downgrade())
                };
                self.worktrees.push(handle);
            } else {
                self.add(
                    &Worktree::remote(project_id, replica_id, worktree, client.clone(), self.path_style(), cx),
                    cx,
                );
            }
        }
        self.send_project_updates(cx);

        Ok(())
    }

    pub fn move_worktree(&mut self, source: WorktreeId, destination: WorktreeId, cx: &mut Context<Self>) -> Result<()> {
        if source == destination {
            return Ok(());
        }

        let mut source_index = None;
        let mut destination_index = None;
        for (i, worktree) in self.worktrees.iter().enumerate() {
            if let Some(worktree) = worktree.upgrade() {
                let worktree_id = worktree.read(cx).id();
                if worktree_id == source {
                    source_index = Some(i);
                    if destination_index.is_some() {
                        break;
                    }
                } else if worktree_id == destination {
                    destination_index = Some(i);
                    if source_index.is_some() {
                        break;
                    }
                }
            }
        }

        let source_index = source_index.with_context(|| format!("Missing worktree for id {source}"))?;
        let destination_index = destination_index.with_context(|| format!("Missing worktree for id {destination}"))?;

        if source_index == destination_index {
            return Ok(());
        }

        let worktree_to_move = self.worktrees.remove(source_index);
        self.worktrees.insert(destination_index, worktree_to_move);
        cx.emit(WorktreeStoreEvent::WorktreeOrderChanged);
        cx.notify();
        Ok(())
    }

    pub fn disconnected_from_host(&mut self, cx: &mut App) {
        for worktree in &self.worktrees {
            if let Some(worktree) = worktree.upgrade() {
                worktree.update(cx, |worktree, _| {
                    if let Some(worktree) = worktree.as_remote_mut() {
                        worktree.disconnected_from_host();
                    }
                });
            }
        }
    }

    pub fn send_project_updates(&mut self, cx: &mut Context<Self>) {
        let Some((downstream_client, project_id)) = self.downstream_client.clone() else {
            return;
        };

        let update = proto::UpdateProject { project_id, worktrees: self.worktree_metadata_protos(cx) };

        // Preserve project update ordering by sending requests in serial.
        downstream_client.send(update).log_err();
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| {
                let worktrees = this.worktrees().collect::<Vec<_>>();

                for worktree in worktrees {
                    worktree.update(cx, |worktree, cx| {
                        let client = downstream_client.clone();
                        worktree.observe_updates(project_id, cx, {
                            move |update| {
                                let client = client.clone();
                                async move { client.send(update).log_err().is_some() }
                            }
                        });
                    });

                    cx.emit(WorktreeStoreEvent::WorktreeUpdateSent(worktree.clone()))
                }

                anyhow::Ok(())
            })
        })
        .detach_and_log_err(cx);
    }

    pub fn worktree_metadata_protos(&self, cx: &App) -> Vec<proto::WorktreeMetadata> {
        self.worktrees()
            .map(|worktree| {
                let worktree = worktree.read(cx);
                proto::WorktreeMetadata {
                    id: worktree.id().to_proto(),
                    root_name: worktree.root_name_str().to_owned(),
                    visible: worktree.is_visible(),
                    library_name: worktree.library_name().map(ToString::to_string),
                    abs_path: worktree.abs_path().to_string_lossy().into_owned(),
                    root_repo_common_dir: worktree.root_repo_common_dir().map(|p| p.to_string_lossy().into_owned()),
                    root_repo_is_linked_worktree: worktree.root_repo_is_linked_worktree(),
                }
            })
            .collect()
    }

    pub fn shared(&mut self, remote_id: u64, downstream_client: AnyProtoClient, cx: &mut Context<Self>) {
        self.retain_worktrees = true;
        self.downstream_client = Some((downstream_client, remote_id));

        // When shared, retain all worktrees
        for worktree_handle in &mut self.worktrees {
            match worktree_handle {
                WorktreeHandle::Strong(_) => {}
                WorktreeHandle::Weak(worktree) => {
                    if let Some(worktree) = worktree.upgrade() {
                        *worktree_handle = WorktreeHandle::Strong(worktree);
                    }
                }
            }
        }
        // The remote server sends its initial project updates after setup.
        // Otherwise we are the remote server which is currently constructing
        // worktree store before the client actually has set up its message
        // handlers.
        if remote_id != REMOTE_SERVER_PROJECT_ID {
            self.send_project_updates(cx);
        }
    }

    pub async fn handle_create_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::CreateProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let worktree = this.update(&mut cx, |this, cx| {
            let worktree_id = WorktreeId::from_proto(envelope.payload.worktree_id);
            this.worktree_for_id(worktree_id, cx).context("worktree not found")
        })?;
        Worktree::handle_create_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_copy_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::CopyProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let new_worktree_id = WorktreeId::from_proto(envelope.payload.new_worktree_id);
        let new_project_path = (new_worktree_id, RelPath::from_unix_str(&envelope.payload.new_path)?);
        let (scan_id, entry) = this.update(&mut cx, |this, cx| {
            if this.entry_for_id(entry_id, cx).is_none() {
                bail!("no such entry");
            }

            let new_worktree = this.worktree_for_id(new_worktree_id, cx).context("no such worktree")?;
            let scan_id = new_worktree.read(cx).scan_id();
            anyhow::Ok((scan_id, this.copy_entry(entry_id, new_project_path.into(), cx)))
        })?;
        let entry = entry.await?;
        Ok(proto::ProjectEntryResponse {
            entry: entry.as_ref().map(|entry| entry.into()),
            worktree_scan_id: scan_id as u64,
        })
    }

    pub async fn handle_trash_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::TrashProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::TrashProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree =
            this.update(&mut cx, |this, cx| this.worktree_for_entry(entry_id, cx).context("worktree not found"))?;
        Worktree::handle_trash_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_delete_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::DeleteProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree =
            this.update(&mut cx, |this, cx| this.worktree_for_entry(entry_id, cx).context("worktree not found"))?;
        Worktree::handle_delete_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_restore_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::RestoreProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::RestoreProjectEntryResponse> {
        let worktree_id = WorktreeId::from_proto(envelope.payload.worktree_id);

        let worktree =
            this.update(&mut cx, |this, cx| this.worktree_for_id(worktree_id, cx).context("worktree not found"))?;

        Worktree::handle_restore_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_rename_project_entry(
        this: Entity<Self>,
        request: proto::RenameProjectEntry,
        mut cx: AsyncApp,
    ) -> Result<proto::ProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(request.entry_id);
        let new_worktree_id = WorktreeId::from_proto(request.new_worktree_id);
        let rel_path = RelPath::from_unix_str(&request.new_path)
            .with_context(|| format!("received invalid relative path {:?}", request.new_path))?;

        let (scan_id, task) = this.update(&mut cx, |this, cx| {
            let worktree = this.worktree_for_entry(entry_id, cx).context("no such worktree")?;

            let scan_id = worktree.read(cx).scan_id();
            anyhow::Ok((scan_id, this.rename_entry(entry_id, (new_worktree_id, rel_path).into(), cx)))
        })?;
        Ok(proto::ProjectEntryResponse {
            entry: match &task.await? {
                CreatedEntry::Included(entry) => Some(entry.into()),
                CreatedEntry::Excluded { .. } => None,
            },
            worktree_scan_id: scan_id as u64,
        })
    }

    pub async fn handle_expand_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ExpandProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ExpandProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree =
            this.update(&mut cx, |this, cx| this.worktree_for_entry(entry_id, cx)).context("invalid request")?;
        Worktree::handle_expand_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_expand_all_for_project_entry(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ExpandAllForProjectEntry>,
        mut cx: AsyncApp,
    ) -> Result<proto::ExpandAllForProjectEntryResponse> {
        let entry_id = ProjectEntryId::from_proto(envelope.payload.entry_id);
        let worktree =
            this.update(&mut cx, |this, cx| this.worktree_for_entry(entry_id, cx)).context("invalid request")?;
        Worktree::handle_expand_all_for_entry(worktree, envelope.payload, cx).await
    }

    pub async fn handle_allocate_worktree_id(
        _this: Entity<Self>,
        _envelope: TypedEnvelope<proto::AllocateWorktreeId>,
        cx: AsyncApp,
    ) -> Result<proto::AllocateWorktreeIdResponse> {
        let worktree_id = cx.update(|cx| WorktreeIdCounter::get(cx).next());
        Ok(proto::AllocateWorktreeIdResponse { worktree_id })
    }

    pub fn fs(&self) -> Option<Arc<dyn Fs>> {
        match &self.state {
            WorktreeStoreState::Local { fs } => Some(fs.clone()),
            WorktreeStoreState::Remote { .. } => None,
        }
    }

    pub fn paths(&self, cx: &App) -> WorktreePaths {
        let (mains, folders): (Vec<PathBuf>, Vec<PathBuf>) = self
            .visible_worktrees(cx)
            .map(|worktree| {
                let snapshot = worktree.read(cx).snapshot();
                let folder_path = snapshot.abs_path().to_path_buf();
                let main_path = snapshot
                    .root_repo_common_dir()
                    .filter(|dir| !crate::git_store::is_submodule_git_dir(dir))
                    .map(|dir| crate::git_store::repo_identity_path(dir, snapshot.path_style()))
                    .filter(|repo_path| {
                        snapshot.root_repo_is_linked_worktree()
                            || *repo_path == folder_path.as_path()
                            || !folder_path.starts_with(*repo_path)
                    })
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| folder_path.clone());
                (main_path, folder_path)
            })
            .unzip();

        WorktreePaths { paths: PathList::new(&folders), main_paths: PathList::new(&mains) }
    }
}

#[derive(Clone, Debug)]
enum WorktreeHandle {
    Strong(Entity<Worktree>),
    Weak(WeakEntity<Worktree>),
}

impl WorktreeHandle {
    fn upgrade(&self) -> Option<Entity<Worktree>> {
        match self {
            WorktreeHandle::Strong(handle) => Some(handle.clone()),
            WorktreeHandle::Weak(handle) => handle.upgrade(),
        }
    }
}
