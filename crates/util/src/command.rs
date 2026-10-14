use std::ffi::OsStr;

mod darwin;

pub use darwin::{Child, Command, Stdio};

pub use gpui_util::new_std_command;

pub fn new_command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}
