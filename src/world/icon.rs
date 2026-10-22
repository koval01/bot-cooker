//! The site's own `favicon.ico`: the usual 16x16 + 32x32 pair of 32-bit
//! bitmaps, drawn in the persona's brand colours.

use crate::persona::{Brand, IconShape};

/// The icon file, byte for byte.
pub fn favicon(brand: &Brand) -> Vec<u8> {
    let images: Vec<(u8, Vec<u8>)> = [16u8, 32].iter().map(|&size| (size, bitmap(brand, size as usize))).collect();
    let mut out = Vec::new();
    // ICONDIR: reserved, type 1 (icon), image count.
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, data) in &images {
        out.extend([*size, *size, 0, 0]);
        out.extend(1u16.to_le_bytes()); // planes
        out.extend(32u16.to_le_bytes()); // bits per pixel
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in images {
        out.extend(data);
    }
    out
}

/// A BITMAPINFOHEADER, bottom-up BGRA pixels and an all-clear AND mask.
fn bitmap(brand: &Brand, size: usize) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + size * size * 4 + mask_row * size);
    out.extend(40u32.to_le_bytes());
    out.extend((size as i32).to_le_bytes());
    out.extend((2 * size as i32).to_le_bytes()); // colour + mask
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // BI_RGB
    out.extend(((size * size * 4 + mask_row * size) as u32).to_le_bytes());
    out.extend([0u8; 16]); // resolution, palette counts
    let primary = rgb(&brand.primary);
    let accent = rgb(&brand.accent);
    for y in (0..size).rev() {
        for x in 0..size {
            let (cover, second) = coverage(brand.icon, x, y, size);
            let [r, g, b] = if second > 0.5 { accent } else { primary };
            out.extend([b, g, r, (cover * 255.0).round() as u8]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, mask_row * size));
    out
}

/// How much of pixel (x, y) the figure covers (4x4 supersampled), and how
/// much of that is in the second colour.
fn coverage(shape: IconShape, x: usize, y: usize, size: usize) -> (f64, f64) {
    const N: usize = 4;
    let (mut inside, mut second) = (0usize, 0usize);
    for sy in 0..N {
        for sx in 0..N {
            // Centred coordinates in -1..1 with a one-pixel margin.
            let scale = size as f64 / 2.0 - 1.0;
            let u = ((x as f64 + (sx as f64 + 0.5) / N as f64) - size as f64 / 2.0) / scale;
            let v = ((y as f64 + (sy as f64 + 0.5) / N as f64) - size as f64 / 2.0) / scale;
            let r = (u * u + v * v).sqrt();
            let (hit, alt) = match shape {
                IconShape::Square => (u.abs() <= 1.0 && v.abs() <= 1.0, false),
                IconShape::Rounded => {
                    let (dx, dy) = ((u.abs() - 0.65).max(0.0), (v.abs() - 0.65).max(0.0));
                    (u.abs() <= 1.0 && v.abs() <= 1.0 && dx * dx + dy * dy <= 0.35 * 0.35, false)
                }
                IconShape::Circle => (r <= 1.0, r <= 0.4),
                IconShape::Diamond => (u.abs() + v.abs() <= 1.0, false),
                IconShape::Split => (u.abs() <= 1.0 && v.abs() <= 1.0, u + v > 0.0),
                IconShape::Ring => ((0.55..=1.0).contains(&r), false),
            };
            inside += hit as usize;
            second += (hit && alt) as usize;
        }
    }
    let cover = inside as f64 / (N * N) as f64;
    (cover, if inside == 0 { 0.0 } else { second as f64 / inside as f64 })
}

/// `#rrggbb` -> [r, g, b].
fn rgb(hex: &str) -> [u8; 3] {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    [(n >> 16) as u8, (n >> 8) as u8, n as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brand(icon: IconShape) -> Brand {
        Brand {
            primary: "#204080".into(),
            accent: "#e0a020".into(),
            ink: "#111111".into(),
            paper: "#ffffff".into(),
            font: "serif",
            radius: 4,
            width: 1140,
            icon,
        }
    }

    #[test]
    fn layout_is_a_valid_two_image_icon() {
        let ico = favicon(&brand(IconShape::Circle));
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        // The common 16+32 32-bit icon is exactly this long.
        assert_eq!(ico.len(), 5430);
        let entry = |i: usize| &ico[6 + 16 * i..6 + 16 * (i + 1)];
        assert_eq!(entry(0)[0], 16);
        assert_eq!(entry(1)[0], 32);
        let at = |e: &[u8], o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap()) as usize;
        assert_eq!(at(entry(0), 12), 38);
        assert_eq!(at(entry(1), 12), 38 + at(entry(0), 8));
        assert_eq!(at(entry(1), 12) + at(entry(1), 8), ico.len());
        // Each image starts with a BITMAPINFOHEADER of twice its height.
        let img = &ico[38..];
        assert_eq!(u32::from_le_bytes(img[..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(img[8..12].try_into().unwrap()), 32);
    }

    #[test]
    fn shapes_leave_corners_clear_and_differ() {
        let pixels = |s| favicon(&brand(s))[38 + 40..38 + 40 + 16 * 16 * 4].to_vec();
        for s in [IconShape::Circle, IconShape::Diamond, IconShape::Ring] {
            // Bottom-left pixel's alpha.
            assert_eq!(pixels(s)[3], 0, "{s:?}");
        }
        assert_ne!(pixels(IconShape::Square), pixels(IconShape::Split));
        assert_ne!(pixels(IconShape::Circle), pixels(IconShape::Ring));
    }
}
