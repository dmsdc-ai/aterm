use std::sync::Arc;

use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{
    layout, mouse, overlay, renderer, text, Clipboard, Shell,
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
        _tree: &Tree,
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
            }
            Event::InputMethod(ime_event) => {
                match ime_event {
                    input_method::Event::Commit(text) => {
                        eprintln!("[IME] commit: {:?}", text);
                        if state.focused && !text.is_empty() {
                            if let Some(on_event) = self.on_event.as_ref() {
                                shell.publish(on_event(TerminalEvent::Input(text.as_bytes().to_vec())));
                                shell.capture_event();
                            }
                        }
                        state.ime_composing = false;
                    }
                    input_method::Event::Preedit(text, _cursor) => {
                        eprintln!("[IME] preedit: {:?}", text);
                        state.ime_composing = !text.is_empty();
                        shell.capture_event();
                    }
                    input_method::Event::Opened => {
                        eprintln!("[IME] opened");
                        state.ime_composing = true;
                    }
                    input_method::Event::Closed => {
                        eprintln!("[IME] closed");
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
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) if state.focused && !state.ime_composing => {
                if let Some(bytes) = map_key_to_bytes(key, text.as_deref(), *modifiers) {
                    if let Some(on_event) = self.on_event.as_ref() {
                        shell.publish(on_event(TerminalEvent::Input(bytes)));
                        shell.capture_event();
                    }
                }
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
}

fn map_key_to_bytes(
    key: &keyboard::Key,
    text: Option<&str>,
    modifiers: keyboard::Modifiers,
) -> Option<Vec<u8>> {
    if modifiers.control() {
        if let Some(ch) = text.and_then(|value| value.chars().next()) {
            let lower = ch.to_ascii_lowercase();
            if lower.is_ascii_lowercase() {
                return Some(vec![(lower as u8) - b'a' + 1]);
            }
        }
    }

    if !modifiers.command() && !modifiers.alt() {
        if let Some(text) = text {
            if !text.is_empty() {
                return Some(text.as_bytes().to_vec());
            }
        }
    }

    match key.as_ref() {
        keyboard::Key::Named(keyboard::key::Named::Enter) => Some(b"\r".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::Tab) => Some(b"\t".to_vec()),
        keyboard::Key::Named(keyboard::key::Named::Backspace) => {
            Some(vec![0x7f])
        }
        keyboard::Key::Named(keyboard::key::Named::Escape) => Some(vec![0x1b]),
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
            Some(b"\x1b[A".to_vec())
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
            Some(b"\x1b[B".to_vec())
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
            Some(b"\x1b[C".to_vec())
        }
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
            Some(b"\x1b[D".to_vec())
        }
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
