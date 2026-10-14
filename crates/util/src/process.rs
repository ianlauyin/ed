use anyhow::{Context as _, Result};
use std::process::Stdio;

/// A wrapper around `smol::process::Child` that attempts to clean up subprocess
/// trees: on Unix by using process groups, and on Windows by using job objects.
///
/// On Windows, if job setup and assignment succeed, dropping this struct
/// closes the job handle and terminates processes in the job. If either step
/// fails, the direct child remains usable but descendants are not tracked.
/// A descendant spawned before job assignment can also escape the job.
pub struct Child {
    process: smol::process::Child,
}

impl std::ops::Deref for Child {
    type Target = smol::process::Child;

    fn deref(&self) -> &Self::Target {
        &self.process
    }
}

impl std::ops::DerefMut for Child {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.process
    }
}

impl Child {
    pub fn spawn(
        mut command: std::process::Command,
        stdin: Stdio,
        stdout: Stdio,
        stderr: Stdio,
    ) -> Result<Self> {
        crate::set_pre_exec_to_start_new_session(&mut command);
        let mut command = smol::process::Command::from(command);
        let process = command
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn command {}",
                    crate::redact::redact_command(&format!("{command:?}"))
                )
            })?;
        Ok(Self { process })
    }

    /// Consumes the child, draining its stdout/stderr and waiting for it to
    /// exit, then returns the collected output.
    pub async fn output(self) -> Result<std::process::Output> {
        // NOTE: Keep `self` alive across this await, do not destructure it to
        // pull `process` out first. On Windows that drops the job object early,
        // which triggers `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and kills the
        // child before `output()` finishes collecting its stdout/stderr.
        Ok(self.process.output().await?)
    }

    pub fn kill(&mut self) -> Result<()> {
        let pid = self.process.id();
        unsafe {
            libc::killpg(pid as i32, libc::SIGKILL);
        }
        Ok(())
    }
}
