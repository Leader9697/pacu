//! GPU AMD Radeon lewat ADLX (amdadlx64.dll, ikut terpasang bersama driver AMD). ADLX adalah SDK
//! resmi AMD yang juga dipakai Adrenalin; tata letak interface-nya diambil dari header C di
//! ADLX SDK (lisensi MIT).
//!
//! Yang diatur: "Tegangan maksimum" dan "Frekuensi maksimum" (IADLXManualGraphicsTuning2),
//! digambarkan sebagai kurva tiruan oleh `AmdModel`. Didukung untuk GPU dengan tegangan absolut
//! (RX 5000 / RX 6000, RDNA1/2). Seri RX 7000/9000 memakai offset tegangan dan belum didukung.

use super::amdmodel::AmdModel;
use super::{Curve, Gpu, GpuInfo, GpuResult, Telemetry};
use std::ffi::{c_void, CStr};
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type Obj = *mut c_void;
const OK: i32 = 0;
const ALREADY_INITIALIZED: i32 = 2;

/// Fungsi ke-`n` di vtable objek ADLX.
unsafe fn slot<F: Copy>(obj: Obj, n: usize) -> F {
    let vtbl = *(obj as *const *const usize);
    std::mem::transmute_copy(&*vtbl.add(n))
}

unsafe fn release(obj: Obj) {
    if !obj.is_null() {
        let f: unsafe extern "system" fn(Obj) -> i64 = slot(obj, 1);
        f(obj);
    }
}

unsafe fn query(obj: Obj, iid: &str) -> Option<Obj> {
    let w: Vec<u16> = iid.encode_utf16().chain([0]).collect();
    let f: unsafe extern "system" fn(Obj, PCWSTR, *mut Obj) -> i32 = slot(obj, 2);
    let mut out: Obj = std::ptr::null_mut();
    (f(obj, PCWSTR(w.as_ptr()), &mut out) == OK && !out.is_null()).then_some(out)
}

/// Memanggil getter `fn(this, *mut T) -> ADLX_RESULT` di slot `n`.
unsafe fn get<T: Default>(obj: Obj, n: usize) -> Option<T> {
    let f: unsafe extern "system" fn(Obj, *mut T) -> i32 = slot(obj, n);
    let mut v = T::default();
    (f(obj, &mut v) == OK).then_some(v)
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
struct IntRange {
    min: i32,
    max: i32,
    step: i32,
}

// Slot vtable (lihat header ADLX SDK).
mod sys {
    pub const GET_GPUS: usize = 1;
    pub const GET_TUNING: usize = 8;
    pub const GET_PERF: usize = 9;
}
mod gpu {
    pub const NAME: usize = 7;
    pub const TOTAL_VRAM: usize = 11;
}
mod tuning {
    pub const IS_AT_FACTORY: usize = 4;
    pub const SUPPORTED_MANUAL_GFX: usize = 8;
    pub const GET_MANUAL_GFX: usize = 14;
}
mod gfx {
    pub const GET_MAX_FREQ_RANGE: usize = 6;
    pub const GET_MAX_FREQ: usize = 7;
    pub const SET_MAX_FREQ: usize = 8;
    pub const GET_VOLT_RANGE: usize = 9;
    pub const GET_VOLT: usize = 10;
    pub const SET_VOLT: usize = 11;
    pub const GET_MAX_FREQ_DEFAULT: usize = 13;
    pub const GET_VOLT_DEFAULT: usize = 14;
}
mod perf {
    pub const GET_CURRENT_GPU_METRICS: usize = 18;
}
mod metrics {
    pub const USAGE: usize = 4;
    pub const CLOCK: usize = 5;
    pub const VRAM_CLOCK: usize = 6;
    pub const TEMP: usize = 7;
    pub const POWER: usize = 9;
    pub const BOARD_POWER: usize = 10;
    pub const VRAM: usize = 12;
    pub const VOLTAGE: usize = 13;
}

pub struct AdlxGpu {
    lib: HMODULE,
    gpu: Obj,
    tuning: Obj,
    gfx: Obj,
    perf: Obj,
    model: AmdModel,
    name: String,
    version: String,
    vram_total: Option<u32>,
    notes: Vec<String>,
}

unsafe impl Send for AdlxGpu {}

#[derive(serde::Serialize, serde::Deserialize)]
struct Defaults {
    gpu: String,
    volt_mv: i32,
    max_mhz: i32,
}

fn defaults_path() -> std::path::PathBuf {
    crate::volt::store::dir().join("amd-bawaan.json")
}

impl AdlxGpu {
    pub fn open() -> GpuResult<Self> {
        unsafe {
            let lib = LoadLibraryW(windows::core::w!("amdadlx64.dll")).map_err(|_| "amdadlx64.dll tidak ditemukan (bukan GPU AMD atau driver AMD belum terpasang)".to_string())?;
            let sym = |n: &CStr| GetProcAddress(lib, PCSTR(n.as_ptr() as _));
            let query_ver: Option<unsafe extern "C" fn(*mut u64) -> i32> = sym(c"ADLXQueryFullVersion").map(|p| std::mem::transmute(p));
            let query_str: Option<unsafe extern "C" fn(*mut *const i8) -> i32> = sym(c"ADLXQueryVersion").map(|p| std::mem::transmute(p));
            let init: unsafe extern "C" fn(u64, *mut Obj) -> i32 =
                std::mem::transmute(sym(c"ADLXInitialize").ok_or("ADLXInitialize tidak ada di amdadlx64.dll")?);
            let mut full = (2u64 << 48) | 125;
            if let Some(q) = query_ver {
                let mut v = 0u64;
                if q(&mut v) == OK && v != 0 {
                    full = v;
                }
            }
            let mut version = format!("{}.{}", full >> 48, (full >> 32) & 0xFFFF);
            if let Some(q) = query_str {
                let mut p: *const i8 = std::ptr::null();
                if q(&mut p) == OK && !p.is_null() {
                    version = CStr::from_ptr(p).to_string_lossy().into_owned();
                }
            }
            let mut system: Obj = std::ptr::null_mut();
            let r = init(full, &mut system);
            if (r != OK && r != ALREADY_INITIALIZED) || system.is_null() {
                return Err(format!("ADLXInitialize gagal: {r}"));
            }

            let mut list: Obj = std::ptr::null_mut();
            let f: unsafe extern "system" fn(Obj, *mut Obj) -> i32 = slot(system, sys::GET_GPUS);
            if f(system, &mut list) != OK || list.is_null() {
                return Err("ADLX tidak menemukan GPU AMD".into());
            }
            let size: unsafe extern "system" fn(Obj) -> u32 = slot(list, 3);
            let at: unsafe extern "system" fn(Obj, u32, *mut Obj) -> i32 = slot(list, 11);
            let mut gpu: Obj = std::ptr::null_mut();
            let n = size(list);
            // GPU diskrit pertama (Type 2 = diskrit), selain itu GPU pertama.
            for i in 0..n {
                let mut g: Obj = std::ptr::null_mut();
                if at(list, i, &mut g) != OK || g.is_null() {
                    continue;
                }
                let ty = get::<i32>(g, 5).unwrap_or(0);
                if gpu.is_null() || ty == 2 {
                    release(gpu);
                    gpu = g;
                    if ty == 2 {
                        break;
                    }
                } else {
                    release(g);
                }
            }
            release(list);
            if gpu.is_null() {
                return Err("ADLX tidak menemukan GPU AMD".into());
            }
            let name = get::<*const i8>(gpu, gpu::NAME)
                .filter(|p| !p.is_null())
                .map(|p| CStr::from_ptr(p).to_string_lossy().into_owned())
                .unwrap_or_else(|| "GPU AMD".into());
            let vram_total = get::<u32>(gpu, gpu::TOTAL_VRAM);

            let tuning: Obj = get::<Obj>(system, sys::GET_TUNING).filter(|p| !p.is_null()).ok_or("Layanan GPU tuning ADLX tidak tersedia")?;
            let f: unsafe extern "system" fn(Obj, Obj, *mut bool) -> i32 = slot(tuning, tuning::SUPPORTED_MANUAL_GFX);
            let mut sup = false;
            if f(tuning, gpu, &mut sup) != OK || !sup {
                return Err(format!("{name} tidak mendukung manual GPU tuning lewat ADLX"));
            }
            let f: unsafe extern "system" fn(Obj, Obj, *mut Obj) -> i32 = slot(tuning, tuning::GET_MANUAL_GFX);
            let mut iface: Obj = std::ptr::null_mut();
            if f(tuning, gpu, &mut iface) != OK || iface.is_null() {
                return Err("ADLX menolak membuka manual GPU tuning".into());
            }
            let has21 = query(iface, "IADLXManualGraphicsTuning2_1");
            let gfx = match has21.or_else(|| query(iface, "IADLXManualGraphicsTuning2")) {
                Some(g) => g,
                None => {
                    release(iface);
                    return Err(format!("{name}: hanya mendukung tuning gaya lama (sebelum RX 5000), belum didukung"));
                }
            };
            release(iface);
            let perf = get::<Obj>(system, sys::GET_PERF).unwrap_or(std::ptr::null_mut());

            let mut notes = vec![format!("ADLX {version}, interface {}", if has21.is_some() { "ManualGraphicsTuning2_1" } else { "ManualGraphicsTuning2" })];
            let vr = get::<IntRange>(gfx, gfx::GET_VOLT_RANGE).ok_or("Rentang tegangan tidak terbaca")?;
            let fr = get::<IntRange>(gfx, gfx::GET_MAX_FREQ_RANGE).unwrap_or_default();
            let cur_v = get::<i32>(gfx, gfx::GET_VOLT).ok_or("Tegangan tidak terbaca")?;
            let cur_f = get::<i32>(gfx, gfx::GET_MAX_FREQ).ok_or("Frekuensi maksimum tidak terbaca")?;
            notes.push(format!("Rentang tegangan {}..{} langkah {}, frekuensi maks {}..{}, sekarang {cur_v} mV / {cur_f} MHz", vr.min, vr.max, vr.step, fr.min, fr.max));
            if vr.min < 0 || vr.max <= 300 {
                return Err(format!("{name} memakai offset tegangan (RX 7000/9000), belum didukung Pacu"));
            }
            let at_factory = {
                let f: unsafe extern "system" fn(Obj, Obj, *mut bool) -> i32 = slot(tuning, tuning::IS_AT_FACTORY);
                let mut b = false;
                f(tuning, gpu, &mut b) == OK && b
            };
            notes.push(format!("Pengaturan pabrik: {}", if at_factory { "ya" } else { "tidak" }));

            // Nilai bawaan: dari API (2_1), dari kondisi pabrik saat ini, atau dari catatan sebelumnya.
            let saved: Option<Defaults> = std::fs::read_to_string(defaults_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).filter(|d: &Defaults| d.gpu == name);
            let (v_def, f_def) = if let (Some(_), Some(v), Some(f)) = (has21, get::<i32>(gfx, gfx::GET_VOLT_DEFAULT), get::<i32>(gfx, gfx::GET_MAX_FREQ_DEFAULT)) {
                (v, f)
            } else if at_factory {
                (cur_v, cur_f)
            } else if let Some(d) = &saved {
                (d.volt_mv, d.max_mhz)
            } else {
                notes.push("Nilai bawaan ditebak: tegangan = batas atas rentang".into());
                (vr.max, cur_f)
            };
            if saved.is_none() || at_factory {
                let d = Defaults { gpu: name.clone(), volt_mv: v_def, max_mhz: f_def };
                let _ = std::fs::create_dir_all(crate::volt::store::dir());
                let _ = std::fs::write(defaults_path(), serde_json::to_string_pretty(&d).unwrap());
            }
            let step = vr.step.max(5);
            let v_min = vr.min.max(700);
            notes.push(format!("Model: bawaan {v_def} mV / {f_def} MHz, titik tiap {step} mV mulai {v_min} mV"));
            Ok(AdlxGpu {
                lib,
                gpu,
                tuning,
                gfx,
                perf,
                model: AmdModel { v_def, v_min, step, f_def },
                name,
                version,
                vram_total,
                notes,
            })
        }
    }

    fn read(&self) -> GpuResult<(i32, i32)> {
        unsafe {
            let v = get::<i32>(self.gfx, gfx::GET_VOLT).ok_or("Tegangan tidak terbaca")?;
            let f = get::<i32>(self.gfx, gfx::GET_MAX_FREQ).ok_or("Frekuensi maksimum tidak terbaca")?;
            Ok((v, f))
        }
    }

    fn write(&self, volt: i32, mhz: i32) -> GpuResult<()> {
        unsafe {
            let set: unsafe extern "system" fn(Obj, i32) -> i32 = slot(self.gfx, gfx::SET_MAX_FREQ);
            let r = set(self.gfx, mhz);
            if r != OK {
                return Err(format!("SetGPUMaxFrequency({mhz}) gagal: {r}"));
            }
            let set: unsafe extern "system" fn(Obj, i32) -> i32 = slot(self.gfx, gfx::SET_VOLT);
            let r = set(self.gfx, volt);
            if r != OK {
                return Err(format!("SetGPUVoltage({volt}) gagal: {r}"));
            }
            Ok(())
        }
    }
}

impl Drop for AdlxGpu {
    fn drop(&mut self) {
        unsafe {
            release(self.perf);
            release(self.gfx);
            release(self.tuning);
            release(self.gpu);
            if let Some(t) = GetProcAddress(self.lib, PCSTR(c"ADLXTerminate".as_ptr() as _)) {
                let t: unsafe extern "C" fn() -> i32 = std::mem::transmute(t);
                t();
            }
        }
    }
}

impl Gpu for AdlxGpu {
    fn info(&self) -> GpuInfo {
        GpuInfo {
            name: self.name.clone(),
            driver: format!("ADLX {}", self.version),
            backend: "ADLX (AMD)".into(),
            min_delta_khz: -3_000_000,
            max_delta_khz: 3_000_000,
        }
    }

    fn default_target_khz(&self) -> Option<u32> {
        Some(self.model.f_def as u32 * 1000)
    }

    fn curve(&mut self) -> GpuResult<Curve> {
        let (v, f) = self.read()?;
        Ok(self.model.curve(v, f))
    }

    fn set_deltas(&mut self, deltas: &[(usize, i32)]) -> GpuResult<()> {
        let (v, f) = self.model.settings_for(deltas).unwrap_or((self.model.v_def, self.model.f_def));
        self.write(v, f)
    }

    fn telemetry(&mut self) -> Telemetry {
        let mut t = Telemetry { vram_total_mb: self.vram_total, ..Default::default() };
        if self.perf.is_null() {
            return t;
        }
        unsafe {
            let f: unsafe extern "system" fn(Obj, Obj, *mut Obj) -> i32 = slot(self.perf, perf::GET_CURRENT_GPU_METRICS);
            let mut m: Obj = std::ptr::null_mut();
            if f(self.perf, self.gpu, &mut m) != OK || m.is_null() {
                return t;
            }
            t.util_pct = get::<f64>(m, metrics::USAGE).map(|x| x.round() as u32);
            t.clock_mhz = get::<i32>(m, metrics::CLOCK).filter(|&x| x > 0).map(|x| x as u32);
            t.mem_clock_mhz = get::<i32>(m, metrics::VRAM_CLOCK).filter(|&x| x > 0).map(|x| x as u32);
            t.temp_c = get::<f64>(m, metrics::TEMP).filter(|&x| x > 0.0).map(|x| x.round() as u32);
            t.power_w = get::<f64>(m, metrics::BOARD_POWER).filter(|&x| x > 0.0).or_else(|| get::<f64>(m, metrics::POWER)).map(|x| x as f32);
            t.vram_used_mb = get::<i32>(m, metrics::VRAM).filter(|&x| x >= 0).map(|x| x as u32);
            t.voltage_mv = get::<i32>(m, metrics::VOLTAGE).filter(|&x| x > 0).map(|x| x as f32);
            release(m);
        }
        t
    }

    fn diagnostics(&mut self) -> String {
        let mut out = vec![format!("GPU: {}", self.name), format!("Backend: ADLX {}", self.version)];
        out.extend(self.notes.iter().cloned());
        match self.read() {
            Ok((v, f)) => out.push(format!("Sekarang: tegangan maks {v} mV, frekuensi maks {f} MHz")),
            Err(e) => out.push(format!("Baca pengaturan gagal: {e}")),
        }
        let t = self.telemetry();
        out.push(format!("Sensor: {t:?}"));
        for e in crate::volt::watchdog::recent_driver_events(10) {
            out.push(format!("  {e}"));
        }
        out.join("\n")
    }
}
