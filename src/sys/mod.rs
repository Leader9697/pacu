//! Lapisan sistem operasi. Di Windows memakai API asli; di platform lain memakai simulasi
//! supaya UI dan logika bisa dikembangkan dan diuji tanpa Windows.

use crate::config::{GpuPriority, IoPriority, Priority};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::WinSys as NativeSys;

#[cfg(not(windows))]
mod sim;
#[cfg(not(windows))]
pub use self::sim::SimSys as NativeSys;

#[derive(Clone, Debug, Default)]
pub struct RawProc {
    pub pid: u32,
    pub ppid: u32,
    pub create_time: u64,
    pub name: String,
    pub session: u32,
    /// Waktu CPU kernel + user, satuan 100 ns.
    pub cpu_time: u64,
    pub working_set: u64,
    pub base_priority: i32,
    pub threads: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Topology {
    pub logical: u32,
    /// ID CPU set untuk P-core dan E-core (kosong bila bukan CPU hybrid).
    pub pcore_sets: Vec<u32>,
    pub ecore_sets: Vec<u32>,
    pub cpu_name: String,
}

impl Topology {
    pub fn hybrid(&self) -> bool {
        !self.ecore_sets.is_empty() && !self.pcore_sets.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Foreground {
    pub pid: u32,
    pub fullscreen: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemInfo {
    pub total_mb: u64,
    pub avail_mb: u64,
    pub load: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
}

pub const ULTIMATE_PERFORMANCE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";
pub const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
pub const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
pub const PACU_PLAN_NAME: &str = "Pacu Performa";

/// Semua operasi yang dibutuhkan mesin. Fungsi `set_*` mengembalikan `false` bila gagal.
pub trait Sys {
    fn processes(&mut self) -> Vec<RawProc>;
    /// (idle, kernel, user) dalam satuan 100 ns; kernel sudah termasuk idle.
    fn system_times(&mut self) -> Option<(u64, u64, u64)>;
    fn topology(&self) -> Topology;
    fn process_path(&mut self, pid: u32) -> Option<String>;
    fn foreground(&self) -> Option<Foreground>;
    fn idle_ms(&self) -> u64;
    fn memory(&self) -> MemInfo;
    fn self_pid(&self) -> u32;
    /// Jam lokal (jam, menit, detik) untuk log.
    fn local_time(&self) -> (u16, u16, u16);

    fn get_priority(&self, pid: u32) -> Option<Priority>;
    fn set_priority(&self, pid: u32, p: Priority) -> bool;
    fn set_io_priority(&self, pid: u32, p: IoPriority) -> bool;
    fn set_gpu_priority(&self, pid: u32, p: GpuPriority) -> bool;
    /// `Some(true)` = paksa Efficiency Mode, `Some(false)` = paksa performa penuh, `None` = serahkan ke Windows.
    fn set_efficiency(&self, pid: u32, on: Option<bool>) -> bool;
    /// Daftar kosong = hapus batasan CPU set.
    fn set_cpu_sets(&self, pid: u32, sets: &[u32]) -> bool;
    fn set_affinity(&self, pid: u32, mask: u64) -> bool;
    fn terminate(&self, pid: u32) -> bool;
    /// Membekukan atau melanjutkan seluruh thread proses.
    fn suspend(&self, pid: u32) -> bool;
    fn resume(&self, pid: u32) -> bool;

    fn power_plans(&self) -> Vec<PowerPlan>;
    fn active_power_plan(&self) -> Option<String>;
    fn set_power_plan(&self, guid: &str) -> bool;
    /// Membuat (bila belum ada) dan menyetel power plan "Pacu Performa". Mengembalikan GUID-nya.
    fn ensure_performance_plan(&self) -> Option<String>;
    /// Meminta resolusi timer 0,5 ms (`true`) atau melepasnya (`false`). Mengembalikan resolusi aktif (ms).
    fn set_timer_resolution(&mut self, on: bool) -> Option<f32>;
    fn prevent_sleep(&mut self, on: bool);
    fn purge_standby(&mut self) -> bool;
    /// Mengosongkan working set semua proses lalu menulis halaman termodifikasi ke pagefile, sehingga
    /// RAM "terpakai" turun. Halaman yang dibutuhkan lagi dibaca balik dari standby (soft fault).
    fn trim_memory(&mut self) -> bool;
    /// Mengosongkan working set satu proses (EmptyWorkingSet).
    fn trim_process(&self, pid: u32) -> bool;
    /// `Some(mb)` = batas keras working set (proses tidak boleh memakai RAM fisik lebih dari ini),
    /// `None` = lepas batas.
    fn limit_working_set(&self, pid: u32, max_mb: Option<u64>) -> bool;
}

#[cfg(windows)]
pub use self::windows::instance;
#[cfg(not(windows))]
pub use self::sim::instance;
