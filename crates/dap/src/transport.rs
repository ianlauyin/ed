use anyhow::{Context as _, Result, anyhow, bail};
use dap_types::{
    ErrorResponse,
    messages::{Message, Response},
};
use futures::{AsyncRead, AsyncReadExt as _, AsyncWrite, FutureExt as _, channel::oneshot, select};
use gpui::{AppContext as _, AsyncApp, BackgroundExecutor, Task};
use parking_lot::Mutex;
use proto::ErrorExt;
use settings::Settings as _;
use smallvec::SmallVec;
use smol::{
    channel::{Receiver, Sender, unbounded},
    io::{AsyncBufReadExt as _, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use task::TcpArgumentsTemplate;
use util::{ConnectionResult, ResultExt, process::Child};

use crate::{
    adapters::{DebugAdapterBinary, TcpArguments},
    client::DapMessageHandler,
    debugger_settings::DebuggerSettings,
};

pub(crate) type IoMessage = str;
pub(crate) type Command = str;
pub type IoHandler = Box<dyn Send + FnMut(IoKind, Option<&Command>, &IoMessage)>;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum LogKind {
    Adapter,
    Rpc,
}

#[derive(Clone, Copy)]
pub enum IoKind {
    StdIn,
    StdOut,
    StdErr,
}

type LogHandlers = Arc<Mutex<SmallVec<[(LogKind, IoHandler); 2]>>>;

pub trait Transport: Send + Sync {
    fn has_adapter_logs(&self) -> bool;
    fn tcp_arguments(&self) -> Option<TcpArguments>;
    fn connect(
        &mut self,
    ) -> Task<
        Result<(
            Box<dyn AsyncWrite + Unpin + Send + 'static>,
            Box<dyn AsyncRead + Unpin + Send + 'static>,
        )>,
    >;
    fn kill(&mut self);
}

async fn start(
    binary: &DebugAdapterBinary,
    log_handlers: LogHandlers,
    cx: &mut AsyncApp,
) -> Result<Box<dyn Transport>> {
    if binary.connection.is_some() {
        Ok(Box::new(
            TcpTransport::start(binary, log_handlers, cx).await?,
        ))
    } else {
        Ok(Box::new(
            StdioTransport::start(binary, log_handlers, cx).await?,
        ))
    }
}

pub(crate) struct PendingRequests {
    inner: Option<HashMap<u64, oneshot::Sender<Result<Response>>>>,
}

impl PendingRequests {
    fn new() -> Self {
        Self {
            inner: Some(HashMap::default()),
        }
    }

    fn flush(&mut self, e: anyhow::Error) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        for (_, sender) in inner.drain() {
            sender.send(Err(e.cloned())).ok();
        }
    }

    pub(crate) fn insert(
        &mut self,
        sequence_id: u64,
        callback_tx: oneshot::Sender<Result<Response>>,
    ) -> anyhow::Result<()> {
        let Some(inner) = self.inner.as_mut() else {
            bail!("client is closed")
        };
        inner.insert(sequence_id, callback_tx);
        Ok(())
    }

    pub(crate) fn remove(
        &mut self,
        sequence_id: u64,
    ) -> anyhow::Result<Option<oneshot::Sender<Result<Response>>>> {
        let Some(inner) = self.inner.as_mut() else {
            bail!("client is closed");
        };
        Ok(inner.remove(&sequence_id))
    }

    pub(crate) fn shutdown(&mut self) {
        self.flush(anyhow!("transport shutdown"));
        self.inner = None;
    }
}

pub(crate) struct TransportDelegate {
    log_handlers: LogHandlers,
    pub(crate) pending_requests: Arc<Mutex<PendingRequests>>,
    pub(crate) transport: Mutex<Box<dyn Transport>>,
    pub(crate) server_tx: smol::lock::Mutex<Option<Sender<Message>>>,
    tasks: Mutex<Vec<Task<()>>>,
}

impl TransportDelegate {
    pub(crate) async fn start(binary: &DebugAdapterBinary, cx: &mut AsyncApp) -> Result<Self> {
        let log_handlers: LogHandlers = Default::default();
        let transport = start(binary, log_handlers.clone(), cx).await?;
        Ok(Self {
            transport: Mutex::new(transport),
            log_handlers,
            server_tx: Default::default(),
            pending_requests: Arc::new(Mutex::new(PendingRequests::new())),
            tasks: Default::default(),
        })
    }

    pub async fn connect(
        &self,
        message_handler: DapMessageHandler,
        cx: &mut AsyncApp,
    ) -> Result<()> {
        let (server_tx, client_rx) = unbounded::<Message>();
        self.tasks.lock().clear();

        let log_dap_communications =
            cx.update(|cx| DebuggerSettings::get_global(cx).log_dap_communications);

        let connect = self.transport.lock().connect();
        let (input, output) = connect.await?;

        let log_handler = if log_dap_communications {
            Some(self.log_handlers.clone())
        } else {
            None
        };

        let pending_requests = self.pending_requests.clone();
        let output_log_handler = log_handler.clone();
        {
            let mut tasks = self.tasks.lock();
            tasks.push(cx.background_spawn(async move {
                match Self::recv_from_server(
                    output,
                    message_handler,
                    pending_requests.clone(),
                    output_log_handler,
                )
                .await
                {
                    Ok(()) => {
                        pending_requests
                            .lock()
                            .flush(anyhow!("debugger shutdown unexpectedly"));
                    }
                    Err(e) => {
                        pending_requests.lock().flush(e);
                    }
                }
            }));

            tasks.push(cx.background_spawn(async move {
                match Self::send_to_server(input, client_rx, log_handler).await {
                    Ok(()) => {}
                    Err(e) => log::error!("Error handling debugger input: {e}"),
                }
            }));
        }

        *self.server_tx.lock().await = Some(server_tx.clone());

        Ok(())
    }

    pub(crate) fn tcp_arguments(&self) -> Option<TcpArguments> {
        self.transport.lock().tcp_arguments()
    }

    pub(crate) async fn send_message(&self, message: Message) -> Result<()> {
        if let Some(server_tx) = self.server_tx.lock().await.as_ref() {
            server_tx.send(message).await.context("sending message")
        } else {
            anyhow::bail!("Server tx already dropped")
        }
    }

    async fn handle_adapter_log(
        stdout: impl AsyncRead + Unpin + Send + 'static,
        iokind: IoKind,
        log_handlers: LogHandlers,
    ) {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        loop {
            line.clear();

            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    log::debug!("handle_adapter_log: {}", e);
                    break;
                }
            }

            // Clean up logs by trimming unnecessary whitespace/newlines before inserting into log.
            let line = line.trim();

            log::debug!("stderr: {line}");

            for (kind, handler) in log_handlers.lock().iter_mut() {
                if matches!(kind, LogKind::Adapter) {
                    handler(iokind, None, line);
                }
            }
        }
    }

    fn build_rpc_message(message: String) -> String {
        format!("Content-Length: {}\r\n\r\n{}", message.len(), message)
    }

    async fn send_to_server<Stdin>(
        mut server_stdin: Stdin,
        client_rx: Receiver<Message>,
        log_handlers: Option<LogHandlers>,
    ) -> Result<()>
    where
        Stdin: AsyncWrite + Unpin + Send + 'static,
    {
        let result = loop {
            match client_rx.recv().await {
                Ok(message) => {
                    let command = match &message {
                        Message::Request(request) => Some(request.command.as_str()),
                        Message::Response(response) => Some(response.command.as_str()),
                        _ => None,
                    };

                    let message = match serde_json::to_string(&message) {
                        Ok(message) => message,
                        Err(e) => break Err(e.into()),
                    };

                    if let Some(log_handlers) = log_handlers.as_ref() {
                        for (kind, log_handler) in log_handlers.lock().iter_mut() {
                            if matches!(kind, LogKind::Rpc) {
                                log_handler(IoKind::StdIn, command, &message);
                            }
                        }
                    }

                    if let Err(e) = server_stdin
                        .write_all(Self::build_rpc_message(message).as_bytes())
                        .await
                    {
                        break Err(e.into());
                    }

                    if let Err(e) = server_stdin.flush().await {
                        break Err(e.into());
                    }
                }
                Err(error) => break Err(error.into()),
            }
        };

        log::debug!("Handle adapter input dropped");

        result
    }

    async fn recv_from_server<Stdout>(
        server_stdout: Stdout,
        mut message_handler: DapMessageHandler,
        pending_requests: Arc<Mutex<PendingRequests>>,
        log_handlers: Option<LogHandlers>,
    ) -> Result<()>
    where
        Stdout: AsyncRead + Unpin + Send + 'static,
    {
        let mut recv_buffer = String::new();
        let mut reader = BufReader::new(server_stdout);

        let result = loop {
            let result =
                Self::receive_server_message(&mut reader, &mut recv_buffer, log_handlers.as_ref())
                    .await;
            match result {
                ConnectionResult::Timeout => anyhow::bail!("Timed out when connecting to debugger"),
                ConnectionResult::ConnectionReset => {
                    log::info!("Debugger closed the connection");
                    return Ok(());
                }
                ConnectionResult::Result(Ok(Message::Response(res))) => {
                    let tx = pending_requests.lock().remove(res.request_seq)?;
                    if let Some(tx) = tx {
                        if let Err(e) = tx.send(Self::process_response(res)) {
                            log::trace!("Did not send response `{:?}` for a cancelled", e);
                        }
                    } else {
                        message_handler(Message::Response(res))
                    }
                }
                ConnectionResult::Result(Ok(message)) => message_handler(message),
                ConnectionResult::Result(Err(e)) => break Err(e),
            }
        };

        log::debug!("Handle adapter output dropped");

        result
    }

    fn process_response(response: Response) -> Result<Response> {
        if response.success {
            Ok(response)
        } else {
            if let Some(error_message) = response
                .body
                .clone()
                .and_then(|body| serde_json::from_value::<ErrorResponse>(body).ok())
                .and_then(|response| {
                    response.error.map(|msg| {
                        let Some(variables) = msg
                            .variables
                            .as_ref()
                            .and_then(serde_json::Value::as_object)
                            .filter(|variables| !variables.is_empty())
                        else {
                            return msg.format;
                        };

                        interpolate_message(msg.format, variables)
                    })
                })
                .or_else(|| response.message.clone())
            {
                anyhow::bail!(error_message);
            };

            anyhow::bail!(
                "Received error response from adapter. Response: {:?}",
                response
            );
        }
    }

    async fn receive_server_message<Stdout>(
        reader: &mut BufReader<Stdout>,
        buffer: &mut String,
        log_handlers: Option<&LogHandlers>,
    ) -> ConnectionResult<Message>
    where
        Stdout: AsyncRead + Unpin + Send + 'static,
    {
        let mut content_length = None;
        loop {
            buffer.clear();
            match reader.read_line(buffer).await {
                Ok(0) => return ConnectionResult::ConnectionReset,
                Ok(_) => {}
                Err(e) => return ConnectionResult::Result(Err(e.into())),
            };

            if buffer == "\r\n" {
                break;
            }

            if let Some(("Content-Length", value)) = buffer.trim().split_once(": ") {
                match value.parse().context("invalid content length") {
                    Ok(length) => content_length = Some(length),
                    Err(e) => return ConnectionResult::Result(Err(e)),
                }
            }
        }

        let content_length = match content_length.context("missing content length") {
            Ok(length) => length,
            Err(e) => return ConnectionResult::Result(Err(e)),
        };

        let mut content = vec![0; content_length];
        if let Err(e) = reader
            .read_exact(&mut content)
            .await
            .with_context(|| "reading after a loop")
        {
            return ConnectionResult::Result(Err(e));
        }

        let message_str = match std::str::from_utf8(&content).context("invalid utf8 from server") {
            Ok(str) => str,
            Err(e) => return ConnectionResult::Result(Err(e)),
        };

        let message =
            serde_json::from_str::<Message>(message_str).context("deserializing server message");

        if let Some(log_handlers) = log_handlers {
            let command = match &message {
                Ok(Message::Request(request)) => Some(request.command.as_str()),
                Ok(Message::Response(response)) => Some(response.command.as_str()),
                _ => None,
            };

            for (kind, log_handler) in log_handlers.lock().iter_mut() {
                if matches!(kind, LogKind::Rpc) {
                    log_handler(IoKind::StdOut, command, message_str);
                }
            }
        }

        ConnectionResult::Result(message)
    }

    pub fn has_adapter_logs(&self) -> bool {
        self.transport.lock().has_adapter_logs()
    }

    pub fn add_log_handler<F>(&self, f: F, kind: LogKind)
    where
        F: 'static + Send + FnMut(IoKind, Option<&Command>, &IoMessage),
    {
        let mut log_handlers = self.log_handlers.lock();
        log_handlers.push((kind, Box::new(f)));
    }
}

fn interpolate_message(
    format: String,
    variables: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format.as_str();

    loop {
        let Some(start) = rest.find('{') else {
            out.push_str(rest);
            break;
        };

        out.push_str(&rest[..start]);
        let placeholder = &rest[start..];

        let Some(end) = placeholder.find('}') else {
            out.push_str(placeholder);
            break;
        };

        let name = &placeholder[1..end];
        if name.is_empty() || name.contains('{') {
            out.push_str(&placeholder[..=end]);
        } else {
            match variables.get(name) {
                Some(serde_json::Value::String(value)) => out.push_str(value),
                Some(value) => out.push_str(&value.to_string()),
                None => out.push_str(&placeholder[..=end]),
            }
        }

        rest = &placeholder[end + 1..];
    }

    out
}

pub struct TcpTransport {
    executor: BackgroundExecutor,
    pub port: u16,
    pub host: IpAddr,
    pub timeout: u64,
    process: Arc<Mutex<Option<Child>>>,
    _stderr_task: Option<Task<()>>,
    _stdout_task: Option<Task<()>>,
}

impl TcpTransport {
    /// Get an open port to use with the tcp client when not supplied by debug config
    pub async fn port(host: &TcpArgumentsTemplate) -> Result<u16> {
        if let Some(port) = host.port {
            Ok(port)
        } else {
            Self::unused_port(host.host()).await
        }
    }

    pub async fn unused_port(host: IpAddr) -> Result<u16> {
        Ok(TcpListener::bind(SocketAddr::new(host, 0))
            .await?
            .local_addr()?
            .port())
    }

    async fn start(
        binary: &DebugAdapterBinary,
        log_handlers: LogHandlers,
        cx: &mut AsyncApp,
    ) -> Result<Self> {
        let connection_args = binary
            .connection
            .as_ref()
            .context("No connection arguments provided")?;

        let host = connection_args.host;
        let port = connection_args.port;

        let mut process = None;
        let mut stdout_task = None;
        let mut stderr_task = None;

        if let Some(command) = &binary.command {
            let mut command = util::command::new_std_command(&command);

            if let Some(cwd) = &binary.cwd {
                command.current_dir(cwd);
            }

            command.args(&binary.arguments);
            command.envs(&binary.envs);

            let mut p = Child::spawn(command, Stdio::null(), Stdio::piped(), Stdio::piped())
                .with_context(|| "failed to start debug adapter.")?;

            stdout_task = p.stdout.take().map(|stdout| {
                cx.background_executor()
                    .spawn(TransportDelegate::handle_adapter_log(
                        stdout,
                        IoKind::StdOut,
                        log_handlers.clone(),
                    ))
            });
            stderr_task = p.stderr.take().map(|stderr| {
                cx.background_executor()
                    .spawn(TransportDelegate::handle_adapter_log(
                        stderr,
                        IoKind::StdErr,
                        log_handlers,
                    ))
            });
            process = Some(p);
        };

        let timeout = connection_args
            .timeout
            .unwrap_or_else(|| cx.update(|cx| DebuggerSettings::get_global(cx).timeout));

        log::info!(
            "Debug adapter has connected to TCP server {}:{}",
            host,
            port
        );

        let this = Self {
            executor: cx.background_executor().clone(),
            port,
            host,
            process: Arc::new(Mutex::new(process)),
            timeout,
            _stdout_task: stdout_task,
            _stderr_task: stderr_task,
        };

        Ok(this)
    }
}

impl Transport for TcpTransport {
    fn has_adapter_logs(&self) -> bool {
        true
    }

    fn kill(&mut self) {
        if let Some(process) = &mut *self.process.lock() {
            process.kill().log_err();
        }
    }

    fn tcp_arguments(&self) -> Option<TcpArguments> {
        Some(TcpArguments {
            host: self.host,
            port: self.port,
            timeout: Some(self.timeout),
        })
    }

    fn connect(
        &mut self,
    ) -> Task<
        Result<(
            Box<dyn AsyncWrite + Unpin + Send + 'static>,
            Box<dyn AsyncRead + Unpin + Send + 'static>,
        )>,
    > {
        let executor = self.executor.clone();
        let timeout = self.timeout;
        let address = SocketAddr::new(self.host, self.port);
        let process = self.process.clone();
        executor.clone().spawn(async move {
            select! {
                _ = executor.timer(Duration::from_millis(timeout)).fuse() => {
                    anyhow::bail!("Connection to TCP DAP timeout {address}");
                },
                result = executor.clone().spawn(async move {
                    loop {
                        match TcpStream::connect(address).await {
                            Ok(stream) => {
                                let (read, write) = stream.split();
                                return Ok((Box::new(write) as _, Box::new(read) as _))
                            },
                            Err(_) => {
                                let has_process = process.lock().is_some();
                                if has_process {
                                    let status = process.lock().as_mut().unwrap().try_status();
                                    if let Ok(Some(_)) = status {
                                        let child = process.lock().take().unwrap();
                                        let output = child.output().await?;
                                        let output = if output.stderr.is_empty() {
                                            String::from_utf8_lossy(&output.stdout).to_string()
                                        } else {
                                            String::from_utf8_lossy(&output.stderr).to_string()
                                        };
                                        anyhow::bail!("{output}\nerror: process exited before debugger attached.");
                                    }
                                }

                                executor.timer(Duration::from_millis(100)).await;
                            }
                        }
                    }
                }).fuse() => result
            }
        })
    }
}

impl Drop for TcpTransport {
    fn drop(&mut self) {
        if let Some(mut p) = self.process.lock().take() {
            p.kill().log_err();
        }
    }
}

pub struct StdioTransport {
    process: Mutex<Child>,
    _stderr_task: Option<Task<()>>,
}

impl StdioTransport {
    // #[allow(dead_code, reason = "This is used in non test builds of Zed")]
    async fn start(
        binary: &DebugAdapterBinary,
        log_handlers: LogHandlers,
        cx: &mut AsyncApp,
    ) -> Result<Self> {
        let Some(binary_command) = &binary.command else {
            bail!(
                "When using the `stdio` transport, the path to a debug adapter binary must be set by Zed."
            );
        };
        let mut command = util::command::new_std_command(&binary_command);

        if let Some(cwd) = &binary.cwd {
            command.current_dir(cwd);
        }

        command.args(&binary.arguments);
        command.envs(&binary.envs);

        let mut process = Child::spawn(command, Stdio::piped(), Stdio::piped(), Stdio::piped())?;

        let _stderr_task = process.stderr.take().map(|stderr| {
            cx.background_spawn(TransportDelegate::handle_adapter_log(
                stderr,
                IoKind::StdErr,
                log_handlers,
            ))
        });

        let process = Mutex::new(process);

        Ok(Self {
            process,
            _stderr_task,
        })
    }
}

impl Transport for StdioTransport {
    fn has_adapter_logs(&self) -> bool {
        true
    }

    fn kill(&mut self) {
        self.process.lock().kill().log_err();
    }

    fn connect(
        &mut self,
    ) -> Task<
        Result<(
            Box<dyn AsyncWrite + Unpin + Send + 'static>,
            Box<dyn AsyncRead + Unpin + Send + 'static>,
        )>,
    > {
        let result = util::maybe!({
            let mut process = self.process.lock();
            Ok((
                Box::new(process.stdin.take().context("Cannot reconnect")?) as _,
                Box::new(process.stdout.take().context("Cannot reconnect")?) as _,
            ))
        });
        Task::ready(result)
    }

    fn tcp_arguments(&self) -> Option<TcpArguments> {
        None
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.process.lock().kill().log_err();
    }
}
