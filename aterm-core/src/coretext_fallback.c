// Core Text font fallback for glyph rendering.
// Called when fontdue (pure-Rust rasterizer) has no glyph in any loaded font.
// Uses CTFontCreateForString — the same API used by Ghostty, Alacritty, Kitty,
// WezTerm, and Contour for macOS system font fallback.

#include <CoreText/CoreText.h>
#include <CoreGraphics/CoreGraphics.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    uint8_t* bitmap;        // RGBA (R=255,G=255,B=255,A=coverage), caller frees
    uint32_t width;
    uint32_t height;
    int32_t  xmin;          // left bearing (matches fontdue Metrics.xmin)
    int32_t  ymin;          // bottom of glyph relative to baseline, y-up (matches fontdue Metrics.ymin)
    float    advance_width;
    float    ascent;        // positive
    float    descent;       // negative (below baseline)
} CoreTextGlyphResult;

int32_t aterm_coretext_rasterize_glyph(
    uint32_t codepoint,
    const char* base_font_name,
    float font_size,
    CoreTextGlyphResult* result
) {
    if (!result) return 0;
    memset(result, 0, sizeof(CoreTextGlyphResult));

    // Encode codepoint as UTF-16
    UniChar chars[2];
    CFIndex char_count;
    if (codepoint > 0xFFFF) {
        uint32_t cp = codepoint - 0x10000;
        chars[0] = (UniChar)(0xD800 + (cp >> 10));
        chars[1] = (UniChar)(0xDC00 + (cp & 0x3FF));
        char_count = 2;
    } else {
        chars[0] = (UniChar)codepoint;
        char_count = 1;
    }

    CFStringRef str = CFStringCreateWithCharacters(kCFAllocatorDefault, chars, char_count);
    if (!str) return 0;

    // Create base font from primary font name (for proper cascade list)
    CTFontRef base_font = NULL;
    if (base_font_name && base_font_name[0]) {
        CFStringRef name_cf = CFStringCreateWithCString(
            kCFAllocatorDefault, base_font_name, kCFStringEncodingUTF8
        );
        if (name_cf) {
            base_font = CTFontCreateWithName(name_cf, font_size, NULL);
            CFRelease(name_cf);
        }
    }
    if (!base_font) {
        base_font = CTFontCreateWithName(CFSTR("Menlo"), font_size, NULL);
    }
    if (!base_font) {
        CFRelease(str);
        return 0;
    }

    // Ask Core Text for the best font for this codepoint
    CTFontRef fallback_font = CTFontCreateForString(base_font, str, CFRangeMake(0, char_count));
    CFRelease(str);
    CFRelease(base_font);

    if (!fallback_font) return 0;

    // Reject LastResort font (Ghostty pattern)
    CFStringRef ps_name = CTFontCopyPostScriptName(fallback_font);
    if (ps_name) {
        Boolean is_last_resort =
            CFStringFind(ps_name, CFSTR("LastResort"), 0).location != kCFNotFound;
        CFRelease(ps_name);
        if (is_last_resort) {
            CFRelease(fallback_font);
            return 0;
        }
    }

    // Get glyph index
    CGGlyph glyphs[2] = {0, 0};
    bool found = CTFontGetGlyphsForCharacters(fallback_font, chars, glyphs, (CFIndex)char_count);
    if (!found || glyphs[0] == 0) {
        CFRelease(fallback_font);
        return 0;
    }

    // Get glyph bounding rect and advance
    CGRect bbox = CTFontGetBoundingRectsForGlyphs(
        fallback_font, kCTFontOrientationDefault, glyphs, NULL, 1
    );
    CGSize advance;
    CTFontGetAdvancesForGlyphs(
        fallback_font, kCTFontOrientationDefault, glyphs, &advance, 1
    );

    // Extract font-level metrics before releasing
    result->ascent       = (float)CTFontGetAscent(fallback_font);
    result->descent      = -(float)CTFontGetDescent(fallback_font);
    result->advance_width = (float)advance.width;

    // Calculate bitmap dimensions from bounding rect
    int32_t x_origin  = (int32_t)floor(bbox.origin.x);
    int32_t y_origin  = (int32_t)floor(bbox.origin.y);
    uint32_t bmp_width  = (uint32_t)(ceil(bbox.origin.x + bbox.size.width)  - floor(bbox.origin.x));
    uint32_t bmp_height = (uint32_t)(ceil(bbox.origin.y + bbox.size.height) - floor(bbox.origin.y));

    if (bmp_width == 0 || bmp_height == 0 || bmp_width > 256 || bmp_height > 256) {
        CFRelease(fallback_font);
        return 0;
    }

    // Allocate RGBA bitmap (zeroed = transparent)
    size_t row_bytes   = (size_t)bmp_width * 4;
    size_t bitmap_size = row_bytes * bmp_height;
    uint8_t* bitmap = (uint8_t*)calloc(1, bitmap_size);
    if (!bitmap) {
        CFRelease(fallback_font);
        return 0;
    }

    // Render glyph via Core Graphics
    CGColorSpaceRef color_space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef ctx = CGBitmapContextCreate(
        bitmap, bmp_width, bmp_height, 8, row_bytes, color_space,
        kCGImageAlphaPremultipliedLast
    );
    CGColorSpaceRelease(color_space);

    if (!ctx) {
        free(bitmap);
        CFRelease(fallback_font);
        return 0;
    }

    CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
    CGContextSetAllowsAntialiasing(ctx, true);
    CGContextSetShouldAntialias(ctx, true);
    CGContextSetShouldSmoothFonts(ctx, false); // no subpixel rendering

    // Draw glyph (CG coords: y-up, origin at bottom-left of bitmap)
    CGPoint position = CGPointMake((CGFloat)(-x_origin), (CGFloat)(-y_origin));
    CTFontDrawGlyphs(fallback_font, glyphs, &position, 1, ctx);

    CGContextRelease(ctx);
    CFRelease(fallback_font);

    // Flip rows: CG y-up → screen y-down
    uint8_t* temp_row = (uint8_t*)malloc(row_bytes);
    if (temp_row) {
        for (uint32_t i = 0; i < bmp_height / 2; i++) {
            uint32_t j = bmp_height - 1 - i;
            memcpy(temp_row, bitmap + i * row_bytes, row_bytes);
            memcpy(bitmap + i * row_bytes, bitmap + j * row_bytes, row_bytes);
            memcpy(bitmap + j * row_bytes, temp_row, row_bytes);
        }
        free(temp_row);
    }

    // Convert premultiplied RGBA → coverage format (R=255,G=255,B=255,A=coverage)
    for (size_t i = 0; i < bitmap_size; i += 4) {
        uint8_t a = bitmap[i + 3];
        bitmap[i + 0] = 255;
        bitmap[i + 1] = 255;
        bitmap[i + 2] = 255;
        bitmap[i + 3] = a;
    }

    result->bitmap = bitmap;
    result->width  = bmp_width;
    result->height = bmp_height;
    result->xmin   = x_origin;
    result->ymin   = y_origin;

    return 1;
}

void aterm_coretext_free_bitmap(uint8_t* bitmap) {
    free(bitmap);
}
