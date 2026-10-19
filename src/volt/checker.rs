//! Pemeriksa error komputasi dengan compute shader Direct3D 11.
//!
//! Shader menjalankan rantai operasi integer dan floating point yang hasilnya selalu sama. Hasil
//! acuan direkam saat kurva masih bawaan (dua kali, harus identik). Selama uji, shader dijalankan
//! terus dan setiap hasil yang berbeda dari acuan dihitung sebagai error: tanda GPU salah hitung
//! karena tegangan terlalu rendah, sering muncul sebelum game crash atau artefak terlihat.

use crate::volt::rig::{CheckStatus, Checker};

#[cfg(windows)]
pub use imp::D3dChecker;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;
    use windows::core::PCSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_IEEE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3};
    use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    const GROUPS: u32 = 8192;
    const THREADS: u32 = GROUPS * 256;
    const ITERS: u32 = 2048;
    const SEEDS: [u32; 2] = [0x9E37_79B9, 0x85EB_CA6B];

    fn shader_src(seed: u32) -> String {
        format!(
            r#"
RWBuffer<uint> o : register(u0);
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {{
    uint x = (id.x * 2654435761u) ^ {seed}u;
    float a = 1.0 + (float)(x & 1023u) / 1024.0;
    float b = 1.0 + (float)((x >> 10) & 1023u) / 1024.0;
    [loop] for (uint i = 0; i < {ITERS}u; ++i) {{
        x = x ^ (x << 13);
        x = x ^ (x >> 17);
        x = x ^ (x << 5);
        a = frac(a * b + 0.61803398) + 1.0;
        b = frac(b * a + 0.41421356) + 1.0;
        a = a + (float)(x & 255u) * (1.0 / 4096.0);
    }}
    o[id.x] = x ^ asuint(a) ^ (asuint(b) << 1);
}}
"#
        )
    }

    struct Gpu {
        dev: ID3D11Device,
        ctx: ID3D11DeviceContext,
        shaders: Vec<ID3D11ComputeShader>,
        uav: ID3D11UnorderedAccessView,
        buf: ID3D11Buffer,
        staging: ID3D11Buffer,
    }

    fn nvidia_adapter() -> Option<IDXGIAdapter1> {
        unsafe {
            let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                if let Ok(d) = a.GetDesc1() {
                    if d.VendorId == 0x10DE {
                        return Some(a);
                    }
                }
                i += 1;
            }
            None
        }
    }

    impl Gpu {
        fn new() -> Result<Self, String> {
            unsafe {
                let adapter = nvidia_adapter().ok_or("Adapter DXGI NVIDIA tidak ditemukan")?;
                let mut dev = None;
                let mut ctx = None;
                D3D11CreateDevice(
                    &adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_FLAG(0),
                    Some(&[D3D_FEATURE_LEVEL_11_0]),
                    D3D11_SDK_VERSION,
                    Some(&mut dev),
                    None,
                    Some(&mut ctx),
                )
                .map_err(|e| format!("D3D11CreateDevice: {e}"))?;
                let dev: ID3D11Device = dev.ok_or("perangkat D3D11 kosong")?;
                let ctx = ctx.ok_or("konteks D3D11 kosong")?;

                let mut shaders = Vec::new();
                for seed in SEEDS {
                    let src = shader_src(seed);
                    let mut blob: Option<ID3DBlob> = None;
                    let mut errs: Option<ID3DBlob> = None;
                    let r = D3DCompile(
                        src.as_ptr() as _,
                        src.len(),
                        PCSTR::null(),
                        None,
                        None,
                        PCSTR(c"main".as_ptr() as _),
                        PCSTR(c"cs_5_0".as_ptr() as _),
                        D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_IEEE_STRICTNESS,
                        0,
                        &mut blob,
                        Some(&mut errs),
                    );
                    if let Err(e) = r {
                        let msg = errs
                            .map(|b| String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).to_string())
                            .unwrap_or_default();
                        return Err(format!("Kompilasi shader gagal: {e} {msg}"));
                    }
                    let blob = blob.ok_or("blob shader kosong")?;
                    let bytes = std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
                    let mut cs = None;
                    dev.CreateComputeShader(bytes, None, Some(&mut cs)).map_err(|e| format!("CreateComputeShader: {e}"))?;
                    shaders.push(cs.ok_or("shader kosong")?);
                }

                let desc = D3D11_BUFFER_DESC {
                    ByteWidth: THREADS * 4,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                };
                let mut buf = None;
                dev.CreateBuffer(&desc, None, Some(&mut buf)).map_err(|e| format!("CreateBuffer: {e}"))?;
                let buf = buf.ok_or("buffer kosong")?;
                let mut uav = None;
                let udesc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_UINT,
                    ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 { Buffer: D3D11_BUFFER_UAV { FirstElement: 0, NumElements: THREADS, Flags: 0 } },
                };
                dev.CreateUnorderedAccessView(&buf, Some(&udesc), Some(&mut uav)).map_err(|e| format!("CreateUAV: {e}"))?;
                let sdesc = D3D11_BUFFER_DESC {
                    ByteWidth: THREADS * 4,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                };
                let mut staging = None;
                dev.CreateBuffer(&sdesc, None, Some(&mut staging)).map_err(|e| format!("CreateBuffer staging: {e}"))?;
                Ok(Self { dev, ctx, shaders, uav: uav.ok_or("UAV kosong")?, buf, staging: staging.ok_or("staging kosong")? })
            }
        }

        /// Menjalankan satu shader dan menyalin hasilnya. Err berarti perangkat hilang (TDR).
        fn run(&self, which: usize, out: &mut Vec<u32>) -> Result<(), String> {
            unsafe {
                self.ctx.CSSetShader(&self.shaders[which], None);
                self.ctx.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.uav.clone())), None);
                self.ctx.Dispatch(GROUPS, 1, 1);
                self.ctx.CopyResource(&self.staging, &self.buf);
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                self.ctx.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut m)).map_err(|e| format!("{e}"))?;
                let data = std::slice::from_raw_parts(m.pData as *const u32, THREADS as usize);
                out.clear();
                out.extend_from_slice(data);
                self.ctx.Unmap(&self.staging, 0);
                let reason = self.dev.GetDeviceRemovedReason();
                if reason.is_err() {
                    return Err(format!("perangkat hilang: {reason:?}"));
                }
            }
            Ok(())
        }
    }

    struct Counters {
        runs: AtomicU64,
        errors: AtomicU64,
        lost: AtomicBool,
        active: AtomicBool,
        quit: AtomicBool,
    }

    pub struct D3dChecker {
        c: Arc<Counters>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl D3dChecker {
        pub fn new() -> Self {
            Self {
                c: Arc::new(Counters {
                    runs: AtomicU64::new(0),
                    errors: AtomicU64::new(0),
                    lost: AtomicBool::new(false),
                    active: AtomicBool::new(false),
                    quit: AtomicBool::new(false),
                }),
                thread: None,
            }
        }

        /// Memulai thread: buat perangkat, rekam acuan, lalu tunggu `active`.
        fn spawn(&mut self) -> Result<(), String> {
            if self.thread.as_ref().is_some_and(|t| !t.is_finished()) {
                return Ok(());
            }
            self.c.quit.store(false, Ordering::Relaxed);
            self.c.lost.store(false, Ordering::Relaxed);
            let c = self.c.clone();
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            let t = std::thread::Builder::new()
                .name("pemeriksa-error".into())
                .spawn(move || {
                    let gpu = match Gpu::new() {
                        Ok(g) => g,
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    };
                    let mut golden = Vec::new();
                    let mut tmp = Vec::new();
                    for i in 0..SEEDS.len() {
                        let mut a = Vec::new();
                        if let Err(e) = gpu.run(i, &mut a).and_then(|_| gpu.run(i, &mut tmp)) {
                            let _ = tx.send(Err(format!("Uji awal gagal: {e}")));
                            return;
                        }
                        if a != tmp {
                            let _ = tx.send(Err("Hasil acuan tidak konsisten bahkan di pengaturan bawaan".into()));
                            return;
                        }
                        golden.push(a);
                    }
                    let _ = tx.send(Ok(()));
                    let mut i = 0usize;
                    while !c.quit.load(Ordering::Relaxed) {
                        if !c.active.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                        let w = i % SEEDS.len();
                        i += 1;
                        match gpu.run(w, &mut tmp) {
                            Ok(()) => {
                                let bad = tmp.iter().zip(&golden[w]).filter(|(x, y)| x != y).count() as u64;
                                c.runs.fetch_add(1, Ordering::Relaxed);
                                if bad > 0 {
                                    c.errors.fetch_add(bad, Ordering::Relaxed);
                                }
                            }
                            Err(_) => {
                                c.lost.store(true, Ordering::Relaxed);
                                return;
                            }
                        }
                        // Beri ruang untuk aplikasi beban 3D.
                        std::thread::sleep(Duration::from_millis(25));
                    }
                })
                .map_err(|e| e.to_string())?;
            self.thread = Some(t);
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(r) => r,
                Err(_) => Err("Pemeriksa error tidak merespons".into()),
            }
        }
    }

    impl Default for D3dChecker {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Checker for D3dChecker {
        fn available(&self) -> bool {
            true
        }
        fn prepare(&mut self) -> Result<(), String> {
            self.stop();
            self.spawn()
        }
        fn start(&mut self) {
            if self.spawn().is_ok() {
                self.c.active.store(true, Ordering::Relaxed);
            }
        }
        fn take(&mut self) -> CheckStatus {
            CheckStatus {
                runs: self.c.runs.swap(0, Ordering::Relaxed),
                errors: self.c.errors.swap(0, Ordering::Relaxed),
                device_lost: self.c.lost.swap(false, Ordering::Relaxed),
            }
        }
        fn stop(&mut self) {
            self.c.active.store(false, Ordering::Relaxed);
            self.c.quit.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Drop for D3dChecker {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

/// Pemeriksa kosong untuk platform tanpa Direct3D.
pub struct NoChecker;

impl Checker for NoChecker {
    fn available(&self) -> bool {
        false
    }
    fn prepare(&mut self) -> Result<(), String> {
        Err("tidak tersedia".into())
    }
    fn start(&mut self) {}
    fn take(&mut self) -> CheckStatus {
        CheckStatus::default()
    }
    fn stop(&mut self) {}
}
