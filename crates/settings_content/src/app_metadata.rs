//! Application build metadata and upstream remote-server release identifiers.

#![deny(missing_docs)]

use std::{env, sync::LazyLock};

/// Upstream stable remote-server distribution used by this fork.
pub static SERVER_RELEASE_NAME: LazyLock<String> = LazyLock::new(|| "stable".into());
/// Upstream remote-server distribution used by this fork.
pub static SERVER_RELEASE: LazyLock<ServerRelease> = LazyLock::new(|| ServerRelease::Stable);

use gpui::{App, Global};
use semver::Version;

const ZED_DOCS_URL: &str = "https://zed.dev/docs";

/// Application identity used for desktop integration.
pub const APP_ID: &str = "dev.zed.Zed";

/// The Git commit SHA that Zed was built at.
#[derive(Clone, Eq, Debug, PartialEq)]
pub struct AppCommitSha(String);

struct GlobalAppCommitSha(AppCommitSha);

impl Global for GlobalAppCommitSha {}

impl AppCommitSha {
    /// Creates a new [`AppCommitSha`].
    pub fn new(sha: String) -> Self {
        AppCommitSha(sha)
    }

    /// Returns the global [`AppCommitSha`], if one is set.
    pub fn try_global(cx: &App) -> Option<AppCommitSha> {
        cx.try_global::<GlobalAppCommitSha>()
            .map(|sha| sha.0.clone())
    }

    /// Sets the global [`AppCommitSha`].
    pub fn set_global(sha: AppCommitSha, cx: &mut App) {
        cx.set_global(GlobalAppCommitSha(sha))
    }

    /// Returns the full commit SHA.
    pub fn full(&self) -> String {
        self.0.to_string()
    }

    /// Returns the short (7 character) commit SHA.
    pub fn short(&self) -> String {
        self.0.chars().take(7).collect()
    }
}

struct GlobalAppVersion(Version);

impl Global for GlobalAppVersion {}

/// The version of Zed.
pub struct AppVersion;

impl AppVersion {
    /// Load the app version from env.
    pub fn load(
        pkg_version: &str,
        build_id: Option<&str>,
        commit_sha: Option<AppCommitSha>,
    ) -> Version {
        let mut version: Version = if let Ok(from_env) = env::var("ZED_APP_VERSION") {
            from_env.parse().expect("invalid ZED_APP_VERSION")
        } else {
            pkg_version.parse().expect("invalid version in Cargo.toml")
        };
        let mut metadata = String::new();

        if let Some(build_id) = build_id {
            metadata.push_str(build_id);
        }

        if let Some(sha) = commit_sha {
            if !metadata.is_empty() {
                metadata.push('.');
            }
            metadata.push_str(&sha.0);
        }
        if let Ok(build) = semver::BuildMetadata::new(&metadata) {
            version.build = build;
        }

        version
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

/// Upstream remote-server release identifiers, retained for transport compatibility.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ServerRelease {
    /// An upstream development server, used by local transport tests.
    Dev,

    /// The upstream nightly server distribution.
    Nightly,

    /// The upstream preview server distribution.
    Preview,

    /// The upstream stable server distribution.
    #[default]
    Stable,
}

struct GlobalServerRelease(ServerRelease);

impl Global for GlobalServerRelease {}

/// Initializes application version and remote-server metadata.
pub fn init(app_version: Version, cx: &mut App) {
    cx.set_global(GlobalAppVersion(app_version));
    cx.set_global(GlobalServerRelease(*SERVER_RELEASE))
}

/// Initializes application metadata for tests that use a different upstream server release.
pub fn init_test(app_version: Version, server_release: ServerRelease, cx: &mut App) {
    cx.set_global(GlobalAppVersion(app_version));
    cx.set_global(GlobalServerRelease(server_release))
}

/// Returns the documentation URL for a given slug.
pub fn docs_url(slug: &str, _cx: &App) -> String {
    if slug.is_empty() {
        ZED_DOCS_URL.to_string()
    } else {
        format!("{ZED_DOCS_URL}/{slug}")
    }
}

impl ServerRelease {
    /// Returns the upstream remote-server release for this application.
    pub fn global(cx: &App) -> Self {
        cx.global::<GlobalServerRelease>().0
    }

    /// Returns the configured upstream remote-server release, if initialized.
    pub fn try_global(cx: &App) -> Option<Self> {
        cx.try_global::<GlobalServerRelease>()
            .map(|release| release.0)
    }

    /// Returns the programmatic name for this [`ServerRelease`].
    pub fn dev_name(&self) -> &'static str {
        match self {
            ServerRelease::Dev => "dev",
            ServerRelease::Nightly => "nightly",
            ServerRelease::Preview => "preview",
            ServerRelease::Stable => "stable",
        }
    }
}
