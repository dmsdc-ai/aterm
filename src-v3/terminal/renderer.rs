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
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            font_size: Pixels(14.0),
            line_height: 1.25,
            foreground: Color::from_rgb8(230, 230, 230),
            background: Color::from_rgb8(13, 17, 23),
        }
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

            let mut foreground = resolve_color(cell.fg, default_foreground, content.colors);
            let mut background = resolve_color(cell.bg, default_background, content.colors);

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
) -> Color {
    match color {
        AnsiColor::Named(named) => colors[named]
            .map(rgb_to_color)
            .unwrap_or_else(|| named_color_fallback(named, fallback)),
        AnsiColor::Spec(rgb) => rgb_to_color(rgb),
        AnsiColor::Indexed(index) => colors[index as usize]
            .map(rgb_to_color)
            .unwrap_or(fallback),
    }
}

fn named_color_fallback(named: NamedColor, default: Color) -> Color {
    match named {
        NamedColor::Black => Color::from_rgb8(12, 16, 21),
        NamedColor::Red => Color::from_rgb8(255, 123, 114),
        NamedColor::Green => Color::from_rgb8(63, 185, 80),
        NamedColor::Yellow => Color::from_rgb8(210, 153, 34),
        NamedColor::Blue => Color::from_rgb8(88, 166, 255),
        NamedColor::Magenta => Color::from_rgb8(188, 140, 255),
        NamedColor::Cyan => Color::from_rgb8(57, 211, 209),
        NamedColor::White => Color::from_rgb8(201, 209, 217),
        NamedColor::BrightBlack => Color::from_rgb8(110, 118, 129),
        NamedColor::BrightRed => Color::from_rgb8(255, 166, 158),
        NamedColor::BrightGreen => Color::from_rgb8(86, 211, 100),
        NamedColor::BrightYellow => Color::from_rgb8(229, 196, 89),
        NamedColor::BrightBlue => Color::from_rgb8(121, 192, 255),
        NamedColor::BrightMagenta => Color::from_rgb8(210, 168, 255),
        NamedColor::BrightCyan => Color::from_rgb8(86, 240, 242),
        NamedColor::BrightWhite => Color::from_rgb8(240, 246, 252),
        NamedColor::Foreground | NamedColor::BrightForeground => default,
        NamedColor::Background => Color::from_rgb8(13, 17, 23),
        NamedColor::Cursor => Color::from_rgb8(201, 209, 217),
        NamedColor::DimBlack => Color::from_rgb8(72, 78, 86),
        NamedColor::DimRed => Color::from_rgb8(191, 97, 106),
        NamedColor::DimGreen => Color::from_rgb8(102, 153, 102),
        NamedColor::DimYellow => Color::from_rgb8(173, 148, 98),
        NamedColor::DimBlue => Color::from_rgb8(102, 153, 204),
        NamedColor::DimMagenta => Color::from_rgb8(139, 104, 155),
        NamedColor::DimCyan => Color::from_rgb8(106, 171, 171),
        NamedColor::DimWhite | NamedColor::DimForeground => {
            Color::from_rgb8(145, 152, 161)
        }
    }
}
