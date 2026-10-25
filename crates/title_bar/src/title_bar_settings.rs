use settings::Settings;
use settings::SettingsContent;

#[derive(Copy, Clone, Debug)]
pub struct TitleBarSettings {
    pub show_branch_status_icon: bool,
    pub show_branch_name: bool,
    pub show_worktree_name: bool,
    pub show_project_items: bool,
}

settings::register_setting!(TitleBarSettings);

impl Settings for TitleBarSettings {
    fn from_settings(s: &SettingsContent) -> Self {
        let content = s.title_bar.clone().unwrap();
        TitleBarSettings {
            show_branch_status_icon: content.show_branch_status_icon.unwrap(),
            show_branch_name: content.show_branch_name.unwrap(),
            show_worktree_name: content.show_worktree_name.unwrap(),
            show_project_items: content.show_project_items.unwrap(),
        }
    }
}
