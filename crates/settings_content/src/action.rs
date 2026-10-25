use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;

use serde::Deserialize;
use serde::Serialize;

/// The value of an entry in `command_aliases`, either a registered action name
/// or an arbitrary string, for example, a vim command like `"w"` or `"wq"`.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(transparent)]
pub struct CommandAliasTarget(String);

impl CommandAliasTarget {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl Display for CommandAliasTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        write!(formatter, "{}", self.0)
    }
}

impl AsRef<str> for CommandAliasTarget {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
