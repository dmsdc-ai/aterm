mod core;
mod ime;
mod terminal;
mod ui;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use iced::widget::{column, container, row, stack, text, Space};
use iced::{
    event, keyboard, time, Alignment, Background, Border, Element, Fill, Result,
    Subscription, Task,
};
use tokio::sync::Notify;

static PTY_NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

use crate::core::{
    normalize_terminal_text, SessionStore, SharedPtyManager, TeleptyClient,
    TeleptySessionInfo, WorkspaceInfo,
};
use crate::ime::{CandidateRect, ImeBridge, Rect, TextRange};
use crate::terminal::{TerminalEvent, TerminalState, TerminalWidget};
use crate::ui::{
    CommandEntry, CommandPalette, CommandPaletteAction, CommandPaletteState, GroupEntry,
    GroupGrid, GroupGridAction, GroupSummaryEntry, HybridPhase,
    Palette, PaletteCommand, SessionEntry, SessionKind, SessionStatus, Sidebar,
    SidebarAction, SidebarModel, ThemeMode,
};

const SESSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_COLUMNS: u16 = 120;
const DEFAULT_ROWS: u16 = 36;

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

static APP_TITLE: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(app_title);

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

        let _ = PTY_NOTIFY.set(
            manager
                .lock()
                .map(|m| m.notify_handle())
                .unwrap_or_else(|_| Arc::new(Notify::new())),
        );

        let mut app = Self {
            manager,
            session_store,
            telepty,
            ime,
            terminal: TerminalState::new(DEFAULT_COLUMNS as usize, DEFAULT_ROWS as usize),
            theme_mode: ThemeMode::Dark,
            palette_state: CommandPaletteState::default(),
            commands: default_commands(),
            current_view: CurrentView::Session,
            active_workspace: String::new(),
            group_view: None,
            local_sessions: Vec::new(),
            telepty_sessions: Vec::new(),
            groups: Vec::new(),
            status_text: "Ready".to_string(),
        };

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
                map_local_session(
                    workspace,
                    matches!(self.current_view, CurrentView::Session)
                        && workspace.id == self.active_workspace,
                )
            })
            .collect();

        self.telepty_sessions = self
            .telepty
            .list_sessions()
            .unwrap_or_default()
            .iter()
            .map(|session| map_telepty_session(session))
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

    fn create_workspace(&mut self) {
        let next_id = format!("session-{}", self.local_sessions.len() + 1);

        if let Ok(mut manager) = self.manager.lock() {
            let result = manager.create(
                next_id.clone(),
                current_dir_string(),
                None,
                None,
                Some(DEFAULT_COLUMNS),
                Some(DEFAULT_ROWS),
                false,
            );

            match result {
                Ok(_) => {
                    self.active_workspace = next_id;
                    self.status_text = "Created a new local session".to_string();
                    let _ = self.session_store.save_shared(&self.manager);
                }
                Err(error) => {
                    self.status_text = format!("Create session failed: {error}");
                }
            }
        }

        self.refresh_sessions();
        self.refresh_active_terminal();
    }

    fn handle_sidebar(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::SelectSession(id) => {
                if self.local_sessions.iter().any(|entry| entry.id.as_ref() == id) {
                    self.current_view = CurrentView::Session;
                    self.group_view = None;
                    self.active_workspace = id;
                    self.status_text = "Selected local session".to_string();
                    self.refresh_sessions();
                    self.refresh_active_terminal();
                } else {
                    self.status_text =
                        "Telepty session selected; attach flow is not wired yet".to_string();
                }
            }
            SidebarAction::SelectGroup(id) => {
                self.open_group_view(&id);
                self.refresh_sessions();
                self.status_text = format!("Selected group: {id}");
            }
            SidebarAction::CreateSession => {
                self.create_workspace();
            }
            SidebarAction::OpenSettings => {
                self.status_text = "settings: panel scaffold pending".to_string();
            }
        }
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
            PaletteCommand::Deliberate => {
                self.status_text = "deliberate: orchestration hook pending".to_string();
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
                self.terminal.resize(columns as usize, rows as usize);
                let _ = self
                    .manager
                    .lock()
                    .ok()
                    .and_then(|manager| manager.resize(&self.active_workspace, columns, rows).ok());

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
    iced::stream::channel(32, |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
        use iced::futures::SinkExt;
        let notify = PTY_NOTIFY
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(Notify::new()));
        loop {
            notify.notified().await;
            // Coalesce rapid notifications — drain any pending, then throttle
            tokio::time::sleep(std::time::Duration::from_millis(8)).await;
            let _ = sender.try_send(Message::PtyDataReady);
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
            eprintln!("[PTY] data ready");
            match app.current_view {
                CurrentView::Session => app.refresh_active_terminal(),
                CurrentView::Group(_) => app.sync_group_view(),
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
        Message::Sidebar(action) => app.handle_sidebar(action),
        Message::Palette(action) => app.handle_palette(action),
        Message::Terminal(event) => app.handle_terminal(event),
        Message::GroupGrid(action) => app.handle_group_grid(action),
        Message::GroupTerminal(workspace_id, event) => {
            app.handle_group_terminal(workspace_id, event)
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
    .height(47)
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
                    text(session_name).size(12).style(move |_| iced::widget::text::Style {
                        color: Some(palette.text_muted),
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
            .height(32)
            .width(Fill)
            .style(move |_| iced::widget::container::Style {
                text_color: Some(palette.text),
                background: Some(Background::Color(palette.surface)),
                border: Border {
                    width: 1.0,
                    radius: 0.0.into(),
                    color: palette.border,
                },
                shadow: iced::Shadow::default(),
                snap: true,
            });

            let terminal_container = container(
                TerminalWidget::new(app.terminal.terminal()).on_event(Message::Terminal),
            )
            .width(Fill)
            .height(Fill)
            .style(palette.panel_style())
            .padding(4);

            container(column![session_header, terminal_container].spacing(0).padding(16))
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

    stack([base.into(), overlay]).into()
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
                    container(
                        TerminalWidget::new(member.terminal.terminal()).on_event({
                            let workspace_id = workspace_id.clone();
                            move |event| Message::GroupTerminal(workspace_id.clone(), event)
                        })
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
    system_cjk_font_candidates()
        .iter()
        .filter_map(|(label, path)| {
            std::fs::read(path).ok().map(|bytes| {
                let label = (*label).to_string();
                iced::font::load(bytes).map(move |result| {
                    Message::FontLoaded(label.clone(), result)
                })
            })
        })
        .collect()
}

fn system_cjk_font_candidates() -> &'static [(&'static str, &'static str)] {
    #[cfg(target_os = "macos")]
    {
        &[
            ("Apple SD Gothic Neo", "/System/Library/Fonts/AppleSDGothicNeo.ttc"),
            ("Hiragino Sans GB", "/System/Library/Fonts/Hiragino Sans GB.ttc"),
            (
                "Arial Unicode",
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            ),
            ("Arial Unicode", "/Library/Fonts/Arial Unicode.ttf"),
        ]
    }

    #[cfg(target_os = "linux")]
    {
        &[
            (
                "Noto Sans CJK KR",
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            ),
            (
                "Noto Sans CJK KR",
                "/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf",
            ),
            (
                "Noto Sans CJK SC",
                "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
            ),
        ]
    }

    #[cfg(target_os = "windows")]
    {
        &[
            ("Malgun Gothic", "C:\\Windows\\Fonts\\malgun.ttf"),
            ("Microsoft YaHei", "C:\\Windows\\Fonts\\msyh.ttc"),
            ("MS Gothic", "C:\\Windows\\Fonts\\msgothic.ttc"),
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
        active,
    }
}

fn map_telepty_session(session: &TeleptySessionInfo) -> SessionEntry<'static> {
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
        active: false,
    }
}
