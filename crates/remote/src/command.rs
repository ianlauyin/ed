use std::io::Read;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::ensure;
use collections::HashMap;
use serde::Deserialize;
use serde::Serialize;
use util::shell::ShellKind;

#[derive(Serialize, Deserialize)]
pub struct RemoteCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<String>,
}

impl RemoteCommand {
    pub fn retain_valid_env(&mut self) {
        self.env.retain(|name, _| {
            let valid = !name.is_empty() && !name.contains(['=', '\0']);
            if !valid {
                log::warn!("Skipping environment variable with invalid name");
            }
            valid
        });
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buffer = vec![0; size_of::<u32>()];
        serde_json::to_writer(&mut buffer, self)?;
        let len = buffer.len() - size_of::<u32>();
        ensure!(len <= MAX_COMMAND_SIZE, "Remote command exceeds 16 MiB");
        buffer[..size_of::<u32>()].copy_from_slice(&(len as u32).to_le_bytes());
        Ok(buffer)
    }

    pub fn read(reader: &mut impl Read) -> Result<Self> {
        let mut len = [0; size_of::<u32>()];
        reader.read_exact(&mut len).context("Failed to read remote command length")?;
        let len = u32::from_le_bytes(len) as usize;
        ensure!(len <= MAX_COMMAND_SIZE, "Remote command exceeds 16 MiB");
        let mut buffer = vec![0; len];
        reader.read_exact(&mut buffer).context("Failed to read remote command")?;
        serde_json::from_slice(&buffer).map_err(|_| anyhow::anyhow!("Invalid remote command"))
    }
}

pub fn stdio_launcher_command(shell_kind: ShellKind, remote_binary_path: &str) -> Result<String> {
    let helper = shell_kind.try_quote(remote_binary_path).context("shell quoting")?;
    Ok(format!("exec {helper} exec"))
}

pub fn home_stdio_launcher_command(
    shell_kind: ShellKind,
    home_dir: Option<&str>,
    relative_binary_path: &str,
) -> Result<String> {
    match home_dir {
        Some(home_dir) => {
            stdio_launcher_command(shell_kind, &format!("{}/{relative_binary_path}", home_dir.trim_end_matches('/')))
        }
        None => Ok(format!("exec {} exec", home_relative_path(shell_kind, Some(relative_binary_path))?)),
    }
}

pub fn home_relative_path(shell_kind: ShellKind, relative_path: Option<&str>) -> Result<String> {
    let quoted =
        relative_path.map(|relative_path| shell_kind.try_quote(relative_path).context("shell quoting")).transpose()?;
    Ok(match (shell_kind, quoted) {
        (_, Some(quoted)) => format!("\"$HOME\"/{quoted}"),
        (_, None) => String::from("\"$HOME\""),
    })
}

const MAX_COMMAND_SIZE: usize = 16 * 1024 * 1024;
