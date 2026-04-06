use portable_pty::{native_pty_system, Child as PtyChild, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub type WorkspaceStatus = Arc<(Mutex<String>, Condvar)>;
use tokio::sync::Notify;

use crate::inject::{
    close_writer_handle, detect_osc133, has_prompt_pattern, normalize_terminal_text,
    run_injector_loop, split_at_utf8_boundary, IdleState, InjectMessage, InjectMessageInfo,
    InjectQueue, InjectSignal, Osc133Mark, SharedInjectQueue,
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
    stopped: Arc<AtomicBool>,
}

impl Default for PtyOutputSignal {
    fn default() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            dirty: Arc::new(AtomicBool::new(false)),
            wake_callback: Arc::new(std::sync::OnceLock::new()),
            stopped: Arc::new(AtomicBool::new(false)),
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
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
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

    /// Signal that the host view is being deallocated. After this call,
    /// mark_dirty() will no longer invoke the wake callback.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
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
    status: WorkspaceStatus,
    inject_queue: SharedInjectQueue,
    inject_signal: InjectSignal,
    idle_state: Arc<Mutex<IdleState>>,
    auto_restart: bool,
    restart_count: Arc<AtomicU32>,
    ephemeral: bool,
    custom_command: Option<String>,
    is_system: bool,
    resume_command: Option<String>,
    /// Pending OSC 133 marks detected in reader_loop, drained by sync_pty.
    pending_osc133: crate::terminal::PendingOsc133,
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
        custom_command: Option<String>,
        is_system: bool,
        resume_command: Option<String>,
        pending_osc133_handle: Option<crate::terminal::PendingOsc133>,
    ) -> Result<String, String> {
        if self.workspaces.contains_key(&id) {
            return Err(format!("Workspace '{}' already exists", id));
        }

        let shell = command.unwrap_or_else(|| {
            read_default_shell()
                .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()))
        });
        let launch_args = args.unwrap_or_default();
        let launch_args = crate::session::ensure_codex_resume_last_arg(&shell, &launch_args);
        let auto_restart = is_restartable_cli(&shell, &launch_args);
        let size = PtySize {
            rows: rows.unwrap_or(24),
            cols: cols.unwrap_or(80),
            pixel_width: 0,
            pixel_height: 0,
        };
        let spawned =
            spawn_workspace_process(&cwd, &shell, &launch_args, clone_size(&size), Some(&id))?;

        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(spawned.writer));
        let inject_queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));
        let inject_signal = InjectSignal::new();
        let idle_state: Arc<Mutex<IdleState>> = Arc::new(Mutex::new(IdleState::new()));
        let size = Arc::new(Mutex::new(size));
        let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(spawned.master));
        let child: Arc<Mutex<Box<dyn PtyChild + Send + Sync>>> =
            Arc::new(Mutex::new(spawned.child));
        let buffer: Arc<Mutex<OutputBuffer>> = Arc::new(Mutex::new(OutputBuffer::new()));
        let term_bytes: PtyByteQueue = Arc::new(Mutex::new(Vec::new()));
        let status: WorkspaceStatus = Arc::new((Mutex::new("running".to_string()), Condvar::new()));
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
        let reader_inject_signal = inject_signal.clone();
        let restart_count: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let reader_restart_count = restart_count.clone();
        let pending_osc133: crate::terminal::PendingOsc133 =
            pending_osc133_handle.unwrap_or_else(|| Arc::new(Mutex::new(Vec::new())));
        debug_log!(
            "[osc133-debug] create: pending_osc133 Arc ptr={:p}, from_terminal={}",
            Arc::as_ptr(&pending_osc133),
            pending_osc133.lock().map(|_| true).unwrap_or(false)
        );
        let reader_pending_osc133 = pending_osc133.clone();

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
                reader_inject_signal,
                reader_restart_count,
                reader_pending_osc133,
            );
        });

        let injector_queue = inject_queue.clone();
        let injector_idle = idle_state.clone();
        let injector_writer = writer.clone();
        let injector_status = status.clone();
        let injector_signal = inject_signal.clone();
        thread::spawn(move || {
            run_injector_loop(
                injector_queue,
                injector_idle,
                injector_writer,
                injector_status,
                injector_signal,
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
            inject_signal,
            idle_state,
            auto_restart,
            restart_count,
            ephemeral,
            custom_command,
            is_system,
            resume_command,
            pending_osc133,
        };

        self.workspaces.insert(id.clone(), workspace);
        log_stderr!("[PTY] workspace created: {}", id);
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

        self.create(
            entry.id,
            entry.cwd,
            cmd,
            args,
            None,
            None,
            false,
            entry.custom_command,
            entry.is_system,
            entry.resume_command,
            None, // no terminal yet during restore
        )
    }

    pub fn close(&mut self, id: &str) -> Result<(), String> {
        let ws = self
            .workspaces
            .remove(id)
            .ok_or_else(|| format!("Workspace '{}' not found", id))?;

        set_workspace_status(&ws.status, "closing");

        // Notify IPC subscribers about the status transition
        if let Ok(app) = crate::global_app().lock() {
            app.broadcast_workspace_event(&serde_json::json!({
                "type": "StatusChanged",
                "id": id,
                "status": "closing"
            }));
        }

        if let Ok(mut child) = ws.child.lock() {
            let _ = child.kill();
        }

        Ok(())
    }

    pub fn send_to_workspace(&self, id: &str, text: &str) -> Result<(), String> {
        let ws = self.workspace(id)?;
        if !workspace_accepts_input(ws) {
            return Err(format!("workspace '{}' is dead", id));
        }
        if let Ok(mut idle) = ws.idle_state.lock() {
            idle.record_user_input();
        }

        let write_result = {
            let mut writer = ws.writer.lock().map_err(|error| error.to_string())?;
            writer
                .write_all(text.as_bytes())
                .and_then(|_| writer.flush())
        };

        match write_result {
            Ok(()) => Ok(()),
            Err(error) => {
                if should_mark_workspace_dead(&error) {
                    mark_workspace_dead(ws);
                    return Err(format!("workspace '{}' is dead", id));
                }
                Err(error.to_string())
            }
        }
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
        let bytes = max_bytes
            .unwrap_or(DEFAULT_SNAPSHOT_BYTES)
            .min(BUFFER_MAX_BYTES);
        Ok(buffer.snapshot(bytes))
    }

    pub fn queue_inject(&self, id: &str, from: &str, text: String) -> Result<usize, String> {
        let ws = self.workspace(id)?;
        if !workspace_accepts_input(ws) {
            return Err(format!("workspace '{}' is dead", id));
        }
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
                enqueued_at: Instant::now(),
            })?
        };

        // Wake the injector loop — message enqueued
        ws.inject_signal.notify();

        Ok(pending)
    }

    /// Get a clone of the inject queue for a workspace (for external dispatch).
    pub fn inject_queue_for(&self, id: &str) -> Option<crate::inject::SharedInjectQueue> {
        self.workspaces.get(id).map(|ws| ws.inject_queue.clone())
    }

    /// Get the inject signal for a workspace (to wake injector on enqueue).
    pub fn inject_signal_for(&self, id: &str) -> Option<InjectSignal> {
        self.workspaces.get(id).map(|ws| ws.inject_signal.clone())
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
                    .0
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
                    .0
                    .lock()
                    .map(|status| status.as_str() != "dead")
                    .unwrap_or(false)
            })
            .map(|ws| SessionEntry {
                id: ws.id.clone(),
                cwd: ws.cwd.clone(),
                command: ws.command.clone(),
                args: strip_claude_continue_arg(&ws.command, &ws.args),
                custom_command: ws.custom_command.clone(),
                is_system: ws.is_system,
                resume_command: ws.resume_command.clone(),
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

    pub fn workspace_status_handle(&self, id: &str) -> Option<WorkspaceStatus> {
        self.workspaces.get(id).map(|ws| ws.status.clone())
    }

    pub fn workspace_is_alive(&self, id: &str) -> bool {
        let Ok(ws) = self.workspace(id) else {
            return false;
        };

        ws.status
            .0
            .lock()
            .map(|status| !matches!(status.as_str(), "dead" | "closing"))
            .unwrap_or(false)
    }

    fn workspace(&self, id: &str) -> Result<&Workspace, String> {
        self.workspaces
            .get(id)
            .ok_or_else(|| format!("Workspace '{}' not found", id))
    }
}

fn set_workspace_status(status: &WorkspaceStatus, next: &str) {
    if let Ok(mut current) = status.0.lock() {
        *current = next.to_string();
    }
    status.1.notify_all();
}

fn workspace_accepts_input(ws: &Workspace) -> bool {
    ws.status
        .0
        .lock()
        .map(|status| !matches!(status.as_str(), "dead" | "closing"))
        .unwrap_or(false)
}

fn should_mark_workspace_dead(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::UnexpectedEof
    ) || error.raw_os_error() == Some(5)
}

fn mark_workspace_dead(ws: &Workspace) {
    mark_workspace_dead_handles(&ws.id, &ws.status, &ws.writer, &ws.inject_queue);
}

fn mark_workspace_dead_handles(
    id: &str,
    status: &WorkspaceStatus,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    inject_queue: &SharedInjectQueue,
) {
    let already_dead = status
        .0
        .lock()
        .map(|current| current.as_str() == "dead")
        .unwrap_or(false);
    set_workspace_status(status, "dead");
    if let Ok(mut queue) = inject_queue.lock() {
        queue.clear();
    }
    close_writer_handle(writer);
    if !already_dead {
        log_stderr!("[PTY] workspace marked dead: {}", id);
    }
    if let Ok(mut app) = crate::global_app().lock() {
        app.handle_workspace_marked_dead(id);
    }
}

// --- Shell integration: OSC 133 ---

const OSC133_ZSH: &str = r#"# aterm OSC 133 shell integration (zsh)
# Emits semantic prompt markers for reliable inject timing.
[ -n "$ATERM_DEBUG_LOG" ] && echo "[osc133-debug] aterm-osc133.zsh sourced" >&2
_aterm_osc133_precmd() {
    local ret=$?
    printf '\e]133;D;%d\a' "$ret"
    printf '\e]133;A\a'
}
_aterm_osc133_preexec() {
    printf '\e]133;C\a'
}
# Emit B (prompt ready) after PS1 renders — append to RPS1 for end-of-prompt placement
_aterm_osc133_prompt_ready() {
    printf '\e]133;B\a'
}
precmd_functions=(_aterm_osc133_precmd "${precmd_functions[@]}")
precmd_functions+=(_aterm_osc133_prompt_ready)
[ -n "$ATERM_DEBUG_LOG" ] && echo "[osc133-debug] precmd registered, precmd_functions=${(j:,:)precmd_functions}" >&2
preexec_functions=(_aterm_osc133_preexec "${preexec_functions[@]}")
# Emit initial A+B for first prompt
printf '\e]133;A\a\e]133;B\a'
"#;

const OSC133_BASH: &str = r#"# aterm OSC 133 shell integration (bash)
_aterm_osc133_prompt() {
    printf '\e]133;D\a'
    printf '\e]133;A\a\e]133;B\a'
}
PROMPT_COMMAND="_aterm_osc133_prompt${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
PS0='\e]133;C\a'
"#;

const OSC133_FISH: &str = r#"# aterm OSC 133 shell integration (fish)
function __aterm_osc133_prompt --on-event fish_prompt
    printf '\e]133;A\a\e]133;B\a'
end
function __aterm_osc133_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end
function __aterm_osc133_postexec --on-event fish_postexec
    printf '\e]133;D\a'
end
"#;

const ZDOTDIR_ZSHENV: &str = r#"# aterm ZDOTDIR wrapper (VS Code pattern) — save, restore, source, re-save
# 1. Save wrapper dir so .zshrc can find us
ATERM_ZDOTDIR="$ZDOTDIR"
# 2. Restore user's original ZDOTDIR
ATERM_USER_ZDOTDIR="${ATERM_ORIGINAL_ZDOTDIR:-$HOME}"
ZDOTDIR="$ATERM_USER_ZDOTDIR"
unset ATERM_ORIGINAL_ZDOTDIR
# 3. Source user's .zshenv
[[ -f "${ZDOTDIR}/.zshenv" ]] && source "${ZDOTDIR}/.zshenv"
# 4. CRITICAL: Restore wrapper ZDOTDIR so zsh finds our .zshrc next
ZDOTDIR="$ATERM_ZDOTDIR"
"#;

const ZDOTDIR_ZSHRC: &str = r#"# aterm ZDOTDIR wrapper (VS Code pattern) — source user rc then inject OSC 133
# 1. Source user's .zshrc using saved original path
[ -n "$ATERM_DEBUG_LOG" ] && echo "[osc133-debug] .zshrc wrapper sourced, ATERM_USER_ZDOTDIR=$ATERM_USER_ZDOTDIR" >&2
[[ -f "${ATERM_USER_ZDOTDIR}/.zshrc" ]] && source "${ATERM_USER_ZDOTDIR}/.zshrc"
# 2. Source OSC 133 shell integration
_aterm_osc133_path="${ATERM_DATA_ROOT:-$HOME/.aigentry}/shell-integration/aterm-osc133.zsh"
[ -n "$ATERM_DEBUG_LOG" ] && echo "[osc133-debug] sourcing $_aterm_osc133_path (exists=$([[ -f "$_aterm_osc133_path" ]] && echo yes || echo no))" >&2
source "$_aterm_osc133_path"
unset _aterm_osc133_path
# 3. Restore user's ZDOTDIR for the rest of the session
ZDOTDIR="$ATERM_USER_ZDOTDIR"
unset ATERM_ZDOTDIR ATERM_USER_ZDOTDIR
"#;

/// Write shell integration scripts to ~/.aigentry/shell-integration/ if missing or outdated.
fn ensure_shell_integration() -> Option<PathBuf> {
    let base = crate::session::data_root().join("shell-integration");
    let zsh_dir = base.join("zsh");

    // Create directories
    std::fs::create_dir_all(&zsh_dir).ok()?;

    // Write scripts (always overwrite to stay current)
    let zsh_path = base.join("aterm-osc133.zsh");
    let bash_path = base.join("aterm-osc133.bash");
    let fish_path = base.join("aterm-osc133.fish");
    let zshenv_path = zsh_dir.join(".zshenv");
    let zshrc_path = zsh_dir.join(".zshrc");
    let writes: &[(&Path, &str)] = &[
        (zsh_path.as_path(), OSC133_ZSH),
        (bash_path.as_path(), OSC133_BASH),
        (fish_path.as_path(), OSC133_FISH),
        (zshenv_path.as_path(), ZDOTDIR_ZSHENV),
        (zshrc_path.as_path(), ZDOTDIR_ZSHRC),
    ];
    for (path, content) in writes {
        if let Ok(existing) = std::fs::read_to_string(path) {
            if existing == *content {
                continue;
            }
        }
        let _ = std::fs::write(path, content);
    }

    Some(base)
}

fn is_shell_command(command: &str) -> bool {
    let basename = Path::new(command)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(command);
    matches!(basename, "zsh" | "bash" | "fish" | "sh")
}

fn is_zsh_command(command: &str) -> bool {
    let basename = Path::new(command)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(command);
    basename == "zsh"
}

fn is_bash_command(command: &str) -> bool {
    let basename = Path::new(command)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(command);
    basename == "bash"
}

fn has_explicit_locale_env() -> bool {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if std::env::var_os(key).is_some() {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn default_utf8_locale() -> &'static str {
    "en_US.UTF-8"
}

#[cfg(not(target_os = "macos"))]
fn default_utf8_locale() -> &'static str {
    "C.UTF-8"
}

fn apply_utf8_locale_fallback(cmd: &mut CommandBuilder) {
    if has_explicit_locale_env() {
        return;
    }

    let locale = default_utf8_locale();
    cmd.env("LANG", locale);
    cmd.env("LC_CTYPE", locale);

    static REPORTED_UTF8_LOCALE_FALLBACK: AtomicBool = AtomicBool::new(false);
    if !REPORTED_UTF8_LOCALE_FALLBACK.swap(true, Ordering::AcqRel) {
        log_stderr!("[aterm] applying default UTF-8 locale fallback: {locale}");
    }
}

fn spawn_workspace_process(
    cwd: &str,
    command: &str,
    args: &[String],
    size: PtySize,
    workspace_id: Option<&str>,
) -> Result<SpawnedWorkspace, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(size)
        .map_err(|error| error.to_string())?;
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
    apply_utf8_locale_fallback(&mut cmd);
    if let Some(ws_id) = workspace_id {
        cmd.env("ATERM_SESSION_ID", ws_id);
        cmd.env("ATERM_WORKSPACE_NAME", ws_id);
        cmd.env("TELEPTY_SESSION_ID", ws_id);
        cmd.env("ATERM_WORKSPACE_CLI", detect_cli_type(command));
    }
    if let Some(path_env) = augmented_path_env() {
        cmd.env("PATH", path_env);
    }
    // Expose Resources/bin path so the aterm CLI can re-add it to PATH
    // even if the login shell resets PATH during startup
    if let Some(ref resources_bin) = app_resources_bin() {
        cmd.env("ATERM_RESOURCES_BIN", resources_bin);
    }

    // Orchestrator session name from aterm.json config (for dynamic hook routing)
    if let Some(orch_name) = read_orchestrator_session_name() {
        cmd.env("ATERM_ORCHESTRATOR_SESSION", &orch_name);
    }

    // IPC socket path for child processes
    let socket_path = format!("/tmp/aterm-{}.sock", std::process::id());
    cmd.env("ATERM_IPC_SOCKET", &socket_path);

    // Propagate ATERM_DATA_ROOT to child so shell integration scripts resolve correctly
    let data_root = crate::session::data_root();
    cmd.env("ATERM_DATA_ROOT", data_root.to_string_lossy().as_ref());

    // Propagate ATERM_DEBUG_LOG to child so shell integration scripts respect it
    if let Ok(v) = std::env::var("ATERM_DEBUG_LOG") {
        cmd.env("ATERM_DEBUG_LOG", v);
    }

    // OSC 133 shell integration — only applicable for shell commands (zsh/bash/fish).
    // AI CLIs (claude/codex/gemini) are TUI apps that don't source shell rc files
    // and don't emit OSC 133 sequences. For those, ShellReady uses heuristic
    // prompt detection and inject uses heuristic + force-inject.
    let is_shell = is_shell_command(command);
    debug_log!(
        "[osc133-debug] spawn: command={command}, is_shell={is_shell}, is_zsh={}",
        is_zsh_command(command)
    );
    if is_shell {
        if let Some(base) = ensure_shell_integration() {
            cmd.env("ATERM_SHELL_INTEGRATION", "1");
            cmd.env(
                "ATERM_SHELL_INTEGRATION_DIR",
                base.to_string_lossy().as_ref(),
            );

            if is_zsh_command(command) {
                // ZDOTDIR trick: wrapper .zshrc sources user config then OSC 133
                let original = std::env::var("ZDOTDIR").unwrap_or_default();
                if !original.is_empty() {
                    cmd.env("ATERM_ORIGINAL_ZDOTDIR", &original);
                }
                let zdotdir = base.join("zsh");
                debug_log!("[osc133-debug] ZDOTDIR set to {}", zdotdir.display());
                cmd.env("ZDOTDIR", zdotdir.to_string_lossy().as_ref());
            } else if is_bash_command(command) {
                // BASH_ENV sources integration for non-interactive; --rcfile for interactive
                let integration = base.join("aterm-osc133.bash");
                cmd.env("BASH_ENV", integration.to_string_lossy().as_ref());
            }
        }
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| error.to_string())?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;

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
    status: &WorkspaceStatus,
    idle_state: &Arc<Mutex<IdleState>>,
    inject_queue: &SharedInjectQueue,
    inject_signal: &InjectSignal,
    signal: &PtyOutputSignal,
    restart_count: &Arc<AtomicU32>,
    pending_osc133: &crate::terminal::PendingOsc133,
) -> bool {
    log_stderr!("[PTY] auto-restart: attempting respawn for {}", ws_id);

    let current_size = size
        .lock()
        .ok()
        .map(|s| clone_size(&*s))
        .unwrap_or(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        });

    let spawned = match spawn_workspace_process(cwd, command, args, current_size, Some(ws_id)) {
        Ok(s) => s,
        Err(e) => {
            log_stderr!("[PTY] auto-restart failed for {}: {}", ws_id, e);
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
    if let Ok(mut s) = status.0.lock() {
        *s = "running".to_string();
    }
    status.1.notify_all();
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

    // Re-register handles with AtermApp so workspace_exists() stays true
    if let Ok(mut app) = crate::global_app().lock() {
        app.update_workspace_handles(
            ws_id,
            writer.clone(),
            inject_queue.clone(),
            inject_signal.clone(),
            status.clone(),
        );
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
    let restart_inject_signal = inject_signal.clone();
    let restart_restart_count = restart_count.clone();
    let restart_pending_osc133 = pending_osc133.clone();

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
            restart_inject_signal,
            restart_restart_count,
            restart_pending_osc133,
        );
    });

    // Spawn new injector
    let injector_queue = inject_queue.clone();
    let injector_idle = idle_state.clone();
    let injector_writer = writer.clone();
    let injector_status = status.clone();
    let injector_signal = inject_signal.clone();
    thread::spawn(move || {
        run_injector_loop(
            injector_queue,
            injector_idle,
            injector_writer,
            injector_status,
            injector_signal,
        );
    });

    signal.mark_dirty();
    log_stderr!("[PTY] auto-restart: respawned {}", ws_id);
    true
}

/// Detects when the shell is ready to accept input after PTY spawn.
/// Checks last 256B of each chunk for prompt pattern → fire immediately.
/// One-shot: fires once per workspace lifecycle, then stops monitoring.
///
/// Why no settle wait: after a CLI displays its prompt, PTY output stops
/// (process waits for input). reader.read() blocks, so feed() is never
/// called again. A settle-based approach caused ShellReady to always fall
/// through to the 10s fallback timeout.
///
/// Why check tail of large chunks: AI CLI startup (claude/gemini) often
/// exceeds 256B in a single PTY read, but the prompt sits at the end.
/// Checking only the last 256B catches the prompt without false-positives
/// from mid-dump content.
struct ShellReadyDetector {
    fired: bool,
    created_at: Instant,
    fallback_timeout: Duration,
}

const SHELL_READY_MAX_CHUNK: usize = 256;
const SHELL_READY_FALLBACK: Duration = Duration::from_secs(10);

impl ShellReadyDetector {
    fn new() -> Self {
        Self {
            fired: false,
            created_at: Instant::now(),
            fallback_timeout: SHELL_READY_FALLBACK,
        }
    }

    /// Feed output chunk. Returns true if shell_ready should fire now.
    fn feed(&mut self, data: &str, chunk_len: usize) -> bool {
        if self.fired {
            return false;
        }

        // Guard 1: fallback timeout
        if self.created_at.elapsed() >= self.fallback_timeout {
            self.fired = true;
            log_stderr!(
                "[shell-ready] fallback timeout ({}s)",
                self.fallback_timeout.as_secs()
            );
            return true;
        }

        // Guard 2: for large chunks, only check the tail for a prompt.
        // AI CLI startup output (claude/gemini) often exceeds 256B in a single
        // PTY read, but the prompt appears at the very end.
        let check_data = if chunk_len >= SHELL_READY_MAX_CHUNK {
            let raw_start = data.len().saturating_sub(SHELL_READY_MAX_CHUNK);
            // Floor to char boundary — avoid slicing mid-UTF-8 (Korean, CJK, box-drawing)
            let start = (raw_start..data.len())
                .find(|&i| data.is_char_boundary(i))
                .unwrap_or(data.len());
            &data[start..]
        } else {
            data
        };

        // Guard 3: prompt pattern in last non-empty line → fire immediately
        let last_nonempty = check_data.lines().rev().find(|l| !l.trim().is_empty());
        let has_prompt = last_nonempty
            .map(|l| has_prompt_pattern(l))
            .unwrap_or(false);

        if has_prompt {
            self.fired = true;
            log_stderr!("[shell-ready] detected (prompt pattern)");
            return true;
        }

        false
    }

    fn has_fired(&self) -> bool {
        self.fired
    }

    fn mark_fired(&mut self) {
        self.fired = true;
    }
}

/// Detects trust prompts in PTY output for auto-acceptance.
/// One-shot: fires once per workspace lifecycle.
/// Scans a rolling buffer of recent output for trust prompt patterns.
const TRUST_PROMPT_BUFFER_BYTES: usize = 512;
const TRUST_PROMPT_PATTERNS: &[&str] = &[
    "Do you trust",
    "do you trust",
    "Trust this",
    "trust this project",
];

struct TrustPromptDetector {
    fired: bool,
    recent: String,
}

impl TrustPromptDetector {
    fn new() -> Self {
        Self {
            fired: false,
            recent: String::new(),
        }
    }

    /// Feed new PTY output data. Returns true if trust prompt detected.
    fn feed(&mut self, data: &str) -> bool {
        if self.fired {
            return false;
        }

        // Append to rolling buffer
        self.recent.push_str(data);
        // Keep only last N bytes (same boundary technique as append_recent_text)
        if self.recent.len() > TRUST_PROMPT_BUFFER_BYTES {
            let mut start = self.recent.len().saturating_sub(TRUST_PROMPT_BUFFER_BYTES);
            while start < self.recent.len() && !self.recent.is_char_boundary(start) {
                start += 1;
            }
            self.recent.drain(..start);
        }

        for pattern in TRUST_PROMPT_PATTERNS {
            if self.recent.contains(pattern) {
                self.fired = true;
                log_stderr!("[trust-prompt] detected pattern: {}", pattern);
                return true;
            }
        }

        false
    }

    fn has_fired(&self) -> bool {
        self.fired
    }
}

fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    ws_id: String,
    buffer: Arc<Mutex<OutputBuffer>>,
    term_bytes: PtyByteQueue,
    status: WorkspaceStatus,
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
    inject_signal: InjectSignal,
    restart_count: Arc<AtomicU32>,
    pending_osc133: crate::terminal::PendingOsc133,
) {
    let mut buf = [0u8; 4096];
    let mut leftover: Vec<u8> = Vec::new();
    let mut combined: Vec<u8> = Vec::new();
    let mut codex_resume_recent = String::new();
    let mut codex_resume_enter_sent = false;
    let mut codex_resume_entered_at: Option<Instant> = None;
    let mut shell_ready = ShellReadyDetector::new();
    let mut trust_prompt = TrustPromptDetector::new();
    let mut osc133_detected = false;
    let mut first_output_logged = false;

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                // Debug: log raw bytes from first few reads to see if OSC 133 is present
                if !first_output_logged {
                    let raw = &buf[..n.min(256)];
                    let has_esc_bracket = raw.windows(2).any(|w| w[0] == 0x1b && w[1] == b']');
                    let hex: String = raw.iter().take(64).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                    debug_log!(
                        "[osc133-debug] first PTY output: {} bytes, has_osc_intro={}, hex(first 64)={}",
                        n, has_esc_bracket, hex
                    );
                    first_output_logged = true;
                }
                // Log any read that contains ESC ] (potential OSC sequence)
                if buf[..n].windows(2).any(|w| w[0] == 0x1b && w[1] == b']') {
                    let hex: String = buf[..n.min(128)].iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                    debug_log!(
                        "[osc133-debug] raw ESC] in PTY read: {} bytes, hex={}",
                        n, hex
                    );
                }

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

                // OSC 133 detection (primary, definitive signal)
                let osc_marks = detect_osc133(&data);
                let has_osc133_prompt = osc_marks.contains(&Osc133Mark::PromptEnd);
                let has_osc133_cmd = osc_marks.contains(&Osc133Mark::CommandStart);

                // Push marks to pending queue for TerminalState to resolve positions
                if !osc_marks.is_empty() {
                    debug_log!(
                        "[osc133-debug] reader_loop detected {} marks: {:?}",
                        osc_marks.len(),
                        osc_marks
                    );
                    if let Ok(mut pending) = pending_osc133.lock() {
                        let before = pending.len();
                        pending.extend_from_slice(&osc_marks);
                        debug_log!(
                            "[osc133-debug] pending queue: {} -> {}",
                            before,
                            pending.len()
                        );
                    }
                }

                if !osc133_detected && !osc_marks.is_empty() {
                    osc133_detected = true;
                    log_stderr!("[prompt] OSC 133 shell integration active");
                }
                if has_osc133_prompt {
                    if let Ok(mut idle) = idle_state.lock() {
                        idle.record_osc133_prompt();
                    }
                    inject_signal.notify();
                }
                if has_osc133_cmd {
                    // Command started = user/inject submitted input
                    if let Ok(mut idle) = idle_state.lock() {
                        idle.record_user_input();
                    }
                }

                // Heuristic prompt detection (fallback for shells without OSC 133
                // and TUI CLIs like claude/codex/gemini)
                if !has_osc133_prompt {
                    let has_prompt = has_prompt_pattern(&data);
                    if let Ok(mut idle) = idle_state.lock() {
                        idle.record_output(has_prompt);
                    }
                    if has_prompt {
                        inject_signal.notify();
                    }
                } else if let Ok(mut idle) = idle_state.lock() {
                    // Still update output timestamp for non-prompt output tracking
                    idle.record_output(false);
                }

                // Shell-ready detection (one-shot) — OSC 133;B is definitive
                let shell_ready_now = if !shell_ready.has_fired() {
                    if has_osc133_prompt {
                        shell_ready.mark_fired();
                        log_stderr!("[shell-ready] detected (OSC 133;B)");
                        true
                    } else {
                        shell_ready.feed(&data, n)
                    }
                } else {
                    false
                };
                if shell_ready_now {
                    if let Ok(app) = crate::global_app().lock() {
                        if let Some(ref host) = app.host_ref() {
                            host.on_workspace_event(
                                aterm_session::types::WorkspaceEvent::ShellReady {
                                    id: ws_id.clone(),
                                },
                            );
                        }
                        // Also broadcast to IPC subscribers
                        app.broadcast_workspace_event(&serde_json::json!({
                            "type": "ShellReady",
                            "id": ws_id
                        }));
                    }
                }

                // Trust prompt detection (one-shot)
                if !trust_prompt.has_fired() && trust_prompt.feed(&data) {
                    if let Ok(app) = crate::global_app().lock() {
                        if let Some(ref host) = app.host_ref() {
                            host.on_workspace_event(
                                aterm_session::types::WorkspaceEvent::TrustPromptDetected {
                                    id: ws_id.clone(),
                                },
                            );
                        }
                        app.broadcast_workspace_event(&serde_json::json!({
                            "type": "TrustPromptDetected",
                            "id": ws_id
                        }));
                    }
                }
            }
            Err(error) => {
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                log_stderr!("[aterm] reader error for {}: {}", ws_id, error);
                break;
            }
        }
    }

    let is_closing = status
        .0
        .lock()
        .map(|current| current.as_str() == "closing")
        .unwrap_or(false);

    if !is_closing && auto_restart {
        let attempts = restart_count.fetch_add(1, Ordering::SeqCst);
        if attempts < 3 {
            let retry_args: Vec<String> = args
                .iter()
                .filter(|a| a.as_str() != "--continue")
                .cloned()
                .collect();
            log_stderr!(
                "[PTY] auto-restart: attempt {}/3 for {} (without --continue)",
                attempts + 1,
                ws_id
            );
            let restarted = try_restart_workspace(
                &ws_id,
                &cwd,
                &command,
                &retry_args,
                &size,
                &master,
                &writer,
                &child,
                &buffer,
                &term_bytes,
                &status,
                &idle_state,
                &inject_queue,
                &inject_signal,
                &signal,
                &restart_count,
                &pending_osc133,
            );
            if restarted {
                return; // New reader thread is running, don't mark dead
            }
            log_stderr!(
                "[PTY] auto-restart: respawn failed for {}, marking dead",
                ws_id
            );
        } else {
            log_stderr!(
                "[PTY] auto-restart: max retries (3) reached for {}, giving up",
                ws_id
            );
        }
    }

    // Only mark dead when: closing, not auto_restart, all retries exhausted, or restart failed
    if !is_closing {
        mark_workspace_dead_handles(&ws_id, &status, &writer, &inject_queue);
    }
    signal.mark_dirty();

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
        "ctrl+c" | "ctrl-c" => Ok("\x03"),
        "ctrl+d" | "ctrl-d" => Ok("\x04"),
        "ctrl+l" | "ctrl-l" => Ok("\x0c"),
        "ctrl+z" | "ctrl-z" => Ok("\x1a"),
        "tab" => Ok("\t"),
        "esc" | "escape" => Ok("\x1b"),
        _ => Err(format!(
            "Unknown key: '{}'. Supported: return, ctrl+c/ctrl-c, ctrl+d/ctrl-d, ctrl+l/ctrl-l, ctrl+z/ctrl-z, tab, esc, escape",
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

    // Prepend app bundle Resources/bin so `aterm` CLI is first in PATH
    if let Some(resources_bin) = app_resources_bin() {
        push_unique_path(&mut paths, &mut seen, resources_bin);
    }

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
        "/Applications/aterm.app/Contents/Resources/bin",
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
    use super::{
        mark_workspace_dead_handles, InjectMessage, InjectQueue, PtyOutputSignal,
        SharedInjectQueue, ShellReadyDetector, TrustPromptDetector, WorkspaceStatus,
        SHELL_READY_FALLBACK, SHELL_READY_MAX_CHUNK, TRUST_PROMPT_BUFFER_BYTES,
        TRUST_PROMPT_PATTERNS,
    };
    use std::io::Write;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

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

    #[test]
    fn mark_workspace_dead_closes_writer_and_clears_queue() {
        let status: WorkspaceStatus = Arc::new((Mutex::new("running".to_string()), Condvar::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));

        queue.lock().unwrap().push(InjectMessage {
            from: "tester".to_string(),
            text: "hello".to_string(),
            timestamp: 0,
            enqueued_at: Instant::now(),
        });

        mark_workspace_dead_handles("dead-test", &status, &writer, &queue);

        assert_eq!(status.0.lock().unwrap().as_str(), "dead");
        assert!(queue.lock().unwrap().is_empty());

        let err = writer.lock().unwrap().write_all(b"x").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    // ===== TEST 2: ShellReadyDetector =====

    #[test]
    fn shell_ready_prompt_fires_immediately() {
        let mut detector = ShellReadyDetector::new();
        // Small chunk with prompt pattern — fires immediately (no settle)
        let data = "Welcome to zsh\n❯";
        let fired = detector.feed(data, data.len());
        assert!(fired, "Should fire immediately on prompt in small chunk");
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_bare_prompt_fires_immediately() {
        let mut detector = ShellReadyDetector::new();
        let data = "❯";
        let fired = detector.feed(data, data.len());
        assert!(fired, "Should fire immediately on bare prompt");
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_large_chunk_with_prompt_at_end() {
        let mut detector = ShellReadyDetector::new();

        // Large chunk with prompt at end — should fire (checks last 256B)
        let large = "x".repeat(SHELL_READY_MAX_CHUNK) + "\n❯";
        let fired = detector.feed(&large, large.len());
        assert!(
            fired,
            "Large chunk with prompt at end should trigger ShellReady"
        );
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_large_chunk_with_multibyte_utf8() {
        let mut detector = ShellReadyDetector::new();

        // Korean/CJK text (3 bytes per char) filling >256B, prompt at end
        // This previously panicked: "byte index not a char boundary"
        let korean = "서버를 시작합니다. 설정을 로드하는 중입니다. ".repeat(10);
        let large = korean + "\n❯";
        assert!(large.len() > SHELL_READY_MAX_CHUNK);
        let fired = detector.feed(&large, large.len());
        assert!(fired, "Should handle multibyte UTF-8 without panic");
    }

    #[test]
    fn shell_ready_large_chunk_without_prompt() {
        let mut detector = ShellReadyDetector::new();

        // Large chunk without prompt at end — should not fire
        let large = "x".repeat(SHELL_READY_MAX_CHUNK + 100);
        let fired = detector.feed(&large, large.len());
        assert!(
            !fired,
            "Large chunk without prompt should not trigger ShellReady"
        );
        assert!(!detector.has_fired());
    }

    #[test]
    fn shell_ready_prompt_fires_even_after_non_prompt_output() {
        let mut detector = ShellReadyDetector::new();

        // Feed non-prompt data — should not fire
        let output = "loading config...";
        let fired = detector.feed(output, output.len());
        assert!(!fired);
        assert!(!detector.has_fired());

        // Feed prompt — should fire immediately
        let prompt = "❯";
        let fired = detector.feed(prompt, prompt.len());
        assert!(
            fired,
            "Prompt should fire immediately even after non-prompt output"
        );
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_fallback_timeout_fires_without_prompt() {
        let mut detector = ShellReadyDetector::new();

        // Override created_at to simulate 10+ seconds ago
        detector.created_at = Instant::now()
            .checked_sub(SHELL_READY_FALLBACK + Duration::from_millis(100))
            .unwrap();

        // Any data (even without prompt) should trigger via fallback
        let data = "no prompt here";
        let fired = detector.feed(data, data.len());
        assert!(fired, "Fallback timeout should fire after 10s");
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_fires_only_once() {
        let mut detector = ShellReadyDetector::new();

        // Trigger via fallback
        detector.created_at = Instant::now()
            .checked_sub(SHELL_READY_FALLBACK + Duration::from_secs(1))
            .unwrap();
        assert!(detector.feed("data", 4));
        assert!(detector.has_fired());

        // Subsequent calls should always return false (one-shot)
        assert!(!detector.feed("❯", 3));
        assert!(!detector.feed("❯", 3));
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_fish_nushell_trailing_newline() {
        let mut detector = ShellReadyDetector::new();

        // fish/nushell may have trailing newline after prompt
        let data = "❯\n";
        let fired = detector.feed(data, data.len());

        // The prompt "❯" should be found in last non-empty line
        // even with trailing newline — fires immediately
        assert!(
            fired,
            "Should fire immediately for fish/nushell prompt with trailing newline"
        );
        assert!(detector.has_fired());
    }

    #[test]
    fn shell_ready_constants() {
        assert_eq!(SHELL_READY_MAX_CHUNK, 256);
        assert_eq!(SHELL_READY_FALLBACK, Duration::from_secs(10));
    }

    // ===== TEST 3: Bootstrap shell_ready callback =====

    #[test]
    fn shell_ready_immediate_trigger_enables_fast_bootstrap() {
        // ShellReady fires immediately on prompt — no settle wait needed.
        // This means bootstrap can happen instantly (not after 10s fallback).
        let mut detector = ShellReadyDetector::new();
        let prompt = "$ ";
        let fired = detector.feed(prompt, prompt.len());
        assert!(
            fired,
            "ShellReady should fire immediately on prompt — enabling instant bootstrap"
        );
    }

    #[test]
    fn shell_ready_fallback_still_works_for_bootstrap() {
        // Even without prompt, fallback ensures bootstrap eventually fires
        let mut detector = ShellReadyDetector::new();
        detector.created_at = Instant::now()
            .checked_sub(SHELL_READY_FALLBACK + Duration::from_millis(50))
            .unwrap();

        let fired = detector.feed("unknown shell output", 20);
        assert!(
            fired,
            "Fallback timeout ensures bootstrap fires even without prompt detection"
        );
    }

    // ===== TEST 6: SIGCHLD / Process exit — PTY reader EOF =====

    #[test]
    fn pty_eof_sets_status_to_dead() {
        // When PTY reader gets EOF (Ok(0)), mark_workspace_dead_handles is called.
        // This sets status to "dead" via Condvar — no polling timer involved.
        let status: WorkspaceStatus = Arc::new((Mutex::new("running".to_string()), Condvar::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));

        assert_eq!(status.0.lock().unwrap().as_str(), "running");

        // Simulate what reader_loop does on EOF: calls mark_workspace_dead_handles
        mark_workspace_dead_handles("eof-test", &status, &writer, &queue);

        assert_eq!(
            status.0.lock().unwrap().as_str(),
            "dead",
            "PTY EOF should set status to 'dead'"
        );
    }

    #[test]
    fn pty_eof_notifies_condvar_waiters() {
        // Verify that marking dead wakes up any Condvar waiters (event-driven, not polled)
        let status: WorkspaceStatus = Arc::new((Mutex::new("running".to_string()), Condvar::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));

        let status2 = status.clone();
        let handle = std::thread::spawn(move || {
            let start = Instant::now();
            let (ref mutex, ref condvar) = *status2;
            let mut current = mutex.lock().unwrap();
            while *current != "dead" {
                current = condvar.wait(current).unwrap();
            }
            start.elapsed()
        });

        // Give waiter time to block
        std::thread::sleep(Duration::from_millis(20));

        // Simulate EOF → mark dead (this notifies Condvar)
        mark_workspace_dead_handles("eof-condvar-test", &status, &writer, &queue);

        let elapsed = handle.join().unwrap();
        assert!(
            elapsed < Duration::from_millis(200),
            "Condvar waiter should wake promptly on dead (event-driven), got {:?}",
            elapsed
        );
    }

    #[test]
    fn pty_eof_no_polling_timer_involved() {
        // The reader_loop blocks on reader.read() — a blocking syscall.
        // EOF (Ok(0)) breaks the loop. No timer, no periodic check.
        // Verify: status transitions happen instantly, not on a timer cadence.
        let status: WorkspaceStatus = Arc::new((Mutex::new("running".to_string()), Condvar::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Vec::<u8>::new())));
        let queue: SharedInjectQueue = Arc::new(Mutex::new(InjectQueue::new()));

        let start = Instant::now();
        mark_workspace_dead_handles("no-timer-test", &status, &writer, &queue);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(500),
            "Dead marking should be near-instant (no timer), got {:?}",
            elapsed
        );
        assert_eq!(status.0.lock().unwrap().as_str(), "dead");
    }

    // ===== TEST 7: TrustPromptDetector =====

    #[test]
    fn trust_prompt_detected_on_matching_pattern() {
        let mut detector = TrustPromptDetector::new();
        for pattern in TRUST_PROMPT_PATTERNS {
            let mut d = TrustPromptDetector::new();
            let result = d.feed(pattern);
            assert!(result, "Should detect trust prompt pattern: '{}'", pattern);
        }
        // Also test embedded in longer output
        let fired = detector.feed("Welcome to project\nDo you trust this folder?\n> ");
        assert!(fired, "Should detect trust prompt in multi-line output");
    }

    #[test]
    fn trust_prompt_non_matching_does_not_fire() {
        let mut detector = TrustPromptDetector::new();

        assert!(!detector.feed("Hello world"));
        assert!(!detector.feed("Loading configuration..."));
        assert!(!detector.feed("npm install completed"));
        assert!(!detector.feed("$ "));
        assert!(!detector.feed("trust")); // partial — not a full pattern
        assert!(!detector.has_fired());
    }

    #[test]
    fn trust_prompt_fires_only_once() {
        let mut detector = TrustPromptDetector::new();

        // First detection
        assert!(detector.feed("Do you trust this project?"));
        assert!(detector.has_fired());

        // Subsequent calls should always return false (one-shot)
        assert!(!detector.feed("Do you trust this project?"));
        assert!(!detector.feed("Trust this"));
        assert!(!detector.feed("do you trust"));
        assert!(detector.has_fired(), "Should remain fired");
    }

    #[test]
    fn trust_prompt_rolling_buffer_trims_to_limit() {
        let mut detector = TrustPromptDetector::new();

        // Feed a large chunk that pushes pattern out of rolling buffer
        let padding = "x".repeat(TRUST_PROMPT_BUFFER_BYTES + 100);
        detector.feed(&padding);

        // Now feed the pattern — should detect in fresh buffer
        let fired = detector.feed("Do you trust");
        assert!(fired, "Should detect pattern after buffer trim");
    }

    #[test]
    fn trust_prompt_pattern_split_across_feeds() {
        let mut detector = TrustPromptDetector::new();

        // Pattern split across two feeds, within rolling buffer window
        detector.feed("Do you ");
        let fired = detector.feed("trust this?");
        assert!(
            fired,
            "Should detect pattern split across consecutive feeds"
        );
    }

    #[test]
    fn trust_prompt_buffer_respects_utf8_boundaries() {
        let mut detector = TrustPromptDetector::new();

        // Feed multi-byte UTF-8 characters to fill near the buffer limit
        let filler = "한".repeat(TRUST_PROMPT_BUFFER_BYTES / 3);
        detector.feed(&filler);

        // Pattern should still be detectable after buffer trimming
        let fired = detector.feed("Do you trust");
        assert!(fired, "Should detect pattern after UTF-8 boundary trim");
    }
}

/// Read the default shell from ~/.aigentry/config/aterm.json (shell.default).
/// Returns None if config doesn't exist or shell.default is not set.
/// Used by GAP 5: aterm.json shell.default takes priority over $SHELL env var.
fn read_default_shell() -> Option<String> {
    let config_path = crate::session::data_root().join("config/aterm.json");
    let data = std::fs::read_to_string(&config_path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&data).ok()?;
    let shell = config.get("shell")?.get("default")?.as_str()?;
    if shell.is_empty() {
        None
    } else {
        Some(shell.to_string())
    }
}

/// Read the orchestrator session name from ~/.aigentry/config/aterm.json
/// Returns None if config doesn't exist or orchestrator is not configured.
fn read_orchestrator_session_name() -> Option<String> {
    let config_path = crate::session::data_root().join("config/aterm.json");
    let data = std::fs::read_to_string(&config_path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&data).ok()?;
    let name = config.get("orchestrator")?.get("name")?.as_str()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn detect_cli_type(command: &str) -> &'static str {
    let bin = command.rsplit('/').next().unwrap_or(command);
    match bin {
        "claude" => "claude",
        "codex" => "codex",
        "gemini" => "gemini",
        _ => "shell",
    }
}

pub fn augmented_path_env() -> Option<std::ffi::OsString> {
    std::env::join_paths(command_search_paths()).ok()
}

/// Return the app bundle's Resources/bin directory (e.g. aterm.app/Contents/Resources/bin).
pub fn app_resources_bin() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // exe: Contents/MacOS/aterm → Contents/ → Contents/Resources/bin
    let resources_bin = exe.parent()?.parent()?.join("Resources").join("bin");
    if resources_bin.is_dir() {
        Some(resources_bin)
    } else {
        None
    }
}
