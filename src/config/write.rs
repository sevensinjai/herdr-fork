#[derive(Clone, Copy)]
pub(crate) enum ConfigEdit<'a> {
    Theme(&'a str),
    StatusIndicators(super::StatusIndicatorStyle),
    Sound(bool),
    ToastDelivery(super::ToastDelivery),
    /// The full `ui.sidebar.agents.group_values` list to write.
    AgentGroupValues(&'a [String]),
}

impl ConfigEdit<'_> {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Theme(_) => "theme",
            Self::StatusIndicators(_) => "status indicators",
            Self::Sound(_) => "sound setting",
            Self::ToastDelivery(_) => "toast setting",
            Self::AgentGroupValues(_) => "agent stage list",
        }
    }

    pub(crate) fn apply(self, content: &str) -> String {
        match self {
            Self::Theme(name) => {
                let content =
                    super::upsert_section_value(content, "theme", "name", &format!("\"{name}\""));
                super::upsert_section_bool(&content, "theme", "auto_switch", false)
            }
            Self::StatusIndicators(style) => super::upsert_section_value(
                content,
                "ui",
                "status_indicators",
                &format!("\"{}\"", style.as_str()),
            ),
            Self::Sound(enabled) => {
                super::upsert_section_bool(content, "ui.sound", "enabled", enabled)
            }
            Self::ToastDelivery(delivery) => {
                let value = match delivery {
                    super::ToastDelivery::Off => "\"off\"",
                    super::ToastDelivery::Herdr => "\"herdr\"",
                    super::ToastDelivery::Terminal => "\"terminal\"",
                    super::ToastDelivery::System => "\"system\"",
                };
                let content = super::upsert_section_value(content, "ui.toast", "delivery", value);
                super::remove_section_key(&content, "ui.toast", "enabled")
            }
            Self::AgentGroupValues(values) => {
                let array = values
                    .iter()
                    .map(|value| toml::Value::String(value.clone()).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                // The line-based upsert only replaces one line, so drop any
                // existing (possibly multi-line) array first.
                let content =
                    remove_section_array_key(content, "ui.sidebar.agents", "group_values");
                super::upsert_section_value(
                    &content,
                    "ui.sidebar.agents",
                    "group_values",
                    &format!("[{array}]"),
                )
            }
        }
    }
}

/// Removes `key = [...]` from `[section]`, including continuation lines of a
/// multi-line array. Brackets inside quoted strings are ignored.
fn remove_section_array_key(content: &str, section: &str, key: &str) -> String {
    let header = format!("[{section}]");
    let mut result = Vec::new();
    let mut in_section = false;
    let mut open_brackets = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if open_brackets > 0 {
            open_brackets = bracket_depth_after(trimmed, open_brackets);
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == header;
        } else if in_section {
            if let Some(value) = trimmed
                .strip_prefix(key)
                .map(str::trim_start)
                .and_then(|rest| rest.strip_prefix('='))
            {
                open_brackets = bracket_depth_after(value, 0);
                continue;
            }
        }
        result.push(line);
    }
    result.join("\n") + "\n"
}

/// Array bracket depth after scanning `text`, starting from `depth`. Skips
/// basic and literal strings and stops at a comment.
fn bracket_depth_after(text: &str, mut depth: usize) -> usize {
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '\'' => {
                for inner in chars.by_ref() {
                    if inner == '\'' {
                        break;
                    }
                }
            }
            '#' => break,
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

pub(crate) fn update_file_at(
    path: &std::path::Path,
    description: &str,
    update: impl FnOnce(&str) -> String,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create config directory: {error}"))?;
    }
    let content = match super::io::read_optional_config(path) {
        Ok(Some(content)) => content,
        Ok(None) => String::new(),
        Err(error) => {
            return Err(format!(
                "failed to read config before saving {description}: {error}"
            ));
        }
    };
    std::fs::write(path, update(&content))
        .map_err(|error| format!("failed to save {description}: {error}"))
}

pub(crate) fn write_edit(edit: ConfigEdit<'_>) -> Result<(), String> {
    update_file_at(&super::config_path(), edit.description(), |content| {
        edit.apply(content)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_file_at_does_not_move_a_leading_bom_into_the_file() {
        let dir = std::env::temp_dir().join(format!("herdr-config-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            b"\xEF\xBB\xBF[terminal]\ndefault_shell = \"pwsh.exe\"\n",
        )
        .unwrap();

        update_file_at(&path, "onboarding setting", |content| {
            crate::config::upsert_top_level_bool(content, "onboarding", false)
        })
        .unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);

        assert!(
            !written.contains('\u{feff}'),
            "unexpected BOM in {written:?}"
        );
        assert!(
            toml::from_str::<toml::Value>(&written).is_ok(),
            "written config is not valid TOML: {written:?}"
        );
    }

    fn parsed_group_values(content: &str) -> Vec<String> {
        toml::from_str::<crate::config::Config>(content)
            .expect("edited config parses")
            .ui
            .sidebar
            .agents
            .group_values
    }

    fn values(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn agent_group_values_edit_creates_the_section() {
        let list = values(&["working", "verifying"]);
        let updated = ConfigEdit::AgentGroupValues(&list).apply("");
        assert_eq!(parsed_group_values(&updated), list);
    }

    #[test]
    fn agent_group_values_edit_replaces_a_single_line_list_in_place() {
        let content = "[ui.sidebar.agents]\ngroup_by = \"$stage\"\ngroup_values = [\"working\"]\n\n[ui.toast]\ndelivery = \"system\"\n";
        let list = values(&["working", "blocked"]);
        let updated = ConfigEdit::AgentGroupValues(&list).apply(content);
        assert_eq!(parsed_group_values(&updated), list);
        assert!(updated.contains("group_by = \"$stage\""));
        assert!(updated.contains("[ui.toast]\ndelivery = \"system\""));
        assert_eq!(updated.matches("group_values").count(), 1);
    }

    #[test]
    fn agent_group_values_edit_replaces_a_multi_line_list() {
        let content = "[ui.sidebar.agents]\ngroup_values = [\n  \"working\",\n  \"odd ] value\",\n]\nrow_gap = 1\n";
        let list = values(&["working", "odd ] value", "blocked"]);
        let updated = ConfigEdit::AgentGroupValues(&list).apply(content);
        assert_eq!(parsed_group_values(&updated), list);
        assert!(updated.contains("row_gap = 1"));
    }

    #[test]
    fn agent_group_values_edit_escapes_values() {
        let list = values(&["say \"hi\"", "back\\slash"]);
        let updated = ConfigEdit::AgentGroupValues(&list).apply("");
        assert_eq!(parsed_group_values(&updated), list);
    }
}
