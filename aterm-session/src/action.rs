use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "action")]
pub enum SessionAction {
    Inject { workspace: String, text: String, from: Option<String> },
    ListWorkspaces,
    WorkspaceStatus { workspace: String },
    FocusWorkspace { workspace: String },
    CloseWorkspace { workspace: String },
    CreateWorkspace { name: String, cli: String, cwd: String },
    ReadScreenText { workspace: String, max_bytes: Option<usize> },
    ListTasks { workspace: String },
    ListLessons { workspace: String },
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
    pub fn ok() -> Self { Self::Ok }
    pub fn data(value: serde_json::Value) -> Self { Self::Data { data: value } }
    pub fn error(msg: impl Into<String>) -> Self { Self::Error { message: msg.into() } }
    pub fn unsupported() -> Self { Self::Unsupported }
}
