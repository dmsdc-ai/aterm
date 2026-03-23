mod core;
mod ime;
mod terminal;
mod ui;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant};

use iced::widget::{column, container, row, stack, text, Space};
use iced::{
    event, keyboard, time, Alignment, Background, Border, Element, Fill, Result,
    Subscription, Task,
};

use crate::core::{
    normalize_terminal_text, PtyOutputSignal, SessionStore, SharedPtyManager,
    TeleptyClient, TeleptySessionInfo, WorkspaceInfo,
};
use crate::ime::{CandidateRect, ImeBridge, Rect, TextRange};
use crate::terminal::{TerminalEvent, TerminalState, TerminalWidget};
use crate::ui::{
    CommandEntry, CommandPalette, CommandPaletteAction, CommandPaletteState,
    CreateSessionAction, CreateSessionDialog, CreateSessionState, CLI_PRESETS,
    DeliberateAction, DeliberateDialog, DeliberateDialogState,
    GroupEntry, GroupGrid, GroupGridAction, GroupSummaryEntry, HybridPhase,
    Palette, PaletteCommand, SessionEntry, SessionKind, SessionStatus,
    SettingsAction, SettingsPanel, SettingsState,
    Sidebar, SidebarAction, SidebarModel, ThemeMode,
};

const SESSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_COLUMNS: u16 = 120;
const DEFAULT_ROWS: u16 = 36;
const PTY_OUTPUT_DEBOUNCE: Duration = Duration::from_millis(16);

static PTY_SIGNAL: OnceLock<PtyOutputSignal> = OnceLock::new();
static PTY_DISPATCH_IN_FLIGHT: LazyLock<AtomicBool> =
    LazyLock::new(|| AtomicBool::new(false));

fn app_title() -> &'static str {
    let hash = env!("ATERM_GIT_HASH");
    let date = env!("ATERM_BUILD_DATE");
    let dirty = env!("ATERM_DIRTY") == "true";
    let version = env!("CARGO_PKG_VERSION");
    let build = env!("ATERM_BUILD_NUMBER");
    let dirty_mark = if dirty { "*" } else { "" };
    let s = format!("aterm v3 \u{2014} {}-{}{} build.{} ({})", version, hash, dirty_mark, build, date);
    Box::leak(s.into_boxed_str())
}

static APP_TITLE: LazyLock<&'static str> = LazyLock::new(app_title);

fn main() -> Result {
    iced::application(Aterm::boot, update, view)
        .title(|_state: &_| (*APP_TITLE).to_string())
        .default_font(iced::Font {
            family: iced::font::Family::SansSerif,
            ..iced::Font::DEFAULT
        })
        .theme(|app: &Aterm| Palette::iced_theme(app.theme_mode))
        .style(|app: &Aterm, _theme: &iced::Theme| iced::theme::Style {
            background_color: app.palette().background,
            text_color: app.palette().text,
        })
        .subscription(subscription)
        .run()
}

struct Aterm {
    manager: SharedPtyManager,
    session_store: SessionStore,
    telepty: TeleptyClient,
    ime: ImeBridge,
    terminal: TerminalState,
    theme_mode: ThemeMode,
    palette_state: CommandPaletteState,
    commands: Vec<CommandEntry<'static>>,
    current_view: CurrentView,
    active_workspace: String,
    group_view: Option<GroupViewState>,
    local_sessions: Vec<SessionEntry<'static>>,
    telepty_sessions: Vec<SessionEntry<'static>>,
    groups: Vec<GroupEntry<'static>>,
    create_session_state: CreateSessionState,
    deliberate_state: DeliberateDialogState,
    settings_state: SettingsState,
    status_text: String,
}

#[derive(Debug, Clone)]
enum Message {
    Tick(Instant),
    PtyDataReady,
    Event(iced::Event),
    FontLoaded(String, std::result::Result<(), iced::font::Error>),
    Sidebar(SidebarAction),
    Palette(CommandPaletteAction),
    Terminal(TerminalEvent),
    GroupGrid(GroupGridAction),
    GroupTerminal(String, TerminalEvent),
    CreateSession(CreateSessionAction),
    FolderSelected(Option<std::path::PathBuf>),
    Deliberate(DeliberateAction),
    Settings(SettingsAction),
    /// Internal routing: deliver text to a workspace without going through telepty.
    RouteToWorkspace { workspace_id: String, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CurrentView {
    Session,
    Group(String),
}

struct GroupTerminalPane {
    workspace_id: String,
    title: String,
    subtitle: String,
    status: String,
    terminal: TerminalState,
}

struct GroupViewState {
    id: String,
    title: String,
    topic: String,
    phase: HybridPhase,
    members: Vec<GroupTerminalPane>,
    summary: Vec<GroupSummaryEntry<'static>>,
}

impl Aterm {
    fn boot() -> (Self, Task<Message>) {
        let manager = core::PtyManager::shared();
        let session_store = SessionStore::new();
        let telepty = TeleptyClient::new();
        let ime = ImeBridge::new();

        let _ = session_store.restore_into(&manager);

        let _ = PTY_SIGNAL.set(
            manager
                .lock()
                .map(|m| m.output_signal())
                .unwrap_or_else(|_| PtyOutputSignal::new()),
        );

        let mut app = Self {
            manager,
            session_store,
            telepty,
            ime,
            terminal: TerminalState::new(DEFAULT_COLUMNS as usize, DEFAULT_ROWS as usize),
            theme_mode: load_theme(),
            palette_state: CommandPaletteState::default(),
            commands: default_commands(),
            current_view: CurrentView::Session,
            active_workspace: String::new(),
            group_view: None,
            local_sessions: Vec::new(),
            telepty_sessions: Vec::new(),
            groups: Vec::new(),
            create_session_state: CreateSessionState::default(),
            deliberate_state: DeliberateDialogState::default(),
            settings_state: SettingsState::default(),
            status_text: "Ready".to_string(),
        };

        #[cfg(target_os = "macos")]
        {
            crate::ime::NativeImeHandler::initialize();
            eprintln!("[NATIVE-IME] handler initialized");
        }

        app.ensure_default_workspace();
        app.refresh_sessions();
        app.refresh_active_terminal();

        let font_tasks = system_cjk_font_tasks();
        let startup_task = if font_tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(font_tasks)
        };

        (app, startup_task)
    }

    fn palette(&self) -> Palette {
        Palette::from_mode(self.theme_mode)
    }

    fn ensure_default_workspace(&mut self) {
        let has_local = self
            .manager
            .lock()
            .map(|manager| !manager.list_workspaces().is_empty())
            .unwrap_or(false);

        if has_local {
            if self.active_workspace.is_empty() {
                if let Ok(manager) = self.manager.lock() {
                    if let Some(first) = manager.list_workspaces().into_iter().next() {
                        self.active_workspace = first.id;
                    }
                }
            }
            return;
        }

        let cwd = current_dir_string();
        let id = "main".to_string();

        if let Ok(mut manager) = self.manager.lock() {
            if let Err(error) = manager.create(
                id.clone(),
                cwd,
                None,
                None,
                Some(DEFAULT_COLUMNS),
                Some(DEFAULT_ROWS),
                false,
            ) {
                self.status_text = format!("Failed to create default session: {error}");
                return;
            }
        }

        self.active_workspace = id;
        let _ = self.session_store.save_shared(&self.manager);
    }

    fn refresh_sessions(&mut self) {
        let workspaces = self
            .manager
            .lock()
            .map(|manager| manager.list_workspaces())
            .unwrap_or_default();

        if self.active_workspace.is_empty() {
            if let Some(first) = workspaces.first() {
                self.active_workspace = first.id.clone();
            }
        }

        self.local_sessions = workspaces
            .iter()
            .map(|workspace| {
                let pending = self.manager.lock().ok()
                    .and_then(|m| m.peek_queue(&workspace.id).ok())
                    .map(|q| q.len())
                    .unwrap_or(0);
                map_local_session_with_injects(
                    workspace,
                    matches!(self.current_view, CurrentView::Session)
                        && workspace.id == self.active_workspace,
                    pending,
                )
            })
            .collect();

        // Sort: active (non-dead) first, then by created_at descending
        self.local_sessions.sort_by(|a, b| {
            let a_dead = matches!(a.status, SessionStatus::Dead);
            let b_dead = matches!(b.status, SessionStatus::Dead);
            a_dead.cmp(&b_dead).then_with(|| b.id.cmp(&a.id))
        });

        self.telepty_sessions = self
            .telepty
            .list_sessions()
            .unwrap_or_default()
            .iter()
            .map(|session| {
                let attach_id = format!("attach:{}", session.id);
                map_telepty_session(
                    session,
                    matches!(self.current_view, CurrentView::Session)
                        && self.active_workspace == attach_id,
                )
            })
            .collect();

        let total_sessions = self.local_sessions.len() + self.telepty_sessions.len();
        self.groups = vec![GroupEntry {
            id: Cow::Borrowed("all"),
            title: Cow::Borrowed("All Sessions"),
            subtitle: Cow::Owned(format!("{total_sessions} sessions available")),
            members: total_sessions,
            active: matches!(&self.current_view, CurrentView::Group(id) if id == "all"),
        }];
    }

    fn refresh_active_terminal(&mut self) {
        if self.active_workspace.is_empty() {
            self.terminal.sync_snapshot("");
            return;
        }

        let snapshot = self
            .manager
            .lock()
            .ok()
            .and_then(|manager| manager.read_screen(&self.active_workspace, None).ok())
            .unwrap_or_default();

        self.terminal.sync_snapshot(&snapshot);
    }

    fn open_group_view(&mut self, id: &str) {
        let title = self
            .groups
            .iter()
            .find(|group| group.id.as_ref() == id)
            .map(|group| group.title.to_string())
            .unwrap_or_else(|| id.to_string());

        self.current_view = CurrentView::Group(id.to_string());
        self.group_view = Some(GroupViewState {
            id: id.to_string(),
            title,
            topic: String::new(),
            phase: HybridPhase::Divergence,
            members: Vec::new(),
            summary: Vec::new(),
        });
        self.sync_group_view();
    }

    fn sync_group_view(&mut self) {
        let Some(group_view) = self.group_view.as_mut() else {
            return;
        };

        let workspaces = self
            .manager
            .lock()
            .map(|manager| manager.list_workspaces())
            .unwrap_or_default();

        let mut next_members = Vec::new();
        let mut existing = std::mem::take(&mut group_view.members);

        for workspace in workspaces {
            let snapshot = self
                .manager
                .lock()
                .ok()
                .and_then(|manager| manager.read_screen(&workspace.id, None).ok())
                .unwrap_or_default();

            if let Some(index) = existing
                .iter()
                .position(|member| member.workspace_id == workspace.id)
            {
                let mut member = existing.swap_remove(index);
                member.title = workspace.id.clone();
                member.subtitle = workspace.cwd.clone();
                member.status = workspace.status.clone();
                member.terminal.sync_snapshot(&snapshot);
                next_members.push(member);
            } else {
                let mut terminal = TerminalState::new(
                    DEFAULT_COLUMNS as usize,
                    DEFAULT_ROWS as usize,
                );
                terminal.sync_snapshot(&snapshot);
                next_members.push(GroupTerminalPane {
                    workspace_id: workspace.id.clone(),
                    title: workspace.id.clone(),
                    subtitle: workspace.cwd.clone(),
                    status: workspace.status.clone(),
                    terminal,
                });
            }
        }

        group_view.members = next_members;
    }

    fn summarize_group(&mut self) {
        let Some(group_view) = self.group_view.as_mut() else {
            return;
        };

        group_view.phase = HybridPhase::Convergence;
        group_view.summary = group_view
            .members
            .iter()
            .map(|member| {
                let snapshot = self
                    .manager
                    .lock()
                    .ok()
                    .and_then(|manager| manager.read_screen(&member.workspace_id, Some(16 * 1024)).ok())
                    .unwrap_or_default();
                GroupSummaryEntry {
                    id: Cow::Owned(member.workspace_id.clone()),
                    title: Cow::Owned(member.title.clone()),
                    summary: Cow::Owned(compact_terminal_summary(&snapshot)),
                }
            })
            .collect();

        self.status_text =
            "Convergence summary generated from current group session screens".to_string();
    }

    fn next_workspace_id(&self) -> String {
        let existing = self
            .manager
            .lock()
            .map(|manager| {
                manager
                    .list_workspaces()
                    .into_iter()
                    .map(|workspace| workspace.id)
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_else(|_| {
                self.local_sessions
                    .iter()
                    .map(|entry| entry.id.to_string())
                    .collect()
            });

        let mut index = 1usize;
        loop {
            let candidate = format!("session-{index}");
            if !existing.contains(&candidate) {
                return candidate;
            }
            index += 1;
        }
    }

    fn create_workspace(&mut self) {
        let previous_workspace = self.active_workspace.clone();
        let next_id = self.next_workspace_id();
        let result = self
            .manager
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut manager| {
                manager.create(
                    next_id.clone(),
                    current_dir_string(),
                    None,
                    None,
                    Some(DEFAULT_COLUMNS),
                    Some(DEFAULT_ROWS),
                    false,
                )
            });

        match result {
            Ok(_) => {
                if previous_workspace.starts_with("attach:") && previous_workspace != next_id {
                    if let Ok(mut manager) = self.manager.lock() {
                        let _ = manager.close(&previous_workspace);
                    }
                }

                self.current_view = CurrentView::Session;
                self.group_view = None;
                self.active_workspace = next_id;
                self.status_text = "Created a new local session".to_string();
                let _ = self.session_store.save_shared(&self.manager);
            }
            Err(error) => {
                self.status_text = format!("Create session failed: {error}");
            }
        }

        self.refresh_sessions();
        self.refresh_active_terminal();
    }

    fn handle_sidebar(&mut self, action: SidebarAction) -> Task<Message> {
        match action {
            SidebarAction::SelectSession(id) => {
                let previous_workspace = self.active_workspace.clone();

                if self.local_sessions.iter().any(|entry| entry.id.as_ref() == id) {
                    self.current_view = CurrentView::Session;
                    self.group_view = None;
                    self.active_workspace = id;
                    self.status_text = "Selected local session".to_string();
                } else if self.telepty_sessions.iter().any(|entry| entry.id.as_ref() == id) {
                    let attach_id = format!("attach:{}", id);

                    let exists = match self.manager.lock() {
                        Ok(mgr) => mgr.list_workspaces().iter().any(|ws| ws.id == attach_id),
                        Err(_) => false,
                    };

                    if !exists {
                        if let Ok(mut manager) = self.manager.lock() {
                            let args = vec!["attach".to_string(), id.clone()];
                            let _ = manager.create(
                                attach_id.clone(),
                                std::env::current_dir().unwrap_or_default().to_string_lossy().to_string(),
                                Some("telepty".to_string()),
                                Some(args),
                                None,
                                None,
                                true,
                            );
                        }
                    }

                    self.current_view = CurrentView::Session;
                    self.group_view = None;
                    self.active_workspace = attach_id;
                    self.status_text = format!("Attached to telepty session {}", id);
                } else if id.starts_with("attach:") {
                    self.current_view = CurrentView::Session;
                    self.group_view = None;
                    self.active_workspace = id.clone();
                    self.status_text = "Selected attached session".to_string();
                } else {
                    self.status_text = format!("Session not found: {}", id);
                }

                if previous_workspace.starts_with("attach:") && previous_workspace != self.active_workspace {
                    if let Ok(mut manager) = self.manager.lock() {
                        let _ = manager.close(&previous_workspace);
                    }
                }

                self.refresh_sessions();
                self.refresh_active_terminal();
            }
            SidebarAction::SelectGroup(id) => {
                self.open_group_view(&id);
                self.refresh_sessions();
                self.status_text = format!("Selected group: {id}");
            }
            SidebarAction::CreateSession => {
                return iced::Task::future(async {
                    let folder = rfd::AsyncFileDialog::new()
                        .set_title("Select project folder")
                        .pick_folder()
                        .await;
                    Message::FolderSelected(folder.map(|f| f.path().to_path_buf()))
                });
            }
            SidebarAction::DeleteSession(id) => {
                if let Ok(mut manager) = self.manager.lock() {
                    let _ = manager.close(&id);
                }
                if self.active_workspace == id {
                    self.active_workspace = self.local_sessions.iter()
                        .find(|s| s.id.as_ref() != id)
                        .map(|s| s.id.to_string())
                        .unwrap_or_default();
                }
                self.refresh_sessions();
                self.refresh_active_terminal();
                let _ = self.session_store.save_shared(&self.manager);
                self.status_text = format!("Closed session: {id}");
            }
            SidebarAction::OpenSettings => {
                self.settings_state.open = !self.settings_state.open;
            }
        }

        Task::none()
    }

    fn handle_palette(&mut self, action: CommandPaletteAction) {
        match action {
            CommandPaletteAction::Close => {
                self.palette_state.open = false;
            }
            CommandPaletteAction::QueryChanged(query) => {
                self.palette_state.query = query;
                self.palette_state.selected = 0;
            }
            CommandPaletteAction::Execute(command) => {
                self.execute_command(command);
            }
            CommandPaletteAction::Submit => {
                let palette = CommandPalette::new(self.palette());
                if let Some(entry) =
                    palette.selected_command(&self.palette_state, &self.commands)
                {
                    self.execute_command(entry.command);
                }
            }
            CommandPaletteAction::Select(index) => {
                self.palette_state.selected = index;
            }
        }
    }

    fn execute_command(&mut self, command: PaletteCommand) {
        match command {
            PaletteCommand::NewSession => {
                self.create_workspace();
            }
            PaletteCommand::Deliberate => {
                self.deliberate_state.open = true;
                self.deliberate_state.topic.clear();
            }
            PaletteCommand::Group => {
                self.status_text = "group: UI scaffold ready, creation flow pending".to_string();
            }
            PaletteCommand::Broadcast => {
                let result = self
                    .manager
                    .lock()
                    .ok()
                    .and_then(|manager| {
                        manager
                            .queue_inject(
                                &self.active_workspace,
                                "command-palette",
                                "broadcast placeholder".to_string(),
                            )
                            .ok()
                    });

                self.status_text = if result.is_some() {
                    "broadcast: queued inject into the active workspace".to_string()
                } else {
                    "broadcast: no active local workspace to inject into".to_string()
                };
            }
            PaletteCommand::Theme(mode) => {
                self.theme_mode = mode;
                save_theme(mode);
                self.status_text = match mode {
                    ThemeMode::Light => "Theme switched to light".to_string(),
                    ThemeMode::Dark => "Theme switched to dark".to_string(),
                };
            }
        }

        self.palette_state.open = false;
    }

    fn handle_terminal(&mut self, event: TerminalEvent) {
        match event {
            TerminalEvent::Input(bytes) => {
                if let Ok(text) = String::from_utf8(bytes) {
                    let result = self
                        .manager
                        .lock()
                        .ok()
                        .and_then(|manager| manager.send_to_workspace(&self.active_workspace, &text).ok());
                    if result.is_none() {
                        self.status_text = "Failed to forward terminal input".to_string();
                    }
                }
            }
            TerminalEvent::Resize { columns, rows } => {
                eprintln!("[RESIZE] terminal {}x{} for workspace '{}'", columns, rows, self.active_workspace);
                self.terminal.resize(columns as usize, rows as usize);
                match self.manager.lock() {
                    Ok(manager) => {
                        match manager.resize(&self.active_workspace, columns, rows) {
                            Ok(()) => eprintln!("[RESIZE] PTY resize OK: {}x{}", columns, rows),
                            Err(e) => eprintln!("[RESIZE] PTY resize FAILED: {}", e),
                        }
                    }
                    Err(e) => eprintln!("[RESIZE] manager lock FAILED: {}", e),
                }

                self.ime.set_candidate_rect(CandidateRect {
                    rect: Rect::new(360.0, 88.0, 0.0, 24.0),
                    actual_range: TextRange::empty(0),
                });
            }
            TerminalEvent::FocusChanged(focused) => {
                self.status_text = if focused {
                    "Terminal focused".to_string()
                } else {
                    "Terminal unfocused".to_string()
                };
            }
            TerminalEvent::Scroll(delta) => {
                self.terminal.scroll(delta);
            }
        }
    }

    fn handle_group_terminal(&mut self, workspace_id: String, event: TerminalEvent) {
        match event {
            TerminalEvent::Input(bytes) => {
                if let Ok(text) = String::from_utf8(bytes) {
                    let result = self
                        .manager
                        .lock()
                        .ok()
                        .and_then(|manager| manager.send_to_workspace(&workspace_id, &text).ok());
                    if result.is_none() {
                        self.status_text =
                            format!("Failed to forward input to group session {workspace_id}");
                    }
                }
            }
            TerminalEvent::Resize { columns, rows } => {
                if let Some(group_view) = self.group_view.as_mut() {
                    if let Some(member) = group_view
                        .members
                        .iter_mut()
                        .find(|member| member.workspace_id == workspace_id)
                    {
                        member.terminal.resize(columns as usize, rows as usize);
                    }
                }

                let _ = self
                    .manager
                    .lock()
                    .ok()
                    .and_then(|manager| manager.resize(&workspace_id, columns, rows).ok());
            }
            TerminalEvent::FocusChanged(focused) => {
                self.status_text = if focused {
                    format!("Focused group session {workspace_id}")
                } else {
                    format!("Unfocused group session {workspace_id}")
                };
            }
            TerminalEvent::Scroll(delta) => {
                if let Some(group_view) = self.group_view.as_mut() {
                    if let Some(member) = group_view
                        .members
                        .iter_mut()
                        .find(|member| member.workspace_id == workspace_id)
                    {
                        member.terminal.scroll(delta);
                    }
                }
            }
        }
    }

    fn handle_group_grid(&mut self, action: GroupGridAction) {
        let Some(group_view) = self.group_view.as_mut() else {
            return;
        };

        match action {
            GroupGridAction::TopicChanged(topic) => {
                group_view.topic = topic;
            }
            GroupGridAction::Converge => {
                self.summarize_group();
            }
            GroupGridAction::Broadcast(topic) => {
                if let Some(gv) = self.group_view.as_ref() {
                    let ws_ids: Vec<String> = gv.members.iter().map(|m| m.workspace_id.clone()).collect();
                    if let Ok(manager) = self.manager.lock() {
                        for ws_id in &ws_ids {
                            let _ = manager.queue_inject(ws_id, "broadcast", topic.clone());
                        }
                    }
                    self.status_text = format!("Broadcast to {} sessions", ws_ids.len());
                }
            }
        }
    }

    fn handle_event(&mut self, event: iced::Event) {
        if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key, modifiers, ..
        }) = event
        {
            match key.as_ref() {
                keyboard::Key::Character("k") if modifiers.command() => {
                    self.palette_state.open = !self.palette_state.open;
                    if self.palette_state.open {
                        self.palette_state.query.clear();
                        self.palette_state.selected = 0;
                    }
                }
                keyboard::Key::Named(keyboard::key::Named::Escape)
                    if self.create_session_state.open =>
                {
                    self.create_session_state.open = false;
                }
                keyboard::Key::Named(keyboard::key::Named::Escape)
                    if self.palette_state.open =>
                {
                    self.palette_state.open = false;
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowDown)
                    if self.palette_state.open =>
                {
                    let len = CommandPalette::new(self.palette())
                        .filtered_commands(&self.palette_state, &self.commands)
                        .len();
                    if len > 0 {
                        self.palette_state.selected =
                            (self.palette_state.selected + 1).min(len - 1);
                    }
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowUp)
                    if self.palette_state.open =>
                {
                    self.palette_state.selected =
                        self.palette_state.selected.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    fn header_text(&self) -> String {
        let active = self
            .local_sessions
            .iter()
            .find(|entry| entry.id.as_ref() == self.active_workspace)
            .map(|entry| entry.title.as_ref())
            .unwrap_or("No session");

        format!("aterm   {}   {}", active, self.status_text)
    }
}

fn pty_output_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(1, |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
        use iced::futures::SinkExt;
        let Some(signal) = PTY_SIGNAL.get().cloned() else {
            return;
        };
        loop {
            signal.notified().await;
            tokio::time::sleep(PTY_OUTPUT_DEBOUNCE).await;

            if PTY_DISPATCH_IN_FLIGHT.load(Ordering::Acquire) {
                continue;
            }

            if !signal.take_dirty() {
                continue;
            }

            PTY_DISPATCH_IN_FLIGHT.store(true, Ordering::Release);

            if sender.send(Message::PtyDataReady).await.is_err() {
                break;
            }
        }
    })
}

fn subscription(_app: &Aterm) -> Subscription<Message> {
    Subscription::batch([
        Subscription::run(pty_output_stream),
        time::every(SESSION_REFRESH_INTERVAL).map(Message::Tick),
        event::listen().map(Message::Event),
    ])
}

fn update(app: &mut Aterm, message: Message) -> Task<Message> {
    match message {
        Message::PtyDataReady => {
            match app.current_view {
                CurrentView::Session => app.refresh_active_terminal(),
                CurrentView::Group(_) => app.sync_group_view(),
            }

            PTY_DISPATCH_IN_FLIGHT.store(false, Ordering::Release);
            if let Some(signal) = PTY_SIGNAL.get() {
                if signal.has_dirty() {
                    signal.poke();
                }
            }
        }
        Message::Tick(_now) => {
            eprintln!("[TICK] session refresh");
            app.refresh_sessions();
        }
        Message::Event(event) => app.handle_event(event),
        Message::FontLoaded(label, result) => {
            eprintln!("[FONT] loaded: {} ok={}", label, result.is_ok());
            if result.is_err() {
                app.status_text = format!("Failed to load CJK font: {label}");
            }
        }
        Message::Sidebar(action) => {
            return app.handle_sidebar(action);
        }
        Message::Palette(action) => app.handle_palette(action),
        Message::Terminal(event) => app.handle_terminal(event),
        Message::GroupGrid(action) => app.handle_group_grid(action),
        Message::GroupTerminal(workspace_id, event) => {
            app.handle_group_terminal(workspace_id, event)
        }
        Message::FolderSelected(Some(path)) => {
            app.create_session_state.cwd = path.display().to_string();
            app.create_session_state.open = true;
            app.create_session_state.selected_preset = 0;
        }
        Message::FolderSelected(None) => {
            // User cancelled folder selection
        }
        Message::CreateSession(action) => {
            match action {
                CreateSessionAction::Close => {
                    app.create_session_state.open = false;
                }
                CreateSessionAction::SelectPreset(i) => {
                    app.create_session_state.selected_preset = i;
                }
                CreateSessionAction::CustomCommandChanged(cmd) => {
                    app.create_session_state.custom_command = cmd;
                }
                CreateSessionAction::CustomArgsChanged(args) => {
                    app.create_session_state.custom_args = args;
                }
                CreateSessionAction::Create => {
                    let state = &app.create_session_state;
                    let preset = &CLI_PRESETS[state.selected_preset];
                    let cwd = state.cwd.clone();
                    let folder = cwd
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("workspace");

                    // Generate unique session ID
                    let base_id = format!("{}-{}", folder, preset.id);
                    let existing: std::collections::HashSet<String> = app
                        .local_sessions
                        .iter()
                        .map(|s| s.id.to_string())
                        .collect();
                    let id = if !existing.contains(&base_id) {
                        base_id.clone()
                    } else {
                        let mut n = 2;
                        loop {
                            let candidate = format!("{}-{}", base_id, n);
                            if !existing.contains(&candidate) {
                                break candidate;
                            }
                            n += 1;
                        }
                    };

                    let is_custom = preset.id == "custom";
                    let (cmd, args) = if is_custom {
                        let cmd = state.custom_command.trim().to_string();
                        let args: Vec<String> = state.custom_args.split_whitespace().map(|s| s.to_string()).collect();
                        (cmd, args)
                    } else {
                        (preset.command.to_string(), preset.args.iter().map(|s| s.to_string()).collect())
                    };
                    if let Ok(mut manager) = app.manager.lock() {
                        let _ = manager.create(
                            id.clone(),
                            cwd,
                            Some(cmd),
                            Some(args),
                            None,
                            None,
                            false,
                        );
                    }

                    app.active_workspace = id;
                    app.current_view = CurrentView::Session;
                    app.create_session_state.open = false;
                    app.refresh_sessions();
                    app.refresh_active_terminal();
                    let _ = app.session_store.save_shared(&app.manager);
                }
            }
        }
        Message::Deliberate(action) => {
            match action {
                DeliberateAction::Close => {
                    app.deliberate_state.open = false;
                }
                DeliberateAction::TopicChanged(topic) => {
                    app.deliberate_state.topic = topic;
                }
                DeliberateAction::Submit => {
                    let topic = app.deliberate_state.topic.trim().to_string();
                    app.deliberate_state.open = false;
                    if !topic.is_empty() {
                        app.status_text = format!("Deliberation started: {}", topic);
                        // TODO: spawn Codex + Gemini ephemeral sessions and create group
                    }
                }
            }
        }
        Message::Settings(action) => {
            match action {
                SettingsAction::Close => {
                    app.settings_state.open = false;
                }
                SettingsAction::SetTheme(mode) => {
                    app.theme_mode = mode;
                    app.settings_state.open = false;
                    save_theme(mode);
                    app.status_text = format!("Theme: {:?}", mode);
                }
            }
        }
        Message::RouteToWorkspace { workspace_id, text } => {
            eprintln!("[ROUTE] inject {} bytes to '{}'", text.len(), workspace_id);
            match app.manager.lock() {
                Ok(manager) => {
                    match manager.queue_inject(&workspace_id, "aterm-internal", text) {
                        Ok(pending) => {
                            app.status_text = format!("Queued inject to {workspace_id} ({pending} pending)");
                        }
                        Err(e) => {
                            app.status_text = format!("Route failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    app.status_text = format!("Manager lock failed: {e}");
                }
            }
        }
    }

    Task::none()
}

fn view(app: &Aterm) -> Element<'_, Message> {
    let palette = app.palette();
    let sidebar = Sidebar::new(palette)
        .view(SidebarModel {
            local_sessions: &app.local_sessions,
            telepty_sessions: &app.telepty_sessions,
            groups: &app.groups,
        })
        .map(Message::Sidebar);

    let header = container(
        row![
            row![
                text("·⣿·").size(14).style(move |_| iced::widget::text::Style {
                    color: Some(palette.accent),
                }),
                text("aterm").size(15).style(move |_| iced::widget::text::Style {
                    color: Some(palette.text),
                }),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            Space::new().width(Fill),
            row![
                container(Space::new().width(6).height(6))
                    .width(6)
                    .height(6)
                    .style(palette.status_dot_style(palette.success)),
                text("Connected").size(12).style(move |_| iced::widget::text::Style {
                    color: Some(palette.text_muted),
                }),
                container(Space::new().width(1).height(16))
                    .width(1)
                    .height(16)
                    .style(move |_| iced::widget::container::Style {
                        text_color: None,
                        background: Some(Background::Color(palette.border)),
                        border: Border::default(),
                        shadow: iced::Shadow::default(),
                        snap: true,
                    }),
                text("⌘K").size(12).style(move |_| iced::widget::text::Style {
                    color: Some(palette.text_muted),
                }),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .align_y(Alignment::Center)
        .padding([0, 20]),
    )
    .height(48)
    .width(Fill)
    .style(move |_| iced::widget::container::Style {
        text_color: Some(palette.text),
        background: Some(Background::Color(palette.surface)),
        border: Border {
            width: 0.0,
            radius: 0.0.into(),
            color: palette.border,
        },
        shadow: iced::Shadow::default(),
        snap: true,
    });

    let header_with_border = column![
        header,
        container(Space::new().height(1).width(Fill))
            .height(1)
            .width(Fill)
            .style(move |_| iced::widget::container::Style {
                text_color: None,
                background: Some(Background::Color(palette.border)),
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            }),
    ]
    .width(Fill);

    let content_panel: Element<'_, Message> = match &app.current_view {
        CurrentView::Session => {
            let active_session = app
                .local_sessions
                .iter()
                .find(|session| session.id.as_ref() == app.active_workspace);
            let session_name = active_session
                .map(|session| session.title.to_string())
                .unwrap_or_default();
            let session_status = active_session
                .map(|session| session.status)
                .unwrap_or(SessionStatus::Unknown);
            let session_status_label = match session_status {
                SessionStatus::Online | SessionStatus::Running => "active",
                SessionStatus::Busy => "busy",
                SessionStatus::Offline => "offline",
                SessionStatus::Stale => "stale",
                SessionStatus::Dead => "dead",
                SessionStatus::Unknown => "unknown",
            };
            let session_status_color = session_status.color(palette);

            let session_header = container(
                row![
                    container(Space::new().width(7).height(7))
                        .width(7)
                        .height(7)
                        .style(palette.status_dot_style(session_status_color)),
                    text(session_name).size(13).style(move |_| iced::widget::text::Style {
                        color: Some(palette.text),
                    }),
                    Space::new().width(Fill),
                    text(session_status_label).size(11).style(move |_| iced::widget::text::Style {
                        color: Some(session_status_color),
                    }),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([0, 14]),
            )
            .height(36)
            .width(Fill)
            .style(move |_| iced::widget::container::Style {
                text_color: Some(palette.text),
                background: Some(Background::Color(palette.surface_alt)),
                border: Border {
                    width: 0.0,
                    radius: 0.0.into(),
                    color: palette.border_subtle,
                },
                shadow: iced::Shadow::default(),
                snap: true,
            });

            let term_renderer = crate::terminal::TerminalRenderer::default()
                .with_colors(palette.text, palette.background)
                .with_light_mode(matches!(app.theme_mode, ThemeMode::Light));
            let terminal_container = container(
                TerminalWidget::new(app.terminal.terminal())
                    .with_terminal_renderer(term_renderer)
                    .on_event(Message::Terminal),
            )
            .width(Fill)
            .height(Fill)
            .style(move |_| iced::widget::container::Style {
                text_color: Some(palette.text),
                background: Some(Background::Color(palette.background)),
                border: Border::default(),
                shadow: iced::Shadow::default(),
                snap: true,
            })
            .padding(4);

            container(column![session_header, terminal_container].spacing(0).padding(0))
                .width(Fill)
                .height(Fill)
                .into()
        }
        CurrentView::Group(_) => view_group_content(app, palette),
    };

    let base = column![
        header_with_border,
        row![sidebar, content_panel].width(Fill).height(Fill),
    ]
    .width(Fill)
    .height(Fill);

    let overlay = CommandPalette::new(palette)
        .view(&app.palette_state, &app.commands)
        .map(Message::Palette);

    let create_dialog = CreateSessionDialog::new(palette)
        .view(&app.create_session_state)
        .map(Message::CreateSession);

    let deliberate_dialog = DeliberateDialog::new(palette)
        .view(&app.deliberate_state)
        .map(Message::Deliberate);

    let settings_panel = SettingsPanel::new(palette)
        .view(&app.settings_state, app.theme_mode)
        .map(Message::Settings);

    stack([base.into(), overlay, create_dialog, deliberate_dialog, settings_panel]).into()
}

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".aterm")
        .join("settings.json")
}

fn load_theme() -> ThemeMode {
    let path = settings_path();
    let contents = std::fs::read_to_string(&path).unwrap_or_default();
    if contents.contains("\"light\"") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    }
}

fn save_theme(mode: ThemeMode) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let value = match mode {
        ThemeMode::Dark => r#"{"theme":"dark"}"#,
        ThemeMode::Light => r#"{"theme":"light"}"#,
    };
    let _ = std::fs::write(&path, value);
}

fn current_dir_string() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("/"))
        .display()
        .to_string()
}

fn view_group_content(app: &Aterm, palette: Palette) -> Element<'_, Message> {
    let Some(group_view) = app.group_view.as_ref() else {
        return container(text("No active group")).width(Fill).height(Fill).into();
    };

    let cells = group_view
        .members
        .iter()
        .map(|member| {
            let workspace_id = member.workspace_id.clone();
            let status = member.status.clone();

            container(
                column![
                    row![
                        column![
                            text(member.title.clone()).size(14),
                            text(member.subtitle.clone()).size(12).style(move |_| {
                                iced::widget::text::Style {
                                    color: Some(palette.text_muted),
                                }
                            }),
                        ]
                        .spacing(2),
                        Space::new().width(Fill),
                        text(status).size(11).style(move |_| {
                            iced::widget::text::Style {
                                color: Some(palette.text_muted),
                            }
                        }),
                    ]
                    .align_y(iced::Alignment::Center),
                    container({
                        let grp_renderer = crate::terminal::TerminalRenderer::default()
                            .with_colors(palette.text, palette.background)
                .with_light_mode(matches!(app.theme_mode, ThemeMode::Light));
                        TerminalWidget::new(member.terminal.terminal())
                            .with_terminal_renderer(grp_renderer)
                            .on_event({
                                let workspace_id = workspace_id.clone();
                                move |event| Message::GroupTerminal(workspace_id.clone(), event)
                            })
                    }
                    )
                    .width(Fill)
                    .height(Fill)
                    .style(palette.panel_style())
                    .padding(10),
                ]
                .spacing(10),
            )
            .padding(12)
            .width(Fill)
            .height(Fill)
            .style(palette.panel_style())
            .into()
        })
        .collect();

    container(
        GroupGrid::new(palette).view(
            group_view.title.as_str(),
            group_view.topic.as_str(),
            group_view.phase,
            &group_view.summary,
            cells,
            Message::GroupGrid,
        ),
    )
    .padding(16)
    .width(Fill)
    .height(Fill)
    .into()
}

fn compact_terminal_summary(snapshot: &str) -> String {
    let normalized = normalize_terminal_text(snapshot);
    if normalized.is_empty() {
        return "No visible terminal output yet.".to_string();
    }

    let mut lines = Vec::new();
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let mut current = String::new();

    for word in words {
        let next_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };

        if next_len > 72 && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
            if lines.len() == 3 {
                break;
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }

    if lines.len() < 3 && !current.is_empty() {
        lines.push(current);
    }

    let mut summary = lines.join("\n");
    if normalized.len() > summary.len() {
        summary.push_str("\n...");
    }
    summary
}

fn default_commands() -> Vec<CommandEntry<'static>> {
    vec![
        CommandEntry {
            command: PaletteCommand::NewSession,
            title: Cow::Borrowed("new"),
            description: Cow::Borrowed("Create a new local session"),
            shortcut: None,
        },
        CommandEntry {
            command: PaletteCommand::Deliberate,
            title: Cow::Borrowed("deliberate"),
            description: Cow::Borrowed("Start a multi-agent deliberation"),
            shortcut: Some(Cow::Borrowed("Cmd+K")),
        },
        CommandEntry {
            command: PaletteCommand::Group,
            title: Cow::Borrowed("group"),
            description: Cow::Borrowed("Create a group from the current session list"),
            shortcut: None,
        },
        CommandEntry {
            command: PaletteCommand::Broadcast,
            title: Cow::Borrowed("broadcast"),
            description: Cow::Borrowed(
                "Queue a broadcast-style inject into the active session",
            ),
            shortcut: None,
        },
        CommandEntry {
            command: PaletteCommand::Theme(ThemeMode::Dark),
            title: Cow::Borrowed("theme dark"),
            description: Cow::Borrowed("Switch to the dark palette"),
            shortcut: None,
        },
        CommandEntry {
            command: PaletteCommand::Theme(ThemeMode::Light),
            title: Cow::Borrowed("theme light"),
            description: Cow::Borrowed("Switch to the light palette"),
            shortcut: None,
        },
    ]
}

fn display_name(cwd: &str, command: &str, args: &[String]) -> String {
    let folder = cwd
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("~");
    let cli = extract_cli_name(command, args);
    format!("{folder} · {cli}")
}

fn extract_cli_name(command: &str, args: &[String]) -> String {
    match basename(command) {
        "claude" => "Claude".to_string(),
        "codex" => "Codex".to_string(),
        "gemini" => "Gemini".to_string(),
        "telepty" => extract_telepty_cli(args),
        other => title_case_command(other),
    }
}

fn extract_telepty_cli(args: &[String]) -> String {
    let mut args = args.iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "allow" => continue,
            "--id" => {
                let _ = args.next();
            }
            value if value.starts_with('-') => continue,
            value => return extract_cli_name(value, &[]),
        }
    }

    "Shell".to_string()
}

fn title_case_command(command: &str) -> String {
    let name = basename(command);
    let mut chars = name.chars();

    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Shell".to_string(),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn split_command_line(command_line: &str) -> (String, Vec<String>) {
    let mut parts = command_line.split_whitespace();
    let command = parts.next().unwrap_or("").to_string();
    let args = parts.map(str::to_owned).collect();

    (command, args)
}

fn current_os_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Unknown"
    }
}

fn local_session_meta() -> String {
    format!("{} · aterm · local", current_os_label())
}

fn infer_tool(session: &TeleptySessionInfo) -> String {
    let (command, args) = split_command_line(&session.command);
    let mut haystack = session.id.to_lowercase();

    if !command.is_empty() {
        haystack.push(' ');
        haystack.push_str(&command.to_lowercase());
    }

    if !args.is_empty() {
        haystack.push(' ');
        haystack.push_str(&args.join(" ").to_lowercase());
    }

    for tool in [
        "cmux",
        "kitty",
        "tmux",
        "aterm",
        "claude",
        "codex",
        "gemini",
    ] {
        if haystack.contains(tool) {
            return tool.to_string();
        }
    }

    if !command.is_empty() {
        return basename(&command).to_string();
    }

    "telepty".to_string()
}

fn remote_session_meta(session: &TeleptySessionInfo) -> String {
    let host_label = if session.host.is_empty()
        || session.host.eq_ignore_ascii_case("local")
        || session.host.eq_ignore_ascii_case("localhost")
    {
        "local".to_string()
    } else {
        session.host.clone()
    };

    let os = if host_label == "local" {
        current_os_label()
    } else {
        "Remote"
    };

    format!("{} · {} · {}", os, infer_tool(session), host_label)
}

fn system_cjk_font_tasks() -> Vec<Task<Message>> {
    // Load only the FIRST available CJK font to save memory.
    // Apple SD Gothic Neo alone is ~27MB; loading all 3 wastes ~74MB.
    for (label, path) in system_cjk_font_candidates() {
        if let Ok(bytes) = std::fs::read(path) {
            let label = (*label).to_string();
            eprintln!("[FONT] loading CJK font: {} ({} bytes)", label, bytes.len());
            return vec![iced::font::load(bytes).map(move |result| {
                Message::FontLoaded(label.clone(), result)
            })];
        }
    }
    Vec::new()
}

fn system_cjk_font_candidates() -> &'static [(&'static str, &'static str)] {
    // Only the first available font is loaded (see system_cjk_font_tasks).
    // Order matters: preferred font first, fallbacks after.
    #[cfg(target_os = "macos")]
    {
        &[
            ("Apple SD Gothic Neo", "/System/Library/Fonts/AppleSDGothicNeo.ttc"),
        ]
    }

    #[cfg(target_os = "linux")]
    {
        &[
            ("Noto Sans CJK KR", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            ("Noto Sans CJK KR", "/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf"),
        ]
    }

    #[cfg(target_os = "windows")]
    {
        &[
            ("Malgun Gothic", "C:\\Windows\\Fonts\\malgun.ttf"),
        ]
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        &[]
    }
}

fn map_local_session(workspace: &WorkspaceInfo, active: bool) -> SessionEntry<'static> {
    SessionEntry {
        id: Cow::Owned(workspace.id.clone()),
        title: Cow::Owned(display_name(
            &workspace.cwd,
            &workspace.command,
            &workspace.args,
        )),
        subtitle: Cow::Owned(local_session_meta()),
        status: match workspace.status.as_str() {
            "running" => SessionStatus::Running,
            "dead" => SessionStatus::Dead,
            _ => SessionStatus::Unknown,
        },
        kind: SessionKind::Local,
        pending_injects: 0,
        active,
    }
}

fn map_local_session_with_injects(workspace: &WorkspaceInfo, active: bool, pending: usize) -> SessionEntry<'static> {
    let mut entry = map_local_session(workspace, active);
    entry.pending_injects = pending;
    entry
}

fn map_telepty_session(session: &TeleptySessionInfo, active: bool) -> SessionEntry<'static> {
    let (command, args) = split_command_line(&session.command);
    let title = if !session.cwd.is_empty() && !command.is_empty() {
        display_name(&session.cwd, &command, &args)
    } else if !command.is_empty() {
        extract_cli_name(&command, &args)
    } else {
        session.id.clone()
    };

    SessionEntry {
        id: Cow::Owned(session.id.clone()),
        title: Cow::Owned(title),
        subtitle: Cow::Owned(remote_session_meta(session)),
        status: match session.status.as_str() {
            "active" | "online" => SessionStatus::Online,
            "busy" => SessionStatus::Busy,
            "offline" => SessionStatus::Offline,
            "stale" => SessionStatus::Stale,
            _ => SessionStatus::Unknown,
        },
        kind: SessionKind::Telepty,
        pending_injects: 0,
        active,
    }
}
