//! Per-client Sidecar view handling and surface streaming. The Sidecar travels
//! as optional `EndpointControl` kinds so the frozen generation-1 surface and
//! input codecs stay untouched.

use super::*;

use crate::protocol::endpoint::{SidecarSurfaceControl, SidecarViewControl};

impl HeadlessServer {
    /// Stores a client's Sidecar view and sizes the shown terminal to it. The
    /// most recent client to open a tab owns that terminal's size.
    pub(super) fn apply_client_sidecar_view(&mut self, client_id: u64, data: &str) -> bool {
        let Ok(view) = serde_json::from_str::<SidecarViewControl>(data) else {
            debug!(client_id, "ignoring malformed sidecar view");
            return false;
        };
        let Some(client) = self.clients.get_mut(&client_id) else {
            return false;
        };
        if !client.is_active_shell_client() {
            return false;
        }
        client.shell_sidecar_view = Some(view);
        let cell = client.cell_size;
        fit_sidecar_to_view(&self.app, view, cell);
        true
    }

    /// Sends this client its Sidecar surface when it differs from the last one
    /// sent. Hidden and never-opened views cost one comparison.
    pub(super) fn stream_client_sidecar(&mut self, client_id: u64) -> Result<(), ()> {
        let Some(client) = self.clients.get(&client_id) else {
            return Ok(());
        };
        let next = match client.shell_sidecar_view {
            Some(view) if view.open => {
                // The view can arrive before `sidecar.show` starts the terminal.
                fit_sidecar_to_view(&self.app, view, client.cell_size);
                Some(sidecar_surface(&self.app, view))
            }
            Some(view) if client.shell_sidecar_sent.is_some() => Some(SidecarSurfaceControl {
                tab: view.tab,
                terminal_id: None,
                frame: None,
                mouse_reporting: false,
            }),
            _ => None,
        };
        let Some(next) = next else {
            return Ok(());
        };
        if client.shell_sidecar_sent.as_ref() == Some(&next) {
            return Ok(());
        }
        let message = crate::protocol::endpoint::sidecar_surface_message(&next).map_err(|err| {
            warn!(client_id, err = %err, "failed to encode sidecar surface");
        })?;
        let framed = Self::frame_server_message(&message).map_err(|err| {
            warn!(client_id, err = %err, "failed to frame sidecar surface");
        })?;
        let Some(client) = self.clients.get_mut(&client_id) else {
            return Ok(());
        };
        let Some(writer) = client.writer.as_ref() else {
            return Err(());
        };
        writer.control.send(framed).map_err(|_| ())?;
        // A closed surface is sent once; afterwards the client has nothing to draw.
        client.shell_sidecar_sent = next.frame.is_some().then_some(next);
        Ok(())
    }
}

/// Resizes the shown Sidecar terminal to `view` when they differ.
fn fit_sidecar_to_view(
    app: &crate::app::App,
    view: SidecarViewControl,
    cell: crate::kitty_graphics::HostCellSize,
) {
    if !view.open || view.cols == 0 || view.rows == 0 {
        return;
    }
    let Some(slot) = app.state.sidecar.slot(view.tab) else {
        return;
    };
    if app
        .state
        .direct_attach_resize_locks
        .contains(&slot.terminal_id)
    {
        return;
    }
    let Some(runtime) = app.terminal_runtimes.get(&slot.terminal_id) else {
        return;
    };
    if runtime.current_size() != (view.rows, view.cols) {
        runtime.resize(view.rows, view.cols, cell.width_px, cell.height_px);
    }
}

fn sidecar_surface(app: &crate::app::App, view: SidecarViewControl) -> SidecarSurfaceControl {
    let closed = SidecarSurfaceControl {
        tab: view.tab,
        terminal_id: None,
        frame: None,
        mouse_reporting: false,
    };
    let Some(slot) = app.state.sidecar.slot(view.tab) else {
        return closed;
    };
    let Some(runtime) = app.terminal_runtimes.get(&slot.terminal_id) else {
        return closed;
    };
    let area = Rect::new(0, 0, view.cols.max(1), view.rows.max(1));
    let (buffer, cursor) = crate::server::render_stream::render_terminal_virtual(runtime, area);
    let hyperlinks = runtime.visible_hyperlinks(area);
    SidecarSurfaceControl {
        tab: view.tab,
        terminal_id: Some(slot.terminal_id.to_string()),
        frame: Some(FrameData::from_ratatui_buffer_with_hyperlinks(
            &buffer,
            cursor,
            &hyperlinks,
        )),
        mouse_reporting: runtime.mouse_reporting_enabled(),
    }
}
