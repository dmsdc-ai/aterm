use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwiftSessionEntry {
    name: String,
    command: Option<String>,
    custom_command: Option<String>,
    cwd: Option<String>,
    is_active: Option<bool>,
    is_system: Option<bool>,
    resume_command: Option<String>,
}

fn save_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;

    #[cfg(target_os = "windows")]
    {
        let _ = std::fs::remove_file(path);
    }

    std::fs::rename(&tmp, path)
}

use crate::pty::{PtyManager, SharedPtyManager};

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
    #[serde(default)]
    pub custom_command: Option<String>,
    #[serde(default)]
    pub is_system: bool,
    #[serde(default)]
    pub resume_command: Option<String>,
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
                // Check for Swift format at ~/.aigentry/config/sessions.json
                let swift_path = data_root().join("config/sessions.json");
                if swift_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&swift_path) {
                        match serde_json::from_str::<Vec<SwiftSessionEntry>>(&content) {
                            Ok(swift_entries) => {
                                let entries: Vec<SessionEntry> = swift_entries
                                    .into_iter()
                                    .map(|s| SessionEntry {
                                        id: s.name,
                                        cwd: s.cwd.unwrap_or_default(),
                                        command: s.command.unwrap_or_default(),
                                        args: vec![],
                                        custom_command: s.custom_command,
                                        is_system: s.is_system.unwrap_or(false),
                                        resume_command: s.resume_command,
                                    })
                                    .collect();
                                let data = SessionData { sessions: entries };
                                if let Ok(json) = serde_json::to_string_pretty(&data) {
                                    if let Some(parent) = self.path.parent() {
                                        let _ = std::fs::create_dir_all(parent);
                                    }
                                    let _ = save_atomic(&self.path, json.as_bytes());
                                }
                                return Ok(data);
                            }
                            Err(e) => {
                                log_stderr!(
                                    "[session] Swift migration parse failed: {e}. Starting fresh."
                                );
                            }
                        }
                    }
                }
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
        save_atomic(&self.path, json.as_bytes()).map_err(|error| error.to_string())
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
                log_stderr!("[aterm] restore_sessions skipped entry: {}", error);
            }
        }

        Ok(())
    }
}

/// Return the aterm data root directory.
/// Uses ATERM_DATA_ROOT env var if set, else ~/.aigentry.
pub fn data_root() -> PathBuf {
    std::env::var("ATERM_DATA_ROOT")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".aigentry")
        })
}

pub fn sessions_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    migrate_dot_aterm(&home);
    data_root().join("data").join("sessions.json")
}

/// One-shot migration: move ~/.aterm/ contents into ~/.aigentry/ then remove ~/.aterm/.
fn migrate_dot_aterm(home: &std::path::Path) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static MIGRATED: AtomicBool = AtomicBool::new(false);
    if MIGRATED.swap(true, Ordering::SeqCst) {
        return;
    }

    let old_root = home.join(".aterm");
    if !old_root.is_dir() {
        return;
    }

    let moves: &[(&str, &str)] = &[
        ("sessions.json", ".aigentry/data/sessions.json"),
        ("aterm.json", ".aigentry/config/aterm.json"),
        ("shell-integration", ".aigentry/shell-integration"),
        ("refs", ".aigentry/refs"),
        ("tailscale", ".aigentry/tailscale"),
        ("aterm.sock", ".aigentry/aterm.sock"),
    ];

    for &(src_rel, dst_rel) in moves {
        let src = old_root.join(src_rel);
        let dst = home.join(dst_rel);
        if !src.exists() || dst.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(&src, &dst).is_ok() {
            eprintln!("[aterm] migrated {} → {}", src.display(), dst.display());
        }
    }

    // Remove ~/.aterm/ if now empty
    let _ = std::fs::remove_dir(&old_root);
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

// -- FFI support --

use std::ffi::{c_char, CString};

#[repr(C)]
pub struct SessionEntryFFI {
    pub id: *const c_char,
    pub cwd: *const c_char,
    pub command: *const c_char,
    pub args_json: *const c_char,
    pub custom_command: *const c_char,
    pub is_system: bool,
    pub resume_command: *const c_char,
}

impl SessionEntryFFI {
    pub fn from_entry(entry: &SessionEntry) -> Self {
        let to_ptr = |s: &str| CString::new(s).unwrap_or_default().into_raw() as *const c_char;
        let opt_ptr = |s: &Option<String>| match s {
            Some(v) => CString::new(v.as_str()).unwrap_or_default().into_raw() as *const c_char,
            None => std::ptr::null(),
        };
        SessionEntryFFI {
            id: to_ptr(&entry.id),
            cwd: to_ptr(&entry.cwd),
            command: to_ptr(&entry.command),
            args_json: to_ptr(
                &serde_json::to_string(&entry.args).unwrap_or_else(|_| "[]".to_string()),
            ),
            custom_command: opt_ptr(&entry.custom_command),
            is_system: entry.is_system,
            resume_command: opt_ptr(&entry.resume_command),
        }
    }

    pub fn null() -> Self {
        SessionEntryFFI {
            id: std::ptr::null(),
            cwd: std::ptr::null(),
            command: std::ptr::null(),
            args_json: std::ptr::null(),
            custom_command: std::ptr::null(),
            is_system: false,
            resume_command: std::ptr::null(),
        }
    }
}
