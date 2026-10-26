use std::path::Path;
use std::sync::Arc;

use gpui::App;
use gpui::Global;
use language::LibraryProvider;
use language::LibraryRoot;

/// Library providers registered by languages.
#[derive(Default)]
pub struct LibraryProviders(Vec<Arc<dyn LibraryProvider>>);

impl Global for LibraryProviders {}

impl LibraryProviders {
    pub fn register(provider: Arc<dyn LibraryProvider>, cx: &mut App) {
        cx.default_global::<LibraryProviders>().0.push(provider);
    }
}

pub fn library_root(abs_path: &Path, cx: &App) -> Option<LibraryRoot> {
    cx.try_global::<LibraryProviders>()?.0.iter().find_map(|provider| provider.library_root(abs_path))
}
