use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use collections::BTreeMap;
use collections::HashMap;
use file_icons::FileIcons;
use git::Oid;
use git::repository::CommitFileStatus;
use git::repository::RepoPath;
use git::status::FileStatus;
use git::status::StatusCode;
use git::status::TrackedStatus;
use gpui::Anchor;
use gpui::AnyElement;
use gpui::App;
use gpui::ClipboardItem;
use gpui::DismissEvent;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::Focusable;
use gpui::Hsla;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::Pixels;
use gpui::Point;
use gpui::ScrollHandle;
use gpui::ScrollStrategy;
use gpui::SharedString;
use gpui::Subscription;
use gpui::Task;
use gpui::UniformListScrollHandle;
use gpui::WeakEntity;
use gpui::Window;
use gpui::anchored;
use gpui::deferred;
use gpui::prelude::*;
use gpui::px;
use gpui::uniform_list;
use language::LanguageRegistry;
use language::line_diff;
use markdown::Markdown;
use markdown::MarkdownElement;
use project::git_store::CommitDataState;
use project::git_store::CommitDiff;
use project::git_store::CommitFile;
use project::git_store::Repository;
use ui::Chip;
use ui::ContextMenu;
use ui::DiffStat;
use ui::Disclosure;
use ui::Divider;
use ui::IndentGuideColors;
use ui::ListItem;
use ui::ListItemSpacing;
use ui::Tooltip;
use ui::WithScrollbar;
use ui::prelude::*;
use util::ResultExt;

use crate::git_graph::ToggleChangedFilesView;
use crate::git_graph::format_timestamp;
use crate::git_status_icon;

const COPIED_STATE_DURATION: Duration = Duration::from_secs(2);
const TREE_INDENT: f32 = 20.0;

pub enum CommitDetailsEvent {
    /// A changed file was clicked.
    OpenFile(RepoPath),
    /// "Open Revision" on a changed file's context menu.
    OpenRevision(RepoPath),
    /// "Open Project File" on a changed file's context menu.
    OpenProjectFile(RepoPath),
    /// A ref chip was right-clicked.
    DeployRefMenu { position: Point<Pixels>, name: SharedString },
}

/// The commit card: author, refs, SHA, message and changed files of one commit.
/// Shown beside the git graph for the selected row and as the side card of a
/// commit view.
pub struct CommitDetailsPanel {
    repository: WeakEntity<Repository>,
    languages: Option<Arc<LanguageRegistry>>,
    sha: Option<Oid>,
    ref_names: Vec<SharedString>,
    ref_color: Hsla,
    files: Option<Vec<ChangedFileEntry>>,
    diff_stats: (usize, usize),
    message: Option<Entity<Markdown>>,
    message_scroll_handle: ScrollHandle,
    search_highlight: Option<(String, bool)>,
    files_scroll_handle: UniformListScrollHandle,
    view_mode: ChangedFilesViewMode,
    expanded_dirs: HashMap<RepoPath, bool>,
    /// The file whose revision is open in the active tab.
    revealed_path: Option<RepoPath>,
    scroll_to_revealed: bool,
    metadata_collapsed: bool,
    message_collapsed: bool,
    context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    _message_task: Option<Task<()>>,
    _diff_task: Option<Task<()>>,
}

impl EventEmitter<CommitDetailsEvent> for CommitDetailsPanel {}

impl CommitDetailsPanel {
    pub fn new(languages: Option<Arc<LanguageRegistry>>) -> Self {
        Self {
            repository: WeakEntity::new_invalid(),
            languages,
            sha: None,
            ref_names: Vec::new(),
            ref_color: Hsla::default(),
            files: None,
            diff_stats: (0, 0),
            message: None,
            message_scroll_handle: ScrollHandle::new(),
            search_highlight: None,
            files_scroll_handle: UniformListScrollHandle::new(),
            view_mode: ChangedFilesViewMode::default(),
            expanded_dirs: HashMap::default(),
            revealed_path: None,
            scroll_to_revealed: false,
            metadata_collapsed: false,
            message_collapsed: false,
            context_menu: None,
            _message_task: None,
            _diff_task: None,
        }
    }

    pub const fn sha(&self) -> Option<Oid> {
        self.sha
    }

    /// Shows `sha`, loading its message. Changed files come from `set_diff` or
    /// `load_diff`. View mode and collapsed sections are kept.
    pub fn set_commit(
        &mut self,
        repository: &Entity<Repository>,
        sha: Oid,
        ref_names: Vec<SharedString>,
        ref_color: Hsla,
        cx: &mut Context<Self>,
    ) {
        self.ref_names = ref_names;
        self.ref_color = ref_color;
        if self.sha == Some(sha) && self.repository == repository.downgrade() {
            cx.notify();
            return;
        }
        self.repository = repository.downgrade();
        self.sha = Some(sha);
        self.files = None;
        self.diff_stats = (0, 0);
        self.message = None;
        self.expanded_dirs.clear();
        self.revealed_path = None;
        self.files_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        self._diff_task = None;
        self._message_task = None;

        match repository.update(cx, |repo, cx| repo.fetch_commit_data(sha, true, cx).clone()) {
            CommitDataState::Loaded(data) => self.set_message(data.message.clone(), cx),
            CommitDataState::Loading(Some(receiver)) => {
                self._message_task = Some(cx.spawn(async move |this, cx| {
                    if let Ok(data) = receiver.await {
                        this.update(cx, |this, cx| {
                            if this.sha == Some(data.sha) {
                                this.set_message(data.message.clone(), cx);
                            }
                        })
                        .log_err();
                    }
                }));
            }
            CommitDataState::Loading(None) => {}
        }
        cx.notify();
    }

    /// Loads the changed files of the current commit from the repository.
    pub fn load_diff(&mut self, cx: &mut Context<Self>) {
        let (Some(sha), Some(repository)) = (self.sha, self.repository.upgrade()) else {
            return;
        };
        let diff_task = repository.update(cx, |repo, cx| repo.load_commit_diff(sha.to_string(), false, cx));
        self._diff_task = Some(cx.spawn(async move |this, cx| {
            if let Ok(diff) = diff_task.await {
                this.update(cx, |this, cx| {
                    if this.sha == Some(sha) {
                        this.set_diff(&diff, cx);
                    }
                })
                .ok();
            }
        }));
    }

    pub fn set_diff(&mut self, diff: &CommitDiff, cx: &mut Context<Self>) {
        self.files = Some(diff.files.iter().map(ChangedFileEntry::from_commit_file).collect());
        self.diff_stats = compute_diff_stats(diff);
        cx.notify();
    }

    /// Highlights `repo_path` in the changed files, expanding its folders and
    /// scrolling it into view.
    pub fn reveal_file(&mut self, repo_path: &RepoPath, cx: &mut Context<Self>) {
        for dir in ancestor_dirs(repo_path) {
            self.expanded_dirs.insert(dir, true);
        }
        self.revealed_path = Some(repo_path.clone());
        self.scroll_to_revealed = true;
        cx.notify();
    }

    /// Highlights `query` matches in the commit message.
    pub fn set_search_highlight(&mut self, query: Option<&str>, case_sensitive: bool, cx: &mut Context<Self>) {
        self.search_highlight = query.map(|query| (query.to_string(), case_sensitive));
        self.apply_search_highlight(cx);
    }

    pub fn toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        self.view_mode = self.view_mode.toggled();
        self.files_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn deploy_file_context_menu(
        &mut self,
        position: Point<Pixels>,
        repo_path: RepoPath,
        window: &mut Window,
        cx: &mut Context<CommitDetailsPanel>,
    ) {
        let panel = cx.weak_entity();
        let context_menu = ContextMenu::build(window, cx, move |menu, _, _| {
            menu.entry("Open Revision", None, {
                let panel = panel.clone();
                let repo_path = repo_path.clone();
                move |_, cx| {
                    panel.update(cx, |_, cx| cx.emit(CommitDetailsEvent::OpenRevision(repo_path.clone()))).ok();
                }
            })
            .entry("Open Project File", None, move |_, cx| {
                panel.update(cx, |_, cx| cx.emit(CommitDetailsEvent::OpenProjectFile(repo_path.clone()))).ok();
            })
        });
        window.focus(&context_menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&context_menu, |this: &mut CommitDetailsPanel, _, _: &DismissEvent, cx| {
            this.context_menu.take();
            cx.notify();
        });
        self.context_menu = Some((context_menu, position, subscription));
        cx.notify();
    }

    fn set_message(&mut self, message: SharedString, cx: &mut Context<Self>) {
        let languages = self.languages.clone();
        self.message = Some(cx.new(|cx| Markdown::new(message, languages, None, cx)));
        self.message_scroll_handle = ScrollHandle::new();
        self._message_task = None;
        self.apply_search_highlight(cx);
        cx.notify();
    }

    fn apply_search_highlight(&self, cx: &mut Context<Self>) {
        let Some(message) = self.message.as_ref() else {
            return;
        };
        let highlight = self.search_highlight.clone();
        message.update(cx, |message, cx| {
            let ranges = highlight
                .map(|(query, case_sensitive)| {
                    crate::git_graph::search_match_ranges(message.source(), &query, case_sensitive)
                })
                .unwrap_or_default();
            if ranges.is_empty() {
                message.clear_search_highlights(cx);
            } else {
                message.set_search_highlights(ranges, None, cx);
            }
        });
    }

    fn render_header(&self, sha: Oid, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let data = self.repository.upgrade().map(|repository| {
            repository.update(cx, |repository, cx| repository.fetch_commit_data(sha, false, cx).clone())
        });
        let (author_name, author_email, commit_timestamp) = match &data {
            Some(CommitDataState::Loaded(data)) => {
                (data.author_name.clone(), data.author_email.clone(), Some(data.commit_timestamp))
            }
            _ => ("Loading…".into(), "".into(), None),
        };
        let date_string = commit_timestamp.map(format_timestamp).unwrap_or_default();
        let full_sha: SharedString = sha.to_string().into();
        let short_sha: SharedString = full_sha.chars().take(7).collect::<String>().into();
        let head_branch_name: Option<SharedString> = self.repository.upgrade().and_then(|repository| {
            repository.read(cx).branch.as_ref().map(|branch| SharedString::from(branch.name().to_string()))
        });
        let background = cx.theme().colors().editor_background;

        v_flex()
            .relative()
            .w_full()
            .p_2()
            .gap_2()
            .child(
                div().absolute().top_2().left_2().child(
                    Disclosure::new("toggle-commit-metadata", !self.metadata_collapsed)
                        .tooltip({
                            let label =
                                if self.metadata_collapsed { "Show Commit Details" } else { "Hide Commit Details" };
                            move |_window, cx: &mut App| Tooltip::simple(label, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.metadata_collapsed = !this.metadata_collapsed;
                            cx.notify();
                        })),
                ),
            )
            .map(|this| {
                if self.metadata_collapsed {
                    return this.child(
                        h_flex()
                            .w_full()
                            .px_6()
                            .gap_1()
                            .justify_center()
                            .overflow_hidden()
                            .child(Label::new(author_name).size(LabelSize::Small).truncate())
                            .child(Label::new(short_sha).size(LabelSize::Small).color(Color::Muted)),
                    );
                }
                this.child(
                    v_flex()
                        .py_1()
                        .w_full()
                        .items_center()
                        .child(Label::new(author_name))
                        .child(Label::new(date_string).color(Color::Muted).size(LabelSize::Small)),
                )
                .children((!self.ref_names.is_empty()).then(|| {
                    h_flex().gap_1().flex_wrap().justify_center().children(self.ref_names.iter().map(|name| {
                        let is_head = crate::git_graph::GitGraph::is_head_ref(name, &head_branch_name);
                        let name_for_menu = name.clone();
                        let this = cx.weak_entity();
                        render_ref_chip(
                            name.clone(),
                            SharedString::from(format!("commit-details-ref-{name}")),
                            self.ref_color,
                            is_head,
                            background,
                            move |position, _window, cx| {
                                this.update(cx, |_, cx| {
                                    cx.emit(CommitDetailsEvent::DeployRefMenu { position, name: name_for_menu.clone() })
                                })
                                .ok();
                            },
                        )
                    }))
                }))
                .child(
                    v_flex()
                        .ml_neg_1()
                        .gap_1p5()
                        .when(!author_email.is_empty(), |this| {
                            this.child(render_copy_button(
                                "author-email-copy",
                                author_email.clone(),
                                IconName::Envelope,
                                "Copy Email",
                                "Email Copied!",
                                window,
                                cx,
                            ))
                        })
                        .child(render_copy_button(
                            "sha-copy",
                            full_sha,
                            IconName::Hash,
                            "Copy Commit SHA",
                            "Commit SHA Copied!",
                            window,
                            cx,
                        )),
                )
            })
    }

    fn render_message(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.message.as_ref() else {
            return gpui::Empty.into_any_element();
        };

        let message_style = editor::hover_markdown_style(window, cx);
        let line_height = message_style.base_text_style.line_height_in_pixels(window.rem_size());
        let collapsed = self.message_collapsed;

        div()
            // Grid instead of flexbox: inside this side panel taffy can't compute
            // a concrete width for flex children, which breaks text reflow.
            .w_full()
            .py_2()
            .pl_2()
            .grid()
            .grid_cols(1)
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        Disclosure::new("toggle-commit-message", !collapsed)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.message_collapsed = !this.message_collapsed;
                                cx.notify();
                            }))
                            .tooltip({
                                let label = if collapsed { "Show Commit Message" } else { "Hide Commit Message" };
                                move |_window, cx: &mut App| Tooltip::simple(label, cx)
                            }),
                    )
                    .child(Label::new("Message").size(LabelSize::Small).color(Color::Muted)),
            )
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .relative()
                        .w_full()
                        .child(
                            div()
                                .id("commit-message")
                                .text_sm()
                                .w_full()
                                .max_h(line_height * 12.)
                                .overflow_y_scroll()
                                .track_scroll(&self.message_scroll_handle)
                                .child(MarkdownElement::new(message.clone(), message_style)),
                        )
                        .vertical_scrollbar_for(&self.message_scroll_handle, window, cx),
                )
            })
            .into_any_element()
    }

    fn render_changed_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut files = self.files.clone().unwrap_or_default();
        let changed_files_count = files.len();
        let (lines_added, lines_removed) = self.diff_stats;
        let is_tree_view = self.view_mode.is_tree();
        if !is_tree_view {
            files.sort_by_key(|file| file.commit_status);
        }
        let flat_entries = Rc::new(files);
        let tree_entries: Rc<Vec<ChangedFileTreeEntry>> = if is_tree_view {
            Rc::new(build_changed_file_tree_entries(flat_entries.as_ref().clone(), &self.expanded_dirs))
        } else {
            Rc::default()
        };
        if std::mem::take(&mut self.scroll_to_revealed)
            && let Some(ix) = self
                .revealed_path
                .as_ref()
                .and_then(|path| revealed_entry_index(is_tree_view, &flat_entries, &tree_entries, path))
        {
            self.files_scroll_handle.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        let revealed_path = self.revealed_path.clone();

        let view_toggle = IconButton::new("toggle-changed-files-view", IconName::ListTree)
            .icon_size(IconSize::Small)
            .toggle_state(is_tree_view)
            .tooltip({
                let tooltip = if is_tree_view { "Show Flat View" } else { "Show Tree View" };
                move |_, cx| Tooltip::for_action(tooltip, &ToggleChangedFilesView, cx)
            })
            .on_click(cx.listener(|this, _, _window, cx| this.toggle_view_mode(cx)));

        v_flex()
            .min_w_0()
            .flex_1()
            .overflow_hidden()
            .child(
                h_flex()
                    .p_2()
                    .pr_3()
                    .pb_1()
                    .gap_1()
                    .w_full()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Label::new(format!(
                                    "{} Changed {}",
                                    changed_files_count,
                                    if changed_files_count == 1 { "File" } else { "Files" }
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            )
                            .child(Divider::vertical())
                            .child(view_toggle),
                    )
                    .child(DiffStat::new("commit-diff-stat", lines_added, lines_removed)),
            )
            .child(
                div()
                    .id("changed-files-container")
                    .flex_1()
                    .min_h_0()
                    .child({
                        let entry_count = if is_tree_view { tree_entries.len() } else { flat_entries.len() };
                        let panel = cx.weak_entity();
                        let indent_tree_entries = tree_entries.clone();

                        uniform_list("changed-files-list", entry_count, move |range, _window, cx| {
                            let is_revealed =
                                |entry: &ChangedFileEntry| revealed_path.as_ref() == Some(&entry.repo_path);
                            range
                                .map(|ix| {
                                    if is_tree_view {
                                        match &tree_entries[ix] {
                                            ChangedFileTreeEntry::Directory(entry) => {
                                                entry.render(ix, panel.clone(), cx)
                                            }
                                            ChangedFileTreeEntry::File(entry) => entry.entry.render(
                                                ix,
                                                entry.depth,
                                                None,
                                                is_revealed(&entry.entry),
                                                panel.clone(),
                                            ),
                                        }
                                    } else {
                                        let directory_label = (!flat_entries[ix].dir_path.is_empty())
                                            .then(|| flat_entries[ix].dir_path.clone());
                                        flat_entries[ix].render(
                                            ix,
                                            0,
                                            directory_label,
                                            is_revealed(&flat_entries[ix]),
                                            panel.clone(),
                                        )
                                    }
                                })
                                .collect()
                        })
                        .when(is_tree_view, |list| {
                            list.with_decoration(
                                ui::indent_guides(px(TREE_INDENT), IndentGuideColors::panel(cx))
                                    .with_left_offset(ui::LIST_ITEM_INDENT_GUIDE_LEFT_OFFSET - px(2.))
                                    .with_compute_indents_fn(cx.entity(), move |_, range, _window, _cx| {
                                        range
                                            .map(|ix| match indent_tree_entries.get(ix) {
                                                Some(ChangedFileTreeEntry::Directory(entry)) => entry.depth,
                                                Some(ChangedFileTreeEntry::File(entry)) => entry.depth,
                                                None => 0,
                                            })
                                            .collect()
                                    }),
                            )
                        })
                        .size_full()
                        .track_scroll(&self.files_scroll_handle)
                    })
                    .vertical_scrollbar_for(&self.files_scroll_handle, window, cx),
            )
    }
}

impl Render for CommitDetailsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(sha) = self.sha else {
            return div().into_any_element();
        };

        v_flex()
            .size_full()
            .child(self.render_header(sha, window, cx))
            .child(Divider::horizontal())
            .child(self.render_message(window, cx))
            .child(Divider::horizontal())
            .child(self.render_changed_files(window, cx))
            .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                deferred(anchored().position(*position).anchor(Anchor::TopLeft).child(menu.clone())).with_priority(1)
            }))
            .into_any_element()
    }
}

struct CopiedState {
    copied_at: Option<Instant>,
}

impl CopiedState {
    const fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { copied_at: None }
    }

    fn is_copied(&self) -> bool {
        self.copied_at.is_some_and(|copied_at| copied_at.elapsed() < COPIED_STATE_DURATION)
    }

    fn mark_copied(&mut self) {
        self.copied_at = Some(Instant::now());
    }
}

fn render_copy_button(
    id: &'static str,
    text: SharedString,
    icon: IconName,
    tooltip_label: &'static str,
    copied_label: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let copied_state: Entity<CopiedState> = window.use_keyed_state(id, cx, CopiedState::new);
    let is_copied = copied_state.read(cx).is_copied();
    let (icon, icon_color, tooltip_label) =
        if is_copied { (IconName::Check, Color::Success, copied_label) } else { (icon, Color::Muted, tooltip_label) };

    Button::new(id, text.clone())
        .start_icon(Icon::new(icon).size(IconSize::Small).color(icon_color))
        .label_size(LabelSize::Small)
        .truncate(true)
        .color(Color::Muted)
        .tooltip({
            let text = text.clone();
            move |_, cx| Tooltip::with_meta(tooltip_label, None, text.clone(), cx)
        })
        .on_click(move |_, _, cx| {
            copied_state.update(cx, |state, _cx| state.mark_copied());
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            let state_id = copied_state.entity_id();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(COPIED_STATE_DURATION).await;
                cx.update(|cx| cx.notify(state_id))
            })
            .detach();
        })
}

pub(crate) fn render_ref_chip(
    name: SharedString,
    id: SharedString,
    accent_color: Hsla,
    is_head: bool,
    background: Hsla,
    on_secondary_click: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let tooltip_text = name.clone();

    div()
        .id(id)
        .min_w_0()
        .overflow_hidden()
        .child(Chip::new(name).label_size(LabelSize::Small).truncate().map(|chip| {
            if is_head {
                chip.icon(IconName::Check)
                    .bg_color(background.blend(accent_color.opacity(0.25)))
                    .border_color(accent_color.opacity(0.5))
            } else {
                chip.icon(IconName::GitBranch)
                    .icon_color(Color::Custom(accent_color))
                    .bg_color(background.blend(accent_color.opacity(0.08)))
                    .border_color(accent_color.opacity(0.25))
            }
        }))
        .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            on_secondary_click(event.position, window, cx);
        })
        .tooltip(move |_window, cx| Tooltip::simple(tooltip_text.clone(), cx))
        .into_any_element()
}

fn compute_diff_stats(diff: &CommitDiff) -> (usize, usize) {
    diff.files.iter().fold((0, 0), |(added, removed), file| {
        let old_text = file.old_text.as_deref().unwrap_or("");
        let new_text = file.new_text.as_deref().unwrap_or("");
        let hunks = line_diff(old_text, new_text);
        hunks.iter().fold((added, removed), |(a, r), (old_range, new_range)| {
            (a + (new_range.end - new_range.start) as usize, r + (old_range.end - old_range.start) as usize)
        })
    })
}

#[derive(Clone, Debug, PartialEq)]
struct ChangedFileEntry {
    status: FileStatus,
    commit_status: CommitFileStatus,
    file_name: SharedString,
    dir_path: SharedString,
    repo_path: RepoPath,
}

impl ChangedFileEntry {
    fn from_commit_file(file: &CommitFile) -> Self {
        let file_name: SharedString = file.path.file_name().map(|n| n.to_string()).unwrap_or_default().into();
        let dir_path: SharedString = file.path.parent().map(|p| p.as_unix_str().to_string()).unwrap_or_default().into();

        let status_code = match (&file.old_text, &file.new_text) {
            (None, Some(_)) => StatusCode::Added,
            (Some(_), None) => StatusCode::Deleted,
            _ => StatusCode::Modified,
        };

        let status =
            FileStatus::Tracked(TrackedStatus { index_status: status_code, worktree_status: StatusCode::Unmodified });

        Self { status, commit_status: file.status(), file_name, dir_path, repo_path: file.path.clone() }
    }

    fn render(
        &self,
        ix: usize,
        depth: usize,
        directory_label: Option<SharedString>,
        is_revealed: bool,
        panel: WeakEntity<CommitDetailsPanel>,
    ) -> AnyElement {
        let file_name = self.file_name.clone();
        let repo_path = self.repo_path.clone();

        ListItem::new(("changed-file", ix))
            .spacing(ListItemSpacing::Sparse)
            .indent_level(depth)
            .indent_step_size(px(TREE_INDENT))
            .toggle_state(is_revealed)
            .start_slot(git_status_icon(self.status))
            .child(Label::new(file_name).size(LabelSize::Small).truncate())
            .when_some(directory_label, |this, directory_label| {
                this.child(Label::new(directory_label).size(LabelSize::Small).color(Color::Muted).truncate_start())
            })
            .on_click({
                let panel = panel.clone();
                let repo_path = repo_path.clone();
                move |_, _, cx| {
                    panel.update(cx, |_, cx| cx.emit(CommitDetailsEvent::OpenFile(repo_path.clone()))).ok();
                }
            })
            .on_secondary_mouse_down(move |event, window, cx| {
                cx.stop_propagation();
                panel
                    .update(cx, |panel, cx| {
                        panel.deploy_file_context_menu(event.position, repo_path.clone(), window, cx);
                    })
                    .ok();
            })
            .into_any_element()
    }
}

#[derive(Debug, PartialEq)]
enum ChangedFileTreeEntry {
    Directory(ChangedFileDirectoryEntry),
    File(ChangedFileTreeStatusEntry),
}

#[derive(Debug, PartialEq)]
struct ChangedFileTreeStatusEntry {
    entry: ChangedFileEntry,
    depth: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ChangedFilesViewMode {
    Flat,
    #[default]
    Tree,
}

impl ChangedFilesViewMode {
    const fn toggled(self) -> Self {
        match self {
            Self::Flat => Self::Tree,
            Self::Tree => Self::Flat,
        }
    }

    const fn is_tree(self) -> bool {
        matches!(self, Self::Tree)
    }
}

#[derive(Debug, PartialEq)]
struct ChangedFileDirectoryEntry {
    path: RepoPath,
    name: SharedString,
    depth: usize,
    expanded: bool,
}

impl ChangedFileDirectoryEntry {
    fn render(&self, ix: usize, panel: WeakEntity<CommitDetailsPanel>, cx: &App) -> AnyElement {
        let path = self.path.clone();
        let expanded = self.expanded;
        let folder_icon = FileIcons::get_folder_icon(expanded, path.as_std_path(), cx)
            .map(|icon| Icon::from_path(icon).size(IconSize::Small).color(Color::Muted))
            .unwrap_or_else(|| {
                let icon = if expanded { IconName::FolderOpen } else { IconName::Folder };
                Icon::new(icon).size(IconSize::Small).color(Color::Muted)
            });

        ListItem::new(("changed-file-dir", ix))
            .spacing(ListItemSpacing::Sparse)
            .indent_level(self.depth)
            .indent_step_size(px(TREE_INDENT))
            .start_slot(folder_icon)
            .child(Label::new(self.name.clone()).size(LabelSize::Small).color(Color::Muted).truncate())
            .tooltip({
                let name = self.name.clone();
                move |_, cx| Tooltip::with_meta("Toggle Folder", None, name.clone(), cx)
            })
            .on_click(move |_, _, cx| {
                panel
                    .update(cx, |panel, cx| {
                        panel.expanded_dirs.insert(path.clone(), !expanded);
                        cx.notify();
                    })
                    .ok();
            })
            .into_any_element()
    }
}

#[derive(Default)]
struct ChangedFileTreeNode {
    name: SharedString,
    path: Option<RepoPath>,
    children: BTreeMap<SharedString, ChangedFileTreeNode>,
    files: Vec<ChangedFileEntry>,
}

/// Folders containing `path`, outermost first.
fn ancestor_dirs(path: &RepoPath) -> Vec<RepoPath> {
    let mut dirs = Vec::new();
    let mut dir = path.parent();
    while let Some(parent) = dir.filter(|parent| !parent.is_empty()) {
        dirs.push(RepoPath::from_rel_path(parent));
        dir = parent.parent();
    }
    dirs.reverse();
    dirs
}

/// Row of `path` in the changed files list for the current view mode.
fn revealed_entry_index(
    is_tree_view: bool,
    flat_entries: &[ChangedFileEntry],
    tree_entries: &[ChangedFileTreeEntry],
    path: &RepoPath,
) -> Option<usize> {
    if is_tree_view {
        tree_entries
            .iter()
            .position(|entry| matches!(entry, ChangedFileTreeEntry::File(file) if &file.entry.repo_path == path))
    } else {
        flat_entries.iter().position(|entry| &entry.repo_path == path)
    }
}

fn build_changed_file_tree_entries(
    mut files: Vec<ChangedFileEntry>,
    expanded_dirs: &HashMap<RepoPath, bool>,
) -> Vec<ChangedFileTreeEntry> {
    files.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));

    let mut root = ChangedFileTreeNode::default();
    for file in files {
        let components: Vec<&str> = file.repo_path.components().collect();
        if components.is_empty() {
            root.files.push(file);
            continue;
        }

        let mut current = &mut root;
        let mut current_path = String::new();

        for (ix, component) in components.iter().enumerate() {
            if ix == components.len() - 1 {
                current.files.push(file.clone());
            } else {
                if !current_path.is_empty() {
                    current_path.push('/');
                }
                current_path.push_str(component);

                let Ok(dir_path) = RepoPath::new(&current_path) else {
                    continue;
                };
                let component = SharedString::from(component.to_string());

                current = current.children.entry(component.clone()).or_insert_with(|| ChangedFileTreeNode {
                    name: component,
                    path: Some(dir_path),
                    ..Default::default()
                });
            }
        }
    }

    flatten_changed_file_tree(&root, 0, expanded_dirs)
}

fn flatten_changed_file_tree(
    node: &ChangedFileTreeNode,
    depth: usize,
    expanded_dirs: &HashMap<RepoPath, bool>,
) -> Vec<ChangedFileTreeEntry> {
    let mut entries = Vec::new();

    for child in node.children.values() {
        let (terminal, name) = compact_changed_file_directory_chain(child);
        let Some(path) = terminal.path.clone().or_else(|| child.path.clone()) else {
            continue;
        };
        let expanded = *expanded_dirs.get(&path).unwrap_or(&true);
        let child_entries = flatten_changed_file_tree(terminal, depth + 1, expanded_dirs);

        entries.push(ChangedFileTreeEntry::Directory(ChangedFileDirectoryEntry { path, name, depth, expanded }));

        if expanded {
            entries.extend(child_entries);
        }
    }

    entries.extend(
        node.files.iter().cloned().map(|entry| ChangedFileTreeEntry::File(ChangedFileTreeStatusEntry { entry, depth })),
    );
    entries
}

fn compact_changed_file_directory_chain(mut node: &ChangedFileTreeNode) -> (&ChangedFileTreeNode, SharedString) {
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
    (node, SharedString::from(parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_file(path: &str, old_text: Option<&str>, new_text: Option<&str>) -> CommitFile {
        CommitFile {
            path: RepoPath::new(path).unwrap(),
            old_text: old_text.map(str::to_string),
            new_text: new_text.map(str::to_string),
            is_binary: false,
        }
    }

    fn entry(path: &str) -> ChangedFileEntry {
        ChangedFileEntry::from_commit_file(&commit_file(path, Some("a"), Some("b")))
    }

    fn describe(entries: &[ChangedFileTreeEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                ChangedFileTreeEntry::Directory(dir) => format!("{}dir {}", "  ".repeat(dir.depth), dir.name),
                ChangedFileTreeEntry::File(file) => {
                    format!("{}file {}", "  ".repeat(file.depth), file.entry.file_name)
                }
            })
            .collect()
    }

    #[test]
    fn test_ancestor_dirs() {
        let dirs = ancestor_dirs(&RepoPath::new("crates/git_ui/src/lib.rs").unwrap());
        let expected: Vec<RepoPath> =
            ["crates", "crates/git_ui", "crates/git_ui/src"].iter().map(|dir| RepoPath::new(dir).unwrap()).collect();
        assert_eq!(dirs, expected);
        assert!(ancestor_dirs(&RepoPath::new("README.md").unwrap()).is_empty());
    }

    #[test]
    fn test_revealed_entry_index() {
        let files = vec![entry("b.rs"), entry("src/a.rs")];
        let path = RepoPath::new("src/a.rs").unwrap();
        assert_eq!(revealed_entry_index(false, &files, &[], &path), Some(1));

        let tree = build_changed_file_tree_entries(files.clone(), &HashMap::default());
        assert_eq!(describe(&tree), ["dir src", "  file a.rs", "file b.rs"]);
        assert_eq!(revealed_entry_index(true, &files, &tree, &path), Some(1));
        assert_eq!(revealed_entry_index(true, &files, &tree, &RepoPath::new("missing.rs").unwrap()), None);
    }

    #[test]
    fn test_changed_file_entry_from_commit_file() {
        let added = ChangedFileEntry::from_commit_file(&commit_file("src/new.rs", None, Some("x")));
        assert_eq!(added.file_name.as_ref(), "new.rs");
        assert_eq!(added.dir_path.as_ref(), "src");
        assert_eq!(added.commit_status, CommitFileStatus::Added);
        assert_eq!(
            added.status,
            FileStatus::Tracked(TrackedStatus {
                index_status: StatusCode::Added,
                worktree_status: StatusCode::Unmodified
            })
        );

        let deleted = ChangedFileEntry::from_commit_file(&commit_file("old.rs", Some("x"), None));
        assert_eq!(deleted.dir_path.as_ref(), "");
        assert_eq!(deleted.commit_status, CommitFileStatus::Deleted);

        let modified = ChangedFileEntry::from_commit_file(&commit_file("a/b/c.rs", Some("x"), Some("y")));
        assert_eq!(modified.dir_path.as_ref(), "a/b");
        assert_eq!(modified.commit_status, CommitFileStatus::Modified);
    }

    #[test]
    fn test_build_changed_file_tree_entries() {
        let files = vec![entry("src/b.rs"), entry("README.md"), entry("src/a.rs"), entry("crates/ui/src/lib.rs")];

        let tree = build_changed_file_tree_entries(files.clone(), &HashMap::default());
        assert_eq!(
            describe(&tree),
            ["dir crates/ui/src", "  file lib.rs", "dir src", "  file a.rs", "  file b.rs", "file README.md"]
        );

        let mut collapsed = HashMap::default();
        collapsed.insert(RepoPath::new("src").unwrap(), false);
        let tree = build_changed_file_tree_entries(files, &collapsed);
        assert_eq!(describe(&tree), ["dir crates/ui/src", "  file lib.rs", "dir src", "file README.md"]);
    }

    #[test]
    fn test_compute_diff_stats() {
        let diff = CommitDiff {
            files: vec![
                commit_file("a.rs", Some("one\ntwo\n"), Some("one\nTWO\nthree\n")),
                commit_file("b.rs", None, Some("new\n")),
                commit_file("c.rs", Some("gone\n"), None),
            ],
            is_shallow_boundary: false,
        };
        assert_eq!(compute_diff_stats(&diff), (3, 2));
    }

    #[test]
    fn test_changed_files_view_mode_toggled() {
        assert_eq!(ChangedFilesViewMode::default(), ChangedFilesViewMode::Tree);
        assert!(ChangedFilesViewMode::Tree.is_tree());
        assert_eq!(ChangedFilesViewMode::Tree.toggled(), ChangedFilesViewMode::Flat);
        assert_eq!(ChangedFilesViewMode::Flat.toggled(), ChangedFilesViewMode::Tree);
    }
}
