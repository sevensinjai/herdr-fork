//! Sidecar panel presentation: whether it is shown, which tab, and focus. The
//! terminals themselves are server-owned (`crate::app::sidecar`).

use super::*;

use crossterm::event::{MouseButton, MouseEventKind};
use tracing::warn;

use crate::api::schema::SidecarTab;
use crate::protocol::endpoint::{SidecarSurfaceControl, SidecarViewControl};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SidecarUi {
    pub(crate) tab: SidecarTab,
    pub(crate) focused: bool,
}

impl ClientShellState {
    /// Stores the active endpoint's Sidecar surface. Surfaces from other
    /// endpoints and for a tab no longer shown are dropped.
    pub(crate) fn set_sidecar_surface(
        &mut self,
        endpoint_id: &ClientEndpointId,
        surface: SidecarSurfaceControl,
    ) {
        if endpoint_id != &self.active_endpoint_id {
            return;
        }
        let shown_tab = self.sidecar.map(|sidecar| sidecar.tab);
        self.sidecar_surface =
            (surface.frame.is_some() && shown_tab == Some(surface.tab)).then_some(surface);
    }

    /// The server can no longer host the Sidecar (older server or reconnect).
    pub(super) fn close_sidecar_for_lost_support(&mut self) {
        self.sidecar = None;
        self.sidecar_surface = None;
    }

    pub(super) fn sidecar_supported(&self) -> bool {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == self.active_endpoint_id)
            .is_some_and(|endpoint| endpoint.sidecar_supported)
    }
}

impl ClientShellState {
    pub(super) fn toggle_sidecar(&mut self, outcome: &mut ClientShellInput) {
        if self.sidecar.take().is_some() {
            self.sidecar_surface = None;
            outcome.repaint = true;
            return;
        }
        if !self.sidecar_supported() {
            return;
        }
        self.sidecar = Some(SidecarUi {
            tab: self.sidecar_tab,
            focused: true,
        });
        self.request_sidecar_show(self.sidecar_tab, outcome);
    }

    /// Shows `tab`, opening and focusing the Sidecar if it was hidden.
    pub(super) fn show_sidecar_tab(&mut self, tab: SidecarTab, outcome: &mut ClientShellInput) {
        if !self.sidecar_supported() {
            return;
        }
        let focused = self.sidecar.is_none_or(|ui| ui.focused);
        if self.sidecar.map(|ui| ui.tab) != Some(tab) {
            self.sidecar_surface = None;
        }
        self.sidecar_tab = tab;
        self.sidecar = Some(SidecarUi { tab, focused });
        self.request_sidecar_show(tab, outcome);
    }

    fn request_sidecar_show(&mut self, tab: SidecarTab, outcome: &mut ClientShellInput) {
        // Force the next compose to report the view even if its size is unchanged.
        self.sidecar_view_sent = None;
        self.push_endpoint_method(
            crate::api::schema::Method::SidecarShow(crate::api::schema::SidecarShowParams { tab }),
            outcome,
        );
        outcome.repaint = true;
    }

    /// Target for keys, text, and paste while the Sidecar has focus and its
    /// terminal is on screen.
    pub(super) fn sidecar_input_target(&self) -> Option<ClientInputTarget> {
        let ui = self.sidecar?;
        if !ui.focused {
            return None;
        }
        let surface = self.sidecar_surface.as_ref()?;
        surface
            .terminal_id
            .clone()
            .filter(|_| surface.tab == ui.tab)
            .map(ClientInputTarget::Popup)
    }

    /// The Sidecar view message the server has not seen yet, if any.
    pub(crate) fn take_sidecar_view_message(&mut self) -> Option<crate::protocol::ClientMessage> {
        let view = self.sidecar_view_pending.take()?;
        match crate::protocol::endpoint::sidecar_view_message(&view) {
            Ok(message) => {
                self.sidecar_view_sent = Some(view);
                Some(message)
            }
            Err(err) => {
                warn!(err = %err, "failed to encode sidecar view");
                None
            }
        }
    }

    /// Handles a mouse event over the Sidecar. Returns true when consumed.
    /// Presses elsewhere only move focus off the Sidecar and fall through.
    pub(super) fn handle_sidecar_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(mut ui) = self.sidecar else {
            return false;
        };
        let pressed = matches!(mouse.kind, MouseEventKind::Down(_));
        if !self
            .hits
            .sidecar_panel
            .is_some_and(|panel| super::contains(panel, point))
        {
            if pressed && ui.focused {
                ui.focused = false;
                self.sidecar = Some(ui);
                outcome.repaint = true;
            }
            return false;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(tab) = self
                .hits
                .sidecar_tabs
                .iter()
                .find(|(rect, _)| super::contains(*rect, point))
                .map(|(_, tab)| *tab)
            {
                ui.focused = true;
                self.sidecar = Some(ui);
                self.show_sidecar_tab(tab, outcome);
                return true;
            }
        }
        if pressed && !ui.focused {
            ui.focused = true;
            self.sidecar = Some(ui);
            outcome.repaint = true;
        }
        if let Some(hit) = self.hits.sidecar_body.clone() {
            if super::contains(hit.inner_rect, point) {
                let forward = match mouse.kind {
                    MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight => true,
                    MouseEventKind::Down(_)
                    | MouseEventKind::Up(_)
                    | MouseEventKind::Drag(_)
                    | MouseEventKind::Moved => hit.mouse_reporting,
                };
                if forward {
                    self.push_pane_mouse_event(&hit, mouse, mouse.modifiers, outcome);
                }
            }
        }
        true
    }
}

/// Draws the Sidecar over the right side of `pane_area` and records its hits.
/// Returns the view the server should know about, when it differs from `sent`.
#[allow(clippy::too_many_arguments)] // Disjoint shell fields, borrowed while the snapshot is.
pub(super) fn compose_sidecar(
    ui: Option<SidecarUi>,
    closed_tab: SidecarTab,
    surface: Option<&SidecarSurfaceControl>,
    sent: Option<SidecarViewControl>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
    frame: &mut FrameData,
    pane_area: Rect,
    occlusion: &mut crate::kitty_graphics::surface::Occlusion,
) -> Option<Option<SidecarViewControl>> {
    let changed = |view: SidecarViewControl| (sent != Some(view)).then_some(view);
    hits.sidecar_panel = None;
    hits.sidecar_body = None;
    hits.sidecar_tabs.clear();
    let Some(ui) = ui else {
        let closed = SidecarViewControl {
            open: false,
            tab: closed_tab,
            cols: 0,
            rows: 0,
        };
        return Some(sent.filter(|view| view.open).and_then(|_| changed(closed)));
    };
    let Some(geometry) =
        crate::popup_size::resolve_right_docked_geometry(config.sidecar_width, pane_area)
    else {
        return Some(None);
    };
    let view = changed(SidecarViewControl {
        open: true,
        tab: ui.tab,
        cols: geometry.inner.width,
        rows: geometry.inner.height,
    });
    occlusion.cover(geometry.outer);

    let palette = &config.palette;
    let border_color = if ui.focused {
        palette.accent
    } else {
        palette.text
    };
    let mut composed = frame.to_ratatui_buffer()?;
    ratatui::widgets::Widget::render(ratatui::widgets::Clear, geometry.outer, &mut composed);
    composed.set_style(
        geometry.outer,
        ratatui::style::Style::default().bg(palette.panel_bg),
    );
    for y in geometry.outer.top()..geometry.outer.bottom() {
        if let Some(cell) = composed.cell_mut((geometry.outer.x, y)) {
            cell.set_symbol("│")
                .set_style(ratatui::style::Style::default().fg(border_color));
        }
    }
    let mut x = geometry.outer.x + 1;
    let header_y = geometry.outer.y;
    for (tab, label) in [(SidecarTab::Notes, " Notes "), (SidecarTab::Chat, " Chat ")] {
        let width = label.chars().count() as u16;
        if x + width > geometry.outer.right() {
            break;
        }
        let style = if tab == ui.tab {
            ratatui::style::Style::default()
                .fg(panel_contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            ratatui::style::Style::default()
                .fg(palette.text)
                .bg(palette.panel_bg)
        };
        composed.set_string(x, header_y, label, style);
        hits.sidecar_tabs
            .push((Rect::new(x, header_y, width, 1), tab));
        x += width + 1;
    }
    let shown = surface.filter(|surface| surface.tab == ui.tab);
    if shown.is_none() {
        composed.set_string(
            geometry.inner.x + 1,
            geometry.inner.y,
            "starting…",
            ratatui::style::Style::default().fg(palette.text),
        );
    }
    frame.replace_from_ratatui_buffer_preserving_effects(&composed, None);
    hits.sidecar_panel = Some(geometry.outer);
    if let Some(surface) = shown {
        if let (Some(source), Some(terminal_id)) = (&surface.frame, &surface.terminal_id) {
            super::blit_pane_surface(frame, source, geometry.inner);
            hits.sidecar_body = Some(PaneHit {
                rect: geometry.outer,
                inner_rect: geometry.inner,
                scrollbar_rect: None,
                scroll: None,
                pane_id: terminal_id.clone(),
                popup: true,
                mouse_reporting: surface.mouse_reporting,
                sgr_pixel_mouse: false,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }
    Some(view)
}
