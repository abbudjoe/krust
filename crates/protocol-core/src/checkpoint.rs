//! Checkpoint/resume protocol for durable execution.
//!
//! Checkpoints capture the full state of an agent task at a point in time,
//! enabling resume after crash, restart, or transfer between hosts.

use crate::intent::Intent;
use crate::state::AgentState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A checkpoint capturing the full state of a task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Unique checkpoint identifier.
    pub id: Uuid,

    /// The task this checkpoint belongs to.
    pub task_id: Uuid,

    /// Current state at checkpoint time.
    pub state: AgentState,

    /// The original task intent.
    pub intent: Intent,

    /// Number of steps completed so far.
    pub steps_completed: u32,

    /// Total steps estimated (may change as plan evolves).
    pub steps_total: Option<u32>,

    /// History of tool call IDs executed so far.
    pub completed_tool_calls: Vec<String>,

    /// When this checkpoint was created.
    pub created_at: DateTime<Utc>,
}

impl Checkpoint {
    pub fn new(task_id: Uuid, state: AgentState, intent: Intent) -> Self {
        Self {
            id: Uuid::new_v4(),
            task_id,
            state,
            intent,
            steps_completed: 0,
            steps_total: None,
            completed_tool_calls: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Check if this checkpoint is stale (older than the given duration).
    pub fn is_stale(&self, max_age: chrono::Duration) -> bool {
        Utc::now() - self.created_at > max_age
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_creation() {
        let task_id = Uuid::new_v4();
        let intent = Intent::new("web.navigate");
        let checkpoint = Checkpoint::new(task_id, AgentState::Planning, intent);

        assert_eq!(checkpoint.task_id, task_id);
        assert_eq!(checkpoint.state, AgentState::Planning);
        assert_eq!(checkpoint.steps_completed, 0);
    }

    #[test]
    fn test_checkpoint_serialization() {
        let checkpoint = Checkpoint::new(
            Uuid::new_v4(),
            AgentState::Planning,
            Intent::new("web.click"),
        );
        let json = serde_json::to_string(&checkpoint).unwrap();
        let deserialized: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, checkpoint.task_id);
    }
}
