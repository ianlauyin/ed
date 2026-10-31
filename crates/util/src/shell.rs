use std::borrow::Cow;
use std::fmt;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

/// Shell configuration to open the terminal with.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    /// Use the system's default terminal configuration in /etc/passwd
    #[default]
    System,
    /// Use a specific program with no arguments.
    Program(String),
    /// Use a specific program with arguments.
    WithArguments {
        /// The program to run.
        program: String,
        /// The arguments to pass to the program.
        args: Vec<String>,
        /// An optional string to override the title of the terminal tab
        title_override: Option<String>,
    },
}

impl Shell {
    pub fn program(&self) -> String {
        match self {
            Shell::Program(program) => program.clone(),
            Shell::WithArguments { program, .. } => program.clone(),
            Shell::System => get_system_shell(),
        }
    }

    pub fn program_and_args(&self) -> (String, &[String]) {
        match self {
            Shell::Program(program) => (program.clone(), &[]),
            Shell::WithArguments { program, args, .. } => (program.clone(), args),
            Shell::System => (get_system_shell(), &[]),
        }
    }

    pub fn shell_kind(&self) -> ShellKind {
        match self {
            Shell::Program(program) => ShellKind::new(program),
            Shell::WithArguments { program, .. } => ShellKind::new(program),
            Shell::System => ShellKind::system(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShellKind {
    #[default]
    Posix,
    Fish,
}

pub fn get_system_shell() -> String {
    std::env::var("SHELL").unwrap_or("/bin/sh".to_string())
}

pub fn get_default_system_shell() -> String {
    "/bin/sh".to_string()
}

/// Get the default system shell, preferring bash on Windows.
pub fn get_default_system_shell_preferring_bash() -> String {
    "/bin/sh".to_string()
}

impl fmt::Display for ShellKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellKind::Posix => write!(f, "sh"),
            ShellKind::Fish => write!(f, "fish"),
        }
    }
}

impl ShellKind {
    pub fn system() -> Self {
        Self::new(get_system_shell())
    }

    pub fn new(program: impl AsRef<Path>) -> Self {
        let program = program.as_ref();
        let program = program.file_stem().unwrap_or(program.as_os_str()).to_string_lossy();

        match &*program {
            "fish" => ShellKind::Fish,
            // Some other shell detected, the user might install and use a
            // unix-like shell.
            _ => ShellKind::Posix,
        }
    }

    pub fn args_for_shell(&self, interactive: bool, combined_command: String) -> Vec<String> {
        interactive.then(|| "-i".to_owned()).into_iter().chain(["-c".to_owned(), combined_command]).collect()
    }

    pub const fn sequential_commands_separator(&self) -> char {
        ';'
    }

    pub const fn sequential_and_commands_separator(&self) -> &'static str {
        "&&"
    }

    pub fn try_quote<'a>(&self, arg: &'a str) -> Option<Cow<'a, str>> {
        shlex::try_quote(arg).ok()
    }

    pub fn split(&self, input: &str) -> Option<Vec<String>> {
        shlex::split(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_kind_new() {
        assert_eq!(ShellKind::new("/opt/homebrew/bin/fish"), ShellKind::Fish);
        assert_eq!(ShellKind::new("fish"), ShellKind::Fish);
        assert_eq!(ShellKind::new("/bin/zsh"), ShellKind::Posix);
        assert_eq!(ShellKind::new("/bin/tcsh"), ShellKind::Posix);
        assert_eq!(ShellKind::new("rc"), ShellKind::Posix);
    }

    #[test]
    fn test_try_quote() {
        for kind in [ShellKind::Posix, ShellKind::Fish] {
            assert_eq!(kind.try_quote("plain").as_deref(), Some("plain"));
            assert_eq!(kind.try_quote("has space").as_deref(), Some("'has space'"));
            assert_eq!(kind.try_quote("nul\0"), None);
        }
    }
}
