// cell.wgsl — Instanced cell-based terminal renderer for aterm.
//
// Renders the entire terminal grid in a SINGLE draw call via instanced rendering.
// Each instance = one terminal cell with background rect + glyph from texture atlas.
//
// Draw call: draw(6 vertices, instance_count = rows * cols)
// Vertex IDs 0..5 form a unit quad; instance data positions each cell.

// ---------------------------------------------------------------------------
// Uniforms — grid metrics shared across all cells
// ---------------------------------------------------------------------------

struct GridUniforms {
    cell_size:     vec2<f32>,  // (width, height) of one cell in pixels
    grid_origin:   vec2<f32>,  // top-left pixel offset of the grid
    viewport_size: vec2<f32>,  // viewport (width, height) in pixels
    atlas_size:    vec2<f32>,  // glyph atlas texture (width, height) in pixels
};

@group(0) @binding(0)
var atlas_texture: texture_2d<f32>;

@group(0) @binding(1)
var atlas_sampler: sampler;

@group(1) @binding(0)
var<uniform> grid: GridUniforms;

// ---------------------------------------------------------------------------
// Instance data — one per terminal cell
// ---------------------------------------------------------------------------
//
// Passed via vertex buffer with step_mode = Instance.
//
// Rust-side layout (repr(C), 64 bytes per instance):
//   grid_pos:      [f32; 2]   — (col, row)
//   atlas_uv_rect: [f32; 4]   — (u_min, v_min, u_max, v_max) in 0..1 UV space
//   fg_color:      [f32; 4]   — foreground RGBA (linear)
//   bg_color:      [f32; 4]   — background RGBA (linear)
//   flags:         u32        — bitfield (see below)
//   _pad:          u32        — alignment padding to 64-byte stride
//
// Flag bits:
//   0: BOLD
//   1: ITALIC
//   2: UNDERLINE
//   3: STRIKETHROUGH
//   4: CURSOR
//   5: SELECTION
//   6: INVERSE  (swap fg ↔ bg)
//   7: WIDE     (2-cell width — stretch quad to 2× cell_size.x)

const FLAG_BOLD:          u32 = 1u;
const FLAG_ITALIC:        u32 = 2u;
const FLAG_UNDERLINE:     u32 = 4u;
const FLAG_STRIKETHROUGH: u32 = 8u;
const FLAG_CURSOR:        u32 = 16u;
const FLAG_SELECTION:     u32 = 32u;
const FLAG_INVERSE:       u32 = 64u;
const FLAG_WIDE:          u32 = 128u;

// ---------------------------------------------------------------------------
// Vertex / Fragment IO
// ---------------------------------------------------------------------------

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0)       uv:       vec2<f32>,  // interpolated atlas UV
    @location(1)       fg:       vec4<f32>,  // resolved foreground color
    @location(2)       bg:       vec4<f32>,  // resolved background color
    @interpolate(flat) @location(3) flags:   u32,
    @location(4)       cell_uv:  vec2<f32>,  // local 0..1 within the cell (for decorations)
};

// ---------------------------------------------------------------------------
// Vertex shader
// ---------------------------------------------------------------------------
//
// vertex_index 0..5 encodes a unit quad (two triangles, CCW):
//
//   0 ---- 1        vertices: 0(0,0) 1(1,0) 2(0,1) 2(0,1) 1(1,0) 3(1,1)
//   | \   |
//   |  \  |
//   |   \ |
//   2 ---- 3

@vertex
fn vs_main(
    @builtin(vertex_index)   vid:       u32,
    // -- instance attributes --
    @location(0) grid_pos:      vec2<f32>,
    @location(1) atlas_uv_rect: vec4<f32>,
    @location(2) fg_color:      vec4<f32>,
    @location(3) bg_color:      vec4<f32>,
    @location(4) flags:         u32,
) -> VertexOutput {
    // Unit quad corners from vertex_index (0..5 → two triangles)
    let quad_index = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),  // tri 0: top-left
        vec2<f32>(1.0, 0.0),  // tri 0: top-right
        vec2<f32>(0.0, 1.0),  // tri 0: bottom-left
        vec2<f32>(0.0, 1.0),  // tri 1: bottom-left
        vec2<f32>(1.0, 0.0),  // tri 1: top-right
        vec2<f32>(1.0, 1.0),  // tri 1: bottom-right
    );
    let corner = quad_index[vid];

    // Cell width multiplier (2× for wide chars)
    var width_mult = 1.0;
    if (flags & FLAG_WIDE) != 0u {
        width_mult = 2.0;
    }

    // Pixel position of this vertex
    let cell_pixel = vec2<f32>(
        grid.grid_origin.x + grid_pos.x * grid.cell_size.x + corner.x * grid.cell_size.x * width_mult,
        grid.grid_origin.y + grid_pos.y * grid.cell_size.y + corner.y * grid.cell_size.y,
    );

    // Convert pixel → NDC (wgpu: X -1..1 left→right, Y -1..1 bottom→top)
    let ndc = vec2<f32>(
         cell_pixel.x / grid.viewport_size.x *  2.0 - 1.0,
        -cell_pixel.y / grid.viewport_size.y *  2.0 + 1.0,
    );

    // Interpolate atlas UV from rect bounds
    let uv = vec2<f32>(
        mix(atlas_uv_rect.x, atlas_uv_rect.z, corner.x),
        mix(atlas_uv_rect.y, atlas_uv_rect.w, corner.y),
    );

    // Resolve inverse flag: swap fg ↔ bg
    var fg = fg_color;
    var bg = bg_color;
    if (flags & FLAG_INVERSE) != 0u {
        fg = bg_color;
        bg = fg_color;
    }

    // Selection tint — blend selection highlight into bg
    if (flags & FLAG_SELECTION) != 0u {
        bg = vec4<f32>(
            mix(bg.r, 0.25, 0.5),
            mix(bg.g, 0.45, 0.5),
            mix(bg.b, 0.75, 0.5),
            bg.a,
        );
    }

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv       = uv;
    out.fg       = fg;
    out.bg       = bg;
    out.flags    = flags;
    out.cell_uv  = corner;
    return out;
}

// ---------------------------------------------------------------------------
// Fragment shader
// ---------------------------------------------------------------------------
//
// 1. Start with background color (solid rect).
// 2. Sample glyph alpha from atlas texture.
// 3. Composite foreground over background using glyph alpha.
// 4. Draw decorations (underline, strikethrough, cursor) procedurally.

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let flags = in.flags;

    // -- Background layer --
    var color = in.bg;

    // -- Glyph layer: sample atlas alpha and composite fg --
    let glyph_sample = textureSample(atlas_texture, atlas_sampler, in.uv);
    // Atlas stores coverage in alpha channel (grayscale glyph mask).
    // For RGB subpixel atlases, use luminance; for alpha-only, use .a directly.
    let glyph_alpha = glyph_sample.a;

    // Composite: foreground over background using glyph coverage
    color = vec4<f32>(
        mix(color.rgb, in.fg.rgb, glyph_alpha * in.fg.a),
        max(color.a, glyph_alpha * in.fg.a),
    );

    // -- Underline decoration --
    // Draw a 1px line at ~90% of cell height
    if (flags & FLAG_UNDERLINE) != 0u {
        let underline_y = 0.9;
        let underline_thickness = 1.0 / 26.0;  // ~1px at 26px line height
        if in.cell_uv.y >= underline_y && in.cell_uv.y < (underline_y + underline_thickness * 2.0) {
            color = vec4<f32>(in.fg.rgb, in.fg.a);
        }
    }

    // -- Strikethrough decoration --
    // Draw a 1px line at ~45% of cell height
    if (flags & FLAG_STRIKETHROUGH) != 0u {
        let strike_y = 0.45;
        let strike_thickness = 1.0 / 26.0;
        if in.cell_uv.y >= strike_y && in.cell_uv.y < (strike_y + strike_thickness * 2.0) {
            color = vec4<f32>(in.fg.rgb, in.fg.a);
        }
    }

    // -- Cursor overlay --
    // Block cursor: full cell tint. Bar/underline cursors handled by flags on host.
    if (flags & FLAG_CURSOR) != 0u {
        // Invert the composited color for maximum visibility
        color = vec4<f32>(
            vec3<f32>(1.0) - color.rgb,
            1.0,
        );
    }

    return color;
}
