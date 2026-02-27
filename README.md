# Ember

**Verified execution protocols for AI agents.**

A Rust toolkit that makes agent actions deterministic, observable, and recoverable. Plug it into any MCP-compatible agent (Claude Code, Codex, Cursor) and get state-machine-backed browser automation with evidence verification out of the box.

---

## Quick Start

### Prerequisites

- **Rust** (1.70+): [rustup.rs](https://rustup.rs)
- **Chrome or Chromium** installed (the MCP server controls it via CDP)

### Build

```bash
git clone https://github.com/abbudjoe/ember.git
cd ember
cargo build --release --bin ember-mcp
```

The binary lands at `target/release/ember-mcp`.

### Configure with Claude Code

Add to `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "ember": {
      "command": "/path/to/ember-mcp",
      "env": {
        "CHROME_PATH": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
      }
    }
  }
}
```

Then restart Claude Code and run `/mcp` to verify ember is connected.

### Configure with Codex

Add to your MCP config (check Codex docs for location):

```json
{
  "mcpServers": {
    "ember": {
      "command": "/path/to/ember-mcp"
    }
  }
}
```

### Configure with Cursor

In Cursor Settings → MCP, add a new server:
- **Name**: ember
- **Command**: `/path/to/ember-mcp`

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHROME_PATH` | auto-detect | Path to Chrome/Chromium binary |
| `TINYFISH_API_KEY` | unset | TinyFish API key for `web_search` primary provider |
| `BRAVE_API_KEY` | unset | Brave Search API key for `web_search` fallback provider |
| `KRUST_HEADLESS` | `true` | Set to `false` to show the browser window |
| `KRUST_LOG` | `info` | Log level: `error`, `warn`, `info`, `debug`, `trace` |
| `KRUST_WINDOW_WIDTH` | `1280` | Browser window width |
| `KRUST_WINDOW_HEIGHT` | `720` | Browser window height |

### BYO API Keys (Recommended)

`web_search` supports bring-your-own keys:
- `TINYFISH_API_KEY` (primary provider)
- `BRAVE_API_KEY` (fallback provider)

If both keys are present, Ember tries **TinyFish first**, then falls back to **Brave**.

**Security rule of thumb:** keep keys in a local secret store and inject them at launch. Avoid plaintext keys in repo files or shared configs.

#### Step 1 — Choose one setup path

Pick **one** of these paths:

- **Path A (recommended):** One-shot setup script
- **Path B:** Manual setup commands

##### Path A — One-shot setup script (recommended)

From repo root:

```bash
# macOS Keychain
./scripts/setup/macos-keychain.sh

# 1Password CLI (macOS/Linux)
./scripts/setup/onepassword.sh

# Linux secret stores (pass or secret-tool)
./scripts/setup/linux-secrets.sh
```

What these scripts do:
1. Prompt for required inputs/secrets.
   - For the first prompt (`Path to ember-mcp binary`), press **Enter** to accept the default shown in brackets.
2. Create a launcher script (`ember-mcp-launch`) that fetches keys from your secret store.
3. Tell you the launcher path to use in MCP config.

Default launcher paths:
- macOS / 1Password: `~/bin/ember-mcp-launch`
- Linux: `~/.local/bin/ember-mcp-launch`

##### Path B — Manual setup commands

###### B1) macOS + Keychain

1. Store keys in Keychain (**choose one sub-option**):

   - **Interactive (preferred; avoids shell history):**

   ```bash
   security add-generic-password -U -a "$USER" -s ember_tinyfish_api_key -w
   security add-generic-password -U -a "$USER" -s ember_brave_api_key -w
   ```

   - **Inline value (faster, less secure):**

   ```bash
   security add-generic-password -U -a "$USER" -s ember_tinyfish_api_key -w 'PASTE_TINYFISH_KEY_HERE'
   security add-generic-password -U -a "$USER" -s ember_brave_api_key -w 'PASTE_BRAVE_KEY_HERE'
   ```

2. Create a launcher script at `~/bin/ember-mcp-launch`:

```bash
#!/usr/bin/env bash
set -euo pipefail

export TINYFISH_API_KEY="$(security find-generic-password -a "$USER" -s ember_tinyfish_api_key -w)"
export BRAVE_API_KEY="$(security find-generic-password -a "$USER" -s ember_brave_api_key -w)"

exec /path/to/ember-mcp "$@"
```

3. Make it executable:

```bash
chmod 700 ~/bin/ember-mcp-launch
```

###### B2) 1Password CLI (macOS/Linux)

1. Install/sign in to `op`.
2. Create launcher script:

```bash
#!/usr/bin/env bash
set -euo pipefail

export TINYFISH_API_KEY="$(op read 'op://Private/Ember API Keys/tinyfish')"
export BRAVE_API_KEY="$(op read 'op://Private/Ember API Keys/brave')"

exec /path/to/ember-mcp "$@"
```

###### B3) Linux (`pass` or `secret-tool`)

- **pass**
  - Store: `pass insert ember/tinyfish_api_key` and `pass insert ember/brave_api_key`
  - Read in launcher: `$(pass show ember/tinyfish_api_key | head -n1)`

- **secret-tool (libsecret)**
  - Store: `secret-tool store --label='Ember TinyFish' service ember account tinyfish_api_key`
  - Read in launcher: `secret-tool lookup service ember account tinyfish_api_key`

#### Step 2 — Point MCP to the launcher

Use the launcher script as the MCP command (instead of calling `ember-mcp` directly).

Example Claude MCP entry:

```json
{
  "mcpServers": {
    "ember": {
      "command": "/absolute/path/to/ember-mcp-launch",
      "env": {
        "CHROME_PATH": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
      }
    }
  }
}
```

#### Step 3 — Smoke test `web_search`

1. Restart your MCP client (Claude/Codex/Cursor).
2. Confirm server appears in MCP.
3. Run a `web_search` prompt; if TinyFish fails, Ember should fallback to Brave.

### Verify Installation

Test that Chrome is discoverable before configuring your agent:

```bash
./target/release/ember-mcp --check
```

If Chrome isn't found, the command prints an error with instructions for setting `CHROME_PATH`.

### Troubleshooting

**"ember not showing in /mcp"**
- Make sure the path to the binary is absolute (not relative)
- Restart Claude Code after editing `mcp.json`
- Check stderr: `./target/release/ember-mcp 2>&1 | head`

**"Browser launch failed"**
- Set `CHROME_PATH` to your Chrome binary explicitly
- macOS: `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
- Linux: `/usr/bin/google-chrome` or `/usr/bin/chromium-browser`
- Verify Chrome works: `"$CHROME_PATH" --headless --dump-dom https://example.com`

**"Connection refused" / timeout errors**
- Chrome might already be running with `--remote-debugging-port`. Close it first.
- Try `KRUST_HEADLESS=false` to see what the browser is doing

**"web_search unavailable"**
- Set `TINYFISH_API_KEY` and/or `BRAVE_API_KEY` in your MCP server environment.
- If using placeholder variables in config, confirm your MCP client expands them; otherwise use a launcher script.

---

## What is this?

AI agents fail ~40% of the time on real-world tasks — not because the models are dumb, but because there's no infrastructure between "AI decides to do something" and "thing actually happens." Every agent app builds this control layer from scratch and gets it wrong differently.

Ember is the open-source protocol layer that fixes this. It provides:

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
| `web_press_key` | Press a keyboard key (Enter/Tab/Escape/etc.) |
| `web_extract` | Extract text from the page or a specific element |
| `web_screenshot` | Take a screenshot and return saved file path |
| `web_wait` | Wait for an element to appear or a duration |
| `web_search` | Search via TinyFish (primary) with Brave fallback |

Every tool call passes through the Ember state machine: policy check → execute → verify evidence → complete or retry.

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
| `ember-protocol-core` | State machine, typed intents, artifact contracts, policy engine |
| `ember-agent-web` | Web interaction abstractions with pluggable backends |
| `ember-agent-tools` | Tool framework and MCP compatibility layer |
| `ember-agent-eval` | Evaluation harness and reliability metrics |
| `ember-mcp` | MCP server binary — point any agent at this |

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
