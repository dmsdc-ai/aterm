//! wgpu + glyphon terminal grid renderer.

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{cell::Cell, cell::Flags, Term};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};

const TERMINAL_FONT_SIZE_PX: f32 = 18.0;
const TERMINAL_LINE_HEIGHT_PX: f32 = 26.0;
const NERD_FONT_FAMILY_PREFERENCES: &[&str] = &[
    "JetBrainsMono Nerd Font Mono",
    "JetBrainsMono Nerd Font",
    "MesloLGS NF",
    "MesloLGS Nerd Font Mono",
    "MesloLGS Nerd Font",
    "Hack Nerd Font Mono",
    "Hack Nerd Font",
    "FiraCode Nerd Font Mono",
    "FiraCode Nerd Font",
    "SauceCodePro Nerd Font Mono",
    "SauceCodePro Nerd Font",
    "CaskaydiaMono Nerd Font Mono",
    "CaskaydiaMono Nerd Font",
    "Symbols Nerd Font Mono",
    "Symbols Nerd Font",
];
#[cfg(target_os = "macos")]
const SYSTEM_MONOSPACE_FAMILY_PREFERENCES: &[&str] = &["SF Mono", "Menlo", "Monaco", "Courier New"];
#[cfg(not(target_os = "macos"))]
const SYSTEM_MONOSPACE_FAMILY_PREFERENCES: &[&str] = &[
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Courier New",
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TerminalThemeMode {
    Dark,
    Light,
}

/// Named color schemes — each provides a full 16-color ANSI palette + bg/fg/cursor.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ColorScheme {
    Dark = 0,
    Light = 1,
    SolarizedDark = 2,
    SolarizedLight = 3,
    Monokai = 4,
    Dracula = 5,
    Nord = 6,
    TokyoNight = 7,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ThemeRgb {
    r: u8,
    g: u8,
    b: u8,
}

impl ThemeRgb {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Copy, Clone)]
struct ThemePalette {
    background: ThemeRgb,
    foreground: ThemeRgb,
    cursor: ThemeRgb,
    selection_fg: ThemeRgb,
    selection_bg: [f32; 4],
    ansi: [ThemeRgb; 16],
}

const DARK_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x48, 0x4f, 0x58),
    ThemeRgb::new(0xf8, 0x53, 0x49),
    ThemeRgb::new(0x3f, 0xb9, 0x50),
    ThemeRgb::new(0xd4, 0xa5, 0x74),
    ThemeRgb::new(0x8b, 0x5c, 0xf6),
    ThemeRgb::new(0xec, 0x48, 0x99),
    ThemeRgb::new(0x06, 0xb6, 0xd4),
    ThemeRgb::new(0xb1, 0xba, 0xc4),
    ThemeRgb::new(0x6e, 0x76, 0x81),
    ThemeRgb::new(0xff, 0x7b, 0x72),
    ThemeRgb::new(0x56, 0xd3, 0x64),
    ThemeRgb::new(0xe3, 0xc1, 0x9c),
    ThemeRgb::new(0xa7, 0x8b, 0xfa),
    ThemeRgb::new(0xf4, 0x72, 0xb6),
    ThemeRgb::new(0x22, 0xd3, 0xee),
    ThemeRgb::new(0xf0, 0xf6, 0xfc),
];

const LIGHT_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x24, 0x29, 0x2f),
    ThemeRgb::new(0xcf, 0x22, 0x2e),
    ThemeRgb::new(0x1a, 0x7f, 0x37),
    ThemeRgb::new(0x93, 0x57, 0x0a),
    ThemeRgb::new(0x6d, 0x28, 0xd9),
    ThemeRgb::new(0xbf, 0x39, 0x89),
    ThemeRgb::new(0x0e, 0x74, 0x90),
    ThemeRgb::new(0x8b, 0x94, 0x9e),
    ThemeRgb::new(0x57, 0x60, 0x6a),
    ThemeRgb::new(0xd5, 0x53, 0x4a),
    ThemeRgb::new(0x2e, 0xa4, 0x4f),
    ThemeRgb::new(0xb4, 0x53, 0x09),
    ThemeRgb::new(0x8b, 0x5c, 0xf6),
    ThemeRgb::new(0xec, 0x48, 0x99),
    ThemeRgb::new(0x06, 0xb6, 0xd4),
    ThemeRgb::new(0x6e, 0x77, 0x81),
];

// --- Solarized Dark ---
const SOLARIZED_DARK_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x07, 0x36, 0x42), ThemeRgb::new(0xdc, 0x32, 0x2f),
    ThemeRgb::new(0x85, 0x99, 0x00), ThemeRgb::new(0xb5, 0x89, 0x00),
    ThemeRgb::new(0x26, 0x8b, 0xd2), ThemeRgb::new(0xd3, 0x36, 0x82),
    ThemeRgb::new(0x2a, 0xa1, 0x98), ThemeRgb::new(0xee, 0xe8, 0xd5),
    ThemeRgb::new(0x00, 0x2b, 0x36), ThemeRgb::new(0xcb, 0x4b, 0x16),
    ThemeRgb::new(0x58, 0x6e, 0x75), ThemeRgb::new(0x65, 0x7b, 0x83),
    ThemeRgb::new(0x83, 0x94, 0x96), ThemeRgb::new(0x6c, 0x71, 0xc4),
    ThemeRgb::new(0x93, 0xa1, 0xa1), ThemeRgb::new(0xfd, 0xf6, 0xe3),
];

// --- Solarized Light ---
// Normal black/white swapped vs dark so text contrasts with cream bg
const SOLARIZED_LIGHT_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x07, 0x36, 0x42), ThemeRgb::new(0xdc, 0x32, 0x2f),
    ThemeRgb::new(0x85, 0x99, 0x00), ThemeRgb::new(0xb5, 0x89, 0x00),
    ThemeRgb::new(0x26, 0x8b, 0xd2), ThemeRgb::new(0xd3, 0x36, 0x82),
    ThemeRgb::new(0x2a, 0xa1, 0x98), ThemeRgb::new(0xee, 0xe8, 0xd5),
    ThemeRgb::new(0x00, 0x2b, 0x36), ThemeRgb::new(0xcb, 0x4b, 0x16),
    ThemeRgb::new(0x58, 0x6e, 0x75), ThemeRgb::new(0x65, 0x7b, 0x83),
    ThemeRgb::new(0x83, 0x94, 0x96), ThemeRgb::new(0x6c, 0x71, 0xc4),
    ThemeRgb::new(0x93, 0xa1, 0xa1), ThemeRgb::new(0xfd, 0xf6, 0xe3),
];

// --- Monokai ---
const MONOKAI_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x27, 0x28, 0x22), ThemeRgb::new(0xf9, 0x26, 0x72),
    ThemeRgb::new(0xa6, 0xe2, 0x2e), ThemeRgb::new(0xf4, 0xbf, 0x75),
    ThemeRgb::new(0x66, 0xd9, 0xef), ThemeRgb::new(0xae, 0x81, 0xff),
    ThemeRgb::new(0xa1, 0xef, 0xe4), ThemeRgb::new(0xf8, 0xf8, 0xf2),
    ThemeRgb::new(0x75, 0x71, 0x5e), ThemeRgb::new(0xf9, 0x26, 0x72),
    ThemeRgb::new(0xa6, 0xe2, 0x2e), ThemeRgb::new(0xf4, 0xbf, 0x75),
    ThemeRgb::new(0x66, 0xd9, 0xef), ThemeRgb::new(0xae, 0x81, 0xff),
    ThemeRgb::new(0xa1, 0xef, 0xe4), ThemeRgb::new(0xf9, 0xf8, 0xf5),
];

// --- Dracula ---
const DRACULA_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x21, 0x22, 0x2c), ThemeRgb::new(0xff, 0x55, 0x55),
    ThemeRgb::new(0x50, 0xfa, 0x7b), ThemeRgb::new(0xf1, 0xfa, 0x8c),
    ThemeRgb::new(0xbd, 0x93, 0xf9), ThemeRgb::new(0xff, 0x79, 0xc6),
    ThemeRgb::new(0x8b, 0xe9, 0xfd), ThemeRgb::new(0xf8, 0xf8, 0xf2),
    ThemeRgb::new(0x62, 0x72, 0xa4), ThemeRgb::new(0xff, 0x6e, 0x6e),
    ThemeRgb::new(0x69, 0xff, 0x94), ThemeRgb::new(0xff, 0xff, 0xa5),
    ThemeRgb::new(0xd6, 0xac, 0xff), ThemeRgb::new(0xff, 0x92, 0xdf),
    ThemeRgb::new(0xa4, 0xff, 0xff), ThemeRgb::new(0xff, 0xff, 0xff),
];

// --- Nord ---
const NORD_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x3b, 0x42, 0x52), ThemeRgb::new(0xbf, 0x61, 0x6a),
    ThemeRgb::new(0xa3, 0xbe, 0x8c), ThemeRgb::new(0xeb, 0xcb, 0x8b),
    ThemeRgb::new(0x81, 0xa1, 0xc1), ThemeRgb::new(0xb4, 0x8e, 0xad),
    ThemeRgb::new(0x88, 0xc0, 0xd0), ThemeRgb::new(0xe5, 0xe9, 0xf0),
    ThemeRgb::new(0x4c, 0x56, 0x6a), ThemeRgb::new(0xbf, 0x61, 0x6a),
    ThemeRgb::new(0xa3, 0xbe, 0x8c), ThemeRgb::new(0xeb, 0xcb, 0x8b),
    ThemeRgb::new(0x81, 0xa1, 0xc1), ThemeRgb::new(0xb4, 0x8e, 0xad),
    ThemeRgb::new(0x8f, 0xbc, 0xbb), ThemeRgb::new(0xec, 0xef, 0xf4),
];

// --- Tokyo Night ---
const TOKYO_NIGHT_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x15, 0x16, 0x1e), ThemeRgb::new(0xf7, 0x76, 0x8e),
    ThemeRgb::new(0x9e, 0xce, 0x6a), ThemeRgb::new(0xe0, 0xaf, 0x68),
    ThemeRgb::new(0x7a, 0xa2, 0xf7), ThemeRgb::new(0xbb, 0x9a, 0xf7),
    ThemeRgb::new(0x7d, 0xcf, 0xff), ThemeRgb::new(0xa9, 0xb1, 0xd6),
    ThemeRgb::new(0x41, 0x48, 0x68), ThemeRgb::new(0xf7, 0x76, 0x8e),
    ThemeRgb::new(0x9e, 0xce, 0x6a), ThemeRgb::new(0xe0, 0xaf, 0x68),
    ThemeRgb::new(0x7a, 0xa2, 0xf7), ThemeRgb::new(0xbb, 0x9a, 0xf7),
    ThemeRgb::new(0x7d, 0xcf, 0xff), ThemeRgb::new(0xc0, 0xca, 0xf5),
];

fn scheme_palette(scheme: ColorScheme) -> ThemePalette {
    match scheme {
        ColorScheme::Dark => theme_palette(TerminalThemeMode::Dark),
        ColorScheme::Light => theme_palette(TerminalThemeMode::Light),
        ColorScheme::SolarizedDark => ThemePalette {
            background: ThemeRgb::new(0x00, 0x2b, 0x36),
            foreground: ThemeRgb::new(0x83, 0x94, 0x96),
            cursor: ThemeRgb::new(0x93, 0xa1, 0xa1),
            selection_fg: ThemeRgb::new(0xfd, 0xf6, 0xe3),
            selection_bg: [0.07, 0.54, 0.82, 0.3],
            ansi: SOLARIZED_DARK_ANSI,
        },
        ColorScheme::SolarizedLight => ThemePalette {
            background: ThemeRgb::new(0xfd, 0xf6, 0xe3),
            foreground: ThemeRgb::new(0x65, 0x7b, 0x83),
            cursor: ThemeRgb::new(0x58, 0x6e, 0x75),
            selection_fg: ThemeRgb::new(0x00, 0x2b, 0x36),
            selection_bg: [0.07, 0.54, 0.82, 0.15],
            ansi: SOLARIZED_LIGHT_ANSI,
        },
        ColorScheme::Monokai => ThemePalette {
            background: ThemeRgb::new(0x27, 0x28, 0x22),
            foreground: ThemeRgb::new(0xf8, 0xf8, 0xf2),
            cursor: ThemeRgb::new(0xf8, 0xf8, 0xf0),
            selection_fg: ThemeRgb::new(0xff, 0xff, 0xff),
            selection_bg: [0.58, 0.44, 0.08, 0.3],
            ansi: MONOKAI_ANSI,
        },
        ColorScheme::Dracula => ThemePalette {
            background: ThemeRgb::new(0x28, 0x2a, 0x36),
            foreground: ThemeRgb::new(0xf8, 0xf8, 0xf2),
            cursor: ThemeRgb::new(0xf8, 0xf8, 0xf2),
            selection_fg: ThemeRgb::new(0xff, 0xff, 0xff),
            selection_bg: [0.27, 0.28, 0.35, 0.5],
            ansi: DRACULA_ANSI,
        },
        ColorScheme::Nord => ThemePalette {
            background: ThemeRgb::new(0x2e, 0x34, 0x40),
            foreground: ThemeRgb::new(0xd8, 0xde, 0xe9),
            cursor: ThemeRgb::new(0xd8, 0xde, 0xe9),
            selection_fg: ThemeRgb::new(0xec, 0xef, 0xf4),
            selection_bg: [0.26, 0.30, 0.37, 0.5],
            ansi: NORD_ANSI,
        },
        ColorScheme::TokyoNight => ThemePalette {
            background: ThemeRgb::new(0x1a, 0x1b, 0x26),
            foreground: ThemeRgb::new(0xa9, 0xb1, 0xd6),
            cursor: ThemeRgb::new(0xc0, 0xca, 0xf5),
            selection_fg: ThemeRgb::new(0xc0, 0xca, 0xf5),
            selection_bg: [0.18, 0.20, 0.36, 0.5],
            ansi: TOKYO_NIGHT_ANSI,
        },
    }
}

fn theme_palette(mode: TerminalThemeMode) -> ThemePalette {
    match mode {
        TerminalThemeMode::Dark => ThemePalette {
            background: ThemeRgb::new(0x0d, 0x11, 0x17),
            foreground: ThemeRgb::new(0xc9, 0xd1, 0xd9),
            cursor: ThemeRgb::new(0xd4, 0xa5, 0x74),
            selection_fg: ThemeRgb::new(0xff, 0xff, 0xff),
            selection_bg: [
                0x33 as f32 / 255.0,
                0x66 as f32 / 255.0,
                0xff as f32 / 255.0,
                0.3,
            ],
            ansi: DARK_ANSI,
        },
        TerminalThemeMode::Light => ThemePalette {
            background: ThemeRgb::new(0xfa, 0xf6, 0xf0),
            foreground: ThemeRgb::new(0x24, 0x29, 0x2f),
            cursor: ThemeRgb::new(0xb4, 0x53, 0x09),
            selection_fg: ThemeRgb::new(0x24, 0x29, 0x2f),
            selection_bg: [
                0xd4 as f32 / 255.0,
                0xa5 as f32 / 255.0,
                0x74 as f32 / 255.0,
                0.15,
            ],
            ansi: LIGHT_ANSI,
        },
    }
}

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
        Self {
            pipeline,
            vertices: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.vertices.clear();
    }

    fn push_rect(&mut self, x: f32, y: f32, w: f32, h: f32, vp_w: f32, vp_h: f32, color: [f32; 4]) {
        let x0 = x / vp_w * 2.0 - 1.0;
        let y0 = 1.0 - y / vp_h * 2.0;
        let x1 = (x + w) / vp_w * 2.0 - 1.0;
        let y1 = 1.0 - (y + h) / vp_h * 2.0;
        self.vertices.extend_from_slice(&[
            RectVertex {
                position: [x0, y0],
                color,
            },
            RectVertex {
                position: [x1, y0],
                color,
            },
            RectVertex {
                position: [x0, y1],
                color,
            },
            RectVertex {
                position: [x1, y0],
                color,
            },
            RectVertex {
                position: [x1, y1],
                color,
            },
            RectVertex {
                position: [x0, y1],
                color,
            },
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

        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rect_vertices"),
                contents: bytes,
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
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
    grid: Vec<GridCell>,
    glyph_cache: HashMap<GlyphCacheKey, Buffer>,
    cursor_buffer: Buffer,
    cursor_text: String,
    cursor_width_cells: u8,
    last_cursor_color: Option<ThemeRgb>,
    rect_renderer: RectRenderer,
    last_font_size: f32,
    last_line_height: f32,
    theme_mode: TerminalThemeMode,
    color_scheme: Option<ColorScheme>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct GlyphCacheKey {
    text: String,
    width_cells: u8,
}

struct GridCell {
    text: String,
    fg: GlyphonColor,
    width_cells: u8,
    active: bool,
}

impl TerminalGridRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        theme_mode: TerminalThemeMode,
        _scale_factor: f32,
    ) -> Self {
        let font_size = TERMINAL_FONT_SIZE_PX;
        let line_height = TERMINAL_LINE_HEIGHT_PX;
        let mut font_system = FontSystem::new();
        configure_terminal_font_system(&mut font_system);
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let rect_renderer = RectRenderer::new(device, format);

        let mut cursor_buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
        cursor_buffer.set_size(&mut font_system, Some(font_size * 0.6), Some(line_height));
        cursor_buffer.set_monospace_width(&mut font_system, Some(font_size * 0.6));

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            font_size,
            line_height,
            grid: Vec::new(),
            glyph_cache: HashMap::new(),
            cursor_buffer,
            cursor_text: String::new(),
            cursor_width_cells: 0,
            last_cursor_color: None,
            rect_renderer,
            last_font_size: -1.0,
            last_line_height: -1.0,
            theme_mode,
            color_scheme: None,
        }
    }

    pub fn set_theme_mode(&mut self, theme_mode: TerminalThemeMode) {
        if self.theme_mode == theme_mode {
            return;
        }
        self.theme_mode = theme_mode;
        self.last_cursor_color = None;
    }

    pub fn set_color_scheme(&mut self, scheme: ColorScheme) {
        self.color_scheme = Some(scheme);
        self.last_cursor_color = None;
    }

    pub fn set_font_size(&mut self, size: f32) {
        let size = size.clamp(8.0, 32.0);
        if (self.font_size - size).abs() < 0.01 {
            return;
        }
        self.font_size = size;
    }

    pub fn set_line_height(&mut self, height: f32) {
        let height = height.clamp(12.0, 64.0);
        if (self.line_height - height).abs() < 0.01 {
            return;
        }
        self.line_height = height;
    }

    fn active_palette(&self) -> ThemePalette {
        match self.color_scheme {
            Some(scheme) => scheme_palette(scheme),
            None => theme_palette(self.theme_mode),
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
        let palette = self.active_palette();
        self.viewport.update(queue, Resolution { width, height });

        let (cols_u16, rows_u16) = self.grid_size(width as f32, height as f32);
        let cols = cols_u16 as usize;
        let rows = rows_u16 as usize;
        self.ensure_grid(cols, rows);

        for cell in &mut self.grid {
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

            if let Some(bg) = cell_background_rgba(cell, is_selected, &palette) {
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

            let fg = cell_foreground(cell, is_selected, &palette);
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

        // Populate glyph cache with any new glyphs needed this frame
        {
            let mut needed: Vec<(String, u8)> = Vec::new();
            for cell in self.grid.iter() {
                if cell.active && !cell.text.is_empty() {
                    let key = GlyphCacheKey {
                        text: cell.text.clone(),
                        width_cells: cell.width_cells,
                    };
                    if !self.glyph_cache.contains_key(&key) {
                        needed.push((cell.text.clone(), cell.width_cells));
                    }
                }
            }
            let metrics = Metrics::new(self.font_size, self.line_height);
            for (text, width_cells) in needed {
                let key = GlyphCacheKey {
                    text: text.clone(),
                    width_cells,
                };
                if self.glyph_cache.contains_key(&key) {
                    continue;
                }
                let width = cw * width_cells as f32;
                let shaping = if text.is_ascii() {
                    Shaping::Basic
                } else {
                    Shaping::Advanced
                };
                let mut buffer = Buffer::new(&mut self.font_system, metrics);
                buffer.set_size(&mut self.font_system, Some(width), Some(lh));
                buffer.set_monospace_width(&mut self.font_system, Some(cw));
                buffer.set_text(
                    &mut self.font_system,
                    &text,
                    Attrs::new().family(Family::Monospace),
                    shaping,
                );
                self.glyph_cache.insert(key, buffer);
            }
        }

        // Build text areas: each cell rendered at exact grid position
        let mut text_areas = Vec::with_capacity(active_cells + usize::from(cursor_visible));
        for row in 0..rows {
            for col in 0..cols {
                let cell = &self.grid[row * cols + col];
                if !cell.active || cell.text.is_empty() {
                    continue;
                }
                let key = GlyphCacheKey {
                    text: cell.text.clone(),
                    width_cells: cell.width_cells,
                };
                if let Some(buffer) = self.glyph_cache.get(&key) {
                    text_areas.push(TextArea {
                        buffer,
                        left: 4.0 + col as f32 * cw,
                        top: 4.0 + row as f32 * lh,
                        scale: 1.0,
                        bounds,
                        default_color: cell.fg,
                        custom_glyphs: &[],
                    });
                }
            }
        }

        if cursor_visible {
            let cursor_color = glyphon_from_rgb(palette.cursor);
            if let Some(cursor_row) = cursor_row {
                text_areas.push(TextArea {
                    buffer: &self.cursor_buffer,
                    left: 4.0 + cursor_col as f32 * cw,
                    top: 4.0 + cursor_row as f32 * lh,
                    scale: 1.0,
                    bounds,
                    default_color: cursor_color,
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
                            r: palette.background.r as f64 / 255.0,
                            g: palette.background.g as f64 / 255.0,
                            b: palette.background.b as f64 / 255.0,
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

    fn ensure_grid(&mut self, cols: usize, rows: usize) {
        let target = cols * rows;
        let default_fg = glyphon_from_rgb(self.active_palette().foreground);

        self.grid.resize_with(target, || GridCell {
            text: String::new(),
            fg: default_fg,
            width_cells: 1,
            active: false,
        });
        self.grid.truncate(target);

        if self.font_size != self.last_font_size || self.line_height != self.last_line_height {
            self.glyph_cache.clear();

            let metrics = Metrics::new(self.font_size, self.line_height);
            let cw = self.cell_width();

            self.cursor_buffer
                .set_metrics(&mut self.font_system, metrics);
            self.cursor_buffer
                .set_size(&mut self.font_system, Some(cw), Some(self.line_height));
            self.cursor_buffer
                .set_monospace_width(&mut self.font_system, Some(cw));

            self.last_font_size = self.font_size;
            self.last_line_height = self.line_height;
            self.cursor_width_cells = 0;
        }
    }

    fn update_cell(&mut self, slot: usize, text: &str, fg: GlyphonColor, width_cells: u8) -> bool {
        let cell = &mut self.grid[slot];
        cell.active = true;

        if cell.text == text && cell.fg == fg && cell.width_cells == width_cells {
            return false;
        }

        cell.text.clear();
        cell.text.push_str(text);
        cell.fg = fg;
        cell.width_cells = width_cells;
        true
    }

    fn update_cursor_buffer(&mut self, width_cells: u8) {
        let width_cells = width_cells.max(1);
        let cw = self.cell_width();
        let width = cw * width_cells as f32;
        let text = if width_cells == 2 { "██" } else { "█" };
        let palette = self.active_palette();
        let cursor_color = glyphon_from_rgb(palette.cursor);

        self.cursor_buffer.set_metrics(
            &mut self.font_system,
            Metrics::new(self.font_size, self.line_height),
        );
        self.cursor_buffer
            .set_size(&mut self.font_system, Some(width), Some(self.line_height));
        self.cursor_buffer
            .set_monospace_width(&mut self.font_system, Some(cw));

        if self.cursor_width_cells != width_cells
            || self.cursor_text != text
            || self.last_cursor_color != Some(palette.cursor)
        {
            self.cursor_buffer.set_text(
                &mut self.font_system,
                text,
                Attrs::new().family(Family::Monospace).color(cursor_color),
                Shaping::Basic,
            );
            self.cursor_text.clear();
            self.cursor_text.push_str(text);
            self.cursor_width_cells = width_cells;
            self.last_cursor_color = Some(palette.cursor);
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

fn cell_foreground(cell: &Cell, is_selected: bool, palette: &ThemePalette) -> GlyphonColor {
    if is_selected {
        return glyphon_from_rgb(palette.selection_fg);
    }

    if cell.flags.contains(Flags::INVERSE) {
        return ansi_to_glyphon(&cell.bg, palette);
    }

    ansi_to_glyphon(&cell.fg, palette)
}

fn cell_background_rgba(
    cell: &Cell,
    is_selected: bool,
    palette: &ThemePalette,
) -> Option<[f32; 4]> {
    if is_selected {
        return Some(palette.selection_bg);
    }

    if cell.flags.contains(Flags::INVERSE) {
        return Some(color_to_rect_rgba(ansi_to_glyphon(&cell.fg, palette)));
    }

    match cell.bg {
        AnsiColor::Named(NamedColor::Background) => None,
        _ => Some(color_to_rect_rgba(ansi_to_glyphon(&cell.bg, palette))),
    }
}

fn color_to_rect_rgba(color: GlyphonColor) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    ]
}

fn glyphon_from_rgb(rgb: ThemeRgb) -> GlyphonColor {
    GlyphonColor::rgb(rgb.r, rgb.g, rgb.b)
}

fn ansi_to_glyphon(color: &AnsiColor, palette: &ThemePalette) -> GlyphonColor {
    match color {
        AnsiColor::Named(named) => named_to_glyphon(*named, palette),
        AnsiColor::Spec(rgb) => GlyphonColor::rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => indexed_to_glyphon(*idx, palette),
    }
}

fn named_to_glyphon(named: NamedColor, palette: &ThemePalette) -> GlyphonColor {
    match named {
        NamedColor::Black => glyphon_from_rgb(palette.ansi[0]),
        NamedColor::Red => glyphon_from_rgb(palette.ansi[1]),
        NamedColor::Green => glyphon_from_rgb(palette.ansi[2]),
        NamedColor::Yellow => glyphon_from_rgb(palette.ansi[3]),
        NamedColor::Blue => glyphon_from_rgb(palette.ansi[4]),
        NamedColor::Magenta => glyphon_from_rgb(palette.ansi[5]),
        NamedColor::Cyan => glyphon_from_rgb(palette.ansi[6]),
        NamedColor::White => glyphon_from_rgb(palette.ansi[7]),
        NamedColor::BrightBlack => glyphon_from_rgb(palette.ansi[8]),
        NamedColor::BrightRed => glyphon_from_rgb(palette.ansi[9]),
        NamedColor::BrightGreen => glyphon_from_rgb(palette.ansi[10]),
        NamedColor::BrightYellow => glyphon_from_rgb(palette.ansi[11]),
        NamedColor::BrightBlue => glyphon_from_rgb(palette.ansi[12]),
        NamedColor::BrightMagenta => glyphon_from_rgb(palette.ansi[13]),
        NamedColor::BrightCyan => glyphon_from_rgb(palette.ansi[14]),
        NamedColor::BrightWhite => glyphon_from_rgb(palette.ansi[15]),
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
            glyphon_from_rgb(palette.foreground)
        }
        NamedColor::Background => glyphon_from_rgb(palette.background),
        NamedColor::Cursor => glyphon_from_rgb(palette.cursor),
        NamedColor::DimBlack => glyphon_from_rgb(palette.ansi[0]),
        NamedColor::DimRed => glyphon_from_rgb(palette.ansi[1]),
        NamedColor::DimGreen => glyphon_from_rgb(palette.ansi[2]),
        NamedColor::DimYellow => glyphon_from_rgb(palette.ansi[3]),
        NamedColor::DimBlue => glyphon_from_rgb(palette.ansi[4]),
        NamedColor::DimMagenta => glyphon_from_rgb(palette.ansi[5]),
        NamedColor::DimCyan => glyphon_from_rgb(palette.ansi[6]),
        NamedColor::DimWhite => glyphon_from_rgb(palette.ansi[7]),
    }
}

fn indexed_to_glyphon(idx: u8, palette: &ThemePalette) -> GlyphonColor {
    if idx < 16 {
        return glyphon_from_rgb(palette.ansi[idx as usize]);
    }

    if idx < 232 {
        let i = idx - 16;
        let r = if i / 36 > 0 { (i / 36) * 40 + 55 } else { 0 };
        let g = if (i % 36) / 6 > 0 {
            ((i % 36) / 6) * 40 + 55
        } else {
            0
        };
        let b = if i % 6 > 0 { (i % 6) * 40 + 55 } else { 0 };
        return GlyphonColor::rgb(r, g, b);
    }

    let v = (idx - 232) * 10 + 8;
    GlyphonColor::rgb(v, v, v)
}

fn configure_terminal_font_system(font_system: &mut FontSystem) {
    if let Some(family) = detect_nerd_font_family(font_system) {
        font_system.db_mut().set_monospace_family(family.clone());
        eprintln!("[aterm-core] renderer using Nerd Font monospace family: {family}");
        return;
    }

    if let Some(family) = detect_system_monospace_family(font_system) {
        font_system.db_mut().set_monospace_family(family);
    }
}

fn detect_nerd_font_family(font_system: &FontSystem) -> Option<String> {
    for preferred in NERD_FONT_FAMILY_PREFERENCES {
        if font_family_exists(font_system, preferred) {
            return Some((*preferred).to_string());
        }
    }

    let mut fallback_candidates: Vec<String> = font_system
        .db()
        .faces()
        .filter(|face| face.monospaced)
        .flat_map(|face| face.families.iter().map(|(family, _)| family.clone()))
        .filter(|family| is_nerd_font_family_name(family))
        .collect();

    fallback_candidates
        .sort_by(|left, right| nerd_font_sort_key(left).cmp(&nerd_font_sort_key(right)));
    fallback_candidates.dedup();
    fallback_candidates.into_iter().next()
}

fn font_family_exists(font_system: &FontSystem, family: &str) -> bool {
    font_system.db().faces().any(|face| {
        face.monospaced
            && face
                .families
                .iter()
                .any(|(candidate, _)| candidate.eq_ignore_ascii_case(family))
    })
}

fn detect_system_monospace_family(font_system: &FontSystem) -> Option<String> {
    for preferred in SYSTEM_MONOSPACE_FAMILY_PREFERENCES {
        if font_family_exists(font_system, preferred) {
            return Some((*preferred).to_string());
        }
    }

    font_system
        .db()
        .faces()
        .find(|face| face.monospaced)
        .and_then(|face| face.families.first().map(|(family, _)| family.clone()))
}

fn is_nerd_font_family_name(family: &str) -> bool {
    let name = family.to_ascii_lowercase();
    name.contains("nerd font") || name.ends_with(" nf") || name.contains(" nf ")
}

fn nerd_font_sort_key(family: &str) -> (bool, bool, bool, usize, String) {
    let name = family.to_ascii_lowercase();
    (
        !name.contains("mono"),
        !name.contains("jetbrains"),
        !name.contains("meslo"),
        family.len(),
        name,
    )
}
