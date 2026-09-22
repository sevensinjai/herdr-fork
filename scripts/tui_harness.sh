#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: scripts/tui_harness.sh <command> [args...]

Runs a debug herdr client in a detached tmux session with isolated config,
state, and sockets, so an agent can drive the real TUI and read what it drew.
Nothing touches the caller's herdr session or the user's config.

Commands:
  up [--size COLSxROWS] [--config FILE] [--no-build]
                        Build, then start herdr. FILE is appended to a base
                        config that disables onboarding. Default size 160x45.
  down                  Stop the herdr server, kill tmux, delete the run dir.
  cli ARGS...           Run a herdr CLI command against the harness server.
  keys KEYS...          tmux send-keys (e.g. `keys Enter`, `keys -l 'echo hi'`).
  prefix KEY            Send the herdr prefix (ctrl+b), then KEY (tmux key name,
                        e.g. `S` for prefix+shift+s).
  click COL ROW [right|middle]
                        Mouse press+release at 1-based screen cell COL,ROW.
  drag COL ROW COL2 ROW2
                        Left-button drag between two cells.
  text                  Print the current screen as plain text.
  find REGEX            Print `row col text` for every line matching REGEX.
  wait-text REGEX [SECS]
                        Poll the screen until REGEX appears (default 10s).
  wait-gone REGEX [SECS]
                        Poll the screen until REGEX disappears (default 10s).
  shot NAME [CAPTION]   Save NAME.txt, NAME.ansi, NAME.html under the shot dir,
                        and append it to the run's manifest.
  report TITLE [OUT]    Combine every shot into one HTML page, in order, with
                        captions (default OUT: <run>/report.html).
  path                  Print the run dir (shots live in <run>/shots).

Environment:
  HTV_NAME   Harness instance name, lets several runs coexist (default: main).
  HTV_BIN    herdr binary to run (default: <repo>/target/debug/herdr).
USAGE
}

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/.." && pwd)"
name="${HTV_NAME:-main}"
# Keep the base short: macOS caps unix socket paths at 104 bytes.
base="/var/tmp/htv-$name"
tmux_sock="htv-$name"
bin="${HTV_BIN:-$repo_dir/target/debug/herdr}"

tm() { tmux -L "$tmux_sock" "$@"; }

# The debug build reads `herdr-dev` dirs; clear every inherited herdr variable
# so neither the client nor the CLI can address the caller's session.
env_args() {
  printf '%s\n' \
    -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH \
    -u HERDR_SESSION -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID -u HERDR_PANE_ID \
    -u TMUX -u TMUX_PANE \
    HERDR_DISABLE_SOUND=1 \
    "XDG_CONFIG_HOME=$base/config" \
    "XDG_STATE_HOME=$base/state" \
    "XDG_RUNTIME_DIR=$base/run" \
    "HERDR_SOCKET_PATH=$base/run/api.sock" \
    SHELL=/bin/sh \
    "ENV=$base/shrc" \
    TERM=xterm-256color
}

herdr_env() {
  local args=()
  while IFS= read -r line; do args+=("$line"); done < <(env_args)
  env "${args[@]}" "$@"
}

require_up() {
  if ! tm has-session -t htv 2>/dev/null; then
    echo "harness '$name' is not running; run: scripts/tui_harness.sh up" >&2
    exit 1
  fi
}

screen_text() { tm capture-pane -p -t htv; }

cmd_up() {
  local size="160x45" extra_config="" build=1
  while (($#)); do
    case "$1" in
      --size) size="$2"; shift 2 ;;
      --config) extra_config="$2"; shift 2 ;;
      --no-build) build=0; shift ;;
      *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
  done
  if tm has-session -t htv 2>/dev/null; then
    echo "harness '$name' already running; run: scripts/tui_harness.sh down" >&2
    exit 1
  fi
  if ((build)); then
    (cd "$repo_dir" && cargo build --quiet)
  fi
  [[ -x "$bin" ]] || { echo "missing herdr binary: $bin" >&2; exit 1; }

  rm -rf "$base"
  mkdir -p "$base/config/herdr-dev" "$base/state" "$base/run" "$base/shots" "$base/home"
  chmod 700 "$base/run"
  # A fixed prompt keeps the host and user name out of captured screens.
  echo "PS1='\$ '" >"$base/shrc"
  {
    echo "onboarding = false"
    if [[ -n "$extra_config" ]]; then
      echo
      cat "$extra_config"
    fi
  } >"$base/config/herdr-dev/config.toml"
  herdr_env "$bin" config check >/dev/null

  local cols="${size%x*}" rows="${size#*x}" args=()
  while IFS= read -r line; do args+=("$line"); done < <(env_args)
  # Keep the tmux pane open after herdr exits so a crash stays readable.
  {
    echo '#!/usr/bin/env bash'
    printf 'env'; printf ' %q' "${args[@]}" "$bin"; echo
    echo "echo '[herdr exited]'; exec sleep 86400"
  } >"$base/launch.sh"
  chmod +x "$base/launch.sh"
  tm -f /dev/null new-session -d -s htv -x "$cols" -y "$rows" -c "$base/home" "$base/launch.sh"
  tm set-option -t htv status off >/dev/null

  for _ in $(seq 1 100); do
    if [[ -S "$base/run/api.sock" ]] && herdr_env "$bin" pane list >/dev/null 2>&1; then
      echo "$base"
      return 0
    fi
    sleep 0.1
  done
  echo "herdr did not become ready; screen:" >&2
  screen_text >&2
  exit 1
}

cmd_down() {
  if [[ -S "$base/run/api.sock" ]]; then
    herdr_env "$bin" server stop >/dev/null 2>&1 || true
  fi
  tm kill-server 2>/dev/null || true
  rm -rf "$base"
}

sgr_button() {
  case "${1:-left}" in
    left) echo 0 ;;
    middle) echo 1 ;;
    right) echo 2 ;;
    *) echo "unknown button: $1" >&2; exit 2 ;;
  esac
}

cmd_click() {
  local col="$1" row="$2" btn
  btn="$(sgr_button "${3:-left}")"
  tm send-keys -t htv -l $'\e'"[<${btn};${col};${row}M"
  tm send-keys -t htv -l $'\e'"[<${btn};${col};${row}m"
}

cmd_drag() {
  local c1="$1" r1="$2" c2="$3" r2="$4"
  tm send-keys -t htv -l $'\e'"[<0;${c1};${r1}M"
  tm send-keys -t htv -l $'\e'"[<32;${c2};${r2}M"
  tm send-keys -t htv -l $'\e'"[<0;${c2};${r2}m"
}

cmd_wait() {
  local mode="$1" regex="$2" secs="${3:-10}"
  local deadline=$((SECONDS + secs))
  while ((SECONDS < deadline)); do
    if screen_text | grep -Eq -- "$regex"; then
      [[ "$mode" == present ]] && return 0
    else
      [[ "$mode" == gone ]] && return 0
    fi
    sleep 0.1
  done
  echo "timed out after ${secs}s waiting for /$regex/ to be $mode; screen:" >&2
  screen_text >&2
  exit 1
}

cmd_find() {
  # ENVIRON keeps backslashes intact; `awk -v` would unescape them.
  screen_text | RE="$1" awk '{ i = match($0, ENVIRON["RE"]); if (i) printf "%d %d %s\n", NR, i, $0 }'
}

cmd_shot() {
  local shot="$1" caption="${2:-}"
  local dir="$base/shots"
  screen_text >"$dir/$shot.txt"
  tm capture-pane -p -e -t htv >"$dir/$shot.ansi"
  python3 "$script_dir/ansi_to_html.py" --fragment <"$dir/$shot.ansi" >"$dir/$shot.html"
  printf '%s\t%s\n' "$shot" "$caption" >>"$dir/manifest.tsv"
  echo "$dir/$shot"
}

cmd_report() {
  local title="$1" out="${2:-$base/report.html}"
  python3 - "$base/shots" "$title" >"$out" <<'PY'
import html, pathlib, sys
shots, title = pathlib.Path(sys.argv[1]), sys.argv[2]
rows = [line.split("\t", 1) for line in (shots / "manifest.tsv").read_text().splitlines()]
print(f"<!doctype html><meta charset=utf-8><title>{html.escape(title)}</title>")
print("<body style='font-family:system-ui;background:#fafafa;color:#222;max-width:1400px;margin:auto;padding:16px'>")
print(f"<h1>{html.escape(title)}</h1>")
for i, (name, caption) in enumerate(rows, 1):
    print(f"<h2>{i}. {html.escape(caption or name)}</h2><div style='overflow-x:auto'>")
    print((shots / f"{name}.html").read_text() + "</div>")
PY
  echo "$out"
}

(($#)) || { usage; exit 2; }
command="$1"
shift
case "$command" in
  up) cmd_up "$@" ;;
  down) cmd_down ;;
  cli) require_up; herdr_env "$bin" "$@" ;;
  keys) require_up; tm send-keys -t htv "$@" ;;
  prefix) require_up; tm send-keys -t htv C-b; sleep 0.05; tm send-keys -t htv "$1" ;;
  click) require_up; cmd_click "$@" ;;
  drag) require_up; cmd_drag "$@" ;;
  text) require_up; screen_text ;;
  find) require_up; cmd_find "$1" ;;
  wait-text) require_up; cmd_wait present "$@" ;;
  wait-gone) require_up; cmd_wait gone "$@" ;;
  shot) require_up; cmd_shot "$@" ;;
  report) cmd_report "$@" ;;
  path) echo "$base" ;;
  -h|--help) usage ;;
  *) echo "unknown command: $command" >&2; usage >&2; exit 2 ;;
esac
