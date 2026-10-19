//! Memilih uji berikutnya hanya dari hasil yang sudah ada. Karena tidak menyimpan keadaan lain,
//! pencarian bisa dilanjutkan dari file sesi setelah PC crash atau restart.
//!
//! Strategi:
//! 1. Turun kasar dari tegangan bawaan untuk frekuensi target, selangkah `step_mv`, sampai gagal
//!    atau mencapai batas bawah.
//! 2. Cari biner di antara titik lolos terakhir dan titik gagal pertama sampai selisih 1 titik.
//! 3. Tambahkan margin keamanan, lalu validasi lebih lama. Bila validasi gagal, tambah margin lagi.

use crate::volt::curve;
use crate::volt::store::{Planned, Session, TestKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Next {
    Baseline,
    Test(Planned),
    /// Posisi yang sudah lolos validasi.
    Done(usize),
    /// Tidak ada titik di bawah tegangan bawaan yang stabil.
    NoUndervolt,
    GiveUp(String),
}

pub const MAX_VALIDATION_FAILS: usize = 3;

pub fn next(s: &Session, min_delta_khz: i32, max_delta_khz: i32) -> Next {
    if s.baseline.is_none() || s.target_khz == 0 {
        return Next::Baseline;
    }
    let pts = &s.curve.points;
    if pts.len() < 2 {
        return Next::GiveUp("Kurva V/F tidak terbaca.".into());
    }
    let target = s.target_khz;
    let s0 = curve::stock_pos_for(&s.curve, target).unwrap_or(pts.len() - 1);
    let max_up = s.settings.max_offset_mhz as i64 * 1000;
    let valid = |p: usize| {
        pts[p].volt_mv() + 0.01 >= s.settings.min_mv as f32
            && target as i64 - pts[p].stock_khz as i64 <= max_up
            && curve::within_range(&curve::flatten(&s.curve, p, target), min_delta_khz, max_delta_khz)
    };
    let plan = |kind, pos: usize| {
        Next::Test(Planned { kind, pos, volt_mv: pts[pos].volt_mv(), target_mhz: target / 1000 })
    };

    let search = || s.results.iter().filter(|r| r.kind == TestKind::Search);
    let fail = search().filter(|r| !r.passed).map(|r| r.pos).max();
    let pass = search().filter(|r| r.passed && fail.is_none_or(|f| r.pos > f)).map(|r| r.pos).min();
    let upper = pass.unwrap_or(s0);

    if let Some(f) = fail {
        if f >= upper {
            return Next::GiveUp(format!(
                "Tidak stabil bahkan di {:.0} mV untuk {} MHz. Periksa suhu, beban, atau turunkan frekuensi target.",
                pts[f].volt_mv(),
                target / 1000
            ));
        }
    }

    let boundary = match fail {
        None => {
            // Turun kasar.
            let step = curve::mv_to_points(&s.curve, upper, s.settings.step_mv);
            let lowest = upper.saturating_sub(step);
            match (lowest..upper).find(|&p| valid(p)) {
                Some(p) => return plan(TestKind::Search, p),
                None => upper,
            }
        }
        Some(f) => {
            if upper - f > 1 {
                let mid = (f + upper) / 2;
                if valid(mid) {
                    return plan(TestKind::Search, mid);
                }
            }
            upper
        }
    };

    if boundary >= s0 && pass.is_none() {
        return Next::NoUndervolt;
    }

    let margin = curve::mv_to_points(&s.curve, boundary, s.settings.margin_mv);
    let vals = || s.results.iter().filter(|r| r.kind == TestKind::Validate);
    let vfails = vals().filter(|r| !r.passed).count();
    let vfail_max = vals().filter(|r| !r.passed).map(|r| r.pos).max();
    let mut cand = boundary + margin;
    if let Some(v) = vfail_max {
        cand = cand.max(v + margin);
    }
    let cand = cand.min(s0.max(boundary)).min(pts.len() - 1);
    if vals().any(|r| r.passed && r.pos == cand) {
        return Next::Done(cand);
    }
    if vfails >= MAX_VALIDATION_FAILS || vfail_max.is_some_and(|v| v >= cand) {
        return Next::GiveUp("Validasi panjang terus gagal. Coba frekuensi target yang lebih rendah.".into());
    }
    plan(TestKind::Validate, cand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volt::gpu::{Curve, VfPoint};
    use crate::volt::store::{Measure, Settings, TestResult};

    fn session() -> Session {
        let points = (0..60)
            .map(|i| VfPoint {
                index: i,
                volt_uv: 700_000 + i as u32 * 6_250,
                stock_khz: 1_200_000 + i as u32 * 15_000,
                delta_khz: 0,
            })
            .collect();
        Session {
            curve: Curve { points },
            baseline: Some(Measure::default()),
            target_khz: 1_800_000, // stok di posisi 40 (950 mV)
            settings: Settings::default(),
            ..Default::default()
        }
    }

    fn drive(mut s: Session, threshold: usize, flaky_val: usize) -> (Next, usize) {
        let mut tests = 0;
        let mut val_fails_left = flaky_val;
        loop {
            match next(&s, -1_000_000, 1_000_000) {
                Next::Test(p) => {
                    tests += 1;
                    let mut passed = p.pos >= threshold;
                    if p.kind == TestKind::Validate && passed && val_fails_left > 0 {
                        val_fails_left -= 1;
                        passed = false;
                    }
                    s.results.push(TestResult {
                        kind: p.kind,
                        pos: p.pos,
                        volt_mv: p.volt_mv,
                        target_mhz: p.target_mhz,
                        passed,
                        reason: String::new(),
                        secs: 0,
                        measure: None,
                    });
                    assert!(tests < 40, "tidak konvergen");
                }
                other => return (other, tests),
            }
        }
    }

    #[test]
    fn finds_boundary_plus_margin() {
        // Stabil mulai posisi 23. Margin 12 mV = 2 titik.
        let (n, tests) = drive(session(), 23, 0);
        assert_eq!(n, Next::Done(25));
        assert!(tests <= 12, "terlalu banyak uji: {tests}");
    }

    #[test]
    fn validation_failure_adds_margin() {
        let (n, _) = drive(session(), 23, 1);
        assert_eq!(n, Next::Done(27));
    }

    #[test]
    fn floor_respected() {
        let mut s = session();
        s.settings.min_mv = 800; // posisi 16
        s.settings.max_offset_mhz = 1000;
        let (n, _) = drive(s, 0, 0);
        assert_eq!(n, Next::Done(18));
    }

    #[test]
    fn offset_cap_respected() {
        let mut s = session();
        s.settings.max_offset_mhz = 150; // 10 titik x 15 MHz di bawah posisi 40
        let (n, _) = drive(s, 0, 0);
        assert_eq!(n, Next::Done(32));
    }

    #[test]
    fn unstable_everywhere_gives_no_undervolt() {
        let (n, _) = drive(session(), 41, 0);
        assert_eq!(n, Next::NoUndervolt);
    }

    #[test]
    fn needs_baseline_first() {
        let mut s = session();
        s.baseline = None;
        assert_eq!(next(&s, -1, 1), Next::Baseline);
    }
}
