//! Manages the collector's own operational state based on commands from the database.

use zzping_proto::zzping::{CollectorRole, HeartbeatResponse};
use log::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    // The collector is actively pinging targets.
    Pinging,
    // The collector is on standby and should not be pinging.
    Standby,
    // The collector should exit gracefully.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The collector has been promoted and needs to seed its buffer.
    SeedBuffer,
}

#[derive(Debug, Clone)]
pub struct StateMachine {
    pub current_state: State,
    pub collector_uuid: String,
}

impl StateMachine {
    pub fn new(collector_uuid: String) -> Self {
        Self {
            // All collectors start in standby until they are promoted to primary.
            current_state: State::Standby,
            collector_uuid,
        }
    }

    /// Processes a heartbeat response from the server, updates the state,
    /// and returns an optional action for the runner to perform.
    pub fn handle_heartbeat_response(&mut self, response: &HeartbeatResponse) -> Option<Action> {
        let new_role = CollectorRole::try_from(response.role).unwrap_or(CollectorRole::Standby);

        let new_state = match new_role {
            CollectorRole::Primary => State::Pinging,
            CollectorRole::Standby | CollectorRole::Supervising => State::Standby,
            CollectorRole::Shutdown => State::Shutdown,
        };

        if self.current_state != new_state {
            info!(
                "Collector state changing from {:?} to {:?}",
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
    use zzping_proto::zzping::HeartbeatResponse;

    #[test]
    fn test_initial_state_is_standby() {
        let machine = StateMachine::new("test".to_string());
        assert_eq!(machine.current_state, State::Standby);
    }

    #[test]
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
