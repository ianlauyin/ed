use anyhow::{Context as _, Result, bail};
use chrono::{DateTime, Utc};
use futures::{AsyncReadExt, FutureExt as _, channel::oneshot, future::Shared};
use semver::{Version, VersionReq};
use serde::Deserialize;
use smol::{fs, lock::Mutex};
use std::collections::HashMap;
use std::{
    env,
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    process::Output,
    sync::Arc,
};
use util::ResultExt;

const NODE_CA_CERTS_ENV_VAR: &str = "NODE_EXTRA_CA_CERTS";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NodeBinaryOptions {
    pub use_paths: Option<(PathBuf, PathBuf)>,
}

/// Use this when you need to launch npm as a long-lived process (for example, an agent server),
/// so the invocation uses the selected Node executable and CA certificates.
#[derive(Clone, Debug)]
pub struct NpmCommand {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

pub enum VersionStrategy<'a> {
    /// Install if current version doesn't match pinned version
    Pin(&'a Version),
    /// Install if current version is older than latest version
    Latest(&'a Version),
}

#[derive(Clone)]
pub struct NodeRuntime(Arc<Mutex<NodeRuntimeState>>);

struct NodeRuntimeState {
    instance: Option<SystemNodeRuntime>,
    last_options: Option<NodeBinaryOptions>,
    options: watch::Receiver<Option<NodeBinaryOptions>>,
    shell_env_loaded: Shared<oneshot::Receiver<()>>,
}

impl NodeRuntime {
    pub fn new(
        shell_env_loaded: Option<oneshot::Receiver<()>>,
        options: watch::Receiver<Option<NodeBinaryOptions>>,
    ) -> Self {
        NodeRuntime(Arc::new(Mutex::new(NodeRuntimeState {
            instance: None,
            last_options: None,
            options,
            shell_env_loaded: shell_env_loaded.unwrap_or(oneshot::channel().1).shared(),
        })))
    }

    pub fn unavailable() -> Self {
        Self::new(None, watch::channel(None).1)
    }

    async fn instance(&self) -> Result<SystemNodeRuntime> {
        let mut state = self.0.lock().await;

        let options = loop {
            if let Some(options) = state.options.borrow().as_ref() {
                break options.clone();
            }
            state
                .options
                .changed()
                .await
                .context("Node.js runtime is unavailable: configuration channel closed")?;
        };

        if state.last_options.as_ref() != Some(&options) {
            state.instance.take();
        }
        if let Some(instance) = state.instance.as_ref() {
            return Ok(instance.clone());
        }

        state.shell_env_loaded.clone().await.ok();
        let (node, npm) = match options.use_paths.as_ref() {
            Some((node, npm)) => (Some(node.clone()), Some(npm.clone())),
            None => (None, None),
        };
        // Cache only successes so installing or repairing Node.js can be retried.
        let instance = SystemNodeRuntime::new(node, npm).await?;
        log::info!("using system Node.js: {:?}", instance);
        state.instance = Some(instance.clone());
        state.last_options = Some(options);
        Ok(instance)
    }

    pub async fn binary_path(&self) -> Result<PathBuf> {
        Ok(self.instance().await?.node)
    }

    pub async fn run_npm_subcommand(
        &self,
        directory: Option<&Path>,
        subcommand: &str,
        args: &[&str],
    ) -> Result<Output> {
        self.instance()
            .await?
            .run_npm_subcommand(directory, subcommand, args)
            .await
    }

    pub async fn npm_package_installed_version(
        &self,
        local_package_directory: &Path,
        name: &str,
    ) -> Result<Option<Version>> {
        self.instance()
            .await?
            .npm_package_installed_version(local_package_directory, name)
            .await
    }

    pub async fn npm_command(
        &self,
        prefix_dir: Option<&Path>,
        subcommand: &str,
        args: &[&str],
    ) -> Result<NpmCommand> {
        Ok(self
            .instance()
            .await?
            .npm_command(prefix_dir, subcommand, args))
    }

    pub async fn npm_package_latest_version(&self, name: &str) -> Result<Version> {
        self.npm_package_latest_version_with_requirement(name, None)
            .await
    }

    pub async fn npm_package_latest_version_with_requirement(
        &self,
        name: &str,
        version_requirement: Option<&VersionReq>,
    ) -> Result<Version> {
        let instance = self.instance().await?;
        let output = instance
            .run_npm_subcommand(
                None,
                "info",
                &[
                    name,
                    "--json",
                    "--fetch-retry-mintimeout",
                    "2000",
                    "--fetch-retry-maxtimeout",
                    "5000",
                    "--fetch-timeout",
                    "5000",
                ],
            )
            .await?;

        let info: NpmInfo = deserialize_npm_info_from_response(&output.stdout).map_err(|e| {
            anyhow::anyhow!(
                "failed to parse npm info response: {e}\nstdout: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })?;
        let before = npm_config_before(&instance)
            .await
            .context("getting npm before config")
            .log_err()
            .flatten();
        let latest_dist_tag = info.dist_tags.latest.clone();
        let selected_version =
            select_npm_package_version(name, info, before.as_deref(), version_requirement)?;
        log::debug!(
            "selected latest npm package version package={name:?} version_requirement={version_requirement:?} before={before:?} dist_tag_latest={latest_dist_tag:?} selected={selected_version}"
        );
        Ok(selected_version)
    }

    pub async fn npm_install_packages(
        &self,
        directory: &Path,
        packages: &[(&str, &str)],
    ) -> Result<()> {
        if packages.is_empty() {
            return Ok(());
        }

        log::debug!(
            "installing npm packages directory={} packages={packages:?}",
            directory.display()
        );

        let packages: Vec<_> = packages
            .iter()
            .map(|(name, version)| format!("{name}@{version}"))
            .collect();

        let arguments: Vec<_> = packages
            .iter()
            .map(|p| p.as_str())
            .chain([
                "--no-package-lock",
                "--save-exact",
                "--fetch-retry-mintimeout",
                "2000",
                "--fetch-retry-maxtimeout",
                "5000",
                "--fetch-timeout",
                "5000",
            ])
            .collect();

        // This is also wrong because the directory is wrong.
        self.run_npm_subcommand(Some(directory), "install", &arguments)
            .await?;
        Ok(())
    }

    pub async fn npm_install_latest_packages(
        &self,
        directory: &Path,
        package_names: &[&str],
    ) -> Result<()> {
        // Let npm apply user config such as `before` and `min-release-age` during resolution.
        log::debug!(
            "installing latest npm packages directory={} packages={package_names:?}",
            directory.display()
        );
        let packages = package_names
            .iter()
            .map(|package_name| (*package_name, "latest"))
            .collect::<Vec<_>>();
        self.npm_install_packages(directory, &packages).await
    }

    pub async fn should_install_npm_package(
        &self,
        package_name: &str,
        local_executable_path: &Path,
        local_package_directory: &Path,
        version_strategy: VersionStrategy<'_>,
    ) -> bool {
        // In the case of the local system not having the package installed,
        // or in the instances where we fail to parse package.json data,
        // we attempt to install the package.
        if fs::metadata(local_executable_path).await.is_err() {
            log::debug!(
                "npm package cache miss package={package_name:?} reason=missing-executable executable={}",
                local_executable_path.display()
            );
            return true;
        }

        let Some(installed_version) = self
            .npm_package_installed_version(local_package_directory, package_name)
            .await
            .log_err()
            .flatten()
        else {
            log::debug!(
                "npm package cache miss package={package_name:?} reason=missing-installed-version package_dir={}",
                local_package_directory.display()
            );
            return true;
        };

        let version_strategy_label = match &version_strategy {
            VersionStrategy::Pin(version) => format!("pin:{version}"),
            VersionStrategy::Latest(version) => format!("latest:{version}"),
        };
        let should_install =
            should_install_npm_package_version(&installed_version, version_strategy);
        log::debug!(
            "npm package cache check package={package_name:?} installed={installed_version} strategy={version_strategy_label} should_install={should_install}"
        );
        should_install
    }
}

fn should_install_npm_package_version(
    installed_version: &Version,
    version_strategy: VersionStrategy<'_>,
) -> bool {
    match version_strategy {
        VersionStrategy::Pin(pinned_version) => installed_version != pinned_version,
        VersionStrategy::Latest(latest_version) => installed_version < latest_version,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NpmInfo {
    #[serde(default)]
    dist_tags: NpmInfoDistTags,
    versions: Vec<Version>,
    #[serde(default, deserialize_with = "deserialize_npm_info_time")]
    time: HashMap<String, String>,
}

/// Parse NpmInfo from npm info --json output, handling both v11 and >= v12 formats.
fn deserialize_npm_info_from_response(data: &[u8]) -> Result<NpmInfo, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(data)?;

    // npm >= 12 returns an array with one object: [ { ... } ]
    if let serde_json::Value::Array(arr) = &value {
        if arr.len() == 1 {
            return NpmInfo::deserialize(&arr[0]);
        }
    }

    // npm <= v11 returns a bare JSON object: { ... }
    NpmInfo::deserialize(value)
}

#[derive(Debug, Deserialize, Default)]
pub struct NpmInfoDistTags {
    latest: Option<Version>,
}

// Some registries put non-string values in the `time` map: JFrog Artifactory emits
// `"unpublished": null`, and npm itself reports `unpublished` as an object when a
// package has had versions unpublished. Only version keys map to the RFC 3339 strings
// we read, so keep the string entries and drop the rest rather than failing to parse
// the entire `npm info` response (which would block language server installation).
fn deserialize_npm_info_time<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let entries = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(value) => Some((key, value)),
            _ => None,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct NpmConfig {
    #[serde(default)]
    before: Option<String>,
}

async fn npm_config_before(node_runtime: &SystemNodeRuntime) -> Result<Option<String>> {
    // `npm config get before` renders Date values for display. The JSON config output keeps the
    // computed cutoff in the same ISO format used by `npm info --json` release times.
    let output = node_runtime
        .run_npm_subcommand(None, "config", &["list", "--json"])
        .await?;
    let config: NpmConfig = serde_json::from_slice(&output.stdout)?;
    Ok(config
        .before
        .filter(|before| !before.trim().is_empty() && before != "null"))
}

fn select_npm_package_version(
    package_name: &str,
    mut info: NpmInfo,
    before: Option<&str>,
    version_requirement: Option<&VersionReq>,
) -> Result<Version> {
    if let Some(version_requirement) = version_requirement {
        info.versions
            .retain(|version| version_requirement.matches(version));
        info.versions.sort();
        info.dist_tags.latest = info
            .dist_tags
            .latest
            .take()
            .filter(|version| version_requirement.matches(version));
    }

    if let Some(before) = before
        && !info.time.is_empty()
    {
        let before_timestamp = DateTime::parse_from_rfc3339(before)
            .with_context(|| format!("parsing npm before config timestamp {before:?}"))?
            .with_timezone(&Utc);
        let latest_version = info.dist_tags.latest.as_ref();

        if let Some(version) = latest_version
            && npm_version_was_published_before(version, &info.time, &before_timestamp)?
        {
            return Ok(version.clone());
        }

        for version in info.versions.iter().rev() {
            if is_allowed_npm_version_before(
                version,
                latest_version,
                &info.time,
                &before_timestamp,
                version_requirement.is_some(),
            )? {
                return Ok(version.clone());
            }
        }

        bail!("no version found for npm package {package_name} before {before}");
    }

    info.dist_tags
        .latest
        .or_else(|| info.versions.pop())
        .with_context(|| format!("no version found for npm package {package_name}"))
}

fn is_allowed_npm_version_before(
    version: &Version,
    latest_version: Option<&Version>,
    published_at_by_version: &HashMap<String, String>,
    before: &DateTime<Utc>,
    allow_prereleases: bool,
) -> Result<bool> {
    if (!allow_prereleases && !version.pre.is_empty())
        || latest_version.is_some_and(|latest_version| version > latest_version)
    {
        return Ok(false);
    }

    npm_version_was_published_before(version, published_at_by_version, before)
}

fn npm_version_was_published_before(
    version: &Version,
    published_at_by_version: &HashMap<String, String>,
    before: &DateTime<Utc>,
) -> Result<bool> {
    let Some(published_at) = published_at_by_version.get(&version.to_string()) else {
        return Ok(false);
    };
    let published_at = DateTime::parse_from_rfc3339(published_at)
        .with_context(|| format!("parsing npm release timestamp for version {version}"))?
        .with_timezone(&Utc);
    Ok(&published_at <= before)
}

fn path_with_node_binary_prepended(node_binary: &Path) -> Option<OsString> {
    let existing_path = env::var_os("PATH");
    let node_bin_dir = node_binary.parent().map(|dir| dir.as_os_str());
    match (existing_path, node_bin_dir) {
        (Some(existing_path), Some(node_bin_dir)) => {
            if let Ok(joined) = env::join_paths(
                [PathBuf::from(node_bin_dir)]
                    .into_iter()
                    .chain(env::split_paths(&existing_path)),
            ) {
                Some(joined)
            } else {
                Some(existing_path)
            }
        }
        (Some(existing_path), None) => Some(existing_path),
        (None, Some(node_bin_dir)) => Some(node_bin_dir.to_owned()),
        _ => None,
    }
}

/// A system Node.js executable whose version has been checked.
///
/// Discovery does not require npm, initialize a cache, or download a runtime.
#[derive(Debug, Clone)]
pub struct SystemNode {
    path: PathBuf,
    version: Version,
}

/// A failure to locate or validate a system Node.js executable.
#[derive(Debug, thiserror::Error)]
pub enum NodeDiscoveryError {
    #[error("Could not read the current directory while looking for Node.js: {0}")]
    CurrentDirectory(#[source] io::Error),
    #[error(
        "Node.js executable {path:?} was not found. Install Node.js {minimum} or newer, or configure its executable path.",
        minimum = SystemNode::MIN_VERSION,
    )]
    NotFound {
        path: PathBuf,
        #[source]
        source: which::Error,
    },
    #[error(
        "Could not run Node.js at {path:?}: {source}. Check that the configured executable can run."
    )]
    Probe {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "Node.js at {path:?} failed its version check ({status}). Check the Node.js installation.\nstdout: {stdout}\nstderr: {stderr}",
        status = .output.status,
        stdout = String::from_utf8_lossy(&.output.stdout),
        stderr = String::from_utf8_lossy(&.output.stderr),
    )]
    UnsuccessfulExit { path: PathBuf, output: Output },
    #[error(
        "Node.js at {path:?} reported an invalid version: {output:?}. Configure a Node.js {minimum} or newer executable.",
        minimum = SystemNode::MIN_VERSION,
    )]
    InvalidVersion {
        path: PathBuf,
        output: String,
        #[source]
        source: semver::Error,
    },
    #[error(
        "Node.js at {path:?} is too old ({version}). Install Node.js {minimum} or newer, or configure a newer executable.",
        minimum = SystemNode::MIN_VERSION,
    )]
    TooOld { path: PathBuf, version: Version },
}

impl SystemNode {
    const MIN_VERSION: Version = Version::new(22, 0, 0);

    /// Finds and validates Node.js using an explicit executable or search path.
    ///
    /// An explicit executable takes precedence and never falls back to another
    /// candidate on failure. Otherwise the first executable named `node` in
    /// `search_path` is checked, preserving PATH order even if it is too old.
    /// `None` disables PATH lookup; it does not read the process's PATH.
    ///
    /// A configured basename is also looked up in `search_path`; use `./name`
    /// to select one in the current directory. Relative paths containing
    /// separators are resolved against that directory. On Windows, lookup
    /// honors PATHEXT, including batch-file wrappers.
    ///
    /// The caller must finish loading its login-shell environment before calling
    /// this method. The version probe inherits that environment, with PATH
    /// replaced by `search_path` when supplied. Discovery does not launch a login
    /// shell to obtain the environment.
    ///
    /// # Errors
    ///
    /// Returns a typed failure if lookup or execution fails, the version cannot
    /// be parsed, or the executable reports a version older than 22.0.0.
    pub async fn discover(
        configured_path: Option<PathBuf>,
        search_path: Option<OsString>,
    ) -> std::result::Result<Self, NodeDiscoveryError> {
        let path = smol::unblock({
            let search_path = search_path.clone();
            move || {
                find_node_path(
                    configured_path.as_deref(),
                    search_path.as_deref(),
                    Path::new("."),
                )
            }
        })
        .await?;
        let mut command = util::command::new_command(&path);
        command.arg("--version").kill_on_drop(true);
        if let Some(search_path) = search_path {
            command.env("PATH", search_path);
        }
        let output = command
            .output()
            .await
            .map_err(|source| NodeDiscoveryError::Probe {
                path: path.clone(),
                source,
            })?;
        let version = check_node_version(&path, output)?;
        Ok(Self { path, version })
    }

    /// Returns the absolute executable path without resolving symlinks.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the version reported by the executable.
    pub fn version(&self) -> &Version {
        &self.version
    }
}

fn find_node_path(
    configured_path: Option<&Path>,
    search_path: Option<&OsStr>,
    directory: &Path,
) -> std::result::Result<PathBuf, NodeDiscoveryError> {
    let path = configured_path.unwrap_or_else(|| Path::new("node"));
    let path = which::which_in(path, search_path, directory).map_err(|source| {
        NodeDiscoveryError::NotFound {
            path: path.to_path_buf(),
            source,
        }
    })?;
    // The cwd may have been deleted even when Node's absolute path still works.
    // Only relative results need it, but those must be anchored before launch
    // so the command cannot search PATH a second time.
    std::path::absolute(directory.join(path)).map_err(NodeDiscoveryError::CurrentDirectory)
}

fn check_node_version(
    path: &Path,
    output: Output,
) -> std::result::Result<Version, NodeDiscoveryError> {
    if !output.status.success() {
        return Err(NodeDiscoveryError::UnsuccessfulExit {
            path: path.to_path_buf(),
            output,
        });
    }
    let version_string = String::from_utf8_lossy(&output.stdout);
    let version =
        Version::parse(version_string.trim().trim_start_matches('v')).map_err(|source| {
            NodeDiscoveryError::InvalidVersion {
                path: path.to_path_buf(),
                output: version_string.into_owned(),
                source,
            }
        })?;
    if version < SystemNode::MIN_VERSION {
        return Err(NodeDiscoveryError::TooOld {
            path: path.to_path_buf(),
            version,
        });
    }
    Ok(version)
}

#[derive(Debug, Clone)]
struct SystemNodeRuntime {
    node: PathBuf,
    npm: PathBuf,
    scratch_dir: PathBuf,
}

impl SystemNodeRuntime {
    async fn new(node: Option<PathBuf>, npm: Option<PathBuf>) -> Result<Self> {
        let node = SystemNode::discover(node, env::var_os("PATH")).await?.path;
        let npm_path = npm.unwrap_or_else(|| PathBuf::from("npm"));
        let npm = which::which(&npm_path).with_context(|| {
            format!(
                "npm executable {:?} was not found. Install npm alongside system Node.js, or configure `node.npm_path`.",
                npm_path
            )
        })?;
        let scratch_dir = paths::data_dir().join("node");
        fs::create_dir(&scratch_dir).await.ok();
        _ = fs::remove_dir_all(scratch_dir.join("cache")).await;
        fs::create_dir(scratch_dir.join("cache")).await.ok();

        Ok(Self {
            node,
            npm,
            scratch_dir,
        })
    }

    async fn run_npm_subcommand(
        &self,
        directory: Option<&Path>,
        subcommand: &str,
        args: &[&str],
    ) -> anyhow::Result<Output> {
        let npm_command = self.npm_command(directory, subcommand, args);
        let mut command = util::command::new_command(npm_command.path);
        command.args(npm_command.args);
        command.envs(npm_command.env);
        if let Some(directory) = directory {
            command.current_dir(directory);
        }
        let output = command.output().await?;
        anyhow::ensure!(
            output.status.success(),
            "failed to execute npm {subcommand} subcommand:\nstdout: {:?}\nstderr: {:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }

    fn npm_command(
        &self,
        prefix_dir: Option<&Path>,
        subcommand: &str,
        args: &[&str],
    ) -> NpmCommand {
        let command_args = build_npm_command_args(
            prefix_dir,
            &self.scratch_dir.join("cache"),
            subcommand,
            args,
        );
        let command_env = npm_command_env(&self.node);

        NpmCommand {
            path: self.npm.clone(),
            args: command_args,
            env: command_env,
        }
    }

    async fn npm_package_installed_version(
        &self,
        local_package_directory: &Path,
        name: &str,
    ) -> Result<Option<Version>> {
        read_package_installed_version(local_package_directory.join("node_modules"), name).await
        // todo: allow returning a globally installed version (requires callers not to hard-code the path)
    }
}

pub async fn read_package_installed_version(
    node_module_directory: PathBuf,
    name: &str,
) -> Result<Option<Version>> {
    let package_json_path = node_module_directory.join(name).join("package.json");

    let mut file = match fs::File::open(package_json_path).await {
        Ok(file) => file,
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }

            Err(err)?
        }
    };

    #[derive(Deserialize)]
    struct PackageJson {
        version: Version,
    }

    let mut contents = String::new();
    file.read_to_string(&mut contents).await?;
    let package_json: PackageJson = serde_json::from_str(&contents)?;
    Ok(Some(package_json.version))
}

pub async fn read_package_executable(
    node_module_directory: PathBuf,
    name: &str,
) -> Result<PathBuf> {
    let package_directory = node_module_directory.join(name);

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Bin {
        Path(String),
        Named(HashMap<String, String>),
    }

    #[derive(Deserialize)]
    struct PackageJson {
        bin: Option<Bin>,
    }

    let package_json_path = package_directory.join("package.json");
    let mut file = fs::File::open(&package_json_path)
        .await
        .with_context(|| format!("opening {}", package_json_path.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).await?;
    let package_json: PackageJson = serde_json::from_str(&contents)
        .with_context(|| format!("parsing {}", package_json_path.display()))?;

    let relative_path = match package_json.bin {
        Some(Bin::Path(path)) => path,
        Some(Bin::Named(bins)) => {
            let unscoped_name = name.rsplit('/').next().unwrap_or(name);
            let path = if bins.len() == 1 {
                bins.values().next()
            } else {
                bins.get(unscoped_name)
            };
            path.with_context(|| {
                format!("npm package {name} declares no executable named {unscoped_name}")
            })?
            .clone()
        }
        None => bail!("npm package {name} declares no executable"),
    };

    Ok(package_directory.join(relative_path))
}

fn build_npm_command_args(
    prefix_dir: Option<&Path>,
    cache_dir: &Path,
    subcommand: &str,
    args: &[&str],
) -> Vec<String> {
    let mut command_args = Vec::new();
    if let Some(prefix_dir) = prefix_dir {
        command_args.push("--prefix".into());
        command_args.push(prefix_dir.to_string_lossy().into_owned());
    }
    command_args.push(subcommand.to_string());
    command_args.push(format!("--cache={}", cache_dir.display()));
    command_args.extend(args.iter().map(|a| a.to_string()));
    command_args
}

pub fn npm_command_env(node_binary: &Path) -> HashMap<String, String> {
    let mut command_env = HashMap::new();
    let env_path = path_with_node_binary_prepended(node_binary).unwrap_or_default();
    command_env.insert("PATH".into(), env_path.to_string_lossy().into_owned());

    if let Ok(node_ca_certs) = env::var(NODE_CA_CERTS_ENV_VAR) {
        if !node_ca_certs.is_empty() {
            command_env.insert(NODE_CA_CERTS_ENV_VAR.to_string(), node_ca_certs);
        }
    }

    command_env
}
