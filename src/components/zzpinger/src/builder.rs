//! Builder module for constructing the zzpinger component.

use actix::Recipient;
use actix::prelude::*;
use zzmem_db::messages::StorePingResult;

use crate::scheduler::PingerSchedulerActor;

/// Builder for creating the Pinger component.
pub struct PingerBuilder {
    memdb_recipient: Option<Recipient<StorePingResult>>,
}

impl Default for PingerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PingerBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            memdb_recipient: None,
        }
    }

    /// Sets the MemDB recipient.
    pub fn with_memdb_recipient(mut self, recipient: Recipient<StorePingResult>) -> Self {
        self.memdb_recipient = Some(recipient);
        self
    }

    /// Builds the Pinger component.
    pub fn build(self) -> Pinger {
        let memdb_recipient = self.memdb_recipient.expect("MemDB recipient not set");
        Pinger { memdb_recipient }
    }
}

/// Handle to the Pinger component.
pub struct Pinger {
    memdb_recipient: Recipient<StorePingResult>,
}

impl Pinger {
    /// Starts the pinger actors on a dedicated arbiter.
    pub fn start(self) -> Addr<PingerSchedulerActor> {
        let arbiter = Arbiter::new();
        let memdb_recipient = self.memdb_recipient;

        PingerSchedulerActor::start_in_arbiter(&arbiter.handle(), move |_| {
            PingerSchedulerActor::new(None, memdb_recipient.clone())
        })
    }
}
