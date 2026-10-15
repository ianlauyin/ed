use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt, path::Path};

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

    pub fn shell_kind(&self, is_windows: bool) -> ShellKind {
        match self {
            Shell::Program(program) => ShellKind::new(program, is_windows),
            Shell::WithArguments { program, .. } => ShellKind::new(program, is_windows),
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
    /// Pre-installed "legacy" powershell for windows
    PowerShell,
    /// PowerShell 7.x
    Pwsh,
    Nushell,
    Cmd,
    Xonsh,
    Elvish,
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

pub fn get_windows_system_shell() -> String {
    return "cmd.exe".to_string();
}

impl fmt::Display for ShellKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellKind::Posix => write!(f, "sh"),
            ShellKind::Csh => write!(f, "csh"),
            ShellKind::Tcsh => write!(f, "tcsh"),
            ShellKind::Fish => write!(f, "fish"),
            ShellKind::PowerShell => write!(f, "powershell"),
            ShellKind::Pwsh => write!(f, "pwsh"),
            ShellKind::Nushell => write!(f, "nu"),
            ShellKind::Cmd => write!(f, "cmd"),
            ShellKind::Rc => write!(f, "rc"),
            ShellKind::Xonsh => write!(f, "xonsh"),
            ShellKind::Elvish => write!(f, "elvish"),
        }
    }
}

impl ShellKind {
    pub fn system() -> Self {
        Self::new(&get_system_shell(), false)
    }

    /// Returns whether this shell's command chaining syntax can be parsed by brush-parser.
    ///
    /// This is used to determine if we can safely parse shell commands to extract sub-commands
    /// for security purposes (e.g., preventing shell injection in "always allow" patterns).
    ///
    /// The brush-parser handles `;` (sequential execution) and `|` (piping), which are
    /// supported by all common shells. It also handles `&&` and `||` for conditional
    /// execution, `$()` and backticks for command substitution, and process substitution.
    ///
    /// # Shell Notes
    ///
    /// - **Nushell**: Uses `;` for sequential execution. The `and`/`or` keywords are boolean
    ///   operators on values (e.g., `$true and $false`), not command chaining operators.
    /// - **Elvish**: Uses `;` to separate pipelines, which brush-parser handles. Elvish does
    ///   not have `&&` or `||` operators. Its `and`/`or` are special commands that operate
    ///   on values, not command chaining (e.g., `and $true $false`).
    /// - **Rc (Plan 9)**: Uses `;` for sequential execution and `|` for piping. Does not
    ///   have `&&`/`||` operators for conditional chaining.
    /// All current shell variants are listed here because brush-parser can handle
    /// their syntax. If a new `ShellKind` variant is added, evaluate whether
    /// brush-parser can safely parse its command chaining syntax before including
    /// it. Omitting a variant will cause `tool_permissions::from_input` to deny
    /// terminal commands that have `always_allow` patterns configured.
    pub fn supports_posix_chaining(&self) -> bool {
        matches!(
            self,
            ShellKind::Posix
                | ShellKind::Fish
                | ShellKind::PowerShell
                | ShellKind::Pwsh
                | ShellKind::Cmd
                | ShellKind::Xonsh
                | ShellKind::Csh
                | ShellKind::Tcsh
                | ShellKind::Nushell
                | ShellKind::Elvish
                | ShellKind::Rc
        )
    }

    pub fn new(program: impl AsRef<Path>, is_windows: bool) -> Self {
        let program = program.as_ref();
        let program = program
            .file_stem()
            .unwrap_or_else(|| program.as_os_str())
            .to_string_lossy();

        match &*program {
            "powershell" => ShellKind::PowerShell,
            "pwsh" => ShellKind::Pwsh,
            "cmd" => ShellKind::Cmd,
            "nu" => ShellKind::Nushell,
            "fish" => ShellKind::Fish,
            "csh" => ShellKind::Csh,
            "tcsh" => ShellKind::Tcsh,
            "rc" => ShellKind::Rc,
            "xonsh" => ShellKind::Xonsh,
            "elvish" => ShellKind::Elvish,
            "sh" | "bash" | "zsh" => ShellKind::Posix,
            _ if is_windows => ShellKind::PowerShell,
            // Some other shell detected, the user might install and use a
            // unix-like shell.
            _ => ShellKind::Posix,
        }
    }

    pub fn to_shell_variable(self, input: &str) -> String {
        match self {
            Self::PowerShell | Self::Pwsh => Self::to_powershell_variable(input),
            Self::Cmd => Self::to_cmd_variable(input),
            Self::Posix => input.to_owned(),
            Self::Fish => input.to_owned(),
            Self::Csh => input.to_owned(),
            Self::Tcsh => input.to_owned(),
            Self::Rc => input.to_owned(),
            Self::Nushell => Self::to_nushell_variable(input),
            Self::Xonsh => input.to_owned(),
            Self::Elvish => input.to_owned(),
        }
    }

    fn to_cmd_variable(input: &str) -> String {
        if let Some(var_str) = input.strip_prefix("${") {
            match var_str.strip_suffix('}') {
                Some(var_name) if !var_name.is_empty() && !var_name.contains(':') => {
                    format!("%{var_name}%")
                }
                // `${SOME_VAR:-SOME_DEFAULT}`, we currently do not handle this situation,
                // which will result in the task failing to run in such cases.
                _ => input.into(),
            }
        } else if let Some(var_str) = input.strip_prefix('$') {
            // If the input starts with "$", directly append to "$env:"
            format!("%{}%", var_str)
        } else {
            // If no prefix is found, return the input as is
            input.into()
        }
    }

    fn to_powershell_variable(input: &str) -> String {
        if let Some(var_str) = input.strip_prefix("${") {
            match var_str.strip_suffix('}') {
                Some(var_name) if !var_name.is_empty() && !var_name.contains(':') => {
                    format!("$env:{var_name}")
                }
                // `${SOME_VAR:-SOME_DEFAULT}`, we currently do not handle this situation,
                // which will result in the task failing to run in such cases.
                _ => input.into(),
            }
        } else if let Some(var_str) = input.strip_prefix('$') {
            // If the input starts with "$", directly append to "$env:"
            format!("$env:{}", var_str)
        } else {
            // If no prefix is found, return the input as is
            input.into()
        }
    }

    fn to_nushell_variable(input: &str) -> String {
        let mut result = String::new();
        let mut source = input;
        let mut is_start = true;

        loop {
            match source.chars().next() {
                None => return result,
                Some('$') => {
                    source = Self::parse_nushell_var(&source[1..], &mut result, is_start);
                    is_start = false;
                }
                Some(_) => {
                    is_start = false;
                    let chunk_end = source.find('$').unwrap_or(source.len());
                    let (chunk, rest) = source.split_at(chunk_end);
                    result.push_str(chunk);
                    source = rest;
                }
            }
        }
    }

    fn parse_nushell_var<'a>(source: &'a str, text: &mut String, is_start: bool) -> &'a str {
        if source.starts_with("env.") {
            text.push('$');
            return source;
        }

        match source.chars().next() {
            Some('{') => {
                let source = &source[1..];
                if let Some(end) = source.find('}') {
                    let var_name = &source[..end];
                    if !var_name.is_empty() {
                        if !is_start {
                            text.push_str("(");
                        }
                        text.push_str("$env.");
                        text.push_str(var_name);
                        if !is_start {
                            text.push_str(")");
                        }
                        &source[end + 1..]
                    } else {
                        text.push_str("${}");
                        &source[end + 1..]
                    }
                } else {
                    text.push_str("${");
                    source
                }
            }
            Some(c) if c.is_alphabetic() || c == '_' => {
                let end = source
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(source.len());
                let var_name = &source[..end];
                if !is_start {
                    text.push_str("(");
                }
                text.push_str("$env.");
                text.push_str(var_name);
                if !is_start {
                    text.push_str(")");
                }
                &source[end..]
            }
            _ => {
                text.push('$');
                source
            }
        }
    }

    pub fn args_for_shell(&self, interactive: bool, combined_command: String) -> Vec<String> {
        match self {
            ShellKind::PowerShell | ShellKind::Pwsh => vec!["-C".to_owned(), combined_command],
            ShellKind::Cmd => vec![
                "/S".to_owned(),
                "/C".to_owned(),
                format!("\"{combined_command}\""),
            ],
            ShellKind::Posix
            | ShellKind::Nushell
            | ShellKind::Fish
            | ShellKind::Csh
            | ShellKind::Tcsh
            | ShellKind::Rc
            | ShellKind::Xonsh
            | ShellKind::Elvish => interactive
                .then(|| "-i".to_owned())
                .into_iter()
                .chain(["-c".to_owned(), combined_command])
                .collect(),
        }
    }

    pub const fn command_prefix(&self) -> Option<char> {
        match self {
            ShellKind::PowerShell | ShellKind::Pwsh => Some('&'),
            ShellKind::Nushell => Some('^'),
            ShellKind::Posix
            | ShellKind::Csh
            | ShellKind::Tcsh
            | ShellKind::Rc
            | ShellKind::Fish
            | ShellKind::Cmd
            | ShellKind::Xonsh
            | ShellKind::Elvish => None,
        }
    }

    pub fn prepend_command_prefix<'a>(&self, command: &'a str) -> Cow<'a, str> {
        match self.command_prefix() {
            Some(prefix) if !command.starts_with(prefix) => {
                Cow::Owned(format!("{prefix}{command}"))
            }
            _ => Cow::Borrowed(command),
        }
    }

    pub const fn sequential_commands_separator(&self) -> char {
        match self {
            ShellKind::Cmd => '&',
            ShellKind::Posix
            | ShellKind::Csh
            | ShellKind::Tcsh
            | ShellKind::Rc
            | ShellKind::Fish
            | ShellKind::PowerShell
            | ShellKind::Pwsh
            | ShellKind::Nushell
            | ShellKind::Xonsh
            | ShellKind::Elvish => ';',
        }
    }

    pub const fn sequential_and_commands_separator(&self) -> &'static str {
        match self {
            ShellKind::Cmd
            | ShellKind::Posix
            | ShellKind::Csh
            | ShellKind::Tcsh
            | ShellKind::Rc
            | ShellKind::Fish
            | ShellKind::Pwsh
            | ShellKind::Xonsh => "&&",
            ShellKind::PowerShell | ShellKind::Nushell | ShellKind::Elvish => ";",
        }
    }

    pub fn try_quote<'a>(&self, arg: &'a str) -> Option<Cow<'a, str>> {
        match self {
            ShellKind::PowerShell => Some(Self::quote_powershell(arg)),
            ShellKind::Pwsh => Some(Self::quote_pwsh(arg)),
            ShellKind::Cmd => Some(Self::quote_cmd(arg)),
            ShellKind::Csh | ShellKind::Tcsh => Self::quote_csh(arg),
            ShellKind::Nushell => Self::quote_nushell(arg),
            ShellKind::Posix
            | ShellKind::Rc
            | ShellKind::Fish
            | ShellKind::Xonsh
            | ShellKind::Elvish => shlex::try_quote(arg).ok(),
        }
    }

    fn quote_windows(arg: &str, enclose: bool) -> Cow<'_, str> {
        if arg.is_empty() {
            return Cow::Borrowed("\"\"");
        }

        let needs_quoting = arg.chars().any(|c| c == ' ' || c == '\t' || c == '"');
        if !needs_quoting {
            return Cow::Borrowed(arg);
        }

        let mut result = String::with_capacity(arg.len() + 2);

        if enclose {
            result.push('"');
        }

        let chars: Vec<char> = arg.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '\\' {
                let mut num_backslashes = 0;
                while i < chars.len() && chars[i] == '\\' {
                    num_backslashes += 1;
                    i += 1;
                }

                if i < chars.len() && chars[i] == '"' {
                    // Backslashes followed by quote: double the backslashes and escape the quote
                    for _ in 0..(num_backslashes * 2 + 1) {
                        result.push('\\');
                    }
                    result.push('"');
                    i += 1;
                } else if i >= chars.len() {
                    // Trailing backslashes: double them (they precede the closing quote)
                    for _ in 0..(num_backslashes * 2) {
                        result.push('\\');
                    }
                } else {
                    // Backslashes not followed by quote: output as-is
                    for _ in 0..num_backslashes {
                        result.push('\\');
                    }
                }
            } else if chars[i] == '"' {
                // Quote not preceded by backslash: escape it
                result.push('\\');
                result.push('"');
                i += 1;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        if enclose {
            result.push('"');
        }
        Cow::Owned(result)
    }

    fn needs_quoting_powershell(s: &str) -> bool {
        s.is_empty()
            || s.chars().any(|c| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '"' | '`'
                            | '$'
                            | '&'
                            | '|'
                            | '<'
                            | '>'
                            | ';'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | ','
                            | '\''
                            | '@'
                    )
            })
    }

    fn need_quotes_powershell(arg: &str) -> bool {
        let mut quote_count = 0;
        for c in arg.chars() {
            if c == '"' {
                quote_count += 1;
            } else if c.is_whitespace() && (quote_count % 2 == 0) {
                return true;
            }
        }
        false
    }

    fn escape_powershell_quotes(s: &str) -> String {
        let mut result = String::with_capacity(s.len() + 4);
        result.push('\'');
        for c in s.chars() {
            if c == '\'' {
                result.push('\'');
            }
            result.push(c);
        }
        result.push('\'');
        result
    }

    pub fn quote_powershell(arg: &str) -> Cow<'_, str> {
        let ps_will_quote = Self::need_quotes_powershell(arg);
        let crt_quoted = Self::quote_windows(arg, !ps_will_quote);

        if !Self::needs_quoting_powershell(arg) {
            return crt_quoted;
        }

        Cow::Owned(Self::escape_powershell_quotes(&crt_quoted))
    }

    pub fn quote_pwsh(arg: &str) -> Cow<'_, str> {
        if arg.is_empty() {
            return Cow::Borrowed("''");
        }

        if !Self::needs_quoting_powershell(arg) {
            return Cow::Borrowed(arg);
        }

        Cow::Owned(Self::escape_powershell_quotes(arg))
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

    pub fn quote_nushell(arg: &str) -> Option<Cow<'_, str>> {
        if arg.contains('\0') {
            return None;
        }
        if Self::is_unquoted_safe(arg) {
            return Some(Cow::Borrowed(arg));
        }
        if !arg.contains('\'') {
            return Some(Cow::Owned(format!("'{arg}'")));
        }
        let mut quoted = String::with_capacity(arg.len() + 2);
        quoted.push('"');
        for character in arg.chars() {
            if character == '\\' || character == '"' {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        Some(Cow::Owned(quoted))
    }

    fn is_unquoted_safe(arg: &str) -> bool {
        !arg.is_empty()
            && arg
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:,+@%".contains(&byte))
    }

    pub fn quote_cmd(arg: &str) -> Cow<'_, str> {
        let crt_quoted = Self::quote_windows(arg, true);

        let needs_cmd_escaping = crt_quoted.contains(['"', '%', '^', '<', '>', '&', '|', '(', ')']);

        if !needs_cmd_escaping {
            return crt_quoted;
        }

        let mut result = String::with_capacity(crt_quoted.len() * 2);
        for c in crt_quoted.chars() {
            match c {
                '^' | '"' | '<' | '>' | '&' | '|' | '(' | ')' => {
                    result.push('^');
                    result.push(c);
                }
                '%' => {
                    result.push_str("%%cd:~,%");
                }
                _ => result.push(c),
            }
        }
        Cow::Owned(result)
    }

    /// Quotes the given argument if necessary, taking into account the command prefix.
    ///
    /// In other words, this will consider quoting arg without its command prefix to not break the command.
    /// You should use this over `try_quote` when you want to quote a shell command.
    pub fn try_quote_prefix_aware<'a>(&self, arg: &'a str) -> Option<Cow<'a, str>> {
        if let Some(char) = self.command_prefix() {
            if let Some(arg) = arg.strip_prefix(char) {
                // we have a command that is prefixed
                for quote in ['\'', '"'] {
                    if let Some(arg) = arg
                        .strip_prefix(quote)
                        .and_then(|arg| arg.strip_suffix(quote))
                    {
                        // and the command itself is wrapped as a literal, that
                        // means the prefix exists to interpret a literal as a
                        // command. So strip the quotes, quote the command, and
                        // re-add the quotes if they are missing after requoting
                        let quoted = self.try_quote(arg)?;
                        return Some(if quoted.starts_with(['\'', '"']) {
                            Cow::Owned(self.prepend_command_prefix(&quoted).into_owned())
                        } else {
                            Cow::Owned(
                                self.prepend_command_prefix(&format!("{quote}{quoted}{quote}"))
                                    .into_owned(),
                            )
                        });
                    }
                }
                return self
                    .try_quote(arg)
                    .map(|quoted| Cow::Owned(self.prepend_command_prefix(&quoted).into_owned()));
            }
        }
        self.try_quote(arg).map(|quoted| match quoted {
            unquoted @ Cow::Borrowed(_) => unquoted,
            Cow::Owned(quoted) => Cow::Owned(self.prepend_command_prefix(&quoted).into_owned()),
        })
    }

    pub fn split(&self, input: &str) -> Option<Vec<String>> {
        shlex::split(input)
    }

    pub const fn activate_keyword(&self) -> &'static str {
        match self {
            ShellKind::Cmd => "",
            ShellKind::Nushell => "overlay use",
            ShellKind::PowerShell | ShellKind::Pwsh => ".",
            ShellKind::Fish
            | ShellKind::Csh
            | ShellKind::Tcsh
            | ShellKind::Posix
            | ShellKind::Rc
            | ShellKind::Xonsh
            | ShellKind::Elvish => "source",
        }
    }

    pub const fn clear_screen_command(&self) -> &'static str {
        match self {
            ShellKind::Cmd => "cls",
            ShellKind::Posix
            | ShellKind::Csh
            | ShellKind::Tcsh
            | ShellKind::Rc
            | ShellKind::Fish
            | ShellKind::PowerShell
            | ShellKind::Pwsh
            | ShellKind::Nushell
            | ShellKind::Xonsh
            | ShellKind::Elvish => "clear",
        }
    }
}
