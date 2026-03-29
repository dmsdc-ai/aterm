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
const IME_RECOVERY_WINDOW: Duration = Duration::from_millis(500);

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
    ime_recovery_until: Option<Instant>,
    ime_recovery_buffer: String,
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
            ime_recovery_until: None,
            ime_recovery_buffer: String::new(),
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
        self.flush_ime_recovery_buffer(false);
        self.ime_recovery_until = None;
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
                self.set_winit_ime_allowed(false);
                self.set_winit_ime_allowed(true);
                eprintln!("[ime/recovery] rearmed after Disabled→Enabled");
            }
        }

        if let Some(until) = self.ime_recovery_until {
            if now >= until && !self.ime_composing {
                self.flush_ime_recovery_buffer(false);
                self.ime_recovery_until = None;
                self.ime_recently_disabled = false;
            }
        }
    }

    fn buffer_ime_recovery_text(&mut self, text: &str, source: &str) -> bool {
        if self.ime_recovery_until.is_none() || !is_all_hangul_jamo(text) {
            return false;
        }

        self.ime_recovery_buffer.push_str(text);
        self.ime_recovery_until = Some(Instant::now() + IME_RECOVERY_WINDOW);
        eprintln!("[ime/recovery] buffered {source}: {:?}", text);
        true
    }

    fn flush_ime_recovery_buffer(&mut self, drop_partial: bool) {
        if self.ime_recovery_buffer.is_empty() {
            return;
        }

        let pending = std::mem::take(&mut self.ime_recovery_buffer);
        let recovered = compose_hangul_jamo_sequence(&pending);
        let has_syllable = recovered.chars().any(is_hangul_syllable);

        if has_syllable || !drop_partial {
            if !recovered.is_empty() {
                eprintln!("[ime/recovery] flush {:?} -> {:?}", pending, recovered);
                self.write_to_pty(&recovered);
            }
        } else {
            eprintln!("[ime/recovery] drop partial {:?}", pending);
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
                    self.flush_ime_recovery_buffer(false);
                    self.ime_recently_disabled = false;
                    self.ime_rearm_at = None;
                    self.ime_recovery_until = None;
                    self.ime_composing = false;
                    #[cfg(target_os = "macos")]
                    {
                        self.ime_marked = None;
                        self.refresh_window_title();
                    }
                }
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => {
                if !self.handle_named_key(&key_event.logical_key) {
                    if let Some(ref text) = key_event.text {
                        let s = text.as_str();
                        if !s.is_empty() && !self.ime_composing {
                            if self.buffer_ime_recovery_text(s, "keyboard") {
                                self.mark_dirty();
                                return;
                            }
                            self.flush_ime_recovery_buffer(false);
                            self.ime_recovery_until = None;
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
                    self.ime_recovery_until = Some(Instant::now() + IME_RECOVERY_WINDOW);
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
                self.ime_composing = false;
                #[cfg(target_os = "macos")]
                {
                    self.ime_marked = None;
                    self.refresh_window_title();
                }
                if self.buffer_ime_recovery_text(text, "commit") {
                    self.mark_dirty();
                    return;
                }
                let drop_partial = text.chars().any(is_hangul_syllable);
                self.flush_ime_recovery_buffer(drop_partial);
                self.ime_recovery_until = None;
                self.ime_recently_disabled = false;
                self.write_to_pty(text);
                eprintln!("[ime] Commit: {:?}", text);
                self.mark_dirty();
            }

            WindowEvent::Ime(Ime::Disabled) => {
                self.ime_composing = false;
                #[cfg(target_os = "macos")]
                {
                    self.ime_marked = None;
                    self.refresh_window_title();
                }
                self.ime_recently_disabled = self.window_focused;
                self.ime_rearm_at = None;
                self.ime_recovery_until = Some(Instant::now() + IME_RECOVERY_WINDOW);
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

fn is_hangul_syllable(ch: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&ch)
}

fn is_hangul_jamo(ch: char) -> bool {
    ('\u{1100}'..='\u{11FF}').contains(&ch) || ('\u{3131}'..='\u{318E}').contains(&ch)
}

fn is_all_hangul_jamo(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_hangul_jamo)
}

fn compose_hangul_jamo_sequence(input: &str) -> String {
    let mut output = String::new();
    let mut lead: Option<char> = None;
    let mut vowel: Option<char> = None;
    let mut trail: Option<char> = None;

    let mut flush = |output: &mut String,
                     lead: &mut Option<char>,
                     vowel: &mut Option<char>,
                     trail: &mut Option<char>| {
        match (*lead, *vowel, *trail) {
            (Some(l), Some(v), t) => {
                if let Some(syllable) = compose_hangul_syllable(l, v, t) {
                    output.push(syllable);
                } else {
                    output.push(l);
                    output.push(v);
                    if let Some(t) = t {
                        output.push(t);
                    }
                }
            }
            (Some(l), None, _) => output.push(l),
            (None, Some(v), _) => output.push(v),
            _ => {}
        }
        *lead = None;
        *vowel = None;
        *trail = None;
    };

    for ch in input.chars() {
        let mapped = normalize_compat_jamo(ch).unwrap_or(ch);

        if is_vowel_jamo(mapped) {
            match (lead, vowel, trail) {
                (Some(_), None, None) => vowel = Some(mapped),
                (Some(_), Some(v), None) => {
                    if let Some(compound) = combine_vowel(v, mapped) {
                        vowel = Some(compound);
                    } else {
                        flush(&mut output, &mut lead, &mut vowel, &mut trail);
                        lead = Some('ㅇ');
                        vowel = Some(mapped);
                    }
                }
                (Some(l), Some(v), Some(t)) => {
                    if let Some((left_t, next_l)) = split_trailing_for_vowel(t) {
                        trail = left_t;
                        flush(&mut output, &mut lead, &mut vowel, &mut trail);
                        lead = Some(next_l);
                        vowel = Some(mapped);
                    } else {
                        let moved = trail.take();
                        flush(&mut output, &mut lead, &mut vowel, &mut trail);
                        lead = moved.or(Some(l));
                        vowel = Some(mapped);
                    }
                }
                (None, None, None) => {
                    lead = Some('ㅇ');
                    vowel = Some(mapped);
                }
                _ => {
                    flush(&mut output, &mut lead, &mut vowel, &mut trail);
                    lead = Some('ㅇ');
                    vowel = Some(mapped);
                }
            }
            continue;
        }

        if is_leading_jamo(mapped) {
            match (lead, vowel, trail) {
                (None, None, None) => lead = Some(mapped),
                (Some(l), None, None) => {
                    flush(&mut output, &mut lead, &mut vowel, &mut trail);
                    lead = Some(mapped);
                }
                (Some(_), Some(_), None) => {
                    if trailing_index(mapped).is_some() {
                        trail = Some(mapped);
                    } else {
                        flush(&mut output, &mut lead, &mut vowel, &mut trail);
                        lead = Some(mapped);
                    }
                }
                (Some(_), Some(_), Some(t)) => {
                    if let Some(compound) = combine_trailing(t, mapped) {
                        trail = Some(compound);
                    } else {
                        flush(&mut output, &mut lead, &mut vowel, &mut trail);
                        lead = Some(mapped);
                    }
                }
                _ => {
                    flush(&mut output, &mut lead, &mut vowel, &mut trail);
                    lead = Some(mapped);
                }
            }
            continue;
        }

        flush(&mut output, &mut lead, &mut vowel, &mut trail);
        output.push(ch);
    }

    flush(&mut output, &mut lead, &mut vowel, &mut trail);
    output
}

fn compose_hangul_syllable(lead: char, vowel: char, trail: Option<char>) -> Option<char> {
    let l = leading_index(lead)? as u32;
    let v = vowel_index(vowel)? as u32;
    let t = trail.and_then(trailing_index).unwrap_or(0) as u32;
    char::from_u32(0xAC00 + (l * 21 + v) * 28 + t)
}

fn normalize_compat_jamo(ch: char) -> Option<char> {
    Some(match ch {
        '\u{1100}' => 'ㄱ',
        '\u{1101}' => 'ㄲ',
        '\u{1102}' => 'ㄴ',
        '\u{1103}' => 'ㄷ',
        '\u{1104}' => 'ㄸ',
        '\u{1105}' => 'ㄹ',
        '\u{1106}' => 'ㅁ',
        '\u{1107}' => 'ㅂ',
        '\u{1108}' => 'ㅃ',
        '\u{1109}' => 'ㅅ',
        '\u{110A}' => 'ㅆ',
        '\u{110B}' => 'ㅇ',
        '\u{110C}' => 'ㅈ',
        '\u{110D}' => 'ㅉ',
        '\u{110E}' => 'ㅊ',
        '\u{110F}' => 'ㅋ',
        '\u{1110}' => 'ㅌ',
        '\u{1111}' => 'ㅍ',
        '\u{1112}' => 'ㅎ',
        '\u{1161}' => 'ㅏ',
        '\u{1162}' => 'ㅐ',
        '\u{1163}' => 'ㅑ',
        '\u{1164}' => 'ㅒ',
        '\u{1165}' => 'ㅓ',
        '\u{1166}' => 'ㅔ',
        '\u{1167}' => 'ㅕ',
        '\u{1168}' => 'ㅖ',
        '\u{1169}' => 'ㅗ',
        '\u{116A}' => 'ㅘ',
        '\u{116B}' => 'ㅙ',
        '\u{116C}' => 'ㅚ',
        '\u{116D}' => 'ㅛ',
        '\u{116E}' => 'ㅜ',
        '\u{116F}' => 'ㅝ',
        '\u{1170}' => 'ㅞ',
        '\u{1171}' => 'ㅟ',
        '\u{1172}' => 'ㅠ',
        '\u{1173}' => 'ㅡ',
        '\u{1174}' => 'ㅢ',
        '\u{1175}' => 'ㅣ',
        '\u{11A8}' => 'ㄱ',
        '\u{11A9}' => 'ㄲ',
        '\u{11AA}' => 'ㄳ',
        '\u{11AB}' => 'ㄴ',
        '\u{11AC}' => 'ㄵ',
        '\u{11AD}' => 'ㄶ',
        '\u{11AE}' => 'ㄷ',
        '\u{11AF}' => 'ㄹ',
        '\u{11B0}' => 'ㄺ',
        '\u{11B1}' => 'ㄻ',
        '\u{11B2}' => 'ㄼ',
        '\u{11B3}' => 'ㄽ',
        '\u{11B4}' => 'ㄾ',
        '\u{11B5}' => 'ㄿ',
        '\u{11B6}' => 'ㅀ',
        '\u{11B7}' => 'ㅁ',
        '\u{11B8}' => 'ㅂ',
        '\u{11B9}' => 'ㅄ',
        '\u{11BA}' => 'ㅅ',
        '\u{11BB}' => 'ㅆ',
        '\u{11BC}' => 'ㅇ',
        '\u{11BD}' => 'ㅈ',
        '\u{11BE}' => 'ㅊ',
        '\u{11BF}' => 'ㅋ',
        '\u{11C0}' => 'ㅌ',
        '\u{11C1}' => 'ㅍ',
        '\u{11C2}' => 'ㅎ',
        _ => return None,
    })
}

fn is_vowel_jamo(ch: char) -> bool {
    matches!(
        ch,
        'ㅏ' | 'ㅐ' | 'ㅑ' | 'ㅒ' | 'ㅓ' | 'ㅔ' | 'ㅕ' | 'ㅖ' | 'ㅗ' | 'ㅘ' | 'ㅙ' | 'ㅚ'
            | 'ㅛ' | 'ㅜ' | 'ㅝ' | 'ㅞ' | 'ㅟ' | 'ㅠ' | 'ㅡ' | 'ㅢ' | 'ㅣ'
    )
}

fn is_leading_jamo(ch: char) -> bool {
    matches!(
        ch,
        'ㄱ' | 'ㄲ' | 'ㄴ' | 'ㄷ' | 'ㄸ' | 'ㄹ' | 'ㅁ' | 'ㅂ' | 'ㅃ' | 'ㅅ' | 'ㅆ' | 'ㅇ'
            | 'ㅈ' | 'ㅉ' | 'ㅊ' | 'ㅋ' | 'ㅌ' | 'ㅍ' | 'ㅎ'
    )
}

fn leading_index(ch: char) -> Option<usize> {
    Some(match ch {
        'ㄱ' => 0,
        'ㄲ' => 1,
        'ㄴ' => 2,
        'ㄷ' => 3,
        'ㄸ' => 4,
        'ㄹ' => 5,
        'ㅁ' => 6,
        'ㅂ' => 7,
        'ㅃ' => 8,
        'ㅅ' => 9,
        'ㅆ' => 10,
        'ㅇ' => 11,
        'ㅈ' => 12,
        'ㅉ' => 13,
        'ㅊ' => 14,
        'ㅋ' => 15,
        'ㅌ' => 16,
        'ㅍ' => 17,
        'ㅎ' => 18,
        _ => return None,
    })
}

fn vowel_index(ch: char) -> Option<usize> {
    Some(match ch {
        'ㅏ' => 0,
        'ㅐ' => 1,
        'ㅑ' => 2,
        'ㅒ' => 3,
        'ㅓ' => 4,
        'ㅔ' => 5,
        'ㅕ' => 6,
        'ㅖ' => 7,
        'ㅗ' => 8,
        'ㅘ' => 9,
        'ㅙ' => 10,
        'ㅚ' => 11,
        'ㅛ' => 12,
        'ㅜ' => 13,
        'ㅝ' => 14,
        'ㅞ' => 15,
        'ㅟ' => 16,
        'ㅠ' => 17,
        'ㅡ' => 18,
        'ㅢ' => 19,
        'ㅣ' => 20,
        _ => return None,
    })
}

fn trailing_index(ch: char) -> Option<usize> {
    Some(match ch {
        'ㄱ' => 1,
        'ㄲ' => 2,
        'ㄳ' => 3,
        'ㄴ' => 4,
        'ㄵ' => 5,
        'ㄶ' => 6,
        'ㄷ' => 7,
        'ㄹ' => 8,
        'ㄺ' => 9,
        'ㄻ' => 10,
        'ㄼ' => 11,
        'ㄽ' => 12,
        'ㄾ' => 13,
        'ㄿ' => 14,
        'ㅀ' => 15,
        'ㅁ' => 16,
        'ㅂ' => 17,
        'ㅄ' => 18,
        'ㅅ' => 19,
        'ㅆ' => 20,
        'ㅇ' => 21,
        'ㅈ' => 22,
        'ㅊ' => 23,
        'ㅋ' => 24,
        'ㅌ' => 25,
        'ㅍ' => 26,
        'ㅎ' => 27,
        _ => return None,
    })
}

fn combine_vowel(first: char, second: char) -> Option<char> {
    Some(match (first, second) {
        ('ㅗ', 'ㅏ') => 'ㅘ',
        ('ㅗ', 'ㅐ') => 'ㅙ',
        ('ㅗ', 'ㅣ') => 'ㅚ',
        ('ㅜ', 'ㅓ') => 'ㅝ',
        ('ㅜ', 'ㅔ') => 'ㅞ',
        ('ㅜ', 'ㅣ') => 'ㅟ',
        ('ㅡ', 'ㅣ') => 'ㅢ',
        _ => return None,
    })
}

fn combine_trailing(first: char, second: char) -> Option<char> {
    Some(match (first, second) {
        ('ㄱ', 'ㅅ') => 'ㄳ',
        ('ㄴ', 'ㅈ') => 'ㄵ',
        ('ㄴ', 'ㅎ') => 'ㄶ',
        ('ㄹ', 'ㄱ') => 'ㄺ',
        ('ㄹ', 'ㅁ') => 'ㄻ',
        ('ㄹ', 'ㅂ') => 'ㄼ',
        ('ㄹ', 'ㅅ') => 'ㄽ',
        ('ㄹ', 'ㅌ') => 'ㄾ',
        ('ㄹ', 'ㅍ') => 'ㄿ',
        ('ㄹ', 'ㅎ') => 'ㅀ',
        ('ㅂ', 'ㅅ') => 'ㅄ',
        _ => return None,
    })
}

fn split_trailing_for_vowel(trailing: char) -> Option<(Option<char>, char)> {
    Some(match trailing {
        'ㄳ' => (Some('ㄱ'), 'ㅅ'),
        'ㄵ' => (Some('ㄴ'), 'ㅈ'),
        'ㄶ' => (Some('ㄴ'), 'ㅎ'),
        'ㄺ' => (Some('ㄹ'), 'ㄱ'),
        'ㄻ' => (Some('ㄹ'), 'ㅁ'),
        'ㄼ' => (Some('ㄹ'), 'ㅂ'),
        'ㄽ' => (Some('ㄹ'), 'ㅅ'),
        'ㄾ' => (Some('ㄹ'), 'ㅌ'),
        'ㄿ' => (Some('ㄹ'), 'ㅍ'),
        'ㅀ' => (Some('ㄹ'), 'ㅎ'),
        'ㅄ' => (Some('ㅂ'), 'ㅅ'),
        other if trailing_index(other).is_some() => (None, other),
        _ => return None,
    })
}
