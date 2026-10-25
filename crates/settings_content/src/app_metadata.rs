//! Application build metadata.

#![deny(missing_docs)]

use gpui::App;

const ZED_DOCS_URL: &str = "https://zed.dev/docs";

/// Application identity used for desktop integration.
pub const APP_ID: &str = "dev.zed.Zed";

/// Returns the documentation URL for a given slug.
pub fn docs_url(slug: &str, _cx: &App) -> String {
    if slug.is_empty() { ZED_DOCS_URL.to_string() } else { format!("{ZED_DOCS_URL}/{slug}") }
}
