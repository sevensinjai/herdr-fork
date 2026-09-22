#!/usr/bin/env bash
# Sidecar opens from its keybind, shows both terminals, switches tabs, closes.
export HTV_NAME="${HTV_NAME:-sidecar}"
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

config="$(mktemp)"
cat >"$config" <<'TOML'
[sidecar]
notes_command = "echo notes-ready; exec /bin/sh"
chat_command = "echo chat-ready; exec /bin/sh"
TOML
h up ${HTV_NO_BUILD:+--no-build} --config "$config" >/dev/null
rm -f "$config"

expect_absent 'Notes +Chat'
h prefix S
expect 'Notes +Chat +✕'
expect 'notes-ready'
h shot 01-notes "prefix+shift+s opens the Sidecar on Notes" >/dev/null

h prefix O
expect 'chat-ready'
h shot 02-chat "prefix+shift+o switches to Chat" >/dev/null

# Close with the header's ✕.
read -r close_row close_col _ < <(h find '✕' | tail -1)
h click "$close_col" "$close_row"
h wait-gone 'Notes +Chat' 5 >/dev/null
h shot 03-closed "clicking ✕ closes the Sidecar" >/dev/null

pass
