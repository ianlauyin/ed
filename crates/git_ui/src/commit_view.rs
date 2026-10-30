use std::any::Any;
use std::any::TypeId;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use buffer_diff::BufferDiff;
use collections::HashMap;
use editor::Addon;
use editor::Editor;
use editor::EditorEvent;
use editor::EditorSettings;
use editor::HiddenDiffHunkRenderer;
use editor::MultiBuffer;
use editor::SelectionEffects;
use editor::SplittableEditor;
use editor::actions::OpenExcerpts;
use editor::multibuffer_context_lines;
use editor::scroll::Autoscroll;
use futures_lite::future::yield_now;
use git::Oid;
use git::repository::CommitDetails;
use git::repository::RepoPath;
use git::status::FileStatus;
use git::status::StatusCode;
use git::status::TrackedStatus;
use gpui::AnyView;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncWindowContext;
use gpui::Context;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::InteractiveElement;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::PromptLevel;
use gpui::Render;
use gpui::Styled;
use gpui::Subscription;
use gpui::Task;
use gpui::WeakEntity;
use gpui::Window;
use gpui::actions;
use language::Buffer;
use language::Capability;
use language::DiskState;
use language::File;
use language::LanguageRegistry;
use language::LineEnding;
use language::OffsetRangeExt as _;
use language::ReplicaId;
use language::Rope;
use language::TextBuffer;
use multi_buffer::PathKey;
use project::Project;
use project::ProjectPath;
use project::WorktreeId;
use project::git_store::CommitDiff;
use project::git_store::Repository;
use project::git_store::UnshallowState;
use settings::DiffViewStyle;
use settings::Settings;
use theme::ActiveTheme;
use ui::ContextMenu;
use ui::Tooltip;
use ui::prelude::*;
use util::ResultExt;
use util::paths::PathStyle;
use util::rel_path::RelPath;
use util::truncate_and_trailoff;
use workspace::Item;
use workspace::ItemHandle;
use workspace::ItemNavHistory;
use workspace::Toast;
use workspace::ToolbarItemEvent;
use workspace::ToolbarItemLocation;
use workspace::ToolbarItemView;
use workspace::Workspace;
use workspace::WorkspaceMode;
use workspace::item::ItemEvent;
use workspace::item::TabTooltipContent;
use workspace::notifications::NotificationId;
use workspace::notifications::NotifyTaskExt;
use workspace::pane;
use workspace::pane::SaveIntent;
use workspace::searchable::SearchableItemHandle;

use crate::commit_details_panel::CommitDetailsEvent;
use crate::commit_details_panel::CommitDetailsPanel;
use crate::git_panel::GitPanel;

actions!(git, [ApplyCurrentStash, PopCurrentStash, DropCurrentStash, OpenFileAtHead,]);

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, _: &ApplyCurrentStash, window, cx| {
            CommitView::apply_stash(workspace, window, cx);
        });
        workspace.register_action(|workspace, _: &DropCurrentStash, window, cx| {
            CommitView::remove_stash(workspace, window, cx);
        });
        workspace.register_action(|workspace, _: &PopCurrentStash, window, cx| {
            CommitView::pop_stash(workspace, window, cx);
        });
    })
    .detach();
}

pub struct CommitView {
    commit: CommitDetails,
    editor: Entity<SplittableEditor>,
    details: Entity<CommitDetailsPanel>,
    stash: Option<usize>,
    multibuffer: Entity<MultiBuffer>,
    repository: Entity<Repository>,
    workspace: WeakEntity<Workspace>,
    is_shallow_boundary: bool,
    file_filter: Option<RepoPath>,
    _load_diff_task: Task<Result<()>>,
    _revision_subscriptions: Vec<Subscription>,
}

pub(crate) struct GitBlob {
    pub(crate) path: RepoPath,
    pub(crate) worktree_id: WorktreeId,
    pub(crate) is_deleted: bool,
    pub(crate) is_binary: bool,
    pub(crate) display_name: String,
}

pub(crate) fn worktree_id_for_repo_path(
    repository: &Repository,
    project: &Project,
    path: &RepoPath,
    cx: &App,
) -> Option<WorktreeId> {
    repository
        .repo_path_to_project_path(path, cx)
        .map(|project_path| project_path.worktree_id)
        .or_else(|| {
            let (worktree, _) = project.find_worktree(&repository.work_directory_abs_path, cx)?;
            Some(worktree.read(cx).id())
        })
        .or_else(|| project.worktrees(cx).next().map(|worktree| worktree.read(cx).id()))
}

/// Opens the project file at `repo_path`, or shows a toast when the commit's
/// file is no longer in the project.
pub(crate) fn open_project_file(
    workspace: &WeakEntity<Workspace>,
    repository: &Entity<Repository>,
    repo_path: &RepoPath,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(workspace) = workspace.upgrade() else {
        return;
    };
    let project_path = repository.read(cx).repo_path_to_project_path(repo_path, cx).filter(|project_path| {
        workspace.read(cx).project().read(cx).entry_for_path(project_path, cx).is_some_and(|entry| entry.is_file())
    });
    workspace.update(cx, |workspace, cx| match project_path {
        Some(project_path) => {
            workspace.open_path_preview(project_path, None, true, false, true, window, cx).detach_and_log_err(cx);
        }
        None => {
            let message = project_file_not_found_message(repo_path);
            workspace.show_toast(Toast::new(NotificationId::unique::<ProjectFileNotFound>(), message), cx);
        }
    });
}

struct ProjectFileNotFound;

fn project_file_not_found_message(repo_path: &RepoPath) -> String {
    format!("{} is not in the project. It may have been moved or deleted.", repo_path.display(PathStyle::local()))
}

/// The changed file a commit view shows once its diff loads.
enum InitialFile {
    Scroll(RepoPath),
    Revision(RepoPath),
}

struct CommitDiffAddon {
    file_statuses: HashMap<language::BufferId, FileStatus>,
    commit_view: WeakEntity<CommitView>,
}

impl Addon for CommitDiffAddon {
    fn to_any(&self) -> &dyn std::any::Any {
        self
    }

    fn override_status_for_buffer_id(&self, buffer_id: language::BufferId, _cx: &App) -> Option<FileStatus> {
        self.file_statuses.get(&buffer_id).copied()
    }

    fn open_file_label(&self) -> Option<&'static str> {
        Some("Open Revision")
    }

    fn extend_buffer_header_context_menu(
        &self,
        menu: ContextMenu,
        buffer: &language::BufferSnapshot,
        _window: &mut Window,
        cx: &mut App,
    ) -> ContextMenu {
        let file_to_open = buffer.file().and_then(|file| {
            let commit_view = self.commit_view.upgrade()?;
            let commit_view = commit_view.read(cx);
            let project_path =
                commit_view.repository.read(cx).repo_path_to_project_path(&RepoPath::from_rel_path(file.path()), cx)?;
            let exists_at_head = commit_view
                .workspace
                .upgrade()?
                .read(cx)
                .project()
                .read(cx)
                .entry_for_path(&project_path, cx)
                .is_some();
            exists_at_head.then(|| file.clone())
        });

        menu.when_some(file_to_open, |menu, file| {
            let commit_view = self.commit_view.clone();
            menu.entry("Open File in Project", Some(Box::new(OpenFileAtHead)), move |window, cx| {
                commit_view.update(cx, |view, cx| view.open_file_at_head(&file, window, cx)).log_err();
            })
        })
    }
}

const FILE_NAMESPACE_SORT_PREFIX: u64 = 1;

impl CommitView {
    pub fn open(
        commit_sha: String,
        repo: WeakEntity<Repository>,
        workspace: WeakEntity<Workspace>,
        stash: Option<usize>,
        file_filter: Option<RepoPath>,
        window: &mut Window,
        cx: &mut App,
    ) {
        Self::open_with_options(commit_sha, repo, workspace, stash, file_filter, false, None, window, cx)
    }

    /// Opens the commit view and, once its diff loads, scrolls to `repo_path`.
    pub fn open_at_file(
        commit_sha: String,
        repo: WeakEntity<Repository>,
        workspace: WeakEntity<Workspace>,
        repo_path: RepoPath,
        window: &mut Window,
        cx: &mut App,
    ) {
        let initial_file = Some(InitialFile::Scroll(repo_path));
        CommitView::open_with_options(commit_sha, repo, workspace, None, None, false, initial_file, window, cx)
    }

    /// Opens the commit view and, once its diff loads, `repo_path`'s revision tab.
    pub fn open_revision(
        commit_sha: String,
        repo: WeakEntity<Repository>,
        workspace: WeakEntity<Workspace>,
        repo_path: RepoPath,
        window: &mut Window,
        cx: &mut App,
    ) {
        let initial_file = Some(InitialFile::Revision(repo_path));
        CommitView::open_with_options(commit_sha, repo, workspace, None, None, false, initial_file, window, cx)
    }

    fn open_with_options(
        commit_sha: String,
        repo: WeakEntity<Repository>,
        workspace: WeakEntity<Workspace>,
        stash: Option<usize>,
        file_filter: Option<RepoPath>,
        ignore_shallow_boundary: bool,
        initial_file: Option<InitialFile>,
        window: &Window,
        cx: &mut App,
    ) {
        let commit_diff =
            repo.update(cx, |repo, cx| repo.load_commit_diff(commit_sha.clone(), ignore_shallow_boundary, cx)).ok();
        let commit_details = repo.update(cx, |repo, cx| repo.show_commit(commit_sha.clone(), cx)).ok();

        window
            .spawn(cx, async move |cx| {
                let commit_diff = commit_diff?;
                let commit_details = commit_details?;
                let (commit_diff, commit_details) = futures::join!(commit_diff, commit_details);
                let mut commit_diff = commit_diff.log_err()?;
                let commit_details = commit_details.log_err()?;

                // Filter to specific file if requested
                if let Some(ref filter_path) = file_filter {
                    commit_diff.files.retain(|f| &f.path == filter_path);
                }

                let repo = repo.upgrade()?;

                workspace
                    .update_in(cx, |workspace, window, cx| {
                        let project = workspace.project();
                        let workspace_entity = cx.entity();
                        let workspace_handle = cx.weak_entity();
                        let commit_view = cx.new(|cx| {
                            CommitView::new(
                                commit_details,
                                commit_diff,
                                repo,
                                project.clone(),
                                workspace_entity,
                                workspace_handle,
                                stash,
                                file_filter,
                                initial_file,
                                window,
                                cx,
                            )
                        });

                        let pane = workspace.git_history_pane().clone();
                        commit_view.update(cx, |view, cx| {
                            let subscription = cx.subscribe_in(&pane, window, |view, _, event, window, cx| {
                                if let pane::Event::AddItem { item } = event
                                    && let Some(editor) = item.downcast::<Editor>()
                                {
                                    view.adopt_revision_editor(&editor, window, cx);
                                }
                            });
                            view._revision_subscriptions.push(subscription);
                        });
                        pane.update(cx, |pane, cx| {
                            let ix = pane.items().position(|item| {
                                let commit_view = item.downcast::<CommitView>();
                                commit_view.is_some_and(|view| view.read(cx).commit.sha == commit_sha)
                            });
                            if let Some(ix) = ix {
                                let existing = pane
                                    .items()
                                    .filter_map(|item| item.downcast::<CommitView>())
                                    .find(|view| view.read(cx).commit.sha == commit_sha)
                                    .unwrap();

                                pane.remove_item(existing.item_id(), false, false, window, cx);
                                pane.add_item(Box::new(commit_view), true, true, Some(ix), window, cx);
                            } else {
                                pane.add_item(Box::new(commit_view), true, true, None, window, cx);
                            }
                        });
                        workspace.set_mode(WorkspaceMode::GitHistory, window, cx);
                    })
                    .log_err()
            })
            .detach();
    }

    fn new(
        commit: CommitDetails,
        commit_diff: CommitDiff,
        repository: Entity<Repository>,
        project: Entity<Project>,
        workspace_entity: Entity<Workspace>,
        workspace: WeakEntity<Workspace>,
        stash: Option<usize>,
        file_filter: Option<RepoPath>,
        initial_file: Option<InitialFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let language_registry = project.read(cx).languages().clone();
        let is_shallow_boundary = commit_diff.is_shallow_boundary;
        let multibuffer = cx.new(|cx| {
            let mut multibuffer = MultiBuffer::new(Capability::ReadOnly);
            multibuffer.set_all_diff_hunks_expanded(cx);
            multibuffer
        });

        let details = cx.new(|cx| {
            let mut details = CommitDetailsPanel::new(Some(language_registry.clone()));
            if let Ok(sha) = commit.sha.parse::<Oid>() {
                details.set_commit(&repository, sha, Vec::new(), gpui::Hsla::default(), cx);
                details.set_diff(&commit_diff, cx);
            }
            details
        });
        cx.subscribe_in(&details, window, |this: &mut Self, _, event, window, cx| match event {
            CommitDetailsEvent::OpenFile(repo_path) => {
                this.details.update(cx, |details, cx| details.reveal_file(repo_path, cx));
                if this.is_revision_tab_active(cx) {
                    this.open_file_revision(repo_path, window, cx);
                } else {
                    this.scroll_to_file(repo_path, window, cx);
                }
            }
            CommitDetailsEvent::OpenRevision(repo_path) => {
                this.details.update(cx, |details, cx| details.reveal_file(repo_path, cx));
                this.open_file_revision(repo_path, window, cx);
            }
            CommitDetailsEvent::OpenProjectFile(repo_path) => {
                open_project_file(&this.workspace, &this.repository, repo_path, window, cx);
            }
            CommitDetailsEvent::DeployRefMenu { .. } => {}
        })
        .detach();

        let editor = cx.new(|cx| {
            let editor = SplittableEditor::new(
                EditorSettings::get_global(cx).diff_view_style,
                multibuffer.clone(),
                project.clone(),
                workspace_entity.clone(),
                window,
                cx,
            );
            editor.set_diff_hunk_renderer(Some(Arc::new(HiddenDiffHunkRenderer)), cx);

            editor
        });
        let commit_sha = Arc::<str>::from(commit.sha.as_ref());

        let repository_clone = repository.clone();
        let project_clone = project.clone();

        let load_diff_task = cx.spawn_in(window, async move |this, cx| {
            let mut binary_buffer_ids: HashSet<language::BufferId> = HashSet::default();
            let mut file_statuses: HashMap<language::BufferId, FileStatus> = HashMap::default();

            for file in commit_diff.files {
                let is_created = file.old_text.is_none();
                let is_deleted = file.new_text.is_none();
                let raw_new_text = file.new_text.unwrap_or_default();
                let raw_old_text = file.old_text;

                let is_binary = file.is_binary;

                let new_text = if is_binary { "(binary file not shown)".to_string() } else { raw_new_text };
                let old_text = if is_binary { None } else { raw_old_text };
                let worktree_id = repository_clone
                    .read_with(cx, |repository, cx| {
                        worktree_id_for_repo_path(repository, project_clone.read(cx), &file.path, cx)
                    })
                    .context("project has no worktrees")?;
                let short_sha = commit_sha.get(0..git::SHORT_SHA_LENGTH).unwrap_or(&commit_sha);
                let file_name = file
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| file.path.display(PathStyle::local()).to_string());
                let display_name = format!("{short_sha} - {file_name}");

                let file =
                    Arc::new(GitBlob { path: file.path.clone(), is_deleted, is_binary, worktree_id, display_name })
                        as Arc<dyn language::File>;

                let buffer = build_buffer(new_text, file, &language_registry, cx).await?;
                let buffer_id = cx.update(|_, cx| buffer.read(cx).remote_id())?;

                let status_code = if is_created {
                    StatusCode::Added
                } else if is_deleted {
                    StatusCode::Deleted
                } else {
                    StatusCode::Modified
                };
                file_statuses.insert(
                    buffer_id,
                    FileStatus::Tracked(TrackedStatus {
                        index_status: status_code,
                        worktree_status: StatusCode::Unmodified,
                    }),
                );

                if is_binary {
                    binary_buffer_ids.insert(buffer_id);
                }

                let buffer_diff = if is_binary {
                    cx.update(|_, cx| {
                        let snapshot = buffer.read(cx).snapshot();
                        cx.new(|cx| {
                            BufferDiff::new_unchanged(
                                &snapshot,
                                snapshot.language().cloned(),
                                Some(language_registry.clone()),
                                cx,
                            )
                        })
                    })?
                } else {
                    build_buffer_diff(old_text, &buffer, &language_registry, cx).await?
                };

                let (excerpt_ranges, path) = cx.update(|_, cx| {
                    let snapshot = buffer.read(cx).snapshot();
                    let path =
                        PathKey::with_sort_prefix(FILE_NAMESPACE_SORT_PREFIX, snapshot.file().unwrap().path().clone());
                    let ranges = if is_binary {
                        vec![language::Point::zero()..snapshot.max_point()]
                    } else {
                        let diff_snapshot = buffer_diff.read(cx).snapshot(cx);
                        let mut hunks = diff_snapshot.hunks(&snapshot).peekable();
                        if hunks.peek().is_none() {
                            vec![language::Point::zero()..snapshot.max_point()]
                        } else {
                            hunks.map(|hunk| hunk.buffer_range.to_point(&snapshot)).collect::<Vec<_>>()
                        }
                    };
                    (ranges, path)
                })?;

                // Batch the insertion of excerpts and yield between batches, to avoid blocking the main thread when a single file has many hunks.
                const EXCERPT_BATCH_SIZE: usize = 10;
                let total = excerpt_ranges.len();
                let mut batch_end = 0;
                while batch_end < total {
                    let is_first_batch = batch_end == 0;
                    batch_end = (batch_end + EXCERPT_BATCH_SIZE).min(total);
                    let ranges = excerpt_ranges[..batch_end].to_vec();
                    this.update_in(cx, |this, window, cx| {
                        this.editor.update(cx, |editor, cx| {
                            editor.update_excerpts_for_path(
                                path.clone(),
                                buffer.clone(),
                                ranges,
                                multibuffer_context_lines(cx),
                                buffer_diff.clone(),
                                cx,
                            );
                            if is_first_batch && editor.diff_view_style() == DiffViewStyle::Split {
                                editor.split(window, cx);
                            }
                        });
                    })?;
                    if batch_end < total {
                        yield_now().await;
                    }
                }
            }

            this.update(cx, |this, cx| {
                let commit_view = cx.weak_entity();
                this.editor.update(cx, |editor, cx| {
                    editor.rhs_editor().update(cx, |editor, _cx| {
                        editor.register_addon(CommitDiffAddon { file_statuses, commit_view });
                    });
                });
                if !binary_buffer_ids.is_empty() {
                    this.editor.update(cx, |editor, cx| {
                        editor.rhs_editor().update(cx, |editor, cx| {
                            editor.fold_buffers(binary_buffer_ids, cx);
                        });
                    });
                }
            })?;

            if let Some(initial_file) = initial_file {
                this.update_in(cx, |this, window, cx| match &initial_file {
                    InitialFile::Scroll(repo_path) => {
                        this.details.update(cx, |details, cx| details.reveal_file(repo_path, cx));
                        this.scroll_to_file(repo_path, window, cx);
                    }
                    InitialFile::Revision(repo_path) => {
                        this.details.update(cx, |details, cx| details.reveal_file(repo_path, cx));
                        this.open_file_revision(repo_path, window, cx);
                    }
                })?;
            }

            anyhow::Ok(())
        });

        Self {
            commit,
            editor,
            details,
            multibuffer,
            stash,
            repository,
            workspace,
            is_shallow_boundary,
            file_filter,
            _load_diff_task: load_diff_task,
            _revision_subscriptions: Vec::new(),
        }
    }

    fn render_shallow_boundary_notice(&self, cx: &App) -> impl IntoElement {
        let commit_sha = self.commit.sha.to_string();
        let repository = self.repository.clone();
        let workspace = self.workspace.clone();
        let stash = self.stash;
        let file_filter = self.file_filter.clone();
        let unshallow_state = self.repository.read(cx).unshallow_state();
        let can_fetch = unshallow_state != UnshallowState::Unshallowed;
        let fetch_in_flight = unshallow_state == UnshallowState::InProgress;
        v_flex()
            .flex_grow(1.)
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Label::new("This commit is at the boundary of a shallow clone.")
                    .color(Color::Muted),
            )
            .child(
                Label::new(
                    "Its parent history was not fetched, so the changes it introduced cannot be shown.",
                )
                .color(Color::Muted),
            )
            .child(
                h_flex()
                    .gap_2()
                    .when(can_fetch, |this| {
                        let commit_sha = commit_sha.clone();
                        let repository = repository.clone();
                        let workspace = workspace.clone();
                        let file_filter = file_filter.clone();
                        this.child(
                            Button::new(
                                "fetch-unshallow",
                                if fetch_in_flight {
                                    "Fetching…"
                                } else {
                                    "Fetch Missing History"
                                },
                            )
                                .style(ButtonStyle::Filled)
                                .disabled(fetch_in_flight)
                                .tooltip(Tooltip::text(
                                    "Run `git fetch --unshallow` to download the full history, then show this commit's changes.",
                                ))
                                .on_click(move |_, window, cx| {
                                    let fetch = crate::commit_tooltip::fetch_unshallow(
                                        repository.clone(),
                                        workspace.clone(),
                                        window,
                                        cx,
                                    );
                                    let commit_sha = commit_sha.clone();
                                    let repository = repository.downgrade();
                                    let workspace = workspace.clone();
                                    let file_filter = file_filter.clone();
                                    window
                                        .spawn(cx, async move |cx| {
                                            fetch.await?;
                                            cx.update(|window, cx| {
                                                Self::open_with_options(
                                                    commit_sha,
                                                    repository,
                                                    workspace,
                                                    stash,
                                                    file_filter,
                                                    false,
                                                    None,
                                                    window,
                                                    cx,
                                                )
                                            })
                                        })
                                        .detach_and_log_err(cx);
                                }),
                        )
                    })
                    .child(
                        Button::new(
                            "load-shallow-snapshot",
                            if file_filter.is_some() {
                                "Load File Snapshot"
                            } else {
                                "Load Full Snapshot"
                            },
                        )
                            .style(ButtonStyle::Outlined)
                            .tooltip(Tooltip::text(if file_filter.is_some() {
                                "Show this file's full contents at this commit as added."
                            } else {
                                "Show every file at this commit as added. This can be slow in large repositories."
                            }))
                            .on_click(move |_, window, cx| {
                                Self::open_with_options(
                                    commit_sha.clone(),
                                    repository.downgrade(),
                                    workspace.clone(),
                                    stash,
                                    file_filter.clone(),
                                    true,
                                    None,
                                    window,
                                    cx,
                                );
                            }),
                    ),
            )
    }

    /// Gives a tab showing one of this commit's file revisions the commit's
    /// sidebar, revealing that file whenever the tab is focused.
    fn adopt_revision_editor(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(buffer) = self.revision_buffer(editor, cx) else {
            return;
        };
        let Some(file) = buffer.read(cx).file() else {
            return;
        };
        let repo_path = RepoPath::from_rel_path(file.path());
        editor.update(cx, |editor, _| editor.set_side_card(Some(self.details.clone().into())));
        self.details.update(cx, |details, cx| details.reveal_file(&repo_path, cx));
        let focus_handle = editor.read(cx).focus_handle(cx);
        self._revision_subscriptions.push(cx.on_focus_in(&focus_handle, window, move |this, _, cx| {
            this.details.update(cx, |details, cx| details.reveal_file(&repo_path, cx));
        }));
    }

    /// The buffer of `editor` when it shows one of this commit's file revisions.
    fn revision_buffer(&self, editor: &Entity<Editor>, cx: &App) -> Option<Entity<language::Buffer>> {
        let buffer = editor.read(cx).buffer().read(cx).as_singleton()?;
        self.multibuffer.read(cx).buffer(buffer.read(cx).remote_id())
    }

    /// Whether the history pane is showing one of this commit's file
    /// revisions, i.e. this commit's sidebar is beside a revision tab.
    fn is_revision_tab_active(&self, cx: &App) -> bool {
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        let active_item = workspace.read(cx).git_history_pane().read(cx).active_item();
        active_item
            .and_then(|item| item.downcast::<Editor>())
            .is_some_and(|editor| self.revision_buffer(&editor, cx).is_some())
    }

    /// Moves the diff's cursor to the first hunk of `repo_path`, returning
    /// false when the commit has no excerpt for it.
    fn select_file(&self, repo_path: &RepoPath, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let path_key = PathKey::with_sort_prefix(FILE_NAMESPACE_SORT_PREFIX, repo_path.as_ref().clone());
        let Some(position) = self.multibuffer.read(cx).location_for_path(&path_key, cx) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.rhs_editor().update(cx, |editor, cx| {
                editor.change_selections(SelectionEffects::scroll(Autoscroll::focused()), window, cx, |s| {
                    s.select_ranges([position..position]);
                });
            })
        });
        true
    }

    fn scroll_to_file(&self, repo_path: &RepoPath, window: &mut Window, cx: &mut Context<Self>) {
        if !self.select_file(repo_path, window, cx) {
            return;
        }
        let focus_handle = self.editor.read(cx).rhs_editor().read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    /// Opens `repo_path`'s revision tab at its first hunk, the same as
    /// "Open File" on its diff excerpt, keeping the diff's cursor in sync.
    fn open_file_revision(&self, repo_path: &RepoPath, window: &mut Window, cx: &mut Context<Self>) {
        if !self.select_file(repo_path, window, cx) {
            return;
        }
        self.editor.update(cx, |editor, cx| {
            editor.rhs_editor().update(cx, |editor, cx| editor.open_excerpts(&OpenExcerpts, window, cx))
        });
    }

    fn open_file_at_head(&self, file: &Arc<dyn language::File>, window: &mut Window, cx: &mut Context<Self>) {
        let rel_path = file.path().clone();
        let worktree_id = file.worktree_id(cx);
        let repo_path = RepoPath::from_rel_path(&rel_path);
        let project_path = self
            .repository
            .read(cx)
            .repo_path_to_project_path(&repo_path, cx)
            .unwrap_or(project::ProjectPath { worktree_id, path: rel_path });

        self.workspace
            .update(cx, |workspace, cx| {
                workspace.open_path_preview(project_path, None, true, false, true, window, cx).detach_and_log_err(cx);
            })
            .log_err();
    }

    fn open_file_at_head_action(&mut self, _: &OpenFileAtHead, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self
            .editor
            .read(cx)
            .focused_editor()
            .read(cx)
            .active_buffer(cx)
            .and_then(|buffer| buffer.read(cx).file().cloned())
        else {
            return;
        };
        self.open_file_at_head(&file, window, cx);
    }

    fn apply_stash(workspace: &Workspace, window: &mut Window, cx: &mut App) {
        Self::stash_action(
            workspace,
            "Apply",
            window,
            cx,
            async move |repository, sha, stash, commit_view, workspace, cx| {
                let result = repository.update(cx, |repo, cx| {
                    if !stash_matches_index(sha, stash, repo) {
                        return Err(anyhow::anyhow!("Stash has changed, not applying"));
                    }
                    Ok(repo.stash_apply(Some(stash), cx))
                });

                match result {
                    Ok(task) => task.await?,
                    Err(err) => {
                        Self::close_commit_view(commit_view, workspace, cx).await?;
                        return Err(err);
                    }
                };
                Self::close_commit_view(commit_view, workspace, cx).await?;
                anyhow::Ok(())
            },
        );
    }

    fn pop_stash(workspace: &Workspace, window: &mut Window, cx: &mut App) {
        Self::stash_action(
            workspace,
            "Pop",
            window,
            cx,
            async move |repository, sha, stash, commit_view, workspace, cx| {
                let result = repository.update(cx, |repo, cx| {
                    if !stash_matches_index(sha, stash, repo) {
                        return Err(anyhow::anyhow!("Stash has changed, pop aborted"));
                    }
                    Ok(repo.stash_pop(Some(stash), cx))
                });

                match result {
                    Ok(task) => task.await?,
                    Err(err) => {
                        Self::close_commit_view(commit_view, workspace, cx).await?;
                        return Err(err);
                    }
                };
                Self::close_commit_view(commit_view, workspace, cx).await?;
                anyhow::Ok(())
            },
        );
    }

    fn remove_stash(workspace: &Workspace, window: &mut Window, cx: &mut App) {
        Self::stash_action(
            workspace,
            "Drop",
            window,
            cx,
            async move |repository, sha, stash, commit_view, workspace, cx| {
                let result = repository.update(cx, |repo, cx| {
                    if !stash_matches_index(sha, stash, repo) {
                        return Err(anyhow::anyhow!("Stash has changed, drop aborted"));
                    }
                    Ok(repo.stash_drop(Some(stash), cx))
                });

                match result {
                    Ok(task) => task.await??,
                    Err(err) => {
                        Self::close_commit_view(commit_view, workspace, cx).await?;
                        return Err(err);
                    }
                };
                Self::close_commit_view(commit_view, workspace, cx).await?;
                anyhow::Ok(())
            },
        );
    }

    fn stash_action<AsyncFn>(
        workspace: &Workspace,
        str_action: &str,
        window: &mut Window,
        cx: &mut App,
        callback: AsyncFn,
    ) where
        AsyncFn: AsyncFnOnce(
                Entity<Repository>,
                &SharedString,
                usize,
                Entity<CommitView>,
                WeakEntity<Workspace>,
                &mut AsyncWindowContext,
            ) -> anyhow::Result<()>
            + 'static,
    {
        let Some(commit_view) =
            workspace.git_history_pane().read(cx).active_item().and_then(|item| item.downcast::<CommitView>())
        else {
            return;
        };
        let Some(stash) = commit_view.read(cx).stash else {
            return;
        };
        let sha = commit_view.read(cx).commit.sha.clone();
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("{} stash@{{{}}}?", str_action, stash),
            None,
            &[str_action, "Cancel"],
            cx,
        );

        let workspace_weak = workspace.weak_handle();
        let commit_view_entity = commit_view;

        window
            .spawn(cx, async move |cx| {
                if answer.await != Ok(0) {
                    return anyhow::Ok(());
                }

                let Some(workspace) = workspace_weak.upgrade() else {
                    return Ok(());
                };

                let repo = workspace.update(cx, |workspace, cx| {
                    workspace.panel::<GitPanel>(cx).and_then(|p| p.read(cx).active_repository.clone())
                });

                let Some(repo) = repo else {
                    return Ok(());
                };

                callback(repo, &sha, stash, commit_view_entity, workspace_weak, cx).await?;
                anyhow::Ok(())
            })
            .detach_and_notify_err(workspace.weak_handle(), window, cx);
    }

    async fn close_commit_view(
        commit_view: Entity<CommitView>,
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> anyhow::Result<()> {
        workspace
            .update_in(cx, |workspace, window, cx| {
                let commit_view_id = commit_view.entity_id();
                workspace
                    .git_history_pane()
                    .update(cx, |pane, cx| pane.close_item_by_id(commit_view_id, SaveIntent::Skip, window, cx))
            })?
            .await?;
        anyhow::Ok(())
    }
}

impl language::File for GitBlob {
    fn as_local(&self) -> Option<&dyn language::LocalFile> {
        None
    }

    fn disk_state(&self) -> DiskState {
        DiskState::Historic { was_deleted: self.is_deleted }
    }

    fn path_style(&self, _: &App) -> PathStyle {
        PathStyle::local()
    }

    fn path(&self) -> &Arc<RelPath> {
        self.path.as_ref()
    }

    fn full_path(&self, _: &App) -> PathBuf {
        self.path.as_std_path().to_path_buf()
    }

    fn file_name<'a>(&'a self, _: &'a App) -> &'a str {
        self.display_name.as_ref()
    }

    fn worktree_id(&self, _: &App) -> WorktreeId {
        self.worktree_id
    }

    fn to_proto(&self, _cx: &App) -> language::proto::File {
        unimplemented!()
    }

    fn can_open(&self) -> bool {
        !self.is_binary
    }
}

pub(crate) async fn build_buffer(
    mut text: String,
    blob: Arc<dyn File>,
    language_registry: &Arc<language::LanguageRegistry>,
    cx: &mut AsyncWindowContext,
) -> Result<Entity<Buffer>> {
    let line_ending = LineEnding::detect(&text);
    LineEnding::normalize(&mut text);
    let text = Rope::from(text);
    let language = cx.update(|_, cx| language_registry.language_for_file(&blob, Some(&text), cx))?;
    let language = if let Some(language_id) = language {
        language_registry.load_language(language_id).await.ok().and_then(|e| e.log_err())
    } else {
        None
    };
    let buffer = cx.new(|cx| {
        let buffer =
            TextBuffer::new_normalized(ReplicaId::LOCAL, cx.entity_id().as_non_zero_u64().into(), line_ending, text);
        let mut buffer = Buffer::build(buffer, Some(blob), Capability::ReadWrite, cx);
        buffer.set_language_registry(language_registry.clone());
        buffer.set_language_async(language, cx);
        buffer
    });
    Ok(buffer)
}

async fn build_buffer_diff(
    mut old_text: Option<String>,
    buffer: &Entity<Buffer>,
    language_registry: &Arc<LanguageRegistry>,
    cx: &mut AsyncWindowContext,
) -> Result<Entity<BufferDiff>> {
    if let Some(old_text) = &mut old_text {
        LineEnding::normalize(old_text);
    }

    let language = cx.update(|_, cx| buffer.read(cx).language().cloned())?;
    let buffer = cx.update(|_, cx| buffer.read(cx).snapshot())?;

    let diff = cx.new(|cx| BufferDiff::new(&buffer.text, language, Some(language_registry.clone()), cx));

    diff.update(cx, |diff, cx| {
        diff.set_base_text(old_text.map(|old_text| Arc::from(old_text.as_str())), buffer.text.clone(), cx)
    })
    .await;

    Ok(diff)
}

impl EventEmitter<EditorEvent> for CommitView {}

impl Focusable for CommitView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Item for CommitView {
    type Event = EditorEvent;

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::GitCommit).color(Color::Muted))
    }

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        let short_sha = self.commit.sha.get(0..7).unwrap_or(&*self.commit.sha);
        let subject = truncate_and_trailoff(self.commit.message.split('\n').next().unwrap(), 20);
        format!("{short_sha} — {subject}").into()
    }

    fn tab_tooltip_content(&self, _: &App) -> Option<TabTooltipContent> {
        let short_sha = self.commit.sha.get(0..16).unwrap_or(&*self.commit.sha);
        let subject = self.commit.message.split('\n').next().unwrap();

        Some(TabTooltipContent::Custom(Box::new(Tooltip::element({
            let subject = subject.to_string();
            let short_sha = short_sha.to_string();

            move |_, _| {
                v_flex()
                    .child(Label::new(subject.clone()))
                    .child(Label::new(short_sha.clone()).color(Color::Muted).size(LabelSize::Small))
                    .into_any_element()
            }
        }))))
    }

    fn to_item_events(event: &EditorEvent, f: &mut dyn FnMut(ItemEvent)) {
        Editor::to_item_events(event, f)
    }

    fn deactivated(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.deactivated(window, cx));
    }

    fn act_as_type<'a>(
        &'a self,
        type_id: TypeId,
        self_handle: &'a Entity<Self>,
        cx: &'a App,
    ) -> Option<gpui::AnyEntity> {
        if type_id == TypeId::of::<Self>() {
            Some(self_handle.clone().into())
        } else if type_id == TypeId::of::<SplittableEditor>() {
            Some(self.editor.clone().into())
        } else if type_id == TypeId::of::<Editor>() {
            Some(self.editor.read(cx).rhs_editor().clone().into())
        } else {
            None
        }
    }

    fn as_searchable(&self, _: &Entity<Self>, _: &App) -> Option<Box<dyn SearchableItemHandle>> {
        Some(Box::new(self.editor.clone()))
    }

    fn for_each_project_item(&self, cx: &App, f: &mut dyn FnMut(gpui::EntityId, &dyn project::ProjectItem)) {
        self.editor.read(cx).for_each_project_item(cx, f)
    }

    fn active_project_path(&self, cx: &App) -> Option<ProjectPath> {
        self.editor.read(cx).active_project_path(cx)
    }

    fn set_nav_history(&mut self, nav_history: ItemNavHistory, _: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.rhs_editor().update(cx, |editor, _| {
                editor.set_nav_history(Some(nav_history));
            });
        });
    }

    fn navigate(&mut self, data: Arc<dyn Any + Send>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.editor.update(cx, |editor, cx| editor.navigate(data, window, cx))
    }

    fn added_to_workspace(&mut self, workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.added_to_workspace(workspace, window, cx));
    }

    fn side_card(&self, _: &App) -> Option<AnyView> {
        Some(self.details.clone().into())
    }
}

impl Render for CommitView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_stash = self.stash.is_some();

        v_flex()
            .key_context(if is_stash { "StashDiff" } else { "CommitDiff" })
            .on_action(cx.listener(Self::open_file_at_head_action))
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .when(!self.editor.read(cx).rhs_editor().read(cx).is_empty(cx), |this| {
                this.child(div().flex_grow(1.).child(self.editor.clone()))
            })
            .when(self.is_shallow_boundary, |this| this.child(self.render_shallow_boundary_notice(cx)))
    }
}

pub struct CommitViewToolbar {
    commit_view: Option<WeakEntity<CommitView>>,
}

impl CommitViewToolbar {
    pub const fn new() -> Self {
        Self { commit_view: None }
    }
}

impl Default for CommitViewToolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter<ToolbarItemEvent> for CommitViewToolbar {}

impl Render for CommitViewToolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(commit_view) = self.commit_view.as_ref().and_then(|w| w.upgrade()) else {
            return div();
        };

        let commit_view_ref = commit_view.read(cx);
        let is_stash = commit_view_ref.stash.is_some();
        let sha_for_graph = commit_view_ref.commit.sha.to_string();

        h_flex()
            .gap_1()
            .child(
                IconButton::new("buffer-search", IconName::MagnifyingGlass)
                    .icon_size(IconSize::Small)
                    .tooltip(move |_, cx| {
                        Tooltip::for_action("Buffer Search", &ed_actions::buffer_search::Deploy::find(), cx)
                    })
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ed_actions::buffer_search::Deploy::find()), cx);
                    }),
            )
            .when(!is_stash, |this| {
                this.child(
                    IconButton::new("show-in-git-graph", IconName::GitGraph)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Show in Git Graph"))
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(
                                Box::new(crate::git_graph::OpenAtCommit { sha: sha_for_graph.clone() }),
                                cx,
                            );
                        }),
                )
            })
    }
}

impl ToolbarItemView for CommitViewToolbar {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        if let Some(entity) = active_pane_item.and_then(|i| i.act_as::<CommitView>(cx)) {
            self.commit_view = Some(entity.downgrade());
            return ToolbarItemLocation::PrimaryRight;
        }
        self.commit_view = None;
        ToolbarItemLocation::Hidden
    }

    fn pane_focus_update(&mut self, _pane_focused: bool, _window: &mut Window, _cx: &mut Context<Self>) {}
}

fn stash_matches_index(sha: &str, stash_index: usize, repo: &Repository) -> bool {
    repo.stash_entries.entries.get(stash_index).map(|entry| entry.oid.to_string() == sha).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use collections::HashMap;
    use editor::Addon;
    use git::repository::RepoPath;
    use gpui::WeakEntity;

    use super::CommitDiffAddon;
    use super::project_file_not_found_message;

    #[test]
    fn test_project_file_not_found_message() {
        let repo_path = RepoPath::new("crates/util/src/shell.rs").unwrap();
        assert_eq!(
            project_file_not_found_message(&repo_path),
            "crates/util/src/shell.rs is not in the project. It may have been moved or deleted."
        );
    }

    #[test]
    fn test_commit_diff_addon_open_file_label() {
        let addon = CommitDiffAddon { file_statuses: HashMap::default(), commit_view: WeakEntity::new_invalid() };
        assert_eq!(addon.open_file_label(), Some("Open Revision"));
    }
}
