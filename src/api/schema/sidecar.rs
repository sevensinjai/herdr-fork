use serde::{Deserialize, Serialize};

/// The two terminals the Sidecar panel hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SidecarTab {
    Notes,
    Chat,
}
