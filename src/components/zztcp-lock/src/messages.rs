use actix::prelude::*;

/// Notification sent to CState when the lock status changes.
#[derive(Message, Debug, Clone, PartialEq)]
#[rtype(result = "()")]
pub struct UpdateLockStatus {
    /// True if we currently hold the TCP port.
    pub locked: bool,
}
