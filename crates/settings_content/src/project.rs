use std::sync::Arc;

use collections::BTreeMap;
use collections::HashMap;
use gpui::Rgba;
use serde::Deserialize;
use serde::Serialize;

use crate::AllLanguageSettingsContent;
use crate::DelayMs;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LspSettingsMap(pub HashMap<Arc<str>, LspSettings>);

impl IntoIterator for LspSettingsMap {
    type Item = (Arc<str>, LspSettings);
    type IntoIter = std::collections::hash_map::IntoIter<Arc<str>, LspSettings>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSettingsContent {
    #[serde(flatten)]
    pub all_languages: AllLanguageSettingsContent,

    #[serde(flatten)]
    pub worktree: WorktreeSettingsContent,

    /// Configuration for language servers.
    ///
    /// The following settings can be overridden for specific language servers:
    /// - initialization_options
    ///
    /// To override settings for a language, add an entry for that language server's
    /// name to the lsp value.
    /// Default: null
    #[serde(default)]
    pub lsp: LspSettingsMap,
}

/// When to scan content of linked directories.
#[derive(
    Copy, Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum ScanSymlinksSetting {
    /// Always scan symlinked directories
    Always,
    /// Only scan symlinked directories when they've been expanded in the workspace
    #[default]
    Expanded,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorktreeSettingsContent {
    /// Exclude files matching these glob patterns from file scans, file searches,
    /// and the project file tree. Takes precedence over `file_scan_inclusions`.
    ///
    /// Default:
    ///
    /// ```json
    /// {
    ///   "file_scan_exclusions": [
    ///     "**/.git",
    ///     "**/.svn",
    ///     "**/.hg",
    ///     "**/.jj",
    ///     "**/.sl",
    ///     "**/.repo",
    ///     "**/CVS",
    ///     "**/.DS_Store",
    ///     "**/Thumbs.db",
    ///     "**/.classpath",
    ///     "**/.settings"
    ///   ]
    /// }
    /// ```
    ///
    /// Use `"..."` to add patterns without repeating ed's defaults. In project
    /// settings, it extends the user or parent configuration value. Omit
    /// `"..."` to replace the inherited list.
    ///
    /// ```json
    /// {
    ///   "file_scan_exclusions": ["**/node_modules", "..."]
    /// }
    /// ```
    ///
    /// Inherited patterns are inserted at `"..."`, and duplicates keep their first
    /// occurrence.
    pub file_scan_exclusions: Option<Vec<String>>,

    /// Include files matching these glob patterns when scanning, even if ignored
    /// by Git. Note that broad patterns can slow file scanning.
    /// `file_scan_exclusions` takes precedence.
    ///
    /// Default:
    ///
    /// ```json
    /// {
    ///   "file_scan_inclusions": [".env*"]
    /// }
    /// ```
    ///
    /// Use `"..."` to add patterns without repeating ed's defaults. In project
    /// settings, it extends the user or parent configuration value. Omit
    /// `"..."` to replace the inherited list.
    ///
    /// ```json
    /// {
    ///   "file_scan_inclusions": ["**/build/**", "..."]
    /// }
    /// ```
    ///
    /// Inherited patterns are inserted at `"..."`, and duplicates keep their first
    /// occurrence.
    pub file_scan_inclusions: Option<Vec<String>>,

    /// When to scan content of linked directories.
    ///
    /// Default: expanded
    pub scan_symlinks: Option<ScanSymlinksSetting>,

    /// Maximum directory depth to eagerly index outside of git repositories;
    /// contents of directories at this depth or deeper are indexed on demand.
    /// Repositories rooted shallower than this depth are always indexed fully.
    /// In projects that are not rooted at a git repository, repositories directly
    /// inside a root folder activate their git features immediately; deeper ones
    /// activate on first use.
    /// `0` means no limit and activates all git repositories immediately.
    ///
    /// Default: 5
    pub file_scan_depth: Option<u32>,

    /// Treat files and folders matching these glob patterns as hidden, including
    /// files inside matching folders. To hide these entries, run
    /// `project panel: toggle hide hidden` from the command palette or set
    /// `project_panel.hide_hidden` to `true`.
    ///
    /// Default:
    ///
    /// ```json
    /// {
    ///   "hidden_files": ["**/.*"]
    /// }
    /// ```
    ///
    /// Use `"..."` to add patterns without repeating ed's defaults. In project
    /// settings, it extends the user or parent configuration value. Omit
    /// `"..."` to replace the inherited list.
    ///
    /// ```json
    /// {
    ///   "hidden_files": ["**/*.log", "..."],
    ///   "project_panel": {
    ///     "hide_hidden": true
    ///   }
    /// }
    /// ```
    ///
    /// Inherited patterns are inserted at `"..."`, and duplicates keep their first
    /// occurrence.
    pub hidden_files: Option<Vec<String>>,

    /// Treat files matching these glob patterns as read-only when opened. You can
    /// view but not edit them, which is useful for build outputs, external
    /// dependencies, or generated files.
    ///
    /// Default:
    ///
    /// ```json
    /// {
    ///   "read_only_files": []
    /// }
    /// ```
    ///
    /// Use `"..."` to add patterns without repeating ed's defaults. In project
    /// settings, it extends the user or parent configuration value. Omit
    /// `"..."` to replace the inherited list.
    ///
    /// ```json
    /// {
    ///   "read_only_files": ["**/build/**", "..."]
    /// }
    /// ```
    ///
    /// Inherited patterns are inserted at `"..."`, and duplicates keep their first
    /// occurrence.
    pub read_only_files: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub struct LspSettings {
    pub binary: Option<BinarySettings>,
    /// Options passed to the language server at startup.
    ///
    /// Ref: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#initialize
    ///
    /// Consult the documentation for the specific language server to see which settings are supported.
    pub initialization_options: Option<serde_json::Value>,
    /// Language server settings.
    ///
    /// Ref: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#workspace_configuration
    ///
    /// Consult the documentation for the specific language server to see which settings are supported.
    pub settings: Option<serde_json::Value>,
    /// If the server supports sending tasks over LSP extensions,
    /// this setting can be used to enable or disable them in ed.
    /// Default: true
    pub enable_lsp_tasks: bool,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self { binary: None, initialization_options: None, settings: None, enable_lsp_tasks: true }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BinarySettings {
    pub arguments: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
}

/// Common language server settings.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalLspSettingsContent {
    /// Whether to show the LSP servers button in the status bar.
    ///
    /// Default: `true`
    pub button: Option<bool>,
    /// The maximum amount of time to wait for responses from language servers, in seconds.
    /// A value of `0` will result in no timeout being applied (causing all LSP responses to wait indefinitely until completed).
    ///
    /// Default: `120`
    pub request_timeout: Option<u64>,
    /// The maximum line length a buffer may contain before language server features are disabled for the entire buffer.
    ///
    /// Default: `20000`
    pub max_buffer_line_length: Option<u32>,
    /// Settings for language server notifications
    pub notifications: Option<LspNotificationSettingsContent>,
    /// Rules for rendering LSP semantic tokens.
    pub semantic_token_rules: Option<SemanticTokenRules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspNotificationSettingsContent {
    /// Timeout in milliseconds for automatically dismissing language server notifications.
    /// Set to 0 to disable auto-dismiss.
    ///
    /// Default: 5000
    pub dismiss_timeout_ms: Option<u64>,
}

/// Custom rules for rendering LSP semantic tokens.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct SemanticTokenRules {
    pub rules: Vec<SemanticTokenRule>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SemanticTokenRule {
    pub token_type: Option<String>,
    #[serde(default)]
    pub token_modifiers: Vec<String>,
    #[serde(default)]
    pub style: Vec<String>,
    pub foreground_color: Option<Rgba>,
    pub background_color: Option<Rgba>,
    pub underline: Option<SemanticTokenColorOverride>,
    pub strikethrough: Option<SemanticTokenColorOverride>,
    pub font_weight: Option<SemanticTokenFontWeight>,
    pub font_style: Option<SemanticTokenFontStyle>,
}

impl SemanticTokenRule {
    pub const fn no_style_defined(&self) -> bool {
        self.style.is_empty()
            && self.foreground_color.is_none()
            && self.background_color.is_none()
            && self.underline.is_none()
            && self.strikethrough.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SemanticTokenColorOverride {
    InheritForeground(bool),
    Replace(Rgba),
}

#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum SemanticTokenFontWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(
    Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum SemanticTokenFontStyle {
    #[default]
    Normal,
    Italic,
}

#[derive(Default, Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SessionSettingsContent {
    /// Whether or not to restore unsaved buffers on restart.
    ///
    /// If this is true, user won't be prompted whether to save/discard
    /// dirty files when closing the application.
    ///
    /// Default: true
    pub restore_unsaved_buffers: Option<bool>,
    /// Whether or not to skip worktree trust checks.
    /// When trusted, project settings are synchronized automatically,
    /// language and MCP servers are downloaded and started automatically.
    ///
    /// Default: false
    pub trust_all_worktrees: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct GitSettings {
    /// Whether or not to enable git integration.
    ///
    /// Default: true
    #[serde(flatten)]
    pub enabled: Option<GitEnabledSettings>,
    /// Whether or not to show the git gutter.
    ///
    /// Default: tracked_files
    pub git_gutter: Option<GitGutterSetting>,
    /// Sets the debounce threshold (in milliseconds) after which changes are reflected in the git gutter.
    ///
    /// Default: 0
    pub gutter_debounce: Option<u64>,
    /// Which information to show in the branch picker.
    ///
    /// Default: on
    pub branch_picker: Option<BranchPickerSettingsContent>,
    /// File diff settings.
    pub file_diff: Option<FileDiffSettingsContent>,
    /// How hunks are displayed visually in the editor.
    ///
    /// Default: staged_hollow
    pub hunk_style: Option<GitHunkStyleSetting>,
    /// How file paths are displayed in the git gutter.
    ///
    /// Default: file_name_first
    pub path_style: Option<GitPathStyle>,
    /// Whether to show the stage and restore buttons on diff hunks.
    ///
    /// Default: true
    pub show_stage_restore_buttons: Option<bool>,
    /// Directory where git worktrees are created, relative to the repository
    /// working directory.
    ///
    /// When the resolved directory is outside the project root, the
    /// project's directory name is automatically appended so that
    /// sibling repos don't collide. For example, with the default
    /// `"../worktrees"` and a project at `~/code/zed`, worktrees are
    /// created under `~/code/worktrees/zed/`.
    ///
    /// When the resolved directory is inside the project root, no
    /// extra component is added (it's already project-scoped).
    ///
    /// Examples:
    /// - `"../worktrees"` — `~/code/worktrees/<project>/` (default)
    /// - `".git/ed-worktrees"` — `<project>/.git/ed-worktrees/`
    /// - `"my-worktrees"` — `<project>/my-worktrees/`
    ///
    /// Trailing slashes are ignored.
    ///
    /// Default: ../worktrees
    pub worktree_directory: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitEnabledSettings {
    pub disable_git: Option<bool>,
    pub enable_status: Option<bool>,
    pub enable_diff: Option<bool>,
}

impl GitEnabledSettings {
    pub fn is_git_status_enabled(&self) -> bool {
        !self.disable_git.unwrap_or(false) && self.enable_status.unwrap_or(true)
    }

    pub fn is_git_diff_enabled(&self) -> bool {
        !self.disable_git.unwrap_or(false) && self.enable_diff.unwrap_or(true)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum GitGutterSetting {
    /// Show git gutter in tracked files.
    #[default]
    TrackedFiles,
    /// Hide git gutter
    Hide,
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BranchPickerSettingsContent {
    /// Whether to show author name as part of the commit information.
    ///
    /// Default: false
    pub show_author_name: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileDiffSettingsContent {
    /// Whether newly opened file diffs show the full file instead of changes only.
    ///
    /// Default: true
    pub show_full_file: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum GitHunkStyleSetting {
    /// Show unstaged hunks with a filled background and staged hunks hollow.
    #[default]
    StagedHollow,
    /// Show unstaged hunks hollow and staged hunks with a filled background.
    UnstagedHollow,
}

#[derive(Copy, Clone, Debug, PartialEq, Default, Serialize, Deserialize, strum::VariantArray, strum::VariantNames)]
#[serde(rename_all = "snake_case")]
pub enum GitPathStyle {
    /// Show file name first, then path
    #[default]
    FileNameFirst,
    /// Show full path first
    FilePathFirst,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DiagnosticsSettingsContent {
    /// Whether to show the project diagnostics button in the status bar.
    pub button: Option<bool>,

    /// Settings for using LSP pull diagnostics mechanism in ed.
    pub lsp_pull_diagnostics: Option<LspPullDiagnosticsSettingsContent>,

    /// Settings for showing inline diagnostics.
    pub inline: Option<InlineDiagnosticsSettingsContent>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPullDiagnosticsSettingsContent {
    /// Whether to pull for diagnostics or not.
    ///
    /// Default: true
    pub enabled: Option<bool>,
    /// Minimum time to wait before pulling diagnostics from the language server(s).
    /// 0 turns the debounce off.
    ///
    /// Default: 50
    pub debounce_ms: Option<DelayMs>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, Eq)]
pub struct InlineDiagnosticsSettingsContent {
    /// Whether to only show the inline diagnostics after a delay after the
    /// last editor event.
    ///
    /// Default: 150
    pub update_debounce_ms: Option<DelayMs>,
    /// The amount of padding between the end of the source line and the start
    /// of the inline diagnostic in units of columns.
    ///
    /// Default: 4
    pub padding: Option<u32>,
    /// The minimum column to display inline diagnostics. This setting can be
    /// used to horizontally align inline diagnostics at some position. Lines
    /// longer than this value will still push diagnostics further to the right.
    ///
    /// Default: 0
    pub min_column: Option<u32>,

    pub max_severity: Option<DiagnosticSeverityContent>,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, strum::VariantArray, strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverityContent {
    // No diagnostics are shown.
    Off,
    Error,
    Warning,
    Info,
    Hint,
    All,
}
