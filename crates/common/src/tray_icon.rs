//! Shared 32x32 system tray icon renderer with Ready (teal) and REC (red pill) status badges.

const BASE_BGRA_PREMUL: &[u8] = include_bytes!("tray_icon.bgra");
pub const ICON_DIM: usize = 32;

/// Visual status overlay drawn on top of the Encore system tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayBadge {
    Offline,
    Ready,
    Recording,
}

fn decode_base_straight_rgba() -> Vec<u8> {
    debug_assert_eq!(BASE_BGRA_PREMUL.len(), ICON_DIM * ICON_DIM * 4);
    let mut out = vec![0u8; BASE_BGRA_PREMUL.len()];
    for (src, dst) in BASE_BGRA_PREMUL.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let [b, g, r, a] = [src[0], src[1], src[2], src[3]];
        if a > 0 && a < 255 {
            let a_u16 = a as u16;
            dst[0] = ((r as u16 * 255 + a_u16 / 2) / a_u16).min(255) as u8;
            dst[1] = ((g as u16 * 255 + a_u16 / 2) / a_u16).min(255) as u8;
            dst[2] = ((b as u16 * 255 + a_u16 / 2) / a_u16).min(255) as u8;
        } else {
            dst[0] = r;
            dst[1] = g;
            dst[2] = b;
        }
        dst[3] = a;
    }
    out
}

fn blend_pixel(rgba: &mut [u8], x: usize, y: usize, r: u8, g: u8, b: u8, a: u8) {
    if x >= ICON_DIM || y >= ICON_DIM || a == 0 {
        return;
    }
    let idx = (y * ICON_DIM + x) * 4;
    let af = a as f32 / 255.0;
    let saf = rgba[idx + 3] as f32 / 255.0;
    let oa = af + saf * (1.0 - af);
    if oa <= 0.0 {
        return;
    }
    let blend = |fg: u8, bg: u8| -> u8 {
        ((fg as f32 * af + bg as f32 * saf * (1.0 - af)) / oa).round().clamp(0.0, 255.0) as u8
    };
    rgba[idx] = blend(r, rgba[idx]);
    rgba[idx + 1] = blend(g, rgba[idx + 1]);
    rgba[idx + 2] = blend(b, rgba[idx + 2]);
    rgba[idx + 3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn draw_ready_dot(rgba: &mut [u8]) {
    let (cx, cy, rad) = (25.0_f32, 25.0_f32, 4.6_f32);
    for y in 18..ICON_DIM {
        for x in 18..ICON_DIM {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d <= rad - 0.4 {
                let hl = (1.0
                    - (((x as f32 + 0.5 - (cx - 1.4)).powi(2)
                        + (y as f32 + 0.5 - (cy - 1.4)).powi(2))
                    .sqrt()
                        / (rad * 1.4)))
                    .max(0.0);
                let r = (22.0 + (255.0 - 22.0) * hl * 0.45) as u8;
                let g = (201.0 + (255.0 - 201.0) * hl * 0.45) as u8;
                let b = (179.0 + (255.0 - 179.0) * hl * 0.45) as u8;
                blend_pixel(rgba, x, y, r, g, b, 255);
            } else if d <= rad + 0.4 {
                let a = (((rad + 0.4 - d) / 0.8).clamp(0.0, 1.0) * 255.0) as u8;
                blend_pixel(rgba, x, y, 22, 201, 179, a);
            } else if d <= rad + 1.7 {
                let a = (((rad + 1.7 - d) / 1.3).clamp(0.0, 1.0) * 235.0) as u8;
                blend_pixel(rgba, x, y, 10, 12, 16, a);
            }
        }
    }
}

fn draw_rec_pill(rgba: &mut [u8]) {
    // Crisp dark-framed red pill in the bottom-right holding 3x5 pixel letters "REC".
    for y in 20..ICON_DIM {
        for x in 11..ICON_DIM {
            let px = x.clamp(13, 29) as f32;
            let py = y.clamp(22, 29) as f32;
            let d = ((x as f32 - px).powi(2) + (y as f32 - py).powi(2)).sqrt();
            if d <= 1.2 {
                let (r, g, b) = if y <= 21 { (255, 112, 112) } else { (239, 52, 52) };
                blend_pixel(rgba, x, y, r, g, b, 255);
            } else if d <= 2.3 {
                blend_pixel(rgba, x, y, 11, 13, 18, 240);
            }
        }
    }
    const GLYPHS: [[u8; 5]; 3] = [
        [0b110, 0b101, 0b110, 0b101, 0b101], // R
        [0b111, 0b100, 0b110, 0b100, 0b111], // E
        [0b011, 0b100, 0b100, 0b100, 0b011], // C
    ];
    for (ci, glyph) in GLYPHS.iter().enumerate() {
        let ox = 16 + ci * 4;
        let oy = 23;
        for (gy, row) in glyph.iter().enumerate() {
            for gx in 0..3 {
                if (row >> (2 - gx)) & 1 == 1 {
                    blend_pixel(rgba, ox + gx, oy + gy, 255, 255, 255, 255);
                }
            }
        }
    }
}

/// Render a 32x32 straight-RGBA icon buffer (suitable for `tauri::image::Image`).
pub fn render_tray_rgba(badge: TrayBadge) -> Vec<u8> {
    let mut rgba = decode_base_straight_rgba();
    match badge {
        TrayBadge::Offline => {}
        TrayBadge::Ready => draw_ready_dot(&mut rgba),
        TrayBadge::Recording => draw_rec_pill(&mut rgba),
    }
    rgba
}

/// Render a 32x32 premultiplied-BGRA icon buffer (suitable for Win32 `CreateBitmap`).
pub fn render_tray_bgra_premul(badge: TrayBadge) -> Vec<u8> {
    if badge == TrayBadge::Offline {
        return BASE_BGRA_PREMUL.to_vec();
    }
    let rgba = render_tray_rgba(badge);
    let mut out = vec![0u8; rgba.len()];
    for (src, dst) in rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let [r, g, b, a] = [src[0] as u16, src[1] as u16, src[2] as u16, src[3] as u16];
        dst[0] = ((b * a + 127) / 255) as u8;
        dst[1] = ((g * a + 127) / 255) as u8;
        dst[2] = ((r * a + 127) / 255) as u8;
        dst[3] = a as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_valid_32x32_icons_for_all_badges() {
        let offline = render_tray_rgba(TrayBadge::Offline);
        let ready = render_tray_rgba(TrayBadge::Ready);
        let rec = render_tray_rgba(TrayBadge::Recording);
        assert_eq!(offline.len(), ICON_DIM * ICON_DIM * 4);
        assert_eq!(ready.len(), ICON_DIM * ICON_DIM * 4);
        assert_eq!(rec.len(), ICON_DIM * ICON_DIM * 4);
        assert_ne!(offline, ready);
        assert_ne!(offline, rec);
        assert_ne!(ready, rec);

        let bgra_rec = render_tray_bgra_premul(TrayBadge::Recording);
        assert_eq!(bgra_rec.len(), ICON_DIM * ICON_DIM * 4);
        assert!(bgra_rec.chunks_exact(4).all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]));
    }
}
