use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "action")]
pub enum SessionAction {
    Inject {
        workspace: String,
        text: String,
        from: Option<String>,
        #[serde(default)]
        force: Option<bool>,
    },
    ListWorkspaces,
    WorkspaceStatus {
        workspace: String,
    },
    FocusWorkspace {
        workspace: String,
    },
    CloseWorkspace {
        workspace: String,
    },
    CreateWorkspace {
        name: String,
        cli: String,
        cwd: String,
    },
    RestartWorkspace {
        workspace: String,
    },
    RestartAllWorkspaces,
    ChangeWorkspaceCLI {
        workspace: String,
        cli: String,
    },
    ReadScreenText {
        workspace: String,
        max_bytes: Option<usize>,
    },
    ListTasks {
        workspace: String,
    },
    RenameWorkspace {
        #[serde(alias = "workspace")]
        old_name: String,
        #[serde(alias = "newName")]
        new_name: String,
    },
    ClearWorkspace {
        workspace: String,
    },
    SendKey {
        workspace: String,
        key: String,
    },
    AttachExternal {
        #[serde(alias = "sessionID")]
        session_id: String,
    },
    ReloadSettings,
    Subscribe {
        events: Vec<String>,
    },
    WaitUntil {
        workspace: String,
        state: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    /// Sub-session signals that its task is finished (#76). Sets state "complete".
    MarkComplete {
        workspace: String,
        #[serde(default)]
        report: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "status")]
pub enum ActionResponse {
    Ok,
    Data { data: serde_json::Value },
    Unsupported,
    Error { message: String },
}

impl ActionResponse {
    pub fn ok() -> Self {
        Self::Ok
    }
    pub fn data(value: serde_json::Value) -> Self {
        Self::Data { data: value }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error {
            message: msg.into(),
        }
    }
    pub fn unsupported() -> Self {
        Self::Unsupported
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum AtermEvent {
    WorkspaceCreated { id: String, cli: String, cwd: String },
    WorkspaceClosed { id: String },
    ShellReady { id: String },
}
