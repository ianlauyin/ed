use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

use collections::BTreeMap;
use collections::HashMap;
use collections::IndexSet;
use ed_actions::buffer_search;
pub use ed_actions::git_graph::Open;
use ed_actions::search::SelectNextMatch;
use ed_actions::search::SelectPreviousMatch;
use ed_actions::search::ToggleCaseSensitive;
use editor::Editor;
use git::Oid;
use git::repository::InitialGraphCommitData;
use git::repository::LogOrder;
use git::repository::LogSource;
use git::repository::RepoPath;
use git::repository::SearchCommitArgs;
use git::repository::commit_hash_search_query;
use gpui::Action;
use gpui::Anchor;
use gpui::AnyElement;
use gpui::AnyView;
use gpui::App;
use gpui::Bounds;
use gpui::ClickEvent;
use gpui::ClipboardItem;
use gpui::DefiniteLength;
use gpui::DismissEvent;
use gpui::ElementId;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::Hsla;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::PathBuilder;
use gpui::Pixels;
use gpui::Point;
use gpui::ScrollStrategy;
use gpui::ScrollWheelEvent;
use gpui::SharedString;
use gpui::Subscription;
use gpui::Task;
use gpui::TextRun;
use gpui::TextStyleRefinement;
use gpui::WeakEntity;
use gpui::Window;
use gpui::actions;
use gpui::anchored;
use gpui::deferred;
use gpui::point;
use gpui::prelude::*;
use gpui::px;
use gpui::relative;
use language::LanguageRegistry;
use menu::SelectFirst;
use menu::SelectLast;
use menu::SelectNext;
use menu::SelectPrevious;
use picker::Picker;
use picker::PickerDelegate;
use project::ProjectPath;
use project::git_store::CommitDataState;
use project::git_store::GitGraphEvent;
use project::git_store::GitStore;
use project::git_store::GitStoreEvent;
use project::git_store::GraphDataResponse;
use project::git_store::Repository;
use project::git_store::RepositoryEvent;
use project::git_store::RepositoryId;
use smallvec::SmallVec;
use smallvec::smallvec;
use theme::AccentColors;
use time::OffsetDateTime;
use time::UtcOffset;
use time::format_description::BorrowedFormatItem;
use ui::Chip;
use ui::ColumnWidthConfig;
use ui::CommonAnimationExt as _;
use ui::ContextMenu;
use ui::HeaderResizeInfo;
use ui::HighlightedLabel;
use ui::ListItem;
use ui::ListItemSpacing;
use ui::RedistributableColumnsState;
use ui::ScrollableHandle;
use ui::Table;
use ui::TableInteractionState;
use ui::TableRenderContext;
use ui::TableResizeBehavior;
use ui::Tooltip;
use ui::bind_redistributable_columns;
use ui::prelude::*;
use ui::redistribute_hidden_fractions;
use ui::redistribute_hidden_widths;
use ui::render_redistributable_columns_resize_handles;
use ui::render_table_header;
use ui::table_row::TableRow;
use workspace::ItemNavHistory;
use workspace::ModalView;
use workspace::Workspace;
use workspace::WorkspaceMode;
use workspace::item::Item;
use workspace::item::ItemEvent;
use workspace::item::TabTooltipContent;

use crate::commit_context_menu::CommitContextMenuData;
pub use crate::commit_context_menu::CopyCommitSha;
pub use crate::commit_context_menu::CopyCommitTag;
pub use crate::commit_context_menu::OpenCommitView;
use crate::commit_context_menu::commit_context_menu;
use crate::commit_details_panel::CommitDetailsEvent;
use crate::commit_details_panel::CommitDetailsPanel;
use crate::commit_details_panel::render_ref_chip;
use crate::commit_view::CommitView;
use crate::commit_view::open_project_file;

const COMMIT_CIRCLE_RADIUS: Pixels = px(4.5);
const COMMIT_CIRCLE_STROKE_WIDTH: Pixels = px(1.5);
const COMMIT_CIRCLE_HALO_WIDTH: Pixels = px(2.0);
const LANE_WIDTH: Pixels = px(18.0);
const LEFT_PADDING: Pixels = px(12.0);
const LINE_WIDTH: Pixels = px(2.0);
const REF_LABEL_GUTTER_MIN_WIDTH: Pixels = px(240.0);
const REF_LABEL_GUTTER_MAX_WIDTH: Pixels = px(280.0);
const COMMIT_TAG_LIST_WIDTH_IN_REMS: Rems = rems(10.);
const TABLE_COLUMN_COUNT: usize = 4;
const ROW_VERTICAL_PADDING: Pixels = px(4.0);

struct CommitTagPicker {
    picker: Entity<Picker<CommitTagPickerDelegate>>,
}

impl CommitTagPicker {
    fn new(tag_names: Vec<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = CommitTagPickerDelegate { picker: cx.entity().downgrade(), tag_names, selected_index: 0 };
        let picker = cx.new(|cx| {
            Picker::nonsearchable_uniform_list(delegate, window, cx).initial_width(COMMIT_TAG_LIST_WIDTH_IN_REMS)
        });
        Self { picker }
    }
}

impl EventEmitter<DismissEvent> for CommitTagPicker {}
impl ModalView for CommitTagPicker {}

impl Focusable for CommitTagPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for CommitTagPicker {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().child(self.picker.clone())
    }
}

struct CommitTagPickerDelegate {
    picker: WeakEntity<CommitTagPicker>,
    tag_names: Vec<SharedString>,
    selected_index: usize,
}

impl PickerDelegate for CommitTagPickerDelegate {
    type ListItem = ListItem;

    fn name() -> &'static str {
        "commit-tag"
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Copy Tag".into()
    }

    fn match_count(&self) -> usize {
        self.tag_names.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {
        self.selected_index = ix;
    }

    fn update_matches(&mut self, _query: String, _window: &mut Window, _cx: &mut Context<Picker<Self>>) -> Task<()> {
        Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(tag_name) = self.tag_names.get(self.selected_index) {
            cx.write_to_clipboard(ClipboardItem::new_string(tag_name.to_string()));
        }
        self.dismissed(window, cx);
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.picker.update(cx, |_this, cx| cx.emit(DismissEvent)).ok();
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(Label::new(self.tag_names.get(ix)?.clone())),
        )
    }
}

enum QueryState {
    Pending(SharedString),
    Confirmed((SharedString, Task<()>)),
    Empty,
}

impl QueryState {
    fn next_state(&mut self) {
        if let Self::Confirmed((query, _)) = self {
            *self = Self::Pending(std::mem::take(query))
        };
    }
}

struct SearchState {
    case_sensitive: bool,
    editor: Entity<Editor>,
    state: QueryState,
    matches: IndexSet<Oid>,
    selected_index: Option<usize>,
}

impl SearchState {
    fn confirmed_query(&self) -> Option<&str> {
        match &self.state {
            QueryState::Confirmed((query, _)) => Some(query.as_str()),
            _ => None,
        }
    }
}

/// Byte ranges of non-overlapping `query` occurrences in `text`, matching what
/// `git log --grep --fixed-strings` searched for.
pub(crate) fn search_match_ranges(text: &str, query: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if case_sensitive {
        return text.match_indices(query).map(|(start, matched)| start..start + matched.len()).collect();
    }

    let mut ranges = Vec::new();
    let mut start = 0;
    while let Some(rest) = text.get(start..).filter(|rest| !rest.is_empty()) {
        if let Some(len) = case_insensitive_prefix_len(rest, query) {
            ranges.push(start..start + len);
            start += len;
        } else {
            start += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    ranges
}

/// Byte length of the prefix of `haystack` that equals `needle` ignoring case.
fn case_insensitive_prefix_len(haystack: &str, needle: &str) -> Option<usize> {
    let mut haystack_chars = haystack.char_indices();
    for needle_char in needle.chars() {
        let (_, haystack_char) = haystack_chars.next()?;
        if !haystack_char.to_lowercase().eq(needle_char.to_lowercase()) {
            return None;
        }
    }
    Some(haystack_chars.next().map_or(haystack.len(), |(index, _)| index))
}

/// Highlighted prefix of a displayed `sha` when `query` was a commit hash search.
fn commit_hash_match_range(sha: &str, query: &str) -> Option<Range<usize>> {
    let hash_query = commit_hash_search_query(query)?;
    let len = hash_query.len().min(sha.len());
    sha.get(..len)?.eq_ignore_ascii_case(&hash_query[..len]).then_some(0..len)
}

actions!(
    git_graph,
    [
        /// Focuses the search field.
        FocusSearch,
        /// Focuses the next git graph tab stop.
        FocusNextTabStop,
        /// Focuses the previous git graph tab stop.
        FocusPreviousTabStop,
        /// Selects a commit half a page above the current selection.
        ScrollUp,
        /// Selects a commit half a page below the current selection.
        ScrollDown,
        /// Toggles the selected commit's changed files between flat and tree views.
        ToggleChangedFilesView,
    ]
);

/// Opens the Git Graph Tab at a specific commit.
#[derive(Clone, PartialEq, serde::Deserialize, gpui::Action)]
#[action(namespace = git_graph)]
pub struct OpenAtCommit {
    pub sha: String,
}

fn timestamp_format() -> &'static [BorrowedFormatItem<'static>] {
    static FORMAT: OnceLock<Vec<BorrowedFormatItem<'static>>> = OnceLock::new();
    FORMAT.get_or_init(|| {
        time::format_description::parse("[day] [month repr:short] [year] [hour]:[minute]").unwrap_or_default()
    })
}

pub(crate) fn format_timestamp(timestamp: i64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(timestamp) else {
        return "Unknown".to_string();
    };

    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local_datetime = datetime.to_offset(local_offset);

    local_datetime.format(timestamp_format()).unwrap_or_default()
}

fn accent_colors_count(accents: &AccentColors) -> usize {
    accents.0.len()
}

#[derive(Copy, Clone, Debug)]
struct BranchColor(u8);

#[derive(Debug)]
enum LaneState {
    Empty,
    Active {
        child: Oid,
        parent: Oid,
        color: Option<BranchColor>,
        starting_row: usize,
        starting_col: usize,
        destination_column: Option<usize>,
        segments: SmallVec<[CommitLineSegment; 1]>,
    },
}

impl LaneState {
    fn take_commit_lines(
        &mut self,
        ending_row: usize,
        lane_column: usize,
        parent_column: usize,
        parent_color: BranchColor,
    ) -> Option<CommitLine> {
        let state = std::mem::replace(self, LaneState::Empty);

        match state {
            LaneState::Active {
                #[allow(unused_variables)]
                parent,
                #[allow(unused_variables)]
                child,
                color,
                starting_row,
                starting_col,
                destination_column,
                mut segments,
            } => {
                let final_destination = destination_column.unwrap_or(parent_column);
                let final_color = color.unwrap_or(parent_color);

                Some(CommitLine {
                    child_column: starting_col,
                    full_interval: starting_row..ending_row,
                    color_idx: final_color.0 as usize,
                    segments: {
                        match segments.last_mut() {
                            Some(CommitLineSegment::Straight { to_row }) if *to_row == usize::MAX => {
                                if final_destination != lane_column {
                                    *to_row = ending_row - 1;

                                    let curved_line = CommitLineSegment::Curve {
                                        to_column: final_destination,
                                        on_row: ending_row,
                                        curve_kind: CurveKind::Checkout,
                                    };

                                    if *to_row == starting_row {
                                        let last_index = segments.len() - 1;
                                        segments[last_index] = curved_line;
                                    } else {
                                        segments.push(curved_line);
                                    }
                                } else {
                                    *to_row = ending_row;
                                }
                            }
                            Some(CommitLineSegment::Curve { on_row, to_column, curve_kind })
                                if *on_row == usize::MAX =>
                            {
                                if *to_column == usize::MAX {
                                    *to_column = final_destination;
                                }
                                if matches!(curve_kind, CurveKind::Merge) {
                                    *on_row = starting_row + 1;
                                    if *on_row < ending_row {
                                        if *to_column != final_destination {
                                            segments.push(CommitLineSegment::Straight { to_row: ending_row - 1 });
                                            segments.push(CommitLineSegment::Curve {
                                                to_column: final_destination,
                                                on_row: ending_row,
                                                curve_kind: CurveKind::Checkout,
                                            });
                                        } else {
                                            segments.push(CommitLineSegment::Straight { to_row: ending_row });
                                        }
                                    } else if *to_column != final_destination {
                                        segments.push(CommitLineSegment::Curve {
                                            to_column: final_destination,
                                            on_row: ending_row,
                                            curve_kind: CurveKind::Checkout,
                                        });
                                    }
                                } else {
                                    *on_row = ending_row;
                                    if *to_column != final_destination {
                                        segments.push(CommitLineSegment::Straight { to_row: ending_row });
                                        segments.push(CommitLineSegment::Curve {
                                            to_column: final_destination,
                                            on_row: ending_row,
                                            curve_kind: CurveKind::Checkout,
                                        });
                                    }
                                }
                            }
                            Some(CommitLineSegment::Curve { on_row, to_column, .. }) => {
                                if *on_row < ending_row {
                                    if *to_column != final_destination {
                                        segments.push(CommitLineSegment::Straight { to_row: ending_row - 1 });
                                        segments.push(CommitLineSegment::Curve {
                                            to_column: final_destination,
                                            on_row: ending_row,
                                            curve_kind: CurveKind::Checkout,
                                        });
                                    } else {
                                        segments.push(CommitLineSegment::Straight { to_row: ending_row });
                                    }
                                } else if *to_column != final_destination {
                                    segments.push(CommitLineSegment::Curve {
                                        to_column: final_destination,
                                        on_row: ending_row,
                                        curve_kind: CurveKind::Checkout,
                                    });
                                }
                            }
                            _ => {}
                        }

                        segments
                    },
                })
            }
            LaneState::Empty => None,
        }
    }

    const fn is_empty(&self) -> bool {
        match self {
            LaneState::Empty => true,
            LaneState::Active { .. } => false,
        }
    }
}

struct CommitEntry {
    data: Arc<InitialGraphCommitData>,
    lane: usize,
    color_idx: usize,
}

type ActiveLaneIdx = usize;

enum AllCommitCount {
    NotLoaded,
    Loading(usize),
    FullyLoaded(usize),
}

#[derive(Debug)]
enum CurveKind {
    Merge,
    Checkout,
}

#[derive(Debug)]
enum CommitLineSegment {
    Straight { to_row: usize },
    Curve { to_column: usize, on_row: usize, curve_kind: CurveKind },
}

#[derive(Debug)]
struct CommitLine {
    child_column: usize,
    full_interval: Range<usize>,
    color_idx: usize,
    segments: SmallVec<[CommitLineSegment; 1]>,
}

impl CommitLine {
    fn get_first_visible_segment_idx(&self, first_visible_row: usize) -> Option<(usize, usize)> {
        if first_visible_row > self.full_interval.end {
            return None;
        } else if first_visible_row <= self.full_interval.start {
            return Some((0, self.child_column));
        }

        let mut current_column = self.child_column;

        for (idx, segment) in self.segments.iter().enumerate() {
            match segment {
                CommitLineSegment::Straight { to_row } => {
                    if *to_row >= first_visible_row {
                        return Some((idx, current_column));
                    }
                }
                CommitLineSegment::Curve { to_column, on_row, .. } => {
                    if *on_row >= first_visible_row {
                        return Some((idx, current_column));
                    }
                    current_column = *to_column;
                }
            }
        }

        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CommitLineKey {
    child: Oid,
    parent: Oid,
}

struct GraphData {
    lane_states: SmallVec<[LaneState; 8]>,
    lane_colors: HashMap<ActiveLaneIdx, BranchColor>,
    parent_to_lanes: HashMap<Oid, SmallVec<[usize; 1]>>,
    next_color: BranchColor,
    accent_colors_count: usize,
    commits: Vec<Rc<CommitEntry>>,
    max_commit_count: AllCommitCount,
    max_lanes: usize,
    /// Tracks the longest ref label we've seen (by character count) so we don't
    /// have to re-scan on every commit. The actual pixel width is computed
    /// lazily in `ref_label_gutter_width`.
    widest_ref_label: Option<(SharedString, usize)>,
    lines: Vec<Rc<CommitLine>>,
    active_commit_lines: HashMap<CommitLineKey, usize>,
    active_commit_lines_by_parent: HashMap<Oid, SmallVec<[usize; 1]>>,
}

impl GraphData {
    fn new(accent_colors_count: usize) -> Self {
        GraphData {
            lane_states: SmallVec::default(),
            lane_colors: HashMap::default(),
            parent_to_lanes: HashMap::default(),
            next_color: BranchColor(0),
            accent_colors_count,
            commits: Vec::default(),
            max_commit_count: AllCommitCount::NotLoaded,
            max_lanes: 0,
            widest_ref_label: None,
            lines: Vec::default(),
            active_commit_lines: HashMap::default(),
            active_commit_lines_by_parent: HashMap::default(),
        }
    }

    fn clear(&mut self) {
        self.lane_states.clear();
        self.lane_colors.clear();
        self.parent_to_lanes.clear();
        self.commits.clear();
        self.lines.clear();
        self.active_commit_lines.clear();
        self.active_commit_lines_by_parent.clear();
        self.next_color = BranchColor(0);
        self.max_commit_count = AllCommitCount::NotLoaded;
        self.max_lanes = 0;
        self.widest_ref_label = None;
    }

    fn first_empty_lane_idx(&mut self) -> ActiveLaneIdx {
        self.lane_states.iter().position(LaneState::is_empty).unwrap_or_else(|| {
            self.lane_states.push(LaneState::Empty);
            self.lane_states.len() - 1
        })
    }

    fn get_lane_color(&mut self, lane_idx: ActiveLaneIdx) -> BranchColor {
        let accent_colors_count = self.accent_colors_count;
        *self.lane_colors.entry(lane_idx).or_insert_with(|| {
            let color_idx = self.next_color;
            self.next_color = BranchColor((self.next_color.0 + 1) % accent_colors_count as u8);
            color_idx
        })
    }

    fn add_commits(&mut self, commits: &[Arc<InitialGraphCommitData>]) {
        self.commits.reserve(commits.len());
        self.lines.reserve(commits.len() / 2);

        for commit in commits {
            let commit_row = self.commits.len();

            let commit_lane = self.parent_to_lanes.get(&commit.sha).and_then(|lanes| lanes.iter().min().copied());

            let commit_lane = commit_lane.unwrap_or_else(|| self.first_empty_lane_idx());

            let commit_color = self.get_lane_color(commit_lane);

            if let Some(lanes) = self.parent_to_lanes.remove(&commit.sha) {
                for lane_column in lanes {
                    let state = &mut self.lane_states[lane_column];

                    if let LaneState::Active { starting_row, segments, .. } = state
                        && let Some(CommitLineSegment::Curve { to_column, curve_kind: CurveKind::Merge, .. }) =
                            segments.first_mut()
                    {
                        let curve_row = *starting_row + 1;
                        let would_overlap = if lane_column != commit_lane && curve_row < commit_row {
                            self.commits[curve_row..commit_row].iter().any(|c| c.lane == commit_lane)
                        } else {
                            false
                        };

                        if would_overlap {
                            *to_column = lane_column;
                        }
                    }

                    if let Some(commit_line) =
                        state.take_commit_lines(commit_row, lane_column, commit_lane, commit_color)
                    {
                        self.lines.push(Rc::new(commit_line));
                    }
                }
            }

            commit.parents.iter().enumerate().for_each(|(parent_idx, parent)| {
                if parent_idx == 0 {
                    self.lane_states[commit_lane] = LaneState::Active {
                        parent: *parent,
                        child: commit.sha,
                        color: Some(commit_color),
                        starting_col: commit_lane,
                        starting_row: commit_row,
                        destination_column: None,
                        segments: smallvec![CommitLineSegment::Straight { to_row: usize::MAX }],
                    };

                    self.parent_to_lanes.entry(*parent).or_default().push(commit_lane);
                } else {
                    let new_lane = self.first_empty_lane_idx();

                    self.lane_states[new_lane] = LaneState::Active {
                        parent: *parent,
                        child: commit.sha,
                        color: None,
                        starting_col: commit_lane,
                        starting_row: commit_row,
                        destination_column: None,
                        segments: smallvec![CommitLineSegment::Curve {
                            to_column: usize::MAX,
                            on_row: usize::MAX,
                            curve_kind: CurveKind::Merge,
                        },],
                    };

                    self.parent_to_lanes.entry(*parent).or_default().push(new_lane);
                }
            });

            self.max_lanes = self.max_lanes.max(self.lane_states.len());

            for ref_name in &commit.ref_names {
                let char_count = ref_name.chars().count();
                let is_wider =
                    self.widest_ref_label.as_ref().is_none_or(|(_, widest_count)| char_count > *widest_count);
                if is_wider {
                    self.widest_ref_label = Some((ref_name.clone(), char_count));
                }
            }

            self.commits.push(Rc::new(CommitEntry {
                data: commit.clone(),
                lane: commit_lane,
                color_idx: commit_color.0 as usize,
            }));
        }

        self.max_commit_count = AllCommitCount::Loading(self.commits.len());
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut workspace::Workspace, _, _| {
        workspace.register_action_renderer(|div, workspace, window, cx| {
            div.when_some(resolve_file_history_target(workspace, window, cx), |div, (repo_id, log_source)| {
                let git_store = workspace.project().read(cx).git_store().clone();
                let workspace = workspace.weak_handle();

                div.on_action(move |_: &git::FileHistory, window, cx| {
                    let git_store = git_store.clone();
                    workspace
                        .update(cx, |workspace, cx| {
                            open_or_reuse_graph(workspace, repo_id, git_store, log_source.clone(), None, window, cx);
                        })
                        .ok();
                })
            })
            .when(workspace.project().read(cx).active_repository(cx).is_some(), |div| {
                let workspace = workspace.weak_handle();

                div.on_action({
                    let workspace = workspace.clone();
                    move |_: &Open, window, cx| {
                        workspace
                            .update(cx, |workspace, cx| {
                                let Some(repo) = workspace.project().read(cx).active_repository(cx) else {
                                    return;
                                };
                                let selected_repo_id = repo.read(cx).id;

                                let git_store = workspace.project().read(cx).git_store().clone();
                                open_or_reuse_graph(
                                    workspace,
                                    selected_repo_id,
                                    git_store,
                                    LogSource::All,
                                    None,
                                    window,
                                    cx,
                                );
                            })
                            .ok();
                    }
                })
                .on_action(move |action: &OpenAtCommit, window, cx| {
                    let sha = action.sha.clone();
                    workspace
                        .update(cx, |workspace, cx| {
                            let Some(repo) = workspace.project().read(cx).active_repository(cx) else {
                                return;
                            };
                            let selected_repo_id = repo.read(cx).id;

                            let git_store = workspace.project().read(cx).git_store().clone();
                            open_or_reuse_graph(
                                workspace,
                                selected_repo_id,
                                git_store,
                                LogSource::All,
                                Some(sha),
                                window,
                                cx,
                            );
                        })
                        .ok();
                })
            })
        });
    })
    .detach();
}

/// Resolves a `git::FileHistory` target from a known project path (used by
/// callers like `project_panel` that own a focused selection but cannot be
/// referenced from this module due to dependency direction).
pub fn resolve_file_history_target_from_project_path(
    workspace: &Workspace,
    project_path: &ProjectPath,
    cx: &App,
) -> Option<(RepositoryId, LogSource)> {
    let git_store = workspace.project().read(cx).git_store();
    let (repo, repo_path) = git_store.read(cx).repository_and_path_for_project_path(project_path, cx)?;
    let log_source = if repo_path.is_empty() { LogSource::All } else { LogSource::Path(repo_path) };
    Some((repo.read(cx).id, log_source))
}

fn resolve_file_history_target(workspace: &Workspace, window: &Window, cx: &App) -> Option<(RepositoryId, LogSource)> {
    if let Some(panel) = workspace.panel::<crate::git_panel::GitPanel>(cx)
        && panel.read(cx).focus_handle(cx).contains_focused(window, cx)
        && let Some((repository, repo_path)) = panel.read(cx).selected_file_history_target()
    {
        return Some((repository.read(cx).id, LogSource::Path(repo_path)));
    }

    let editor = workspace.active_item_as::<Editor>(cx)?;

    let file = editor.read(cx).file_at(editor.read(cx).selections.newest_anchor().head(), cx)?;
    let project_path = ProjectPath { worktree_id: file.worktree_id(cx), path: file.path().clone() };

    let git_store = workspace.project().read(cx).git_store();
    let (repo, repo_path) = git_store.read(cx).repository_and_path_for_project_path(&project_path, cx)?;
    Some((repo.read(cx).id, LogSource::Path(repo_path)))
}

/// Shows the git graph for `repo_id` and `log_source` in the git history
/// pane, reusing an open one, and switches the window to Git History mode.
pub fn open_or_reuse_graph(
    workspace: &mut Workspace,
    repo_id: RepositoryId,
    git_store: Entity<GitStore>,
    log_source: LogSource,
    sha: Option<String>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let pane = workspace.git_history_pane().clone();
    let existing = pane.read(cx).items_of_type::<GitGraph>().find(|graph| {
        let graph = graph.read(cx);
        graph.repo_id == repo_id && graph.log_source == log_source
    });

    let git_graph = if let Some(existing) = existing {
        pane.update(cx, |pane, cx| {
            if let Some(ix) = pane.index_for_item(&existing) {
                pane.activate_item(ix, true, true, window, cx);
            }
        });
        existing
    } else {
        let workspace_handle = workspace.weak_handle();
        let languages = workspace.project().read(cx).languages().clone();
        let git_graph =
            cx.new(|cx| GitGraph::new(repo_id, git_store, workspace_handle, languages, Some(log_source), window, cx));
        // The repository-wide graph is always the first tab.
        let destination = (git_graph.read(cx).log_source == LogSource::All).then_some(0);
        pane.update(cx, |pane, cx| pane.add_item(Box::new(git_graph.clone()), true, true, destination, window, cx));
        git_graph
    };
    workspace.set_mode(WorkspaceMode::GitHistory, window, cx);

    if let Some(sha) = sha {
        cx.defer(move |cx| {
            git_graph.update(cx, |graph, cx| {
                graph.select_commit_by_sha(sha.as_str(), cx);
            });
        });
    }
}

/// The path a file history reveals in each commit's changed files.
fn history_reveal_path(log_source: &LogSource) -> Option<RepoPath> {
    match log_source {
        LogSource::Path(path) => Some(path.clone()),
        LogSource::All | LogSource::Branch(_) | LogSource::Sha(_) => None,
    }
}

fn lane_center_x(bounds: Bounds<Pixels>, lane: f32) -> Pixels {
    bounds.origin.x + LEFT_PADDING + lane * LANE_WIDTH + LANE_WIDTH / 2.0
}

fn to_row_center(to_row: usize, row_height: Pixels, scroll_offset: Pixels, bounds: Bounds<Pixels>) -> Pixels {
    bounds.origin.y + to_row as f32 * row_height + row_height / 2.0 - scroll_offset
}

fn fill_circle(center_x: Pixels, center_y: Pixels, radius: Pixels, color: Hsla, window: &mut Window) {
    let mut builder = PathBuilder::fill();

    // Start at the rightmost point of the circle
    builder.move_to(point(center_x + radius, center_y));

    // Draw the circle using two arc_to calls (top half, then bottom half)
    builder.arc_to(point(radius, radius), px(0.), false, true, point(center_x - radius, center_y));
    builder.arc_to(point(radius, radius), px(0.), false, true, point(center_x + radius, center_y));
    builder.close();

    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Paints a horizontal dashed line between two x positions at a given y, used
/// to connect a branch/tag ref label in the gutter to its commit node.
fn paint_dashed_connector(from_x: Pixels, to_x: Pixels, y: Pixels, color: Hsla, window: &mut Window) {
    const DASH: f32 = 4.0;
    const GAP: f32 = 3.0;
    const THICKNESS: f32 = 1.0;

    let (start, end) = if from_x <= to_x { (from_x, to_x) } else { (to_x, from_x) };

    let mut x = start;
    while x < end {
        let segment_end = (x + px(DASH)).min(end);
        let dash_bounds = Bounds::new(
            point(x, y - px(THICKNESS / 2.0)),
            gpui::Size { width: segment_end - x, height: px(THICKNESS) },
        );
        window.paint_quad(gpui::fill(dash_bounds, color));
        x = segment_end + px(GAP);
    }
}

/// Draws a commit node: a background-colored "halo" ring that masks any
/// connector lines passing through the lane, an outer ring in the branch
/// color, and a lighter center so the node reads as a clear, distinct point.
fn draw_commit_circle(center_x: Pixels, center_y: Pixels, color: Hsla, background: Hsla, window: &mut Window) {
    // Halo: clears space around the node so crossing lines don't touch it.
    fill_circle(center_x, center_y, COMMIT_CIRCLE_RADIUS + COMMIT_CIRCLE_HALO_WIDTH, background, window);

    // Outer ring in the branch color.
    fill_circle(center_x, center_y, COMMIT_CIRCLE_RADIUS, color, window);

    // Inner fill makes the node read as a ring rather than a solid dot.
    fill_circle(
        center_x,
        center_y,
        (COMMIT_CIRCLE_RADIUS - COMMIT_CIRCLE_STROKE_WIDTH).max(px(0.)),
        background,
        window,
    );
}

struct GitGraphContextMenu {
    menu: Entity<ContextMenu>,
    position: Point<Pixels>,
    target_entry_index: Option<usize>,
    _subscription: Subscription,
}

#[derive(Clone)]
struct RefMenuEntry {
    ref_name: Option<SharedString>,
}

impl RefMenuEntry {
    fn add_entries(self, mut menu: ContextMenu) -> ContextMenu {
        if let Some(ref_name) = self.ref_name {
            menu = menu.entry("Copy Ref Name", None, move |_window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(ref_name.to_string()));
            });
        }
        menu
    }
}

pub struct GitGraph {
    focus_handle: FocusHandle,
    search_state: SearchState,
    graph_data: GraphData,
    git_store: Entity<GitStore>,
    workspace: WeakEntity<Workspace>,
    context_menu: Option<GitGraphContextMenu>,
    table_interaction_state: Entity<TableInteractionState>,
    column_widths: Entity<RedistributableColumnsState>,
    /// Per-column visibility mask owned by the view (not the resize state) so columns can be
    /// hidden regardless of whether the table is resizable. `true` means the column is hidden.
    column_visibility: TableRow<bool>,
    selected_entry_idx: Option<usize>,
    hovered_entry_idx: Option<usize>,
    graph_canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    log_source: LogSource,
    log_order: LogOrder,
    commit_details: Entity<CommitDetailsPanel>,
    repo_id: RepositoryId,
    pending_select_sha: Option<Oid>,
    nav_history: Option<ItemNavHistory>,
}

impl GitGraph {
    fn invalidate_state(&mut self, cx: &mut Context<Self>) {
        self.graph_data.clear();
        self.search_state.matches.clear();
        self.search_state.selected_index = None;
        self.search_state.state.next_state();
        self.context_menu = None;
        cx.emit(ItemEvent::Edit);
        cx.notify();
    }

    /// Computes the height of a single commit row in the git graph.
    ///
    /// The returned value is snapped to the nearest physical pixel. This is
    /// required so that the canvas's float math and the `uniform_list` layout
    /// (which snaps to device pixels) agree on row positions; otherwise rows
    /// drift apart as the user scrolls when `ui_font_size` is fractional.
    fn row_height(window: &Window, _cx: &App) -> Pixels {
        let rem_size = window.rem_size();
        let line_height = window.text_style().line_height_in_pixels(rem_size);
        let raw = line_height + ROW_VERTICAL_PADDING;
        let scale = window.scale_factor();

        (raw * scale).round() / scale
    }

    fn visible_row_count(&self, window: &Window, cx: &App) -> usize {
        let row_height = Self::row_height(window, cx);
        let viewport_height = self
            .table_interaction_state
            .read(cx)
            .scroll_handle
            .0
            .borrow()
            .last_item_size
            .map_or(window.viewport_size().height, |size| size.item.height);

        ((viewport_height / row_height).ceil() as usize).min(self.graph_data.commits.len())
    }

    fn graph_canvas_content_width(&self) -> Pixels {
        (LANE_WIDTH * self.graph_data.max_lanes.max(6) as f32) + LEFT_PADDING * 2.0
    }

    fn ref_label_gutter_width(&self, window: &Window, cx: &App) -> Pixels {
        let Some((widest_label, _)) = self.graph_data.widest_ref_label.as_ref() else {
            return px(0.);
        };

        let text_style = window.text_style();
        let font_size = TextSize::Small.rems(cx).to_pixels(window.rem_size());
        let label_width = window
            .text_system()
            .layout_line(
                widest_label,
                font_size,
                &[TextRun {
                    len: widest_label.len(),
                    font: text_style.font(),
                    color: text_style.color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width;

        label_width.clamp(REF_LABEL_GUTTER_MIN_WIDTH, REF_LABEL_GUTTER_MAX_WIDTH)
    }

    /// Returns the column fractions in display order:
    /// `[graph, description, date, author, commit]`.
    /// For path history there is no graph column, so its fraction is 0.
    fn preview_column_fractions(&self, window: &Window, cx: &App) -> [f32; 5] {
        let raw = self.column_widths.read(cx).preview_fractions(window.rem_size());
        let fractions = redistribute_hidden_fractions(&raw, Some(&self.column_visibility));

        // Hidden columns occupy no space in the layout, so report them as zero here even though
        // the shared redistribution helper preserves their stored width for when they return.
        let value = |idx: usize| {
            if self.column_visibility.get(idx).copied().unwrap_or(false) { 0.0 } else { fractions[idx] }
        };

        let is_path_history = matches!(self.log_source, LogSource::Path(_));
        let graph_fraction = if is_path_history { 0.0 } else { value(0) };
        let offset = if is_path_history { 0 } else { 1 };

        [graph_fraction, value(offset), value(offset + 1), value(offset + 2), value(offset + 3)]
    }

    fn table_column_width_config(&self, window: &Window, cx: &App) -> ColumnWidthConfig {
        let [_, description, date, author, commit] = self.preview_column_fractions(window, cx);
        let table_total = description + date + author + commit;

        let widths = if table_total > 0.0 {
            vec![
                DefiniteLength::Fraction(description / table_total),
                DefiniteLength::Fraction(date / table_total),
                DefiniteLength::Fraction(author / table_total),
                DefiniteLength::Fraction(commit / table_total),
            ]
        } else {
            vec![DefiniteLength::Fraction(0.25); 4]
        };

        ColumnWidthConfig::explicit(widths)
    }

    pub fn new(
        repo_id: RepositoryId,
        git_store: Entity<GitStore>,
        workspace: WeakEntity<Workspace>,
        languages: Arc<LanguageRegistry>,
        log_source: Option<LogSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        cx.on_focus(&focus_handle, window, |_, _, cx| cx.notify()).detach();

        let accent_colors = cx.theme().accents();
        let graph = GraphData::new(accent_colors_count(accent_colors));
        let log_source = log_source.unwrap_or_default();
        let log_order = LogOrder::default();

        cx.subscribe(&git_store, |this, _, event, cx| {
            if let GitStoreEvent::RepositoryUpdated(updated_repo_id, repo_event, _) = event
                && this.repo_id == *updated_repo_id
                && let Some(repository) = this.get_repository(cx)
            {
                this.on_repository_event(repository, repo_event, cx);
            }
        })
        .detach();

        let search_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Search commits…", window, cx);
            editor
        });

        let table_interaction_state = cx.new(|cx| {
            let mut state = TableInteractionState::new(cx);
            state.focus_handle = state.focus_handle.tab_index(1).tab_stop(true);
            state
        });

        let column_widths = if matches!(log_source, LogSource::Path(_)) {
            cx.new(|_cx| {
                RedistributableColumnsState::new(
                    4,
                    vec![
                        DefiniteLength::Fraction(0.64),
                        DefiniteLength::Fraction(0.14),
                        DefiniteLength::Fraction(0.12),
                        DefiniteLength::Fraction(0.10),
                    ],
                    vec![TableResizeBehavior::Resizable; 4],
                )
            })
        } else {
            cx.new(|_cx| {
                RedistributableColumnsState::new(
                    5,
                    vec![
                        DefiniteLength::Fraction(0.30),
                        DefiniteLength::Fraction(0.37),
                        DefiniteLength::Fraction(0.13),
                        DefiniteLength::Fraction(0.11),
                        DefiniteLength::Fraction(0.09),
                    ],
                    vec![TableResizeBehavior::Resizable; 5],
                )
            })
        };
        let column_visibility = TableRow::from_element(
            false,
            if matches!(log_source, LogSource::Path(_)) { TABLE_COLUMN_COUNT } else { TABLE_COLUMN_COUNT + 1 },
        );
        let mut row_height = Self::row_height(window, cx);

        cx.observe_global_in::<settings::SettingsStore>(window, move |this, window, cx| {
            let new_row_height = Self::row_height(window, cx);
            if new_row_height != row_height {
                // The `uniform_list` powering the table caches the item size
                // from its last layout; invalidate it so it re-measures with
                // the new row height on the next frame.
                this.table_interaction_state.update(cx, |state, _cx| {
                    state.scroll_handle.0.borrow_mut().last_item_size = None;
                });
                row_height = new_row_height;
                cx.notify();
            }
        })
        .detach();

        let commit_details = cx.new(|_| CommitDetailsPanel::new(Some(languages)));
        cx.subscribe_in(&commit_details, window, Self::handle_commit_details_event).detach();

        let mut this = GitGraph {
            focus_handle,
            git_store,
            search_state: SearchState {
                case_sensitive: false,
                editor: search_editor,
                matches: IndexSet::default(),
                selected_index: None,
                state: QueryState::Empty,
            },
            workspace,
            graph_data: graph,
            context_menu: None,
            table_interaction_state,
            column_widths,
            column_visibility,
            selected_entry_idx: None,
            hovered_entry_idx: None,
            graph_canvas_bounds: Rc::new(Cell::new(None)),
            log_source,
            log_order,
            commit_details,
            repo_id,
            pending_select_sha: None,
            nav_history: None,
        };

        this.fetch_initial_graph_data(cx);
        this
    }

    fn on_repository_event(&mut self, repository: Entity<Repository>, event: &RepositoryEvent, cx: &mut Context<Self>) {
        match event {
            RepositoryEvent::GraphEvent((source, order), event)
                if source == &self.log_source && order == &self.log_order =>
            {
                match event {
                    GitGraphEvent::FullyLoaded => {
                        if let Some(pending_sha_index) = self.pending_select_sha.take().and_then(|oid| {
                            repository
                                .read(cx)
                                .get_graph_data(source.clone(), *order)
                                .and_then(|data| data.commit_oid_to_index.get(&oid).copied())
                        }) {
                            self.select_entry(pending_sha_index, ScrollStrategy::Nearest, cx);
                        }
                        self.select_newest_if_unselected(cx);
                        let count = match self.graph_data.max_commit_count {
                            AllCommitCount::FullyLoaded(count) | AllCommitCount::Loading(count) => count,
                            AllCommitCount::NotLoaded => 0,
                        };
                        self.graph_data.max_commit_count = AllCommitCount::FullyLoaded(count);
                        cx.notify();
                    }
                    GitGraphEvent::LoadingError => {
                        cx.notify();
                    }
                    GitGraphEvent::CountUpdated(commit_count) => {
                        let old_count = self.graph_data.commits.len();

                        if let Some(pending_selection_index) = repository.update(cx, |repository, cx| {
                            let GraphDataResponse { commits, is_loading, error: _ } =
                                repository.graph_data(source.clone(), *order, old_count..*commit_count, cx);
                            self.graph_data.add_commits(commits);

                            let pending_sha_index = self.pending_select_sha.and_then(|oid| {
                                repository
                                    .get_graph_data(source.clone(), *order)
                                    .and_then(|data| data.commit_oid_to_index.get(&oid).copied())
                            });

                            if !is_loading && pending_sha_index.is_none() {
                                self.pending_select_sha.take();
                            }

                            pending_sha_index
                        }) {
                            self.select_entry(pending_selection_index, ScrollStrategy::Nearest, cx);
                            self.pending_select_sha.take();
                        }
                        self.select_newest_if_unselected(cx);

                        cx.notify();
                    }
                }
            }
            RepositoryEvent::TagsChanged => {
                self.pending_select_sha = None;
                self.invalidate_state(cx);
            }
            RepositoryEvent::HeadChanged | RepositoryEvent::BranchListChanged => {
                // Only invalidate if we scanned atleast once,
                // meaning we are not inside the initial repo loading state
                // NOTE: this fixes an loading performance regression
                if repository.read(cx).scan_id > 1 {
                    self.pending_select_sha = None;
                    self.invalidate_state(cx);
                }
            }
            RepositoryEvent::StashEntriesChanged if self.log_source == LogSource::All => {
                // Stash entries initial's scan id is 2, so we don't want to invalidate the graph before that
                if repository.read(cx).scan_id > 2 {
                    self.pending_select_sha = None;
                    self.invalidate_state(cx);
                }
            }
            RepositoryEvent::GraphEvent(_, _) => {}
            _ => {}
        }
    }

    fn fetch_initial_graph_data(&mut self, cx: &mut App) {
        if let Some(repository) = self.get_repository(cx) {
            repository.update(cx, |repository, cx| {
                let commits = repository.graph_data(self.log_source.clone(), self.log_order, 0..usize::MAX, cx).commits;
                self.graph_data.add_commits(commits);
            });
        }
    }

    fn get_repository(&self, cx: &App) -> Option<Entity<Repository>> {
        let git_store = self.git_store.read(cx);
        git_store.repositories().get(&self.repo_id).cloned()
    }

    /// Checks whether a ref name from git's `%D` decoration
    ///  format refers to the currently checked-out branch.
    pub(crate) fn is_head_ref(ref_name: &str, head_branch_name: &Option<SharedString>) -> bool {
        head_branch_name
            .as_ref()
            .is_some_and(|head| ref_name == head.as_ref() || ref_name.strip_prefix("HEAD -> ") == Some(head.as_ref()))
    }

    /// Extracts a ref name (branch, remote ref, or tag) from a decoration in
    /// git's `%D` format, returning `None` for a detached `HEAD`.
    fn ref_name_from_decoration(decoration: &str) -> Option<SharedString> {
        let name =
            decoration.strip_prefix("tag: ").or_else(|| decoration.strip_prefix("HEAD -> ")).unwrap_or(decoration);
        if name.is_empty() || name == "HEAD" {
            return None;
        }
        Some(SharedString::from(name.to_string()))
    }

    /// Renders a ref chip for the commit at `commit_idx`. Chips get a
    /// right-click handler that opens a ref-specific context menu, so that
    /// custom commands can be resolved against the clicked ref.
    fn render_ref_chip(
        &self,
        name: SharedString,
        accent_color: gpui::Hsla,
        is_head: bool,
        commit_idx: usize,
        background: gpui::Hsla,
        cx: &Context<Self>,
    ) -> AnyElement {
        let chip_id = SharedString::from(format!("git-graph-ref-chip-{commit_idx}-{name}"));
        let this = cx.weak_entity();
        let menu_name = name.clone();
        render_ref_chip(name, chip_id, accent_color, is_head, background, move |position, window, cx| {
            this.update(cx, |this, cx| {
                this.deploy_ref_context_menu(position, commit_idx, menu_name.clone(), window, cx);
            })
            .ok();
        })
    }

    fn render_ref_count_chip(
        &self,
        hidden_refs: Vec<SharedString>,
        accent_color: gpui::Hsla,
        commit_idx: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let hidden_refs_text =
            hidden_refs.iter().map(|ref_name| format!("- {ref_name}")).collect::<Vec<_>>().join("\n");
        let chip_id = SharedString::from(format!("git-graph-ref-count-chip-{commit_idx}"));

        div()
            .id(chip_id)
            .flex_none()
            .cursor_pointer()
            .child(
                Chip::new(format!("+{}", hidden_refs.len()))
                    .label_size(LabelSize::Small)
                    .label_color(Color::Muted)
                    .border_color(accent_color.opacity(0.25)),
            )
            .tooltip(move |_window, cx| Tooltip::simple(hidden_refs_text.clone(), cx))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();

                    this.deploy_hidden_refs_context_menu(event.position, commit_idx, hidden_refs.clone(), window, cx);
                }),
            )
            .into_any_element()
    }

    /// A horizontal dashed line that grows to fill the space between a ref badge
    /// and the right edge of the gutter, visually connecting the badge to its
    /// commit node (the canvas paints the rest of the connector up to the node).
    fn render_dashed_connector(color: Hsla) -> AnyElement {
        gpui::canvas(
            move |_bounds, _window, _cx| {},
            move |bounds: Bounds<Pixels>, _: (), window: &mut Window, _cx: &mut App| {
                let y = bounds.origin.y + bounds.size.height / 2.0;
                paint_dashed_connector(bounds.origin.x, bounds.origin.x + bounds.size.width, y, color, window);
            },
        )
        .flex_1()
        .h_full()
        .into_any_element()
    }

    fn render_table_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Vec<AnyElement>> {
        let repository = self.get_repository(cx);

        let row_height = Self::row_height(window, cx);

        // We fetch data outside the visible viewport to avoid loading entries when
        // users scroll through the git graph
        if let Some(repository) = repository.as_ref() {
            const FETCH_RANGE: usize = 100;
            repository.update(cx, |repository, cx| {
                self.graph_data.commits[range.start.saturating_sub(FETCH_RANGE)
                    ..(range.end + FETCH_RANGE).min(self.graph_data.commits.len().saturating_sub(1))]
                    .iter()
                    .for_each(|commit| {
                        repository.fetch_commit_data(commit.data.sha, false, cx);
                    });
            });
        }

        range
            .map(|idx| {
                let Some((commit, repository)) = self.graph_data.commits.get(idx).zip(repository.as_ref()) else {
                    return (0..4).map(|_| div().h(row_height).into_any_element()).collect();
                };

                let data = repository
                    .update(cx, |repository, cx| repository.fetch_commit_data(commit.data.sha, false, cx).clone());

                let short_sha = commit.data.sha.display_short();
                let mut formatted_time = String::new();
                let subject: SharedString;
                let author_name: Option<SharedString>;

                if let CommitDataState::Loaded(ref data) = data {
                    subject = data.subject.clone();
                    author_name = (!data.author_name.is_empty()).then(|| data.author_name.clone());
                    formatted_time = format_timestamp(data.commit_timestamp);
                } else {
                    subject = "Loading…".into();
                    author_name = None;
                }

                let is_selected = self.selected_entry_idx == Some(idx);
                let is_matched = self.search_state.matches.contains(&commit.data.sha);
                let column_label = |label: SharedString| {
                    Label::new(label).when(!is_selected, |c| c.color(Color::Muted)).truncate().into_any_element()
                };

                let query = is_matched.then(|| self.search_state.confirmed_query()).flatten();
                let subject_label = if let Some(query) = query {
                    let highlight_ranges = search_match_ranges(&subject, query, self.search_state.case_sensitive);
                    HighlightedLabel::from_ranges(subject, highlight_ranges)
                        .when(!is_selected, |c| c.color(Color::Muted))
                        .truncate()
                        .into_any_element()
                } else {
                    column_label(subject)
                };
                let sha_label = if let Some(query) = query {
                    let highlight_ranges = commit_hash_match_range(&short_sha, query).into_iter().collect();
                    HighlightedLabel::from_ranges(short_sha, highlight_ranges)
                        .when(!is_selected, |c| c.color(Color::Muted))
                        .truncate()
                        .into_any_element()
                } else {
                    column_label(short_sha.into())
                };

                vec![
                    div()
                        .id(ElementId::NamedInteger("commit-subject".into(), idx as u64))
                        .overflow_hidden()
                        .child(h_flex().gap_2().overflow_hidden().child(subject_label))
                        .into_any_element(),
                    column_label(formatted_time.into()),
                    h_flex()
                        .id(ElementId::NamedInteger("commit-author".into(), idx as u64))
                        .h(row_height)
                        .items_center()
                        .overflow_hidden()
                        .when_some(author_name, |this, author_name| this.child(column_label(author_name)))
                        .into_any_element(),
                    sha_label,
                ]
            })
            .collect()
    }

    /// Selects the newest commit once commits arrive, so the details sidebar is
    /// always shown. A pending selection (e.g. "Show in Git Graph") wins.
    fn select_newest_if_unselected(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = newest_commit_to_select(
            self.selected_entry_idx,
            self.pending_select_sha.is_some(),
            self.graph_data.commits.len(),
        ) {
            self.select_entry(idx, ScrollStrategy::Top, cx);
        }
    }

    fn handle_commit_details_event(
        &mut self,
        _: &Entity<CommitDetailsPanel>,
        event: &CommitDetailsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CommitDetailsEvent::OpenFile(repo_path) => {
                let (Some(sha), Some(repository)) = (self.commit_details.read(cx).sha(), self.get_repository(cx))
                else {
                    return;
                };
                CommitView::open_at_file(
                    sha.to_string(),
                    repository.downgrade(),
                    self.workspace.clone(),
                    repo_path.clone(),
                    self.commit_details.read(cx).diff(),
                    window,
                    cx,
                );
            }
            CommitDetailsEvent::OpenRevision(repo_path) => {
                let (Some(sha), Some(repository)) = (self.commit_details.read(cx).sha(), self.get_repository(cx))
                else {
                    return;
                };
                CommitView::open_revision(
                    sha.to_string(),
                    repository.downgrade(),
                    self.workspace.clone(),
                    repo_path.clone(),
                    self.commit_details.read(cx).diff(),
                    window,
                    cx,
                );
            }
            CommitDetailsEvent::OpenProjectFile(repo_path) => {
                if let Some(repository) = self.get_repository(cx) {
                    open_project_file(&self.workspace, &repository, repo_path, window, cx);
                }
            }
            CommitDetailsEvent::DeployRefMenu { position, name } => {
                if let Some(selected_idx) = self.selected_entry_idx {
                    self.deploy_ref_context_menu(*position, selected_idx, name.clone(), window, cx);
                }
            }
        }
    }

    fn select_first(&mut self, _: &SelectFirst, _window: &mut Window, cx: &mut Context<Self>) {
        self.select_entry(0, ScrollStrategy::Nearest, cx);
    }

    fn select_prev(&mut self, _: &SelectPrevious, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected_entry_idx) = &self.selected_entry_idx {
            self.select_entry(selected_entry_idx.saturating_sub(1), ScrollStrategy::Nearest, cx);
        } else {
            self.select_first(&SelectFirst, window, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected_entry_idx) = &self.selected_entry_idx {
            self.select_entry(
                selected_entry_idx.saturating_add(1).min(self.graph_data.commits.len().saturating_sub(1)),
                ScrollStrategy::Nearest,
                cx,
            );
        } else {
            self.select_prev(&SelectPrevious, window, cx);
        }
    }

    fn select_last(&mut self, _: &SelectLast, _window: &mut Window, cx: &mut Context<Self>) {
        self.select_entry(self.graph_data.commits.len().saturating_sub(1), ScrollStrategy::Nearest, cx);
    }

    fn scroll_up(&mut self, _: &ScrollUp, window: &mut Window, cx: &mut Context<Self>) {
        let step = (self.visible_row_count(window, cx) / 2).max(1);
        let target_idx = self.selected_entry_idx.unwrap_or(0).saturating_sub(step);

        self.select_entry(target_idx, ScrollStrategy::Nearest, cx);
    }

    fn scroll_down(&mut self, _: &ScrollDown, window: &mut Window, cx: &mut Context<Self>) {
        let Some(last_entry_idx) = self.graph_data.commits.len().checked_sub(1) else {
            return;
        };

        let step = (self.visible_row_count(window, cx) / 2).max(1);
        let target_idx = self.selected_entry_idx.unwrap_or(0).saturating_add(step).min(last_entry_idx);

        self.select_entry(target_idx, ScrollStrategy::Nearest, cx);
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selected_commit_view(window, cx);
    }

    fn toggle_changed_files_view(&mut self, _: &ToggleChangedFilesView, _window: &mut Window, cx: &mut Context<Self>) {
        self.commit_details.update(cx, |details, cx| details.toggle_view_mode(cx));
        cx.notify();
    }

    fn search(&mut self, query: SharedString, cx: &mut Context<Self>) {
        let Some(repo) = self.get_repository(cx) else {
            return;
        };

        self.search_state.matches.clear();
        self.search_state.selected_index = None;
        self.search_state.editor.update(cx, |editor, _cx| {
            editor.set_text_style_refinement(TextStyleRefinement::default());
        });

        if query.as_str().is_empty() {
            self.search_state.state = QueryState::Empty;
            self.update_commit_message_search_highlights(cx);
            cx.notify();
            return;
        }

        let (request_tx, request_rx) = async_channel::unbounded::<Oid>();

        repo.update(cx, |repo, cx| {
            repo.search_commits(
                self.log_source.clone(),
                SearchCommitArgs { query: query.clone(), case_sensitive: self.search_state.case_sensitive },
                request_tx,
                cx,
            );
        });

        let search_task = cx.spawn(async move |this, cx| {
            while let Ok(first_oid) = request_rx.recv().await {
                let mut pending_oids = vec![first_oid];
                while let Ok(oid) = request_rx.try_recv() {
                    pending_oids.push(oid);
                }

                this.update(cx, |this, cx| {
                    if this.search_state.selected_index.is_none() {
                        this.search_state.selected_index = Some(0);
                        this.select_commit_by_sha(first_oid, cx);
                    }

                    this.search_state.matches.extend(pending_oids);
                    cx.notify();
                })
                .ok();
            }

            this.update(cx, |this, cx| {
                if this.search_state.matches.is_empty() {
                    this.search_state.editor.update(cx, |editor, cx| {
                        editor.set_text_style_refinement(TextStyleRefinement {
                            color: Some(Color::Error.color(cx)),
                            ..Default::default()
                        });
                    });
                }
            })
            .ok();
        });

        self.search_state.state = QueryState::Confirmed((query, search_task));
        self.update_commit_message_search_highlights(cx);
        cx.emit(ItemEvent::Edit);
    }

    fn confirm_search(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search_state.editor.read(cx).text(cx).into();
        self.search(query, cx);
    }

    fn activate_search_editor_if_focused(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_state.editor.update(cx, |editor, cx| {
            if editor.is_focused(window) {
                editor.select_all(&editor::actions::SelectAll, window, cx);
                editor.show_cursor(cx);
            }
        });
    }

    fn focus_next_tab_stop(&mut self, _: &FocusNextTabStop, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
        self.activate_search_editor_if_focused(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn focus_previous_tab_stop(&mut self, _: &FocusPreviousTabStop, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
        self.activate_search_editor_if_focused(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn select_entry(&mut self, idx: usize, scroll_strategy: ScrollStrategy, cx: &mut Context<Self>) {
        if self.selected_entry_idx == Some(idx) || idx >= self.graph_data.commits.len() {
            debug_assert!(
                idx < self.graph_data.commits.len(),
                "attempted to select out of bounds index: {idx}, commits.len: {}",
                self.graph_data.commits.len()
            );
            return;
        }

        if self.selected_entry_idx.replace(idx).is_none() {
            cx.emit(ItemEvent::UpdateTab);
        }
        self.table_interaction_state.update(cx, |state, cx| {
            state.scroll_handle.scroll_to_item(idx, scroll_strategy);
            cx.notify();
        });

        let (Some(commit), Some(repository)) = (self.graph_data.commits.get(idx), self.get_repository(cx)) else {
            return;
        };
        let sha = commit.data.sha;
        let ref_names = commit.data.ref_names.clone();
        let accent_colors = cx.theme().accents();
        let ref_color = accent_colors
            .0
            .get(commit.color_idx)
            .copied()
            .unwrap_or_else(|| accent_colors.0.first().copied().unwrap_or_default());
        let history_path = history_reveal_path(&self.log_source);
        self.commit_details.update(cx, |details, cx| {
            details.set_commit(&repository, sha, ref_names, ref_color, cx);
            details.load_diff(cx);
            if let Some(path) = history_path {
                details.reveal_file(&path, cx);
            }
        });

        cx.emit(ItemEvent::Edit);
        cx.notify();
    }

    fn update_commit_message_search_highlights(&self, cx: &mut Context<Self>) {
        let query = self.search_state.confirmed_query().map(str::to_string);
        let case_sensitive = self.search_state.case_sensitive;
        self.commit_details.update(cx, |details, cx| {
            details.set_search_highlight(query.as_deref(), case_sensitive, cx);
        });
    }

    fn select_previous_match(&mut self, cx: &mut Context<Self>) {
        if self.search_state.matches.is_empty() {
            return;
        }

        let mut prev_selection = self.search_state.selected_index.unwrap_or_default();

        if prev_selection == 0 {
            prev_selection = self.search_state.matches.len() - 1;
        } else {
            prev_selection -= 1;
        }

        let Some(&oid) = self.search_state.matches.get_index(prev_selection) else {
            return;
        };

        self.search_state.selected_index = Some(prev_selection);
        self.select_commit_by_sha(oid, cx);
    }

    fn select_next_match(&mut self, cx: &mut Context<Self>) {
        if self.search_state.matches.is_empty() {
            return;
        }

        let mut next_selection = self.search_state.selected_index.map(|index| index + 1).unwrap_or_default();

        if next_selection >= self.search_state.matches.len() {
            next_selection = 0;
        }

        let Some(&oid) = self.search_state.matches.get_index(next_selection) else {
            return;
        };

        self.search_state.selected_index = Some(next_selection);
        self.select_commit_by_sha(oid, cx);
    }

    pub fn set_repo_id(&mut self, repo_id: RepositoryId, cx: &mut Context<Self>) {
        if repo_id != self.repo_id && self.git_store.read(cx).repositories().contains_key(&repo_id) {
            self.repo_id = repo_id;
            self.invalidate_state(cx);
        }
    }

    pub fn select_commit_by_sha(&mut self, sha: impl TryInto<Oid>, cx: &mut Context<Self>) {
        fn inner(this: &mut GitGraph, oid: Oid, cx: &mut Context<GitGraph>) {
            let Some(selected_repository) = this.get_repository(cx) else {
                return;
            };

            let Some(index) = selected_repository
                .read(cx)
                .get_graph_data(this.log_source.clone(), this.log_order)
                .and_then(|data| data.commit_oid_to_index.get(&oid))
                .copied()
            else {
                this.pending_select_sha = Some(oid);
                return;
            };

            this.pending_select_sha = None;
            this.select_entry(index, ScrollStrategy::Center, cx);
        }

        if let Ok(oid) = sha.try_into() {
            inner(self, oid, cx);
        }
    }

    fn open_selected_commit_view(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_entry_index) = self.selected_entry_idx else {
            return;
        };

        self.open_commit_view(selected_entry_index, window, cx);
    }

    fn open_commit_view(&self, entry_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit_entry) = self.graph_data.commits.get(entry_index) else {
            return;
        };

        let Some(repository) = self.get_repository(cx) else {
            return;
        };

        CommitView::open(
            commit_entry.data.sha.to_string(),
            repository.downgrade(),
            self.workspace.clone(),
            None,
            None,
            window,
            cx,
        );
    }

    fn copy_commit_sha(&self, entry_index: usize, cx: &Context<Self>) {
        let Some(commit) = self.graph_data.commits.get(entry_index) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(commit.data.sha.to_string()));
    }

    fn copy_selected_commit_sha(&mut self, _: &CopyCommitSha, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_entry_index) = self.selected_entry_idx else {
            return;
        };
        self.copy_commit_sha(selected_entry_index, cx);
    }

    fn copy_commit_tag(&self, entry_index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(commit) = self.graph_data.commits.get(entry_index) else {
            return;
        };

        let tag_names = commit
            .data
            .tag_names()
            .into_iter()
            .map(|tag_name| SharedString::from(tag_name.to_string()))
            .collect::<Vec<_>>();

        match tag_names.as_slice() {
            [] => {}
            [tag_name] => cx.write_to_clipboard(ClipboardItem::new_string(tag_name.to_string())),
            _ => {
                self.workspace
                    .update(cx, |workspace, cx| {
                        workspace.toggle_modal(window, cx, |window, cx| CommitTagPicker::new(tag_names, window, cx));
                    })
                    .ok();
            }
        }
    }

    fn copy_selected_commit_tag(&mut self, _: &CopyCommitTag, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_entry_index) = self.selected_entry_idx else {
            return;
        };
        self.copy_commit_tag(selected_entry_index, window, cx);
    }

    fn deploy_ref_context_menu(
        &mut self,
        position: Point<Pixels>,
        index: usize,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.graph_data.commits.get(index).is_none() {
            return;
        }

        let ref_entry = Self::ref_menu_entry(&name);
        let context_menu = ContextMenu::build(window, cx, move |menu, _, _| {
            menu.header(format!("Ref {name}")).map(|menu| ref_entry.add_entries(menu))
        });

        self.set_context_menu(context_menu, position, Some(index), window, cx);
    }

    /// Builds a context menu listing the refs that aren't shown inline in the
    /// gutter. Each ref is a submenu exposing that ref's actions, so users can
    /// still act on refs hidden behind the "+N" badge.
    fn deploy_hidden_refs_context_menu(
        &mut self,
        position: Point<Pixels>,
        index: usize,
        hidden_refs: Vec<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.graph_data.commits.get(index).is_none() {
            return;
        }

        let ref_entries = hidden_refs.into_iter().map(|name| (Self::ref_menu_entry(&name), name)).collect::<Vec<_>>();

        let context_menu = ContextMenu::build(window, cx, move |mut menu, _window, _| {
            menu = menu.header("Refs");
            for (ref_entry, name) in ref_entries {
                menu = menu.submenu(name, move |submenu, _window, _cx| ref_entry.clone().add_entries(submenu));
            }
            menu
        });

        self.set_context_menu(context_menu, position, Some(index), window, cx);
    }

    /// Resolves the data needed to populate a single ref's context-menu entries.
    /// Kept separate so the same entries can be rendered both as a standalone
    /// menu and as a submenu.
    fn ref_menu_entry(name: &SharedString) -> RefMenuEntry {
        RefMenuEntry { ref_name: Self::ref_name_from_decoration(name) }
    }

    fn deploy_entry_context_menu(
        &mut self,
        position: Point<Pixels>,
        index: usize,
        ref_name: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(commit) = self.graph_data.commits.get(index) else {
            return;
        };
        let repository = self.get_repository(cx).map(|repository| repository.downgrade());
        let context_menu = commit_context_menu(
            CommitContextMenuData {
                sha: commit.data.sha,
                tag_names: commit
                    .data
                    .tag_names()
                    .into_iter()
                    .map(|tag_name| SharedString::from(tag_name.to_string()))
                    .collect(),
            },
            ref_name,
            self.focus_handle.clone(),
            repository,
            self.workspace.clone(),
            window,
            cx,
        );
        self.set_context_menu(context_menu, position, Some(index), window, cx);
    }

    fn set_context_menu(
        &mut self,
        context_menu: Entity<ContextMenu>,
        position: Point<Pixels>,
        target_entry_index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&context_menu.focus_handle(cx), cx);

        let subscription = cx.subscribe_in(&context_menu, window, |this, _, _: &DismissEvent, window, cx| {
            if this
                .context_menu
                .as_ref()
                .is_some_and(|context_menu| context_menu.menu.focus_handle(cx).contains_focused(window, cx))
            {
                cx.focus_self(window);
            }
            this.context_menu.take();
            cx.notify();
        });
        self.context_menu =
            Some(GitGraphContextMenu { menu: context_menu, position, target_entry_index, _subscription: subscription });
        cx.notify();
    }

    fn toggle_column_visibility(&mut self, col_idx: usize, cx: &mut Context<Self>) {
        if let Some(slot) = self.column_visibility.as_mut_slice().get_mut(col_idx) {
            *slot = !*slot;
            // Column visibility is persisted per item, so schedule a workspace serialization.
            cx.emit(ItemEvent::Edit);
        }
    }

    fn deploy_header_context_menu(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let is_path_history = matches!(self.log_source, LogSource::Path(_));
        let columns: &[&str] = if is_path_history {
            &["Description", "Date", "Author", "Commit"]
        } else {
            &["Graph", "Description", "Date", "Author", "Commit"]
        };

        let filter = self.column_visibility.clone();
        let visible_count = filter.as_slice().iter().filter(|filtered| !**filtered).count();

        let focus_handle = self.focus_handle.clone();
        let git_graph = cx.entity();
        let context_menu = ContextMenu::build(window, cx, |mut context_menu, _window, _cx| {
            context_menu = context_menu.context(focus_handle).header("Columns");
            for (col_idx, label) in columns.iter().enumerate() {
                let is_visible = !filter.get(col_idx).copied().unwrap_or(false);
                // Disable hiding the last remaining visible column.
                let can_toggle = !is_visible || visible_count > 1;
                let git_graph = git_graph.clone();
                context_menu = context_menu.toggleable_entry_disabled_when(
                    label.to_string(),
                    is_visible,
                    !can_toggle,
                    IconPosition::End,
                    None,
                    move |_window, cx| {
                        git_graph.update(cx, |this, cx| {
                            this.toggle_column_visibility(col_idx, cx);
                            cx.notify();
                        });
                    },
                );
            }
            context_menu
        });

        self.set_context_menu(context_menu, position, None, window, cx);
    }

    fn render_search_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let color = cx.theme().colors();
        let query_focus_handle = self.search_state.editor.focus_handle(cx).tab_index(1).tab_stop(true);

        h_flex()
            .key_context("GitGraphSearchBar")
            .tab_index(1)
            .tab_group()
            .tab_stop(false)
            .w_full()
            .p_1p5()
            .gap_1p5()
            .child(
                h_flex()
                    .h_8()
                    .flex_1()
                    .min_w_0()
                    .px_1p5()
                    .gap_1()
                    .track_focus(&query_focus_handle)
                    .border_1()
                    .border_color(color.border_variant)
                    .rounded_md()
                    .bg(color.toolbar_background)
                    .on_action(cx.listener(Self::confirm_search))
                    .child(self.search_state.editor.clone())
                    .child({
                        let focus_handle = query_focus_handle.clone();
                        IconButton::new("git-graph-search-case-sensitive", IconName::CaseSensitive)
                            .shape(ui::IconButtonShape::Square)
                            .toggle_state(self.search_state.case_sensitive)
                            .on_click({
                                let focus_handle = query_focus_handle.clone();
                                move |_, window, cx| {
                                    if !focus_handle.is_focused(window) {
                                        window.focus(&focus_handle, cx);
                                    }
                                    window.dispatch_action(ToggleCaseSensitive.boxed_clone(), cx);
                                }
                            })
                            .tooltip(move |_window, cx| {
                                Tooltip::for_action_in(
                                    "Match Case Sensitivity",
                                    &ToggleCaseSensitive,
                                    &focus_handle,
                                    cx,
                                )
                            })
                    }),
            )
            .child(
                h_flex()
                    .min_w_64()
                    .gap_1()
                    .child({
                        let focus_handle = self.focus_handle.clone();
                        IconButton::new("git-graph-search-prev", IconName::ChevronLeft)
                            .shape(ui::IconButtonShape::Square)
                            .icon_size(IconSize::Small)
                            .tooltip(move |_, cx| {
                                Tooltip::for_action_in("Select Previous Match", &SelectPreviousMatch, &focus_handle, cx)
                            })
                            .map(|this| {
                                if self.search_state.matches.is_empty() {
                                    this.disabled(true)
                                } else {
                                    this.disabled(false).on_click(cx.listener(|this, _, _, cx| {
                                        this.select_previous_match(cx);
                                    }))
                                }
                            })
                    })
                    .child({
                        let focus_handle = self.focus_handle.clone();
                        IconButton::new("git-graph-search-next", IconName::ChevronRight)
                            .shape(ui::IconButtonShape::Square)
                            .icon_size(IconSize::Small)
                            .tooltip(move |_, cx| {
                                Tooltip::for_action_in("Select Next Match", &SelectNextMatch, &focus_handle, cx)
                            })
                            .map(|this| {
                                if self.search_state.matches.is_empty() {
                                    this.disabled(true)
                                } else {
                                    this.disabled(false).on_click(cx.listener(|this, _, _, cx| {
                                        this.select_next_match(cx);
                                    }))
                                }
                            })
                    })
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Label::new(format!(
                                    "{}/{}",
                                    self.search_state.selected_index.map(|index| index + 1).unwrap_or(0),
                                    self.search_state.matches.len()
                                ))
                                .size(LabelSize::Small)
                                .when(self.search_state.matches.is_empty(), |this| this.color(Color::Disabled)),
                            )
                            .when(
                                matches!(
                                    &self.search_state.state,
                                    QueryState::Confirmed((_, task)) if !task.is_ready()
                                ),
                                |this| {
                                    this.child(
                                        Icon::new(IconName::ArrowCircle)
                                            .color(Color::Accent)
                                            .size(IconSize::Small)
                                            .with_rotate_animation(2)
                                            .into_any_element(),
                                    )
                                },
                            ),
                    ),
            )
    }

    fn render_loading_spinner(&self, cx: &App) -> AnyElement {
        let rems = TextSize::Large.rems(cx);
        Icon::new(IconName::LoadCircle)
            .size(IconSize::Custom(rems))
            .color(Color::Accent)
            .with_rotate_animation(3)
            .into_any_element()
    }

    fn render_graph_canvas(&self, window: &Window, cx: &Context<GitGraph>) -> impl IntoElement {
        let row_height = Self::row_height(window, cx);
        let visible_row_count = self.visible_row_count(window, cx);
        let table_state = self.table_interaction_state.read(cx);
        let viewport_height = table_state
            .scroll_handle
            .0
            .borrow()
            .last_item_size
            .map(|size| size.item.height)
            .unwrap_or(window.viewport_size().height);
        let loaded_commit_count = self.graph_data.commits.len();

        let content_height = row_height * loaded_commit_count;
        let max_scroll = (content_height - viewport_height).max(px(0.));
        let scroll_offset_y = (-table_state.scroll_offset().y).clamp(px(0.), max_scroll);

        let first_visible_row = (scroll_offset_y / row_height).floor() as usize;
        let vertical_scroll_offset = scroll_offset_y - (first_visible_row as f32 * row_height);

        let graph_content_width = self.graph_canvas_content_width();
        let last_visible_row = first_visible_row + visible_row_count + 1;

        let viewport_range =
            first_visible_row.min(loaded_commit_count.saturating_sub(1))..(last_visible_row).min(loaded_commit_count);
        let rows = self.graph_data.commits[viewport_range.clone()].to_vec();
        let commit_lines: Vec<_> = self
            .graph_data
            .lines
            .iter()
            .filter(|line| {
                line.full_interval.start <= viewport_range.end && line.full_interval.end >= viewport_range.start
            })
            .cloned()
            .collect();

        let mut lines: BTreeMap<usize, Vec<_>> = BTreeMap::new();

        let hovered_entry_idx = self.hovered_entry_idx;
        let selected_entry_idx = self.selected_entry_idx;
        let context_menu_target_index = self.context_menu.as_ref().and_then(|menu| menu.target_entry_index);
        let is_focused = self.focus_handle.is_focused(window);
        let graph_canvas_bounds = self.graph_canvas_bounds.clone();

        gpui::canvas(
            move |_bounds, _window, _cx| {},
            move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
                graph_canvas_bounds.set(Some(bounds));

                window.paint_layer(bounds, |window| {
                    let theme = cx.theme();
                    let accent_colors = theme.accents();
                    let background = theme.colors().editor_background;

                    let hover_bg = theme.colors().element_hover.opacity(0.6);
                    let selected_bg =
                        if is_focused { theme.colors().element_selected } else { theme.colors().element_hover };

                    for visible_row_idx in 0..rows.len() {
                        let absolute_row_idx = first_visible_row + visible_row_idx;
                        let is_hovered = hovered_entry_idx == Some(absolute_row_idx);
                        let is_selected = selected_entry_idx == Some(absolute_row_idx);
                        let is_context_menu_target = context_menu_target_index == Some(absolute_row_idx);

                        if is_hovered || is_selected || is_context_menu_target {
                            let row_y = bounds.origin.y + visible_row_idx as f32 * row_height - vertical_scroll_offset;
                            let row_bounds = Bounds::new(
                                point(bounds.origin.x, row_y),
                                gpui::Size { width: bounds.size.width, height: row_height },
                            );

                            let bg_color = if is_selected || is_context_menu_target { selected_bg } else { hover_bg };
                            window.paint_quad(gpui::fill(row_bounds, bg_color));
                        }
                    }

                    for line in commit_lines {
                        let Some((start_segment_idx, start_column)) =
                            line.get_first_visible_segment_idx(first_visible_row)
                        else {
                            continue;
                        };

                        let line_x = lane_center_x(bounds, start_column as f32);

                        let start_row =
                            (line.full_interval.start as u32).cast_signed() - (first_visible_row as u32).cast_signed();

                        let from_y = bounds.origin.y + start_row as f32 * row_height + row_height / 2.0
                            - vertical_scroll_offset
                            + COMMIT_CIRCLE_RADIUS;

                        let mut current_row = from_y;
                        let mut current_column = line_x;

                        let mut builder = PathBuilder::stroke(LINE_WIDTH);
                        builder.move_to(point(line_x, from_y));

                        let segments = &line.segments[start_segment_idx..];
                        let desired_curve_height = row_height / 3.0;
                        let desired_curve_width = LANE_WIDTH / 3.0;

                        for (segment_idx, segment) in segments.iter().enumerate() {
                            let is_last = segment_idx + 1 == segments.len();

                            match segment {
                                CommitLineSegment::Straight { to_row } => {
                                    let mut dest_row = to_row_center(
                                        to_row - first_visible_row,
                                        row_height,
                                        vertical_scroll_offset,
                                        bounds,
                                    );
                                    if is_last {
                                        dest_row -= COMMIT_CIRCLE_RADIUS;
                                    }

                                    let dest_point = point(current_column, dest_row);

                                    current_row = dest_point.y;
                                    builder.line_to(dest_point);
                                    builder.move_to(dest_point);
                                }
                                CommitLineSegment::Curve { to_column, on_row, curve_kind } => {
                                    let mut to_column = lane_center_x(bounds, *to_column as f32);

                                    let mut to_row = to_row_center(
                                        *on_row - first_visible_row,
                                        row_height,
                                        vertical_scroll_offset,
                                        bounds,
                                    );

                                    // This means that this branch was a checkout
                                    let going_right = to_column > current_column;
                                    let column_shift = if going_right {
                                        COMMIT_CIRCLE_RADIUS + COMMIT_CIRCLE_STROKE_WIDTH
                                    } else {
                                        -COMMIT_CIRCLE_RADIUS - COMMIT_CIRCLE_STROKE_WIDTH
                                    };

                                    match curve_kind {
                                        CurveKind::Checkout => {
                                            if is_last {
                                                to_column -= column_shift;
                                            }

                                            let available_curve_width = (to_column - current_column).abs();
                                            let available_curve_height = (to_row - current_row).abs();
                                            let curve_width = desired_curve_width.min(available_curve_width);
                                            let curve_height = desired_curve_height.min(available_curve_height);
                                            let signed_curve_width =
                                                if going_right { curve_width } else { -curve_width };
                                            let curve_start = point(current_column, to_row - curve_height);
                                            let curve_end = point(current_column + signed_curve_width, to_row);
                                            let curve_control = point(current_column, to_row);

                                            builder.move_to(point(current_column, current_row));
                                            builder.line_to(curve_start);
                                            builder.move_to(curve_start);
                                            builder.curve_to(curve_end, curve_control);
                                            builder.move_to(curve_end);
                                            builder.line_to(point(to_column, to_row));
                                        }
                                        CurveKind::Merge => {
                                            if is_last {
                                                to_row -= COMMIT_CIRCLE_RADIUS;
                                            }

                                            let merge_start = point(
                                                current_column + column_shift,
                                                current_row - COMMIT_CIRCLE_RADIUS,
                                            );
                                            let available_curve_width = (to_column - merge_start.x).abs();
                                            let available_curve_height = (to_row - merge_start.y).abs();
                                            let curve_width = desired_curve_width.min(available_curve_width);
                                            let curve_height = desired_curve_height.min(available_curve_height);
                                            let signed_curve_width =
                                                if going_right { curve_width } else { -curve_width };
                                            let curve_start = point(to_column - signed_curve_width, merge_start.y);
                                            let curve_end = point(to_column, merge_start.y + curve_height);
                                            let curve_control = point(to_column, merge_start.y);

                                            builder.move_to(merge_start);
                                            builder.line_to(curve_start);
                                            builder.move_to(curve_start);
                                            builder.curve_to(curve_end, curve_control);
                                            builder.move_to(curve_end);
                                            builder.line_to(point(to_column, to_row));
                                        }
                                    }
                                    current_row = to_row;
                                    current_column = to_column;
                                    builder.move_to(point(current_column, current_row));
                                }
                            }
                        }

                        builder.close();
                        lines.entry(line.color_idx).or_default().push(builder);
                    }

                    for (color_idx, builders) in lines {
                        let line_color = accent_colors.color_for_index(color_idx as u32);

                        for builder in builders {
                            if let Ok(path) = builder.build() {
                                // we paint each color on it's own layer to stop overlapping lines
                                // of different colors changing the color of a line
                                window.paint_layer(bounds, |window| {
                                    window.paint_path(path, line_color);
                                });
                            }
                        }
                    }

                    // Commit nodes are painted last so they sit on top of the
                    // connector lines, keeping each commit a clear, distinct point.
                    for (row_idx, row) in rows.into_iter().enumerate() {
                        let absolute_row_idx = first_visible_row + row_idx;
                        let row_color = accent_colors.color_for_index(row.color_idx as u32);
                        let row_y_center =
                            bounds.origin.y + row_idx as f32 * row_height + row_height / 2.0 - vertical_scroll_offset;

                        let commit_x = lane_center_x(bounds, row.lane as f32);

                        if !row.data.ref_names.is_empty() {
                            paint_dashed_connector(
                                bounds.origin.x,
                                commit_x - COMMIT_CIRCLE_RADIUS,
                                row_y_center,
                                row_color.opacity(0.5),
                                window,
                            );
                        }

                        // Match the node halo to the row's effective background so
                        // the ring blends seamlessly on hovered/selected rows.
                        let node_background = if selected_entry_idx == Some(absolute_row_idx)
                            || context_menu_target_index == Some(absolute_row_idx)
                        {
                            background.blend(selected_bg)
                        } else if hovered_entry_idx == Some(absolute_row_idx) {
                            background.blend(hover_bg)
                        } else {
                            background
                        };

                        draw_commit_circle(commit_x, row_y_center, row_color, node_background, window);
                    }
                })
            },
        )
        .w_full()
        .min_w(graph_content_width)
        .h_full()
    }

    /// Renders the branch/tag ref labels as an overlay anchored to the left of
    /// the graph column, positioned at the row of the commit each ref points to
    /// (i.e. just before that commit's node). This mirrors the scroll math used
    /// by `render_graph_canvas` so the labels stay in sync as the graph scrolls.
    fn render_graph_ref_labels(&self, window: &Window, cx: &Context<GitGraph>) -> impl IntoElement {
        let row_height = Self::row_height(window, cx);
        let visible_row_count = self.visible_row_count(window, cx);
        let table_state = self.table_interaction_state.read(cx);
        let viewport_height = table_state
            .scroll_handle
            .0
            .borrow()
            .last_item_size
            .map(|size| size.item.height)
            .unwrap_or(window.viewport_size().height);
        let loaded_commit_count = self.graph_data.commits.len();

        let content_height = row_height * loaded_commit_count;
        let max_scroll = (content_height - viewport_height).max(px(0.));
        let scroll_offset_y = (-table_state.scroll_offset().y).clamp(px(0.), max_scroll);

        let first_visible_row = (scroll_offset_y / row_height).floor() as usize;
        let vertical_scroll_offset = scroll_offset_y - (first_visible_row as f32 * row_height);
        let last_visible_row = first_visible_row + visible_row_count + 1;
        let viewport_range =
            first_visible_row.min(loaded_commit_count.saturating_sub(1))..last_visible_row.min(loaded_commit_count);

        let head_branch_name: Option<SharedString> = self
            .get_repository(cx)
            .and_then(|repo| repo.read(cx).branch.as_ref().map(|branch| SharedString::from(branch.name().to_string())));

        let theme = cx.theme();
        let accent_colors = theme.accents().clone();
        let background = theme.colors().editor_background;

        // Hover/selected highlight colors, matching `render_graph_canvas` so the
        // band painted behind the labels in the gutter lines up with the rest of
        // the row.
        let hovered_entry_idx = self.hovered_entry_idx;
        let selected_entry_idx = self.selected_entry_idx;
        let context_menu_entry_idx = self.context_menu.as_ref().and_then(|menu| menu.target_entry_index);
        let is_focused = self.focus_handle.is_focused(window);
        let hover_bg = theme.colors().element_hover.opacity(0.6);
        let selected_bg = if is_focused { theme.colors().element_selected } else { theme.colors().element_hover };

        // Background highlight bands for hovered/selected rows, painted first so
        // the labels render on top of them.
        let mut elements = Vec::with_capacity(viewport_range.len());
        for absolute_idx in viewport_range.clone() {
            let is_selected = selected_entry_idx == Some(absolute_idx) || context_menu_entry_idx == Some(absolute_idx);
            let is_hovered = hovered_entry_idx == Some(absolute_idx);
            if !is_selected && !is_hovered {
                continue;
            }

            let visible_row_idx = absolute_idx - first_visible_row;
            let top = visible_row_idx as f32 * row_height - vertical_scroll_offset;
            let bg_color = if is_selected { selected_bg } else { hover_bg };

            elements.push(div().absolute().top(top).left_0().w_full().h(row_height).bg(bg_color).into_any_element());
        }

        for absolute_idx in viewport_range {
            let Some(commit) = self.graph_data.commits.get(absolute_idx) else {
                continue;
            };
            if commit.data.ref_names.is_empty() {
                continue;
            }

            let visible_row_idx = absolute_idx - first_visible_row;
            let top = visible_row_idx as f32 * row_height - vertical_scroll_offset;
            let accent_color = accent_colors.color_for_index(commit.color_idx as u32);
            let ref_names = commit.data.ref_names.clone();

            // When a commit carries multiple refs we only render a single badge
            // (preferring the checked-out branch, falling back to the first ref)
            // plus a "+N" counter, so the gutter stays legible instead of
            // cramming several truncated chips together.
            let (primary_name, is_head) =
                match ref_names.iter().find(|name| Self::is_head_ref(name.as_ref(), &head_branch_name)) {
                    Some(head_ref) => (head_ref.clone(), true),
                    None => match ref_names.first() {
                        Some(first) => (first.clone(), false),
                        None => continue,
                    },
                };

            let primary_chip =
                self.render_ref_chip(primary_name.clone(), accent_color, is_head, absolute_idx, background, cx);

            let hidden_refs = ref_names.iter().filter(|name| *name != &primary_name).cloned().collect::<Vec<_>>();
            let count_chip = (!hidden_refs.is_empty())
                .then(|| self.render_ref_count_chip(hidden_refs, accent_color, absolute_idx, cx));

            elements.push(
                div()
                    .absolute()
                    .top(top)
                    .left_0()
                    .w_full()
                    .h(row_height)
                    .child(
                        h_flex()
                            .h_full()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .justify_start()
                            .gap_1()
                            .pl_1p5()
                            .child(primary_chip)
                            .children(count_chip)
                            .child(Self::render_dashed_connector(accent_color.opacity(0.5))),
                    )
                    .into_any_element(),
            );
        }

        div().absolute().inset_0().overflow_hidden().children(elements)
    }

    fn row_at_position(&self, position_y: Pixels, window: &Window, cx: &Context<Self>) -> Option<usize> {
        let canvas_bounds = self.graph_canvas_bounds.get()?;
        let table_state = self.table_interaction_state.read(cx);
        let scroll_offset_y = -table_state.scroll_offset().y;

        let local_y = position_y - canvas_bounds.origin.y;

        if local_y >= px(0.) && local_y < canvas_bounds.size.height {
            let absolute_y = local_y + scroll_offset_y;
            let row_height = Self::row_height(window, cx);
            let absolute_row = (absolute_y / row_height).floor() as usize;

            if absolute_row < self.graph_data.commits.len() {
                return Some(absolute_row);
            }
        }

        None
    }

    fn handle_graph_mouse_move(&mut self, event: &gpui::MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.row_at_position(event.position.y, window, cx) {
            if self.hovered_entry_idx != Some(row) {
                self.hovered_entry_idx = Some(row);
                cx.notify();
            }
        } else if self.hovered_entry_idx.is_some() {
            self.hovered_entry_idx = None;
            cx.notify();
        }
    }

    fn handle_entry_click(
        &mut self,
        entry_idx: usize,
        event: &ClickEvent,
        scroll_strategy: ScrollStrategy,
        focus_handle: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Right-clicks open the context menu, not the details panel.
        if event.is_right_click() {
            return;
        }

        if let Some(focus_handle) = focus_handle {
            focus_handle.focus(window, cx);
        }

        self.select_entry(entry_idx, scroll_strategy, cx);

        if event.click_count() >= 2 {
            self.open_commit_view(entry_idx, window, cx);
        }
    }

    fn handle_graph_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.row_at_position(event.position().y, window, cx) {
            self.handle_entry_click(row, event, ScrollStrategy::Nearest, None, window, cx);
        }
    }

    fn handle_entry_secondary_mouse_down(
        &mut self,
        entry_idx: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.deploy_entry_context_menu(event.position, entry_idx, None, window, cx);
        cx.stop_propagation();
    }

    fn handle_graph_secondary_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.row_at_position(event.position.y, window, cx) else {
            return;
        };

        self.handle_entry_secondary_mouse_down(row, event, window, cx);
    }

    fn handle_graph_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let line_height = window.line_height();
        let delta = event.delta.pixel_delta(line_height);

        let table_state = self.table_interaction_state.read(cx);
        let current_offset = table_state.scroll_offset();

        let viewport_height = table_state.scroll_handle.viewport().size.height;

        let commit_count = match self.graph_data.max_commit_count {
            AllCommitCount::Loading(count) => count,
            AllCommitCount::FullyLoaded(count) => count,
            AllCommitCount::NotLoaded => self.graph_data.commits.len(),
        };
        let content_height = Self::row_height(window, cx) * commit_count;
        let max_vertical_scroll = (viewport_height - content_height).min(px(0.));

        let new_y = (current_offset.y + delta.y).clamp(max_vertical_scroll, px(0.));
        let new_offset = Point::new(current_offset.x, new_y);

        if new_offset != current_offset {
            table_state.set_scroll_offset(new_offset);
            cx.notify();
        }
    }

    fn commit_count_and_loading_state(&mut self, cx: &mut Context<Self>) -> (usize, bool) {
        match self.graph_data.max_commit_count {
            AllCommitCount::FullyLoaded(count) => (count, false),
            AllCommitCount::Loading(count) => {
                let is_loading = self
                    .get_repository(cx)
                    .map(|repository| {
                        repository.update(cx, |repository, cx| {
                            repository.graph_data(self.log_source.clone(), self.log_order, 0..0, cx).is_loading
                        })
                    })
                    .unwrap_or(false);

                (count, is_loading)
            }
            AllCommitCount::NotLoaded => {
                let (commit_count, is_loading) = if let Some(repository) = self.get_repository(cx) {
                    repository.update(cx, |repository, cx| {
                        // Start loading the graph data if we haven't started already
                        let GraphDataResponse { commits, is_loading, error: _ } =
                            repository.graph_data(self.log_source.clone(), self.log_order, 0..usize::MAX, cx);
                        self.graph_data.add_commits(commits);
                        (commits.len(), is_loading)
                    })
                } else {
                    (0, false)
                };

                (commit_count, is_loading)
            }
        }
    }
}

impl Render for GitGraph {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // This happens when we changed branches, we should refresh our search as well
        if let QueryState::Pending(query) = &mut self.search_state.state {
            let query = std::mem::take(query);
            self.search_state.state = QueryState::Empty;
            self.search(query, cx);
        }
        let (commit_count, is_loading) = self.commit_count_and_loading_state(cx);

        let error = self.get_repository(cx).and_then(|repo| {
            repo.read(cx).get_graph_data(self.log_source.clone(), self.log_order).and_then(|data| data.error.clone())
        });

        let content = if commit_count == 0 {
            let message = if let Some(error) = &error {
                format!("Error loading: {}", error)
            } else if is_loading {
                "Loading".to_string()
            } else {
                "No commits found".to_string()
            };
            let label = Label::new(message).color(Color::Muted).size(LabelSize::Large);

            h_flex()
                .size_full()
                .gap_1()
                .justify_center()
                .child(label)
                .when(is_loading && error.is_none(), |this| this.child(self.render_loading_spinner(cx)))
        } else {
            let is_path_history = matches!(self.log_source, LogSource::Path(_));
            let header_resize_info = HeaderResizeInfo::from_redistributable(&self.column_widths, cx);

            let column_filter = self.column_visibility.clone();

            // The graph column (index 0) only exists in the non-path-history layout and is
            // rendered as a separate canvas outside the table.
            let graph_visible = is_path_history || !column_filter.get(0usize).copied().unwrap_or(false);

            let table_offset = if is_path_history { 0 } else { 1 };
            let table_filter = column_filter
                .as_slice()
                .get(table_offset..table_offset + TABLE_COLUMN_COUNT)
                .map(|slice| TableRow::from_vec(slice.to_vec(), TABLE_COLUMN_COUNT))
                .unwrap_or_else(|| TableRow::from_element(false, TABLE_COLUMN_COUNT));
            let header_widths =
                redistribute_hidden_widths(&self.column_widths.read(cx).widths_to_render(), Some(&column_filter));
            let header_context = TableRenderContext::for_column_widths(Some(header_widths), true)
                .with_column_filter(Some(column_filter));

            let [graph_fraction, description_fraction, date_fraction, author_fraction, commit_fraction] =
                self.preview_column_fractions(window, cx);
            let table_fraction = description_fraction + date_fraction + author_fraction + commit_fraction;
            let table_width_config = self.table_column_width_config(window, cx);

            let table_collapsed = table_fraction <= f32::EPSILON;
            let graph_content_width = self.graph_canvas_content_width();
            let gutter_width = self.ref_label_gutter_width(window, cx);

            h_flex().size_full().child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.deploy_header_context_menu(event.position, window, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(render_table_header(
                                if !is_path_history {
                                    TableRow::from_vec(
                                        vec![
                                            Label::new("Graph").color(Color::Muted).truncate().into_any_element(),
                                            Label::new("Description").color(Color::Muted).into_any_element(),
                                            Label::new("Date").color(Color::Muted).into_any_element(),
                                            Label::new("Author").color(Color::Muted).into_any_element(),
                                            Label::new("Commit").color(Color::Muted).into_any_element(),
                                        ],
                                        5,
                                    )
                                } else {
                                    TableRow::from_vec(
                                        vec![
                                            Label::new("Description").color(Color::Muted).into_any_element(),
                                            Label::new("Date").color(Color::Muted).into_any_element(),
                                            Label::new("Author").color(Color::Muted).into_any_element(),
                                            Label::new("Commit").color(Color::Muted).into_any_element(),
                                        ],
                                        4,
                                    )
                                },
                                header_context,
                                Some(header_resize_info),
                                Some(self.column_widths.entity_id()),
                                cx,
                            )),
                    )
                    .child({
                        let row_height = Self::row_height(window, cx);
                        let selected_entry_idx = self.selected_entry_idx;
                        let hovered_entry_idx = self.hovered_entry_idx;
                        let context_menu_target_index =
                            self.context_menu.as_ref().and_then(|menu| menu.target_entry_index);
                        let weak_self = cx.weak_entity();
                        let focus_handle = self.focus_handle.clone();
                        let table_focus_handle = self.table_interaction_state.read(cx).focus_handle.clone();

                        let graph_canvas = div()
                            .id("graph-canvas")
                            .size_full()
                            .overflow_hidden()
                            .child(div().size_full().child(self.render_graph_canvas(window, cx)));

                        let commits_table = Table::new(4)
                            .interactable(&self.table_interaction_state)
                            .hide_row_borders()
                            .hide_row_hover()
                            .width_config(table_width_config)
                            .column_filter(table_filter)
                            .map_row(move |(index, row), window, cx| {
                                let is_selected = selected_entry_idx == Some(index);
                                let is_hovered = hovered_entry_idx == Some(index);
                                let is_context_menu_target = context_menu_target_index == Some(index);
                                let table_focus_handle = table_focus_handle.clone();
                                let is_focused =
                                    focus_handle.is_focused(window) || table_focus_handle.is_focused(window);
                                let weak = weak_self.clone();
                                let weak_for_hover = weak.clone();
                                let weak_for_context_menu = weak.clone();

                                let hover_bg = cx.theme().colors().element_hover.opacity(0.6);
                                let selected_bg = if is_focused {
                                    cx.theme().colors().element_selected
                                } else {
                                    cx.theme().colors().element_hover
                                };

                                row.h(row_height)
                                    .cursor_pointer()
                                    .when(is_selected || is_context_menu_target, |row| row.bg(selected_bg))
                                    .when(is_hovered && !is_selected && !is_context_menu_target, |row| row.bg(hover_bg))
                                    .on_hover(move |&is_hovered, _, cx| {
                                        weak_for_hover
                                            .update(cx, |this, cx| {
                                                if is_hovered {
                                                    if this.hovered_entry_idx != Some(index) {
                                                        this.hovered_entry_idx = Some(index);
                                                        cx.notify();
                                                    }
                                                } else if this.hovered_entry_idx == Some(index) {
                                                    this.hovered_entry_idx = None;
                                                    cx.notify();
                                                }
                                            })
                                            .ok();
                                    })
                                    .on_click(move |event, window, cx| {
                                        weak.update(cx, |this, cx| {
                                            this.handle_entry_click(
                                                index,
                                                event,
                                                ScrollStrategy::Center,
                                                Some(&table_focus_handle),
                                                window,
                                                cx,
                                            );
                                        })
                                        .ok();
                                    })
                                    .on_mouse_down(MouseButton::Right, move |event: &MouseDownEvent, window, cx| {
                                        weak_for_context_menu
                                            .update(cx, |this, cx| {
                                                this.handle_entry_secondary_mouse_down(index, event, window, cx);
                                            })
                                            .ok();
                                    })
                                    .into_any_element()
                            })
                            .uniform_list("git-graph-commits", commit_count, cx.processor(Self::render_table_rows));

                        bind_redistributable_columns(
                            div()
                                .relative()
                                .flex_1()
                                .w_full()
                                .overflow_hidden()
                                .child(
                                    h_flex()
                                        .size_full()
                                        .when(!is_path_history && graph_visible, |this| {
                                            this.child(
                                                h_flex()
                                                    .id("graph-column")
                                                    .map(|this| {
                                                        if table_collapsed {
                                                            this.w(graph_content_width)
                                                        } else {
                                                            this.w(DefiniteLength::Fraction(graph_fraction))
                                                        }
                                                    })
                                                    .h_full()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .cursor_pointer()
                                                    .on_scroll_wheel(cx.listener(Self::handle_graph_scroll))
                                                    .on_mouse_move(cx.listener(Self::handle_graph_mouse_move))
                                                    .on_click(cx.listener(Self::handle_graph_click))
                                                    .on_mouse_down(
                                                        MouseButton::Right,
                                                        cx.listener(Self::handle_graph_secondary_mouse_down),
                                                    )
                                                    .on_hover(cx.listener(|this, &is_hovered: &bool, _, cx| {
                                                        if !is_hovered && this.hovered_entry_idx.is_some() {
                                                            this.hovered_entry_idx = None;
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .when(gutter_width > px(0.), |this| {
                                                        this.child(
                                                            div()
                                                                .relative()
                                                                .h_full()
                                                                .w(gutter_width)
                                                                .max_w(relative(0.5))
                                                                .flex_shrink_0()
                                                                .overflow_hidden()
                                                                .child(self.render_graph_ref_labels(window, cx)),
                                                        )
                                                    })
                                                    .child(
                                                        div()
                                                            .relative()
                                                            .flex_1()
                                                            .h_full()
                                                            .min_w_0()
                                                            .overflow_hidden()
                                                            .child(graph_canvas),
                                                    ),
                                            )
                                        })
                                        .child(
                                            div()
                                                .tab_index(2)
                                                .tab_group()
                                                .tab_stop(false)
                                                .map(|this| {
                                                    if table_collapsed {
                                                        this.flex_1()
                                                    } else {
                                                        this.w(DefiniteLength::Fraction(table_fraction))
                                                    }
                                                })
                                                .h_full()
                                                .min_w_0()
                                                .child(commits_table),
                                        ),
                                )
                                .child(render_redistributable_columns_resize_handles(
                                    &self.column_widths,
                                    Some(&self.column_visibility),
                                    window,
                                    cx,
                                )),
                            self.column_widths.clone(),
                            Some(self.column_visibility.clone()),
                        )
                    }),
            )
        };

        div()
            .key_context("GitGraph")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .on_action(cx.listener(|this, _: &OpenCommitView, window, cx| {
                this.open_selected_commit_view(window, cx);
            }))
            .on_action(cx.listener(Self::copy_selected_commit_sha))
            .on_action(cx.listener(Self::copy_selected_commit_tag))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.search_state.editor.update(cx, |editor, cx| editor.focus_handle(cx).focus(window, cx));
                this.activate_search_editor_if_focused(window, cx);
            }))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::scroll_up))
            .on_action(cx.listener(Self::scroll_down))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::toggle_changed_files_view))
            .on_action(cx.listener(Self::focus_next_tab_stop))
            .on_action(cx.listener(Self::focus_previous_tab_stop))
            .on_action(cx.listener(|this, _: &SelectNextMatch, _window, cx| {
                this.select_next_match(cx);
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousMatch, _window, cx| {
                this.select_previous_match(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleCaseSensitive, _window, cx| {
                this.search_state.case_sensitive = !this.search_state.case_sensitive;
                this.search_state.state.next_state();
                cx.emit(ItemEvent::Edit);
                cx.notify();
            }))
            .child(v_flex().size_full().child(self.render_search_bar(cx)).child(div().flex_1().child(content)))
            .children(self.context_menu.as_ref().map(|context_menu| {
                deferred(
                    anchored().position(context_menu.position).anchor(Anchor::TopLeft).child(context_menu.menu.clone()),
                )
                .with_priority(1)
            }))
            .on_action(cx.listener(|_, _: &buffer_search::Deploy, window, cx| {
                window.dispatch_action(Box::new(FocusSearch), cx);
                cx.stop_propagation();
            }))
    }
}

impl EventEmitter<ItemEvent> for GitGraph {}

impl Focusable for GitGraph {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_state.editor.read(cx).focus_handle(cx)
    }
}

impl Item for GitGraph {
    type Event = ItemEvent;

    fn side_card(&self, _: &App) -> Option<AnyView> {
        self.selected_entry_idx.is_some().then(|| self.commit_details.clone().into())
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::GitGraph))
    }

    fn tab_tooltip_content(&self, cx: &App) -> Option<TabTooltipContent> {
        let repo_name = self.get_repository(cx).and_then(|repo| {
            repo.read(cx).work_directory_abs_path.file_name().map(|name| name.to_string_lossy().to_string())
        });
        let path_history_path = match &self.log_source {
            LogSource::Path(path) => Some(path.as_unix_str().to_string()),
            _ => None,
        };

        Some(TabTooltipContent::Custom(Box::new(Tooltip::element({
            move |_, _| {
                v_flex()
                    .child(Label::new(if path_history_path.is_some() { "Path History" } else { "Git Graph" }))
                    .when_some(path_history_path.clone(), |this, path| {
                        this.child(Label::new(path).color(Color::Muted).size(LabelSize::Small))
                    })
                    .when_some(repo_name.clone(), |this, name| {
                        this.child(Label::new(name).color(Color::Muted).size(LabelSize::Small))
                    })
                    .into_any_element()
            }
        }))))
    }

    fn tab_content_text(&self, _detail: usize, cx: &App) -> SharedString {
        if let LogSource::Path(path) = &self.log_source {
            return path
                .as_ref()
                .file_name()
                .map(|name| SharedString::from(name.to_string()))
                .unwrap_or_else(|| SharedString::from(path.as_unix_str().to_string()));
        }

        self.get_repository(cx)
            .and_then(|repo| {
                repo.read(cx).work_directory_abs_path.file_name().map(|name| name.to_string_lossy().to_string())
            })
            .map_or_else(|| "Git Graph".into(), SharedString::from)
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }

    fn deactivated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(nav_history) = self.nav_history.as_mut() {
            nav_history.push::<()>(None, None, cx);
        }
    }

    fn set_nav_history(&mut self, nav_history: ItemNavHistory, _window: &mut Window, _cx: &mut Context<Self>) {
        self.nav_history = Some(nav_history);
    }
}

/// The commit to select when nothing is selected yet: the newest one, unless a
/// specific commit is still being loaded for selection.
fn newest_commit_to_select(selected: Option<usize>, has_pending: bool, commit_count: usize) -> Option<usize> {
    (selected.is_none() && !has_pending && commit_count > 0).then_some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_reveal_path() {
        let path = RepoPath::new("crates/ui/src/ui_input.rs").unwrap();
        assert_eq!(history_reveal_path(&LogSource::Path(path.clone())), Some(path));
        assert_eq!(history_reveal_path(&LogSource::All), None);
        assert_eq!(history_reveal_path(&LogSource::Branch("main".into())), None);
    }

    #[test]
    fn test_newest_commit_to_select() {
        assert_eq!(newest_commit_to_select(None, false, 3), Some(0));
        assert_eq!(newest_commit_to_select(Some(2), false, 3), None);
        assert_eq!(newest_commit_to_select(None, true, 3), None);
        assert_eq!(newest_commit_to_select(None, false, 0), None);
    }

    #[test]
    fn test_search_match_ranges() {
        assert_eq!(search_match_ranges("Fix fix FIX", "fix", true), vec![4..7]);
        assert_eq!(search_match_ranges("Fix fix FIX", "fix", false), vec![0..3, 4..7, 8..11]);
        assert_eq!(search_match_ranges("aaaa", "aa", false), vec![0..2, 2..4]);
        assert_eq!(search_match_ranges("anything", "", false), Vec::<Range<usize>>::new());
        assert_eq!(search_match_ranges("no match", "xyz", false), Vec::<Range<usize>>::new());
        // Lowercasing `İ` changes its byte length; ranges must stay in original offsets.
        assert_eq!(search_match_ranges("İx fix", "fix", false), vec![4..7]);
        assert_eq!(search_match_ranges("Ünïcode ÜNÏ", "ünï", false), vec![0..5, 10..15]);
    }

    #[test]
    fn test_case_insensitive_prefix_len() {
        assert_eq!(case_insensitive_prefix_len("FooBar", "foo"), Some(3));
        assert_eq!(case_insensitive_prefix_len("Foo", "foo"), Some(3));
        assert_eq!(case_insensitive_prefix_len("Fo", "foo"), None);
        assert_eq!(case_insensitive_prefix_len("Bar", "foo"), None);
        assert_eq!(case_insensitive_prefix_len("ÄBC", "äb"), Some(3));
    }

    #[test]
    fn test_is_head_ref() {
        let head = Some(SharedString::from("main"));
        assert!(GitGraph::is_head_ref("main", &head));
        assert!(GitGraph::is_head_ref("HEAD -> main", &head));
        assert!(!GitGraph::is_head_ref("origin/main", &head));
        assert!(!GitGraph::is_head_ref("main", &None));
    }

    #[test]
    fn test_commit_hash_match_range() {
        assert_eq!(commit_hash_match_range("abc1234", "ABC1234"), Some(0..7));
        assert_eq!(commit_hash_match_range("abc1234", "abc1234def"), Some(0..7));
        assert_eq!(commit_hash_match_range("abc1234", "abc1235"), None);
        assert_eq!(commit_hash_match_range("abc1234", "abc"), None);
        assert_eq!(commit_hash_match_range("abc1234", "fix bug"), None);
    }
}
