use std::collections::hash_map;
use std::io;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use futures::FutureExt as _;
use futures::future::Shared;
use gpui::App;
use gpui::AppContext as _;
use gpui::Context;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::Subscription;
use gpui::Task;
use gpui::WeakEntity;
use language::Buffer;
use language::BufferEvent;
use language::Capability;
use language::DiskState;
use language::File as _;
use language::Language;
use language::LineEnding;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use text::BufferId;
use util::ResultExt as _;
use util::rel_path::RelPath;
use worktree::File;
use worktree::PathChange;
use worktree::ProjectEntryId;
use worktree::Worktree;

use crate::ProjectPath;
use crate::worktree_store::WorktreeStore;
use crate::worktree_store::WorktreeStoreEvent;

/// A set of open buffers.
pub struct BufferStore {
    local: LocalBufferStore,
    #[allow(clippy::type_complexity)]
    loading_buffers: FxHashMap<ProjectPath, Shared<Task<Result<Entity<Buffer>, Arc<anyhow::Error>>>>>,
    worktree_store: Entity<WorktreeStore>,
    opened_buffers: FxHashMap<BufferId, WeakEntity<Buffer>>,
    path_to_buffer_id: FxHashMap<ProjectPath, BufferId>,
    non_searchable_buffers: FxHashSet<BufferId>,
}

struct LocalBufferStore {
    local_buffer_ids_by_entry_id: FxHashMap<ProjectEntryId, BufferId>,
    worktree_store: Entity<WorktreeStore>,
    _subscription: Subscription,
}

pub enum BufferStoreEvent {
    BufferAdded(Entity<Buffer>),
    BufferDropped(BufferId),
    BufferChangedFilePath { buffer: Entity<Buffer>, old_file: Option<Arc<dyn language::File>> },
}

#[derive(Default, Debug, Clone)]
pub struct ProjectTransaction(pub FxHashMap<Entity<Buffer>, language::Transaction>);

impl PartialEq for ProjectTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|(buffer, transaction)| other.0.get(buffer).is_some_and(|t| t.id == transaction.id))
    }
}

impl EventEmitter<BufferStoreEvent> for BufferStore {}

impl LocalBufferStore {
    fn save_local_buffer(
        &self,
        buffer_handle: Entity<Buffer>,
        worktree: Entity<Worktree>,
        path: Arc<RelPath>,
        mut has_changed_file: bool,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<()>> {
        // Line endings are always LF.
        buffer_handle.update(cx, |buffer, cx| {
            if buffer.line_ending() != LineEnding::Unix {
                buffer.set_line_ending(LineEnding::Unix, cx);
            }
        });
        let buffer = buffer_handle.read(cx);

        let text = buffer.as_rope().clone();
        let line_ending = buffer.line_ending();
        let encoding = buffer.encoding();
        let has_bom = buffer.has_bom();
        let version = buffer.version();
        let _buffer_id = buffer.buffer_id();
        let file = buffer.file().cloned();
        if file.as_ref().is_some_and(|file| file.disk_state() == DiskState::New) {
            has_changed_file = true;
        }

        let save =
            worktree.update(cx, |worktree, cx| worktree.write_file(path, text, line_ending, encoding, has_bom, cx));

        cx.spawn(async move |_this, cx| {
            let new_file = save.await?;
            let mtime = new_file.disk_state().mtime();
            buffer_handle.update(cx, |buffer, cx| {
                if has_changed_file {
                    buffer.file_updated(new_file, cx);
                }
                buffer.did_save(version.clone(), mtime, cx);
            });
            Ok(())
        })
    }

    fn subscribe_to_worktree(&self, worktree: &Entity<Worktree>, cx: &mut Context<BufferStore>) {
        cx.subscribe(worktree, |this, worktree, event, cx| {
            if let worktree::Event::UpdatedEntries(changes) = event {
                Self::local_worktree_entries_changed(this, &worktree, changes, cx);
            }
        })
        .detach();
    }

    fn local_worktree_entries_changed(
        this: &mut BufferStore,
        worktree_handle: &Entity<Worktree>,
        changes: &[(Arc<RelPath>, ProjectEntryId, PathChange)],
        cx: &mut Context<BufferStore>,
    ) {
        let snapshot = worktree_handle.read(cx).snapshot();
        for (path, entry_id, _) in changes {
            Self::local_worktree_entry_changed(this, *entry_id, path, worktree_handle, &snapshot, cx);
        }
    }

    fn local_worktree_entry_changed(
        this: &mut BufferStore,
        entry_id: ProjectEntryId,
        path: &Arc<RelPath>,
        worktree: &Entity<worktree::Worktree>,
        snapshot: &worktree::Snapshot,
        cx: &mut Context<BufferStore>,
    ) -> Option<()> {
        let project_path = ProjectPath { worktree_id: snapshot.id(), path: Arc::clone(path) };

        let buffer_id = this
            .local
            .local_buffer_ids_by_entry_id
            .get(&entry_id)
            .copied()
            .or_else(|| this.path_to_buffer_id.get(&project_path).copied())?;

        let buffer = if let Some(buffer) = this.get(buffer_id) {
            Some(buffer)
        } else {
            this.opened_buffers.remove(&buffer_id);
            this.non_searchable_buffers.remove(&buffer_id);
            None
        };

        let Some(buffer) = buffer else {
            this.path_to_buffer_id.remove(&project_path);
            this.local.local_buffer_ids_by_entry_id.remove(&entry_id);
            return None;
        };

        if snapshot.entry_for_id(entry_id).is_none()
            && snapshot.entry_for_path(path.as_ref()).is_none()
            && Self::unloaded_ancestor_hides_path(snapshot, path)
        {
            let refresh = worktree.read(cx).refresh_entries_for_paths(vec![Arc::clone(path)]);
            cx.spawn({
                let path = Arc::clone(path);
                let worktree = worktree.clone();
                async move |this, cx| {
                    refresh.await.ok();
                    this.update(cx, |this, cx| {
                        let snapshot = worktree.read(cx).snapshot();
                        if Self::unloaded_ancestor_hides_path(&snapshot, &path) {
                            log::warn!("buffer path {path:?} is still hidden by an unloaded directory after a refresh");
                        } else {
                            Self::local_worktree_entry_changed(this, entry_id, &path, &worktree, &snapshot, cx);
                        }
                    })
                    .ok();
                }
            })
            .detach();
            return None;
        }

        let events = buffer.update(cx, |buffer, cx| {
            let file = Arc::clone(buffer.file()?);
            let old_file = File::from_dyn(Some(&file))?;
            if old_file.worktree != *worktree {
                return None;
            }

            let snapshot_entry = old_file
                .entry_id
                .and_then(|entry_id| snapshot.entry_for_id(entry_id))
                .or_else(|| snapshot.entry_for_path(old_file.path.as_ref()));

            if let Some(entry) = snapshot_entry {
                buffer.set_capability(worktree_file_capability(old_file, entry.is_writable, cx), cx);
            }

            let new_file = if let Some(entry) = snapshot_entry {
                File {
                    disk_state: match entry.mtime {
                        Some(mtime) => DiskState::Present { mtime, size: entry.size },
                        None => old_file.disk_state,
                    },
                    entry_id: Some(entry.id),
                    path: Arc::clone(&entry.path),
                    worktree: worktree.clone(),
                }
            } else {
                File {
                    disk_state: DiskState::Deleted,
                    entry_id: old_file.entry_id,
                    path: Arc::clone(&old_file.path),
                    worktree: worktree.clone(),
                }
            };

            if new_file == *old_file {
                return None;
            }

            let mut events = Vec::new();
            if new_file.path != old_file.path {
                this.path_to_buffer_id
                    .remove(&ProjectPath { path: Arc::clone(&old_file.path), worktree_id: old_file.worktree_id(cx) });
                this.path_to_buffer_id.insert(
                    ProjectPath { worktree_id: new_file.worktree_id(cx), path: Arc::clone(&new_file.path) },
                    buffer_id,
                );
                events.push(BufferStoreEvent::BufferChangedFilePath {
                    buffer: cx.entity(),
                    old_file: buffer.file().cloned(),
                });
            }
            let local = &mut this.local;
            if new_file.entry_id != old_file.entry_id {
                if let Some(entry_id) = old_file.entry_id {
                    local.local_buffer_ids_by_entry_id.remove(&entry_id);
                }
                if let Some(entry_id) = new_file.entry_id {
                    local.local_buffer_ids_by_entry_id.insert(entry_id, buffer_id);
                }
            }

            buffer.file_updated(Arc::new(new_file), cx);
            Some(events)
        })?;

        for event in events {
            cx.emit(event);
        }

        None
    }

    fn unloaded_ancestor_hides_path(snapshot: &worktree::Snapshot, path: &RelPath) -> bool {
        path.ancestors()
            .skip(1)
            .find_map(|ancestor| snapshot.entry_for_path(ancestor))
            .is_some_and(|entry| entry.kind == worktree::EntryKind::UnloadedDir)
    }

    fn save_buffer(&self, buffer: Entity<Buffer>, cx: &mut Context<BufferStore>) -> Task<Result<()>> {
        let Some(file) = File::from_dyn(buffer.read(cx).file()) else {
            return Task::ready(Err(anyhow!("buffer doesn't have a file")));
        };
        let worktree = file.worktree.clone();
        self.save_local_buffer(buffer, worktree, Arc::clone(&file.path), false, cx)
    }

    fn save_buffer_as(
        &self,
        buffer: Entity<Buffer>,
        path: ProjectPath,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<()>> {
        let Some(worktree) = self.worktree_store.read(cx).worktree_for_id(path.worktree_id, cx) else {
            return Task::ready(Err(anyhow!("no such worktree")));
        };
        self.save_local_buffer(buffer, worktree, path.path, true, cx)
    }

    fn open_buffer(
        &self,
        path: Arc<RelPath>,
        worktree: Entity<Worktree>,
        cx: &mut Context<BufferStore>,
    ) -> Task<Result<Entity<Buffer>>> {
        let load_file = worktree.update(cx, |worktree, cx| worktree.load_file(path.as_ref(), cx));
        cx.spawn(async move |this, cx| {
            let path = Arc::clone(&path);
            let buffer = match load_file.await {
                Ok(loaded) => {
                    let reservation = cx.reserve_entity::<Buffer>();
                    let buffer_id = BufferId::from(reservation.entity_id().as_non_zero_u64());
                    let text_buffer = cx
                        .background_spawn(async move {
                            text::Buffer::new_normalized(buffer_id, loaded.line_ending, loaded.text)
                        })
                        .await;
                    cx.insert_entity(reservation, |cx| {
                        let capability = worktree_file_capability(&loaded.file, loaded.is_writable, cx);
                        let mut buffer = Buffer::build(text_buffer, Some(loaded.file), capability, cx);
                        // Bulk loads (project search, multi-file diffs) mostly never get
                        // displayed; editors start the parse for the buffers they draw.
                        buffer.defer_parsing();
                        buffer.set_encoding(loaded.encoding);
                        buffer.set_has_bom(loaded.has_bom);
                        buffer
                    })
                }
                Err(error) if is_not_found_error(&error) => cx.new(|cx| {
                    let buffer_id = BufferId::from(cx.entity_id().as_non_zero_u64());
                    let text_buffer = text::Buffer::new(buffer_id, "");
                    let mut buffer = Buffer::build(
                        text_buffer,
                        Some(Arc::new(File { worktree, path, disk_state: DiskState::New, entry_id: None })),
                        Capability::ReadWrite,
                        cx,
                    );
                    apply_initial_line_ending(&mut buffer, cx);
                    buffer
                }),
                Err(e) => return Err(e),
            };
            this.update(cx, |this, cx| {
                this.add_buffer(buffer.clone(), cx)?;
                let buffer_id = buffer.read(cx).buffer_id();
                if let Some(file) = File::from_dyn(buffer.read(cx).file()) {
                    let project_path = ProjectPath { worktree_id: file.worktree_id(cx), path: Arc::clone(&file.path) };
                    let entry_id = file.entry_id;
                    this.path_to_buffer_id.insert(project_path, buffer_id);
                    let this = &mut this.local;
                    if let Some(entry_id) = entry_id {
                        this.local_buffer_ids_by_entry_id.insert(entry_id, buffer_id);
                    }
                }

                anyhow::Ok(())
            })??;

            Ok(buffer)
        })
    }

    fn create_buffer(
        &self,
        language: Option<Arc<Language>>,
        project_searchable: bool,
        cx: &Context<BufferStore>,
    ) -> Task<Result<Entity<Buffer>>> {
        cx.spawn(async move |buffer_store, cx| {
            let buffer = cx.new(|cx| {
                let mut buffer =
                    Buffer::local("", cx).with_language(language.unwrap_or_else(|| language::PLAIN_TEXT.clone()), cx);
                apply_initial_line_ending(&mut buffer, cx);
                buffer
            });
            buffer_store.update(cx, |buffer_store, cx| {
                buffer_store.add_buffer(buffer.clone(), cx).log_err();
                if !project_searchable {
                    buffer_store.non_searchable_buffers.insert(buffer.read(cx).buffer_id());
                }
            })?;
            Ok(buffer)
        })
    }

    fn reload_buffers(
        &self,
        buffers: FxHashSet<Entity<Buffer>>,
        push_to_history: bool,
        cx: &Context<BufferStore>,
    ) -> Task<Result<ProjectTransaction>> {
        cx.spawn(async move |_, cx| {
            let mut project_transaction = ProjectTransaction::default();
            for buffer in buffers {
                let transaction = buffer.update(cx, |buffer, cx| buffer.reload(cx)).await?;
                buffer.update(cx, |buffer, cx| {
                    if let Some(transaction) = transaction {
                        if !push_to_history {
                            buffer.forget_transaction(transaction.id);
                        }
                        project_transaction.0.insert(cx.entity(), transaction);
                    }
                });
            }

            Ok(project_transaction)
        })
    }
}

impl BufferStore {
    /// Creates a buffer store, optionally retaining its buffers.
    pub fn local(worktree_store: Entity<WorktreeStore>, cx: &mut Context<Self>) -> Self {
        Self {
            local: LocalBufferStore {
                local_buffer_ids_by_entry_id: FxHashMap::default(),
                worktree_store: worktree_store.clone(),
                _subscription: cx.subscribe(&worktree_store, |this, _, event, cx| {
                    if let WorktreeStoreEvent::WorktreeAdded(worktree) = event {
                        this.local.subscribe_to_worktree(worktree, cx);
                    }
                }),
            },
            opened_buffers: FxHashMap::default(),
            path_to_buffer_id: FxHashMap::default(),
            loading_buffers: FxHashMap::default(),
            non_searchable_buffers: FxHashSet::default(),
            worktree_store,
        }
    }

    pub fn open_buffer(&mut self, project_path: ProjectPath, cx: &mut Context<Self>) -> Task<Result<Entity<Buffer>>> {
        if let Some(buffer) = self.get_by_path(&project_path) {
            return Task::ready(Ok(buffer));
        }

        let task = match self.loading_buffers.entry(project_path.clone()) {
            hash_map::Entry::Occupied(e) => e.get().clone(),
            hash_map::Entry::Vacant(entry) => {
                let path = Arc::clone(&project_path.path);
                let Some(worktree) = self.worktree_store.read(cx).worktree_for_id(project_path.worktree_id, cx) else {
                    return Task::ready(Err(anyhow!("no such worktree")));
                };
                let is_library = worktree.read(cx).library_name().is_some();
                let load_buffer = self.local.open_buffer(path, worktree, cx);

                entry
                    .insert(
                        cx.spawn(async move |this, cx| {
                            let load_result = load_buffer.await;
                            this.update(cx, |this, cx| {
                                // Record the fact that the buffer is no longer loading.
                                this.loading_buffers.remove(&project_path);

                                let buffer = load_result.map_err(Arc::new)?;
                                if is_library {
                                    buffer.update(cx, |buffer, cx| buffer.set_capability(Capability::ReadOnly, cx));
                                }
                                Ok(buffer)
                            })?
                        })
                        .shared(),
                    )
                    .clone()
            }
        };

        cx.background_spawn(async move { task.await.map_err(|e| anyhow!("{e}")) })
    }

    pub fn create_buffer(
        &mut self,
        language: Option<Arc<Language>>,
        project_searchable: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<Buffer>>> {
        self.local.create_buffer(language, project_searchable, cx)
    }

    pub fn save_buffer(&mut self, buffer: Entity<Buffer>, cx: &mut Context<Self>) -> Task<Result<()>> {
        self.local.save_buffer(buffer, cx)
    }

    pub fn save_buffer_as(
        &mut self,
        buffer: Entity<Buffer>,
        path: ProjectPath,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let old_file = buffer.read(cx).file().cloned();
        let task = self.local.save_buffer_as(buffer.clone(), path, cx);
        cx.spawn(async move |this, cx| {
            task.await?;
            this.update(cx, |this, cx| {
                old_file.clone().and_then(|file| {
                    this.path_to_buffer_id
                        .remove(&ProjectPath { worktree_id: file.worktree_id(cx), path: Arc::clone(file.path()) })
                });

                cx.emit(BufferStoreEvent::BufferChangedFilePath { buffer, old_file });
            })
        })
    }

    fn add_buffer(&mut self, buffer_entity: Entity<Buffer>, cx: &mut Context<Self>) -> Result<()> {
        let buffer = buffer_entity.read(cx);
        let buffer_id = buffer.buffer_id();
        let path = File::from_dyn(buffer.file())
            .map(|file| ProjectPath { path: Arc::clone(&file.path), worktree_id: file.worktree_id(cx) });

        if self.opened_buffers.get(&buffer_id).is_some_and(|buffer| buffer.upgrade().is_some()) {
            anyhow::bail!("buffer {buffer_id} was already registered");
        }
        self.opened_buffers.insert(buffer_id, buffer_entity.downgrade());

        let handle = cx.entity().downgrade();
        buffer_entity.update(cx, move |_, cx| {
            cx.on_release(move |buffer, cx| {
                handle.update(cx, |_, cx| cx.emit(BufferStoreEvent::BufferDropped(buffer.buffer_id()))).ok();
            })
            .detach();
        });

        if let Some(path) = path {
            self.path_to_buffer_id.insert(path, buffer_id);
        }

        cx.subscribe(&buffer_entity, Self::on_buffer_event).detach();
        cx.emit(BufferStoreEvent::BufferAdded(buffer_entity));
        Ok(())
    }

    pub fn buffers(&self) -> impl '_ + Iterator<Item = Entity<Buffer>> {
        self.opened_buffers.values().filter_map(gpui::WeakEntity::upgrade)
    }

    pub(crate) fn is_searchable(&self, id: BufferId) -> bool {
        !self.non_searchable_buffers.contains(&id)
    }

    pub fn buffer_id_for_project_path(&self, project_path: &ProjectPath) -> Option<&BufferId> {
        self.path_to_buffer_id.get(project_path)
    }

    pub fn get_by_path(&self, path: &ProjectPath) -> Option<Entity<Buffer>> {
        let buffer_id = self.path_to_buffer_id.get(path)?;
        self.get(*buffer_id)
    }

    pub fn get(&self, buffer_id: BufferId) -> Option<Entity<Buffer>> {
        self.opened_buffers.get(&buffer_id)?.upgrade()
    }

    fn buffer_changed_file(&mut self, buffer: Entity<Buffer>, cx: &App) -> Option<()> {
        let file = File::from_dyn(buffer.read(cx).file())?;

        let buffer_id = buffer.read(cx).buffer_id();
        if let Some(entry_id) = file.entry_id {
            {
                let local = &mut self.local;
                match local.local_buffer_ids_by_entry_id.get(&entry_id) {
                    Some(_) => {
                        return None;
                    }
                    None => {
                        local.local_buffer_ids_by_entry_id.insert(entry_id, buffer_id);
                    }
                }
            }
            self.path_to_buffer_id
                .insert(ProjectPath { worktree_id: file.worktree_id(cx), path: Arc::clone(&file.path) }, buffer_id);
        }

        Some(())
    }

    fn on_buffer_event(&mut self, buffer: Entity<Buffer>, event: &BufferEvent, cx: &mut Context<Self>) {
        if matches!(event, BufferEvent::FileHandleChanged) {
            self.buffer_changed_file(buffer, cx);
        }
    }

    pub fn reload_buffers(
        &self,
        buffers: FxHashSet<Entity<Buffer>>,
        push_to_history: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<ProjectTransaction>> {
        if buffers.is_empty() {
            return Task::ready(Ok(ProjectTransaction::default()));
        }
        self.local.reload_buffers(buffers, push_to_history, cx)
    }

    pub fn create_local_buffer(
        &mut self,
        text: &str,
        language: Option<Arc<Language>>,
        project_searchable: bool,
        cx: &mut Context<Self>,
    ) -> Entity<Buffer> {
        let buffer = cx.new(|cx| {
            let mut buffer =
                Buffer::local(text, cx).with_language(language.unwrap_or_else(|| language::PLAIN_TEXT.clone()), cx);
            apply_initial_line_ending(&mut buffer, cx);
            buffer
        });

        self.add_buffer(buffer.clone(), cx).log_err();
        let buffer_id = buffer.read(cx).buffer_id();
        if !project_searchable {
            self.non_searchable_buffers.insert(buffer_id);
        }

        if let Some(file) = File::from_dyn(buffer.read(cx).file()) {
            self.path_to_buffer_id
                .insert(ProjectPath { worktree_id: file.worktree_id(cx), path: Arc::clone(&file.path) }, buffer_id);
            let this = &mut self.local;
            if let Some(entry_id) = file.entry_id {
                this.local_buffer_ids_by_entry_id.insert(entry_id, buffer_id);
            }
        }
        buffer
    }
}

fn is_not_found_error(error: &anyhow::Error) -> bool {
    error.root_cause().downcast_ref::<io::Error>().is_some_and(|err| err.kind() == io::ErrorKind::NotFound)
}

fn apply_initial_line_ending(buffer: &mut Buffer, cx: &mut Context<Buffer>) {
    // Only applies for empty rope or a single line with no trailing newline.
    if buffer.max_point().row > 0 {
        return;
    }
    if buffer.line_ending() != LineEnding::Unix {
        buffer.set_line_ending(LineEnding::Unix, cx);
    }
}

/// A worktree file is read-only when it lacks write permission or is a library source.
/// Re-derived whenever its worktree entry changes, local or remote.
fn worktree_file_capability(file: &File, is_writable: bool, cx: &App) -> Capability {
    file_capability(is_writable, file.worktree.read(cx).library_name().is_some())
}

const fn file_capability(is_writable: bool, is_library: bool) -> Capability {
    if is_writable && !is_library { Capability::ReadWrite } else { Capability::ReadOnly }
}

#[cfg(test)]
mod file_capability_tests {
    use language::Capability;

    use super::file_capability;

    #[test]
    fn file_capability_works() {
        assert_eq!(file_capability(true, false), Capability::ReadWrite);
        assert_eq!(file_capability(false, false), Capability::ReadOnly);
        assert_eq!(file_capability(true, true), Capability::ReadOnly);
        assert_eq!(file_capability(false, true), Capability::ReadOnly);
    }
}
