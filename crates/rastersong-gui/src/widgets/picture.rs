//! A picture from packed RGB8 pixels, drawn without smoothing so a held signal stays crisp.

use eframe::egui::{self, Id, TextureOptions, Ui, Vec2, vec2};

/// Draws `rgb` (`width` by `height`, three bytes a pixel) `display_width` wide, as tall as its
/// shape asks. The texture is kept under `id` and rebuilt only when `version` changes, so
/// calling this every frame with an unchanged picture costs nothing. Returns the size it took.
pub fn picture(
    ui: &mut Ui,
    id: Id,
    version: u64,
    [width, height]: [usize; 2],
    rgb: &[u8],
    display_width: f32,
) -> Vec2 {
    if rgb.len() != width * height * 3 || width == 0 || height == 0 {
        return Vec2::ZERO;
    }
    let key = id.with("picture");
    let cached = ui.data(|d| d.get_temp::<(u64, egui::TextureHandle)>(key));
    let texture = match cached {
        Some((v, texture)) if v == version => texture,
        _ => {
            let image = egui::ColorImage::from_rgb([width.max(1), height.max(1)], rgb);
            let texture = ui
                .ctx()
                .load_texture("picture", image, TextureOptions::NEAREST);
            ui.data_mut(|d| d.insert_temp(key, (version, texture.clone())));
            texture
        }
    };
    let size = vec2(
        display_width,
        display_width * height as f32 / width.max(1) as f32,
    );
    ui.add(egui::Image::new((texture.id(), size)));
    size
}
