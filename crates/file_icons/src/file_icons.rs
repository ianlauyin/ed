mod icons;

use std::{path::Path, str};

use gpui::{App, SharedString};
use settings_content::FolderIndicator;
use util::paths::PathExt;

use crate::icons::ICONS;

/// File and folder icons for panels and pickers, from the built-in set in `icons.rs`.
#[derive(Debug)]
pub struct FileIcons;

/// What a panel draws ahead of a directory's name, in render order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FolderIndicators {
    pub chevron: Option<SharedString>,
    pub icon: Option<SharedString>,
}

impl FileIcons {
    pub fn get_icon(path: &Path, cx: &App) -> Option<SharedString> {
        let get_icon_from_suffix = |suffix: &str| -> Option<SharedString> {
            ICONS
                .file_stems
                .get(suffix)
                .or_else(|| ICONS.file_suffixes.get(suffix))
                .and_then(|typ| Self::get_icon_for_type(typ, cx))
        };
        // TODO: Associate a type with the languages and have the file's language
        //       override these associations

        if let Some(mut typ) = path.file_name().and_then(|typ| typ.to_str()) {
            // check if file name is in suffixes
            // e.g. catch file named `eslint.config.js` instead of `.eslint.config.js`
            let maybe_path = get_icon_from_suffix(typ);
            if maybe_path.is_some() {
                return maybe_path;
            }

            let mut first_prefix = None;
            while let Some((prefix, suffix)) = typ.split_once('.') {
                first_prefix.get_or_insert(prefix);
                let maybe_path = get_icon_from_suffix(suffix);
                if maybe_path.is_some() {
                    return maybe_path;
                }
                typ = suffix;
            }
            if let Some(prefix) = first_prefix.and_then(get_icon_from_suffix) {
                // check if stem (before first dot) is in stems
                // e.g. catch file named `Dockerfile.prod` or `Podfile.lock`
                return Some(prefix);
            }
        }

        // handle cases where the file extension is made up of multiple important
        // parts (e.g Component.stories.tsx) that refer to an alternative icon style
        if let Some(suffix) = path.multiple_extensions() {
            let maybe_path = get_icon_from_suffix(suffix.as_str());
            if maybe_path.is_some() {
                return maybe_path;
            }
        }

        // primary case: check if the files extension or the hidden file name
        // matches some icon path
        if let Some(suffix) = path.extension_or_hidden_file_name() {
            let maybe_path = get_icon_from_suffix(suffix);
            if maybe_path.is_some() {
                return maybe_path;
            }
        }

        // this _should_ only happen when the file is hidden (has leading '.')
        // and is not a "special" file we have an icon (e.g. not `.eslint.config.js`)
        // that should be caught above. In the remaining cases, we want to check
        // for a normal supported extension e.g. `.data.json` -> `json`
        let extension = path.extension().and_then(|ext| ext.to_str());
        if let Some(extension) = extension {
            let maybe_path = get_icon_from_suffix(extension);
            if maybe_path.is_some() {
                return maybe_path;
            }
        }
        Self::get_icon_for_type("default", cx)
    }

    pub fn get_icon_for_type(typ: &str, _cx: &App) -> Option<SharedString> {
        ICONS.file_icons.get(typ).cloned()
    }

    pub fn get_folder_icon(expanded: bool, path: &Path, _cx: &App) -> Option<SharedString> {
        let named = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .and_then(|name| ICONS.named_directories.get(name));
        Some(named.unwrap_or(&ICONS.directory).get(expanded))
    }

    pub fn get_chevron_icon(expanded: bool, _cx: &App) -> Option<SharedString> {
        Some(ICONS.chevron.get(expanded))
    }

    /// Resolves what a panel should draw ahead of a directory's name. Shared by every
    /// panel that exposes a `folder_indicator` setting so they stay in agreement.
    pub fn get_folder_indicators(
        indicator: FolderIndicator,
        expanded: bool,
        path: &Path,
        cx: &App,
    ) -> FolderIndicators {
        let chevron = indicator
            .shows_chevron()
            .then(|| Self::get_chevron_icon(expanded, cx))
            .flatten();
        let icon = indicator
            .shows_icon()
            .then(|| Self::get_folder_icon(expanded, path, cx))
            .flatten();

        FolderIndicators { chevron, icon }
    }
}
