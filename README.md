# Krust

**Verified execution protocols for AI agents.**

A Rust toolkit that makes agent actions deterministic, observable, and recoverable.

---

## What is this?

AI agents fail ~40% of the time on real-world tasks — not because the models are dumb, but because there's no infrastructure between "AI decides to do something" and "thing actually happens." Every agent app builds this control layer from scratch and gets it wrong differently.

Krust is the open-source protocol layer that fixes this. It provides:

- **A state machine** that governs agent task execution with deterministic transitions
- **Artifact contracts** that verify actions actually worked (not just that the tool returned "success")
- **A policy engine** that gates sensitive actions with allow/deny/confirm decisions
- **Checkpoint/resume** for durable execution that survives crashes and restarts
- **A web interaction layer** with pluggable backends (CDP, accessibility APIs, native browsers)

## Architecture

```
┌─────────────────────────────────┐
│  protocol-core                  │  State machine, intents, artifacts,
│  (pure logic, no I/O)           │  policy engine, checkpoint/resume
├─────────────────────────────────┤
│  agent-tools                    │  Tool framework + MCP compatibility
├──────────┬──────────────────────┤
│ agent-web│  Other adapters...   │  Pluggable capability modules
│ (browser)│  (fs, app, sensors)  │
└──────────┴──────────────────────┘
```

## Crates

| Crate | Description |
|-------|-------------|
| `krust-protocol-core` | State machine, typed intents, artifact contracts, policy engine |
| `krust-agent-web` | Web interaction abstractions with pluggable backends |
| `krust-agent-tools` | Tool framework and MCP compatibility layer |
| `krust-agent-eval` | Evaluation harness and reliability metrics |
| `krust-mcp` | MCP server binary — point any agent at this |

## Quick Start

```bash
# Build everything
cargo build

# Run tests
cargo test

# Start the MCP server (coming soon)
cargo run --bin krust-mcp
```

## Testing with Claude Code / Codex

Krust exposes its tools as an MCP server. Any MCP-compatible agent can use it:

```bash
# Add to your Claude Code MCP config:
{
  "krust": {
    "command": "cargo",
    "args": ["run", "--bin", "krust-mcp"]
  }
}
```

## Status

🚧 **Early development.** The protocol-core state machine and type system are taking shape. Agent-web CDP backend and MCP server are next.

## License

MIT
