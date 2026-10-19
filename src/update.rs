//! Pembaruan otomatis dari GitHub Releases: memeriksa rilis terbaru, mengunduh Pacu.exe beserta
//! Pacu.exe.sha256 lewat HTTPS (WinHTTP, memakai proxy dan sertifikat Windows), mencocokkan
//! SHA-256, lalu mengganti exe yang sedang berjalan dan membuka versi baru.
//!
//! Penggantian exe: file yang sedang berjalan tidak bisa ditimpa, tetapi bisa diganti nama. Exe
//! lama dipindah ke `Pacu.exe.old`, exe baru ditulis di namanya, lalu Pacu ditutup dan versi baru
//! dibuka dengan `--wait-pid` supaya ia menunggu proses lama benar-benar keluar. `.old` dihapus
//! saat versi baru menyala.

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Repo rilis bawaan (bisa diganti di config.json, bagian `update.repo`).
pub const DEFAULT_REPO: &str = "Leader9697/pacu";
pub const EXE_ASSET: &str = "Pacu.exe";
pub const SHA_ASSET: &str = "Pacu.exe.sha256";

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v0.4.1" atau "0.4.1" ke (0, 4, 1). Bagian yang hilang dianggap 0.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u32>());
    let major = it.next()?.ok()?;
    let minor = it.next().unwrap_or(Ok(0)).ok()?;
    let patch = it.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub exe_url: String,
    pub sha_url: String,
    pub size: u64,
}

/// Mengurai jawaban `GET /repos/{repo}/releases/latest`.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Jawaban GitHub tidak terbaca: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("Rilis tidak punya tag")?;
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let find = |name: &str| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)));
    let exe = find(EXE_ASSET).ok_or_else(|| format!("Rilis {tag} tidak berisi {EXE_ASSET}"))?;
    let sha = find(SHA_ASSET).ok_or_else(|| format!("Rilis {tag} tidak berisi {SHA_ASSET}"))?;
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().unwrap_or("").trim().to_string(),
        exe_url: exe["browser_download_url"].as_str().ok_or("URL unduhan exe kosong")?.to_string(),
        sha_url: sha["browser_download_url"].as_str().ok_or("URL unduhan sha256 kosong")?.to_string(),
        size: exe["size"].as_u64().unwrap_or(0),
    })
}

/// Isi Pacu.exe.sha256: "hex  Pacu.exe" (format sha256sum) atau hex saja.
pub fn parse_sha_file(text: &str) -> Option<[u8; 32]> {
    let hex = text.split_whitespace().next()?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Exe Windows yang utuh: header MZ dan ukuran masuk akal.
pub fn looks_like_exe(data: &[u8]) -> bool {
    data.len() > 512 * 1024 && data.starts_with(b"MZ")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading { got: u64, total: u64 },
    /// Exe baru sudah terpasang; tinggal menutup Pacu dan membuka versi baru.
    Ready(String),
    Failed(String),
}

#[derive(Default)]
pub struct State {
    pub phase: Phase,
    /// Waktu pemeriksaan terakhir (detik sejak UNIX epoch).
    pub last_check: u64,
}

/// Pemeriksa dan pengunduh di thread latar. UI hanya membaca `state`.
#[derive(Clone, Default)]
pub struct Updater {
    pub state: Arc<Mutex<State>>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Updater {
    pub fn busy(&self) -> bool {
        matches!(self.state.lock().phase, Phase::Checking | Phase::Downloading { .. })
    }

    pub fn check(&self, repo: &str) {
        if self.busy() {
            return;
        }
        self.state.lock().phase = Phase::Checking;
        let st = self.state.clone();
        let repo = repo.to_string();
        std::thread::spawn(move || {
            let r = http::get(&format!("https://api.github.com/repos/{repo}/releases/latest"), &mut |_, _| {})
                .and_then(|b| parse_release(&String::from_utf8_lossy(&b)));
            let mut s = st.lock();
            s.last_check = now();
            s.phase = match r {
                Ok(rel) if is_newer(&rel.version, current_version()) => Phase::Available(rel),
                Ok(_) => Phase::UpToDate,
                Err(e) => Phase::Failed(e),
            };
        });
    }

    pub fn install(&self, rel: Release) {
        if self.busy() {
            return;
        }
        self.state.lock().phase = Phase::Downloading { got: 0, total: rel.size };
        let st = self.state.clone();
        std::thread::spawn(move || {
            let r = download_and_swap(&rel, &st);
            st.lock().phase = match r {
                Ok(()) => Phase::Ready(rel.version.clone()),
                Err(e) => Phase::Failed(e),
            };
        });
    }
}

fn download_and_swap(rel: &Release, st: &Arc<Mutex<State>>) -> Result<(), String> {
    let sha_text = http::get(&rel.sha_url, &mut |_, _| {})?;
    let want = parse_sha_file(&String::from_utf8_lossy(&sha_text)).ok_or("File SHA-256 rilis tidak valid")?;
    let data = http::get(&rel.exe_url, &mut |got, total| {
        st.lock().phase = Phase::Downloading { got, total: if total > 0 { total } else { rel.size } };
    })?;
    if sha256(&data) != want {
        return Err("SHA-256 unduhan tidak cocok; pembaruan dibatalkan".into());
    }
    if !looks_like_exe(&data) {
        return Err("File unduhan bukan exe Windows".into());
    }
    swap_exe(&data)
}

/// Mengganti exe yang sedang berjalan. Bila gagal di tengah, exe lama dikembalikan.
pub fn swap_exe(data: &[u8]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Lokasi Pacu tidak diketahui: {e}"))?;
    let old = old_path(&exe);
    let tmp = exe.with_extension("exe.new");
    std::fs::write(&tmp, data).map_err(|e| format!("Gagal menulis {}: {e}", tmp.display()))?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Gagal memindah exe lama: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("Gagal memasang exe baru: {e}"));
    }
    Ok(())
}

fn old_path(exe: &std::path::Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Dipanggil saat Pacu menyala: hapus sisa exe lama dari pembaruan sebelumnya.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(exe.with_extension("exe.new"));
    }
}

/// Diisi UI saat pengguna menekan "Mulai ulang"; dibaca `main` setelah semua thread berhenti.
pub static RESTART: OnceLock<()> = OnceLock::new();

/// Membuka exe (yang sudah versi baru) dan memintanya menunggu proses ini keluar.
pub fn spawn_new_instance() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--wait-pid").arg(std::process::id().to_string()).spawn();
    }
}

/// `--wait-pid <pid>`: tunggu proses Pacu lama keluar (maks. 20 detik) sebelum mengambil kunci
/// satu-instance.
pub fn wait_for_pid(pid: u32) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 20_000);
            let _ = windows::Win32::Foundation::CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}

#[cfg(windows)]
mod http {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    struct H(*mut core::ffi::c_void);
    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn split(url: &str) -> Result<(String, String), String> {
        let rest = url.strip_prefix("https://").ok_or("Hanya URL https yang diizinkan")?;
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        Ok((host.to_string(), format!("/{path}")))
    }

    fn query_num(req: *mut core::ffi::c_void, what: u32) -> Option<u32> {
        let mut v: u32 = 0;
        let mut len = 4u32;
        unsafe {
            WinHttpQueryHeaders(req, what | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut v as *mut u32 as _), &mut len, std::ptr::null_mut()).ok()?;
        }
        Some(v)
    }

    /// GET lewat HTTPS. Pengalihan (GitHub ke penyimpanan rilis) diikuti otomatis oleh WinHTTP.
    pub fn get(url: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<u8>, String> {
        let (host, path) = split(url)?;
        let agent = HSTRING::from(format!("Pacu/{}", super::current_version()));
        unsafe {
            let session = H(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
            if session.0.is_null() {
                return Err(format!("WinHttpOpen gagal: {}", windows::core::Error::from_thread()));
            }
            let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 30_000);
            let conn = H(WinHttpConnect(session.0, &HSTRING::from(host.as_str()), 443, 0));
            if conn.0.is_null() {
                return Err(format!("Tidak bisa terhubung ke {host}: {}", windows::core::Error::from_thread()));
            }
            let req = H(WinHttpOpenRequest(conn.0, w!("GET"), &HSTRING::from(path.as_str()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE));
            if req.0.is_null() {
                return Err(format!("WinHttpOpenRequest gagal: {}", windows::core::Error::from_thread()));
            }
            let headers: Vec<u16> = "Accept: application/vnd.github+json, application/octet-stream\r\n".encode_utf16().collect();
            WinHttpSendRequest(req.0, Some(&headers), None, 0, 0, 0).map_err(|e| format!("Gagal menghubungi {host}: {e}"))?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| format!("Tidak ada jawaban dari {host}: {e}"))?;
            let status = query_num(req.0, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
            if status == 404 {
                return Err("Belum ada rilis di GitHub (404)".into());
            }
            if status != 200 {
                return Err(format!("GitHub menjawab HTTP {status}"));
            }
            let total = query_num(req.0, WINHTTP_QUERY_CONTENT_LENGTH).unwrap_or(0) as u64;
            let mut out = Vec::with_capacity(total as usize);
            loop {
                let mut avail = 0u32;
                WinHttpQueryDataAvailable(req.0, &mut avail).map_err(|e| format!("Unduhan terputus: {e}"))?;
                if avail == 0 {
                    break;
                }
                let start = out.len();
                out.resize(start + avail as usize, 0);
                let mut read = 0u32;
                WinHttpReadData(req.0, out[start..].as_mut_ptr() as _, avail, &mut read).map_err(|e| format!("Unduhan terputus: {e}"))?;
                out.truncate(start + read as usize);
                progress(out.len() as u64, total);
                if out.len() > 64 * 1024 * 1024 {
                    return Err("Unduhan terlalu besar".into());
                }
            }
            if total > 0 && out.len() as u64 != total {
                return Err(format!("Unduhan tidak lengkap ({} dari {total} byte)", out.len()));
            }
            Ok(out)
        }
    }
}

#[cfg(not(windows))]
mod http {
    pub fn get(_url: &str, _progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<u8>, String> {
        Err("Pembaruan hanya tersedia di Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.4.1"), Some((0, 4, 1)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("0.5.0-beta.1"), Some((0, 5, 0)));
        assert_eq!(parse_version("x"), None);
        assert!(is_newer("v0.4.0", "0.3.9"));
        assert!(is_newer("0.10.0", "0.9.5"));
        assert!(!is_newer("v0.3.0", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
    }

    #[test]
    fn release_json() {
        let j = r#"{"tag_name":"v0.4.0","body":"Catatan","assets":[
            {"name":"Pacu.zip","browser_download_url":"https://x/z","size":5},
            {"name":"Pacu.exe","browser_download_url":"https://github.com/a/b/releases/download/v0.4.0/Pacu.exe","size":7000000},
            {"name":"Pacu.exe.sha256","browser_download_url":"https://github.com/a/b/releases/download/v0.4.0/Pacu.exe.sha256","size":80}]}"#;
        let r = parse_release(j).unwrap();
        assert_eq!(r.version, "0.4.0");
        assert_eq!(r.size, 7_000_000);
        assert!(r.exe_url.ends_with("/Pacu.exe"));
        assert!(r.sha_url.ends_with(".sha256"));
        assert!(parse_release(r#"{"tag_name":"v1","assets":[]}"#).is_err());
    }

    #[test]
    fn sha_file() {
        let d = sha256(b"abc");
        let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(parse_sha_file(&format!("{hex}  Pacu.exe\n")), Some(d));
        assert_eq!(parse_sha_file(&hex.to_uppercase()), Some(d));
        assert_eq!(parse_sha_file("zz"), None);
    }
}
