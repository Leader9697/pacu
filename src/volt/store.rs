//! Pengaturan, sesi pencarian, dan profil hasil, disimpan sebagai JSON di %APPDATA%\PacuVolt.

use crate::volt::gpu::Curve;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;

pub fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let d = base.join("PacuVolt");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub(crate) fn load<T: DeserializeOwned>(name: &str) -> Option<T> {
    let s = std::fs::read_to_string(dir().join(name)).ok()?;
    serde_json::from_str(&s).ok()
}

/// Menulis lewat file sementara lalu rename, supaya file tidak rusak bila PC mati mendadak.
pub(crate) fn save<T: Serialize>(name: &str, v: &T) {
    let p = dir().join(name);
    let tmp = p.with_extension("tmp");
    if let Ok(s) = serde_json::to_string_pretty(v) {
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

fn remove(name: &str) {
    let _ = std::fs::remove_file(dir().join(name));
}

pub fn append_log(line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir().join("pacuvolt.log")) {
        let _ = writeln!(f, "{line}");
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Aplikasi beban 3D. Kosong berarti pengguna menjalankannya sendiri.
    pub load_exe: String,
    pub load_args: String,
    /// Frekuensi target dalam MHz. None = samakan dengan clock bawaan saat dibebani.
    pub target_mhz: Option<u32>,
    pub step_mv: u32,
    pub step_secs: u32,
    pub validate_secs: u32,
    pub margin_mv: u32,
    pub min_mv: u32,
    pub temp_limit_c: u32,
    pub use_checker: bool,
    pub baseline_secs: u32,
    /// Offset positif maksimum di titik uji. Mencegah uji dengan offset ekstrem yang bisa
    /// membuat PC hang total alih-alih driver reset biasa.
    pub max_offset_mhz: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            load_exe: String::new(),
            load_args: String::new(),
            target_mhz: None,
            step_mv: 25,
            step_secs: 180,
            validate_secs: 900,
            margin_mv: 12,
            min_mv: 700,
            temp_limit_c: 87,
            use_checker: true,
            baseline_secs: 60,
            max_offset_mhz: 350,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        load("settings.json").unwrap_or_default()
    }
    pub fn save(&self) {
        save("settings.json", self)
    }
}

/// Ringkasan sensor selama satu uji.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Measure {
    pub clock_mhz: f32,
    pub volt_mv: f32,
    pub power_w: f32,
    pub temp_c: f32,
    pub util_pct: f32,
    pub samples: u32,
    pub power_limited_pct: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestKind {
    Search,
    Validate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub kind: TestKind,
    /// Posisi di `Session::curve.points`.
    pub pos: usize,
    pub volt_mv: f32,
    pub target_mhz: u32,
    pub passed: bool,
    pub reason: String,
    pub secs: u32,
    pub measure: Option<Measure>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Planned {
    pub kind: TestKind,
    pub pos: usize,
    pub volt_mv: f32,
    pub target_mhz: u32,
}

/// Seluruh keadaan pencarian. Disimpan setelah setiap langkah supaya bisa dilanjutkan setelah
/// PC crash atau restart.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub gpu_name: String,
    pub started: u64,
    pub settings: Settings,
    /// Kurva bawaan (offset 0) saat sesi dimulai.
    pub curve: Curve,
    pub baseline: Option<Measure>,
    pub target_khz: u32,
    pub results: Vec<TestResult>,
    /// Uji yang sedang berjalan. Bila masih terisi saat aplikasi dibuka, uji itu berakhir dengan
    /// crash atau restart.
    pub testing: Option<Planned>,
    pub finished: bool,
}

impl Session {
    pub fn load() -> Option<Self> {
        load("session.json")
    }
    pub fn save(&self) {
        save("session.json", self)
    }
    pub fn discard() {
        remove("session.json")
    }
}

/// Profil undervolt yang sudah lolos validasi.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Profile {
    pub gpu_name: String,
    pub created: u64,
    pub volt_mv: f32,
    pub target_mhz: u32,
    /// (indeks NVAPI, offset kHz) untuk setiap titik grafis.
    pub deltas: Vec<(usize, i32)>,
    pub baseline: Option<Measure>,
    pub result: Option<Measure>,
    /// Pasang otomatis saat login.
    pub apply_on_login: bool,
}

impl Profile {
    pub fn load() -> Option<Self> {
        load("profile.json")
    }

    /// Profil yang dibuat untuk GPU ini (profil dari GPU lain, misalnya setelah ganti kartu, diabaikan).
    pub fn load_for(gpu_name: &str) -> Option<Self> {
        Self::load().filter(|p| p.gpu_name.is_empty() || p.gpu_name == gpu_name)
    }
    pub fn save(&self) {
        save("profile.json", self)
    }
    pub fn remove() {
        remove("profile.json")
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Jam lokal HH:MM:SS untuk log.
pub fn stamp() -> String {
    #[cfg(windows)]
    {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
    }
    #[cfg(not(windows))]
    {
        let s = now_secs();
        format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
    }
}
