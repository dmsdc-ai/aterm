/// Safe stderr logging — never panics, even if stderr is unavailable.
/// macOS .app bundles launched from Finder may not have stderr attached,
/// causing `log_stderr!` to panic. This macro silently discards write errors.
macro_rules! log_stderr {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), $($arg)*);
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
pub mod cli_presets;
pub mod inject;
pub mod pty;
pub mod renderer;
pub mod renderer_atlas;
pub mod renderer_glyph;
pub mod session;
pub mod tailscale;
pub mod telepty;
pub mod telepty_bridge;
pub mod terminal;

use std::ffi::{c_char, c_void, CStr, CString};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::pty::{PtyManager, PtyOutputSignal};
use crate::renderer::{ColorScheme, TerminalGridRenderer, TerminalThemeMode};
use crate::terminal::TerminalState;

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

// -- Global event queue (wakeup+drain pattern) --
static EVENT_QUEUE: OnceLock<Mutex<Vec<aterm_session::types::WorkspaceEvent>>> = OnceLock::new();

fn global_event_queue() -> &'static Mutex<Vec<aterm_session::types::WorkspaceEvent>> {
    EVENT_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
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
    gpu: Option<GpuState>,
    renderer: Option<TerminalGridRenderer>,
    terminal: Option<TerminalState>,
    pty_manager: PtyManager,
    pty_signal: PtyOutputSignal,
    workspace_id: Option<String>,
    dirty: Arc<AtomicBool>,
    dirty_callback: Option<DirtyCallback>,
    dirty_userdata: *mut c_void,
    theme_mode: TerminalThemeMode,
    /// Set when surface.configure() panics — next render retries.
    needs_reconfigure: AtomicBool,
    /// Deferred surface reconfigure (Fix 3/5): pending pixel dimensions.
    /// Actual configure happens lazily at render time, coalescing multiple
    /// resize events per frame into a single GPU reconfigure.
    pending_surface_width: AtomicU32,
    pending_surface_height: AtomicU32,
    /// PTY SIGWINCH coalescing (Fix 6): last grid cols/rows sent to PTY.
    /// Skips redundant ioctl(TIOCSWINSZ) when pixel size changes but
    /// grid dimensions stay the same.
    last_pty_cols: u16,
    last_pty_rows: u16,
    /// Geometry revision counter (Fix 7): cheap monotonic counter for
    /// stale-frame detection without expensive dimension comparison.
    geometry_revision: AtomicU64,
    /// Render lock (Fix #153): prevents concurrent render() calls from
    /// CVDisplayLink thread and PTY dirty-callback thread. try_render()
    /// skips if locked; direct render() spins briefly then skips.
    render_lock: AtomicBool,
}

// SAFETY: The raw pointer dirty_userdata is only used from the main thread callback
unsafe impl Send for AtermCore {}

impl AtermCore {
    fn new() -> Self {
        let pty_manager = PtyManager::new();
        let pty_signal = pty_manager.output_signal();
        Self {
            gpu: None,
            renderer: None,
            terminal: None,
            pty_manager,
            pty_signal,
            workspace_id: None,
            dirty: Arc::new(AtomicBool::new(false)),
            dirty_callback: None,
            dirty_userdata: std::ptr::null_mut(),
            theme_mode: TerminalThemeMode::Dark,
            needs_reconfigure: AtomicBool::new(false),
            pending_surface_width: AtomicU32::new(0),
            pending_surface_height: AtomicU32::new(0),
            last_pty_cols: 0,
            last_pty_rows: 0,
            geometry_revision: AtomicU64::new(0),
            render_lock: AtomicBool::new(false),
        }
    }

    fn init_gpu(&mut self, ns_view: *mut c_void, width: u32, height: u32, scale: f32) -> i32 {
        if ns_view.is_null() {
            return -1;
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });

        // Create surface from raw NSView pointer using raw-window-handle
        let surface = unsafe {
            use raw_window_handle::{
                AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
            };
            let window_handle = AppKitWindowHandle::new(std::ptr::NonNull::new(ns_view).unwrap());
            let display_handle = AppKitDisplayHandle::new();
            let raw_window = RawWindowHandle::AppKit(window_handle);
            let raw_display = RawDisplayHandle::AppKit(display_handle);
            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: raw_display,
                raw_window_handle: raw_window,
            };
            match instance.create_surface_unsafe(target) {
                Ok(s) => s,
                Err(e) => {
                    log_stderr!("[aterm-core] surface creation failed: {e}");
                    return -2;
                }
            }
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }));
        let adapter = match adapter {
            Some(a) => a,
            None => {
                log_stderr!("[aterm-core] no suitable GPU adapter");
                return -3;
            }
        };

        let (device, queue) = match pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("aterm-core"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        )) {
            Ok(dq) => dq,
            Err(e) => {
                log_stderr!("[aterm-core] device creation failed: {e}");
                return -4;
            }
        };

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer =
            TerminalGridRenderer::new(&device, &queue, format, self.theme_mode, scale.max(1.0));
        let (cols, rows) = renderer.grid_size(width as f32, height as f32);
        let terminal = TerminalState::new(cols as usize, rows as usize);

        self.renderer = Some(renderer);
        self.terminal = Some(terminal);
        self.gpu = Some(GpuState {
            surface,
            device,
            queue,
            config,
        });

        // Sync pending dimensions so resize() dedup works from the start
        self.pending_surface_width.store(width.max(1), Ordering::Relaxed);
        self.pending_surface_height.store(height.max(1), Ordering::Relaxed);

        0 // success
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

    fn sync_pty(&mut self) {
        let Some(ref id) = self.workspace_id else {
            return;
        };
        let Some(ref mut terminal) = self.terminal else {
            return;
        };

        match self.pty_manager.drain_term_bytes(id) {
            Ok(bytes) if !bytes.is_empty() => terminal.advance(&bytes),
            Err(e) => log_stderr!("[aterm-core] drain error: {e}"),
            _ => {}
        }
    }

    fn render(&mut self) {
        // Always drain PTY — doesn't touch GPU, prevents child process blocking
        self.sync_pty();

        // Deferred surface reconfigure (Fix 3/5): apply pending size change.
        // Coalesces multiple resize() calls between frames into a single
        // GPU surface reconfigure, matching Ghostty/cmux patterns.
        let pending_w = self.pending_surface_width.load(Ordering::Acquire);
        let pending_h = self.pending_surface_height.load(Ordering::Acquire);
        if pending_w > 0 && pending_h > 0 {
            if let Some(ref mut gpu) = self.gpu {
                if gpu.config.width != pending_w || gpu.config.height != pending_h {
                    gpu.config.width = pending_w;
                    gpu.config.height = pending_h;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        gpu.surface.configure(&gpu.device, &gpu.config);
                    }));
                    if result.is_err() {
                        log_stderr!("[aterm-core] surface.configure panicked during deferred resize — will retry");
                        self.needs_reconfigure.store(true, Ordering::SeqCst);
                    }
                }
            }
        }

        // Retry configure if a previous attempt panicked (recovery path)
        if self.needs_reconfigure.load(Ordering::SeqCst) {
            if let Some(ref gpu) = self.gpu {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }));
                if result.is_err() {
                    // Still broken — skip frame, retry next frame (no infinite loop)
                    return;
                }
                self.needs_reconfigure.store(false, Ordering::SeqCst);
                log_stderr!("[aterm-core] surface reconfigured successfully after panic");
            }
        }

        let (gpu, renderer, terminal) = match (&self.gpu, &mut self.renderer, &self.terminal) {
            (Some(g), Some(r), Some(t)) => (g, r, t),
            _ => return,
        };

        let output = match gpu.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // Wrap reconfigure in catch_unwind — may race with resize
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }));
                if result.is_err() {
                    self.needs_reconfigure.store(true, Ordering::SeqCst);
                }
                return;
            }
            Err(e) => {
                log_stderr!("[aterm-core] surface error: {e}");
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let shared = terminal.terminal();
        if let Ok(mut term) = shared.lock() {
            renderer.render(
                &mut term,
                &gpu.device,
                &gpu.queue,
                &view,
                gpu.config.width,
                gpu.config.height,
            );
        }

        output.present();
    }

    fn set_theme_mode(&mut self, theme_mode: TerminalThemeMode) {
        self.theme_mode = theme_mode;
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_theme_mode(theme_mode);
        }
    }

    fn set_color_scheme(&mut self, scheme: ColorScheme) {
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_color_scheme(scheme);
        }
    }

    fn set_font_size(&mut self, size: f32) {
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_font_size(size);
        }
    }

    fn set_line_height(&mut self, height: f32) {
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_line_height(height);
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        // Pixel-level dedup against pending dimensions
        let prev_w = self.pending_surface_width.load(Ordering::Relaxed);
        let prev_h = self.pending_surface_height.load(Ordering::Relaxed);
        if prev_w == width && prev_h == height {
            return;
        }

        // Store pending dimensions — surface.configure() is deferred to render()
        // (Fix 3/5). This coalesces multiple resize events per frame into a single
        // GPU reconfigure, eliminating the main bottleneck during live window drag.
        self.pending_surface_width.store(width, Ordering::Release);
        self.pending_surface_height.store(height, Ordering::Release);

        // Geometry revision counter (Fix 7)
        self.geometry_revision.fetch_add(1, Ordering::Relaxed);

        if let Some(ref renderer) = self.renderer {
            let (cols, rows) = renderer.grid_size(width as f32, height as f32);
            if let Some(ref mut terminal) = self.terminal {
                terminal.resize(cols as usize, rows as usize);
            }
            // PTY SIGWINCH coalescing (Fix 6): only send ioctl(TIOCSWINSZ)
            // when grid dimensions actually change. During a drag, many pixel-
            // level resizes map to the same cols/rows — no need to flood the
            // child process with redundant SIGWINCH.
            if cols != self.last_pty_cols || rows != self.last_pty_rows {
                self.last_pty_cols = cols;
                self.last_pty_rows = rows;
                if let Some(ref id) = self.workspace_id {
                    let _ = self.pty_manager.resize(id, cols, rows);
                }
            }
        }
        // Mark dirty so CVDisplayLink renders with new dimensions
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
            let point = Point::new(Line(line), Column(col));
            let sel = Selection::new(SelectionType::Simple, point, s);
            if let Ok(mut term) = terminal.terminal().lock() {
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
            let point = Point::new(Line(line), Column(col));
            if let Ok(mut term) = terminal.terminal().lock() {
                if let Some(ref mut sel) = term.selection {
                    sel.update(point, s);
                }
            }
        }
    }

    fn selection_clear(&mut self) {
        if let Some(ref mut terminal) = self.terminal {
            if let Ok(mut term) = terminal.terminal().lock() {
                term.selection = None;
            }
        }
    }

    fn selection_text(&self) -> Option<String> {
        if let Some(ref terminal) = self.terminal {
            if let Ok(term) = terminal.terminal().lock() {
                return term.selection_to_string();
            }
        }
        None
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

/// Stop the PTY output signal callback. Must be called BEFORE aterm_core_free
/// to prevent use-after-free when the host view is deallocated.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_stop(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).pty_signal.stop());
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_init_gpu(
    core: *mut AtermCore,
    ns_view: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    if core.is_null() {
        return -1;
    }
    ffi_catch!(-1, (*core).init_gpu(ns_view, width, height, scale))
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
#[no_mangle]
pub unsafe extern "C" fn aterm_core_render(core: *mut AtermCore) {
    if core.is_null() {
        return;
    }
    // Acquire render lock — spin briefly for direct UI calls that need immediate feedback.
    // If another thread (CVDisplayLink or PTY callback) is rendering, wait up to 8ms.
    let start = std::time::Instant::now();
    loop {
        if (*core)
            .render_lock
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
        if start.elapsed() > std::time::Duration::from_millis(8) {
            return; // Skip — another thread is rendering, CVDisplayLink will catch up
        }
        std::hint::spin_loop();
    }
    ffi_catch!((*core).render());
    (*core).render_lock.store(false, Ordering::Release);
}

/// Try to render if dirty. Returns 1 if rendered, 0 if skipped.
/// Thread-safe — used by CVDisplayLink and PTY dirty callback for immediate
/// render without CVDisplayLink latency (Ghostty/Alacritty pattern, Fix #153).
#[no_mangle]
pub unsafe extern "C" fn aterm_core_try_render(core: *mut AtermCore) -> i32 {
    if core.is_null() {
        return 0;
    }
    // Try to acquire render lock — skip immediately if another thread is rendering
    if (*core)
        .render_lock
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return 0;
    }
    let rendered = if (*core).pty_signal.take_dirty() {
        ffi_catch!((*core).render());
        1
    } else {
        0
    };
    (*core).render_lock.store(false, Ordering::Release);
    rendered
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_resize(core: *mut AtermCore, width: u32, height: u32) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).resize(width, height));
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_grid_size(
    core: *const AtermCore,
    width: f32,
    height: f32,
    out_cols: *mut u16,
    out_rows: *mut u16,
) {
    if core.is_null() {
        return;
    }
    ffi_catch!({
        if let Some(ref renderer) = (*core).renderer {
            let (cols, rows) = renderer.grid_size(width, height);
            if !out_cols.is_null() {
                *out_cols = cols;
            }
            if !out_rows.is_null() {
                *out_rows = rows;
            }
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_cell_size(
    core: *const AtermCore,
    out_width: *mut f32,
    out_height: *mut f32,
) {
    if core.is_null() {
        return;
    }
    ffi_catch!({
        if let Some(ref renderer) = (*core).renderer {
            if !out_width.is_null() {
                *out_width = renderer.cell_width();
            }
            if !out_height.is_null() {
                *out_height = renderer.cell_height();
            }
        }
    });
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
    ffi_catch!((*core).sync_pty());
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

#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_theme_mode(core: *mut AtermCore, mode: u8) {
    if core.is_null() {
        return;
    }
    let theme_mode = if mode == 1 {
        TerminalThemeMode::Light
    } else {
        TerminalThemeMode::Dark
    };
    ffi_catch!((*core).set_theme_mode(theme_mode));
}

/// Set color scheme: 0=Dark, 1=Light, 2=SolarizedDark, 3=SolarizedLight,
/// 4=Monokai, 5=Dracula, 6=Nord, 7=TokyoNight
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_color_scheme(core: *mut AtermCore, scheme: u8) {
    if core.is_null() {
        return;
    }
    let s = match scheme {
        0 => ColorScheme::Dark,
        1 => ColorScheme::Light,
        2 => ColorScheme::SolarizedDark,
        3 => ColorScheme::SolarizedLight,
        4 => ColorScheme::Monokai,
        5 => ColorScheme::Dracula,
        6 => ColorScheme::Nord,
        7 => ColorScheme::TokyoNight,
        8 => ColorScheme::Default,
        _ => return,
    };
    ffi_catch!((*core).set_color_scheme(s));
}

/// Set font size in pixels (clamped to 8..32)
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_font_size(core: *mut AtermCore, size: f32) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).set_font_size(size));
}

/// Set line height in pixels (clamped to 12..64)
#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_line_height(core: *mut AtermCore, height: f32) {
    if core.is_null() {
        return;
    }
    ffi_catch!((*core).set_line_height(height));
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
    dirs::home_dir()
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
