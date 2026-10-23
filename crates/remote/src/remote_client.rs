use std::collections::VecDeque;
use std::fmt;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use collections::HashMap;
use futures::Future;
use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::mpsc::Sender;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::mpsc::UnboundedSender;
use futures::channel::mpsc::{self};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::future::Shared;
use futures::future::WeakShared;
use futures::select;
use futures::select_biased;
use futures::stream::BoxStream;
use gpui::App;
use gpui::AppContext as _;
use gpui::AsyncApp;
use gpui::BackgroundExecutor;
use gpui::BorrowAppContext;
use gpui::Context;
use gpui::Entity;
use gpui::EventEmitter;
use gpui::FutureExt;
use gpui::Global;
use gpui::Task;
use gpui::TaskExt;
use gpui::WeakEntity;
use parking_lot::Mutex;
use rpc::AnyProtoClient;
use rpc::ErrorExt;
use rpc::ProtoClient;
use rpc::ProtoMessageHandlerSet;
use rpc::RpcError;
use rpc::proto::Envelope;
use rpc::proto::EnvelopedMessage;
use rpc::proto::PeerId;
use rpc::proto::RequestMessage;
use rpc::proto::build_typed_envelope;
use rpc::proto::{self};
use util::ResultExt;
use util::paths::PathStyle;
use util::paths::RemotePathBuf;

use crate::SshConnectionOptions;
use crate::command::RemoteCommand;
use crate::protocol::MessageId;
use crate::proxy::ProxyLaunchError;
use crate::transport::ssh::SshRemoteConnection;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RemoteOs {
    Linux,
    MacOs,
}

impl RemoteOs {
    pub const fn as_str(&self) -> &'static str {
        match self {
            RemoteOs::Linux => "linux",
            RemoteOs::MacOs => "macos",
        }
    }

    /// ignoring the compositor (as we run headless on remotes).
    pub const fn display_name(&self) -> &'static str {
        match self {
            RemoteOs::Linux => "Linux",
            RemoteOs::MacOs => "macOS",
        }
    }
}

impl std::fmt::Display for RemoteOs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RemoteArch {
    X86_64,
    Aarch64,
}

impl RemoteArch {
    pub const fn as_str(&self) -> &'static str {
        match self {
            RemoteArch::X86_64 => "x86_64",
            RemoteArch::Aarch64 => "aarch64",
        }
    }
}

impl std::fmt::Display for RemoteArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Copy, Clone, Debug)]
pub struct RemotePlatform {
    pub os: RemoteOs,
    pub arch: RemoteArch,
}

#[derive(Clone, Debug)]
pub struct CommandTemplate {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// Whether a command should be run with TTY allocation for interactive use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interactive {
    /// Allocate a pseudo-TTY for interactive terminal use.
    Yes,
    /// Do not allocate a TTY - for commands that communicate via piped stdio.
    No,
}

pub trait RemoteClientDelegate: Send + Sync {
    fn set_status(&self, status: Option<&str>, cx: &mut AsyncApp);
}

const MAX_MISSED_HEARTBEATS: usize = 5;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_CONNECTION_TIMEOUT: Duration = Duration::from_secs(if cfg!(debug_assertions) { 5 } else { 60 });
const RECONNECT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(if cfg!(debug_assertions) { 5 } else { 15 });
const RESYNC_TIMEOUT: Duration = Duration::from_secs(if cfg!(debug_assertions) { 5 } else { 15 });

pub const MAX_RECONNECT_ATTEMPTS: usize = 3;

enum State {
    Connecting,
    Connected {
        remote_connection: Arc<dyn RemoteConnection>,
        delegate: Arc<dyn RemoteClientDelegate>,

        multiplex_task: Task<Result<()>>,
        heartbeat_task: Task<Result<()>>,
    },
    HeartbeatMissed {
        missed_heartbeats: usize,

        remote_connection: Arc<dyn RemoteConnection>,
        delegate: Arc<dyn RemoteClientDelegate>,

        multiplex_task: Task<Result<()>>,
        heartbeat_task: Task<Result<()>>,
    },
    Reconnecting,
    ReconnectFailed {
        remote_connection: Arc<dyn RemoteConnection>,
        delegate: Arc<dyn RemoteClientDelegate>,

        error: anyhow::Error,
        attempts: usize,
    },
    ReconnectExhausted,
    ServerNotRunning,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connecting => write!(f, "connecting"),
            Self::Connected { .. } => write!(f, "connected"),
            Self::Reconnecting => write!(f, "reconnecting"),
            Self::ReconnectFailed { .. } => write!(f, "reconnect failed"),
            Self::ReconnectExhausted => write!(f, "reconnect exhausted"),
            Self::HeartbeatMissed { .. } => write!(f, "heartbeat missed"),
            Self::ServerNotRunning { .. } => write!(f, "server not running"),
        }
    }
}

impl State {
    fn remote_connection(&self) -> Option<Arc<dyn RemoteConnection>> {
        match self {
            Self::Connected { remote_connection, .. } => Some(remote_connection.clone()),
            Self::HeartbeatMissed { remote_connection, .. } => Some(remote_connection.clone()),
            Self::ReconnectFailed { remote_connection, .. } => Some(remote_connection.clone()),
            _ => None,
        }
    }

    const fn can_reconnect(&self) -> bool {
        match self {
            Self::Connected { .. } | Self::HeartbeatMissed { .. } | Self::ReconnectFailed { .. } => true,
            State::Connecting | State::Reconnecting | State::ReconnectExhausted | State::ServerNotRunning => false,
        }
    }

    const fn is_reconnect_failed(&self) -> bool {
        matches!(self, Self::ReconnectFailed { .. })
    }

    const fn is_reconnect_exhausted(&self) -> bool {
        matches!(self, Self::ReconnectExhausted { .. })
    }

    const fn is_server_not_running(&self) -> bool {
        matches!(self, Self::ServerNotRunning)
    }

    const fn is_reconnecting(&self) -> bool {
        matches!(self, Self::Reconnecting { .. })
    }

    fn heartbeat_recovered(self) -> Self {
        match self {
            Self::HeartbeatMissed { remote_connection, delegate, multiplex_task, heartbeat_task, .. } => {
                Self::Connected { remote_connection, delegate, multiplex_task, heartbeat_task }
            }
            _ => self,
        }
    }

    fn heartbeat_missed(self) -> Self {
        match self {
            Self::Connected { remote_connection, delegate, multiplex_task, heartbeat_task } => Self::HeartbeatMissed {
                missed_heartbeats: 1,
                remote_connection,
                delegate,
                multiplex_task,
                heartbeat_task,
            },
            Self::HeartbeatMissed {
                missed_heartbeats,
                remote_connection,
                delegate,
                multiplex_task,
                heartbeat_task,
            } => Self::HeartbeatMissed {
                missed_heartbeats: missed_heartbeats + 1,
                remote_connection,
                delegate,
                multiplex_task,
                heartbeat_task,
            },
            _ => self,
        }
    }
}

/// The state of the ssh connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    HeartbeatMissed,
    Reconnecting,
    Disconnected,
}

impl From<&State> for ConnectionState {
    fn from(value: &State) -> Self {
        match value {
            State::Connecting => Self::Connecting,
            State::Connected { .. } => Self::Connected,
            State::Reconnecting | State::ReconnectFailed { .. } => Self::Reconnecting,
            State::HeartbeatMissed { .. } => Self::HeartbeatMissed,
            State::ReconnectExhausted => Self::Disconnected,
            State::ServerNotRunning => Self::Disconnected,
        }
    }
}

pub struct RemoteClient {
    client: Arc<ChannelClient>,
    unique_identifier: String,
    connection_options: RemoteConnectionOptions,
    path_style: PathStyle,
    platform: RemotePlatform,
    os_version: Option<String>,
    state: Option<State>,
}

#[derive(Debug)]
pub enum RemoteClientEvent {
    Disconnected { server_not_running: bool },
    Reconnected,
}

impl EventEmitter<RemoteClientEvent> for RemoteClient {}

/// Identifies the socket on the remote server so that reconnects
/// can re-join the same project.
pub enum ConnectionIdentifier {
    Setup(u64),
    Workspace(i64),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl ConnectionIdentifier {
    pub fn setup() -> Self {
        Self::Setup(NEXT_ID.fetch_add(1, SeqCst))
    }
}

// This string gets used in a socket name, and so must be relatively short.
// The total length of:
//   /home/{username}/.local/share/zed/server_state/{name}/stdout.sock
// Must be less than about 100 characters
//   https://unix.stackexchange.com/questions/367008/why-is-socket-path-length-limited-to-a-hundred-chars
// So our strings should be at most 20 characters or so.
impl std::fmt::Display for ConnectionIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Setup(setup_id) => write!(f, "setup-{setup_id}"),
            Self::Workspace(workspace_id) => write!(f, "workspace-{workspace_id}"),
        }
    }
}

pub async fn connect(
    connection_options: RemoteConnectionOptions,
    delegate: Arc<dyn RemoteClientDelegate>,
    cx: &mut AsyncApp,
) -> Result<Arc<dyn RemoteConnection>> {
    cx.update(|cx| {
        cx.update_default_global(|pool: &mut ConnectionPool, cx| {
            pool.connect(connection_options.clone(), delegate.clone(), cx)
        })
    })
    .await
    .map_err(|e| e.cloned())
}

/// Returns `true` if the global [`ConnectionPool`] already has a live
/// connection for the given options. Callers can use this to decide
/// whether to show connection progress UI before
/// connecting.
pub fn has_active_connection(opts: &RemoteConnectionOptions, cx: &App) -> bool {
    cx.try_global::<ConnectionPool>().is_some_and(|pool| {
        matches!(
            pool.connections.get(opts),
            Some(ConnectionPoolEntry::Connected(remote))
                if remote.upgrade().is_some_and(|r| !r.has_been_killed())
        )
    })
}

impl RemoteClient {
    pub fn new(
        unique_identifier: ConnectionIdentifier,
        remote_connection: Arc<dyn RemoteConnection>,
        cancellation: oneshot::Receiver<()>,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut App,
    ) -> Task<Result<Option<Entity<Self>>>> {
        let unique_identifier = unique_identifier.to_string();
        cx.spawn(async move |cx| {
            let success = Box::pin(async move {
                let (outgoing_tx, outgoing_rx) = mpsc::unbounded::<Envelope>();
                let (incoming_tx, incoming_rx) = mpsc::unbounded::<Envelope>();
                let (connection_activity_tx, connection_activity_rx) = mpsc::channel::<()>(1);

                let client = cx.update(|cx| ChannelClient::new(incoming_rx, outgoing_tx, cx, "client"));

                let path_style = remote_connection.path_style();
                let platform = remote_connection.remote_platform();
                let os_version = remote_connection.remote_os_version();
                let connection_options = remote_connection.connection_options();
                let _connection_type = connection_options.connection_type();
                let this = cx.new(|_| Self {
                    client: client.clone(),
                    unique_identifier: unique_identifier.clone(),
                    connection_options,
                    path_style,
                    platform,
                    os_version: os_version.clone(),
                    state: Some(State::Connecting),
                });

                let io_task = remote_connection.start_proxy(
                    unique_identifier,
                    false,
                    incoming_tx,
                    outgoing_rx,
                    connection_activity_tx,
                    delegate.clone(),
                    cx,
                );

                let ready = client
                    .wait_for_remote_started()
                    .with_timeout(INITIAL_CONNECTION_TIMEOUT, cx.background_executor())
                    .await;
                match ready {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        let mut error = "remote client exited before becoming ready".to_owned();
                        if let Some(status) = io_task.now_or_never() {
                            match status {
                                Ok(exit_code) => error.push_str(&format!(", exit_code={exit_code:?}")),
                                Err(e) => error.push_str(&format!(", error={e:?}")),
                            }
                        }
                        let error = anyhow::anyhow!("{error}");
                        log::error!("failed to establish connection: {}", error);
                        return Err(error);
                    }
                    Err(_) => {
                        let mut error = String::new();
                        if let Some(status) = io_task.now_or_never() {
                            error.push_str("Client exited with ");
                            match status {
                                Ok(exit_code) => error.push_str(&format!("exit_code {exit_code:?}")),
                                Err(e) => error.push_str(&format!("error {e:?}")),
                            }
                        } else {
                            error.push_str("client did not become ready within the timeout");
                        }
                        let error = anyhow::anyhow!("{error}");
                        log::error!("failed to establish connection: {error}");
                        return Err(error);
                    }
                }
                let multiplex_task = Self::monitor(this.downgrade(), io_task, cx);
                if let Err(error) = client.ping(HEARTBEAT_TIMEOUT).await {
                    log::error!("failed to establish connection: {}", error);
                    return Err(error);
                }

                let heartbeat_task = Self::heartbeat(this.downgrade(), connection_activity_rx, cx);

                this.update(cx, |this, _| {
                    this.state = Some(State::Connected { remote_connection, delegate, multiplex_task, heartbeat_task });
                });

                // Use the same `remote_*` property schema as the forwarded

                Ok(Some(this))
            });

            select! {
                _ = cancellation.fuse() => {
                    Ok(None)
                }
                result = success.fuse() =>  result
            }
        })
    }

    pub fn proto_client_from_channels(
        incoming_rx: mpsc::UnboundedReceiver<Envelope>,
        outgoing_tx: mpsc::UnboundedSender<Envelope>,
        cx: &App,
        name: &'static str,
    ) -> AnyProtoClient {
        ChannelClient::new(incoming_rx, outgoing_tx, cx, name).into()
    }

    pub fn shutdown_processes<T: RequestMessage>(
        &mut self,
        shutdown_request: Option<T>,
        executor: BackgroundExecutor,
    ) -> Option<impl Future<Output = ()> + use<T>> {
        let state = self.state.take()?;
        log::info!("shutting down remote processes");

        let State::Connected { multiplex_task, heartbeat_task, remote_connection, delegate } = state else {
            return None;
        };

        let client = self.client.clone();

        Some(async move {
            if let Some(shutdown_request) = shutdown_request {
                client.send(shutdown_request).log_err();
                // We wait 50ms instead of waiting for a response, because
                // waiting for a response would require us to wait on the main thread
                // which we want to avoid in an `on_app_quit` callback.
                executor.timer(Duration::from_millis(50)).await;
            }

            // Drop `multiplex_task` because it owns our remote_connection_proxy_process, which is a
            // child of master_process.
            drop(multiplex_task);
            // Now drop the rest of state, which kills master process.
            drop(heartbeat_task);
            drop(remote_connection);
            drop(delegate);
        })
    }

    fn reconnect(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let can_reconnect = self.state.as_ref().map(|state| state.can_reconnect()).unwrap_or(false);
        if !can_reconnect {
            let state =
                if let Some(state) = self.state.as_ref() { state.to_string() } else { "no state set".to_string() };
            log::info!("aborting reconnect, because not in state that allows reconnecting: {state}");
            anyhow::bail!("aborting reconnect, because not in state that allows reconnecting: {state}");
        }

        let state = self.state.take().unwrap();
        let (attempts, remote_connection, delegate) = match state {
            State::Connected { remote_connection, delegate, multiplex_task, heartbeat_task }
            | State::HeartbeatMissed { remote_connection, delegate, multiplex_task, heartbeat_task, .. } => {
                drop(multiplex_task);
                drop(heartbeat_task);
                (0, remote_connection, delegate)
            }
            State::ReconnectFailed { attempts, remote_connection, delegate, .. } => {
                (attempts, remote_connection, delegate)
            }
            State::Connecting | State::Reconnecting | State::ReconnectExhausted | State::ServerNotRunning => {
                unreachable!()
            }
        };

        let attempts = attempts + 1;
        if attempts > MAX_RECONNECT_ATTEMPTS {
            log::error!("Failed to reconnect to after {} attempts, giving up", MAX_RECONNECT_ATTEMPTS);
            self.set_state(State::ReconnectExhausted, cx);
            return Ok(());
        }

        self.set_state(State::Reconnecting, cx);

        log::info!("Trying to reconnect to remote server... Attempt {}", attempts);

        let unique_identifier = self.unique_identifier.clone();
        let client = self.client.clone();
        let reconnect_task = cx.spawn(async move |this, cx| {
            macro_rules! failed {
                ($error:expr, $attempts:expr, $remote_connection:expr, $delegate:expr) => {
                    delegate.set_status(Some(&format!("{error:#}", error = $error)), cx);
                    return State::ReconnectFailed {
                        error: anyhow!($error),
                        attempts: $attempts,
                        remote_connection: $remote_connection,
                        delegate: $delegate,
                    };
                };
            }

            if let Err(error) = remote_connection.kill().await.context("Failed to kill remote_connection process") {
                failed!(error, attempts, remote_connection, delegate);
            };

            let connection_options = remote_connection.connection_options();

            let (outgoing_tx, outgoing_rx) = mpsc::unbounded::<Envelope>();
            let (incoming_tx, incoming_rx) = mpsc::unbounded::<Envelope>();
            let (connection_activity_tx, connection_activity_rx) = mpsc::channel::<()>(1);

            let (remote_connection, io_task) = match async {
                let remote_connection = cx
                    .update_global(|pool: &mut ConnectionPool, cx| {
                        pool.connect(connection_options, delegate.clone(), cx)
                    })
                    .await
                    .map_err(|error| error.cloned())?;

                let io_task = remote_connection.start_proxy(
                    unique_identifier,
                    true,
                    incoming_tx,
                    outgoing_rx,
                    connection_activity_tx,
                    delegate.clone(),
                    cx,
                );
                anyhow::Ok((remote_connection, io_task))
            }
            .await
            {
                Ok((remote_connection, io_task)) => (remote_connection, io_task),
                Err(error) => {
                    failed!(error, attempts, remote_connection, delegate);
                }
            };

            let multiplex_task = Self::monitor(this.clone(), io_task, cx);
            let connection_ready_rx = client.reconnect(incoming_rx, outgoing_tx, cx);
            let resync = client.resync(RESYNC_TIMEOUT);

            if let Err(error) = async {
                connection_ready_rx
                    .with_timeout(RECONNECT_HANDSHAKE_TIMEOUT, cx.background_executor())
                    .await
                    .context("Timed out waiting for remote connection")?
                    .context("Remote connection closed before becoming ready")?;

                resync.await
            }
            .await
            {
                failed!(error, attempts, remote_connection, delegate);
            };

            State::Connected {
                remote_connection,
                delegate,
                multiplex_task,
                heartbeat_task: Self::heartbeat(this.clone(), connection_activity_rx, cx),
            }
        });

        cx.spawn(async move |this, cx| {
            let new_state = reconnect_task.await;
            this.update(cx, |this, cx| {
                let reconnected =
                    this.state_is(State::is_reconnecting) && matches!(&new_state, State::Connected { .. });
                this.try_set_state(cx, |old_state| {
                    if old_state.is_reconnecting() {
                        match &new_state {
                            State::Connecting
                            | State::Reconnecting
                            | State::HeartbeatMissed { .. }
                            | State::ServerNotRunning => {}
                            State::Connected { .. } => {
                                log::info!("Successfully reconnected");
                            }
                            State::ReconnectFailed { error, attempts, .. } => {
                                log::error!(
                                    "Reconnect attempt {} failed: {:?}. Starting new attempt...",
                                    attempts,
                                    error
                                );
                            }
                            State::ReconnectExhausted => {
                                log::error!("Reconnect attempt failed and all attempts exhausted");
                            }
                        }
                        Some(new_state)
                    } else {
                        None
                    }
                });

                if reconnected {
                    cx.emit(RemoteClientEvent::Reconnected);
                }

                if this.state_is(State::is_reconnect_failed) {
                    this.reconnect(cx)
                } else if this.state_is(State::is_reconnect_exhausted) {
                    Ok(())
                } else {
                    log::debug!("State has transition from Reconnecting into new state while attempting reconnect.");
                    Ok(())
                }
            })
        })
        .detach_and_log_err(cx);

        Ok(())
    }

    fn heartbeat(
        this: WeakEntity<Self>,
        mut connection_activity_rx: mpsc::Receiver<()>,
        cx: &AsyncApp,
    ) -> Task<Result<()>> {
        let Ok(client) = this.read_with(cx, |this, _| this.client.clone()) else {
            return Task::ready(Err(anyhow!("remote_connectionRemoteClient lost")));
        };

        cx.spawn(async move |cx| {
            let mut missed_heartbeats = 0;

            let keepalive_timer = cx.background_executor().timer(HEARTBEAT_INTERVAL).fuse();
            futures::pin_mut!(keepalive_timer);

            loop {
                select_biased! {
                    result = connection_activity_rx.next().fuse() => {
                        if result.is_none() {
                            log::warn!("remote heartbeat: connection activity channel has been dropped. stopping.");
                            return Ok(());
                        }

                        if missed_heartbeats != 0 {
                            missed_heartbeats = 0;
                            let _ =this.update(cx, |this, cx| {
                                this.handle_heartbeat_result(missed_heartbeats, cx)
                            })?;
                        }
                    }
                    _ = keepalive_timer => {
                        log::debug!("Sending heartbeat to server...");

                        let result = select_biased! {
                            _ = connection_activity_rx.next().fuse() => {
                                Ok(())
                            }
                            ping_result = client.ping(HEARTBEAT_TIMEOUT).fuse() => {
                                ping_result
                            }
                        };

                        if result.is_err() {
                            missed_heartbeats += 1;
                            log::warn!(
                                "No heartbeat from server after {:?}. Missed heartbeat {} out of {}.",
                                HEARTBEAT_TIMEOUT,
                                missed_heartbeats,
                                MAX_MISSED_HEARTBEATS
                            );
                        } else if missed_heartbeats != 0 {
                            missed_heartbeats = 0;
                        } else {
                            continue;
                        }

                        let result = this.update(cx, |this, cx| {
                            this.handle_heartbeat_result(missed_heartbeats, cx)
                        })?;
                        if result.is_break() {
                            return Ok(());
                        }
                    }
                }

                keepalive_timer.set(cx.background_executor().timer(HEARTBEAT_INTERVAL).fuse());
            }
        })
    }

    fn handle_heartbeat_result(&mut self, missed_heartbeats: usize, cx: &mut Context<Self>) -> ControlFlow<()> {
        let state = self.state.take().unwrap();
        let next_state = if missed_heartbeats > 0 { state.heartbeat_missed() } else { state.heartbeat_recovered() };

        self.set_state(next_state, cx);

        if missed_heartbeats >= MAX_MISSED_HEARTBEATS {
            log::error!("Missed last {} heartbeats. Reconnecting...", missed_heartbeats);

            self.reconnect(cx).context("failed to start reconnect process after missing heartbeats").log_err();
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn monitor(this: WeakEntity<Self>, io_task: Task<Result<i32>>, cx: &AsyncApp) -> Task<Result<()>> {
        cx.spawn(async move |cx| {
            let result = io_task.await;

            match result {
                Ok(exit_code) => {
                    if let Some(error) = ProxyLaunchError::from_exit_code(exit_code) {
                        match error {
                            ProxyLaunchError::ServerNotRunning => {
                                log::error!("failed to reconnect because server is not running");
                                this.update(cx, |this, cx| {
                                    this.set_state(State::ServerNotRunning, cx);
                                })?;
                            }
                        }
                    } else {
                        log::error!("proxy process terminated unexpectedly: {exit_code}");
                        this.update(cx, |this, cx| {
                            this.reconnect(cx).ok();
                        })?;
                    }
                }
                Err(error) => {
                    log::warn!("remote io task died with error: {:?}. reconnecting...", error);
                    this.update(cx, |this, cx| {
                        this.reconnect(cx).ok();
                    })?;
                }
            }

            Ok(())
        })
    }

    fn state_is(&self, check: impl FnOnce(&State) -> bool) -> bool {
        self.state.as_ref().is_some_and(check)
    }

    fn try_set_state(&mut self, cx: &mut Context<Self>, map: impl FnOnce(&State) -> Option<State>) {
        let new_state = self.state.as_ref().and_then(map);
        if let Some(new_state) = new_state {
            self.state.replace(new_state);
            cx.notify();
        }
    }

    fn set_state(&mut self, state: State, cx: &mut Context<Self>) {
        log::info!("setting state to '{state}'");

        let is_reconnect_exhausted = state.is_reconnect_exhausted();
        let is_server_not_running = state.is_server_not_running();
        self.state.replace(state);

        if is_reconnect_exhausted || is_server_not_running {
            cx.emit(RemoteClientEvent::Disconnected { server_not_running: is_server_not_running });
        }
        cx.notify();
    }

    pub fn shell(&self) -> Option<String> {
        Some(self.remote_connection()?.shell())
    }

    pub fn default_system_shell(&self) -> Option<String> {
        Some(self.remote_connection()?.default_system_shell())
    }

    pub fn shares_network_interface(&self) -> bool {
        self.remote_connection().is_some_and(|connection| connection.shares_network_interface())
    }

    pub fn build_command(
        &self,
        program: Option<String>,
        args: &[String],
        env: &HashMap<String, String>,
        working_dir: Option<String>,
        port_forward: Option<(u16, String, u16)>,
        interactive: Interactive,
    ) -> Result<CommandTemplate> {
        let Some(connection) = self.remote_connection() else {
            return Err(anyhow!("no remote connection"));
        };
        connection.build_command(program, args, env, working_dir, port_forward, interactive)
    }

    pub fn build_stdio_command(&self, command: RemoteCommand) -> Result<(CommandTemplate, Vec<u8>)> {
        let Some(connection) = self.remote_connection() else {
            return Err(anyhow!("no remote connection"));
        };
        connection.build_stdio_command(command)
    }

    pub fn build_forward_ports_command(&self, forwards: Vec<(u16, String, u16)>) -> Result<CommandTemplate> {
        let Some(connection) = self.remote_connection() else {
            return Err(anyhow!("no remote connection"));
        };
        connection.build_forward_ports_command(forwards)
    }

    pub fn upload_directory(&self, src_path: PathBuf, dest_path: RemotePathBuf, cx: &App) -> Task<Result<()>> {
        let Some(connection) = self.remote_connection() else {
            return Task::ready(Err(anyhow!("no remote connection")));
        };
        connection.upload_directory(src_path, dest_path, cx)
    }

    pub fn proto_client(&self) -> AnyProtoClient {
        self.client.clone().into()
    }

    pub fn connection_options(&self) -> RemoteConnectionOptions {
        self.connection_options.clone()
    }

    pub fn connection(&self) -> Option<Arc<dyn RemoteConnection>> {
        if let State::Connected { remote_connection, .. } = self.state.as_ref()? {
            Some(remote_connection.clone())
        } else {
            None
        }
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.state.as_ref().map(ConnectionState::from).unwrap_or(ConnectionState::Disconnected)
    }

    pub fn is_disconnected(&self) -> bool {
        self.connection_state() == ConnectionState::Disconnected
    }

    pub const fn path_style(&self) -> PathStyle {
        self.path_style
    }

    /// The platform (OS and architecture) of the remote host, detected during
    /// connection setup.
    pub const fn remote_platform(&self) -> RemotePlatform {
        self.platform
    }

    /// The OS version of the remote host (e.g. `"ubuntu 24.04"`), detected
    /// during connection setup. `None` if it could not be determined.
    pub fn remote_os_version(&self) -> Option<String> {
        self.os_version.clone()
    }

    /// A stable identifier for the kind of remote connection (`"ssh"`).
    pub const fn connection_type(&self) -> &'static str {
        self.connection_options.connection_type()
    }

    /// Forcibly disconnects from the remote server by killing the underlying connection.
    /// This will trigger the reconnection logic if reconnection attempts remain.
    /// Useful for testing reconnection behavior in real environments.
    pub fn force_disconnect(&mut self, cx: &mut Context<Self>) -> Task<Result<()>> {
        let Some(connection) = self.remote_connection() else {
            return Task::ready(Err(anyhow!("no active remote connection to disconnect")));
        };

        log::info!("force_disconnect: killing remote connection");

        cx.spawn(async move |_, _| {
            connection.kill().await?;
            Ok(())
        })
    }

    /// Simulates a timeout by pausing heartbeat responses.
    /// This will cause heartbeat failures and eventually trigger reconnection
    /// after MAX_MISSED_HEARTBEATS are missed.
    /// Useful for testing timeout behavior in real environments.
    pub fn force_heartbeat_timeout(&mut self, attempts: usize, cx: &mut Context<Self>) {
        log::info!("force_heartbeat_timeout: triggering heartbeat failure state");

        if let Some(State::Connected { remote_connection, delegate, multiplex_task, heartbeat_task }) =
            self.state.take()
        {
            self.set_state(
                if attempts == 0 {
                    State::HeartbeatMissed {
                        missed_heartbeats: MAX_MISSED_HEARTBEATS,
                        remote_connection,
                        delegate,
                        multiplex_task,
                        heartbeat_task,
                    }
                } else {
                    State::ReconnectFailed {
                        remote_connection,
                        delegate,
                        error: anyhow!("forced heartbeat timeout"),
                        attempts,
                    }
                },
                cx,
            );

            self.reconnect(cx).context("failed to start reconnect after forced timeout").log_err();
        } else {
            log::warn!("force_heartbeat_timeout: not in Connected state, ignoring");
        }
    }

    pub fn remote_connection(&self) -> Option<Arc<dyn RemoteConnection>> {
        self.state.as_ref().and_then(|state| state.remote_connection())
    }
}

enum ConnectionPoolEntry {
    Connecting(WeakShared<Task<Result<Arc<dyn RemoteConnection>, Arc<anyhow::Error>>>>),
    Connected(Weak<dyn RemoteConnection>),
}

#[derive(Default)]
struct ConnectionPool {
    connections: HashMap<RemoteConnectionOptions, ConnectionPoolEntry>,
}

impl Global for ConnectionPool {}

impl ConnectionPool {
    fn connect(
        &mut self,
        opts: RemoteConnectionOptions,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &App,
    ) -> Shared<Task<Result<Arc<dyn RemoteConnection>, Arc<anyhow::Error>>>> {
        let connection = self.connections.get(&opts);
        match connection {
            Some(ConnectionPoolEntry::Connecting(task)) => {
                if let Some(task) = task.upgrade() {
                    log::debug!("Connecting task is still alive");
                    cx.spawn(async move |cx| delegate.set_status(Some("Waiting for existing connection attempt"), cx))
                        .detach();
                    return task;
                }
                log::debug!("Connecting task is dead, removing it and restarting a connection");
                self.connections.remove(&opts);
            }
            Some(ConnectionPoolEntry::Connected(remote)) => {
                if let Some(remote) = remote.upgrade()
                    && !remote.has_been_killed()
                {
                    log::debug!("Connection is still alive");
                    return Task::ready(Ok(remote)).shared();
                }
                log::debug!("Connection is dead, removing it and restarting a connection");
                self.connections.remove(&opts);
            }
            None => {
                log::debug!("No existing connection found, starting a new one");
            }
        }

        let task = cx
            .spawn({
                let opts = opts.clone();
                let delegate = delegate.clone();
                async move |cx| {
                    let connection = match opts.clone() {
                        RemoteConnectionOptions::Ssh(opts) => SshRemoteConnection::new(opts, delegate, cx)
                            .await
                            .map(|connection| Arc::new(connection) as Arc<dyn RemoteConnection>),
                    };

                    cx.update_global(|pool: &mut Self, _| {
                        debug_assert!(matches!(pool.connections.get(&opts), Some(ConnectionPoolEntry::Connecting(_))));
                        match connection {
                            Ok(connection) => {
                                pool.connections
                                    .insert(opts.clone(), ConnectionPoolEntry::Connected(Arc::downgrade(&connection)));
                                Ok(connection)
                            }
                            Err(error) => {
                                pool.connections.remove(&opts);
                                Err(Arc::new(error))
                            }
                        }
                    })
                }
            })
            .shared();
        if let Some(task) = task.downgrade() {
            self.connections.insert(opts.clone(), ConnectionPoolEntry::Connecting(task));
        }
        task
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RemoteConnectionOptions {
    Ssh(SshConnectionOptions),
}

impl RemoteConnectionOptions {
    pub fn display_name(&self) -> String {
        match self {
            RemoteConnectionOptions::Ssh(opts) => opts.nickname.clone().unwrap_or_else(|| opts.host.to_string()),
        }
    }

    /// A stable identifier for the kind of remote connection, suitable for
    pub const fn connection_type(&self) -> &'static str {
        match self {
            RemoteConnectionOptions::Ssh(_) => "ssh",
        }
    }

    pub fn host(&self) -> String {
        match self {
            RemoteConnectionOptions::Ssh(opts) => opts.host.to_string(),
        }
    }
}

impl From<SshConnectionOptions> for RemoteConnectionOptions {
    fn from(opts: SshConnectionOptions) -> Self {
        RemoteConnectionOptions::Ssh(opts)
    }
}

// async_trait marks its boxed futures `#[must_use]`
#[allow(clippy::double_must_use)]
#[async_trait(?Send)]
pub trait RemoteConnection: Send + Sync {
    fn start_proxy(
        &self,
        unique_identifier: String,
        reconnect: bool,
        incoming_tx: UnboundedSender<Envelope>,
        outgoing_rx: UnboundedReceiver<Envelope>,
        connection_activity_tx: Sender<()>,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Task<Result<i32>>;
    fn upload_directory(&self, src_path: PathBuf, dest_path: RemotePathBuf, cx: &App) -> Task<Result<()>>;
    async fn kill(&self) -> Result<()>;
    fn has_been_killed(&self) -> bool;
    fn shares_network_interface(&self) -> bool {
        false
    }
    fn build_command(
        &self,
        program: Option<String>,
        args: &[String],
        env: &HashMap<String, String>,
        working_dir: Option<String>,
        port_forward: Option<(u16, String, u16)>,
        interactive: Interactive,
    ) -> Result<CommandTemplate>;
    fn build_stdio_command(&self, _command: RemoteCommand) -> Result<(CommandTemplate, Vec<u8>)> {
        Err(anyhow!("stdio commands are not supported by this remote connection"))
    }
    fn build_forward_ports_command(&self, forwards: Vec<(u16, String, u16)>) -> Result<CommandTemplate>;
    fn connection_options(&self) -> RemoteConnectionOptions;
    fn path_style(&self) -> PathStyle;
    /// The remote platform (OS and architecture), detected during connection setup.
    fn remote_platform(&self) -> RemotePlatform;
    /// The remote host's OS version (e.g. `"ubuntu 24.04"` or `"15.6.1"`),
    /// detected during connection setup. `None` if it could not be determined.
    fn remote_os_version(&self) -> Option<String>;
    fn shell(&self) -> String;
    fn default_system_shell(&self) -> String;
}

type ResponseChannels = Mutex<HashMap<MessageId, oneshot::Sender<(Envelope, oneshot::Sender<()>)>>>;
type StreamResponseChannels = Arc<Mutex<HashMap<MessageId, UnboundedSender<(Result<Envelope>, oneshot::Sender<()>)>>>>;

struct Signal<T: 'static> {
    tx: Mutex<Option<oneshot::Sender<T>>>,
    rx: Shared<Task<Option<T>>>,
}

impl<T: Send + Clone + 'static> Signal<T> {
    pub fn new(cx: &App) -> Self {
        let (tx, rx) = oneshot::channel();

        let task = cx.background_executor().spawn(async move { rx.await.ok() }).shared();

        Self { tx: Mutex::new(Some(tx)), rx: task }
    }

    fn set(&self, value: T) {
        if let Some(tx) = self.tx.lock().take() {
            let _ = tx.send(value);
        }
    }

    fn wait(&self) -> Shared<Task<Option<T>>> {
        self.rx.clone()
    }
}

pub(crate) struct ChannelClient {
    next_message_id: AtomicU32,
    outgoing_tx: Mutex<mpsc::UnboundedSender<Envelope>>,
    buffer: Mutex<VecDeque<Envelope>>,
    response_channels: ResponseChannels,
    stream_response_channels: StreamResponseChannels,
    message_handlers: Mutex<ProtoMessageHandlerSet>,
    max_received: AtomicU32,
    name: &'static str,
    task: Mutex<Task<Result<()>>>,
    remote_started: Signal<()>,
    executor: BackgroundExecutor,
}

impl ChannelClient {
    pub(crate) fn new(
        incoming_rx: mpsc::UnboundedReceiver<Envelope>,
        outgoing_tx: mpsc::UnboundedSender<Envelope>,
        cx: &App,
        name: &'static str,
    ) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            outgoing_tx: Mutex::new(outgoing_tx),
            next_message_id: AtomicU32::new(0),
            max_received: AtomicU32::new(0),
            response_channels: ResponseChannels::default(),
            stream_response_channels: StreamResponseChannels::default(),
            message_handlers: Default::default(),
            buffer: Mutex::new(VecDeque::new()),
            name,
            executor: cx.background_executor().clone(),
            task: Mutex::new(Self::start_handling_messages(this.clone(), incoming_rx, &cx.to_async())),
            remote_started: Signal::new(cx),
        })
    }

    fn wait_for_remote_started(&self) -> Shared<Task<Option<()>>> {
        self.remote_started.wait()
    }

    fn send_handshake(&self) -> Result<u32> {
        let handshake_id = self.next_message_id.fetch_add(1, SeqCst);
        let envelope = proto::RemoteStarted {}.into_envelope(handshake_id, None, None);
        self.outgoing_tx.lock().unbounded_send(envelope)?;
        Ok(handshake_id)
    }

    fn start_handling_messages(
        this: Weak<Self>,
        incoming_rx: mpsc::UnboundedReceiver<Envelope>,
        cx: &AsyncApp,
    ) -> Task<Result<()>> {
        cx.spawn(async move |cx| {
            let handshake_id = if let Some(this) = this.upgrade() {
                this.send_handshake()?
            } else {
                return anyhow::Ok(());
            };

            Self::read_messages(this, incoming_rx, handshake_id, cx).await
        })
    }

    async fn read_messages(
        this: Weak<Self>,
        mut incoming_rx: mpsc::UnboundedReceiver<Envelope>,
        handshake_id: u32,
        cx: &AsyncApp,
    ) -> Result<()> {
        while let Some(incoming) = incoming_rx.next().await {
            let Some(this) = this.upgrade() else {
                return anyhow::Ok(());
            };
            if incoming.responding_to == Some(handshake_id)
                && matches!(incoming.payload, Some(proto::envelope::Payload::Ack(_)))
            {
                // Handshake replies do not acknowledge buffered messages that still need replay.
                continue;
            }
            Self::handle_message(this, incoming, cx).await;
        }
        anyhow::Ok(())
    }

    async fn handle_message(this: Arc<Self>, incoming: Envelope, cx: &AsyncApp) {
        let peer_id = PeerId { owner_id: 0, id: 0 };
        if let Some(ack_id) = incoming.ack_id {
            let mut buffer = this.buffer.lock();
            while buffer.front().is_some_and(|msg| msg.id <= ack_id) {
                buffer.pop_front();
            }
        }
        if let Some(proto::envelope::Payload::FlushBufferedMessages(_)) = &incoming.payload {
            log::debug!("{}:remote message received. name:FlushBufferedMessages", this.name);
            {
                let buffer = this.buffer.lock();
                for envelope in buffer.iter() {
                    this.outgoing_tx.lock().unbounded_send(envelope.clone()).ok();
                }
            }
            let mut envelope = proto::Ack {}.into_envelope(0, Some(incoming.id), None);
            envelope.id = this.next_message_id.fetch_add(1, SeqCst);
            this.outgoing_tx.lock().unbounded_send(envelope).ok();
            return;
        }

        if let Some(proto::envelope::Payload::RemoteStarted(_)) = &incoming.payload {
            this.remote_started.set(());
            let mut envelope = proto::Ack {}.into_envelope(0, Some(incoming.id), None);
            envelope.id = this.next_message_id.fetch_add(1, SeqCst);
            this.outgoing_tx.lock().unbounded_send(envelope).ok();
            return;
        }

        this.max_received.store(incoming.id, SeqCst);

        if let Some(request_id) = incoming.responding_to {
            let request_id = MessageId(request_id);
            // An incoming response with no payload is malformed; drop
            // it. The request future and any stream consumers will
            // remain pending until either a real response arrives or
            // the connection is torn down.
            if incoming.payload.is_none() {
                return;
            }
            let sender = this.response_channels.lock().remove(&request_id);
            if let Some(sender) = sender {
                let (tx, rx) = oneshot::channel();
                sender.send((incoming, tx)).ok();
                rx.await.ok();
            } else {
                let terminal_stream_response = matches!(
                    &incoming.payload,
                    Some(proto::envelope::Payload::Error(_)) | Some(proto::envelope::Payload::EndStream(_))
                );
                let sender = if terminal_stream_response {
                    this.stream_response_channels.lock().remove(&request_id)
                } else {
                    this.stream_response_channels.lock().get(&request_id).cloned()
                };
                if let Some(sender) = sender {
                    let (tx, rx) = oneshot::channel();
                    if sender.unbounded_send((Ok(incoming), tx)).is_err() {
                        this.stream_response_channels.lock().remove(&request_id);
                        return;
                    }
                    rx.await.ok();
                }
            }
        } else if let Some(envelope) = build_typed_envelope(peer_id, Instant::now(), incoming) {
            let type_name = envelope.payload_type_name();
            let message_id = envelope.message_id();
            if let Some(future) = ProtoMessageHandlerSet::handle_message(
                &this.message_handlers,
                envelope,
                this.clone().into(),
                cx.clone(),
            ) {
                log::debug!("{}:remote message received. name:{type_name}", this.name);
                cx.foreground_executor()
                    .spawn(async move {
                        match future.await {
                            Ok(_) => {
                                log::debug!("{}:remote message handled. name:{type_name}", this.name);
                            }
                            Err(error) => {
                                log::error!(
                                    "{}:error handling message. type:{}, error:{:#}",
                                    this.name,
                                    type_name,
                                    format!("{error:#}").lines().fold(String::new(), |mut message, line| {
                                        if !message.is_empty() {
                                            message.push(' ');
                                        }
                                        message.push_str(line);
                                        message
                                    })
                                );
                            }
                        }
                    })
                    .detach()
            } else {
                log::error!("{}:unhandled remote message name:{type_name}", this.name);
                if let Err(e) = AnyProtoClient::from(this.clone())
                    .send_response(message_id, anyhow::anyhow!("no handler registered for {type_name}").to_proto())
                {
                    log::error!("{}:error sending error response for {type_name}:{e:#}", this.name);
                }
            }
        }
    }

    pub(crate) fn reconnect(
        self: &Arc<Self>,
        mut incoming_rx: UnboundedReceiver<Envelope>,
        outgoing_tx: UnboundedSender<Envelope>,
        cx: &AsyncApp,
    ) -> oneshot::Receiver<()> {
        *self.outgoing_tx.lock() = outgoing_tx;
        let handshake_id = self.send_handshake();
        let (connection_ready_tx, connection_ready_rx) = oneshot::channel();

        *self.task.lock() = cx.spawn({
            let this = Arc::downgrade(self);
            async move |cx| {
                let handshake_id = handshake_id?;

                loop {
                    let Some(incoming) = incoming_rx.next().await else {
                        return anyhow::Ok(());
                    };
                    let Some(this) = this.upgrade() else {
                        return anyhow::Ok(());
                    };
                    if incoming.responding_to == Some(handshake_id)
                        && matches!(incoming.payload, Some(proto::envelope::Payload::Ack(_)))
                    {
                        connection_ready_tx.send(()).ok();
                        break;
                    }
                    Self::handle_message(this, incoming, cx).await;
                }

                Self::read_messages(this, incoming_rx, handshake_id, cx).await
            }
        });

        connection_ready_rx
    }

    fn request<T: RequestMessage>(&self, payload: T) -> impl 'static + Future<Output = Result<T::Response>> {
        self.request_internal(payload, true)
    }

    fn request_internal<T: RequestMessage>(
        &self,
        payload: T,
        use_buffer: bool,
    ) -> impl 'static + Future<Output = Result<T::Response>> {
        log::debug!("remote request start. name:{}", T::NAME);
        let response = self.request_dynamic(payload.into_envelope(0, None, None), T::NAME, use_buffer);
        async move {
            let response = response.await?;
            log::debug!("remote request finish. name:{}", T::NAME);
            T::Response::from_envelope(response).context("received a response of the wrong type")
        }
    }

    fn resync(&self, timeout: Duration) -> impl Future<Output = Result<()>> {
        // Queue the flush before waiting for the handshake, or its ack_id could
        // cause the server to discard messages we missed while disconnected.
        let response = self.request_internal(proto::FlushBufferedMessages {}, false);

        async move {
            response.with_timeout(timeout, &self.executor).await.context("Timed out resyncing remote client")??;

            for envelope in self.buffer.lock().iter() {
                self.outgoing_tx.lock().unbounded_send(envelope.clone()).ok();
            }

            Ok(())
        }
    }

    async fn ping(&self, timeout: Duration) -> Result<()> {
        smol::future::or(
            async {
                self.request(proto::Ping {}).await?;
                Ok(())
            },
            async {
                self.executor.timer(timeout).await;
                anyhow::bail!("Timed out pinging remote client")
            },
        )
        .await
    }

    fn send<T: EnvelopedMessage>(&self, payload: T) -> Result<()> {
        log::debug!("remote send name:{}", T::NAME);
        self.send_dynamic(payload.into_envelope(0, None, None))
    }

    fn request_dynamic(
        &self,
        mut envelope: proto::Envelope,
        type_name: &'static str,
        use_buffer: bool,
    ) -> impl 'static + Future<Output = Result<proto::Envelope>> {
        envelope.id = self.next_message_id.fetch_add(1, SeqCst);
        let (tx, rx) = oneshot::channel();
        let mut response_channels_lock = self.response_channels.lock();
        response_channels_lock.insert(MessageId(envelope.id), tx);
        drop(response_channels_lock);

        let result = if use_buffer { self.send_buffered(envelope) } else { self.send_unbuffered(envelope) };
        async move {
            if let Err(error) = &result {
                log::error!("failed to send message: {error}");
                anyhow::bail!("failed to send message: {error}");
            }

            let response = rx.await.context("connection lost")?.0;
            if let Some(proto::envelope::Payload::Error(error)) = &response.payload {
                return Err(RpcError::from_proto(error, type_name));
            }
            Ok(response)
        }
    }

    fn request_stream_dynamic(
        &self,
        mut envelope: proto::Envelope,
        type_name: &'static str,
    ) -> impl 'static + Future<Output = Result<BoxStream<'static, Result<proto::Envelope>>>> {
        envelope.id = self.next_message_id.fetch_add(1, SeqCst);
        let message_id = MessageId(envelope.id);
        let (tx, rx) = mpsc::unbounded();
        let stream_response_channels = self.stream_response_channels.clone();
        stream_response_channels.lock().insert(message_id, tx);

        let result = self.send_buffered(envelope);
        async move {
            if let Err(error) = &result {
                log::error!("failed to send message: {error}");
                anyhow::bail!("failed to send message: {error}");
            }

            let cleanup_stream_response_channel = util::defer({
                let stream_response_channels = stream_response_channels.clone();
                move || {
                    stream_response_channels.lock().remove(&message_id);
                }
            });

            Ok(rx
                .filter_map(move |(response, _barrier)| {
                    // Keep the cleanup guard alive until the returned stream is dropped.
                    let _keep_cleanup_guard_alive = &cleanup_stream_response_channel;
                    futures::future::ready(match response {
                        Ok(response) => {
                            if let Some(proto::envelope::Payload::Error(error)) = &response.payload {
                                Some(Err(RpcError::from_proto(error, type_name)))
                            } else if let Some(proto::envelope::Payload::EndStream(_)) = &response.payload {
                                None
                            } else {
                                Some(Ok(response))
                            }
                        }
                        Err(error) => Some(Err(error)),
                    })
                })
                .boxed())
        }
    }

    pub fn send_dynamic(&self, mut envelope: proto::Envelope) -> Result<()> {
        envelope.id = self.next_message_id.fetch_add(1, SeqCst);
        self.send_buffered(envelope)
    }

    fn send_buffered(&self, mut envelope: proto::Envelope) -> Result<()> {
        envelope.ack_id = Some(self.max_received.load(SeqCst));
        self.buffer.lock().push_back(envelope.clone());
        // ignore errors on send (happen while we're reconnecting)
        // assume that the global "disconnected" overlay is sufficient.
        self.outgoing_tx.lock().unbounded_send(envelope).ok();
        Ok(())
    }

    fn send_unbuffered(&self, mut envelope: proto::Envelope) -> Result<()> {
        envelope.ack_id = Some(self.max_received.load(SeqCst));
        self.outgoing_tx.lock().unbounded_send(envelope).ok();
        Ok(())
    }
}

impl ProtoClient for ChannelClient {
    fn request(
        &self,
        envelope: proto::Envelope,
        request_type: &'static str,
    ) -> BoxFuture<'static, Result<proto::Envelope>> {
        self.request_dynamic(envelope, request_type, true).boxed()
    }

    fn request_stream(
        &self,
        envelope: proto::Envelope,
        request_type: &'static str,
    ) -> BoxFuture<'static, Result<BoxStream<'static, Result<proto::Envelope>>>> {
        self.request_stream_dynamic(envelope, request_type).boxed()
    }

    fn send(&self, envelope: proto::Envelope, _message_type: &'static str) -> Result<()> {
        self.send_dynamic(envelope)
    }

    fn send_response(&self, envelope: Envelope, _message_type: &'static str) -> anyhow::Result<()> {
        self.send_dynamic(envelope)
    }

    fn message_handler_set(&self) -> &Mutex<ProtoMessageHandlerSet> {
        &self.message_handlers
    }
}
