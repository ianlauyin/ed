use gpui::Pixels;
use settings::FolderIndicator;
use settings::GitPanelClickBehavior;
use settings::GitPanelCommitEditor;
use settings::GitPanelGroupBy;
use settings::GitPanelSortBy;
use settings::IntoGpui;
use settings::RegisterSetting;
use settings::Settings;
use settings::StatusStyle;
use workspace::dock::DockPosition;

#[derive(Debug, Clone, PartialEq, RegisterSetting)]
pub struct GitPanelSettings {
    pub button: bool,
    pub dock: DockPosition,
    pub default_width: Pixels,
    pub status_style: StatusStyle,
    pub folder_indicator: FolderIndicator,
    pub fallback_branch_name: String,
    pub sort_by: GitPanelSortBy,
    pub group_by: GitPanelGroupBy,
    pub collapse_untracked_diff: bool,
    pub tree_view: bool,
    pub diff_stats: bool,
    pub commit_title_max_length: usize,
    pub entry_primary_click_action: GitPanelClickBehavior,
    pub commit_editor: GitPanelCommitEditor,
}

impl Settings for GitPanelSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let git_panel = content.git_panel.clone().unwrap();
        Self {
            button: git_panel.button.unwrap(),
            dock: git_panel.dock.unwrap().into(),
            default_width: git_panel.default_width.unwrap().into_gpui(),
            status_style: git_panel.status_style.unwrap(),
            folder_indicator: git_panel.folder_indicator.unwrap(),
            fallback_branch_name: git_panel.fallback_branch_name.unwrap(),
            sort_by: git_panel.sort_by.unwrap(),
            group_by: git_panel.group_by.unwrap(),
            collapse_untracked_diff: git_panel.collapse_untracked_diff.unwrap(),
            tree_view: git_panel.tree_view.unwrap(),
            diff_stats: git_panel.diff_stats.unwrap(),
            commit_title_max_length: git_panel.commit_title_max_length.unwrap(),
            entry_primary_click_action: git_panel.entry_primary_click_action.unwrap(),
            commit_editor: git_panel.commit_editor.unwrap(),
        }
    }
}
