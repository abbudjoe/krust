//! # krust-protocol-core
//!
//! State machine, typed intents, artifact contracts, and policy engine
//! for verified AI agent execution.
//!
//! This crate is pure logic — no I/O, no platform APIs. It defines:
//! - **States and transitions** for agent task execution
//! - **Typed intents** describing what an agent wants to accomplish
//! - **Artifact contracts** defining what "done" looks like, with evidence
//! - **Policy engine** for allow/deny/confirm decisions on actions
//! - **Checkpoint/resume** protocol for durable execution

pub mod artifact;
pub mod checkpoint;
pub mod error;
pub mod intent;
pub mod policy;
pub mod state;

pub use artifact::{ArtifactContract, Evidence};
pub use checkpoint::Checkpoint;
pub use error::ProtocolError;
pub use intent::Intent;
pub use policy::{Policy, PolicyDecision};
pub use state::{AgentState, Transition};
