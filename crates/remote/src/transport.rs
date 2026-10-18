use std::io::Write;

use crate::{
    RemoteArch, RemoteOs, RemotePlatform,
    json_log::LogRecord,
    protocol::{MESSAGE_LEN_SIZE, message_len_from_buffer, read_message_with_len, write_message},
};
use anyhow::{Context as _, Result};
use futures::{
    AsyncReadExt as _, FutureExt as _, StreamExt as _,
    channel::mpsc::{Sender, UnboundedReceiver, UnboundedSender},
};
use gpui::{AppContext as _, AsyncApp, Task};
use rpc::proto::Envelope;
use util::command::Child;

pub mod ssh;

/// Parses the output of `uname -sm` to determine the remote platform.
/// Takes the last line to skip possible shell initialization output.
fn parse_platform(output: &str) -> Result<RemotePlatform> {
    let output = output.trim();
    let uname = output.rsplit_once('\n').map_or(output, |(_, last)| last);
    let Some((os, arch)) = uname.split_once(" ") else {
        anyhow::bail!("unknown uname: {uname:?}")
    };

    let os = match os {
        "Darwin" => RemoteOs::MacOs,
        "Linux" => RemoteOs::Linux,
        _ => anyhow::bail!(
            "Prebuilt remote servers are not yet available for {os:?}. See https://zed.dev/docs/remote-development"
        ),
    };

    // exclude armv5,6,7 as they are 32-bit.
    let arch = if arch.starts_with("armv8")
        || arch.starts_with("armv9")
        || arch.starts_with("arm64")
        || arch.starts_with("aarch64")
    {
        RemoteArch::Aarch64
    } else if arch.starts_with("x86") {
        RemoteArch::X86_64
    } else {
        anyhow::bail!(
            "Prebuilt remote servers are not yet available for {arch:?}. See https://zed.dev/docs/remote-development"
        )
    };

    Ok(RemotePlatform { os, arch })
}

/// Parses a remote host's OS version into a human-readable string.
///
/// For Linux this is "{ID} {VERSION_ID}" (e.g. "ubuntu 24.04"); for macOS it
/// is the product version (e.g. "15.6.1"). Returns `None` if nothing usable
/// could be parsed.
pub(crate) fn parse_os_version(os: RemoteOs, output: &str) -> Option<String> {
    let output = output.trim();
    if output.is_empty() {
        return None;
    }
    match os {
        RemoteOs::Linux => util::parse_os_release(output),
        RemoteOs::MacOs => {
            // `sw_vers -productVersion` prints a single version line.
            output
                .lines()
                .next_back()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
        }
    }
}

/// Parses the output of `echo $SHELL` to determine the remote shell.
/// Takes the last line to skip possible shell initialization output.
fn parse_shell(output: &str, fallback_shell: &str) -> String {
    let output = output.trim();
    let shell = output.rsplit_once('\n').map_or(output, |(_, last)| last);
    if shell.is_empty() {
        log::error!("$SHELL is not set, falling back to {fallback_shell}");
        fallback_shell.to_owned()
    } else {
        shell.to_owned()
    }
}

const HOME_DIR_PROGRAM: &str = "/bin/sh";
const HOME_DIR_ARGS: [&str; 2] = [
    "-c",
    "printf ZED_HOME_BEGIN; echo; pwd; printf ZED_HOME_END",
];

fn parse_home_dir(output: &str) -> Option<String> {
    let (_, output) = output.split_once("ZED_HOME_BEGIN")?;
    let (home_dir, _) = output.rsplit_once("ZED_HOME_END")?;
    let line_ending = if home_dir.starts_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let home_dir = home_dir
        .strip_prefix(line_ending)?
        .strip_suffix(line_ending)?;
    (home_dir.starts_with('/') && !home_dir.contains('\u{FFFD}')).then(|| home_dir.to_owned())
}

fn handle_rpc_messages_over_child_process_stdio(
    mut remote_proxy_process: Child,
    incoming_tx: UnboundedSender<Envelope>,
    mut outgoing_rx: UnboundedReceiver<Envelope>,
    mut connection_activity_tx: Sender<()>,
    cx: &AsyncApp,
) -> Task<Result<i32>> {
    let mut child_stderr = remote_proxy_process.stderr.take().unwrap();
    let mut child_stdout = remote_proxy_process.stdout.take().unwrap();
    let mut child_stdin = remote_proxy_process.stdin.take().unwrap();

    let mut stdin_buffer = Vec::new();
    let mut stdout_buffer = Vec::new();
    let mut stderr_buffer = Vec::new();
    let mut stderr_offset = 0;

    let stdin_task = cx.background_spawn(async move {
        while let Some(outgoing) = outgoing_rx.next().await {
            write_message(&mut child_stdin, &mut stdin_buffer, outgoing).await?;
        }
        anyhow::Ok(())
    });

    let stdout_task = cx.background_spawn({
        let mut connection_activity_tx = connection_activity_tx.clone();
        async move {
            loop {
                stdout_buffer.resize(MESSAGE_LEN_SIZE, 0);
                let len = child_stdout.read(&mut stdout_buffer).await?;

                if len == 0 {
                    return anyhow::Ok(());
                }

                if len < MESSAGE_LEN_SIZE {
                    child_stdout.read_exact(&mut stdout_buffer[len..]).await?;
                }

                let message_len = message_len_from_buffer(&stdout_buffer);
                let envelope =
                    read_message_with_len(&mut child_stdout, &mut stdout_buffer, message_len)
                        .await?;
                connection_activity_tx.try_send(()).ok();
                incoming_tx.unbounded_send(envelope).ok();
            }
        }
    });

    let stderr_task: Task<anyhow::Result<()>> = cx.background_spawn(async move {
        loop {
            stderr_buffer.resize(stderr_offset + 1024, 0);

            let len = child_stderr
                .read(&mut stderr_buffer[stderr_offset..])
                .await?;
            if len == 0 {
                return anyhow::Ok(());
            }

            stderr_offset += len;
            let mut start_ix = 0;
            while let Some(ix) = stderr_buffer[start_ix..stderr_offset]
                .iter()
                .position(|b| b == &b'\n')
            {
                let line_ix = start_ix + ix;
                let content = &stderr_buffer[start_ix..line_ix];
                start_ix = line_ix + 1;
                if let Ok(record) = serde_json::from_slice::<LogRecord>(content) {
                    record.log(log::logger())
                } else {
                    std::io::stderr()
                        .write_fmt(format_args!(
                            "(remote) {}\n",
                            String::from_utf8_lossy(content)
                        ))
                        .ok();
                }
            }
            stderr_buffer.drain(0..start_ix);
            stderr_offset -= start_ix;

            connection_activity_tx.try_send(()).ok();
        }
    });

    cx.background_spawn(async move {
        let result = futures::select! {
            result = stdin_task.fuse() => {
                result.context("stdin")
            }
            result = stdout_task.fuse() => {
                result.context("stdout")
            }
            result = stderr_task.fuse() => {
                result.context("stderr")
            }
        };
        let exit_status = remote_proxy_process.status().await?;
        let status = exit_status.code().unwrap_or_else(|| {
            let status = std::os::unix::process::ExitStatusExt::signal(&exit_status).unwrap_or(1);

            status
        });
        match result {
            Ok(_) => Ok(status),
            Err(error) => Err(error),
        }
    })
}
