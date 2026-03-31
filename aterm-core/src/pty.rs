use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use tokio::sync::Notify;

use crate::inject::{
    has_prompt_pattern, normalize_terminal_text, run_injector_loop, split_at_utf8_boundary,
    IdleState, InjectMessage, InjectMessageInfo, InjectQueue, SharedInjectQueue,
};
use crate::session::{restored_session_args, strip_claude_continue_arg, SessionEntry};

pub const BUFFER_MAX_BYTES: usize = 1024 * 1024;
pub const DEFAULT_SNAPSHOT_BYTES: usize = 256 * 1024;
const CODEX_RESUME_BUFFER_BYTES: usize = 8 * 1024;

pub type SharedPtyManager = Arc<Mutex<PtyManager>>;
pub type PtyByteQueue = Arc<Mutex<Vec<u8>>>;

type WakeCallback = Arc<std::sync::OnceLock<Box<dyn Fn() + Send + Sync>>>;

#[derive(Clone)]
pub struct PtyOutputSignal {
    notify: Arc<Notify>,
    dirty: Arc<AtomicBool>,
    wake_callback: WakeCallback,
}

impl Default for PtyOutputSignal {
    fn default() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            dirty: Arc::new(AtomicBool::new(false)),
            wake_callback: Arc::new(std::sync::OnceLock::new()),
        }
    }
}

impl PtyOutputSignal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a callback invoked on every mark_dirty() — used to wake the
    /// platform event loop (e.g. via winit EventLoopProxy).
    pub fn set_wake_callback(&self, cb: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake_callback.set(Box::new(cb));
    }

    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
        self.notify.notify_one();
        if let Some(f) = self.wake_callback.get() {
            f();
        }
    }

    pub fn notified(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let notify = self.notify.clone();
        async move {
            notify.notified().await;
        }
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn has_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    pub fn poke(&self) {
        self.notify.notify_one();
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
    pub status: String,
    pub created_at: String,
    pub buffer_lines: usize,
}

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
    term_bytes: PtyByteQueue,
    created_at: String,
    status: Arc<Mutex<String>>,
    inject_queue: SharedInjectQueue,
    idle_state: Arc<Mutex<IdleState>>,
    auto_restart: bool,
    ephemeral: bool,
}

pub struct PtyManager {
    workspaces: HashMap<String, Workspace>,
    pty_signal: PtyOutputSignal,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self {
            workspaces: HashMap::new(),
            pty_signal: PtyOutputSignal::new(),
        }
    }
}

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

    fn clear(&mut self) {
        self.chunks.clear();
        self.bytes = 0;
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

struct SpawnedWorkspace {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn PtyChild + Send + Sync>,
    reader: Box<dyn Read + Send>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> SharedPtyManager {
        Arc::new(Mutex::new(Self::new()))
    }

    pub fn output_signal(&self) -> PtyOutputSignal {
        self.pty_signal.clone()
    }

    pub fn create(
        &mut self,
        id: String,
        cwd: String,
        command: Option<String>,
        args: Option<Vec<String>>,
        cols: Option<u16>,
        rows: Option<u16>,
        ephemeral: bool,
    ) -> Result<String, String> {
        if self.workspaces.contains_key(&id) {
            return Err(format!("Workspace '{}' already exists", id));
        }

        let shell = command
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()));
        let launch_args = args.unwrap_or_default();
        let launch_args = crate::session::ensure_codex_resume_last_arg(&shell, &launch_args);
        let auto_restart = is_restartable_cli(&shell, &launch_args);
        let size = PtySize {
            rows: rows.unwrap_or(24),
            cols: cols.unwrap_or(80),
            pixel_width: 0,
            pixel_height: 0,
        };
        let spawned = spawn_workspace_process(&cwd, &shell, &launch_args, clone_size(&size))?;

        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(spawned.writer));
        let inject_queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));
        let idle_state: Arc<Mutex<IdleState>> = Arc::new(Mutex::new(IdleState::new()));
        let size = Arc::new(Mutex::new(size));
        let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(spawned.master));
        let child: Arc<Mutex<Box<dyn PtyChild + Send + Sync>>> = Arc::new(Mutex::new(spawned.child));
        let buffer: Arc<Mutex<OutputBuffer>> = Arc::new(Mutex::new(OutputBuffer::new()));
        let term_bytes: PtyByteQueue = Arc::new(Mutex::new(Vec::new()));
        let status: Arc<Mutex<String>> = Arc::new(Mutex::new("running".to_string()));
        let now = chrono_now();

        let reader_buffer = buffer.clone();
        let reader_term_bytes = term_bytes.clone();
        let reader_status = status.clone();
        let reader_idle = idle_state.clone();
        let reader_writer = writer.clone();
        let codex_resume_monitor = is_codex_resume_session(&shell, &launch_args);
        let ws_id = id.clone();
        let reader_signal = self.pty_signal.clone();
        let reader_cwd = cwd.clone();
        let reader_command = shell.clone();
        let reader_args = launch_args.clone();
        let reader_size = size.clone();
        let reader_master = master.clone();
        let reader_child = child.clone();
        let reader_inject_queue = inject_queue.clone();

        thread::spawn(move || {
            reader_loop(
                spawned.reader,
                ws_id,
                reader_buffer,
                reader_term_bytes,
                reader_status,
                reader_idle,
                reader_writer,
                codex_resume_monitor,
                reader_signal,
                auto_restart,
                reader_cwd,
                reader_command,
                reader_args,
                reader_size,
                reader_master,
                reader_child,
                reader_inject_queue,
            );
        });

        let injector_queue = inject_queue.clone();
        let injector_idle = idle_state.clone();
        let injector_writer = writer.clone();
        let injector_status = status.clone();
        thread::spawn(move || {
            run_injector_loop(
                injector_queue,
                injector_idle,
                injector_writer,
                injector_status,
            );
        });

        let workspace = Workspace {
            id: id.clone(),
            cwd,
            command: shell,
            args: launch_args,
            size,
            master,
            writer,
            child,
            buffer,
            term_bytes,
            created_at: now,
            status,
            inject_queue,
            idle_state,
            auto_restart,
            ephemeral,
        };

        self.workspaces.insert(id.clone(), workspace);
        eprintln!("[PTY] workspace created: {}", id);
        Ok(id)
    }

    pub fn restore_session_entry(&mut self, entry: SessionEntry) -> Result<String, String> {
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

        self.create(entry.id, entry.cwd, cmd, args, None, None, false)
    }

    pub fn close(&mut self, id: &str) -> Result<(), String> {
        let ws = self
            .workspaces
            .remove(id)
            .ok_or_else(|| format!("Workspace '{}' not found", id))?;

        set_workspace_status(&ws.status, "closing");

        if let Ok(mut child) = ws.child.lock() {
            let _ = child.kill();
        }

        Ok(())
    }

    pub fn send_to_workspace(&self, id: &str, text: &str) -> Result<(), String> {
        let ws = self.workspace(id)?;
        if let Ok(mut idle) = ws.idle_state.lock() {
            idle.record_user_input();
        }

        let mut writer = ws.writer.lock().map_err(|error| error.to_string())?;
        writer
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())
    }

    pub fn send_key(&self, id: &str, key: &str) -> Result<(), String> {
        let mapped = map_key(key)?;
        self.send_to_workspace(id, mapped)
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let ws = self.workspace(id)?;
        let next_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        ws.master
            .lock()
            .map_err(|error| error.to_string())?
            .resize(clone_size(&next_size))
            .map_err(|error| error.to_string())?;

        if let Ok(mut size) = ws.size.lock() {
            *size = next_size;
        }

        Ok(())
    }

    pub fn read_screen(&self, id: &str, max_bytes: Option<usize>) -> Result<String, String> {
        let ws = self.workspace(id)?;
        let buffer = ws.buffer.lock().map_err(|error| error.to_string())?;
        let bytes = max_bytes.unwrap_or(DEFAULT_SNAPSHOT_BYTES).min(BUFFER_MAX_BYTES);
        Ok(buffer.snapshot(bytes))
    }

    pub fn queue_inject(&self, id: &str, from: &str, text: String) -> Result<usize, String> {
        let ws = self.workspace(id)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let pending = {
            let mut queue = ws.inject_queue.lock().map_err(|error| error.to_string())?;
            queue.push(InjectMessage {
                from: from.to_string(),
                text,
                timestamp,
            })
        };

        Ok(pending)
    }

    pub fn peek_queue(&self, id: &str) -> Result<Vec<InjectMessageInfo>, String> {
        let ws = self.workspace(id)?;
        let queue = ws.inject_queue.lock().map_err(|error| error.to_string())?;
        Ok(queue.snapshot())
    }

    pub fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .values()
            .map(|ws| WorkspaceInfo {
                id: ws.id.clone(),
                cwd: ws.cwd.clone(),
                command: ws.command.clone(),
                args: ws.args.clone(),
                status: ws
                    .status
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|_| "unknown".to_string()),
                created_at: ws.created_at.clone(),
                buffer_lines: ws.buffer.lock().map(|b| b.chunks.len()).unwrap_or(0),
            })
            .collect()
    }

    pub fn session_entries(&self) -> Vec<SessionEntry> {
        self.workspaces
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
            .collect()
    }

    pub fn drain_term_bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        let ws = self.workspace(id)?;
        let mut queue = ws.term_bytes.lock().map_err(|e| e.to_string())?;
        Ok(std::mem::take(&mut *queue))
    }

    /// Get a clone of the workspace's PTY writer for terminal write-back (DA responses, etc.).
    pub fn workspace_writer(&self, id: &str) -> Option<Arc<Mutex<Box<dyn Write + Send>>>> {
        self.workspaces.get(id).map(|ws| ws.writer.clone())
    }

    pub fn workspace_is_alive(&self, id: &str) -> bool {
        let Ok(ws) = self.workspace(id) else {
            return false;
        };

        let alive = ws
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.try_wait().ok())
            .map(|status| status.is_none())
            .unwrap_or_else(|| {
                ws.status
                    .lock()
                    .map(|status| status.as_str() != "dead")
                    .unwrap_or(false)
            });

        if !alive {
            set_workspace_status(&ws.status, "dead");
        }

        alive
    }

    fn workspace(&self, id: &str) -> Result<&Workspace, String> {
        self.workspaces
            .get(id)
            .ok_or_else(|| format!("Workspace '{}' not found", id))
    }
}

fn set_workspace_status(status: &Arc<Mutex<String>>, next: &str) {
    if let Ok(mut current) = status.lock() {
        *current = next.to_string();
    }
}

fn spawn_workspace_process(
    cwd: &str,
    command: &str,
    args: &[String],
    size: PtySize,
) -> Result<SpawnedWorkspace, String> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(size).map_err(|error| error.to_string())?;
    let resolved_command = resolve_command_binary(command);

    let mut cmd = CommandBuilder::new(resolved_command);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "aterm");
    cmd.env("TERM_PROGRAM_VERSION", "3.0");
    if let Some(path_env) = augmented_path_env() {
        cmd.env("PATH", path_env);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|error| error.to_string())?;
    let reader = pair.master.try_clone_reader().map_err(|error| error.to_string())?;
    let writer = pair.master.take_writer().map_err(|error| error.to_string())?;

    Ok(SpawnedWorkspace {
        master: pair.master,
        writer,
        child,
        reader,
    })
}

fn try_restart_workspace(
    ws_id: &str,
    cwd: &str,
    command: &str,
    args: &[String],
    size: &Arc<Mutex<PtySize>>,
    master: &Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    child: &Arc<Mutex<Box<dyn PtyChild + Send + Sync>>>,
    buffer: &Arc<Mutex<OutputBuffer>>,
    term_bytes: &PtyByteQueue,
    status: &Arc<Mutex<String>>,
    idle_state: &Arc<Mutex<IdleState>>,
    inject_queue: &SharedInjectQueue,
    signal: &PtyOutputSignal,
) -> bool {
    eprintln!("[PTY] auto-restart: attempting respawn for {}", ws_id);

    let current_size = size.lock().ok().map(|s| clone_size(&*s)).unwrap_or(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    });

    let spawned = match spawn_workspace_process(cwd, command, args, current_size) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[PTY] auto-restart failed for {}: {}", ws_id, e);
            return false;
        }
    };

    // Swap arcs
    if let Ok(mut m) = master.lock() {
        *m = spawned.master;
    }
    if let Ok(mut w) = writer.lock() {
        *w = spawned.writer;
    }
    if let Ok(mut c) = child.lock() {
        *c = spawned.child;
    }
    if let Ok(mut s) = status.lock() {
        *s = "running".to_string();
    }
    if let Ok(mut b) = buffer.lock() {
        b.clear();
    }
    if let Ok(mut tb) = term_bytes.lock() {
        tb.clear();
    }
    if let Ok(mut idle) = idle_state.lock() {
        *idle = IdleState::new();
    }
    if let Ok(mut q) = inject_queue.lock() {
        q.clear();
    }

    // Spawn new reader thread
    let reader_buffer = buffer.clone();
    let reader_term_bytes = term_bytes.clone();
    let reader_status = status.clone();
    let reader_idle = idle_state.clone();
    let reader_writer = writer.clone();
    let reader_signal = signal.clone();
    let ws_id_clone = ws_id.to_string();
    let codex_resume = crate::session::codex_resume_index(command, args).is_some();
    let restart_cwd = cwd.to_string();
    let restart_command = command.to_string();
    let restart_args = args.to_vec();
    let restart_size = size.clone();
    let restart_master = master.clone();
    let restart_child = child.clone();
    let restart_inject_queue = inject_queue.clone();

    thread::spawn(move || {
        reader_loop(
            spawned.reader,
            ws_id_clone,
            reader_buffer,
            reader_term_bytes,
            reader_status,
            reader_idle,
            reader_writer,
            codex_resume,
            reader_signal,
            true,
            restart_cwd,
            restart_command,
            restart_args,
            restart_size,
            restart_master,
            restart_child,
            restart_inject_queue,
        );
    });

    // Spawn new injector
    let injector_queue = inject_queue.clone();
    let injector_idle = idle_state.clone();
    let injector_writer = writer.clone();
    let injector_status = status.clone();
    thread::spawn(move || {
        run_injector_loop(injector_queue, injector_idle, injector_writer, injector_status);
    });

    signal.mark_dirty();
    eprintln!("[PTY] auto-restart: respawned {}", ws_id);
    true
}

fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    ws_id: String,
    buffer: Arc<Mutex<OutputBuffer>>,
    term_bytes: PtyByteQueue,
    status: Arc<Mutex<String>>,
    idle_state: Arc<Mutex<IdleState>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    codex_resume_monitor: bool,
    signal: PtyOutputSignal,
    auto_restart: bool,
    cwd: String,
    command: String,
    args: Vec<String>,
    size: Arc<Mutex<PtySize>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn PtyChild + Send + Sync>>>,
    inject_queue: SharedInjectQueue,
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
                // Feed RAW bytes to VTE parser immediately — no UTF-8 filtering.
                // Prevents escape sequence loss when non-UTF-8 bytes are present.
                if let Ok(mut tb) = term_bytes.lock() {
                    tb.extend_from_slice(&buf[..n]);
                }
                signal.mark_dirty();

                // UTF-8 safe path for display buffer + codex resume monitoring
                combined.clear();
                combined.extend_from_slice(&leftover);
                combined.extend_from_slice(&buf[..n]);

                let (valid, remainder) = split_at_utf8_boundary(&combined);
                let data = std::str::from_utf8(valid).unwrap_or("").to_string();
                leftover = remainder.to_vec();
                // Safety: prevent leftover from growing unbounded with stuck invalid bytes
                if leftover.len() > 4 && std::str::from_utf8(&leftover).is_err() {
                    leftover.clear();
                }

                if data.is_empty() {
                    continue;
                }

                if let Ok(mut b) = buffer.lock() {
                    b.push(data.clone());
                }

                if codex_resume_monitor {
                    let normalized = normalize_terminal_text(&data);
                    if !normalized.is_empty() {
                        append_recent_text(
                            &mut codex_resume_recent,
                            &normalized,
                            CODEX_RESUME_BUFFER_BYTES,
                        );
                    }

                    if codex_resume_recent.contains("enter to resume")
                        && codex_resume_recent.contains("> ")
                        && !codex_resume_enter_sent
                    {
                        if let Ok(mut handle) = writer.lock() {
                            if handle.write_all(b"\r").is_ok() && handle.flush().is_ok() {
                                codex_resume_enter_sent = true;
                                codex_resume_entered_at = Some(Instant::now());
                                codex_resume_recent.clear();
                            }
                        }
                    }
                }

                if let Ok(mut idle) = idle_state.lock() {
                    idle.record_output(has_prompt_pattern(&data));
                }

            }
            Err(error) => {
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                eprintln!("[aterm] reader error for {}: {}", ws_id, error);
                break;
            }
        }
    }

    if let Ok(mut current) = status.lock() {
        if current.as_str() != "closing" {
            *current = "dead".to_string();
        }
    }

    if auto_restart {
        let restarted = try_restart_workspace(
            &ws_id, &cwd, &command, &args,
            &size, &master, &writer, &child,
            &buffer, &term_bytes, &status, &idle_state, &inject_queue, &signal,
        );
        if restarted {
            return; // New reader thread is running
        }
    }

    let _ = codex_resume_entered_at;
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

fn is_codex_resume_session(command: &str, args: &[String]) -> bool {
    crate::session::codex_resume_index(command, args).is_some()
}

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

fn clone_size(size: &PtySize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

pub fn command_search_paths() -> Vec<PathBuf> {
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

fn push_unique_path(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        paths.push(path);
    }
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

pub fn resolve_command_binary(command: &str) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::PtyOutputSignal;

    #[test]
    fn pty_output_signal_coalesces_until_taken() {
        let signal = PtyOutputSignal::new();

        assert!(!signal.has_dirty());
        assert!(!signal.take_dirty());

        signal.mark_dirty();
        signal.mark_dirty();

        assert!(signal.has_dirty());
        assert!(signal.take_dirty());
        assert!(!signal.has_dirty());
        assert!(!signal.take_dirty());
    }

    #[test]
    fn pty_output_signal_can_be_rearmed() {
        let signal = PtyOutputSignal::new();

        signal.mark_dirty();
        assert!(signal.take_dirty());

        signal.mark_dirty();
        assert!(signal.has_dirty());
        assert!(signal.take_dirty());
    }
}

pub fn augmented_path_env() -> Option<std::ffi::OsString> {
    std::env::join_paths(command_search_paths()).ok()
}
