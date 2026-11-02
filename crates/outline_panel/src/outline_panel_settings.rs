use gpui::Pixels;
pub use settings::DockSide;
pub use settings::FolderIndicator;
use settings::IntoGpui;
pub use settings::Settings;
pub use settings::ShowIndentGuides;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlinePanelSettings {
    pub button: bool,
    pub default_width: Pixels,
    pub dock: DockSide,
    pub file_icons: bool,
    pub folder_indicator: FolderIndicator,
    pub git_status: bool,
    pub indent_size: f32,
    pub indent_guides: IndentGuidesSettings,
    pub auto_reveal_entries: bool,
    pub auto_fold_dirs: bool,
    pub expand_outlines_with_depth: usize,
}

settings::register_setting!(OutlinePanelSettings);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IndentGuidesSettings {
    pub show: ShowIndentGuides,
}

impl Settings for OutlinePanelSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let panel = content.outline_panel.as_ref().unwrap();
        Self {
            button: panel.button.unwrap(),
            default_width: panel.default_width.unwrap().into_gpui(),
            dock: panel.dock.unwrap(),
            file_icons: panel.file_icons.unwrap(),
            folder_indicator: panel.folder_indicator.unwrap(),
            git_status: panel.git_status.unwrap()
                && content.git.as_ref().unwrap().enabled.unwrap().is_git_status_enabled(),
            indent_size: *panel.indent_size.unwrap(),
            indent_guides: IndentGuidesSettings { show: panel.indent_guides.unwrap().show.unwrap() },
            auto_reveal_entries: panel.auto_reveal_entries.unwrap(),
            auto_fold_dirs: panel.auto_fold_dirs.unwrap(),
            expand_outlines_with_depth: panel.expand_outlines_with_depth.unwrap(),
        }
    }
}
