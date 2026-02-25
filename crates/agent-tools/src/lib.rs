//! # krust-agent-tools
//!
//! Tool framework bridging protocol-core with actual capabilities.
//!
//! - **Tool trait**: any capability implements this
//! - **ToolRegistry**: manages available tools and dispatches calls
//! - **Result validation**: verify tool results match evidence claims

pub mod tool;
pub mod registry;

pub use tool::{Tool, ToolCall, ToolResult};
pub use registry::ToolRegistry;
