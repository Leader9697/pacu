//! Ikon Pacu digambar secara prosedural, supaya tidak perlu file gambar atau crate `image`.
//! Dipakai oleh build.rs (untuk .ico) dan oleh aplikasi (ikon jendela dan tray).

/// Menghasilkan piksel RGBA berukuran `size` x `size`.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4; // supersampling per sumbu
    let n = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / n;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / n;
                    if let Some([pr, pg, pb]) = sample(u, v) {
                        r += pr;
                        g += pg;
                        b += pb;
                        a += 1.0;
                    }
                }
            }
            let i = ((y * size + x) * 4) as usize;
            if a > 0.0 {
                out[i] = (r / a * 255.0) as u8;
                out[i + 1] = (g / a * 255.0) as u8;
                out[i + 2] = (b / a * 255.0) as u8;
            }
            out[i + 3] = (a / (SS * SS) as f32 * 255.0) as u8;
        }
    }
    out
}

fn sample(u: f32, v: f32) -> Option<[f32; 3]> {
    // Kotak membulat.
    let rad = 0.22;
    let cx = u.clamp(rad, 1.0 - rad);
    let cy = v.clamp(rad, 1.0 - rad);
    if (u - cx).powi(2) + (v - cy).powi(2) > rad * rad {
        return None;
    }
    // Dua chevron putih ">>" sebagai simbol kecepatan.
    if chevron(u, v, 0.24) || chevron(u, v, 0.48) {
        return Some([1.0, 1.0, 1.0]);
    }
    // Gradien diagonal ungu ke sian.
    let t = ((u + v) / 2.0).clamp(0.0, 1.0);
    let c0 = [0.486, 0.227, 0.929];
    let c1 = [0.024, 0.714, 0.831];
    Some([
        c0[0] + (c1[0] - c0[0]) * t,
        c0[1] + (c1[1] - c0[1]) * t,
        c0[2] + (c1[2] - c0[2]) * t,
    ])
}

fn chevron(u: f32, v: f32, left: f32) -> bool {
    let half_h = 0.27;
    let w = 0.22;
    let dy = (v - 0.5).abs();
    if dy > half_h {
        return false;
    }
    let x_line = left + w * (1.0 - dy / half_h);
    (u - x_line).abs() <= 0.065
}

/// Menyusun file .ico (format BMP 32-bit) dari beberapa ukuran.
pub fn ico_bytes(sizes: &[u32]) -> Vec<u8> {
    let mut images = Vec::new();
    for &s in sizes {
        let rgba = icon_rgba(s);
        let mut bmp = Vec::new();
        // BITMAPINFOHEADER, tinggi digandakan (XOR + AND mask).
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(s as i32).to_le_bytes());
        bmp.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 24]);
        for y in (0..s).rev() {
            for x in 0..s {
                let i = ((y * s + x) * 4) as usize;
                bmp.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        let mask_row = s.div_ceil(32) * 4;
        bmp.extend(std::iter::repeat_n(0u8, (mask_row * s) as usize));
        images.push((s, bmp));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, bmp) in &images {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[d, d, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += bmp.len() as u32;
    }
    for (_, bmp) in images {
        out.extend_from_slice(&bmp);
    }
    out
}
