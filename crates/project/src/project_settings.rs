use std::time::Duration;

use collections::HashMap;
use git::repository::DEFAULT_WORKTREE_DIRECTORY;
use lsp::DEFAULT_LSP_REQUEST_TIMEOUT_SECS;
use lsp::LanguageServerName;
use serde::Deserialize;
use serde::Serialize;
pub use settings::BinarySettings;
pub use settings::LspSettings;
use settings::SemanticTokenRules;
use settings::Settings;

#[derive(Debug, Clone)]
pub struct ProjectSettings {
    /// Configuration for language servers.
    ///
    /// The following settings can be overridden for specific language servers:
    /// - initialization_options
    ///
    /// To override settings for a language, add an entry for that language server's
    /// name to the lsp value.
    /// Default: null
    // todo(settings-follow-up)
    // We should change to use a non content type (settings::LspSettings is a content type)
    // Note: Will either require merging with defaults, which also requires deciding where the defaults come from,
    //       or case by case deciding which fields are optional and which are actually required.
    pub lsp: HashMap<LanguageServerName, settings::LspSettings>,

    /// Common language server settings.
    pub global_lsp_settings: GlobalLspSettings,

    pub diagnostics: DiagnosticsSettings,

    /// Configuration for Git-related features
    pub git: GitSettings,

    /// Configuration for session-related features
    pub session: SessionSettings,
}

settings::register_setting!(ProjectSettings);

#[derive(Copy, Clone, Debug)]
pub struct SessionSettings {
    /// Whether or not to restore unsaved buffers on restart.
    ///
    /// If this is true, user won't be prompted whether to save/discard
    /// dirty files when closing the application.
    ///
    /// Default: true
    pub restore_unsaved_buffers: bool,
    /// Whether or not to skip worktree trust checks.
    /// When trusted, project settings are synchronized automatically,
    /// language and MCP servers are downloaded and started automatically.
    ///
    /// Default: false
    pub trust_all_worktrees: bool,
}

/// Common language server settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalLspSettings {
    /// Whether to show the LSP servers button in the status bar.
    ///
    /// Default: `true`
    pub button: bool,
    /// The maximum amount of time to wait for responses from language servers, in seconds.
    /// A value of `0` will result in no timeout being applied (causing all LSP responses to wait
    /// indefinitely until completed).
    /// This should not be used outside of serialization/de-serialization in favor of get_request_timeout.
    ///
    /// Default: `120`
    pub request_timeout: u64,
    /// The maximum line length a buffer may contain before language server features are disabled for the entire buffer.
    ///
    /// Default: `20000`
    pub max_buffer_line_length: u32,
    pub notifications: LspNotificationSettings,

    /// Rules for highlighting semantic tokens.
    pub semantic_token_rules: SemanticTokenRules,
}

impl Default for GlobalLspSettings {
    fn default() -> Self {
        Self {
            button: true,
            request_timeout: DEFAULT_LSP_REQUEST_TIMEOUT_SECS,
            max_buffer_line_length: 20_000,
            notifications: LspNotificationSettings::default(),
            semantic_token_rules: SemanticTokenRules::default(),
        }
    }
}

impl GlobalLspSettings {
    /// Returns the timeout duration for LSP-related interactions, or Duration::ZERO if no timeout should be applied.
    /// Zero durations are treated as no timeout by language servers, so code using this in an async context can
    /// simply call unwrap_or_default.
    pub const fn get_request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout)
    }
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "source", rename_all = "snake_case")]
pub struct LspNotificationSettings {
    /// Timeout in milliseconds for dismissing language server notifications.
    /// Zero disables automatic dismissal.
    pub dismiss_timeout_ms: Option<u64>,
}

impl Default for LspNotificationSettings {
    fn default() -> Self {
        Self { dismiss_timeout_ms: Some(5000) }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DiagnosticSeverity {
    // No diagnostics are shown.
    Off,
    Error,
    Warning,
    Info,
    Hint,
}

impl DiagnosticSeverity {
    pub const fn into_lsp(self) -> Option<lsp::DiagnosticSeverity> {
        match self {
            DiagnosticSeverity::Off => None,
            DiagnosticSeverity::Error => Some(lsp::DiagnosticSeverity::ERROR),
            DiagnosticSeverity::Warning => Some(lsp::DiagnosticSeverity::WARNING),
            DiagnosticSeverity::Info => Some(lsp::DiagnosticSeverity::INFORMATION),
            DiagnosticSeverity::Hint => Some(lsp::DiagnosticSeverity::HINT),
        }
    }
}

impl From<settings::DiagnosticSeverityContent> for DiagnosticSeverity {
    fn from(severity: settings::DiagnosticSeverityContent) -> Self {
        match severity {
            settings::DiagnosticSeverityContent::Off => DiagnosticSeverity::Off,
            settings::DiagnosticSeverityContent::Error => DiagnosticSeverity::Error,
            settings::DiagnosticSeverityContent::Warning => DiagnosticSeverity::Warning,
            settings::DiagnosticSeverityContent::Info => DiagnosticSeverity::Info,
            settings::DiagnosticSeverityContent::Hint => DiagnosticSeverity::Hint,
            settings::DiagnosticSeverityContent::All => DiagnosticSeverity::Hint,
        }
    }
}

/// Determines the severity of the diagnostic that should be moved to.
#[derive(PartialEq, PartialOrd, Clone, Copy, Debug, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoToDiagnosticSeverity {
    /// Errors
    Error = 3,
    /// Warnings
    Warning = 2,
    /// Information
    Information = 1,
    /// Hints
    Hint = 0,
}

impl From<lsp::DiagnosticSeverity> for GoToDiagnosticSeverity {
    fn from(severity: lsp::DiagnosticSeverity) -> Self {
        match severity {
            lsp::DiagnosticSeverity::ERROR => Self::Error,
            lsp::DiagnosticSeverity::WARNING => Self::Warning,
            lsp::DiagnosticSeverity::INFORMATION => Self::Information,
            lsp::DiagnosticSeverity::HINT => Self::Hint,
            _ => Self::Error,
        }
    }
}

impl GoToDiagnosticSeverity {
    pub const fn min() -> Self {
        Self::Hint
    }

    pub const fn max() -> Self {
        Self::Error
    }
}

/// Allows filtering diagnostics that should be moved to.
#[derive(PartialEq, Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
pub enum GoToDiagnosticSeverityFilter {
    /// Move to diagnostics of a specific severity.
    Only(GoToDiagnosticSeverity),

    /// Specify a range of severities to include.
    Range {
        /// Minimum severity to move to. Defaults no "error".
        #[serde(default = "GoToDiagnosticSeverity::min")]
        min: GoToDiagnosticSeverity,
        /// Maximum severity to move to. Defaults to "hint".
        #[serde(default = "GoToDiagnosticSeverity::max")]
        max: GoToDiagnosticSeverity,
    },
}

impl Default for GoToDiagnosticSeverityFilter {
    fn default() -> Self {
        Self::Range { min: GoToDiagnosticSeverity::min(), max: GoToDiagnosticSeverity::max() }
    }
}

impl GoToDiagnosticSeverityFilter {
    pub fn matches(&self, severity: lsp::DiagnosticSeverity) -> bool {
        let severity: GoToDiagnosticSeverity = severity.into();
        match self {
            Self::Only(target) => *target == severity,
            Self::Range { min, max } => severity >= *min && severity <= *max,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitSettings {
    /// Whether or not git integration is enabled.
    ///
    /// Default: true
    pub enabled: GitEnabledSettings,
    /// Whether or not to show the git gutter.
    ///
    /// Default: tracked_files
    pub git_gutter: settings::GitGutterSetting,
    /// Sets the debounce threshold (in milliseconds) after which changes are reflected in the git gutter.
    ///
    /// Default: 0
    pub gutter_debounce: u64,
    /// Which information to show in the branch picker.
    ///
    /// Default: on
    pub branch_picker: BranchPickerSettings,
    /// How hunks are displayed visually in the editor.
    ///
    /// Default: staged_hollow
    pub hunk_style: settings::GitHunkStyleSetting,
    /// How file paths are displayed in the git gutter.
    ///
    /// Default: file_name_first
    pub path_style: GitPathStyle,
    /// Whether to show the stage and restore buttons on diff hunks.
    ///
    /// Default: true
    pub show_stage_restore_buttons: bool,
    /// Directory where git worktrees are created, relative to the repository
    /// working directory. When the resolved directory is outside the project
    /// root, the project's directory name is automatically appended so that
    /// sibling repos don't collide.
    ///
    /// Default: ../worktrees
    pub worktree_directory: String,
}

#[derive(Clone, Copy, Debug)]
pub struct GitEnabledSettings {
    /// Whether git integration is enabled for showing git status.
    ///
    /// Default: true
    pub status: bool,
    /// Whether git integration is enabled for showing diffs.
    ///
    /// Default: true
    pub diff: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum GitPathStyle {
    #[default]
    FileNameFirst,
    FilePathFirst,
}

impl From<settings::GitPathStyle> for GitPathStyle {
    fn from(style: settings::GitPathStyle) -> Self {
        match style {
            settings::GitPathStyle::FileNameFirst => GitPathStyle::FileNameFirst,
            settings::GitPathStyle::FilePathFirst => GitPathStyle::FilePathFirst,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BranchPickerSettings {
    /// Whether to show author name as part of the commit information.
    ///
    /// Default: false
    #[serde(default)]
    pub show_author_name: bool,
}

impl Default for BranchPickerSettings {
    fn default() -> Self {
        Self { show_author_name: true }
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticsSettings {
    /// Whether to show the project diagnostics button in the status bar.
    pub button: bool,

    /// Settings for using LSP pull diagnostics mechanism in ed.
    pub lsp_pull_diagnostics: LspPullDiagnosticsSettings,

    /// Settings for showing inline diagnostics.
    pub inline: InlineDiagnosticsSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineDiagnosticsSettings {
    /// Whether to only show the inline diagnostics after a delay after the
    /// last editor event.
    ///
    /// Default: 150
    pub update_debounce_ms: u64,
    /// The amount of padding between the end of the source line and the start
    /// of the inline diagnostic in units of columns.
    ///
    /// Default: 4
    pub padding: u32,
    /// The minimum column to display inline diagnostics. This setting can be
    /// used to horizontally align inline diagnostics at some position. Lines
    /// longer than this value will still push diagnostics further to the right.
    ///
    /// Default: 0
    pub min_column: u32,

    pub max_severity: Option<DiagnosticSeverity>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LspPullDiagnosticsSettings {
    /// Whether to pull for diagnostics or not.
    ///
    /// Default: true
    pub enabled: bool,
    /// Minimum time to wait before pulling diagnostics from the language server(s).
    /// 0 turns the debounce off.
    ///
    /// Default: 50
    pub debounce_ms: u64,
}

impl Settings for ProjectSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let project = &content.project.clone();
        let diagnostics = content.diagnostics.as_ref().unwrap();
        let lsp_pull_diagnostics = diagnostics.lsp_pull_diagnostics.as_ref().unwrap();
        let inline_diagnostics = diagnostics.inline.as_ref().unwrap();

        let git = content.git.as_ref().unwrap();
        let git_enabled = {
            GitEnabledSettings {
                status: git.enabled.as_ref().unwrap().is_git_status_enabled(),
                diff: git.enabled.as_ref().unwrap().is_git_diff_enabled(),
            }
        };
        let git_settings = GitSettings {
            enabled: git_enabled,
            git_gutter: git.git_gutter.unwrap(),
            gutter_debounce: git.gutter_debounce.unwrap_or_default(),
            branch_picker: {
                let branch_picker = git.branch_picker.unwrap();
                BranchPickerSettings { show_author_name: branch_picker.show_author_name.unwrap() }
            },
            hunk_style: git.hunk_style.unwrap(),
            path_style: git.path_style.unwrap().into(),
            show_stage_restore_buttons: git.show_stage_restore_buttons.unwrap_or(true),
            worktree_directory: git
                .worktree_directory
                .clone()
                .unwrap_or_else(|| DEFAULT_WORKTREE_DIRECTORY.to_string()),
        };
        Self {
            lsp: project.lsp.clone().into_iter().map(|(key, value)| (LanguageServerName(key.into()), value)).collect(),
            global_lsp_settings: GlobalLspSettings {
                button: content.global_lsp_settings.as_ref().unwrap().button.unwrap(),
                request_timeout: content.global_lsp_settings.as_ref().unwrap().request_timeout.unwrap(),
                max_buffer_line_length: content.global_lsp_settings.as_ref().unwrap().max_buffer_line_length.unwrap(),
                notifications: LspNotificationSettings {
                    dismiss_timeout_ms: content
                        .global_lsp_settings
                        .as_ref()
                        .unwrap()
                        .notifications
                        .as_ref()
                        .unwrap()
                        .dismiss_timeout_ms,
                },
                semantic_token_rules: content
                    .global_lsp_settings
                    .as_ref()
                    .unwrap()
                    .semantic_token_rules
                    .as_ref()
                    .unwrap()
                    .clone(),
            },
            diagnostics: DiagnosticsSettings {
                button: diagnostics.button.unwrap(),
                lsp_pull_diagnostics: LspPullDiagnosticsSettings {
                    enabled: lsp_pull_diagnostics.enabled.unwrap(),
                    debounce_ms: lsp_pull_diagnostics.debounce_ms.unwrap().0,
                },
                inline: InlineDiagnosticsSettings {
                    update_debounce_ms: inline_diagnostics.update_debounce_ms.unwrap().0,
                    padding: inline_diagnostics.padding.unwrap(),
                    min_column: inline_diagnostics.min_column.unwrap(),
                    max_severity: inline_diagnostics.max_severity.map(Into::into),
                },
            },
            git: git_settings,
            session: SessionSettings {
                restore_unsaved_buffers: content.session.unwrap().restore_unsaved_buffers.unwrap(),
                trust_all_worktrees: content.session.unwrap().trust_all_worktrees.unwrap(),
            },
        }
    }
}
