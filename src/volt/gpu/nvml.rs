//! Sensor GPU lewat NVML (nvml.dll, ikut terpasang bersama driver NVIDIA). API ini resmi dan
//! terdokumentasi, jadi dipakai untuk semua pembacaan selain tegangan.

use std::ffi::c_void;
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type Dev = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

pub struct Nvml {
    dev: Dev,
    clock: unsafe extern "C" fn(Dev, u32, *mut u32) -> i32,
    power: unsafe extern "C" fn(Dev, *mut u32) -> i32,
    limit: Option<unsafe extern "C" fn(Dev, *mut u32) -> i32>,
    temp: unsafe extern "C" fn(Dev, u32, *mut u32) -> i32,
    util: unsafe extern "C" fn(Dev, *mut Utilization) -> i32,
    fan: Option<unsafe extern "C" fn(Dev, *mut u32) -> i32>,
    reasons: Option<unsafe extern "C" fn(Dev, *mut u64) -> i32>,
    memory: Option<unsafe extern "C" fn(Dev, *mut Memory) -> i32>,
}

unsafe impl Send for Nvml {}

unsafe fn sym<T>(lib: HMODULE, name: &std::ffi::CStr) -> Option<T> {
    GetProcAddress(lib, PCSTR(name.as_ptr() as _)).map(|p| std::mem::transmute_copy(&p))
}

impl Nvml {
    pub fn load() -> Result<Self, String> {
        unsafe {
            let lib = LoadLibraryW(windows::core::w!("nvml.dll"))
                .or_else(|_| {
                    let p: Vec<u16> = "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll\0".encode_utf16().collect();
                    LoadLibraryW(PCWSTR(p.as_ptr()))
                })
                .map_err(|e| format!("nvml.dll tidak ditemukan: {e}"))?;
            let init: unsafe extern "C" fn() -> i32 = sym(lib, c"nvmlInit_v2").ok_or("nvmlInit_v2 tidak ada")?;
            let r = init();
            if r != 0 {
                return Err(format!("nvmlInit gagal: {r}"));
            }
            let by_index: unsafe extern "C" fn(u32, *mut Dev) -> i32 =
                sym(lib, c"nvmlDeviceGetHandleByIndex_v2").ok_or("nvmlDeviceGetHandleByIndex_v2 tidak ada")?;
            let mut dev: Dev = std::ptr::null_mut();
            let r = by_index(0, &mut dev);
            if r != 0 {
                return Err(format!("NVML tidak menemukan GPU: {r}"));
            }
            Ok(Self {
                dev,
                clock: sym(lib, c"nvmlDeviceGetClockInfo").ok_or("nvmlDeviceGetClockInfo tidak ada")?,
                power: sym(lib, c"nvmlDeviceGetPowerUsage").ok_or("nvmlDeviceGetPowerUsage tidak ada")?,
                limit: sym(lib, c"nvmlDeviceGetEnforcedPowerLimit"),
                temp: sym(lib, c"nvmlDeviceGetTemperature").ok_or("nvmlDeviceGetTemperature tidak ada")?,
                util: sym(lib, c"nvmlDeviceGetUtilizationRates").ok_or("nvmlDeviceGetUtilizationRates tidak ada")?,
                fan: sym(lib, c"nvmlDeviceGetFanSpeed"),
                reasons: sym(lib, c"nvmlDeviceGetCurrentClocksEventReasons")
                    .or_else(|| sym(lib, c"nvmlDeviceGetCurrentClocksThrottleReasons")),
                memory: sym(lib, c"nvmlDeviceGetMemoryInfo"),
            })
        }
    }

    pub fn clock_mhz(&self, kind: u32) -> Option<u32> {
        let mut v = 0;
        (unsafe { (self.clock)(self.dev, kind, &mut v) } == 0).then_some(v)
    }

    pub fn power_w(&self) -> Option<f32> {
        let mut v = 0;
        (unsafe { (self.power)(self.dev, &mut v) } == 0).then_some(v as f32 / 1000.0)
    }

    pub fn power_limit_w(&self) -> Option<f32> {
        let f = self.limit?;
        let mut v = 0;
        (unsafe { f(self.dev, &mut v) } == 0).then_some(v as f32 / 1000.0)
    }

    pub fn temp_c(&self) -> Option<u32> {
        let mut v = 0;
        (unsafe { (self.temp)(self.dev, 0, &mut v) } == 0).then_some(v)
    }

    pub fn util_pct(&self) -> Option<u32> {
        let mut u = Utilization::default();
        (unsafe { (self.util)(self.dev, &mut u) } == 0).then_some(u.gpu)
    }

    pub fn fan_pct(&self) -> Option<u32> {
        let f = self.fan?;
        let mut v = 0;
        (unsafe { f(self.dev, &mut v) } == 0).then_some(v)
    }

    /// VRAM (terpakai, total) dalam MB.
    pub fn vram_mb(&self) -> Option<(u32, u32)> {
        let f = self.memory?;
        let mut m = Memory::default();
        (unsafe { f(self.dev, &mut m) } == 0).then_some(((m.used >> 20) as u32, (m.total >> 20) as u32))
    }

    /// Alasan clock ditahan, diterjemahkan.
    pub fn limits(&self) -> Vec<&'static str> {
        let Some(f) = self.reasons else { return vec![] };
        let mut v = 0u64;
        if unsafe { f(self.dev, &mut v) } != 0 {
            return vec![];
        }
        let mut out = vec![];
        if v & 0x4 != 0 {
            out.push("daya");
        }
        if v & (0x20 | 0x40) != 0 {
            out.push("suhu");
        }
        if v & (0x8 | 0x80) != 0 {
            out.push("perangkat keras");
        }
        out
    }
}
