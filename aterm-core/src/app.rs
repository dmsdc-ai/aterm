use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aterm_ipc::server::IpcServer;
use aterm_session::action::{ActionResponse, SessionAction};
use aterm_session::host::PlatformHost;
use aterm_session::types::WorkspaceInfo;

use crate::inject::{InjectMessage, SharedInjectQueue};

/// App-level singleton that owns the IPC server and routes inject messages.
/// Workspace PTY processes are owned by per-view AtermCore instances.
/// This singleton only stores inject queue references for IPC routing.
pub struct AtermApp {
    /// workspace_name → inject queue (cloned Arc from PtyManager)
    inject_queues: HashMap<String, SharedInjectQueue>,
    /// workspace metadata for ListWorkspaces fallback
    workspace_meta: HashMap<String, WorkspaceMeta>,
    ipc_server: Option<IpcServer>,
    host: Option<Box<dyn PlatformHost>>,
    socket_path: String,
    token: String,
}

struct WorkspaceMeta {
    name: String,
    command: String,
    cwd: String,
}

impl AtermApp {
    pub fn new() -> Self {
        let socket_path = format!("/tmp/aterm-{}.sock", std::process::id());
        let token = generate_token();
        Self {
            inject_queues: HashMap::new(),
            workspace_meta: HashMap::new(),
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

    /// Register a workspace's inject queue so IPC can route to it.
    pub fn register_workspace(&mut self, name: &str, queue: SharedInjectQueue, command: &str, cwd: &str) {
        self.inject_queues.insert(name.to_string(), queue);
        self.workspace_meta.insert(name.to_string(), WorkspaceMeta {
            name: name.to_string(),
            command: command.to_string(),
            cwd: cwd.to_string(),
        });
        eprintln!("[aterm-app] registered workspace: {}", name);
    }

    /// Deregister a workspace (on close).
    pub fn deregister_workspace(&mut self, name: &str) {
        self.inject_queues.remove(name);
        self.workspace_meta.remove(name);
        eprintln!("[aterm-app] deregistered workspace: {}", name);
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
                if let Some(queue) = self.inject_queues.get(&workspace) {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    match queue.lock() {
                        Ok(mut q) => {
                            let pending = q.push(InjectMessage {
                                from: from.unwrap_or_default(),
                                text,
                                timestamp,
                            });
                            ActionResponse::data(serde_json::json!({ "queued": pending }))
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
                    let infos: Vec<WorkspaceInfo> = self.workspace_meta.values().map(|m| WorkspaceInfo {
                        id: m.name.clone(),
                        name: m.name.clone(),
                        cli: m.command.clone(),
                        cwd: m.cwd.clone(),
                        status: "running".to_string(),
                    }).collect();
                    ActionResponse::data(serde_json::to_value(infos).unwrap_or_default())
                }
            }
            SessionAction::WorkspaceStatus { workspace } => {
                let exists = self.inject_queues.contains_key(&workspace);
                ActionResponse::data(serde_json::json!({ "alive": exists }))
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
                ActionResponse::ok()
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
