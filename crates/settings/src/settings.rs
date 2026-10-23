mod action_schema;
mod action_sequence;
mod built_in;
mod built_in_keymap;
mod built_in_semantic_token_rules;
mod content_into_gpui;
mod editable_setting_control;
mod granted_write_path;
mod keymap;
mod settings_file;
mod settings_store;

pub use settings_macros::RegisterSetting;

pub mod settings_content {
    pub use ::settings_content::*;
}

pub mod fallible_options {
    pub use ::settings_content::FallibleOption;
    pub use ::settings_content::parse_json;
}

#[doc(hidden)]
pub mod private {
    pub use inventory;

    pub use crate::settings_store::RegisteredSetting;
    pub use crate::settings_store::SettingValue;
}

use std::fmt;

pub use ::settings_content::*;
pub use action_schema::*;
pub use action_sequence::ActionSequence;
pub use built_in_keymap::keymap as built_in_keymap;
pub use content_into_gpui::IntoGpui;
pub use editable_setting_control::*;
use gpui::App;
pub use granted_write_path::GrantedWritePath;
pub use keymap::KeyBindingSpec;
pub use keymap::built_in_key_bindings;
pub use settings_file::*;
pub use settings_store::DefaultSemanticTokenRules;
pub use settings_store::Settings;
pub use settings_store::SettingsLocation;
pub use settings_store::SettingsStore;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct WorktreeId(usize);

impl From<WorktreeId> for usize {
    fn from(value: WorktreeId) -> Self {
        value.0
    }
}

impl WorktreeId {
    pub const fn from_usize(handle_id: usize) -> Self {
        Self(handle_id)
    }

    pub const fn from_proto(id: u64) -> Self {
        Self(id as usize)
    }

    pub const fn to_proto(self) -> u64 {
        self.0 as u64
    }

    pub const fn to_usize(self) -> usize {
        self.0
    }
}

impl fmt::Display for WorktreeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

pub use built_in::DEFAULT_WINDOW_TITLE_FORMAT;

pub fn init(cx: &mut App) {
    let settings = SettingsStore::new(cx, built_in::settings());
    cx.set_global(settings);
}
