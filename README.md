# Krust

**Verified execution protocols for AI agents.**

A Rust toolkit that makes agent actions deterministic, observable, and recoverable. Plug it into any MCP-compatible agent (Claude Code, Codex, Cursor) and get state-machine-backed browser automation with evidence verification out of the box.

---

## Quick Start

### Prerequisites

- **Rust** (1.70+): [rustup.rs](https://rustup.rs)
- **Chrome or Chromium** installed (the MCP server controls it via CDP)

### Build

```bash
git clone https://github.com/abbudjoe/krust.git
cd krust
cargo build --release --bin krust-mcp
```

The binary lands at `target/release/krust-mcp`.

### Configure with Claude Code

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "krust": {
      "command": "/path/to/krust-mcp",
      "env": {
        "CHROME_PATH": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
      }
    }
  }
}
```

Then restart Claude Code and run `/mcp` to verify krust is connected.

### Configure with Codex

Add to your MCP config (check Codex docs for location):

```json
{
  "mcpServers": {
    "krust": {
      "command": "/path/to/krust-mcp"
    }
  }
}
```

### Configure with Cursor

In Cursor Settings → MCP, add a new server:
- **Name**: krust
- **Command**: `/path/to/krust-mcp`

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHROME_PATH` | auto-detect | Path to Chrome/Chromium binary |
| `KRUST_HEADLESS` | `true` | Set to `false` to show the browser window |
| `KRUST_LOG` | `info` | Log level: `error`, `warn`, `info`, `debug`, `trace` |
| `KRUST_WINDOW_WIDTH` | `1280` | Browser window width |
| `KRUST_WINDOW_HEIGHT` | `720` | Browser window height |

### Verify Installation

Test that Chrome is discoverable before configuring your agent:

```bash
./target/release/krust-mcp --check
```

If Chrome isn't found, the command prints an error with instructions for setting `CHROME_PATH`.

### Troubleshooting

**"krust not showing in /mcp"**
- Make sure the path to the binary is absolute (not relative)
- Restart Claude Code after editing `mcp.json`
- Check stderr: `./target/release/krust-mcp 2>&1 | head`

**"Browser launch failed"**
- Set `CHROME_PATH` to your Chrome binary explicitly
- macOS: `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
- Linux: `/usr/bin/google-chrome` or `/usr/bin/chromium-browser`
- Verify Chrome works: `"$CHROME_PATH" --headless --dump-dom https://example.com`

**"Connection refused" / timeout errors**
- Chrome might already be running with `--remote-debugging-port`. Close it first.
- Try `KRUST_HEADLESS=false` to see what the browser is doing

---

## What is this?

AI agents fail ~40% of the time on real-world tasks — not because the models are dumb, but because there's no infrastructure between "AI decides to do something" and "thing actually happens." Every agent app builds this control layer from scratch and gets it wrong differently.

Krust is the open-source protocol layer that fixes this. It provides:

- **A state machine** that governs agent task execution with deterministic transitions
- **Artifact contracts** that verify actions actually worked (not just that the tool returned "success")
- **A policy engine** that gates sensitive actions with allow/deny/confirm decisions
- **Checkpoint/resume** for durable execution that survives crashes and restarts
- **A web interaction layer** with pluggable backends (CDP, accessibility APIs, native browsers)

## Available Tools

When connected via MCP, your agent gets these tools:

| Tool | Description |
|------|-------------|
| `web_navigate` | Navigate to a URL |
| `web_click` | Click an element by CSS selector |
| `web_type` | Type text into an input element |
| `web_extract` | Extract text from the page or a specific element |
| `web_screenshot` | Take a screenshot of the current page |
| `web_wait` | Wait for an element to appear or a duration |

Every tool call passes through the Krust state machine: policy check → execute → verify evidence → complete or retry.

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

## Development

```bash
# Run all tests
cargo test --workspace

# Check for issues
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all
```

## Status

🚧 **Early development.** The protocol core, MCP server, and CDP backend are functional. Under active development.

## License

MIT
