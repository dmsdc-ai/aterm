// Shaders.metal — aterm GPU terminal renderer
//
// Five render pipelines for the terminal:
//   1. bg_color    — fullscreen background fill
//   2. cell_bg     — per-cell background colors
//   3. cell_text   — instanced glyph rendering from atlas
//   4. cursor      — cursor overlay (block / underline / bar)
//   5. selection   — selection highlight overlay
//
// Pixel format: bgra8Unorm_srgb (Metal handles sRGB gamma automatically)
// Coordinate system: pixel coordinates projected via orthographic matrix

#include <metal_stdlib>
using namespace metal;

// ---------------------------------------------------------------------------
// MARK: - Shared Structures
// ---------------------------------------------------------------------------

/// GPU uniforms — MUST match Swift-side layout exactly (144 bytes).
/// Explicit padding ensures no implicit compiler gaps.
struct Uniforms {
    float4x4 projection_matrix;  // 64 bytes, offset 0
    float2   screen_size;        //  8 bytes, offset 64
    float2   cell_size;          //  8 bytes, offset 72
    uint2    grid_size;          //  8 bytes, offset 80 (cols, rows)
    float2   _pad0;              //  8 bytes, offset 88 (align float4 at 96)
    float4   grid_padding;       // 16 bytes, offset 96 (top, right, bottom, left)
    float2   cursor_pos;         //  8 bytes, offset 112 (col, row as float)
    uchar4   cursor_color;       //  4 bytes, offset 120
    uchar4   bg_color;           //  4 bytes, offset 124
    uint     flags;              //  4 bytes, offset 128 (bit 0: cursor_wide, bit 1: cursor_visible)
    float    scale;              //  4 bytes, offset 132 (display contentsScale for Retina glyph sizing)
    float    min_contrast;       //  4 bytes, offset 136 (WCAG 2.0 min contrast ratio, 1.0=disabled)
    float    _pad3;              //  4 bytes, offset 140
    // Total: 144 bytes
};

/// Per-cell text instance data (32 bytes, tightly packed).
struct CellText {
    uint2    glyph_pos;   //  8 bytes — atlas pixel position (u, v)
    uint2    glyph_size;  //  8 bytes — glyph pixel size (w, h)
    short2   bearings;    //  4 bytes — glyph bearings (x, y)
    ushort2  grid_pos;    //  4 bytes — grid column, row
    uchar4   fg_color;    //  4 bytes — foreground RGBA
    uchar    atlas_type;  //  1 byte  — 0=grayscale, 1=color
    uchar    cell_flags;  //  1 byte  — bit 0: bold, bit 1: italic, bit 2: underline, bit 3: is_cursor_glyph
    uchar2   _pad;        //  2 bytes padding
};

/// Cursor rectangle descriptor (24 bytes).
struct CursorRect {
    float2 pos;    // pixel position
    float2 size;   // pixel size
    uchar4 color;  // RGBA
    uchar  style;  // 0=block, 1=underline, 2=bar
    uchar3 _pad;
};

/// Selection range descriptor (12 bytes).
struct SelectionRange {
    ushort2 start;  // col, row
    ushort2 end;    // col, row
    uchar4  color;  // selection highlight RGBA
};

// ---------------------------------------------------------------------------
// MARK: - Vertex Outputs
// ---------------------------------------------------------------------------

struct FullScreenVertexOut {
    float4 position [[position]];
    float2 tex_coord;
};

struct CellTextVertexOut {
    float4 position [[position]];
    float2 tex_coord;
    float4 fg_color;
    uint   atlas_type;
};

struct CursorVertexOut {
    float4 position [[position]];
    float4 color;
};

struct SelectionVertexOut {
    float4 position [[position]];
    float4 color;
};

// ---------------------------------------------------------------------------
// MARK: - Helpers
// ---------------------------------------------------------------------------

/// Expand a triangle-strip quad corner from vertex_id (0..3).
/// Returns (0,0), (1,0), (0,1), (1,1) for vid 0,1,2,3.
static inline float2 quad_corner(uint vid) {
    return float2(
        (vid == 1 || vid == 3) ? 1.0 : 0.0,
        (vid == 2 || vid == 3) ? 1.0 : 0.0
    );
}

/// Convert uchar4 RGBA [0..255] to float4 [0..1].
static inline float4 uchar4_to_float4(uchar4 c) {
    return float4(c) / 255.0;
}

/// Correct sRGB → linear conversion (IEC 61966-2-1 piecewise).
static inline float3 srgb_to_linear(float3 c) {
    return select(
        c / 12.92,
        pow((c + 0.055) / 1.055, float3(2.4)),
        c > float3(0.04045)
    );
}

/// WCAG 2.0 relative luminance (ITU-R BT.709).
static inline float luminance(float3 c) {
    return 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
}

/// WCAG 2.0 contrast ratio between two luminance values.
static inline float contrast_ratio(float l1, float l2) {
    float lighter = max(l1, l2);
    float darker  = min(l1, l2);
    return (lighter + 0.05) / (darker + 0.05);
}

/// Adjust fg color to meet minimum contrast ratio against bg (Ghostty pattern).
/// Returns fg unchanged if contrast is already sufficient or min_contrast <= 1.0.
static inline float3 contrasted_color(float3 fg, float3 bg, float min_contrast) {
    float fg_lum = luminance(fg);
    float bg_lum = luminance(bg);
    if (contrast_ratio(fg_lum, bg_lum) >= min_contrast) return fg;
    // Compute target luminance that satisfies the contrast ratio
    float target_lum = (bg_lum + 0.05) * min_contrast - 0.05;
    if (target_lum > 1.0) target_lum = (bg_lum + 0.05) / min_contrast - 0.05;
    float ratio = (target_lum - fg_lum) / (1.0 - fg_lum + 0.0001);
    return mix(fg, float3(target_lum > fg_lum ? 1.0 : 0.0), clamp(ratio, 0.0, 1.0));
}

// ---------------------------------------------------------------------------
// MARK: - Pipeline 1: bg_color (fullscreen background)
// ---------------------------------------------------------------------------

/// Fullscreen triangle (3 vertices, no vertex buffer needed).
/// Covers clip space [-1,1] with a single oversized triangle.
vertex FullScreenVertexOut bg_color_vertex(uint vid [[vertex_id]]) {
    FullScreenVertexOut out;

    // Three vertices that cover the entire screen:
    //   vid 0 → (-1, -3)   bottom-left  (below screen)
    //   vid 1 → (-1,  1)   top-left
    //   vid 2 → ( 3,  1)   top-right    (past screen)
    float2 positions[3] = {
        float2(-1.0, -3.0),
        float2(-1.0,  1.0),
        float2( 3.0,  1.0)
    };

    // Corresponding texture coordinates [0..1] (y-flipped for Metal)
    float2 texcoords[3] = {
        float2(0.0,  2.0),
        float2(0.0,  0.0),
        float2(2.0,  0.0)
    };

    out.position  = float4(positions[vid], 0.0, 1.0);
    out.tex_coord = texcoords[vid];
    return out;
}

/// Fill the entire screen with the terminal background color.
fragment float4 bg_color_fragment(
    FullScreenVertexOut in [[stage_in]],
    constant Uniforms&  u  [[buffer(0)]]
) {
    float4 color = uchar4_to_float4(u.bg_color);
    color.rgb = srgb_to_linear(color.rgb);
    return color;
}

// ---------------------------------------------------------------------------
// MARK: - Pipeline 2: cell_bg (per-cell background colors)
// ---------------------------------------------------------------------------

/// Fullscreen triangle vertex shader for the cell_bg pipeline.
/// Identical geometry to bg_color_vertex — covers entire screen so the
/// fragment shader can map every pixel to a grid cell and look up its color.
vertex FullScreenVertexOut cell_bg_vertex(uint vid [[vertex_id]]) {
    FullScreenVertexOut out;

    float2 positions[3] = {
        float2(-1.0, -3.0),
        float2(-1.0,  1.0),
        float2( 3.0,  1.0)
    };

    float2 texcoords[3] = {
        float2(0.0,  2.0),
        float2(0.0,  0.0),
        float2(2.0,  0.0)
    };

    out.position  = float4(positions[vid], 0.0, 1.0);
    out.tex_coord = texcoords[vid];
    return out;
}

/// Fragment shader: reads per-cell background from the cells_bg buffer.
fragment float4 cell_bg_fragment(
    FullScreenVertexOut       in       [[stage_in]],
    constant Uniforms&        u        [[buffer(0)]],
    device const uchar4*      cells_bg [[buffer(1)]]
) {
    // Fragment position in pixels
    float2 frag_px = in.tex_coord * u.screen_size;

    // Subtract grid padding: .w = left, .x = top
    float2 adjusted = frag_px - float2(u.grid_padding.w, u.grid_padding.x);

    // If outside the grid area (negative = in padding region), transparent
    if (adjusted.x < 0.0 || adjusted.y < 0.0) {
        discard_fragment();
    }

    // Compute cell column and row
    uint col = uint(adjusted.x / u.cell_size.x);
    uint row = uint(adjusted.y / u.cell_size.y);

    // Bounds check against grid dimensions
    if (col >= u.grid_size.x || row >= u.grid_size.y) {
        discard_fragment();
    }

    // Look up the per-cell background color
    uint  index = row * u.grid_size.x + col;
    uchar4 bg   = cells_bg[index];

    // Skip fully transparent cells — let the bg_color pass show through
    if (bg.a == 0) {
        discard_fragment();
    }

    // Convert to float and apply sRGB linearization.
    // Note: bgra8Unorm_srgb pixel format handles final linear→sRGB on write,
    // so we feed it linear values by removing the sRGB curve from our input.
    float4 color = uchar4_to_float4(bg);
    color.rgb = srgb_to_linear(color.rgb);
    return color;
}

// ---------------------------------------------------------------------------
// MARK: - Pipeline 3: cell_text (instanced glyph rendering)
// ---------------------------------------------------------------------------

/// Instanced vertex shader: one quad per glyph, 4 vertices per instance
/// (drawn as triangle strip: vid 0..3).
vertex CellTextVertexOut cell_text_vertex(
    uint                    vid   [[vertex_id]],
    uint                    iid   [[instance_id]],
    device const CellText*  cells [[buffer(0)]],
    constant Uniforms&      u     [[buffer(1)]]
) {
    CellTextVertexOut out;

    CellText cell = cells[iid];
    float2 corner = quad_corner(vid);

    // Cell top-left in pixel coordinates (grid_padding: .w=left, .x=top)
    float2 cell_origin = float2(u.grid_padding.w, u.grid_padding.x)
                       + float2(cell.grid_pos) * u.cell_size;

    // Foreground color (linearized for sRGB framebuffer) and atlas type — shared by all paths
    float4 fg = uchar4_to_float4(cell.fg_color);
    float3 fg_linear = srgb_to_linear(fg.rgb);

    // WCAG 2.0 min contrast: adjust fg if contrast against bg is too low
    if (u.min_contrast > 1.0) {
        float3 bg_linear = srgb_to_linear(uchar4_to_float4(u.bg_color).rgb);
        fg_linear = contrasted_color(fg_linear, bg_linear, u.min_contrast);
    }

    out.fg_color   = float4(fg_linear, fg.a);
    out.atlas_type = uint(cell.atlas_type);

    if (cell.atlas_type == 2) {
        // Block drawing character (U+2580-U+259F) — pixel-perfect rect.
        // Fields encode cell-relative fractions scaled by 256:
        //   bearings   = (x_start * 256, y_start * 256)
        //   glyph_size = (width * 256,   height * 256)
        // Multiplied by cell_size for exact pixel coverage — no rounding gaps.
        float2 frac_origin = float2(cell.bearings) / 256.0;
        float2 frac_sz     = float2(cell.glyph_size) / 256.0;
        float2 pos = cell_origin + (frac_origin + frac_sz * corner) * u.cell_size;
        out.position  = u.projection_matrix * float4(pos, 0.0, 1.0);
        out.tex_coord = float2(0);
        return out;
    }

    // Glyph pixel dimensions — atlas stores physical pixels (Retina 2x),
    // divide by scale to get logical quad size matching cell dimensions.
    float2 glyph_sz = float2(cell.glyph_size) / u.scale;

    // Apply glyph bearings (physical pixels from atlas, divide by scale for logical):
    //   bearing_x shifts right from cell left edge
    //   bearing_y is distance from baseline to glyph top;
    //   baseline sits at (cell_size.y - descent), so glyph top = cell_size.y - bearing_y
    float2 offset = float2(float(cell.bearings.x) / u.scale,
                           u.cell_size.y - float(cell.bearings.y) / u.scale);

    // Final pixel position of this vertex
    float2 pos = cell_origin + offset + glyph_sz * corner;

    // Project to clip space
    out.position = u.projection_matrix * float4(pos, 0.0, 1.0);

    // Texture coordinate in pixel space (sampler uses coord::pixel)
    out.tex_coord = float2(cell.glyph_pos) + float2(cell.glyph_size) * corner;

    return out;
}

/// Sample the glyph from the atlas and composite with foreground color.
/// Grayscale atlas: use alpha from texture, tint with fg_color (premultiplied).
/// Color atlas: use texture color directly (already premultiplied).
fragment float4 cell_text_fragment(
    CellTextVertexOut         in              [[stage_in]],
    texture2d<float>          atlas_grayscale [[texture(0)]],
    texture2d<float>          atlas_color     [[texture(1)]]
) {
    constexpr sampler s(coord::pixel, address::clamp_to_edge, filter::nearest);

    if (in.atlas_type == 2) {
        // Block drawing character — solid color rect, no atlas sampling.
        // Premultiply alpha for blending (src=one, dst=oneMinusSourceAlpha).
        float4 c = in.fg_color;
        c.rgb *= c.a;
        return c;
    }

    if (in.atlas_type == 0) {
        // Grayscale glyph: .r channel holds coverage (ghostty pattern)
        float a = atlas_grayscale.sample(s, in.tex_coord).r;
        float4 color = in.fg_color;
        color *= a;  // premultiplied alpha
        return color;
    } else {
        // Color glyph (emoji, etc.): already premultiplied in atlas
        return atlas_color.sample(s, in.tex_coord);
    }
}

// ---------------------------------------------------------------------------
// MARK: - Pipeline 4: cursor
// ---------------------------------------------------------------------------

/// Draw the cursor as a simple colored quad.
/// Style determines shape: block (full cell), underline (bottom 2px), bar (left 2px).
vertex CursorVertexOut cursor_vertex(
    uint                   vid    [[vertex_id]],
    constant Uniforms&     u      [[buffer(0)]],
    constant CursorRect&   cursor [[buffer(1)]]
) {
    CursorVertexOut out;

    float2 corner = quad_corner(vid);

    // Compute the cursor rectangle based on style
    float2 pos  = cursor.pos;
    float2 size = cursor.size;

    if (cursor.style == 1) {
        // Underline: bottom 2 pixels of the cell
        pos.y  = cursor.pos.y + cursor.size.y - 2.0;
        size.y = 2.0;
    } else if (cursor.style == 2) {
        // Bar: left 2 pixels of the cell
        size.x = 2.0;
    }
    // style == 0 (block): use full pos/size as-is

    float2 vertex_pos = pos + size * corner;

    out.position = u.projection_matrix * float4(vertex_pos, 0.0, 1.0);
    out.color    = uchar4_to_float4(cursor.color);

    return out;
}

/// Pass through the cursor color (linearized for sRGB framebuffer).
fragment float4 cursor_fragment(CursorVertexOut in [[stage_in]]) {
    float4 color = in.color;
    color.rgb = srgb_to_linear(color.rgb);
    return color;
}

// ---------------------------------------------------------------------------
// MARK: - Pipeline 5: selection (highlight overlay)
// ---------------------------------------------------------------------------

/// Draw selection highlight quads. Each instance covers one contiguous
/// row-span of selected cells.
vertex SelectionVertexOut selection_vertex(
    uint                          vid    [[vertex_id]],
    uint                          iid    [[instance_id]],
    device const SelectionRange*  ranges [[buffer(0)]],
    constant Uniforms&            u      [[buffer(1)]]
) {
    SelectionVertexOut out;

    SelectionRange range = ranges[iid];
    float2 corner = quad_corner(vid);

    // Selection rectangle in pixel coordinates
    // grid_padding: .w=left, .x=top
    float2 origin = float2(u.grid_padding.w, u.grid_padding.x)
                  + float2(range.start) * u.cell_size;

    // Size spans from start to end (inclusive), so add 1 cell
    float2 span = float2(ushort2(range.end.x - range.start.x + 1,
                                  range.end.y - range.start.y + 1));
    float2 size = span * u.cell_size;

    float2 pos = origin + size * corner;

    out.position = u.projection_matrix * float4(pos, 0.0, 1.0);
    out.color    = uchar4_to_float4(range.color);

    return out;
}

/// Pass through the selection color (linearized for sRGB framebuffer).
/// Blending mode: source_alpha, one_minus_source_alpha (standard alpha blend).
fragment float4 selection_fragment(SelectionVertexOut in [[stage_in]]) {
    float4 color = in.color;
    color.rgb = srgb_to_linear(color.rgb);
    return color;
}
