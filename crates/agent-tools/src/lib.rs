//! # ember-agent-tools
//!
//! Tool framework bridging protocol-core with actual capabilities.
//!
//! - **Tool trait**: any capability implements this
//! - **ToolRegistry**: manages available tools and dispatches calls
//! - **Result validation**: verify tool results match evidence claims

pub mod registry;
pub mod tool;

pub use registry::ToolRegistry;
pub use tool::{Tool, ToolCall, ToolResult};
