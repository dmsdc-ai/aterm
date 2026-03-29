# aterm v3 Phase 1: Minimal winit+wgpu Terminal

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace iced with a bare winit+wgpu terminal that renders alacritty_terminal grid via glyphon, with keyboard input → PTY → display.

**Architecture:** winit EventLoop drives the app. wgpu renders to the window surface. glyphon (cosmic-text) rasterizes monospace glyphs. alacritty_terminal manages terminal state. core/pty.rs manages PTY I/O (reused as-is).

**Tech Stack:** winit 0.30, wgpu 23, glyphon 0.6, alacritty_terminal 0.26, portable-pty 0.9

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` | Modify | Remove iced, add wgpu/glyphon/winit direct deps |
| `src-v3/main.rs` | Rewrite | winit EventLoop + app state + PTY integration |
| `src-v3/renderer.rs` | Create | wgpu + glyphon terminal grid renderer |
| `src-v3/core/pty.rs` | Keep | PTY management (no changes) |
| `src-v3/core/inject.rs` | Keep | Inject queue (no changes) |
| `src-v3/core/session.rs` | Keep | Session persistence (no changes) |
| `src-v3/core/telepty.rs` | Keep | Telepty client (no changes) |
| `src-v3/core/mod.rs` | Keep | Re-exports (no changes) |
| `src-v3/terminal/mod.rs` | Modify | Remove iced imports, keep TerminalState |
| `src-v3/terminal/renderer.rs` | Delete | Replaced by top-level renderer.rs |
| `src-v3/terminal/widget.rs` | Delete | No more iced widget |
| `src-v3/ui/*` | Delete | No more iced UI (Phase 2+ rebuilds) |
| `src-v3/ime/mod.rs` | Keep | IME types (no changes) |
| `src-v3/ime/macos.rs` | Keep | ClassBuilder IME (Phase 5 reconnects) |

---

## Chunk 1: Dependencies & Skeleton

### Task 1: Update Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Remove iced, add winit+wgpu+glyphon**

Replace dependencies section:
```toml
[dependencies]
alacritty_terminal = "0.26.0-rc1"
dirs = "6"
glyphon = "0.6"
portable-pty = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt", "sync"] }
wgpu = "23"
winit = { version = "0.30", features = ["rwh_06"] }
pollster = "0.4"

[target.'cfg(target_os = "macos")'.dependencies]
block2 = "0.6"
objc2 = "0.6.4"
objc2-app-kit = { version = "0.3.2", features = ["NSEvent", "NSTextInputClient", "NSTextInputContext", "NSView", "NSApplication", "NSRunningApplication", "NSWindow", "NSResponder"] }
objc2-core-foundation = "0.3.2"
objc2-foundation = { version = "0.3.2", features = ["NSArray", "NSAttributedString", "NSGeometry", "NSRange", "NSString"] }
```

Remove `rfd` (file dialog — Phase 2+). Remove `[patch.crates-io]` if present.

- [ ] **Step 2: Verify dependency resolution**

Run: `cargo check 2>&1 | head -5`
Expected: dependency resolution succeeds (code won't compile yet — that's OK)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: replace iced with winit+wgpu+glyphon dependencies"
```

### Task 2: Delete iced-dependent files

**Files:**
- Delete: `src-v3/terminal/widget.rs`
- Delete: `src-v3/terminal/renderer.rs`
- Delete: `src-v3/ui/sidebar.rs`
- Delete: `src-v3/ui/theme.rs`
- Delete: `src-v3/ui/command_palette.rs`
- Delete: `src-v3/ui/group_grid.rs`
- Delete: `src-v3/ui/create_session_dialog.rs`
- Delete: `src-v3/ui/deliberate_dialog.rs`
- Delete: `src-v3/ui/settings.rs`
- Delete: `src-v3/ui/mod.rs`

- [ ] **Step 1: Remove iced UI files**

```bash
rm src-v3/terminal/widget.rs src-v3/terminal/renderer.rs
rm src-v3/ui/sidebar.rs src-v3/ui/theme.rs src-v3/ui/command_palette.rs
rm src-v3/ui/group_grid.rs src-v3/ui/create_session_dialog.rs
rm src-v3/ui/deliberate_dialog.rs src-v3/ui/settings.rs src-v3/ui/mod.rs
```

- [ ] **Step 2: Update module declarations**

In `src-v3/terminal/mod.rs`: remove `pub mod renderer;` and `pub mod widget;` and their re-exports.
In `src-v3/main.rs`: will be fully rewritten in Task 3, so no changes needed here yet.

- [ ] **Step 3: Keep cli_presets.rs**

Move `src-v3/ui/cli_presets.rs` to `src-v3/core/cli_presets.rs` and update `src-v3/core/mod.rs` to re-export.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: remove iced-dependent UI files, keep core"
```

### Task 3: Minimal winit window

**Files:**
- Rewrite: `src-v3/main.rs`

- [ ] **Step 1: Write minimal main.rs with winit window**

```rust
mod core;
mod ime;
mod terminal;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId, WindowAttributes};

struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = WindowAttributes::default()
                .with_title("aterm v3")
                .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));
            self.window = Some(event_loop.create_window(attrs).unwrap());
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                // wgpu render will go here
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { window: None };
    event_loop.run_app(&mut app).unwrap();
}
```

- [ ] **Step 2: Verify it compiles and opens a window**

Run: `cargo run`
Expected: empty window titled "aterm v3" appears

- [ ] **Step 3: Commit**

```bash
git add src-v3/main.rs
git commit -m "feat: minimal winit window (no iced)"
```

---

## Chunk 2: wgpu Rendering + glyphon Text

### Task 4: wgpu surface setup

**Files:**
- Modify: `src-v3/main.rs`

- [ ] **Step 1: Add wgpu initialization in resumed()**

After window creation, set up wgpu Instance → Adapter → Device → Queue → Surface.
Store in App struct: `device`, `queue`, `surface`, `surface_config`.

- [ ] **Step 2: Render clear color on RedrawRequested**

In `window_event(RedrawRequested)`: get surface texture, create CommandEncoder, begin_render_pass with dark background color (#131010), submit, present.

- [ ] **Step 3: Handle resize**

On `WindowEvent::Resized`: reconfigure surface with new size.

- [ ] **Step 4: Verify dark window renders**

Run: `cargo run`
Expected: dark brown (#131010) window

- [ ] **Step 5: Commit**

```bash
git add src-v3/main.rs
git commit -m "feat: wgpu surface with dark background"
```

### Task 5: glyphon text rendering

**Files:**
- Create: `src-v3/renderer.rs`
- Modify: `src-v3/main.rs`

- [ ] **Step 1: Create renderer.rs with glyphon setup**

```rust
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer, Viewport,
};

pub struct TerminalRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
}
```

Initialize FontSystem with monospace font, create TextAtlas and TextRenderer.

- [ ] **Step 2: Add render_grid method**

Takes alacritty_terminal grid → creates glyphon Buffer with cell content → prepare → render.

- [ ] **Step 3: Integrate in main.rs RedrawRequested**

Call `renderer.render_grid(&terminal, &device, &queue, &surface_texture)`.

- [ ] **Step 4: Display "Hello, terminal!" test text**

Hardcode a test string to verify glyphon renders correctly.

- [ ] **Step 5: Commit**

```bash
git add src-v3/renderer.rs src-v3/main.rs
git commit -m "feat: glyphon text rendering on wgpu surface"
```

---

## Chunk 3: Terminal + PTY Integration

### Task 6: PTY + TerminalState integration

**Files:**
- Modify: `src-v3/main.rs`
- Modify: `src-v3/terminal/mod.rs`

- [ ] **Step 1: Clean terminal/mod.rs of iced references**

Remove any `pub use renderer::*` or `pub use widget::*`. Keep TerminalState, SharedTerminal, feed_output, sync_snapshot, resize.

- [ ] **Step 2: Create default PTY workspace in App::resumed**

Use `core::PtyManager::shared()` to create a default workspace. Set up PTY output signal polling.

- [ ] **Step 3: Feed PTY output to TerminalState**

On RedrawRequested (or via a polling mechanism): drain pending PTY output → `terminal.feed_output(data)`.

- [ ] **Step 4: Render terminal grid from TerminalState**

In renderer.rs `render_grid`: iterate `Term::renderable_content()` cells → position each character with glyphon.

- [ ] **Step 5: Verify shell prompt renders**

Run: `cargo run`
Expected: shell prompt visible in the window

- [ ] **Step 6: Commit**

```bash
git add src-v3/main.rs src-v3/terminal/mod.rs src-v3/renderer.rs
git commit -m "feat: PTY output renders in terminal grid"
```

### Task 7: Keyboard input → PTY

**Files:**
- Modify: `src-v3/main.rs`

- [ ] **Step 1: Handle KeyboardInput events**

On `WindowEvent::KeyboardInput`: extract text → `manager.send_to_workspace(id, text)`.
Handle named keys: Enter→\r, Backspace→\x7f, Tab→\t, arrows→ANSI sequences.

- [ ] **Step 2: Handle IME events**

On `WindowEvent::Ime(Ime::Commit(text))`: send text to PTY.
On `WindowEvent::Ime(Ime::Preedit(..))`: store preedit state.
Call `window.set_ime_allowed(true)` + `set_ime_purpose(ImePurpose::Terminal)` at startup.

- [ ] **Step 3: Handle resize → PTY resize**

On `WindowEvent::Resized`: calculate cols/rows from new size → `manager.resize(id, cols, rows)` + `terminal.resize(cols, rows)`.

- [ ] **Step 4: Continuous redraw for PTY output**

Request redraw when PTY output arrives (signal-based or timer-based polling).

- [ ] **Step 5: Verify interactive terminal**

Run: `cargo run`
Expected: can type commands, see output, ls/cd/echo work

- [ ] **Step 6: Commit**

```bash
git add src-v3/main.rs
git commit -m "feat: keyboard input + IME + resize = interactive terminal"
```

---

## Chunk 4: Polish & Validation

### Task 8: Session persistence + cleanup

**Files:**
- Modify: `src-v3/main.rs`

- [ ] **Step 1: Restore session on startup**

Use `session_store.restore_into(&manager)` to restore previous sessions.

- [ ] **Step 2: Save session on exit**

On `WindowEvent::CloseRequested`: save sessions before exit.

- [ ] **Step 3: TERM environment variable**

Verify PTY sets `TERM=xterm-256color` and `TERM_PROGRAM=aterm` (already in core/pty.rs).

- [ ] **Step 4: Commit**

```bash
git add src-v3/main.rs
git commit -m "feat: session persistence on startup/exit"
```

### Task 9: Final validation

- [ ] **Step 1: cargo check** — 0 errors
- [ ] **Step 2: cargo run** — window opens, shell prompt visible
- [ ] **Step 3: Type "echo hello"** — output appears
- [ ] **Step 4: Korean input** — set_ime_allowed(true) enables IME, test 한글
- [ ] **Step 5: Resize window** — terminal reflows
- [ ] **Step 6: Ctrl+C** — sends interrupt
- [ ] **Step 7: Close window** — clean exit, session saved
