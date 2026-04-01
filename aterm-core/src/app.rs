use std::sync::{Arc, Mutex};

use aterm_ipc::server::IpcServer;
use aterm_session::action::{ActionResponse, SessionAction};
use aterm_session::host::PlatformHost;
use aterm_session::types::WorkspaceInfo;

use crate::pty::{PtyManager, SharedPtyManager};

/// App-level singleton that owns all workspaces and the IPC server.
/// Follows Ghostty ghostty_app_new() / Alacritty Processor pattern.
pub struct AtermApp {
    pub pty_manager: SharedPtyManager,
    ipc_server: Option<IpcServer>,
    host: Option<Box<dyn PlatformHost>>,
    socket_path: String,
    token: String,
}

impl AtermApp {
    pub fn new() -> Self {
        let socket_path = format!("/tmp/aterm-{}.sock", std::process::id());
        let token = generate_token();
        Self {
            pty_manager: PtyManager::shared(),
            ipc_server: None,
            host: None,
            socket_path,
            token,
        }
    }

    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn set_host(&mut self, host: Box<dyn PlatformHost>) {
        self.host = Some(host);
    }

    /// Start the embedded IPC server. Must be called after the app is wrapped in Arc<Mutex<>>.
    pub fn start_ipc(app: &Arc<Mutex<Self>>) {
        let app_clone = app.clone();
        let socket_path = {
            let app = app.lock().unwrap();
            app.socket_path.clone()
        };

        let dispatcher: Arc<dyn Fn(SessionAction) -> ActionResponse + Send + Sync> =
            Arc::new(move |action| {
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
                eprintln!("[aterm-app] IPC server started: {}", app.socket_path);
            }
            Err(e) => {
                eprintln!("[aterm-app] IPC server failed to start: {}", e);
            }
        }
    }

    /// Dispatch a SessionAction and return a response.
    pub fn dispatch(&mut self, action: SessionAction) -> ActionResponse {
        match action {
            SessionAction::Inject { workspace, text, from } => {
                match self.pty_manager.lock() {
                    Ok(mgr) => {
                        match mgr.queue_inject(&workspace, &from.unwrap_or_default(), text) {
                            Ok(queued) => ActionResponse::data(serde_json::json!({ "queued": queued })),
                            Err(e) => ActionResponse::error(e),
                        }
                    }
                    Err(e) => ActionResponse::error(e.to_string()),
                }
            }
            SessionAction::ListWorkspaces => {
                if let Some(ref host) = self.host {
                    let list = host.list_workspaces();
                    ActionResponse::data(serde_json::to_value(list).unwrap_or_default())
                } else {
                    match self.pty_manager.lock() {
                        Ok(mgr) => {
                            let ws = mgr.list_workspaces();
                            let infos: Vec<WorkspaceInfo> = ws.iter().map(|w| WorkspaceInfo {
                                id: w.id.clone(),
                                name: w.id.clone(),
                                cli: w.command.clone(),
                                cwd: w.cwd.clone(),
                                status: w.status.clone(),
                            }).collect();
                            ActionResponse::data(serde_json::to_value(infos).unwrap_or_default())
                        }
                        Err(e) => ActionResponse::error(e.to_string()),
                    }
                }
            }
            SessionAction::WorkspaceStatus { workspace } => {
                match self.pty_manager.lock() {
                    Ok(mgr) => {
                        let alive = mgr.workspace_is_alive(&workspace);
                        ActionResponse::data(serde_json::json!({ "alive": alive }))
                    }
                    Err(e) => ActionResponse::error(e.to_string()),
                }
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
                match self.pty_manager.lock() {
                    Ok(mut mgr) => {
                        match mgr.close(&workspace) {
                            Ok(()) => {
                                if let Some(ref host) = self.host {
                                    host.close_workspace_view(&workspace);
                                }
                                ActionResponse::ok()
                            }
                            Err(e) => ActionResponse::error(e),
                        }
                    }
                    Err(e) => ActionResponse::error(e.to_string()),
                }
            }
            SessionAction::CreateWorkspace { name, cli, cwd } => {
                if let Some(ref host) = self.host {
                    let config = aterm_session::types::WorkspaceConfig {
                        name: name.clone(),
                        cli,
                        cwd,
                        cols: 80,
                        rows: 24,
                    };
                    host.create_workspace_view(&name, &config);
                    ActionResponse::ok()
                } else {
                    ActionResponse::unsupported()
                }
            }
            SessionAction::ReadScreenText { workspace: _, max_bytes: _ } => {
                ActionResponse::unsupported()
            }
        }
    }
}

fn generate_token() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut buf);
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
