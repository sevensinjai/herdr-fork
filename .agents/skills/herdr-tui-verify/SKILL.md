---
name: herdr-tui-verify
description: Verify a Herdr TUI change by driving the real debug client in tmux and reading what it drew. Use after changing anything the client renders or handles (sidebar, panels, modals, menus, keybinds, mouse), before claiming a UI change works, and when the user wants screenshots or a walkthrough of Herdr's UI.
---

# Herdr TUI verification

Unit tests prove logic. `pane read` shows what runs inside a pane, not Herdr's
own drawing. This skill closes that gap with three layers. Use the cheapest one
that proves the claim.

| Layer | Proves | Command |
|---|---|---|
| Frame snapshots | Composed frame for fixed state, in `just test` | `cargo nextest run -E 'test(frame_snapshot)'` |
| Scenarios | Real client + server + PTYs, end to end | `just tui-scenarios [name...]` |
| Ad-hoc harness | Anything interactive while developing | `scripts/tui_harness.sh ...` |

## Frame snapshots

`src/client/shell/tests/frame_snapshots.rs::assert_frame_snapshot(name, &frame)`
compares a `ClientShellState::compose(w, h)` frame with
`src/client/shell/tests/snapshots/<name>.txt`.

- New or intended change: rerun with `HERDR_BLESS=1`, then read the `.txt` diff
  before committing it. Never bless a snapshot you haven't looked at.
- Build state from `tests/mod.rs::snapshot()` and `surface()`. Config fields
  hold parsed values (e.g. `group_by = "stage"`, not `"$stage"`).
- Snapshots don't exercise the server or the wire. A green snapshot says
  nothing about whether the server ever sends that state.

## Harness

`scripts/tui_harness.sh` runs `target/debug/herdr` in a detached tmux session
(default 160x45). Config, state, and sockets live in `/var/tmp/htv-<name>`, and
every inherited `HERDR_*` variable is cleared, so it cannot reach the caller's
session. Safe to run from inside Herdr. Run `--help` for every command.

```bash
H=scripts/tui_harness.sh
$H up --config my.toml         # appended to a config with onboarding off
$H cli pane list               # any herdr CLI command, against the harness
$H prefix c                    # ctrl+b then c (new tab)
$H click 60 10 right           # 1-based col,row; SGR mouse
$H wait-text 'review' 5        # prefer waits to sleeps
$H find 'review \(2\)'         # "row col line"; col is a screen cell (Python regex)
$H shot 01-open "caption"      # .txt/.ansi/.html under <run>/shots
$H report "Title"              # all shots in one HTML page
$H down                        # always, including after failure
```

- `up` returns once the client has drawn its first snapshot. Keys sent before
  that are refused with a "not ready" notice, as they would be for a user.
- `HTV_NAME=<name>` runs several instances side by side. Subagents must use
  their own name.
- Seed fake agents with `pane report-agent` and tokens with
  `pane report-metadata` (see `scripts/tui_scenarios/sidebar_group_by.sh`).
  Don't start real agents unless the change depends on one.
- Use cheap stand-ins for configured commands (`echo ready; exec /bin/sh`),
  so readiness is a string you can wait for.
- The screen is captured by tmux, not a real terminal. Colors are close, and
  images or other terminal graphics are not captured.
- tmux sends no focus events or mouse motion, so the incidental full renders a
  real terminal triggers don't happen. That surfaces missing redraw triggers
  that users would rarely notice. When a screen only updates after
  `tmux -L htv-<name> resize-window`, the change is missing a render trigger.
- If herdr crashes, the tmux pane stays open showing `[herdr exited]`. Logs are
  at `<run>/config/herdr-dev/herdr-{server,client}.log`.
  `HERDR_RENDER_PROF=1` on `up` logs which render path ran.

## Scenarios

One script per feature in `scripts/tui_scenarios/<name>.sh`. Each script sources
`lib.sh`, starts its own named harness, asserts with `expect`,
`expect_absent`, and `expect_above`, saves shots, and prints `PASS: <name>`.
Teardown runs on exit. `HTV_KEEP=1` leaves the harness up for inspection.

When you add a user-visible feature, add or extend a scenario. A scenario must
fail without the change. Check this once by breaking the assertion or
reverting the change.

## Showing the result

For a walkthrough or screenshots, capture numbered shots with captions that
say what changed, run `report`, and publish the HTML as an Artifact. The
shots are HTML, so they stay sharp and small.
