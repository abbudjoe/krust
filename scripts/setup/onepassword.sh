#!/usr/bin/env bash
set -euo pipefail

if ! command -v op >/dev/null 2>&1; then
  echo "1Password CLI (op) not found. Install it first: https://developer.1password.com/docs/cli/get-started/" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_BINARY="$REPO_ROOT/target/release/krust-mcp"
BINARY_PATH="${KRUST_MCP_BINARY:-$DEFAULT_BINARY}"
LAUNCHER_PATH="${KRUST_LAUNCHER_PATH:-$HOME/bin/krust-mcp-launch}"
DEFAULT_TINY_REF="op://Private/Krust API Keys/tinyfish"
DEFAULT_BRAVE_REF="op://Private/Krust API Keys/brave"

echo "== Krust 1Password setup =="
read -r -p "Path to krust-mcp binary [default: $BINARY_PATH] (press Enter to accept): " input_binary
if [[ -n "${input_binary}" ]]; then
  BINARY_PATH="$input_binary"
fi

echo "Using krust-mcp binary: $BINARY_PATH"

read -r -p "TinyFish secret reference [$DEFAULT_TINY_REF]: " tiny_ref
read -r -p "Brave secret reference [$DEFAULT_BRAVE_REF]: " brave_ref
tiny_ref="${tiny_ref:-$DEFAULT_TINY_REF}"
brave_ref="${brave_ref:-$DEFAULT_BRAVE_REF}"

echo
echo "Checking 1Password access..."
if ! op read "$tiny_ref" >/dev/null 2>&1; then
  echo "Warning: could not read TinyFish reference now: $tiny_ref" >&2
  echo "Make sure 'op signin' is active and the reference exists." >&2
fi
if ! op read "$brave_ref" >/dev/null 2>&1; then
  echo "Warning: could not read Brave reference now: $brave_ref" >&2
  echo "Make sure 'op signin' is active and the reference exists." >&2
fi

mkdir -p "$(dirname "$LAUNCHER_PATH")"
cat > "$LAUNCHER_PATH" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if command -v op >/dev/null 2>&1; then
  if TINY_VALUE=\$(op read '$tiny_ref' 2>/dev/null); then
    export TINYFISH_API_KEY="\$TINY_VALUE"
  fi
  if BRAVE_VALUE=\$(op read '$brave_ref' 2>/dev/null); then
    export BRAVE_API_KEY="\$BRAVE_VALUE"
  fi
fi

exec "$BINARY_PATH" "\$@"
EOF
chmod 700 "$LAUNCHER_PATH"

echo
echo "Done. Launcher created at: $LAUNCHER_PATH"
echo "Set your MCP command to: $LAUNCHER_PATH"
