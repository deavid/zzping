//! Messages for the `zztcp-lock` component.
use actix::prelude::*;

/// Notification sent to `CState` when the lock status changes.
#[derive(Message, Debug, Clone, PartialEq)]
#[rtype(result = "()")]
pub struct UpdateLockStatus {
    /// True if we currently hold the TCP port.
    pub locked: bool,
}

/// Command sent to `TcpLockActor` to enable or disable lock acquisition.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetLockDesired {
    /// True if the actor should attempt to hold the lock.
    pub required: bool,
}

/// Internal message to trigger a lock acquisition check.
/// This allows both automatic (via run_later) and manual (via tests) retry attempts.
#[derive(Message)]
#[rtype(result = "()")]
pub struct CheckLock;
