mod conn;
mod message_stream;
mod peer;

pub use conn::Connection;
pub use peer::*;
pub use proto;
pub use proto::{Receipt, TypedEnvelope, error::*};
mod macros;

#[cfg(feature = "gpui")]
mod proto_client;
#[cfg(feature = "gpui")]
pub use proto_client::*;

pub const PROTOCOL_VERSION: u32 = 68;

/// Identifies a project in the remote editor protocol.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct ProjectId(pub u64);
impl ProjectId {
    pub fn to_proto(self) -> u64 {
        self.0
    }
}
