mod core;
mod ime;
mod renderer;
mod terminal;

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::core::pty::{PtyManager, PtyOutputSignal};
#[cfg(target_os = "macos")]
use crate::ime::NativeImeHandler;
use crate::renderer::TerminalGridRenderer;
use crate::terminal::TerminalState;

/// 60fps is sufficient for a terminal.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const IME_REARM_DELAY: Duration = Duration::from_millis(30);

struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuState>,
    renderer: Option<TerminalGridRenderer>,
    terminal: Option<TerminalState>,
    pty_manager: PtyManager,
    pty_signal: PtyOutputSignal,
    workspace_id: Option<String>,

    // Frame pacing (alacritty pattern).
    dirty: bool,
    has_frame: bool,
    next_frame: Option<Instant>,
    window_focused: bool,

    #[cfg(target_os = "macos")]
    native_ime: Option<&'static NativeImeHandler>,
    #[cfg(target_os = "macos")]
    native_ime_active: bool,
    #[cfg(target_os = "macos")]
    ime_marked: Option<String>,
    ime_composing: bool,
    ime_recently_disabled: bool,
    ime_rearm_at: Option<Instant>,
}

impl App {
    fn new() -> Self {
        let pty_manager = PtyManager::new();
        let pty_signal = pty_manager.output_signal();
        Self {
            window: None,
            gpu: None,
            renderer: None,
            terminal: None,
            pty_manager,
            pty_signal,
            workspace_id: None,
            dirty: false,
            has_frame: true,
            next_frame: None,
            window_focused: false,
            #[cfg(target_os = "macos")]
            native_ime: None,
            #[cfg(target_os = "macos")]
            native_ime_active: false,
            #[cfg(target_os = "macos")]
            ime_marked: None,
            ime_composing: false,
            ime_recently_disabled: false,
            ime_rearm_at: None,
        }
    }

    fn init_gpu(&mut self) {
        let window = self.window.as_ref().unwrap().clone();
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window).unwrap();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("No suitable GPU adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("aterm"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .expect("Failed to create device");

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = TerminalGridRenderer::new(&device, &queue, format);
        let (cols, rows) = renderer.grid_size(size.width as f32, size.height as f32);
        let terminal = TerminalState::new(cols as usize, rows as usize);

        self.renderer = Some(renderer);
        self.terminal = Some(terminal);
        self.gpu = Some(GpuState {
            surface,
            device,
            queue,
            config,
        });
    }

    fn spawn_default_shell(&mut self) {
        let Some(ref renderer) = self.renderer else { return };
        let Some(ref gpu) = self.gpu else { return };

        let (cols, rows) = renderer.grid_size(gpu.config.width as f32, gpu.config.height as f32);
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default())
            .to_string_lossy()
            .to_string();

        match self.pty_manager.create(
            "main".to_string(),
            cwd,
            None,
            None,
            Some(cols),
            Some(rows),
            false,
        ) {
            Ok(id) => {
                eprintln!("[aterm] shell spawned: {id}");
                self.workspace_id = Some(id);
            }
            Err(e) => eprintln!("[aterm] failed to spawn shell: {e}"),
        }
    }

    fn write_to_pty(&self, text: &str) {
        if let Some(ref id) = self.workspace_id {
            if let Err(e) = self.pty_manager.send_to_workspace(id, text) {
                eprintln!("[aterm] PTY write error: {e}");
            }
        }
    }

    fn sync_pty_output(&mut self) {
        let Some(ref id) = self.workspace_id else { return };
        let Some(ref mut terminal) = self.terminal else { return };

        match self.pty_manager.drain_term_bytes(id) {
            Ok(bytes) if !bytes.is_empty() => terminal.advance(&bytes),
            Err(e) => eprintln!("[aterm] drain_term_bytes error: {e}"),
            _ => {}
        }
    }

    fn handle_named_key(&mut self, key: &Key) -> bool {
        let bytes: &str = match key {
            Key::Named(NamedKey::Enter) => "\r",
            Key::Named(NamedKey::Backspace) => "\x7f",
            Key::Named(NamedKey::Delete) => "\x1b[3~",
            Key::Named(NamedKey::Tab) => "\t",
            Key::Named(NamedKey::Escape) => "\x1b",
            Key::Named(NamedKey::ArrowUp) => "\x1b[A",
            Key::Named(NamedKey::ArrowDown) => "\x1b[B",
            Key::Named(NamedKey::ArrowRight) => "\x1b[C",
            Key::Named(NamedKey::ArrowLeft) => "\x1b[D",
            Key::Named(NamedKey::Home) => "\x1b[H",
            Key::Named(NamedKey::End) => "\x1b[F",
            Key::Named(NamedKey::PageUp) => "\x1b[5~",
            Key::Named(NamedKey::PageDown) => "\x1b[6~",
            _ => return false,
        };
        self.write_to_pty(bytes);
        true
    }

    /// Mark terminal content as changed. Request redraw if frame budget available.
    fn mark_dirty(&mut self) {
        self.dirty = true;
        if self.has_frame {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    /// Called at the end of draw. Consumes the frame budget and schedules next frame.
    fn request_frame(&mut self) {
        self.has_frame = false;
        self.next_frame = Some(Instant::now() + FRAME_INTERVAL);
    }

    fn draw(&mut self) {
        self.dirty = false;
        self.has_frame = false;
        self.sync_pty_output();
        let draw_start = Instant::now();

        let (gpu, renderer, terminal) =
            match (&self.gpu, &mut self.renderer, &self.terminal) {
                (Some(g), Some(r), Some(t)) => (g, r, t),
                _ => return,
            };

        let output = match gpu.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                self.has_frame = true;
                return;
            }
            Err(e) => {
                eprintln!("[wgpu] surface error: {e}");
                self.has_frame = true;
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let shared_term = terminal.terminal();
        if let Ok(term) = shared_term.lock() {
            renderer.render(
                &term,
                &gpu.device,
                &gpu.queue,
                &view,
                gpu.config.width,
                gpu.config.height,
            );
        }

        output.present();
        let draw_elapsed = draw_start.elapsed();
        if draw_elapsed.as_millis() > 16 {
            eprintln!("[perf] draw: {}ms", draw_elapsed.as_millis());
        }
        self.has_frame = true;
    }

    fn set_winit_ime_allowed(&self, allowed: bool) {
        if let Some(window) = &self.window {
            window.set_ime_allowed(allowed);
            eprintln!("[ime] set_ime_allowed({allowed})");
        }
    }

    fn process_ime_recovery(&mut self) {
        let now = Instant::now();

        if let Some(rearm_at) = self.ime_rearm_at {
            if now >= rearm_at && self.window_focused {
                self.ime_rearm_at = None;
                self.ime_recently_disabled = false;
                self.set_winit_ime_allowed(false);
                self.set_winit_ime_allowed(true);
                eprintln!("[ime/recovery] rearmed after Disabled→Enabled");
            }
        }
    }

    fn clear_ime_preedit_state(&mut self) {
        self.ime_composing = false;
        #[cfg(target_os = "macos")]
        {
            self.ime_marked = None;
            self.refresh_window_title();
        }
    }

    fn refresh_window_title(&self) {
        let mut title = String::from("aterm v3");
        #[cfg(target_os = "macos")]
        if let Some(marked) = self.ime_marked.as_ref().filter(|marked| !marked.is_empty()) {
            title.push_str(" [");
            title.push_str(marked);
            title.push(']');
        }

        if let Some(window) = &self.window {
            window.set_title(&title);
        }
    }

    #[cfg(target_os = "macos")]
    fn activate_native_ime(&mut self) {
        let Some(window) = self.window.as_ref() else { return };
        let handler = *self
            .native_ime
            .get_or_insert_with(NativeImeHandler::initialize);

        let attached = handler.attach_to_window(window.as_ref());
        let active = attached && handler.activate_for_window(window.as_ref());
        self.native_ime_active = active;
        eprintln!("[ime] activate_native_ime: active={}", active);
    }

    #[cfg(not(target_os = "macos"))]
    fn activate_native_ime(&mut self) {}

    #[cfg(target_os = "macos")]
    fn deactivate_native_ime(&mut self) {
        let Some(window) = self.window.as_ref() else { return };
        if let Some(handler) = self.native_ime {
            handler.deactivate_for_window(window.as_ref());
            self.native_ime_active = false;
            self.ime_marked = None;
            self.refresh_window_title();
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn deactivate_native_ime(&mut self) {}

    #[cfg(target_os = "macos")]
    fn sync_native_ime(&mut self) {
        if !self.native_ime_active {
            return;
        }

        let Some(handler) = self.native_ime else { return };
        let mut wrote_input = false;

        for bytes in handler.drain_key_bytes() {
            let text = String::from_utf8_lossy(&bytes);
            if !text.is_empty() {
                self.write_to_pty(text.as_ref());
                wrote_input = true;
            }
        }

        for text in handler.drain_committed() {
            if !text.is_empty() {
                eprintln!("[ime] sync drain_committed: {:?}", text);
                self.write_to_pty(&text);
                wrote_input = true;
            }
        }

        let marked = handler.marked_text();
        if marked != self.ime_marked {
            self.ime_marked = marked;
            self.refresh_window_title();
            wrote_input = true;
        }

        if wrote_input {
            self.mark_dirty();
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn sync_native_ime(&mut self) {}
}

impl ApplicationHandler for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // PTY thread sent Wakeup via EventLoopProxy (alacritty pattern).
        self.mark_dirty();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.process_ime_recovery();
        // Native IME disabled — no sync needed.
        // self.sync_native_ime();
        // Poll PTY dirty flag every tick (EventLoopProxy unreliable on macOS).
        if self.pty_signal.take_dirty() {
            self.dirty = true;
        }
        if self.dirty && self.has_frame {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        // Wake every 8ms (~120fps). CPU cost: ~1-2%.
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + FRAME_INTERVAL,
        ));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = WindowAttributes::default()
                .with_title("aterm v3")
                .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));
            match event_loop.create_window(attrs) {
                Ok(w) => {
                    self.window = Some(Arc::new(w));
                    self.set_winit_ime_allowed(true);
                    self.refresh_window_title();
                    self.init_gpu();
                    self.spawn_default_shell();
                    // Native IME disabled — using winit's built-in Ime events.
                    // self.activate_native_ime();
                    #[cfg(target_os = "macos")]
                    {
                        use objc2_app_kit::NSApplication;
                        use objc2_foundation::MainThreadMarker;
                        if let Some(mtm) = MainThreadMarker::new() {
                            let ns_app = NSApplication::sharedApplication(mtm);
                            #[allow(deprecated)]
                            ns_app.activateIgnoringOtherApps(true);
                        }
                    }
                    if let Some(window) = &self.window {
                        window.focus_window();
                    }
                    self.set_winit_ime_allowed(true);
                }
                Err(e) => eprintln!("[aterm] failed to create window: {e}"),
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(ref id) = self.workspace_id {
                    let _ = self.pty_manager.close(id);
                }
                event_loop.exit();
            }

            WindowEvent::Resized(new_size) => {
                if new_size.width == 0 || new_size.height == 0 {
                    return;
                }
                if let Some(ref mut gpu) = self.gpu {
                    gpu.config.width = new_size.width;
                    gpu.config.height = new_size.height;
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
                if let Some(ref renderer) = self.renderer {
                    let (cols, rows) =
                        renderer.grid_size(new_size.width as f32, new_size.height as f32);
                    if let Some(ref mut terminal) = self.terminal {
                        terminal.resize(cols as usize, rows as usize);
                    }
                    if let Some(ref id) = self.workspace_id {
                        let _ = self.pty_manager.resize(id, cols, rows);
                    }
                }
                self.mark_dirty();
            }

            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                eprintln!("[ime] Window focused={focused}");
                if focused {
                    self.set_winit_ime_allowed(true);
                } else {
                    self.ime_recently_disabled = false;
                    self.ime_rearm_at = None;
                    self.clear_ime_preedit_state();
                }
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => {
                if !self.handle_named_key(&key_event.logical_key) {
                    if let Some(ref text) = key_event.text {
                        let s = text.as_str();
                        if !s.is_empty() && !self.ime_composing {
                            self.ime_recently_disabled = false;
                            self.write_to_pty(s);
                        }
                    }
                }
                self.mark_dirty();
            }

            WindowEvent::Ime(Ime::Enabled) => {
                eprintln!("[ime] Enabled");
                if self.ime_recently_disabled {
                    self.ime_rearm_at = Some(Instant::now() + IME_REARM_DELAY);
                    eprintln!("[ime/recovery] scheduled after Disabled→Enabled");
                }
            }

            WindowEvent::Ime(Ime::Preedit(ref text, _cursor)) => {
                self.ime_composing = !text.is_empty();
                eprintln!("[ime] Preedit: composing={} text={:?}", self.ime_composing, text);
                #[cfg(target_os = "macos")]
                {
                    self.ime_marked = if text.is_empty() { None } else { Some(text.clone()) };
                    self.refresh_window_title();
                }
                self.mark_dirty();
            }

            WindowEvent::Ime(Ime::Commit(ref text)) => {
                self.clear_ime_preedit_state();
                self.ime_recently_disabled = false;
                self.write_to_pty(text);
                eprintln!("[ime] Commit: {:?}", text);
                self.mark_dirty();
            }

            WindowEvent::Ime(Ime::Disabled) => {
                self.clear_ime_preedit_state();
                self.ime_recently_disabled = self.window_focused;
                self.ime_rearm_at = None;
                eprintln!("[ime] Disabled");
            }

            WindowEvent::RedrawRequested => {
                self.draw();
            }

            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();

    // Push-driven wakeup: PTY reader thread calls mark_dirty() which invokes
    // this callback, posting an NSEvent to wake the macOS run loop immediately.
    let proxy = event_loop.create_proxy();
    app.pty_signal.set_wake_callback(move || {
        let _ = proxy.send_event(());
    });

    event_loop.run_app(&mut app).unwrap();
}
