# Shared setup for TUI harness scenarios. Source from a scenario script after
# setting HTV_NAME. Tears the harness down on exit unless HTV_KEEP=1.
set -euo pipefail

scenario_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
htv="$scenario_dir/../tui_harness.sh"

h() { "$htv" "$@"; }

teardown() {
  local status=$?
  if ((status != 0)); then
    echo "FAIL: $HTV_NAME (screen above; shots in $(h path)/shots)" >&2
  fi
  if [[ "${HTV_KEEP:-0}" != 1 ]]; then
    h down
  fi
  exit "$status"
}
trap teardown EXIT

# Fail unless REGEX matches a screen line within 5s.
expect() { h wait-text "$1" "${2:-5}" >/dev/null; }

# Fail if REGEX matches any screen line right now.
expect_absent() {
  if h text | grep -Eq -- "$1"; then
    echo "unexpected /$1/ on screen:" >&2
    h text >&2
    exit 1
  fi
}

# Print the screen row (1-based) of the first line matching REGEX.
row_of() { h find "$1" | head -1 | cut -d' ' -f1; }

# Fail unless the first line matching REGEX_A is above the first matching REGEX_B.
expect_above() {
  local a b
  a="$(row_of "$1")"
  b="$(row_of "$2")"
  if [[ -z "$a" || -z "$b" ]] || ((a >= b)); then
    echo "expected /$1/ (row ${a:-none}) above /$2/ (row ${b:-none}):" >&2
    h text >&2
    exit 1
  fi
}

pass() { echo "PASS: $HTV_NAME"; }
