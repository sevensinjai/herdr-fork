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

## Status

This change has NOT been compiled. It was written in an environment that
could not build herdr, because the Zig build step for the vendored
`libghostty-vt` fetches dependencies from a host that was unreachable there.

Verified: the changed files parse and are rustfmt-clean; the real bodies of
`apply_grouping`, `group_agent_rows` and `fit_group_label` were compiled
against the ratatui and unicode-width versions this repo pins and exercised
with assertions covering ordering, heading boundaries, counts, untagged
agents, narrow sidebars, and wide-character labels.

Not verified: that the whole crate compiles, `just ci`, or the panel inside a
running herdr. Expect to fix small compile errors on a first build.

## Not for upstream

herdr does not accept unsolicited pull requests; see upstream
`CONTRIBUTING.md`. This fork exists for personal use. Feature requests belong
in upstream GitHub Discussions.
