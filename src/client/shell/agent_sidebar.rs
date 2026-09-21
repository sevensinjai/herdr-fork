use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
    /// Value of the `ui.sidebar.agents.group_by` token for this agent, when
    /// that setting is on and the agent reported the token.
    pub(super) group: Option<String>,
}

/// One line of the Agents panel: either a group heading or an agent.
pub(super) enum AgentListEntry {
    Header { label: String, count: usize },
    Agent(AgentRow),
}

/// Insert a heading wherever the group value changes between consecutive
/// rows. Rows arrive already partitioned by `apply_grouping`, so this only
/// marks the boundaries and never reorders.
pub(super) fn group_agent_rows(rows: Vec<AgentRow>, ungrouped_label: &str) -> Vec<AgentListEntry> {
    let mut entries: Vec<AgentListEntry> = Vec::new();
    let mut current: Option<Option<String>> = None;
    let mut header_index = 0usize;
    for row in rows {
        if current.as_ref() != Some(&row.group) {
            current = Some(row.group.clone());
            header_index = entries.len();
            entries.push(AgentListEntry::Header {
                label: row
                    .group
                    .clone()
                    .unwrap_or_else(|| ungrouped_label.to_string()),
                count: 0,
            });
        }
        if let Some(AgentListEntry::Header { count, .. }) = entries.get_mut(header_index) {
            *count += 1;
        }
        entries.push(AgentListEntry::Agent(row));
    }
    entries
}

/// Stable-partition pane ids by the value of `group_by`, preserving the
/// incoming order of both the groups and the agents inside them. Agents
/// missing the token move to the end.
///
/// Grouping is applied to the canonical order rather than at render time, so
/// the sidebar, Agent navigation, and indexed focus all agree on what "the
/// third agent" means.
fn apply_grouping(
    snapshot: &ClientShellSnapshot,
    pane_ids: Vec<String>,
    group_by: Option<&str>,
) -> Vec<String> {
    let Some(token) = group_by else {
        return pane_ids;
    };
    let mut order: Vec<Option<String>> = Vec::new();
    let mut buckets: Vec<Vec<String>> = Vec::new();
    for pane_id in pane_ids {
        let key = snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
            .and_then(|agent| {
                agent
                    .tokens
                    .iter()
                    .find(|(name, _)| name.as_str() == token)
                    .map(|(_, value)| value.clone())
            });
        match order.iter().position(|candidate| candidate == &key) {
            Some(index) => buckets[index].push(pane_id),
            None => {
                order.push(key);
                buckets.push(vec![pane_id]);
            }
        }
    }
    if let Some(index) = order.iter().position(Option::is_none) {
        order.remove(index);
        let bucket = buckets.remove(index);
        buckets.push(bucket);
    }
    buckets.into_iter().flatten().collect()
}

pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
    group_by: Option<&str>,
) -> Vec<String> {
    if snapshot.agent_view_label.is_some() {
        let ordered = snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str())
            })
            .cloned()
            .collect();
        return apply_grouping(snapshot, ordered, group_by);
    }
    let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
    if sort == crate::config::AgentPanelSortConfig::Priority {
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(status_priority(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
    }
    let ordered = agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect();
    apply_grouping(snapshot, ordered, group_by)
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let rows = agent_rows(snapshot, config, None);
    let empty_message = snapshot
        .agent_view_label
        .as_ref()
        .map(|_| " no matching agents");

    if config.agents.group_by.is_some() {
        let entries = group_agent_rows(rows, "ungrouped");
        render_agent_list(
            buffer,
            area,
            &entries,
            empty_message,
            config,
            agent_scroll,
            hits,
            |entry| match entry {
                AgentListEntry::Header { .. } => 1,
                AgentListEntry::Agent(row) => row.rows.len(),
            },
            |buffer, rect, entry, hits| match entry {
                // Headings are not agents: they register no hit region, so a
                // click on one cannot focus a pane.
                AgentListEntry::Header { label, count } => {
                    render_group_header(buffer, rect, label, *count, config);
                }
                AgentListEntry::Agent(row) => {
                    hits.agents.push((rect, row.pane_id.clone()));
                    render_agent_row(buffer, rect, row, config);
                }
            },
        );
        return;
    }

    render_agent_list(
        buffer,
        area,
        &rows,
        empty_message,
        config,
        agent_scroll,
        hits,
        |row| row.rows.len(),
        |buffer, rect, row, hits| {
            hits.agents.push((rect, row.pane_id.clone()));
            render_agent_row(buffer, rect, row, config);
        },
    );
}

/// Build a heading's text, truncating the label so at least `MIN_RULE` cells
/// are left for the trailing rule. Sidebars are narrow (default width 26) and
/// group labels are user-chosen, so without this a long label fills the line
/// and the heading reads as a wrapped agent row.
fn fit_group_label(label: &str, count: usize, width: usize) -> String {
    const MIN_RULE: usize = 3;
    let suffix = format!(" ({count}) ");
    let budget = width
        .saturating_sub(MIN_RULE)
        .saturating_sub(display_width(&suffix))
        .saturating_sub(1);
    if display_width(label) <= budget {
        return format!(" {label}{suffix}");
    }
    // Truncate by display width, not character count: CJK and other wide
    // characters occupy two cells each, so taking N chars can still overflow.
    let room = budget.saturating_sub(1);
    let mut shown = String::new();
    let mut used = 0usize;
    for character in label.chars() {
        let cell_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + cell_width > room {
            break;
        }
        shown.push(character);
        used += cell_width;
    }
    if !shown.is_empty() {
        shown.push('\u{2026}');
    }
    format!(" {shown}{suffix}")
}

/// Draw one group heading: the label, its agent count, and a rule filling the
/// rest of the line.
fn render_group_header(
    buffer: &mut Buffer,
    rect: Rect,
    label: &str,
    count: usize,
    config: &ClientShellConfig,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let palette = &config.palette;
    let text = fit_group_label(label, count, rect.width as usize);
    let text_width = display_width(&text).min(rect.width as usize);
    put_text(
        buffer,
        rect.x,
        rect.y,
        text_width as u16,
        &text,
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    let rule_x = rect.x.saturating_add(text_width as u16);
    let rule_width = rect.width.saturating_sub(text_width as u16);
    if rule_width > 0 {
        put_text(
            buffer,
            rule_x,
            rect.y,
            rule_width,
            &"\u{2500}".repeat(rule_width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let row_heights = rows
        .iter()
        .map(|row| row_lines(row).max(1).min(u16::MAX as usize) as u16)
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        render_row(buffer, rect, row, hits);
        y = y
            .saturating_add(height)
            .saturating_add(if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            });
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Vec<AgentRow> {
    ordered_agent_pane_ids(
        snapshot,
        config.agent_panel_sort,
        config.agents.group_by.as_deref(),
    )
    .into_iter()
    .filter_map(|pane_id| agent_row(snapshot, &pane_id, config, machine))
    .collect()
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        status: agent.agent_status,
        focused: agent.focused,
        rows,
        group: config
            .agents
            .group_by
            .as_ref()
            .and_then(|token| tokens.get(token))
            .cloned(),
    })
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 { 1 } else { 3 };
        let mut spans = vec![ratatui::text::Span::raw(" ".repeat(indent))];
        spans.extend(crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            rect.width.saturating_sub(indent as u16) as usize,
        ));
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;
    use crate::api::schema::AgentStatus;

    fn row(pane_id: &str, group: Option<&str>) -> AgentRow {
        AgentRow {
            pane_id: pane_id.to_string(),
            status: AgentStatus::Idle,
            focused: false,
            rows: Vec::new(),
            group: group.map(str::to_string),
        }
    }

    fn shape(entries: &[AgentListEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                AgentListEntry::Header { label, count } => format!("# {label} ({count})"),
                AgentListEntry::Agent(row) => format!("  {}", row.pane_id),
            })
            .collect()
    }

    #[test]
    fn marks_a_heading_at_each_run_boundary() {
        let entries = group_agent_rows(
            vec![
                row("p1", Some("waiting QA")),
                row("p2", Some("waiting QA")),
                row("p3", Some("iterating")),
            ],
            "ungrouped",
        );
        assert_eq!(
            shape(&entries),
            vec![
                "# waiting QA (2)",
                "  p1",
                "  p2",
                "# iterating (1)",
                "  p3"
            ]
        );
    }

    #[test]
    fn preserves_incoming_order_without_reordering() {
        // Ordering is apply_grouping's job. If the same value appears in two
        // runs, each run gets its own heading rather than being merged.
        let entries = group_agent_rows(
            vec![
                row("p1", Some("a")),
                row("p2", Some("b")),
                row("p3", Some("a")),
            ],
            "ungrouped",
        );
        assert_eq!(
            shape(&entries),
            vec!["# a (1)", "  p1", "# b (1)", "  p2", "# a (1)", "  p3"]
        );
    }

    #[test]
    fn agents_without_the_token_get_the_fallback_label() {
        let entries = group_agent_rows(vec![row("p1", None), row("p2", None)], "ungrouped");
        assert_eq!(shape(&entries), vec!["# ungrouped (2)", "  p1", "  p2"]);
    }

    #[test]
    fn no_agents_produces_no_headings() {
        assert!(group_agent_rows(Vec::new(), "ungrouped").is_empty());
    }

    #[test]
    fn grouping_never_drops_an_agent() {
        let rows = (0..25)
            .map(|index| {
                row(
                    &format!("p{index}"),
                    (index % 4 != 0).then(|| ["a", "b", "c"][index % 3]),
                )
            })
            .collect::<Vec<_>>();
        let kept = group_agent_rows(rows, "ungrouped")
            .into_iter()
            .filter(|entry| matches!(entry, AgentListEntry::Agent(_)))
            .count();
        assert_eq!(kept, 25);
    }

    #[test]
    fn long_labels_keep_room_for_the_rule() {
        // herdr's default sidebar_width is 26; these are realistic stage names.
        for width in [26usize, 34, 46] {
            let text = fit_group_label("waiting preview verification", 1, width);
            assert!(
                display_width(&text) + 3 <= width,
                "width {width}: {text:?} left no room for the rule"
            );
            assert!(
                text.contains("(1)"),
                "width {width}: count was truncated away"
            );
        }
    }

    #[test]
    fn wide_characters_are_truncated_by_display_width() {
        // CJK characters take two cells, so truncating by character count
        // would overflow the line even though the char count looks fine.
        for (label, width) in [("\u{5f85}\u{6a5f}\u{4e2d}\u{306e}\u{30d7}\u{30ec}\u{30d3}\u{30e5}\u{30fc}\u{691c}\u{8a3c}\u{5f8c}\u{306e}\u{78ba}\u{8a8d}", 26usize), ("\u{7b49}\u{5f85}\u{9884}\u{89c8}\u{9a8c}\u{8bc1}\u{5b8c}\u{6210}", 20)] {
            let text = fit_group_label(label, 3, width);
            assert!(
                display_width(&text) + 3 <= width,
                "width {width}: {text:?} overflowed the line"
            );
        }
    }

    #[test]
    fn short_labels_are_not_truncated() {
        assert_eq!(fit_group_label("iterating", 2, 46), " iterating (2) ");
    }

    #[test]
    fn absurdly_narrow_widths_do_not_panic() {
        for width in 0usize..12 {
            let _ = fit_group_label("waiting preview verification", 10, width);
        }
    }

    #[test]
    fn header_count_matches_agents_beneath_it() {
        let entries = group_agent_rows(
            vec![
                row("p1", Some("a")),
                row("p2", Some("a")),
                row("p3", Some("b")),
            ],
            "ungrouped",
        );
        for (index, entry) in entries.iter().enumerate() {
            if let AgentListEntry::Header { count, .. } = entry {
                let beneath = entries[index + 1..]
                    .iter()
                    .take_while(|entry| matches!(entry, AgentListEntry::Agent(_)))
                    .count();
                assert_eq!(beneath, *count);
            }
        }
    }
}
