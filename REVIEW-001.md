# Krust Code Review — Day-One Architecture Audit (Opus)

## Blocking Issues

### 1. `cdp.rs` — Hand-rolled base64 encoder is broken
**File**: `crates/agent-web/src/cdp.rs`
Hand-rolled base64 encoder processes entire input as one `write()` call. If `Write` infrastructure splits buffer across calls, output corrupts at chunk boundaries. `.ok()` swallows errors silently.
**Fix**: Use the `base64` crate. Remove hand-rolled encoder entirely.

### 2. `cdp.rs` — Back/Forward use wrong CDP command
**File**: `crates/agent-web/src/cdp.rs`
`NavigateToHistoryEntryParams::new(-1)` / `new(1)` is wrong — CDP takes absolute `entryId`, not relative offset. `.ok()` swallows errors.
**Fix**: Use JS `history.back()`/`history.forward()` via `page.evaluate()`.

### 3. `cdp.rs` — `WaitCondition::Selector` doesn't actually wait
**File**: `crates/agent-web/src/cdp.rs`
`page.find_element(&sel)` does a single DOM query, doesn't poll. Returns Err immediately if element missing, mapped to misleading Timeout error.
**Fix**: Poll with backoff and a real timeout.

### 4. State machine — `Verifying → Executing` via `PlanReady` resets step to 1
**File**: `crates/protocol-core/src/state.rs`
Multi-step tasks lose progress. Step from `Verifying` is discarded.
**Fix**: `PlanReady` should carry step number, or transition should increment step.

### 5. State machine — `Retrying` doesn't increment `attempt`
**File**: `crates/protocol-core/src/state.rs`
`ToolCompleted { success: false }` always creates `Retrying { attempt: 1 }`. Retry limit is dead code — retries loop forever.
**Fix**: Add attempt count to `Executing` state or to the transition event so it's preserved through the retry cycle.

### 6. MCP server bypasses protocol-core entirely
**File**: `crates/krust-mcp/src/main.rs`
Server calls backend directly. No state machine, no policy checks, no evidence verification, no artifact contracts. The core value prop is not wired in.
**Fix**: Wire MCP tool handlers through protocol-core: Planning → policy check → Executing → Verifying → Completed/Retry.
