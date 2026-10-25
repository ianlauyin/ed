use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, PartialEq, Default, Serialize, Deserialize, Debug)]
pub struct TitleBarSettingsContent {
    /// Whether to show git status indicators on the branch icon in the title bar.
    /// When enabled, the branch icon changes to reflect the current repository
    /// status (e.g. modified, added, deleted, or conflict).
    ///
    /// Default: false
    pub show_branch_status_icon: Option<bool>,
    /// Whether to show the branch name button in the titlebar.
    ///
    /// Default: true
    pub show_branch_name: Option<bool>,
    /// Whether to show the worktree name button in the titlebar.
    ///
    /// Default: true
    pub show_worktree_name: Option<bool>,
    /// Whether to show the project host and name in the titlebar.
    ///
    /// Default: true
    pub show_project_items: Option<bool>,
}
