//! Manages the collector's operational state based on commands from the database.
//!
//! This module translates the `CollectorRole` received from the database's
//! scheduler into a simple, local `State`. The main `runner` loop uses this
//! state machine to decide whether it should be actively pinging targets or
//! standing by.

use log::info;
use zzping_proto::zzping::{CollectorRole, HeartbeatResponse};

/// The set of possible operational states for a collector instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The collector is the active `Primary` and should be pinging all targets
    /// defined in the `intent.ron` configuration.
    Pinging,
    /// The collector is a `Standby` instance. It maintains a connection to the
    /// database but does not perform any pinging. It is ready to be promoted
    /// to `Pinging` at any time.
    Standby,
    /// The collector has been commanded to shut down by the database, typically
    /// after a successful handoff to a new `Primary`. The main loop should
    /// exit gracefully upon seeing this state.
    Shutdown,
}

/// An action that the `runner` loop should perform in response to a state change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The collector has been promoted from `Standby` to `Pinging` and needs to
    /// seed its internal buffer with recent data from the database. This ensures
    /// a seamless data stream during a handoff.
    SeedBuffer,
}

/// A simple state machine that models the collector's lifecycle.
#[derive(Debug, Clone)]
pub struct StateMachine {
    /// The current operational state of the collector.
    pub current_state: State,
    /// The unique identifier for this collector, used for logging and identification.
    pub collector_uuid: String,
}

impl StateMachine {
    /// Creates a new `StateMachine` for a collector.
    ///
    /// All collectors begin their lifecycle in the `Standby` state. They will
    /// only transition to `Pinging` after being explicitly promoted to `Primary`
    /// by the database scheduler.
    pub fn new(collector_uuid: String) -> Self {
        Self {
            current_state: State::Standby,
            collector_uuid,
        }
    }

    /// Processes a `HeartbeatResponse` from the server, updates the internal state,
    /// and returns an `Option<Action>` for the `runner` to execute.
    ///
    /// This is the core logic of the state machine. It translates the `CollectorRole`
    /// into a local `State` and determines if any special actions are required
    /// as a result of a state transition.
    pub fn handle_heartbeat_response(&mut self, response: &HeartbeatResponse) -> Option<Action> {
        let new_role = CollectorRole::try_from(response.role).unwrap_or(CollectorRole::Standby);

        let new_state = match new_role {
            CollectorRole::Primary => State::Pinging,
            CollectorRole::Standby | CollectorRole::Supervising => State::Standby,
            CollectorRole::Shutdown => State::Shutdown,
        };

        if self.current_state != new_state {
            info!(
                "State transition: {:?} -> {:?}",
                self.current_state, new_state
            );
            let old_state = self.current_state.clone();
            self.current_state = new_state;

            // If we just got promoted from Standby to Pinging, we need to seed our buffer.
            if old_state == State::Standby && self.current_state == State::Pinging {
                return Some(Action::SeedBuffer);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;
    use zzping_proto::zzping::HeartbeatResponse;

    #[test]
    #[timeout(100)]
    fn test_initial_state_is_standby() {
        let machine = StateMachine::new("test".to_string());
        assert_eq!(machine.current_state, State::Standby);
    }

    #[test]
    #[timeout(100)]
    fn test_promotion_to_primary_returns_seed_action() {
        let mut machine = StateMachine::new("test".to_string());
        assert_eq!(machine.current_state, State::Standby);
        let response = HeartbeatResponse {
            role: CollectorRole::Primary as i32,
            ..Default::default()
        };
        let action = machine.handle_heartbeat_response(&response);
        assert_eq!(machine.current_state, State::Pinging);
        assert_eq!(action, Some(Action::SeedBuffer));
    }

    #[test]
    #[timeout(100)]
    fn test_demotion_to_standby_returns_no_action() {
        let mut machine = StateMachine::new("test".to_string());
        machine.current_state = State::Pinging;
        let response = HeartbeatResponse {
            role: CollectorRole::Standby as i32,
            ..Default::default()
        };
        let action = machine.handle_heartbeat_response(&response);
        assert_eq!(machine.current_state, State::Standby);
        assert_eq!(action, None);
    }

    #[test]
    #[timeout(100)]
    fn test_shutdown_role_returns_no_action() {
        let mut machine = StateMachine::new("test".to_string());
        let response = HeartbeatResponse {
            role: CollectorRole::Shutdown as i32,
            ..Default::default()
        };
        let action = machine.handle_heartbeat_response(&response);
        assert_eq!(machine.current_state, State::Shutdown);
        assert_eq!(action, None);
    }

    #[test]
    #[timeout(100)]
    fn test_no_state_change_returns_no_action() {
        let mut machine = StateMachine::new("test".to_string());
        machine.current_state = State::Pinging;
        let response = HeartbeatResponse {
            role: CollectorRole::Primary as i32,
            ..Default::default()
        };
        let action = machine.handle_heartbeat_response(&response);
        assert_eq!(machine.current_state, State::Pinging);
        assert_eq!(action, None);
    }
}
