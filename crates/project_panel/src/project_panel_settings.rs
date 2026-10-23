use gpui::Pixels;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use settings::DockSide;
use settings::FolderIndicator;
use settings::IntoGpui;
use settings::ProjectPanelEntrySpacing;
use settings::ProjectPanelSortMode;
use settings::ProjectPanelSortOrder;
use settings::ProjectPanelTitleTooltipDelay;
use settings::RegisterSetting;
use settings::Settings;
use settings::ShowDiagnostics;
use settings::ShowIndentGuides;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, RegisterSetting)]
pub struct ProjectPanelSettings {
    pub button: bool,
    pub hide_gitignore: bool,
    pub default_width: Pixels,
    pub title_tooltip_delay: ProjectPanelTitleTooltipDelay,
    pub dock: DockSide,
    pub entry_spacing: ProjectPanelEntrySpacing,
    pub file_icons: bool,
    pub folder_indicator: FolderIndicator,
    pub git_status: bool,
    pub indent_size: f32,
    pub indent_guides: IndentGuidesSettings,
    pub sticky_scroll: bool,
    pub auto_reveal_entries: bool,
    pub auto_fold_dirs: bool,
    pub starts_open: bool,
    pub show_diagnostics: ShowDiagnostics,
    pub hide_root: bool,
    pub hide_hidden: bool,
    pub drag_and_drop: bool,
    pub auto_open: AutoOpenSettings,
    pub sort_mode: ProjectPanelSortMode,
    pub sort_order: ProjectPanelSortOrder,
    pub diagnostic_badges: bool,
    pub git_status_indicator: bool,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct IndentGuidesSettings {
    pub show: ShowIndentGuides,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AutoOpenSettings {
    pub on_create: bool,
    pub on_paste: bool,
    pub on_drop: bool,
}

impl AutoOpenSettings {
    #[inline]
    pub const fn should_open_on_create(self) -> bool {
        self.on_create
    }

    #[inline]
    pub const fn should_open_on_paste(self) -> bool {
        self.on_paste
    }

    #[inline]
    pub const fn should_open_on_drop(self) -> bool {
        self.on_drop
    }
}

impl Settings for ProjectPanelSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let project_panel = content.project_panel.clone().unwrap();
        Self {
            button: project_panel.button.unwrap(),
            hide_gitignore: project_panel.hide_gitignore.unwrap(),
            default_width: project_panel.default_width.unwrap().into_gpui(),
            dock: project_panel.dock.unwrap(),
            title_tooltip_delay: project_panel.title_tooltip_delay.unwrap(),
            entry_spacing: project_panel.entry_spacing.unwrap(),
            file_icons: project_panel.file_icons.unwrap(),
            folder_indicator: project_panel.folder_indicator.unwrap(),
            git_status: project_panel.git_status.unwrap()
                && content.git.as_ref().unwrap().enabled.unwrap().is_git_status_enabled(),
            indent_size: *project_panel.indent_size.unwrap(),
            indent_guides: IndentGuidesSettings { show: project_panel.indent_guides.unwrap().show.unwrap() },
            sticky_scroll: project_panel.sticky_scroll.unwrap(),
            auto_reveal_entries: project_panel.auto_reveal_entries.unwrap(),
            auto_fold_dirs: project_panel.auto_fold_dirs.unwrap(),
            starts_open: project_panel.starts_open.unwrap(),
            show_diagnostics: project_panel.show_diagnostics.unwrap(),
            hide_root: project_panel.hide_root.unwrap(),
            hide_hidden: project_panel.hide_hidden.unwrap(),
            drag_and_drop: project_panel.drag_and_drop.unwrap(),
            auto_open: {
                let auto_open = project_panel.auto_open.unwrap();
                AutoOpenSettings {
                    on_create: auto_open.on_create.unwrap(),
                    on_paste: auto_open.on_paste.unwrap(),
                    on_drop: auto_open.on_drop.unwrap(),
                }
            },
            sort_mode: project_panel.sort_mode.unwrap(),
            sort_order: project_panel.sort_order.unwrap(),
            diagnostic_badges: project_panel.diagnostic_badges.unwrap(),
            git_status_indicator: project_panel.git_status_indicator.unwrap(),
        }
    }
}
