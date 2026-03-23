use std::sync::Arc;

use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{
    clipboard, layout, mouse, overlay, renderer, text, Clipboard, Shell,
};
use iced::advanced::input_method;
use iced::{
    alignment, keyboard, window, Color, Element, Event, Font, Length, Point, Rectangle, Size,
};

use super::renderer::{RenderedTerminal, TerminalRenderer};
use super::SharedTerminal;

#[derive(Debug, Clone)]
pub enum TerminalEvent {
    Input(Vec<u8>),
    Resize { columns: u16, rows: u16 },
    FocusChanged(bool),
    /// Scroll the viewport (positive = up into history, negative = down).
    Scroll(i32),
}

/// A minimal iced widget that renders terminal content with the GPU-backed renderer.
pub struct TerminalWidget<Message> {
    terminal: SharedTerminal,
    renderer: TerminalRenderer,
    on_event: Option<Arc<dyn Fn(TerminalEvent) -> Message + Send + Sync>>,
}

impl<Message> TerminalWidget<Message> {
    pub fn new(terminal: SharedTerminal) -> Self {
        Self {
            terminal,
            renderer: TerminalRenderer::default(),
            on_event: None,
        }
    }

    pub fn with_renderer(terminal: SharedTerminal, renderer: TerminalRenderer) -> Self {
        Self {
            terminal,
            renderer,
            on_event: None,
        }
    }

    pub fn on_event(
        mut self,
        on_event: impl Fn(TerminalEvent) -> Message + Send + Sync + 'static,
    ) -> Self {
        self.on_event = Some(Arc::new(on_event));
        self
    }

    pub fn with_terminal_renderer(mut self, renderer: TerminalRenderer) -> Self {
        self.renderer = renderer;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TerminalWidget<Message>
where
    Renderer: renderer::Renderer + text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let terminal = self.terminal.lock().ok();
        let Some(terminal) = terminal.as_deref() else {
            return;
        };

        let frame = self.renderer.snapshot(terminal);
        self.draw_frame(renderer, bounds, viewport, &frame);

        // Draw selection highlight overlay
        let state = tree.state.downcast_ref::<State>();
        if let (Some(start), Some(end)) = (state.selection_start, state.selection_end) {
            if start != end {
                self.draw_selection(renderer, bounds, start, end);
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: iced::advanced::Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();

        self.publish_resize(state, layout.bounds(), shell);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let focused = cursor.is_over(layout.bounds());
                if state.focused != focused {
                    state.focused = focused;
                    if let Some(on_event) = self.on_event.as_ref() {
                        shell.publish(on_event(TerminalEvent::FocusChanged(focused)));
                    }
                }
                if focused {
                    if let Some(pos) = cursor.position_in(layout.bounds()) {
                        let col = (pos.x / self.renderer.cell_width()) as usize;
                        let row = (pos.y / self.renderer.cell_height()) as usize;
                        state.selecting = true;
                        state.selection_start = Some((row, col));
                        state.selection_end = Some((row, col));
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.selecting => {
                if let Some(pos) = cursor.position_in(layout.bounds()) {
                    let col = (pos.x / self.renderer.cell_width()) as usize;
                    let row = (pos.y / self.renderer.cell_height()) as usize;
                    state.selection_end = Some((row, col));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.selecting => {
                state.selecting = false;
                if let (Some(start), Some(end)) = (state.selection_start, state.selection_end) {
                    if start != end {
                        let selected_text = self.extract_selection(start, end);
                        if !selected_text.is_empty() {
                            _clipboard.write(clipboard::Kind::Standard, selected_text);
                            eprintln!("[SELECT] copied to clipboard");
                        }
                    }
                }
            }
            Event::InputMethod(ime_event) => {
                match ime_event {
                    input_method::Event::Commit(text) => {
                        eprintln!("[EVENT] InputMethod::Commit({:?}) composing={} last_commit={:?}",
                            text, state.ime_composing, state.last_commit_text);
                        if state.focused && !text.is_empty() {
                            if let Some(on_event) = self.on_event.as_ref() {
                                shell.publish(on_event(TerminalEvent::Input(text.as_bytes().to_vec())));
                                shell.capture_event();
                            }
                        }
                        state.ime_composing = false;
                        state.last_commit_text = Some(text.to_string());
                    }
                    input_method::Event::Preedit(text, _cursor) => {
                        eprintln!("[EVENT] InputMethod::Preedit({:?}) composing={}",
                            text, state.ime_composing);
                        state.ime_composing = !text.is_empty();
                        shell.capture_event();
                    }
                    input_method::Event::Opened => {
                        eprintln!("[EVENT] InputMethod::Opened");
                        state.ime_composing = true;
                    }
                    input_method::Event::Closed => {
                        eprintln!("[EVENT] InputMethod::Closed");
                        state.ime_composing = false;
                    }
                }
            }
            Event::Window(window::Event::RedrawRequested(_)) => {
                if state.focused {
                    let cursor_rect = Rectangle::new(
                        Point::new(0.0, 0.0),
                        Size::new(1.0, self.renderer.cell_height()),
                    );
                    shell.request_input_method(&input_method::InputMethod::Enabled {
                        cursor: cursor_rect,
                        purpose: input_method::Purpose::Terminal,
                        preedit: None::<input_method::Preedit<String>>,
                    });
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if state.focused => {
                if cursor.is_over(layout.bounds()) {
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => *y,
                        mouse::ScrollDelta::Pixels { y, .. } => y / self.renderer.cell_height(),
                    };
                    // Positive y = scroll up (into history), negative = scroll down
                    let scroll_delta = lines.round() as i32;
                    if scroll_delta != 0 {
                        if let Some(on_event) = self.on_event.as_ref() {
                            shell.publish(on_event(TerminalEvent::Scroll(scroll_delta)));
                            shell.capture_event();
                        }
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) if state.focused && !state.ime_composing => {
                eprintln!("[EVENT] KeyPressed key={:?} text={:?} mods={:?} composing={} last_commit={:?}",
                    key, text, modifiers, state.ime_composing, state.last_commit_text);

                // Contract enforcement: Hangul in KeyPressed is always a winit/macOS
                // race condition artifact. Hangul must arrive through IME Commit only.
                if let Some(t) = text.as_deref() {
                    if contains_hangul(t) {
                        eprintln!("[IME] hangul in KeyPressed blocked: {:?} (must come via Commit)", t);
                        shell.capture_event();
                        return;
                    }
                }

                if modifiers.shift()
                    && matches!(
                        key.as_ref(),
                        keyboard::Key::Named(keyboard::key::Named::PageUp)
                            | keyboard::Key::Named(keyboard::key::Named::PageDown)
                    )
                {
                    // Shift+PageUp/Down → viewport scroll (not PTY)
                    let delta = if matches!(key.as_ref(), keyboard::Key::Named(keyboard::key::Named::PageUp)) {
                        state.rows as i32
                    } else {
                        -(state.rows as i32)
                    };
                    if let Some(on_event) = self.on_event.as_ref() {
                        shell.publish(on_event(TerminalEvent::Scroll(delta)));
                        shell.capture_event();
                    }
                } else if modifiers.command()
                    && matches!(key.as_ref(), keyboard::Key::Character("c"))
                {
                    // Cmd+C → copy selection to clipboard
                    if let (Some(start), Some(end)) =
                        (state.selection_start, state.selection_end)
                    {
                        if start != end {
                            let text = self.extract_selection(start, end);
                            if !text.is_empty() {
                                _clipboard.write(clipboard::Kind::Standard, text);
                                eprintln!("[SELECT] Cmd+C copied to clipboard");
                            }
                        }
                    }
                    // Clear selection after copy
                    state.selection_start = None;
                    state.selection_end = None;
                    shell.capture_event();
                } else if is_paste_shortcut(key, *modifiers) {
                    // Cmd+V → paste from clipboard
                    if let Some(content) = _clipboard.read(clipboard::Kind::Standard) {
                        eprintln!("[CLIPBOARD] paste: {} bytes", content.len());
                        if let Some(on_event) = self.on_event.as_ref() {
                            shell.publish(on_event(TerminalEvent::Input(content.into_bytes())));
                            shell.capture_event();
                        }
                    }
                } else {
                    // Content-based dedup: if KeyPressed text matches last Commit text, it's a duplicate.
                    let is_duplicate = text.as_deref()
                        .filter(|t| !t.is_empty())
                        .and_then(|t| state.last_commit_text.as_deref().map(|ct| ct == t))
                        .unwrap_or(false);

                    if is_duplicate {
                        eprintln!("[IME] dedup: KeyPressed text {:?} matches last Commit — skipping", text);
                        state.last_commit_text = None;
                        shell.capture_event();
                    } else {
                        // Clear last_commit_text since this KeyPressed is different
                        if text.as_deref().filter(|t| !t.is_empty()).is_some() {
                            state.last_commit_text = None;
                        }
                        if let Some(bytes) = map_key_to_bytes(key, text.as_deref(), *modifiers) {
                            if let Some(on_event) = self.on_event.as_ref() {
                                shell.publish(on_event(TerminalEvent::Input(bytes)));
                                shell.capture_event();
                            }
                        }
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key, text, ..
            }) if state.focused && state.ime_composing => {
                eprintln!("[EVENT] KeyPressed BLOCKED by ime_composing key={:?} text={:?}", key, text);
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        _layout: iced::advanced::Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::Text
    }

    fn overlay<'a>(
        &'a mut self,
        _tree: &'a mut Tree,
        _layout: iced::advanced::Layout<'a>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        _translation: iced::Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        None
    }
}

impl<Message> TerminalWidget<Message> {
    fn draw_frame<Renderer>(
        &self,
        renderer: &mut Renderer,
        bounds: Rectangle,
        viewport: &Rectangle,
        frame: &RenderedTerminal,
    ) where
        Renderer: renderer::Renderer + text::Renderer<Font = Font>,
    {
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            self.renderer.background,
        );

        let font = Font::MONOSPACE;
        let font_size = self.renderer.font_size;
        let line_height = self.renderer.cell_height();
        let cell_width = self.renderer.cell_width();

        for line in &frame.lines {
            for cell in &line.cells {
                let x = bounds.x + cell.column as f32 * cell_width;
                let y = bounds.y + cell.row as f32 * line_height;

                if cell.background != self.renderer.background {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(x, y),
                                Size::new(cell_width * cell.width as f32, line_height),
                            ),
                            ..renderer::Quad::default()
                        },
                        cell.background,
                    );
                }

                if cell.ch != ' ' {
                    renderer.fill_text(
                        text::Text {
                            content: cell.as_string(),
                            bounds: Size::new(cell_width * cell.width as f32, line_height),
                            size: font_size,
                            line_height: text::LineHeight::Relative(self.renderer.line_height),
                            font,
                            align_x: text::Alignment::Left,
                            align_y: alignment::Vertical::Top,
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(x, y),
                        cell.foreground,
                        *viewport,
                    );
                }
            }
        }

        if let Some((row, col)) = frame.cursor {
            let x = bounds.x + col as f32 * cell_width;
            let y = bounds.y + row as f32 * line_height;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(x, y),
                        Size::new(cell_width, line_height),
                    ),
                    ..renderer::Quad::default()
                },
                Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            );
        }
    }

    fn extract_selection(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let terminal = self.terminal.lock().ok();
        let Some(terminal) = terminal.as_deref() else {
            return String::new();
        };

        let frame = self.renderer.snapshot(terminal);

        // Normalize so start <= end in reading order
        let (start, end) = if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
            (start, end)
        } else {
            (end, start)
        };

        let mut result = String::new();
        for line in &frame.lines {
            if line.row < start.0 || line.row > end.0 {
                continue;
            }

            let mut line_text = String::new();
            for cell in &line.cells {
                let include = if line.row == start.0 && line.row == end.0 {
                    cell.column >= start.1 && cell.column <= end.1
                } else if line.row == start.0 {
                    cell.column >= start.1
                } else if line.row == end.0 {
                    cell.column <= end.1
                } else {
                    true
                };

                if include {
                    line_text.push(cell.ch);
                }
            }

            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line_text.trim_end());
        }

        result
    }

    fn draw_selection<R: renderer::Renderer>(
        &self,
        renderer: &mut R,
        bounds: Rectangle,
        start: (usize, usize),
        end: (usize, usize),
    ) {
        // Normalize so start <= end in reading order
        let (start, end) = if start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1) {
            (start, end)
        } else {
            (end, start)
        };

        let cell_w = self.renderer.cell_width();
        let cell_h = self.renderer.cell_height();
        let highlight_color = Color::from_rgba(0.85, 0.47, 0.02, 0.25); // amber selection

        for row in start.0..=end.0 {
            let col_start = if row == start.0 { start.1 } else { 0 };
            let col_end = if row == end.0 { end.1 + 1 } else { 200 }; // wide enough

            let x = bounds.x + col_start as f32 * cell_w;
            let y = bounds.y + row as f32 * cell_h;
            let w = (col_end - col_start) as f32 * cell_w;

            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(Point::new(x, y), Size::new(w, cell_h)),
                    ..renderer::Quad::default()
                },
                highlight_color,
            );
        }
    }

    fn publish_resize(
        &self,
        state: &mut State,
        bounds: Rectangle,
        shell: &mut Shell<'_, Message>,
    ) {
        let columns = (bounds.width / self.renderer.cell_width()).floor().max(2.0) as u16;
        let rows = (bounds.height / self.renderer.cell_height()).floor().max(1.0) as u16;

        if state.columns == columns && state.rows == rows {
            return;
        }

        eprintln!("[RESIZE] widget bounds changed: {}x{} -> {}x{} cols/rows", state.columns, state.rows, columns, rows);
        state.columns = columns;
        state.rows = rows;

        if let Some(on_event) = self.on_event.as_ref() {
            shell.publish(on_event(TerminalEvent::Resize { columns, rows }));
        }
    }
}

#[derive(Debug, Default)]
struct State {
    focused: bool,
    columns: u16,
    rows: u16,
    ime_composing: bool,
    /// Text from the last IME Commit — used to suppress duplicate KeyPressed with identical text.
    last_commit_text: Option<String>,
    /// Whether the user is currently dragging to select text.
    selecting: bool,
    /// Selection anchor (row, col) in cell coordinates.
    selection_start: Option<(usize, usize)>,
    /// Selection moving end (row, col) in cell coordinates.
    selection_end: Option<(usize, usize)>,
}

/// Hangul characters in KeyPressed text are always race condition artifacts.
/// Contract: Hangul must arrive through InputMethod::Commit only.
fn contains_hangul(text: &str) -> bool {
    text.chars().any(|c| matches!(c,
        '\u{1100}'..='\u{11FF}'   // Hangul Jamo
        | '\u{3130}'..='\u{318F}' // Hangul Compatibility Jamo
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
    ))
}

/// Check if this is a Cmd+V paste action.
fn is_paste_shortcut(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    modifiers.command()
        && matches!(key.as_ref(), keyboard::Key::Character("v"))
}

/// Map keyboard events to PTY byte sequences.
///
/// macOS IME behaviour:
/// - Korean mode: text arrives via Preedit → Commit. KeyPressed may echo the
///   same text — deduplicated by content comparison in the caller.
/// - English mode: text arrives via KeyPressed only. No Commit event fires.
/// - Ctrl+<letter>: bypasses IME, arrives via KeyPressed.
fn map_key_to_bytes(
    key: &keyboard::Key,
    text: Option<&str>,
    modifiers: keyboard::Modifiers,
) -> Option<Vec<u8>> {
    // Ctrl+<letter> → control codes (e.g., Ctrl+C → \x03)
    if modifiers.control() {
        if let Some(ch) = text.and_then(|value| value.chars().next()) {
            let lower = ch.to_ascii_lowercase();
            if lower.is_ascii_lowercase() {
                return Some(vec![(lower as u8) - b'a' + 1]);
            }
        }
    }

    // Normal text input
    if !modifiers.command() && !modifiers.alt() {
        if let Some(text) = text {
            if !text.is_empty() {
                return Some(text.as_bytes().to_vec());
            }
        }
    }

    // Named keys — these don't go through IME.
    match key.as_ref() {
        keyboard::Key::Named(keyboard::key::Named::Enter) => Some(b"\r".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::Tab) => Some(b"\t".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::Backspace) => Some(vec![0x7f]),
        keyboard::Key::Named(keyboard::key::Named::Escape) => Some(vec![0x1b]),
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some(b"\x1b[A".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some(b"\x1b[B".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some(b"\x1b[C".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some(b"\x1b[D".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::Home) => Some(b"\x1b[H".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::End) => Some(b"\x1b[F".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::Delete) => Some(b"\x1b[3~".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::PageUp) => Some(b"\x1b[5~".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::PageDown) => Some(b"\x1b[6~".to_vec()),
        _ => None,
    }
}

impl<'a, Message, Theme, Renderer> From<TerminalWidget<Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + text::Renderer<Font = Font> + 'a,
{
    fn from(widget: TerminalWidget<Message>) -> Self {
        Element::new(widget)
    }
}
