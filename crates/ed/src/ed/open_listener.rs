use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use fs::Fs;
use futures::channel::mpsc;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use git_ui::multi_diff_view::MultiDiffView;
use git_ui_core::file_diff_view::FileDiffView;
use gpui::App;
use gpui::AsyncApp;
use gpui::Global;
use gpui::WindowHandle;
use recent_projects::RemoteSettings;
use recent_projects::navigate_to_positions;
use remote::RemoteConnectionOptions;
use settings::Settings;
use util::ResultExt;
use util::paths::PathWithPosition;
use workspace::AppState;
use workspace::MultiWorkspace;
use workspace::OpenResult;
use workspace::item::ItemHandle;

#[derive(Default, Debug)]
pub struct OpenRequest {
    pub open_paths: Vec<String>,
    pub diff_paths: Vec<[String; 2]>,
    pub diff_all: bool,

    pub remote_connection: Option<RemoteConnectionOptions>,
}

impl OpenRequest {
    pub fn parse(request: RawOpenRequest, cx: &App) -> Result<Self> {
        let mut this = Self { diff_paths: request.diff_paths, diff_all: request.diff_all, ..Default::default() };

        for url in request.urls {
            if let Some(file) = url.strip_prefix("file://") {
                this.parse_file_path(file)
            } else if url.starts_with("ssh://") {
                this.parse_ssh_file_path(&url, cx)?
            } else {
                log::error!("unhandled url: {}", url);
            }
        }

        Ok(this)
    }

    fn parse_file_path(&mut self, file: &str) {
        if let Some(decoded) = urlencoding::decode(file).log_err() {
            self.open_paths.push(decoded.into_owned())
        }
    }

    fn parse_ssh_file_path(&mut self, file: &str, cx: &App) -> Result<()> {
        let url = parse_ssh_url(file)?;
        let host = match url.host().with_context(|| format!("missing host in ssh url: {url}"))? {
            url::Host::Domain(host) => host.to_string(),
            url::Host::Ipv4(host) => host.to_string(),
            url::Host::Ipv6(host) => host.to_string(),
        };
        let username =
            if url.username().is_empty() { None } else { Some(urlencoding::decode(url.username())?.into_owned()) };
        let port = url.port();
        anyhow::ensure!(self.open_paths.is_empty(), "cannot open both local and ssh paths");
        anyhow::ensure!(
            url.password().is_none(),
            "SSH password authentication is unsupported; configure non-interactive authentication"
        );
        let connection_options = RemoteSettings::get_global(cx).connection_options_for(host, port, username);

        let connection_options = RemoteConnectionOptions::Ssh(connection_options);
        if let Some(ssh_connection) = &self.remote_connection {
            anyhow::ensure!(*ssh_connection == connection_options, "cannot open multiple different remote connections");
        }
        self.remote_connection = Some(connection_options);
        self.parse_file_path(url.path());
        Ok(())
    }
}

fn parse_ssh_url(url: &str) -> Result<url::Url> {
    if let Ok(url) = url::Url::parse(url) {
        return Ok(url);
    }
    // SCP/git style urls use ':' to separate from Authority and Path.
    // They are unsupported by Url::parse, but can be normalized into a Url.
    //   SCPUrl("ssh://user@host:~/relpath") => Url("ssh://user@host/~/relpath")
    //   SCPUrl("ssh://user@host:/abs/path") => Url("ssh://user@host/abs/path")
    //   SCPUrl("ssh://[2600::]:~/foo") => Url("ssh://[2600::]/~/foo")
    let ssh_target = url.strip_prefix("ssh://").with_context(|| format!("invalid ssh url: {url}"))?;

    let (authority, path) = if let Some((authority, path)) = ssh_target.rsplit_once(":~/") {
        (authority, format!("/~/{path}"))
    } else if let Some((authority, path)) = ssh_target.rsplit_once(":/") {
        (authority, format!("/{path}"))
    } else {
        anyhow::bail!("invalid ssh url: {url}");
    };

    let (userinfo, host) =
        authority.rsplit_once('@').map_or((None, authority), |(userinfo, host)| (Some(userinfo), host));
    anyhow::ensure!(!host.is_empty() && url::Host::parse(host).is_ok(), "invalid ssh url: {url}");

    let normalized_authority = if let Some(userinfo) = userinfo {
        let (username, colon_password) = if let Some((username, password)) = userinfo.split_once(':') {
            (
                urlencoding::encode(&urlencoding::decode(username)?).into_owned(),
                format!(":{}", urlencoding::encode(&urlencoding::decode(password)?).into_owned()),
            )
        } else {
            (urlencoding::encode(&urlencoding::decode(userinfo)?).into_owned(), String::new())
        };
        format!("{username}{colon_password}@{host}")
    } else {
        authority.to_string()
    };

    Ok(url::Url::parse(&format!("ssh://{normalized_authority}{path}"))?)
}

#[derive(Clone)]
pub struct OpenListener(UnboundedSender<RawOpenRequest>);

#[derive(Default)]
pub struct RawOpenRequest {
    pub urls: Vec<String>,
    pub diff_paths: Vec<[String; 2]>,
    pub diff_all: bool,
}

impl Global for OpenListener {}

impl OpenListener {
    pub fn new() -> (Self, UnboundedReceiver<RawOpenRequest>) {
        let (tx, rx) = mpsc::unbounded();
        (OpenListener(tx), rx)
    }

    pub fn open(&self, request: RawOpenRequest) {
        self.0.unbounded_send(request).context("no listener for open requests").log_err();
    }
}

pub async fn open_paths_with_positions(
    path_positions: &[PathWithPosition],
    diff_paths: &[[String; 2]],
    diff_all: bool,
    app_state: Arc<AppState>,
    open_options: workspace::OpenOptions,
    cx: &mut AsyncApp,
) -> Result<(WindowHandle<MultiWorkspace>, Vec<Option<Result<Box<dyn ItemHandle>>>>)> {
    let paths = path_positions.iter().map(|path_with_position| path_with_position.path.clone()).collect::<Vec<_>>();

    let OpenResult { window: multi_workspace, opened_items: mut items, .. } =
        cx.update(|cx| workspace::open_paths(&paths, app_state.clone(), open_options, cx)).await?;

    if diff_all && !diff_paths.is_empty() {
        let mut diff_pairs = Vec::with_capacity(diff_paths.len());
        for diff_pair in diff_paths {
            let parsed = derive_paths_with_position(app_state.fs.as_ref(), diff_pair).await;
            let (Some(old_parsed), Some(new_parsed)) = (parsed.first(), parsed.get(1)) else {
                continue;
            };
            diff_pairs
                .push([old_parsed.path.to_string_lossy().into_owned(), new_parsed.path.to_string_lossy().into_owned()]);
        }
        if let Ok(diff_view) = multi_workspace.update(cx, |multi_workspace, window, cx| {
            multi_workspace
                .workspace()
                .update(cx, |workspace, cx| MultiDiffView::open(diff_pairs, workspace, window, cx))
        }) && let Some(diff_view) = diff_view.await.log_err()
        {
            items.push(Some(Ok(Box::new(diff_view))));
        }
    } else {
        let workspace_weak =
            multi_workspace.read_with(cx, |multi_workspace, _cx| multi_workspace.workspace().downgrade())?;
        let canonicalize = async |parsed: &PathWithPosition| {
            app_state
                .fs
                .canonicalize(&parsed.path)
                .await
                .with_context(|| format!("opening --diff path {:?}", parsed.path))
        };
        for diff_pair in diff_paths {
            let parsed = derive_paths_with_position(app_state.fs.as_ref(), diff_pair).await;
            let (Some(old_parsed), Some(new_parsed)) = (parsed.first(), parsed.get(1)) else {
                continue;
            };
            let (old_path, new_path) = match futures::join!(canonicalize(old_parsed), canonicalize(new_parsed)) {
                (Ok(old), Ok(new)) => (old, new),
                (old, new) => {
                    for result in [old, new] {
                        if let Err(err) = result {
                            items.push(Some(Err(err)));
                        }
                    }
                    continue;
                }
            };
            let target_position = new_parsed.row.map(|row| {
                language::Point::new(row.saturating_sub(1), new_parsed.column.unwrap_or(0).saturating_sub(1))
            });
            if let Ok(diff_view) = multi_workspace.update(cx, |_multi_workspace, window, cx| {
                FileDiffView::open(old_path, new_path, target_position, workspace_weak.clone(), window, cx)
            }) && let Some(diff_view) = diff_view.await.log_err()
            {
                items.push(Some(Ok(Box::new(diff_view))))
            }
        }
    }

    for (item, path) in items.iter_mut().zip(&paths) {
        if let Some(Err(error)) = item {
            *error = anyhow!("error opening {path:?}: {error:#}");
        }
    }

    let items_for_navigation =
        items.iter().map(|item| item.as_ref().and_then(|r| r.as_ref().ok()).cloned()).collect::<Vec<_>>();
    navigate_to_positions(&multi_workspace, items_for_navigation, path_positions, cx);

    Ok((multi_workspace, items))
}

/// Open options for an open request (e.g. from Finder or a URL), following the
/// `default_open_behavior` setting.
pub(crate) fn open_options_for_request(cx: &App) -> workspace::OpenOptions {
    match workspace::WorkspaceSettings::get_global(cx).default_open_behavior {
        settings::DefaultOpenBehavior::ExistingWindow => workspace::OpenOptions::default(),
        settings::DefaultOpenBehavior::NewWindow => workspace::OpenOptions {
            workspace_matching: workspace::WorkspaceMatching::MatchSubpaths,
            add_dirs_to_sidebar: false,
            ..Default::default()
        },
    }
}

pub async fn derive_paths_with_position(
    fs: &dyn Fs,
    path_strings: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<PathWithPosition> {
    let path_strings: Vec<_> = path_strings.into_iter().collect();
    let mut result = Vec::with_capacity(path_strings.len());
    for path_str in path_strings {
        let original_path = Path::new(path_str.as_ref());
        let mut parsed = PathWithPosition::parse_str(path_str.as_ref());

        // If the unparsed path string actually points to an existing file or directory, use it
        // instead of parsing out the line/col number. This matters for paths whose final
        // component looks like a position suffix, e.g. a folder named `Test (3)` would
        // otherwise be parsed as `Test ` at row 3.
        // Colon : is not valid in NTFS file names, so skip this logic if colon on windows.
        let has_colon = original_path.file_name().and_then(|name| name.to_str()).is_none_or(|name| name.contains(':'));

        if (!has_colon || !false)
            && parsed.row.is_some()
            && parsed.path != original_path
            && (fs.is_file(original_path).await || fs.is_dir(original_path).await)
        {
            parsed = PathWithPosition::from_path(original_path.to_path_buf());
        }

        if let Ok(canonicalized) = fs.canonicalize(&parsed.path).await {
            parsed.path = canonicalized;
        }

        result.push(parsed);
    }
    result
}
