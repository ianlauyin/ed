use std::borrow::Cow;
use std::fmt;
use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Shell configuration to open the terminal with.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Hash)]
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
    Csh,
    Tcsh,
    Rc,
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
            ShellKind::Csh => write!(f, "csh"),
            ShellKind::Tcsh => write!(f, "tcsh"),
            ShellKind::Fish => write!(f, "fish"),
            ShellKind::Rc => write!(f, "rc"),
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
            "csh" => ShellKind::Csh,
            "tcsh" => ShellKind::Tcsh,
            "rc" => ShellKind::Rc,
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
        match self {
            ShellKind::Csh | ShellKind::Tcsh => Self::quote_csh(arg),
            ShellKind::Posix | ShellKind::Rc | ShellKind::Fish => shlex::try_quote(arg).ok(),
        }
    }

    pub fn quote_csh(arg: &str) -> Option<Cow<'_, str>> {
        if arg.contains('\0') {
            return None;
        }
        if Self::is_unquoted_safe(arg) && !arg.starts_with('%') {
            return Some(Cow::Borrowed(arg));
        }
        if arg.is_empty() {
            return Some(Cow::Borrowed("''"));
        }
        let mut quoted = String::with_capacity(arg.len() + 2);
        let mut in_quotes = false;
        for character in arg.chars() {
            match character {
                '\'' | '!' | '\\' => {
                    if in_quotes {
                        quoted.push('\'');
                        in_quotes = false;
                    }
                    quoted.push('\\');
                    quoted.push(character);
                }
                character => {
                    if !in_quotes {
                        quoted.push('\'');
                        in_quotes = true;
                    }
                    if character == '\n' {
                        quoted.push('\\');
                    }
                    quoted.push(character);
                }
            }
        }
        if in_quotes {
            quoted.push('\'');
        }
        Some(Cow::Owned(quoted))
    }

    fn is_unquoted_safe(arg: &str) -> bool {
        !arg.is_empty() && arg.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_./:,+@%".contains(&byte))
    }

    pub fn split(&self, input: &str) -> Option<Vec<String>> {
        shlex::split(input)
    }
}
