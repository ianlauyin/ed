use std::any::Any;
use std::collections::BTreeMap;
use std::mem;
use std::time::Duration;

use ed_actions::DecreaseBufferFontSize;
use ed_actions::IncreaseBufferFontSize;
use ed_actions::ResetBufferFontSize;
use ed_actions::git_panel::ToggleFocus;
use ed_actions::workspace::CopyPath;
use ed_actions::workspace::CopyRelativePath;
use editor::Editor;
use editor::EditorMode;
use editor::EditorStyle;
use editor::MultiBuffer;
use editor::actions::Copy;
use editor::actions::Cut;
use editor::actions::Paste;
use file_icons::FileIcons;
use futures::channel::oneshot::Canceled;
use git::Amend;
use git::Commit;
use git::ToggleCommitEditor;
use git::ToggleFillCommitEditor;
use git::ViewFile;
use git::repository::GitCommitTemplate;
use git::repository::LogSource;
use git::repository::RepoPath;
use git::stash::GitStash;
use git::status::DiffStat;
use git::status::FileStatus;
use git::status::StageStatus;
use git_ui_core::notifications::show_error_toast;
use gpui::AbsoluteLength;
use gpui::Action;
use gpui::AnyElement;
use gpui::AppContext as _;
use gpui::AsyncWindowContext;
use gpui::ClipboardItem;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::KeyContext;
use gpui::Pixels;
use gpui::PromptLevel;
use gpui::Subscription;
use gpui::Task;
use gpui::TaskExt as _;
use gpui::TextStyle;
use gpui::UniformListScrollHandle;
use gpui::WeakEntity;
use gpui::actions;
use itertools::Itertools as _;
use language::Buffer;
use menu;
use multi_buffer::ExcerptBoundaryInfo;
use project::Project;
use project::git_store::GitAccess;
use project::git_store::GitStoreEvent;
use project::git_store::Repository;
use project::git_store::RepositoryEvent;
use project::git_store::pending_op;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use strum::IntoEnumIterator;
use strum::VariantNames;
use theme::built_in::BUFFER_LINE_HEIGHT;
use ui::ContextMenu;
use ui::PopoverMenuHandle;
use ui::font;
use ui::prelude::ActiveTheme as _;
use ui::prelude::App;
use ui::prelude::BorrowAppContext as _;
use ui::prelude::Context;
use ui::prelude::Div;
use ui::prelude::Rems;
use ui::prelude::SharedString;
use ui::prelude::Styled as _;
use ui::prelude::ToggleState;
use ui::prelude::Window;
use ui::prelude::px;
use ui::prelude::rems;
use ui::prelude::v_flex;
use util::ResultExt as _;
use workspace::Item as _;
use workspace::Workspace;
use workspace::dock::DockPosition;
use workspace::dock::Panel;
use workspace::dock::PanelEvent;
use workspace::dock::SIDEBAR_MIN_WIDTH;
use workspace::notifications::NotifyTaskExt as _;

use crate::git_graph;
use crate::git_panel_view_options::GitPanelGroupBy;
use crate::git_panel_view_options::GitPanelSortBy;
use crate::git_panel_view_options::GitPanelViewOptions;
use crate::project_diff::ProjectDiff;
use crate::solo_diff_view::SoloDiffView;
use crate::staged_diff::StagedDiff;
use crate::unstaged_diff::UnstagedDiff;

mod commit;
mod discard;
mod remote;
mod render;
mod selection;
mod staging;
mod stash;

use render::GitPanelContextMenu;
use selection::MarkRangeGesture;
use staging::BulkStaging;
use staging::StageIntent;

const GIT_PANEL_KEY: &str = "GitPanel";
const UPDATE_DEBOUNCE: Duration = Duration::from_millis(50);
const DEFAULT_WIDTH: Pixels = px(360.0);
/// Initial branch for `git init` when git has no `init.defaultBranch`.
const FALLBACK_BRANCH_NAME: &str = "main";
// TODO: We should revise this part. It seems the indentation width is not aligned with the one in project panel
pub(crate) const TREE_INDENT: f32 = 16.0;
// Horizontal offset that aligns the tree indent guides with the row icon column.
pub(crate) const INDENT_GUIDE_LEFT_OFFSET: gpui::Pixels = gpui::px(19.);
/// Changed-file row height, shared with the Git History changed files list.
pub(crate) const LIST_ITEM_HEIGHT: Rems = rems(1.75);

actions!(
    git_panel,
    [
        /// Closes the git panel.
        Close,
        /// Toggles the git panel.
        Toggle,
        /// Focuses on the commit message editor.
        FocusEditor,
        /// Focuses on the changes list.
        FocusChanges,
        /// Select next git panel menu item, and show it in the diff view
        NextEntry,
        /// Select previous git panel menu item, and show it in the diff view
        PreviousEntry,
        /// Select first git panel menu item, and show it in the diff view
        FirstEntry,
        /// Select last git panel menu item, and show it in the diff view
        LastEntry,
        /// Sorts entries by path.
        SetSortByPath,
        /// Sorts entries by name.
        SetSortByName,
        /// Disables grouping entries by status.
        SetGroupByNone,
        /// Groups entries by status.
        SetGroupByStatus,
        /// Groups entries by staging state.
        SetGroupByStaging,
        /// Toggles showing entries in tree vs flat view.
        ToggleTreeView,
        /// Expands the selected entry to show its children.
        ExpandSelectedEntry,
        /// Collapses the selected entry to hide its children.
        CollapseSelectedEntry,
        /// View unstaged changes
        ViewUnstagedChanges,
        /// View staged changes
        ViewStagedChanges,
    ]
);

fn prompt<T>(msg: &str, detail: Option<&str>, window: &mut Window, cx: &mut App) -> Task<anyhow::Result<T>>
where
    T: IntoEnumIterator + VariantNames + 'static,
{
    let rx = window.prompt(PromptLevel::Info, msg, detail, T::VARIANTS, cx);
    cx.spawn(async move |_| Ok(T::iter().nth(rx.await?).expect("prompt answer is a variant index")))
}

// We only allow a single remote operation at a time to avoid concurrent
// credential prompts and competing ref/working-tree updates.
#[derive(Clone, Copy)]
pub(crate) enum RemoteOperationKind {
    Fetch,
    Pull,
    Rebase,
    Push,
}

pub fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<GitPanel>(window, cx);
    });
    workspace.register_action(|workspace, _: &Toggle, window, cx| {
        if !workspace.toggle_panel_focus::<GitPanel>(window, cx) {
            workspace.close_panel::<GitPanel>(window, cx);
        }
    });
    workspace.register_action(|workspace, _: &Commit, window, cx| {
        focus_commit_editor(workspace, false, window, cx);
    });
    workspace.register_action(|workspace, _: &Amend, window, cx| {
        focus_commit_editor(workspace, true, window, cx);
    });
    workspace.register_action(|workspace, _: &ToggleFillCommitEditor, window, cx| {
        if let Some(panel) = workspace.panel::<GitPanel>(cx) {
            panel.update(cx, |panel, cx| panel.toggle_fill_commit_editor(&ToggleFillCommitEditor, window, cx));
        }
    });
    workspace.register_action(|workspace, _: &ToggleCommitEditor, window, cx| {
        if let Some(panel) = workspace.panel::<GitPanel>(cx) {
            panel.update(cx, |panel, cx| panel.toggle_commit_editor(&ToggleCommitEditor, window, cx));
        }
    });
    workspace.register_action(|workspace, _: &git::Init, window, cx| {
        if let Some(panel) = workspace.panel::<GitPanel>(cx) {
            panel.update(cx, |panel, cx| panel.git_init(window, cx));
        }
    });
}

/// Handles `git::Commit` / `git::Amend` dispatched outside the commit editor:
/// opens the git panel and focuses its commit editor in the requested mode.
fn focus_commit_editor(workspace: &mut Workspace, amend: bool, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(git_panel) = workspace.panel::<GitPanel>(cx) else {
        return;
    };
    workspace.open_panel::<GitPanel>(window, cx);
    git_panel.update(cx, |git_panel, cx| {
        if amend {
            if git_panel.head_commit(cx).is_some() && !git_panel.amend_pending() {
                git_panel.set_amend_pending(true, cx);
                git_panel.load_last_commit_message(cx);
            }
        } else if git_panel.amend_pending() {
            git_panel.set_amend_pending(false, cx);
        }
        if git_panel.commit_editor_collapsed {
            git_panel.toggle_commit_editor(&ToggleCommitEditor, window, cx);
        }
        git_panel.focus_editor(&FocusEditor, window, cx);
    });
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
enum Section {
    Conflict,
    Tracked,
    New,
    Staged,
    Unstaged,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct GitHeaderEntry {
    header: Section,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct ProjectedChangeEntry {
    section: Section,
    index: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffTarget {
    Uncommitted,
    Staged,
    Unstaged,
}

impl GitHeaderEntry {
    pub(crate) fn contains(&self, status_entry: &GitStatusEntry, repo: &Repository) -> bool {
        let this = &self.header;
        let status = status_entry.status;
        match this {
            Section::Conflict => repo.had_conflict_on_last_merge_head_change(&status_entry.repo_path),
            Section::Tracked => !status.is_created(),
            Section::New => status.is_created(),
            // Conflicted files render only under the Conflict section, so the
            // Staged/Unstaged bulk operations must not sweep them up: "Unstage
            // All" would silently un-resolve conflicts, and "Stage All" would
            // silently mark them resolved.
            Section::Staged => {
                !repo.had_conflict_on_last_merge_head_change(&status_entry.repo_path)
                    && GitPanel::stage_status_for_entry(status_entry, repo).has_staged()
            }
            Section::Unstaged => {
                !repo.had_conflict_on_last_merge_head_change(&status_entry.repo_path)
                    && GitPanel::stage_status_for_entry(status_entry, repo).has_unstaged()
            }
        }
    }
    pub(crate) const fn title(&self) -> &'static str {
        match self.header {
            Section::Conflict => "Conflicts",
            Section::Tracked => "Tracked",
            Section::New => "Untracked",
            Section::Staged => "Staged",
            Section::Unstaged => "Unstaged",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum GitListEntry {
    Status(GitStatusEntry),
    TreeStatus(GitTreeStatusEntry),
    Directory(GitTreeDirEntry),
    Header(GitHeaderEntry),
    EmptySection(Section),
}

impl GitListEntry {
    const fn status_entry(&self) -> Option<&GitStatusEntry> {
        match self {
            GitListEntry::Status(entry) => Some(entry),
            GitListEntry::TreeStatus(entry) => Some(&entry.entry),
            _ => None,
        }
    }

    const fn directory_entry(&self) -> Option<&GitTreeDirEntry> {
        match self {
            GitListEntry::Directory(entry) => Some(entry),
            _ => None,
        }
    }

    /// Returns the tree indentation depth for this entry.
    const fn depth(&self) -> usize {
        match self {
            GitListEntry::Directory(dir) => dir.depth,
            GitListEntry::TreeStatus(status) => status.depth,
            _ => 0,
        }
    }

    const fn is_selectable(&self) -> bool {
        matches!(self, GitListEntry::Status(_) | GitListEntry::TreeStatus(_) | GitListEntry::Directory(_))
    }

    const fn repo_path(&self) -> Option<&RepoPath> {
        match self {
            GitListEntry::Status(entry) => Some(&entry.repo_path),
            GitListEntry::TreeStatus(entry) => Some(&entry.entry.repo_path),
            GitListEntry::Directory(entry) => Some(&entry.key.path),
            GitListEntry::Header(_) | GitListEntry::EmptySection(_) => None,
        }
    }
}

/// Identifies whether the current operation targets one file, one directory, or
/// multiple marked entries.
#[derive(Copy, Clone, PartialEq)]
enum SelectionTargetKind {
    File,
    Directory,
    Multiple,
}

enum GitPanelViewMode {
    Flat,
    Tree(TreeViewState),
}

impl GitPanelViewMode {
    fn from_settings(cx: &App) -> Self {
        if GitPanelViewOptions::get(cx).tree_view {
            GitPanelViewMode::Tree(TreeViewState::default())
        } else {
            GitPanelViewMode::Flat
        }
    }

    const fn tree_state(&self) -> Option<&TreeViewState> {
        match self {
            GitPanelViewMode::Tree(state) => Some(state),
            GitPanelViewMode::Flat => None,
        }
    }

    const fn tree_state_mut(&mut self) -> Option<&mut TreeViewState> {
        match self {
            GitPanelViewMode::Tree(state) => Some(state),
            GitPanelViewMode::Flat => None,
        }
    }
}

#[derive(Default)]
struct TreeViewState {
    // Maps visible index to actual entry index.
    // Length equals the number of visible entries.
    // This is needed because some entries (like collapsed directories) may be hidden.
    logical_indices: Vec<usize>,
    expanded_dirs: FxHashMap<TreeKey, bool>,
    directory_descendants: FxHashMap<TreeKey, Vec<GitStatusEntry>>,
}

impl TreeViewState {
    fn build_tree_entries(
        &mut self,
        section: Section,
        mut entries: Vec<GitStatusEntry>,
        seen_directories: &mut FxHashSet<TreeKey>,
    ) -> Vec<(GitListEntry, bool)> {
        if entries.is_empty() {
            return Vec::new();
        }

        entries.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));

        let mut root = TreeNode::default();
        for entry in entries {
            let components: Vec<&str> = entry.repo_path.components().collect();
            if components.is_empty() {
                root.files.push(entry);
                continue;
            }

            let mut current = &mut root;
            let mut current_path = String::new();

            for (ix, component) in components.iter().enumerate() {
                if ix == components.len() - 1 {
                    current.files.push(entry.clone());
                } else {
                    if !current_path.is_empty() {
                        current_path.push('/');
                    }
                    current_path.push_str(component);
                    let dir_path = RepoPath::new(&current_path).expect("repo path from status entry component");

                    let component = SharedString::from(component.to_string());

                    current = current.children.entry(component.clone()).or_insert_with(|| TreeNode {
                        name: component,
                        path: Some(dir_path),
                        ..Default::default()
                    });
                }
            }
        }

        let (flattened, _) = self.flatten_tree(&root, section, 0, seen_directories);
        flattened
    }

    fn flatten_tree(
        &mut self,
        node: &TreeNode,
        section: Section,
        depth: usize,
        seen_directories: &mut FxHashSet<TreeKey>,
    ) -> (Vec<(GitListEntry, bool)>, Vec<GitStatusEntry>) {
        let mut all_statuses = Vec::new();
        let mut flattened = Vec::new();

        for child in node.children.values() {
            let (terminal, name) = Self::compact_directory_chain(child);
            let Some(path) = terminal.path.clone().or_else(|| child.path.clone()) else {
                continue;
            };
            let (child_flattened, mut child_statuses) =
                self.flatten_tree(terminal, section, depth + 1, seen_directories);
            let key = TreeKey { section, path };
            let expanded = *self.expanded_dirs.get(&key).unwrap_or(&true);
            self.expanded_dirs.entry(key.clone()).or_insert(true);
            seen_directories.insert(key.clone());

            self.directory_descendants.insert(key.clone(), child_statuses.clone());

            flattened.push((GitListEntry::Directory(GitTreeDirEntry { key, name, depth, expanded }), true));

            if expanded {
                flattened.extend(child_flattened);
            } else {
                flattened.extend(child_flattened.into_iter().map(|(child, _)| (child, false)));
            }

            all_statuses.append(&mut child_statuses);
        }

        for file in &node.files {
            all_statuses.push(file.clone());
            flattened.push((GitListEntry::TreeStatus(GitTreeStatusEntry { entry: file.clone(), depth }), true));
        }

        (flattened, all_statuses)
    }

    fn compact_directory_chain(mut node: &TreeNode) -> (&TreeNode, SharedString) {
        let mut parts = vec![node.name.clone()];
        while node.files.is_empty() && node.children.len() == 1 {
            let Some(child) = node.children.values().next() else {
                continue;
            };
            if child.path.is_none() {
                break;
            }
            parts.push(child.name.clone());
            node = child;
        }
        let name = parts.join("/");
        (node, SharedString::from(name))
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct GitTreeStatusEntry {
    entry: GitStatusEntry,
    depth: usize,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
struct TreeKey {
    section: Section,
    path: RepoPath,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct GitTreeDirEntry {
    key: TreeKey,
    name: SharedString,
    depth: usize,
    expanded: bool,
}

#[derive(Default)]
struct TreeNode {
    name: SharedString,
    path: Option<RepoPath>,
    children: BTreeMap<SharedString, TreeNode>,
    files: Vec<GitStatusEntry>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct GitStatusEntry {
    pub(crate) repo_path: RepoPath,
    pub(crate) status: FileStatus,
    pub(crate) staging: StageStatus,
    pub(crate) diff_stat: Option<DiffStat>,
}

impl GitStatusEntry {
    fn display_name(&self) -> String {
        self.repo_path.file_name().map_or_else(|| self.repo_path.display().to_owned(), |name| name.to_owned())
    }

    fn parent_dir(&self) -> Option<String> {
        self.repo_path.parent().map(|parent| parent.display().to_owned())
    }
}

pub struct GitPanel {
    pub(crate) active_repository: Option<Entity<Repository>>,
    pub(crate) commit_editor: Entity<Editor>,
    /// Whether the commit editor should fill the vertical height of the panel.
    commit_editor_expanded: bool,
    /// Whether the commit editor is hidden, leaving only the commit button row.
    commit_editor_collapsed: bool,
    conflicted_count: usize,
    conflicted_staged_count: usize,

    entries: Vec<GitListEntry>,
    collapsed_sections: FxHashSet<Section>,
    view_mode: GitPanelViewMode,
    tree_expanded_dirs: FxHashMap<TreeKey, bool>,
    projected_entries_by_path: FxHashMap<RepoPath, SmallVec<[ProjectedChangeEntry; 2]>>,
    single_staged_entry: Option<GitStatusEntry>,
    single_tracked_entry: Option<GitStatusEntry>,
    focus_handle: FocusHandle,
    new_count: usize,
    entry_count: usize,
    changes_count: usize,
    diff_stat_total: DiffStat,
    new_staged_count: usize,
    pending_commit: Option<Task<()>>,
    pending_remote_operation: Option<RemoteOperationKind>,
    amend_pending: bool,
    original_commit_message: Option<String>,
    signoff_enabled: bool,
    skip_hooks_enabled: bool,
    pub(crate) project: Entity<Project>,
    scroll_handle: UniformListScrollHandle,
    max_width_item_index: Option<usize>,
    selected_entry: Option<usize>,
    marked_entries: FxHashSet<RepoPath>,
    marked_directories: FxHashSet<TreeKey>,
    mark_range_gesture: Option<MarkRangeGesture>,
    tracked_count: usize,
    tracked_staged_count: usize,
    section_file_counts: FxHashMap<Section, usize>,
    update_visible_entries_task: Task<()>,
    reopen_commit_buffer_task: Task<()>,
    pub(crate) workspace: WeakEntity<Workspace>,
    context_menu: Option<GitPanelContextMenu>,
    show_placeholders: bool,
    commit_template: Option<GitCommitTemplate>,
    bulk_staging: Option<BulkStaging>,
    stash_entries: GitStash,
    _commit_message_buffer_subscription: Option<Subscription>,
    git_access: Option<GitAccess>,
    commit_menu_handle: PopoverMenuHandle<ContextMenu>,
    changes_actions_menu_handle: PopoverMenuHandle<ContextMenu>,
    remote_action_menu_handle: PopoverMenuHandle<ContextMenu>,
}

const MAX_PANEL_EDITOR_LINES: usize = 6;

pub(crate) fn commit_message_editor(
    commit_message_buffer: Entity<Buffer>,
    placeholder: Option<SharedString>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Editor {
    let buffer = cx.new(|cx| MultiBuffer::singleton(commit_message_buffer, cx));
    let mut commit_editor = Editor::new(
        EditorMode::AutoHeight { min_lines: MAX_PANEL_EDITOR_LINES, max_lines: Some(MAX_PANEL_EDITOR_LINES) },
        buffer,
        None,
        window,
        cx,
    );
    commit_editor.set_use_autoclose(false);
    commit_editor.set_show_gutter(false, cx);
    commit_editor.set_use_modal_editing(true);
    commit_editor.set_show_wrap_guides(false, cx);
    commit_editor.set_show_indent_guides(false, cx);
    let placeholder = placeholder.unwrap_or_else(|| "Enter commit message".into());
    commit_editor.set_placeholder_text(&placeholder, window, cx);
    commit_editor.set_custom_context_menu(|editor, _point, window, cx| {
        let has_selection = editor.has_non_empty_selection(&editor.display_snapshot(cx));
        let focus_handle = editor.focus_handle(cx);

        Some(ContextMenu::build(window, cx, |menu, _, _| {
            menu.context(focus_handle)
                .action_disabled_when(!has_selection, "Cut", Box::new(Cut))
                .action_disabled_when(!has_selection, "Copy", Box::new(Copy))
                .action("Paste", Box::new(Paste))
        }))
    });
    commit_editor
}

const GIT_COMMIT_FONT_SIZE: Pixels = px(12.0);

/// In-memory zoom of the commit message font size, independent of UI and buffer zoom.
struct GitCommitFontSize(Pixels);

impl gpui::Global for GitCommitFontSize {}

fn git_commit_font_size(cx: &App) -> Pixels {
    cx.try_global::<GitCommitFontSize>().map_or(GIT_COMMIT_FONT_SIZE, |size| size.0)
}

impl GitPanel {
    fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Entity<Self> {
        let project = workspace.project().clone();
        let git_store = project.read(cx).git_store().clone();
        let active_repository = project.read(cx).active_repository(cx);
        let signoff_enabled = false;
        let commit_editor_collapsed = false;
        let amend_pending = false;
        let original_commit_message = None;
        let initial_commit_message = String::new();

        cx.new(|cx: &mut Context<Self>| {
            let focus_handle = cx.focus_handle();
            let mut was_view_options = GitPanelViewOptions::get(cx);
            cx.observe_global_in::<GitPanelViewOptions>(window, move |this, window, cx| {
                let view_options = GitPanelViewOptions::get(cx);
                if view_options == was_view_options {
                    return;
                }
                if view_options.tree_view != was_view_options.tree_view {
                    match (&mut this.view_mode, view_options.tree_view) {
                        (GitPanelViewMode::Tree(state), false) => {
                            this.tree_expanded_dirs = state.expanded_dirs.clone();
                            this.view_mode = GitPanelViewMode::Flat;
                        }
                        (GitPanelViewMode::Flat, true) => {
                            this.view_mode = GitPanelViewMode::Tree(TreeViewState {
                                expanded_dirs: this.tree_expanded_dirs.clone(),
                                ..Default::default()
                            });
                        }
                        _ => {}
                    }
                }
                this.bulk_staging.take();
                this.update_visible_entries(window, cx);
                was_view_options = view_options;
            })
            .detach();

            cx.observe_global::<FileIcons>(|_, cx| {
                cx.notify();
            })
            .detach();

            // just to let us render a placeholder editor.
            // Once the active git repo is set, this buffer will be replaced.
            let temporary_buffer = cx.new(|cx| Buffer::local(initial_commit_message, cx));
            let commit_editor = cx.new(|cx| commit_message_editor(temporary_buffer, None, window, cx));

            let scroll_handle = UniformListScrollHandle::new();

            cx.subscribe_in(&git_store, window, move |this, _git_store, event, window, cx| match event {
                GitStoreEvent::RepositoryUpdated(
                    _,
                    RepositoryEvent::StatusesChanged | RepositoryEvent::HeadChanged,
                    true,
                )
                | GitStoreEvent::RepositoryAdded
                | GitStoreEvent::RepositoryRemoved(_)
                | GitStoreEvent::ActiveRepositoryChanged(_) => {
                    this.schedule_update(window, cx);
                }
                GitStoreEvent::GlobalConfigurationUpdated => {
                    this.git_access = None;
                    this.schedule_update(window, cx);
                }
                GitStoreEvent::IndexWriteError(error) => {
                    this.workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_error(format!("{error}"), cx);
                        })
                        .ok();
                }
                GitStoreEvent::RepositoryUpdated(_, _, _) => {}
                GitStoreEvent::JobsUpdated | GitStoreEvent::ConflictsUpdated | GitStoreEvent::ActiveWorktreeChanged => {
                }
            })
            .detach();

            let mut this = Self {
                active_repository,
                commit_editor,
                commit_editor_expanded: false,
                commit_editor_collapsed,
                conflicted_count: 0,
                conflicted_staged_count: 0,

                entries: Vec::new(),
                collapsed_sections: FxHashSet::default(),
                view_mode: GitPanelViewMode::from_settings(cx),
                tree_expanded_dirs: FxHashMap::default(),
                projected_entries_by_path: FxHashMap::default(),
                focus_handle,
                new_count: 0,
                new_staged_count: 0,
                changes_count: 0,
                diff_stat_total: DiffStat::default(),
                pending_commit: None,
                pending_remote_operation: None,
                amend_pending,
                original_commit_message,
                signoff_enabled,
                skip_hooks_enabled: false,
                single_staged_entry: None,
                single_tracked_entry: None,
                project,
                scroll_handle,
                max_width_item_index: None,
                selected_entry: None,
                marked_entries: FxHashSet::default(),
                marked_directories: FxHashSet::default(),
                mark_range_gesture: None,
                tracked_count: 0,
                tracked_staged_count: 0,
                section_file_counts: FxHashMap::default(),
                update_visible_entries_task: Task::ready(()),
                reopen_commit_buffer_task: Task::ready(()),
                show_placeholders: false,

                commit_template: None,
                context_menu: None,
                workspace: workspace.weak_handle(),
                entry_count: 0,
                bulk_staging: None,
                stash_entries: GitStash::default(),
                _commit_message_buffer_subscription: None,
                git_access: None,
                commit_menu_handle: PopoverMenuHandle::default(),
                changes_actions_menu_handle: PopoverMenuHandle::default(),
                remote_action_menu_handle: PopoverMenuHandle::default(),
            };

            this.schedule_update(window, cx);
            this
        })
    }

    fn dispatch_context(&self, window: &Window, cx: &Context<Self>) -> KeyContext {
        let mut dispatch_context = KeyContext::new_with_defaults();
        dispatch_context.add("GitPanel");

        if self.commit_editor.read(cx).is_focused(window) {
            dispatch_context.add("CommitEditor");
        } else if self.focus_handle.contains_focused(window, cx) || self.context_menu.is_some() {
            // Preserve the panel's list context while a context menu is open.
            // Its focus handle may not appear as a descendant of the panel
            // until the next frame, so `FocusHandle::contains_focused` would
            // return `false`.
            dispatch_context.add("menu");
            dispatch_context.add("ChangesList");
        }

        dispatch_context
    }

    fn close_panel(&mut self, _: &Close, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PanelEvent::Close);
    }

    fn focus_editor(&mut self, _: &FocusEditor, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor.update(cx, |editor, cx| {
            window.focus(&editor.focus_handle(cx), cx);
        });
        cx.notify();
    }

    fn focus_changes_list(&mut self, _: &FocusChanges, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        self.select_first_entry_if_none(window, cx);
    }

    fn open_diff(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_target_kind() == Some(SelectionTargetKind::Directory)
            && let Some(GitListEntry::Directory(dir_entry)) =
                self.selected_entry.and_then(|i| self.entries.get(i)).cloned()
        {
            self.toggle_directory(&dir_entry.key, window, cx);
            return;
        }
        (|| {
            let selected_index = self.selected_entry?;
            let entry = self.entries.get(selected_index)?.status_entry()?;
            let workspace = self.workspace.upgrade()?;
            let git_repo = self.active_repository.as_ref()?;
            let target = Self::diff_target_for_section(self.section_for_entry_index(selected_index));

            if target == DiffTarget::Uncommitted
                && let Some(project_diff) = workspace.read(cx).active_item_as::<ProjectDiff>(cx)
                && let Some(project_path) = project_diff.read(cx).active_project_path(cx)
                && Some(&entry.repo_path) == git_repo.read(cx).project_path_to_repo_path(&project_path, cx).as_ref()
            {
                project_diff.focus_handle(cx).focus(window, cx);
                project_diff.update(cx, |project_diff, cx| project_diff.autoscroll(cx));
                return None;
            }

            self.workspace
                .update(cx, |workspace, cx| match target {
                    DiffTarget::Uncommitted => {
                        ProjectDiff::deploy_at(workspace, Some(entry.clone()), window, cx);
                    }
                    DiffTarget::Staged => {
                        StagedDiff::deploy_at(workspace, Some(entry.clone()), window, cx);
                    }
                    DiffTarget::Unstaged => {
                        UnstagedDiff::deploy_at(workspace, Some(entry.clone()), window, cx);
                    }
                })
                .ok();
            self.focus_handle.focus(window, cx);

            Some(())
        })();
    }

    fn open_solo_diff(&mut self, _: &menu::SecondaryConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repository) = self.active_repository.clone() else {
            return;
        };
        for entry in self.effective_status_entries() {
            SoloDiffView::open_or_focus(entry, repository.clone(), self.workspace.clone(), window, cx)
                .detach_and_notify_err(self.workspace.clone(), window, cx);
        }
    }

    fn view_file(&mut self, _: &ViewFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repository) = self.active_repository.as_ref() else {
            return;
        };
        let project_paths = self
            .effective_status_entries()
            .iter()
            .filter_map(|entry| repository.read(cx).repo_path_to_project_path(&entry.repo_path, cx))
            .collect::<Vec<_>>();

        for project_path in project_paths {
            self.workspace
                .update(cx, |workspace, cx| {
                    workspace
                        .open_path_preview(project_path, None, false, false, true, window, cx)
                        .detach_and_notify_err(self.workspace.clone(), window, cx);
                })
                .log_err();
        }
    }

    fn view_file_history(&mut self, _: &git::FileHistory, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.effective_status_entries();
        if entries.len() <= 1 {
            cx.propagate();
            return;
        }
        let Some(repository) = self.active_repository.as_ref() else {
            return;
        };
        let repository_id = repository.read(cx).id;
        let git_store = self.project.read(cx).git_store().clone();
        self.workspace
            .update(cx, |workspace, cx| {
                for entry in entries {
                    if !entry.status.is_created() {
                        git_graph::open_or_reuse_graph(
                            workspace,
                            repository_id,
                            git_store.clone(),
                            LogSource::Path(entry.repo_path),
                            None,
                            window,
                            cx,
                        );
                    }
                }
            })
            .log_err();
        cx.stop_propagation();
    }

    fn copy_path(&mut self, _: &CopyPath, _: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.active_repository.as_ref() else {
            cx.propagate();
            return;
        };
        let paths = self.effective_repo_paths();
        if paths.is_empty() {
            cx.propagate();
            return;
        }
        let repo = repo.read(cx);
        cx.write_to_clipboard(ClipboardItem::new_string(
            paths.into_iter().map(|path| repo.repo_path_to_abs_path(&path).to_string_lossy().into_owned()).join("\n"),
        ));
    }

    fn copy_relative_path(&mut self, _: &CopyRelativePath, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.effective_repo_paths();
        if paths.is_empty() {
            cx.propagate();
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            paths.into_iter().map(|path| path.display().to_owned()).join("\n"),
        ));
    }

    fn open_selected_entry_on_click(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Self>) {
        if secondary {
            self.open_solo_diff(&menu::SecondaryConfirm, window, cx);
        } else {
            self.open_diff(&menu::Confirm, window, cx);
            self.focus_handle.focus(window, cx);
        }
    }

    fn set_sort_by_path(&mut self, _: &SetSortByPath, _: &mut Window, cx: &mut Context<Self>) {
        GitPanelViewOptions::update(cx, |options| options.sort_by = GitPanelSortBy::Path);
    }

    fn set_sort_by_name(&mut self, _: &SetSortByName, _: &mut Window, cx: &mut Context<Self>) {
        GitPanelViewOptions::update(cx, |options| options.sort_by = GitPanelSortBy::Name);
    }

    fn set_group_by_none(&mut self, _: &SetGroupByNone, _: &mut Window, cx: &mut Context<Self>) {
        GitPanelViewOptions::update(cx, |options| options.group_by = GitPanelGroupBy::None);
    }

    fn set_group_by_status(&mut self, _: &SetGroupByStatus, _: &mut Window, cx: &mut Context<Self>) {
        GitPanelViewOptions::update(cx, |options| options.group_by = GitPanelGroupBy::Status);
    }

    fn view_staged_changes(&mut self, _: &ViewStagedChanges, window: &mut Window, cx: &mut Context<Self>) {
        let entry = if self.effective_status_entries().len() > 1 {
            None
        } else {
            self.get_selected_entry().and_then(GitListEntry::status_entry).cloned()
        };
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                StagedDiff::deploy_at(workspace, entry, window, cx);
            });
        }
    }

    fn view_unstaged_changes(&mut self, _: &ViewUnstagedChanges, window: &mut Window, cx: &mut Context<Self>) {
        let entry = if self.effective_status_entries().len() > 1 {
            None
        } else {
            self.get_selected_entry().and_then(GitListEntry::status_entry).cloned()
        };
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                UnstagedDiff::deploy_at(workspace, entry, window, cx);
            });
        }
    }

    fn set_group_by_staging(&mut self, _: &SetGroupByStaging, _: &mut Window, cx: &mut Context<Self>) {
        GitPanelViewOptions::update(cx, |options| options.group_by = GitPanelGroupBy::Staging);
    }

    fn toggle_tree_view(&mut self, _: &ToggleTreeView, _: &mut Window, cx: &mut Context<Self>) {
        GitPanelViewOptions::update(cx, |options| options.tree_view = !options.tree_view);
    }

    pub(crate) fn increase_font_size(&mut self, _: &IncreaseBufferFontSize, _: &mut Window, cx: &mut Context<Self>) {
        Self::adjust_font_size(px(1.0), cx);
    }

    pub(crate) fn decrease_font_size(&mut self, _: &DecreaseBufferFontSize, _: &mut Window, cx: &mut Context<Self>) {
        Self::adjust_font_size(px(-1.0), cx);
    }

    fn adjust_font_size(delta: Pixels, cx: &mut Context<Self>) {
        let size = font::clamp_font_size(git_commit_font_size(cx) + delta);
        cx.set_global(GitCommitFontSize(size));
        cx.refresh_windows();
    }

    pub(crate) fn reset_font_size(&mut self, _: &ResetBufferFontSize, _: &mut Window, cx: &mut Context<Self>) {
        if cx.has_global::<GitCommitFontSize>() {
            cx.remove_global::<GitCommitFontSize>();
            cx.refresh_windows();
        }
    }

    fn toggle_directory(&mut self, key: &TreeKey, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = self.view_mode.tree_state_mut() {
            let expanded = state.expanded_dirs.entry(key.clone()).or_insert(true);
            *expanded = !*expanded;
            self.tree_expanded_dirs = state.expanded_dirs.clone();
            self.update_visible_entries(window, cx);
        } else {
            log::error!("Attempted to toggle directory in flat Git Panel state");
        }
    }

    fn schedule_update(&mut self, window: &Window, cx: &mut Context<Self>) {
        let handle = cx.entity().downgrade();
        let new_active_repository = self.project.read(cx).active_repository(cx);
        let active_repository_changed = self.active_repository.as_ref().map(Entity::entity_id)
            != new_active_repository.as_ref().map(Entity::entity_id);
        if active_repository_changed {
            self.clear_marks();
            self.set_skip_hooks_enabled(false, cx);
            if self.amend_pending {
                // Leaving a repository with a pending amend: undo it so the amend
                // state doesn't carry over to the newly active repository. The
                // commit editor still holds the previous repository's buffer here
                // (`reopen_commit_buffer` swaps it asynchronously below), so this
                // restores the pre-amend draft into that repository's buffer.
                self.set_amend_pending(false, cx);
            }
            self.git_access = None;
        }
        self.active_repository = new_active_repository;
        self.reopen_commit_buffer(window, cx);
        self.update_visible_entries_task = cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(UPDATE_DEBOUNCE).await;
            if let Some(git_panel) = handle.upgrade() {
                git_panel
                    .update_in(cx, |git_panel, window, cx| {
                        git_panel.update_visible_entries(window, cx);
                    })
                    .ok();
            }
        });
    }

    fn update_visible_entries(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected_change = self.selected_entry.and_then(|index| {
            let entry = self.entries.get(index)?.status_entry()?;
            Some((entry.repo_path.clone(), self.section_for_entry_index(index)))
        });
        let bulk_staging = self.bulk_staging.take();
        let last_staged_path_prev_index = bulk_staging.as_ref().and_then(|op| self.entry_by_path(&op.anchor));

        let active_repository = self.project.read(cx).active_repository(cx);
        if active_repository != self.active_repository {
            self.active_repository = active_repository;
            self.git_access = None;
            self.clear_marks();
        }
        self.entries.clear();
        self.projected_entries_by_path.clear();
        self.single_staged_entry.take();
        self.single_tracked_entry.take();
        self.conflicted_count = 0;
        self.conflicted_staged_count = 0;
        self.changes_count = 0;
        self.diff_stat_total = DiffStat::default();
        self.new_count = 0;
        self.tracked_count = 0;
        self.new_staged_count = 0;
        self.tracked_staged_count = 0;
        self.entry_count = 0;
        self.section_file_counts.clear();
        self.max_width_item_index = None;

        let GitPanelViewOptions { sort_by, group_by, .. } = GitPanelViewOptions::get(cx);
        let group_by_file_status = group_by == GitPanelGroupBy::Status;
        let group_by_staging_state = group_by == GitPanelGroupBy::Staging;
        let is_tree_view = matches!(self.view_mode, GitPanelViewMode::Tree(_));

        if let Some(active_repo) = self.active_repository.as_ref()
            && self.git_access.is_none()
        {
            let access = active_repo.update(cx, |active_repo, cx| active_repo.access(cx));

            cx.spawn_in(window, async move |git_panel, cx| {
                // When the user does not own the `.git` folder, the
                // `GitStore.spawn_local_git_worker` will fail to create the
                // receiver for Git jobs, so this access check will be
                // cancelled.
                //
                // We assume `GitAccess::No` on cancellation. I believe this is
                // imprecise, other failures could also cause cancellation, but
                // the consequence is just showing the "unsafe repo" UI, which
                // seems acceptable for this edge case.
                let access = match access.await {
                    Ok(access) => access,
                    Err(Canceled) => GitAccess::No,
                };

                git_panel.update(cx, |this, _cx| {
                    this.git_access = Some(access);
                })
            })
            .detach_and_log_err(cx);
        }

        let mut changed_entries = Vec::new();
        let mut new_entries = Vec::new();
        let mut conflict_entries = Vec::new();
        let mut staged_entries = Vec::new();
        let mut unstaged_entries = Vec::new();
        let mut tracked_entries = Vec::new();
        let mut single_staged_entry = None;
        let mut staged_count = 0;
        let mut seen_directories = FxHashSet::default();
        let mut max_width_estimate = 0_usize;
        let mut max_width_item_index = None;

        let Some(repo) = self.active_repository.as_ref() else {
            // Just clear entries if no repository is active.
            cx.notify();
            return;
        };

        let repo = repo.read(cx);

        self.stash_entries = repo.cached_stash();

        for status_entry in repo.cached_status() {
            self.changes_count += 1;
            let is_conflict = repo.had_conflict_on_last_merge_head_change(&status_entry.repo_path);
            let is_new = status_entry.status.is_created();
            let staging = status_entry.status.staging();

            if let Some(pending) = repo.pending_ops_for_path(&status_entry.repo_path)
                && pending.ops.iter().any(|op| op.git_status == pending_op::GitStatus::Reverted && op.finished())
            {
                continue;
            }

            let entry = GitStatusEntry {
                repo_path: status_entry.repo_path.clone(),
                status: status_entry.status,
                staging,
                diff_stat: status_entry.diff_stat,
            };

            if !is_conflict && !is_new {
                tracked_entries.push(entry.clone());
            }

            if staging.has_staged() {
                staged_count += 1;
                single_staged_entry = Some(entry.clone());
            }

            if group_by_staging_state && is_conflict {
                conflict_entries.push(entry);
            } else if group_by_staging_state {
                if staging.has_staged() {
                    staged_entries.push(GitStatusEntry { diff_stat: status_entry.staged_diff_stat, ..entry.clone() });
                }
                if staging.has_unstaged() {
                    unstaged_entries.push(GitStatusEntry { diff_stat: status_entry.unstaged_diff_stat, ..entry });
                }
            } else if group_by_file_status && is_conflict {
                conflict_entries.push(entry);
            } else if group_by_file_status && is_new {
                new_entries.push(entry);
            } else {
                changed_entries.push(entry);
            }
        }

        if conflict_entries.is_empty() {
            if staged_count == 1
                && let Some(entry) = single_staged_entry.as_ref()
            {
                if let Some(ops) = repo.pending_ops_for_path(&entry.repo_path) {
                    if ops.staged() {
                        self.single_staged_entry = single_staged_entry;
                    }
                } else {
                    self.single_staged_entry = single_staged_entry;
                }
            } else if repo.pending_ops_summary().item_summary.staging_count == 1
                && let Some(ops) = repo.pending_ops().find(|ops| ops.staging())
            {
                self.single_staged_entry = repo.status_for_path(&ops.repo_path).map(|status| GitStatusEntry {
                    repo_path: ops.repo_path.clone(),
                    status: status.status,
                    staging: StageStatus::Staged,
                    diff_stat: status.diff_stat,
                });
            }
        }

        if tracked_entries.len() == 1 {
            self.single_tracked_entry = tracked_entries.pop();
        }

        if !is_tree_view {
            let sort_entries = |entries: &mut Vec<GitStatusEntry>| match sort_by {
                GitPanelSortBy::Path => entries.sort_by(|a, b| a.repo_path.cmp(&b.repo_path)),
                GitPanelSortBy::Name => entries.sort_by(|a, b| {
                    a.repo_path.file_name().cmp(&b.repo_path.file_name()).then_with(|| a.repo_path.cmp(&b.repo_path))
                }),
            };

            sort_entries(&mut conflict_entries);
            sort_entries(&mut changed_entries);
            sort_entries(&mut new_entries);
            sort_entries(&mut staged_entries);
            sort_entries(&mut unstaged_entries);
        }

        let mut push_entry = |this: &mut Self,
                              entry: GitListEntry,
                              section: Section,
                              is_visible: bool,
                              logical_indices: Option<&mut Vec<usize>>| {
            if let Some(estimate) = this.width_estimate_for_list_entry(is_tree_view, &entry)
                && estimate > max_width_estimate
            {
                max_width_estimate = estimate;
                max_width_item_index = Some(this.entries.len());
            }

            if let Some(repo_path) = entry.status_entry().map(|status| status.repo_path.clone()) {
                this.projected_entries_by_path
                    .entry(repo_path)
                    .or_default()
                    .push(ProjectedChangeEntry { section, index: this.entries.len() });
            }

            if let (Some(indices), true) = (logical_indices, is_visible) {
                indices.push(this.entries.len());
            }

            this.entries.push(entry);
        };

        let section_entries = if group_by_staging_state {
            vec![
                (Section::Conflict, mem::take(&mut conflict_entries)),
                (Section::Staged, mem::take(&mut staged_entries)),
                (Section::Unstaged, mem::take(&mut unstaged_entries)),
            ]
        } else {
            vec![
                (Section::Conflict, mem::take(&mut conflict_entries)),
                (Section::Tracked, mem::take(&mut changed_entries)),
                (Section::New, mem::take(&mut new_entries)),
            ]
        };

        // Keep Staged/Unstaged headers pinned even when empty (as long as there's
        // anything to show at all) so the layout stays stable while staging.
        let has_any_section_entries = section_entries.iter().any(|(_, entries)| !entries.is_empty());
        self.section_file_counts.extend(section_entries.iter().map(|(section, entries)| (*section, entries.len())));
        let show_when_empty = |section: Section| {
            group_by_staging_state && has_any_section_entries && matches!(section, Section::Staged | Section::Unstaged)
        };

        match &mut self.view_mode {
            GitPanelViewMode::Tree(tree_state) => {
                tree_state.logical_indices.clear();
                tree_state.directory_descendants.clear();

                // This is just to get around the borrow checker
                // because push_entry mutably borrows self
                let mut tree_state = mem::take(tree_state);

                for (section, entries) in section_entries {
                    if entries.is_empty() && !show_when_empty(section) {
                        continue;
                    }

                    let section_has_header = section != Section::Tracked || group_by != GitPanelGroupBy::None;
                    let section_is_collapsed = section_has_header && self.collapsed_sections.contains(&section);

                    if section_has_header {
                        push_entry(
                            self,
                            GitListEntry::Header(GitHeaderEntry { header: section }),
                            section,
                            true,
                            Some(&mut tree_state.logical_indices),
                        );
                    }

                    if entries.is_empty() {
                        push_entry(
                            self,
                            GitListEntry::EmptySection(section),
                            section,
                            !section_is_collapsed,
                            Some(&mut tree_state.logical_indices),
                        );
                    }

                    for (entry, is_visible) in tree_state.build_tree_entries(section, entries, &mut seen_directories) {
                        push_entry(
                            self,
                            entry,
                            section,
                            is_visible && !section_is_collapsed,
                            Some(&mut tree_state.logical_indices),
                        );
                    }
                }

                tree_state.expanded_dirs.retain(|key, _| seen_directories.contains(key));
                self.tree_expanded_dirs.clone_from(&tree_state.expanded_dirs);
                self.view_mode = GitPanelViewMode::Tree(tree_state);
            }
            GitPanelViewMode::Flat => {
                for (section, entries) in section_entries {
                    if entries.is_empty() && !show_when_empty(section) {
                        continue;
                    }

                    if section != Section::Tracked || group_by != GitPanelGroupBy::None {
                        push_entry(self, GitListEntry::Header(GitHeaderEntry { header: section }), section, true, None);
                    }

                    if entries.is_empty() {
                        push_entry(self, GitListEntry::EmptySection(section), section, true, None);
                    }

                    for entry in entries {
                        push_entry(self, GitListEntry::Status(entry), section, true, None);
                    }
                }
            }
        }

        self.max_width_item_index = max_width_item_index;

        self.update_counts(repo);

        let bulk_staging_anchor_new_index =
            bulk_staging.as_ref().filter(|op| op.repo_id == repo.id).and_then(|op| self.entry_by_path(&op.anchor));
        if bulk_staging_anchor_new_index == last_staged_path_prev_index
            && let Some(index) = bulk_staging_anchor_new_index
            && let Some(entry) = self.entries.get(index)
            && let Some(entry) = entry.status_entry()
            && GitPanel::stage_status_for_entry(entry, repo).as_bool().unwrap_or(false)
        {
            self.bulk_staging = bulk_staging;
        }

        match &self.view_mode {
            GitPanelViewMode::Tree(state) => {
                let directories = &state.directory_descendants;
                self.marked_directories.retain(|key| directories.contains_key(key));
            }
            GitPanelViewMode::Flat => self.marked_directories.clear(),
        }
        if !self.marked_entries.is_empty() {
            let present_paths: FxHashSet<RepoPath> = self
                .entries
                .iter()
                .filter_map(|entry| entry.status_entry())
                .map(|entry| entry.repo_path.clone())
                .collect();
            self.marked_entries.retain(|path| present_paths.contains(path));
        }
        // The rebuild may have reordered rows, invalidating the anchor index.
        self.mark_range_gesture = None;

        if let Some((path, section)) = selected_change {
            self.selected_entry = section
                .and_then(|section| self.entry_by_path_in_section(&path, section))
                .or_else(|| self.entry_by_path(&path));
        }
        self.select_first_entry_if_none(window, cx);
        self.select_last_entry_if_out_of_bounds(window, cx);

        let suggested_commit_message = self.suggest_commit_message(cx);
        let placeholder_text = suggested_commit_message.unwrap_or_else(|| "Enter commit message".into());

        self.commit_editor.update(cx, |editor, cx| editor.set_placeholder_text(&placeholder_text, window, cx));

        cx.notify();
    }

    const fn header_state(&self, header_type: Section) -> ToggleState {
        let (staged_count, count) = match header_type {
            Section::New => (self.new_staged_count, self.new_count),
            Section::Tracked => (self.tracked_staged_count, self.tracked_count),
            Section::Conflict => (self.conflicted_staged_count, self.conflicted_count),
            Section::Staged => (self.entry_count, self.entry_count),
            Section::Unstaged => (0, self.entry_count),
        };
        if staged_count == 0 {
            ToggleState::Unselected
        } else if count == staged_count {
            ToggleState::Selected
        } else {
            ToggleState::Indeterminate
        }
    }

    fn section_for_entry_index(&self, ix: usize) -> Option<Section> {
        self.entries
            .get(..=ix)?
            .iter()
            .rev()
            .find_map(|entry| if let GitListEntry::Header(header) = entry { Some(header.header) } else { None })
    }

    fn visible_flat_entry_indices(&self) -> Vec<usize> {
        let mut section = None;
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                if let GitListEntry::Header(header) = entry {
                    section = Some(header.header);
                    return Some(index);
                }

                if section.is_some_and(|section| self.collapsed_sections.contains(&section)) {
                    None
                } else {
                    Some(index)
                }
            })
            .collect()
    }

    fn toggle_section_collapsed(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        if !self.collapsed_sections.remove(&section) {
            self.collapsed_sections.insert(section);
        }
        self.update_visible_entries(window, cx);
    }

    fn stage_intent_for_entry_index(&self, ix: usize) -> StageIntent {
        self.section_for_entry_index(ix).map_or(StageIntent::Toggle, StageIntent::for_section)
    }

    // A conflict that has been marked resolved (fully staged) is locked
    // against toggling: unstaging would rebuild the index entry from HEAD,
    // silently discarding the unmerged (base/ours/theirs) stages — a
    // round-trip git can't actually perform. The explicit git::UnstageFile
    // action remains as an escape hatch.
    fn is_resolved_conflict(&self, ix: usize, cx: &App) -> bool {
        if self.section_for_entry_index(ix) != Some(Section::Conflict) {
            return false;
        }
        let Some(entry) = self.entries.get(ix) else {
            return false;
        };
        let Some(repo) = self.active_repository.as_ref() else {
            return false;
        };
        let repo = repo.read(cx);
        match entry {
            GitListEntry::Directory(directory) => {
                self.stage_status_for_directory(directory, repo) == StageStatus::Staged
            }
            entry => entry.status_entry().is_some_and(|status_entry| {
                GitPanel::stage_status_for_entry(status_entry, repo) == StageStatus::Staged
            }),
        }
    }

    const fn diff_target_for_section(section: Option<Section>) -> DiffTarget {
        match section {
            Some(Section::Staged) => DiffTarget::Staged,
            Some(Section::Unstaged) => DiffTarget::Unstaged,
            _ => DiffTarget::Uncommitted,
        }
    }

    fn update_counts(&mut self, repo: &Repository) {
        self.show_placeholders = false;
        self.conflicted_count = 0;
        self.conflicted_staged_count = 0;
        self.new_count = 0;
        self.tracked_count = 0;
        self.new_staged_count = 0;
        self.tracked_staged_count = 0;
        self.entry_count = 0;
        self.diff_stat_total = DiffStat::default();

        let change_entries = self.change_entries_by_path().cloned().collect::<Vec<_>>();
        for status_entry in change_entries {
            self.entry_count += 1;
            if let Some(diff_stat) = status_entry.diff_stat {
                self.diff_stat_total.added = self.diff_stat_total.added.saturating_add(diff_stat.added);
                self.diff_stat_total.deleted = self.diff_stat_total.deleted.saturating_add(diff_stat.deleted);
            }

            let stage_status = GitPanel::stage_status_for_entry(&status_entry, repo);

            if repo.had_conflict_on_last_merge_head_change(&status_entry.repo_path) {
                self.conflicted_count += 1;
                if stage_status.has_staged() {
                    self.conflicted_staged_count += 1;
                }
            } else if status_entry.status.is_created() {
                self.new_count += 1;
                if stage_status.has_staged() {
                    self.new_staged_count += 1;
                }
            } else {
                self.tracked_count += 1;
                if stage_status.has_staged() {
                    self.tracked_staged_count += 1;
                }
            }
        }
    }

    pub(crate) const fn has_staged_changes(&self) -> bool {
        self.tracked_staged_count > 0 || self.new_staged_count > 0 || self.conflicted_staged_count > 0
    }

    pub(crate) fn has_unstaged_changes(&self) -> bool {
        self.change_entries_by_path().any(|entry| entry.staging.has_unstaged())
    }

    const fn has_tracked_changes(&self) -> bool {
        self.tracked_count > 0
    }

    fn has_staged_tracked_changes(&self) -> bool {
        Self::contains_staged_tracked_entry(self.change_entries_by_path())
    }

    pub fn has_unstaged_conflicts(&self) -> bool {
        self.change_entries_by_path().any(|entry| entry.status.is_conflicted() && entry.staging.has_unstaged())
    }

    fn show_error_toast(&self, action: impl Into<SharedString>, e: anyhow::Error, cx: &mut App) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        show_error_toast(workspace, action, e, cx);
    }

    pub fn load(workspace: &WeakEntity<Workspace>, cx: &mut AsyncWindowContext) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(cx, |workspace, window, cx| GitPanel::new(workspace, window, cx))
    }
}

impl Focusable for GitPanel {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for GitPanel {}

pub(crate) struct GitPanelAddon {
    pub(crate) workspace: WeakEntity<Workspace>,
}

impl editor::Addon for GitPanelAddon {
    fn to_any(&self) -> &dyn Any {
        self
    }

    fn render_buffer_header_controls(
        &self,
        _excerpt_info: &ExcerptBoundaryInfo,
        buffer: &language::BufferSnapshot,
        window: &Window,
        cx: &App,
    ) -> Option<AnyElement> {
        let file = buffer.file()?;
        let git_panel = self.workspace.upgrade()?.read(cx).panel::<GitPanel>(cx)?;

        git_panel.read(cx).render_buffer_header_controls(&git_panel, file, window, cx)
    }
}

impl Panel for GitPanel {
    fn activation_focus_handle(&self, cx: &App) -> FocusHandle {
        if !self.commit_editor_collapsed && (self.entries.is_empty() || self.commit_editor_expanded) {
            self.commit_editor.focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }

    fn persistent_name() -> &'static str {
        "GitPanel"
    }

    fn panel_key() -> &'static str {
        GIT_PANEL_KEY
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Left
    }

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        DEFAULT_WIDTH
    }

    fn min_size(&self, _: &Window, _: &App) -> Option<Pixels> {
        Some(SIDEBAR_MIN_WIDTH)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::GitBranch)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Git Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        3
    }
}

pub fn panel_editor_container(_window: &mut Window, cx: &mut App) -> Div {
    v_flex().size_full().bg(cx.theme().colors().editor_background)
}

pub(crate) fn git_commit_editor_style(font_size: gpui::Pixels, cx: &App) -> EditorStyle {
    EditorStyle {
        background: cx.theme().colors().editor_background,
        cursor_colors: cx.theme().cursor_colors(),
        text: TextStyle {
            color: cx.theme().colors().text,
            font_family: font::buffer_font().family.clone(),
            font_fallbacks: font::buffer_font().fallbacks.clone(),
            font_features: font::buffer_font().features.clone(),
            font_size: AbsoluteLength::from(font_size),
            font_weight: font::buffer_font().weight,
            line_height: (font_size * BUFFER_LINE_HEIGHT).into(),
            ..Default::default()
        },
        syntax: cx.theme().syntax(),
        ..Default::default()
    }
}
