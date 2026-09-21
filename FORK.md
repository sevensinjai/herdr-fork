# Fork notice

This repository is a fork of [herdrdev/herdr](https://github.com/herdrdev/herdr),
licensed under Apache-2.0. The upstream `LICENSE` is retained unchanged.

Per Apache-2.0 section 4(b), the modifications made in this fork are:

## Added `ui.sidebar.agents.group_by`

Lists agents in the Agents panel under headings you define, instead of one
flat list. Set it to a custom pane metadata token:

```toml
[ui.sidebar.agents]
group_by = "$stage"
```

```bash
herdr pane report-metadata <pane_id> --source my-tool --token stage="waiting QA"
```

Files changed relative to upstream:

- `src/config/sidebar.rs` — added the `group_by` field and its validation.
- `src/client/shell/agent_sidebar.rs` — added `apply_grouping`,
  `group_agent_rows`, `fit_group_label`, `render_group_header`, the
  `AgentListEntry` type, and a `group_tests` module.
- `src/client/shell/sidebar.rs`, `src/client/shell/actions.rs` — pass the new
  setting into the shared ordering function.
- `src/client/shell/aggregate_navigation.rs` — passes `None`; the aggregate
  multi-machine list is unchanged.
- `docs/next/website/src/content/docs/configuration.mdx` — documents the
  setting.

Base commit: f090ce95d05d2e7811869be93f9a23bf4ac2c3af

## Added the Sidecar panel

A panel that slides in from the right over the panes, holding two terminals
that keep running while it is hidden. **Notes** is an editor on
`~/.config/herdr/sidecar/<session>.md`, and **Chat** is a scratch agent
(`claude` by default). There is one Sidecar per named session. Select text in
any pane and send it with the pane right-click menu or `prefix+shift+y`.
Configure it under `[sidecar]`. The keys are `sidecar_toggle`
(`prefix+shift+s`), `sidecar_switch_tab` (`prefix+shift+o`), and
`sidecar_send_selection` (`prefix+shift+y`).

Transport: the frozen generation-1 codecs (`PaneSurfaceFrame`, `ClientMessage`,
`ServerMessage`) are unchanged. The Sidecar travels over the optional
`sidecar` endpoint capability with two `EndpointControl` kinds,
`endpoint.sidecar-view.v1` and `endpoint.sidecar-surface.v1`. Two new API
methods, `sidecar.show` and `sidecar.send`, are pinned separately from the v1
method fixture. Older servers and clients ignore all of it.

Files: `src/app/sidecar.rs`, `src/server/headless/sidecar.rs`,
`src/client/shell/sidecar.rs`, `src/config/sidecar.rs`,
`src/api/schema/sidecar.rs`, plus wiring in the endpoint protocol, client
shell input/mouse/composition, keybindings, and docs.

## Status

Both changes compile and pass `cargo clippy -D warnings` and the full
nextest suite on macOS arm64. The exceptions are five integration tests
(`api_ping`, `live_handoff`, `multi_client`) that fail identically on
unmodified `master` in this environment. The Windows cross-lint was not run.

## Not for upstream

herdr does not accept unsolicited pull requests; see upstream
`CONTRIBUTING.md`. This fork exists for personal use. Feature requests belong
in upstream GitHub Discussions.
