use crate::math_assets::MathSvgAsset;
use egui::{Color32, ColorImage, Context, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;

/// Rasterized GPU texture cache for pre-baked MathJax `.svgz` equations.
#[derive(Default)]
pub struct MathSvgCache {
    textures: HashMap<(&'static str, u32), TextureHandle>,
}

impl MathSvgCache {
    /// Returns a GPU texture for `asset` rasterized at `pixels_per_point`, rendering lazily once.
    pub fn texture_for(
        &mut self,
        ctx: &Context,
        asset: &MathSvgAsset,
        pixels_per_point: f32,
    ) -> Option<&TextureHandle> {
        // Bucket DPI scale cleanly (e.g., 1.0, 1.25, 1.5, 2.0) to avoid thrashing cache keys.
        let scale_key = (pixels_per_point.clamp(1.0, 4.0) * 100.0).round() as u32;
        let key = (asset.id, scale_key);

        if let std::collections::hash_map::Entry::Vacant(e) = self.textures.entry(key) {
            let effective_ppp = (scale_key as f32) / 100.0;
            let (px_w, px_h, rgba) = rasterize_svg_rgba(asset, effective_ppp).ok()?;
            let image = ColorImage::from_rgba_premultiplied([px_w as usize, px_h as usize], &rgba);
            let tex_name = format!("math_svg_{}_{}", asset.id, scale_key);
            // tiny-skia already computes subpixel AA into exact physical pixels; NEAREST prevents GPU bilinear blur.
            let handle = ctx.load_texture(tex_name, image, TextureOptions::NEAREST);
            e.insert(handle);
        }

        self.textures.get(&key)
    }

    /// Returns a GPU texture for an inline UI SVG icon of logical size `size_pt` at `pixels_per_point`.
    pub fn ui_icon_texture(
        &mut self,
        ctx: &Context,
        id: &'static str,
        svg_bytes: &[u8],
        size_pt: f32,
        pixels_per_point: f32,
    ) -> Option<&TextureHandle> {
        let px = (size_pt * pixels_per_point.clamp(1.0, 4.0)).round().clamp(8.0, 256.0) as u32;
        let key = (id, px);

        if let std::collections::hash_map::Entry::Vacant(e) = self.textures.entry(key) {
            let opts = usvg::Options::default();
            let tree = usvg::Tree::from_data(svg_bytes, &opts).ok()?;
            let svg_size = tree.size();
            let scale = ((px as f32) / svg_size.width().max(1.0))
                .min((px as f32) / svg_size.height().max(1.0));
            let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            let image =
                ColorImage::from_rgba_premultiplied([px as usize, px as usize], &pixmap.take());
            let tex_name = format!("ui_svg_{}_{}", id, px);
            let handle = ctx.load_texture(tex_name, image, TextureOptions::NEAREST);
            e.insert(handle);
        }

        self.textures.get(&key)
    }
}

/// Renders a pre-baked `MathSvgAsset` in `ui` snapped 1:1 to physical pixels and tinted to `text_color`.
pub fn paint_math_svg(
    ui: &mut Ui,
    ctx: &Context,
    cache: &mut MathSvgCache,
    asset: &MathSvgAsset,
    text_color: Color32,
) {
    let ppp = ctx.pixels_per_point().max(1.0);
    if let Some(tex) = cache.texture_for(ctx, asset, ppp) {
        let logical_size = tex.size_vec2() / ppp;
        let (rect, _) = ui.allocate_exact_size(logical_size, Sense::hover());
        if ui.is_rect_visible(rect) {
            let snapped_min = Pos2::new(
                (rect.min.x * ppp).round() / ppp,
                (rect.min.y * ppp).round() / ppp,
            );
            let snapped_rect = Rect::from_min_size(snapped_min, logical_size);
            ui.painter().image(
                tex.id(),
                snapped_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                text_color,
            );
        }
    }
}

/// Decompresses a baked `.svgz` asset and rasterizes it into premultiplied RGBA pixels.
pub fn rasterize_svg_rgba(
    asset: &MathSvgAsset,
    pixels_per_point: f32,
) -> Result<(u32, u32, Vec<u8>), String> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_data(asset.svgz, &opts)
        .map_err(|e| format!("failed to parse {}: {}", asset.id, e))?;

    let svg_size = tree.size();
    let scale_x = (asset.width_pt * pixels_per_point) / svg_size.width().max(1.0);
    let scale_y = (asset.height_pt * pixels_per_point) / svg_size.height().max(1.0);
    let scale = scale_x.min(scale_y).max(0.1);

    let px_w = (asset.width_pt * pixels_per_point).ceil().clamp(1.0, 8192.0) as u32;
    let px_h = (asset.height_pt * pixels_per_point).ceil().clamp(1.0, 8192.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(px_w, px_h)
        .ok_or_else(|| format!("invalid pixmap dimensions {}x{} for {}", px_w, px_h, asset.id))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    Ok((px_w, px_h, pixmap.take()))
}

/// Rasterizes a standalone SVG icon into unpremultiplied RGBA [`egui::IconData`].
pub fn rasterize_icon_svg(svg_bytes: &[u8], target_px: u32) -> Result<egui::IconData, String> {
    let opts = usvg::Options::default();
    let tree =
        usvg::Tree::from_data(svg_bytes, &opts).map_err(|e| format!("failed to parse icon SVG: {e}"))?;

    let svg_size = tree.size();
    let px = target_px.clamp(16, 1024);
    let scale_x = (px as f32) / svg_size.width().max(1.0);
    let scale_y = (px as f32) / svg_size.height().max(1.0);
    let scale = scale_x.min(scale_y);

    let mut pixmap =
        tiny_skia::Pixmap::new(px, px).ok_or_else(|| format!("invalid icon pixmap {px}x{px}"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();

    Ok(egui::IconData {
        rgba,
        width: px,
        height: px,
    })
}
