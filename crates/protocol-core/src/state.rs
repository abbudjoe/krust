//! Agent execution state machine.
//!
//! Defines the states an agent task passes through and the valid
//! transitions between them. The state machine is deterministic:
//! given a state and an event, the next state is always predictable.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
        attempt: u32,
    },

    /// Waiting for human confirmation (policy gate).
    WaitingHuman {
        request_id: String,
        tool_call_id: String,
        reason: String,
        step: u32,
        attempt: u32,
    },

    /// Verifying that an action produced the expected result.
    Verifying {
        tool_call_id: String,
        step: u32,
        attempt: u32,
    },

    /// Action failed, evaluating retry/fallback.
    Retrying {
        tool_call_id: String,
        step: u32,
        attempt: u32,
        max_attempts: u32,
    },

    /// Task completed with verified artifacts.
    Completed { artifacts: Vec<String> },

    /// Task failed permanently (retries exhausted or unrecoverable).
    Failed { reason: String },

    /// Task was cancelled by user or policy.
    Cancelled { reason: String },
}

/// An event that triggers a state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransitionEvent {
    /// Planning complete, ready to execute.
    PlanReady { tool_call_id: String, step: u32 },

    /// Tool execution completed, evidence attached.
    ToolCompleted { tool_call_id: String, success: bool },

    /// Policy requires human confirmation before proceeding.
    PolicyGate {
        request_id: String,
        tool_call_id: String,
        reason: String,
    },

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
        (AgentState::Planning, TransitionEvent::PlanReady { tool_call_id, step }) => {
            Some(AgentState::Executing {
                tool_call_id: tool_call_id.clone(),
                step: *step,
                attempt: 0,
            })
        }

        // Planning → Cancelled
        (AgentState::Planning, TransitionEvent::Cancel { reason }) => Some(AgentState::Cancelled {
            reason: reason.clone(),
        }),

        // Executing → PolicyGate (WaitingHuman)
        (
            AgentState::Executing { step, attempt, .. },
            TransitionEvent::PolicyGate {
                request_id,
                tool_call_id,
                reason,
            },
        ) => Some(AgentState::WaitingHuman {
            request_id: request_id.clone(),
            tool_call_id: tool_call_id.clone(),
            reason: reason.clone(),
            step: *step,
            attempt: *attempt,
        }),

        // Executing → Verifying (tool completed successfully)
        (
            AgentState::Executing {
                tool_call_id,
                step,
                attempt,
            },
            TransitionEvent::ToolCompleted { success: true, .. },
        ) => Some(AgentState::Verifying {
            tool_call_id: tool_call_id.clone(),
            step: *step,
            attempt: *attempt,
        }),

        // Executing → Retrying (tool failed)
        (
            AgentState::Executing {
                tool_call_id,
                step,
                attempt,
            },
            TransitionEvent::ToolCompleted { success: false, .. },
        ) => Some(AgentState::Retrying {
            tool_call_id: tool_call_id.clone(),
            step: *step,
            attempt: *attempt,
            max_attempts: 3,
        }),

        // Executing → Cancelled
        (AgentState::Executing { .. }, TransitionEvent::Cancel { reason }) => {
            Some(AgentState::Cancelled {
                reason: reason.clone(),
            })
        }

        // WaitingHuman → Executing (approved) — preserves step/attempt context
        (
            AgentState::WaitingHuman {
                tool_call_id,
                step,
                attempt,
                ..
            },
            TransitionEvent::HumanDecision { approved: true, .. },
        ) => Some(AgentState::Executing {
            tool_call_id: tool_call_id.clone(),
            step: *step,
            attempt: *attempt,
        }),

        // WaitingHuman → Cancelled (denied)
        (
            AgentState::WaitingHuman { .. },
            TransitionEvent::HumanDecision {
                approved: false, ..
            },
        ) => Some(AgentState::Cancelled {
            reason: "Human denied action".to_string(),
        }),

        // Verifying → Completed
        (AgentState::Verifying { .. }, TransitionEvent::VerificationPassed { artifacts }) => {
            Some(AgentState::Completed {
                artifacts: artifacts.clone(),
            })
        }

        // Verifying → Retrying (verification failed)
        (
            AgentState::Verifying {
                tool_call_id,
                step,
                attempt,
            },
            TransitionEvent::VerificationFailed { .. },
        ) => Some(AgentState::Retrying {
            tool_call_id: tool_call_id.clone(),
            step: *step,
            attempt: *attempt,
            max_attempts: 3,
        }),

        // Verifying → Executing (move to next step via PlanReady)
        (
            AgentState::Verifying { step, .. },
            TransitionEvent::PlanReady {
                tool_call_id,
                step: next_step,
            },
        ) => {
            // Use the step from PlanReady if provided, otherwise increment
            let new_step = if *next_step > 0 { *next_step } else { step + 1 };
            Some(AgentState::Executing {
                tool_call_id: tool_call_id.clone(),
                step: new_step,
                attempt: 0,
            })
        }

        // Retrying → Executing (retry attempt)
        (
            AgentState::Retrying {
                tool_call_id,
                step,
                attempt,
                max_attempts,
            },
            TransitionEvent::RetryRequested { .. },
        ) => {
            if *attempt < *max_attempts {
                Some(AgentState::Executing {
                    tool_call_id: tool_call_id.clone(),
                    step: *step,
                    attempt: attempt + 1,
                })
            } else {
                None
            }
        }

        // Retrying → Failed (exhausted)
        (AgentState::Retrying { .. }, TransitionEvent::RetriesExhausted { reason }) => {
            Some(AgentState::Failed {
                reason: reason.clone(),
            })
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
            step: 1,
        };
        let next = apply_transition(&state, &event).unwrap();
        assert_eq!(
            next,
            AgentState::Executing {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 0,
            }
        );
    }

    #[test]
    fn test_executing_to_policy_gate() {
        let state = AgentState::Executing {
            tool_call_id: "tc_1".to_string(),
            step: 1,
            attempt: 0,
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
            step: 2,
            attempt: 1,
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
    fn test_human_approve_preserves_step_and_attempt() {
        let state = AgentState::WaitingHuman {
            request_id: "req_1".to_string(),
            tool_call_id: "tc_9".to_string(),
            reason: "confirm payment".to_string(),
            step: 3,
            attempt: 2,
        };
        let event = TransitionEvent::HumanDecision {
            request_id: "req_1".to_string(),
            approved: true,
        };
        let next = apply_transition(&state, &event).unwrap();
        assert_eq!(
            next,
            AgentState::Executing {
                tool_call_id: "tc_9".to_string(),
                step: 3,
                attempt: 2,
            }
        );
    }

    #[test]
    fn test_verification_to_completed() {
        let state = AgentState::Verifying {
            tool_call_id: "tc_1".to_string(),
            step: 1,
            attempt: 0,
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
    fn test_verification_failure_preserves_attempt() {
        let executing = AgentState::Executing {
            tool_call_id: "tc_1".to_string(),
            step: 1,
            attempt: 2,
        };

        let verifying = apply_transition(
            &executing,
            &TransitionEvent::ToolCompleted {
                tool_call_id: "tc_1".to_string(),
                success: true,
            },
        )
        .unwrap();

        assert_eq!(
            verifying,
            AgentState::Verifying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 2,
            }
        );

        let retrying = apply_transition(
            &verifying,
            &TransitionEvent::VerificationFailed {
                reason: "missing evidence".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            retrying,
            AgentState::Retrying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 2,
                max_attempts: 3,
            }
        );
    }

    #[test]
    fn test_invalid_transition_returns_none() {
        let state = AgentState::Completed { artifacts: vec![] };
        let event = TransitionEvent::PlanReady {
            tool_call_id: "tc_1".to_string(),
            step: 1,
        };
        assert!(apply_transition(&state, &event).is_none());
    }

    #[test]
    fn test_retry_attempt_increments_and_exhausts() {
        // Start executing
        let state = AgentState::Executing {
            tool_call_id: "tc_1".to_string(),
            step: 1,
            attempt: 0,
        };

        // Fail → Retrying (attempt carries from Executing)
        let fail_event = TransitionEvent::ToolCompleted {
            tool_call_id: "tc_1".to_string(),
            success: false,
        };
        let retrying = apply_transition(&state, &fail_event).unwrap();
        assert_eq!(
            retrying,
            AgentState::Retrying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 0,
                max_attempts: 3,
            }
        );

        // Retry → Executing with attempt=1
        let retry_event = TransitionEvent::RetryRequested { max_attempts: 3 };
        let exec1 = apply_transition(&retrying, &retry_event).unwrap();
        assert_eq!(
            exec1,
            AgentState::Executing {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 1,
            }
        );

        // Fail again → Retrying attempt=1
        let retrying2 = apply_transition(&exec1, &fail_event).unwrap();
        assert_eq!(
            retrying2,
            AgentState::Retrying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 1,
                max_attempts: 3,
            }
        );

        // Retry → Executing with attempt=2
        let exec2 = apply_transition(&retrying2, &retry_event).unwrap();
        assert_eq!(
            exec2,
            AgentState::Executing {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 2,
            }
        );

        // Fail again → Retrying attempt=2
        let retrying3 = apply_transition(&exec2, &fail_event).unwrap();
        assert_eq!(
            retrying3,
            AgentState::Retrying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 2,
                max_attempts: 3,
            }
        );

        // Retry → Executing with attempt=3
        let exec3 = apply_transition(&retrying3, &retry_event).unwrap();
        assert_eq!(
            exec3,
            AgentState::Executing {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 3,
            }
        );

        // Fail again → Retrying attempt=3
        let retrying4 = apply_transition(&exec3, &fail_event).unwrap();
        assert_eq!(
            retrying4,
            AgentState::Retrying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 3,
                max_attempts: 3,
            }
        );

        // Retry should now fail (attempt >= max_attempts)
        assert!(apply_transition(&retrying4, &retry_event).is_none());

        // Exhaustion → Failed
        let exhausted = TransitionEvent::RetriesExhausted {
            reason: "Max retries reached".to_string(),
        };
        let failed = apply_transition(&retrying4, &exhausted).unwrap();
        match failed {
            AgentState::Failed { reason } => assert_eq!(reason, "Max retries reached"),
            _ => panic!("Expected Failed state"),
        }
    }

    #[test]
    fn test_multi_step_progression() {
        // Step 1: Planning → Executing step=1
        let state = AgentState::Planning;
        let exec1 = apply_transition(
            &state,
            &TransitionEvent::PlanReady {
                tool_call_id: "tc_1".to_string(),
                step: 1,
            },
        )
        .unwrap();
        assert_eq!(
            exec1,
            AgentState::Executing {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 0,
            }
        );

        // Succeed → Verifying step=1
        let verifying1 = apply_transition(
            &exec1,
            &TransitionEvent::ToolCompleted {
                tool_call_id: "tc_1".to_string(),
                success: true,
            },
        )
        .unwrap();
        assert_eq!(
            verifying1,
            AgentState::Verifying {
                tool_call_id: "tc_1".to_string(),
                step: 1,
                attempt: 0,
            }
        );

        // PlanReady for step 2 → Executing step=2
        let exec2 = apply_transition(
            &verifying1,
            &TransitionEvent::PlanReady {
                tool_call_id: "tc_2".to_string(),
                step: 2,
            },
        )
        .unwrap();
        assert_eq!(
            exec2,
            AgentState::Executing {
                tool_call_id: "tc_2".to_string(),
                step: 2,
                attempt: 0,
            }
        );

        // Succeed → Verifying step=2
        let verifying2 = apply_transition(
            &exec2,
            &TransitionEvent::ToolCompleted {
                tool_call_id: "tc_2".to_string(),
                success: true,
            },
        )
        .unwrap();
        assert_eq!(
            verifying2,
            AgentState::Verifying {
                tool_call_id: "tc_2".to_string(),
                step: 2,
                attempt: 0,
            }
        );

        // Complete
        let completed = apply_transition(
            &verifying2,
            &TransitionEvent::VerificationPassed {
                artifacts: vec!["done".to_string()],
            },
        )
        .unwrap();
        match completed {
            AgentState::Completed { artifacts } => assert_eq!(artifacts, vec!["done"]),
            _ => panic!("Expected Completed"),
        }
    }
}
