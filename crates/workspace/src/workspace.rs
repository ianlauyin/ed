use std::mem;
use std::sync::atomic::Ordering;

use ed_actions::mode::Editor;
use ed_actions::mode::Terminal;
use ui::font;
pub mod dock;
mod docks;
pub mod invalid_item_view;
pub mod item;
mod item_registry;
mod modal_layer;
mod mode;
mod navigation_history;
pub mod notifications;
mod open;
pub mod pane;
pub mod pane_group;
mod pane_management;
mod pane_motion;
pub mod path_list {
    pub use util::path_list::PathList;
    pub use util::path_list::SerializedPathList;
}
pub mod path_link;
mod persistence;
pub mod preview;
pub mod searchable;
mod serialization;

mod status_bar;
pub mod tasks;
mod theme_preview;
mod toast_layer;
mod toolbar;
mod window_header;
pub mod workspace_error;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use dock::Dock;
use dock::DockPosition;
pub use dock::Panel;
use dock::PanelButtons;
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use gpui::Action;
use gpui::AnyView;
use gpui::AnyWeakView;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::Bounds;
use gpui::Context;
use gpui::DEFAULT_WINDOW_SIZE;
use gpui::DisplayId;
use gpui::Entity;
use gpui::EntityId;
use gpui::EventEmitter;
use gpui::FocusHandle;
use gpui::Focusable;
use gpui::Global;
use gpui::KeyContext;
use gpui::Keystroke;
use gpui::ManagedView;
use gpui::PathPromptOptions;
use gpui::Point;
use gpui::PromptLevel;
use gpui::Render;
use gpui::Size;
use gpui::StyleRefinement;
use gpui::Subscription;
use gpui::Task;
use gpui::TaskExt as _;
use gpui::WeakEntity;
use gpui::WindowBounds;
use gpui::WindowHandle;
use gpui::WindowOptions;
use gpui::actions;
use gpui::point;
use gpui::size;
#[allow(clippy::wildcard_imports)]
pub use item::Item;
pub use item::ItemHandle;
pub use item::ProjectItem;
pub use item::SerializableItem;
pub use item::SerializableItemHandle;
pub use item::WeakItemHandle;
pub use item_registry::register_project_item;
pub use item_registry::register_serializable_item;
use language::LanguageRegistry;
#[allow(clippy::wildcard_imports)]
pub use modal_layer::*;
use notifications::DetachAndPromptErr as _;
use notifications::Notifications;
use notifications::dismiss_app_notification;
use notifications::simple_message_notification::MessageNotification;
pub use open::OpenOptions;
pub use open::OpenResult;
pub use open::WorkspaceMatching;
pub use open::find_existing_workspace;
pub use open::open_new;
pub use open::open_paths;
pub use open::prompt_for_open_path_and_open;
#[allow(clippy::wildcard_imports)]
pub use pane::*;
pub use pane_group::HANDLE_HITBOX_SIZE;
pub use pane_group::Member;
pub use pane_group::PaneAxis;
pub use pane_group::PaneGroup;
pub use pane_group::PaneRenderContext;
pub use pane_group::SplitDirection;
pub use pane_management::move_active_item;
pub use pane_management::move_item;
pub use path_list::PathList;
pub use path_list::SerializedPathList;
pub use persistence::ProjectLocation;
pub use persistence::ProjectsStore;
pub use persistence::RecentWorkspace;
pub use persistence::delete_recent_workspace_group;
pub use persistence::last_session_workspaces;
pub use persistence::model::DockData;
pub use persistence::model::DockStructure;
pub use persistence::model::SessionWorkspace;
pub use persistence::recent_project_workspaces;
use project::DirectoryLister;
use project::Project;
use project::ProjectPath;
use project::Worktree;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use serde::Deserialize;
pub use serialization::flush_windows_serialization;
pub use serialization::last_opened_workspace_location;
pub use serialization::restore_workspace;
use status_bar::StatusBar;
pub use status_bar::StatusItemView;
pub use status_bar::status_item_color;
use task::SpawnInTerminal;
use theme::ActiveTheme as _;
pub use toast_layer::ToastAction;
pub use toast_layer::ToastLayer;
pub use toast_layer::ToastView;
pub use toolbar::PaneSearchBarCallbacks;
pub use toolbar::Toolbar;
pub use toolbar::ToolbarItemEvent;
pub use toolbar::ToolbarItemLocation;
pub use toolbar::ToolbarItemView;
pub use ui;
use ui::Window;
use ui::prelude::Div;
use ui::prelude::FluentBuilder as _;
use ui::prelude::InteractiveElement as _;
use ui::prelude::IntoElement;
use ui::prelude::ParentElement as _;
use ui::prelude::Pixels;
use ui::prelude::SharedString;
use ui::prelude::Styled as _;
use ui::prelude::div;
use ui::prelude::h_flex;
use ui::prelude::px;
use util::ResultExt as _;
use uuid::Uuid;

use crate::docks::adjust_active_dock_size_by_px;
use crate::docks::adjust_open_docks_size_by_px;
use crate::docks::px_with_ui_font_fallback;
use crate::item::ItemBufferKind;
use crate::mode::DockMotion;
use crate::mode::GIT_HISTORY_SIDEBAR_WIDTH;
use crate::mode::ModeView;
use crate::mode::TerminalLayout;
pub use crate::mode::TerminalModeDelegate;
use crate::mode::is_dragged_terminal;
pub use crate::mode::set_terminal_mode_delegate;
use crate::notifications::NotificationId;
use crate::open::prompt_and_open_paths;
use crate::serialization::flush_windows_serialization_on_quit;
use crate::serialization::open_items;

pub const SERIALIZATION_THROTTLE_TIME: Duration = Duration::from_millis(200);
pub const MAX_RECENT_SELECTIONS: usize = 20;

/// Space between the floating pane cards, and between them and the surrounding chrome.
pub use ui::utils::CARD_GAP;
/// Corner radius of the floating pane cards.
pub const CARD_RADIUS: Pixels = px(10.);
/// GPUI clips children to rectangles only, so the card's content is inset just enough
/// (inset >= radius * (1 - 1/sqrt 2)) for square child corners to stay inside the rounded edge.
pub const CARD_INSET: Pixels = px(3.);

const WINDOW_TITLE_SEPARATOR: &str = " — ";
/// The macOS window corner radius, which the window's coloured border follows.
const WINDOW_CORNER_RADIUS: Pixels = px(16.);

/// The window title: the project name, followed by the active file's name when there is one.
fn window_title(project_name: &str, file_name: Option<&str>) -> String {
    match file_name {
        Some(file_name) if !file_name.is_empty() => format!("{project_name}{WINDOW_TITLE_SEPARATOR}{file_name}"),
        _ => project_name.to_owned(),
    }
}

static ED_WINDOW_SIZE: LazyLock<Option<Size<Pixels>>> =
    LazyLock::new(|| env::var("ED_WINDOW_SIZE").ok().as_deref().and_then(parse_pixel_size_env_var));

static ED_WINDOW_POSITION: LazyLock<Option<Point<Pixels>>> =
    LazyLock::new(|| env::var("ED_WINDOW_POSITION").ok().as_deref().and_then(parse_pixel_position_env_var));

pub trait TerminalProvider {
    fn spawn(&self, task: SpawnInTerminal, window: &mut Window, cx: &mut App) -> Task<Option<Result<ExitStatus>>>;
}

/// Opens a file or directory.
#[derive(Clone, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = workspace)]
pub struct Open {
    /// When true, opens in a new window. When false, replaces the current
    /// window's workspace. When omitted, uses `default_open_behavior`.
    #[serde(default)]
    pub create_new_window: Option<bool>,
}

impl Open {
    pub const DEFAULT: Self = Self { create_new_window: None };
}

impl Default for Open {
    fn default() -> Self {
        Self::DEFAULT
    }
}

actions!(
    workspace,
    [
        /// Activates the next pane in the workspace.
        ActivateNextPane,
        /// Activates the previous pane in the workspace.
        ActivatePreviousPane,
        /// Activates the last pane in the workspace.
        ActivateLastPane,
        /// Moves focus to the next major region of the window (editor, open
        /// panels, status bar), cycling and wrapping around. Intended as a
        /// discoverable, screen-reader-friendly way to navigate the window.
        FocusNextPart,
        /// Moves focus to the previous major region of the window. See
        /// [`FocusNextPart`].
        FocusPreviousPart,
        /// Switches to the next window.
        ActivateNextWindow,
        /// Switches to the previous window.
        ActivatePreviousWindow,
        /// Adds a folder to the current project.
        AddFolderToProject,
        /// Clears all notifications.
        ClearAllNotifications,
        /// Rotates the left dock through its panels, then closes it.
        CycleLeftDock,
        /// Clears all navigation history, including forward/backward navigation, recently opened files, and recently closed tabs. **This action is irreversible**.
        ClearNavigationHistory,
        /// Closes the active dock.
        CloseActiveDock,
        /// Closes all docks.
        CloseAllDocks,
        /// Closes the current window.
        CloseWindow,
        /// Closes the current project.
        CloseProject,
        /// Navigates back in the workspace's shared history.
        GoBack,
        /// Navigates forward in the workspace's shared history.
        GoForward,
        /// Creates a new file.
        NewFile,
        /// Creates a new file in a vertical split.
        NewFileSplitVertical,
        /// Creates a new file in a horizontal split.
        NewFileSplitHorizontal,
        /// Opens a new search.
        NewSearch,
        /// Opens a new window.
        NewWindow,
        /// Opens multiple files.
        OpenFiles,
        /// Opens the current location in terminal.
        OpenInTerminal,
        /// Rescans all project files and reloads git state, for changes the file
        /// watcher missed (e.g. made by the host of a VM-mounted folder).
        ReloadProjectFiles,
        /// Reopens the most recently dismissed picker in the current window.
        ReopenLastPicker,
        /// Resets the active dock to its default size.
        ResetActiveDockSize,
        /// Resets all open docks to their default sizes.
        ResetOpenDocksSize,
        /// Resets all panes in the center group to equal sizes, preserving the split layout.
        ResetPaneSizes,
        /// Saves the current file with a new name.
        SaveAs,
        /// Suppresses the current notification.
        SuppressNotification,
        /// Toggles the bottom dock.
        ToggleBottomDock,
        /// Toggles the left dock.
        ToggleLeftDock,
        /// Toggles zoom on the active pane.
        ToggleZoom,
        /// Toggles maximizing the active editor pane within the center area,
        /// hiding other split panes but leaving docks/panels unaffected.
        ToggleEditorZoom,
        /// Zooms in on the active pane.
        ZoomIn,
        /// Zooms out of the active pane.
        ZoomOut,
    ]
);

/// Activates a specific pane by its index.
#[derive(Clone, Deserialize, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
pub struct ActivatePane(pub usize);

/// Moves an item to a specific pane by index.
#[derive(Clone, Deserialize, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields, default)]
pub struct MoveItemToPane {
    pub destination: usize,
    pub focus: bool,
}

impl Default for MoveItemToPane {
    fn default() -> MoveItemToPane {
        MoveItemToPane { destination: 1, focus: true }
    }
}

/// Moves an item to a pane in the specified direction.
#[derive(Clone, Deserialize, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields, default)]
pub struct MoveItemToPaneInDirection {
    pub direction: SplitDirection,
    pub focus: bool,
}

impl Default for MoveItemToPaneInDirection {
    fn default() -> MoveItemToPaneInDirection {
        MoveItemToPaneInDirection { direction: SplitDirection::Right, focus: true }
    }
}

/// Creates a new file in a split of the desired direction.
#[derive(Clone, Deserialize, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct NewFileSplit(pub SplitDirection);

/// Saves all open files in the workspace.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct SaveAll {
    #[serde(default)]
    pub save_intent: Option<SaveIntent>,
}

/// Saves the current file with the specified options.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct Save {
    #[serde(default)]
    pub save_intent: Option<SaveIntent>,
}

/// Moves Focus to the central panes in the workspace.
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
pub struct FocusCenterPane;

///  Closes all items and panes in the workspace.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Default, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct CloseAllItemsAndPanes {
    #[serde(default)]
    pub save_intent: Option<SaveIntent>,
}

/// Closes all inactive tabs and panes in the workspace.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Default, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct CloseInactiveTabsAndPanes {
    #[serde(default)]
    pub save_intent: Option<SaveIntent>,
}

/// Closes the active item across all panes.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Default, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct CloseItemInAllPanes {
    #[serde(default)]
    pub save_intent: Option<SaveIntent>,
    #[serde(default)]
    pub close_pinned: bool,
}

/// Sends a sequence of keystrokes to the active element.
#[derive(Clone, Deserialize, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
pub struct SendKeystrokes(pub String);

actions!(
    project_symbols,
    [
        /// Toggles the project symbols search.
        #[action(name = "Toggle")]
        ToggleProjectSymbols
    ]
);

/// Toggles the file finder interface.
#[derive(Default, PartialEq, Eq, Clone, Deserialize, Action)]
#[action(namespace = file_finder, name = "Toggle")]
#[serde(deny_unknown_fields)]
pub struct ToggleFileFinder {
    #[serde(default)]
    pub separate_history: bool,
    #[serde(default)]
    pub include_ignored: Option<bool>,
}

/// Opens a new terminal.
#[derive(Default, PartialEq, Eq, Clone, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct NewTerminal;

/// Increases size of a currently focused dock by a given amount of pixels.
#[derive(Clone, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct IncreaseActiveDockSize {
    /// For 0px parameter, uses UI font size value.
    #[serde(default)]
    pub px: u32,
}

/// Decreases size of a currently focused dock by a given amount of pixels.
#[derive(Clone, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct DecreaseActiveDockSize {
    /// For 0px parameter, uses UI font size value.
    #[serde(default)]
    pub px: u32,
}

/// Increases size of all currently visible docks uniformly, by a given amount of pixels.
#[derive(Clone, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct IncreaseOpenDocksSize {
    /// For 0px parameter, uses UI font size value.
    #[serde(default)]
    pub px: u32,
}

/// Decreases size of all currently visible docks uniformly, by a given amount of pixels.
#[derive(Clone, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct DecreaseOpenDocksSize {
    /// For 0px parameter, uses UI font size value.
    #[serde(default)]
    pub px: u32,
}

actions!(
    workspace,
    [
        /// Activates the pane to the left.
        ActivatePaneLeft,
        /// Activates the pane to the right.
        ActivatePaneRight,
        /// Activates the pane above.
        ActivatePaneUp,
        /// Activates the pane below.
        ActivatePaneDown,
        /// Swaps the current pane with the one to the left.
        SwapPaneLeft,
        /// Swaps the current pane with the one to the right.
        SwapPaneRight,
        /// Swaps the current pane with the one above.
        SwapPaneUp,
        /// Swaps the current pane with the one below.
        SwapPaneDown,
        // Swaps the current pane with the first available adjacent pane (searching in order: below, above, right, left) and activates that pane.
        SwapPaneAdjacent,
        /// Move the current pane to be at the far left.
        MovePaneLeft,
        /// Move the current pane to be at the far right.
        MovePaneRight,
        /// Move the current pane to be at the very top.
        MovePaneUp,
        /// Move the current pane to be at the very bottom.
        MovePaneDown,
    ]
);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseIntent {
    /// Quit the program entirely.
    Quit,
    /// Close a window.
    CloseWindow,
    /// Replace the workspace in an existing window.
    ReplaceWindow,
}

#[derive(Clone)]
pub struct Toast {
    id: NotificationId,
    message: Cow<'static, str>,
    autohide: bool,
    #[allow(clippy::type_complexity)]
    on_click: Option<(Cow<'static, str>, Arc<dyn Fn(&mut Window, &mut App)>)>,
}

impl Toast {
    pub fn new<I: Into<Cow<'static, str>>>(id: NotificationId, message: I) -> Self {
        Toast { id, message: message.into(), on_click: None, autohide: false }
    }

    pub fn on_click<F, M>(mut self, message: M, on_click: F) -> Self
    where
        M: Into<Cow<'static, str>>,
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some((message.into(), Arc::new(on_click)));
        self
    }

    pub const fn autohide(mut self) -> Self {
        self.autohide = true;
        self
    }
}

impl PartialEq for Toast {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.message == other.message && self.on_click.is_some() == other.on_click.is_some()
    }
}

/// Opens a new terminal with the specified working directory.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct OpenTerminal {
    pub working_directory: PathBuf,
}

pub fn init(app_state: Arc<AppState>, cx: &mut App) {
    theme_preview::init(cx);
    toast_layer::init(cx);
    persistence::init(Arc::clone(&app_state.fs), cx);

    cx.on_app_quit(flush_windows_serialization_on_quit).detach();

    cx.on_action(|_: &CloseWindow, cx| Workspace::close_global(cx))
        .on_action(|action: &Open, cx: &mut App| {
            let app_state = AppState::global(cx);
            prompt_and_open_paths(
                app_state,
                PathPromptOptions { files: true, directories: true, multiple: true, prompt: None },
                action.create_new_window.unwrap_or(false),
                cx,
            );
        })
        .on_action(|_: &OpenFiles, cx: &mut App| {
            let app_state = AppState::global(cx);
            prompt_and_open_paths(
                app_state,
                PathPromptOptions { files: true, directories: true, multiple: true, prompt: None },
                true,
                cx,
            );
        });
}

pub struct AppState {
    pub languages: Arc<LanguageRegistry>,

    pub fs: Arc<dyn fs::Fs>,
    pub build_window_options: fn(Option<Uuid>, &mut App) -> WindowOptions,
}

struct GlobalAppState(Arc<AppState>);

impl Global for GlobalAppState {}

/// Tracks worktree creation progress for the workspace.
/// Read by the title bar to show a loading indicator on the worktree button.
#[derive(Default)]
pub struct ActiveWorktreeCreation {
    pub label: Option<SharedString>,
    pub is_switch: bool,
}

/// Captured workspace state used when switching between worktrees.
/// Stores the layout and open files so they can be restored in the new workspace.
pub struct PreviousWorkspaceState {
    pub dock_structure: DockStructure,
    pub open_file_paths: Vec<PathBuf>,
    pub active_file_path: Option<PathBuf>,
    pub focused_dock: Option<DockPosition>,
}

impl AppState {
    #[track_caller]
    pub fn global(cx: &App) -> Arc<Self> {
        Arc::clone(&cx.global::<GlobalAppState>().0)
    }
    pub fn try_global(cx: &App) -> Option<Arc<Self>> {
        cx.try_global::<GlobalAppState>().map(|state| Arc::clone(&state.0))
    }
    pub fn set_global(state: Arc<AppState>, cx: &mut App) {
        cx.set_global(GlobalAppState(state));
    }
}

/// What fills the window below the title bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkspaceMode {
    /// Docks and center panes.
    #[default]
    Editor,
    /// Only the git history pane (git graph and commit tabs).
    GitHistory,
    /// Only terminal panes.
    Terminal,
}

impl WorkspaceMode {
    /// Buffers outside the project (commit revisions) stay in the history pane.
    pub fn opens_in_history_pane(self, is_project_file: bool) -> bool {
        self == WorkspaceMode::GitHistory && !is_project_file
    }

    /// Whether this mode shows the docks. Dock actions are ignored in modes
    /// without them, rather than acting on hidden docks or switching modes.
    pub const fn has_docks(self) -> bool {
        matches!(self, WorkspaceMode::Editor)
    }
}

/// Whether the status bar is drawn: only in Editor mode, and not while
/// something is zoomed there. Git History mode has no use for it.
const fn status_bar_shown(mode: WorkspaceMode, zoomed: bool) -> bool {
    matches!(mode, WorkspaceMode::Editor) && !zoomed
}

pub enum Event {
    PaneAdded(Entity<Pane>),
    PaneRemoved,
    ItemAdded {
        item: Box<dyn ItemHandle>,
    },
    /// An item was added to one of the git history panes.
    GitHistoryItemAdded {
        item: Box<dyn ItemHandle>,
    },
    ActiveItemChanged,
    ItemRemoved {
        item_id: EntityId,
    },
    UserSavedItem {
        pane: WeakEntity<Pane>,
        item: Box<dyn WeakItemHandle>,
        save_intent: SaveIntent,
    },
    WorkspaceCreated(WeakEntity<Workspace>),
    OpenBundledFile {
        text: Cow<'static, str>,
        title: &'static str,
        language: &'static str,
    },
    ZoomChanged,
    ModalOpened,
    Activate,
    PanelAdded(AnyView),
    WorktreeCreationChanged,
}

/// Controls which types of items should be made visible in the project panel
/// when opened.
#[derive(Debug, Clone)]
pub enum OpenVisible {
    /// Make all opened items visible (both files and directories).
    All,
    /// Don't make any opened items visible.
    None,
    /// Only make opened files visible, not directories.
    OnlyFiles,
    /// Only make opened directories visible, not files.
    OnlyDirectories,
}

type PromptForNewPath = Box<
    dyn Fn(
        &mut Workspace,
        DirectoryLister,
        Option<String>,
        &mut Window,
        &mut Context<Workspace>,
    ) -> oneshot::Receiver<Option<Vec<PathBuf>>>,
>;

type PromptForOpenPath = Box<
    dyn Fn(
        &mut Workspace,
        DirectoryLister,
        &mut Window,
        &mut Context<Workspace>,
    ) -> oneshot::Receiver<Option<Vec<PathBuf>>>,
>;

#[derive(Default)]
struct DispatchingKeystrokes {
    dispatched: FxHashSet<Vec<Keystroke>>,
    queue: VecDeque<Keystroke>,
    task: Option<Shared<Task<()>>>,
}

/// Collects everything project-related for a certain window opened.
/// In some way, is a counterpart of a window, as the [`WindowHandle`] could be downcast into `Workspace`.
///
/// A `Workspace` usually consists of 1 or more projects, a central pane group, 3 docks and a status bar.
/// The `Workspace` owns everybody's state and serves as a default, "global context",
/// that can be used to register a global action to be triggered from any place in the window.
pub struct Workspace {
    weak_self: WeakEntity<Self>,
    #[allow(clippy::type_complexity)]
    workspace_actions: Vec<Box<dyn Fn(Div, &Workspace, &mut Window, &mut Context<Self>) -> Div>>,
    zoomed: Option<AnyWeakView>,
    previous_dock_drag_coordinates: Option<Point<Pixels>>,
    /// Open/close overlay state of each dock (left, bottom, right).
    dock_motions: [Rc<DockMotion>; 2],
    zoomed_position: Option<DockPosition>,
    maximized_pane: Option<WeakEntity<Pane>>,
    center: PaneGroup,
    left_dock: Entity<Dock>,
    bottom_dock: Entity<Dock>,
    panes: Vec<Entity<Pane>>,
    panes_by_item: FxHashMap<EntityId, WeakEntity<Pane>>,
    active_pane: Entity<Pane>,
    last_active_center_pane: Option<WeakEntity<Pane>>,
    mode: WorkspaceMode,
    /// Hosts the git graph and commit views; its panes are never part of
    /// `center` or `panes`.
    git_history_center: PaneGroup,
    active_git_history_pane: Entity<Pane>,
    git_history_sidebar_width: Pixels,
    editor_view: Entity<ModeView>,
    git_history_view: Entity<ModeView>,
    /// Hosts terminal mode's terminals; its panes are never part of `center`
    /// or `panes`.
    terminal_center: PaneGroup,
    active_terminal_pane: Entity<Pane>,
    terminal_layout: TerminalLayout,
    terminal_mode_view: Entity<ModeView>,
    window_header: Entity<window_header::WindowHeader>,

    status_bar: Entity<StatusBar>,
    pub(crate) modal_layer: Entity<ModalLayer>,
    toast_layer: Entity<ToastLayer>,
    titlebar_item: Option<AnyView>,
    /// Drawn around the window to tell it apart from other windows: an index
    /// into the theme's accent colours.
    border_color: Option<u32>,
    titlebar_focus_handle: FocusHandle,
    notifications: Notifications,
    suppressed_notifications: FxHashSet<NotificationId>,
    project: Entity<Project>,

    window_edited: bool,
    last_window_title: Option<String>,
    dirty_items: FxHashMap<EntityId, Subscription>,

    app_state: Arc<AppState>,
    dispatching_keystrokes: Rc<RefCell<DispatchingKeystrokes>>,
    _subscriptions: Vec<Subscription>,

    _schedule_serialize_workspace: Option<Task<()>>,
    _serialize_workspace_task: Option<Task<()>>,
    pane_history_timestamp: Arc<AtomicUsize>,
    /// One back/forward history shared by every center and Git History pane.
    nav_history: NavHistory,
    /// The content (center or Git History) pane focused last: where the user is,
    /// for the history. Docks never take it.
    last_content_pane: WeakEntity<Pane>,
    bounds: Bounds<Pixels>,
    on_prompt_for_new_path: Option<PromptForNewPath>,
    on_prompt_for_open_path: Option<PromptForOpenPath>,
    terminal_provider: Option<Box<dyn TerminalProvider>>,
    /// The `.ed/workspace.json` contents last written, to skip identical writes.
    last_serialized_state: Option<String>,
    scheduled_tasks: Vec<Task<()>>,
    removing: bool,
    _panels_task: Option<Task<Result<()>>>,
    active_worktree_creation: ActiveWorktreeCreation,
    deferred_save_items: Vec<Box<dyn WeakItemHandle>>,
    persisted_recent_navigation_history: Vec<PathBuf>,
    last_active_project_path: Option<ProjectPath>,
    restoring_workspace: bool,
}

impl EventEmitter<Event> for Workspace {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenMode {
    /// Open the workspace in a new window.
    NewWindow,
    /// Replace the requesting window's workspace.
    #[default]
    Activate,
}

impl Workspace {
    pub fn new(
        project: Entity<Project>,
        app_state: Arc<AppState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe_in(&project, window, move |this, _, event, window, cx| {
            match event {
                &project::Event::WorktreeRemoved(_) => {
                    this.update_window_title(window, cx);
                    this.serialize_workspace(window, cx);
                    this.update_history(cx);
                }

                &project::Event::WorktreeAdded(id) => {
                    this.update_window_title(window, cx);
                    if this.project().read(cx).worktree_for_id(id, cx).is_some_and(|wt| wt.read(cx).is_visible()) {
                        this.serialize_workspace(window, cx);
                        this.update_history(cx);
                    }
                }
                project::Event::WorktreeUpdatedEntries(worktree_id, changes) => {
                    this.remove_deleted_navigation_history(*worktree_id, changes, cx);
                    this.update_window_title(window, cx);
                    this.serialize_workspace(window, cx);
                }

                project::Event::EntryRenamed { old_abs_path, new_abs_path, .. } => {
                    if this.rename_persisted_navigation_history_paths(old_abs_path, new_abs_path) {
                        this.serialize_workspace(window, cx);
                    }
                }

                project::Event::Closed => {
                    window.remove_window();
                }

                project::Event::DeletedEntry(_, entry_id) => {
                    for pane in &this.panes {
                        pane.update(cx, |pane, cx| pane.handle_deleted_project_item(*entry_id, window, cx));
                    }
                }

                project::Event::Toast { notification_id, message, link } => {
                    this.show_notification(NotificationId::named(notification_id.clone()), cx, |cx| {
                        let mut notification = MessageNotification::new(message.clone(), cx);
                        if let Some(link) = link {
                            notification = notification.more_info_message(link.label).more_info_url(link.url);
                        }

                        cx.new(|_| notification)
                    });
                }

                project::Event::HideToast { notification_id } => {
                    this.dismiss_notification(&NotificationId::named(notification_id.clone()), cx);
                }

                project::Event::LanguageServerPrompt(request) => {
                    struct LanguageServerPrompt;

                    this.show_notification(NotificationId::composite::<LanguageServerPrompt>(request.id), cx, |cx| {
                        cx.new(|cx| notifications::LanguageServerPrompt::new(request.clone(), cx))
                    });
                }

                _ => {}
            }
            cx.notify();
        })
        .detach();

        cx.on_focus_lost(window, |this, window, cx| {
            let focus_handle =
                window.focus_lost_restore_target(cx).unwrap_or_else(|| this.fallback_focus_handle(window, cx));
            window.focus(&focus_handle, cx);
        })
        .detach();

        let weak_handle = cx.entity().downgrade();
        let pane_history_timestamp = Arc::new(AtomicUsize::new(0));
        let nav_history = NavHistory::new(weak_handle.clone(), Arc::clone(&pane_history_timestamp));

        let center_pane = cx.new(|cx| {
            let mut center_pane = Pane::new(
                weak_handle.clone(),
                project.clone(),
                Arc::clone(&pane_history_timestamp),
                Some(Arc::new(|dragged, _, cx| !is_dragged_terminal(dragged, cx))),
                NewFile.boxed_clone(),
                window,
                cx,
            );
            center_pane.set_can_split(Some(Arc::new(|_, _, _, _| true)));
            pane::set_editor_tab_bar_buttons(&mut center_pane, cx);
            center_pane.set_nav_history(nav_history.clone());
            center_pane
        });
        cx.subscribe_in(&center_pane, window, Self::handle_pane_event).detach();

        window.focus(&center_pane.focus_handle(cx), cx);

        cx.emit(Event::PaneAdded(center_pane.clone()));

        let git_history_pane = Self::new_git_history_pane(
            weak_handle.clone(),
            project.clone(),
            Arc::clone(&pane_history_timestamp),
            nav_history.clone(),
            window,
            cx,
        );

        let terminal_pane = Self::new_terminal_pane(weak_handle.clone(), project.clone(), window, cx);

        cx.emit(Event::WorkspaceCreated(weak_handle.clone()));
        let modal_layer = cx.new(|_| ModalLayer::new());
        let toast_layer = cx.new(|_| ToastLayer::new());
        cx.subscribe(&modal_layer, |_, _, _: &modal_layer::ModalOpenedEvent, cx| {
            cx.emit(Event::ModalOpened);
        })
        .detach();

        let left_dock = Dock::new(DockPosition::Left, modal_layer.clone(), window, cx);
        let bottom_dock = Dock::new(DockPosition::Bottom, modal_layer.clone(), window, cx);
        let left_dock_buttons = cx.new(|cx| PanelButtons::new(left_dock.clone(), cx));
        let bottom_dock_buttons = cx.new(|cx| PanelButtons::new(bottom_dock.clone(), cx));
        let status_bar = cx.new(|cx| {
            let mut status_bar = StatusBar::new(&center_pane.clone(), window, cx);
            status_bar.add_left_item(left_dock_buttons, window, cx);
            status_bar.add_right_item(bottom_dock_buttons, window, cx);
            status_bar
        });

        let workspace = cx.entity();
        let editor_view = cx
            .new(|cx| ModeView::new(&workspace, WorkspaceMode::Editor, &[left_dock.clone(), bottom_dock.clone()], cx));
        let git_history_view = cx.new(|cx| ModeView::new(&workspace, WorkspaceMode::GitHistory, &[], cx));
        let terminal_mode_view = cx.new(|cx| ModeView::new(&workspace, WorkspaceMode::Terminal, &[], cx));
        let window_header = cx.new(|cx| window_header::WindowHeader::new(&workspace, &left_dock, &project, cx));

        let subscriptions = vec![
            cx.observe_window_activation(window, Self::on_window_activation_changed),
            cx.observe_window_bounds(window, |_, _, cx| cx.notify()),
        ];

        cx.defer_in(window, move |this, window, cx| {
            this.update_window_title(window, cx);
            this.show_initial_notifications(cx);
        });

        let mut center = PaneGroup::new(center_pane.clone());
        center.set_is_center(true);
        center.mark_positions(cx);

        Workspace {
            weak_self: weak_handle.clone(),
            zoomed: None,
            zoomed_position: None,
            maximized_pane: None,
            previous_dock_drag_coordinates: None,
            dock_motions: Default::default(),
            center,
            panes: vec![center_pane.clone()],
            panes_by_item: FxHashMap::default(),
            active_pane: center_pane.clone(),
            last_active_center_pane: Some(center_pane.downgrade()),
            mode: WorkspaceMode::Editor,
            git_history_center: PaneGroup::new(git_history_pane.clone()),
            active_git_history_pane: git_history_pane,
            git_history_sidebar_width: GIT_HISTORY_SIDEBAR_WIDTH,
            editor_view,
            git_history_view,
            terminal_center: PaneGroup::new(terminal_pane.clone()),
            active_terminal_pane: terminal_pane,
            terminal_layout: TerminalLayout::Pending(None),
            terminal_mode_view,
            window_header,

            status_bar,
            modal_layer,
            toast_layer,
            titlebar_item: None,
            border_color: None,
            titlebar_focus_handle: cx.focus_handle(),
            notifications: Notifications::default(),
            suppressed_notifications: FxHashSet::default(),
            left_dock,
            bottom_dock,
            _panels_task: None,
            project: project.clone(),

            dispatching_keystrokes: Rc::default(),
            window_edited: false,
            last_window_title: None,
            dirty_items: FxHashMap::default(),

            app_state,

            _schedule_serialize_workspace: None,
            _serialize_workspace_task: None,

            _subscriptions: subscriptions,
            pane_history_timestamp,
            nav_history,
            last_content_pane: center_pane.downgrade(),
            workspace_actions: Vec::default(),
            // This data will be incorrect, but it will be overwritten by the time it needs to be used.
            bounds: Bounds::default(),
            on_prompt_for_new_path: None,
            on_prompt_for_open_path: None,
            terminal_provider: None,
            last_serialized_state: None,

            scheduled_tasks: Vec::new(),
            removing: false,
            active_worktree_creation: ActiveWorktreeCreation::default(),
            deferred_save_items: Vec::new(),
            persisted_recent_navigation_history: Vec::new(),
            last_active_project_path: None,
            restoring_workspace: false,
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn new_local(
        abs_paths: Vec<PathBuf>,
        app_state: Arc<AppState>,
        requesting_window: Option<WindowHandle<Workspace>>,
        init: Option<Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send>>,
        open_mode: OpenMode,
        cx: &mut App,
    ) -> Task<anyhow::Result<OpenResult>> {
        let project_handle = Project::local(Arc::clone(&app_state.languages), Arc::clone(&app_state.fs), cx);

        cx.spawn(async move |cx| {
            let mut paths_to_open = Vec::with_capacity(abs_paths.len());
            for path in abs_paths {
                if let Ok(canonical) = app_state.fs.canonicalize(&path).await {
                    paths_to_open.push(canonical);
                } else {
                    paths_to_open.push(path);
                }
            }

            let serialized_workspace = match paths_to_open.first() {
                Some(root) => persistence::load_local_workspace_state(app_state.fs.as_ref(), root).await,
                None => None,
            };

            // Get project paths for all of the abs_paths
            let mut project_paths: Vec<(PathBuf, Option<ProjectPath>)> = Vec::with_capacity(paths_to_open.len());

            for path in paths_to_open {
                if let Some((_, project_entry)) = cx
                    .update(|cx| Workspace::project_path_for_path(project_handle.clone(), &path, true, cx))
                    .await
                    .log_err()
                {
                    project_paths.push((path, Some(project_entry)));
                } else {
                    project_paths.push((path, None));
                }
            }

            let window_to_replace = match open_mode {
                OpenMode::NewWindow => None,
                OpenMode::Activate => requesting_window,
            };

            let (window, workspace): (WindowHandle<Workspace>, Entity<Workspace>) = if let Some(window) =
                window_to_replace
            {
                let workspace = window.update(cx, |old_workspace, window, cx| {
                    old_workspace.stop_language_servers(cx);
                    replace_window_workspace(window, cx, |window, cx| {
                        let mut workspace = Workspace::new(project_handle.clone(), Arc::clone(&app_state), window, cx);

                        // Call init callback to add items before window renders
                        if let Some(init) = init {
                            init(&mut workspace, window, cx);
                        }

                        workspace
                    })
                })?;
                (window, workspace)
            } else {
                let options = cx.update(|cx| (app_state.build_window_options)(None, cx));
                let window = cx.open_window(options, {
                    let app_state = Arc::clone(&app_state);
                    let project_handle = project_handle.clone();
                    move |window, cx| {
                        cx.new(|cx| {
                            let mut workspace = Workspace::new(project_handle, app_state, window, cx);

                            // Call init callback to add items before window renders
                            if let Some(init) = init {
                                init(&mut workspace, window, cx);
                            }

                            workspace
                        })
                    }
                })?;
                let workspace = window.update(cx, |_, _, cx| cx.entity())?;
                (window, workspace)
            };

            let opened_items = window
                .update(cx, |_, window, cx| open_items(serialized_workspace, project_paths, window, cx))?
                .await
                .unwrap_or_default();

            window
                .update(cx, |workspace, window, cx| {
                    workspace.update_history(cx);
                    window.activate_window();
                })
                .log_err();

            Ok(OpenResult { window, workspace, opened_items })
        })
    }

    pub fn weak_handle(&self) -> WeakEntity<Self> {
        self.weak_self.clone()
    }

    pub const fn active_worktree_creation(&self) -> &ActiveWorktreeCreation {
        &self.active_worktree_creation
    }

    pub fn set_active_worktree_creation(
        &mut self,
        label: Option<SharedString>,
        is_switch: bool,
        cx: &mut Context<Self>,
    ) {
        self.active_worktree_creation.label = label;
        self.active_worktree_creation.is_switch = is_switch;
        cx.emit(Event::WorktreeCreationChanged);
        cx.notify();
    }

    /// Captures the current workspace state for restoring after a worktree switch.
    /// This includes dock layout, open file paths, and the active file path.
    pub fn capture_state_for_worktree_switch(
        &self,
        window: &Window,
        fallback_focused_dock: Option<DockPosition>,
        cx: &App,
    ) -> PreviousWorkspaceState {
        let dock_structure = self.capture_dock_state(window, cx);
        let open_file_paths = self.open_item_abs_paths(cx);
        let active_file_path = self
            .active_item(cx)
            .and_then(|item| item.project_path(cx))
            .and_then(|pp| self.project().read(cx).absolute_path(&pp, cx));

        let focused_dock = self.focused_dock_position(window, cx).or(fallback_focused_dock);

        PreviousWorkspaceState { dock_structure, open_file_paths, active_file_path, focused_dock }
    }

    pub fn open_item_abs_paths(&self, cx: &App) -> Vec<PathBuf> {
        self.items(cx)
            .filter_map(|item| {
                let project_path = item.project_path(cx)?;
                self.project.read(cx).absolute_path(&project_path, cx)
            })
            .collect()
    }

    pub const fn status_bar(&self) -> &Entity<StatusBar> {
        &self.status_bar
    }

    /// The status bar is shown only in Editor mode, and hidden there while
    /// something is zoomed, so the zoomed view extends down to the window edge.
    pub fn status_bar_visible(&self, cx: &App) -> bool {
        status_bar_shown(self.mode, self.has_zoomed(self.mode, cx))
    }

    pub const fn mode(&self) -> WorkspaceMode {
        self.mode
    }

    /// The pane filling the window in the current mode: the active center
    /// pane, or the history pane.
    pub const fn shown_pane(&self) -> &Entity<Pane> {
        match self.mode {
            WorkspaceMode::Editor => &self.active_pane,
            WorkspaceMode::GitHistory => &self.active_git_history_pane,
            WorkspaceMode::Terminal => &self.active_terminal_pane,
        }
    }

    /// Switches what fills the window and moves focus into it. A zoomed dock
    /// panel takes focus back in Editor mode, so it is still zoomed on return.
    pub fn set_mode(&mut self, mode: WorkspaceMode, window: &mut Window, cx: &mut Context<Self>) {
        if !self.switch_mode(mode, cx) {
            return;
        }
        window.focus(&self.fallback_focus_handle(window, cx), cx);
    }

    /// Switches the mode without touching focus, for when focus already moved
    /// into the newly shown area. Returns whether the mode changed.
    fn switch_mode(&mut self, mode: WorkspaceMode, cx: &mut Context<Self>) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        // A mode switch replaces everything rather than changing a local layout.
        match mode {
            WorkspaceMode::Editor => self.center.snap_motion(),
            WorkspaceMode::GitHistory => self.git_history_center.snap_motion(),
            WorkspaceMode::Terminal => self.terminal_center.snap_motion(),
        }
        cx.notify();
        true
    }

    pub const fn app_state(&self) -> &Arc<AppState> {
        &self.app_state
    }

    pub fn set_panels_task(&mut self, task: Task<Result<()>>) {
        self._panels_task = Some(task);
    }

    pub const fn take_panels_task(&mut self) -> Option<Task<Result<()>>> {
        self._panels_task.take()
    }

    pub const fn project(&self) -> &Entity<Project> {
        &self.project
    }

    pub fn set_titlebar_item(&mut self, item: AnyView, _: &mut Window, cx: &mut Context<Self>) {
        self.titlebar_item = Some(item);
        cx.notify();
    }

    pub const fn border_color(&self) -> Option<u32> {
        self.border_color
    }

    /// Sets the window's border colour and saves it with the workspace.
    pub fn set_border_color(&mut self, color: Option<u32>, window: &Window, cx: &mut Context<Self>) {
        self.border_color = color;
        self.serialize_workspace(window, cx);
        cx.notify();
    }

    pub fn set_prompt_for_new_path(&mut self, prompt: PromptForNewPath) {
        self.on_prompt_for_new_path = Some(prompt);
    }

    pub fn set_prompt_for_open_path(&mut self, prompt: PromptForOpenPath) {
        self.on_prompt_for_open_path = Some(prompt);
    }

    pub fn set_terminal_provider(&mut self, provider: impl TerminalProvider + 'static) {
        self.terminal_provider = Some(Box::new(provider));
    }

    pub fn worktrees<'a>(&self, cx: &'a App) -> impl 'a + Iterator<Item = Entity<Worktree>> {
        self.project.read(cx).worktrees(cx)
    }

    pub fn visible_worktrees<'a>(&self, cx: &'a App) -> impl 'a + Iterator<Item = Entity<Worktree>> {
        self.project.read(cx).visible_worktrees(cx)
    }

    pub fn close_global(cx: &mut App) {
        cx.defer(|cx| {
            cx.windows().iter().find(|window| {
                window
                    .update(cx, |_, window, _| {
                        if window.is_window_active() {
                            //This can only get called when the window's project connection has been lost
                            //so we don't need to prompt the user for anything and instead just close the window
                            window.remove_window();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false)
            });
        });
    }

    pub fn prepare_to_close(
        &mut self,
        close_intent: CloseIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<bool>> {
        cx.spawn_in(window, async move |this, cx| {
            let running_items = this.update(cx, |this, cx| this.terminals_with_running_process(cx))?;
            if !pane::confirm_close_running_items(&running_items, cx).await? {
                return Ok(false);
            }

            this.update(cx, |this, _| {
                if close_intent == CloseIntent::CloseWindow {
                    this.removing = true;
                }
            })?;

            this.update_in(cx, |this, window, cx| this.save_all_internal(SaveIntent::Close, window, cx))?.await
        })
    }

    fn save_all(&mut self, action: &SaveAll, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all_internal(action.save_intent.unwrap_or(SaveIntent::SaveAll), window, cx).detach_and_log_err(cx);
    }

    fn send_keystrokes(&mut self, action: &SendKeystrokes, window: &mut Window, cx: &mut Context<Self>) {
        let keystrokes: Vec<Keystroke> = action
            .0
            .split(' ')
            .filter_map(|k| Keystroke::parse(k).log_err())
            .map(|k| cx.keyboard_mapper().map_key_equivalent(k, false).inner().clone())
            .collect();
        drop(self.send_keystrokes_impl(keystrokes, window, cx));
    }

    pub fn send_keystrokes_impl(
        &mut self,
        keystrokes: Vec<Keystroke>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Shared<Task<()>> {
        let mut state = self.dispatching_keystrokes.borrow_mut();
        if !state.dispatched.insert(keystrokes.clone()) {
            cx.propagate();
            return state.task.clone().expect("dispatch task is set while dispatching");
        }

        state.queue.extend(keystrokes);

        let keystrokes = Rc::clone(&self.dispatching_keystrokes);
        if state.task.is_none() {
            state.task = Some(
                window
                    .spawn(cx, async move |cx| {
                        // limit to 100 keystrokes to avoid infinite recursion.
                        for _ in 0..100 {
                            let keystroke = {
                                let mut state = keystrokes.borrow_mut();
                                let Some(keystroke) = state.queue.pop_front() else {
                                    state.dispatched.clear();
                                    state.task.take();
                                    return;
                                };
                                keystroke
                            };
                            let focus_changed = cx
                                .update(|window, cx| {
                                    let focused = window.focused(cx);
                                    window.dispatch_keystroke(keystroke.clone(), cx);
                                    if window.focused(cx) != focused {
                                        // dispatch_keystroke may cause the focus to change.
                                        // draw's side effect is to schedule the FocusChanged events in the current flush effect cycle
                                        // And we need that to happen before the next keystroke to keep vim mode happy...
                                        // (Note that the tests always do this implicitly, so you must manually test with something like:
                                        //   "bindings": { "g z": ["workspace::SendKeystrokes", ": j <enter> u"]}
                                        // )
                                        window.draw(cx).clear(cx);
                                        return true;
                                    }
                                    false
                                })
                                .unwrap_or(false);

                            if focus_changed {
                                async_macos::yield_now().await;
                            }
                        }

                        *keystrokes.borrow_mut() = DispatchingKeystrokes::default();
                        log::error!("over 100 keystrokes passed to send_keystrokes");
                    })
                    .shared(),
            );
        }
        state.task.clone().expect("dispatch task was just set")
    }

    /// Saves every dirty item that already has a file, without prompting for
    /// a path for untitled ones. Resolves to false if a save was cancelled.
    pub fn save_dirty_files(&self, window: &Window, cx: &Context<Self>) -> Task<Result<bool>> {
        let dirty_items = self
            .panes
            .iter()
            .flat_map(|pane| {
                pane.read(cx)
                    .items()
                    .filter(|item| item.is_dirty(cx) && item.can_save(cx))
                    .map(|item| (pane.clone(), item.boxed_clone()))
            })
            .collect::<Vec<_>>();
        let project = self.project.clone();
        cx.spawn_in(window, async move |_, cx| {
            for (pane, item) in dirty_items {
                if !Pane::save_item(project.clone(), pane, &*item, SaveIntent::SaveAll, cx).await? {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }

    fn save_all_internal(
        &self,
        mut save_intent: SaveIntent,
        window: &Window,
        cx: &Context<Self>,
    ) -> Task<Result<bool>> {
        let dirty_items = self
            .panes
            .iter()
            .flat_map(|pane| {
                pane.read(cx).items().filter(|&item| item.is_dirty(cx)).map(|item| {
                    item.tab_content_text(0, cx);
                    (pane.clone(), item.boxed_clone())
                })
            })
            .collect::<Vec<_>>();

        let project = self.project.clone();
        cx.spawn_in(window, async move |workspace, cx| {
            if save_intent == SaveIntent::Close && !dirty_items.is_empty() {
                workspace.update(cx, |_, cx| cx.emit(Event::Activate))?;

                if dirty_items.len() > 1 {
                    let answer = workspace.update_in(cx, |_, window, cx| {
                        let detail = Pane::file_names_for_prompt(&mut dirty_items.iter().map(|(_, handle)| handle), cx);
                        window.prompt(
                            PromptLevel::Warning,
                            "Do you want to save all changes in the following files?",
                            Some(&detail),
                            &["Save all", "Discard all", "Cancel"],
                            cx,
                        )
                    })?;
                    match answer.await.log_err() {
                        Some(0) => save_intent = SaveIntent::SaveAll,
                        Some(1) => save_intent = SaveIntent::Skip,
                        Some(2) => return Ok(false),
                        _ => {}
                    }
                }
            }

            for (pane, item) in dirty_items {
                let (singleton, project_entry_ids) =
                    cx.update(|_, cx| (item.buffer_kind(cx) == ItemBufferKind::Singleton, item.project_entry_ids(cx)))?;
                if (singleton || !project_entry_ids.is_empty())
                    && !Pane::save_item(project.clone(), pane, &*item, save_intent, cx).await?
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }

    pub fn items<'a>(&'a self, cx: &'a App) -> impl 'a + Iterator<Item = &'a Box<dyn ItemHandle>> {
        self.panes.iter().flat_map(|pane| pane.read(cx).items())
    }

    pub fn item_of_type<T: Item>(&self, cx: &App) -> Option<Entity<T>> {
        self.items_of_type(cx).max_by_key(item::ItemHandle::item_id)
    }

    pub fn items_of_type<'a, T: Item>(&'a self, cx: &'a App) -> impl 'a + Iterator<Item = Entity<T>> {
        self.panes.iter().flat_map(|pane| pane.read(cx).items_of_type())
    }

    pub fn active_item(&self, cx: &App) -> Option<Box<dyn ItemHandle>> {
        self.active_pane().read(cx).active_item()
    }

    pub fn active_item_as<I: 'static>(&self, cx: &App) -> Option<Entity<I>> {
        let item = self.active_item(cx)?;
        // Prefer an exact downcast so that we return the active item itself when
        // its concrete type matches, preserving entity identity for callers that
        // compare `entity_id`s. Fall back to `act_as` so that wrapper items (e.g.
        // diff views) resolve to the inner view they expose.
        item.to_any_view().downcast::<I>().ok().or_else(|| item.act_as::<I>(cx))
    }

    fn active_project_path(&self, cx: &App) -> Option<ProjectPath> {
        let item = self.active_item(cx)?;
        item.project_path(cx)
    }

    pub fn most_recent_active_path(&self, cx: &App) -> Option<PathBuf> {
        self.recent_navigation_history_iter(cx).find_map(|(path, abs_path)| {
            let worktree = self.project.read(cx).worktree_for_id(path.worktree_id, cx)?;
            if !worktree.read(cx).is_visible() {
                return None;
            }
            abs_path
        })
    }

    pub fn save_active_item(&mut self, save_intent: SaveIntent, window: &mut Window, cx: &mut App) -> Task<Result<()>> {
        let project = self.project.clone();
        let pane = self.active_pane().clone();
        let item = pane.read(cx).active_item();

        window.spawn(cx, async move |cx| {
            if let Some(item) = item {
                Pane::save_item(project, pane, item.as_ref(), save_intent, cx).await.map(|_| ())
            } else {
                Ok(())
            }
        })
    }

    pub fn close_inactive_items_and_panes(
        &mut self,
        action: &CloseInactiveTabsAndPanes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(task) = self.close_all_internal(true, action.save_intent.unwrap_or(SaveIntent::Close), window, cx) {
            task.detach_and_log_err(cx);
        }
    }

    pub fn close_all_items_and_panes(
        &mut self,
        action: &CloseAllItemsAndPanes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(task) = self.close_all_internal(false, action.save_intent.unwrap_or(SaveIntent::Close), window, cx)
        {
            task.detach_and_log_err(cx);
        }
    }

    /// Closes the active item across all panes.
    pub fn close_item_in_all_panes(
        &mut self,
        action: &CloseItemInAllPanes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_item) = self.active_pane().read(cx).active_item() else {
            return;
        };

        let save_intent = action.save_intent.unwrap_or(SaveIntent::Close);
        let close_pinned = action.close_pinned;

        if let Some(project_path) = active_item.project_path(cx) {
            self.close_items_with_project_path(&project_path, save_intent, close_pinned, window, cx);
        } else if close_pinned || !self.active_pane().read(cx).is_active_item_pinned() {
            let item_id = active_item.item_id();
            self.active_pane().update(cx, |pane, cx| {
                pane.close_item_by_id(item_id, save_intent, window, cx).detach_and_log_err(cx);
            });
        }
    }

    /// Closes all items with the given project path across all panes.
    pub fn close_items_with_project_path(
        &mut self,
        project_path: &ProjectPath,
        save_intent: SaveIntent,
        close_pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panes = self.panes().to_vec();
        for pane in panes {
            pane.update(cx, |pane, cx| {
                pane.close_items_for_project_path(project_path, save_intent, close_pinned, window, cx)
                    .detach_and_log_err(cx);
            });
        }
    }

    fn close_all_internal(
        &self,
        retain_active_pane: bool,
        save_intent: SaveIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<Result<()>>> {
        let current_pane = self.active_pane();

        let mut tasks = Vec::new();

        if retain_active_pane {
            let current_pane_close = current_pane.update(cx, |pane, cx| {
                pane.close_other_items(&CloseOtherItems { save_intent: None, close_pinned: false }, None, window, cx)
            });

            tasks.push(current_pane_close);
        }

        for pane in self.panes() {
            if retain_active_pane && pane.entity_id() == current_pane.entity_id() {
                continue;
            }

            let close_pane_items = pane.update(cx, |pane: &mut Pane, cx| {
                pane.close_all_items(&CloseAllItems { save_intent: Some(save_intent), close_pinned: false }, window, cx)
            });

            tasks.push(close_pane_items);
        }

        if tasks.is_empty() {
            None
        } else {
            Some(cx.spawn_in(window, async move |_, _| {
                for task in tasks {
                    task.await?;
                }
                Ok(())
            }))
        }
    }

    // If a dock panel is zoomed, focus it instead of the center pane.
    // Otherwise, focusing the center pane triggers dismiss_zoomed_items_to_reveal
    // which closes the zoomed dock. Docks are hidden in Git History mode.
    pub fn fallback_focus_handle(&self, window: &Window, cx: &App) -> FocusHandle {
        self.all_docks()
            .into_iter()
            .filter(|_| self.mode == WorkspaceMode::Editor)
            .find_map(|dock| {
                let dock = dock.read(cx);
                if !dock.is_open() {
                    return None;
                }
                let panel = dock.active_panel()?;
                panel.is_zoomed(window, cx).then(|| panel.activation_focus_handle(cx))
            })
            .unwrap_or_else(|| self.shown_pane().read(cx).focus_handle(cx))
    }

    fn flush_deferred_saves(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let deferred = mem::take(&mut self.deferred_save_items);
        for weak_item in deferred {
            let Some(item) = weak_item.upgrade() else {
                continue;
            };
            // Skip if focus returned to this item
            let focus_handle = item.item_focus_handle(cx);
            if focus_handle.contains_focused(window, cx) {
                continue;
            }
            Pane::autosave_item(item.as_ref(), self.project.clone(), window, cx).detach_and_log_err(cx);
        }
    }

    pub(crate) fn active_item_path_changed(
        &mut self,
        focus_changed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(Event::ActiveItemChanged);
        let active_entry = self.active_project_path(cx);
        let active_project_path_changed = self.last_active_project_path.as_ref() != active_entry.as_ref();
        self.project.update(cx, |project, cx| project.set_active_path(active_entry.clone(), cx));

        // Infer the active repository only from singleton items.
        // A multibuffer's active path represents the cursor's location within
        // an aggregate view, so we assume it's not the user's intent to switch
        // repositories.
        if focus_changed
            && let Some(project_path) = &active_entry
            && self.active_item(cx).is_some_and(|item| item.buffer_kind(cx) == ItemBufferKind::Singleton)
        {
            let git_store_entity = self.project.read(cx).git_store().clone();
            git_store_entity.update(cx, |git_store, cx| {
                git_store.set_active_repo_for_path(project_path, cx);
            });
        }

        if active_project_path_changed {
            match active_entry.as_ref() {
                None => self.last_active_project_path = None,
                Some(path)
                    if !self.is_deleted_history_path(path, cx) && self.remember_navigation_history_path(path, cx) =>
                {
                    self.serialize_workspace(window, cx);
                }
                Some(_) => {}
            }
        }

        self.update_window_title(window, cx);
    }

    /// Whether this workspace may write the platform window's title and edited
    /// indicator: only the window's root may, so a workspace that was replaced
    /// in its window but is still alive can't clobber its successor's title.
    fn owns_window_chrome(&self, window: &Window) -> bool {
        window.root::<Workspace>().flatten().is_none_or(|root| root.entity_id() == self.weak_self.entity_id())
    }

    fn update_window_title(&mut self, window: &mut Window, cx: &App) {
        if !self.owns_window_chrome(window) {
            return;
        }
        self.apply_window_title(window, cx);
    }

    fn apply_window_title(&mut self, window: &mut Window, cx: &App) {
        let project = self.project().read(cx);
        let active_project_path = self.active_item(cx).and_then(|item| item.project_path(cx));
        let file_name = active_project_path.as_ref().and_then(|project_path| {
            project_path.path.file_name().map(str::to_owned).or_else(|| {
                Some(project.worktree_for_id(project_path.worktree_id, cx)?.read(cx).root_name_str().to_owned())
            })
        });
        let title = window_title(&project_window_title(project, cx), file_name.as_deref());

        let document_path = active_project_path.as_ref().and_then(|path| project.absolute_path(path, cx));
        window.set_document_path(document_path.as_deref());

        if let Some(last_title) = self.last_window_title.as_ref()
            && &title == last_title
        {
            return;
        }
        window.set_window_title(&title);
        self.last_window_title = Some(title);
    }

    fn is_window_edited(&self) -> bool {
        !self.dirty_items.is_empty()
    }

    fn update_window_edited(&mut self, window: &mut Window, _cx: &App) {
        if !self.owns_window_chrome(window) {
            return;
        }
        let is_edited = self.is_window_edited();
        if is_edited != self.window_edited {
            self.window_edited = is_edited;
            window.set_window_edited(self.window_edited);
        }
    }

    /// Re-applies this workspace's title and edited indicator to the platform
    /// window, bypassing the change-detection caches.
    ///
    /// A workspace that replaces another in the same platform window inherits
    /// that window's title and edited state. The `last_window_title`/
    /// `window_edited` caches assume those reflect *this* workspace, so clearing
    /// them forces the values to be re-applied.
    ///
    /// This is only ever invoked for the workspace that just became the root, so
    /// it writes directly rather than going through `update_window_title` /
    /// `update_window_edited`, whose change-detection caches would otherwise
    /// suppress the write when this workspace's computed title/edited state
    /// matches what *it* last set (even though the shared window currently
    /// shows a different workspace's state).
    pub fn refresh_window_state(&mut self, window: &mut Window, cx: &mut App) {
        self.last_window_title = None;
        self.apply_window_title(window, cx);

        self.window_edited = self.is_window_edited();
        window.set_window_edited(self.window_edited);
    }

    fn update_item_dirty_state(&mut self, item: &dyn ItemHandle, window: &mut Window, cx: &mut App) {
        let is_dirty = item.is_dirty(cx);
        let item_id = item.item_id();
        let was_dirty = self.dirty_items.contains_key(&item_id);
        if is_dirty == was_dirty {
            return;
        }
        if was_dirty {
            self.dirty_items.remove(&item_id);
            self.update_window_edited(window, cx);
            return;
        }

        let workspace = self.weak_handle();
        // Not the typed handle: updating it would lease the root workspace,
        // which may be this one.
        let window_handle = window.window_handle();
        let on_release_callback = Box::new(move |cx: &mut App| {
            window_handle
                .update(cx, |_, window, cx| {
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.dirty_items.remove(&item_id);
                            workspace.update_window_edited(window, cx);
                        })
                        .ok();
                })
                .ok();
        });

        let s = item.on_release(cx, on_release_callback);
        self.dirty_items.insert(item_id, s);
        self.update_window_edited(window, cx);
    }

    fn render_notifications(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Div> {
        if self.notifications.is_empty() {
            None
        } else {
            Some(
                div()
                    .absolute()
                    .right_3()
                    .bottom_3()
                    .w_112()
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap_2()
                    .children(
                        self.notifications.iter().map(|(_, notification)| notification.clone().into_any_element()),
                    ),
            )
        }
    }

    // RPC handlers

    pub fn on_window_activation_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_window_active() {
            self.update_history(cx);
        } else {
            // When window is deactivated, flush any deferred saves since focus has left the window
            self.flush_deferred_saves(window, cx);
            for pane in &self.panes {
                pane.update(cx, |pane, cx| {
                    if let Some(item) = pane.active_item() {
                        item.workspace_deactivated(window, cx);
                    }
                    for item in pane.items() {
                        Pane::autosave_item(item.as_ref(), self.project.clone(), window, cx).detach_and_log_err(cx);
                    }
                });
            }
        }
    }

    pub fn key_context(&self, cx: &App) -> KeyContext {
        let mut context = KeyContext::new_with_defaults();
        context.add("Workspace");
        context.set("keyboard_layout", cx.keyboard_layout().name().to_owned());
        if self.left_dock.read(cx).is_open()
            && let Some(active_panel) = self.left_dock.read(cx).active_panel()
        {
            context.set("left_dock", active_panel.panel_key());
        }

        if self.bottom_dock.read(cx).is_open()
            && let Some(active_panel) = self.bottom_dock.read(cx).active_panel()
        {
            context.set("bottom_dock", active_panel.panel_key());
        }

        context
    }

    pub fn actions(&self, div: Div, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let active_item_is_read_only = self.active_item(cx).is_some_and(|item| !item.capability(cx).editable());

        self.add_workspace_actions_listeners(div, window, cx)
            .on_action(
                cx.listener(|workspace, _: &GoBack, window, cx| workspace.go_back(window, cx).detach_and_log_err(cx)),
            )
            .on_action(cx.listener(|workspace, _: &GoForward, window, cx| {
                workspace.go_forward(window, cx).detach_and_log_err(cx);
            }))
            .on_action(cx.listener(Self::close_inactive_items_and_panes))
            .on_action(cx.listener(Self::close_all_items_and_panes))
            .on_action(cx.listener(Self::close_item_in_all_panes))
            .on_action(cx.listener(Self::save_all))
            .on_action(cx.listener(Self::send_keystrokes))
            .on_action(cx.listener(Self::add_folder_to_project))
            .on_action(cx.listener(|workspace, _: &ReloadProjectFiles, window, cx| {
                workspace.project.read(cx).worktree_store().read(cx).rescan_worktrees(cx).detach_and_prompt_err(
                    "Failed to reload project files",
                    window,
                    cx,
                    |_| None,
                );
            }))
            .on_action(cx.listener(Self::activate_pane_at_index))
            .on_action(cx.listener(Self::move_item_to_pane_at_index))
            .on_action(cx.listener(Self::reopen_last_picker))
            .when(!active_item_is_read_only, |this| {
                this.on_action(cx.listener(|workspace, action: &Save, window, cx| {
                    workspace
                        .save_active_item(action.save_intent.unwrap_or(SaveIntent::Save), window, cx)
                        .detach_and_prompt_err("Failed to save", window, cx, |_| None);
                }))
            })
            .on_action(cx.listener(|workspace, _: &SaveAs, window, cx| {
                workspace.save_active_item(SaveIntent::SaveAs, window, cx).detach_and_prompt_err(
                    "Failed to save",
                    window,
                    cx,
                    |_| None,
                );
            }))
            .on_action(cx.listener(|workspace, _: &ActivatePreviousPane, window, cx| {
                workspace.activate_previous_pane(window, cx);
            }))
            .on_action(
                cx.listener(|workspace, _: &ActivateNextPane, window, cx| workspace.activate_next_pane(window, cx)),
            )
            .on_action(
                cx.listener(|workspace, _: &ActivateLastPane, window, cx| workspace.activate_last_pane(window, cx)),
            )
            .on_action(cx.listener(|workspace, _: &FocusNextPart, window, cx| {
                workspace.move_part_focus(true, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &FocusPreviousPart, window, cx| {
                workspace.move_part_focus(false, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &ActivateNextWindow, _window, cx| workspace.activate_next_window(cx)))
            .on_action(cx.listener(|workspace, _: &ActivatePreviousWindow, _window, cx| {
                workspace.activate_previous_window(cx);
            }))
            .on_action(cx.listener(|workspace, _: &ActivatePaneLeft, window, cx| {
                workspace.activate_pane_in_direction(SplitDirection::Left, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &ActivatePaneRight, window, cx| {
                workspace.activate_pane_in_direction(SplitDirection::Right, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &ActivatePaneUp, window, cx| {
                workspace.activate_pane_in_direction(SplitDirection::Up, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &ActivatePaneDown, window, cx| {
                workspace.activate_pane_in_direction(SplitDirection::Down, window, cx);
            }))
            .on_action(cx.listener(|workspace, action: &MoveItemToPaneInDirection, window, cx| {
                workspace.move_item_to_pane_in_direction(action, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &SwapPaneLeft, _, cx| {
                workspace.swap_pane_in_direction(SplitDirection::Left, cx);
            }))
            .on_action(cx.listener(|workspace, _: &SwapPaneRight, _, cx| {
                workspace.swap_pane_in_direction(SplitDirection::Right, cx);
            }))
            .on_action(cx.listener(|workspace, _: &SwapPaneUp, _, cx| {
                workspace.swap_pane_in_direction(SplitDirection::Up, cx);
            }))
            .on_action(cx.listener(|workspace, _: &SwapPaneDown, _, cx| {
                workspace.swap_pane_in_direction(SplitDirection::Down, cx);
            }))
            .on_action(cx.listener(|workspace, _: &SwapPaneAdjacent, window, cx| {
                const DIRECTION_PRIORITY: [SplitDirection; 4] =
                    [SplitDirection::Down, SplitDirection::Up, SplitDirection::Right, SplitDirection::Left];
                for dir in DIRECTION_PRIORITY {
                    if workspace.find_pane_in_direction(dir, cx).is_some() {
                        workspace.swap_pane_in_direction(dir, cx);
                        workspace.activate_pane_in_direction(dir.opposite(), window, cx);
                        break;
                    }
                }
            }))
            .on_action(cx.listener(|workspace, _: &MovePaneLeft, _, cx| {
                workspace.move_pane_to_border(SplitDirection::Left, cx);
            }))
            .on_action(cx.listener(|workspace, _: &MovePaneRight, _, cx| {
                workspace.move_pane_to_border(SplitDirection::Right, cx);
            }))
            .on_action(
                cx.listener(|workspace, _: &MovePaneUp, _, cx| workspace.move_pane_to_border(SplitDirection::Up, cx)),
            )
            .on_action(cx.listener(|workspace, _: &MovePaneDown, _, cx| {
                workspace.move_pane_to_border(SplitDirection::Down, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleLeftDock, window, cx| {
                this.toggle_dock(DockPosition::Left, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &CycleLeftDock, window, cx| {
                workspace.cycle_dock(DockPosition::Left, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ToggleBottomDock, window, cx| {
                workspace.toggle_dock(DockPosition::Bottom, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &CloseActiveDock, window, cx| {
                if !workspace.close_active_dock(window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &Editor, window, cx| {
                workspace.set_mode(WorkspaceMode::Editor, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &Terminal, window, cx| {
                workspace.show_terminal_mode(true, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &CloseAllDocks, window, cx| {
                workspace.close_all_docks(window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ClearAllNotifications, _, cx| {
                workspace.clear_all_notifications(cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ClearNavigationHistory, window, cx| {
                workspace.clear_navigation_history(window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &SuppressNotification, _, cx| {
                if let Some((notification_id, _)) = workspace.notifications.pop() {
                    workspace.suppress_notification(&notification_id, cx);
                }
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ReopenClosedItem, window, cx| {
                workspace.reopen_closed_item(window, cx).detach();
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ResetActiveDockSize, window, cx| {
                if let Some(dock) = workspace.active_dock(window, cx).cloned() {
                    dock.update(cx, |dock, cx| {
                        dock.reset_panel_sizes(window, cx);
                    });
                }
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ResetOpenDocksSize, window, cx| {
                if !workspace.mode.has_docks() {
                    return;
                }
                for dock in workspace.all_docks() {
                    if dock.read(cx).visible_panel().is_some() {
                        dock.update(cx, |dock, cx| {
                            dock.reset_panel_sizes(window, cx);
                        });
                    }
                }
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, _: &ResetPaneSizes, _window, cx| {
                workspace.reset_pane_sizes(cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, act: &IncreaseActiveDockSize, window, cx| {
                adjust_active_dock_size_by_px(px_with_ui_font_fallback(act.px, cx), workspace, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, act: &DecreaseActiveDockSize, window, cx| {
                adjust_active_dock_size_by_px(px_with_ui_font_fallback(act.px, cx) * -1., workspace, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, act: &IncreaseOpenDocksSize, window, cx| {
                adjust_open_docks_size_by_px(px_with_ui_font_fallback(act.px, cx), workspace, window, cx);
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, act: &DecreaseOpenDocksSize, window, cx| {
                adjust_open_docks_size_by_px(px_with_ui_font_fallback(act.px, cx) * -1., workspace, window, cx);
            }))
            .on_action(cx.listener(Workspace::toggle_editor_zoom))
            .on_action(cx.listener(|workspace: &mut Workspace, action: &pane::ActivateNextItem, window, cx| {
                if let Some(active_dock) = workspace.active_dock(window, cx) {
                    let dock = active_dock.read(cx);
                    if let Some(active_panel) = dock.active_panel()
                        && active_panel.pane(cx).is_none()
                    {
                        let mut recent_pane: Option<Entity<Pane>> = None;
                        let mut recent_timestamp = 0;
                        for pane_handle in workspace.panes() {
                            let pane = pane_handle.read(cx);
                            for entry in pane.activation_history() {
                                if entry.timestamp > recent_timestamp {
                                    recent_timestamp = entry.timestamp;
                                    recent_pane = Some(pane_handle.clone());
                                }
                            }
                        }

                        if let Some(pane) = recent_pane {
                            let wrap_around = action.wrap_around;
                            pane.update(cx, |pane, cx| {
                                let current_index = pane.active_item_index();
                                let items_len = pane.items_len();
                                if items_len > 0 {
                                    let next_index = if current_index + 1 < items_len {
                                        current_index + 1
                                    } else if wrap_around {
                                        0
                                    } else {
                                        return;
                                    };
                                    pane.activate_item(next_index, false, false, window, cx);
                                }
                            });
                            return;
                        }
                    }
                }
                cx.propagate();
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, action: &pane::ActivatePreviousItem, window, cx| {
                if let Some(active_dock) = workspace.active_dock(window, cx) {
                    let dock = active_dock.read(cx);
                    if let Some(active_panel) = dock.active_panel()
                        && active_panel.pane(cx).is_none()
                    {
                        let mut recent_pane: Option<Entity<Pane>> = None;
                        let mut recent_timestamp = 0;
                        for pane_handle in workspace.panes() {
                            let pane = pane_handle.read(cx);
                            for entry in pane.activation_history() {
                                if entry.timestamp > recent_timestamp {
                                    recent_timestamp = entry.timestamp;
                                    recent_pane = Some(pane_handle.clone());
                                }
                            }
                        }

                        if let Some(pane) = recent_pane {
                            let wrap_around = action.wrap_around;
                            pane.update(cx, |pane, cx| {
                                let current_index = pane.active_item_index();
                                let items_len = pane.items_len();
                                if items_len > 0 {
                                    let prev_index = if current_index > 0 {
                                        current_index - 1
                                    } else if wrap_around {
                                        items_len.saturating_sub(1)
                                    } else {
                                        return;
                                    };
                                    pane.activate_item(prev_index, false, false, window, cx);
                                }
                            });
                            return;
                        }
                    }
                }
                cx.propagate();
            }))
            .on_action(cx.listener(|workspace: &mut Workspace, action: &pane::CloseActiveItem, window, cx| {
                if let Some(active_dock) = workspace.active_dock(window, cx) {
                    let dock = active_dock.read(cx);
                    if let Some(active_panel) = dock.active_panel()
                        && active_panel.pane(cx).is_none()
                    {
                        let active_pane = workspace.active_pane().clone();
                        active_pane.update(cx, |pane, cx| {
                            pane.close_active_item(action, window, cx).detach_and_log_err(cx);
                        });
                        return;
                    }
                }
                cx.propagate();
            }))
            .on_action(cx.listener(|workspace, _: &FocusCenterPane, window, cx| {
                workspace.focus_center_pane(window, cx);
            }))
            .on_action(cx.listener(Workspace::cancel))
    }

    pub fn register_action<A: Action>(
        &mut self,
        callback: impl Fn(&mut Self, &A, &mut Window, &mut Context<Self>) + 'static,
    ) -> &mut Self {
        let callback = Arc::new(callback);

        self.workspace_actions.push(Box::new(move |div, _, _, cx| {
            let callback = Arc::clone(&callback);
            div.on_action(cx.listener(move |workspace, event, window, cx| (callback)(workspace, event, window, cx)))
        }));
        self
    }
    pub fn register_action_renderer(
        &mut self,
        callback: impl Fn(Div, &Workspace, &mut Window, &mut Context<Self>) -> Div + 'static,
    ) -> &mut Self {
        self.workspace_actions.push(Box::new(callback));
        self
    }

    fn add_workspace_actions_listeners(&self, mut div: Div, window: &mut Window, cx: &mut Context<Self>) -> Div {
        for action in &self.workspace_actions {
            div = (action)(div, self, window, cx);
        }
        div
    }

    pub fn has_active_modal(&self, _: &mut Window, cx: &mut App) -> bool {
        self.modal_layer.read(cx).has_active_modal()
    }

    pub fn active_modal<V: ManagedView + 'static>(&self, cx: &App) -> Option<Entity<V>> {
        self.modal_layer.read(cx).active_modal()
    }

    /// Toggles a modal of type `V`. If a modal of the same type is currently active,
    /// it will be hidden. If a different modal is active, it will be replaced with the new one.
    /// If no modal is active, the new modal will be shown.
    ///
    /// If closing the current modal fails (e.g., due to `on_before_dismiss` returning
    /// `DismissDecision::Dismiss(false)` or `DismissDecision::Pending`), the new modal
    /// will not be shown.
    pub fn toggle_modal<V: ModalView, B>(&mut self, window: &mut Window, cx: &mut App, build: B)
    where
        B: FnOnce(&mut Window, &mut Context<V>) -> V,
    {
        self.modal_layer.update(cx, |modal_layer, cx| modal_layer.toggle_modal(window, cx, build));
    }

    pub fn hide_modal(&mut self, window: &mut Window, cx: &mut App) -> bool {
        self.modal_layer.update(cx, |modal_layer, cx| modal_layer.hide_modal(window, cx))
    }

    fn reopen_last_picker(&mut self, _: &ReopenLastPicker, window: &mut Window, cx: &mut Context<Self>) {
        // When triggered from within another modal (e.g. the command palette), that
        // modal's dismissal is asynchronous, so defer the reveal until it has closed;
        // otherwise a modal would still be active and the reveal would be a no-op.
        cx.defer_in(window, |workspace, window, cx| {
            workspace.modal_layer.update(cx, |modal_layer, cx| {
                modal_layer.reveal_stashed_modal(window, cx);
            });
        });
    }

    pub fn toggle_status_toast<V: ToastView>(&mut self, entity: Entity<V>, cx: &mut App) {
        self.toast_layer.update(cx, |toast_layer, cx| toast_layer.toggle_toast(cx, entity));
    }

    pub fn toggle_editor_zoom(&mut self, _: &ToggleEditorZoom, window: &mut Window, cx: &mut Context<Self>) {
        if self.zoomed.is_some() {
            self.active_pane.update(cx, |pane, cx| {
                pane.set_zoomed(false, cx);
            });
            self.zoomed = None;
            self.zoomed_position = None;
            cx.emit(Event::ZoomChanged);
        }

        if let Some(maximized) = self.maximized_pane.take()
            && maximized.upgrade().as_ref() == Some(&self.active_pane)
        {
            cx.notify();
            return;
        }

        self.maximized_pane = Some(self.active_pane.downgrade());
        window.focus(&self.active_pane.focus_handle(cx), cx);
        cx.notify();
    }

    /// Returns the currently-visible major window regions ("parts"), in a stable
    /// cyclic order: title bar, left dock, editor, right dock, bottom dock,
    /// status bar. Closed docks are skipped. Used by
    /// [`FocusNextPart`]/[`FocusPreviousPart`] so keyboard users can move
    /// between regions without a mouse.
    fn focusable_parts(&self, cx: &App) -> Vec<FocusablePart> {
        // The interactive focus target inside an open dock: the active panel, or
        // the dock itself as a fallback.
        fn dock_content_handle(dock: &Entity<Dock>, cx: &App) -> FocusHandle {
            let dock = dock.read(cx);
            dock.active_panel().map_or_else(|| dock.focus_handle(cx), |panel| panel.activation_focus_handle(cx))
        }

        let dock_part = |dock: &Entity<Dock>| {
            dock.read(cx).is_open().then(|| FocusablePart::landmark(dock_content_handle(dock, cx)))
        };

        let mut parts = Vec::new();
        if self.titlebar_item.is_some() {
            // Region navigation lands on the title bar's first control rather
            // than the toolbar container.
            parts.push(FocusablePart::toolbar(self.titlebar_focus_handle.clone()));
        }
        parts.extend(dock_part(&self.left_dock));

        let center_pane = self
            .last_active_center_pane
            .as_ref()
            .and_then(gpui::WeakEntity::upgrade)
            .unwrap_or_else(|| self.center.first_pane());
        parts.push(FocusablePart::landmark(center_pane.read(cx).focus_handle(cx)));

        parts.extend(dock_part(&self.bottom_dock));
        // Region navigation lands on the status bar's first control rather than
        // the toolbar container.
        parts.push(FocusablePart::toolbar(self.status_bar.read(cx).focus_handle(cx)));
        parts
    }

    /// Moves focus to the next (or previous) visible window region. See
    /// [`FocusNextPart`].
    fn move_part_focus(&self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let parts = self.focusable_parts(cx);
        if parts.is_empty() {
            return;
        }
        let current = parts.iter().position(|part| part.contains_focused(window, cx));
        let next_index = match current {
            Some(index) if forward => (index + 1) % parts.len(),
            Some(index) => (index + parts.len() - 1) % parts.len(),
            None => 0,
        };
        let part = &parts[next_index];
        match &part.behavior {
            PartBehavior::Toolbar => {
                // Focus rests on the first control, not the container. Focusing the tab-group container
                // and advancing descends into its first control; if the toolbar
                // has no focusable control, restore focus to the container so
                // navigation doesn't escape into an unrelated region.
                let container = part.container.clone();
                window.focus(&container, cx);
                window.focus_next(cx);
                let landed_inside = window.focused(cx).is_some_and(|handle| container.contains(&handle, window));
                if !landed_inside {
                    window.focus(&container, cx);
                }
            }
            PartBehavior::Landmark => {
                window.focus(&part.container, cx);
            }
        }
        cx.notify();
    }

    /// Moves focus between the interactive controls within the title bar
    /// toolbar in response to arrow keys. Navigation is clamped to the title
    /// bar so arrows move between items and stop at the ends; Tab is still used to leave the toolbar.
    fn move_titlebar_item_focus(&self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let previous = window.focused(cx);
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
        let landed_in_titlebar =
            window.focused(cx).is_some_and(|handle| self.titlebar_focus_handle.contains(&handle, window));
        // If Tab navigation wandered out of the toolbar, restore the previous
        // item so the ends of the toolbar act as stops rather than exits.
        if !landed_in_titlebar && let Some(previous) = previous {
            window.focus(&previous, cx);
        }
        cx.notify();
    }

    pub fn for_window(window: &Window, _cx: &App) -> Option<Entity<Workspace>> {
        window.root::<Workspace>().flatten()
    }

    /// Stops the project's language servers when the workspace leaves its window,
    /// so they don't outlive it if anything still holds the project.
    fn stop_language_servers(&self, cx: &mut App) {
        self.project.read(cx).lsp_store().update(cx, |lsp_store, cx| lsp_store.stop_all_language_servers(cx));
    }

    /// Closes this workspace's window once it consents (save prompts).
    pub fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(window_handle) = window.window_handle().downcast::<Workspace>() else {
            log::error!("cannot close a window whose root is not a Workspace");
            return;
        };
        cx.spawn(async move |_, cx| {
            if !prepare_window_to_close(window_handle, CloseIntent::CloseWindow, cx)
                .await
                .context("preparing the window to close")?
            {
                return anyhow::Ok(());
            }

            flush_windows_serialization(&[window_handle], cx).await;

            window_handle
                .update(cx, |workspace, window, cx| {
                    workspace.stop_language_servers(cx);
                    window.remove_window();
                })
                .context("removing the closed window")?;

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    pub const fn zoomed_item(&self) -> Option<&AnyWeakView> {
        self.zoomed.as_ref()
    }

    pub fn activate_next_window(&mut self, cx: &mut Context<Self>) {
        let Some(current_window_id) = cx.active_window().map(|a| a.window_id()) else {
            return;
        };
        let windows = cx.windows();
        let next_window = windows.iter().cycle().skip_while(|window| window.window_id() != current_window_id).nth(1);

        if let Some(window) = next_window {
            window.update(cx, |_, window, _| window.activate_window()).ok();
        }
    }

    pub fn activate_previous_window(&mut self, cx: &mut Context<Self>) {
        let Some(current_window_id) = cx.active_window().map(|a| a.window_id()) else {
            return;
        };
        let windows = cx.windows();
        let prev_window =
            windows.iter().rev().cycle().skip_while(|window| window.window_id() != current_window_id).nth(1);

        if let Some(window) = prev_window {
            window.update(cx, |_, window, _| window.activate_window()).ok();
        }
    }

    pub fn cancel(&mut self, _: &menu::Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if cx.stop_active_drag(window) {
        } else if let Some((notification_id, _)) = self.notifications.pop() {
            dismiss_app_notification(&notification_id, cx);
        } else {
            cx.propagate();
        }
    }
}

fn project_window_title(project: &Project, cx: &App) -> String {
    let mut title = String::new();

    for (index, worktree) in project.visible_worktrees(cx).enumerate() {
        let name = worktree.read(cx).root_name_str();
        if index > 0 {
            title.push_str(", ");
        }
        title.push_str(name);
    }

    if title.is_empty() {
        // Keep the default untitled-window text instead of showing a blank title.
        "empty project".to_owned()
    } else {
        title
    }
}

fn window_bounds_env_override() -> Option<Bounds<Pixels>> {
    ED_WINDOW_POSITION.zip(*ED_WINDOW_SIZE).map(|(position, size)| Bounds { origin: position, size })
}

/// A focusable major window region used by region navigation
/// ([`FocusNextPart`]/[`FocusPreviousPart`]).
struct FocusablePart {
    /// The region's focus handle. For landmark regions this is the interactive
    /// content; for toolbars it is the toolbar container.
    container: FocusHandle,
    behavior: PartBehavior,
}

enum PartBehavior {
    /// A toolbar (title bar, status bar). Region navigation focuses the
    /// container and descends to its first control.
    Toolbar,
    /// A dock or the editor. Region navigation focuses its content directly.
    Landmark,
}

impl FocusablePart {
    const fn toolbar(container: FocusHandle) -> Self {
        Self { container, behavior: PartBehavior::Toolbar }
    }

    const fn landmark(content: FocusHandle) -> Self {
        Self { container: content, behavior: PartBehavior::Landmark }
    }

    /// Whether focus currently lies within this region.
    fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        self.container.contains_focused(window, cx)
    }
}

/// Makes the workspace built by `build` the root of `window`. Returns the new
/// workspace.
pub fn replace_window_workspace(
    window: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<Workspace>) -> Workspace,
) -> Entity<Workspace> {
    let workspace = window.replace_root(cx, build);
    workspace.update(cx, |workspace, cx| {
        // The platform window still shows the previous workspace's title and
        // edited state.
        workspace.refresh_window_state(window, cx);
        let focus_handle = workspace.fallback_focus_handle(window, cx);
        window.focus(&focus_handle, cx);
    });
    workspace
}

impl Focusable for Workspace {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_pane.focus_handle(cx)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        static FIRST_PAINT: AtomicBool = AtomicBool::new(true);
        if FIRST_PAINT.swap(false, Ordering::Relaxed) {
            log::info!("Rendered first frame");
        }

        let ui_font = font::setup_ui_font(window, cx);

        let theme = cx.theme();
        let colors = theme.colors();
        let notification_entities =
            self.notifications.iter().map(|(_, notification)| notification.entity_id()).collect::<Vec<_>>();
        let key_context = self.key_context(cx);
        let root = self
            .actions(h_flex(), window, cx)
            .key_context(key_context)
            .relative()
            .size_full()
            .font(ui_font.clone())
            .text_color(colors.text)
            .on_action(cx.listener(Self::close_window));

        let content = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font(ui_font)
            .gap_0()
            .justify_start()
            .items_start()
            .text_color(colors.text)
            .overflow_hidden()
            .on_modifiers_changed(move |_, _, cx| {
                for &id in &notification_entities {
                    cx.notify(id);
                }
            })
            .child(
                div()
                    .size_full()
                    .relative()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("workspace")
                            .bg(colors.background)
                            .relative()
                            .flex_1()
                            .w_full()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(self.mode_view(self.mode).clone().cached(StyleRefinement::default().size_full()))
                            .children(self.render_notifications(window, cx)),
                    )
                    .when(self.status_bar_visible(cx), |parent| {
                        // Cached, so editor frames don't re-render it; a cached view needs a definite size.
                        let height = StatusBar::height(window);
                        parent.child(
                            self.status_bar.clone().cached(StyleRefinement::default().flex_none().w_full().h(height)),
                        )
                    })
                    .child(self.toast_layer.clone()),
            );

        root.child(div().flex().flex_1().size_full().overflow_hidden().child(content))
            .child(self.modal_layer.clone())
            // Painted over the chrome's edge, so picking a colour moves nothing.
            .when_some(self.border_color, |root, color| {
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .when(!window.is_fullscreen(), |border| border.rounded(WINDOW_CORNER_RADIUS))
                        .border_2()
                        .border_color(theme.accents().color_for_index(color)),
                )
            })
    }
}

/// Bounds every new window opens with on the display `display_id` (the
/// primary display when `None`): the environment override, else the
/// display's whole visible area. Sized directly rather than zoomed, since
/// macOS zooms a not-yet-shown window against the wrong screen. Windows don't
/// remember their bounds.
pub fn new_window_bounds(display_id: Option<DisplayId>, cx: &App) -> WindowBounds {
    let display = display_id.and_then(|id| cx.find_display(id)).or_else(|| cx.primary_display());
    WindowBounds::Windowed(window_bounds_or_fill(
        window_bounds_env_override(),
        display.map(|display| display.visible_bounds()),
    ))
}

/// `env_override`, else the display's `visible` area, else the default size.
fn window_bounds_or_fill(env_override: Option<Bounds<Pixels>>, visible: Option<Bounds<Pixels>>) -> Bounds<Pixels> {
    env_override.or(visible).unwrap_or_else(|| Bounds::new(Point::default(), DEFAULT_WINDOW_SIZE))
}

actions!(
    ed,
    [
        /// Opens the ed log file.
        OpenLog,
        /// Reveals the ed log file in the system file manager.
        RevealLogInFileManager
    ]
);

pub fn workspace_windows(cx: &App) -> Vec<WindowHandle<Workspace>> {
    cx.windows().into_iter().filter_map(|window| window.downcast::<Workspace>()).collect()
}

pub async fn prepare_windows_to_quit(workspace_windows: &[WindowHandle<Workspace>], cx: &mut AsyncApp) -> bool {
    // If the user cancels any save prompt, then keep the app open.
    let mut prepared_windows = Vec::new();
    let mut cancelled = false;
    for window in workspace_windows {
        match prepare_window_to_close(*window, CloseIntent::Quit, cx).await {
            Ok(true) => prepared_windows.push(*window),
            Ok(false) => {
                cancelled = true;
                break;
            }
            Err(error) => {
                log::error!("failed to prepare window {:?} to close before quitting: {error:#}", window.window_id());
                cancelled = true;
                break;
            }
        }
    }

    if cancelled {
        flush_windows_serialization(&prepared_windows, cx).await;
        for window in prepared_windows {
            window
                .update(cx, |_, window, _cx| {
                    window.remove_window();
                })
                .log_err();
        }
        return false;
    }

    // Flush all pending workspace serialization before quitting.
    flush_windows_serialization(workspace_windows, cx).await;

    true
}

pub(crate) async fn prepare_window_to_close(
    window: WindowHandle<Workspace>,
    close_intent: CloseIntent,
    cx: &mut AsyncApp,
) -> Result<bool> {
    let prepared = window
        .update(cx, |workspace, window, cx| {
            if close_intent == CloseIntent::Quit {
                window.activate_window();
            }
            workspace.prepare_to_close(close_intent, window, cx)
        })
        .log_err();
    let Some(prepared) = prepared else {
        return Ok(true);
    };
    let prepared = prepared.await.with_context(|| format!("preparing window {:?} to close", window.window_id()));

    if !matches!(prepared, Ok(true)) {
        window
            .update(cx, |workspace, _, _| {
                workspace.removing = false;
            })
            .log_err();
    }

    prepared
}

fn parse_pixel_position_env_var(value: &str) -> Option<Point<Pixels>> {
    let mut parts = value.split(',');
    let x: usize = parts.next()?.parse().ok()?;
    let y: usize = parts.next()?.parse().ok()?;
    Some(point(px(x as f32), px(y as f32)))
}

fn parse_pixel_size_env_var(value: &str) -> Option<Size<Pixels>> {
    let mut parts = value.split(',');
    let width: usize = parts.next()?.parse().ok()?;
    let height: usize = parts.next()?.parse().ok()?;
    Some(size(px(width as f32), px(height as f32)))
}

pub fn with_active_or_new_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send + 'static,
) {
    if let Some(workspace) = cx.active_window().and_then(|w| w.downcast::<Workspace>()) {
        cx.defer(move |cx| {
            workspace
                .update(cx, |workspace, window, cx| {
                    f(workspace, window, cx);
                })
                .log_err();
        });
    } else {
        let app_state = AppState::global(cx);
        open_new(OpenOptions::default(), app_state, cx, move |workspace, window, cx| f(workspace, window, cx))
            .detach_and_log_err(cx);
    }
}

/// The pane to activate when restoring a pane tree: the zoomed one, since it
/// covers the others, else the one saved as active.
pub(crate) fn restored_active_pane<T>(active: Option<T>, zoomed: Option<T>) -> Option<T> {
    zoomed.or(active)
}

/// A dock is hidden when something other than that dock is zoomed.
fn hidden_by_zoom(zoomed: bool, zoomed_position: Option<DockPosition>, position: DockPosition) -> bool {
    zoomed && zoomed_position != Some(position)
}

#[cfg(test)]
mod tests {
    use ed_actions::mode::Editor;
    use ed_actions::mode::GitHistory;

    use super::WorkspaceMode;

    #[test]
    fn mode_action_names() {
        use gpui::Action as _;

        // The built-in keymap binds `cmd-1` / `cmd-2` by these names.
        assert_eq!(Editor::name_for_type(), "mode::Editor");
        assert_eq!(GitHistory::name_for_type(), "mode::GitHistory");
    }

    #[test]
    fn restored_active_pane_works() {
        use super::restored_active_pane;

        assert_eq!(restored_active_pane(Some(1), None), Some(1));
        assert_eq!(restored_active_pane(Some(1), Some(2)), Some(2));
        assert_eq!(restored_active_pane(None, Some(2)), Some(2));
        assert_eq!(restored_active_pane::<i32>(None, None), None);
    }

    #[test]
    fn hidden_by_zoom_works() {
        use super::DockPosition;
        use super::hidden_by_zoom;

        assert!(!hidden_by_zoom(false, None, DockPosition::Bottom));
        assert!(hidden_by_zoom(true, None, DockPosition::Bottom));
        assert!(hidden_by_zoom(true, Some(DockPosition::Left), DockPosition::Bottom));
        assert!(!hidden_by_zoom(true, Some(DockPosition::Bottom), DockPosition::Bottom));
    }

    #[test]
    fn status_bar_shown_works() {
        use super::status_bar_shown;

        assert!(status_bar_shown(WorkspaceMode::Editor, false));
        assert!(!status_bar_shown(WorkspaceMode::Editor, true));
        assert!(!status_bar_shown(WorkspaceMode::GitHistory, false));
        assert!(!status_bar_shown(WorkspaceMode::GitHistory, true));
    }

    #[test]
    fn window_title_works() {
        use super::window_title;

        assert_eq!(window_title("editor", Some("main.rs")), "editor — main.rs");
        assert_eq!(window_title("editor", Some("")), "editor");
        assert_eq!(window_title("editor", None), "editor");
    }

    #[test]
    fn window_bounds_or_fill_works() {
        use gpui::Bounds;
        use gpui::DEFAULT_WINDOW_SIZE;
        use gpui::point;
        use gpui::px;
        use gpui::size;

        let env = Bounds::new(point(px(1.), px(2.)), size(px(300.), px(400.)));
        let visible = Bounds::new(point(px(0.), px(25.)), size(px(800.), px(575.)));
        assert_eq!(super::window_bounds_or_fill(Some(env), Some(visible)), env);
        assert_eq!(super::window_bounds_or_fill(None, Some(visible)), visible);
        assert_eq!(super::window_bounds_or_fill(None, None).size, DEFAULT_WINDOW_SIZE);
    }

    #[test]
    fn opens_in_history_pane() {
        assert!(WorkspaceMode::GitHistory.opens_in_history_pane(false));
        assert!(!WorkspaceMode::GitHistory.opens_in_history_pane(true));
        assert!(!WorkspaceMode::Editor.opens_in_history_pane(false));
        assert!(!WorkspaceMode::Editor.opens_in_history_pane(true));
    }

    #[test]
    fn has_docks() {
        assert!(WorkspaceMode::Editor.has_docks());
        assert!(!WorkspaceMode::GitHistory.has_docks());
    }
}
