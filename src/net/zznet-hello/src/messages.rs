//! Message types for zznet-hello.

use actix::Message;

/// A message to get the role of the connection manager.
#[derive(Message, Clone)]
#[rtype(result = "String")]
pub struct GetRole;
