//! Sidecar panel presentation: whether it is shown, which tab, and focus. The
//! terminals themselves are server-owned (`crate::app::sidecar`).

use super::*;

use crate::api::schema::SidecarTab;
use crate::protocol::endpoint::SidecarSurfaceControl;

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
