/// Safe stderr logging — never panics, even if stderr is unavailable.
/// macOS .app bundles launched from Finder may not have stderr attached,
/// causing `log_stderr!` to panic. This macro silently discards write errors.
macro_rules! log_stderr {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

/// Returns true when debug logging is enabled.
/// Enabled only by ATERM_DEBUG_LOG=1 (or "true"). Cached at first call.
pub(crate) fn debug_log_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("ATERM_DEBUG_LOG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// Debug-only stderr logging — gated by ATERM_DEBUG_LOG=1.
/// Use for development diagnostics that should not appear in production npm builds.
macro_rules! debug_log {
    ($($arg:tt)*) => {{
        if $crate::debug_log_enabled() {
            log_stderr!($($arg)*);
        }
    }};
}

/// Catch panics at FFI boundary — prevents abort from unwinding through extern "C".
macro_rules! ffi_catch {
    ($default:expr, $body:expr) => {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)) {
            Ok(val) => val,
            Err(_) => {
                log_stderr!("[aterm] panic caught at FFI boundary");
                $default
            }
        }
    };
    ($body:expr) => {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)).is_err() {
            log_stderr!("[aterm] panic caught at FFI boundary");
        }
    };
}

pub mod app;
pub mod inject;
pub mod mailbox;
pub mod pty;
pub mod session;
pub mod tailscale;
pub mod telepty_bridge;
pub mod terminal;

use std::ffi::{c_char, c_void, CStr, CString};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::pty::{PtyManager, PtyOutputSignal};
use crate::terminal::{AdvanceHandle, TerminalState};

// -- Named key codes --
pub const ATERM_KEY_ENTER: u32 = 1;
pub const ATERM_KEY_BACKSPACE: u32 = 2;
pub const ATERM_KEY_DELETE: u32 = 3;
pub const ATERM_KEY_TAB: u32 = 4;
pub const ATERM_KEY_ESCAPE: u32 = 5;
pub const ATERM_KEY_ARROW_UP: u32 = 6;
pub const ATERM_KEY_ARROW_DOWN: u32 = 7;
pub const ATERM_KEY_ARROW_RIGHT: u32 = 8;
pub const ATERM_KEY_ARROW_LEFT: u32 = 9;
pub const ATERM_KEY_HOME: u32 = 10;
pub const ATERM_KEY_END: u32 = 11;
pub const ATERM_KEY_PAGE_UP: u32 = 12;
pub const ATERM_KEY_PAGE_DOWN: u32 = 13;

// -- Event type discriminants (C ABI) --
pub const ATERM_EVENT_CREATED: u8 = 0;
pub const ATERM_EVENT_CLOSED: u8 = 1;
pub const ATERM_EVENT_STATUS_CHANGED: u8 = 2;
pub const ATERM_EVENT_TITLE_CHANGED: u8 = 3;
pub const ATERM_EVENT_SHELL_READY: u8 = 4;
pub const ATERM_EVENT_TRUST_PROMPT: u8 = 5;

/// C-safe workspace event — flat struct, unused fields are NULL.
#[repr(C)]
pub struct AtermEventFFI {
    pub event_type: u8,
    pub id: *const c_char,
    pub name: *const c_char,
    pub status: *const c_char,
    pub title: *const c_char,
}

/// Batch of events returned by aterm_drain_events(). Caller frees with aterm_free_events().
#[repr(C)]
pub struct AtermEventBatch {
    pub events: *mut AtermEventFFI,
    pub count: u32,
}

/// Per-row selection range for Metal renderer — 12 bytes, matches Shaders.metal SelectionRange.
/// Each range covers one row of selected cells (multi-line selections split into per-row ranges).
#[repr(C)]
pub struct SelectionRangeFFI {
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Per-cell terminal data for Metal renderer — populated by aterm_core_get_render_data().
/// Swift allocates a flat array of these and passes it to the Metal render pass.
#[repr(C)]
pub struct CellDataFFI {
    pub col: u16,
    pub row: u16,
    pub fg_r: u8,
    pub fg_g: u8,
    pub fg_b: u8,
    pub fg_a: u8,
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
    pub bg_a: u8,
    pub character: u32, // Unicode codepoint
    pub flags: u8,      // bold=1, italic=2, underline=4
}

// -- Global event queue (wakeup+drain pattern) --
static EVENT_QUEUE: OnceLock<Mutex<Vec<aterm_session::types::WorkspaceEvent>>> = OnceLock::new();

fn global_event_queue() -> &'static Mutex<Vec<aterm_session::types::WorkspaceEvent>> {
    EVENT_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Callback type for dirty notifications
type DirtyCallback = unsafe extern "C" fn(*mut c_void);

#[derive(serde::Serialize)]
struct CliDetectionStatus {
    claude: bool,
    codex: bool,
    gemini: bool,
}

pub struct AtermCore {
    terminal: Option<TerminalState>,
    /// Pending AdvanceHandle — created with TerminalState, consumed by spawn_shell.
    advance_handle: Option<AdvanceHandle>,
    pty_manager: PtyManager,
    pty_signal: PtyOutputSignal,
    workspace_id: Option<String>,
    dirty: Arc<AtomicBool>,
    dirty_callback: Option<DirtyCallback>,
    dirty_userdata: *mut c_void,
    /// Theme mode for no-wgpu builds: 0 = dark, 1 = light.
    theme_mode_raw: u8,
    /// IME preedit text for inline rendering (#206). Empty = no preedit.
    preedit_text: String,
    /// PTY SIGWINCH coalescing (Fix 6): last grid cols/rows sent to PTY.
    /// Skips redundant ioctl(TIOCSWINSZ) when pixel size changes but
    /// grid dimensions stay the same.
    last_pty_cols: u16,
    last_pty_rows: u16,
    /// Last display_offset for scroll-change detection (dirty tracking #231).
    last_display_offset: usize,
    /// Geometry revision counter (Fix 7): cheap monotonic counter for
    /// stale-frame detection without expensive dimension comparison.
    geometry_revision: AtomicU64,
}

// SAFETY: The raw pointer dirty_userdata is only used from the main thread callback
unsafe impl Send for AtermCore {}

// --- No-wgpu metric helpers (match renderer.rs defaults) ---

const NO_WGPU_FONT_SIZE: f32 = 18.0;
const NO_WGPU_LINE_HEIGHT: f32 = 21.0;
const NO_WGPU_CELL_WIDTH: f32 = 10.8; // font_size * 0.6
const NO_WGPU_CELL_HEIGHT: f32 = 21.0; // ~Ghostty default: ascent + descent for 18px font

// Runtime-adjustable cell dimensions (set via aterm_core_set_line_height / set_cell_width).
// Stored as f32 bits in AtomicU32; 0 means "use default const".
static NO_WGPU_CELL_HEIGHT_ATOMIC: AtomicU32 = AtomicU32::new(0);
static NO_WGPU_CELL_WIDTH_ATOMIC: AtomicU32 = AtomicU32::new(0);

fn no_wgpu_cell_height() -> f32 {
    let bits = NO_WGPU_CELL_HEIGHT_ATOMIC.load(Ordering::Relaxed);
    if bits == 0 { NO_WGPU_CELL_HEIGHT } else { f32::from_bits(bits) }
}

fn no_wgpu_cell_width() -> f32 {
    let bits = NO_WGPU_CELL_WIDTH_ATOMIC.load(Ordering::Relaxed);
    if bits == 0 { NO_WGPU_CELL_WIDTH } else { f32::from_bits(bits) }
}

fn no_wgpu_grid_size(width: f32, height: f32) -> (u16, u16) {
    let usable_w = (width - 4.0f32).max(0.0);
    let usable_h = (height - 4.0f32).max(0.0);
    let cols = (usable_w / no_wgpu_cell_width()).floor().max(2.0) as u16;
    let rows = (usable_h / no_wgpu_cell_height()).floor().max(1.0) as u16;
    (cols, rows)
}

fn no_wgpu_grid_padding(width: f32, height: f32) -> (f32, f32) {
    let (cols, rows) = no_wgpu_grid_size(width, height);
    let grid_w = cols as f32 * no_wgpu_cell_width();
    let grid_h = rows as f32 * no_wgpu_cell_height();
    let pad_x = ((width - grid_w) / 2.0).floor().max(0.0);
    let pad_y = ((height - grid_h) / 2.0).floor().max(0.0);
    (pad_x, pad_y)
}

impl AtermCore {
    fn new() -> Self {
        let pty_manager = PtyManager::new();
        let pty_signal = pty_manager.output_signal();
        Self {
            terminal: None,
            advance_handle: None,
            pty_manager,
            pty_signal,
            workspace_id: None,
            dirty: Arc::new(AtomicBool::new(false)),
            dirty_callback: None,
            dirty_userdata: std::ptr::null_mut(),
            theme_mode_raw: 0, // 0 = dark
            preedit_text: String::new(),
            last_pty_cols: 0,
            last_pty_rows: 0,
            last_display_offset: 0,
            geometry_revision: AtomicU64::new(0),
        }
    }

    fn spawn_shell(
        &mut self,
        name: &str,
        cwd: &str,
        command: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> i32 {
        // Parse command string into program + args directly.
        // augmented_path_env() and resolve_command_binary() handle PATH resolution.
        let (cmd, args) = match command {
            Some(s) if !s.is_empty() => {
                let parts: Vec<&str> = s.split_whitespace().collect();
                let program = parts[0].to_string();
                let cmd_args: Vec<String> = parts[1..].iter().map(|a| a.to_string()).collect();
                (
                    Some(program),
                    if cmd_args.is_empty() {
                        None
                    } else {
                        Some(cmd_args)
                    },
                )
            }
            _ => (None, None),
        };
        log_stderr!("[aterm-core] spawn_shell: name={name} cmd={cmd:?} args={args:?}");
        let osc133_handle = self.terminal.as_ref().map(|t| t.pending_osc133());
        let advance_handle = self.advance_handle.take();
        match self.pty_manager.create(
            name.to_string(),
            cwd.to_string(),
            cmd,
            args,
            Some(cols),
            Some(rows),
            false,
            None,
            false,
            None,
            osc133_handle,
            advance_handle,
        ) {
            Ok(id) => {
                log_stderr!("[aterm-core] spawned: {id}");
                // Connect PTY writer to terminal so DA responses flow back
                if let Some(ref terminal) = self.terminal {
                    if let Some(writer) = self.pty_manager.workspace_writer(&id) {
                        terminal.set_pty_writer(writer);
                    }
                }
                self.workspace_id = Some(id.clone());
                // Register inject queue + writer with global IPC app
                if let Some(queue) = self.pty_manager.inject_queue_for(&id) {
                    let signal = self.pty_manager.inject_signal_for(&id);
                    let writer = self.pty_manager.workspace_writer(&id);
                    let status = self.pty_manager.workspace_status_handle(&id);
                    if let Ok(mut app) = crate::global_app().lock() {
                        app.register_workspace(
                            &id,
                            queue,
                            signal,
                            writer,
                            status,
                            command.unwrap_or(""),
                            cwd,
                        );
                    }
                }
                0
            }
            Err(e) => {
                log_stderr!("[aterm-core] shell spawn failed: {e}");
                -1
            }
        }
    }

    fn write_pty(&self, text: &str) {
        if let Some(ref id) = self.workspace_id {
            if !self.pty_manager.workspace_is_alive(id) {
                return;
            }
            if let Err(e) = self.pty_manager.send_to_workspace(id, text) {
                log_stderr!("[aterm-core] pty write error: {e}");
            }
        }
    }

    fn workspace_is_alive(&self) -> bool {
        self.workspace_id
            .as_ref()
            .map(|id| self.pty_manager.workspace_is_alive(id))
            .unwrap_or(false)
    }

    fn named_key(&self, code: u32) {
        let bytes: &str = match code {
            ATERM_KEY_ENTER => "\r",
            ATERM_KEY_BACKSPACE => "\x7f",
            ATERM_KEY_DELETE => "\x1b[3~",
            ATERM_KEY_TAB => "\t",
            ATERM_KEY_ESCAPE => "\x1b",
            ATERM_KEY_ARROW_UP => "\x1b[A",
            ATERM_KEY_ARROW_DOWN => "\x1b[B",
            ATERM_KEY_ARROW_RIGHT => "\x1b[C",
            ATERM_KEY_ARROW_LEFT => "\x1b[D",
            ATERM_KEY_HOME => "\x1b[H",
            ATERM_KEY_END => "\x1b[F",
            ATERM_KEY_PAGE_UP => "\x1b[5~",
            ATERM_KEY_PAGE_DOWN => "\x1b[6~",
            _ => return,
        };
        self.write_pty(bytes);
    }

    /// Drain any leftover startup bytes from the term_bytes queue.
    /// After the AdvanceHandle is connected, the reader thread advances
    /// directly — this only handles bytes that arrived before connection.
    fn sync_pty_startup(&mut self) {
        let Some(ref id) = self.workspace_id else { return; };
        let Some(ref terminal) = self.terminal else { return; };

        match self.pty_manager.drain_term_bytes(id, 65536) {
            Ok(bytes) if !bytes.is_empty() => {
                debug_log!("[sync-pty] drained {} startup bytes for workspace '{}'", bytes.len(), id);
                terminal.advance_startup(&bytes);
            }
            Err(e) => log_stderr!("[aterm-core] drain error: {e}"),
            _ => {}
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.geometry_revision.fetch_add(1, Ordering::Relaxed);

        let (cols, rows) = no_wgpu_grid_size(width as f32, height as f32);
        debug_log!("[no-wgpu resize] {}x{} px → {}x{} grid, terminal={}", width, height, cols, rows, self.terminal.is_some());
        if let Some(ref mut terminal) = self.terminal {
            terminal.resize(cols as usize, rows as usize);
        }
        if cols != self.last_pty_cols || rows != self.last_pty_rows {
            self.last_pty_cols = cols;
            self.last_pty_rows = rows;
            if let Some(ref id) = self.workspace_id {
                let _ = self.pty_manager.resize(id, cols, rows);
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn selection_start(&mut self, col: usize, line: i32, side: u8) {
        use alacritty_terminal::index::{Column, Line, Point};
        use alacritty_terminal::selection::{Selection, SelectionType};

        if let Some(ref mut terminal) = self.terminal {
            let s = if side == 0 {
                alacritty_terminal::index::Side::Left
            } else {
                alacritty_terminal::index::Side::Right
            };
            // Convert viewport-relative row to grid coordinates (scrollback-aware).
            // viewport row 0 + display_offset 5 = grid Line(-5).
            {
                let term_arc = terminal.terminal();
                let term = term_arc.lock();
                let display_offset = term.grid().display_offset() as i32;
                let grid_line = line - display_offset;
                let point = Point::new(Line(grid_line), Column(col));
                drop(term);
                let mut term = term_arc.lock();
                let sel = Selection::new(SelectionType::Simple, point, s);
                term.selection = Some(sel);
            }
        }
    }

    fn selection_update(&mut self, col: usize, line: i32, side: u8) {
        use alacritty_terminal::index::{Column, Line, Point};

        if let Some(ref mut terminal) = self.terminal {
            let s = if side == 0 {
                alacritty_terminal::index::Side::Left
            } else {
                alacritty_terminal::index::Side::Right
            };
            // Convert viewport-relative row to grid coordinates (scrollback-aware).
            // Auto-scroll changes display_offset; selection must track absolute position.
            {
                let term_arc = terminal.terminal();
                let mut term = term_arc.lock();
                let display_offset = term.grid().display_offset() as i32;
                let grid_line = line - display_offset;
                let point = Point::new(Line(grid_line), Column(col));
                if let Some(ref mut sel) = term.selection {
                    sel.update(point, s);
                }
            }
        }
    }

    fn selection_clear(&mut self) {
        if let Some(ref mut terminal) = self.terminal {
            let term_arc = terminal.terminal();
            let mut term = term_arc.lock();
            term.selection = None;
        }
    }

    fn selection_text(&self) -> Option<String> {
        if let Some(ref terminal) = self.terminal {
            let term_arc = terminal.terminal();
            let term = term_arc.lock();
            return term.selection_to_string();
        }
        None
    }

    fn select_all(&mut self) {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line, Point};
        use alacritty_terminal::selection::{Selection, SelectionType};

        if let Some(ref mut terminal) = self.terminal {
            let term_arc = terminal.terminal();
            let mut term = term_arc.lock();
            let history = term.grid().history_size() as i32;
            let screen_lines = term.grid().screen_lines();
            let cols = term.grid().columns();

            let start = Point::new(Line(-(history)), Column(0));
            let end = Point::new(Line(screen_lines as i32 - 1), Column(cols.saturating_sub(1)));

            let mut sel = Selection::new(SelectionType::Simple, start, alacritty_terminal::index::Side::Left);
            sel.update(end, alacritty_terminal::index::Side::Right);
            term.selection = Some(sel);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }
}

// -- C-FFI Functions --

#[no_mangle]
pub extern "C" fn aterm_core_new() -> *mut AtermCore {
    ffi_catch!(
        std::ptr::null_mut(),
        Box::into_raw(Box::new(AtermCore::new()))
    )
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_free(core: *mut AtermCore) {
    if !core.is_null() {
        ffi_catch!(drop(Box::from_raw(core)));
    }
}

/// Suspend GPU resources for inactive workspace (#208 memory optimization).
/// Drops Surface + renderer caches. Terminal state preserved. Call resume to reactivate.
/// No-op stub when wgpu feature is disabled (Metal renderer handles GPU lifecycle).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_suspend_gpu(_core: *mut AtermCore) {}

/// Stop the PTY output signal callback. Must be called BEFORE aterm_core_free
/// to prevent use-after-free when the host view is deallocated.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_stop(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).pty_signal.stop());
}

/// No-wgpu init: skip GPU setup but create Terminal state (alacritty_terminal is GPU-independent).
/// Without this, c.terminal stays None and get_render_data returns empty.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_init_gpu(
    core: *mut AtermCore,
    _ns_view: *mut c_void,
    width: u32,
    height: u32,
    _scale: f32,
) -> i32 {
    if core.is_null() {
        return -1;
    }
    ffi_catch!(-1, {
        let c = &mut *core;
        if c.terminal.is_none() {
            let (cols, rows) = no_wgpu_grid_size(width as f32, height as f32);
            let (terminal, advance_handle) = TerminalState::new(
                cols as usize,
                rows as usize,
                Some(c.pty_signal.sync_active_flag()),
            );
            // Sync listener colors from runtime defaults set BEFORE terminal existed.
            // applySettingsToView() stores fg/bg to globals but terminal is None at that point,
            // so set_listener_colors is skipped. Re-apply here so OSC 10/11 queries return
            // the correct scheme colors from the very first shell output.
            let fg_packed = DEFAULT_FG_PACKED.load(Ordering::Relaxed);
            let bg_packed = DEFAULT_BG_PACKED.load(Ordering::Relaxed);
            if let (Some((fr, fg2, fb)), Some((br, bg2, bb))) = (unpack_rgb(fg_packed), unpack_rgb(bg_packed)) {
                terminal.set_listener_colors([fr, fg2, fb], [br, bg2, bb]);
                log_stderr!("[aterm-core] init_gpu: synced listener colors fg=({},{},{}) bg=({},{},{})", fr, fg2, fb, br, bg2, bb);
            }

            c.terminal = Some(terminal);
            c.advance_handle = Some(advance_handle);
            log_stderr!("[aterm-core] no-wgpu init: terminal created {}x{}", cols, rows);
        }
        0
    })
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_spawn_shell(
    core: *mut AtermCore,
    name: *const c_char,
    cwd: *const c_char,
    command: *const c_char,
    cols: u16,
    rows: u16,
) -> i32 {
    if core.is_null() || cwd.is_null() {
        return -1;
    }
    let name_str = if name.is_null() {
        "main".into()
    } else {
        CStr::from_ptr(name).to_string_lossy()
    };
    let cwd_str = CStr::from_ptr(cwd).to_string_lossy();
    let cmd = if command.is_null() {
        None
    } else {
        Some(CStr::from_ptr(command).to_string_lossy())
    };
    ffi_catch!(
        -1,
        (*core).spawn_shell(&name_str, &cwd_str, cmd.as_deref(), cols, rows)
    )
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_write_pty(
    core: *mut AtermCore,
    text: *const c_char,
    len: usize,
) {
    if core.is_null() || text.is_null() {
        return;
    }
    ffi_catch!({
        let slice = std::slice::from_raw_parts(text as *const u8, len);
        if let Ok(s) = std::str::from_utf8(slice) {
            (*core).write_pty(s);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_workspace_is_alive(core: *const AtermCore) -> i32 {
    if core.is_null() {
        return 0;
    }
    ffi_catch!(0, (*core).workspace_is_alive() as i32)
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_named_key(core: *mut AtermCore, key_code: u32) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).named_key(key_code));
}

/// Render — acquires render_lock with brief spin (max 8ms) for direct UI calls
/// (mouseDown, scroll, theme change). Skips if lock cannot be acquired in time.
/// No-op stub when wgpu feature is disabled (Metal renderer on Swift side).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_render(_core: *mut AtermCore) {}

/// Try to render if dirty. Returns 1 if rendered, 0 if skipped.
/// Thread-safe — used by CVDisplayLink and PTY dirty callback for immediate
/// render without CVDisplayLink latency (Ghostty/Alacritty pattern, Fix #153).
/// No-op stub when wgpu feature is disabled.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_try_render(_core: *mut AtermCore) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_resize(core: *mut AtermCore, width: u32, height: u32) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).resize(width, height));
}

/// Compute grid size from pixel dimensions (no-wgpu: uses hardcoded cell metrics).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_grid_size(
    _core: *const AtermCore,
    width: f32,
    height: f32,
    out_cols: *mut u16,
    out_rows: *mut u16,
) {
    let (cols, rows) = no_wgpu_grid_size(width, height);
    if !out_cols.is_null() { *out_cols = cols; }
    if !out_rows.is_null() { *out_rows = rows; }
}

/// Set IME preedit text for inline rendering (#206). Empty string clears.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_preedit(
    core: *mut AtermCore,
    text: *const u8,
    len: u32,
) {
    if core.is_null() {
        return;
    }
    ffi_catch!({
        let s = if text.is_null() || len == 0 {
            String::new()
        } else {
            let slice = std::slice::from_raw_parts(text, len as usize);
            String::from_utf8_lossy(slice).into_owned()
        };
        (*core).preedit_text = s;
    });
}

/// Get cursor position in backing pixels (for IME popup placement, #206).
/// Compute cursor position from terminal grid (no-wgpu: uses hardcoded cell metrics).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_cursor_position(
    core: *const AtermCore,
    width: f32,
    height: f32,
    out_x: *mut f32,
    out_y: *mut f32,
) {
    if core.is_null() { return; }
    let (pad_x, pad_y) = no_wgpu_grid_padding(width, height);
    if let Some(ref terminal) = (*core).terminal {
        let term_arc = terminal.terminal();
        let term = term_arc.lock();
        let cursor = term.grid().cursor.point;
        let display_offset = term.grid().display_offset() as f32;
        let col = cursor.column.0 as f32;
        let row = cursor.line.0 as f32 + display_offset;
        let cell_h = no_wgpu_cell_height();
        let cell_w = no_wgpu_cell_width();
        if !out_x.is_null() { *out_x = pad_x + col * cell_w; }
        if !out_y.is_null() { *out_y = pad_y + row * cell_h; }
        return;
    }
    if !out_x.is_null() { *out_x = 0.0; }
    if !out_y.is_null() { *out_y = 0.0; }
}

/// Return cell dimensions (no-wgpu: uses runtime value from set_line_height, or default).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_cell_size(
    _core: *const AtermCore,
    out_width: *mut f32,
    out_height: *mut f32,
) {
    if !out_width.is_null() { *out_width = no_wgpu_cell_width(); }
    if !out_height.is_null() { *out_height = no_wgpu_cell_height(); }
}

/// Compute centered grid padding (no-wgpu: uses hardcoded cell metrics).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_grid_padding(
    _core: *const AtermCore,
    width: f32,
    height: f32,
    out_pad_x: *mut f32,
    out_pad_y: *mut f32,
) {
    let (pad_x, pad_y) = no_wgpu_grid_padding(width, height);
    if !out_pad_x.is_null() { *out_pad_x = pad_x; }
    if !out_pad_y.is_null() { *out_pad_y = pad_y; }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_take_dirty(core: *mut AtermCore) -> i32 {
    if core.is_null() {
        return 0;
    }
    ffi_catch!(0, (*core).pty_signal.take_dirty() as i32)
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_dirty_callback(
    core: *mut AtermCore,
    callback: Option<unsafe extern "C" fn(*mut c_void)>,
    userdata: *mut c_void,
) {
    if core.is_null() {
        return;
    }
    ffi_catch!({
        let c = &mut *core;
        c.dirty_callback = callback;
        c.dirty_userdata = userdata;

        if let Some(cb) = callback {
            let ud = userdata as usize; // Convert to usize for Send
            c.pty_signal.set_wake_callback(move || unsafe {
                cb(ud as *mut c_void);
            });
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_sync_pty(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    // PTY advance now happens on reader thread.
    // This only drains any leftover startup bytes.
    ffi_catch!((*core).sync_pty_startup());
}

/// Collect terminal grid data for Metal renderer.
/// Swift calls this each frame; Rust locks the terminal grid and fills
/// out_cells[0..out_count] with per-cell character + color + flags.
/// PTY advance happens on the reader thread — this is a pure read.
/// Colors are resolved using the Tokyo Night Dark palette. Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_get_render_data(
    core: *mut AtermCore,
    out_cells: *mut CellDataFFI,
    max_cells: u32,
    out_count: *mut u32,
    out_cols: *mut u16,
    out_rows: *mut u16,
    out_dirty_rows: *mut u8,
) -> i32 {
    // DIAGNOSTIC: print CellDataFFI layout once (evidence for #209 garbling debug)
    use std::sync::Once;
    static DIAG: Once = Once::new();
    DIAG.call_once(|| {
        eprintln!("[FFI-DIAG] CellDataFFI size={} align={}",
            std::mem::size_of::<CellDataFFI>(),
            std::mem::align_of::<CellDataFFI>());
        eprintln!("[FFI-DIAG] offsets: col={} row={} fg_r={} fg_g={} fg_b={} fg_a={} bg_r={} bg_g={} bg_b={} bg_a={} character={} flags={}",
            std::mem::offset_of!(CellDataFFI, col),
            std::mem::offset_of!(CellDataFFI, row),
            std::mem::offset_of!(CellDataFFI, fg_r),
            std::mem::offset_of!(CellDataFFI, fg_g),
            std::mem::offset_of!(CellDataFFI, fg_b),
            std::mem::offset_of!(CellDataFFI, fg_a),
            std::mem::offset_of!(CellDataFFI, bg_r),
            std::mem::offset_of!(CellDataFFI, bg_g),
            std::mem::offset_of!(CellDataFFI, bg_b),
            std::mem::offset_of!(CellDataFFI, bg_a),
            std::mem::offset_of!(CellDataFFI, character),
            std::mem::offset_of!(CellDataFFI, flags),
        );
    });

    if core.is_null() || out_cells.is_null() || out_count.is_null() {
        return -1;
    }
    ffi_catch!(-1, {
        let c = &mut *core;
        // PTY advance now happens on reader thread — pure read here.

        let Some(ref terminal) = c.terminal else {
            *out_count = 0;
            if !out_cols.is_null() { *out_cols = 0; }
            if !out_rows.is_null() { *out_rows = 0; }
            debug_log!("[render-data] terminal is None (shell not yet spawned)");
            return 0;
        };

        let term_arc = terminal.terminal();
        let mut term = term_arc.lock();  // FairMutex::lock() — fair, no poison

        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line, Point};
        use alacritty_terminal::term::cell::Flags;
        use alacritty_terminal::term::TermDamage;
        use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

        let cols = term.grid().columns() as u16;
        let rows = term.grid().screen_lines() as u16;

        // --- Row-level dirty tracking (#231) ---
        // Collect damage from alacritty_terminal before reading cells.
        let rows_usize = rows as usize;
        let mut dirty_rows = vec![false; rows_usize];
        let mut all_dirty = match term.damage() {
            TermDamage::Full => { dirty_rows.fill(true); true }
            TermDamage::Partial(iter) => {
                for ld in iter {
                    if ld.line < rows_usize { dirty_rows[ld.line] = true; }
                }
                false
            }
        };
        // Force full damage on scroll offset change or active selection (renderer.rs pattern).
        let display_offset = term.grid().display_offset();
        if display_offset != c.last_display_offset || term.selection.is_some() {
            all_dirty = true;
            dirty_rows.fill(true);
        }
        c.last_display_offset = display_offset;
        term.reset_damage();

        // Output dirty mask to Swift (1 = dirty, 0 = clean).
        if !out_dirty_rows.is_null() {
            let dirty_out = std::slice::from_raw_parts_mut(out_dirty_rows, rows_usize);
            for (i, &d) in dirty_rows.iter().enumerate() {
                dirty_out[i] = d as u8;
            }
        }

        if !out_cols.is_null() {
            *out_cols = cols;
        }
        if !out_rows.is_null() {
            *out_rows = rows;
        }

        // Theme-aware defaults — read from atomics (set by aterm_core_set_default_colors),
        // fallback to hardcoded values if not set.
        let is_light = {
            { c.theme_mode_raw == 1 }
        };
        let hardcoded_fg = if is_light { (0x24u8, 0x29u8, 0x2fu8) } else { (0xc0u8, 0xcau8, 0xf5u8) };
        let hardcoded_bg = if is_light { (0xfau8, 0xf6u8, 0xf0u8) } else { (0x28u8, 0x2cu8, 0x34u8) };
        let default_fg = unpack_rgb(DEFAULT_FG_PACKED.load(Ordering::Relaxed)).unwrap_or(hardcoded_fg);
        let default_bg = unpack_rgb(DEFAULT_BG_PACKED.load(Ordering::Relaxed)).unwrap_or(hardcoded_bg);

        // ANSI 16-color palette — dynamic from current color scheme
        let ansi16 = ansi16_for_scheme(NO_WGPU_COLOR_SCHEME.load(Ordering::Relaxed));

        let resolve_color = |color: &AnsiColor, is_fg: bool| -> (u8, u8, u8) {
            match color {
                AnsiColor::Named(named) => match named {
                    NamedColor::Black => ansi16[0],
                    NamedColor::Red => ansi16[1],
                    NamedColor::Green => ansi16[2],
                    NamedColor::Yellow => ansi16[3],
                    NamedColor::Blue => ansi16[4],
                    NamedColor::Magenta => ansi16[5],
                    NamedColor::Cyan => ansi16[6],
                    NamedColor::White => ansi16[7],
                    NamedColor::BrightBlack => ansi16[8],
                    NamedColor::BrightRed => ansi16[9],
                    NamedColor::BrightGreen => ansi16[10],
                    NamedColor::BrightYellow => ansi16[11],
                    NamedColor::BrightBlue => ansi16[12],
                    NamedColor::BrightMagenta => ansi16[13],
                    NamedColor::BrightCyan => ansi16[14],
                    NamedColor::BrightWhite => ansi16[15],
                    NamedColor::Foreground
                    | NamedColor::BrightForeground
                    | NamedColor::DimForeground => default_fg,
                    NamedColor::Background => default_bg,
                    NamedColor::Cursor => default_fg,
                    _ => if is_fg { default_fg } else { default_bg },
                },
                AnsiColor::Spec(rgb) => (rgb.r, rgb.g, rgb.b),
                AnsiColor::Indexed(idx) => {
                    let i = *idx;
                    if i < 16 {
                        return ansi16[i as usize];
                    }
                    if i < 232 {
                        let v = i - 16;
                        let r = if v / 36 > 0 { (v / 36) * 40 + 55 } else { 0 };
                        let g = if (v % 36) / 6 > 0 { ((v % 36) / 6) * 40 + 55 } else { 0 };
                        let b = if v % 6 > 0 { (v % 6) * 40 + 55 } else { 0 };
                        return (r, g, b);
                    }
                    let v = (i - 232) * 10 + 8;
                    (v, v, v)
                }
            }
        };

        let display_offset = term.grid().display_offset() as i32;

        // Debug: dump first 20 characters from row 0 to check if grid has content
        if debug_log_enabled() {
            let mut sample = String::with_capacity(40);
            for ci in 0..std::cmp::min(20, cols as usize) {
                let pt = Point::new(Line(0 - display_offset), Column(ci));
                let ch = term.grid()[pt].c;
                if ch == ' ' {
                    sample.push('·');
                } else {
                    sample.push(ch);
                }
            }
            let cursor = term.grid().cursor.point;
            debug_log!(
                "[render-data] grid={}x{} display_offset={} cursor=({},{}) row0=[{}] workspace={:?}",
                cols, rows, display_offset,
                cursor.column.0, cursor.line.0,
                sample,
                c.workspace_id
            );
        }

        let cells_slice = std::slice::from_raw_parts_mut(out_cells, max_cells as usize);
        let mut count = 0u32;

        'outer: for row_idx in 0..rows {
            // Skip clean rows — Swift keeps previous frame data (#231).
            if !all_dirty && !dirty_rows[row_idx as usize] {
                continue;
            }
            let grid_line = row_idx as i32 - display_offset;
            for col_idx in 0..cols {
                if count >= max_cells {
                    break 'outer;
                }
                let point = Point::new(Line(grid_line), Column(col_idx as usize));
                let cell = &term.grid()[point];
                let (fg_r, fg_g, fg_b) = resolve_color(&cell.fg, true);

                // INVERSE: swap fg→bg (renderer.rs cell_background_rgba pattern)
                // Suppress INVERSE on cursor cell during preedit so IME text is visible.
                let is_inverse = cell.flags.contains(Flags::INVERSE)
                    && !PREEDIT_ACTIVE.load(Ordering::Relaxed);
                let (bg_r, bg_g, bg_b) = if is_inverse {
                    resolve_color(&cell.fg, true)
                } else {
                    resolve_color(&cell.bg, false)
                };

                let flags = (cell.flags.contains(Flags::BOLD) as u8)
                    | ((cell.flags.contains(Flags::ITALIC) as u8) << 1)
                    | ((cell.flags.contains(Flags::UNDERLINE) as u8) << 2);

                let bg_a: u8 = if is_inverse {
                    255  // INVERSE: fg color as bg, must be opaque
                } else if matches!(cell.bg, AnsiColor::Named(NamedColor::Background)) {
                    0    // default bg: transparent — bg_color pass covers it
                } else {
                    255  // explicit TrueColor/Indexed/Named: render the actual terminal bg color
                };

                cells_slice[count as usize] = CellDataFFI {
                    col: col_idx,
                    row: row_idx,
                    fg_r,
                    fg_g,
                    fg_b,
                    fg_a: 255,
                    bg_r,
                    bg_g,
                    bg_b,
                    bg_a,
                    character: cell.c as u32,
                    flags,
                };
                count += 1;
            }
        }

        // DIAGNOSTIC Phase 2: first 10 non-space cell character values (evidence for #209)
        // Compare Rust vs Swift: MATCH → garbling is in atlas/shader. MISMATCH → data transfer bug.
        // Deferred: fires on FIRST frame with actual non-space content (not empty grid).
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static DIAG2_DONE: AtomicBool = AtomicBool::new(false);
            if !DIAG2_DONE.load(Ordering::Relaxed) {
                let non_space: Vec<_> = cells_slice[..count as usize].iter()
                    .enumerate()
                    .filter(|(_, c)| c.character > 0x20 && c.character < 0x10000)
                    .take(10)
                    .collect();
                if !non_space.is_empty() {
                    DIAG2_DONE.store(true, Ordering::Relaxed);
                    for (i, c) in &non_space {
                        let ch = char::from_u32(c.character).unwrap_or('?');
                        eprintln!(
                            "[FFI-DIAG2] cell[{}] col={} row={} char=U+{:04X}('{}') fg=({},{},{},{}) bg=({},{},{},{}) flags={}",
                            i, c.col, c.row, c.character, ch,
                            c.fg_r, c.fg_g, c.fg_b, c.fg_a,
                            c.bg_r, c.bg_g, c.bg_b, c.bg_a,
                            c.flags,
                        );
                        let ptr = *c as *const CellDataFFI as *const u8;
                        let bytes: Vec<u8> = (0..std::mem::size_of::<CellDataFFI>())
                            .map(|j| *ptr.add(j))
                            .collect();
                        eprintln!("[FFI-DIAG2] cell[{}] raw bytes: {:02X?}", i, bytes);
                    }
                    eprintln!("[FFI-DIAG2] total non-space cells (char>0x20): {}/{}",
                        cells_slice[..count as usize].iter().filter(|c| c.character > 0x20).count(), count);
                }
            }
        }

        *out_count = count;
        0
    })
}

/// Drain all pending workspace events as a C struct batch.
/// Caller must free the returned batch with aterm_free_events().
#[no_mangle]
pub extern "C" fn aterm_drain_events() -> AtermEventBatch {
    let empty = AtermEventBatch {
        events: std::ptr::null_mut(),
        count: 0,
    };
    ffi_catch!(empty, {
        use aterm_session::types::WorkspaceEvent;

        let events = {
            let Ok(mut queue) = global_event_queue().lock() else {
                return AtermEventBatch {
                    events: std::ptr::null_mut(),
                    count: 0,
                };
            };
            std::mem::take(&mut *queue)
        };

        if events.is_empty() {
            return AtermEventBatch {
                events: std::ptr::null_mut(),
                count: 0,
            };
        }

        let count = events.len() as u32;
        let mut ffi_events: Vec<AtermEventFFI> = Vec::with_capacity(events.len());

        for event in events {
            let ffi = match event {
                WorkspaceEvent::Created { id, name } => AtermEventFFI {
                    event_type: ATERM_EVENT_CREATED,
                    id: CString::new(id).unwrap_or_default().into_raw(),
                    name: CString::new(name).unwrap_or_default().into_raw(),
                    status: std::ptr::null(),
                    title: std::ptr::null(),
                },
                WorkspaceEvent::Closed { id } => AtermEventFFI {
                    event_type: ATERM_EVENT_CLOSED,
                    id: CString::new(id).unwrap_or_default().into_raw(),
                    name: std::ptr::null(),
                    status: std::ptr::null(),
                    title: std::ptr::null(),
                },
                WorkspaceEvent::StatusChanged { id, status } => AtermEventFFI {
                    event_type: ATERM_EVENT_STATUS_CHANGED,
                    id: CString::new(id).unwrap_or_default().into_raw(),
                    name: std::ptr::null(),
                    status: CString::new(status).unwrap_or_default().into_raw(),
                    title: std::ptr::null(),
                },
                WorkspaceEvent::TitleChanged { id, title } => AtermEventFFI {
                    event_type: ATERM_EVENT_TITLE_CHANGED,
                    id: CString::new(id).unwrap_or_default().into_raw(),
                    name: std::ptr::null(),
                    status: std::ptr::null(),
                    title: CString::new(title).unwrap_or_default().into_raw(),
                },
                WorkspaceEvent::ShellReady { id } => AtermEventFFI {
                    event_type: ATERM_EVENT_SHELL_READY,
                    id: CString::new(id).unwrap_or_default().into_raw(),
                    name: std::ptr::null(),
                    status: std::ptr::null(),
                    title: std::ptr::null(),
                },
                WorkspaceEvent::TrustPromptDetected { id } => AtermEventFFI {
                    event_type: ATERM_EVENT_TRUST_PROMPT,
                    id: CString::new(id).unwrap_or_default().into_raw(),
                    name: std::ptr::null(),
                    status: std::ptr::null(),
                    title: std::ptr::null(),
                },
            };
            ffi_events.push(ffi);
        }

        let ptr = ffi_events.as_mut_ptr();
        std::mem::forget(ffi_events);

        AtermEventBatch { events: ptr, count }
    })
}

/// Free a batch returned by aterm_drain_events().
#[no_mangle]
pub unsafe extern "C" fn aterm_free_events(batch: AtermEventBatch) {
    if batch.events.is_null() || batch.count == 0 {
        return;
    }
    ffi_catch!({
        let events = Vec::from_raw_parts(batch.events, batch.count as usize, batch.count as usize);
        for event in events {
            if !event.id.is_null() {
                drop(CString::from_raw(event.id as *mut c_char));
            }
            if !event.name.is_null() {
                drop(CString::from_raw(event.name as *mut c_char));
            }
            if !event.status.is_null() {
                drop(CString::from_raw(event.status as *mut c_char));
            }
            if !event.title.is_null() {
                drop(CString::from_raw(event.title as *mut c_char));
            }
        }
    });
}

/// Store theme mode when wgpu feature is disabled (used by get_render_data).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_theme_mode(core: *mut AtermCore, mode: u8) {
    if core.is_null() { return; }
    ffi_catch!({
        (*core).theme_mode_raw = mode; // 0 = dark, 1 = light
    });
}

/// Set color scheme: 0=Dark, 1=Light, 2=SolarizedDark, 3=SolarizedLight,
/// 4=Monokai, 5=Dracula, 6=Nord, 7=TokyoNight
/// Store color scheme for no-wgpu path.
static NO_WGPU_COLOR_SCHEME: AtomicU8 = AtomicU8::new(8); // 8 = Default
static PREEDIT_ACTIVE: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_color_scheme(_core: *mut AtermCore, scheme: u8) {
    if scheme <= 8 {
        NO_WGPU_COLOR_SCHEME.store(scheme, Ordering::Relaxed);
    }
}

/// Set preedit (IME composition) active state.
/// When active, cursor cell INVERSE is suppressed so preedit text is visible.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_preedit_active(
    _core: *mut AtermCore,
    active: bool,
) {
    PREEDIT_ACTIVE.store(active, Ordering::Relaxed);
}

/// Return ANSI 16-color palette for a color scheme index.
/// Used by get_render_data (no-wgpu path) for dynamic palette resolution.
fn ansi16_for_scheme(scheme: u8) -> [(u8, u8, u8); 16] {
    match scheme {
        0 => [ // Dark
            (0x48,0x4f,0x58),(0xf8,0x53,0x49),(0x3f,0xb9,0x50),(0xd4,0xa5,0x74),
            (0x8b,0x5c,0xf6),(0xec,0x48,0x99),(0x06,0xb6,0xd4),(0xd0,0xd7,0xde),
            (0x6e,0x76,0x81),(0xff,0x7b,0x72),(0x56,0xd3,0x64),(0xe3,0xc1,0x9c),
            (0xa7,0x8b,0xfa),(0xf4,0x72,0xb6),(0x22,0xd3,0xee),(0xf0,0xf6,0xfc),
        ],
        1 => [ // Light
            (0x24,0x29,0x2f),(0xcf,0x22,0x2e),(0x1a,0x7f,0x37),(0x93,0x57,0x0a),
            (0x6d,0x28,0xd9),(0xbf,0x39,0x89),(0x0e,0x74,0x90),(0x8b,0x94,0x9e),
            (0x57,0x60,0x6a),(0xd5,0x53,0x4a),(0x2e,0xa4,0x4f),(0xb4,0x53,0x09),
            (0x8b,0x5c,0xf6),(0xec,0x48,0x99),(0x06,0xb6,0xd4),(0x6e,0x77,0x81),
        ],
        2 => [ // SolarizedDark
            (0x07,0x36,0x42),(0xdc,0x32,0x2f),(0x85,0x99,0x00),(0xb5,0x89,0x00),
            (0x26,0x8b,0xd2),(0xd3,0x36,0x82),(0x2a,0xa1,0x98),(0xee,0xe8,0xd5),
            (0x00,0x2b,0x36),(0xcb,0x4b,0x16),(0x58,0x6e,0x75),(0x65,0x7b,0x83),
            (0x83,0x94,0x96),(0x6c,0x71,0xc4),(0x93,0xa1,0xa1),(0xfd,0xf6,0xe3),
        ],
        3 => [ // SolarizedLight
            (0x07,0x36,0x42),(0xdc,0x32,0x2f),(0x85,0x99,0x00),(0xb5,0x89,0x00),
            (0x26,0x8b,0xd2),(0xd3,0x36,0x82),(0x2a,0xa1,0x98),(0xee,0xe8,0xd5),
            (0x00,0x2b,0x36),(0xcb,0x4b,0x16),(0x58,0x6e,0x75),(0x65,0x7b,0x83),
            (0x83,0x94,0x96),(0x6c,0x71,0xc4),(0x93,0xa1,0xa1),(0xfd,0xf6,0xe3),
        ],
        4 => [ // Monokai
            (0x27,0x28,0x22),(0xf9,0x26,0x72),(0xa6,0xe2,0x2e),(0xf4,0xbf,0x75),
            (0x66,0xd9,0xef),(0xae,0x81,0xff),(0xa1,0xef,0xe4),(0xf8,0xf8,0xf2),
            (0x75,0x71,0x5e),(0xf9,0x26,0x72),(0xa6,0xe2,0x2e),(0xf4,0xbf,0x75),
            (0x66,0xd9,0xef),(0xae,0x81,0xff),(0xa1,0xef,0xe4),(0xf9,0xf8,0xf5),
        ],
        5 => [ // Dracula
            (0x21,0x22,0x2c),(0xff,0x55,0x55),(0x50,0xfa,0x7b),(0xf1,0xfa,0x8c),
            (0xbd,0x93,0xf9),(0xff,0x79,0xc6),(0x8b,0xe9,0xfd),(0xf8,0xf8,0xf2),
            (0x62,0x72,0xa4),(0xff,0x6e,0x6e),(0x69,0xff,0x94),(0xff,0xff,0xa5),
            (0xd6,0xac,0xff),(0xff,0x92,0xdf),(0xa4,0xff,0xff),(0xff,0xff,0xff),
        ],
        6 => [ // Nord
            (0x3b,0x42,0x52),(0xbf,0x61,0x6a),(0xa3,0xbe,0x8c),(0xeb,0xcb,0x8b),
            (0x81,0xa1,0xc1),(0xb4,0x8e,0xad),(0x88,0xc0,0xd0),(0xe5,0xe9,0xf0),
            (0x4c,0x56,0x6a),(0xbf,0x61,0x6a),(0xa3,0xbe,0x8c),(0xeb,0xcb,0x8b),
            (0x81,0xa1,0xc1),(0xb4,0x8e,0xad),(0x8f,0xbc,0xbb),(0xec,0xef,0xf4),
        ],
        7 => [ // TokyoNight
            (0x15,0x16,0x1e),(0xf7,0x76,0x8e),(0x9e,0xce,0x6a),(0xe0,0xaf,0x68),
            (0x7a,0xa2,0xf7),(0xbb,0x9a,0xf7),(0x7d,0xcf,0xff),(0xc0,0xca,0xf5),
            (0x41,0x48,0x68),(0xf7,0x76,0x8e),(0x9e,0xce,0x6a),(0xe0,0xaf,0x68),
            (0x7a,0xa2,0xf7),(0xbb,0x9a,0xf7),(0x7d,0xcf,0xff),(0xc0,0xca,0xf5),
        ],
        _ => [ // Default (xterm-256)
            (0x00,0x00,0x00),(0xCC,0x00,0x00),(0x00,0xCC,0x00),(0xCC,0xCC,0x00),
            (0x00,0x00,0xCC),(0xCC,0x00,0xCC),(0x00,0xCC,0xCC),(0xC0,0xCA,0xF5),
            (0x55,0x55,0x55),(0xFF,0x00,0x00),(0x00,0xFF,0x00),(0xFF,0xFF,0x00),
            (0x55,0x55,0xFF),(0xFF,0x00,0xFF),(0x00,0xFF,0xFF),(0xFF,0xFF,0xFF),
        ],
    }
}

// Runtime-overridable default fg/bg colors (set via aterm_core_set_default_colors).
// Packed as (1 << 24) | (r << 16) | (g << 8) | b; 0 = not set (use hardcoded fallback).
static DEFAULT_FG_PACKED: AtomicU32 = AtomicU32::new(0);
static DEFAULT_BG_PACKED: AtomicU32 = AtomicU32::new(0);

fn unpack_rgb(packed: u32) -> Option<(u8, u8, u8)> {
    if packed == 0 { return None; }
    Some(((packed >> 16) as u8, (packed >> 8) as u8, packed as u8))
}

fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    (1u32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Set default fg/bg colors used by render_cells for palette Background/Foreground.
/// Also syncs colors to terminal listener for OSC 10/11 query responses.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_default_colors(
    core: *mut AtermCore,
    fg_r: u8, fg_g: u8, fg_b: u8,
    bg_r: u8, bg_g: u8, bg_b: u8,
) {
    DEFAULT_FG_PACKED.store(pack_rgb(fg_r, fg_g, fg_b), Ordering::Relaxed);
    DEFAULT_BG_PACKED.store(pack_rgb(bg_r, bg_g, bg_b), Ordering::Relaxed);
    // Sync to terminal listener so OSC 10/11 queries return the correct colors (#196).
    if !core.is_null() {
        if let Some(ref terminal) = (*core).terminal {
            terminal.set_listener_colors([fg_r, fg_g, fg_b], [bg_r, bg_g, bg_b]);
        }
    }
}

/// Return the foreground RGB for a color scheme index.
/// Always available (no wgpu gate) — used by Swift Metal path.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_scheme_fg_color(
    scheme: u8,
    out_r: *mut u8,
    out_g: *mut u8,
    out_b: *mut u8,
) {
    let (r, g, b) = match scheme {
        0 => (0xc0, 0xca, 0xf5), // Dark
        1 => (0x24, 0x29, 0x2f), // Light
        2 => (0x83, 0x94, 0x96), // SolarizedDark
        3 => (0x65, 0x7b, 0x83), // SolarizedLight
        4 => (0xf8, 0xf8, 0xf2), // Monokai
        5 => (0xf8, 0xf8, 0xf2), // Dracula
        6 => (0xd8, 0xde, 0xe9), // Nord
        7 => (0xc0, 0xca, 0xf5), // TokyoNight
        8 | _ => (0xC0, 0xCA, 0xF5), // Default
    };
    if !out_r.is_null() { *out_r = r; }
    if !out_g.is_null() { *out_g = g; }
    if !out_b.is_null() { *out_b = b; }
}

/// Return the background RGB for a color scheme index.
/// Always available (no wgpu gate) — used by Swift Metal path.
/// scheme: 0=Dark, 1=Light, 2=SolarizedDark, 3=SolarizedLight,
///         4=Monokai, 5=Dracula, 6=Nord, 7=TokyoNight, 8=Default
#[no_mangle]
pub unsafe extern "C" fn aterm_core_scheme_bg_color(
    scheme: u8,
    out_r: *mut u8,
    out_g: *mut u8,
    out_b: *mut u8,
) {
    let (r, g, b) = match scheme {
        0 => (0x28, 0x2C, 0x34), // Dark
        1 => (0xfa, 0xf6, 0xf0), // Light
        2 => (0x00, 0x2b, 0x36), // SolarizedDark
        3 => (0xfd, 0xf6, 0xe3), // SolarizedLight
        4 => (0x27, 0x28, 0x22), // Monokai
        5 => (0x28, 0x2a, 0x36), // Dracula
        6 => (0x2e, 0x34, 0x40), // Nord
        7 => (0x1a, 0x1b, 0x26), // TokyoNight
        8 | _ => (0x28, 0x2C, 0x34), // Default (Atom One Dark)
    };
    if !out_r.is_null() { *out_r = r; }
    if !out_g.is_null() { *out_g = g; }
    if !out_b.is_null() { *out_b = b; }
}

/// Set font size in pixels (clamped to 8..32)
/// No-op stub when wgpu feature is disabled (font size managed on Swift side).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_font_size(_core: *mut AtermCore, _size: f32) {}

/// Set line height in pixels (clamped to 12..64)
/// Store line height for no-wgpu path (clamped to 12..64).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_line_height(_core: *mut AtermCore, height: f32) {
    let clamped = height.clamp(12.0, 64.0);
    NO_WGPU_CELL_HEIGHT_ATOMIC.store(clamped.to_bits(), Ordering::Relaxed);
}

/// Set cell width in pixels (clamped to 6..32)
/// Store cell width for no-wgpu path (clamped to 6..32).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_cell_width(_core: *mut AtermCore, width: f32) {
    let clamped = width.clamp(6.0, 32.0);
    NO_WGPU_CELL_WIDTH_ATOMIC.store(clamped.to_bits(), Ordering::Relaxed);
}

/// Deprecated compatibility no-op. Background blending has been removed and
/// terminal cell colors are now rendered exactly as provided by the terminal.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_bg_blend_threshold(_core: *mut AtermCore, _threshold: f32) {
    ffi_catch!({});
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_scroll(core: *mut AtermCore, delta: i32) {
    if core.is_null() {
        return;
    }
    ffi_catch!({
        if let Some(ref mut terminal) = (*core).terminal {
            terminal.scroll(delta);
        }
    });
}

/// Scroll to the next/previous shell prompt (OSC 133 marks).
/// direction < 0 = previous prompt (up), direction > 0 = next prompt (down).
/// Returns 1 if scrolled, 0 if no prompt found in that direction.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_scroll_to_prompt(
    core: *mut AtermCore,
    direction: i32,
) -> i32 {
    if core.is_null() {
        return 0;
    }
    ffi_catch!(0, {
        if let Some(ref mut terminal) = (*core).terminal {
            if terminal.scroll_to_prompt(direction) {
                1
            } else {
                0
            }
        } else {
            0
        }
    })
}

/// Returns the number of OSC 133 prompt marks currently stored.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_prompt_mark_count(core: *const AtermCore) -> u32 {
    if core.is_null() {
        return 0;
    }
    ffi_catch!(0, {
        if let Some(ref terminal) = (*core).terminal {
            terminal.prompt_mark_count() as u32
        } else {
            0
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_selection_start(
    core: *mut AtermCore,
    col: u32,
    line: i32,
    side: u8,
) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).selection_start(col as usize, line, side));
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_selection_update(
    core: *mut AtermCore,
    col: u32,
    line: i32,
    side: u8,
) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).selection_update(col as usize, line, side));
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_selection_clear(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).selection_clear());
}

/// Returns selected text or NULL. Caller must free with aterm_core_free_string.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_selection_text(core: *const AtermCore) -> *mut c_char {
    if core.is_null() {
        return std::ptr::null_mut();
    }
    ffi_catch!(std::ptr::null_mut(), {
        match (*core).selection_text() {
            Some(text) => match std::ffi::CString::new(text) {
                Ok(cs) => cs.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_select_all(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).select_all());
}

/// Export the current selection as per-row ranges for Metal selection overlay.
/// Multi-line selections are split into one range per visible row.
/// Returns the number of ranges written via `out_count`.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_selection_ranges(
    core: *const AtermCore,
    out_ranges: *mut SelectionRangeFFI,
    max_ranges: u32,
    out_count: *mut u32,
) {
    if core.is_null() || out_ranges.is_null() || out_count.is_null() {
        if !out_count.is_null() {
            *out_count = 0;
        }
        return;
    }
    ffi_catch!({
        *out_count = 0;
        let c = &*core;
        let terminal = match &c.terminal {
            Some(t) => t,
            None => return,
        };
        let term_arc = terminal.terminal();
        let term = term_arc.lock();

        use alacritty_terminal::grid::Dimensions;

        let selection = match &term.selection {
            Some(s) => s,
            None => return,
        };

        let sel_range = match selection.to_range(&term) {
            Some(r) => r,
            None => return,
        };

        let display_offset = term.grid().display_offset() as i32;
        let cols = term.grid().columns() as u16;
        let rows = term.grid().screen_lines() as u16;

        // Convert grid-absolute to viewport-relative
        let start_vp_row = sel_range.start.line.0 + display_offset;
        let end_vp_row = sel_range.end.line.0 + display_offset;
        let start_col = sel_range.start.column.0 as u16;
        let end_col = sel_range.end.column.0 as u16;

        // Selection highlight color — semi-transparent blue
        let (sr, sg, sb, sa) = (51u8, 102u8, 204u8, 100u8);

        let ranges_slice = std::slice::from_raw_parts_mut(out_ranges, max_ranges as usize);
        let mut count: u32 = 0;

        if sel_range.is_block {
            // Block selection: same columns on every row
            for row in start_vp_row..=end_vp_row {
                if row < 0 || row >= rows as i32 {
                    continue;
                }
                if count >= max_ranges {
                    break;
                }
                ranges_slice[count as usize] = SelectionRangeFFI {
                    start_col,
                    start_row: row as u16,
                    end_col,
                    end_row: row as u16,
                    r: sr, g: sg, b: sb, a: sa,
                };
                count += 1;
            }
        } else if start_vp_row == end_vp_row {
            // Single-line selection
            if start_vp_row >= 0 && start_vp_row < rows as i32 && count < max_ranges {
                ranges_slice[count as usize] = SelectionRangeFFI {
                    start_col,
                    start_row: start_vp_row as u16,
                    end_col,
                    end_row: start_vp_row as u16,
                    r: sr, g: sg, b: sb, a: sa,
                };
                count += 1;
            }
        } else {
            // Multi-line: first row, middle rows, last row
            // First row: start_col → end of line
            if start_vp_row >= 0 && start_vp_row < rows as i32 && count < max_ranges {
                ranges_slice[count as usize] = SelectionRangeFFI {
                    start_col,
                    start_row: start_vp_row as u16,
                    end_col: cols.saturating_sub(1),
                    end_row: start_vp_row as u16,
                    r: sr, g: sg, b: sb, a: sa,
                };
                count += 1;
            }
            // Middle rows: full line
            for row in (start_vp_row + 1)..end_vp_row {
                if row < 0 || row >= rows as i32 {
                    continue;
                }
                if count >= max_ranges {
                    break;
                }
                ranges_slice[count as usize] = SelectionRangeFFI {
                    start_col: 0,
                    start_row: row as u16,
                    end_col: cols.saturating_sub(1),
                    end_row: row as u16,
                    r: sr, g: sg, b: sb, a: sa,
                };
                count += 1;
            }
            // Last row: start of line → end_col
            if end_vp_row >= 0 && end_vp_row < rows as i32 && count < max_ranges {
                ranges_slice[count as usize] = SelectionRangeFFI {
                    start_col: 0,
                    start_row: end_vp_row as u16,
                    end_col,
                    end_row: end_vp_row as u16,
                    r: sr, g: sg, b: sb, a: sa,
                };
                count += 1;
            }
        }

        *out_count = count;
    });
}

/// Check if the visible terminal screen contains a text pattern. Returns 1 if found, 0 otherwise.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_screen_contains(
    core: *const AtermCore,
    pattern: *const c_char,
) -> i32 {
    if core.is_null() || pattern.is_null() {
        return 0;
    }
    ffi_catch!(0, {
        let pattern_str = CStr::from_ptr(pattern).to_string_lossy();
        match &(*core).terminal {
            Some(terminal) => terminal.screen_contains(&pattern_str) as i32,
            None => 0,
        }
    })
}

/// Returns JSON string of internal workspaces. Caller must free with aterm_core_free_string.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_list_workspaces(core: *const AtermCore) -> *mut c_char {
    if core.is_null() {
        return std::ptr::null_mut();
    }
    ffi_catch!(std::ptr::null_mut(), {
        let workspaces = (*core).pty_manager.list_workspaces();
        let json = serde_json::to_string(&workspaces).unwrap_or_else(|_| "[]".to_string());
        match std::ffi::CString::new(json) {
            Ok(cs) => cs.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        ffi_catch!(drop(std::ffi::CString::from_raw(ptr)));
    }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_tailscale_connect(
    hostname: *const c_char,
    control_url: *const c_char,
    auth_key: *const c_char,
) -> i32 {
    let hostname = optional_c_string(hostname);
    let control_url = optional_c_string(control_url);
    let auth_key = optional_c_string(auth_key);

    ffi_catch!(-1, {
        match tailscale::global_manager().connect(
            hostname.as_deref(),
            control_url.as_deref(),
            auth_key.as_deref(),
        ) {
            Ok(()) => 0,
            Err(error) => {
                log_stderr!("[aterm-core] tailscale connect failed: {error}");
                -1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn aterm_tailscale_shutdown() {
    ffi_catch!(tailscale::global_manager().shutdown());
}

#[no_mangle]
pub extern "C" fn aterm_tailscale_status_json() -> *mut c_char {
    ffi_catch!(std::ptr::null_mut(), {
        let status = tailscale::global_manager().status();
        let json = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
        match std::ffi::CString::new(json) {
            Ok(cs) => cs.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

#[no_mangle]
pub extern "C" fn aterm_core_detect_clis() -> *mut c_char {
    ffi_catch!(std::ptr::null_mut(), {
        let detection = CliDetectionStatus {
            claude: detect_cli_installed("claude", ".claude"),
            codex: detect_cli_installed("codex", ".codex"),
            gemini: detect_cli_installed("gemini", ".gemini"),
        };
        let json = serde_json::to_string(&detection).unwrap_or_else(|_| "{}".to_string());
        match std::ffi::CString::new(json) {
            Ok(cs) => cs.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

fn optional_c_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }

    let text = unsafe { CStr::from_ptr(value) }.to_string_lossy();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn detect_cli_installed(command: &str, config_dir_name: &str) -> bool {
    command_available(command) && config_dir_exists(config_dir_name)
}

fn command_available(command: &str) -> bool {
    let mut process = ProcessCommand::new("/bin/sh");
    process.args(["-lc", &format!("command -v -- {command} >/dev/null 2>&1")]);
    if let Some(path_env) = crate::pty::augmented_path_env() {
        process.env("PATH", path_env);
    }

    process
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn config_dir_exists(config_dir_name: &str) -> bool {
    std::env::home_dir()
        .map(|home| home.join(config_dir_name).is_dir())
        .unwrap_or(false)
}

// -- Session persistence FFI --

use crate::session::{sessions_path, SessionEntryFFI, SessionStore};

#[no_mangle]
pub unsafe extern "C" fn aterm_session_count(_core: *const AtermCore) -> u32 {
    ffi_catch!(0, {
        let store = SessionStore::with_path(sessions_path());
        store.load().map(|d| d.sessions.len() as u32).unwrap_or(0)
    })
}

#[no_mangle]
pub unsafe extern "C" fn aterm_session_get(_core: *const AtermCore, index: u32) -> SessionEntryFFI {
    ffi_catch!(SessionEntryFFI::null(), {
        let store = SessionStore::with_path(sessions_path());
        let entries = store.load().map(|d| d.sessions).unwrap_or_default();
        if (index as usize) < entries.len() {
            SessionEntryFFI::from_entry(&entries[index as usize])
        } else {
            SessionEntryFFI::null()
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn aterm_session_free(entry: SessionEntryFFI) {
    ffi_catch!({
        let free_ptr = |p: *const c_char| {
            if !p.is_null() {
                drop(CString::from_raw(p as *mut c_char));
            }
        };
        free_ptr(entry.id);
        free_ptr(entry.cwd);
        free_ptr(entry.command);
        free_ptr(entry.args_json);
        free_ptr(entry.custom_command);
        free_ptr(entry.resume_command);
    });
}

#[no_mangle]
pub unsafe extern "C" fn aterm_sessions_save(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    ffi_catch!({
        let store = SessionStore::with_path(sessions_path());
        let _ = store.save(&(*core).pty_manager);
    });
}

#[no_mangle]
pub unsafe extern "C" fn aterm_sessions_restore(core: *mut AtermCore) -> u32 {
    if core.is_null() {
        return 0;
    }
    ffi_catch!(0, {
        let core = &mut *core;
        let store = SessionStore::with_path(sessions_path());
        match store.load() {
            Ok(data) => {
                let count = data.sessions.len() as u32;
                for entry in data.sessions {
                    if let Err(e) = core.pty_manager.restore_session_entry(entry) {
                        log_stderr!("[aterm] restore session skipped: {e}");
                    }
                }
                count
            }
            Err(_) => 0,
        }
    })
}

// -- IPC Phase 1: AtermApp singleton + C ABI --

use crate::app::AtermApp;

static ATERM_APP: OnceLock<Arc<std::sync::Mutex<AtermApp>>> = OnceLock::new();

pub(crate) fn global_app() -> &'static Arc<std::sync::Mutex<AtermApp>> {
    ATERM_APP.get_or_init(|| {
        let app = Arc::new(std::sync::Mutex::new(AtermApp::new()));
        AtermApp::start_ipc(&app);
        app
    })
}

/// C callback table for PlatformHost trait.
#[repr(C)]
pub struct AtermHostCallbacks {
    pub userdata: *mut c_void,
    pub create_workspace_view:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    pub close_workspace_view: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub focus_workspace: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub rename_workspace: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    pub send_key: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    pub attach_external_session: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub reload_settings: Option<unsafe extern "C" fn(*mut c_void)>,
    pub list_workspaces: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_char>,
    pub on_events_available: Option<unsafe extern "C" fn(*mut c_void)>,
    pub request_redraw: Option<unsafe extern "C" fn(*mut c_void)>,
}

unsafe impl Send for AtermHostCallbacks {}
unsafe impl Sync for AtermHostCallbacks {}

/// Adapter: wraps C callbacks into PlatformHost trait
struct HostBridge {
    callbacks: AtermHostCallbacks,
}

unsafe impl Send for HostBridge {}
unsafe impl Sync for HostBridge {}

impl aterm_session::host::PlatformHost for HostBridge {
    fn create_workspace_view(&self, id: &str, config: &aterm_session::types::WorkspaceConfig) {
        if let Some(cb) = self.callbacks.create_workspace_view {
            let id_c = CString::new(id).unwrap_or_default();
            let config_json = serde_json::to_string(config).unwrap_or_default();
            let config_c = CString::new(config_json).unwrap_or_default();
            unsafe { cb(self.callbacks.userdata, id_c.as_ptr(), config_c.as_ptr()) };
        }
    }

    fn close_workspace_view(&self, id: &str) {
        if let Some(cb) = self.callbacks.close_workspace_view {
            let id_c = CString::new(id).unwrap_or_default();
            unsafe { cb(self.callbacks.userdata, id_c.as_ptr()) };
        }
    }

    fn focus_workspace(&self, id: &str) {
        if let Some(cb) = self.callbacks.focus_workspace {
            let id_c = CString::new(id).unwrap_or_default();
            unsafe { cb(self.callbacks.userdata, id_c.as_ptr()) };
        }
    }

    fn rename_workspace(&self, old_name: &str, new_name: &str) {
        if let Some(cb) = self.callbacks.rename_workspace {
            let old_c = CString::new(old_name).unwrap_or_default();
            let new_c = CString::new(new_name).unwrap_or_default();
            unsafe { cb(self.callbacks.userdata, old_c.as_ptr(), new_c.as_ptr()) };
        }
    }

    fn send_key(&self, workspace: &str, key: &str) {
        if let Some(cb) = self.callbacks.send_key {
            let workspace_c = CString::new(workspace).unwrap_or_default();
            let key_c = CString::new(key).unwrap_or_default();
            unsafe {
                cb(
                    self.callbacks.userdata,
                    workspace_c.as_ptr(),
                    key_c.as_ptr(),
                )
            };
        }
    }

    fn attach_external_session(&self, session_id: &str) {
        if let Some(cb) = self.callbacks.attach_external_session {
            let session_c = CString::new(session_id).unwrap_or_default();
            unsafe { cb(self.callbacks.userdata, session_c.as_ptr()) };
        }
    }

    fn reload_settings(&self) {
        if let Some(cb) = self.callbacks.reload_settings {
            unsafe { cb(self.callbacks.userdata) };
        }
    }

    fn list_workspaces(&self) -> Vec<aterm_session::types::WorkspaceInfo> {
        if let Some(cb) = self.callbacks.list_workspaces {
            let ptr = unsafe { cb(self.callbacks.userdata) };
            if !ptr.is_null() {
                let json = unsafe { CStr::from_ptr(ptr).to_string_lossy() };
                let result = serde_json::from_str(&json).unwrap_or_default();
                unsafe { drop(CString::from_raw(ptr)) };
                return result;
            }
        }
        Vec::new()
    }

    fn on_workspace_event(&self, event: aterm_session::types::WorkspaceEvent) {
        // Wakeup+drain: push to global queue, then signal host to drain
        if let Ok(mut queue) = global_event_queue().lock() {
            queue.push(event);
        }
        if let Some(cb) = self.callbacks.on_events_available {
            unsafe { cb(self.callbacks.userdata) };
        }
    }

    fn request_redraw(&self) {
        if let Some(cb) = self.callbacks.request_redraw {
            unsafe { cb(self.callbacks.userdata) };
        }
    }
}

/// Register platform host callbacks. Call once at startup.
#[no_mangle]
pub unsafe extern "C" fn aterm_set_host(callbacks: AtermHostCallbacks) {
    ffi_catch!({
        let host = Box::new(HostBridge { callbacks });
        if let Ok(mut app) = global_app().lock() {
            app.set_host(host);
        }
    });
}

/// Dispatch a SessionAction (JSON) and return a response (JSON).
/// Caller must free the returned string with aterm_core_free_string.
#[no_mangle]
pub unsafe extern "C" fn aterm_dispatch(
    action_json: *const c_char,
    action_len: usize,
) -> *mut c_char {
    if action_json.is_null() {
        return std::ptr::null_mut();
    }
    ffi_catch!(std::ptr::null_mut(), {
        let slice = std::slice::from_raw_parts(action_json as *const u8, action_len);
        let action_str = match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => {
                return ipc_to_c_string(r#"{"status":"Error","message":"invalid utf8"}"#);
            }
        };

        let action = match serde_json::from_str::<aterm_session::action::SessionAction>(action_str)
        {
            Ok(a) => a,
            Err(e) => {
                let resp = format!(r#"{{"status":"Error","message":"parse error: {}"}}"#, e);
                return ipc_to_c_string(&resp);
            }
        };

        let response = match global_app().lock() {
            Ok(mut app) => app.dispatch(action),
            Err(e) => aterm_session::action::ActionResponse::error(e.to_string()),
        };

        let json = serde_json::to_string(&response)
            .unwrap_or_else(|_| r#"{"status":"Error","message":"serialize failed"}"#.to_string());
        ipc_to_c_string(&json)
    })
}

/// Re-register all workspaces with telepty daemon.
/// Call after session restore to ensure all sessions are visible.
#[no_mangle]
pub extern "C" fn aterm_sync_telepty() {
    ffi_catch!({
        if let Ok(app) = global_app().lock() {
            app.sync_telepty_registrations();
        }
    });
}

/// Get the IPC socket path. Caller must free with aterm_core_free_string.
#[no_mangle]
pub extern "C" fn aterm_ipc_socket_path() -> *mut c_char {
    ffi_catch!(std::ptr::null_mut(), {
        let path = global_app()
            .lock()
            .map(|app| app.socket_path().to_string())
            .unwrap_or_default();
        ipc_to_c_string(&path)
    })
}

/// Get the IPC auth token. Caller must free with aterm_core_free_string.
#[no_mangle]
pub extern "C" fn aterm_ipc_token() -> *mut c_char {
    ffi_catch!(std::ptr::null_mut(), {
        let token = global_app()
            .lock()
            .map(|app| app.token().to_string())
            .unwrap_or_default();
        ipc_to_c_string(&token)
    })
}

fn ipc_to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// Session lifecycle FFI — explicit close, batch close, trigger save
// ---------------------------------------------------------------------------

/// Explicit single workspace close — deterministic, not ARC-dependent.
/// Idempotent: double-close is a no-op.
#[no_mangle]
pub unsafe extern "C" fn aterm_workspace_close(workspace_id: *const c_char) {
    if workspace_id.is_null() {
        return;
    }
    ffi_catch!({
        let id = CStr::from_ptr(workspace_id).to_string_lossy().to_string();
        if id.is_empty() {
            return;
        }

        // Close via global app (deregister + notify Swift host)
        if let Ok(mut app) = global_app().lock() {
            // Tell Swift host to close the view
            app.dispatch(aterm_session::action::SessionAction::CloseWorkspace {
                workspace: id.clone(),
            });
        }

        // Trigger debounced save after close
        crate::session::trigger_save();
        log_stderr!("[aterm-ffi] workspace_close: {}", id);
    });
}

/// Batch close multiple workspaces. More efficient than individual close calls.
/// After all workspaces are closed, emits a single WorkspaceBatchClosed event
/// and triggers a debounced save.
#[no_mangle]
pub unsafe extern "C" fn aterm_batch_close(
    workspace_ids: *const *const c_char,
    count: u32,
) {
    if workspace_ids.is_null() || count == 0 {
        return;
    }
    ffi_catch!({
        // Convert C strings to Rust strings
        let ids: Vec<String> = (0..count as usize)
            .filter_map(|i| {
                let ptr = *workspace_ids.add(i);
                if ptr.is_null() {
                    None
                } else {
                    Some(CStr::from_ptr(ptr).to_string_lossy().to_string())
                }
            })
            .filter(|s| !s.is_empty())
            .collect();

        if ids.is_empty() {
            return;
        }

        let closed_ids: Vec<String> = {
            let mut app = match global_app().lock() {
                Ok(app) => app,
                Err(_) => return,
            };

            let mut closed = Vec::with_capacity(ids.len());
            for id in &ids {
                app.dispatch(aterm_session::action::SessionAction::CloseWorkspace {
                    workspace: id.clone(),
                });
                closed.push(id.clone());
            }
            closed
        };

        // Emit batch event via EventBus
        if !closed_ids.is_empty() {
            if let Ok(app) = global_app().lock() {
                app.event_bus().publish(
                    aterm_session::action::AtermEvent::WorkspaceBatchClosed {
                        ids: closed_ids.clone(),
                    },
                );
            }
        }

        // Trigger debounced save after batch
        crate::session::trigger_save();
        log_stderr!(
            "[aterm-ffi] batch_close: {} workspaces closed",
            closed_ids.len()
        );
    });
}

/// Hint Rust to save sessions if debounce allows.
/// Marks the session store as dirty and starts a 500ms debounce timer.
/// Actual save happens after 500ms of quiet (no new triggers).
#[no_mangle]
pub extern "C" fn aterm_trigger_save() {
    ffi_catch!({
        crate::session::trigger_save();
    });
}

/// Update the is_system flag for a workspace in the global session registry.
/// Swift calls this after spawn to mark orchestrator/system workspaces so that
/// SaveCoordinator writes the correct flag to sessions.json.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_workspace_system(
    _core: *mut AtermCore,
    workspace_name: *const c_char,
    is_system: bool,
) {
    if workspace_name.is_null() {
        return;
    }
    ffi_catch!({
        let name = CStr::from_ptr(workspace_name).to_string_lossy().to_string();
        if !name.is_empty() {
            crate::session::set_session_system(&name, is_system);
        }
    });
}

#[cfg(test)]
mod ffi_tests {
    use super::*;

    fn ffi_function_blocks(source: &str) -> Vec<(String, String)> {
        let lines: Vec<&str> = source.lines().collect();
        let mut blocks = Vec::new();
        let mut index = 0usize;

        while index < lines.len() {
            if !lines[index].contains("#[no_mangle]") {
                index += 1;
                continue;
            }

            let mut sig_index = index + 1;
            while sig_index < lines.len() && !lines[sig_index].contains("extern \"C\" fn") {
                sig_index += 1;
            }
            if sig_index >= lines.len() {
                break;
            }

            let signature = lines[sig_index];
            let Some(name_part) = signature.split("fn ").nth(1) else {
                index = sig_index + 1;
                continue;
            };
            let Some(name) = name_part.split('(').next() else {
                index = sig_index + 1;
                continue;
            };

            let mut block = String::new();
            let mut cursor = sig_index;
            while cursor < lines.len()
                && (cursor == sig_index || !lines[cursor].contains("#[no_mangle]"))
            {
                if !block.is_empty() {
                    block.push('\n');
                }
                block.push_str(lines[cursor]);
                cursor += 1;
            }

            blocks.push((name.trim().to_string(), block));
            index = cursor;
        }

        blocks
    }

    #[test]
    fn aterm_core_resize_does_not_panic_on_zero_size() {
        let core = aterm_core_new();

        unsafe {
            aterm_core_resize(core, 0, 0);
            aterm_core_free(core);
        }
    }

    #[test]
    fn aterm_core_resize_does_not_panic_on_very_large_size() {
        let core = aterm_core_new();

        unsafe {
            aterm_core_resize(core, u32::MAX, u32::MAX);
            aterm_core_free(core);
        }
    }

    #[test]
    fn all_ffi_entry_points_wrapped_in_catch_unwind() {
        let source = include_str!("lib.rs");
        let blocks = ffi_function_blocks(source);

        assert!(
            !blocks.is_empty(),
            "expected to discover #[no_mangle] FFI functions"
        );

        let missing: Vec<String> = blocks
            .into_iter()
            .filter(|(_, block)| !block.contains("ffi_catch!") && !block.contains("catch_unwind"))
            .map(|(name, _)| name)
            .collect();

        assert!(
            missing.is_empty(),
            "FFI entry points missing panic guard: {:?}",
            missing
        );
    }
}
