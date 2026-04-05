//! wgpu instanced cell renderer — glyph atlas + single draw call per frame.

use glyphon::{Color as GlyphonColor, FontSystem};
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::{cell::Cell, cell::Flags, Term, TermDamage};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};

use crate::renderer_atlas::AtlasManager;
use crate::renderer_glyph::GlyphCache;

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
    Default = 8,
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

    fn luminance(&self) -> f32 {
        0.299 * (self.r as f32 / 255.0)
            + 0.587 * (self.g as f32 / 255.0)
            + 0.114 * (self.b as f32 / 255.0)
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
    ThemeRgb::new(0xd0, 0xd7, 0xde),
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
    ThemeRgb::new(0x07, 0x36, 0x42),
    ThemeRgb::new(0xdc, 0x32, 0x2f),
    ThemeRgb::new(0x85, 0x99, 0x00),
    ThemeRgb::new(0xb5, 0x89, 0x00),
    ThemeRgb::new(0x26, 0x8b, 0xd2),
    ThemeRgb::new(0xd3, 0x36, 0x82),
    ThemeRgb::new(0x2a, 0xa1, 0x98),
    ThemeRgb::new(0xee, 0xe8, 0xd5),
    ThemeRgb::new(0x00, 0x2b, 0x36),
    ThemeRgb::new(0xcb, 0x4b, 0x16),
    ThemeRgb::new(0x58, 0x6e, 0x75),
    ThemeRgb::new(0x65, 0x7b, 0x83),
    ThemeRgb::new(0x83, 0x94, 0x96),
    ThemeRgb::new(0x6c, 0x71, 0xc4),
    ThemeRgb::new(0x93, 0xa1, 0xa1),
    ThemeRgb::new(0xfd, 0xf6, 0xe3),
];

// --- Solarized Light ---
// Normal black/white swapped vs dark so text contrasts with cream bg
const SOLARIZED_LIGHT_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x07, 0x36, 0x42),
    ThemeRgb::new(0xdc, 0x32, 0x2f),
    ThemeRgb::new(0x85, 0x99, 0x00),
    ThemeRgb::new(0xb5, 0x89, 0x00),
    ThemeRgb::new(0x26, 0x8b, 0xd2),
    ThemeRgb::new(0xd3, 0x36, 0x82),
    ThemeRgb::new(0x2a, 0xa1, 0x98),
    ThemeRgb::new(0xee, 0xe8, 0xd5),
    ThemeRgb::new(0x00, 0x2b, 0x36),
    ThemeRgb::new(0xcb, 0x4b, 0x16),
    ThemeRgb::new(0x58, 0x6e, 0x75),
    ThemeRgb::new(0x65, 0x7b, 0x83),
    ThemeRgb::new(0x83, 0x94, 0x96),
    ThemeRgb::new(0x6c, 0x71, 0xc4),
    ThemeRgb::new(0x93, 0xa1, 0xa1),
    ThemeRgb::new(0xfd, 0xf6, 0xe3),
];

// --- Monokai ---
const MONOKAI_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x27, 0x28, 0x22),
    ThemeRgb::new(0xf9, 0x26, 0x72),
    ThemeRgb::new(0xa6, 0xe2, 0x2e),
    ThemeRgb::new(0xf4, 0xbf, 0x75),
    ThemeRgb::new(0x66, 0xd9, 0xef),
    ThemeRgb::new(0xae, 0x81, 0xff),
    ThemeRgb::new(0xa1, 0xef, 0xe4),
    ThemeRgb::new(0xf8, 0xf8, 0xf2),
    ThemeRgb::new(0x75, 0x71, 0x5e),
    ThemeRgb::new(0xf9, 0x26, 0x72),
    ThemeRgb::new(0xa6, 0xe2, 0x2e),
    ThemeRgb::new(0xf4, 0xbf, 0x75),
    ThemeRgb::new(0x66, 0xd9, 0xef),
    ThemeRgb::new(0xae, 0x81, 0xff),
    ThemeRgb::new(0xa1, 0xef, 0xe4),
    ThemeRgb::new(0xf9, 0xf8, 0xf5),
];

// --- Dracula ---
const DRACULA_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x21, 0x22, 0x2c),
    ThemeRgb::new(0xff, 0x55, 0x55),
    ThemeRgb::new(0x50, 0xfa, 0x7b),
    ThemeRgb::new(0xf1, 0xfa, 0x8c),
    ThemeRgb::new(0xbd, 0x93, 0xf9),
    ThemeRgb::new(0xff, 0x79, 0xc6),
    ThemeRgb::new(0x8b, 0xe9, 0xfd),
    ThemeRgb::new(0xf8, 0xf8, 0xf2),
    ThemeRgb::new(0x62, 0x72, 0xa4),
    ThemeRgb::new(0xff, 0x6e, 0x6e),
    ThemeRgb::new(0x69, 0xff, 0x94),
    ThemeRgb::new(0xff, 0xff, 0xa5),
    ThemeRgb::new(0xd6, 0xac, 0xff),
    ThemeRgb::new(0xff, 0x92, 0xdf),
    ThemeRgb::new(0xa4, 0xff, 0xff),
    ThemeRgb::new(0xff, 0xff, 0xff),
];

// --- Nord ---
const NORD_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x3b, 0x42, 0x52),
    ThemeRgb::new(0xbf, 0x61, 0x6a),
    ThemeRgb::new(0xa3, 0xbe, 0x8c),
    ThemeRgb::new(0xeb, 0xcb, 0x8b),
    ThemeRgb::new(0x81, 0xa1, 0xc1),
    ThemeRgb::new(0xb4, 0x8e, 0xad),
    ThemeRgb::new(0x88, 0xc0, 0xd0),
    ThemeRgb::new(0xe5, 0xe9, 0xf0),
    ThemeRgb::new(0x4c, 0x56, 0x6a),
    ThemeRgb::new(0xbf, 0x61, 0x6a),
    ThemeRgb::new(0xa3, 0xbe, 0x8c),
    ThemeRgb::new(0xeb, 0xcb, 0x8b),
    ThemeRgb::new(0x81, 0xa1, 0xc1),
    ThemeRgb::new(0xb4, 0x8e, 0xad),
    ThemeRgb::new(0x8f, 0xbc, 0xbb),
    ThemeRgb::new(0xec, 0xef, 0xf4),
];

// --- Tokyo Night ---
const TOKYO_NIGHT_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x15, 0x16, 0x1e),
    ThemeRgb::new(0xf7, 0x76, 0x8e),
    ThemeRgb::new(0x9e, 0xce, 0x6a),
    ThemeRgb::new(0xe0, 0xaf, 0x68),
    ThemeRgb::new(0x7a, 0xa2, 0xf7),
    ThemeRgb::new(0xbb, 0x9a, 0xf7),
    ThemeRgb::new(0x7d, 0xcf, 0xff),
    ThemeRgb::new(0xa9, 0xb1, 0xd6),
    ThemeRgb::new(0x41, 0x48, 0x68),
    ThemeRgb::new(0xf7, 0x76, 0x8e),
    ThemeRgb::new(0x9e, 0xce, 0x6a),
    ThemeRgb::new(0xe0, 0xaf, 0x68),
    ThemeRgb::new(0x7a, 0xa2, 0xf7),
    ThemeRgb::new(0xbb, 0x9a, 0xf7),
    ThemeRgb::new(0x7d, 0xcf, 0xff),
    ThemeRgb::new(0xc0, 0xca, 0xf5),
];

// --- Default (standard xterm-256 — CLI native passthrough) ---
const XTERM_DEFAULT_ANSI: [ThemeRgb; 16] = [
    ThemeRgb::new(0x00, 0x00, 0x00), // Black
    ThemeRgb::new(0xCC, 0x00, 0x00), // Red
    ThemeRgb::new(0x00, 0xCC, 0x00), // Green
    ThemeRgb::new(0xCC, 0xCC, 0x00), // Yellow
    ThemeRgb::new(0x00, 0x00, 0xCC), // Blue
    ThemeRgb::new(0xCC, 0x00, 0xCC), // Magenta
    ThemeRgb::new(0x00, 0xCC, 0xCC), // Cyan
    ThemeRgb::new(0xCC, 0xCC, 0xCC), // White
    ThemeRgb::new(0x55, 0x55, 0x55), // Bright Black
    ThemeRgb::new(0xFF, 0x00, 0x00), // Bright Red
    ThemeRgb::new(0x00, 0xFF, 0x00), // Bright Green
    ThemeRgb::new(0xFF, 0xFF, 0x00), // Bright Yellow
    ThemeRgb::new(0x55, 0x55, 0xFF), // Bright Blue
    ThemeRgb::new(0xFF, 0x00, 0xFF), // Bright Magenta
    ThemeRgb::new(0x00, 0xFF, 0xFF), // Bright Cyan
    ThemeRgb::new(0xFF, 0xFF, 0xFF), // Bright White
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
        ColorScheme::Default => ThemePalette {
            background: ThemeRgb::new(0x00, 0x00, 0x00),
            foreground: ThemeRgb::new(0xCC, 0xCC, 0xCC),
            cursor: ThemeRgb::new(0xCC, 0xCC, 0xCC),
            selection_fg: ThemeRgb::new(0xFF, 0xFF, 0xFF),
            selection_bg: [0.33, 0.33, 0.33, 0.5],
            ansi: XTERM_DEFAULT_ANSI,
        },
    }
}

fn theme_palette(mode: TerminalThemeMode) -> ThemePalette {
    match mode {
        TerminalThemeMode::Dark => ThemePalette {
            background: ThemeRgb::new(0x1a, 0x1b, 0x26),
            foreground: ThemeRgb::new(0xa9, 0xb1, 0xd6),
            cursor: ThemeRgb::new(0xa9, 0xb1, 0xd6),
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

const CELL_SHADER: &str = include_str!("shaders/cell.wgsl");

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

    fn make_rect_vertices(x: f32, y: f32, w: f32, h: f32, vp_w: f32, vp_h: f32, color: [f32; 4]) -> [RectVertex; 6] {
        let x0 = x / vp_w * 2.0 - 1.0;
        let y0 = 1.0 - y / vp_h * 2.0;
        let x1 = (x + w) / vp_w * 2.0 - 1.0;
        let y1 = 1.0 - (y + h) / vp_h * 2.0;
        [
            RectVertex { position: [x0, y0], color },
            RectVertex { position: [x1, y0], color },
            RectVertex { position: [x0, y1], color },
            RectVertex { position: [x1, y0], color },
            RectVertex { position: [x1, y1], color },
            RectVertex { position: [x0, y1], color },
        ]
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

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct CellInstance {
    pub grid_pos: [f32; 2],
    pub atlas_uv_rect: [f32; 4],
    pub fg_color: [f32; 4],
    pub bg_color: [f32; 4],
    pub flags: u32,
    pub _pad: u32,
}

impl CellInstance {
    const STRIDE: wgpu::BufferAddress = std::mem::size_of::<Self>() as wgpu::BufferAddress;

    fn as_bytes(instances: &[Self]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                instances.as_ptr() as *const u8,
                std::mem::size_of_val(instances),
            )
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct CellUniforms {
    cell_size: [f32; 2],
    grid_origin: [f32; 2],
    viewport_size: [f32; 2],
    atlas_size: [f32; 2],
}

impl CellUniforms {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (self as *const Self) as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}

const CELL_INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = [
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
    wgpu::VertexAttribute {
        offset: 24,
        shader_location: 2,
        format: wgpu::VertexFormat::Float32x4,
    },
    wgpu::VertexAttribute {
        offset: 40,
        shader_location: 3,
        format: wgpu::VertexFormat::Float32x4,
    },
    wgpu::VertexAttribute {
        offset: 56,
        shader_location: 4,
        format: wgpu::VertexFormat::Uint32,
    },
];

struct CellRenderer {
    atlas: AtlasManager,
    atlas_bind_group_layout: wgpu::BindGroupLayout,
    atlas_sampler: wgpu::Sampler,
    atlas_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
}

impl CellRenderer {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let mut atlas = AtlasManager::new();
        atlas.ensure_page(device);

        let atlas_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cell_atlas_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cell_atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let atlas_bind_group = Self::create_atlas_bind_group(
            device,
            &atlas_bind_group_layout,
            atlas.texture_view(0).expect("atlas page 0 must exist"),
            &atlas_sampler,
        );

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cell_uniforms"),
            contents: CellUniforms::default().as_bytes(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cell_uniform_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cell_uniform_bind_group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell_shader"),
            source: wgpu::ShaderSource::Wgsl(CELL_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell_pipeline_layout"),
            bind_group_layouts: &[&atlas_bind_group_layout, &uniform_bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cell_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: CellInstance::STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &CELL_INSTANCE_ATTRIBUTES,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied alpha: shader outputs (rgb*a, a) for correct
                    // compositing over rect_renderer backgrounds.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
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
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cell_instances"),
            size: CellInstance::STRIDE.max(1),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            atlas,
            atlas_bind_group_layout,
            atlas_sampler,
            atlas_bind_group,
            uniform_buffer,
            uniform_bind_group,
            pipeline,
            instance_buffer,
            instance_capacity: 0,
            instance_count: 0,
        }
    }

    fn update_uniforms(
        &mut self,
        queue: &wgpu::Queue,
        cell_size: [f32; 2],
        grid_origin: [f32; 2],
        viewport_size: [f32; 2],
    ) {
        let atlas_size = self.atlas.atlas_size() as f32;
        let uniforms = CellUniforms {
            cell_size,
            grid_origin,
            viewport_size,
            atlas_size: [atlas_size, atlas_size],
        };
        queue.write_buffer(&self.uniform_buffer, 0, uniforms.as_bytes());
    }

    fn update_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[CellInstance],
    ) {
        self.instance_count = instances.len() as u32;
        if instances.is_empty() {
            return;
        }

        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two().max(1);
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cell_instances"),
                size: self.instance_capacity as u64 * CellInstance::STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        queue.write_buffer(&self.instance_buffer, 0, CellInstance::as_bytes(instances));
    }

    fn sync_atlas_bind_group(&mut self, device: &wgpu::Device) {
        self.atlas.ensure_page(device);
        self.atlas_bind_group = Self::create_atlas_bind_group(
            device,
            &self.atlas_bind_group_layout,
            self.atlas.texture_view(0).expect("atlas page 0 must exist"),
            &self.atlas_sampler,
        );
    }

    fn create_atlas_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cell_atlas_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}

// --- Terminal grid renderer ---

pub struct TerminalGridRenderer {
    font_system: FontSystem,
    font_size: f32,
    line_height: f32,
    grid: Vec<GridCell>,
    glyph_cache: GlyphCache,
    rect_renderer: RectRenderer,
    cell_renderer: CellRenderer,
    persistent_instances: Vec<CellInstance>,
    persistent_block_rects: Vec<Vec<RectVertex>>,
    last_font_size: f32,
    last_line_height: f32,
    theme_mode: TerminalThemeMode,
    color_scheme: Option<ColorScheme>,
    damage_tracker: DamageTracker,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct PromptDebugCell {
    col: usize,
    ch: char,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextRun {
    text: String,
    fg: GlyphonColor,
    row: usize,
    start_col: usize,
    width_cells: usize,
}

#[cfg(test)]
fn count_active_text_cells(grid: &[GridCell]) -> usize {
    grid.iter()
        .filter(|cell| cell.active && !cell.text.is_empty())
        .count()
}

fn build_text_runs(grid: &[GridCell], cols: usize, rows: usize) -> Vec<TextRun> {
    let mut runs = Vec::new();

    for row in 0..rows {
        let mut current: Option<TextRun> = None;
        for col in 0..cols {
            let idx = row * cols + col;
            let cell = match grid.get(idx) {
                Some(c) => c,
                None => break,
            };
            if !cell.active || cell.text.is_empty() {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
                continue;
            }

            let same_fg = current.as_ref().map_or(false, |r| r.fg == cell.fg);
            if same_fg {
                if let Some(run) = current.as_mut() {
                    run.text.push_str(&cell.text);
                    run.width_cells += cell.width_cells as usize;
                }
            } else {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
                current = Some(TextRun {
                    text: cell.text.clone(),
                    fg: cell.fg,
                    row,
                    start_col: col,
                    width_cells: cell.width_cells as usize,
                });
            }
        }

        if let Some(run) = current.take() {
            runs.push(run);
        }
    }

    runs
}

/// Dirty-line tracker: reads alacritty_terminal's TermDamage state and provides
/// per-row damage queries. Pluggable module — call `collect_damage()` once per
/// frame, then query `is_line_damaged(row)` to skip unchanged lines.
struct DamageTracker {
    /// Per visual-row damaged flag for the current frame.
    damaged_rows: Vec<bool>,
    /// True when the entire terminal needs a full redraw.
    full_damage: bool,
}

impl DamageTracker {
    fn new() -> Self {
        Self {
            damaged_rows: Vec::new(),
            full_damage: true,
        }
    }

    fn apply_damage_rows(&mut self, rows: usize, full_damage: bool, damaged_lines: &[usize]) {
        self.damaged_rows.clear();
        self.damaged_rows.resize(rows, false);

        self.full_damage = full_damage;
        if full_damage {
            for flag in &mut self.damaged_rows {
                *flag = true;
            }
            return;
        }

        for &line in damaged_lines {
            if line < rows {
                self.damaged_rows[line] = true;
            }
        }
    }

    /// Read damage from `Term`, populate per-row flags, then reset damage state.
    /// Must be called once per frame BEFORE `renderable_content()`.
    fn collect_damage<T: EventListener>(&mut self, term: &mut Term<T>, rows: usize) {
        let mut damaged_lines = Vec::new();
        let full_damage = match term.damage() {
            TermDamage::Full => true,
            TermDamage::Partial(iter) => {
                damaged_lines.extend(iter.map(|ld| ld.line));
                false
            }
        };
        self.apply_damage_rows(rows, full_damage, &damaged_lines);
        term.reset_damage();
    }

    /// Query whether a specific visual row needs updating this frame.
    #[inline]
    fn is_line_damaged(&self, row: usize) -> bool {
        self.full_damage || self.damaged_rows.get(row).copied().unwrap_or(true)
    }
}

impl TerminalGridRenderer {
    pub fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        theme_mode: TerminalThemeMode,
        _scale_factor: f32,
    ) -> Self {
        let font_size = TERMINAL_FONT_SIZE_PX;
        let line_height = TERMINAL_LINE_HEIGHT_PX;
        let mut font_system = FontSystem::new();
        configure_terminal_font_system(&mut font_system);

        let (font_name, font_bytes) = load_monospace_font_data(&font_system);
        let mut glyph_cache = GlyphCache::from_named_bytes(0, font_name, font_bytes)
            .expect("[aterm] failed to create glyph cache from font data");
        for (fallback_name, fallback_bytes, face_index) in load_fallback_font_data(&font_system) {
            glyph_cache.add_named_fallback(fallback_name, fallback_bytes, face_index);
        }

        let rect_renderer = RectRenderer::new(device, format);
        let cell_renderer = CellRenderer::new(device, format);

        Self {
            font_system,
            font_size,
            line_height,
            grid: Vec::new(),
            glyph_cache,
            rect_renderer,
            cell_renderer,
            persistent_instances: Vec::new(),
            persistent_block_rects: Vec::new(),
            last_font_size: -1.0,
            last_line_height: -1.0,
            theme_mode,
            color_scheme: None,
            damage_tracker: DamageTracker::new(),
        }
    }

    pub fn set_theme_mode(&mut self, theme_mode: TerminalThemeMode) {
        if self.theme_mode == theme_mode {
            return;
        }
        self.theme_mode = theme_mode;
    }

    pub fn set_color_scheme(&mut self, scheme: ColorScheme) {
        self.color_scheme = Some(scheme);
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
        let usable_w = (width - 40.0).max(0.0);
        let usable_h = (height - 40.0).max(0.0);
        let cols = (usable_w / self.cell_width()).floor().max(2.0) as u16;
        let rows = (usable_h / self.cell_height()).floor().max(1.0) as u16;
        (cols, rows)
    }

    /// Compute centered grid padding — distributes remainder pixels evenly so
    /// partial rows never appear at the bottom edge.
    pub fn grid_padding(&self, width: f32, height: f32) -> (f32, f32) {
        let (cols, rows) = self.grid_size(width, height);
        let grid_w = cols as f32 * self.cell_width();
        let grid_h = rows as f32 * self.cell_height();
        let pad_x = ((width - grid_w) / 2.0).floor().max(0.0);
        let pad_y = ((height - grid_h) / 2.0).floor().max(0.0);
        (pad_x, pad_y)
    }

    pub fn render<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        // Skip rendering before layout provides valid surface dimensions.
        if width == 0 || height == 0 {
            return;
        }
        let render_start = std::time::Instant::now();
        let palette = self.active_palette();
        let (cols_u16, rows_u16) = self.grid_size(width as f32, height as f32);
        let cols = cols_u16 as usize;
        let rows = rows_u16 as usize;
        self.ensure_grid(cols, rows);

        // Collect damage BEFORE renderable_content (damage() requires &mut Term).
        self.damage_tracker.collect_damage(term, rows);

        // Only reset cells on damaged rows — undamaged rows keep previous frame state.
        for row in 0..rows {
            if self.damage_tracker.is_line_damaged(row) {
                for col_idx in 0..cols {
                    self.grid[row * cols + col_idx].active = false;
                }
                if row < self.persistent_block_rects.len() {
                    self.persistent_block_rects[row].clear();
                }
            }
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
        let (pad_x, pad_y) = self.grid_padding(w_f, h_f);

        self.rect_renderer.clear();

        let mut current_line: Option<i32> = None;
        let mut row_index = 0usize;
        let mut cursor_row: Option<usize> = None;
        let mut cursor_width_cells = 1u8;
        let mut changed_cells = 0usize;
        let mut active_cells = 0usize;
        let mut last_non_empty_row: Option<usize> = None;
        let mut prompt_debug_lines: Vec<Option<i32>> = vec![None; rows];
        let mut prompt_debug_cells: Vec<Vec<PromptDebugCell>> = vec![Vec::new(); rows];

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

            let cell = indexed.cell;

            if prompt_debug_lines[row_index].is_none() {
                prompt_debug_lines[row_index] = Some(line);
            }
            if col < 5 && prompt_debug_cells[row_index].len() < 5 {
                prompt_debug_cells[row_index].push(PromptDebugCell { col, ch: cell.c });
            }
            if cell_has_visible_content(cell) {
                last_non_empty_row = Some(row_index);
            }

            if cursor_visible && line == cursor_line {
                cursor_row = Some(row_index);
            }

            // Skip heavy cell processing for undamaged lines — their grid state
            // is preserved from the previous frame. Still track cursor width.
            if !self.damage_tracker.is_line_damaged(row_index) {
                if cursor_visible && line == cursor_line && col == cursor_col {
                    cursor_width_cells = if cell.flags.contains(Flags::WIDE_CHAR) && col + 1 < cols
                    {
                        2
                    } else {
                        1
                    };
                }
                continue;
            }

            let point = alacritty_terminal::index::Point::new(
                alacritty_terminal::index::Line(line),
                alacritty_terminal::index::Column(col),
            );
            let is_selected = selection.as_ref().map_or(false, |sel| sel.contains(point));

            if let Some(bg) = cell_background_rgba(cell, is_selected, &palette) {
                self.rect_renderer.push_rect(
                    pad_x + col as f32 * cw,
                    pad_y + row_index as f32 * lh,
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

            // Block drawing characters — pixel-perfect rects bypass glyphon.
            // Uses foreground color; rect exactly fills the fractional cell area.
            let first_ch = text.chars().next().unwrap_or(' ');
            if let Some((rx, ry, rw, rh)) = block_drawing_rect(first_ch) {
                let fg_color = cell_foreground(cell, is_selected, &palette);
                let fg_rgba = color_to_rect_rgba(fg_color);
                let cell_x = pad_x + col as f32 * cw;
                let cell_y = pad_y + row_index as f32 * lh;
                let verts = RectRenderer::make_rect_vertices(
                    cell_x + rx * cw, cell_y + ry * lh,
                    rw * cw, rh * lh, w_f, h_f, fg_rgba,
                );
                if row_index < self.persistent_block_rects.len() {
                    self.persistent_block_rects[row_index].extend_from_slice(&verts);
                }
                // Mark cell active with cleared text — no glyph needed, rect handles it.
                let slot = row_index * cols + col;
                let cell_mut = &mut self.grid[slot];
                cell_mut.active = true;
                cell_mut.text.clear();
                continue;
            }

            // Quadrant block elements — multiple pixel-perfect rects per cell.
            if let Some(rects) = quadrant_drawing_rects(first_ch) {
                let fg_color = cell_foreground(cell, is_selected, &palette);
                let fg_rgba = color_to_rect_rgba(fg_color);
                let cell_x = pad_x + col as f32 * cw;
                let cell_y = pad_y + row_index as f32 * lh;
                if row_index < self.persistent_block_rects.len() {
                    for &(rx, ry, rw, rh) in rects {
                        let verts = RectRenderer::make_rect_vertices(
                            cell_x + rx * cw, cell_y + ry * lh,
                            rw * cw, rh * lh, w_f, h_f, fg_rgba,
                        );
                        self.persistent_block_rects[row_index].extend_from_slice(&verts);
                    }
                }
                let slot = row_index * cols + col;
                let cell_mut = &mut self.grid[slot];
                cell_mut.active = true;
                cell_mut.text.clear();
                continue;
            }

            let fg = cell_foreground(cell, is_selected, &palette);
            let width_cells = if cell.flags.contains(Flags::WIDE_CHAR) && col + 1 < cols {
                2
            } else {
                1
            };

            let slot = row_index * cols + col;
            if self.update_cell(slot, &text, fg, width_cells) {
                changed_cells += 1;
            }
            active_cells += 1;
        }

        log_last_non_empty_line_cells(last_non_empty_row, &prompt_debug_lines, &prompt_debug_cells);

        // Append persistent block/quadrant rects (survive across frames for undamaged rows).
        for row_verts in &self.persistent_block_rects {
            self.rect_renderer.vertices.extend_from_slice(row_verts);
        }

        let rect_buffer = self.rect_renderer.prepare(device);

        let cell_w_px = cw.ceil() as u32;
        let cell_h_px = lh.ceil() as u32;

        // --- Update persistent CellInstance buffer (dirty rows only) ---
        let build_instances_start = std::time::Instant::now();
        let mut glyph_lookups = 0u32;
        let total_cells = rows * cols;

        for row in 0..rows {
            if !self.damage_tracker.is_line_damaged(row) {
                continue;
            }
            for col in 0..cols {
                let idx = row * cols + col;
                if idx >= total_cells {
                    break;
                }
                let cell = &self.grid[idx];
                if !cell.active || cell.text.is_empty() {
                    self.persistent_instances[idx] = CellInstance::default();
                    continue;
                }

                let ch = cell.text.chars().next().unwrap_or(' ');
                let glyph_info = self.glyph_cache.get_or_insert(
                    &mut self.cell_renderer.atlas,
                    device,
                    queue,
                    self.font_size,
                    ch,
                    cell_w_px,
                    cell_h_px,
                );
                glyph_lookups += 1;
                let uv_rect = match glyph_info {
                    Some(info) => info.uv_rect,
                    None => [0.0; 4],
                };

                let fg = [
                    cell.fg.r() as f32 / 255.0,
                    cell.fg.g() as f32 / 255.0,
                    cell.fg.b() as f32 / 255.0,
                    cell.fg.a() as f32 / 255.0,
                ];

                let mut flags = 0u32;
                if cell.width_cells == 2 {
                    flags |= 128; // FLAG_WIDE
                }

                self.persistent_instances[idx] = CellInstance {
                    grid_pos: [col as f32, row as f32],
                    atlas_uv_rect: uv_rect,
                    fg_color: fg,
                    bg_color: [0.0, 0.0, 0.0, 0.0],
                    flags,
                    _pad: 0,
                };
            }
        }

        // Cursor instance at end of persistent buffer
        let mut instance_count = total_cells;
        if cursor_visible {
            if let Some(crow) = cursor_row {
                let cc = palette.cursor;
                let ch = '\u{2588}'; // █
                let glyph_info = self.glyph_cache.get_or_insert(
                    &mut self.cell_renderer.atlas,
                    device,
                    queue,
                    self.font_size,
                    ch,
                    cell_w_px,
                    cell_h_px,
                );
                let uv_rect = match glyph_info {
                    Some(info) => info.uv_rect,
                    None => [0.0; 4],
                };
                let mut flags = 16u32; // FLAG_CURSOR
                if cursor_width_cells == 2 {
                    flags |= 128; // FLAG_WIDE
                }
                self.persistent_instances[total_cells] = CellInstance {
                    grid_pos: [cursor_col as f32, crow as f32],
                    atlas_uv_rect: uv_rect,
                    fg_color: [
                        cc.r as f32 / 255.0,
                        cc.g as f32 / 255.0,
                        cc.b as f32 / 255.0,
                        1.0,
                    ],
                    bg_color: [0.0, 0.0, 0.0, 0.0],
                    flags,
                    _pad: 0,
                };
                instance_count += 1;
            }
        }

        let build_instances_elapsed = build_instances_start.elapsed();

        // Upload instances and sync atlas bind group (new glyphs may have been added)
        self.cell_renderer.sync_atlas_bind_group(device);
        self.cell_renderer
            .update_uniforms(queue, [cw, lh], [pad_x, pad_y], [w_f, h_f]);
        self.cell_renderer.update_instances(
            device,
            queue,
            &self.persistent_instances[..instance_count],
        );

        // --- GPU render pass ---
        let gpu_start = std::time::Instant::now();
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

            // Scissor rect: clip rendering exactly to the grid area so no
            // partial rows or glyphs bleed beyond the grid boundary.
            {
                let grid_w = cols as f32 * cw;
                let grid_h = rows as f32 * lh;
                pass.set_scissor_rect(
                    pad_x as u32,
                    pad_y as u32,
                    (grid_w.ceil() as u32).min(width),
                    (grid_h.ceil() as u32).min(height),
                );
            }

            // 1. Background rectangles (selection, colored cell backgrounds)
            if let Some(ref buf) = rect_buffer {
                pass.set_pipeline(&self.rect_renderer.pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.rect_renderer.vertices.len() as u32, 0..1);
            }

            // 2. Cell glyphs — single instanced draw call
            if self.cell_renderer.instance_count > 0 {
                pass.set_pipeline(&self.cell_renderer.pipeline);
                pass.set_bind_group(0, &self.cell_renderer.atlas_bind_group, &[]);
                pass.set_bind_group(1, &self.cell_renderer.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.cell_renderer.instance_buffer.slice(..));
                pass.draw(0..6, 0..self.cell_renderer.instance_count);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
        let gpu_elapsed = gpu_start.elapsed();

        let total_elapsed = render_start.elapsed();
        if total_elapsed.as_millis() > 8 {
            debug_log!(
                "[perf] build={:.1}ms gpu={:.1}ms total={:.1}ms | instances={} glyph_lookups={} active={} changed={} damaged={}/{} full={}",
                build_instances_elapsed.as_secs_f64() * 1000.0,
                gpu_elapsed.as_secs_f64() * 1000.0,
                total_elapsed.as_secs_f64() * 1000.0,
                instance_count,
                glyph_lookups,
                active_cells,
                changed_cells,
                self.damage_tracker.damaged_rows.iter().filter(|&&d| d).count(),
                rows,
                self.damage_tracker.full_damage,
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

        if self.persistent_instances.len() != target + 1 {
            self.persistent_instances.clear();
            self.persistent_instances
                .resize(target + 1, CellInstance::default());
        }

        self.persistent_block_rects.resize_with(rows, Vec::new);
        self.persistent_block_rects.truncate(rows);

        if self.font_size != self.last_font_size || self.line_height != self.last_line_height {
            // Font metrics changed — clear atlas, glyph cache, and instance buffer
            self.cell_renderer.atlas.clear();
            self.glyph_cache.clear();
            for inst in self.persistent_instances.iter_mut() {
                *inst = CellInstance::default();
            }
            for row_rects in self.persistent_block_rects.iter_mut() {
                row_rects.clear();
            }
            self.last_font_size = self.font_size;
            self.last_line_height = self.line_height;
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
}

fn cell_text(cell: &Cell) -> String {
    if cell.flags.contains(Flags::HIDDEN) {
        return String::new();
    }

    let mut text = String::new();
    match cell.c {
        c if is_blank_cell_char(c) => {}
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

/// Returns fractional rect geometry (x, y, w, h) relative to cell dimensions
/// for Unicode Block Elements (U+2580–U+2595).  These are rendered as
/// pixel-perfect colored rectangles instead of font glyphs, eliminating
/// line-height gaps that occur when glyphon/cosmic-text rasterises them.
/// Shade characters (U+2591–U+2593) are left to the font renderer.
fn block_drawing_rect(ch: char) -> Option<(f32, f32, f32, f32)> {
    match ch {
        '\u{2580}' => Some((0.0, 0.0, 1.0, 0.5)),     // ▀ Upper half
        '\u{2581}' => Some((0.0, 0.875, 1.0, 0.125)),  // ▁ Lower ⅛
        '\u{2582}' => Some((0.0, 0.75, 1.0, 0.25)),    // ▂ Lower ¼
        '\u{2583}' => Some((0.0, 0.625, 1.0, 0.375)),  // ▃ Lower ⅜
        '\u{2584}' => Some((0.0, 0.5, 1.0, 0.5)),      // ▄ Lower half
        '\u{2585}' => Some((0.0, 0.375, 1.0, 0.625)),  // ▅ Lower ⅝
        '\u{2586}' => Some((0.0, 0.25, 1.0, 0.75)),    // ▆ Lower ¾
        '\u{2587}' => Some((0.0, 0.125, 1.0, 0.875)),  // ▇ Lower ⅞
        '\u{2588}' => Some((0.0, 0.0, 1.0, 1.0)),      // █ Full block
        '\u{2589}' => Some((0.0, 0.0, 0.875, 1.0)),    // ▉ Left ⅞
        '\u{258A}' => Some((0.0, 0.0, 0.75, 1.0)),     // ▊ Left ¾
        '\u{258B}' => Some((0.0, 0.0, 0.625, 1.0)),    // ▋ Left ⅝
        '\u{258C}' => Some((0.0, 0.0, 0.5, 1.0)),      // ▌ Left half
        '\u{258D}' => Some((0.0, 0.0, 0.375, 1.0)),    // ▍ Left ⅜
        '\u{258E}' => Some((0.0, 0.0, 0.25, 1.0)),     // ▎ Left ¼
        '\u{258F}' => Some((0.0, 0.0, 0.125, 1.0)),    // ▏ Left ⅛
        '\u{2590}' => Some((0.5, 0.0, 0.5, 1.0)),      // ▐ Right half
        '\u{2594}' => Some((0.0, 0.0, 1.0, 0.125)),    // ▔ Upper ⅛
        '\u{2595}' => Some((0.875, 0.0, 0.125, 1.0)),  // ▕ Right ⅛
        '\u{2500}' => Some((0.0, 0.46875, 1.0, 0.0625)), // ─ Box light horizontal
        _ => None,
    }
}

/// Returns multiple fractional rect geometries for Quadrant Block Elements
/// (U+2596–U+259F).  Each quadrant fills one quarter of the cell; combined
/// quadrants produce the full character.
fn quadrant_drawing_rects(ch: char) -> Option<&'static [(f32, f32, f32, f32)]> {
    const UL: (f32, f32, f32, f32) = (0.0, 0.0, 0.5, 0.5);
    const UR: (f32, f32, f32, f32) = (0.5, 0.0, 0.5, 0.5);
    const LL: (f32, f32, f32, f32) = (0.0, 0.5, 0.5, 0.5);
    const LR: (f32, f32, f32, f32) = (0.5, 0.5, 0.5, 0.5);
    match ch {
        '\u{2596}' => Some(&[LL]),             // ▖ Lower Left
        '\u{2597}' => Some(&[LR]),             // ▗ Lower Right
        '\u{2598}' => Some(&[UL]),             // ▘ Upper Left
        '\u{2599}' => Some(&[UL, LL, LR]),     // ▙ UL+LL+LR
        '\u{259A}' => Some(&[UL, LR]),         // ▚ UL+LR (diagonal)
        '\u{259B}' => Some(&[UL, UR, LL]),     // ▛ UL+UR+LL
        '\u{259C}' => Some(&[UL, UR, LR]),     // ▜ UL+UR+LR
        '\u{259D}' => Some(&[UR]),             // ▝ Upper Right
        '\u{259E}' => Some(&[UR, LL]),         // ▞ UR+LL (diagonal)
        '\u{259F}' => Some(&[UR, LL, LR]),     // ▟ UR+LL+LR
        _ => None,
    }
}

fn cell_has_visible_content(cell: &Cell) -> bool {
    !is_blank_cell_char(cell.c)
        || cell
            .zerowidth()
            .map(|chars| !chars.is_empty())
            .unwrap_or(false)
}

fn is_blank_cell_char(ch: char) -> bool {
    matches!(ch, '\0' | ' ' | '\u{00A0}')
}

fn log_last_non_empty_line_cells(
    row: Option<usize>,
    term_lines: &[Option<i32>],
    samples: &[Vec<PromptDebugCell>],
) {
    let Some(row) = row else {
        return;
    };
    let Some(cells) = samples.get(row) else {
        return;
    };
    if cells.is_empty() {
        return;
    }

    let term_line = term_lines
        .get(row)
        .and_then(|line| *line)
        .unwrap_or_default();
    let cell_summary = cells
        .iter()
        .map(|cell| {
            format!(
                "c{}='{}'/U+{:04X}",
                cell.col,
                format_prompt_debug_char(cell.ch),
                cell.ch as u32
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let message = format!(
        "[aterm] last non-empty line row={} term_line={} cells: {}",
        row, term_line, cell_summary
    );

    static LAST_LOGGED_PROMPT_CELLS: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    let last_logged = LAST_LOGGED_PROMPT_CELLS.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = last_logged.lock() {
        if slot.as_ref() == Some(&message) {
            return;
        }
        *slot = Some(message.clone());
    }

    debug_log!("{}", message);
}

fn format_prompt_debug_char(ch: char) -> String {
    match ch {
        '\0' => "\\0".to_string(),
        '\t' => "\\t".to_string(),
        ' ' => "space".to_string(),
        _ => ch.escape_default().to_string(),
    }
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

/// Load monospace font bytes from the FontSystem's font database.
/// Scans for monospace faces and reads the first available font file.
fn load_monospace_font_data(font_system: &FontSystem) -> (String, Vec<u8>) {
    static PRIMARY_FONT: OnceLock<(String, Vec<u8>)> = OnceLock::new();

    let (family_name, bytes) = PRIMARY_FONT.get_or_init(|| select_monospace_font_data(font_system));
    (family_name.clone(), bytes.clone())
}

fn select_monospace_font_data(font_system: &FontSystem) -> (String, Vec<u8>) {
    // Preferred fonts — try to match the Nerd Font / system font detection order.
    let preferred: &[&str] = &[
        "JetBrainsMono Nerd Font Mono",
        "JetBrainsMono Nerd Font",
        "MesloLGS NF",
        "MesloLGS Nerd Font Mono",
        "Hack Nerd Font Mono",
        "SF Mono",
        "Menlo",
        "Monaco",
    ];

    // First pass: try preferred families in order.
    for family_name in preferred {
        for face in font_system.db().faces() {
            if !face.monospaced {
                continue;
            }
            let matches = face
                .families
                .iter()
                .any(|(f, _)| f.eq_ignore_ascii_case(family_name));
            if !matches {
                continue;
            }
            if let Some(bytes) = read_font_source(&face.source) {
                log_stderr!("[aterm] glyph cache font: {}", family_name);
                return ((*family_name).to_string(), bytes);
            }
        }
    }

    // Fallback: any monospace face that we can read.
    for face in font_system.db().faces() {
        if !face.monospaced {
            continue;
        }
        if let Some(bytes) = read_font_source(&face.source) {
            let name = face
                .families
                .first()
                .map(|(f, _)| f.as_str())
                .unwrap_or("unknown");
            log_stderr!("[aterm] glyph cache font (fallback): {}", name);
            return (name.to_string(), bytes);
        }
    }

    panic!("[aterm] no monospace font found for glyph rasterizer");
}

/// Extract raw bytes from a fontdb Source.
fn read_font_source(source: &glyphon::cosmic_text::fontdb::Source) -> Option<Vec<u8>> {
    use glyphon::cosmic_text::fontdb::Source;
    match source {
        Source::File(path) => std::fs::read(path).ok(),
        Source::SharedFile(path, _) => std::fs::read(path).ok(),
        Source::Binary(data) => Some(data.as_ref().as_ref().to_vec()),
    }
}

fn load_fallback_font_data(font_system: &FontSystem) -> Vec<(String, Vec<u8>, u32)> {
    static FALLBACK_FONTS: OnceLock<Vec<(String, Vec<u8>, u32)>> = OnceLock::new();

    FALLBACK_FONTS
        .get_or_init(|| select_fallback_font_data(font_system))
        .clone()
}

fn select_fallback_font_data(font_system: &FontSystem) -> Vec<(String, Vec<u8>, u32)> {
    // Primary font is always tried first in renderer_glyph.rs.
    // Fallback order should keep standard monospace/symbol faces ahead of
    // decorative dingbat fonts like Zapf Dingbats.
    #[cfg(target_os = "macos")]
    const FALLBACK_FAMILIES: &[&str] = &[
        // Standard monospace/symbol fallbacks
        "Menlo",
        "Apple Symbols",
        "Noto Sans Symbols 2",
        "Symbola",
        "Apple Color Emoji",
        "SF Pro",
        "SF Pro Text",
        "Lucida Grande",
        // CJK (Korean, Chinese, Japanese)
        "Apple SD Gothic Neo",
        "AppleSDGothicNeo",
        "PingFang SC",
        "PingFang TC",
        "Hiragino Sans",
        "Hiragino Kaku Gothic ProN",
        "Noto Sans CJK KR",
        "Noto Sans CJK SC",
        // Broad Unicode fallback
        "Arial Unicode MS",
        "LastResort",
        // Decorative dingbats last: some Unicode codepoints have stylistic forms
        // here that do not match standard terminal glyph expectations.
        "Zapf Dingbats",
    ];
    #[cfg(not(target_os = "macos"))]
    const FALLBACK_FAMILIES: &[&str] = &[
        // Symbols + Emoji
        "Noto Sans Symbols",
        "Noto Sans Symbols 2",
        "Noto Color Emoji",
        "DejaVu Sans",
        "FreeSans",
        // CJK
        "Noto Sans CJK KR",
        "Noto Sans CJK SC",
        "Noto Sans CJK TC",
        "Noto Sans CJK JP",
        "Source Han Sans",
        "WenQuanYi Micro Hei Mono",
        // Broad fallback
        "Symbola",
        "Unifont",
    ];

    let mut result: Vec<(String, Vec<u8>, u32)> = Vec::new();
    let mut loaded = std::collections::HashSet::<String>::new();
    for family_name in FALLBACK_FAMILIES {
        if loaded.contains(*family_name) {
            continue;
        }
        for face in font_system.db().faces() {
            let matches = face
                .families
                .iter()
                .any(|(f, _)| f.eq_ignore_ascii_case(family_name));
            if !matches {
                continue;
            }
            if let Some(bytes) = read_font_source(&face.source) {
                log_stderr!(
                    "[aterm] fallback font: {} (index={})",
                    family_name,
                    face.index
                );
                loaded.insert(family_name.to_string());
                result.push((family_name.to_string(), bytes, face.index));
                break;
            }
        }
    }
    result
}

fn configure_terminal_font_system(font_system: &mut FontSystem) {
    if let Some(family) = detect_nerd_font_family(font_system) {
        font_system.db_mut().set_monospace_family(family.clone());
        log_stderr!("[aterm-core] renderer using Nerd Font monospace family: {family}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use fontdue::{Font, FontSettings};
    use std::sync::OnceLock;

    fn active_cell(text: &str, fg: GlyphonColor) -> GridCell {
        GridCell {
            text: text.to_string(),
            fg,
            width_cells: 1,
            active: true,
        }
    }

    fn inactive_cell(fg: GlyphonColor) -> GridCell {
        GridCell {
            text: String::new(),
            fg,
            width_cells: 1,
            active: false,
        }
    }

    fn headless_device_and_queue() -> (&'static wgpu::Device, &'static wgpu::Queue) {
        static GPU: OnceLock<(wgpu::Device, wgpu::Queue)> = OnceLock::new();
        let (device, queue) = GPU.get_or_init(|| {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..Default::default()
            });
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                }))
                .expect("expected headless GPU adapter for renderer tests");
            pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("renderer_tests"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            ))
            .expect("expected headless wgpu device for renderer tests")
        });
        (device, queue)
    }

    fn test_font_bytes_for(ch: char) -> Vec<u8> {
        let mut font_system = FontSystem::new();
        configure_terminal_font_system(&mut font_system);

        for prefer_monospaced in [true, false] {
            for face in font_system.db().faces() {
                if face.monospaced != prefer_monospaced {
                    continue;
                }
                let Some(bytes) = read_font_source(&face.source) else {
                    continue;
                };
                let Ok(font) = Font::from_bytes(bytes.clone(), FontSettings::default()) else {
                    continue;
                };
                if font.lookup_glyph_index(ch) != 0 {
                    return bytes;
                }
            }
        }

        panic!("no readable font found for test glyph {ch:?}");
    }

    fn test_glyph_cache_for(ch: char) -> GlyphCache {
        GlyphCache::from_bytes(99, test_font_bytes_for(ch))
            .expect("expected test glyph cache to load font bytes")
    }

    fn cache_ascii_glyph_for_reset_test(
        renderer: &mut TerminalGridRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> f32 {
        let size_px = renderer.font_size;
        let cell_w_px = renderer.cell_width().ceil() as u32;
        let cell_h_px = renderer.cell_height().ceil() as u32;
        let info = renderer
            .glyph_cache
            .get_or_insert(
                &mut renderer.cell_renderer.atlas,
                device,
                queue,
                size_px,
                'A',
                cell_w_px,
                cell_h_px,
            )
            .expect("expected cached ASCII glyph");

        assert_eq!(info.width, cell_w_px);
        assert_eq!(info.height, cell_h_px);
        assert_eq!(info.cell_width, 1);
        assert!(renderer.glyph_cache.get(size_px, 'A').is_some());
        size_px
    }

    #[test]
    fn run_based_batching_produces_fewer_text_areas_than_per_cell() {
        let red = GlyphonColor::rgb(255, 0, 0);
        let blue = GlyphonColor::rgb(0, 120, 255);
        let grid = vec![
            active_cell("a", red),
            active_cell("b", red),
            active_cell("c", red),
            active_cell("d", blue),
            active_cell("e", blue),
            inactive_cell(red),
            active_cell("f", blue),
            active_cell("g", blue),
        ];

        let per_cell_count = count_active_text_cells(&grid);
        let runs = build_text_runs(&grid, 4, 2);

        assert_eq!(per_cell_count, 7);
        assert_eq!(runs.len(), 4);
        assert!(runs.len() < per_cell_count);
        assert_eq!(runs[0].text, "abc");
        assert_eq!(runs[0].row, 0);
        assert_eq!(runs[0].start_col, 0);
        assert_eq!(runs[0].width_cells, 3);
    }

    #[test]
    fn aterm_core_render_does_not_panic_on_empty_grid() {
        let result = std::panic::catch_unwind(|| build_text_runs(&[], 0, 0));

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn aterm_core_render_does_not_panic_on_mismatched_grid_dimensions() {
        let red = GlyphonColor::rgb(255, 0, 0);
        let grid = vec![active_cell("a", red), active_cell("b", red)];

        let result = std::panic::catch_unwind(|| build_text_runs(&grid, 4, 2));

        assert!(result.is_ok());
        let runs = result.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "ab");
    }

    #[test]
    fn glyph_bitmap_padding_matches_cell_dimensions() {
        let (device, queue) = headless_device_and_queue();
        let mut glyph_cache = test_glyph_cache_for('A');
        let mut atlas = AtlasManager::with_size(256);
        let size_px = 18.0;
        let cell_w = 13;
        let cell_h = 29;

        let info = glyph_cache
            .get_or_insert(&mut atlas, device, queue, size_px, 'A', cell_w, cell_h)
            .expect("expected padded glyph info");

        assert_eq!(info.width, cell_w);
        assert_eq!(info.height, cell_h);
        assert_eq!(info.cell_width, 1);
    }

    #[test]
    fn atlas_clear_triggers_glyph_cache_clear_no_stale_glyphs() {
        let (device, queue) = headless_device_and_queue();
        let mut renderer = TerminalGridRenderer::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            TerminalThemeMode::Dark,
            1.0,
        );
        renderer.last_font_size = renderer.font_size;
        renderer.last_line_height = renderer.line_height;

        let cached_size = cache_ascii_glyph_for_reset_test(&mut renderer, device, queue);
        assert_eq!(renderer.cell_renderer.atlas.page_count(), 1);

        renderer.set_line_height(renderer.line_height + 4.0);
        renderer.ensure_grid(2, 2);

        assert_eq!(renderer.cell_renderer.atlas.page_count(), 0);
        assert!(renderer.glyph_cache.get(cached_size, 'A').is_none());
        assert_eq!(renderer.last_font_size, renderer.font_size);
        assert_eq!(renderer.last_line_height, renderer.line_height);
    }

    #[test]
    fn font_size_change_resets_both_atlas_and_glyph_cache() {
        let (device, queue) = headless_device_and_queue();
        let mut renderer = TerminalGridRenderer::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            TerminalThemeMode::Dark,
            1.0,
        );
        renderer.last_font_size = renderer.font_size;
        renderer.last_line_height = renderer.line_height;

        let cached_size = cache_ascii_glyph_for_reset_test(&mut renderer, device, queue);

        renderer.set_font_size(renderer.font_size + 2.0);
        renderer.ensure_grid(4, 3);

        assert_eq!(renderer.cell_renderer.atlas.page_count(), 0);
        assert!(renderer.glyph_cache.get(cached_size, 'A').is_none());
        assert_eq!(renderer.last_font_size, renderer.font_size);
        assert_eq!(renderer.last_line_height, renderer.line_height);
    }

    #[test]
    fn ascii_preload_uses_cell_dimensions() {
        let (device, queue) = headless_device_and_queue();
        let mut glyph_cache = test_glyph_cache_for('A');
        let mut atlas = AtlasManager::with_size(256);
        let size_px = 18.0;
        let cell_w = 14;
        let cell_h = 28;

        glyph_cache.preload_ascii(&mut atlas, device, queue, size_px, cell_w, cell_h);

        assert!(atlas.page_count() > 0);
        for ch in ['A', '0', '~'] {
            let info = glyph_cache
                .get(size_px, ch)
                .unwrap_or_else(|| panic!("expected preloaded ASCII glyph {ch:?}"));
            assert_eq!(info.width, cell_w);
            assert_eq!(info.height, cell_h);
            assert_eq!(info.cell_width, 1);
        }
    }

    #[test]
    fn cjk_wide_char_padding_matches_two_cell_width() {
        let (device, queue) = headless_device_and_queue();
        let mut glyph_cache = test_glyph_cache_for('가');
        let mut atlas = AtlasManager::with_size(256);
        let size_px = 18.0;
        let cell_w = 14;
        let cell_h = 28;

        let info = glyph_cache
            .get_or_insert(&mut atlas, device, queue, size_px, '가', cell_w, cell_h)
            .expect("expected padded CJK glyph info");

        assert_eq!(info.width, cell_w * 2);
        assert_eq!(info.height, cell_h);
        assert_eq!(info.cell_width, 2);
    }

    #[test]
    fn direct_background_colors_render_as_provided() {
        let palette = theme_palette(TerminalThemeMode::Dark);
        let cell = Cell {
            c: 'x',
            fg: AnsiColor::Named(NamedColor::White),
            bg: AnsiColor::Spec(alacritty_terminal::vte::ansi::Rgb {
                r: 0x24,
                g: 0x28,
                b: 0x31,
            }),
            ..Cell::default()
        };

        let bg =
            cell_background_rgba(&cell, false, &palette).expect("expected explicit background");
        assert_eq!(
            bg,
            [
                0x24 as f32 / 255.0,
                0x28 as f32 / 255.0,
                0x31 as f32 / 255.0,
                1.0,
            ]
        );
    }

    #[test]
    fn nbsp_is_treated_as_blank_cell_char() {
        assert!(is_blank_cell_char('\u{00A0}'));
        assert_eq!(
            cell_text(&Cell {
                c: '\u{00A0}',
                ..Cell::default()
            }),
            ""
        );
    }

    #[test]
    fn run_based_batching_handles_single_character_line() {
        let red = GlyphonColor::rgb(255, 0, 0);
        let grid = vec![active_cell("x", red)];

        let runs = build_text_runs(&grid, 1, 1);

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "x");
        assert_eq!(runs[0].row, 0);
        assert_eq!(runs[0].start_col, 0);
        assert_eq!(runs[0].width_cells, 1);
    }

    #[test]
    fn run_based_batching_handles_empty_line() {
        let red = GlyphonColor::rgb(255, 0, 0);
        let grid = vec![inactive_cell(red), inactive_cell(red), inactive_cell(red)];

        let runs = build_text_runs(&grid, 3, 1);

        assert!(runs.is_empty());
    }

    #[test]
    fn run_based_batching_handles_line_with_all_different_attributes() {
        let red = GlyphonColor::rgb(255, 0, 0);
        let green = GlyphonColor::rgb(0, 255, 0);
        let blue = GlyphonColor::rgb(0, 0, 255);
        let grid = vec![
            active_cell("a", red),
            active_cell("b", green),
            active_cell("c", blue),
        ];

        let runs = build_text_runs(&grid, 3, 1);

        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "a");
        assert_eq!(runs[1].text, "b");
        assert_eq!(runs[2].text, "c");
        assert_eq!(runs[0].start_col, 0);
        assert_eq!(runs[1].start_col, 1);
        assert_eq!(runs[2].start_col, 2);
    }

    #[test]
    fn damage_tracker_partial_damage_skips_unchanged_lines() {
        let mut tracker = DamageTracker::new();

        tracker.apply_damage_rows(5, false, &[1, 3]);

        assert!(!tracker.full_damage);
        assert_eq!(tracker.damaged_rows, vec![false, true, false, true, false]);
    }

    #[test]
    fn damage_tracker_full_damage_processes_all_lines() {
        let mut tracker = DamageTracker::new();

        tracker.apply_damage_rows(4, true, &[]);

        assert!(tracker.full_damage);
        assert_eq!(tracker.damaged_rows, vec![true, true, true, true]);
        assert!((0..4).all(|row| tracker.is_line_damaged(row)));
    }

    #[test]
    fn damage_tracker_is_line_damaged_returns_false_for_unchanged_lines() {
        let mut tracker = DamageTracker::new();

        tracker.apply_damage_rows(4, false, &[2]);

        assert!(!tracker.is_line_damaged(0));
        assert!(!tracker.is_line_damaged(1));
        assert!(tracker.is_line_damaged(2));
        assert!(!tracker.is_line_damaged(3));
    }
}
