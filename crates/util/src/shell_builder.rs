use std::borrow::Cow;

use crate::shell::Shell;
use crate::shell::ShellKind;
use crate::shell::get_system_shell;

/// ShellBuilder is used to turn a user-requested task into a
/// program that can be executed by the shell.
pub struct ShellBuilder {
    /// The shell to run
    program: String,
    args: Vec<String>,
    interactive: bool,
    /// Whether to redirect stdin to /dev/null for the spawned command as a subshell.
    redirect_stdin: bool,
    kind: ShellKind,
}

impl ShellBuilder {
    /// Create a new ShellBuilder as configured.
    pub fn new(shell: &Shell) -> Self {
        let (program, args) = match shell {
            Shell::System => (get_system_shell(), Vec::new()),
            Shell::Program(shell) => (shell.clone(), Vec::new()),
            Shell::WithArguments { program, args, .. } => (program.clone(), args.clone()),
        };

        let kind = ShellKind::new(&program);
        Self { program, args, interactive: true, kind, redirect_stdin: false }
    }
    pub const fn non_interactive(mut self) -> Self {
        self.interactive = false;
        self
    }

    /// Returns the label to show in the terminal tab
    pub fn command_label(&self, command_to_use_in_label: &str) -> String {
        if command_to_use_in_label.trim().is_empty() {
            self.program.clone()
        } else {
            let interactivity = if self.interactive { "-i " } else { Default::default() };
            format!("{PROGRAM} {interactivity}-c '{command_to_use_in_label}'", PROGRAM = self.program)
        }
    }

    pub const fn redirect_stdin_to_dev_null(mut self) -> Self {
        self.redirect_stdin = true;
        self
    }

    /// Returns the program and arguments to run this task in a shell.
    pub fn build(mut self, task_command: Option<String>, task_args: &[String]) -> (String, Vec<String>) {
        if let Some(task_command) = task_command {
            let task_command = if !task_args.is_empty() {
                match self.kind.try_quote(&task_command) {
                    Some(task_command) => task_command.into_owned(),
                    None => task_command,
                }
            } else {
                task_command
            };
            let mut combined_command = task_args.iter().fold(task_command, |mut command, arg| {
                command.push(' ');
                command.push_str(&self.kind.try_quote(arg).unwrap_or(Cow::Borrowed(arg)));
                command
            });
            if self.redirect_stdin {
                match self.kind {
                    ShellKind::Posix => {
                        // Perform the STDIN redirection prior to the actual
                        // command on a separate line, so that it is already
                        // active if the command contains a syntax error.
                        // Otherwise, with -i, dash will fall back to an
                        // interactive shell in this case.
                        combined_command.insert_str(0, "exec </dev/null\n");
                    }
                    ShellKind::Fish => {
                        combined_command.insert_str(0, "begin; ");
                        combined_command.push_str("; end </dev/null");
                    }
                }
            }

            self.args.extend(self.kind.args_for_shell(self.interactive, combined_command));
        }

        (self.program, self.args)
    }

    // This should not exist, but our task infra is broken beyond repair right now
    #[doc(hidden)]
    pub fn build_no_quote(mut self, task_command: Option<String>, task_args: &[String]) -> (String, Vec<String>) {
        if let Some(task_command) = task_command {
            let mut combined_command = task_args.iter().fold(task_command, |mut command, arg| {
                command.push(' ');
                command.push_str(arg);
                command
            });
            if self.redirect_stdin {
                match self.kind {
                    ShellKind::Posix => {
                        combined_command.insert_str(0, "exec </dev/null\n");
                    }
                    ShellKind::Fish => {
                        combined_command.insert_str(0, "begin; ");
                        combined_command.push_str("; end </dev/null");
                    }
                }
            }

            self.args.extend(self.kind.args_for_shell(self.interactive, combined_command));
        }

        (self.program, self.args)
    }

    /// Builds a `smol::process::Command` with the given task command and arguments.
    ///
    /// Prefer this over manually constructing a command with the output of `Self::build`,
    /// as this method handles `cmd` weirdness on windows correctly.
    pub fn build_smol_command(self, task_command: Option<String>, task_args: &[String]) -> smol::process::Command {
        smol::process::Command::from(self.build_std_command(task_command, task_args))
    }

    /// Builds a `std::process::Command` with the given task command and arguments.
    ///
    /// Prefer this over manually constructing a command with the output of `Self::build`,
    /// as this method handles `cmd` weirdness on windows correctly.
    pub fn build_std_command(self, mut task_command: Option<String>, task_args: &[String]) -> std::process::Command {
        if task_args.is_empty() {
            task_command =
                task_command.as_ref().map(|cmd| self.kind.try_quote(cmd).map(Cow::into_owned)).unwrap_or(task_command);
        }
        let (program, args) = self.build(task_command, task_args);

        let mut child = crate::command::new_std_command(program);

        child.args(args);

        child
    }

    pub const fn kind(&self) -> ShellKind {
        self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_redirected(program: &str, quote: bool) -> Vec<String> {
        let builder = ShellBuilder::new(&Shell::Program(program.into())).non_interactive().redirect_stdin_to_dev_null();
        let command = Some("echo".to_string());
        let args = ["a b".to_string()];
        let (_, args) = if quote { builder.build(command, &args) } else { builder.build_no_quote(command, &args) };
        args
    }

    #[test]
    fn test_build_redirect_stdin() {
        assert_eq!(build_redirected("/bin/zsh", true), ["-c", "exec </dev/null\necho 'a b'"]);
        assert_eq!(build_redirected("/opt/homebrew/bin/fish", true), ["-c", "begin; echo 'a b'; end </dev/null"]);
        // Unsupported shells are treated as POSIX.
        assert_eq!(build_redirected("/bin/tcsh", true), ["-c", "exec </dev/null\necho 'a b'"]);
    }

    #[test]
    fn test_build_no_quote_redirect_stdin() {
        assert_eq!(build_redirected("/bin/zsh", false), ["-c", "exec </dev/null\necho a b"]);
        assert_eq!(build_redirected("fish", false), ["-c", "begin; echo a b; end </dev/null"]);
    }
}
