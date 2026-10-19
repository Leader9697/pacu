//! Perhitungan kurva undervolt.
//!
//! Undervolt gaya Afterburner: pilih satu titik tegangan V dan frekuensi target F, lalu
//! - titik di V dan di atasnya dibuat datar di F, sehingga GPU tidak pernah naik melewati V;
//! - titik di bawah V dibiarkan bawaan (hanya diturunkan bila bawaannya melebihi F),
//!   supaya bagian kurva yang tidak diuji tidak ikut di-overclock.

use crate::volt::gpu::{Curve, VfPoint};

/// Clock GPU NVIDIA modern bergerak dalam langkah 15 MHz.
pub const CLOCK_STEP_KHZ: u32 = 15_000;

pub fn snap_down_khz(khz: u32) -> u32 {
    khz / CLOCK_STEP_KHZ * CLOCK_STEP_KHZ
}

/// Offset per titik untuk kurva datar di posisi `pos` (indeks di `curve.points`) dengan
/// frekuensi `target_khz`. Mengembalikan pasangan (indeks NVAPI, offset kHz).
pub fn flatten(curve: &Curve, pos: usize, target_khz: u32) -> Vec<(usize, i32)> {
    curve
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let eff = if i >= pos { target_khz } else { p.stock_khz.min(target_khz) };
            (p.index, eff as i32 - p.stock_khz as i32)
        })
        .collect()
}

/// Posisi pertama yang frekuensi bawaannya sudah mencapai target. Inilah tegangan yang dipakai
/// GPU tanpa undervolt untuk frekuensi itu.
pub fn stock_pos_for(curve: &Curve, target_khz: u32) -> Option<usize> {
    curve.points.iter().position(|p| p.stock_khz >= target_khz)
}

/// Rata-rata jarak tegangan antartitik (mV) di sekitar `pos`.
pub fn spacing_mv(curve: &Curve, pos: usize) -> f32 {
    let n = curve.points.len();
    if n < 2 {
        return 6.25;
    }
    let lo = pos.saturating_sub(4).min(n - 2);
    let hi = (pos + 4).min(n - 1).max(lo + 1);
    let dv = curve.points[hi].volt_uv as f32 - curve.points[lo].volt_uv as f32;
    let s = dv / 1000.0 / (hi - lo) as f32;
    if s > 0.5 {
        s
    } else {
        6.25
    }
}

/// Mengubah langkah dalam mV menjadi jumlah titik (minimal 1).
pub fn mv_to_points(curve: &Curve, pos: usize, mv: u32) -> usize {
    ((mv as f32 / spacing_mv(curve, pos)).round() as usize).max(1)
}

/// Mengecek apakah semua offset masih dalam batas driver.
pub fn within_range(deltas: &[(usize, i32)], min_khz: i32, max_khz: i32) -> bool {
    deltas.iter().all(|&(_, d)| d >= min_khz && d <= max_khz)
}

/// Menggabungkan kurva bawaan dengan offset, untuk ditampilkan.
pub fn with_deltas(curve: &Curve, deltas: &[(usize, i32)]) -> Curve {
    let mut c = curve.clone();
    for p in &mut c.points {
        if let Some(&(_, d)) = deltas.iter().find(|(i, _)| *i == p.index) {
            p.delta_khz = d;
        }
    }
    c
}

/// Apakah offset yang terbaca sama dengan yang ditulis (toleransi pembulatan driver).
pub fn deltas_match(curve: &Curve, wanted: &[(usize, i32)], tol_khz: i32) -> bool {
    wanted.iter().all(|&(idx, d)| {
        curve
            .points
            .iter()
            .find(|p| p.index == idx)
            .map(|p| (p.delta_khz - d).abs() <= tol_khz)
            .unwrap_or(false)
    })
}

/// Apakah semua offset kembali ke nol (tanda driver di-reset).
pub fn all_zero(points: &[VfPoint]) -> bool {
    points.iter().all(|p| p.delta_khz == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> Curve {
        Curve {
            points: (0..10)
                .map(|i| VfPoint { index: i + 3, volt_uv: 800_000 + i as u32 * 12_500, stock_khz: 1_500_000 + i as u32 * 50_000, delta_khz: 0 })
                .collect(),
        }
    }

    #[test]
    fn flatten_caps_above_and_keeps_below() {
        let c = curve();
        let d = flatten(&c, 4, 1_800_000);
        // Posisi 4: stok 1700 -> +100 MHz.
        assert_eq!(d[4], (7, 100_000));
        // Di atasnya datar di 1800.
        assert_eq!(d[9], (12, 1_800_000 - 1_950_000));
        // Di bawahnya tetap.
        assert_eq!(d[0], (3, 0));
        let eff = with_deltas(&c, &d);
        let f: Vec<i64> = eff.points.iter().map(|p| p.effective_khz()).collect();
        assert!(f.windows(2).all(|w| w[0] <= w[1]), "kurva harus naik monoton: {f:?}");
    }

    #[test]
    fn stock_pos_and_steps() {
        let c = curve();
        assert_eq!(stock_pos_for(&c, 1_640_000), Some(3));
        assert_eq!(mv_to_points(&c, 4, 25), 2);
        assert_eq!(snap_down_khz(1_947_000), 1_935_000);
    }
}
