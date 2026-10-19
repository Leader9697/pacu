//! Dunia simulasi: GPU mirip RTX 3070 Ti, beban 3D, pemeriksa error, dan log driver, berbagi satu
//! keadaan. Dipakai untuk mengembangkan UI di Linux dan menguji algoritma pencarian.
//!
//! Model kestabilan: tiap chip punya "bonus silikon" (MHz) di atas kurva bawaan. Titik kerja stabil
//! bila frekuensinya tidak melebihi kurva bawaan + bonus pada tegangan itu. Di dekat batas, crash
//! terjadi secara acak seperti pada GPU sungguhan.

use super::{Curve, Gpu, GpuInfo, GpuResult, Telemetry, VfPoint};
use crate::volt::rig::{CheckStatus, Checker, Clock, Load, LoadStatus, Rig, Watchdog};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

/// Titik jangkar kurva bawaan (mV, MHz), kira-kira seperti RTX 3070 Ti.
const ANCHORS: [(f32, f32); 9] = [
    (700.0, 1155.0),
    (750.0, 1365.0),
    (800.0, 1560.0),
    (850.0, 1710.0),
    (900.0, 1815.0),
    (950.0, 1890.0),
    (1000.0, 1950.0),
    (1050.0, 1995.0),
    (1081.0, 2010.0),
];

fn stock_mhz(mv: f32) -> f32 {
    if mv <= ANCHORS[0].0 {
        return ANCHORS[0].1;
    }
    for w in ANCHORS.windows(2) {
        let ((v0, f0), (v1, f1)) = (w[0], w[1]);
        if mv <= v1 {
            return f0 + (f1 - f0) * (mv - v0) / (v1 - v0);
        }
    }
    ANCHORS[ANCHORS.len() - 1].1
}

pub struct World {
    pub t: f64,
    rng: u64,
    pub points: Vec<VfPoint>,
    pub bonus_mhz: f32,
    pub power_limit_w: f32,
    pub load_running: bool,
    pub load_crashed: Option<String>,
    pub checker_on: bool,
    pub check: CheckStatus,
    pub events: Vec<String>,
    pub event_mark: usize,
    /// Kecepatan waktu: 1 detik simulasi = 1/speed detik nyata. 0 = tanpa jeda.
    pub speed: f64,
    pub op: (f32, f32, f32),
}

impl World {
    pub fn new(seed: u64, bonus_mhz: f32, speed: f64) -> Self {
        // Titik 0..63: 700..1093,75 mV per 6,25 mV, lalu datar di 1100 mV.
        let mut points: Vec<VfPoint> = (0..64)
            .map(|i| {
                let mv = 700.0 + i as f32 * 6.25;
                VfPoint { index: i, volt_uv: (mv * 1000.0) as u32, stock_khz: (stock_mhz(mv) / 15.0).floor() as u32 * 15_000, delta_khz: 0 }
            })
            .collect();
        for i in 64..70 {
            points.push(VfPoint { index: i, volt_uv: 1_100_000, stock_khz: 2_010_000, delta_khz: 0 });
        }
        Self {
            t: 0.0,
            rng: seed | 1,
            points,
            bonus_mhz,
            power_limit_w: 290.0,
            load_running: false,
            load_crashed: None,
            checker_on: false,
            check: CheckStatus::default(),
            events: Vec::new(),
            event_mark: 0,
            speed,
            op: (210.0, 750.0, 25.0),
        }
    }

    fn rand(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn power(mhz: f32, mv: f32) -> f32 {
        40.0 + 0.128 * mhz * (mv / 1000.0).powi(2)
    }

    /// Titik kerja di bawah beban: frekuensi tertinggi yang muat dalam batas daya, dengan tegangan
    /// terendah untuk frekuensi itu.
    fn operating_point(&self) -> (f32, f32, f32) {
        if !self.load_running {
            return (210.0, 750.0, 25.0);
        }
        let mut best: Option<(f32, f32)> = None;
        for p in &self.points {
            let f = p.effective_mhz();
            let v = p.volt_mv();
            if Self::power(f, v) > self.power_limit_w {
                continue;
            }
            match best {
                Some((bf, _)) if f <= bf => {}
                _ => best = Some((f, v)),
            }
        }
        let (f, v) = best.unwrap_or((self.points[0].effective_mhz(), self.points[0].volt_mv()));
        (f, v, Self::power(f, v))
    }

    pub fn advance(&mut self, dt: f64) {
        let steps = dt.ceil().max(1.0) as usize;
        let h = dt / steps as f64;
        for _ in 0..steps {
            self.t += h;
            let (f, v, p) = self.operating_point();
            self.op = (f, v, p);
            if self.checker_on {
                self.check.runs += (h * 4.0) as u64;
            }
            if !self.load_running {
                continue;
            }
            let margin = stock_mhz(v) + self.bonus_mhz - f;
            let hazard = if margin < 0.0 {
                0.4
            } else if margin < 20.0 {
                0.003
            } else {
                0.0
            };
            if self.rand() < hazard * h {
                let r = self.rand();
                if r < 0.45 {
                    self.events.push("Display 4101: driver tampilan nvlddmkm berhenti merespons dan berhasil pulih".into());
                    for p in &mut self.points {
                        p.delta_khz = 0;
                    }
                    self.check.device_lost = self.checker_on;
                    self.load_running = false;
                    self.load_crashed = Some("kode keluar 0xc0000005".into());
                } else if r < 0.7 {
                    self.load_running = false;
                    self.load_crashed = Some("kode keluar 0xc0000005".into());
                } else if self.checker_on {
                    self.check.errors += 1;
                } else {
                    self.events.push("nvlddmkm 13: Graphics SM Warp Exception".into());
                }
            }
        }
    }
}

pub type Shared = Arc<Mutex<World>>;

pub struct SimGpu(pub Shared);

impl SimGpu {
    pub fn standalone() -> Self {
        SimGpu(Arc::new(Mutex::new(World::new(7, 150.0, 1.0))))
    }
}

impl Gpu for SimGpu {
    fn info(&self) -> GpuInfo {
        GpuInfo {
            name: "NVIDIA GeForce RTX 3070 Ti (simulasi)".into(),
            driver: "simulasi".into(),
            backend: "Simulasi".into(),
            min_delta_khz: -1_000_000,
            max_delta_khz: 1_000_000,
        }
    }
    fn curve(&mut self) -> GpuResult<Curve> {
        Ok(Curve { points: self.0.lock().points.clone() })
    }
    fn set_deltas(&mut self, deltas: &[(usize, i32)]) -> GpuResult<()> {
        let mut w = self.0.lock();
        for &(i, d) in deltas {
            if let Some(p) = w.points.iter_mut().find(|p| p.index == i) {
                p.delta_khz = d;
            }
        }
        let op = w.operating_point();
        w.op = op;
        Ok(())
    }
    fn telemetry(&mut self) -> Telemetry {
        let mut w = self.0.lock();
        let op = w.operating_point();
        w.op = op;
        let (f, v, p) = op;
        let j = (w.rand() - 0.5) as f32;
        let loaded = w.load_running;
        let limited = loaded && (p - w.power_limit_w).abs() < 12.0;
        Telemetry {
            clock_mhz: Some(f as u32),
            mem_clock_mhz: Some(if loaded { 9501 } else { 405 }),
            voltage_mv: Some(v),
            power_w: Some(p + j * 4.0),
            power_limit_w: Some(w.power_limit_w),
            temp_c: Some((30.0 + p * 0.16) as u32),
            util_pct: Some(if loaded { 98 } else { 2 }),
            fan_pct: Some(if loaded { 62 } else { 0 }),
            limits: if limited { vec!["daya"] } else { vec![] },
            vram_used_mb: Some(if loaded { 5200 } else { 900 }),
            vram_total_mb: Some(8192),
        }
    }
    fn diagnostics(&mut self) -> String {
        "GPU simulasi: tidak ada NVAPI.".into()
    }
}

pub struct SimLoad(pub Shared);

impl Load for SimLoad {
    fn start(&mut self) -> Result<(), String> {
        let mut w = self.0.lock();
        w.load_running = true;
        w.load_crashed = None;
        Ok(())
    }
    fn status(&mut self) -> LoadStatus {
        let w = self.0.lock();
        match (&w.load_crashed, w.load_running) {
            (_, true) => LoadStatus::Running,
            (Some(c), false) => LoadStatus::Exited(c.clone()),
            (None, false) => LoadStatus::Exited("belum dijalankan".into()),
        }
    }
    fn stop(&mut self) {
        self.0.lock().load_running = false;
    }
}

pub struct SimChecker(pub Shared);

impl Checker for SimChecker {
    fn available(&self) -> bool {
        true
    }
    fn prepare(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn start(&mut self) {
        self.0.lock().checker_on = true;
    }
    fn take(&mut self) -> CheckStatus {
        std::mem::take(&mut self.0.lock().check)
    }
    fn stop(&mut self) {
        self.0.lock().checker_on = false;
    }
}

pub struct SimWatchdog(pub Shared);

impl Watchdog for SimWatchdog {
    fn mark(&mut self) {
        let mut w = self.0.lock();
        w.event_mark = w.events.len();
    }
    fn events_since_mark(&mut self) -> Vec<String> {
        let w = self.0.lock();
        w.events[w.event_mark..].to_vec()
    }
}

pub struct SimClock(pub Shared);

impl Clock for SimClock {
    fn sleep(&self, d: Duration) {
        let speed = {
            let mut w = self.0.lock();
            w.advance(d.as_secs_f64());
            w.speed
        };
        if speed > 0.0 {
            std::thread::sleep(d.div_f64(speed));
        }
    }
}

pub fn rig(seed: u64, bonus_mhz: f32, speed: f64) -> (Rig, Shared) {
    let w: Shared = Arc::new(Mutex::new(World::new(seed, bonus_mhz, speed)));
    let rig = Rig {
        gpu: Box::new(SimGpu(w.clone())),
        load: Box::new(SimLoad(w.clone())),
        checker: Box::new(SimChecker(w.clone())),
        watchdog: Box::new(SimWatchdog(w.clone())),
        clock: Box::new(SimClock(w.clone())),
    };
    (rig, w)
}
