use super::*;
use crate::api::schema::SidecarTab;
use crate::protocol::endpoint::{SidecarSurfaceControl, SidecarViewControl, SIDECAR_VIEW_KIND};

fn sidecar_state(supported: bool) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.set_endpoint_methods(Some(vec![
        "sidecar.show".into(),
        "sidecar.send".into(),
        "pane.selection.read".into(),
    ]));
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
            exited: false,
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

fn endpoint_request_id(outcome: &ClientShellInput) -> String {
    outcome
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.id.clone()),
            _ => None,
        })
        .expect("endpoint request")
}

fn select_in_pane_1(state: &mut ClientShellState) {
    state.selection = Some(crate::selection::Selection::absolute_range(
        "pane_1".into(),
        (0, 0),
        (0, 2),
    ));
}

fn right_click_pane(state: &mut ClientShellState) -> Vec<ClientContextMenuAction> {
    state.compose(120, 30).expect("frame");
    let pane = state.hits.panes[0].inner_rect;
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: pane.x + 1,
        row: pane.y,
        modifiers: KeyModifiers::empty(),
    })]);
    match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => {
            menu.items().iter().map(|item| item.action).collect()
        }
        _ => panic!("pane context menu"),
    }
}

#[test]
fn pane_menu_offers_sidecar_send_only_for_a_selection_in_that_pane() {
    let mut state = sidecar_state(true);
    let without = right_click_pane(&mut state);
    assert!(!without.contains(&ClientContextMenuAction::SendSelectionToSidecarNotes));

    state.overlay = None;
    select_in_pane_1(&mut state);
    let with = right_click_pane(&mut state);
    assert!(with.contains(&ClientContextMenuAction::SendSelectionToSidecarNotes));
    assert!(with.contains(&ClientContextMenuAction::SendSelectionToSidecarChat));

    let mut state = sidecar_state(false);
    select_in_pane_1(&mut state);
    let unsupported = right_click_pane(&mut state);
    assert!(!unsupported.contains(&ClientContextMenuAction::SendSelectionToSidecarNotes));
}

#[test]
fn sending_a_selection_reads_it_then_calls_sidecar_send() {
    let mut state = sidecar_state(true);
    select_in_pane_1(&mut state);
    let mut outcome = ClientShellInput::default();
    state.request_selection_send_to_sidecar(SidecarTab::Chat, &mut outcome);
    let read_id = endpoint_request_id(&outcome);
    assert!(outcome.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(request.method, crate::api::schema::Method::PaneSelectionRead(_))
    )));

    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &read_id,
        Ok(crate::api::schema::ResponseResult::PaneSelection {
            pane_id: "pane_1".into(),
            text: "picked".into(),
        }),
    );
    assert!(actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::SidecarSend(params)
                    if params.tab == SidecarTab::Chat && params.text == "picked"
            )
    )));
}

#[test]
fn send_selection_key_targets_notes_until_another_tab_is_used() {
    let mut state = sidecar_state(true);
    select_in_pane_1(&mut state);
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::SidecarSendSelection),
        &mut outcome,
    );
    let read_id = endpoint_request_id(&outcome);
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &read_id,
        Ok(crate::api::schema::ResponseResult::PaneSelection {
            pane_id: "pane_1".into(),
            text: "note this".into(),
        }),
    );
    assert!(actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(
                &request.method,
                crate::api::schema::Method::SidecarSend(params) if params.tab == SidecarTab::Notes
            )
    )));
}

fn one_row_patch(state: &ClientShellState) -> crate::protocol::PaneSurfacePatch {
    let mut updated_pane = state.pane_surface.as_ref().expect("surface").panes[0].clone();
    updated_pane.content_revision += 1;
    crate::protocol::PaneSurfacePatch {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 1,
        surface_revision: 2,
        rows: vec![crate::protocol::PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![
                crate::protocol::CellData {
                    symbol: "N".into(),
                    fg: 0,
                    bg: 0,
                    modifier: 0,
                    skip: false,
                    hyperlink: None,
                };
                4
            ],
        }],
        panes: vec![updated_pane],
        cursor: None,
    }
}

#[test]
fn open_sidecar_forces_pane_patches_through_a_full_recompose() {
    let mut state = sidecar_state(true);
    let patch = one_row_patch(&state);
    assert!(
        matches!(
            state.apply_pane_surface_patch(patch),
            ClientPaneSurfacePatchOutcome::Applied(Some(_))
        ),
        "hidden sidecar keeps the fast path"
    );

    let mut state = sidecar_state(true);
    toggle(&mut state);
    install_surface(&mut state, SidecarTab::Notes, "notes");
    state.compose(120, 30).expect("frame");
    let patch = one_row_patch(&state);
    assert!(
        matches!(
            state.apply_pane_surface_patch(patch),
            ClientPaneSurfacePatchOutcome::Applied(None)
        ),
        "pane rows must not be written over the sidecar"
    );
}

fn global_menu_labels(state: &ClientShellState) -> Vec<&'static str> {
    super::super::global_menu::global_menu_items(
        state.snapshot.as_deref().expect("snapshot"),
        state.sidecar_menu_state(),
    )
    .into_iter()
    .map(|(label, _)| label)
    .collect()
}

#[test]
fn launcher_menu_opens_and_closes_the_sidecar() {
    let unsupported = sidecar_state(false);
    assert!(!global_menu_labels(&unsupported)
        .iter()
        .any(|label| label.contains("sidecar")));

    let mut state = sidecar_state(true);
    let labels = global_menu_labels(&state);
    let open_index = labels
        .iter()
        .position(|label| *label == "open sidecar")
        .expect("open item");
    state.toggle_global_menu();
    let mut outcome = ClientShellInput::default();
    state.activate_global_menu_item(open_index, &mut outcome);
    assert!(state.sidecar.is_some());
    assert_eq!(shown_tabs(&outcome), vec![SidecarTab::Notes]);

    let labels = global_menu_labels(&state);
    let close_index = labels
        .iter()
        .position(|label| *label == "close sidecar")
        .expect("close item");
    state.toggle_global_menu();
    state.activate_global_menu_item(close_index, &mut ClientShellInput::default());
    assert!(state.sidecar.is_none());
}

#[test]
fn header_close_button_hides_the_sidecar() {
    let mut state = sidecar_state(true);
    toggle(&mut state);
    install_surface(&mut state, SidecarTab::Notes, "notes");
    let frame = state.compose(120, 30).expect("frame");
    let close = state.hits.sidecar_close.expect("close button hit");
    let panel = state.hits.sidecar_panel.expect("panel");
    assert_eq!(close.y, panel.y);
    assert!(frame_rows(&frame)[close.y as usize].contains('✕'));

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: close.x,
        row: close.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(state.sidecar.is_none());
}

#[test]
fn an_exited_tab_says_so_instead_of_starting() {
    let mut state = sidecar_state(true);
    toggle(&mut state);
    state.set_sidecar_surface(
        &ClientEndpointId::Local,
        SidecarSurfaceControl {
            tab: SidecarTab::Notes,
            terminal_id: None,
            frame: None,
            mouse_reporting: false,
            exited: true,
        },
    );
    let rows = frame_rows(&state.compose(120, 30).expect("frame")).join("\n");
    assert!(rows.contains("Notes exited"), "{rows}");
    assert!(!rows.contains("starting"));
}

#[test]
fn open_sidecar_frame_snapshot() {
    let mut state = sidecar_state(true);
    toggle(&mut state);
    install_surface(&mut state, SidecarTab::Notes, "# notes for this session");
    let frame = state.compose(120, 30).expect("frame");
    super::frame_snapshots::assert_frame_snapshot("open_sidecar_notes", &frame);
}
