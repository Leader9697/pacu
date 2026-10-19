//! GPU NVIDIA sungguhan: kurva lewat NVAPI, sensor lewat NVML.
//!
//! Perilaku yang ditiru dari Afterburner (lihat CATATAN-RE.md):
//! - ClkVfPointsGetInfo dipanggil dengan mask kosong; mask hasilnya dipakai untuk Status/Control.
//! - Bila ClkVfPointsGetStatus versi 2 diterima driver, struct Status/Control memakai versi 2 dan
//!   satuan clock 1 kHz. Selain itu versi 1, dengan satuan 0,5 kHz pada GPU lama (Pascal).
//! - Frekuensi di Status sudah termasuk offset; frekuensi bawaan = Status - offset.
//! - Titik grafis adalah titik pertama..terakhir dari ClkDomainsGetInfo untuk domain grafis.
//! - Penulisan: baca Control, ubah offset, tulis kembali dengan mask yang sama, lalu lepas kunci
//!   tegangan/frekuensi (ClockBoostLock) supaya kurva benar-benar dipakai.

use super::nvapi::{self, le_i32, le_u32, NvApi};
use super::nvml::Nvml;
use super::{Curve, Gpu, GpuInfo, GpuResult, Telemetry, VfPoint};

const ID_LOCK_SET: u32 = 0x3944_2CFB;
const LOCK_SIZE: usize = 0x030C;

pub struct NativeGpu {
    nv: NvApi,
    nvml: Option<Nvml>,
    nvml_err: Option<String>,
    mask: [u8; 32],
    /// Versi struct Status/Control (1 atau 2).
    ver: u32,
    /// Satuan mentah per kHz (1 atau 2).
    mult: i64,
    first: usize,
    last: usize,
    range: (i32, i32),
    name: String,
    driver: String,
    notes: Vec<String>,
}

fn versioned(size: usize, ver: u32) -> Vec<u8> {
    let mut b = vec![0u8; size];
    b[..4].copy_from_slice(&((size as u32) | (ver << 16)).to_le_bytes());
    b
}

impl NativeGpu {
    pub fn open() -> GpuResult<Self> {
        let nv = NvApi::load()?;
        let (nvml, nvml_err) = match Nvml::load() {
            Ok(n) => (Some(n), None),
            Err(e) => (None, Some(e)),
        };
        let name = nv.name();
        let driver = nv.driver();
        let mut g = Self {
            nv,
            nvml,
            nvml_err,
            mask: [0; 32],
            ver: 1,
            mult: 1,
            first: 0,
            last: 0,
            range: (-1_000_000, 1_000_000),
            name,
            driver,
            notes: Vec::new(),
        };
        g.probe()?;
        Ok(g)
    }

    /// Mendeteksi mask, versi struct, satuan, rentang titik, dan batas offset.
    fn probe(&mut self) -> GpuResult<()> {
        let info = self.nv.vf_info(false)?;
        self.mask.copy_from_slice(&info[4..36]);

        // Versi 2 Status ("adjusted clock" di Afterburner).
        let mut st = versioned(nvapi::STATUS_SIZE, 2);
        st[4..36].copy_from_slice(&self.mask);
        let r = self.nv.call(nvapi::ID_VF_STATUS, &mut st);
        self.ver = if r == 0 { 2 } else { 1 };
        self.notes.push(format!("ClkVfPointsGetStatus v2: {r} -> struct versi {}", self.ver));

        // Rentang titik grafis dan batas offset dari ClkDomainsGetInfo.
        let mut d = nvapi::buf(nvapi::DOMAINS_SIZE);
        let r = self.nv.call(nvapi::ID_CLK_DOMAINS, &mut d);
        let mut found = false;
        if r == 0 {
            let dmask = le_u32(&d, 4);
            for i in 0..32 {
                let e = 0x28 + i * 0x48;
                if dmask & (1 << i) == 0 || e + 0x48 > d.len() {
                    continue;
                }
                if le_u32(&d, e + 4) == 0 {
                    let max = le_i32(&d, e + 0x28);
                    let min = le_i32(&d, e + 0x2C);
                    let first = d[e + 0x30] as usize;
                    let last = d[e + 0x31] as usize;
                    self.notes.push(format!("Domain grafis: offset {min}..{max}, titik {first}..{last}"));
                    if last > first {
                        self.first = first;
                        self.last = last.min(nvapi::MAX_POINTS - 1);
                        found = true;
                    }
                    if max > 0 && min < 0 {
                        self.range = (min, max);
                    }
                    break;
                }
            }
        } else {
            self.notes.push(nvapi::err("ClkDomainsGetInfo", r));
        }
        if !found {
            // Cadangan: titik dengan tipe clock 0 yang aktif di GetInfo.
            let pts: Vec<usize> = (0..nvapi::MAX_POINTS)
                .filter(|&i| {
                    let e = nvapi::ENTRY_BASE + i * nvapi::INFO_STRIDE;
                    nvapi::mask_bit(&self.mask, i) && le_u32(&info, e) == 0 && info[e + 4] == 1
                })
                .collect();
            if pts.len() < 2 {
                return Err("Titik kurva V/F grafis tidak ditemukan".into());
            }
            self.first = pts[0];
            self.last = *pts.last().unwrap();
            self.notes.push(format!("Rentang titik dari GetInfo: {}..{}", self.first, self.last));
        }

        // Satuan: frekuensi mentah tertinggi di atas 3,5 juta berarti 0,5 kHz (GPU lama).
        let st = self.status()?;
        let max_raw = (self.first..=self.last).map(|i| le_u32(&st, nvapi::ENTRY_BASE + i * nvapi::STATUS_STRIDE + 4)).max().unwrap_or(0);
        self.mult = if self.ver == 1 && max_raw > 3_500_000 { 2 } else { 1 };
        self.range = ((self.range.0 as i64 / self.mult) as i32, (self.range.1 as i64 / self.mult) as i32);
        self.notes.push(format!("Frekuensi mentah tertinggi {max_raw}, satuan x{}", self.mult));
        Ok(())
    }

    fn status(&self) -> GpuResult<Vec<u8>> {
        let mut b = versioned(nvapi::STATUS_SIZE, self.ver);
        b[4..36].copy_from_slice(&self.mask);
        let r = self.nv.call(nvapi::ID_VF_STATUS, &mut b);
        if r != 0 {
            return Err(nvapi::err("ClkVfPointsGetStatus", r));
        }
        Ok(b)
    }

    fn control(&self) -> GpuResult<Vec<u8>> {
        let mut b = versioned(nvapi::CONTROL_SIZE, self.ver);
        b[4..36].copy_from_slice(&self.mask);
        let r = self.nv.call(nvapi::ID_VF_GET_CONTROL, &mut b);
        if r != 0 {
            return Err(nvapi::err("ClkVfPointsGetControl", r));
        }
        Ok(b)
    }

    fn read_curve(&self) -> GpuResult<Curve> {
        let st = self.status()?;
        let ct = self.control()?;
        let mut points = Vec::new();
        for i in self.first..=self.last {
            let s = nvapi::ENTRY_BASE + i * nvapi::STATUS_STRIDE;
            let c = nvapi::ENTRY_BASE + i * nvapi::CONTROL_STRIDE;
            let freq_raw = le_u32(&st, s + 4) as i64;
            let volt = le_u32(&st, s + 8);
            let delta_raw = le_i32(&ct, c + nvapi::CONTROL_DELTA) as i64;
            if freq_raw == 0 || volt == 0 {
                continue;
            }
            points.push(VfPoint {
                index: i,
                volt_uv: volt,
                stock_khz: ((freq_raw - delta_raw) / self.mult).max(0) as u32,
                delta_khz: (delta_raw / self.mult) as i32,
            });
        }
        points.sort_by_key(|p| (p.volt_uv, p.index));
        Ok(Curve { points })
    }

    fn write(&self, deltas: &[(usize, i32)]) -> GpuResult<()> {
        let mut ct = self.control()?;
        for &(i, d) in deltas {
            if i < self.first || i > self.last {
                continue;
            }
            let d = d.clamp(self.range.0, self.range.1) as i64 * self.mult;
            let off = nvapi::ENTRY_BASE + i * nvapi::CONTROL_STRIDE + nvapi::CONTROL_DELTA;
            ct[off..off + 4].copy_from_slice(&(d as i32).to_le_bytes());
        }
        ct[..4].copy_from_slice(&((nvapi::CONTROL_SIZE as u32) | (self.ver << 16)).to_le_bytes());
        ct[4..36].copy_from_slice(&self.mask);
        let r = self.nv.call(nvapi::ID_VF_SET_CONTROL, &mut ct);
        if r != 0 {
            return Err(nvapi::err("ClkVfPointsSetControl", r));
        }
        self.clear_locks();
        Ok(())
    }

    /// Melepas kunci tegangan (id 6) dan frekuensi (id 0), seperti Afterburner setiap memasang kurva.
    fn clear_locks(&self) {
        for id in [6u32, 0] {
            let mut b = versioned(LOCK_SIZE, 2);
            b[8..12].copy_from_slice(&1u32.to_le_bytes());
            b[12..16].copy_from_slice(&id.to_le_bytes());
            let _ = self.nv.call(ID_LOCK_SET, &mut b);
        }
    }

    /// Mencoba ulang sekali setelah inisialisasi ulang NVAPI (handle berubah setelah driver reset).
    fn retry<T>(&mut self, f: impl Fn(&Self) -> GpuResult<T>) -> GpuResult<T> {
        match f(self) {
            Ok(v) => Ok(v),
            Err(e) => {
                if self.nv.init().is_ok() && self.probe().is_ok() {
                    f(self)
                } else {
                    Err(e)
                }
            }
        }
    }
}

impl Gpu for NativeGpu {
    fn info(&self) -> GpuInfo {
        GpuInfo {
            name: self.name.clone(),
            driver: self.driver.clone(),
            backend: format!("NVAPI v{} x{}{}", self.ver, self.mult, if self.nvml.is_some() { " + NVML" } else { "" }),
            min_delta_khz: self.range.0,
            max_delta_khz: self.range.1,
        }
    }

    fn curve(&mut self) -> GpuResult<Curve> {
        self.retry(|g| g.read_curve())
    }

    fn set_deltas(&mut self, deltas: &[(usize, i32)]) -> GpuResult<()> {
        let d = deltas.to_vec();
        self.retry(move |g| g.write(&d))
    }

    fn telemetry(&mut self) -> Telemetry {
        let mut t = Telemetry { voltage_mv: self.nv.voltage_mv(), ..Default::default() };
        if let Some(n) = &self.nvml {
            t.clock_mhz = n.clock_mhz(0);
            t.mem_clock_mhz = n.clock_mhz(2);
            t.power_w = n.power_w();
            t.power_limit_w = n.power_limit_w();
            t.temp_c = n.temp_c();
            t.util_pct = n.util_pct();
            t.fan_pct = n.fan_pct();
            t.limits = n.limits();
            if let Some((u, total)) = n.vram_mb() {
                t.vram_used_mb = Some(u);
                t.vram_total_mb = Some(total);
            }
        }
        t
    }

    fn diagnostics(&mut self) -> String {
        let mut out = vec![
            format!("GPU: {}", self.name),
            format!("Driver: {}", self.driver),
            format!("Backend: {}", self.info().backend),
            format!("NVML: {}", self.nvml_err.clone().unwrap_or_else(|| "OK".into())),
        ];
        out.extend(self.notes.iter().cloned());
        out.push(format!("Mask: {}", self.mask.iter().map(|b| format!("{b:02x}")).collect::<String>()));
        match self.status() {
            Ok(st) => out.push(format!("Status (awal):\n{}", nvapi::hex_dump(&st, 0x44 + 28 * 4))),
            Err(e) => out.push(e),
        }
        match self.control() {
            Ok(ct) => out.push(format!("Control (awal):\n{}", nvapi::hex_dump(&ct, 0x44 + 36 * 4))),
            Err(e) => out.push(e),
        }
        match self.read_curve() {
            Ok(c) => {
                out.push(format!("{} titik grafis:", c.points.len()));
                for p in &c.points {
                    out.push(format!("  #{:3} {:7.1} mV  bawaan {:7.1} MHz  offset {:+7.1} MHz", p.index, p.volt_mv(), p.stock_mhz(), p.delta_khz as f32 / 1000.0));
                }
            }
            Err(e) => out.push(format!("Baca kurva: {e}")),
        }
        out.push(format!("Tegangan sekarang: {:?} mV", self.nv.voltage_mv()));
        out.push("Event driver terakhir:".into());
        for e in crate::volt::watchdog::recent_driver_events(10) {
            out.push(format!("  {e}"));
        }
        out.join("\n")
    }
}

/// GPU yang gagal dibuka; semua operasi mengembalikan pesan errornya.
pub struct DeadGpu(pub String);

impl Gpu for DeadGpu {
    fn info(&self) -> GpuInfo {
        GpuInfo { name: "GPU tidak terdeteksi".into(), backend: "-".into(), ..Default::default() }
    }
    fn curve(&mut self) -> GpuResult<Curve> {
        Err(self.0.clone())
    }
    fn set_deltas(&mut self, _: &[(usize, i32)]) -> GpuResult<()> {
        Err(self.0.clone())
    }
    fn telemetry(&mut self) -> Telemetry {
        Telemetry::default()
    }
    fn diagnostics(&mut self) -> String {
        let mut s = format!("Gagal membuka GPU: {}\n", self.0);
        for e in crate::volt::watchdog::recent_driver_events(10) {
            s.push_str(&format!("  {e}\n"));
        }
        s
    }
}
