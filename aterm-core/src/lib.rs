pub mod renderer;
pub mod terminal;
pub mod pty;
pub mod inject;
pub mod session;
pub mod telepty;
pub mod cli_presets;

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::pty::{PtyManager, PtyOutputSignal};
use crate::renderer::TerminalGridRenderer;
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

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

/// Callback type for dirty notifications
type DirtyCallback = unsafe extern "C" fn(*mut c_void);

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
            let window_handle = AppKitWindowHandle::new(
                std::ptr::NonNull::new(ns_view).unwrap(),
            );
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
                    eprintln!("[aterm-core] surface creation failed: {e}");
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
                eprintln!("[aterm-core] no suitable GPU adapter");
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
                eprintln!("[aterm-core] device creation failed: {e}");
                return -4;
            }
        };

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
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

        let renderer = TerminalGridRenderer::new(&device, &queue, format, scale.max(1.0));
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

        0 // success
    }

    fn spawn_shell(&mut self, cwd: &str, cols: u16, rows: u16) -> i32 {
        match self.pty_manager.create(
            "main".to_string(),
            cwd.to_string(),
            None,
            None,
            Some(cols),
            Some(rows),
            false,
        ) {
            Ok(id) => {
                eprintln!("[aterm-core] shell spawned: {id}");
                self.workspace_id = Some(id);
                0
            }
            Err(e) => {
                eprintln!("[aterm-core] shell spawn failed: {e}");
                -1
            }
        }
    }

    fn write_pty(&self, text: &str) {
        if let Some(ref id) = self.workspace_id {
            if let Err(e) = self.pty_manager.send_to_workspace(id, text) {
                eprintln!("[aterm-core] pty write error: {e}");
            }
        }
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
        let Some(ref id) = self.workspace_id else { return };
        let Some(ref mut terminal) = self.terminal else { return };

        match self.pty_manager.drain_term_bytes(id) {
            Ok(bytes) if !bytes.is_empty() => terminal.advance(&bytes),
            Err(e) => eprintln!("[aterm-core] drain error: {e}"),
            _ => {}
        }
    }

    fn render(&mut self) {
        self.sync_pty();

        let (gpu, renderer, terminal) = match (&self.gpu, &mut self.renderer, &self.terminal) {
            (Some(g), Some(r), Some(t)) => (g, r, t),
            _ => return,
        };

        let output = match gpu.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
            Err(e) => {
                eprintln!("[aterm-core] surface error: {e}");
                return;
            }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shared = terminal.terminal();
        if let Ok(term) = shared.lock() {
            renderer.render(&term, &gpu.device, &gpu.queue, &view, gpu.config.width, gpu.config.height);
        }

        output.present();
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if let Some(ref mut gpu) = self.gpu {
            gpu.config.width = width;
            gpu.config.height = height;
            gpu.surface.configure(&gpu.device, &gpu.config);
        }
        if let Some(ref renderer) = self.renderer {
            let (cols, rows) = renderer.grid_size(width as f32, height as f32);
            if let Some(ref mut terminal) = self.terminal {
                terminal.resize(cols as usize, rows as usize);
            }
            if let Some(ref id) = self.workspace_id {
                let _ = self.pty_manager.resize(id, cols, rows);
            }
        }
    }
}

// -- C-FFI Functions --

#[no_mangle]
pub extern "C" fn aterm_core_new() -> *mut AtermCore {
    Box::into_raw(Box::new(AtermCore::new()))
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_free(core: *mut AtermCore) {
    if !core.is_null() {
        drop(Box::from_raw(core));
    }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_init_gpu(
    core: *mut AtermCore,
    ns_view: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    if core.is_null() { return -1; }
    (*core).init_gpu(ns_view, width, height, scale)
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_spawn_shell(
    core: *mut AtermCore,
    cwd: *const c_char,
    cols: u16,
    rows: u16,
) -> i32 {
    if core.is_null() || cwd.is_null() { return -1; }
    let cwd_str = CStr::from_ptr(cwd).to_string_lossy();
    (*core).spawn_shell(&cwd_str, cols, rows)
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_write_pty(
    core: *mut AtermCore,
    text: *const c_char,
    len: usize,
) {
    if core.is_null() || text.is_null() { return; }
    let slice = std::slice::from_raw_parts(text as *const u8, len);
    if let Ok(s) = std::str::from_utf8(slice) {
        (*core).write_pty(s);
    }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_named_key(core: *mut AtermCore, key_code: u32) {
    if core.is_null() { return; }
    (*core).named_key(key_code);
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_render(core: *mut AtermCore) {
    if core.is_null() { return; }
    (*core).render();
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_resize(core: *mut AtermCore, width: u32, height: u32) {
    if core.is_null() { return; }
    (*core).resize(width, height);
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_grid_size(
    core: *const AtermCore,
    width: f32,
    height: f32,
    out_cols: *mut u16,
    out_rows: *mut u16,
) {
    if core.is_null() { return; }
    if let Some(ref renderer) = (*core).renderer {
        let (cols, rows) = renderer.grid_size(width, height);
        if !out_cols.is_null() { *out_cols = cols; }
        if !out_rows.is_null() { *out_rows = rows; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_take_dirty(core: *mut AtermCore) -> i32 {
    if core.is_null() { return 0; }
    (*core).pty_signal.take_dirty() as i32
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_set_dirty_callback(
    core: *mut AtermCore,
    callback: Option<unsafe extern "C" fn(*mut c_void)>,
    userdata: *mut c_void,
) {
    if core.is_null() { return; }
    let c = &mut *core;
    c.dirty_callback = callback;
    c.dirty_userdata = userdata;

    if let Some(cb) = callback {
        let ud = userdata as usize; // Convert to usize for Send
        c.pty_signal.set_wake_callback(move || {
            unsafe { cb(ud as *mut c_void); }
        });
    }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_sync_pty(core: *mut AtermCore) {
    if core.is_null() { return; }
    (*core).sync_pty();
}

/// Returns JSON string of internal workspaces. Caller must free with aterm_core_free_string.
#[no_mangle]
pub unsafe extern "C" fn aterm_core_list_workspaces(core: *const AtermCore) -> *mut c_char {
    if core.is_null() {
        return std::ptr::null_mut();
    }
    let workspaces = (*core).pty_manager.list_workspaces();
    let json = serde_json::to_string(&workspaces).unwrap_or_else(|_| "[]".to_string());
    match std::ffi::CString::new(json) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn aterm_core_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(std::ffi::CString::from_raw(ptr));
    }
}
