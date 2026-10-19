//! Model kurva untuk GPU AMD.
//!
//! AMD (lewat ADLX, sama seperti slider di Adrenalin) tidak membuka kurva V/F per titik. Yang bisa
//! diatur hanya "Tegangan maksimum" dan "Frekuensi maksimum". Supaya algoritma pencarian yang sama
//! bisa dipakai, pengaturan itu digambarkan sebagai kurva tiruan: satu titik per langkah tegangan,
//! dengan frekuensi bawaan turun 2 MHz per mV di bawah tegangan bawaan. "Meratakan kurva di titik
//! V ke frekuensi F" (lihat `curve::flatten`) diterjemahkan menjadi tegangan maksimum V dan
//! frekuensi maksimum F.

use super::{Curve, VfPoint};
use crate::volt::curve;

/// Kemiringan model: frekuensi bawaan turun sekian MHz per mV. Dengan batas offset 350 MHz,
/// pencarian bisa turun sampai 175 mV di bawah tegangan bawaan.
pub const MHZ_PER_MV: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmdModel {
    /// Tegangan maksimum bawaan (mV).
    pub v_def: i32,
    /// Tegangan terendah yang diizinkan driver (mV).
    pub v_min: i32,
    /// Jarak antar titik (mV).
    pub step: i32,
    /// Frekuensi maksimum bawaan (MHz).
    pub f_def: i32,
}

impl AmdModel {
    pub fn points(&self) -> usize {
        ((self.v_def - self.v_min) / self.step.max(1)).max(1) as usize + 1
    }

    pub fn volt_at(&self, i: usize) -> i32 {
        self.v_def - (self.points() - 1 - i) as i32 * self.step
    }

    pub fn stock_khz_at(&self, i: usize) -> u32 {
        ((self.f_def - (self.v_def - self.volt_at(i)) * MHZ_PER_MV).max(100) * 1000) as u32
    }

    fn stock(&self) -> Curve {
        Curve {
            points: (0..self.points())
                .map(|i| VfPoint { index: i, volt_uv: self.volt_at(i) as u32 * 1000, stock_khz: self.stock_khz_at(i), delta_khz: 0 })
                .collect(),
        }
    }

    /// Kurva tiruan untuk pengaturan driver sekarang.
    pub fn curve(&self, volt_mv: i32, max_mhz: i32) -> Curve {
        let mut c = self.stock();
        if volt_mv >= self.v_def && max_mhz == self.f_def {
            return c;
        }
        let pos = (0..self.points()).min_by_key(|&i| (self.volt_at(i) - volt_mv).abs()).unwrap_or(0);
        let d = curve::flatten(&c, pos, max_mhz as u32 * 1000);
        for (p, (_, delta)) in c.points.iter_mut().zip(d) {
            p.delta_khz = delta;
        }
        c
    }

    /// Pengaturan driver (tegangan mV, frekuensi MHz) untuk offset titik. None = bawaan.
    pub fn settings_for(&self, deltas: &[(usize, i32)]) -> Option<(i32, i32)> {
        if deltas.iter().all(|&(_, d)| d == 0) {
            return None;
        }
        let eff = |i: usize| self.stock_khz_at(i) as i64 + deltas.iter().find(|(x, _)| *x == i).map(|&(_, d)| d as i64).unwrap_or(0);
        let top = self.points() - 1;
        let target = eff(top);
        let mut pos = top;
        while pos > 0 && eff(pos - 1) == target {
            pos -= 1;
        }
        Some((self.volt_at(pos), (target / 1000) as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> AmdModel {
        AmdModel { v_def: 1200, v_min: 700, step: 5, f_def: 2725 }
    }

    #[test]
    fn stock_curve_is_flat_zero() {
        let m = model();
        let c = m.curve(1200, 2725);
        assert_eq!(c.points.len(), 101);
        assert!(curve::all_zero(&c.points));
        assert_eq!(c.points.last().unwrap().stock_khz, 2_725_000);
        assert_eq!(c.points[0].volt_uv, 700_000);
        assert_eq!(m.settings_for(&curve::flatten(&c, 100, 2_725_000)), None);
    }

    #[test]
    fn flatten_round_trips_to_slider_values() {
        let m = model();
        let stock = m.curve(1200, 2725);
        // 1100 mV = titik 80.
        let d = curve::flatten(&stock, 80, 2_725_000);
        assert_eq!(m.settings_for(&d), Some((1100, 2725)));
        // Membaca balik pengaturan itu memberi offset yang sama.
        let back = m.curve(1100, 2725);
        assert!(curve::deltas_match(&back, &d, 0));
        assert!(!curve::all_zero(&back.points));
    }

    #[test]
    fn custom_target_frequency() {
        let m = model();
        let stock = m.curve(1200, 2725);
        let d = curve::flatten(&stock, 60, 2_500_000);
        assert_eq!(m.settings_for(&d), Some((1000, 2500)));
        assert!(curve::deltas_match(&m.curve(1000, 2500), &d, 0));
    }

    #[test]
    fn plan_starts_at_default_voltage() {
        let m = model();
        let c = m.curve(1200, 2725);
        assert_eq!(curve::stock_pos_for(&c, 2_725_000), Some(100));
        // 25 mV = 5 titik.
        assert_eq!(curve::mv_to_points(&c, 100, 25), 5);
    }
}
