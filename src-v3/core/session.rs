use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::pty::{PtyManager, SharedPtyManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionData {
    pub sessions: Vec<SessionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            path: sessions_path(),
        }
    }

    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn load(&self) -> Result<SessionData, String> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SessionData {
                    sessions: Vec::new(),
                });
            }
            Err(error) => return Err(error.to_string()),
        };
        serde_json::from_str(&contents).map_err(|error| error.to_string())
    }

    pub fn save(&self, manager: &PtyManager) -> Result<(), String> {
        let entries = manager.session_entries();
        let data = SessionData { sessions: entries };
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let json = serde_json::to_string_pretty(&data).map_err(|error| error.to_string())?;
        std::fs::write(&self.path, json).map_err(|error| error.to_string())
    }

    pub fn save_shared(&self, manager: &SharedPtyManager) -> Result<(), String> {
        let guard = manager.lock().map_err(|error| error.to_string())?;
        self.save(&*guard)
    }

    pub fn restore_into(&self, manager: &SharedPtyManager) -> Result<(), String> {
        let data = self.load()?;

        for entry in data.sessions {
            let mut guard = manager.lock().map_err(|error| error.to_string())?;
            if let Err(error) = guard.restore_session_entry(entry) {
                eprintln!("[aterm] restore_sessions skipped entry: {}", error);
            }
        }

        Ok(())
    }
}

pub fn sessions_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".aterm").join("sessions.json")
}

pub fn is_claude_session(command: &str, args: &[String]) -> bool {
    command == "claude" || (command == "telepty" && args.iter().any(|arg| arg == "claude"))
}

pub fn codex_resume_index(command: &str, args: &[String]) -> Option<usize> {
    if command == "codex" {
        return args.iter().position(|arg| arg == "resume");
    }
    if command != "telepty" {
        return None;
    }

    let codex_index = args.iter().position(|arg| arg == "codex")?;
    args.iter()
        .enumerate()
        .skip(codex_index + 1)
        .find(|(_, arg)| arg.as_str() == "resume")
        .map(|(index, _)| index)
}

pub fn ensure_codex_resume_last_arg(command: &str, args: &[String]) -> Vec<String> {
    let Some(resume_index) = codex_resume_index(command, args) else {
        return args.to_vec();
    };

    if args.iter().any(|arg| arg == "--last") {
        return args.to_vec();
    }

    let has_positional_tail = args
        .iter()
        .skip(resume_index + 1)
        .any(|arg| !arg.starts_with('-'));
    if has_positional_tail {
        return args.to_vec();
    }

    let mut normalized = args.to_vec();
    normalized.insert(resume_index + 1, "--last".to_string());
    normalized
}

pub fn strip_claude_continue_arg(command: &str, args: &[String]) -> Vec<String> {
    if !is_claude_session(command, args) {
        return args.to_vec();
    }

    args.iter()
        .filter(|arg| arg.as_str() != "--continue")
        .cloned()
        .collect()
}

pub fn restored_session_args(command: &str, cwd: &str, args: &[String]) -> Vec<String> {
    let stripped = strip_claude_continue_arg(command, args);
    let mut normalized = ensure_codex_resume_last_arg(command, &stripped);
    if is_claude_session(command, &normalized)
        && check_claude_history(cwd)
        && !normalized.iter().any(|arg| arg == "--continue")
    {
        normalized.push("--continue".to_string());
    }
    normalized
}

pub fn check_claude_history(cwd: &str) -> bool {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let key = cwd.replace('/', "-");
    let history_dir = home.join(".claude").join("projects").join(&key);
    if !history_dir.is_dir() {
        return false;
    }

    std::fs::read_dir(&history_dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        })
        .unwrap_or(false)
}
