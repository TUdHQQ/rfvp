use std::sync::Arc;

use ab_glyph::{Font, FontRef, GlyphId, PxScale};
use rfvp_render::{GpuCommonResources, TextureBindGroup};
use wgpu::TextureFormat;

use crate::render::{
    dynamic_atlas::{AtlasImage, DynamicAtlas, ImageProvider},
    overlay::{OverlayCollector, OverlayVisitable},
};

struct FontImageProvider<'a> {
    font: FontRef<'a>,
    color_r: u8,
    color_g: u8,
    color_b: u8,
    size_vertical: f32,
    size_horizontal: f32,
    border_size: f32,
    border_color_r: u8,
    border_color_g: u8,
    border_color_b: u8,
}

impl<'a> FontImageProvider<'a> {
    pub fn set_color(&mut self, r: u8, g: u8, b: u8) {
        self.color_r = r;
        self.color_g = g;
        self.color_b = b;
    }

    pub fn set_size(&mut self, vertical: f32, horizontal: f32) {
        self.size_vertical = vertical;
        self.size_horizontal = horizontal;
    }

    pub fn set_border_size(&mut self, size: f32) {
        self.border_size = size;
    }

    pub fn set_border_color(&mut self, r: u8, g: u8, b: u8) {
        self.border_color_r = r;
        self.border_color_g = g;
        self.border_color_b = b;
    }
}

impl<'a> ImageProvider for FontImageProvider<'a> {
    const IMAGE_FORMAT: TextureFormat = TextureFormat::R8Unorm;
    const MIPMAP_LEVELS: u32 = 4;
    type Id = GlyphId;

    fn get_image(&self, id: Self::Id) -> (Vec<Vec<u8>>, (u32, u32)) {
        let glyph = id.with_scale(PxScale { x: self.size_horizontal, y: self.size_vertical });
        let size = (glyph.scale.x as u32, glyph.scale.y as u32);

        let mut result = Vec::new();
        if let Some(q) = self.font.outline_glyph(glyph) {
            q.draw(|x, y, c| { /* draw pixel `(x, y)` with coverage: `c` */ });
        }
        result.push()
        for mip_level in GlyphMipLevel::iter() {
            let image = glyph.get_image(mip_level);
            result.push(image.to_vec());
        }

        (result, size)
    }
}

const TEXTURE_SIZE: (u32, u32) = (2048, 2048);

// TODO: later this should migrate away from the MessageLayer and ideally should be shared with all the game
pub struct FontAtlas<'a> {
    atlas: DynamicAtlas<FontImageProvider<'a>>,
    font: FontRef<'a>,
}

const COMMON_CHARACTERS: &str =
    "…\u{3000}、。「」あいうえおかがきくけこさしじすせそただちっつてでとどなにねのはひまめもゃやよらりるれろわをんー亞人代右宮戦真里\u{f8f0}！？";

impl<'a> FontAtlas<'a> {
    pub fn new(resources: &GpuCommonResources, font: FontRef) -> Self {
        let provider = FontImageProvider { font };
        let atlas = DynamicAtlas::new(resources, provider, TEXTURE_SIZE, Some("FontAtlas"));

        // Preload some common characters (not unloadable)
        for c in COMMON_CHARACTERS.chars() {
            let glyph_id = atlas.provider().font.glyph_id(c);
            let _ = atlas.get_image(resources, glyph_id);
        }

        Self { atlas }
    }

    pub fn get_font(&self) -> &FontRef {
        &self.atlas.provider().font
    }

    pub fn texture_bind_group(&self) -> &TextureBindGroup {
        self.atlas.texture_bind_group()
    }

    pub fn texture_size(&self) -> (u32, u32) {
        self.atlas.texture_size()
    }

    pub fn get_glyph(&self, resources: &GpuCommonResources, charcode: u16) -> AtlasImage {
        let glyph_id = self.get_font().get_character_mapping()[charcode as usize];
        self.atlas
            .get_image(resources, glyph_id)
            .expect("Could not fit image in atlas")
    }

    pub fn free_glyph(&self, charcode: u16) {
        let glyph_id = self.get_font().get_character_mapping()[charcode as usize];
        self.atlas.free_image(glyph_id);
    }

    pub fn free_space(&self) -> f32 {
        self.atlas.free_space()
    }
}

impl OverlayVisitable for FontAtlas<'a> {
    fn visit_overlay(&self, collector: &mut OverlayCollector) {
        self.atlas.visit_overlay(collector);
    }
}
