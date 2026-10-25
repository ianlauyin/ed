//! Application build metadata.

#![deny(missing_docs)]

use gpui::App;
use gpui::Global;
use semver::Version;

const ZED_DOCS_URL: &str = "https://zed.dev/docs";

/// Application identity used for desktop integration.
pub const APP_ID: &str = "dev.zed.Zed";

struct GlobalAppVersion(Version);

impl Global for GlobalAppVersion {}

/// The version of Zed.
pub struct AppVersion;

impl AppVersion {
    /// Parses the app version from the package version in Cargo.toml.
    pub fn load(pkg_version: &str) -> Version {
        pkg_version.parse().expect("invalid version in Cargo.toml")
    }

    /// Returns the global version number.
    pub fn global(cx: &App) -> Version {
        if cx.has_global::<GlobalAppVersion>() {
            cx.global::<GlobalAppVersion>().0.clone()
        } else {
            Version::new(0, 0, 0)
        }
    }
}

/// Initializes application version metadata.
pub fn init(app_version: Version, cx: &mut App) {
    cx.set_global(GlobalAppVersion(app_version));
}

/// Returns the documentation URL for a given slug.
pub fn docs_url(slug: &str, _cx: &App) -> String {
    if slug.is_empty() { ZED_DOCS_URL.to_string() } else { format!("{ZED_DOCS_URL}/{slug}") }
}
