use crate::handle_open_request;
use crate::restore_or_create_workspace;
use anyhow::{Context as _, Result, anyhow};
use cli::{CliRequest, CliResponse, CliResponseSink};
use cli::{IpcHandshake, ipc};
use editor::Editor;
use fs::Fs;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::channel::{mpsc, oneshot};
use futures::future;

use futures::{FutureExt, StreamExt};
use git_ui::multi_diff_view::MultiDiffView;
use git_ui_core::file_diff_view::FileDiffView;
use gpui::{App, AsyncApp, Global, TaskExt, WindowHandle};
use recent_projects::{RemoteSettings, navigate_to_positions, open_remote_project};
use remote::RemoteConnectionOptions;
use settings::Settings;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use util::ResultExt;
use util::debug_panic;
use util::paths::PathWithPosition;
use workspace::PathList;
use workspace::item::ItemHandle;
use workspace::{AppState, MultiWorkspace, OpenOptions, OpenResult, SerializedWorkspaceLocation};

#[derive(Default, Debug)]
pub struct OpenRequest {
    pub kind: Option<OpenRequestKind>,
    pub open_paths: Vec<String>,
    pub diff_paths: Vec<[String; 2]>,
    pub diff_all: bool,

    pub remote_connection: Option<RemoteConnectionOptions>,
    pub open_behavior: Option<cli::OpenBehavior>,
}

pub enum OpenRequestKind {
    CliConnection(
        (
            mpsc::UnboundedReceiver<CliRequest>,
            Box<dyn CliResponseSink>,
        ),
    ),
}

impl std::fmt::Debug for OpenRequestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliConnection(_) => write!(f, "CliConnection(..)"),
        }
    }
}

impl OpenRequest {
    pub fn parse(request: RawOpenRequest, cx: &App) -> Result<Self> {
        let mut this = Self::default();

        this.diff_paths = request.diff_paths;
        this.diff_all = request.diff_all;
        this.open_behavior = request.open_behavior;

        for url in request.urls {
            if let Some(server_name) = url.strip_prefix("ed-cli://") {
                this.kind = Some(OpenRequestKind::CliConnection(connect_to_cli(server_name)?));
            } else if let Some(file) = url.strip_prefix("file://") {
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
        let host = match url
            .host()
            .with_context(|| format!("missing host in ssh url: {url}"))?
        {
            url::Host::Domain(host) => host.to_string(),
            url::Host::Ipv4(host) => host.to_string(),
            url::Host::Ipv6(host) => host.to_string(),
        };
        let username = if url.username().is_empty() {
            None
        } else {
            Some(urlencoding::decode(url.username())?.into_owned())
        };
        let port = url.port();
        anyhow::ensure!(
            self.open_paths.is_empty(),
            "cannot open both local and ssh paths"
        );
        anyhow::ensure!(
            url.password().is_none(),
            "SSH password authentication is unsupported; configure non-interactive authentication"
        );
        let connection_options =
            RemoteSettings::get_global(cx).connection_options_for(host, port, username);

        let connection_options = RemoteConnectionOptions::Ssh(connection_options);
        if let Some(ssh_connection) = &self.remote_connection {
            anyhow::ensure!(
                *ssh_connection == connection_options,
                "cannot open multiple different remote connections"
            );
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
    let ssh_target = url
        .strip_prefix("ssh://")
        .with_context(|| format!("invalid ssh url: {url}"))?;

    let (authority, path) = if let Some((authority, path)) = ssh_target.rsplit_once(":~/") {
        (authority, format!("/~/{path}"))
    } else if let Some((authority, path)) = ssh_target.rsplit_once(":/") {
        (authority, format!("/{path}"))
    } else {
        anyhow::bail!("invalid ssh url: {url}");
    };

    let (userinfo, host) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(userinfo, host)| (Some(userinfo), host));
    anyhow::ensure!(
        !host.is_empty() && url::Host::parse(host).is_ok(),
        "invalid ssh url: {url}"
    );

    let normalized_authority = if let Some(userinfo) = userinfo {
        let (username, colon_password) =
            if let Some((username, password)) = userinfo.split_once(':') {
                (
                    urlencoding::encode(&urlencoding::decode(username)?).into_owned(),
                    format!(
                        ":{}",
                        urlencoding::encode(&urlencoding::decode(password)?).into_owned()
                    ),
                )
            } else {
                (
                    urlencoding::encode(&urlencoding::decode(userinfo)?).into_owned(),
                    String::new(),
                )
            };
        format!("{username}{colon_password}@{host}")
    } else {
        authority.to_string()
    };

    Ok(url::Url::parse(&format!(
        "ssh://{normalized_authority}{path}"
    ))?)
}

#[derive(Clone)]
pub struct OpenListener(UnboundedSender<RawOpenRequest>);

#[derive(Default)]
pub struct RawOpenRequest {
    pub urls: Vec<String>,
    pub diff_paths: Vec<[String; 2]>,
    pub diff_all: bool,
    pub open_behavior: Option<cli::OpenBehavior>,
}

impl Global for OpenListener {}

impl OpenListener {
    pub fn new() -> (Self, UnboundedReceiver<RawOpenRequest>) {
        let (tx, rx) = mpsc::unbounded();
        (OpenListener(tx), rx)
    }

    pub fn open(&self, request: RawOpenRequest) {
        self.0
            .unbounded_send(request)
            .context("no listener for open requests")
            .log_err();
    }
}

fn connect_to_cli(
    server_name: &str,
) -> Result<(
    mpsc::UnboundedReceiver<CliRequest>,
    Box<dyn CliResponseSink>,
)> {
    let handshake_tx = ipc::IpcSender::<IpcHandshake>::connect(server_name.to_string())
        .context("error connecting to cli")?;
    let (request_tx, request_rx) = ipc::channel::<CliRequest>()?;
    let (response_tx, response_rx) = ipc::channel::<CliResponse>()?;

    handshake_tx
        .send(IpcHandshake {
            requests: request_tx,
            responses: response_rx,
        })
        .context("error sending ipc handshake")?;

    let (async_request_tx, async_request_rx) = futures::channel::mpsc::unbounded::<CliRequest>();
    thread::spawn(move || {
        while let Ok(cli_request) = request_rx.recv() {
            if async_request_tx.unbounded_send(cli_request).is_err() {
                break;
            }
        }
        anyhow::Ok(())
    });

    Ok((async_request_rx, Box::new(response_tx)))
}

pub async fn open_paths_with_positions(
    path_positions: &[PathWithPosition],
    diff_paths: &[[String; 2]],
    diff_all: bool,
    app_state: Arc<AppState>,
    open_options: workspace::OpenOptions,
    cx: &mut AsyncApp,
) -> Result<(
    WindowHandle<MultiWorkspace>,
    Vec<Option<Result<Box<dyn ItemHandle>>>>,
)> {
    let paths = path_positions
        .iter()
        .map(|path_with_position| path_with_position.path.clone())
        .collect::<Vec<_>>();

    let OpenResult {
        window: multi_workspace,
        opened_items: mut items,
        ..
    } = cx
        .update(|cx| workspace::open_paths(&paths, app_state.clone(), open_options, cx))
        .await?;

    if diff_all && !diff_paths.is_empty() {
        let mut diff_pairs = Vec::with_capacity(diff_paths.len());
        for diff_pair in diff_paths {
            let parsed = derive_paths_with_position(app_state.fs.as_ref(), diff_pair).await;
            let (Some(old_parsed), Some(new_parsed)) = (parsed.first(), parsed.get(1)) else {
                continue;
            };
            diff_pairs.push([
                old_parsed.path.to_string_lossy().into_owned(),
                new_parsed.path.to_string_lossy().into_owned(),
            ]);
        }
        if let Ok(diff_view) = multi_workspace.update(cx, |multi_workspace, window, cx| {
            multi_workspace.workspace().update(cx, |workspace, cx| {
                MultiDiffView::open(diff_pairs, workspace, window, cx)
            })
        }) {
            if let Some(diff_view) = diff_view.await.log_err() {
                items.push(Some(Ok(Box::new(diff_view))));
            }
        }
    } else {
        let workspace_weak = multi_workspace.read_with(cx, |multi_workspace, _cx| {
            multi_workspace.workspace().downgrade()
        })?;
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
            let (old_path, new_path) =
                match futures::join!(canonicalize(old_parsed), canonicalize(new_parsed)) {
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
                language::Point::new(
                    row.saturating_sub(1),
                    new_parsed.column.unwrap_or(0).saturating_sub(1),
                )
            });
            if let Ok(diff_view) = multi_workspace.update(cx, |_multi_workspace, window, cx| {
                FileDiffView::open(
                    old_path,
                    new_path,
                    target_position,
                    workspace_weak.clone(),
                    window,
                    cx,
                )
            }) {
                if let Some(diff_view) = diff_view.await.log_err() {
                    items.push(Some(Ok(Box::new(diff_view))))
                }
            }
        }
    }

    for (item, path) in items.iter_mut().zip(&paths) {
        if let Some(Err(error)) = item {
            *error = anyhow!("error opening {path:?}: {error:#}");
        }
    }

    let items_for_navigation = items
        .iter()
        .map(|item| item.as_ref().and_then(|r| r.as_ref().ok()).cloned())
        .collect::<Vec<_>>();
    navigate_to_positions(&multi_workspace, items_for_navigation, path_positions, cx);

    Ok((multi_workspace, items))
}

pub async fn handle_cli_connection(
    (mut requests, responses): (
        mpsc::UnboundedReceiver<CliRequest>,
        Box<dyn CliResponseSink>,
    ),
    app_state: Arc<AppState>,
    cx: &mut AsyncApp,
) {
    if let Some(request) = requests.next().await {
        match request {
            CliRequest::Open {
                urls,
                paths,
                diff_paths,
                diff_all,
                wait,
                mut open_behavior,
                env,
                user_data_dir: _,
                cwd,
            } => {
                if !urls.is_empty() {
                    cx.update(|cx| {
                        match OpenRequest::parse(
                            RawOpenRequest {
                                urls,
                                diff_paths,
                                diff_all,
                                open_behavior: Some(open_behavior),
                            },
                            cx,
                        ) {
                            Ok(open_request) => {
                                cx.activate(true);
                                handle_open_request(open_request, app_state.clone(), cx);
                                responses.send(CliResponse::Exit { status: 0 }).log_err();
                            }
                            Err(e) => {
                                responses
                                    .send(CliResponse::Stderr {
                                        message: format!("{e}"),
                                    })
                                    .log_err();
                                responses.send(CliResponse::Exit { status: 1 }).log_err();
                            }
                        };
                    });
                    return;
                }

                if open_behavior == cli::OpenBehavior::Default {
                    open_behavior = cx.update(|cx| open_behavior_for_default_setting(cx));
                }

                cx.update(|cx| cx.activate(true));

                let open_workspace_result = open_workspaces(
                    paths,
                    diff_paths,
                    diff_all,
                    open_behavior,
                    responses.as_ref(),
                    wait,
                    app_state.clone(),
                    env,
                    cwd,
                    cx,
                )
                .await;

                let status = if open_workspace_result.is_err() { 1 } else { 0 };
                responses.send(CliResponse::Exit { status }).log_err();
            }
            CliRequest::SetOpenBehavior { .. } => {
                debug_panic!("unexpected SetOpenBehavior message");
            }
        }
    }
}

pub(crate) fn open_options_for_request(
    open_behavior: Option<cli::OpenBehavior>,
    location: &SerializedWorkspaceLocation,
    cx: &App,
) -> workspace::OpenOptions {
    let open_behavior = open_behavior.unwrap_or_else(|| {
        match workspace::WorkspaceSettings::get_global(cx).default_open_behavior {
            settings::DefaultOpenBehavior::ExistingWindow => cli::OpenBehavior::ExistingWindow,
            settings::DefaultOpenBehavior::NewWindow => cli::OpenBehavior::PreferNewWindow,
        }
    });
    open_options_for_behavior(open_behavior, location, cx)
}

pub(crate) fn open_options_for_behavior(
    open_behavior: cli::OpenBehavior,
    location: &SerializedWorkspaceLocation,
    cx: &App,
) -> workspace::OpenOptions {
    let open_behavior = if open_behavior == cli::OpenBehavior::Default {
        open_behavior_for_default_setting(cx)
    } else {
        open_behavior
    };

    // If reuse flag is passed, open a new workspace in an existing window.
    let requesting_window = if open_behavior == cli::OpenBehavior::Reuse {
        workspace::workspace_windows_for_location(location, cx)
            .into_iter()
            .next()
    } else {
        None
    };
    workspace::OpenOptions {
        workspace_matching: match open_behavior {
            cli::OpenBehavior::AlwaysNew | cli::OpenBehavior::Reuse => {
                workspace::WorkspaceMatching::None
            }
            cli::OpenBehavior::PreferNewWindow => workspace::WorkspaceMatching::MatchSubpaths,
            cli::OpenBehavior::Add => workspace::WorkspaceMatching::MatchSubdirectory,
            _ => workspace::WorkspaceMatching::MatchExact,
        },
        add_dirs_to_sidebar: match open_behavior {
            cli::OpenBehavior::ExistingWindow => true,
            _ => false,
        },
        requesting_window,
        ..Default::default()
    }
}

fn open_behavior_for_default_setting(cx: &App) -> cli::OpenBehavior {
    match workspace::WorkspaceSettings::get_global(cx).cli_default_open_behavior {
        settings::CliDefaultOpenBehavior::ExistingWindow => cli::OpenBehavior::ExistingWindow,
        settings::CliDefaultOpenBehavior::NewWindow => cli::OpenBehavior::PreferNewWindow,
    }
}

async fn open_workspaces(
    paths: Vec<String>,
    diff_paths: Vec<[String; 2]>,
    diff_all: bool,
    open_behavior: cli::OpenBehavior,
    responses: &dyn CliResponseSink,
    wait: bool,
    app_state: Arc<AppState>,
    env: Option<collections::HashMap<String, String>>,
    cwd: Option<PathBuf>,
    cx: &mut AsyncApp,
) -> Result<()> {
    if paths.is_empty()
        && diff_paths.is_empty()
        && !matches!(open_behavior, cli::OpenBehavior::AlwaysNew)
    {
        return restore_or_create_workspace(app_state, cx).await;
    }

    let grouped_locations: Vec<(SerializedWorkspaceLocation, PathList)> =
        if paths.is_empty() && diff_paths.is_empty() {
            Vec::new()
        } else {
            vec![(
                SerializedWorkspaceLocation::Local,
                PathList::new(&paths.into_iter().map(PathBuf::from).collect::<Vec<_>>()),
            )]
        };

    if grouped_locations.is_empty() {
        // If we have no paths to open, show an empty window with empty editor
        cx.update(|cx| {
            let open_options = OpenOptions {
                env,
                ..Default::default()
            };
            workspace::open_new(open_options, app_state, cx, |workspace, window, cx| {
                Editor::new_file(workspace, &Default::default(), window, cx)
            })
            .detach_and_log_err(cx);
        });
        return Ok(());
    }
    // If there are paths to open, open a workspace for each grouping of paths
    let mut errored = false;

    for (location, workspace_paths) in grouped_locations {
        let base_open_options =
            cx.update(|cx| open_options_for_behavior(open_behavior, &location, cx));
        let open_options = workspace::OpenOptions {
            wait,
            env: env.clone(),
            ..base_open_options
        };

        match location {
            SerializedWorkspaceLocation::Local => {
                let workspace_paths = workspace_paths
                    .paths()
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();

                let workspace_failed_to_open = open_local_workspace(
                    workspace_paths,
                    diff_paths.clone(),
                    diff_all,
                    open_options,
                    cwd.clone(),
                    responses,
                    &app_state,
                    cx,
                )
                .await;

                if workspace_failed_to_open {
                    errored = true
                }
            }
            SerializedWorkspaceLocation::Remote(mut connection) => {
                let app_state = app_state.clone();
                let RemoteConnectionOptions::Ssh(options) = &mut connection;
                cx.update(|cx| {
                    RemoteSettings::get_global(cx).fill_connection_options_from_settings(options)
                });

                cx.spawn(async move |cx| {
                    open_remote_project(
                        connection,
                        workspace_paths.paths().to_vec(),
                        app_state,
                        open_options,
                        cx,
                    )
                    .await
                    .log_err();
                })
                .detach();
            }
        }
    }

    anyhow::ensure!(!errored, "failed to open a workspace");

    Ok(())
}

async fn open_local_workspace(
    mut workspace_paths: Vec<String>,
    diff_paths: Vec<[String; 2]>,
    diff_all: bool,
    open_options: workspace::OpenOptions,
    cwd: Option<PathBuf>,
    responses: &dyn CliResponseSink,
    app_state: &Arc<AppState>,
    cx: &mut AsyncApp,
) -> bool {
    let user_provided_paths = !workspace_paths.is_empty();

    // When only diff paths are provided (no regular paths), add the CLI's
    // working directory so the workspace opens with the right context.
    // Note: must use the CLI process's cwd (forwarded via `cli_cwd`), not
    // `std::env::current_dir()`, since the Zed app process's cwd is typically
    // `/` on macOS bundles or the launch dir of an already-running instance.
    if !user_provided_paths
        && !diff_paths.is_empty()
        && let Some(cwd) = cwd
    {
        workspace_paths.push(cwd.to_string_lossy().to_string());
    }

    let paths_with_position =
        derive_paths_with_position(app_state.fs.as_ref(), workspace_paths).await;

    let (workspace, items) = match open_paths_with_positions(
        &paths_with_position,
        &diff_paths,
        diff_all,
        app_state.clone(),
        open_options.clone(),
        cx,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            let paths = paths_with_position
                .iter()
                .map(|p| p.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            log::error!("failed to open workspace [{paths}]: {error:#}");
            responses
                .send(CliResponse::Stderr {
                    message: format!("error opening [{paths}]: {error:#}"),
                })
                .log_err();
            return true;
        }
    };

    let mut errored = false;
    let mut item_release_futures = Vec::new();
    let mut subscriptions = Vec::new();
    // If --wait flag is used with no paths, or a directory, then wait until
    // the entire workspace is closed.
    if open_options.wait {
        let mut wait_for_window_close = paths_with_position.is_empty() && diff_paths.is_empty();
        if user_provided_paths {
            for path_with_position in &paths_with_position {
                if app_state.fs.is_dir(&path_with_position.path).await {
                    wait_for_window_close = true;
                    break;
                }
            }
        }

        if wait_for_window_close {
            let (release_tx, release_rx) = oneshot::channel();
            item_release_futures.push(release_rx);
            subscriptions.push(workspace.update(cx, |_, _, cx| {
                cx.on_release(move |_, _| {
                    let _ = release_tx.send(());
                })
            }));
        }
    }

    for item in items {
        match item {
            Some(Ok(item)) => {
                if open_options.wait {
                    let (release_tx, release_rx) = oneshot::channel();
                    item_release_futures.push(release_rx);
                    subscriptions.push(Ok(cx.update(|cx| {
                        item.on_release(
                            cx,
                            Box::new(move |_| {
                                release_tx.send(()).ok();
                            }),
                        )
                    })));
                }
            }
            Some(Err(err)) => {
                log::error!("{err:#}");
                responses
                    .send(CliResponse::Stderr {
                        message: format!("{err:#}"),
                    })
                    .log_err();
                errored = true;
            }
            None => {}
        }
    }

    if open_options.wait {
        let wait = async move {
            let _subscriptions = subscriptions;
            let _ = future::try_join_all(item_release_futures).await;
        }
        .fuse();
        futures::pin_mut!(wait);

        let background = cx.background_executor().clone();
        loop {
            // Repeatedly check if CLI is still open to avoid wasting resources
            // waiting for files or workspaces to close.
            let mut timer = background.timer(Duration::from_secs(1)).fuse();
            futures::select_biased! {
                _ = wait => break,
                _ = timer => {
                    if responses.send(CliResponse::Ping).is_err() {
                        break;
                    }
                }
            }
        }
    }

    errored
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
        let has_colon = original_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| name.contains(':'));

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
