#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This setup script is for macOS (Keychain) only." >&2
  exit 1
fi

if ! command -v security >/dev/null 2>&1; then
  echo "Missing 'security' CLI. It should be available on macOS." >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_BINARY="$REPO_ROOT/target/release/krust-mcp"
BINARY_PATH="${KRUST_MCP_BINARY:-$DEFAULT_BINARY}"
LAUNCHER_PATH="${KRUST_LAUNCHER_PATH:-$HOME/bin/krust-mcp-launch}"
TINY_SERVICE="${KRUST_TINYFISH_SERVICE:-krust_tinyfish_api_key}"
BRAVE_SERVICE="${KRUST_BRAVE_SERVICE:-krust_brave_api_key}"

echo "== Krust macOS Keychain setup =="
echo
read -r -p "Path to krust-mcp binary [default: $BINARY_PATH] (press Enter to accept): " input_binary
if [[ -n "${input_binary}" ]]; then
  BINARY_PATH="$input_binary"
fi

echo "Using krust-mcp binary: $BINARY_PATH"

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Warning: binary not executable at: $BINARY_PATH" >&2
  echo "Build it first with: cargo build --release --bin krust-mcp" >&2
fi

echo
echo "Enter API keys (leave blank to skip either provider)."
read -r -s -p "TinyFish API key: " tiny_key
echo
read -r -s -p "Brave API key: " brave_key
echo

if [[ -n "$tiny_key" ]]; then
  security add-generic-password -U -a "$USER" -s "$TINY_SERVICE" -w "$tiny_key"
  echo "Stored TinyFish key in Keychain service: $TINY_SERVICE"
else
  echo "Skipped TinyFish key storage."
fi

if [[ -n "$brave_key" ]]; then
  security add-generic-password -U -a "$USER" -s "$BRAVE_SERVICE" -w "$brave_key"
  echo "Stored Brave key in Keychain service: $BRAVE_SERVICE"
else
  echo "Skipped Brave key storage."
fi

unset tiny_key brave_key

mkdir -p "$(dirname "$LAUNCHER_PATH")"
cat > "$LAUNCHER_PATH" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if TINY_VALUE=\$(security find-generic-password -a "\$USER" -s "$TINY_SERVICE" -w 2>/dev/null); then
  export TINYFISH_API_KEY="\$TINY_VALUE"
fi

if BRAVE_VALUE=\$(security find-generic-password -a "\$USER" -s "$BRAVE_SERVICE" -w 2>/dev/null); then
  export BRAVE_API_KEY="\$BRAVE_VALUE"
fi

exec "$BINARY_PATH" "\$@"
EOF
chmod 700 "$LAUNCHER_PATH"

echo
echo "Done. Launcher created at: $LAUNCHER_PATH"
echo "Set your MCP command to: $LAUNCHER_PATH"
