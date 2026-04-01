use crate::types::{WorkspaceConfig, WorkspaceEvent, WorkspaceInfo};

pub trait PlatformHost: Send + Sync {
    fn create_workspace_view(&self, id: &str, config: &WorkspaceConfig);
    fn close_workspace_view(&self, id: &str);
    fn focus_workspace(&self, id: &str);
    fn rename_workspace(&self, old_name: &str, new_name: &str);
    fn send_key(&self, workspace: &str, key: &str);
    fn attach_external_session(&self, session_id: &str);
    fn reload_settings(&self);
    fn list_workspaces(&self) -> Vec<WorkspaceInfo>;
    fn on_workspace_event(&self, event: WorkspaceEvent);
    fn request_redraw(&self);
}
