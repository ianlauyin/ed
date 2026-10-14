pub mod platform;
pub use gpui::GpuSpecs;
use gpui::{App, AppContext as _, Task, Window, actions};
use human_bytes::human_bytes;
use semver::Version;
use serde::Serialize;
use settings_content::app_metadata::{AppCommitSha, AppVersion};
use std::{env, fmt::Display};
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

actions!(
    zed,
    [
        /// Copies system specifications to the clipboard for bug reports.
        CopySystemSpecsIntoClipboard,
    ]
);

#[derive(Clone, Debug, Serialize)]
pub struct SystemSpecs {
    app_version: String,
    os_name: String,
    os_version: String,
    memory: u64,
    architecture: &'static str,
    commit_sha: Option<String>,
    bundle_type: Option<String>,
    gpu_specs: Option<String>,
}

impl SystemSpecs {
    pub fn new(
        window: &mut Window,
        cx: &mut App,
        os_name: String,
        os_version: String,
    ) -> Task<Self> {
        let app_version = AppVersion::global(cx).to_string();
        let system = System::new_with_specifics(
            RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
        );
        let memory = system.total_memory();
        let architecture = env::consts::ARCH;
        let commit_sha = AppCommitSha::try_global(cx).map(|sha| sha.full());
        let bundle_type = bundle_type();

        let gpu_specs = window.gpu_specs().map(|specs| {
            format!(
                "{} || {} || {}",
                specs.device_name, specs.driver_name, specs.driver_info
            )
        });

        cx.background_spawn(async move {
            SystemSpecs {
                app_version,
                bundle_type,
                os_name,
                os_version,
                memory,
                architecture,
                commit_sha,
                gpu_specs,
            }
        })
    }

    pub fn new_stateless(
        app_version: Version,
        app_commit_sha: Option<AppCommitSha>,
        os_name: String,
        os_version: String,
    ) -> Self {
        let system = System::new_with_specifics(
            RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
        );
        let memory = system.total_memory();
        let architecture = env::consts::ARCH;
        let commit_sha = app_commit_sha.map(|sha| sha.full());
        let bundle_type = bundle_type();

        Self {
            app_version: app_version.to_string(),
            os_name,
            os_version,
            memory,
            architecture,
            commit_sha,
            bundle_type,
            gpu_specs: try_determine_available_gpus(),
        }
    }
}

impl Display for SystemSpecs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let os_information = format!("OS: {} {}", self.os_name, self.os_version);
        let app_version_information = format!(
            "Zed: v{} ({}) {}{}",
            self.app_version,
            match &self.commit_sha {
                Some(commit_sha) => commit_sha.to_string(),
                None => String::new(),
            },
            if let Some(bundle_type) = &self.bundle_type {
                format!("({bundle_type})")
            } else {
                "".to_string()
            },
            if cfg!(debug_assertions) {
                "(Taylor's Version)"
            } else {
                ""
            },
        );
        let system_specs = [
            app_version_information,
            os_information,
            format!("Memory: {}", human_bytes(self.memory as f64)),
            format!("Architecture: {}", self.architecture),
        ]
        .into_iter()
        .chain(
            self.gpu_specs
                .as_ref()
                .map(|specs| format!("GPU: {}", specs)),
        )
        .collect::<Vec<String>>()
        .join("\n");

        write!(f, "{system_specs}")
    }
}

fn try_determine_available_gpus() -> Option<String> {
    None
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, Clone)]
pub struct GpuInfo {
    pub device_name: Option<String>,
    pub device_pci_id: u16,
    pub vendor_name: Option<String>,
    pub vendor_pci_id: u16,
    pub driver_version: Option<String>,
    pub driver_name: Option<String>,
}

/// Returns value of `ZED_BUNDLE_TYPE` set at compiletime or else at runtime.
///
/// The compiletime value is used by flatpak since it doesn't seem to have a way to provide a
/// runtime environment variable.
///
/// The runtime value is used by snap since the Zed snaps use release binaries directly, and so
/// cannot have this baked in.
fn bundle_type() -> Option<String> {
    option_env!("ZED_BUNDLE_TYPE")
        .map(|bundle_type| bundle_type.to_string())
        .or_else(|| env::var("ZED_BUNDLE_TYPE").ok())
}
