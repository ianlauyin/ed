mod shells {
    pub use clap_complete::aot::Bash;
    pub use clap_complete::aot::Fish;
    pub use clap_complete::aot::Zsh;
}

use clap_complete::Generator;

#[derive(Clone, Debug, clap::ValueEnum)]
#[non_exhaustive]
#[value(rename_all = "lower")]
pub(crate) enum Shell {
    Bash,
    Fish,
    Zsh,
}

impl Generator for Shell {
    fn file_name(&self, name: &str) -> String {
        match self {
            Shell::Bash => self::shells::Bash.file_name(name),
            Shell::Fish => self::shells::Fish.file_name(name),
            Shell::Zsh => self::shells::Zsh.file_name(name),
        }
    }

    fn generate(&self, cmd: &clap::Command, buf: &mut dyn std::io::Write) {
        match self {
            Shell::Bash => self::shells::Bash.generate(cmd, buf),
            Shell::Fish => self::shells::Fish.generate(cmd, buf),
            Shell::Zsh => self::shells::Zsh.generate(cmd, buf),
        }
    }
}

pub(crate) fn main(cmd: &clap::Command, shell: &Shell) {
    let buf = &mut std::io::stdout();
    shell.generate(cmd, buf);
}
