//! Akses GPU: membaca/menulis kurva V/F dan membaca sensor.
//!
//! Di Windows kurva V/F diakses lewat fungsi NVAPI yang sama dengan yang dipakai MSI Afterburner
//! (ClkVfPoints*), sedangkan sensor dibaca lewat NVML. Di Linux dipakai GPU simulasi untuk
//! pengembangan dan pengujian algoritma.

pub mod amdmodel;
pub mod sim;

#[cfg(windows)]
pub mod adlx;
#[cfg(windows)]
pub mod nvapi;
#[cfg(windows)]
pub mod nvml;
#[cfg(windows)]
pub mod native;

use serde::{Deserialize, Serialize};

/// Satu titik kurva V/F grafis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VfPoint {
    /// Indeks titik di tabel NVAPI (bukan urutan di vektor).
    pub index: usize,
    pub volt_uv: u32,
    /// Frekuensi bawaan pabrik pada tegangan ini (tanpa offset).
    pub stock_khz: u32,
    /// Offset yang sedang terpasang.
    pub delta_khz: i32,
}

impl VfPoint {
    pub fn volt_mv(&self) -> f32 {
        self.volt_uv as f32 / 1000.0
    }
    pub fn stock_mhz(&self) -> f32 {
        self.stock_khz as f32 / 1000.0
    }
    pub fn effective_khz(&self) -> i64 {
        self.stock_khz as i64 + self.delta_khz as i64
    }
    pub fn effective_mhz(&self) -> f32 {
        self.effective_khz() as f32 / 1000.0
    }
}

/// Kurva grafis, diurutkan dari tegangan terendah ke tertinggi.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Curve {
    pub points: Vec<VfPoint>,
}

#[derive(Clone, Debug, Default)]
pub struct Telemetry {
    pub clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub voltage_mv: Option<f32>,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub temp_c: Option<u32>,
    pub util_pct: Option<u32>,
    pub fan_pct: Option<u32>,
    pub vram_used_mb: Option<u32>,
    pub vram_total_mb: Option<u32>,
    /// Alasan clock ditahan (bitmask NVML), sudah diterjemahkan.
    pub limits: Vec<&'static str>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub driver: String,
    pub backend: String,
    /// Batas offset yang diizinkan driver (kHz).
    pub min_delta_khz: i32,
    pub max_delta_khz: i32,
}

pub type GpuResult<T> = Result<T, String>;

pub trait Gpu: Send {
    fn info(&self) -> GpuInfo;
    /// Membaca kurva grafis beserta offset yang terpasang.
    fn curve(&mut self) -> GpuResult<Curve>;
    /// Menulis offset untuk titik tertentu: (indeks NVAPI, offset kHz).
    fn set_deltas(&mut self, deltas: &[(usize, i32)]) -> GpuResult<()>;
    fn telemetry(&mut self) -> Telemetry;
    /// Laporan mentah untuk diagnosis kompatibilitas.
    fn diagnostics(&mut self) -> String;
    /// Target frekuensi bawaan bila GPU tidak bisa memakai clock terukur (AMD: frekuensi maksimum).
    fn default_target_khz(&self) -> Option<u32> {
        None
    }

    /// Mengembalikan semua titik grafis ke offset 0.
    fn reset(&mut self) -> GpuResult<()> {
        let c = self.curve()?;
        let d: Vec<(usize, i32)> = c.points.iter().map(|p| (p.index, 0)).collect();
        self.set_deltas(&d)
    }
}

/// Membuka GPU NVIDIA pertama, atau GPU AMD (ADLX) bila tidak ada NVIDIA. Di luar Windows selalu
/// mengembalikan GPU simulasi.
pub fn open() -> GpuResult<Box<dyn Gpu>> {
    #[cfg(windows)]
    {
        match native::NativeGpu::open() {
            Ok(g) => Ok(Box::new(g) as Box<dyn Gpu>),
            Err(nv) => match adlx::AdlxGpu::open() {
                Ok(g) => Ok(Box::new(g) as Box<dyn Gpu>),
                Err(amd) => Err(format!("NVIDIA: {nv}\nAMD: {amd}")),
            },
        }
    }
    #[cfg(not(windows))]
    {
        Ok(Box::new(sim::SimGpu::standalone()))
    }
}
