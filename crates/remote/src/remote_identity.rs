use crate::RemoteConnectionOptions;

/// A normalized remote identity for matching live remote hosts against
/// persisted remote metadata.
///
/// This mirrors workspace persistence identity semantics rather than full
/// `RemoteConnectionOptions` equality, so runtime-only fields like SSH
/// nicknames do not affect matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RemoteConnectionIdentity {
    Ssh {
        host: String,
        username: Option<String>,
        port: Option<u16>,
    },
    Wsl {
        distro_name: String,
        user: Option<String>,
    },
}

impl RemoteConnectionIdentity {
    /// A stable string form of this identity, suitable for use in
    /// persistence keys (e.g. database keys scoped to a remote host).
    pub fn persistence_key(&self) -> String {
        match self {
            Self::Ssh {
                host,
                username,
                port,
            } => format!(
                "ssh:{}@{}:{}",
                username.as_deref().unwrap_or_default(),
                host,
                port.map(|port| port.to_string()).unwrap_or_default()
            ),
            Self::Wsl { distro_name, user } => format!(
                "wsl:{}@{}",
                user.as_deref().unwrap_or_default(),
                distro_name
            ),
        }
    }
}

impl From<&RemoteConnectionOptions> for RemoteConnectionIdentity {
    fn from(options: &RemoteConnectionOptions) -> Self {
        match options {
            RemoteConnectionOptions::Ssh(options) => Self::Ssh {
                host: options.host.to_string(),
                username: options.username.clone(),
                port: options.port,
            },
            RemoteConnectionOptions::Wsl(options) => Self::Wsl {
                distro_name: options.distro_name.clone(),
                user: options.user.clone(),
            },
        }
    }
}

pub fn remote_connection_identity(options: &RemoteConnectionOptions) -> RemoteConnectionIdentity {
    options.into()
}

pub fn same_remote_connection_identity(
    left: Option<&RemoteConnectionOptions>,
    right: Option<&RemoteConnectionOptions>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            remote_connection_identity(left) == remote_connection_identity(right)
        }
        (None, None) => true,
        _ => false,
    }
}
