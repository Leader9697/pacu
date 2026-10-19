//! Optimasi Windows untuk gaming, sekali klik. Setiap nilai lama dicadangkan supaya bisa
//! dikembalikan persis seperti semula.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Root {
    Hklm,
    Hkcu,
}

#[derive(Clone, Copy, Debug)]
pub enum Val {
    Dword(u32),
    Str(&'static str),
}

pub struct RegItem {
    pub root: Root,
    pub path: &'static str,
    pub name: &'static str,
    pub value: Val,
}

const fn dw(root: Root, path: &'static str, name: &'static str, v: u32) -> RegItem {
    RegItem { root, path, name, value: Val::Dword(v) }
}

const fn st(root: Root, path: &'static str, name: &'static str, v: &'static str) -> RegItem {
    RegItem { root, path, name, value: Val::Str(v) }
}

pub enum Kind {
    Registry(&'static [RegItem]),
    /// "Optimizations for windowed games" disimpan dalam satu string gabungan.
    WindowedOptimizations,
    /// Akselerasi mouse: registry plus diterapkan langsung tanpa logout.
    MouseAcceleration,
    /// Jenis startup layanan Windows (nilai `Start`: 3 manual, 4 nonaktif). Layanan yang tidak ada
    /// di PC ini dilewati.
    Services(&'static [(&'static str, u32)]),
    /// Profil driver NVIDIA untuk exe ini (lihat `nvprofile`).
    NvidiaProfile(&'static [&'static str]),
}

/// Halaman tempat tweak ditampilkan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Optimasi Windows (game).
    Game,
    /// RAM Ekstrem.
    Ram,
    /// Optimasi Windows, bagian berisiko (mengurangi keamanan). Tidak ikut "Terapkan semua".
    Risky,
}

pub struct Tweak {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub desc: &'static str,
    pub reboot: bool,
    pub kind: Kind,
}

use Root::*;

const MMCSS: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile";
const MMCSS_GAMES: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games";
const MOUSE: &str = "Control Panel\\Mouse";

pub const TWEAKS: &[Tweak] = &[
    Tweak {
        id: "game_dvr",
        group: Group::Game,
        title: "Matikan perekaman latar Xbox Game Bar",
        desc: "Game DVR terus merekam di latar dan memakan GPU serta disk.",
        reboot: false,
        kind: Kind::Registry(&[
            dw(Hkcu, "System\\GameConfigStore", "GameDVR_Enabled", 0),
            dw(Hkcu, "Software\\Microsoft\\Windows\\CurrentVersion\\GameDVR", "AppCaptureEnabled", 0),
        ]),
    },
    Tweak {
        id: "win_game_mode",
        group: Group::Game,
        title: "Nyalakan Game Mode Windows",
        desc: "Windows menunda update dan tugas latar saat game berjalan.",
        reboot: false,
        kind: Kind::Registry(&[
            dw(Hkcu, "Software\\Microsoft\\GameBar", "AutoGameModeEnabled", 1),
            dw(Hkcu, "Software\\Microsoft\\GameBar", "AllowAutoGameMode", 1),
        ]),
    },
    Tweak {
        id: "hags",
        group: Group::Game,
        title: "Nyalakan Hardware-accelerated GPU scheduling",
        desc: "GPU mengatur antreannya sendiri; latensi lebih rendah di GPU NVIDIA/AMD modern.",
        reboot: true,
        kind: Kind::Registry(&[dw(Hklm, "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers", "HwSchMode", 2)]),
    },
    Tweak {
        id: "windowed_opt",
        group: Group::Game,
        title: "Nyalakan Optimizations for windowed games",
        desc: "Game borderless/windowed (seperti Deadlock) memakai flip model: latensi lebih rendah, mendukung VRR.",
        reboot: false,
        kind: Kind::WindowedOptimizations,
    },
    Tweak {
        id: "global_timer",
        group: Group::Game,
        title: "Izinkan resolusi timer global",
        desc: "Di Windows 11, supaya pengaturan timer 0,5 ms dari Pacu berlaku untuk game.",
        reboot: true,
        kind: Kind::Registry(&[dw(
            Hklm,
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel",
            "GlobalTimerResolutionRequests",
            1,
        )]),
    },
    Tweak {
        id: "mmcss",
        group: Group::Game,
        title: "Prioritaskan game di penjadwal multimedia",
        desc: "Mengurangi jatah CPU untuk tugas latar dan menaikkan prioritas kelas Games.",
        reboot: true,
        kind: Kind::Registry(&[
            dw(Hklm, MMCSS, "SystemResponsiveness", 10),
            dw(Hklm, MMCSS, "NetworkThrottlingIndex", 0xFFFF_FFFF),
            dw(Hklm, MMCSS_GAMES, "GPU Priority", 8),
            dw(Hklm, MMCSS_GAMES, "Priority", 6),
            st(Hklm, MMCSS_GAMES, "Scheduling Category", "High"),
            st(Hklm, MMCSS_GAMES, "SFIO Priority", "High"),
        ]),
    },
    Tweak {
        id: "power_throttling",
        group: Group::Game,
        title: "Matikan power throttling Windows",
        desc: "Windows tidak lagi memperlambat aplikasi yang dianggap latar.",
        reboot: true,
        kind: Kind::Registry(&[dw(
            Hklm,
            "SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling",
            "PowerThrottlingOff",
            1,
        )]),
    },
    Tweak {
        id: "nv_profile_deadlock",
        group: Group::Game,
        title: "Profil driver NVIDIA untuk Deadlock",
        desc: "Seperti Program Settings di NVIDIA Control Panel: Power management Prefer maximum performance, Low Latency Mode On, Threaded optimization On, Shader Cache On. Hanya untuk GPU NVIDIA.",
        reboot: false,
        kind: Kind::NvidiaProfile(&["project8.exe"]),
    },
    Tweak {
        id: "memory_integrity",
        group: Group::Risky,
        title: "Matikan Memory Integrity (HVCI)",
        desc: "Bisa menambah beberapa persen FPS, tetapi melemahkan perlindungan Windows terhadap driver dan malware tingkat kernel. Beberapa anti-cheat (misalnya Vanguard, FACEIT) bisa meminta fitur ini menyala. Bila dikunci UEFI atau kebijakan perusahaan, pengaturan ini tidak berpengaruh.",
        reboot: true,
        kind: Kind::Registry(&[dw(
            Hklm,
            "SYSTEM\\CurrentControlSet\\Control\\DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity",
            "Enabled",
            0,
        )]),
    },
    Tweak {
        id: "mouse_accel",
        group: Group::Game,
        title: "Matikan akselerasi mouse",
        desc: "\"Enhance pointer precision\" mati supaya aim konsisten.",
        reboot: false,
        kind: Kind::MouseAcceleration,
    },
];

pub const RAM_TWEAKS_NOTE: &str = "Layanan yang dimatikan baru berhenti setelah restart. Semua bisa dikembalikan dari halaman ini.";

/// Tweak halaman RAM Ekstrem: layanan dan aplikasi latar yang memakan RAM tanpa membantu game.
/// Fitur keamanan (Defender, Firewall, Windows Update, VBS) sengaja tidak disentuh.
pub const RAM_TWEAKS: &[Tweak] = &[
    Tweak {
        id: "ram_sysmain",
        group: Group::Ram,
        title: "Matikan SysMain (Superfetch)",
        desc: "Berhenti mengisi RAM dengan aplikasi yang diperkirakan akan dibuka. Membuka aplikasi pertama kali bisa sedikit lebih lambat, terutama di HDD.",
        reboot: true,
        kind: Kind::Services(&[("SysMain", 4)]),
    },
    Tweak {
        id: "ram_search",
        group: Group::Ram,
        title: "Matikan pengindeksan Windows Search",
        desc: "SearchIndexer tidak lagi berjalan di latar. Cari file dari Start dan Explorer tetap bisa, tetapi lebih lambat.",
        reboot: true,
        kind: Kind::Services(&[("WSearch", 4)]),
    },
    Tweak {
        id: "ram_telemetry",
        group: Group::Ram,
        title: "Matikan layanan telemetri",
        desc: "Connected User Experiences and Telemetry (DiagTrack) dan WAP Push berhenti mengirim data diagnostik di latar.",
        reboot: true,
        kind: Kind::Services(&[("DiagTrack", 4), ("dmwappushservice", 4)]),
    },
    Tweak {
        id: "ram_minor_services",
        group: Group::Ram,
        title: "Matikan layanan yang jarang dipakai",
        desc: "Peta offline, lokasi (geolocation), mode demo toko, Program Compatibility Assistant, dan pelaporan error. Aplikasi cuaca dan Find My Device tidak bisa memakai lokasi.",
        reboot: true,
        kind: Kind::Services(&[("MapsBroker", 4), ("lfsvc", 4), ("RetailDemo", 4), ("PcaSvc", 4), ("WerSvc", 4)]),
    },
    Tweak {
        id: "ram_spooler",
        group: Group::Ram,
        title: "Matikan Print Spooler",
        desc: "Hanya bila kamu tidak memakai printer. Mencetak (termasuk ke PDF lewat Microsoft Print to PDF) tidak bisa sampai dikembalikan.",
        reboot: true,
        kind: Kind::Services(&[("Spooler", 4)]),
    },
    Tweak {
        id: "ram_widgets_copilot",
        group: Group::Ram,
        title: "Matikan Widgets dan Copilot",
        desc: "Widgets (berita dan cuaca di taskbar) serta Copilot tidak lagi berjalan di latar.",
        reboot: true,
        kind: Kind::Registry(&[
            dw(Hklm, "SOFTWARE\\Policies\\Microsoft\\Dsh", "AllowNewsAndInterests", 0),
            dw(Hkcu, "Software\\Policies\\Microsoft\\Windows\\WindowsCopilot", "TurnOffWindowsCopilot", 1),
            dw(Hklm, "SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsCopilot", "TurnOffWindowsCopilot", 1),
        ]),
    },
    Tweak {
        id: "ram_edge",
        group: Group::Ram,
        title: "Larang Microsoft Edge berjalan di latar",
        desc: "Startup boost dan mode latar Edge dimatikan, jadi Edge tidak memakan RAM saat ditutup. Edge akan menampilkan \"dikelola oleh organisasi\"; itu karena pengaturan ini.",
        reboot: false,
        kind: Kind::Registry(&[
            dw(Hklm, "SOFTWARE\\Policies\\Microsoft\\Edge", "StartupBoostEnabled", 0),
            dw(Hklm, "SOFTWARE\\Policies\\Microsoft\\Edge", "BackgroundModeEnabled", 0),
        ]),
    },
    Tweak {
        id: "ram_bg_apps",
        group: Group::Ram,
        title: "Larang aplikasi Microsoft Store berjalan di latar",
        desc: "Aplikasi dari Store (Phone Link, Mail, Xbox, dan lainnya) hanya berjalan saat dibuka. Notifikasinya tidak muncul saat aplikasinya tertutup.",
        reboot: true,
        kind: Kind::Registry(&[dw(Hklm, "SOFTWARE\\Policies\\Microsoft\\Windows\\AppPrivacy", "LetAppsRunInBackground", 2)]),
    },
];

/// Semua tweak, berurutan: halaman Optimasi Windows lalu halaman RAM Ekstrem.
pub fn all() -> impl Iterator<Item = &'static Tweak> {
    TWEAKS.iter().chain(RAM_TWEAKS.iter())
}

const SERVICES: &str = "SYSTEM\\CurrentControlSet\\Services";

fn service_path(name: &str) -> String {
    format!("{SERVICES}\\{name}")
}

const MOUSE_ITEMS: &[RegItem] = &[
    st(Hkcu, MOUSE, "MouseSpeed", "0"),
    st(Hkcu, MOUSE, "MouseThreshold1", "0"),
    st(Hkcu, MOUSE, "MouseThreshold2", "0"),
];

const WINDOWED_PATH: &str = "Software\\Microsoft\\DirectX\\UserGpuPreferences";
const WINDOWED_NAME: &str = "DirectXUserGlobalSettings";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Saved {
    Dword(u32),
    Str(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedValue {
    pub root: Root,
    pub path: String,
    pub name: String,
    /// `None` = nilai belum ada sebelumnya (akan dihapus saat dikembalikan).
    pub prev: Option<Saved>,
}

/// Cadangan nilai lama per tweak, disimpan di %APPDATA%\Pacu\tweaks-backup.json.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Backup(pub HashMap<String, Vec<SavedValue>>);

impl Backup {
    fn path() -> std::path::PathBuf {
        crate::config::data_dir().join("tweaks-backup.json")
    }

    pub fn load() -> Backup {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Applied,
    NotApplied,
    Unknown,
}

fn items_of(t: &Tweak) -> &'static [RegItem] {
    match t.kind {
        Kind::Registry(items) => items,
        Kind::MouseAcceleration => MOUSE_ITEMS,
        Kind::WindowedOptimizations | Kind::Services(_) | Kind::NvidiaProfile(_) => &[],
    }
}

fn matches(item: &RegItem, cur: &Option<Saved>) -> bool {
    match (item.value, cur) {
        (Val::Dword(v), Some(Saved::Dword(c))) => v == *c,
        (Val::Str(v), Some(Saved::Str(c))) => v == c,
        _ => false,
    }
}

fn windowed_enabled(s: &str) -> bool {
    s.split(';').any(|kv| kv.trim().eq_ignore_ascii_case("SwapEffectUpgradeEnable=1"))
}

fn windowed_set(s: &str) -> String {
    let mut parts: Vec<String> = s
        .split(';')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && !p.to_lowercase().starts_with("swapeffectupgradeenable="))
        .collect();
    parts.insert(0, "SwapEffectUpgradeEnable=1".into());
    parts.join(";") + ";"
}

pub fn status(t: &Tweak) -> State {
    match t.kind {
        Kind::WindowedOptimizations => match reg::read(Hkcu, WINDOWED_PATH, WINDOWED_NAME) {
            Ok(Some(Saved::Str(s))) if windowed_enabled(&s) => State::Applied,
            Ok(_) => State::NotApplied,
            Err(_) => State::Unknown,
        },
        Kind::NvidiaProfile(exes) => match crate::nvprofile::status(exes) {
            Ok(true) => State::Applied,
            Ok(false) => State::NotApplied,
            Err(_) => State::Unknown,
        },
        Kind::Services(list) => {
            let mut all = true;
            for (name, start) in list {
                match reg::read(Hklm, &service_path(name), "Start") {
                    Ok(Some(Saved::Dword(v))) => all &= v == *start,
                    // Layanan tidak ada di PC ini.
                    Ok(_) => {}
                    Err(_) => return State::Unknown,
                }
            }
            if all { State::Applied } else { State::NotApplied }
        }
        _ => {
            let mut all = true;
            for item in items_of(t) {
                match reg::read(item.root, item.path, item.name) {
                    Ok(cur) => all &= matches(item, &cur),
                    Err(_) => return State::Unknown,
                }
            }
            if all { State::Applied } else { State::NotApplied }
        }
    }
}

pub fn apply(t: &Tweak, backup: &mut Backup) -> Result<(), String> {
    let first_time = !backup.0.contains_key(t.id);
    let mut saved = Vec::new();
    match t.kind {
        Kind::WindowedOptimizations => {
            let cur = reg::read(Hkcu, WINDOWED_PATH, WINDOWED_NAME)?;
            let text = match &cur {
                Some(Saved::Str(s)) => s.clone(),
                _ => String::new(),
            };
            saved.push(SavedValue { root: Hkcu, path: WINDOWED_PATH.into(), name: WINDOWED_NAME.into(), prev: cur });
            reg::write(Hkcu, WINDOWED_PATH, WINDOWED_NAME, &Saved::Str(windowed_set(&text)))?;
        }
        Kind::NvidiaProfile(exes) => crate::nvprofile::apply(exes)?,
        Kind::Services(list) => {
            for (name, start) in list {
                let path = service_path(name);
                let Some(cur) = reg::read(Hklm, &path, "Start")? else { continue };
                saved.push(SavedValue { root: Hklm, path: path.clone(), name: "Start".into(), prev: Some(cur) });
                reg::write(Hklm, &path, "Start", &Saved::Dword(*start))?;
            }
        }
        _ => {
            for item in items_of(t) {
                let cur = reg::read(item.root, item.path, item.name)?;
                saved.push(SavedValue { root: item.root, path: item.path.into(), name: item.name.into(), prev: cur });
                let v = match item.value {
                    Val::Dword(d) => Saved::Dword(d),
                    Val::Str(s) => Saved::Str(s.into()),
                };
                reg::write(item.root, item.path, item.name, &v)?;
            }
            if matches!(t.kind, Kind::MouseAcceleration) {
                reg::apply_mouse(false);
            }
        }
    }
    if first_time {
        backup.0.insert(t.id.to_string(), saved);
        backup.save();
    }
    Ok(())
}

pub fn revert(t: &Tweak, backup: &mut Backup) -> Result<(), String> {
    let Some(saved) = backup.0.remove(t.id) else {
        return Err("Tidak ada cadangan; pengaturan ini belum pernah diubah oleh Pacu.".into());
    };
    if let Kind::NvidiaProfile(exes) = t.kind {
        crate::nvprofile::revert(exes)?;
    }
    for s in &saved {
        match &s.prev {
            Some(v) => reg::write(s.root, &s.path, &s.name, v)?,
            None => reg::delete(s.root, &s.path, &s.name)?,
        }
    }
    if matches!(t.kind, Kind::MouseAcceleration) {
        let on = matches!(reg::read(Hkcu, MOUSE, "MouseSpeed"), Ok(Some(Saved::Str(ref v))) if v != "0");
        reg::apply_mouse(on);
    }
    backup.save();
    Ok(())
}

pub fn has_backup(t: &Tweak, backup: &Backup) -> bool {
    backup.0.contains_key(t.id)
}

#[cfg(windows)]
pub(crate) mod reg {
    use super::{Root, Saved};
    use std::ffi::c_void;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(
            key: isize,
            sub: *const u16,
            res: u32,
            class: *const u16,
            opts: u32,
            sam: u32,
            attr: *const c_void,
            out: *mut isize,
            disp: *mut u32,
        ) -> i32;
        fn RegOpenKeyExW(key: isize, sub: *const u16, opts: u32, sam: u32, out: *mut isize) -> i32;
        fn RegQueryValueExW(key: isize, name: *const u16, res: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
        fn RegSetValueExW(key: isize, name: *const u16, res: u32, ty: u32, data: *const u8, len: u32) -> i32;
        fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
        fn RegCloseKey(key: isize) -> i32;
        fn RegEnumValueW(key: isize, i: u32, name: *mut u16, name_len: *mut u32, res: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
        fn RegEnumKeyExW(key: isize, i: u32, name: *mut u16, name_len: *mut u32, res: *mut u32, class: *mut u16, class_len: *mut u32, time: *mut u64) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, flags: u32) -> i32;
    }

    const KEY_READ: u32 = 0x20019;
    const KEY_WRITE: u32 = 0x20006;
    const KEY_WOW64_64KEY: u32 = 0x0100;
    const REG_SZ: u32 = 1;
    const REG_DWORD: u32 = 4;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn hkey(root: Root) -> isize {
        match root {
            Root::Hklm => 0x8000_0002u32 as i32 as isize,
            Root::Hkcu => 0x8000_0001u32 as i32 as isize,
        }
    }

    pub fn read(root: Root, path: &str, name: &str) -> Result<Option<Saved>, String> {
        unsafe {
            let mut k: isize = 0;
            let r = RegOpenKeyExW(hkey(root), wide(path).as_ptr(), 0, KEY_READ | KEY_WOW64_64KEY, &mut k);
            if r == 2 {
                return Ok(None);
            }
            if r != 0 {
                return Err(format!("Gagal membuka registry {path} (kode {r})"));
            }
            let mut ty = 0u32;
            let mut buf = vec![0u8; 4096];
            let mut len = buf.len() as u32;
            let r = RegQueryValueExW(k, wide(name).as_ptr(), std::ptr::null_mut(), &mut ty, buf.as_mut_ptr(), &mut len);
            RegCloseKey(k);
            if r == 2 {
                return Ok(None);
            }
            if r != 0 {
                return Err(format!("Gagal membaca {name} (kode {r})"));
            }
            Ok(match ty {
                REG_DWORD if len >= 4 => Some(Saved::Dword(u32::from_le_bytes(buf[..4].try_into().unwrap()))),
                REG_SZ | 2 => {
                    let w: Vec<u16> = buf[..len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    Some(Saved::Str(String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()))
                }
                _ => None,
            })
        }
    }

    pub fn write(root: Root, path: &str, name: &str, v: &Saved) -> Result<(), String> {
        unsafe {
            let mut k: isize = 0;
            let r = RegCreateKeyExW(
                hkey(root),
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                0,
                KEY_WRITE | KEY_WOW64_64KEY,
                std::ptr::null(),
                &mut k,
                std::ptr::null_mut(),
            );
            if r != 0 {
                return Err(format!("Gagal membuka registry {path} untuk ditulis (kode {r})"));
            }
            let r = match v {
                Saved::Dword(d) => RegSetValueExW(k, wide(name).as_ptr(), 0, REG_DWORD, d.to_le_bytes().as_ptr(), 4),
                Saved::Str(s) => {
                    let w = wide(s);
                    RegSetValueExW(k, wide(name).as_ptr(), 0, REG_SZ, w.as_ptr() as *const u8, (w.len() * 2) as u32)
                }
            };
            RegCloseKey(k);
            if r != 0 {
                return Err(format!("Gagal menulis {name} (kode {r})"));
            }
            Ok(())
        }
    }

    pub fn delete(root: Root, path: &str, name: &str) -> Result<(), String> {
        unsafe {
            let mut k: isize = 0;
            let r = RegOpenKeyExW(hkey(root), wide(path).as_ptr(), 0, KEY_WRITE | KEY_WOW64_64KEY, &mut k);
            if r == 2 {
                return Ok(());
            }
            if r != 0 {
                return Err(format!("Gagal membuka registry {path} (kode {r})"));
            }
            let r = RegDeleteValueW(k, wide(name).as_ptr());
            RegCloseKey(k);
            if r != 0 && r != 2 {
                return Err(format!("Gagal menghapus {name} (kode {r})"));
            }
            Ok(())
        }
    }

    fn open(root: Root, path: &str, sam: u32) -> Option<isize> {
        let mut k: isize = 0;
        (unsafe { RegOpenKeyExW(hkey(root), wide(path).as_ptr(), 0, sam | KEY_WOW64_64KEY, &mut k) } == 0).then_some(k)
    }

    fn utf16(buf: &[u8]) -> String {
        let w: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()
    }

    /// Nilai string (REG_SZ / REG_EXPAND_SZ) di sebuah key.
    pub fn values(root: Root, path: &str) -> Vec<(String, String)> {
        let Some(k) = open(root, path, KEY_READ) else { return Vec::new() };
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = vec![0u16; 512];
            let mut nlen = name.len() as u32;
            let mut ty = 0u32;
            let mut data = vec![0u8; 8192];
            let mut dlen = data.len() as u32;
            let r = unsafe { RegEnumValueW(k, i, name.as_mut_ptr(), &mut nlen, std::ptr::null_mut(), &mut ty, data.as_mut_ptr(), &mut dlen) };
            if r == 259 {
                break;
            }
            if r != 0 {
                continue;
            }
            if ty == REG_SZ || ty == 2 {
                out.push((String::from_utf16_lossy(&name[..nlen as usize]), utf16(&data[..dlen as usize])));
            }
        }
        unsafe { RegCloseKey(k) };
        out
    }

    pub fn subkeys(root: Root, path: &str) -> Vec<String> {
        let Some(k) = open(root, path, KEY_READ) else { return Vec::new() };
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = vec![0u16; 512];
            let mut nlen = name.len() as u32;
            let r = unsafe { RegEnumKeyExW(k, i, name.as_mut_ptr(), &mut nlen, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
            if r != 0 {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..nlen as usize]));
        }
        unsafe { RegCloseKey(k) };
        out
    }

    pub fn read_bin(root: Root, path: &str, name: &str) -> Option<Vec<u8>> {
        let k = open(root, path, KEY_READ)?;
        let mut buf = vec![0u8; 256];
        let mut len = buf.len() as u32;
        let mut ty = 0u32;
        let r = unsafe { RegQueryValueExW(k, wide(name).as_ptr(), std::ptr::null_mut(), &mut ty, buf.as_mut_ptr(), &mut len) };
        unsafe { RegCloseKey(k) };
        (r == 0).then(|| buf[..len as usize].to_vec())
    }

    pub fn write_bin(root: Root, path: &str, name: &str, data: &[u8]) -> Result<(), String> {
        unsafe {
            let mut k: isize = 0;
            let r = RegCreateKeyExW(hkey(root), wide(path).as_ptr(), 0, std::ptr::null(), 0, KEY_WRITE | KEY_WOW64_64KEY, std::ptr::null(), &mut k, std::ptr::null_mut());
            if r != 0 {
                return Err(format!("Gagal membuka registry {path} untuk ditulis (kode {r})"));
            }
            let r = RegSetValueExW(k, wide(name).as_ptr(), 0, 3, data.as_ptr(), data.len() as u32);
            RegCloseKey(k);
            if r != 0 {
                return Err(format!("Gagal menulis {name} (kode {r})"));
            }
            Ok(())
        }
    }

    /// Menerapkan akselerasi mouse langsung (SPI_SETMOUSE) tanpa perlu logout.
    pub fn apply_mouse(enabled: bool) {
        let mut v: [i32; 3] = if enabled { [6, 10, 1] } else { [0, 0, 0] };
        unsafe {
            SystemParametersInfoW(4, 0, v.as_mut_ptr() as *mut c_void, 0x3);
        }
    }
}

#[cfg(not(windows))]
pub(crate) mod reg {
    use super::{Root, Saved};

    pub fn read(_: Root, _: &str, _: &str) -> Result<Option<Saved>, String> {
        Err("Hanya tersedia di Windows".into())
    }

    pub fn write(_: Root, _: &str, _: &str, _: &Saved) -> Result<(), String> {
        Err("Hanya tersedia di Windows".into())
    }

    pub fn delete(_: Root, _: &str, _: &str) -> Result<(), String> {
        Err("Hanya tersedia di Windows".into())
    }

    pub fn apply_mouse(_: bool) {}
    pub fn values(_: Root, _: &str) -> Vec<(String, String)> {
        Vec::new()
    }
    pub fn subkeys(_: Root, _: &str) -> Vec<String> {
        Vec::new()
    }
    pub fn read_bin(_: Root, _: &str, _: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn write_bin(_: Root, _: &str, _: &str, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_string_merge() {
        assert!(!windowed_enabled(""));
        assert_eq!(windowed_set(""), "SwapEffectUpgradeEnable=1;");
        let s = windowed_set("VRROptimizeEnable=0;SwapEffectUpgradeEnable=0;");
        assert_eq!(s, "SwapEffectUpgradeEnable=1;VRROptimizeEnable=0;");
        assert!(windowed_enabled(&s));
    }
}
