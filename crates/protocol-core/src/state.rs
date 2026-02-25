//! Agent execution state machine.
//!
//! Defines the states an agent task passes through and the valid
//! transitions between them. The state machine is deterministic:
//! given a state and an event, the next state is always predictable.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The execution state of an agent task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum AgentState {
    /// Task received, intent parsed, planning next action.
    Planning,

    /// Actively executing a tool action.
    Executing {
        tool_call_id: String,
        step: u32,
    },

    /// Waiting for human confirmation (policy gate).
    WaitingHuman {
        request_id: String,
        tool_call_id: String,
        reason: String,
    },

    /// Verifying that an action produced the expected result.
    Verifying {
        tool_call_id: String,
        step: u32,
    },

    /// Action failed, evaluating retry/fallback.
    Retrying {
        tool_call_id: String,
        step: u32,
        attempt: u32,
        max_attempts: u32,
    },

    /// Task completed with verified artifacts.
    Completed {
        artifacts: Vec<String>,
    },

    /// Task failed permanently (retries exhausted or unrecoverable).
    Failed {
        reason: String,
    },

    /// Task was cancelled by user or policy.
    Cancelled {
        reason: String,
    },
}

/// An event that triggers a state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransitionEvent {
    /// Planning complete, ready to execute.
    PlanReady { tool_call_id: String },

    /// Tool execution completed, evidence attached.
    ToolCompleted { tool_call_id: String, success: bool },

    /// Policy requires human confirmation before proceeding.
    PolicyGate { request_id: String, tool_call_id: String, reason: String },

    /// Human responded to confirmation request.
    HumanDecision { request_id: String, approved: bool },

    /// Artifact verification passed.
    VerificationPassed { artifacts: Vec<String> },

    /// Artifact verification failed.
    VerificationFailed { reason: String },

    /// Retry requested.
    RetryRequested { max_attempts: u32 },

    /// All retries exhausted.
    RetriesExhausted { reason: String },

    /// User or system cancelled the task.
    Cancel { reason: String },
}

/// A recorded state transition with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub id: Uuid,
    pub from: AgentState,
    pub to: AgentState,
    pub event: TransitionEvent,
    pub timestamp: DateTime<Utc>,
}

impl Transition {
    pub fn new(from: AgentState, to: AgentState, event: TransitionEvent) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            event,
            timestamp: Utc::now(),
        }
    }
}

/// Apply a transition event to a state, returning the new state.
///
/// Returns `None` if the transition is invalid for the current state.
pub fn apply_transition(state: &AgentState, event: &TransitionEvent) -> Option<AgentState> {
    match (state, event) {
        // Planning → Executing
        (AgentState::Planning, TransitionEvent::PlanReady { tool_call_id }) => {
            Some(AgentState::Executing {
                tool_call_id: tool_call_id.clone(),
                step: 1,
            })
        }

        // Planning → Cancelled
        (AgentState::Planning, TransitionEvent::Cancel { reason }) => {
            Some(AgentState::Cancelled { reason: reason.clone() })
        }

        // Executing → PolicyGate (WaitingHuman)
        (AgentState::Executing { .. }, TransitionEvent::PolicyGate { request_id, tool_call_id, reason }) => {
            Some(AgentState::WaitingHuman {
                request_id: request_id.clone(),
                tool_call_id: tool_call_id.clone(),
                reason: reason.clone(),
            })
        }

        // Executing → Verifying (tool completed successfully)
        (AgentState::Executing { tool_call_id, step }, TransitionEvent::ToolCompleted { success: true, .. }) => {
            Some(AgentState::Verifying {
                tool_call_id: tool_call_id.clone(),
                step: *step,
            })
        }

        // Executing → Retrying (tool failed)
        (AgentState::Executing { tool_call_id, step }, TransitionEvent::ToolCompleted { success: false, .. }) => {
            Some(AgentState::Retrying {
                tool_call_id: tool_call_id.clone(),
                step: *step,
                attempt: 1,
                max_attempts: 3,
            })
        }

        // Executing → Cancelled
        (AgentState::Executing { .. }, TransitionEvent::Cancel { reason }) => {
            Some(AgentState::Cancelled { reason: reason.clone() })
        }

        // WaitingHuman → Executing (approved)
        (AgentState::WaitingHuman { tool_call_id, .. }, TransitionEvent::HumanDecision { approved: true, .. }) => {
            Some(AgentState::Executing {
                tool_call_id: tool_call_id.clone(),
                step: 1,
            })
        }

        // WaitingHuman → Cancelled (denied)
        (AgentState::WaitingHuman { .. }, TransitionEvent::HumanDecision { approved: false, .. }) => {
            Some(AgentState::Cancelled {
                reason: "Human denied action".to_string(),
            })
        }

        // Verifying → Completed
        (AgentState::Verifying { .. }, TransitionEvent::VerificationPassed { artifacts }) => {
            Some(AgentState::Completed { artifacts: artifacts.clone() })
        }

        // Verifying → Retrying (verification failed)
        (AgentState::Verifying { tool_call_id, step }, TransitionEvent::VerificationFailed { .. }) => {
            Some(AgentState::Retrying {
                tool_call_id: tool_call_id.clone(),
                step: *step,
                attempt: 1,
                max_attempts: 3,
            })
        }

        // Verifying → Planning (move to next step)
        (AgentState::Verifying { .. }, TransitionEvent::PlanReady { tool_call_id }) => {
            Some(AgentState::Executing {
                tool_call_id: tool_call_id.clone(),
                step: 1,
            })
        }

        // Retrying → Executing (retry attempt)
        (AgentState::Retrying { tool_call_id, step, attempt, max_attempts }, TransitionEvent::RetryRequested { .. }) => {
            if *attempt < *max_attempts {
                Some(AgentState::Executing {
                    tool_call_id: tool_call_id.clone(),
                    step: *step,
                })
            } else {
                None
            }
        }

        // Retrying → Failed (exhausted)
        (AgentState::Retrying { .. }, TransitionEvent::RetriesExhausted { reason }) => {
            Some(AgentState::Failed { reason: reason.clone() })
        }

        // Invalid transition
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planning_to_executing() {
        let state = AgentState::Planning;
        let event = TransitionEvent::PlanReady {
            tool_call_id: "tc_1".to_string(),
        };
        let next = apply_transition(&state, &event).unwrap();
        assert_eq!(
            next,
            AgentState::Executing {
                tool_call_id: "tc_1".to_string(),
                step: 1,
            }
        );
    }

    #[test]
    fn test_executing_to_policy_gate() {
        let state = AgentState::Executing {
            tool_call_id: "tc_1".to_string(),
            step: 1,
        };
        let event = TransitionEvent::PolicyGate {
            request_id: "req_1".to_string(),
            tool_call_id: "tc_1".to_string(),
            reason: "payment action".to_string(),
        };
        let next = apply_transition(&state, &event).unwrap();
        match next {
            AgentState::WaitingHuman { reason, .. } => {
                assert_eq!(reason, "payment action");
            }
            _ => panic!("Expected WaitingHuman state"),
        }
    }

    #[test]
    fn test_human_deny_cancels() {
        let state = AgentState::WaitingHuman {
            request_id: "req_1".to_string(),
            tool_call_id: "tc_1".to_string(),
            reason: "payment".to_string(),
        };
        let event = TransitionEvent::HumanDecision {
            request_id: "req_1".to_string(),
            approved: false,
        };
        let next = apply_transition(&state, &event).unwrap();
        match next {
            AgentState::Cancelled { .. } => {}
            _ => panic!("Expected Cancelled state"),
        }
    }

    #[test]
    fn test_verification_to_completed() {
        let state = AgentState::Verifying {
            tool_call_id: "tc_1".to_string(),
            step: 1,
        };
        let event = TransitionEvent::VerificationPassed {
            artifacts: vec!["confirmation_number: ABC123".to_string()],
        };
        let next = apply_transition(&state, &event).unwrap();
        match next {
            AgentState::Completed { artifacts } => {
                assert_eq!(artifacts.len(), 1);
            }
            _ => panic!("Expected Completed state"),
        }
    }

    #[test]
    fn test_invalid_transition_returns_none() {
        let state = AgentState::Completed {
            artifacts: vec![],
        };
        let event = TransitionEvent::PlanReady {
            tool_call_id: "tc_1".to_string(),
        };
        assert!(apply_transition(&state, &event).is_none());
    }
}
