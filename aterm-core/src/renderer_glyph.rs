//! Fontdue-backed glyph cache for the atlas/cell rendering path.
//!
//! This stays separate from `renderer.rs` so the new pipeline can land without
//! forcing the glyphon-based renderer to be removed in the same change.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use fontdue::{Font, FontSettings, LineMetrics, Metrics, OutlineBounds};

use crate::renderer_atlas::{AtlasManager, AtlasRegion};

// Core Text system font fallback (macOS only)
#[cfg(target_os = "macos")]
#[repr(C)]
struct CoreTextGlyphResult {
    bitmap: *mut u8,
    width: u32,
    height: u32,
    xmin: i32,
    ymin: i32,
    advance_width: f32,
    ascent: f32,
    descent: f32,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn aterm_coretext_rasterize_glyph(
        codepoint: u32,
        base_font_name: *const std::ffi::c_char,
        font_size: f32,
        result: *mut CoreTextGlyphResult,
    ) -> i32;

    fn aterm_coretext_free_bitmap(bitmap: *mut u8);
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GlyphKey {
    pub font_key: u64,
    pub size_px: u16,
    pub ch: char,
}

#[derive(Clone, Debug)]
pub struct GlyphInfo {
    pub page: usize,
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub width: u32,
    pub height: u32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub advance: f32,
    pub uv_rect: [f32; 4],
    pub cell_width: u8,
}

struct RasterizedGlyph {
    metrics: Metrics,
    line_metrics: Option<LineMetrics>,
    rgba_bitmap: Vec<u8>,
    cell_width: u8,
}

struct PaddedBitmap {
    bitmap: Vec<u8>,
    width: u32,
    height: u32,
}

struct FallbackFont {
    name: String,
    font: Font,
}

pub struct GlyphCache {
    font_name: String,
    font: Font,
    font_key: u64,
    fallback_fonts: Vec<FallbackFont>,
    glyphs: HashMap<GlyphKey, GlyphInfo>,
    missing_from_all: HashSet<char>,
}

impl GlyphCache {
    pub fn from_bytes(font_key: u64, font_bytes: Vec<u8>) -> Result<Self, &'static str> {
        Self::from_named_bytes(font_key, "primary".to_string(), font_bytes)
    }

    pub fn from_named_bytes(
        font_key: u64,
        font_name: String,
        font_bytes: Vec<u8>,
    ) -> Result<Self, &'static str> {
        let font = Font::from_bytes(font_bytes, FontSettings::default())?;
        Ok(Self {
            font_name,
            font,
            font_key,
            fallback_fonts: Vec::new(),
            glyphs: HashMap::new(),
            missing_from_all: HashSet::new(),
        })
    }

    pub fn clear(&mut self) {
        self.glyphs.clear();
        self.missing_from_all.clear();
    }

    pub fn add_fallback(&mut self, font_bytes: Vec<u8>, collection_index: u32) {
        self.add_named_fallback(
            format!("fallback[{collection_index}]"),
            font_bytes,
            collection_index,
        );
    }

    pub fn add_named_fallback(
        &mut self,
        font_name: String,
        font_bytes: Vec<u8>,
        collection_index: u32,
    ) {
        let settings = FontSettings {
            collection_index,
            ..FontSettings::default()
        };
        if let Ok(font) = Font::from_bytes(font_bytes, settings) {
            self.fallback_fonts.push(FallbackFont {
                name: font_name,
                font,
            });
        }
    }

    pub fn preload_ascii(
        &mut self,
        atlas: &mut AtlasManager,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size_px: f32,
        cell_w: u32,
        cell_h: u32,
    ) {
        for codepoint in 0x20u8..=0x7eu8 {
            let _ = self.get_or_insert(
                atlas,
                device,
                queue,
                size_px,
                codepoint as char,
                cell_w,
                cell_h,
            );
        }
    }

    pub fn get(&self, size_px: f32, ch: char) -> Option<&GlyphInfo> {
        let key = self.make_key(size_px, ch);
        self.glyphs.get(&key)
    }

    pub fn get_or_insert(
        &mut self,
        atlas: &mut AtlasManager,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size_px: f32,
        ch: char,
        cell_w: u32,
        cell_h: u32,
    ) -> Option<&GlyphInfo> {
        let key = self.make_key(size_px, ch);
        if !self.glyphs.contains_key(&key) {
            let rasterized = self.rasterize(ch, size_px);
            let gw = rasterized.metrics.width as u32;
            let gh = rasterized.metrics.height as u32;

            if gw > 0 && gh > 0 {
                let padded = self.pad_to_cell_size(&rasterized, size_px, cell_w, cell_h);
                let region =
                    atlas.upload_glyph(device, queue, &padded.bitmap, padded.width, padded.height);
                let info = GlyphInfo {
                    page: region.page,
                    atlas_x: region.x,
                    atlas_y: region.y,
                    width: region.w,
                    height: region.h,
                    bearing_x: 0,
                    bearing_y: 0,
                    advance: rasterized.metrics.advance_width,
                    uv_rect: region.uv_rect(atlas.atlas_size()),
                    cell_width: rasterized.cell_width,
                };
                self.glyphs.insert(key.clone(), info);
            } else {
                let region = atlas.upload_glyph(device, queue, &rasterized.rgba_bitmap, gw, gh);
                let info = Self::build_glyph_info(
                    &rasterized.metrics,
                    region,
                    atlas.atlas_size(),
                    rasterized.cell_width,
                );
                self.glyphs.insert(key.clone(), info);
            }
        }

        self.glyphs.get(&key)
    }

    fn make_key(&self, size_px: f32, ch: char) -> GlyphKey {
        GlyphKey {
            font_key: self.font_key,
            size_px: size_px.round().clamp(1.0, u16::MAX as f32) as u16,
            ch,
        }
    }

    fn rasterize(&mut self, ch: char, size_px: f32) -> RasterizedGlyph {
        let px = size_px.max(1.0);
        let preferred_fallbacks = preferred_fallback_families(ch);

        if let Some(glyph) =
            self.rasterize_from_named_fallbacks(ch, px, preferred_fallbacks, "preferred-fallback")
        {
            return glyph;
        }

        if self.font.has_glyph(ch) {
            log_special_glyph_source(ch, &self.font_name, "primary");
            let (metrics, coverage_bitmap) = self.font.rasterize(ch, px);
            return RasterizedGlyph {
                metrics,
                line_metrics: self.font.horizontal_line_metrics(px),
                rgba_bitmap: coverage_to_rgba(&coverage_bitmap),
                cell_width: cell_width_for(ch),
            };
        }

        if !self.missing_from_all.contains(&ch) {
            if let Some(glyph) =
                self.rasterize_from_remaining_fallbacks(ch, px, preferred_fallbacks)
            {
                return glyph;
            }

            // Core Text system font fallback (macOS only)
            #[cfg(target_os = "macos")]
            if let Some(glyph) = self.rasterize_from_coretext(ch, px) {
                return glyph;
            }

            self.missing_from_all.insert(ch);
        }

        log_special_glyph_source(ch, &self.font_name, "primary-missing-fallback");
        let (metrics, coverage_bitmap) = self.font.rasterize(ch, px);
        RasterizedGlyph {
            metrics,
            line_metrics: self.font.horizontal_line_metrics(px),
            rgba_bitmap: coverage_to_rgba(&coverage_bitmap),
            cell_width: cell_width_for(ch),
        }
    }

    fn rasterize_from_named_fallbacks(
        &self,
        ch: char,
        px: f32,
        family_names: &[&str],
        source: &str,
    ) -> Option<RasterizedGlyph> {
        for family_name in family_names {
            let Some(fallback) = self
                .fallback_fonts
                .iter()
                .find(|fallback| fallback.name.eq_ignore_ascii_case(family_name))
            else {
                continue;
            };
            if fallback.font.has_glyph(ch) {
                log_special_glyph_source(ch, &fallback.name, source);
                let (metrics, coverage_bitmap) = fallback.font.rasterize(ch, px);
                return Some(RasterizedGlyph {
                    metrics,
                    line_metrics: fallback.font.horizontal_line_metrics(px),
                    rgba_bitmap: coverage_to_rgba(&coverage_bitmap),
                    cell_width: cell_width_for(ch),
                });
            }
        }
        None
    }

    fn rasterize_from_remaining_fallbacks(
        &self,
        ch: char,
        px: f32,
        skipped_family_names: &[&str],
    ) -> Option<RasterizedGlyph> {
        for fallback in &self.fallback_fonts {
            if skipped_family_names
                .iter()
                .any(|family| fallback.name.eq_ignore_ascii_case(family))
            {
                continue;
            }
            if fallback.font.has_glyph(ch) {
                log_special_glyph_source(ch, &fallback.name, "fallback");
                let (metrics, coverage_bitmap) = fallback.font.rasterize(ch, px);
                return Some(RasterizedGlyph {
                    metrics,
                    line_metrics: fallback.font.horizontal_line_metrics(px),
                    rgba_bitmap: coverage_to_rgba(&coverage_bitmap),
                    cell_width: cell_width_for(ch),
                });
            }
        }
        None
    }

    #[cfg(target_os = "macos")]
    fn rasterize_from_coretext(&self, ch: char, px: f32) -> Option<RasterizedGlyph> {
        let codepoint = ch as u32;
        let font_name_c = std::ffi::CString::new(self.font_name.as_str()).ok()?;
        let mut result = CoreTextGlyphResult {
            bitmap: std::ptr::null_mut(),
            width: 0,
            height: 0,
            xmin: 0,
            ymin: 0,
            advance_width: 0.0,
            ascent: 0.0,
            descent: 0.0,
        };

        let success = unsafe {
            aterm_coretext_rasterize_glyph(
                codepoint,
                font_name_c.as_ptr(),
                px,
                &mut result,
            )
        };

        if success == 0 || result.bitmap.is_null() || result.width == 0 || result.height == 0 {
            return None;
        }

        // Copy bitmap into Rust-owned Vec, then free C allocation
        let bitmap_size = (result.width as usize) * (result.height as usize) * 4;
        let bitmap_data = unsafe {
            std::slice::from_raw_parts(result.bitmap, bitmap_size).to_vec()
        };
        unsafe { aterm_coretext_free_bitmap(result.bitmap) };

        debug_log!(
            "[aterm] Core Text fallback: U+{:04X} '{}' → {}x{}",
            codepoint, ch, result.width, result.height
        );

        Some(RasterizedGlyph {
            metrics: Metrics {
                xmin: result.xmin,
                ymin: result.ymin,
                width: result.width as usize,
                height: result.height as usize,
                advance_width: result.advance_width,
                advance_height: 0.0,
                bounds: OutlineBounds {
                    xmin: 0.0,
                    ymin: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            },
            line_metrics: Some(LineMetrics {
                ascent: result.ascent,
                descent: result.descent,
                line_gap: 0.0,
                new_line_size: result.ascent - result.descent,
            }),
            rgba_bitmap: bitmap_data,
            cell_width: cell_width_for(ch),
        })
    }

    fn pad_to_cell_size(
        &self,
        rasterized: &RasterizedGlyph,
        size_px: f32,
        cell_w: u32,
        cell_h: u32,
    ) -> PaddedBitmap {
        let glyph_w = rasterized.metrics.width as u32;
        let glyph_h = rasterized.metrics.height as u32;
        let effective_w = cell_w * rasterized.cell_width as u32;

        let ascent = rasterized
            .line_metrics
            .map(|m| m.ascent)
            .unwrap_or(size_px * 0.8);
        let descent = rasterized
            .line_metrics
            .map(|m| m.descent)
            .unwrap_or(-(size_px * 0.2));

        let font_height = ascent - descent;
        let vertical_pad = (cell_h as f32 - font_height) / 2.0;
        let baseline_from_top = (vertical_pad + ascent).round() as i32;

        let glyph_x = rasterized.metrics.xmin;
        let glyph_y = baseline_from_top - rasterized.metrics.ymin - glyph_h as i32;

        let mut bitmap = vec![0u8; (effective_w * cell_h * 4) as usize];

        for src_row in 0..glyph_h {
            let dst_y = glyph_y + src_row as i32;
            if dst_y < 0 || dst_y >= cell_h as i32 {
                continue;
            }
            for src_col in 0..glyph_w {
                let dst_x = glyph_x + src_col as i32;
                if dst_x < 0 || dst_x >= effective_w as i32 {
                    continue;
                }
                let src_off = (src_row * glyph_w + src_col) as usize * 4;
                let dst_off = (dst_y as u32 * effective_w + dst_x as u32) as usize * 4;
                bitmap[dst_off..dst_off + 4]
                    .copy_from_slice(&rasterized.rgba_bitmap[src_off..src_off + 4]);
            }
        }

        PaddedBitmap {
            bitmap,
            width: effective_w,
            height: cell_h,
        }
    }

    fn build_glyph_info(
        metrics: &Metrics,
        region: AtlasRegion,
        atlas_size: u32,
        cell_width: u8,
    ) -> GlyphInfo {
        GlyphInfo {
            page: region.page,
            atlas_x: region.x,
            atlas_y: region.y,
            width: region.w,
            height: region.h,
            bearing_x: metrics.xmin,
            bearing_y: metrics.ymin,
            advance: metrics.advance_width,
            uv_rect: region.uv_rect(atlas_size),
            cell_width,
        }
    }
}

fn coverage_to_rgba(coverage_bitmap: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(coverage_bitmap.len() * 4);
    for &alpha in coverage_bitmap {
        rgba.extend_from_slice(&[255, 255, 255, alpha]);
    }
    rgba
}

fn log_special_glyph_source(ch: char, font_name: &str, source: &str) {
    static REPORTED_PROMPT_GLYPH: AtomicBool = AtomicBool::new(false);
    static REPORTED_BYPASS_GLYPH: AtomicBool = AtomicBool::new(false);
    static REPORTED_PLAN_GLYPH: AtomicBool = AtomicBool::new(false);

    if ch == '❯' && !REPORTED_PROMPT_GLYPH.swap(true, Ordering::Relaxed) {
        debug_log!("[aterm] glyph source for U+276F '❯': {source} font={font_name}");
    }
    if ch == '⏵' && !REPORTED_BYPASS_GLYPH.swap(true, Ordering::Relaxed) {
        debug_log!("[aterm] glyph source for U+23F5 '⏵': {source} font={font_name}");
    }
    if ch == '⏸' && !REPORTED_PLAN_GLYPH.swap(true, Ordering::Relaxed) {
        debug_log!("[aterm] glyph source for U+23F8 '⏸': {source} font={font_name}");
    }
}

fn cell_width_for(ch: char) -> u8 {
    if is_cjk_wide(ch) {
        2
    } else {
        1
    }
}

fn is_cjk_wide(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff01..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faf8
            | 0x20000..=0x3fffd
    )
}

fn preferred_fallback_families(ch: char) -> &'static [&'static str] {
    match ch {
        '\u{276E}' | '\u{276F}' => &["Apple Symbols", "Noto Sans Symbols 2", "Symbola"],
        _ if matches!(ch as u32, 0x2300..=0x23ff) => {
            &["Apple Symbols", "Noto Sans Symbols 2", "Symbola"]
        }
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_bitmap_expands_to_rgba() {
        let rgba = coverage_to_rgba(&[0, 127, 255]);
        assert_eq!(
            rgba,
            vec![255, 255, 255, 0, 255, 255, 255, 127, 255, 255, 255, 255,]
        );
    }

    #[test]
    fn wide_cell_detection_matches_cjk_examples() {
        assert_eq!(cell_width_for('A'), 1);
        assert_eq!(cell_width_for('가'), 2);
        assert_eq!(cell_width_for('界'), 2);
    }

    #[test]
    fn preferred_symbol_fallbacks_cover_prompt_and_misc_technical() {
        let preferred = &["Apple Symbols", "Noto Sans Symbols 2", "Symbola"];

        assert_eq!(preferred_fallback_families('❯'), preferred);
        assert_eq!(preferred_fallback_families('⏵'), preferred);
        assert_eq!(preferred_fallback_families('⏸'), preferred);
        assert!(preferred_fallback_families('A').is_empty());
    }
}
