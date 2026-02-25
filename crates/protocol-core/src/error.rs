//! Error types for the protocol core.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("Invalid state transition from {from} via {event}")]
    InvalidTransition { from: String, event: String },

    #[error("Policy denied action: {reason}")]
    PolicyDenied { reason: String },

    #[error("Artifact verification failed: {reason}")]
    VerificationFailed { reason: String },

    #[error("Checkpoint not found: {id}")]
    CheckpointNotFound { id: String },

    #[error("Checkpoint is stale (created at {created_at})")]
    StaleCheckpoint { created_at: String },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
