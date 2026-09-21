//! Sidecar: two long-lived terminals (Notes and Chat) owned by the server
//! session and shown in a right-docked panel. Unlike popups they are not modal
//! and hiding the panel does not stop them.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tracing::warn;

use crate::api::schema::SidecarTab;
use crate::app::App;
use crate::layout::PaneId;
use crate::pane::PaneLaunchEnv;
use crate::terminal::{TerminalId, TerminalRuntime, TerminalState};

/// Environment variable holding the notes file path for the Notes command.
pub(crate) const SIDECAR_NOTES_ENV: &str = "HERDR_SIDECAR_NOTES";
/// Largest text `sidecar.send` accepts.
pub(crate) const MAX_SIDECAR_SEND_BYTES: usize = 64 * 1024;
/// Queued Chat text is pasted after this even if the agent never enables
/// bracketed paste.
pub(crate) const CHAT_PASTE_FALLBACK: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SidecarSlot {
    pub pane_id: PaneId,
    pub terminal_id: TerminalId,
}

/// Text sent to a Chat terminal that had not started yet.
#[derive(Clone, Debug)]
pub(crate) struct PendingChat {
    pub text: String,
    pub since: Instant,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SidecarState {
    pub notes: Option<SidecarSlot>,
    pub chat: Option<SidecarSlot>,
    pub pending_chat: Option<PendingChat>,
}

impl SidecarState {
    pub(crate) fn slot(&self, tab: SidecarTab) -> Option<&SidecarSlot> {
        match tab {
            SidecarTab::Notes => self.notes.as_ref(),
            SidecarTab::Chat => self.chat.as_ref(),
        }
    }

    fn slot_mut(&mut self, tab: SidecarTab) -> &mut Option<SidecarSlot> {
        match tab {
            SidecarTab::Notes => &mut self.notes,
            SidecarTab::Chat => &mut self.chat,
        }
    }

    pub(crate) fn slots(&self) -> impl Iterator<Item = (SidecarTab, &SidecarSlot)> {
        [
            (SidecarTab::Notes, self.notes.as_ref()),
            (SidecarTab::Chat, self.chat.as_ref()),
        ]
        .into_iter()
        .filter_map(|(tab, slot)| slot.map(|slot| (tab, slot)))
    }
}

/// `<config dir>/sidecar/<session>.md`. Kept outside the session data dir so
/// `herdr session delete` does not remove notes.
pub(crate) fn sidecar_notes_path() -> PathBuf {
    let session = crate::session::active_name()
        .unwrap_or_else(|| crate::session::DEFAULT_SESSION_NAME.to_string());
    crate::config::config_dir()
        .join("sidecar")
        .join(format!("{session}.md"))
}

fn open_notes_for_append(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Appends `text` as its own line, creating the file and its directory.
pub(crate) fn append_notes(path: &Path, text: &str) -> std::io::Result<()> {
    let mut file = open_notes_for_append(path)?;
    file.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    Ok(())
}

impl App {
    /// Starts the tab's terminal if it is not running and returns its id.
    pub(crate) fn sidecar_show(&mut self, tab: SidecarTab) -> std::io::Result<TerminalId> {
        if let Some(slot) = self.state.sidecar.slot(tab) {
            return Ok(slot.terminal_id.clone());
        }
        self.spawn_sidecar_slot(tab)
    }

    pub(crate) fn sidecar_tab_for_terminal(&self, terminal_id: &TerminalId) -> Option<SidecarTab> {
        self.state
            .sidecar
            .slots()
            .find(|(_, slot)| &slot.terminal_id == terminal_id)
            .map(|(tab, _)| tab)
    }

    /// Delivers text to a Sidecar tab. A running terminal gets a paste. A
    /// stopped Notes tab gets the text appended to its file before the editor
    /// opens it. A stopped Chat tab is started and the text is pasted once the
    /// agent is ready.
    pub(crate) fn sidecar_send(&mut self, tab: SidecarTab, text: String) -> std::io::Result<()> {
        if text.len() > MAX_SIDECAR_SEND_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("sidecar text must be at most {MAX_SIDECAR_SEND_BYTES} bytes"),
            ));
        }
        if let Some(slot) = self.state.sidecar.slot(tab).cloned() {
            return self.paste_into_sidecar(&slot.terminal_id, text);
        }
        match tab {
            SidecarTab::Notes => {
                append_notes(&sidecar_notes_path(), &text)?;
                self.spawn_sidecar_slot(SidecarTab::Notes)?;
            }
            SidecarTab::Chat => {
                self.spawn_sidecar_slot(SidecarTab::Chat)?;
                self.state.sidecar.pending_chat = Some(PendingChat {
                    text,
                    since: Instant::now(),
                });
                self.wake_render_after(CHAT_PASTE_FALLBACK);
            }
        }
        Ok(())
    }

    /// Pastes queued Chat text once the agent enables bracketed paste (its
    /// input is ready) or the fallback delay has passed. Called every render
    /// pass; cheap when nothing is queued.
    pub(crate) fn flush_pending_sidecar_chat(&mut self, now: Instant) {
        let Some(pending) = self.state.sidecar.pending_chat.as_ref() else {
            return;
        };
        let Some(slot) = self.state.sidecar.chat.clone() else {
            self.state.sidecar.pending_chat = None;
            return;
        };
        let ready = self
            .terminal_runtimes
            .get(&slot.terminal_id)
            .is_some_and(|runtime| runtime.bracketed_paste_enabled())
            || now.saturating_duration_since(pending.since) >= CHAT_PASTE_FALLBACK;
        if !ready {
            return;
        }
        if let Some(pending) = self.state.sidecar.pending_chat.take() {
            if let Err(error) = self.paste_into_sidecar(&slot.terminal_id, pending.text) {
                warn!(err = %error, "queued sidecar chat paste failed");
            }
        }
    }

    /// Clears the slot whose process exited. Returns false for other panes.
    pub(crate) fn handle_sidecar_pane_died(&mut self, pane_id: PaneId) -> bool {
        let Some(tab) = self
            .state
            .sidecar
            .slots()
            .find(|(_, slot)| slot.pane_id == pane_id)
            .map(|(tab, _)| tab)
        else {
            return false;
        };
        if let Some(slot) = self.state.sidecar.slot_mut(tab).take() {
            self.state
                .direct_attach_resize_locks
                .remove(&slot.terminal_id);
            self.state.terminals.remove(&slot.terminal_id);
            self.shutdown_terminal_runtime(slot.terminal_id);
        }
        if tab == SidecarTab::Chat {
            self.state.sidecar.pending_chat = None;
        }
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
        true
    }

    fn paste_into_sidecar(&self, terminal_id: &TerminalId, text: String) -> std::io::Result<()> {
        let runtime = self
            .terminal_runtimes
            .get(terminal_id)
            .ok_or_else(|| std::io::Error::other("sidecar terminal is not running"))?;
        runtime
            .try_send_paste(text)
            .map_err(|_| std::io::Error::other("sidecar terminal is not accepting input"))
    }

    fn spawn_sidecar_slot(&mut self, tab: SidecarTab) -> std::io::Result<TerminalId> {
        let config = self.state.sidecar_config.clone();
        let (command, extra_env) = match tab {
            SidecarTab::Notes => {
                let path = sidecar_notes_path();
                open_notes_for_append(&path)?;
                (
                    config.notes_command,
                    vec![(SIDECAR_NOTES_ENV.to_string(), path.display().to_string())],
                )
            }
            SidecarTab::Chat => (config.chat_command, Vec::new()),
        };
        let cwd = self.sidecar_cwd();
        let (rows, cols) = self.sidecar_initial_size(config.width);
        let pane_id = PaneId::alloc();
        let terminal_id = TerminalId::alloc();
        let launch_env = PaneLaunchEnv::from_extra(extra_env).without_pane_identity();
        let runtime = TerminalRuntime::spawn_shell_command(
            pane_id,
            rows,
            cols,
            cwd.clone(),
            &command,
            &launch_env,
            crate::pane::AgentDetection::Disabled,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            self.state.host_terminal_appearance,
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        )?;
        self.install_sidecar_runtime(tab, pane_id, terminal_id.clone(), runtime, cwd);
        Ok(terminal_id)
    }

    pub(crate) fn install_sidecar_runtime(
        &mut self,
        tab: SidecarTab,
        pane_id: PaneId,
        terminal_id: TerminalId,
        runtime: TerminalRuntime,
        cwd: PathBuf,
    ) {
        self.terminal_runtimes.insert(terminal_id.clone(), runtime);
        self.state.terminals.insert(
            terminal_id.clone(),
            TerminalState::new(terminal_id.clone(), cwd),
        );
        *self.state.sidecar.slot_mut(tab) = Some(SidecarSlot {
            pane_id,
            terminal_id,
        });
        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }

    /// The focused pane's directory, else the server's.
    fn sidecar_cwd(&self) -> PathBuf {
        self.state
            .active
            .and_then(|index| self.state.workspaces.get(index))
            .and_then(|workspace| {
                let tab = workspace.active_tab()?;
                let pane = workspace.focused_pane_id()?;
                tab.cwd_for_pane(pane, &self.state.terminals, &self.terminal_runtimes)
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// Starting size before any client reports its Sidecar view.
    fn sidecar_initial_size(&self, width: crate::popup_size::PopupSize) -> (u16, u16) {
        let area = if self.state.view.terminal_area.width >= 4
            && self.state.view.terminal_area.height >= 4
        {
            self.state.view.terminal_area
        } else {
            let (rows, cols) = self.state.estimate_pane_size();
            ratatui::layout::Rect::new(0, 0, cols, rows)
        };
        crate::popup_size::resolve_right_docked_geometry(width, area)
            .map(|geometry| (geometry.inner.height, geometry.inner.width))
            .unwrap_or((area.height.max(1), area.width.max(1)))
    }

    /// Requests one render pass after `delay` so queued Sidecar work is
    /// flushed even when no terminal output arrives.
    fn wake_render_after(&self, delay: Duration) {
        let render_dirty = self.render_dirty.clone();
        let render_notify = self.render_notify.clone();
        let spawned = std::thread::Builder::new()
            .name("herdr-sidecar-wake".into())
            .spawn(move || {
                std::thread::sleep(delay);
                render_dirty.request_generic();
                render_notify.notify_one();
            });
        if let Err(error) = spawned {
            warn!(err = %error, "failed to schedule sidecar wake-up");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::SidecarTab;

    fn test_app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("sidecar")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app
    }

    fn install(
        app: &mut App,
        tab: SidecarTab,
    ) -> (SidecarSlot, tokio::sync::mpsc::Receiver<bytes::Bytes>) {
        let (runtime, rx) = TerminalRuntime::test_with_channel(40, 10);
        let pane_id = PaneId::alloc();
        let terminal_id = TerminalId::alloc();
        app.install_sidecar_runtime(
            tab,
            pane_id,
            terminal_id.clone(),
            runtime,
            PathBuf::from("/sidecar"),
        );
        (
            SidecarSlot {
                pane_id,
                terminal_id,
            },
            rx,
        )
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("herdr-sidecar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[tokio::test]
    async fn api_rejects_oversized_sidecar_text() {
        let mut app = test_app();
        let (_chat, _rx) = install(&mut app, SidecarTab::Chat);
        let response = app.handle_api_request(crate::api::schema::Request {
            id: "big".into(),
            method: crate::api::schema::Method::SidecarSend(
                crate::api::schema::SidecarSendParams {
                    tab: SidecarTab::Chat,
                    text: "x".repeat(MAX_SIDECAR_SEND_BYTES + 1),
                },
            ),
        });
        let value: serde_json::Value = serde_json::from_str(&response).expect("json");
        assert_eq!(value["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn api_show_returns_the_running_terminal() {
        let mut app = test_app();
        let (notes, _rx) = install(&mut app, SidecarTab::Notes);
        let response = app.handle_api_request(crate::api::schema::Request {
            id: "show".into(),
            method: crate::api::schema::Method::SidecarShow(
                crate::api::schema::SidecarShowParams {
                    tab: SidecarTab::Notes,
                },
            ),
        });
        let value: serde_json::Value = serde_json::from_str(&response).expect("json");
        assert_eq!(value["result"]["type"], "sidecar_shown");
        assert_eq!(
            value["result"]["terminal_id"],
            notes.terminal_id.to_string()
        );
    }

    #[test]
    fn notes_path_is_per_session_under_config_dir() {
        let dir = temp_dir("path");
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        std::env::remove_var(crate::session::SESSION_ENV_VAR);
        assert!(sidecar_notes_path().ends_with("sidecar/default.md"));
        assert!(sidecar_notes_path().starts_with(&dir));
        std::env::set_var(crate::session::SESSION_ENV_VAR, "work");
        assert!(sidecar_notes_path().ends_with("sidecar/work.md"));
    }

    #[test]
    fn append_notes_creates_the_file_and_keeps_order() {
        let path = temp_dir("append").join("sidecar").join("default.md");
        append_notes(&path, "first").expect("append");
        append_notes(&path, "second\n").expect("append");
        assert_eq!(
            std::fs::read_to_string(&path).expect("notes"),
            "first\nsecond\n"
        );
    }

    #[tokio::test]
    async fn pane_died_clears_only_that_slot() {
        let mut app = test_app();
        let (notes, _notes_rx) = install(&mut app, SidecarTab::Notes);
        let (chat, _chat_rx) = install(&mut app, SidecarTab::Chat);

        assert!(app.handle_sidecar_pane_died(notes.pane_id));
        assert!(app.state.sidecar.notes.is_none());
        assert!(!app.state.terminals.contains_key(&notes.terminal_id));
        assert_eq!(app.state.sidecar.chat, Some(chat));
        assert!(!app.handle_sidecar_pane_died(PaneId::alloc()));
    }

    #[tokio::test]
    async fn show_reuses_a_running_slot() {
        let mut app = test_app();
        let (notes, _rx) = install(&mut app, SidecarTab::Notes);
        assert_eq!(
            app.sidecar_show(SidecarTab::Notes).expect("show"),
            notes.terminal_id
        );
        assert_eq!(
            app.sidecar_tab_for_terminal(&notes.terminal_id),
            Some(SidecarTab::Notes)
        );
    }

    #[tokio::test]
    async fn send_to_a_running_slot_pastes() {
        let mut app = test_app();
        let (_chat, mut rx) = install(&mut app, SidecarTab::Chat);
        app.sidecar_send(SidecarTab::Chat, "hello".into())
            .expect("send");
        assert_eq!(rx.try_recv().expect("pasted").as_ref(), b"hello");
    }

    #[test]
    fn send_rejects_oversized_text() {
        let mut app = test_app();
        let error = app
            .sidecar_send(SidecarTab::Notes, "x".repeat(MAX_SIDECAR_SEND_BYTES + 1))
            .expect_err("too big");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn queued_chat_text_waits_for_readiness_or_timeout() {
        let mut app = test_app();
        let (_chat, mut rx) = install(&mut app, SidecarTab::Chat);
        let start = Instant::now();
        app.state.sidecar.pending_chat = Some(PendingChat {
            text: "later".into(),
            since: start,
        });

        app.flush_pending_sidecar_chat(start + Duration::from_millis(100));
        assert!(rx.try_recv().is_err(), "not ready yet");

        app.flush_pending_sidecar_chat(start + CHAT_PASTE_FALLBACK);
        assert_eq!(rx.try_recv().expect("pasted").as_ref(), b"later");
        assert!(app.state.sidecar.pending_chat.is_none());
    }
}
