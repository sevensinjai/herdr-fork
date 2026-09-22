#!/usr/bin/env bash
# The default Notes editor takes typing immediately and saves to the notes file.
export HTV_NAME="${HTV_NAME:-sidecar-notes}"
source "$(dirname -- "${BASH_SOURCE[0]}")/lib.sh"

if ! command -v nano >/dev/null; then
  echo "SKIP: $HTV_NAME (nano not installed; Notes falls back to \$EDITOR or vi)"
  exit 0
fi

h up ${HTV_NO_BUILD:+--no-build} >/dev/null
notes="$(h path)/config/herdr-dev/sidecar/default.md"

h prefix S
expect 'Notes +Chat'
expect '\^X Exit'
h keys -l 'typed without pressing i'
expect 'typed without pressing i'
h shot 01-typing "Notes opens in nano: typing goes straight into the file" >/dev/null

h keys C-o
expect 'File Name to write'
h keys Enter
expect 'Wrote 1 line'
grep -qx 'typed without pressing i' "$notes"
h shot 02-saved "ctrl+o, Enter saves to the session's notes file" >/dev/null

pass
