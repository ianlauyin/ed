use crate::{
    RemoteArch, RemoteClientDelegate, RemoteOs, RemotePlatform,
    command::{RemoteCommand, home_relative_path, home_stdio_launcher_command},
    remote_client::{CommandTemplate, Interactive, RemoteConnection, RemoteConnectionOptions},
    transport::{HOME_DIR_ARGS, HOME_DIR_PROGRAM, parse_home_dir, parse_platform, parse_shell},
};
use anyhow::{Context as _, Result, anyhow};
use async_trait::async_trait;
use collections::HashMap;
use futures::{
    AsyncReadExt as _, FutureExt as _,
    channel::mpsc::{Sender, UnboundedReceiver, UnboundedSender},
    select_biased,
};
use gpui::{App, AppContext as _, AsyncApp, Task};
use parking_lot::Mutex;
use rpc::proto::Envelope;
pub use settings_content::SshPortForwardOption;
use std::{
    fmt::Write as _,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use util::command::{Child, Stdio};
use util::{
    paths::{PathStyle, RemotePathBuf},
    rel_path::RelPath,
    shell::ShellKind,
};

/// How long to wait for SSH to establish a connection.
const SSH_CONNECTION_TIMEOUT: Duration = Duration::from_secs(17);

pub(crate) struct SshRemoteConnection {
    socket: SshSocket,
    master_process: Mutex<Option<MasterProcess>>,
    /// Whether `kill()` has been called. Separate from `master_process` because
    /// reused ControlMaster sessions start with `master_process` as `None`.
    killed: AtomicBool,
    remote_binary_path: Option<Arc<RelPath>>,
    ssh_platform: RemotePlatform,
    ssh_os_version: Option<String>,
    ssh_path_style: PathStyle,
    ssh_shell: String,
    ssh_shell_kind: ShellKind,
    ssh_default_system_shell: String,
    ssh_home_dir: Option<String>,
    _temp_dir: TempDir,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SshConnectionHost {
    IpAddr(IpAddr),
    Hostname(String),
}

impl SshConnectionHost {
    pub fn to_bracketed_string(&self) -> String {
        match self {
            Self::IpAddr(IpAddr::V4(ip)) => ip.to_string(),
            Self::IpAddr(IpAddr::V6(ip)) => format!("[{}]", ip),
            Self::Hostname(hostname) => hostname.clone(),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::IpAddr(ip) => ip.to_string(),
            Self::Hostname(hostname) => hostname.clone(),
        }
    }
}

impl From<&str> for SshConnectionHost {
    fn from(value: &str) -> Self {
        if let Ok(address) = value.parse() {
            Self::IpAddr(address)
        } else {
            Self::Hostname(value.to_string())
        }
    }
}

impl From<String> for SshConnectionHost {
    fn from(value: String) -> Self {
        if let Ok(address) = value.parse() {
            Self::IpAddr(address)
        } else {
            Self::Hostname(value)
        }
    }
}

impl Default for SshConnectionHost {
    fn default() -> Self {
        Self::Hostname(Default::default())
    }
}

fn bracket_ipv6(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]", host)
    } else {
        host.to_string()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SshConnectionOptions {
    pub host: SshConnectionHost,
    pub username: Option<String>,
    pub port: Option<u16>,
    pub args: Option<Vec<String>>,
    pub port_forwards: Option<Vec<SshPortForwardOption>>,
    pub connection_timeout: Option<u16>,

    pub nickname: Option<String>,
    pub upload_binary_over_ssh: bool,
}

impl From<settings_content::SshConnection> for SshConnectionOptions {
    fn from(val: settings_content::SshConnection) -> Self {
        SshConnectionOptions {
            host: val.host.to_string().into(),
            username: val.username,
            port: val.port,
            args: Some(val.args),
            nickname: val.nickname,
            upload_binary_over_ssh: val.upload_binary_over_ssh.unwrap_or_default(),
            port_forwards: val.port_forwards,
            connection_timeout: val.connection_timeout,
        }
    }
}

struct SshSocket {
    connection_options: SshConnectionOptions,

    socket_path: std::path::PathBuf,
    /// Extra environment variables needed for the ssh process
    envs: HashMap<String, String>,
}

struct RemoteEnvironment {
    shell: String,
    platform: RemotePlatform,
    os_version: Option<String>,
}

impl RemoteEnvironment {
    fn posix_script() -> String {
        r#"
            platform=$(uname -sm) || exit;
            printf "\000%s\000%s\000" "$SHELL" "$platform";
            status=0;
            case "$platform" in
                "Linux "*) cat /etc/os-release || status=$?;;
                "Darwin "*) sw_vers -productVersion || status=$?;;
            esac;
            printf "\000%s" "$status";
        "#
        // Remove extra whitespaces
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
    }

    fn windows_script() -> String {
        r#"
            $shell = "";
            try {
                $process = Get-CimInstance Win32_Process -Filter "ProcessId=$PID" -ErrorAction Stop;
                $shell = (Get-CimInstance Win32_Process -Filter "ProcessId=$($process.ParentProcessId)" -ErrorAction Stop).Name;
            } catch {
                Write-Error -ErrorRecord $_ -ErrorAction Continue;
            };

            $architecture = $env:PROCESSOR_ARCHITEW6432;
            if (-not $architecture) {
                $architecture = $env:PROCESSOR_ARCHITECTURE;
            };

            $version = "";
            $status = 1;
            try {
                $version = cmd.exe /c ver;
                $status = $LASTEXITCODE;
            } catch {
                Write-Error -ErrorRecord $_ -ErrorAction Continue;
            };

            Write-Output ([string]::Join([char]0, @("", $shell, ($architecture -join "`n"), ($version -join "`n"), $status)));
        "#
        // Remove extra whitespaces
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
    }

    fn parse_posix(output: &str) -> Result<Self> {
        let mut fields = output.splitn(5, '\0').skip(1);

        let shell = parse_shell(fields.next().context("Missing remote shell")?, "sh");
        let platform = parse_platform(fields.next().context("Missing remote platform")?)?;
        let version = fields.next().context("Missing remote OS version")?;
        let status = fields
            .next()
            .context("Missing remote OS version exit status")?;
        let os_version = if status == "0" {
            super::parse_os_version(platform.os, version)
        } else {
            log::warn!("Failed to determine remote OS version: exit status {status}");
            None
        };

        Ok(Self {
            shell,
            platform,
            os_version,
        })
    }

    fn parse_windows(output: &str) -> Result<Self> {
        let mut fields = output.splitn(5, '\0').skip(1);

        let shell = parse_shell(fields.next().context("Missing remote shell")?, "cmd.exe");
        let architecture = fields
            .next()
            .context("Missing remote Windows architecture")?
            .trim();
        let platform = RemotePlatform {
            os: RemoteOs::Windows,
            arch: match architecture {
                "AMD64" => RemoteArch::X86_64,
                "ARM64" => RemoteArch::Aarch64,
                architecture => anyhow::bail!(
                    "Prebuilt remote servers are not yet available for windows-{architecture}. See https://zed.dev/docs/remote-development"
                ),
            },
        };
        let version = fields.next().context("Missing remote OS version")?;
        let status = fields
            .next()
            .context("Missing remote OS version exit status")?
            .trim();
        let os_version = if status == "0" {
            super::parse_os_version(RemoteOs::Windows, version)
        } else {
            log::warn!("Failed to determine remote OS version: exit status {status}");
            None
        };

        Ok(Self {
            shell,
            platform,
            os_version,
        })
    }
}

struct MasterProcess {
    process: Child,
}

impl MasterProcess {
    pub fn new(
        additional_args: Vec<String>,
        socket_path: &std::path::Path,
        destination: &str,
    ) -> Result<Self> {
        let args = [
            "-N",
            "-o",
            "ControlPersist=no",
            "-o",
            "ControlMaster=yes",
            "-o",
        ];

        let mut master_process = util::command::new_command("ssh");
        master_process
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("SSH_ASKPASS", "/usr/bin/false")
            .args(additional_args)
            .args(args);

        master_process.arg(format!("ControlPath={}", socket_path.display()));

        let process = master_process.arg(&destination).spawn()?;

        Ok(MasterProcess { process })
    }

    pub async fn wait_connected(&mut self) -> Result<()> {
        let Some(mut stdout) = self.process.stdout.take() else {
            anyhow::bail!("ssh process stdout capture failed");
        };

        let mut output = Vec::new();
        stdout.read_to_end(&mut output).await?;
        Ok(())
    }
}

impl AsRef<Child> for MasterProcess {
    fn as_ref(&self) -> &Child {
        &self.process
    }
}

impl AsMut<Child> for MasterProcess {
    fn as_mut(&mut self) -> &mut Child {
        &mut self.process
    }
}

#[async_trait(?Send)]
impl RemoteConnection for SshRemoteConnection {
    async fn kill(&self) -> Result<()> {
        self.killed.store(true, Ordering::Release);
        let Some(mut process) = self.master_process.lock().take() else {
            log::debug!("no master process to kill (external ControlMaster session)");
            return Ok(());
        };
        process.as_mut().kill().ok();
        process.as_mut().status().await?;
        Ok(())
    }

    fn has_been_killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }

    fn connection_options(&self) -> RemoteConnectionOptions {
        RemoteConnectionOptions::Ssh(self.socket.connection_options.clone())
    }

    fn shell(&self) -> String {
        self.ssh_shell.clone()
    }

    fn default_system_shell(&self) -> String {
        self.ssh_default_system_shell.clone()
    }

    fn build_command(
        &self,
        input_program: Option<String>,
        input_args: &[String],
        input_env: &HashMap<String, String>,
        working_dir: Option<String>,
        port_forward: Option<(u16, String, u16)>,
        interactive: Interactive,
    ) -> Result<CommandTemplate> {
        let Self {
            ssh_path_style,
            socket,
            ssh_shell_kind,
            ssh_shell,
            ..
        } = self;
        let env = socket.envs.clone();

        if self.ssh_platform.os.is_windows() {
            build_command_windows(
                input_program,
                input_args,
                input_env,
                working_dir,
                port_forward,
                env,
                *ssh_path_style,
                ssh_shell,
                *ssh_shell_kind,
                socket.ssh_command_options(),
                &socket.connection_options.ssh_destination(),
                interactive,
            )
        } else {
            build_command_posix(
                input_program,
                input_args,
                input_env,
                working_dir,
                port_forward,
                env,
                *ssh_path_style,
                ssh_shell,
                *ssh_shell_kind,
                socket.ssh_command_options(),
                &socket.connection_options.ssh_destination(),
                interactive,
            )
        }
    }

    fn build_stdio_command(
        &self,
        mut command: RemoteCommand,
    ) -> Result<(CommandTemplate, Vec<u8>)> {
        if self.ssh_platform.os.is_windows() {
            let template = self.build_command(
                Some(command.program),
                &command.args,
                &HashMap::default(),
                command.working_dir,
                None,
                Interactive::No,
            )?;
            return Ok((template, Vec::new()));
        }
        let remote_binary_path = self
            .remote_binary_path
            .as_ref()
            .context("Remote binary path not set")?;
        command.retain_valid_env();
        let mut exec = posix_working_dir_prefix(
            command.working_dir.take(),
            self.ssh_path_style,
            self.ssh_shell_kind,
        )?;
        exec.push_str(&home_stdio_launcher_command(
            self.ssh_shell_kind,
            self.ssh_home_dir.as_deref(),
            &remote_binary_path.display(PathStyle::Unix),
        )?);
        let template = ssh_command_template(
            self.socket.ssh_command_options(),
            None,
            Interactive::No,
            &self.socket.connection_options.ssh_destination(),
            exec,
            self.socket.envs.clone(),
        );
        Ok((template, command.encode()?))
    }

    fn build_forward_ports_command(
        &self,
        forwards: Vec<(u16, String, u16)>,
    ) -> Result<CommandTemplate> {
        let Self { socket, .. } = self;
        let mut args = socket.ssh_command_options();
        args.push("-N".into());
        for (local_port, host, remote_port) in forwards {
            args.push("-L".into());
            args.push(format!(
                "{}:{}:{}",
                local_port,
                bracket_ipv6(&host),
                remote_port
            ));
        }
        args.push(socket.connection_options.ssh_destination());
        Ok(CommandTemplate {
            program: "ssh".into(),
            args,
            env: Default::default(),
        })
    }

    fn upload_directory(
        &self,
        src_path: PathBuf,
        dest_path: RemotePathBuf,
        cx: &App,
    ) -> Task<Result<()>> {
        let dest_path_str = dest_path.to_string();
        let src_path_display = src_path.display().to_string();

        let mut sftp_command = self.build_sftp_command();
        sftp_command.kill_on_drop(true);
        let mut scp_command =
            self.build_scp_command(&src_path, &dest_path_str, Some(&["-C", "-r"]));
        scp_command.kill_on_drop(true);

        cx.background_spawn(async move {
            // We will try SFTP first, and if that fails, we will fall back to SCP.
            // If SCP fails also, we give up and return an error.
            if Self::is_sftp_available().await {
                log::debug!("using SFTP for directory upload");
                let mut child = sftp_command.spawn()?;
                if let Some(mut stdin) = child.stdin.take() {
                    use futures::AsyncWriteExt;
                    let sftp_batch = format!("put -r \"{src_path_display}\" \"{dest_path_str}\"\n");
                    stdin.write_all(sftp_batch.as_bytes()).await?;
                    stdin.flush().await?;
                }

                let output = child.output().await?;
                if output.status.success() {
                    return Ok(());
                }

                let stderr = String::from_utf8_lossy(&output.stderr);
                log::debug!("failed to upload directory via SFTP {src_path_display} -> {dest_path_str}: {stderr}");
            }

            log::debug!("using SCP for directory upload");
            let output = scp_command.output().await?;

            if output.status.success() {
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            log::debug!("failed to upload directory via SCP {src_path_display} -> {dest_path_str}: {stderr}");

            anyhow::bail!(
                "failed to upload directory via SFTP/SCP {} -> {}: {}",
                src_path_display,
                dest_path_str,
                stderr,
            );
        })
    }

    fn start_proxy(
        &self,
        unique_identifier: String,
        reconnect: bool,
        incoming_tx: UnboundedSender<Envelope>,
        outgoing_rx: UnboundedReceiver<Envelope>,
        connection_activity_tx: Sender<()>,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Task<Result<i32>> {
        const VARS: [&str; 3] = ["RUST_LOG", "RUST_BACKTRACE", "ZED_GENERATE_MINIDUMPS"];
        delegate.set_status(Some("Starting proxy"), cx);

        let Some(remote_binary_path) = self.remote_binary_path.clone() else {
            return Task::ready(Err(anyhow!("Remote binary path not set")));
        };

        let mut ssh_command = if self.ssh_platform.os.is_windows() {
            // TODO: Set the `VARS` environment variables, we do not have `env` on windows
            // so this needs a different approach
            let mut proxy_args = vec![];
            proxy_args.push("proxy".to_owned());
            proxy_args.push("--identifier".to_owned());
            proxy_args.push(unique_identifier);

            if reconnect {
                proxy_args.push("--reconnect".to_owned());
            }
            self.socket.ssh_command(
                self.ssh_shell_kind,
                &remote_binary_path.display(self.path_style()),
                &proxy_args,
                false,
            )
        } else {
            let mut proxy_args = vec![];
            for env_var in VARS {
                if let Some(value) = std::env::var(env_var).ok() {
                    proxy_args.push(format!("{env_var}={value}"));
                }
            }
            proxy_args.push(remote_binary_path.display(self.path_style()).into_owned());
            proxy_args.push("proxy".to_owned());
            proxy_args.push("--identifier".to_owned());
            proxy_args.push(unique_identifier);

            if reconnect {
                proxy_args.push("--reconnect".to_owned());
            }
            self.socket
                .ssh_command(self.ssh_shell_kind, "env", &proxy_args, false)
        };

        let ssh_proxy_process = match ssh_command
            // IMPORTANT: we kill this process when we drop the task that uses it.
            .kill_on_drop(true)
            .spawn()
        {
            Ok(process) => process,
            Err(error) => {
                return Task::ready(Err(
                    anyhow::Error::new(error).context("failed to spawn remote server")
                ));
            }
        };

        super::handle_rpc_messages_over_child_process_stdio(
            ssh_proxy_process,
            incoming_tx,
            outgoing_rx,
            connection_activity_tx,
            cx,
        )
    }

    fn path_style(&self) -> PathStyle {
        self.ssh_path_style
    }

    fn remote_platform(&self) -> RemotePlatform {
        self.ssh_platform
    }

    fn remote_os_version(&self) -> Option<String> {
        self.ssh_os_version.clone()
    }

    fn has_wsl_interop(&self) -> bool {
        false
    }
}

/// Check if the user already has an active SSH ControlMaster session for the
/// given destination. See: https://github.com/zed-industries/zed/issues/45271
async fn find_existing_control_master(
    destination: &str,
    additional_args: &[String],
) -> Option<PathBuf> {
    // Use `ssh -G` to resolve the user's effective SSH config for this host.
    // This expands ControlPath tokens (%h, %p, %r, %C, etc.) into actual paths.
    let output = match util::command::new_command("ssh")
        .args(additional_args)
        .arg("-G")
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("failed to run ssh -G: {e}");
            return None;
        }
    };

    if !output.status.success() {
        log::debug!("ssh -G failed for {destination}, skipping ControlMaster reuse");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let control_path = stdout.lines().find_map(|line| {
        let path = line.strip_prefix("controlpath ")?.trim();
        if path == "none" || path.is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        }
    })?;

    // Verify the master is actually alive by sending a control command.
    let check = match util::command::new_command("ssh")
        .args(additional_args)
        .args(["-O", "check"])
        .arg("-o")
        .arg(format!("ControlPath={}", control_path.display()))
        .arg(destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("failed to run ssh -O check: {e}");
            return None;
        }
    };

    if check.status.success() {
        log::info!(
            "reusing existing SSH ControlMaster at {}",
            control_path.display()
        );
        Some(control_path)
    } else {
        log::debug!(
            "ControlMaster socket at {} is not alive, creating new connection",
            control_path.display()
        );
        None
    }
}

impl SshRemoteConnection {
    pub(crate) async fn new(
        connection_options: SshConnectionOptions,
        known_os: Option<RemoteOs>,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Result<Self> {
        let destination = connection_options.ssh_destination();

        let temp_dir = tempfile::Builder::new()
            .prefix("zed-ssh-session")
            .tempdir()?;

        // On non-Windows, check if the user already has an active ControlMaster
        // session for this host. If so, reuse it without authenticating again.

        let reused_socket =
            find_existing_control_master(&destination, &connection_options.additional_args()).await;

        let (socket, master_process_option) = if let Some(reused_path) = reused_socket {
            delegate.set_status(Some("Connecting (reusing session)"), cx);
            log::info!("reusing existing ControlMaster, skipping authentication");
            let socket = SshSocket::new(connection_options, reused_path).await?;
            (socket, None)
        } else {
            delegate.set_status(Some("Connecting"), cx);

            // Start the master SSH process, which does not do anything except
            // for establish the connection and keep it open, allowing other ssh
            // commands to reuse it via a control socket.
            let socket_path = temp_dir.path().join("ssh.sock");
            let mut master_process = MasterProcess::new(
                connection_options.additional_args(),
                &socket_path,
                &destination,
            )?;

            let result = select_biased! {
                _ = cx.background_executor().timer(SSH_CONNECTION_TIMEOUT).fuse() => {
                    anyhow::bail!("connecting to host timed out")
                }
                result = master_process.wait_connected().fuse() => result,
            };

            if let Err(e) = result {
                return Err(e.context("Failed to connect to host"));
            }

            if master_process.as_mut().try_status()?.is_some() {
                let mut output = Vec::new();
                let mut stderr = master_process.as_mut().stderr.take().unwrap();
                stderr.read_to_end(&mut output).await?;

                let error_message = format!(
                    "failed to connect: {}",
                    String::from_utf8_lossy(&output).trim()
                );
                anyhow::bail!(error_message);
            }

            let socket = SshSocket::new(connection_options, socket_path).await?;
            (socket, Some(master_process))
        };

        let is_windows = match known_os {
            Some(os) => os.is_windows(),
            None => socket.probe_is_windows().await,
        };
        log::info!("Remote is windows: {}", is_windows);

        let RemoteEnvironment {
            shell: ssh_shell,
            platform: ssh_platform,
            os_version: ssh_os_version,
        } = if is_windows {
            socket.environment_windows().await?
        } else {
            socket.environment_posix().await?
        };
        log::info!("Remote shell discovered: {}", ssh_shell);

        let ssh_shell_kind = ShellKind::new(&ssh_shell, is_windows);
        log::info!("Remote platform discovered: {:?}", ssh_platform);

        log::info!("Remote OS version discovered: {:?}", ssh_os_version);

        let ssh_home_dir = if is_windows {
            None
        } else {
            socket.home_dir(ssh_shell_kind).await
        };
        log::info!("Remote home directory discovered: {:?}", ssh_home_dir);

        let (ssh_path_style, ssh_default_system_shell) = match ssh_platform.os {
            RemoteOs::Windows => (PathStyle::Windows, ssh_shell.clone()),
            _ => (PathStyle::Unix, String::from("/bin/sh")),
        };

        let mut this = Self {
            socket,
            master_process: Mutex::new(master_process_option),
            killed: AtomicBool::new(false),
            _temp_dir: temp_dir,
            remote_binary_path: None,
            ssh_path_style,
            ssh_platform,
            ssh_os_version,
            ssh_shell,
            ssh_shell_kind,
            ssh_default_system_shell,
            ssh_home_dir,
        };

        delegate.set_status(Some("Checking remote server"), cx);
        this.remote_binary_path = Some(this.check_server_binary().await?);

        Ok(this)
    }

    async fn check_server_binary(&self) -> Result<Arc<RelPath>> {
        let path = RelPath::from_unix_str(".local/bin/ed_remote_server").unwrap();
        self.socket
            .run_command(
                self.ssh_shell_kind,
                &path.display(self.path_style()),
                &["version"],
                false,
            )
            .await
            .context(
                "Could not run SSH remote server at ~/.local/bin/ed_remote_server. \
                 Ensure the binary exists and is executable on the target server",
            )?;
        Ok(path.into())
    }

    fn build_scp_command(
        &self,
        src_path: &Path,
        dest_path_str: &str,
        additional_args: Option<&[&str]>,
    ) -> util::command::Command {
        /// These arguments exist for `ssh` but don't exist / don't have the same semantic for `scp`.
        const SSH_DENY_ARGS_FOR_SCP: &[&str] = &["-X", "-Y"];

        let mut command = util::command::new_command("scp");
        self.socket
            .ssh_options(&mut command, false, Some(SSH_DENY_ARGS_FOR_SCP))
            .args(
                self.socket
                    .connection_options
                    .port
                    .map(|port| vec!["-P".to_string(), port.to_string()])
                    .unwrap_or_default(),
            );
        if let Some(args) = additional_args {
            command.args(args);
        }
        command.arg(src_path).arg(format!(
            "{}:{}",
            self.socket.connection_options.scp_destination(),
            dest_path_str
        ));
        command
    }

    fn build_sftp_command(&self) -> util::command::Command {
        // these arguments exist for "ssh" but don't exist / don't have the same semantic for "sftp"
        const SSH_DENY_ARGS_FOR_SFTP: &[&str] = &["-X", "-Y"];

        let mut command = util::command::new_command("sftp");
        self.socket
            .ssh_options(&mut command, false, Some(SSH_DENY_ARGS_FOR_SFTP))
            .args(
                self.socket
                    .connection_options
                    .port
                    .map(|port| vec!["-P".to_string(), port.to_string()])
                    .unwrap_or_default(),
            );
        command.arg("-b").arg("-");
        command.arg(self.socket.connection_options.scp_destination());
        command.stdin(Stdio::piped());
        command
    }

    async fn is_sftp_available() -> bool {
        which::which("sftp").is_ok()
    }
}

impl SshSocket {
    async fn new(options: SshConnectionOptions, socket_path: PathBuf) -> Result<Self> {
        Ok(Self {
            connection_options: options,
            envs: HashMap::default(),
            socket_path,
        })
    }

    // :WARNING: ssh unquotes arguments when executing on the remote :WARNING:
    // e.g. $ ssh host sh -c 'ls -l' is equivalent to $ ssh host sh -c ls -l
    // and passes -l as an argument to sh, not to ls.
    // Furthermore, some setups (e.g. Coder) will change directory when SSH'ing
    // into a machine. You must use `cd` to get back to $HOME.
    // You need to do it like this: $ ssh host "cd; sh -c 'ls -l /tmp'"
    fn ssh_command(
        &self,
        shell_kind: ShellKind,
        program: &str,
        args: &[impl AsRef<str>],
        allow_pseudo_tty: bool,
    ) -> util::command::Command {
        let mut command = util::command::new_command("ssh");
        let program = shell_kind.prepend_command_prefix(program);
        let mut to_run = shell_kind
            .try_quote_prefix_aware(&program)
            .expect("shell quoting")
            .into_owned();
        for arg in args {
            // We're trying to work with: sh, bash, zsh, fish, tcsh, ...?
            debug_assert!(
                !arg.as_ref().contains('\n'),
                "multiline arguments do not work in all shells"
            );
            to_run.push(' ');
            to_run.push_str(&shell_kind.try_quote(arg.as_ref()).expect("shell quoting"));
        }
        let to_run = if shell_kind == ShellKind::Cmd {
            to_run // 'cd' prints the current directory in CMD
        } else {
            let separator = shell_kind.sequential_commands_separator();
            format!("cd{separator} {to_run}")
        };
        self.ssh_options(&mut command, true, None)
            .arg(self.connection_options.ssh_destination());
        if !allow_pseudo_tty {
            command.arg("-T");
        }
        command.arg(to_run);
        log::debug!("ssh {:?}", command);
        command
    }

    async fn run_command(
        &self,
        shell_kind: ShellKind,
        program: &str,
        args: &[impl AsRef<str>],
        allow_pseudo_tty: bool,
    ) -> Result<String> {
        let mut command = self.ssh_command(shell_kind, program, args, allow_pseudo_tty);
        let output = command.output().await?;
        log::debug!("{:?}: {:?}", command, output);
        anyhow::ensure!(
            output.status.success(),
            "failed to run command {command:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn ssh_options<'a>(
        &self,
        command: &'a mut util::command::Command,
        include_port_forwards: bool,
        deny_args: Option<&[&str]>,
    ) -> &'a mut util::command::Command {
        let mut args = if include_port_forwards {
            self.connection_options.additional_args()
        } else {
            self.connection_options.additional_args_for_scp()
        };

        // draining all arguments that are explicitly denied
        if let Some(deny_args) = deny_args {
            args.retain(|x| !deny_args.contains(&x.as_str()));
        }

        let cmd = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(args);
        cmd.args(["-o", "ControlMaster=no", "-o"])
            .arg(format!("ControlPath={}", self.socket_path.display()));
        cmd
    }

    // Returns the SSH command-line options (without the destination) for building commands.
    // This includes the ControlPath option to reuse the existing connection.
    // Note: The destination must be added separately after all options to ensure proper
    // SSH command structure: ssh [options] destination [command]
    fn ssh_command_options(&self) -> Vec<String> {
        let mut arguments = self.connection_options.additional_args();
        arguments.extend([
            "-o".to_string(),
            "ControlMaster=no".to_string(),
            "-o".to_string(),
            format!("ControlPath={}", self.socket_path.display()),
        ]);
        arguments
    }

    async fn environment_posix(&self) -> Result<RemoteEnvironment> {
        let script = RemoteEnvironment::posix_script();

        let output = self
            .run_command(ShellKind::Posix, "sh", &["-c", &script], false)
            .await
            .context("Failed to determine remote environment")?;
        RemoteEnvironment::parse_posix(&output)
    }

    /// Best-effort detection of the remote OS version. Failures are logged and
    /// result in `None` rather than failing the connection, since this is only
    async fn os_version(&self, os: RemoteOs, shell: ShellKind) -> Option<String> {
        let (program, args) = super::os_version_command(os);
        match self.run_command(shell, program, args, false).await {
            Ok(output) => super::parse_os_version(os, &output),
            Err(error) => {
                log::warn!("Failed to determine remote OS version: {error:#}");
                None
            }
        }
    }

    async fn home_dir(&self, shell: ShellKind) -> Option<String> {
        match self
            .run_command(shell, HOME_DIR_PROGRAM, &HOME_DIR_ARGS, false)
            .await
        {
            Ok(output) => {
                let home_dir = parse_home_dir(&output);
                if home_dir.is_none() {
                    log::warn!("Failed to parse remote home directory from {output:?}");
                }
                home_dir
            }
            Err(error) => {
                log::warn!("Failed to determine remote home directory: {error:#}");
                None
            }
        }
    }

    async fn platform_windows(&self, shell: ShellKind) -> Result<RemotePlatform> {
        let output = self
            .run_command(
                shell,
                "cmd.exe",
                &["/c", "echo", "%PROCESSOR_ARCHITECTURE%"],
                false,
            )
            .await
            .context(
                "Failed to run 'echo %PROCESSOR_ARCHITECTURE%' to determine Windows architecture",
            )?;

        Ok(RemotePlatform {
            os: RemoteOs::Windows,
            arch: match output.trim() {
                "AMD64" => RemoteArch::X86_64,
                "ARM64" => RemoteArch::Aarch64,
                arch => anyhow::bail!(
                    "Prebuilt remote servers are not yet available for windows-{arch}. See https://zed.dev/docs/remote-development"
                ),
            },
        })
    }

    /// Probes whether the remote host is running Windows.
    ///
    /// This is done by attempting to run a simple Windows-specific command.
    /// If it succeeds and returns Windows-like output, we assume it's Windows.
    async fn probe_is_windows(&self) -> bool {
        match self
            .run_command(ShellKind::Cmd, "cmd.exe", &["/c", "ver"], false)
            .await
        {
            // Windows 'ver' command outputs something like "Microsoft Windows [Version 10.0.19045.5011]"
            Ok(output) => output.trim().contains("indows"),
            Err(_) => false,
        }
    }

    async fn environment_windows(&self) -> Result<RemoteEnvironment> {
        use base64::Engine as _;

        let script = RemoteEnvironment::windows_script();
        // The SSH shell is still unknown, encode the script to avoid quoting issues.
        let encoded_script = base64::engine::general_purpose::STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        match self
            .run_command(
                ShellKind::Cmd,
                "powershell",
                &["-E", &encoded_script],
                false,
            )
            .await
        {
            Ok(output) => RemoteEnvironment::parse_windows(&output),
            Err(error) => {
                log::error!("Failed to determine remote Windows environment: {error:#}");
                let platform = self.platform_windows(ShellKind::Cmd).await?;
                let os_version = self.os_version(RemoteOs::Windows, ShellKind::Cmd).await;
                return Ok(RemoteEnvironment {
                    shell: "cmd.exe".to_owned(),
                    platform,
                    os_version,
                });
            }
        }
    }
}

fn parse_port_number(port_str: &str) -> Result<u16> {
    port_str
        .parse()
        .with_context(|| format!("parsing port number: {port_str}"))
}

fn split_port_forward_tokens(spec: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut chars = spec.chars().peekable();

    while chars.peek().is_some() {
        if chars.peek() == Some(&'[') {
            chars.next();
            let mut bracket_content = String::new();
            loop {
                match chars.next() {
                    Some(']') => break,
                    Some(ch) => bracket_content.push(ch),
                    None => anyhow::bail!("Unmatched '[' in port forward spec: {spec}"),
                }
            }
            tokens.push(bracket_content);
            if chars.peek() == Some(&':') {
                chars.next();
            }
        } else {
            let mut token = String::new();
            for ch in chars.by_ref() {
                if ch == ':' {
                    break;
                }
                token.push(ch);
            }
            tokens.push(token);
        }
    }

    Ok(tokens)
}

fn parse_port_forward_spec(spec: &str) -> Result<SshPortForwardOption> {
    let tokens = if spec.contains('[') {
        split_port_forward_tokens(spec)?
    } else {
        spec.split(':').map(String::from).collect()
    };

    match tokens.len() {
        4 => {
            let local_port = parse_port_number(&tokens[1])?;
            let remote_port = parse_port_number(&tokens[3])?;

            Ok(SshPortForwardOption {
                local_host: Some(tokens[0].clone()),
                local_port,
                remote_host: Some(tokens[2].clone()),
                remote_port,
            })
        }
        3 => {
            let local_port = parse_port_number(&tokens[0])?;
            let remote_port = parse_port_number(&tokens[2])?;

            Ok(SshPortForwardOption {
                local_host: None,
                local_port,
                remote_host: Some(tokens[1].clone()),
                remote_port,
            })
        }
        _ => anyhow::bail!("Invalid port forward format: {spec}"),
    }
}

impl SshConnectionOptions {
    pub fn parse_command_line(input: &str) -> Result<Self> {
        let input = input.trim_start_matches("ssh ");
        let mut hostname: Option<String> = None;
        let mut username: Option<String> = None;
        let mut port: Option<u16> = None;
        let mut args = Vec::new();
        let mut port_forwards: Vec<SshPortForwardOption> = Vec::new();

        // disallowed: -E, -e, -F, -f, -G, -g, -M, -N, -n, -O, -q, -S, -s, -T, -t, -V, -v, -W
        const ALLOWED_OPTS: &[&str] = &[
            "-4", "-6", "-A", "-a", "-C", "-K", "-k", "-X", "-x", "-Y", "-y",
        ];
        const ALLOWED_ARGS: &[&str] = &[
            "-B", "-b", "-c", "-D", "-F", "-I", "-i", "-J", "-l", "-m", "-o", "-P", "-p", "-R",
            "-w",
        ];

        let mut tokens = ShellKind::Posix
            .split(input)
            .context("invalid input")?
            .into_iter();

        'outer: while let Some(arg) = tokens.next() {
            if ALLOWED_OPTS.contains(&(&arg as &str)) {
                args.push(arg.to_string());
                continue;
            }
            if arg == "-p" {
                port = tokens.next().and_then(|arg| arg.parse().ok());
                continue;
            } else if let Some(p) = arg.strip_prefix("-p") {
                port = p.parse().ok();
                continue;
            }
            if arg == "-l" {
                username = tokens.next();
                continue;
            } else if let Some(l) = arg.strip_prefix("-l") {
                username = Some(l.to_string());
                continue;
            }
            if arg == "-L" || arg.starts_with("-L") {
                let forward_spec = if arg == "-L" {
                    tokens.next()
                } else {
                    Some(arg.strip_prefix("-L").unwrap().to_string())
                };

                if let Some(spec) = forward_spec {
                    port_forwards.push(parse_port_forward_spec(&spec)?);
                } else {
                    anyhow::bail!("Missing port forward format");
                }
            }

            for a in ALLOWED_ARGS {
                if arg == *a {
                    args.push(arg);
                    if let Some(next) = tokens.next() {
                        args.push(next);
                    }
                    continue 'outer;
                } else if arg.starts_with(a) {
                    args.push(arg);
                    continue 'outer;
                }
            }
            if arg.starts_with("-") || hostname.is_some() {
                anyhow::bail!("unsupported argument: {:?}", arg);
            }
            let mut input = &arg as &str;
            // Destination might be: username1@username2@ip2@ip1
            if let Some((u, rest)) = input.rsplit_once('@') {
                input = rest;
                username = Some(u.to_string());
            }

            // Handle port parsing, accounting for IPv6 addresses
            // IPv6 addresses can be: 2001:db8::1 or [2001:db8::1]:22
            if input.starts_with('[') {
                if let Some((rest, p)) = input.rsplit_once("]:") {
                    input = rest.strip_prefix('[').unwrap_or(rest);
                    port = p.parse().ok();
                } else if input.ends_with(']') {
                    input = input.strip_prefix('[').unwrap_or(input);
                    input = input.strip_suffix(']').unwrap_or(input);
                }
            } else if let Some((rest, p)) = input.rsplit_once(':')
                && !rest.contains(":")
            {
                input = rest;
                port = p.parse().ok();
            }

            hostname = Some(input.to_string())
        }

        let Some(hostname) = hostname else {
            anyhow::bail!("missing hostname");
        };

        let port_forwards = match port_forwards.len() {
            0 => None,
            _ => Some(port_forwards),
        };

        Ok(Self {
            host: hostname.into(),
            username,
            port,
            port_forwards,
            args: Some(args),
            nickname: None,
            upload_binary_over_ssh: false,
            connection_timeout: None,
        })
    }

    pub fn ssh_destination(&self) -> String {
        let mut result = String::default();
        if let Some(username) = &self.username {
            // Username might be: username1@username2@ip2
            let username = urlencoding::encode(username);
            result.push_str(&username);
            result.push('@');
        }

        result.push_str(&self.host.to_string());
        result
    }

    pub fn additional_args_for_scp(&self) -> Vec<String> {
        let mut args = vec!["-o".to_string(), "BatchMode=yes".to_string()];
        args.extend(self.args.iter().flatten().cloned());
        args
    }

    pub fn additional_args(&self) -> Vec<String> {
        let mut args = self.additional_args_for_scp();

        if let Some(timeout) = self.connection_timeout {
            args.extend(["-o".to_string(), format!("ConnectTimeout={}", timeout)]);
        }

        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }

        if let Some(forwards) = &self.port_forwards {
            args.extend(forwards.iter().map(|pf| {
                let local_host = match &pf.local_host {
                    Some(host) => host,
                    None => "localhost",
                };
                let remote_host = match &pf.remote_host {
                    Some(host) => host,
                    None => "localhost",
                };

                format!(
                    "-L{}:{}:{}:{}",
                    bracket_ipv6(local_host),
                    pf.local_port,
                    bracket_ipv6(remote_host),
                    pf.remote_port
                )
            }));
        }

        args
    }

    fn scp_destination(&self) -> String {
        if let Some(username) = &self.username {
            format!("{}@{}", username, self.host.to_bracketed_string())
        } else {
            self.host.to_string()
        }
    }

    pub fn connection_string(&self) -> String {
        let host = if let Some(port) = &self.port {
            format!("{}:{}", self.host.to_bracketed_string(), port)
        } else {
            self.host.to_string()
        };

        if let Some(username) = &self.username {
            format!("{}@{}", username, host)
        } else {
            host
        }
    }
}

fn build_command_posix(
    input_program: Option<String>,
    input_args: &[String],
    input_env: &HashMap<String, String>,
    working_dir: Option<String>,
    port_forward: Option<(u16, String, u16)>,
    ssh_env: HashMap<String, String>,
    ssh_path_style: PathStyle,
    ssh_shell: &str,
    ssh_shell_kind: ShellKind,
    ssh_options: Vec<String>,
    ssh_destination: &str,
    interactive: Interactive,
) -> Result<CommandTemplate> {
    let mut exec = posix_working_dir_prefix(working_dir, ssh_path_style, ssh_shell_kind)?;
    write!(exec, "exec env ")?;

    for (k, v) in input_env.iter() {
        let assignment = format!("{k}={v}");
        let assignment = ssh_shell_kind
            .try_quote(&assignment)
            .context("shell quoting")?;
        write!(exec, "{assignment} ")?;
    }

    if let Some(input_program) = input_program {
        write!(
            exec,
            "{}",
            ssh_shell_kind
                .try_quote_prefix_aware(&input_program)
                .context("shell quoting")?
        )?;
        for arg in input_args {
            let arg = ssh_shell_kind.try_quote(&arg).context("shell quoting")?;
            write!(exec, " {arg}")?;
        }
    } else {
        write!(exec, "{ssh_shell} -l")?;
    };

    Ok(ssh_command_template(
        ssh_options,
        port_forward,
        interactive,
        ssh_destination,
        exec,
        ssh_env,
    ))
}

fn posix_working_dir_prefix(
    working_dir: Option<String>,
    ssh_path_style: PathStyle,
    ssh_shell_kind: ShellKind,
) -> Result<String> {
    let mut exec = String::new();
    if let Some(working_dir) = working_dir {
        let working_dir = RemotePathBuf::new(working_dir, ssh_path_style).to_string();

        // For paths starting with ~/, we need $HOME to expand, but the remainder
        // must be properly quoted to prevent command injection.
        // Pattern: cd "$HOME"/'quoted/remainder' - $HOME expands, rest is single-quoted
        const TILDE_PREFIX: &str = "~/";
        if working_dir.starts_with(TILDE_PREFIX) {
            let remainder = working_dir.trim_start_matches(TILDE_PREFIX);
            write!(
                exec,
                "cd {} {} ",
                home_relative_path(ssh_shell_kind, (!remainder.is_empty()).then_some(remainder))?,
                ssh_shell_kind.sequential_and_commands_separator()
            )?;
        } else {
            let quoted_dir = ssh_shell_kind
                .try_quote(&working_dir)
                .context("shell quoting")?;
            write!(
                exec,
                "cd {quoted_dir} {} ",
                ssh_shell_kind.sequential_and_commands_separator()
            )?;
        }
    } else {
        write!(
            exec,
            "cd {} ",
            ssh_shell_kind.sequential_and_commands_separator()
        )?;
    };
    Ok(exec)
}

fn ssh_command_template(
    ssh_options: Vec<String>,
    port_forward: Option<(u16, String, u16)>,
    interactive: Interactive,
    ssh_destination: &str,
    exec: String,
    ssh_env: HashMap<String, String>,
) -> CommandTemplate {
    let mut args = Vec::new();
    args.extend(ssh_options);

    if let Some((local_port, host, remote_port)) = port_forward {
        args.push("-L".into());
        args.push(format!(
            "{}:{}:{}",
            local_port,
            bracket_ipv6(&host),
            remote_port
        ));
    }

    // LogLevel=ERROR suppresses the "Connection to ... closed." message while
    // preserving SSH errors.
    args.extend(["-o".into(), "LogLevel=ERROR".into()]);
    match interactive {
        // -t forces pseudo-TTY allocation (for interactive use)
        Interactive::Yes => args.push("-t".into()),
        // -T disables pseudo-TTY allocation (for non-interactive piped stdio)
        Interactive::No => args.push("-T".into()),
    }
    // The destination must come after all options but before the command
    args.push(ssh_destination.into());
    args.push(exec);

    CommandTemplate {
        program: "ssh".into(),
        args,
        env: ssh_env,
    }
}

fn build_command_windows(
    input_program: Option<String>,
    input_args: &[String],
    _input_env: &HashMap<String, String>,
    working_dir: Option<String>,
    port_forward: Option<(u16, String, u16)>,
    ssh_env: HashMap<String, String>,
    ssh_path_style: PathStyle,
    ssh_shell: &str,
    _ssh_shell_kind: ShellKind,
    ssh_options: Vec<String>,
    ssh_destination: &str,
    interactive: Interactive,
) -> Result<CommandTemplate> {
    use base64::Engine as _;
    use std::fmt::Write as _;

    let mut exec = String::new();
    let shell_kind = ShellKind::PowerShell;

    if let Some(working_dir) = working_dir {
        let working_dir = RemotePathBuf::new(working_dir, ssh_path_style).to_string();

        write!(
            exec,
            "Set-Location -Path {} {} ",
            shell_kind
                .try_quote(&working_dir)
                .context("shell quoting")?,
            shell_kind.sequential_and_commands_separator()
        )?;
    }

    // Windows OpenSSH has an 8K character limit for command lines. Sending a lot of environment variables easily puts us over the limit.
    // Until we have a better solution for this, we just won't set environment variables for now.
    // for (k, v) in input_env.iter() {
    //     write!(
    //         exec,
    //         "$env:{}={} {} ",
    //         k,
    //         shell_kind.try_quote(v).context("shell quoting")?,
    //         shell_kind.sequential_and_commands_separator()
    //     )?;
    // }

    if let Some(input_program) = input_program {
        write!(
            exec,
            "{}",
            shell_kind
                .try_quote_prefix_aware(&shell_kind.prepend_command_prefix(&input_program))
                .context("shell quoting")?
        )?;
        for arg in input_args {
            let arg = shell_kind.try_quote(arg).context("shell quoting")?;
            write!(exec, " {arg}")?;
        }
    } else {
        // Launch an interactive shell session
        write!(exec, "{ssh_shell}")?;
    };

    let mut args = Vec::new();
    args.extend(ssh_options);

    if let Some((local_port, host, remote_port)) = port_forward {
        args.push("-L".into());
        args.push(format!(
            "{}:{}:{}",
            local_port,
            bracket_ipv6(&host),
            remote_port
        ));
    }

    // LogLevel=ERROR suppresses the "Connection to ... closed." message while
    // preserving SSH errors.
    args.extend(["-o".into(), "LogLevel=ERROR".into()]);
    match interactive {
        // -t forces pseudo-TTY allocation (for interactive use)
        Interactive::Yes => args.push("-t".into()),
        // -T disables pseudo-TTY allocation (for non-interactive piped stdio)
        Interactive::No => args.push("-T".into()),
    }

    // The destination must come after all options but before the command
    args.push(ssh_destination.into());

    // Windows OpenSSH server incorrectly escapes the command string when the PTY is used.
    // The simplest way to work around this is to use a base64 encoded command, which doesn't require escaping.
    let utf16_bytes: Vec<u16> = exec.encode_utf16().collect();
    let byte_slice: Vec<u8> = utf16_bytes.iter().flat_map(|&u| u.to_le_bytes()).collect();
    let base64_encoded = base64::engine::general_purpose::STANDARD.encode(&byte_slice);

    args.push(format!("powershell.exe -E {}", base64_encoded));

    Ok(CommandTemplate {
        program: "ssh".into(),
        args,
        env: ssh_env,
    })
}
