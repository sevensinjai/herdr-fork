//! Whole-frame golden snapshots of the composed client shell.
//!
//! Each snapshot is the plain text of a `compose()` frame, stored under
//! `src/client/shell/tests/snapshots/<name>.txt`. Run with `HERDR_BLESS=1` to
//! write or refresh them, then review the diff like any other code change.

use super::*;

const BLESS_ENV_VAR: &str = "HERDR_BLESS";

fn snapshot_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/client/shell/tests/snapshots")
        .join(format!("{name}.txt"))
}

fn frame_text(frame: &FrameData) -> String {
    let mut text = String::new();
    for row in frame_rows(frame) {
        text.push_str(row.trim_end());
        text.push('\n');
    }
    text
}

/// Compare `frame` with the stored snapshot `name`, or rewrite it under
/// `HERDR_BLESS=1`. A mismatch lists every differing row.
pub(super) fn assert_frame_snapshot(name: &str, frame: &FrameData) {
    let actual = frame_text(frame);
    let path = snapshot_path(name);
    if std::env::var_os(BLESS_ENV_VAR).is_some() {
        std::fs::create_dir_all(path.parent().expect("snapshot dir")).expect("create snapshot dir");
        std::fs::write(&path, &actual).expect("write snapshot");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run with {BLESS_ENV_VAR}=1 to create it",
            path.display()
        )
    });
    if actual == expected {
        return;
    }
    let expected_rows: Vec<&str> = expected.lines().collect();
    let actual_rows: Vec<&str> = actual.lines().collect();
    let mut diff = String::new();
    for row in 0..expected_rows.len().max(actual_rows.len()) {
        let want = expected_rows.get(row).copied().unwrap_or("<missing>");
        let got = actual_rows.get(row).copied().unwrap_or("<missing>");
        if want != got {
            diff.push_str(&format!("row {:>3} - {want}\n        + {got}\n", row + 1));
        }
    }
    panic!(
        "frame differs from snapshot {}; if the change is intended, rerun with \
         {BLESS_ENV_VAR}=1 and review the diff\n{diff}",
        path.display()
    );
}

fn agent(pane_id: &str, name: &str, status: AgentStatus, stage: Option<&str>) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some(name.into()),
        display_agent: None,
        agent: Some(name.into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: stage
            .map(|stage| vec![("stage".into(), stage.into())])
            .unwrap_or_default(),
        focused: pane_id == "pane_1",
    }
}

#[test]
fn agents_panel_grouped_by_stage() {
    let mut projected = snapshot();
    projected.agents = vec![
        agent("pane_1", "claude", AgentStatus::Working, Some("review")),
        agent("pane_2", "codex", AgentStatus::Blocked, Some("waiting QA")),
        agent("pane_3", "opencode", AgentStatus::Idle, None),
        agent("pane_4", "pi", AgentStatus::Working, Some("review")),
    ];
    let mut config = Config::default();
    config.ui.sidebar.agents.group_by = Some("stage".into());
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    let frame = state.compose(100, 30).expect("frame");
    assert_frame_snapshot("agents_panel_grouped_by_stage", &frame);
}
