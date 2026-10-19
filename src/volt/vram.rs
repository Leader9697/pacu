//! Overclock VRAM otomatis (GPU NVIDIA).
//!
//! GDDR6/GDDR6X memakai koreksi error (EDC): saat clock memori terlalu tinggi, data yang salah
//! dikirim ulang sehingga bandwidth turun sebelum muncul artefak atau crash. Karena itu ukurannya
//! adalah bandwidth nyata: compute shader D3D11 membaca dan menulis buffer besar sambil memeriksa
//! setiap nilai. Offset dinaikkan per 100 MHz selama bandwidth naik dan tidak ada error; titik
//! terbaik lalu dikurangi 50 MHz sebagai margin dan divalidasi 60 detik.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const STEP: i32 = 100;
pub const MARGIN: i32 = 50;
/// Bandwidth turun lebih dari ini dibanding titik terbaik = EDC mulai mengirim ulang data.
pub const DROP: f64 = 0.015;
pub const MEASURE_SECS: u32 = 12;
pub const VALIDATE_SECS: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Next {
    Test(i32),
    Done(i32),
}

/// Pencarian offset (MHz, satuan sama dengan Afterburner). Murni, tanpa akses GPU.
#[derive(Clone, Debug)]
pub struct MemSearch {
    pub max: i32,
    pub best: (i32, f64),
    pub fail_at: Option<i32>,
    fine: bool,
}

impl MemSearch {
    pub fn new(max: i32) -> Self {
        MemSearch { max, best: (0, 0.0), fail_at: None, fine: false }
    }

    /// Hasil uji di `offset`: `Some(bw)` bila lolos tanpa error, `None` bila gagal.
    pub fn report(&mut self, offset: i32, bw: Option<f64>) -> Next {
        let ok = match bw {
            Some(b) if self.best.1 > 0.0 && b < self.best.1 * (1.0 - DROP) => false,
            Some(b) => {
                if b >= self.best.1 {
                    self.best = (offset, b);
                }
                true
            }
            None => false,
        };
        if !ok {
            if offset == 0 {
                return Next::Done(0);
            }
            self.fail_at = Some(self.fail_at.map_or(offset, |f| f.min(offset)));
            if !self.fine && offset - self.best.0 > STEP / 2 {
                self.fine = true;
                return Next::Test(self.best.0 + STEP / 2);
            }
            return Next::Done((self.best.0 - MARGIN).max(0));
        }
        if self.fine {
            return Next::Done((self.best.0 - MARGIN).max(0));
        }
        let next = offset + STEP;
        if next > self.max {
            return Next::Done(self.best.0);
        }
        Next::Test(next)
    }
}

/// Hasil tersimpan, dipasang lagi oleh `--volt-apply`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VramProfile {
    pub gpu_name: String,
    pub offset_mhz: i32,
    pub base_gbs: f64,
    pub best_gbs: f64,
    pub created: u64,
}

impl VramProfile {
    pub fn load() -> Option<Self> {
        crate::volt::store::load("vram.json")
    }
    pub fn save(&self) {
        crate::volt::store::save("vram.json", self)
    }
    pub fn clear() {
        let _ = std::fs::remove_file(crate::volt::store::dir().join("vram.json"));
    }
}

#[derive(Clone, Debug, Default)]
pub struct Measure {
    pub gbs: f64,
    pub errors: u64,
    /// Perangkat D3D hilang (TDR) atau event driver.
    pub lost: Option<String>,
    /// Uji tidak bisa disiapkan sama sekali (bukan tanda memori tidak stabil).
    pub setup_error: Option<String>,
}

pub trait MemRig: Send {
    fn gpu_name(&self) -> String;
    /// (sekarang, min, maks) dalam MHz.
    fn offset(&self) -> Result<(i32, i32, i32), String>;
    fn set_offset(&mut self, mhz: i32) -> Result<(), String>;
    fn measure(&mut self, secs: u32, stop: &AtomicBool) -> Measure;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub phase: Phase,
    pub msg: String,
    pub testing: Option<i32>,
    /// (offset, GB/s, error)
    pub results: Vec<(i32, f64, u64, bool)>,
    pub log: Vec<String>,
}

pub struct VramTuner {
    pub view: Arc<parking_lot::RwLock<View>>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Default for VramTuner {
    fn default() -> Self {
        Self::new()
    }
}

impl VramTuner {
    pub fn new() -> Self {
        VramTuner { view: Default::default(), stop: Arc::new(AtomicBool::new(false)), handle: None }
    }

    pub fn running(&self) -> bool {
        self.handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false)
    }

    pub fn start(&mut self) {
        if self.running() {
            return;
        }
        self.stop.store(false, Ordering::Relaxed);
        let view = self.view.clone();
        let stop = self.stop.clone();
        *view.write() = View { phase: Phase::Running, msg: "Menyiapkan uji bandwidth VRAM".into(), ..Default::default() };
        self.handle = Some(std::thread::spawn(move || match open_rig() {
            Ok(rig) => run(rig, &view, &stop),
            Err(e) => {
                let mut v = view.write();
                v.phase = Phase::Failed;
                v.msg = e;
            }
        }));
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for VramTuner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn log(view: &parking_lot::RwLock<View>, s: String) {
    crate::volt::store::append_log(&format!("{} VRAM: {s}", crate::volt::store::stamp()));
    view.write().log.push(s);
}

fn fail(rig: &mut dyn MemRig, view: &parking_lot::RwLock<View>, original: i32, msg: String) {
    let _ = rig.set_offset(original);
    log(view, format!("{msg}. Offset dikembalikan ke {original:+} MHz."));
    let mut v = view.write();
    v.phase = Phase::Failed;
    v.testing = None;
    v.msg = msg;
}

/// Uji satu offset. `Some(bw)` bila lolos.
fn test(rig: &mut dyn MemRig, view: &parking_lot::RwLock<View>, stop: &AtomicBool, off: i32, secs: u32) -> Result<Option<f64>, String> {
    rig.set_offset(off)?;
    {
        let mut v = view.write();
        v.testing = Some(off);
        v.msg = format!("Menguji offset memori {off:+} MHz");
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    let m = rig.measure(secs, stop);
    if stop.load(Ordering::Relaxed) {
        return Err("Dihentikan".into());
    }
    if let Some(e) = m.setup_error {
        return Err(format!("Uji bandwidth tidak bisa berjalan: {e}"));
    }
    let ok = m.errors == 0 && m.lost.is_none() && m.gbs > 0.0;
    view.write().results.push((off, m.gbs, m.errors, ok));
    let detail = match (&m.lost, m.errors) {
        (Some(e), _) => format!("gagal: {e}"),
        (None, n) if n > 0 => format!("gagal: {n} error data"),
        _ => "lolos".into(),
    };
    log(view, format!("{off:+} MHz: {:.1} GB/s, {detail}", m.gbs));
    Ok(ok.then_some(m.gbs))
}

pub fn run(mut rig: Box<dyn MemRig>, view: &parking_lot::RwLock<View>, stop: &AtomicBool) {
    let rig = rig.as_mut();
    let (original, _, max) = match rig.offset() {
        Ok(o) => o,
        Err(e) => return fail(rig, view, 0, format!("Offset memori tidak terbaca: {e}")),
    };
    let max = max.min(3000);
    if max <= 0 {
        return fail(rig, view, original, "Driver tidak mengizinkan menaikkan clock memori".into());
    }
    log(view, format!("Mulai. Offset sekarang {original:+} MHz, batas driver +{max} MHz"));
    let mut search = MemSearch::new(max);
    let mut off = 0;
    let base;
    loop {
        let r = match test(rig, view, stop, off, MEASURE_SECS) {
            Ok(r) => r,
            Err(e) => return fail(rig, view, original, e),
        };
        if off == 0 && r.is_none() {
            return fail(rig, view, original, "Memori sudah error di clock bawaan; pengujian dihentikan".into());
        }
        match search.report(off, r) {
            Next::Test(n) => off = n,
            Next::Done(f) => {
                off = f;
                break;
            }
        }
    }
    base = view.read().results.first().map(|r| r.1).unwrap_or(0.0);
    // Validasi; bila gagal turunkan 100 MHz dan coba sekali lagi.
    let mut final_off = off;
    for attempt in 0..2 {
        log(view, format!("Validasi {final_off:+} MHz selama {VALIDATE_SECS} detik"));
        match test(rig, view, stop, final_off, VALIDATE_SECS) {
            Ok(Some(bw)) if bw >= search.best.1 * (1.0 - DROP * 2.0) || final_off == 0 => {
                let p = VramProfile { gpu_name: rig.gpu_name(), offset_mhz: final_off, base_gbs: base, best_gbs: bw, created: crate::volt::store::now_secs() };
                p.save();
                let gain = if base > 0.0 { (bw / base - 1.0) * 100.0 } else { 0.0 };
                let msg = format!("Offset memori {final_off:+} MHz: bandwidth {base:.0} ke {bw:.0} GB/s ({gain:+.1}%)");
                log(view, format!("{msg}. Disimpan dan terpasang."));
                let mut v = view.write();
                v.phase = Phase::Done;
                v.testing = None;
                v.msg = msg;
                return;
            }
            Ok(_) if attempt == 0 && final_off > 0 => final_off = (final_off - STEP).max(0),
            Ok(_) => return fail(rig, view, original, "Validasi gagal".into()),
            Err(e) => return fail(rig, view, original, e),
        }
    }
    fail(rig, view, original, "Validasi gagal".into())
}

/// Dipanggil `--volt-apply` dan tombol "Pasang": memasang offset memori tersimpan.
pub fn apply_saved() -> Result<Option<i32>, String> {
    let Some(p) = VramProfile::load() else { return Ok(None) };
    let mut rig = open_rig()?;
    if !p.gpu_name.is_empty() && rig.gpu_name() != p.gpu_name {
        return Err(format!("GPU berbeda ({} vs {})", rig.gpu_name(), p.gpu_name));
    }
    rig.set_offset(p.offset_mhz)?;
    Ok(Some(p.offset_mhz))
}

/// Offset memori yang sedang terpasang di driver (MHz). Ringan: hanya membaca Pstates20.
pub fn current_offset() -> Result<i32, String> {
    #[cfg(windows)]
    {
        let api = crate::volt::gpu::nvapi::NvApi::load()?;
        Ok(api.mem_offset()?.0 / 1000)
    }
    #[cfg(not(windows))]
    {
        Ok(0)
    }
}

pub fn reset() -> Result<(), String> {
    open_rig()?.set_offset(0)
}

#[cfg(windows)]
fn open_rig() -> Result<Box<dyn MemRig>, String> {
    Ok(Box::new(imp::NativeMem::new()?))
}

#[cfg(not(windows))]
fn open_rig() -> Result<Box<dyn MemRig>, String> {
    Ok(Box::new(SimMem { off: 0 }))
}

/// Memori tiruan untuk pengembangan di Linux: bandwidth naik sampai +1300 MHz, lalu EDC.
#[cfg(not(windows))]
struct SimMem {
    off: i32,
}

#[cfg(not(windows))]
impl MemRig for SimMem {
    fn gpu_name(&self) -> String {
        "GPU simulasi".into()
    }
    fn offset(&self) -> Result<(i32, i32, i32), String> {
        Ok((self.off, -1000, 1500))
    }
    fn set_offset(&mut self, mhz: i32) -> Result<(), String> {
        self.off = mhz;
        Ok(())
    }
    fn measure(&mut self, _secs: u32, _stop: &AtomicBool) -> Measure {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let o = self.off as f64;
        let gbs = if o <= 1300.0 { 560.0 * (1.0 + o / 9500.0) } else { 560.0 * (1.0 + 1300.0 / 9500.0) * 0.9 };
        Measure { gbs, errors: if self.off > 1500 { 3 } else { 0 }, ..Default::default() }
    }
}

#[cfg(windows)]
mod imp {
    use super::{MemRig, Measure};
    use crate::volt::gpu::nvapi::NvApi;
    use crate::volt::rig::Watchdog;
    use crate::volt::watchdog::EventLogWatchdog;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;
    use windows::core::PCSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
    use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    /// 32 Mi elemen per buffer (128 MB), 4 elemen per thread.
    const N: u32 = 32 * 1024 * 1024;
    const GROUPS: u32 = N / 1024;
    const PASSES: u32 = 200;

    const INIT: &str = r#"
RWBuffer<uint> dst : register(u1);
cbuffer C : register(b0) { uint seed; uint n; uint p0; uint p1; };
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    uint i = id.x * 4;
    uint s = seed * 40503u;
    dst[i] = i * 2654435761u + s;
    dst[i + 1] = (i + 1) * 2654435761u + s;
    dst[i + 2] = (i + 2) * 2654435761u + s;
    dst[i + 3] = (i + 3) * 2654435761u + s;
}
"#;

    // Membaca src (harus sama dengan pola seed), menulis pola seed+1 ke dst.
    const STEP: &str = r#"
RWBuffer<uint> src : register(u0);
RWBuffer<uint> dst : register(u1);
RWBuffer<uint> err : register(u2);
cbuffer C : register(b0) { uint seed; uint n; uint p0; uint p1; };
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    uint i = id.x * 4;
    uint s = seed * 40503u;
    uint w0 = i * 2654435761u + s;
    uint w1 = (i + 1) * 2654435761u + s;
    uint w2 = (i + 2) * 2654435761u + s;
    uint w3 = (i + 3) * 2654435761u + s;
    uint bad = (src[i] != w0 ? 1u : 0u) + (src[i + 1] != w1 ? 1u : 0u) + (src[i + 2] != w2 ? 1u : 0u) + (src[i + 3] != w3 ? 1u : 0u);
    dst[i] = w0 + 40503u;
    dst[i + 1] = w1 + 40503u;
    dst[i + 2] = w2 + 40503u;
    dst[i + 3] = w3 + 40503u;
    // Tanpa atomik (tidak semua compiler HLSL mendukungnya): jumlahnya bisa kurang, tetapi tidak pernah 0 bila ada error.
    if (bad != 0) { err[0] = err[0] + bad; }
}
"#;

    fn adapter() -> Option<IDXGIAdapter1> {
        unsafe {
            let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                if a.GetDesc1().map(|d| d.VendorId == 0x10DE).unwrap_or(false) {
                    return Some(a);
                }
                i += 1;
            }
            None
        }
    }

    unsafe fn compile(dev: &ID3D11Device, src: &str) -> Result<ID3D11ComputeShader, String> {
        let mut blob: Option<ID3DBlob> = None;
        let mut errs: Option<ID3DBlob> = None;
        if let Err(e) = D3DCompile(
            src.as_ptr() as _,
            src.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(c"main".as_ptr() as _),
            PCSTR(c"cs_5_0".as_ptr() as _),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut blob,
            Some(&mut errs),
        ) {
            let msg = errs
                .map(|b| String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).to_string())
                .unwrap_or_default();
            return Err(format!("Kompilasi shader gagal: {e} {msg}"));
        }
        let blob = blob.ok_or("blob kosong")?;
        let bytes = std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
        let mut cs = None;
        dev.CreateComputeShader(bytes, None, Some(&mut cs)).map_err(|e| format!("CreateComputeShader: {e}"))?;
        cs.ok_or_else(|| "shader kosong".into())
    }

    struct Bench {
        dev: ID3D11Device,
        ctx: ID3D11DeviceContext,
        init: ID3D11ComputeShader,
        step: ID3D11ComputeShader,
        uavs: [ID3D11UnorderedAccessView; 2],
        err: ID3D11Buffer,
        err_uav: ID3D11UnorderedAccessView,
        staging: ID3D11Buffer,
        cb: ID3D11Buffer,
        seed: u32,
        flip: usize,
    }

    impl Bench {
        fn new() -> Result<Self, String> {
            unsafe {
                let a = adapter().ok_or("Adapter DXGI NVIDIA tidak ditemukan")?;
                let (mut dev, mut ctx) = (None, None);
                D3D11CreateDevice(&a, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_FLAG(0), Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))
                    .map_err(|e| format!("D3D11CreateDevice: {e}"))?;
                let dev: ID3D11Device = dev.ok_or("perangkat kosong")?;
                let ctx = ctx.ok_or("konteks kosong")?;
                let buffer = |elems: u32, usage: D3D11_USAGE, bind: u32, cpu: u32| -> Result<ID3D11Buffer, String> {
                    let d = D3D11_BUFFER_DESC { ByteWidth: elems * 4, Usage: usage, BindFlags: bind, CPUAccessFlags: cpu, MiscFlags: 0, StructureByteStride: 0 };
                    let mut b = None;
                    dev.CreateBuffer(&d, None, Some(&mut b)).map_err(|e| format!("CreateBuffer: {e}"))?;
                    b.ok_or_else(|| "buffer kosong".into())
                };
                let uav = |b: &ID3D11Buffer, elems: u32| -> Result<ID3D11UnorderedAccessView, String> {
                    let d = D3D11_UNORDERED_ACCESS_VIEW_DESC {
                        Format: DXGI_FORMAT_R32_UINT,
                        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 { Buffer: D3D11_BUFFER_UAV { FirstElement: 0, NumElements: elems, Flags: 0 } },
                    };
                    let mut u = None;
                    dev.CreateUnorderedAccessView(b, Some(&d), Some(&mut u)).map_err(|e| format!("CreateUAV: {e}"))?;
                    u.ok_or_else(|| "UAV kosong".into())
                };
                let ua = D3D11_BIND_UNORDERED_ACCESS.0 as u32;
                let a0 = buffer(N, D3D11_USAGE_DEFAULT, ua, 0)?;
                let a1 = buffer(N, D3D11_USAGE_DEFAULT, ua, 0)?;
                let err = buffer(4, D3D11_USAGE_DEFAULT, ua, 0)?;
                let staging = buffer(4, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ.0 as u32)?;
                let cb = buffer(4, D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER.0 as u32, 0)?;
                let uavs = [uav(&a0, N)?, uav(&a1, N)?];
                let err_uav = uav(&err, 4)?;
                let init = compile(&dev, INIT)?;
                let step = compile(&dev, STEP)?;
                let mut b = Bench { dev, ctx, init, step, uavs, err, err_uav, staging, cb, seed: 1, flip: 0 };
                b.reset_pattern();
                Ok(b)
            }
        }

        fn set_seed(&self) {
            unsafe {
                let c = [self.seed, N, 0, 0];
                self.ctx.UpdateSubresource(&self.cb, 0, None, c.as_ptr() as _, 0, 0);
                self.ctx.CSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            }
        }

        /// Menulis ulang pola ke buffer sumber (juga setelah error, supaya error tidak terbawa).
        fn reset_pattern(&mut self) {
            unsafe {
                self.set_seed();
                self.ctx.CSSetShader(&self.init, None);
                let u: [Option<ID3D11UnorderedAccessView>; 2] = [None, Some(self.uavs[self.flip].clone())];
                self.ctx.CSSetUnorderedAccessViews(0, 2, Some(u.as_ptr()), None);
                self.ctx.Dispatch(GROUPS, 1, 1);
                self.flip = 1 - self.flip;
                self.ctx.ClearUnorderedAccessViewUint(&self.err_uav, &[0; 4]);
            }
        }

        /// `PASSES` kali baca+tulis 2 x 128 MB. Mengembalikan (byte, error) atau Err bila perangkat hilang.
        fn batch(&mut self) -> Result<(f64, u64), String> {
            unsafe {
                self.ctx.CSSetShader(&self.step, None);
                for _ in 0..PASSES {
                    // flip menunjuk buffer berikutnya untuk ditulis; sumber = yang lain.
                    let src = 1 - self.flip;
                    self.set_seed();
                    let u = [Some(self.uavs[src].clone()), Some(self.uavs[self.flip].clone()), Some(self.err_uav.clone())];
                    self.ctx.CSSetUnorderedAccessViews(0, 3, Some(u.as_ptr()), None);
                    self.ctx.Dispatch(GROUPS, 1, 1);
                    self.seed = self.seed.wrapping_add(1);
                    self.flip = 1 - self.flip;
                }
                self.ctx.CopyResource(&self.staging, &self.err);
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                self.ctx.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut m)).map_err(|e| format!("{e}"))?;
                let errors = *(m.pData as *const u32) as u64;
                self.ctx.Unmap(&self.staging, 0);
                if let Err(e) = self.dev.GetDeviceRemovedReason() {
                    return Err(format!("perangkat D3D hilang ({e})"));
                }
                if errors > 0 {
                    self.reset_pattern();
                }
                Ok((PASSES as f64 * N as f64 * 8.0, errors))
            }
        }
    }

    pub struct NativeMem {
        api: NvApi,
        bench: Option<Bench>,
        wd: EventLogWatchdog,
        name: String,
    }

    // Objek D3D11 hanya dipakai dari thread pengujian.
    unsafe impl Send for NativeMem {}

    impl NativeMem {
        pub fn new() -> Result<Self, String> {
            let api = NvApi::load()?;
            let name = api.name();
            Ok(NativeMem { api, bench: None, wd: EventLogWatchdog::new(), name })
        }
    }

    impl MemRig for NativeMem {
        fn gpu_name(&self) -> String {
            self.name.clone()
        }

        fn offset(&self) -> Result<(i32, i32, i32), String> {
            let (c, lo, hi) = self.api.mem_offset()?;
            Ok((c / 1000, lo / 1000, hi / 1000))
        }

        fn set_offset(&mut self, mhz: i32) -> Result<(), String> {
            self.api.set_mem_offset(mhz * 1000)
        }

        fn measure(&mut self, secs: u32, stop: &AtomicBool) -> Measure {
            if self.bench.is_none() {
                match Bench::new() {
                    Ok(b) => self.bench = Some(b),
                    Err(e) => return Measure { setup_error: Some(e), ..Default::default() },
                }
            }
            self.wd.mark();
            let b = self.bench.as_mut().unwrap();
            // Pemanasan supaya clock memori naik ke P0.
            let _ = b.batch();
            let (mut bytes, mut errors, mut lost) = (0.0, 0u64, None);
            let start = Instant::now();
            while start.elapsed().as_secs() < secs as u64 && !stop.load(Ordering::Relaxed) {
                match b.batch() {
                    Ok((n, e)) => {
                        bytes += n;
                        errors += e;
                    }
                    Err(e) => {
                        lost = Some(e);
                        break;
                    }
                }
            }
            let t = start.elapsed().as_secs_f64();
            if lost.is_some() {
                self.bench = None;
                let _ = self.api.init();
            }
            if lost.is_none() {
                lost = self.wd.events_since_mark().into_iter().next();
            }
            Measure { gbs: if t > 0.0 { bytes / t / 1e9 } else { 0.0 }, errors, lost, setup_error: None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(bw: impl Fn(i32) -> Option<f64>, max: i32) -> (i32, Vec<i32>) {
        let mut s = MemSearch::new(max);
        let mut off = 0;
        let mut seen = vec![];
        loop {
            seen.push(off);
            match s.report(off, bw(off)) {
                Next::Test(n) => off = n,
                Next::Done(f) => return (f, seen),
            }
        }
    }

    #[test]
    fn stops_when_bandwidth_drops() {
        // Naik sampai +1300, lalu EDC menurunkan bandwidth 5%.
        let bw = |o: i32| Some(if o <= 1300 { 500.0 + o as f64 * 0.05 } else { 500.0 * 0.95 });
        let (f, seen) = drive(bw, 2000);
        assert_eq!(seen.last(), Some(&1350));
        assert_eq!(f, 1250);
    }

    #[test]
    fn errors_count_as_failure() {
        let bw = |o: i32| if o >= 900 { None } else { Some(500.0 + o as f64 * 0.05) };
        let (f, seen) = drive(bw, 2000);
        assert!(seen.contains(&850));
        // 850 gagal juga (>= 900? tidak, 850 lolos) jadi terbaik 850, dikurangi margin.
        assert_eq!(f, 800);
    }

    #[test]
    fn reaches_driver_limit() {
        let bw = |o: i32| Some(500.0 + o as f64 * 0.05);
        let (f, _) = drive(bw, 1000);
        assert_eq!(f, 1000);
    }

    #[test]
    fn baseline_failure_gives_zero() {
        let (f, _) = drive(|_| None, 1000);
        assert_eq!(f, 0);
    }
}
