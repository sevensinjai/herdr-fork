use super::*;
use crate::api::schema::SidecarTab;
use crate::protocol::endpoint::{SidecarSurfaceControl, SidecarViewControl, SIDECAR_VIEW_KIND};

fn sidecar_state(supported: bool) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.set_endpoint_methods(Some(vec!["sidecar.show".into(), "sidecar.send".into()]));
    state.set_endpoint_sidecar_supported(&ClientEndpointId::Local, supported);
    state.compose(120, 30).expect("frame");
    state
}

fn toggle(state: &mut ClientShellState) -> ClientShellInput {
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::SidecarToggle),
        &mut outcome,
    );
    outcome
}

fn shown_tabs(outcome: &ClientShellInput) -> Vec<SidecarTab> {
    outcome
        .actions
        .iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                crate::api::schema::Method::SidecarShow(params) => Some(params.tab),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn take_view(state: &mut ClientShellState) -> Option<SidecarViewControl> {
    match state.take_sidecar_view_message()? {
        crate::protocol::ClientMessage::EndpointControl { kind, data } => {
            assert_eq!(kind, SIDECAR_VIEW_KIND);
            Some(serde_json::from_str(&data).expect("view json"))
        }
        other => panic!("unexpected sidecar message {other:?}"),
    }
}

fn install_surface(state: &mut ClientShellState, tab: SidecarTab, text: &str) {
    let buffer = Buffer::with_lines([text]);
    state.set_sidecar_surface(
        &ClientEndpointId::Local,
        SidecarSurfaceControl {
            tab,
            terminal_id: Some("sidecar-terminal".into()),
            frame: Some(FrameData::from_ratatui_buffer_with_hyperlinks(
                &buffer,
                None,
                &[],
            )),
            mouse_reporting: false,
        },
    );
}

fn frame_rows(frame: &FrameData) -> Vec<String> {
    frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol.as_str()).collect())
        .collect()
}

#[test]
fn toggle_needs_the_sidecar_capability() {
    let mut state = sidecar_state(false);
    let outcome = toggle(&mut state);
    assert!(state.sidecar.is_none());
    assert!(shown_tabs(&outcome).is_empty());
}

#[test]
fn toggle_opens_focused_on_notes_then_closes() {
    let mut state = sidecar_state(true);
    let surface_size = state.surface_size(120, 30);

    let outcome = toggle(&mut state);
    assert_eq!(shown_tabs(&outcome), vec![SidecarTab::Notes]);
    assert_eq!(
        state.sidecar.map(|ui| (ui.tab, ui.focused)),
        Some((SidecarTab::Notes, true))
    );
    state.compose(120, 30).expect("frame");
    let view = take_view(&mut state).expect("open view");
    assert!(view.open);
    assert_eq!(view.tab, SidecarTab::Notes);
    assert!(view.cols >= crate::popup_size::SIDECAR_MIN_COLS - 1 && view.rows > 0);
    assert!(take_view(&mut state).is_none(), "sent once");
    assert_eq!(
        state.surface_size(120, 30),
        surface_size,
        "panes are not resized"
    );

    toggle(&mut state);
    assert!(state.sidecar.is_none());
    state.compose(120, 30).expect("frame");
    assert!(!take_view(&mut state).expect("closed view").open);
}

#[test]
fn open_sidecar_draws_its_header_and_terminal_at_the_right_edge() {
    let mut state = sidecar_state(true);
    toggle(&mut state);
    install_surface(&mut state, SidecarTab::Notes, "NOTES_BODY");
    let frame = state.compose(120, 30).expect("frame");
    let rows = frame_rows(&frame);
    let panel = state.hits.sidecar_panel.expect("panel hit");
    assert_eq!(panel.right(), 120);
    assert!(rows[panel.y as usize].contains("Notes"));
    assert!(rows[panel.y as usize].contains("Chat"));
    assert!(rows.iter().any(|row| row.contains("NOTES_BODY")));
}

#[test]
fn focused_sidecar_takes_typing_but_not_the_prefix() {
    let mut state = sidecar_state(true);
    toggle(&mut state);
    install_surface(&mut state, SidecarTab::Notes, "notes");
    state.compose(120, 30).expect("frame");

    let typed = state.handle_raw_events(vec![RawInputEvent::Text(crate::input::TextCommit::new(
        "x",
    ))]);
    assert!(matches!(
        typed.requests.as_slice(),
        [ClientMessage::ClientShellPopupInput { terminal_id, .. }] if terminal_id == "sidecar-terminal"
    ));

    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    ))]);
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('S'),
        KeyModifiers::SHIFT,
    ))]);
    assert!(state.sidecar.is_none(), "prefix+shift+s closed it");
}

#[test]
fn clicking_a_tab_switches_and_clicking_a_pane_unfocuses() {
    let mut state = sidecar_state(true);
    toggle(&mut state);
    install_surface(&mut state, SidecarTab::Notes, "notes");
    state.compose(120, 30).expect("frame");

    let (chat_rect, _) = *state
        .hits
        .sidecar_tabs
        .iter()
        .find(|(_, tab)| *tab == SidecarTab::Chat)
        .expect("chat tab hit");
    let clicked = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: chat_rect.x,
        row: chat_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert_eq!(shown_tabs(&clicked), vec![SidecarTab::Chat]);
    assert_eq!(state.sidecar.map(|ui| ui.tab), Some(SidecarTab::Chat));

    state.compose(120, 30).expect("frame");
    let pane = state.hits.panes[0].inner_rect;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.x,
        row: pane.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert_eq!(state.sidecar.map(|ui| ui.focused), Some(false));
    let typed = state.handle_raw_events(vec![RawInputEvent::Text(crate::input::TextCommit::new(
        "y",
    ))]);
    assert!(matches!(
        typed.requests.as_slice(),
        [ClientMessage::ClientShellPaneInput { .. }]
    ));
}
