//! GPU texture atlas with shelf-based packing for glyph bitmaps.
//!
//! Inspired by Alacritty's atlas.rs — uses a shelf (row) allocator to pack
//! variable-sized glyph bitmaps into GPU textures. When a page fills up, a new
//! atlas page is created automatically.

/// Default atlas page dimensions (pixels).
const ATLAS_SIZE: u32 = 1024;

/// Pixel format — RGBA 4 bytes per pixel.
const BYTES_PER_PIXEL: u32 = 4;

/// UV coordinates for a glyph region within an atlas page.
#[derive(Copy, Clone, Debug)]
pub struct AtlasRegion {
    /// Atlas page index (0-based).
    pub page: usize,
    /// Top-left X in texels.
    pub x: u32,
    /// Top-left Y in texels.
    pub y: u32,
    /// Width in texels.
    pub w: u32,
    /// Height in texels.
    pub h: u32,
}

impl AtlasRegion {
    /// UV coordinates normalised to [0, 1] for the atlas page size.
    #[inline]
    pub fn uv_rect(&self, atlas_size: u32) -> [f32; 4] {
        let s = atlas_size as f32;
        [
            self.x as f32 / s,
            self.y as f32 / s,
            (self.x + self.w) as f32 / s,
            (self.y + self.h) as f32 / s,
        ]
    }
}

// ---------------------------------------------------------------------------
// Shelf allocator
// ---------------------------------------------------------------------------

/// A horizontal shelf (row) inside an atlas page.
struct Shelf {
    /// Y offset of this shelf's top edge.
    y: u32,
    /// Height of the shelf (tallest glyph placed so far).
    height: u32,
    /// Next free X position along this shelf.
    cursor_x: u32,
}

impl Shelf {
    fn new(y: u32, height: u32) -> Self {
        Self {
            y,
            height,
            cursor_x: 0,
        }
    }

    /// Try to allocate `width × height` from this shelf.
    /// Returns `Some(x)` on success with the X origin for the allocation.
    fn allocate(&mut self, width: u32, height: u32, atlas_size: u32) -> Option<u32> {
        if height > self.height {
            return None;
        }
        if self.cursor_x + width > atlas_size {
            return None;
        }
        let x = self.cursor_x;
        self.cursor_x += width;
        Some(x)
    }
}

// ---------------------------------------------------------------------------
// Atlas page
// ---------------------------------------------------------------------------

/// One GPU texture page.
struct AtlasPage {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    shelves: Vec<Shelf>,
    /// Next Y for a brand-new shelf.
    next_shelf_y: u32,
    size: u32,
}

impl AtlasPage {
    fn new(device: &wgpu::Device, size: u32, page_index: usize) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("atlas_page_{page_index}")),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            shelves: Vec::new(),
            next_shelf_y: 0,
            size,
        }
    }

    /// Try to place a glyph of `width × height` into this page.
    /// Returns `Some((x, y))` on success.
    fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        // 1. Try existing shelves (best-fit: smallest shelf that fits).
        let mut best_idx: Option<usize> = None;
        let mut best_waste = u32::MAX;
        for (i, shelf) in self.shelves.iter().enumerate() {
            if height <= shelf.height && shelf.cursor_x + width <= self.size {
                let waste = shelf.height - height;
                if waste < best_waste {
                    best_waste = waste;
                    best_idx = Some(i);
                }
            }
        }

        if let Some(idx) = best_idx {
            let x = self.shelves[idx].allocate(width, height, self.size)?;
            let y = self.shelves[idx].y;
            return Some((x, y));
        }

        // 2. Open a new shelf if space remains vertically.
        if self.next_shelf_y + height <= self.size {
            let y = self.next_shelf_y;
            let mut shelf = Shelf::new(y, height);
            let x = shelf.allocate(width, height, self.size)?;
            self.next_shelf_y += height;
            self.shelves.push(shelf);
            return Some((x, y));
        }

        None // Page full.
    }
}

// ---------------------------------------------------------------------------
// AtlasManager — public API
// ---------------------------------------------------------------------------

/// Manages one or more GPU texture atlas pages with shelf-based packing.
pub struct AtlasManager {
    pages: Vec<AtlasPage>,
    atlas_size: u32,
}

impl AtlasManager {
    /// Create a new atlas manager.  The first page is allocated lazily on the
    /// first `upload_glyph` call so that no GPU work is done if the atlas is
    /// never used.
    pub fn new() -> Self {
        Self {
            pages: Vec::new(),
            atlas_size: ATLAS_SIZE,
        }
    }

    /// Create with a custom atlas page size (must be power-of-two, ≥ 256).
    pub fn with_size(size: u32) -> Self {
        let size = size.max(256).next_power_of_two();
        Self {
            pages: Vec::new(),
            atlas_size: size,
        }
    }

    /// Number of atlas pages currently allocated.
    #[inline]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Size in texels for each square atlas page.
    #[inline]
    pub fn atlas_size(&self) -> u32 {
        self.atlas_size
    }

    /// Ensure that page 0 exists so renderer pipeline setup can bind an atlas
    /// view before any glyph uploads occur.
    pub fn ensure_page(&mut self, device: &wgpu::Device) {
        if self.pages.is_empty() {
            self.pages.push(AtlasPage::new(device, self.atlas_size, 0));
        }
    }

    /// Get the `wgpu::TextureView` for a given page index.
    pub fn texture_view(&self, page: usize) -> Option<&wgpu::TextureView> {
        self.pages.get(page).map(|p| &p.view)
    }

    /// Upload a glyph bitmap into the atlas and return its region.
    ///
    /// `bitmap` must be RGBA (`width * height * 4` bytes).
    /// If the glyph doesn't fit in any existing page, a new page is created.
    pub fn upload_glyph(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bitmap: &[u8],
        width: u32,
        height: u32,
    ) -> AtlasRegion {
        assert_eq!(
            bitmap.len() as u32,
            width * height * BYTES_PER_PIXEL,
            "bitmap length mismatch: expected {}×{}×{} = {}, got {}",
            width,
            height,
            BYTES_PER_PIXEL,
            width * height * BYTES_PER_PIXEL,
            bitmap.len(),
        );

        // Degenerate glyph (e.g. space) — return a zero-sized region on page 0.
        if width == 0 || height == 0 {
            if self.pages.is_empty() {
                self.pages.push(AtlasPage::new(device, self.atlas_size, 0));
            }
            return AtlasRegion {
                page: 0,
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            };
        }

        // Glyph larger than a single page — not expected for terminal glyphs,
        // but guard anyway.
        if width > self.atlas_size || height > self.atlas_size {
            panic!(
                "glyph {}×{} exceeds atlas page size {}",
                width, height, self.atlas_size,
            );
        }

        // Try existing pages.
        for (page_idx, page) in self.pages.iter_mut().enumerate() {
            if let Some((x, y)) = page.allocate(width, height) {
                Self::write_texture(queue, &page.texture, bitmap, x, y, width, height);
                return AtlasRegion {
                    page: page_idx,
                    x,
                    y,
                    w: width,
                    h: height,
                };
            }
        }

        // All pages full — create a new one.
        let page_idx = self.pages.len();
        let mut page = AtlasPage::new(device, self.atlas_size, page_idx);
        let (x, y) = page
            .allocate(width, height)
            .expect("fresh atlas page must fit a single glyph");
        Self::write_texture(queue, &page.texture, bitmap, x, y, width, height);
        self.pages.push(page);

        AtlasRegion {
            page: page_idx,
            x,
            y,
            w: width,
            h: height,
        }
    }

    /// Clear all pages and start fresh (e.g. on font size change).
    pub fn clear(&mut self) {
        self.pages.clear();
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn write_texture(
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        bitmap: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bitmap,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * BYTES_PER_PIXEL),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_basic_allocation() {
        let mut shelf = Shelf::new(0, 20);
        assert_eq!(shelf.allocate(10, 20, 1024), Some(0));
        assert_eq!(shelf.allocate(10, 20, 1024), Some(10));
        assert_eq!(shelf.cursor_x, 20);
    }

    #[test]
    fn shelf_rejects_too_tall() {
        let mut shelf = Shelf::new(0, 10);
        assert_eq!(shelf.allocate(5, 11, 1024), None);
    }

    #[test]
    fn shelf_rejects_overflow_x() {
        let mut shelf = Shelf::new(0, 20);
        shelf.cursor_x = 1020;
        assert_eq!(shelf.allocate(10, 20, 1024), None);
    }

    #[test]
    fn atlas_region_uv_rect() {
        let r = AtlasRegion {
            page: 0,
            x: 256,
            y: 512,
            w: 128,
            h: 64,
        };
        let uv = r.uv_rect(1024);
        assert!((uv[0] - 0.25).abs() < 1e-6);
        assert!((uv[1] - 0.50).abs() < 1e-6);
        assert!((uv[2] - 0.375).abs() < 1e-6);
        assert!((uv[3] - 0.5625).abs() < 1e-6);
    }

    #[test]
    fn with_size_clamps_and_rounds() {
        let mgr = AtlasManager::with_size(100);
        assert_eq!(mgr.atlas_size, 256);

        let mgr = AtlasManager::with_size(500);
        assert_eq!(mgr.atlas_size, 512);

        let mgr = AtlasManager::with_size(2048);
        assert_eq!(mgr.atlas_size, 2048);
    }
}
