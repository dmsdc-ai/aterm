//! wgpu + glyphon terminal grid renderer.

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{cell::Flags, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};

const DEFAULT_FG: GlyphonColor = GlyphonColor::rgb(0xe8, 0xe4, 0xe0);

// --- wgpu colored-rectangle pipeline for selection/cursor backgrounds ---

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

    /// Push a colored rect in pixel coordinates. Converts to clip space internally.
    fn push_rect(&mut self, x: f32, y: f32, w: f32, h: f32, vp_w: f32, vp_h: f32, color: [f32; 4]) {
        let x0 = x / vp_w * 2.0 - 1.0;
        let y0 = 1.0 - y / vp_h * 2.0;
        let x1 = (x + w) / vp_w * 2.0 - 1.0;
        let y1 = 1.0 - (y + h) / vp_h * 2.0;
        let c = color;
        self.vertices.extend_from_slice(&[
            RectVertex { position: [x0, y0], color: c },
            RectVertex { position: [x1, y0], color: c },
            RectVertex { position: [x0, y1], color: c },
            RectVertex { position: [x1, y0], color: c },
            RectVertex { position: [x1, y1], color: c },
            RectVertex { position: [x0, y1], color: c },
        ]);
    }

    /// Create a GPU buffer from collected vertices. Call BEFORE begin_render_pass.
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
    scale_factor: f32,
    lines: Vec<RenderLine>,
    rect_renderer: RectRenderer,
    scratch_spans: Vec<(String, GlyphonColor)>,
    last_width: f32,
    last_font_size: f32,
    last_alt_screen: bool,
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
        let rect_renderer = RectRenderer::new(device, format);

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
            rect_renderer,
            scratch_spans: Vec::with_capacity(256),
            last_width: -1.0,
            last_font_size: -1.0,
            last_alt_screen: false,
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

        // --- Screen mode change detection (Ghostty pattern) ---
        let alt_screen = term.mode().contains(TermMode::ALT_SCREEN);
        if alt_screen != self.last_alt_screen {
            for line in &mut self.lines {
                line.spans.clear();
            }
            self.last_alt_screen = alt_screen;
        }

        let content = term.renderable_content();
        let selection = &content.selection;

        let cursor = &content.cursor;
        let cursor_visible = cursor.shape != CursorShape::Hidden;
        let cursor_line = cursor.point.line.0;
        let cursor_col = cursor.point.column.0;

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
                // Do not synthesize a spacer glyph here. The wide character itself
                // should occupy the visual width; pushing an extra space creates
                // visible gaps for Korean text.
                current_col += 1;
                continue;
            }

            has_non_ascii |= !cell.c.is_ascii();

            // Quick-fix cursor path: render a solid block glyph instead of relying
            // on the background rect, which is currently unreliable.
            let at_cursor = cursor_visible && line == cursor_line && col == cursor_col;

            // Check if cell is selected
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

            // Determine foreground color with priority: cursor > selection > inverse > normal
            let fg = if at_cursor {
                GlyphonColor::rgb(0xff, 0xff, 0xff)
            } else if is_selected {
                GlyphonColor::rgb(0xff, 0xff, 0xff)
            } else if cell.flags.contains(Flags::INVERSE) {
                GlyphonColor::rgb(0x13, 0x10, 0x10)
            } else {
                ansi_to_glyphon(&cell.fg)
            };

            if fg != current_color && !current_text.is_empty() {
                self.scratch_spans
                    .push((std::mem::take(&mut current_text), current_color));
            }
            current_color = fg;
            let ch = if at_cursor {
                '\u{2588}'
            } else if cell.c == '\0' {
                ' '
            } else {
                cell.c
            };
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

        // --- Build background rects (selection only) ---
        let cw = self.cell_width();
        let lh = self.line_height;
        let w_f = width as f32;
        let h_f = height as f32;
        self.rect_renderer.clear();

        for (idx, line_sel) in line_selections.iter().enumerate() {
            for (c, &selected) in line_sel.iter().enumerate() {
                if selected {
                    self.rect_renderer.push_rect(
                        4.0 + c as f32 * cw,
                        4.0 + idx as f32 * lh,
                        cw, lh, w_f, h_f,
                        [0.2, 0.4, 0.8, 0.7],
                    );
                }
            }
        }

        let rect_buffer = self.rect_renderer.prepare(device);

        let shape_elapsed = render_start.elapsed();

        // Build text areas — foreground only
        let text_areas: Vec<TextArea<'_>> = self.lines.iter().enumerate().map(|(idx, line)| {
            TextArea {
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
            }
        }).collect();

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

            // Background rects (selection) drawn FIRST
            if let Some(ref buf) = rect_buffer {
                pass.set_pipeline(&self.rect_renderer.pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.rect_renderer.vertices.len() as u32, 0..1);
            }

            // Text on top
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
        let cw = self.cell_width();
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

        // Keep fixed cell advances for ASCII alignment. The quick wide-char fix
        // avoids injecting synthetic spacer spaces, so Korean no longer picks
        // up an extra visible gap per wide glyph.
        line.buffer.set_monospace_width(&mut self.font_system, Some(cw));

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
