use crate::types::{WorkspaceConfig, WorkspaceEvent, WorkspaceInfo};

pub trait PlatformHost: Send + Sync {
    fn create_workspace_view(&self, id: &str, config: &WorkspaceConfig);
    fn close_workspace_view(&self, id: &str);
    fn focus_workspace(&self, id: &str);
    fn list_workspaces(&self) -> Vec<WorkspaceInfo>;
    fn on_workspace_event(&self, event: WorkspaceEvent);
    fn request_redraw(&self);
}
