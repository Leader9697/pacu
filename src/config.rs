//! Konfigurasi Pacu, disimpan sebagai JSON di %APPDATA%\Pacu\config.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl Priority {
    pub const ALL: [Priority; 6] = [
        Priority::Idle,
        Priority::BelowNormal,
        Priority::Normal,
        Priority::AboveNormal,
        Priority::High,
        Priority::Realtime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Priority::Idle => "Idle",
            Priority::BelowNormal => "Di bawah normal",
            Priority::Normal => "Normal",
            Priority::AboveNormal => "Di atas normal",
            Priority::High => "Tinggi",
            Priority::Realtime => "Realtime",
        }
    }

    /// Prioritas dasar Windows untuk tiap kelas (dipakai saat membaca daftar proses).
    pub fn from_base_priority(base: i32) -> Priority {
        match base {
            i32::MIN..=4 => Priority::Idle,
            5..=6 => Priority::BelowNormal,
            7..=8 => Priority::Normal,
            9..=10 => Priority::AboveNormal,
            11..=15 => Priority::High,
            _ => Priority::Realtime,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoPriority {
    VeryLow,
    Low,
    Normal,
    High,
}

impl IoPriority {
    pub const ALL: [IoPriority; 4] =
        [IoPriority::VeryLow, IoPriority::Low, IoPriority::Normal, IoPriority::High];

    pub fn label(self) -> &'static str {
        match self {
            IoPriority::VeryLow => "Sangat rendah",
            IoPriority::Low => "Rendah",
            IoPriority::Normal => "Normal",
            IoPriority::High => "Tinggi",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuPriority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl GpuPriority {
    pub const ALL: [GpuPriority; 6] = [
        GpuPriority::Idle,
        GpuPriority::BelowNormal,
        GpuPriority::Normal,
        GpuPriority::AboveNormal,
        GpuPriority::High,
        GpuPriority::Realtime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GpuPriority::Idle => "Idle",
            GpuPriority::BelowNormal => "Di bawah normal",
            GpuPriority::Normal => "Normal",
            GpuPriority::AboveNormal => "Di atas normal",
            GpuPriority::High => "Tinggi",
            GpuPriority::Realtime => "Realtime",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BalancerConfig {
    pub enabled: bool,
    /// Ambang CPU total sistem (%). `None` = otomatis menurut jumlah CPU.
    pub total_threshold: Option<f32>,
    /// Ambang CPU per proses (% dari seluruh CPU). `None` = otomatis.
    pub per_process_threshold: Option<f32>,
    /// Proses dipulihkan bila CPU-nya di bawah nilai ini (%). `None` = otomatis.
    pub restore_threshold: Option<f32>,
    /// Lama melewati ambang sebelum diredam (ms). `None` = otomatis.
    pub time_over_ms: Option<u64>,
    /// Lama minimum diredam (ms). `None` = otomatis.
    pub min_restraint_ms: Option<u64>,
    /// Lama maksimum diredam (ms), 0 = tanpa batas.
    pub max_restraint_ms: u64,
    pub only_normal_priority: bool,
    pub lower_to_idle: bool,
    pub lower_io_priority: bool,
    pub use_efficiency_mode: bool,
    /// Di CPU hybrid, pindahkan proses yang diredam ke E-core.
    pub move_to_ecores: bool,
    pub exclude_services: bool,
    pub exclude_foreground: bool,
    pub exclude_foreground_children: bool,
    pub pause_when_idle: bool,
    pub idle_ms: u64,
    pub exclusions: Vec<String>,
}

impl Default for BalancerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            total_threshold: None,
            per_process_threshold: None,
            restore_threshold: None,
            time_over_ms: None,
            min_restraint_ms: None,
            max_restraint_ms: 0,
            only_normal_priority: true,
            lower_to_idle: false,
            lower_io_priority: true,
            use_efficiency_mode: false,
            move_to_ecores: true,
            exclude_services: true,
            exclude_foreground: true,
            exclude_foreground_children: true,
            pause_when_idle: false,
            idle_ms: 30_000,
            exclusions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PowerPlanChoice {
    /// Power plan "Pacu Performa" buatan Pacu (dibuat otomatis bila belum ada).
    Auto,
    /// Jangan ubah power plan.
    Keep,
    /// GUID power plan tertentu.
    Plan(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameConfig {
    /// Mode Game otomatis menyala saat game terdeteksi.
    pub auto: bool,
    /// Daftar game (nama exe atau pola, misalnya `valorant*.exe`).
    pub games: Vec<String>,
    /// Deteksi game dari folder Steam, Epic, Riot, Xbox, dan lainnya.
    pub detect_store_folders: bool,
    /// Deteksi aplikasi layar penuh di depan sebagai game.
    pub detect_fullscreen: bool,
    pub fullscreen_exclusions: Vec<String>,
    pub game_priority: Option<Priority>,
    pub game_io_priority: Option<IoPriority>,
    pub game_gpu_priority: Option<GpuPriority>,
    pub power_plan: PowerPlanChoice,
    pub timer_resolution: bool,
    pub prevent_sleep: bool,
    pub clear_standby: bool,
    pub standby_free_mb_threshold: u64,
    /// Redam aplikasi latar lebih cepat saat bermain.
    pub aggressive_balancer: bool,
    /// Matikan Efficiency Mode Windows pada game.
    pub disable_efficiency_on_game: bool,
    /// Di CPU hybrid, jalankan game hanya di P-core.
    pub game_on_pcores: bool,
    /// Bersihkan RAM standby berkala selama bermain, bukan hanya saat mulai.
    pub standby_periodic: bool,
    pub standby_check_secs: u64,
    /// Bekukan aplikasi latar ini selama bermain, lalu jalankan lagi setelahnya.
    pub freeze_enabled: bool,
    pub freeze_apps: Vec<String>,
    /// Ukur FPS dan frame time game dengan PresentMon.
    pub fps_meter: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            auto: true,
            games: vec!["project8.exe".to_string()],
            detect_store_folders: true,
            detect_fullscreen: true,
            fullscreen_exclusions: [
                "chrome.exe", "msedge.exe", "firefox.exe", "opera.exe", "brave.exe",
                "vivaldi.exe", "vlc.exe", "mpc-hc64.exe", "mpc-be64.exe", "potplayermini64.exe",
                "wmplayer.exe", "explorer.exe", "applicationframehost.exe", "powerpnt.exe",
                "obs64.exe", "discord.exe", "spotify.exe", "mpv.exe",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            game_priority: Some(Priority::High),
            game_io_priority: Some(IoPriority::High),
            game_gpu_priority: Some(GpuPriority::High),
            power_plan: PowerPlanChoice::Auto,
            timer_resolution: true,
            prevent_sleep: true,
            clear_standby: true,
            standby_free_mb_threshold: 1024,
            aggressive_balancer: true,
            disable_efficiency_on_game: true,
            game_on_pcores: false,
            standby_periodic: true,
            standby_check_secs: 30,
            freeze_enabled: true,
            freeze_apps: [
                "OneDrive.exe", "PhoneExperienceHost.exe", "Widgets.exe", "WidgetService.exe",
                "GoogleDriveFS.exe", "Dropbox.exe", "AdobeCollabSync.exe", "CCXProcess.exe",
                "Creative Cloud.exe", "MicrosoftEdgeUpdate.exe", "GoogleUpdate.exe",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            fps_meter: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessRule {
    pub pattern: String,
    pub priority: Option<Priority>,
    pub io_priority: Option<IoPriority>,
    pub gpu_priority: Option<GpuPriority>,
    /// Bitmask CPU (bit 0 = CPU 0).
    pub affinity: Option<u64>,
    pub efficiency_mode: Option<bool>,
    pub never_restrain: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub refresh_ms: u64,
    pub log_to_file: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { start_minimized: false, close_to_tray: true, refresh_ms: 1000, log_to_file: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub balancer: BalancerConfig,
    pub game: GameConfig,
    pub rules: Vec<ProcessRule>,
    pub general: GeneralConfig,
    pub overlay: OverlayConfig,
    pub ram: RamConfig,
    pub discord: DiscordConfig,
    pub wake: WakeConfig,
    pub update: UpdateConfig,
}

/// Perangkat yang tidak boleh membangunkan PC dari sleep (nama persis dari powercfg). Ditegakkan
/// ulang setiap Pacu menyala.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WakeConfig {
    pub blocked: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Periksa rilis baru saat Pacu menyala dan tiap 6 jam.
    pub auto_check: bool,
    /// Repo GitHub tempat rilis ("pemilik/nama").
    pub repo: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        UpdateConfig { auto_check: true, repo: crate::update::DEFAULT_REPO.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RamConfig {
    /// Rampingkan RAM berkala saat tidak sedang bermain.
    pub auto_trim: bool,
    pub interval_min: u64,
    /// Hanya bila RAM terpakai di atas angka ini (MB).
    pub min_used_mb: u64,
}

impl Default for RamConfig {
    fn default() -> Self {
        RamConfig { auto_trim: true, interval_min: 5, min_used_mb: 1536 }
    }
}

/// Penghemat RAM khusus Discord (semua proses Discord.exe, PTB, dan Canary).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordConfig {
    pub enabled: bool,
    /// Kosongkan working set Discord berkala selama jendelanya tidak di depan.
    pub trim: bool,
    pub trim_interval_secs: u64,
    /// Tetap rampingkan saat Mode Game (hanya Discord, game tidak ikut tersentuh).
    pub trim_in_game: bool,
    /// Saat Mode Game: prioritas CPU di bawah normal, I/O rendah, dan Efficiency Mode.
    pub tame_in_game: bool,
    /// Batas keras RAM fisik per proses Discord (MB), 0 = mati. Dilepas saat Discord dibuka.
    pub hard_limit_mb: u64,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        DiscordConfig { enabled: true, trim: true, trim_interval_secs: 60, trim_in_game: true, tame_in_game: true, hard_limit_mb: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayMode {
    /// Lewat RTSS bila sedang berjalan, selain itu jendela Pacu.
    Auto,
    Rtss,
    Window,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];
    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "Kiri atas",
            Corner::TopRight => "Kanan atas",
            Corner::BottomLeft => "Kiri bawah",
            Corner::BottomRight => "Kanan bawah",
        }
    }
}

/// Tampilan statistik di atas game (seperti OSD RivaTuner).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub enabled: bool,
    pub mode: OverlayMode,
    pub corner: Corner,
    /// Tampil juga saat tidak ada game (untuk mengatur posisi).
    pub always: bool,
    pub scale: f32,
    pub show_fps: bool,
    pub show_frametime: bool,
    pub show_gpu: bool,
    pub show_vram: bool,
    pub show_cpu: bool,
    pub show_ram: bool,
    /// Jalankan RTSS otomatis supaya overlay tampil juga di Fullscreen eksklusif.
    pub launch_rtss: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: OverlayMode::Auto,
            corner: Corner::TopLeft,
            always: false,
            scale: 1.0,
            show_fps: true,
            show_frametime: true,
            show_gpu: true,
            show_vram: true,
            show_cpu: true,
            show_ram: true,
            launch_rtss: true,
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Pacu")
}

impl Config {
    pub fn path() -> PathBuf {
        data_dir().join("config.json")
    }

    pub fn load() -> Config {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap())?;
        std::fs::rename(tmp, path)
    }

    pub fn rule_for(&self, name_lc: &str, path_lc: &str) -> Option<&ProcessRule> {
        self.rules.iter().find(|r| crate::matcher::matches(&r.pattern, name_lc, path_lc))
    }
}
