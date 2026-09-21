use serde::{Deserialize, Serialize};

/// The two terminals the Sidecar panel hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SidecarTab {
    Notes,
    Chat,
}

/// `sidecar.show`: start the tab's terminal if needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SidecarShowParams {
    pub tab: SidecarTab,
}

/// `sidecar.send`: deliver text to a tab. A running terminal receives it as a
/// paste; a stopped Notes tab gets it appended to the notes file; a stopped
/// Chat tab is started and receives it once ready. At most 64 KiB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SidecarSendParams {
    pub tab: SidecarTab,
    pub text: String,
}
