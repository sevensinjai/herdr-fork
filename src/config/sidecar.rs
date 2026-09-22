use serde::{Deserialize, Deserializer, Serialize};

use crate::popup_size::PopupSize;

/// `[sidecar]`: the right-docked Notes/Chat panel.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct SidecarConfig {
    /// Panel width as cells or a percentage of the pane area.
    pub width: PopupSize,
    /// Shell command for the Notes tab. `$HERDR_SIDECAR_NOTES` holds the file path.
    #[serde(deserialize_with = "non_blank_command")]
    pub notes_command: String,
    /// Shell command for the Chat tab.
    #[serde(deserialize_with = "non_blank_command")]
    pub chat_command: String,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            width: PopupSize::Percent(35),
            notes_command: "${EDITOR:-vi} \"$HERDR_SIDECAR_NOTES\"".to_string(),
            chat_command: "claude".to_string(),
        }
    }
}

fn non_blank_command<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "sidecar command must not be empty",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use crate::popup_size::PopupSize;

    #[test]
    fn defaults() {
        let config: crate::config::Config = toml::from_str("").expect("defaults");
        assert_eq!(config.sidecar.width, PopupSize::Percent(35));
        assert_eq!(
            config.sidecar.notes_command,
            "${EDITOR:-vi} \"$HERDR_SIDECAR_NOTES\""
        );
        assert_eq!(config.sidecar.chat_command, "claude");
    }

    #[test]
    fn parses_overrides_and_rejects_blank_commands() {
        let config: crate::config::Config = toml::from_str(
            "[sidecar]\nwidth = 60\nnotes_command = \"micro $HERDR_SIDECAR_NOTES\"\nchat_command = \"codex\"\n",
        )
        .expect("overrides");
        assert_eq!(config.sidecar.width, PopupSize::Cells(60));
        assert_eq!(config.sidecar.chat_command, "codex");
        assert!(
            toml::from_str::<crate::config::Config>("[sidecar]\nchat_command = \"  \"\n").is_err()
        );
    }

    #[test]
    fn default_sidecar_keys_are_bound() {
        let keys = crate::config::KeysConfig::default();
        assert_eq!(
            keys.sidecar_toggle,
            crate::config::BindingConfig::one("prefix+shift+s")
        );
        assert_eq!(
            keys.sidecar_switch_tab,
            crate::config::BindingConfig::one("prefix+shift+o")
        );
        assert_eq!(
            keys.sidecar_send_selection,
            crate::config::BindingConfig::one("prefix+shift+y")
        );
        let config = crate::config::Config::default();
        assert!(
            config.collect_diagnostics().is_empty(),
            "{:?}",
            config.collect_diagnostics()
        );
    }
}
