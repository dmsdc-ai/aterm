use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{cell::Flags, RenderableContent, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb};
use iced::{Color, Pixels};

/// A single rendered cell derived from Alacritty terminal state.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedCell {
    pub row: usize,
    pub column: usize,
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub width: usize,
}

impl RenderedCell {
    pub fn as_string(&self) -> String {
        self.ch.to_string()
    }
}

/// One terminal row.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedLine {
    pub row: usize,
    pub cells: Vec<RenderedCell>,
}

impl RenderedLine {
    pub fn text(&self) -> String {
        self.cells.iter().map(RenderedCell::as_string).collect()
    }
}

/// A minimal terminal frame used by the widget renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTerminal {
    pub lines: Vec<RenderedLine>,
    pub cursor: Option<(usize, usize)>,
}

impl RenderedTerminal {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Helper that converts terminal grid state into drawable rows and cells.
#[derive(Debug, Clone)]
pub struct TerminalRenderer {
    pub font_size: Pixels,
    pub line_height: f32,
    pub foreground: Color,
    pub background: Color,
    pub is_light: bool,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            font_size: Pixels(14.0),
            line_height: 1.25,
            foreground: Color::from_rgb8(0xe8, 0xe4, 0xe0),
            background: Color::from_rgb8(0x13, 0x10, 0x10),
            is_light: false,
        }
    }
}

impl TerminalRenderer {
    pub fn with_colors(mut self, foreground: Color, background: Color) -> Self {
        self.foreground = foreground;
        self.background = background;
        self
    }

    pub fn with_light_mode(mut self, is_light: bool) -> Self {
        self.is_light = is_light;
        self
    }
}

impl TerminalRenderer {
    pub fn snapshot<T: EventListener>(&self, term: &Term<T>) -> RenderedTerminal {
        let content = term.renderable_content();
        self.snapshot_from_content(content)
    }

    fn snapshot_from_content(&self, content: RenderableContent<'_>) -> RenderedTerminal {
        let mut lines: Vec<RenderedLine> = Vec::new();
        let mut current_line: Option<i32> = None;
        let mut row = 0usize;

        let default_foreground = content.colors[NamedColor::Foreground]
            .map(rgb_to_color)
            .unwrap_or(self.foreground);
        let default_background = content.colors[NamedColor::Background]
            .map(rgb_to_color)
            .unwrap_or(self.background);

        let light = self.is_light;

        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            if current_line != Some(line) {
                if current_line.is_some() {
                    row += 1;
                }
                lines.push(RenderedLine {
                    row,
                    cells: Vec::new(),
                });
                current_line = Some(line);
            }

            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }

            let mut foreground = resolve_color(cell.fg, default_foreground, content.colors, light);
            let mut background = resolve_color(cell.bg, default_background, content.colors, light);

            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }

            lines
                .last_mut()
                .expect("line exists after push")
                .cells
                .push(RenderedCell {
                    row,
                    column: indexed.point.column.0.max(0) as usize,
                    ch: cell.c,
                    foreground,
                    background,
                    width: if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 },
                });
        }

        let cursor = content
            .mode
            .contains(TermMode::SHOW_CURSOR)
            .then_some((
                content.cursor.point.line.0.max(0) as usize,
                content.cursor.point.column.0.max(0) as usize,
            ));
        RenderedTerminal { lines, cursor }
    }

    pub fn cell_width(&self) -> f32 {
        (self.font_size.0 * 0.62).max(1.0)
    }

    pub fn cell_height(&self) -> f32 {
        (self.font_size.0 * self.line_height).max(1.0)
    }
}

fn rgb_to_color(rgb: alacritty_terminal::vte::ansi::Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

fn resolve_color(
    color: AnsiColor,
    fallback: Color,
    colors: &alacritty_terminal::term::color::Colors,
    is_light: bool,
) -> Color {
    match color {
        AnsiColor::Named(named) => colors[named]
            .map(rgb_to_color)
            .unwrap_or_else(|| named_color_fallback(named, fallback, is_light)),
        AnsiColor::Spec(rgb) => rgb_to_color(rgb),
        AnsiColor::Indexed(index) => colors[index as usize]
            .map(rgb_to_color)
            .unwrap_or(fallback),
    }
}

fn named_color_fallback(named: NamedColor, default: Color, is_light: bool) -> Color {
    if is_light {
        return named_color_light(named, default);
    }
    named_color_dark(named, default)
}

/// v2 dark ANSI palette (from colors.css --term-*)
fn named_color_dark(named: NamedColor, default: Color) -> Color {
    match named {
        NamedColor::Black => Color::from_rgb8(0x50, 0x48, 0x40),
        NamedColor::Red => Color::from_rgb8(0xd9, 0x6c, 0x6c),
        NamedColor::Green => Color::from_rgb8(0x5c, 0xb9, 0x7a),
        NamedColor::Yellow => Color::from_rgb8(0xd4, 0xa8, 0x53),
        NamedColor::Blue => Color::from_rgb8(0xd9, 0x77, 0x06),
        NamedColor::Magenta => Color::from_rgb8(0xb8, 0xa0, 0xd8),
        NamedColor::Cyan => Color::from_rgb8(0x5c, 0xb0, 0xb8),
        NamedColor::White => Color::from_rgb8(0xb0, 0xa8, 0x98),
        NamedColor::BrightBlack => Color::from_rgb8(0x6a, 0x60, 0x58),
        NamedColor::BrightRed => Color::from_rgb8(0xe8, 0x80, 0x80),
        NamedColor::BrightGreen => Color::from_rgb8(0x70, 0xcc, 0x8a),
        NamedColor::BrightYellow => Color::from_rgb8(0xe8, 0xbb, 0x6a),
        NamedColor::BrightBlue => Color::from_rgb8(0xf5, 0x9e, 0x0b),
        NamedColor::BrightMagenta => Color::from_rgb8(0xc8, 0xb0, 0xe8),
        NamedColor::BrightCyan => Color::from_rgb8(0x6a, 0xc8, 0xd0),
        NamedColor::BrightWhite => Color::from_rgb8(0xe8, 0xe4, 0xe0),
        NamedColor::Foreground | NamedColor::BrightForeground => default,
        NamedColor::Background => Color::from_rgb8(0x13, 0x10, 0x10),
        NamedColor::Cursor => Color::from_rgb8(0xd9, 0x77, 0x06),
        NamedColor::DimBlack => Color::from_rgb8(0x3d, 0x38, 0x30),
        NamedColor::DimRed => Color::from_rgb8(0xbf, 0x61, 0x6a),
        NamedColor::DimGreen => Color::from_rgb8(0x4a, 0x99, 0x66),
        NamedColor::DimYellow => Color::from_rgb8(0xad, 0x94, 0x62),
        NamedColor::DimBlue => Color::from_rgb8(0xb4, 0x53, 0x09),
        NamedColor::DimMagenta => Color::from_rgb8(0x8b, 0x68, 0x9b),
        NamedColor::DimCyan => Color::from_rgb8(0x4a, 0x90, 0x98),
        NamedColor::DimWhite | NamedColor::DimForeground => {
            Color::from_rgb8(0x8a, 0x7e, 0x74)
        }
    }
}

/// v2 light ANSI palette (from colors.css light mode --term-*)
fn named_color_light(named: NamedColor, default: Color) -> Color {
    match named {
        NamedColor::Black => Color::from_rgb8(0xaa, 0xaa, 0xaa),
        NamedColor::Red => Color::from_rgb8(0xd4, 0x55, 0x55),
        NamedColor::Green => Color::from_rgb8(0x3d, 0xa8, 0x5e),
        NamedColor::Yellow => Color::from_rgb8(0xc4, 0x95, 0x2e),
        NamedColor::Blue => Color::from_rgb8(0xb4, 0x53, 0x09),
        NamedColor::Magenta => Color::from_rgb8(0x9b, 0x7f, 0xd0),
        NamedColor::Cyan => Color::from_rgb8(0x08, 0x91, 0xb2),
        NamedColor::White => Color::from_rgb8(0x3d, 0x3d, 0x3d),
        NamedColor::BrightBlack => Color::from_rgb8(0x88, 0x88, 0x88),
        NamedColor::BrightRed => Color::from_rgb8(0xe0, 0x70, 0x70),
        NamedColor::BrightGreen => Color::from_rgb8(0x50, 0xbb, 0x75),
        NamedColor::BrightYellow => Color::from_rgb8(0xd4, 0xa8, 0x53),
        NamedColor::BrightBlue => Color::from_rgb8(0xf5, 0x9e, 0x0b),
        NamedColor::BrightMagenta => Color::from_rgb8(0xb0, 0x90, 0xe0),
        NamedColor::BrightCyan => Color::from_rgb8(0x06, 0xb6, 0xd4),
        NamedColor::BrightWhite => Color::from_rgb8(0x1a, 0x1a, 0x1a),
        NamedColor::Foreground | NamedColor::BrightForeground => default,
        NamedColor::Background => Color::from_rgb8(0xfa, 0xf6, 0xf0),
        NamedColor::Cursor => Color::from_rgb8(0xd9, 0x77, 0x06),
        NamedColor::DimBlack => Color::from_rgb8(0xc0, 0xc0, 0xc0),
        NamedColor::DimRed => Color::from_rgb8(0xb0, 0x55, 0x55),
        NamedColor::DimGreen => Color::from_rgb8(0x30, 0x88, 0x50),
        NamedColor::DimYellow => Color::from_rgb8(0xa0, 0x80, 0x30),
        NamedColor::DimBlue => Color::from_rgb8(0x90, 0x45, 0x08),
        NamedColor::DimMagenta => Color::from_rgb8(0x80, 0x65, 0xa8),
        NamedColor::DimCyan => Color::from_rgb8(0x06, 0x70, 0x88),
        NamedColor::DimWhite | NamedColor::DimForeground => {
            Color::from_rgb8(0x66, 0x66, 0x66)
        }
    }
}
