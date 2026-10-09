use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// Global monotonic sequence counter for IPC events.
/// Incremented on every broadcast — subscribers use this to detect gaps.
static EVENT_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_seq() -> u64 {
    EVENT_SEQ.fetch_add(1, Ordering::Relaxed)
}

use aterm_ipc::server::IpcServer;
use aterm_session::action::{ActionResponse, AtermEvent, SessionAction};
use aterm_session::host::PlatformHost;
use aterm_session::types::{WorkspaceEvent, WorkspaceInfo};

use crate::inject::{InjectMessage, InjectSignal, SharedInjectQueue};
use crate::pty::WorkspaceStatus;
use crate::telepty_bridge::TeleptyBridge;

/// App-level singleton that owns the IPC server and routes inject messages.
/// Workspace PTY processes are owned by per-view AtermCore instances.
/// This singleton only stores inject queue references for IPC routing.
pub struct AtermApp {
    /// workspace_name → inject queue (cloned Arc from PtyManager)
    inject_queues: HashMap<String, SharedInjectQueue>,
    /// workspace_name → inject signal (wake injector on enqueue)
    inject_signals: HashMap<String, InjectSignal>,
    /// workspace_name → PTY writer for direct (force) inject
    workspace_writers: HashMap<String, Arc<Mutex<Box<dyn IoWrite + Send>>>>,
    /// workspace_name → lifecycle status (running/closing/dead)
    workspace_statuses: HashMap<String, WorkspaceStatus>,
    /// workspace metadata for ListWorkspaces fallback
    workspace_meta: HashMap<String, WorkspaceMeta>,
    /// workspace_name → report passed to MarkComplete (capped at MAX_REPORT_BYTES)
    completion_reports: HashMap<String, String>,
    ipc_server: Option<IpcServer>,
    host: Option<Box<dyn PlatformHost>>,
    telepty_bridge: Option<TeleptyBridge>,
    socket_path: String,
    /// Condvar pulsed when any workspace is registered. Used by CreateWorkspace
    /// to avoid polling for workspace availability.
    registration_signal: Arc<(Mutex<()>, Condvar)>,
}

#[derive(Clone)]
struct WorkspaceMeta {
    name: String,
    command: String,
    cwd: String,
}

impl AtermApp {
    pub fn new() -> Self {
        cleanup_stale_socket_files();
        let socket_path = format!("/tmp/aterm-{}.sock", std::process::id());
        let telepty_bridge = TeleptyBridge::try_connect();
        Self {
            inject_queues: HashMap::new(),
            inject_signals: HashMap::new(),
            workspace_writers: HashMap::new(),
            workspace_statuses: HashMap::new(),
            workspace_meta: HashMap::new(),
            completion_reports: HashMap::new(),
            ipc_server: None,
            host: None,
            telepty_bridge,
            socket_path,
            registration_signal: Arc::new((Mutex::new(()), Condvar::new())),
        }
    }

    /// Test-only constructor: no telepty bridge (never probes or POSTs to a live
    /// daemon), no stale-socket cleanup, no /tmp socket path.
    #[cfg(test)]
    fn new_detached() -> Self {
        Self {
            inject_queues: HashMap::new(),
            inject_signals: HashMap::new(),
            workspace_writers: HashMap::new(),
            workspace_statuses: HashMap::new(),
            workspace_meta: HashMap::new(),
            completion_reports: HashMap::new(),
            ipc_server: None,
            host: None,
            telepty_bridge: None,
            socket_path: String::new(),
            registration_signal: Arc::new((Mutex::new(()), Condvar::new())),
        }
    }

    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    pub fn registration_signal(&self) -> Arc<(Mutex<()>, Condvar)> {
        self.registration_signal.clone()
    }

    pub fn set_host(&mut self, host: Box<dyn PlatformHost>) {
        self.host = Some(host);
    }

    pub fn host_ref(&self) -> Option<&dyn PlatformHost> {
        self.host.as_deref()
    }

    fn workspace_exists(&self, name: &str) -> bool {
        self.workspace_meta.contains_key(name) && self.inject_queues.contains_key(name)
    }

    fn workspace_state(&self, name: &str) -> Option<String> {
        self.workspace_statuses
            .get(name)
            .and_then(|status| status.0.lock().ok().map(|current| current.clone()))
    }

    /// Get the status Arc for a workspace (used by WaitUntil without long-holding app lock).
    pub fn workspace_status_arc(&self, name: &str) -> Option<WorkspaceStatus> {
        self.workspace_statuses.get(name).cloned()
    }

    fn workspace_is_dead(&self, name: &str) -> bool {
        matches!(
            self.workspace_state(name).as_deref(),
            Some("dead" | "closing")
        )
    }

    fn rename_workspace_registration(
        &mut self,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), String> {
        if old_name == new_name {
            return Ok(());
        }
        if self.workspace_meta.contains_key(new_name) || self.inject_queues.contains_key(new_name) {
            return Err(format!("workspace '{}' already exists", new_name));
        }

        let queue = self
            .inject_queues
            .remove(old_name)
            .ok_or_else(|| format!("workspace '{}' not found", old_name))?;
        let sig = self.inject_signals.remove(old_name);
        let writer = self.workspace_writers.remove(old_name);
        let status = self.workspace_statuses.remove(old_name);
        let mut meta = match self.workspace_meta.remove(old_name) {
            Some(meta) => meta,
            None => {
                self.inject_queues.insert(old_name.to_string(), queue);
                if let Some(si) = sig {
                    self.inject_signals.insert(old_name.to_string(), si);
                }
                if let Some(w) = writer {
                    self.workspace_writers.insert(old_name.to_string(), w);
                }
                if let Some(s) = status {
                    self.workspace_statuses.insert(old_name.to_string(), s);
                }
                return Err(format!("workspace '{}' not found", old_name));
            }
        };

        meta.name = new_name.to_string();
        self.inject_queues.insert(new_name.to_string(), queue);
        if let Some(si) = sig {
            self.inject_signals.insert(new_name.to_string(), si);
        }
        if let Some(w) = writer {
            self.workspace_writers.insert(new_name.to_string(), w);
        }
        if let Some(s) = status {
            self.workspace_statuses.insert(new_name.to_string(), s);
        }
        self.workspace_meta
            .insert(new_name.to_string(), meta.clone());
        if let Some(report) = self.completion_reports.remove(old_name) {
            self.completion_reports.insert(new_name.to_string(), report);
        }

        if let Some(ref bridge) = self.telepty_bridge {
            bridge.deregister(old_name);
            bridge.register(
                new_name,
                new_name,
                &meta.command,
                &meta.cwd,
                &self.socket_path,
            );
        }

        Ok(())
    }

    /// Register a workspace's inject queue and PTY writer so IPC can route to it.
    pub fn register_workspace(
        &mut self,
        name: &str,
        queue: SharedInjectQueue,
        signal: Option<InjectSignal>,
        writer: Option<Arc<Mutex<Box<dyn IoWrite + Send>>>>,
        status: Option<WorkspaceStatus>,
        command: &str,
        cwd: &str,
    ) {
        self.inject_queues.insert(name.to_string(), queue);
        if let Some(sig) = signal {
            self.inject_signals.insert(name.to_string(), sig);
        }
        if let Some(w) = writer {
            self.workspace_writers.insert(name.to_string(), w);
        }
        if let Some(s) = status {
            self.workspace_statuses.insert(name.to_string(), s);
        }
        self.workspace_meta.insert(
            name.to_string(),
            WorkspaceMeta {
                name: name.to_string(),
                command: command.to_string(),
                cwd: cwd.to_string(),
            },
        );
        if let Some(ref bridge) = self.telepty_bridge {
            bridge.register(name, name, command, cwd, &self.socket_path);
        }
        self.publish_event(AtermEvent::WorkspaceCreated {
            id: name.to_string(),
            cli: command.to_string(),
            cwd: cwd.to_string(),
        });
        // Wake CreateWorkspace waiters — workspace is now registered
        if let Ok(_guard) = self.registration_signal.0.lock() {
            self.registration_signal.1.notify_all();
        }
        log_stderr!("[aterm-app] registered workspace: {}", name);
    }

    /// Deregister a workspace (on close).
    pub fn deregister_workspace(&mut self, name: &str) {
        self.inject_queues.remove(name);
        self.inject_signals.remove(name);
        self.workspace_writers.remove(name);
        self.workspace_statuses.remove(name);
        self.workspace_meta.remove(name);
        self.completion_reports.remove(name);
        if let Some(ref bridge) = self.telepty_bridge {
            bridge.deregister(name);
        }
        log_stderr!("[aterm-app] deregistered workspace: {}", name);
    }

    /// Re-register all current workspaces with telepty.
    /// Called after session restore to ensure all workspaces are visible.
    pub fn sync_telepty_registrations(&self) {
        if let Some(ref bridge) = self.telepty_bridge {
            let workspaces: Vec<(String, String, String, String)> = self
                .workspace_meta
                .values()
                .map(|m| {
                    (
                        m.name.clone(),
                        m.command.clone(),
                        m.cwd.clone(),
                        self.socket_path.clone(),
                    )
                })
                .collect();

            if !workspaces.is_empty() {
                log_stderr!(
                    "[aterm-app] syncing {} workspaces to telepty",
                    workspaces.len()
                );
                bridge.sync_all(workspaces);
            }
        }
    }

    /// Re-register workspace handles after auto-restart so workspace_exists() stays true.
    pub fn update_workspace_handles(
        &mut self,
        name: &str,
        writer: Arc<Mutex<Box<dyn IoWrite + Send>>>,
        inject_queue: SharedInjectQueue,
        inject_signal: InjectSignal,
        status: WorkspaceStatus,
    ) {
        self.workspace_writers.insert(name.to_string(), writer);
        self.inject_queues.insert(name.to_string(), inject_queue);
        self.inject_signals.insert(name.to_string(), inject_signal);
        self.workspace_statuses.insert(name.to_string(), status);
    }

    pub fn handle_workspace_marked_dead(&mut self, name: &str) {
        let exists = self.workspace_exists(name);
        if exists && !self.workspace_is_dead(name) {
            return;
        }
        // Always fire Closed event — even if workspace was already deregistered,
        // Swift sidebar may still have a stale entry that needs removal.
        if let Some(ref host) = self.host {
            host.on_workspace_event(WorkspaceEvent::Closed {
                id: name.to_string(),
            });
        }
        if exists {
            self.deregister_workspace(name);
        }
        self.publish_event(AtermEvent::WorkspaceClosed {
            id: name.to_string(),
        });
    }

    /// Start the embedded IPC server. Must be called after the app is wrapped in Arc<Mutex<>>.
    pub fn start_ipc(app: &Arc<Mutex<Self>>) {
        let app_clone = app.clone();
        let socket_path = {
            let app = app.lock().unwrap();
            app.socket_path.clone()
        };

        let reg_signal = {
            let app = app.lock().unwrap();
            app.registration_signal()
        };

        let dispatcher: Arc<dyn Fn(SessionAction) -> ActionResponse + Send + Sync> =
            Arc::new(move |action| {
                // WaitUntil: extract status arc briefly, then release app lock and block on Condvar.
                // Must NOT hold the app Mutex while blocking — that would deadlock all IPC.
                if let SessionAction::WaitUntil {
                    ref workspace,
                    ref state,
                    timeout_ms,
                } = action
                {
                    let status_arc = {
                        if let Ok(app) = app_clone.lock() {
                            app.workspace_status_arc(workspace)
                        } else {
                            return ActionResponse::error("app lock failed");
                        }
                    };
                    // App lock released here
                    let Some(status_arc) = status_arc else {
                        return ActionResponse::error(format!(
                            "workspace '{}' not found",
                            workspace
                        ));
                    };
                    let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(300_000));
                    if status_arc.0.is_poisoned() {
                        return ActionResponse::error("status lock poisoned");
                    }
                    return ActionResponse::data(wait_for_state(&status_arc, state, timeout));
                }

                // CreateWorkspace: dispatch, then wait for shell "running" before returning.
                if let SessionAction::CreateWorkspace { ref name, .. } = action {
                    let workspace_name = name.clone();
                    let resp = {
                        if let Ok(mut app) = app_clone.lock() {
                            app.dispatch(action)
                        } else {
                            return ActionResponse::error("app lock failed");
                        }
                    };
                    if !matches!(resp, ActionResponse::Ok) {
                        return resp;
                    }
                    // Wait for workspace to register and reach "running" (up to 15s)
                    let deadline = Instant::now() + std::time::Duration::from_secs(15);
                    loop {
                        let status_arc = {
                            if let Ok(app) = app_clone.lock() {
                                app.workspace_status_arc(&workspace_name)
                            } else {
                                return resp;
                            }
                        };
                        if let Some(status_arc) = status_arc {
                            let (ref mutex, ref condvar) = *status_arc;
                            let mut current = match mutex.lock() {
                                Ok(g) => g,
                                Err(_) => return resp,
                            };
                            loop {
                                if *current == "running" {
                                    return ActionResponse::data(serde_json::json!({
                                        "ready": true
                                    }));
                                }
                                let remaining = deadline.saturating_duration_since(Instant::now());
                                if remaining.is_zero() {
                                    return ActionResponse::data(serde_json::json!({
                                        "ready": false, "timeout": true
                                    }));
                                }
                                let result = condvar
                                    .wait_timeout(current, remaining)
                                    .unwrap_or_else(|e| e.into_inner());
                                current = result.0;
                            }
                        }
                        if Instant::now() >= deadline {
                            return resp;
                        }
                        // Wait for workspace registration event (Condvar) instead of polling
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        let (ref lock, ref cvar) = *reg_signal;
                        if let Ok(guard) = lock.lock() {
                            let _ = cvar.wait_timeout(guard, remaining);
                        }
                    }
                }

                if let Ok(mut app) = app_clone.lock() {
                    app.dispatch(action)
                } else {
                    ActionResponse::error("app lock failed")
                }
            });

        match IpcServer::start(&socket_path, dispatcher) {
            Ok(server) => {
                let mut app = app.lock().unwrap();
                app.ipc_server = Some(server);
                log_stderr!("[aterm-app] IPC server started: {}", app.socket_path);
            }
            Err(e) => {
                log_stderr!("[aterm-app] IPC server failed to start: {}", e);
            }
        }
    }

    /// Dispatch a SessionAction and return a response.
    pub fn dispatch(&mut self, action: SessionAction) -> ActionResponse {
        match action {
            SessionAction::Inject {
                workspace,
                text,
                from,
                force,
            } => {
                if self.workspace_is_dead(&workspace) {
                    return ActionResponse::error(format!("workspace '{}' is dead", workspace));
                }
                if force.unwrap_or(false) {
                    // Direct PTY write — bypass idle gate
                    // Split text and Enter so TUI frameworks process them separately.
                    if let Some(writer) = self.workspace_writers.get(&workspace) {
                        let text_only = text.trim_end_matches(|c: char| c == '\r' || c == '\n');
                        let text_result = match writer.lock() {
                            Ok(mut w) => w.write_all(text_only.as_bytes()).and_then(|_| w.flush()),
                            Err(e) => Err(std::io::Error::other(e.to_string())),
                        };
                        if let Err(e) = text_result {
                            return ActionResponse::error(e.to_string());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        match writer.lock() {
                            Ok(mut w) => match w.write_all(b"\r").and_then(|_| w.flush()) {
                                Ok(_) => {
                                    log_stderr!(
                                        "[aterm-app] force-injected {} bytes + Enter into '{}'",
                                        text_only.len(),
                                        workspace
                                    );
                                    ActionResponse::ok()
                                }
                                Err(e) => ActionResponse::error(e.to_string()),
                            },
                            Err(e) => ActionResponse::error(e.to_string()),
                        }
                    } else {
                        ActionResponse::error(format!("workspace '{}' writer not found", workspace))
                    }
                } else if let Some(queue) = self.inject_queues.get(&workspace) {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    match queue.lock() {
                        Ok(mut q) => {
                            match q.push(InjectMessage {
                                from: from.unwrap_or_default(),
                                text,
                                timestamp,
                                enqueued_at: Instant::now(),
                            }) {
                                Ok(pending) => {
                                    // Wake injector loop — message enqueued
                                    if let Some(sig) = self.inject_signals.get(&workspace) {
                                        sig.notify();
                                    }
                                    log_stderr!(
                                        "[aterm-app] queued inject into '{}' (pending: {})",
                                        workspace,
                                        pending
                                    );
                                    ActionResponse::data(serde_json::json!({ "queued": pending }))
                                }
                                Err(e) => ActionResponse::error(e),
                            }
                        }
                        Err(e) => ActionResponse::error(e.to_string()),
                    }
                } else {
                    ActionResponse::error(format!("workspace '{}' not found", workspace))
                }
            }
            SessionAction::ListWorkspaces => {
                if let Some(ref host) = self.host {
                    let list = host.list_workspaces();
                    ActionResponse::data(serde_json::to_value(list).unwrap_or_default())
                } else {
                    let infos: Vec<WorkspaceInfo> = self
                        .workspace_meta
                        .values()
                        .map(|m| {
                            let status = self
                                .workspace_state(&m.name)
                                .unwrap_or_else(|| "running".to_string());
                            WorkspaceInfo {
                                id: m.name.clone(),
                                name: m.name.clone(),
                                cli: m.command.clone(),
                                cwd: m.cwd.clone(),
                                status,
                                custom_command: None,
                                created_at: None,
                                last_activity_at: None,
                                is_system: None,
                            }
                        })
                        .collect();
                    ActionResponse::data(serde_json::to_value(infos).unwrap_or_default())
                }
            }
            SessionAction::WorkspaceStatus { workspace } => {
                let exists = self.inject_queues.contains_key(&workspace);
                let state = self
                    .workspace_state(&workspace)
                    .unwrap_or_else(|| "unknown".to_string());
                ActionResponse::data(serde_json::json!({
                    "alive": exists && !matches!(state.as_str(), "dead" | "closing"),
                    "state": state,
                    "report": self.completion_reports.get(&workspace)
                }))
            }
            SessionAction::MarkComplete { workspace, report } => {
                if !self.workspace_exists(&workspace) {
                    return ActionResponse::error(format!("workspace '{}' not found", workspace));
                }
                if self.workspace_is_dead(&workspace) {
                    return ActionResponse::error(format!("workspace '{}' is dead", workspace));
                }
                if let Some(status) = self.workspace_statuses.get(&workspace) {
                    if let Ok(mut current) = status.0.lock() {
                        *current = "complete".to_string();
                    }
                    status.1.notify_all();
                }
                if let Some(report) = report {
                    self.completion_reports
                        .insert(workspace.clone(), truncate_report(&report));
                }
                // Reaches IPC subscribers only (mirrors pty.rs "closing").
                self.broadcast_workspace_event(&serde_json::json!({
                    "type": "StatusChanged",
                    "id": workspace,
                    "status": "complete"
                }));
                ActionResponse::ok()
            }
            SessionAction::FocusWorkspace { workspace } => {
                if let Some(ref host) = self.host {
                    host.focus_workspace(&workspace);
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::CloseWorkspace { workspace } => {
                if let Some(ref host) = self.host {
                    host.close_workspace_view(&workspace);
                }
                self.deregister_workspace(&workspace);
                self.publish_event(AtermEvent::WorkspaceClosed {
                    id: workspace.to_string(),
                });
                ActionResponse::ok()
            }
            SessionAction::CreateWorkspace { name, cli, cwd } => {
                if self.workspace_exists(&name) {
                    return ActionResponse::error(format!("workspace '{}' already exists", name));
                }
                let created = if let Some(ref host) = self.host {
                    let config = aterm_session::types::WorkspaceConfig {
                        name: name.clone(),
                        cli: cli.clone(),
                        cwd: cwd.clone(),
                        cols: 80,
                        rows: 24,
                    };
                    host.create_workspace_view(&name, &config);
                    true
                } else {
                    false
                };
                if created {
                    self.publish_event(AtermEvent::WorkspaceCreated {
                        id: name.to_string(),
                        cli,
                        cwd,
                    });
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::RestartWorkspace { workspace } => {
                let meta = self.workspace_meta.get(&workspace).cloned();
                if let Some(meta) = meta {
                    if let Some(ref host) = self.host {
                        host.close_workspace_view(&workspace);
                    }
                    self.deregister_workspace(&workspace);
                    self.publish_event(AtermEvent::WorkspaceClosed {
                        id: workspace.to_string(),
                    });
                    let restarted = if let Some(ref host) = self.host {
                        let config = aterm_session::types::WorkspaceConfig {
                            name: meta.name.clone(),
                            cli: meta.command.clone(),
                            cwd: meta.cwd.clone(),
                            cols: 80,
                            rows: 24,
                        };
                        host.create_workspace_view(&meta.name, &config);
                        true
                    } else {
                        false
                    };
                    if restarted {
                        self.publish_event(AtermEvent::WorkspaceCreated {
                            id: meta.name.clone(),
                            cli: meta.command.clone(),
                            cwd: meta.cwd.clone(),
                        });
                        ActionResponse::ok()
                    } else {
                        ActionResponse::unsupported()
                    }
                } else {
                    ActionResponse::error(format!("workspace '{}' not found", workspace))
                }
            }
            SessionAction::RestartAllWorkspaces => {
                let metas: Vec<WorkspaceMeta> = self.workspace_meta.values().cloned().collect();
                if let Some(ref host) = self.host {
                    for meta in &metas {
                        host.close_workspace_view(&meta.name);
                    }
                }
                for meta in &metas {
                    self.deregister_workspace(&meta.name);
                    self.publish_event(AtermEvent::WorkspaceClosed {
                        id: meta.name.clone(),
                    });
                }
                if let Some(ref host) = self.host {
                    for meta in &metas {
                        let config = aterm_session::types::WorkspaceConfig {
                            name: meta.name.clone(),
                            cli: meta.command.clone(),
                            cwd: meta.cwd.clone(),
                            cols: 80,
                            rows: 24,
                        };
                        host.create_workspace_view(&meta.name, &config);
                    }
                }
                for meta in &metas {
                    self.publish_event(AtermEvent::WorkspaceCreated {
                        id: meta.name.clone(),
                        cli: meta.command.clone(),
                        cwd: meta.cwd.clone(),
                    });
                }
                ActionResponse::data(serde_json::json!({ "restarted": metas.len() }))
            }
            SessionAction::ChangeWorkspaceCLI { workspace, cli } => {
                let meta = self.workspace_meta.get(&workspace).cloned();
                if let Some(meta) = meta {
                    if let Some(ref host) = self.host {
                        host.close_workspace_view(&workspace);
                    }
                    self.deregister_workspace(&workspace);
                    self.publish_event(AtermEvent::WorkspaceClosed {
                        id: workspace.to_string(),
                    });
                    let changed = if let Some(ref host) = self.host {
                        let config = aterm_session::types::WorkspaceConfig {
                            name: meta.name.clone(),
                            cli: cli.clone(),
                            cwd: meta.cwd.clone(),
                            cols: 80,
                            rows: 24,
                        };
                        host.create_workspace_view(&meta.name, &config);
                        true
                    } else {
                        false
                    };
                    if changed {
                        self.publish_event(AtermEvent::WorkspaceCreated {
                            id: meta.name.clone(),
                            cli,
                            cwd: meta.cwd.clone(),
                        });
                        ActionResponse::ok()
                    } else {
                        ActionResponse::unsupported()
                    }
                } else {
                    ActionResponse::error(format!("workspace '{}' not found", workspace))
                }
            }
            SessionAction::ListTasks { workspace } => {
                if let Some(meta) = self.workspace_meta.get(&workspace) {
                    let file_path = std::path::Path::new(&meta.cwd)
                        .join("state")
                        .join("task-queue.json");
                    let data = if file_path.exists() {
                        match std::fs::read_to_string(&file_path) {
                            Ok(content) => serde_json::from_str(&content)
                                .unwrap_or(serde_json::json!({"tasks":[],"completed":[]})),
                            Err(_) => serde_json::json!({"tasks":[],"completed":[]}),
                        }
                    } else {
                        serde_json::json!({"tasks":[],"completed":[]})
                    };
                    ActionResponse::data(data)
                } else {
                    ActionResponse::error(format!("workspace '{}' not found", workspace))
                }
            }
            SessionAction::RenameWorkspace { old_name, new_name } => {
                let new_name = new_name.trim().to_string();
                if new_name.is_empty() {
                    return ActionResponse::error("new workspace name cannot be empty");
                }
                if !self.workspace_exists(&old_name) {
                    return ActionResponse::error(format!("workspace '{}' not found", old_name));
                }
                if let Some(ref host) = self.host {
                    host.rename_workspace(&old_name, &new_name);
                    match self.rename_workspace_registration(&old_name, &new_name) {
                        Ok(()) => ActionResponse::ok(),
                        Err(message) => ActionResponse::error(message),
                    }
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::ClearWorkspace { workspace } => {
                if !self.workspace_exists(&workspace) {
                    return ActionResponse::error(format!("workspace '{}' not found", workspace));
                }
                if let Some(ref host) = self.host {
                    host.send_key(&workspace, "ctrl+l");
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::SendKey { workspace, key } => {
                if !self.workspace_exists(&workspace) {
                    return ActionResponse::error(format!("workspace '{}' not found", workspace));
                }
                if !is_supported_send_key(&key) {
                    return ActionResponse::error(format!(
                        "unknown key '{}'; supported: enter, return, ctrl+c/ctrl-c, ctrl+d/ctrl-d, ctrl+l/ctrl-l, ctrl+z/ctrl-z, tab, esc, escape",
                        key
                    ));
                }
                if let Some(ref host) = self.host {
                    host.send_key(&workspace, &key);
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::AttachExternal { session_id } => {
                let session_id = session_id.trim().to_string();
                if session_id.is_empty() {
                    return ActionResponse::error("session_id cannot be empty");
                }
                if let Some(ref host) = self.host {
                    host.attach_external_session(&session_id);
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::ReloadSettings => {
                if let Some(ref host) = self.host {
                    host.reload_settings();
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::ReadScreenText {
                workspace: _,
                max_bytes: _,
            } => ActionResponse::unsupported(),
            SessionAction::Subscribe { .. } => {
                // Return workspace snapshot so IPC server can send it on subscribe
                ActionResponse::data(self.workspace_snapshot())
            }
            SessionAction::WaitUntil { .. } => {
                // Handled in start_ipc dispatcher (outside app lock); should not reach here
                ActionResponse::error("WaitUntil must be handled outside app lock")
            }
        }
    }

    fn publish_event(&self, event: AtermEvent) {
        // Serialize to JSON for IPC broadcast (existing mechanism)
        if let Ok(json) = serde_json::to_value(&event) {
            self.broadcast_event(&json);
        }
    }

    /// Broadcast a workspace event to all IPC subscribers.
    pub fn broadcast_workspace_event(&self, event: &serde_json::Value) {
        self.broadcast_event(event);
    }

    fn broadcast_event(&self, event: &serde_json::Value) {
        if let Some(ref server) = self.ipc_server {
            let seq = next_seq();
            let mut event = event.clone();
            if let Some(obj) = event.as_object_mut() {
                obj.insert("seq".to_string(), serde_json::json!(seq));
            }
            let json = serde_json::to_string(&event).unwrap_or_default();
            let snapshot_fn = || {
                let snap = self.workspace_snapshot();
                serde_json::to_string(&snap).unwrap_or_default()
            };
            server.broadcast(&json, &snapshot_fn);
        }
    }

    /// Build a full workspace snapshot with current sequence number.
    /// Used for initial Subscribe response and gap re-sync.
    pub fn workspace_snapshot(&self) -> serde_json::Value {
        let workspaces: Vec<serde_json::Value> = self
            .workspace_meta
            .values()
            .map(|m| {
                let status = self
                    .workspace_state(&m.name)
                    .unwrap_or_else(|| "running".to_string());
                serde_json::json!({
                    "id": m.name,
                    "name": m.name,
                    "cli": m.command,
                    "cwd": m.cwd,
                    "status": status
                })
            })
            .collect();

        let seq = EVENT_SEQ.load(Ordering::Relaxed);
        serde_json::json!({
            "type": "Snapshot",
            "seq": seq,
            "workspaces": workspaces
        })
    }
}

fn cleanup_stale_socket_files() {
    let Ok(entries) = std::fs::read_dir("/tmp") else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("aterm-") || !name.ends_with(".sock") {
            continue;
        }

        if UnixStream::connect(&path).is_ok() {
            continue;
        }

        if std::fs::remove_file(&path).is_ok() {
            log_stderr!("[aterm-app] removed stale socket: {}", path.display());
        }
    }
}

/// Block on a workspace status Condvar until `target` is reached, `timeout`
/// elapses, or the workspace enters a terminal state it can never leave for
/// `target` (dead/closing/closed) — then return immediately with `terminal`.
fn wait_for_state(
    status: &WorkspaceStatus,
    target: &str,
    timeout: std::time::Duration,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + timeout;
    let (ref mutex, ref condvar) = **status;
    // Condvar wait — zero CPU when idle, instant response on state change
    let mut current = mutex.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if *current == target {
            return serde_json::json!({
                "reached": true, "state": *current
            });
        }
        if matches!(current.as_str(), "dead" | "closing" | "closed") {
            return serde_json::json!({
                "reached": false, "state": *current, "terminal": true
            });
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return serde_json::json!({
                "reached": false, "state": *current, "timeout": true
            });
        }
        let result = condvar
            .wait_timeout(current, remaining)
            .unwrap_or_else(|e| e.into_inner());
        current = result.0;
    }
}

/// Max bytes kept from a MarkComplete report.
const MAX_REPORT_BYTES: usize = 4096;

/// Truncate to at most MAX_REPORT_BYTES without splitting a UTF-8 char.
fn truncate_report(report: &str) -> String {
    let mut end = report.len().min(MAX_REPORT_BYTES);
    while !report.is_char_boundary(end) {
        end -= 1;
    }
    report[..end].to_string()
}

fn is_supported_send_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "enter"
            | "return"
            | "ctrl+c"
            | "ctrl-c"
            | "ctrl+d"
            | "ctrl-d"
            | "ctrl+l"
            | "ctrl-l"
            | "ctrl+z"
            | "ctrl-z"
            | "tab"
            | "esc"
            | "escape"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::InjectQueue;
    use std::time::Duration;

    fn status_with(state: &str) -> WorkspaceStatus {
        Arc::new((Mutex::new(state.to_string()), Condvar::new()))
    }

    /// Register `name` on a detached app (no telepty bridge, no IPC socket).
    fn detached_with(name: &str, status: &WorkspaceStatus) -> AtermApp {
        let mut app = AtermApp::new_detached();
        let queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));
        app.register_workspace(name, queue, None, None, Some(status.clone()), "zsh", "/tmp");
        app
    }

    fn mark_complete(app: &mut AtermApp, name: &str, report: Option<&str>) -> ActionResponse {
        app.dispatch(SessionAction::MarkComplete {
            workspace: name.to_string(),
            report: report.map(str::to_string),
        })
    }

    fn status_data(app: &mut AtermApp, name: &str) -> serde_json::Value {
        match app.dispatch(SessionAction::WorkspaceStatus {
            workspace: name.to_string(),
        }) {
            ActionResponse::Data { data } => data,
            other => panic!("WorkspaceStatus should return Data, got {:?}", other),
        }
    }

    fn error_message(resp: ActionResponse) -> String {
        match resp {
            ActionResponse::Error { message } => message,
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn mark_complete_sets_state_and_report() {
        let status = status_with("running");
        let mut app = detached_with("w1", &status);

        let resp = mark_complete(&mut app, "w1", Some("done: x"));
        assert!(matches!(resp, ActionResponse::Ok), "got {:?}", resp);
        assert_eq!(*status.0.lock().unwrap(), "complete");

        let data = status_data(&mut app, "w1");
        assert_eq!(data["state"], "complete");
        assert_eq!(data["report"], "done: x");
        assert_eq!(data["alive"], true);
    }

    #[test]
    fn mark_complete_unknown_workspace_errors() {
        let mut app = AtermApp::new_detached();
        let message = error_message(mark_complete(&mut app, "nope", Some("x")));
        assert_eq!(message, "workspace 'nope' not found");
    }

    #[test]
    fn mark_complete_dead_workspace_errors() {
        let status = status_with("dead");
        let mut app = detached_with("w1", &status);

        let message = error_message(mark_complete(&mut app, "w1", Some("x")));
        assert_eq!(message, "workspace 'w1' is dead");
        assert_eq!(*status.0.lock().unwrap(), "dead");
        assert!(status_data(&mut app, "w1")["report"].is_null());
    }

    #[test]
    fn mark_complete_wakes_condvar_waiter() {
        let status = status_with("running");
        let mut app = detached_with("w1", &status);

        let status2 = status.clone();
        let handle = std::thread::spawn(move || {
            let start = Instant::now();
            let (ref mutex, ref condvar) = *status2;
            let guard = mutex.lock().unwrap();
            // Bounded so a regression fails the assert instead of hanging the suite.
            let (guard, _) = condvar
                .wait_timeout_while(guard, Duration::from_secs(2), |s| s != "complete")
                .unwrap();
            (start.elapsed(), guard.clone())
        });

        // Give waiter time to block
        std::thread::sleep(Duration::from_millis(20));
        assert!(matches!(
            mark_complete(&mut app, "w1", None),
            ActionResponse::Ok
        ));

        let (elapsed, seen) = handle.join().unwrap();
        assert_eq!(seen, "complete");
        assert!(
            elapsed < Duration::from_millis(200),
            "Condvar waiter should wake promptly on complete, got {:?}",
            elapsed
        );
    }

    #[test]
    fn wait_for_state_returns_early_on_terminal_state() {
        let status = status_with("running");
        let status2 = status.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            if let Ok(mut current) = status2.0.lock() {
                *current = "dead".to_string();
            }
            status2.1.notify_all();
        });

        let start = Instant::now();
        let result = wait_for_state(&status, "complete", Duration::from_secs(5));
        let elapsed = start.elapsed();
        setter.join().unwrap();

        assert_eq!(result["reached"], false);
        assert_eq!(result["state"], "dead");
        assert_eq!(result["terminal"], true);
        assert!(result.get("timeout").is_none(), "got {}", result);
        assert!(
            elapsed < Duration::from_millis(500),
            "terminal state should return early, got {:?}",
            elapsed
        );
    }

    #[test]
    fn wait_for_state_times_out() {
        let status = status_with("running");

        let start = Instant::now();
        let result = wait_for_state(&status, "complete", Duration::from_millis(50));
        let elapsed = start.elapsed();

        assert_eq!(result["reached"], false);
        assert_eq!(result["state"], "running");
        assert_eq!(result["timeout"], true);
        assert!(result.get("terminal").is_none(), "got {}", result);
        assert!(elapsed >= Duration::from_millis(50), "got {:?}", elapsed);
    }

    #[test]
    fn report_truncated_to_4096_bytes_on_char_boundary() {
        let status = status_with("running");
        let mut app = detached_with("w1", &status);
        // 3-byte chars: 4096 is not a char boundary (4096 = 3 * 1365 + 1).
        let long = "가".repeat(2000);

        assert!(matches!(
            mark_complete(&mut app, "w1", Some(&long)),
            ActionResponse::Ok
        ));

        let data = status_data(&mut app, "w1");
        let report = data["report"].as_str().expect("report should be a string");
        assert_eq!(report.len(), 4095);
        assert_eq!(report, "가".repeat(1365));
    }

    #[test]
    fn deregister_clears_report() {
        let status = status_with("running");
        let mut app = detached_with("w1", &status);
        assert!(matches!(
            mark_complete(&mut app, "w1", Some("done")),
            ActionResponse::Ok
        ));

        app.deregister_workspace("w1");
        assert!(!app.completion_reports.contains_key("w1"));

        // A new workspace reusing the name must not inherit the old report.
        let fresh = status_with("running");
        let queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));
        app.register_workspace("w1", queue, None, None, Some(fresh), "zsh", "/tmp");
        let data = status_data(&mut app, "w1");
        assert_eq!(data["state"], "running");
        assert!(data["report"].is_null());
    }

    fn create_workspace(app: &mut AtermApp, name: &str) -> ActionResponse {
        app.dispatch(SessionAction::CreateWorkspace {
            name: name.to_string(),
            cli: "zsh".to_string(),
            cwd: "/tmp".to_string(),
        })
    }

    #[test]
    fn create_workspace_rejects_existing_name() {
        let status = status_with("running");
        let mut app = detached_with("w1", &status);

        let message = error_message(create_workspace(&mut app, "w1"));
        assert!(message.contains("already exists"), "got {:?}", message);
        assert_eq!(message, "workspace 'w1' already exists");
    }

    #[test]
    fn create_workspace_unknown_name_without_host_is_unsupported() {
        let status = status_with("running");
        let mut app = detached_with("w1", &status);

        let resp = create_workspace(&mut app, "w2");
        assert!(matches!(resp, ActionResponse::Unsupported), "got {:?}", resp);
    }
}
