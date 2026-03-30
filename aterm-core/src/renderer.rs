//! wgpu + glyphon terminal grid renderer.

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{cell::Flags, Term};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};

const DEFAULT_FG: GlyphonColor = GlyphonColor::rgb(0xe8, 0xe4, 0xe0);

pub struct TerminalGridRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    font_size: f32,
    line_height: f32,
    scale_factor: f32,
    lines: Vec<RenderLine>,
    sel_bg_lines: Vec<Option<Buffer>>,
    scratch_spans: Vec<(String, GlyphonColor)>,
    last_width: f32,
    last_font_size: f32,
}

struct RenderLine {
    buffer: Buffer,
    spans: Vec<(String, GlyphonColor)>,
    has_non_ascii: bool,
}

impl TerminalGridRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, scale_factor: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            font_size: 13.0 * scale_factor,
            line_height: 19.0 * scale_factor,
            scale_factor,
            lines: Vec::new(),
            sel_bg_lines: Vec::new(),
            scratch_spans: Vec::with_capacity(256),
            last_width: -1.0,
            last_font_size: -1.0,
        }
    }

    pub fn cell_width(&self) -> f32 {
        self.font_size * 0.6
    }

    pub fn cell_height(&self) -> f32 {
        self.line_height
    }

    pub fn grid_size(&self, width: f32, height: f32) -> (u16, u16) {
        // Subtract padding (4px left + 4px right, 4px top + 4px bottom)
        let usable_w = (width - 8.0).max(0.0);
        let usable_h = (height - 8.0).max(0.0);
        let cols = (usable_w / self.cell_width()).floor().max(2.0) as u16;
        let rows = (usable_h / self.cell_height()).floor().max(1.0) as u16;
        (cols, rows)
    }

    pub fn render<T: EventListener>(
        &mut self,
        term: &Term<T>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        let render_start = std::time::Instant::now();
        self.viewport.update(queue, Resolution { width, height });
        let rows = self.grid_size(width as f32, height as f32).1 as usize;
        self.ensure_line_buffers(width as f32, rows);

        let content = term.renderable_content();
        let selection = &content.selection;

        // Cursor position for block cursor rendering.
        let cursor = &content.cursor;
        let cursor_visible = cursor.shape != CursorShape::Hidden;
        let cursor_line = cursor.point.line.0;
        let cursor_col = cursor.point.column.0;

        // Track selected cells for background highlight
        let cols_count = self.grid_size(width as f32, height as f32).0 as usize;
        let mut line_selections: Vec<Vec<bool>> = Vec::new();

        let mut current_text = String::new();
        let mut current_color = DEFAULT_FG;
        let mut current_line: Option<i32> = None;
        let mut current_col: usize = 0;
        let mut has_non_ascii = false;
        let mut visible_non_ascii = false;
        let mut line_index = 0usize;
        let mut changed_lines = 0usize;

        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            let col = indexed.point.column.0;

            if current_line != Some(line) {
                if current_line.is_some() {
                    changed_lines += self.finish_line(
                        line_index,
                        &mut current_text,
                        current_color,
                        has_non_ascii,
                        width as f32,
                    ) as usize;
                    visible_non_ascii |= has_non_ascii;
                    line_index += 1;
                }
                current_line = Some(line);
                current_col = 0;
                current_color = DEFAULT_FG;
                has_non_ascii = false;
                // Ensure line_selections has an entry for this line
                while line_selections.len() <= line_index {
                    line_selections.push(vec![false; cols_count]);
                }
            }

            while current_col < col {
                current_text.push(' ');
                current_col += 1;
            }

            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                current_col += 1;
                continue;
            }

            has_non_ascii |= !cell.c.is_ascii();

            // Cursor: bright white block at cursor position — always visible.
            let at_cursor = cursor_visible && line == cursor_line && col == cursor_col;
            if at_cursor {
                let cursor_color = GlyphonColor::rgb(0xff, 0xff, 0xff);
                if cursor_color != current_color && !current_text.is_empty() {
                    self.scratch_spans
                        .push((std::mem::take(&mut current_text), current_color));
                }
                current_color = cursor_color;
                current_text.push('\u{2588}');
                current_col += 1;
                continue;
            }

            // Check if cell is selected — swap fg/bg (highlight)
            let is_selected = selection.as_ref().map_or(false, |sel| {
                let point = alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    alacritty_terminal::index::Column(col),
                );
                sel.contains(point)
            });
            if is_selected && line_index < line_selections.len() && col < cols_count {
                line_selections[line_index][col] = true;
            }
            let fg = if is_selected {
                // Selected: white text on blue background (rendered via color swap)
                GlyphonColor::rgb(0xff, 0xff, 0xff)
            } else if cell.flags.contains(Flags::INVERSE) {
                // INVERSE flag: use background color as foreground
                GlyphonColor::rgb(0x13, 0x10, 0x10)
            } else {
                ansi_to_glyphon(&cell.fg)
            };
            if fg != current_color && !current_text.is_empty() {
                self.scratch_spans
                    .push((std::mem::take(&mut current_text), current_color));
            }
            current_color = fg;
            // Treat null bytes as spaces to avoid rendering artifacts
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            current_text.push(ch);
            current_col += 1;
        }

        if current_line.is_some() && line_index < self.lines.len() {
            changed_lines += self.finish_line(
                line_index,
                &mut current_text,
                current_color,
                has_non_ascii,
                width as f32,
            ) as usize;
            visible_non_ascii |= has_non_ascii;
            line_index += 1;
        }

        for idx in line_index..self.lines.len() {
            changed_lines += self.update_line(idx, false, width as f32) as usize;
        }

        // --- Selection background buffers ---
        // Ensure sel_bg_lines has correct capacity
        while self.sel_bg_lines.len() < self.lines.len() {
            self.sel_bg_lines.push(None);
        }
        self.sel_bg_lines.truncate(self.lines.len());

        let sel_color = GlyphonColor::rgb(0x33, 0x66, 0xCC);
        let cursor_bg_color = GlyphonColor::rgb(0xff, 0xff, 0xff); // bright white cursor block
        let cw = self.cell_width();
        let metrics = Metrics::new(self.font_size, self.line_height);
        let w_f32 = width as f32;
        let lh = self.line_height;

        for idx in 0..self.lines.len() {
            let selected_cols = if idx < line_selections.len() { &line_selections[idx] } else { &vec![] as &Vec<bool> };
            let has_selection = selected_cols.iter().any(|&s| s);
            // Check if cursor is on this line
            let cursor_on_line = cursor_visible && idx == cursor_line as usize && (cursor_line as usize) < rows;

            if has_selection || cursor_on_line {
                if self.sel_bg_lines[idx].is_none() {
                    let mut buf = Buffer::new(&mut self.font_system, metrics);
                    buf.set_size(&mut self.font_system, Some(w_f32), Some(lh));
                    buf.set_monospace_width(&mut self.font_system, Some(cw));
                    self.sel_bg_lines[idx] = Some(buf);
                }

                // Build background: selection blue for selected, cursor color for cursor, space for rest
                let bg_spans: Vec<(String, GlyphonColor)> = (0..cols_count)
                    .map(|c| {
                        let is_sel = c < selected_cols.len() && selected_cols[c];
                        let is_cur = cursor_on_line && c == cursor_col;
                        if is_sel {
                            ("\u{2588}".to_string(), sel_color)
                        } else if is_cur {
                            ("\u{2588}".to_string(), cursor_bg_color)
                        } else {
                            (" ".to_string(), GlyphonColor::rgba(0, 0, 0, 0))
                        }
                    })
                    .collect();

                let rich: Vec<(&str, Attrs)> = bg_spans
                    .iter()
                    .map(|(text, color)| {
                        (text.as_str(), Attrs::new().family(Family::Monospace).color(*color))
                    })
                    .collect();

                let bg_buf = self.sel_bg_lines[idx].as_mut().unwrap();
                bg_buf.set_metrics(&mut self.font_system, metrics);
                bg_buf.set_size(&mut self.font_system, Some(w_f32), Some(lh));
                bg_buf.set_monospace_width(&mut self.font_system, Some(cw));
                bg_buf.set_rich_text(
                    &mut self.font_system,
                    rich,
                    Attrs::new().family(Family::Monospace).color(sel_color),
                    Shaping::Basic,
                );
                bg_buf.shape_until_scroll(&mut self.font_system, false);
            } else {
                self.sel_bg_lines[idx] = None;
            }
        }
        // Clear background buffers for lines beyond line_selections
        for idx in line_selections.len()..self.sel_bg_lines.len() {
            self.sel_bg_lines[idx] = None;
        }

        let shape_elapsed = render_start.elapsed();

        // Build text_areas: background layer (selection) FIRST, then foreground (text)
        let mut text_areas: Vec<TextArea<'_>> = Vec::new();

        // Background layer (selection highlights)
        for (idx, bg) in self.sel_bg_lines.iter().enumerate() {
            if let Some(ref buffer) = bg {
                text_areas.push(TextArea {
                    buffer,
                    left: 4.0,
                    top: 4.0 + idx as f32 * self.line_height,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: width as i32,
                        bottom: height as i32,
                    },
                    default_color: sel_color,
                    custom_glyphs: &[],
                });
            }
        }

        // Foreground layer (text)
        for (idx, line) in self.lines.iter().enumerate() {
            text_areas.push(TextArea {
                buffer: &line.buffer,
                left: 4.0,
                top: 4.0 + idx as f32 * self.line_height,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: DEFAULT_FG,
                custom_glyphs: &[],
            });
        }

        self.text_renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .unwrap();

        // Single render pass: clear background + render text
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("terminal_render"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terminal"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.075,
                            g: 0.063,
                            b: 0.063,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        queue.submit(std::iter::once(encoder.finish()));
        let total_elapsed = render_start.elapsed();
        if total_elapsed.as_millis() > 16 {
            eprintln!(
                "[perf] render: lines_changed={} visible_non_ascii={} font={} shape={}ms total={}ms",
                changed_lines,
                visible_non_ascii,
                self.font_size,
                shape_elapsed.as_millis(),
                total_elapsed.as_millis()
            );
        }
    }

    fn ensure_line_buffers(&mut self, width: f32, rows: usize) {
        let cw = self.cell_width();

        while self.lines.len() < rows {
            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(self.font_size, self.line_height),
            );
            buffer.set_size(&mut self.font_system, Some(width), Some(self.line_height));
            buffer.set_monospace_width(&mut self.font_system, Some(cw));
            self.lines.push(RenderLine {
                buffer,
                spans: Vec::new(),
                has_non_ascii: false,
            });
        }

        self.lines.truncate(rows);

        if width != self.last_width || self.font_size != self.last_font_size {
            for line in &mut self.lines {
                line.buffer.set_metrics(
                    &mut self.font_system,
                    Metrics::new(self.font_size, self.line_height),
                );
                line.buffer
                    .set_size(&mut self.font_system, Some(width), Some(self.line_height));
                line.buffer
                    .set_monospace_width(&mut self.font_system, Some(cw));
            }
            self.last_width = width;
            self.last_font_size = self.font_size;
        }
    }

    fn finish_line(
        &mut self,
        line_index: usize,
        current_text: &mut String,
        current_color: GlyphonColor,
        has_non_ascii: bool,
        width: f32,
    ) -> bool {
        if !current_text.is_empty() {
            self.scratch_spans
                .push((std::mem::take(current_text), current_color));
        }
        let changed = self.update_line(line_index, has_non_ascii, width);
        self.scratch_spans.clear();
        changed
    }

    fn update_line(&mut self, line_index: usize, has_non_ascii: bool, width: f32) -> bool {
        let line = &mut self.lines[line_index];
        if line.has_non_ascii == has_non_ascii && line.spans == self.scratch_spans {
            return false;
        }

        line.has_non_ascii = has_non_ascii;
        line.spans.clear();
        line.spans.extend(self.scratch_spans.iter().cloned());

        let rich: Vec<(&str, Attrs)> = line
            .spans
            .iter()
            .map(|(text, color)| {
                (
                    text.as_str(),
                    Attrs::new().family(Family::Monospace).color(*color),
                )
            })
            .collect();
        let shaping = if has_non_ascii { Shaping::Advanced } else { Shaping::Basic };

        line.buffer.set_metrics(
            &mut self.font_system,
            Metrics::new(self.font_size, self.line_height),
        );
        line.buffer
            .set_size(&mut self.font_system, Some(width), Some(self.line_height));
        line.buffer.set_rich_text(
            &mut self.font_system,
            rich,
            Attrs::new().family(Family::Monospace).color(DEFAULT_FG),
            shaping,
        );
        line.buffer.shape_until_scroll(&mut self.font_system, false);
        true
    }
}

fn ansi_to_glyphon(color: &AnsiColor) -> GlyphonColor {
    match color {
        AnsiColor::Named(named) => named_to_glyphon(*named),
        AnsiColor::Spec(rgb) => GlyphonColor::rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => indexed_to_glyphon(*idx),
    }
}

fn named_to_glyphon(named: NamedColor) -> GlyphonColor {
    match named {
        NamedColor::Black => GlyphonColor::rgb(0x1a, 0x1a, 0x2e),
        NamedColor::Red => GlyphonColor::rgb(0xcc, 0x33, 0x33),
        NamedColor::Green => GlyphonColor::rgb(0x4e, 0x9a, 0x06),
        NamedColor::Yellow => GlyphonColor::rgb(0xc4, 0xa0, 0x00),
        NamedColor::Blue => GlyphonColor::rgb(0x34, 0x65, 0xa4),
        NamedColor::Magenta => GlyphonColor::rgb(0x75, 0x50, 0x7b),
        NamedColor::Cyan => GlyphonColor::rgb(0x06, 0x98, 0x9a),
        NamedColor::White => GlyphonColor::rgb(0xd3, 0xd7, 0xcf),
        NamedColor::BrightBlack => GlyphonColor::rgb(0x55, 0x57, 0x53),
        NamedColor::BrightRed => GlyphonColor::rgb(0xef, 0x29, 0x29),
        NamedColor::BrightGreen => GlyphonColor::rgb(0x8a, 0xe2, 0x34),
        NamedColor::BrightYellow => GlyphonColor::rgb(0xfc, 0xe9, 0x4f),
        NamedColor::BrightBlue => GlyphonColor::rgb(0x72, 0x9f, 0xcf),
        NamedColor::BrightMagenta => GlyphonColor::rgb(0xad, 0x7f, 0xa8),
        NamedColor::BrightCyan => GlyphonColor::rgb(0x34, 0xe2, 0xe2),
        NamedColor::BrightWhite => GlyphonColor::rgb(0xee, 0xee, 0xec),
        NamedColor::Foreground => DEFAULT_FG,
        _ => DEFAULT_FG,
    }
}

fn indexed_to_glyphon(idx: u8) -> GlyphonColor {
    if idx < 16 {
        return named_to_glyphon(match idx {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            15 => NamedColor::BrightWhite,
            _ => unreachable!(),
        });
    }

    if idx < 232 {
        let i = idx - 16;
        let r = if i / 36 > 0 { (i / 36) * 40 + 55 } else { 0 };
        let g = if (i % 36) / 6 > 0 { ((i % 36) / 6) * 40 + 55 } else { 0 };
        let b = if i % 6 > 0 { (i % 6) * 40 + 55 } else { 0 };
        return GlyphonColor::rgb(r, g, b);
    }

    let v = (idx - 232) * 10 + 8;
    GlyphonColor::rgb(v, v, v)
}
