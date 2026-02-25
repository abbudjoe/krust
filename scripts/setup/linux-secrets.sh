#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This setup script is for Linux secret stores." >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

DEFAULT_BINARY="$REPO_ROOT/target/release/krust-mcp"
BINARY_PATH="${KRUST_MCP_BINARY:-$DEFAULT_BINARY}"
LAUNCHER_PATH="${KRUST_LAUNCHER_PATH:-$HOME/.local/bin/krust-mcp-launch}"

echo "== Krust Linux secret setup =="
read -r -p "Path to krust-mcp binary [$BINARY_PATH]: " input_binary
if [[ -n "${input_binary}" ]]; then
  BINARY_PATH="$input_binary"
fi

echo
echo "Choose backend:"
echo "  1) pass"
echo "  2) secret-tool (libsecret)"
read -r -p "Selection [1/2]: " sel

mkdir -p "$(dirname "$LAUNCHER_PATH")"

if [[ "$sel" == "1" ]]; then
  if ! command -v pass >/dev/null 2>&1; then
    echo "'pass' not found. Install it first." >&2
    exit 1
  fi

  tiny_entry="${KRUST_TINYFISH_PASS_ENTRY:-krust/tinyfish_api_key}"
  brave_entry="${KRUST_BRAVE_PASS_ENTRY:-krust/brave_api_key}"

  echo
  read -r -p "Store TinyFish key now in pass entry '$tiny_entry'? [y/N]: " store_tiny
  if [[ "${store_tiny,,}" == "y" ]]; then
    pass insert "$tiny_entry"
  fi

  read -r -p "Store Brave key now in pass entry '$brave_entry'? [y/N]: " store_brave
  if [[ "${store_brave,,}" == "y" ]]; then
    pass insert "$brave_entry"
  fi

  cat > "$LAUNCHER_PATH" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if command -v pass >/dev/null 2>&1; then
  if TINY_VALUE=\$(pass show '$tiny_entry' 2>/dev/null | head -n1); then
    export TINYFISH_API_KEY="\$TINY_VALUE"
  fi
  if BRAVE_VALUE=\$(pass show '$brave_entry' 2>/dev/null | head -n1); then
    export BRAVE_API_KEY="\$BRAVE_VALUE"
  fi
fi

exec "$BINARY_PATH" "\$@"
EOF

elif [[ "$sel" == "2" ]]; then
  if ! command -v secret-tool >/dev/null 2>&1; then
    echo "'secret-tool' not found. Install libsecret-tools first." >&2
    exit 1
  fi

  tiny_account="${KRUST_TINYFISH_SECRET_ACCOUNT:-tinyfish_api_key}"
  brave_account="${KRUST_BRAVE_SECRET_ACCOUNT:-brave_api_key}"

  echo
  read -r -p "Store TinyFish key now with secret-tool account '$tiny_account'? [y/N]: " store_tiny
  if [[ "${store_tiny,,}" == "y" ]]; then
    secret-tool store --label='Krust TinyFish API Key' service krust account "$tiny_account"
  fi

  read -r -p "Store Brave key now with secret-tool account '$brave_account'? [y/N]: " store_brave
  if [[ "${store_brave,,}" == "y" ]]; then
    secret-tool store --label='Krust Brave API Key' service krust account "$brave_account"
  fi

  cat > "$LAUNCHER_PATH" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if command -v secret-tool >/dev/null 2>&1; then
  if TINY_VALUE=\$(secret-tool lookup service krust account '$tiny_account' 2>/dev/null); then
    export TINYFISH_API_KEY="\$TINY_VALUE"
  fi
  if BRAVE_VALUE=\$(secret-tool lookup service krust account '$brave_account' 2>/dev/null); then
    export BRAVE_API_KEY="\$BRAVE_VALUE"
  fi
fi

exec "$BINARY_PATH" "\$@"
EOF

else
  echo "Invalid selection. Use 1 or 2." >&2
  exit 1
fi

chmod 700 "$LAUNCHER_PATH"

echo
echo "Done. Launcher created at: $LAUNCHER_PATH"
echo "Set your MCP command to: $LAUNCHER_PATH"
