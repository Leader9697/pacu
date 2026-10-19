//! Integrasi dengan RivaTuner Statistics Server (RTSS) lewat shared memory "RTSSSharedMemoryV2".
//!
//! Formatnya didokumentasikan di SDK yang ikut terpasang bersama RTSS (RTSSSharedMemory.h). RTSS
//! yang menyuntikkan overlay ke game, Pacu hanya membaca FPS per aplikasi dan menitipkan teks ke
//! salah satu slot OSD, cara yang sama dengan HWiNFO dan AIDA64. Pacu sendiri tidak menyuntik apa
//! pun ke game, jadi tidak menambah risiko anti-cheat.

pub const OWNER: &str = "Pacu";
const SIGNATURE: u32 = 0x5254_5353; // 'RTSS'

#[derive(Clone, Debug, PartialEq)]
pub struct RtssApp {
    pub pid: u32,
    pub name: String,
    pub fps: f32,
    pub frametime_ms: Option<f32>,
}

fn u32_at(m: &[u8], off: usize) -> Option<u32> {
    m.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Versi shared memory, atau None bila belum siap / sudah ditandai dihapus.
pub fn version(m: &[u8]) -> Option<u32> {
    (u32_at(m, 0)? == SIGNATURE).then_some(())?;
    let v = u32_at(m, 4)?;
    (v >= 0x0002_0000).then_some(v)
}

/// Aplikasi 3D yang dipantau RTSS. `pid` 0 = aplikasi di depan menurut RTSS (v2.16+).
pub fn find_app(m: &[u8], pid: u32) -> Option<RtssApp> {
    let ver = version(m)?;
    let (size, off, count) = (u32_at(m, 8)? as usize, u32_at(m, 12)? as usize, u32_at(m, 16)? as usize);
    let pid = if pid == 0 && ver >= 0x0002_0010 { u32_at(m, 68)? } else { pid };
    if pid == 0 || size < 284 {
        return None;
    }
    (0..count).find_map(|i| {
        let e = m.get(off + i * size..off + (i + 1) * size)?;
        if u32_at(e, 0)? != pid {
            return None;
        }
        let (t0, t1, frames, ft) = (u32_at(e, 268)?, u32_at(e, 272)?, u32_at(e, 276)?, u32_at(e, 280)?);
        let fps = if t1 > t0 && t0 != 0 { 1000.0 * frames as f32 / (t1 - t0) as f32 } else { 0.0 };
        if fps <= 0.0 {
            return None;
        }
        let name = cstr(&e[4..264]);
        let name = name.rsplit(['\\', '/']).next().unwrap_or(&name).to_string();
        Some(RtssApp { pid, name, fps, frametime_ms: (ft > 0).then(|| ft as f32 / 1000.0) })
    })
}

/// Teks ke byte ANSI (Windows-1252) untuk RTSS.
pub fn ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '°' => 0xB0,
            c if c.is_ascii() => c as u8,
            _ => b'?',
        })
        .collect()
}

/// Mengembalikan indeks slot OSD milik Pacu, atau mengklaim slot kosong. Slot 0 dibiarkan untuk
/// Afterburner, seperti contoh di SDK RTSS.
pub fn claim_slot(m: &mut [u8], claim: bool) -> Option<usize> {
    version(m)?;
    let (size, off, count) = (u32_at(m, 20)? as usize, u32_at(m, 24)? as usize, u32_at(m, 28)? as usize);
    if size < 512 + 4096 {
        return None;
    }
    let owner_at = |i: usize| off + i * size + 256;
    let mine = (1..count).find(|&i| m.get(owner_at(i)..owner_at(i) + 256).is_some_and(|b| cstr(b) == OWNER));
    if mine.is_some() || !claim {
        return mine;
    }
    let free = (1..count).find(|&i| m.get(owner_at(i)).is_some_and(|&b| b == 0))?;
    let o = owner_at(free);
    m[o..o + OWNER.len()].copy_from_slice(OWNER.as_bytes());
    m[o + OWNER.len()] = 0;
    Some(free)
}

/// Menulis teks ke slot OSD (bagian szOSDEx, 4096 byte).
pub fn write_slot(m: &mut [u8], slot: usize, text: &[u8]) {
    let (size, off) = (u32_at(m, 20).unwrap_or(0) as usize, u32_at(m, 24).unwrap_or(0) as usize);
    let ex = off + slot * size + 512;
    let n = text.len().min(4095);
    m[ex..ex + n].copy_from_slice(&text[..n]);
    m[ex + n] = 0;
    // Slot teks pendek dikosongkan supaya tidak tampil ganda.
    m[off + slot * size] = 0;
}

pub fn bump_frame(m: &mut [u8]) {
    let f = u32_at(m, 32).unwrap_or(0).wrapping_add(1);
    m[32..36].copy_from_slice(&f.to_le_bytes());
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};
    use windows::core::w;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_ALL_ACCESS, MEMORY_BASIC_INFORMATION};

    /// Memetakan shared memory RTSS selama `f` berjalan. None bila RTSS tidak berjalan.
    pub fn with_mem<R>(f: impl FnOnce(&mut [u8]) -> R) -> Option<R> {
        unsafe {
            let h = OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, w!("RTSSSharedMemoryV2")).ok()?;
            let view = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            let out = if view.Value.is_null() {
                None
            } else {
                let mut info = MEMORY_BASIC_INFORMATION::default();
                let n = VirtualQuery(Some(view.Value), &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
                let r = (n != 0).then(|| {
                    let mem = std::slice::from_raw_parts_mut(view.Value as *mut u8, info.RegionSize);
                    f(mem)
                });
                let _ = UnmapViewOfFile(view);
                r
            };
            let _ = CloseHandle(h);
            out
        }
    }

    pub fn running() -> bool {
        with_mem(|m| version(m).is_some()).unwrap_or(false)
    }

    pub fn app(pid: u32) -> Option<RtssApp> {
        with_mem(|m| find_app(m, pid)).flatten()
    }

    /// Menampilkan teks di OSD RTSS. Mengembalikan false bila RTSS tidak berjalan atau semua slot penuh.
    pub fn show(text: &str) -> bool {
        let bytes = ansi(text);
        with_mem(|m| {
            let ver = version(m)?;
            let slot = claim_slot(m, true)?;
            // v2.14+: bit 0 dwBusy dipakai RTSS saat sedang menggambar OSD.
            let busy = (ver >= 0x0002_000E).then(|| unsafe { &*(m.as_ptr().add(36) as *const AtomicI32) });
            if let Some(b) = busy {
                if b.fetch_or(1, Ordering::AcqRel) & 1 != 0 {
                    return Some(true);
                }
            }
            write_slot(m, slot, &bytes);
            if let Some(b) = busy {
                b.store(0, Ordering::Release);
            }
            bump_frame(m);
            Some(true)
        })
        .flatten()
        .unwrap_or(false)
    }

    /// Melepas slot OSD milik Pacu.
    pub fn release() {
        with_mem(|m| {
            if let Some(slot) = claim_slot(m, false) {
                write_slot(m, slot, b"");
                let (size, off) = (u32_at(m, 20).unwrap_or(0) as usize, u32_at(m, 24).unwrap_or(0) as usize);
                let o = off + slot * size + 256;
                m[o..o + 256].fill(0);
                bump_frame(m);
            }
        });
    }
}

#[cfg(windows)]
pub use imp::{app, release, running, show};

/// Lokasi RTSS.exe: dari registry installer, lalu folder bawaan.
pub fn find_exe() -> Option<std::path::PathBuf> {
    use crate::tweaks::{reg, Root, Saved};
    let keys = [
        ("SOFTWARE\\WOW6432Node\\Unwinder\\RTSS", "InstallDir"),
        ("SOFTWARE\\WOW6432Node\\Unwinder\\RTSS", "InstallPath"),
        ("SOFTWARE\\Unwinder\\RTSS", "InstallDir"),
        ("SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\RTSS", "InstallLocation"),
    ];
    let mut dirs: Vec<std::path::PathBuf> = keys
        .iter()
        .filter_map(|(k, v)| match reg::read(Root::Hklm, k, v) {
            Ok(Some(Saved::Str(s))) if !s.is_empty() => Some(std::path::PathBuf::from(s.trim_matches('"'))),
            _ => None,
        })
        .collect();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(var) {
            dirs.push(std::path::PathBuf::from(p).join("RivaTuner Statistics Server"));
        }
    }
    dirs.into_iter().map(|d| if d.extension().is_some() { d } else { d.join("RTSS.exe") }).find(|p| p.is_file())
}

/// Menjalankan RTSS (ke tray). RTSS sendiri yang menggambar OSD di dalam game; Pacu tidak menyuntik apa pun.
pub fn launch() -> Result<std::path::PathBuf, String> {
    let exe = find_exe().ok_or("RTSS belum terpasang")?;
    std::process::Command::new(&exe)
        .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
        .spawn()
        .map_err(|e| format!("Gagal menjalankan {}: {e}", exe.display()))?;
    Ok(exe)
}

#[cfg(not(windows))]
pub fn running() -> bool {
    false
}
#[cfg(not(windows))]
pub fn app(_pid: u32) -> Option<RtssApp> {
    None
}
#[cfg(not(windows))]
pub fn show(_text: &str) -> bool {
    false
}
#[cfg(not(windows))]
pub fn release() {}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: usize = 0x1000;
    const OSD: usize = 0x4000;
    const OSD_SIZE: usize = 512 + 4096 + 16;

    fn put(m: &mut [u8], off: usize, v: u32) {
        m[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn mem() -> Vec<u8> {
        let mut m = vec![0u8; OSD + OSD_SIZE * 4];
        put(&mut m, 0, SIGNATURE);
        put(&mut m, 4, 0x0002_0016);
        put(&mut m, 8, 300);
        put(&mut m, 12, APP as u32);
        put(&mut m, 16, 4);
        put(&mut m, 20, OSD_SIZE as u32);
        put(&mut m, 24, OSD as u32);
        put(&mut m, 28, 4);
        put(&mut m, 68, 4242);
        let e = APP + 300;
        put(&mut m, e, 4242);
        m[e + 4..e + 4 + 22].copy_from_slice(b"C:\\Game\\project8.exe\0\0");
        put(&mut m, e + 268, 10_000);
        put(&mut m, e + 272, 11_000);
        put(&mut m, e + 276, 144);
        put(&mut m, e + 280, 6944);
        m
    }

    #[test]
    fn reads_foreground_app() {
        let m = mem();
        let a = find_app(&m, 0).unwrap();
        assert_eq!(a.pid, 4242);
        assert_eq!(a.name, "project8.exe");
        assert!((a.fps - 144.0).abs() < 0.01);
        assert!((a.frametime_ms.unwrap() - 6.944).abs() < 0.001);
        assert!(find_app(&m, 7).is_none());
    }

    #[test]
    fn rejects_dead_memory() {
        let mut m = mem();
        put(&mut m, 0, 0xDEAD);
        assert!(find_app(&m, 0).is_none());
        assert!(claim_slot(&mut m, true).is_none());
    }

    #[test]
    fn claims_free_slot_after_first() {
        let mut m = mem();
        // Slot 1 dipakai aplikasi lain.
        m[OSD + OSD_SIZE + 256..OSD + OSD_SIZE + 256 + 6].copy_from_slice(b"HWiNFO");
        assert_eq!(claim_slot(&mut m, false), None);
        assert_eq!(claim_slot(&mut m, true), Some(2));
        assert_eq!(claim_slot(&mut m, true), Some(2));
        write_slot(&mut m, 2, &ansi("GPU 65°C"));
        let ex = OSD + 2 * OSD_SIZE + 512;
        assert_eq!(&m[ex..ex + 9], b"GPU 65\xB0C\0");
        bump_frame(&mut m);
        assert_eq!(u32_at(&m, 32), Some(1));
    }
}
