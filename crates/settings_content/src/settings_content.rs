pub mod app_metadata;

mod action;
mod editor;
mod language;
mod project;
mod serde_helper;
mod terminal;
mod theme;
mod title_bar;
mod workspace;

pub use action::CommandAliasTarget;
use anyhow::Context;
use collections::HashMap;
pub use editor::*;
pub use language::*;
pub use project::*;
use serde::Deserialize;
use serde::Serialize;
pub use serde_helper::serialize_f32_with_two_decimal_places;
pub use serde_helper::serialize_optional_f32_with_two_decimal_places;
pub use terminal::*;
pub use theme::*;
pub use title_bar::*;
pub use workspace::*;

/// A non-negative size in pixels.
///
/// Valid range: 0.0 and up
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    PartialOrd,
    derive_more::FromStr,
    derive_more::Deref,
    derive_more::From,
)]
#[serde(transparent)]
pub struct PixelSetting(#[serde(serialize_with = "crate::serialize_f32_with_two_decimal_places")] pub f32);

impl std::fmt::Display for PixelSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let rounded = (self.0 * 100.0).round() / 100.0;
        write!(f, "{rounded}")
    }
}

use std::hash::Hash;

pub use util::serde::default_true;

/// Determines when the mouse cursor should be hidden in response to keyboard
/// input.
///
/// Default: on_typing_and_action
#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum HideMouseMode {
    /// Never hide the mouse cursor
    Never,
    /// Hide only when typing
    OnTyping,
    /// Hide on typing and on key bindings that resolve to an action
    #[default]
    OnTypingAndAction,
}

/// Determines whether to reduce non-essential motion in the UI, such as
/// loading spinners and pulsating labels, by rendering them in a static state.
///
/// Default: off
#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum ReduceMotionMode {
    /// Always reduce motion
    On,
    /// Never reduce motion
    #[default]
    Off,
}

#[derive(Debug, PartialEq, Default, Clone, Serialize, Deserialize)]
pub struct SettingsContent {
    #[serde(flatten)]
    pub project: ProjectSettingsContent,

    #[serde(flatten)]
    pub theme: Box<ThemeSettingsContent>,

    #[serde(flatten)]
    pub workspace: WorkspaceSettingsContent,

    #[serde(flatten)]
    pub editor: EditorSettingsContent,

    #[serde(flatten)]
    pub remote: RemoteSettingsContent,

    /// Settings related to the command palette.
    pub command_palette: Option<CommandPaletteSettingsContent>,

    /// Settings related to the file finder.
    pub file_finder: Option<FileFinderSettingsContent>,

    pub call_hierarchy: Option<CallHierarchySettingsContent>,

    pub git_panel: Option<GitPanelSettingsContent>,

    pub tabs: Option<ItemSettingsContent>,
    pub tab_bar: Option<TabBarSettingsContent>,
    pub tab_switcher: Option<TabSwitcherSettingsContent>,
    pub status_bar: Option<StatusBarSettingsContent>,

    pub preview_tabs: Option<PreviewTabsSettingsContent>,

    /// Configuration for Diagnostics-related features.
    pub diagnostics: Option<DiagnosticsSettingsContent>,

    /// Configuration for Git-related features
    pub git: Option<GitSettings>,

    /// Common language server settings.
    pub global_lsp_settings: Option<GlobalLspSettingsContent>,

    /// The settings for the image viewer.
    pub image_viewer: Option<ImageViewerSettingsContent>,

    /// The settings for the markdown preview.
    pub markdown_preview: Option<MarkdownPreviewSettingsContent>,

    /// Determines when the mouse cursor should be hidden in response to
    /// keyboard input. Applies globally across all input surfaces (editors,
    /// terminals, palettes, etc.).
    ///
    /// Default: on_typing_and_action
    pub hide_mouse: Option<HideMouseMode>,

    /// A map of log scopes to the desired log level.
    /// Useful for filtering out noisy logs or enabling more verbose logging.
    ///
    /// Example: {"log": {"client": "warn"}}
    pub log: Option<HashMap<String, String>>,

    pub line_indicator_format: Option<LineIndicatorFormat>,

    pub outline_panel: Option<OutlinePanelSettingsContent>,

    pub project_panel: Option<ProjectPanelSettingsContent>,

    /// Whether to reduce non-essential motion in the UI, such as loading
    /// spinners and pulsating labels, by rendering them in a static state.
    ///
    /// Default: off
    pub reduce_motion: Option<ReduceMotionMode>,

    /// Configuration for session-related features
    pub session: Option<SessionSettingsContent>,

    /// Configuration of the terminal in Zed.
    pub terminal: Option<TerminalSettingsContent>,

    pub title_bar: Option<TitleBarSettingsContent>,

    /// Number of lines to search for modelines at the beginning and end of files.
    /// Modelines contain editor directives (e.g., vim/emacs settings) that configure
    /// the editor behavior for specific files.
    ///
    /// Default: 5
    pub modeline_lines: Option<usize>,
}

impl SettingsContent {
    pub const fn languages_mut(&mut self) -> &mut HashMap<String, LanguageSettingsContent> {
        &mut self.project.all_languages.languages.0
    }
}

pub struct LanguageDefaultsContent {
    pub all_languages: AllLanguageSettingsContent,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum DockPosition {
    Left,
    Bottom,
    Right,
}

#[derive(Clone, PartialEq, Default, Serialize, Deserialize, Debug)]
pub struct GitPanelSettingsContent {
    /// Whether to show the panel button in the status bar.
    ///
    /// Default: true
    pub button: Option<bool>,
    /// Where to dock the panel.
    ///
    /// Default: left
    pub dock: Option<DockPosition>,
    /// Default width of the panel in pixels.
    ///
    /// Default: 360
    pub default_width: Option<PixelSetting>,
    /// How entry statuses are displayed.
    ///
    /// Default: icon
    pub status_style: Option<StatusStyle>,

    /// What to show for directories in the git panel.
    ///
    /// Default: icon
    pub folder_indicator: Option<FolderIndicator>,

    /// What the default branch name should be when
    /// `init.defaultBranch` is not set in git
    ///
    /// Default: main
    pub fallback_branch_name: Option<String>,

    /// How to sort entries in the git panel.
    ///
    /// Default: path
    pub sort_by: Option<GitPanelSortBy>,

    /// How to group entries in the git panel.
    ///
    /// Default: status
    pub group_by: Option<GitPanelGroupBy>,

    /// Whether to collapse untracked files in the diff panel.
    ///
    /// Default: false
    pub collapse_untracked_diff: Option<bool>,

    /// Whether to show entries with tree or flat view in the panel
    ///
    /// Default: false
    pub tree_view: Option<bool>,

    /// Whether to show the addition/deletion change count next to each file in the Git panel.
    ///
    /// Default: true
    pub diff_stats: Option<bool>,

    /// Maximum length of the commit message title before a warning is shown.
    /// Set to 0 to disable.
    ///
    /// Default: 0
    pub commit_title_max_length: Option<usize>,

    /// Default action when clicking a changed file in the Git panel.
    ///
    /// Default: project_diff
    pub entry_primary_click_action: Option<GitPanelClickBehavior>,

    /// Whether the commit message editor is shown in the Git panel by default.
    ///
    /// Default: expanded
    pub commit_editor: Option<GitPanelCommitEditor>,
}

#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum GitPanelCommitEditor {
    /// Show the commit message editor.
    #[default]
    Expanded,
    /// Hide the commit message editor, leaving only the commit button row.
    Collapsed,
}

#[derive(
    Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum GitPanelClickBehavior {
    /// Open the project diff, showing all changed files.
    #[default]
    ProjectDiff,
    /// Open a single-file diff view.
    FileDiff,
    /// Open the file in the editor without a diff view.
    ViewFile,
}

#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum GitPanelSortBy {
    #[default]
    Path,
    Name,
}

#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum GitPanelGroupBy {
    None,
    #[default]
    Status,
    Staging,
}

#[derive(
    Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum StatusStyle {
    #[default]
    Icon,
    LabelColor,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
pub struct CommandPaletteSettingsContent {
    /// Whether to use command history ranking for sorting in the command palette.
    ///
    /// Default: true
    pub use_command_history: Option<bool>,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
pub struct FileFinderSettingsContent {
    /// Whether to show file icons in the file finder.
    ///
    /// Default: true
    pub file_icons: Option<bool>,
    /// Determines how much space the file finder can take up in relation to the available window width.
    ///
    /// Default: small
    pub modal_max_width: Option<ModalWidthContent>,
    /// Determines whether the file finder should skip focus for the active file in search results.
    ///
    /// Default: true
    pub skip_focus_for_active_in_search: Option<bool>,
    /// Whether to use gitignored files when searching.
    /// Only the file Zed had indexed will be used, not necessary all the gitignored files.
    ///
    /// Default: Smart
    pub include_ignored: Option<IncludeIgnoredContent>,
    /// Whether to pre-fill the file finder's query with the text selected in the
    /// focused item (e.g. an editor or a terminal) when it is opened.
    ///
    /// Default: true
    pub prefill_query_from_selection: Option<bool>,
}

#[derive(
    Debug, PartialEq, Eq, Clone, Copy, Default, Serialize, Deserialize, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum IncludeIgnoredContent {
    /// Use all gitignored files
    All,
    /// Use only the files Zed had indexed
    Indexed,
    /// Be smart and search for ignored when called from a gitignored worktree
    #[default]
    Smart,
}

#[derive(
    Debug, PartialEq, Eq, Clone, Copy, Default, Serialize, Deserialize, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "lowercase")]
pub enum ModalWidthContent {
    #[default]
    Small,
    Medium,
    Large,
    XLarge,
    Full,
}

impl ModalWidthContent {
    pub fn to_pixels(self, base_width: gpui::Pixels, window_width: gpui::Pixels) -> gpui::Pixels {
        match self {
            ModalWidthContent::Small => base_width,
            ModalWidthContent::Full => window_width,
            ModalWidthContent::XLarge => (window_width - gpui::px(512.)).max(base_width),
            ModalWidthContent::Large => (window_width - gpui::px(768.)).max(base_width),
            ModalWidthContent::Medium => (window_width - gpui::px(1024.)).max(base_width),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
pub struct CallHierarchySettingsContent {
    /// Determines how much space the call hierarchy picker can take up in relation to the available window width.
    ///
    /// Default: medium
    pub modal_max_width: Option<ModalWidthContent>,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq)]
pub struct OutlinePanelSettingsContent {
    /// Whether to show the outline panel button in the status bar.
    ///
    /// Default: true
    pub button: Option<bool>,
    /// Customize default width (in pixels) taken by outline panel
    ///
    /// Default: 240
    pub default_width: Option<PixelSetting>,
    /// The position of outline panel
    ///
    /// Default: right
    pub dock: Option<DockSide>,
    /// Whether to show file icons in the outline panel.
    ///
    /// Default: true
    pub file_icons: Option<bool>,
    /// What to show for directories in the outline panel.
    ///
    /// Default: icon
    pub folder_indicator: Option<FolderIndicator>,
    /// Whether to show the git status in the outline panel.
    ///
    /// Default: true
    pub git_status: Option<bool>,
    /// Amount of indentation (in pixels) for nested items.
    ///
    /// Default: 20
    pub indent_size: Option<PixelSetting>,
    /// Whether to reveal it in the outline panel automatically,
    /// when a corresponding project entry becomes active.
    /// Gitignored entries are never auto revealed.
    ///
    /// Default: true
    pub auto_reveal_entries: Option<bool>,
    /// Whether to fold directories automatically
    /// when directory has only one directory inside.
    ///
    /// Default: true
    pub auto_fold_dirs: Option<bool>,
    /// Settings related to indent guides in the outline panel.
    pub indent_guides: Option<IndentGuidesSettingsContent>,
    /// Default depth to expand outline items in the current file.
    /// The default depth to which outline entries are expanded on reveal.
    /// - Set to 0 to collapse all items that have children
    /// - Set to 1 or higher to collapse items at that depth or deeper
    ///
    /// Default: 100
    pub expand_outlines_with_depth: Option<usize>,
    /// Whether to hide symbols, excerpts and search matches in the outline panel
    /// when a multi-buffer view (e.g. a diff or search results) is active,
    /// showing only files and directories.
    /// Does not affect single-file views.
    ///
    /// Default: false
    pub multi_buffer_hide_symbols: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum DockSide {
    Left,
    Right,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum ShowIndentGuides {
    Always,
    Never,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IndentGuidesSettingsContent {
    /// When to show the scrollbar in the outline panel.
    pub show: Option<ShowIndentGuides>,
}

#[derive(Clone, Copy, Default, PartialEq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineIndicatorFormat {
    Short,
    #[default]
    Long,
}

/// The settings for the markdown preview.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct MarkdownPreviewSettingsContent {
    /// The name of a font to use for rendering in the markdown preview.
    /// Falls back to the UI font if unset.
    pub font_family: Option<FontFamilyName>,
    /// The name of a font to use for code (code blocks and inline code) in the
    /// markdown preview. Falls back to the buffer font if unset.
    pub code_font_family: Option<FontFamilyName>,
    /// The font size to use for rendering in the markdown preview.
    /// Falls back to the UI font size if unset.
    pub font_size: Option<FontSize>,
    /// The theme to use for the markdown preview.
    /// Falls back to the main editor theme if unset.
    pub theme: Option<ThemeSelection>,
    /// The weight of headings (H1 through H6) in the markdown preview, in CSS
    /// units from 100 to 900.
    ///
    /// Default: 600
    pub heading_font_weight: Option<FontWeightContent>,
    /// Whether to automatically open Markdown files in the preview.
    ///
    /// Default: false
    pub open_markdown_files_in_preview: Option<bool>,
    /// Whether to limit the width of the rendered markdown content. When
    /// enabled, content is constrained to `max_width` and centered
    /// horizontally within the preview pane, for optimal readability.
    ///
    /// Default: true
    pub limit_content_width: Option<bool>,
    /// The maximum width, in pixels, of the rendered markdown content when
    /// `limit_content_width` is enabled.
    ///
    /// Default: 800
    pub max_width: Option<PixelSetting>,
}

/// The settings for the image viewer.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct ImageViewerSettingsContent {
    /// The unit to use for displaying image file sizes.
    ///
    /// Default: "binary"
    pub unit: Option<ImageFileSizeUnit>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum ImageFileSizeUnit {
    /// Displays file size in binary units (e.g., KiB, MiB).
    #[default]
    Binary,
    /// Displays file size in decimal units (e.g., KB, MB).
    Decimal,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RemoteSettingsContent {
    pub ssh_connections: Option<Vec<SshConnection>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SshConnection {
    pub host: String,
    pub username: Option<String>,
    pub port: Option<u16>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub projects: collections::BTreeSet<RemoteProject>,
    /// Name to use for this server in UI.
    pub nickname: Option<String>,
    pub port_forwards: Option<Vec<SshPortForwardOption>>,
    /// Timeout in seconds for the SSH connection.
    /// Defaults to 10 seconds if not specified.
    pub connection_timeout: Option<u16>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct RemoteProject {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct SshPortForwardOption {
    pub local_host: Option<String>,
    pub local_port: u16,
    pub remote_host: Option<String>,
    pub remote_port: u16,
}

/// Settings for configuring the tab switcher behaviour.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TabSwitcherSettingsContent {
    /// When false, files are not shown while cycling through the tab switcher.
    /// Only the confirmed file (when the modifier key is released) is shown and
    /// recorded in the MRU history.
    ///
    /// Default: true
    pub show_file_preview: Option<bool>,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DelayMs(pub u64);

impl From<u64> for DelayMs {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

impl std::fmt::Display for DelayMs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}ms", self.0)
    }
}

impl std::str::FromStr for DelayMs {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim()
            .strip_suffix("ms")
            .unwrap_or(s.trim())
            .parse::<u64>()
            .map(DelayMs)
            .with_context(|| format!("failed to parse delay duration: {s}"))
    }
}
