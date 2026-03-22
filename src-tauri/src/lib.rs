use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

const BUFFER_MAX_BYTES: usize = 1024 * 1024;
const DEFAULT_SNAPSHOT_BYTES: usize = 256 * 1024;

// ── Event payloads ──────────────────────────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PtyOutputPayload {
    workspace: String,
    data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInfo {
    id: String,
    cwd: String,
    command: String,
    args: Vec<String>,
    status: String,
    created_at: String,
    buffer_lines: usize,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct TeleptySessionInfo {
    id: String,
    cwd: String,
    command: String,
    host: String,
    clients: usize,
    started_at: String,
    status: String,
}

// ── Workspace ───────────────────────────────────────────────────
// Thread-safety notes:
//   - writer: accessed only through PtyState mutex
//   - buffer, status: Arc<Mutex<>> because the reader thread also writes to them
//   - master: kept for resize (MasterPty is Send but not Sync — only accessed under PtyState lock)

struct Workspace {
    id: String,
    cwd: String,
    command: String,
    args: Vec<String>,
    size: Arc<Mutex<PtySize>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn PtyChild + Send + Sync>>>,
    buffer: Arc<Mutex<OutputBuffer>>,
    created_at: String,
    status: Arc<Mutex<String>>,
    inject_queue: Arc<Mutex<InjectQueue>>,
    idle_state: Arc<Mutex<IdleState>>,
    auto_restart: bool,
    ephemeral: bool,
}

struct PtyState(Mutex<HashMap<String, Workspace>>);

struct OutputBuffer {
    chunks: VecDeque<String>,
    bytes: usize,
}

impl OutputBuffer {
    fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
        }
    }

    fn push(&mut self, chunk: String) {
        self.bytes += chunk.len();
        self.chunks.push_back(chunk);

        while self.bytes > BUFFER_MAX_BYTES {
            let Some(removed) = self.chunks.pop_front() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.len());
        }
    }

    fn snapshot(&self, max_bytes: usize) -> String {
        if self.chunks.is_empty() || max_bytes == 0 {
            return String::new();
        }

        if self.bytes <= max_bytes {
            let mut snapshot = String::with_capacity(self.bytes);
            for chunk in &self.chunks {
                snapshot.push_str(chunk);
            }
            return snapshot;
        }

        let mut remaining = max_bytes;
        let mut selected: Vec<&str> = Vec::new();

        for chunk in self.chunks.iter().rev() {
            if remaining == 0 {
                break;
            }

            if chunk.len() <= remaining {
                selected.push(chunk.as_str());
                remaining -= chunk.len();
                continue;
            }

            let mut start = chunk.len().saturating_sub(remaining);
            while start < chunk.len() && !chunk.is_char_boundary(start) {
                start += 1;
            }
            selected.push(&chunk[start..]);
            remaining = 0;
        }

        selected.reverse();

        let total_len: usize = selected.iter().map(|chunk| chunk.len()).sum();
        let mut snapshot = String::with_capacity(total_len);
        for chunk in selected {
            snapshot.push_str(chunk);
        }
        snapshot
    }
}

// ── Inject queue ────────────────────────────────────────────────

struct InjectMessage {
    from: String,
    text: String,
    timestamp: u64,
}

struct InjectQueue {
    messages: VecDeque<InjectMessage>,
}

impl InjectQueue {
    fn new() -> Self {
        Self {
            messages: VecDeque::new(),
        }
    }
}

struct IdleState {
    last_user_input: Instant,
    last_output_at: Instant,
    last_output_has_prompt: bool,
}

impl IdleState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            last_user_input: now,
            last_output_at: now,
            last_output_has_prompt: false,
        }
    }
}

const PROMPT_PATTERNS: &[&str] = &["❯", "> ", "$ ", "% "];
const IDLE_THRESHOLD: Duration = Duration::from_secs(2);
const OUTPUT_SETTLE: Duration = Duration::from_secs(1);
const INJECTOR_POLL: Duration = Duration::from_millis(500);
const CODEX_RESUME_SELECTOR_PATTERNS: &[&str] =
    &["Resume a previous session", "Select a conversation"];
const CODEX_RESUME_PROMPT_GUARD: Duration = Duration::from_secs(3);
const CODEX_RESUME_BUFFER_BYTES: usize = 8 * 1024;

fn has_prompt_pattern(data: &str) -> bool {
    PROMPT_PATTERNS.iter().any(|p| data.contains(p))
}

fn append_recent_text(buffer: &mut String, chunk: &str, max_bytes: usize) {
    if chunk.is_empty() {
        return;
    }

    if !buffer.is_empty() {
        buffer.push(' ');
    }
    buffer.push_str(chunk);

    if buffer.len() <= max_bytes {
        return;
    }

    let mut start = buffer.len().saturating_sub(max_bytes);
    while start < buffer.len() && !buffer.is_char_boundary(start) {
        start += 1;
    }
    buffer.drain(..start);
}

fn is_codex_resume_selector(text: &str) -> bool {
    CODEX_RESUME_SELECTOR_PATTERNS
        .iter()
        .any(|pattern| text.contains(pattern))
}

fn normalize_terminal_text(input: &str) -> String {
    let stripped = strip_ansi(input);
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn with_terminal_enter(text: &str) -> String {
    let normalized = text.trim_end_matches(|ch| matches!(ch, '\r' | '\n'));
    format!("{}\r", normalized)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct InjectEventPayload {
    workspace: String,
    from: String,
    pending: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct InjectMessageInfo {
    from: String,
    text: String,
    timestamp: u64,
}

// ── Session persistence ─────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SessionData {
    sessions: Vec<SessionEntry>,
}

#[derive(Serialize, Deserialize)]
struct SessionEntry {
    id: String,
    cwd: String,
    command: String,
    args: Vec<String>,
}

fn is_claude_session(command: &str, args: &[String]) -> bool {
    command == "claude" || (command == "telepty" && args.iter().any(|arg| arg == "claude"))
}

fn codex_resume_index(command: &str, args: &[String]) -> Option<usize> {
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

fn is_codex_resume_session(command: &str, args: &[String]) -> bool {
    codex_resume_index(command, args).is_some()
}

fn ensure_codex_resume_last_arg(command: &str, args: &[String]) -> Vec<String> {
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

fn strip_claude_continue_arg(command: &str, args: &[String]) -> Vec<String> {
    if !is_claude_session(command, args) {
        return args.to_vec();
    }

    args.iter()
        .filter(|arg| arg.as_str() != "--continue")
        .cloned()
        .collect()
}

fn restored_session_args(command: &str, cwd: &str, args: &[String]) -> Vec<String> {
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

fn is_restartable_cli(command: &str, args: &[String]) -> bool {
    if matches!(command, "claude" | "codex" | "gemini") {
        return true;
    }
    if command != "telepty" {
        return false;
    }
    args.iter()
        .any(|arg| matches!(arg.as_str(), "claude" | "codex" | "gemini"))
}

fn clone_size(size: &PtySize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

fn set_workspace_status(status: &Arc<Mutex<String>>, next: &str) {
    if let Ok(mut current) = status.lock() {
        *current = next.to_string();
    }
}

fn sessions_path() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    home.join(".aterm").join("sessions.json")
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        paths.push(path);
    }
}

fn command_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    if let Some(path_var) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path_var) {
            if !entry.as_os_str().is_empty() {
                push_unique_path(&mut paths, &mut seen, entry);
            }
        }
    }

    if let Some(home) = dirs::home_dir() {
        for relative in ["bin", ".local/bin", ".cargo/bin", ".nvm/bin"] {
            push_unique_path(&mut paths, &mut seen, home.join(relative));
        }

        let nvm_versions = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(&nvm_versions) {
            let mut version_dirs: Vec<PathBuf> = entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path().join("bin");
                    path.is_dir().then_some(path)
                })
                .collect();
            version_dirs.sort();
            version_dirs.reverse();
            for path in version_dirs {
                push_unique_path(&mut paths, &mut seen, path);
            }
        }
    }

    for path in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/bin",
        "/bin",
        "/Applications/cmux.app/Contents/Resources/bin",
    ] {
        push_unique_path(&mut paths, &mut seen, PathBuf::from(path));
    }

    paths
}

fn shell_escape_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn resolve_with_login_shell(command: &str) -> Option<PathBuf> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string());
    let output = ProcessCommand::new(shell)
        .args([
            "-lic",
            &format!("command -v -- {}", shell_escape_single(command)),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let candidate = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let resolved = PathBuf::from(candidate);
    is_executable_file(&resolved).then_some(resolved)
}

fn resolve_command_binary(command: &str) -> PathBuf {
    let candidate = PathBuf::from(command);
    if candidate.components().count() > 1 {
        return candidate;
    }

    for dir in command_search_paths() {
        let path = dir.join(command);
        if is_executable_file(&path) {
            return path;
        }
    }

    resolve_with_login_shell(command).unwrap_or(candidate)
}

fn augmented_path_env() -> Option<std::ffi::OsString> {
    std::env::join_paths(command_search_paths()).ok()
}

fn save_sessions(state: &PtyState) {
    let Ok(map) = state.0.lock() else { return };
    let sessions: Vec<SessionEntry> = map
        .values()
        .filter(|ws| {
            if ws.ephemeral {
                return false;
            }
            ws.status
                .lock()
                .map(|status| status.as_str() != "dead")
                .unwrap_or(false)
        })
        .map(|ws| SessionEntry {
            id: ws.id.clone(),
            cwd: ws.cwd.clone(),
            command: ws.command.clone(),
            args: strip_claude_continue_arg(&ws.command, &ws.args),
        })
        .collect();
    let data = SessionData { sessions };
    let path = sessions_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        let _ = std::fs::write(&path, json);
    }
}

fn restore_sessions(app: &AppHandle, state: &State<PtyState>) {
    let path = sessions_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(data) = serde_json::from_str::<SessionData>(&contents) else {
        return;
    };
    for entry in data.sessions {
        let restored_args = restored_session_args(&entry.command, &entry.cwd, &entry.args);
        let args = if restored_args.is_empty() {
            None
        } else {
            Some(restored_args)
        };
        let cmd = if entry.command.is_empty() {
            None
        } else {
            Some(entry.command)
        };
        if let Err(error) = do_create_workspace(
            entry.id,
            entry.cwd,
            cmd,
            args,
            None,
            None,
            false,
            app.clone(),
            state,
        ) {
            eprintln!("[aterm] restore_sessions skipped entry: {}", error);
        }
    }
}

struct SpawnedWorkspace {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn PtyChild + Send + Sync>,
    reader: Box<dyn Read + Send>,
}

fn spawn_workspace_process(
    cwd: &str,
    command: &str,
    args: &[String],
    size: PtySize,
) -> Result<SpawnedWorkspace, String> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(size).map_err(|e| e.to_string())?;
    let resolved_command = resolve_command_binary(command);

    let mut cmd = CommandBuilder::new(resolved_command);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    if let Some(path_env) = augmented_path_env() {
        cmd.env("PATH", path_env);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    Ok(SpawnedWorkspace {
        master: pair.master,
        writer,
        child,
        reader,
    })
}

fn respawn_workspace(app: &AppHandle, ws_id: &str) -> Result<(), String> {
    let state = app.state::<PtyState>();
    let (cwd, command, args, size, status, buffer, idle_state, master, writer, child) = {
        let map = state.0.lock().map_err(|e| e.to_string())?;
        let ws = map
            .get(ws_id)
            .ok_or_else(|| format!("Workspace '{}' not found", ws_id))?;
        if !ws.auto_restart {
            return Err(format!("Workspace '{}' is not restartable", ws_id));
        }

        set_workspace_status(&ws.status, "restarting");

        (
            ws.cwd.clone(),
            ws.command.clone(),
            ws.args.clone(),
            ws.size
                .lock()
                .map(|current| clone_size(&current))
                .unwrap_or(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                }),
            ws.status.clone(),
            ws.buffer.clone(),
            ws.idle_state.clone(),
            ws.master.clone(),
            ws.writer.clone(),
            ws.child.clone(),
        )
    };
    let codex_resume_monitor = is_codex_resume_session(&command, &args);

    let _ = app.emit("workspace-updated", ws_id);
    std::thread::sleep(Duration::from_millis(300));

    let spawned = spawn_workspace_process(&cwd, &command, &args, clone_size(&size))?;

    {
        let mut master_lock = master.lock().map_err(|e| e.to_string())?;
        *master_lock = spawned.master;
    }
    {
        let mut writer_lock = writer.lock().map_err(|e| e.to_string())?;
        *writer_lock = spawned.writer;
    }
    {
        let mut child_lock = child.lock().map_err(|e| e.to_string())?;
        *child_lock = spawned.child;
    }
    {
        let map = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(ws) = map.get(ws_id) {
            if let Ok(mut size_lock) = ws.size.lock() {
                *size_lock = size;
            }
        }
    }
    if let Ok(mut idle) = idle_state.lock() {
        *idle = IdleState::new();
    }
    set_workspace_status(&status, "running");

    let app_clone = app.clone();
    let ws_id_string = ws_id.to_string();
    std::thread::spawn(move || {
        reader_loop(
            spawned.reader,
            app_clone,
            ws_id_string,
            buffer,
            status,
            idle_state,
            writer,
            codex_resume_monitor,
        );
    });

    save_sessions(&state);
    let _ = app.emit("workspace-updated", ws_id);
    Ok(())
}

// ── Key mapping ─────────────────────────────────────────────────

fn map_key(key: &str) -> Result<&'static str, String> {
    match key.to_lowercase().as_str() {
        "return" | "enter" => Ok("\r"),
        "ctrl+c" => Ok("\x03"),
        "ctrl+d" => Ok("\x04"),
        "ctrl+z" => Ok("\x1a"),
        "tab" => Ok("\t"),
        "escape" => Ok("\x1b"),
        _ => Err(format!(
            "Unknown key: '{}'. Supported: return, ctrl+c, ctrl+d, ctrl+z, tab, escape",
            key
        )),
    }
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut escaping = false;

    for ch in input.chars() {
        if escaping {
            if ch.is_ascii_alphabetic() || matches!(ch, '~' | '\\') {
                escaping = false;
            }
            continue;
        }

        if ch == '\u{1b}' {
            escaping = true;
            continue;
        }

        output.push(ch);
    }

    output
}

fn parse_telepty_sessions_text(raw: &str) -> Vec<TeleptySessionInfo> {
    let cleaned = strip_ansi(raw);
    let mut sessions: Vec<TeleptySessionInfo> = Vec::new();
    let mut current: Option<TeleptySessionInfo> = None;

    for line in cleaned.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "Active Sessions:" {
            continue;
        }

        if let Some(id) = trimmed.strip_prefix("- ID: ") {
            if let Some(session) = current.take() {
                sessions.push(session);
            }
            current = Some(TeleptySessionInfo {
                id: id.trim().to_string(),
                status: "active".to_string(),
                ..Default::default()
            });
            continue;
        }

        let Some(session) = current.as_mut() else {
            continue;
        };

        if let Some(host) = trimmed.strip_prefix("Host: ") {
            session.host = host.trim().to_string();
        } else if let Some(command) = trimmed.strip_prefix("Command: ") {
            session.command = command.trim().to_string();
        } else if let Some(cwd) = trimmed.strip_prefix("CWD: ") {
            session.cwd = cwd.trim().to_string();
        } else if let Some(clients) = trimmed.strip_prefix("Clients: ") {
            session.clients = clients.trim().parse::<usize>().unwrap_or(0);
        } else if let Some(started) = trimmed.strip_prefix("Started: ") {
            session.started_at = started.trim().to_string();
        }
    }

    if let Some(session) = current.take() {
        sessions.push(session);
    }

    sessions
}

#[tauri::command]
fn telepty_list_sessions() -> Vec<TeleptySessionInfo> {
    let telepty = resolve_command_binary("telepty");
    let path_env = augmented_path_env();

    let run_list = |args: &[&str]| {
        let mut command = ProcessCommand::new(&telepty);
        command.args(args);
        if let Some(path_env) = path_env.as_ref() {
            command.env("PATH", path_env);
        }
        command.output()
    };

    for args in [&["list", "--json"][..], &["list"][..]] {
        let Ok(output) = run_list(args) else {
            continue;
        };
        if !output.status.success() && args == &["list", "--json"] {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim();

        if trimmed.starts_with('[') {
            if let Ok(sessions) = serde_json::from_str::<Vec<TeleptySessionInfo>>(trimmed) {
                return sessions;
            }
        }

        if trimmed.starts_with('{') {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct TeleptySessionEnvelope {
                sessions: Vec<TeleptySessionInfo>,
            }

            if let Ok(envelope) = serde_json::from_str::<TeleptySessionEnvelope>(trimmed) {
                return envelope.sessions;
            }
        }

        if !trimmed.is_empty() || output.status.success() {
            return parse_telepty_sessions_text(trimmed);
        }
    }

    eprintln!(
        "[aterm] telepty_list_sessions failed to launch binary: {}",
        telepty.display()
    );
    Vec::new()
}

// ── Core workspace creation logic ───────────────────────────────

fn do_create_workspace(
    id: String,
    cwd: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
    ephemeral: bool,
    app: AppHandle,
    state: &PtyState,
) -> Result<String, String> {
    // Check duplicate
    {
        let map = state.0.lock().map_err(|e| e.to_string())?;
        if map.contains_key(&id) {
            return Err(format!("Workspace '{}' already exists", id));
        }
    }

    let shell = command
        .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()));
    let launch_args = ensure_codex_resume_last_arg(&shell, &args.unwrap_or_default());
    let auto_restart = is_restartable_cli(&shell, &launch_args);
    let size = PtySize {
        rows: rows.unwrap_or(24),
        cols: cols.unwrap_or(80),
        pixel_width: 0,
        pixel_height: 0,
    };
    let spawned = spawn_workspace_process(&cwd, &shell, &launch_args, clone_size(&size))?;

    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(spawned.writer));
    let inject_queue: Arc<Mutex<InjectQueue>> = Arc::new(Mutex::new(InjectQueue::new()));
    let idle_state: Arc<Mutex<IdleState>> = Arc::new(Mutex::new(IdleState::new()));
    let size = Arc::new(Mutex::new(size));
    let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(spawned.master));
    let child: Arc<Mutex<Box<dyn PtyChild + Send + Sync>>> = Arc::new(Mutex::new(spawned.child));

    let buffer: Arc<Mutex<OutputBuffer>> = Arc::new(Mutex::new(OutputBuffer::new()));
    let status: Arc<Mutex<String>> = Arc::new(Mutex::new("running".to_string()));

    let now = chrono_now();

    // Clone Arc references for the reader thread BEFORE inserting into the map.
    let buf_clone = buffer.clone();
    let status_clone = status.clone();
    let app_clone = app.clone();
    let ws_id = id.clone();
    let idle_state_reader = idle_state.clone();
    let writer_for_reader = writer.clone();
    let codex_resume_monitor = is_codex_resume_session(&shell, &launch_args);

    // Clones for injector thread
    let writer_clone = writer.clone();
    let inject_queue_clone = inject_queue.clone();
    let idle_state_clone = idle_state.clone();
    let status_clone2 = status.clone();
    let app_clone2 = app.clone();
    let ws_id2 = id.clone();

    // Spawn reader thread
    std::thread::spawn(move || {
        reader_loop(
            spawned.reader,
            app_clone,
            ws_id,
            buf_clone,
            status_clone,
            idle_state_reader,
            writer_for_reader,
            codex_resume_monitor,
        );
    });

    // Spawn injector thread
    std::thread::spawn(move || {
        injector_loop(
            inject_queue_clone,
            idle_state_clone,
            writer_clone,
            status_clone2,
            app_clone2,
            ws_id2,
        );
    });

    let ws = Workspace {
        id: id.clone(),
        cwd: cwd.clone(),
        command: shell,
        args: launch_args,
        size,
        master,
        writer,
        child,
        buffer,
        created_at: now,
        status,
        inject_queue,
        idle_state,
        auto_restart,
        ephemeral,
    };

    {
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.insert(id.clone(), ws);
    }

    save_sessions(state);

    let _ = app.emit("workspace-created", &id);
    Ok(id)
}

fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    app: AppHandle,
    ws_id: String,
    buffer: Arc<Mutex<OutputBuffer>>,
    status: Arc<Mutex<String>>,
    idle_state: Arc<Mutex<IdleState>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    codex_resume_monitor: bool,
) {
    let mut buf = [0u8; 4096];
    let mut leftover: Vec<u8> = Vec::new();
    let mut combined: Vec<u8> = Vec::new();
    let mut codex_resume_recent = String::new();
    let mut codex_resume_enter_sent = false;
    let mut codex_resume_entered_at: Option<Instant> = None;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                // Combine leftover bytes from previous read with new data
                combined.clear();
                combined.extend_from_slice(&leftover);
                combined.extend_from_slice(&buf[..n]);

                // Find the last valid UTF-8 boundary
                let (valid, remainder) = split_at_utf8_boundary(&combined);
                let data = std::str::from_utf8(valid).unwrap_or("").to_string();

                // Save only the incomplete trailing bytes for the next read
                leftover = remainder.to_vec();

                if !data.is_empty() {
                    let mut block_prompt = false;
                    let mut auto_selected = false;
                    if codex_resume_monitor {
                        let normalized = normalize_terminal_text(&data);
                        if !normalized.is_empty() {
                            append_recent_text(
                                &mut codex_resume_recent,
                                &normalized,
                                CODEX_RESUME_BUFFER_BYTES,
                            );
                        }

                        let selector_visible = is_codex_resume_selector(&codex_resume_recent);
                        if selector_visible
                            && !codex_resume_enter_sent
                            && codex_resume_recent.contains("enter to resume")
                            && codex_resume_recent.contains("> ")
                        {
                            if let Ok(mut handle) = writer.lock() {
                                if handle.write_all(b"\r").is_ok() && handle.flush().is_ok() {
                                    codex_resume_enter_sent = true;
                                    codex_resume_entered_at = Some(Instant::now());
                                    codex_resume_recent.clear();
                                    auto_selected = true;
                                }
                            }
                        }

                        block_prompt = selector_visible
                            || codex_resume_entered_at
                                .map(|instant| instant.elapsed() < CODEX_RESUME_PROMPT_GUARD)
                                .unwrap_or(false);
                    }

                    // Keep a raw PTY stream buffer for initial screen restore.
                    if let Ok(mut b) = buffer.lock() {
                        b.push(data.clone());
                    }
                    // Emit to frontend
                    let _ = app.emit(
                        "pty-output",
                        PtyOutputPayload {
                            workspace: ws_id.clone(),
                            data: data.clone(),
                        },
                    );

                    // Update idle state with prompt detection
                    if let Ok(mut idle) = idle_state.lock() {
                        let now = Instant::now();
                        idle.last_output_at = now;
                        if auto_selected {
                            idle.last_user_input = now;
                        }
                        idle.last_output_has_prompt = !block_prompt && has_prompt_pattern(&data);
                    }
                }
            }
            Err(e) => {
                // EINTR is transient — retry the read
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // Any other error is fatal — exit the loop
                eprintln!("[aterm] reader error for {}: {}", ws_id, e);
                break;
            }
        }
    }
    let state = app.state::<PtyState>();
    let should_restart = {
        let Ok(map) = state.0.lock() else {
            return;
        };
        let Some(ws) = map.get(&ws_id) else {
            return;
        };
        let current_status = ws
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "dead".to_string());
        current_status != "closing" && ws.auto_restart
    };

    if should_restart && respawn_workspace(&app, &ws_id).is_ok() {
        return;
    }

    set_workspace_status(&status, "dead");
    save_sessions(&state);
    let _ = app.emit("workspace-updated", &ws_id);
    let _ = app.emit("workspace-closed", &ws_id);
}

fn injector_loop(
    inject_queue: Arc<Mutex<InjectQueue>>,
    idle_state: Arc<Mutex<IdleState>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    status: Arc<Mutex<String>>,
    app: AppHandle,
    ws_id: String,
) {
    loop {
        std::thread::sleep(INJECTOR_POLL);

        // Exit if workspace is closed or permanently dead
        if let Ok(s) = status.lock() {
            if matches!(s.as_str(), "dead" | "closing") {
                break;
            }
        }

        // Check idle conditions
        let should_inject = {
            let idle = idle_state.lock().ok();
            let queue = inject_queue.lock().ok();
            match (idle, queue) {
                (Some(idle), Some(queue)) => {
                    idle.last_output_has_prompt
                        && idle.last_output_at.elapsed() >= OUTPUT_SETTLE
                        && idle.last_user_input.elapsed() >= IDLE_THRESHOLD
                        && !queue.messages.is_empty()
                }
                _ => false,
            }
        };

        if should_inject {
            // Pop message
            let msg = inject_queue
                .lock()
                .ok()
                .and_then(|mut q| q.messages.pop_front());

            if let Some(msg) = msg {
                // Write to PTY
                if let Ok(mut w) = writer.lock() {
                    let payload = with_terminal_enter(&msg.text);
                    let _ = w.write_all(payload.as_bytes());
                    let _ = w.flush();
                }

                // Reset prompt detection
                if let Ok(mut idle) = idle_state.lock() {
                    idle.last_output_has_prompt = false;
                }

                // Get remaining count
                let pending = inject_queue
                    .lock()
                    .ok()
                    .map(|q| q.messages.len())
                    .unwrap_or(0);

                // Emit inject-delivered
                let _ = app.emit(
                    "inject-delivered",
                    InjectEventPayload {
                        workspace: ws_id.clone(),
                        from: msg.from,
                        pending,
                    },
                );
            }
        }
    }
}

/// Split a byte slice at the last valid UTF-8 character boundary.
/// Returns (valid_utf8_bytes, remaining_incomplete_bytes).
fn split_at_utf8_boundary(bytes: &[u8]) -> (&[u8], &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(_) => (bytes, &[]),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            (&bytes[..valid_up_to], &bytes[valid_up_to..])
        }
    }
}

/// Simple ISO-8601-ish timestamp without pulling in the chrono crate.
fn chrono_now() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Return Unix-epoch seconds as a string; the frontend can format it.
    format!("{}", secs)
}

// ── Tauri commands ──────────────────────────────────────────────

#[tauri::command]
fn list_workspaces(state: State<PtyState>) -> Vec<WorkspaceInfo> {
    let Ok(map) = state.0.lock() else {
        return Vec::new();
    };
    map.values()
        .map(|ws| {
            let status = ws
                .status
                .lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "unknown".to_string());
            let buffer_lines = ws.buffer.lock().map(|b| b.chunks.len()).unwrap_or(0);
            WorkspaceInfo {
                id: ws.id.clone(),
                cwd: ws.cwd.clone(),
                command: ws.command.clone(),
                args: ws.args.clone(),
                status,
                created_at: ws.created_at.clone(),
                buffer_lines,
            }
        })
        .collect()
}

#[tauri::command]
fn create_workspace(
    id: String,
    cwd: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
    ephemeral: bool,
    app: AppHandle,
    state: State<PtyState>,
) -> Result<String, String> {
    do_create_workspace(id, cwd, command, args, cols, rows, ephemeral, app, &state)
}

#[tauri::command]
fn close_workspace(id: String, state: State<PtyState>) -> Result<(), String> {
    let mut map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .remove(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;

    set_workspace_status(&ws.status, "closing");

    // Kill the child process — ignore errors (may already be dead).
    if let Ok(mut child) = ws.child.lock() {
        let _ = child.kill();
    }

    // Dropping `ws` closes the writer → reader thread gets EOF → marks status "dead".
    drop(ws);

    // Release the lock before saving (save_sessions acquires its own lock).
    drop(map);
    save_sessions(&state);

    Ok(())
}

#[tauri::command]
fn send_to_workspace(id: String, text: String, state: State<PtyState>) -> Result<(), String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .get(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;

    // Update idle state: user is typing
    if let Ok(mut idle) = ws.idle_state.lock() {
        idle.last_user_input = Instant::now();
        idle.last_output_has_prompt = false;
    }

    let mut writer = ws.writer.lock().map_err(|e| e.to_string())?;
    writer
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn send_key(id: String, key: String, state: State<PtyState>) -> Result<(), String> {
    let mapped = map_key(&key)?;
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .get(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;

    // Update idle state: user is typing
    if let Ok(mut idle) = ws.idle_state.lock() {
        idle.last_user_input = Instant::now();
        idle.last_output_has_prompt = false;
    }

    let mut writer = ws.writer.lock().map_err(|e| e.to_string())?;
    writer
        .write_all(mapped.as_bytes())
        .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn read_screen(
    id: String,
    max_bytes: Option<usize>,
    state: State<PtyState>,
) -> Result<String, String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .get(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;
    let buf = ws.buffer.lock().map_err(|e| e.to_string())?;
    let bytes = max_bytes
        .unwrap_or(DEFAULT_SNAPSHOT_BYTES)
        .min(BUFFER_MAX_BYTES);
    Ok(buf.snapshot(bytes))
}

#[tauri::command]
fn resize_workspace(
    id: String,
    cols: u16,
    rows: u16,
    state: State<PtyState>,
) -> Result<(), String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .get(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;
    let next_size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    ws.master
        .lock()
        .map_err(|e| e.to_string())?
        .resize(clone_size(&next_size))
        .map_err(|e| e.to_string())?;
    if let Ok(mut size) = ws.size.lock() {
        *size = next_size;
    }
    Ok(())
}

/// Check if Claude Code has conversation history for a given project cwd.
/// Claude stores history at ~/.claude/projects/-{path-segments-joined-by-dash}/
/// Returns true if that directory exists and contains at least one .jsonl file.
fn check_claude_history(cwd: &str) -> bool {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    // Convert "/Users/foo/projects/bar" → "-Users-foo-projects-bar"
    let key = cwd.replace('/', "-");
    let history_dir = home.join(".claude").join("projects").join(&key);
    if !history_dir.is_dir() {
        return false;
    }
    // Check for at least one .jsonl conversation file
    std::fs::read_dir(&history_dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().map_or(false, |ext| ext == "jsonl"))
        })
        .unwrap_or(false)
}

#[tauri::command]
fn has_claude_history(cwd: String) -> bool {
    check_claude_history(&cwd)
}

#[tauri::command]
fn debug_log(msg: String) {
    eprintln!("[aterm-js] {}", msg);
}

#[tauri::command]
fn queue_inject(
    id: String,
    from: String,
    text: String,
    app: AppHandle,
    state: State<PtyState>,
) -> Result<usize, String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .get(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let pending = {
        let mut queue = ws.inject_queue.lock().map_err(|e| e.to_string())?;
        queue.messages.push_back(InjectMessage {
            from: from.clone(),
            text,
            timestamp,
        });
        queue.messages.len()
    };

    let _ = app.emit(
        "inject-queued",
        InjectEventPayload {
            workspace: id,
            from,
            pending,
        },
    );

    Ok(pending)
}

#[tauri::command]
fn peek_queue(id: String, state: State<PtyState>) -> Result<Vec<InjectMessageInfo>, String> {
    let map = state.0.lock().map_err(|e| e.to_string())?;
    let ws = map
        .get(&id)
        .ok_or_else(|| format!("Workspace '{}' not found", id))?;
    let queue = ws.inject_queue.lock().map_err(|e| e.to_string())?;
    Ok(queue
        .messages
        .iter()
        .map(|m| InjectMessageInfo {
            from: m.from.clone(),
            text: m.text.clone(),
            timestamp: m.timestamp,
        })
        .collect())
}

#[tauri::command]
fn suggest_paths(query: String) -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
    let home_str = home.to_string_lossy().to_string();

    let mut results: Vec<String> = Vec::new();

    if query.starts_with('/') || query.starts_with('~') {
        // Absolute / home-relative path completion
        let expanded = if query.starts_with('~') {
            query.replacen('~', &home_str, 1)
        } else {
            query.clone()
        };

        let path = std::path::Path::new(&expanded);
        let (parent, prefix) = if expanded.ends_with('/') {
            (path, "")
        } else {
            (
                path.parent().unwrap_or(std::path::Path::new("/")),
                path.file_name().and_then(|f| f.to_str()).unwrap_or(""),
            )
        };

        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    continue;
                }
                if !prefix.is_empty() && !name_str.starts_with(prefix) {
                    continue;
                }
                let full = entry.path().to_string_lossy().to_string();
                let display = full.replacen(&home_str, "~", 1);
                results.push(display);
            }
        }
    } else {
        // Search ~/projects/ for matching directory names
        let projects = home.join("projects");
        if let Ok(entries) = std::fs::read_dir(&projects) {
            let query_lower = query.to_lowercase();
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if !ft.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    continue;
                }
                if name_str.to_lowercase().contains(&query_lower) {
                    let display = format!("~/projects/{}", name_str);
                    results.push(display);
                }
            }
        }
    }

    results.sort();
    results.truncate(10);
    results
}

// ── Entrypoint ──────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(PtyState(Mutex::new(HashMap::new())))
        .invoke_handler(tauri::generate_handler![
            list_workspaces,
            create_workspace,
            close_workspace,
            send_to_workspace,
            send_key,
            read_screen,
            resize_workspace,
            queue_inject,
            peek_queue,
            suggest_paths,
            telepty_list_sessions,
            has_claude_history,
            debug_log,
        ])
        .setup(|app| {
            restore_sessions(&app.handle(), &app.state::<PtyState>());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}
