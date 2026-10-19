//! Akses NVAPI (nvapi64.dll dari driver NVIDIA) lewat nvapi_QueryInterface.
//!
//! Fungsi kurva V/F tidak didokumentasikan NVIDIA. ID fungsi dan versi struct di bawah sama dengan
//! yang dipakai RTHAL.dll milik MSI Afterburner 4.6.6 (dicek dengan analisis statis), dan tata
//! letaknya sesuai catatan komunitas (nvapi-rs, nvapioc, nvcurve):
//!
//! | Fungsi                        | ID         | Ukuran | Entri (mulai 0x44)             |
//! |-------------------------------|------------|--------|--------------------------------|
//! | ClkVfPointsGetInfo            | 0x507B4B59 | 0x182C | 24 B: tipe clock, aktif (u8)   |
//! | ClkVfPointsGetStatus          | 0x21537AD4 | 0x1C28 | 28 B: tipe, kHz, µV            |
//! | ClkVfPointsGetControl / Set   | 0x23F1B133 / 0x0733E009 | 0x2420 | 36 B: tipe, offset kHz di +0x14 |
//! | ClkDomainsGetInfo             | 0x64B43A6A | 0x0928 | batas offset                   |
//! | ClientVoltRailsGetStatus      | 0x465F9BCF | 0x004C | µV di 0x28                     |
//!
//! Semua struct versi 1: `version = ukuran | (1 << 16)`. Mask 256 bit ada di offset 4.

#![allow(clippy::upper_case_acronyms)]

use std::ffi::c_void;
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

pub const ID_INITIALIZE: u32 = 0x0150_E828;
pub const ID_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
pub const ID_GET_FULL_NAME: u32 = 0xCEEE_8E9F;
pub const ID_DRIVER_VERSION: u32 = 0x2926_AAAD;
pub const ID_VF_INFO: u32 = 0x507B_4B59;
pub const ID_VF_STATUS: u32 = 0x2153_7AD4;
pub const ID_VF_GET_CONTROL: u32 = 0x23F1_B133;
pub const ID_VF_SET_CONTROL: u32 = 0x0733_E009;
pub const ID_CLK_DOMAINS: u32 = 0x64B4_3A6A;
pub const ID_VOLT_STATUS: u32 = 0x465F_9BCF;
/// NvAPI_GPU_GetPstates20 (publik di NVAPI SDK) dan SetPstates20 (ID sama dengan yang dipakai
/// RTHAL.dll Afterburner untuk offset memori). Struct V1 0x1C94 / V2 0x1CF8.
pub const ID_GET_PSTATES20: u32 = 0x6FF8_1213;
pub const ID_SET_PSTATES20: u32 = 0x0F4D_AE6B;
const PST_V1_SIZE: usize = 0x1C94;
const PST_V2_SIZE: usize = 0x1CF8;
const PST_BASE: usize = 20;
const PST_STRIDE: usize = 456;
const PST_CLOCKS: usize = 8;
const CLK_STRIDE: usize = 44;
const CLK_DELTA: usize = 12;
const CLOCK_MEMORY: u32 = 4;

/// Offset clock memori P0 dari buffer GetPstates20: (sekarang, min, maks) dalam kHz.
pub fn parse_mem_offset(b: &[u8]) -> Option<(i32, i32, i32)> {
    let np = le_u32(b, 8).min(16) as usize;
    let nc = le_u32(b, 12).min(8) as usize;
    for p in 0..np {
        let ps = PST_BASE + p * PST_STRIDE;
        if le_u32(b, ps) != 0 {
            continue;
        }
        for c in 0..nc {
            let ce = ps + PST_CLOCKS + c * CLK_STRIDE;
            if le_u32(b, ce) == CLOCK_MEMORY {
                let d = ce + CLK_DELTA;
                return Some((le_i32(b, d), le_i32(b, d + 4), le_i32(b, d + 8)));
            }
        }
    }
    None
}

/// Buffer SetPstates20 V1 berisi satu pstate (P0) dengan satu clock (memori).
pub fn mem_offset_request(khz: i32) -> Vec<u8> {
    let mut b = vec![0u8; PST_V1_SIZE];
    b[..4].copy_from_slice(&((PST_V1_SIZE as u32) | (1 << 16)).to_le_bytes());
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&1u32.to_le_bytes());
    let ce = PST_BASE + PST_CLOCKS;
    b[ce..ce + 4].copy_from_slice(&CLOCK_MEMORY.to_le_bytes());
    b[ce + CLK_DELTA..ce + CLK_DELTA + 4].copy_from_slice(&khz.to_le_bytes());
    b
}

pub const INFO_SIZE: usize = 0x182C;
pub const STATUS_SIZE: usize = 0x1C28;
pub const CONTROL_SIZE: usize = 0x2420;
pub const DOMAINS_SIZE: usize = 0x0928;
pub const VOLT_SIZE: usize = 0x004C;

pub const ENTRY_BASE: usize = 0x44;
pub const INFO_STRIDE: usize = 24;
pub const STATUS_STRIDE: usize = 28;
pub const CONTROL_STRIDE: usize = 36;
pub const CONTROL_DELTA: usize = 0x14;
pub const MAX_POINTS: usize = 255;

pub fn status_name(code: i32) -> &'static str {
    match code {
        0 => "OK",
        -1 => "NVAPI_ERROR",
        -2 => "LIBRARY_NOT_FOUND",
        -3 => "NO_IMPLEMENTATION",
        -4 => "API_NOT_INITIALIZED",
        -5 => "INVALID_ARGUMENT",
        -6 => "NVIDIA_DEVICE_NOT_FOUND",
        -8 => "INVALID_HANDLE",
        -9 => "INCOMPATIBLE_STRUCT_VERSION",
        -10 => "HANDLE_INVALIDATED",
        -101 => "EXPECTED_PHYSICAL_GPU_HANDLE",
        -104 => "NOT_SUPPORTED",
        -137 => "INVALID_USER_PRIVILEGE",
        _ => "?",
    }
}

pub fn err(what: &str, code: i32) -> String {
    format!("{what} gagal: {code} ({})", status_name(code))
}

type QueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type FnVoid = unsafe extern "C" fn() -> i32;
type FnHandleBuf = unsafe extern "C" fn(*mut c_void, *mut u8) -> i32;
type FnEnum = unsafe extern "C" fn(*mut *mut c_void, *mut u32) -> i32;
type FnDriver = unsafe extern "C" fn(*mut u32, *mut u8) -> i32;

pub struct NvApi {
    qi: QueryInterface,
    pub gpu: *mut c_void,
}

// Handle NVAPI hanya dipakai dari thread pekerja.
unsafe impl Send for NvApi {}

pub fn le_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
pub fn le_i32(b: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// Buffer struct NVAPI dengan field versi sudah terisi.
pub fn buf(size: usize) -> Vec<u8> {
    let mut b = vec![0u8; size];
    b[..4].copy_from_slice(&((size as u32) | (1 << 16)).to_le_bytes());
    b
}

impl NvApi {
    pub fn load() -> Result<Self, String> {
        unsafe {
            let lib = LoadLibraryW(windows::core::w!("nvapi64.dll"))
                .map_err(|e| format!("nvapi64.dll tidak ditemukan ({e}). Driver NVIDIA terpasang?"))?;
            let p = GetProcAddress(lib, PCSTR(c"nvapi_QueryInterface".as_ptr() as _))
                .ok_or("nvapi_QueryInterface tidak ada di nvapi64.dll")?;
            let qi: QueryInterface = std::mem::transmute(p);
            let mut me = Self { qi, gpu: std::ptr::null_mut() };
            me.init()?;
            Ok(me)
        }
    }

    pub fn func(&self, id: u32) -> Result<*mut c_void, String> {
        let p = unsafe { (self.qi)(id) };
        if p.is_null() {
            Err(format!("Fungsi NVAPI {id:#010X} tidak tersedia di driver ini"))
        } else {
            Ok(p)
        }
    }

    /// Inisialisasi ulang, juga dipakai setelah driver di-reset.
    pub fn init(&mut self) -> Result<(), String> {
        unsafe {
            let init: FnVoid = std::mem::transmute(self.func(ID_INITIALIZE)?);
            let r = init();
            if r != 0 {
                return Err(err("NvAPI_Initialize", r));
            }
            let en: FnEnum = std::mem::transmute(self.func(ID_ENUM_PHYSICAL_GPUS)?);
            let mut handles = [std::ptr::null_mut::<c_void>(); 64];
            let mut n = 0u32;
            let r = en(handles.as_mut_ptr(), &mut n);
            if r != 0 {
                return Err(err("NvAPI_EnumPhysicalGPUs", r));
            }
            if n == 0 {
                return Err("Tidak ada GPU NVIDIA".into());
            }
            self.gpu = handles[0];
        }
        Ok(())
    }

    /// Memanggil fungsi berbentuk `f(handle, *struct)`.
    pub fn call(&self, id: u32, b: &mut [u8]) -> i32 {
        match self.func(id) {
            Ok(p) => unsafe {
                let f: FnHandleBuf = std::mem::transmute(p);
                f(self.gpu, b.as_mut_ptr())
            },
            Err(_) => -3,
        }
    }

    /// Offset clock memori sekarang dan batasnya (kHz).
    pub fn mem_offset(&self) -> Result<(i32, i32, i32), String> {
        let mut b = vec![0u8; PST_V2_SIZE];
        b[..4].copy_from_slice(&((PST_V2_SIZE as u32) | (2 << 16)).to_le_bytes());
        let r = self.call(ID_GET_PSTATES20, &mut b);
        if r != 0 {
            return Err(err("NvAPI_GPU_GetPstates20", r));
        }
        parse_mem_offset(&b).ok_or_else(|| "Clock memori P0 tidak ada di Pstates20".to_string())
    }

    pub fn set_mem_offset(&self, khz: i32) -> Result<(), String> {
        let mut b = mem_offset_request(khz);
        let r = self.call(ID_SET_PSTATES20, &mut b);
        if r != 0 {
            return Err(err("NvAPI_GPU_SetPstates20", r));
        }
        Ok(())
    }

    pub fn name(&self) -> String {
        let mut b = [0u8; 64];
        let r = self.call(ID_GET_FULL_NAME, &mut b);
        if r != 0 {
            return "GPU NVIDIA".into();
        }
        let end = b.iter().position(|&c| c == 0).unwrap_or(64);
        String::from_utf8_lossy(&b[..end]).trim().to_string()
    }

    pub fn driver(&self) -> String {
        let Ok(p) = self.func(ID_DRIVER_VERSION) else { return "?".into() };
        unsafe {
            let f: FnDriver = std::mem::transmute(p);
            let mut v = 0u32;
            let mut branch = [0u8; 64];
            if f(&mut v, branch.as_mut_ptr()) == 0 {
                format!("{}.{:02}", v / 100, v % 100)
            } else {
                "?".into()
            }
        }
    }

    /// ClkVfPointsGetInfo. `fill_mask` mengisi mask masukan dengan semua bit.
    pub fn vf_info(&self, fill_mask: bool) -> Result<Vec<u8>, String> {
        let mut b = buf(INFO_SIZE);
        if fill_mask {
            b[4..36].fill(0xFF);
        }
        let r = self.call(ID_VF_INFO, &mut b);
        if r != 0 {
            return Err(err("ClkVfPointsGetInfo", r));
        }
        Ok(b)
    }

    pub fn voltage_mv(&self) -> Option<f32> {
        let mut b = buf(VOLT_SIZE);
        if self.call(ID_VOLT_STATUS, &mut b) != 0 {
            return None;
        }
        let uv = le_u32(&b, 0x28);
        (uv > 300_000 && uv < 2_000_000).then_some(uv as f32 / 1000.0)
    }
}

pub fn mask_bit(mask: &[u8], i: usize) -> bool {
    mask[i / 8] & (1 << (i % 8)) != 0
}

pub fn hex_dump(b: &[u8], len: usize) -> String {
    b.iter().take(len).collect::<Vec<_>>().chunks(16).map(|c| c.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n")
}
