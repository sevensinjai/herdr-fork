#!/usr/bin/env bash
# A pane that takes right-clicks gets a clickable ✕ on its top border, and the
# pane menu's "Open new chat in right pane" splits and starts the chat command.
export HTV_NAME="${HTV_NAME:-pane-close-chat}"
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

# A stand-in `claude` so no real agent starts; /bin/sh keeps this PATH.
stub="$(mktemp -d)"
printf '#!/bin/sh\necho fake claude ready\nexec /bin/sh\n' >"$stub/claude"
chmod +x "$stub/claude"
SHELL=/bin/sh PATH="$stub:$PATH" h up ${HTV_NO_BUILD:+--no-build} >/dev/null

panes() { h cli pane list | jq '.result.panes | length'; }

p2="$(h cli pane split w1:p1 --direction right --right-click pane --no-focus | jq -r .result.pane.pane_id)"
expect '✕'
h shot 01-close-button "The right pane takes right-clicks, so its top border shows a ✕" >/dev/null
read -r row col _ < <(h find '✕' | head -1)
h click "$col" "$row"
for _ in $(seq 1 50); do [[ "$(panes)" == 1 ]] && break; sleep 0.1; done
[[ "$(panes)" == 1 ]] || { echo "✕ did not close $p2" >&2; exit 1; }
expect_absent '✕'
h shot 02-closed "Clicking the ✕ closed the pane" >/dev/null

h click 80 12 right
expect 'Open new chat in right pane'
h shot 03-menu "The pane menu offers Open new chat in right pane" >/dev/null
read -r row col _ < <(h find 'Open new chat in right pane' | head -1)
h click "$((col + 2))" "$row"
expect 'fake claude ready' 10
[[ "$(panes)" == 2 ]] || { echo "new chat did not split" >&2; exit 1; }
h shot 04-chat "A new right pane opened and ran the chat command" >/dev/null

pass
