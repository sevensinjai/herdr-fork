#!/usr/bin/env bash
# Agents panel groups agents under their `$stage` token, and follows changes.
export HTV_NAME="${HTV_NAME:-group-by}"
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

config="$(mktemp)"
cat >"$config" <<'TOML'
[ui.sidebar.agents]
group_by = "$stage"
TOML
h up ${HTV_NO_BUILD:+--no-build} --config "$config" >/dev/null
rm -f "$config"

p1=w1:p1
p2="$(h cli pane split "$p1" --direction right --no-focus | jq -r .result.pane.pane_id)"
p3="$(h cli pane split "$p2" --direction down --no-focus | jq -r .result.pane.pane_id)"
p4="$(h cli pane split "$p3" --direction down --no-focus | jq -r .result.pane.pane_id)"

agent() { h cli pane report-agent "$1" --source htv --agent "$2" --state "$3" --seq 1 >/dev/null; }
stage() { h cli pane report-metadata "$1" --source htv --token "stage=$2" --seq "$3" >/dev/null; }

agent "$p1" claude working
agent "$p2" codex blocked
agent "$p3" pi idle
agent "$p4" opencode working
stage "$p1" review 1
stage "$p2" review 1
stage "$p3" "waiting QA" 1

expect 'review \(2\)'
expect 'waiting QA \(1\)'
expect 'ungrouped \(1\)'
# Headers keep first-seen order and the unstaged bucket sorts last.
expect_above 'review \(2\)' 'waiting QA \(1\)'
expect_above 'waiting QA \(1\)' 'ungrouped \(1\)'
h shot 01-grouped "Agents grouped by \$stage: review (2), waiting QA (1), unstaged agent last" >/dev/null

stage "$p3" review 2
expect 'review \(3\)'
h wait-gone 'waiting QA' 5 >/dev/null
h shot 02-restaged "pi moved to review; the empty waiting QA group disappears" >/dev/null

pass
