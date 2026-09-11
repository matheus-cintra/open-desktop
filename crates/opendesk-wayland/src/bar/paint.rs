use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Transform};

use crate::error::WaylandError;
use crate::events::BarStyle;

pub fn paint_bar(
    width: u32,
    height: u32,
    style: BarStyle,
    alpha_multiplier: f32,
) -> Result<Vec<u8>, WaylandError> {
    let mut pixmap = Pixmap::new(width, height).ok_or(WaylandError::Paint("empty pixmap"))?;
    let path = capsule_path(width as f32, height as f32)
        .ok_or(WaylandError::Paint("degenerate capsule"))?;
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(
        style.red,
        style.green,
        style.blue,
        scaled_alpha(style.alpha, alpha_multiplier),
    );
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(pixels)
}

fn capsule_path(width: f32, height: f32) -> Option<Path> {
    let radius = width.min(height) / 2.0;
    let mut builder = PathBuilder::new();
    builder.push_circle(radius, radius, radius);
    builder.push_circle(width - radius, height - radius, radius);
    let body = if width >= height {
        Rect::from_xywh(radius, 0.0, width - 2.0 * radius, height)
    } else {
        Rect::from_xywh(0.0, radius, width, height - 2.0 * radius)
    };
    if let Some(body) = body {
        builder.push_rect(body);
    }
    builder.finish()
}

fn scaled_alpha(alpha: u8, multiplier: f32) -> u8 {
    (f32::from(alpha) * multiplier.clamp(0.0, 1.0)).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * width + x) * 4) as usize;
        [
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        ]
    }

    fn close(actual: u8, expected: u8) -> bool {
        actual.abs_diff(expected) <= 2
    }

    #[test]
    fn center_pixel_is_the_premultiplied_style_color_in_bgra_order() {
        let style = BarStyle::default();
        let pixels = paint_bar(4, 100, style, 1.0).unwrap();
        assert_eq!(pixels.len(), 4 * 100 * 4);
        let [blue, green, red, alpha] = pixel(&pixels, 4, 2, 50);
        assert!(close(alpha, 0xCC), "alpha {alpha}");
        assert!(close(red, 75), "red {red}");
        assert!(close(green, 103), "green {green}");
        assert!(close(blue, 138), "blue {blue}");
    }

    #[test]
    fn corners_are_transparent_and_the_body_is_opaque_along_the_length() {
        let pixels = paint_bar(8, 120, BarStyle::default(), 1.0).unwrap();
        assert_eq!(pixel(&pixels, 8, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&pixels, 8, 7, 119), [0, 0, 0, 0]);
        assert!(pixel(&pixels, 8, 4, 0)[3] > 150);
        assert!(pixel(&pixels, 8, 4, 119)[3] > 150);
        assert!(close(pixel(&pixels, 8, 4, 1)[3], 0xCC));
        assert!(close(pixel(&pixels, 8, 4, 118)[3], 0xCC));
        let thin = paint_bar(4, 60, BarStyle::default(), 1.0).unwrap();
        assert!(pixel(&thin, 4, 0, 0)[3] < pixel(&thin, 4, 2, 30)[3]);
        let horizontal = paint_bar(200, 4, BarStyle::default(), 1.0).unwrap();
        assert!(close(pixel(&horizontal, 200, 100, 2)[3], 0xCC));
        assert_eq!(pixel(&horizontal, 200, 0, 0)[3], pixel(&thin, 4, 0, 0)[3]);
    }

    #[test]
    fn alpha_multiplier_scales_the_whole_buffer() {
        let faded = paint_bar(4, 100, BarStyle::default(), 0.5).unwrap();
        assert!(close(pixel(&faded, 4, 2, 50)[3], 102));
        let gone = paint_bar(4, 100, BarStyle::default(), 0.0).unwrap();
        assert!(gone.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn zero_size_is_an_error() {
        assert!(paint_bar(0, 10, BarStyle::default(), 1.0).is_err());
    }
}
