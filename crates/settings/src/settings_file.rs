use std::sync::Arc;

use fs::Fs;
use gpui::App;
use gpui::ReadGlobal;
use gpui::UpdateGlobal;

use crate::settings_content::SettingsContent;
use crate::settings_store::SettingsStore;

/// Apply a setting change for this session. The legacy helper name is retained
/// for callers; settings are never persisted to a configuration file.
pub fn update_settings_file(
    _fs: Arc<dyn Fs>,
    cx: &App,
    update: impl 'static + Send + FnOnce(&mut SettingsContent, &App),
) {
    cx.spawn(async move |cx| {
        cx.update(|cx| {
            let mut content = SettingsStore::global(cx).settings().clone();
            update(&mut content, cx);
            SettingsStore::update_global(cx, |store, cx| store.set_settings(content, cx));
        });
    })
    .detach();
}
