//! CordFast's icon, drawn rather than shipped: a blurple rounded square
//! with a white lightning bolt. The window, the tray and notifications use it.

/// The bolt, in a unit square.
const BOLT: [(f32, f32); 7] = [
    (0.58, 0.14),
    (0.30, 0.55),
    (0.48, 0.55),
    (0.40, 0.86),
    (0.71, 0.42),
    (0.53, 0.42),
    (0.62, 0.14),
];

fn inside(points: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// RGBA pixels, `size` × `size`, with 4× supersampled edges.
pub fn rgba(size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 4];
    let radius = 0.22;
    for py in 0..size {
        for px in 0..size {
            let (mut fill, mut bolt) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / size as f32;
                    let dx = (radius - x).max(x - (1.0 - radius)).max(0.0);
                    let dy = (radius - y).max(y - (1.0 - radius)).max(0.0);
                    if dx * dx + dy * dy <= radius * radius {
                        fill += 1;
                        if inside(&BOLT, x, y) {
                            bolt += 1;
                        }
                    }
                }
            }
            let i = (py * size + px) * 4;
            let t = bolt as f32 / fill.max(1) as f32;
            let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
            out[i] = mix(0x58, 0xff);
            out[i + 1] = mix(0x65, 0xff);
            out[i + 2] = mix(0xf2, 0xff);
            out[i + 3] = (fill * 255 / 16) as u8;
        }
    }
    out
}

pub fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: rgba(64),
        width: 64,
        height: 64,
    }
}

/// The icon as a PNG, for notifications.
pub fn png(size: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbaImage::from_raw(size as u32, size as u32, rgba(size))
        .expect("icon buffer matches its size");
    let _ = image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png);
    bytes
}

/// The icon as a Windows .ico: a PNG per size, which Windows reads since
/// Vista. The executable embeds it (build.rs, assets/cordfast.rc) for the
/// taskbar, Explorer and shortcuts. Written by the `icon_file` test.
#[cfg(test)]
pub fn ico(sizes: &[usize]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|size| png(*size)).collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (size, image) in sizes.iter().zip(&images) {
        // 0 stands for 256.
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend([side, side, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((image.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in images {
        out.extend(image);
    }
    out
}

#[cfg(test)]
mod tests {
    /// The embedded icon is the drawn one: `cargo test icon_file -- --ignored`
    /// writes it again after the drawing changes.
    #[test]
    #[ignore]
    fn icon_file() {
        std::fs::write("assets/cordfast.ico", super::ico(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();
    }

    #[test]
    fn ico_lists_every_size() {
        let ico = super::ico(&[16, 256]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(ico[6], 16);
        assert_eq!(ico[22], 0, "256 is written as 0");
        let offset = u32::from_le_bytes(ico[18..22].try_into().unwrap()) as usize;
        assert_eq!(&ico[offset + 1..offset + 4], b"PNG");
    }
}
