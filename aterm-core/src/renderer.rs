//! wgpu + glyphon terminal grid renderer.

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{cell::Cell, cell::Flags, Term};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};

const DEFAULT_FG: GlyphonColor = GlyphonColor::rgb(0xe8, 0xe4, 0xe0);
const DEFAULT_BG: GlyphonColor = GlyphonColor::rgb(0x00, 0x00, 0x00);
const CURSOR_FG: GlyphonColor = GlyphonColor::rgb(0xff, 0xff, 0xff);

// --- wgpu colored-rectangle pipeline for selection/background rendering ---

const RECT_SHADER: &str = "
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
";

#[repr(C)]
#[derive(Copy, Clone)]
struct RectVertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    vertices: Vec<RectVertex>,
}

impl RectRenderer {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RectVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self { pipeline, vertices: Vec::new() }
    }

    fn clear(&mut self) {
        self.vertices.clear();
    }

    fn push_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        vp_w: f32,
        vp_h: f32,
        color: [f32; 4],
    ) {
        let x0 = x / vp_w * 2.0 - 1.0;
        let y0 = 1.0 - y / vp_h * 2.0;
        let x1 = (x + w) / vp_w * 2.0 - 1.0;
        let y1 = 1.0 - (y + h) / vp_h * 2.0;
        self.vertices.extend_from_slice(&[
            RectVertex { position: [x0, y0], color },
            RectVertex { position: [x1, y0], color },
            RectVertex { position: [x0, y1], color },
            RectVertex { position: [x1, y0], color },
            RectVertex { position: [x1, y1], color },
            RectVertex { position: [x0, y1], color },
        ]);
    }

    fn prepare(&self, device: &wgpu::Device) -> Option<wgpu::Buffer> {
        if self.vertices.is_empty() {
            return None;
        }

        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                self.vertices.as_ptr() as *const u8,
                self.vertices.len() * std::mem::size_of::<RectVertex>(),
            )
        };

        Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect_vertices"),
            contents: bytes,
            usage: wgpu::BufferUsages::VERTEX,
        }))
    }
}

// --- Terminal grid renderer ---

pub struct TerminalGridRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    font_size: f32,
    line_height: f32,
    cells: Vec<RenderCell>,
    cursor_buffer: Buffer,
    cursor_text: String,
    cursor_width_cells: u8,
    rect_renderer: RectRenderer,
    last_font_size: f32,
    last_line_height: f32,
}

struct RenderCell {
    buffer: Buffer,
    text: String,
    fg: GlyphonColor,
    width_cells: u8,
    has_non_ascii: bool,
    active: bool,
}

impl TerminalGridRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        scale_factor: f32,
    ) -> Self {
        let font_size = 13.0 * scale_factor;
        let line_height = 19.0 * scale_factor;
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let rect_renderer = RectRenderer::new(device, format);

        let mut cursor_buffer =
            Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
        cursor_buffer.set_size(
            &mut font_system,
            Some(font_size * 0.6),
            Some(line_height),
        );
        cursor_buffer.set_monospace_width(&mut font_system, Some(font_size * 0.6));

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            font_size,
            line_height,
            cells: Vec::new(),
            cursor_buffer,
            cursor_text: String::new(),
            cursor_width_cells: 0,
            rect_renderer,
            last_font_size: -1.0,
            last_line_height: -1.0,
        }
    }

    pub fn cell_width(&self) -> f32 {
        self.font_size * 0.6
    }

    pub fn cell_height(&self) -> f32 {
        self.line_height
    }

    pub fn grid_size(&self, width: f32, height: f32) -> (u16, u16) {
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

        let (cols_u16, rows_u16) = self.grid_size(width as f32, height as f32);
        let cols = cols_u16 as usize;
        let rows = rows_u16 as usize;
        self.ensure_cell_buffers(cols, rows);

        for cell in &mut self.cells {
            cell.active = false;
        }

        let content = term.renderable_content();
        let selection = &content.selection;
        let cursor = &content.cursor;
        let cursor_visible = cursor.shape != CursorShape::Hidden;
        let cursor_line = cursor.point.line.0;
        let cursor_col = cursor.point.column.0 as usize;

        let cw = self.cell_width();
        let lh = self.line_height;
        let w_f = width as f32;
        let h_f = height as f32;

        self.rect_renderer.clear();

        let mut current_line: Option<i32> = None;
        let mut row_index = 0usize;
        let mut cursor_row: Option<usize> = None;
        let mut cursor_width_cells = 1u8;
        let mut visible_non_ascii = false;
        let mut changed_cells = 0usize;
        let mut active_cells = 0usize;

        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            let col = indexed.point.column.0 as usize;

            if current_line != Some(line) {
                if current_line.is_some() {
                    row_index += 1;
                }
                current_line = Some(line);
            }

            if row_index >= rows || col >= cols {
                continue;
            }

            if cursor_visible && line == cursor_line {
                cursor_row = Some(row_index);
            }

            let point = alacritty_terminal::index::Point::new(
                alacritty_terminal::index::Line(line),
                alacritty_terminal::index::Column(col),
            );
            let cell = indexed.cell;
            let is_selected = selection.as_ref().map_or(false, |sel| sel.contains(point));

            if let Some(bg) = cell_background_rgba(cell, is_selected) {
                self.rect_renderer.push_rect(
                    4.0 + col as f32 * cw,
                    4.0 + row_index as f32 * lh,
                    cw,
                    lh,
                    w_f,
                    h_f,
                    bg,
                );
            }

            if cursor_visible && line == cursor_line && col == cursor_col {
                cursor_width_cells = if cell.flags.contains(Flags::WIDE_CHAR) && col + 1 < cols {
                    2
                } else {
                    1
                };
            }

            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }

            let text = cell_text(cell);
            if text.is_empty() {
                continue;
            }

            let fg = cell_foreground(cell, is_selected);
            let width_cells = if cell.flags.contains(Flags::WIDE_CHAR) && col + 1 < cols {
                2
            } else {
                1
            };
            visible_non_ascii |= !text.is_ascii();

            let slot = row_index * cols + col;
            if self.update_cell(slot, &text, fg, width_cells) {
                changed_cells += 1;
            }
            active_cells += 1;
        }

        let rect_buffer = self.rect_renderer.prepare(device);

        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };

        if cursor_visible {
            self.update_cursor_buffer(cursor_width_cells);
        }

        let mut text_areas = Vec::with_capacity(active_cells + usize::from(cursor_visible));
        for row in 0..rows {
            for col in 0..cols {
                let cell = &self.cells[row * cols + col];
                if !cell.active {
                    continue;
                }

                text_areas.push(TextArea {
                    buffer: &cell.buffer,
                    left: 4.0 + col as f32 * cw,
                    top: 4.0 + row as f32 * lh,
                    scale: 1.0,
                    bounds,
                    default_color: DEFAULT_FG,
                    custom_glyphs: &[],
                });
            }
        }

        if cursor_visible {
            if let Some(cursor_row) = cursor_row {
                text_areas.push(TextArea {
                    buffer: &self.cursor_buffer,
                    left: 4.0 + cursor_col as f32 * cw,
                    top: 4.0 + cursor_row as f32 * lh,
                    scale: 1.0,
                    bounds,
                    default_color: CURSOR_FG,
                    custom_glyphs: &[],
                });
            }
        }

        let shape_elapsed = render_start.elapsed();

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
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if let Some(ref buf) = rect_buffer {
                pass.set_pipeline(&self.rect_renderer.pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.rect_renderer.vertices.len() as u32, 0..1);
            }

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        queue.submit(std::iter::once(encoder.finish()));

        let total_elapsed = render_start.elapsed();
        if total_elapsed.as_millis() > 16 {
            eprintln!(
                "[perf] render: cells_changed={} visible_non_ascii={} active_cells={} shape={}ms total={}ms",
                changed_cells,
                visible_non_ascii,
                active_cells,
                shape_elapsed.as_millis(),
                total_elapsed.as_millis()
            );
        }
    }

    fn ensure_cell_buffers(&mut self, cols: usize, rows: usize) {
        let target = cols * rows;
        let cw = self.cell_width();
        let metrics = Metrics::new(self.font_size, self.line_height);

        while self.cells.len() < target {
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(&mut self.font_system, Some(cw), Some(self.line_height));
            buffer.set_monospace_width(&mut self.font_system, Some(cw));
            self.cells.push(RenderCell {
                buffer,
                text: String::new(),
                fg: DEFAULT_FG,
                width_cells: 1,
                has_non_ascii: false,
                active: false,
            });
        }
        self.cells.truncate(target);

        if self.font_size != self.last_font_size || self.line_height != self.last_line_height {
            for cell in &mut self.cells {
                cell.buffer.set_metrics(&mut self.font_system, metrics);
                cell.buffer
                    .set_size(&mut self.font_system, Some(cw), Some(self.line_height));
                cell.buffer
                    .set_monospace_width(&mut self.font_system, Some(cw));
            }

            self.cursor_buffer.set_metrics(&mut self.font_system, metrics);
            self.cursor_buffer
                .set_size(&mut self.font_system, Some(cw), Some(self.line_height));
            self.cursor_buffer
                .set_monospace_width(&mut self.font_system, Some(cw));

            self.last_font_size = self.font_size;
            self.last_line_height = self.line_height;
            self.cursor_width_cells = 0;
        }
    }

    fn update_cell(
        &mut self,
        slot: usize,
        text: &str,
        fg: GlyphonColor,
        width_cells: u8,
    ) -> bool {
        let cw = self.cell_width();
        let width = cw * width_cells as f32;
        let metrics = Metrics::new(self.font_size, self.line_height);
        let shaping = if text.is_ascii() {
            Shaping::Basic
        } else {
            Shaping::Advanced
        };
        let cell = &mut self.cells[slot];
        cell.active = true;

        if cell.text == text && cell.fg == fg && cell.width_cells == width_cells {
            return false;
        }

        cell.buffer.set_metrics(&mut self.font_system, metrics);
        cell.buffer
            .set_size(&mut self.font_system, Some(width), Some(self.line_height));
        cell.buffer
            .set_monospace_width(&mut self.font_system, Some(cw));
        cell.buffer.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::Monospace).color(fg),
            shaping,
        );

        cell.text.clear();
        cell.text.push_str(text);
        cell.fg = fg;
        cell.width_cells = width_cells;
        cell.has_non_ascii = !text.is_ascii();
        true
    }

    fn update_cursor_buffer(&mut self, width_cells: u8) {
        let width_cells = width_cells.max(1);
        let cw = self.cell_width();
        let width = cw * width_cells as f32;
        let text = if width_cells == 2 { "██" } else { "█" };

        self.cursor_buffer.set_metrics(
            &mut self.font_system,
            Metrics::new(self.font_size, self.line_height),
        );
        self.cursor_buffer
            .set_size(&mut self.font_system, Some(width), Some(self.line_height));
        self.cursor_buffer
            .set_monospace_width(&mut self.font_system, Some(cw));

        if self.cursor_width_cells != width_cells || self.cursor_text != text {
            self.cursor_buffer.set_text(
                &mut self.font_system,
                text,
                Attrs::new()
                    .family(Family::Monospace)
                    .color(CURSOR_FG),
                Shaping::Basic,
            );
            self.cursor_text.clear();
            self.cursor_text.push_str(text);
            self.cursor_width_cells = width_cells;
        }
    }
}

fn cell_text(cell: &Cell) -> String {
    if cell.flags.contains(Flags::HIDDEN) {
        return String::new();
    }

    let mut text = String::new();
    match cell.c {
        '\0' | ' ' => {}
        '\t' => text.push(' '),
        c => text.push(c),
    }

    if let Some(zerowidth) = cell.zerowidth() {
        if !zerowidth.is_empty() && text.is_empty() {
            text.push(' ');
        }
        for c in zerowidth {
            text.push(*c);
        }
    }

    text
}

fn cell_foreground(cell: &Cell, is_selected: bool) -> GlyphonColor {
    if is_selected {
        return CURSOR_FG;
    }

    if cell.flags.contains(Flags::INVERSE) {
        return ansi_to_glyphon(&cell.bg);
    }

    ansi_to_glyphon(&cell.fg)
}

fn cell_background_rgba(cell: &Cell, is_selected: bool) -> Option<[f32; 4]> {
    if is_selected {
        return Some([0.2, 0.4, 0.8, 0.7]);
    }

    if cell.flags.contains(Flags::INVERSE) {
        return Some(color_to_rect_rgba(ansi_to_glyphon(&cell.fg)));
    }

    // Force a pure black terminal background for all normal cells. This avoids
    // CLI-specific ANSI background fills (for example Gemini's gray panel) from
    // overriding the app-level black canvas. Selection and inverse are handled
    // above as explicit overlays.
    let _ = cell;
    None
}

fn color_to_rect_rgba(color: GlyphonColor) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    ]
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
        NamedColor::Background => DEFAULT_BG,
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
