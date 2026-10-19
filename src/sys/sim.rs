//! Simulasi sistem untuk pengembangan di luar Windows: daftar proses palsu dengan beban CPU
//! yang berubah-ubah, termasuk satu proses latar yang rakus CPU dan satu game Steam.

use super::{Foreground, MemInfo, PowerPlan, RawProc, Sys, Topology};
use crate::config::{GpuPriority, IoPriority, Priority};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

struct SimProc {
    pid: u32,
    name: &'static str,
    path: &'static str,
    session: u32,
    load: fn(f64) -> f64,
    cpu_time: u64,
    priority: Priority,
    ws_mb: u64,
}

pub struct SimSys {
    start: Instant,
    last: f64,
    procs: Vec<SimProc>,
    idle: u64,
    kernel: u64,
    user: u64,
    prio: Mutex<HashMap<u32, Priority>>,
    plan: Mutex<String>,
    /// Waktu proses terakhir dirampingkan; working set-nya tumbuh lagi pelan-pelan.
    trimmed: Mutex<HashMap<u32, Instant>>,
}

const LOGICAL: u32 = 20;

impl SimSys {
    pub fn new() -> SimSys {
        let p = |pid, name, path, session, load: fn(f64) -> f64, ws_mb| SimProc {
            pid,
            name,
            path,
            session,
            load,
            cpu_time: 0,
            priority: Priority::Normal,
            ws_mb,
        };
        let procs = vec![
            p(4, "System", "", 0, |_| 0.3, 1),
            p(812, "svchost.exe", "C:\\Windows\\System32\\svchost.exe", 0, |_| 0.4, 30),
            p(1020, "MsMpEng.exe", "C:\\ProgramData\\Microsoft\\Windows Defender\\MsMpEng.exe", 0, |t| if (t as u64 / 20).is_multiple_of(3) { 3.5 } else { 0.2 }, 210),
            p(2210, "explorer.exe", "C:\\Windows\\explorer.exe", 1, |_| 0.6, 140),
            p(3300, "dwm.exe", "C:\\Windows\\System32\\dwm.exe", 1, |_| 1.5, 90),
            p(4120, "chrome.exe", "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", 1, |t| 1.0 + (t * 0.7).sin().abs() * 2.0, 480),
            p(4122, "chrome.exe", "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", 1, |_| 0.4, 220),
            p(5010, "Discord.exe", "C:\\Users\\Faris\\AppData\\Local\\Discord\\app-1.0\\Discord.exe", 1, |_| 0.8, 310),
            p(5011, "Discord.exe", "C:\\Users\\Faris\\AppData\\Local\\Discord\\app-1.0\\Discord.exe", 1, |_| 0.4, 420),
            p(5012, "Discord.exe", "C:\\Users\\Faris\\AppData\\Local\\Discord\\app-1.0\\Discord.exe", 1, |_| 0.2, 180),
            p(5502, "OneDrive.exe", "C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe", 1, |t| if (t as u64 / 15).is_multiple_of(2) { 9.0 } else { 0.3 }, 120),
            p(6100, "HandBrake.exe", "C:\\Program Files\\HandBrake\\HandBrake.exe", 1, |_| 38.0, 650),
            p(7020, "steam.exe", "C:\\Program Files (x86)\\Steam\\steam.exe", 1, |_| 0.7, 260),
            p(7400, "project8.exe", "D:\\SteamLibrary\\steamapps\\common\\Deadlock\\game\\bin\\win64\\project8.exe", 1, |_| 24.0, 5200),
            p(8800, "Spotify.exe", "C:\\Users\\Faris\\AppData\\Roaming\\Spotify\\Spotify.exe", 1, |_| 0.9, 180),
        ];
        SimSys {
            start: Instant::now(),
            last: 0.0,
            procs,
            idle: 0,
            kernel: 0,
            user: 0,
            prio: Mutex::new(HashMap::new()),
            plan: Mutex::new("381b4222-f694-41f0-9685-ff5bb260df2e".into()),
            trimmed: Mutex::new(HashMap::new()),
        }
    }

    fn advance(&mut self) {
        let now = self.start.elapsed().as_secs_f64();
        let dt = now - self.last;
        if dt <= 0.0 {
            return;
        }
        self.last = now;
        let capacity = dt * 1e7 * LOGICAL as f64;
        let mut used = 0.0;
        let prio = self.prio.lock();
        for p in &mut self.procs {
            let mut pct = (p.load)(now);
            if let Some(pr) = prio.get(&p.pid) {
                p.priority = *pr;
                if *pr < Priority::Normal {
                    pct *= 0.85;
                }
            }
            let t = capacity * pct / 100.0;
            p.cpu_time += t as u64;
            used += t;
        }
        self.user += used as u64;
        self.kernel += (capacity - used) as u64;
        self.idle += (capacity - used) as u64;
    }
}

impl Default for SimSys {
    fn default() -> Self {
        Self::new()
    }
}

impl Sys for SimSys {
    fn processes(&mut self) -> Vec<RawProc> {
        self.advance();
        let trimmed = self.trimmed.lock().clone();
        self.procs
            .iter()
            .map(|p| RawProc {
                pid: p.pid,
                ppid: if p.pid == 4122 { 4120 } else { 2210 },
                create_time: 1,
                name: p.name.to_string(),
                session: p.session,
                cpu_time: p.cpu_time,
                working_set: match trimmed.get(&p.pid) {
                    // Setelah dirampingkan tinggal ~15%, lalu naik kembali dalam 2 menit.
                    Some(t) => {
                        let f = (0.15 + t.elapsed().as_secs_f64() / 120.0 * 0.85).min(1.0);
                        ((p.ws_mb as f64 * f) as u64) << 20
                    }
                    None => p.ws_mb << 20,
                },
                base_priority: match p.priority {
                    Priority::Idle => 4,
                    Priority::BelowNormal => 6,
                    Priority::Normal => 8,
                    Priority::AboveNormal => 10,
                    Priority::High => 13,
                    Priority::Realtime => 24,
                },
                threads: 12,
            })
            .collect()
    }

    fn system_times(&mut self) -> Option<(u64, u64, u64)> {
        Some((self.idle, self.kernel, self.user))
    }

    fn topology(&self) -> Topology {
        Topology {
            logical: LOGICAL,
            pcore_sets: (256..268).collect(),
            ecore_sets: (268..276).collect(),
            cpu_name: "Simulasi: Intel Core i7-12700H".into(),
        }
    }

    fn process_path(&mut self, pid: u32) -> Option<String> {
        self.procs.iter().find(|p| p.pid == pid).map(|p| p.path.to_string()).filter(|s| !s.is_empty())
    }

    fn foreground(&self) -> Option<Foreground> {
        Some(Foreground { pid: 7400, fullscreen: true })
    }

    fn idle_ms(&self) -> u64 {
        0
    }

    fn memory(&self) -> MemInfo {
        MemInfo { total_mb: 16_384, avail_mb: 6_900, load: 58 }
    }

    fn self_pid(&self) -> u32 {
        std::process::id()
    }

    fn local_time(&self) -> (u16, u16, u16) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() + 7 * 3600;
        (((t / 3600) % 24) as u16, ((t / 60) % 60) as u16, (t % 60) as u16)
    }

    fn get_priority(&self, pid: u32) -> Option<Priority> {
        Some(self.prio.lock().get(&pid).copied().unwrap_or(Priority::Normal))
    }

    fn set_priority(&self, pid: u32, p: Priority) -> bool {
        self.prio.lock().insert(pid, p);
        true
    }

    fn set_io_priority(&self, _: u32, _: IoPriority) -> bool {
        true
    }

    fn set_gpu_priority(&self, _: u32, _: GpuPriority) -> bool {
        true
    }

    fn set_efficiency(&self, _: u32, _: Option<bool>) -> bool {
        true
    }

    fn set_cpu_sets(&self, _: u32, _: &[u32]) -> bool {
        true
    }

    fn set_affinity(&self, _: u32, _: u64) -> bool {
        true
    }

    fn terminate(&self, _: u32) -> bool {
        false
    }

    fn suspend(&self, _: u32) -> bool {
        true
    }

    fn resume(&self, _: u32) -> bool {
        true
    }

    fn ensure_performance_plan(&self) -> Option<String> {
        Some("0e5c0a11-0000-4000-8000-00000000pacu".into())
    }

    fn power_plans(&self) -> Vec<PowerPlan> {
        vec![
            PowerPlan { guid: "0e5c0a11-0000-4000-8000-00000000pacu".into(), name: super::PACU_PLAN_NAME.into() },
            PowerPlan { guid: "381b4222-f694-41f0-9685-ff5bb260df2e".into(), name: "Balanced".into() },
            PowerPlan { guid: super::HIGH_PERFORMANCE.into(), name: "High performance".into() },
            PowerPlan { guid: super::ULTIMATE_PERFORMANCE.into(), name: "Ultimate Performance".into() },
        ]
    }

    fn active_power_plan(&self) -> Option<String> {
        Some(self.plan.lock().clone())
    }

    fn set_power_plan(&self, guid: &str) -> bool {
        *self.plan.lock() = guid.to_string();
        true
    }

    fn set_timer_resolution(&mut self, on: bool) -> Option<f32> {
        Some(if on { 0.5 } else { 15.6 })
    }

    fn prevent_sleep(&mut self, _: bool) {}

    fn purge_standby(&mut self) -> bool {
        true
    }

    fn trim_memory(&mut self) -> bool {
        true
    }

    fn trim_process(&self, pid: u32) -> bool {
        self.trimmed.lock().insert(pid, Instant::now());
        true
    }

    fn limit_working_set(&self, _: u32, _: Option<u64>) -> bool {
        true
    }
}

pub mod instance {
    pub struct Instance;

    pub fn acquire() -> Option<Instance> {
        Some(Instance)
    }

    impl Instance {
        pub fn show_requested(&self) -> bool {
            false
        }
    }
}
