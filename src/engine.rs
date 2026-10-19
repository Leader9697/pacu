//! Mesin inti: membaca proses tiap siklus, menerapkan aturan, Mode Game, dan penyeimbang CPU.
//! Berjalan di thread sendiri; UI hanya membaca `Snapshot` dan mengirim `Command`.

use crate::balancer::{self, Action, Balancer, ProcKey, Sample};
use crate::config::{Config, GpuPriority, IoPriority, PowerPlanChoice, Priority};
use crate::discord;
use crate::matcher;
use crate::sys::{MemInfo, PowerPlan, RawProc, Sys, Topology, HIGH_PERFORMANCE, ULTIMATE_PERFORMANCE};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Proses yang tidak pernah diredam karena penting untuk sistem atau tampilan.
const CRITICAL: &[&str] = &[
    "system", "registry", "memory compression", "smss.exe", "csrss.exe", "wininit.exe",
    "winlogon.exe", "services.exe", "lsass.exe", "dwm.exe", "audiodg.exe", "fontdrvhost.exe",
    "ctfmon.exe", "sihost.exe", "searchhost.exe", "startmenuexperiencehost.exe", "textinputhost.exe",
];

/// Folder toko game; exe di dalamnya dianggap game bila sedang di depan.
const STORE_FOLDERS: &[&str] = &[
    "*\\steamapps\\common\\*",
    "*\\epic games\\*",
    "*\\riot games\\*",
    "*\\xboxgames\\*",
    "*\\windowsapps\\*",
    "*\\gog galaxy\\games\\*",
    "*\\ubisoft game launcher\\games\\*",
    "*\\ea games\\*",
    "*\\battle.net\\*",
];

/// Launcher dan proses pembantu di folder toko yang bukan game.
const NOT_GAMES: &[&str] = &[
    "steam.exe", "steamwebhelper.exe", "epicgameslauncher.exe", "unrealcefsubprocess.exe",
    "crashreportclient.exe", "riotclientservices.exe", "riotclientux.exe", "riotclientuxrender.exe",
    "battle.net.exe", "agent.exe", "eadesktop.exe", "upc.exe", "galaxyclient.exe", "*crash*.exe",
    "*launcher*.exe", "*setup*.exe", "*installer*.exe", "unins*.exe", "easyanticheat*.exe",
    "beservice*.exe", "vgc.exe", "vgtray.exe",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Normal,
    Restrained,
    Game,
    Ruled,
}

#[derive(Clone, Debug)]
pub struct ProcView {
    pub pid: u32,
    pub name: String,
    pub path: String,
    pub cpu: f32,
    pub mem_mb: f32,
    pub priority: Priority,
    pub threads: u32,
    pub session: u32,
    pub status: Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Restrain,
    Restore,
    Game,
    Rule,
    Warn,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: String,
    pub kind: LogKind,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct GameInfo {
    pub name: String,
    pub pid: Option<u32>,
    pub since: Instant,
    pub forced: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub procs: Vec<ProcView>,
    pub total_cpu: f32,
    pub cpu_history: VecDeque<f32>,
    pub mem: MemInfo,
    pub game: Option<GameInfo>,
    pub forced: Option<bool>,
    pub restrained_now: usize,
    pub restraints_total: u64,
    pub topology: Topology,
    pub power_plans: Vec<PowerPlan>,
    pub active_plan: String,
    pub timer_ms: Option<f32>,
    pub thresholds: Option<balancer::Thresholds>,
    pub log: VecDeque<LogEntry>,
    pub balancer_active: bool,
    pub frozen: Vec<String>,
    pub last_trim: Option<TrimResult>,
    pub discord: DiscordStat,
    /// Naik setiap siklus supaya UI tahu ada data baru.
    pub version: u64,
}

/// Hasil perampingan RAM terakhir.
#[derive(Clone, Debug, PartialEq)]
pub struct TrimResult {
    pub before_mb: u64,
    pub after_mb: u64,
    pub auto: bool,
    pub at: (u16, u16, u16),
}

/// Keadaan penghemat RAM Discord untuk halaman RAM Ekstrem.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiscordStat {
    pub procs: usize,
    pub mem_mb: f32,
    /// Sedang diredam karena Mode Game.
    pub tamed: bool,
    /// Batas keras working set sedang terpasang.
    pub capped: bool,
    pub last: Option<TrimResult>,
}

pub enum Command {
    /// Rampingkan semua proses Discord sekarang.
    TrimDiscord,
    /// Rampingkan RAM sekarang (dari tombol di halaman RAM).
    TrimRam,
    UpdateConfig(Box<Config>),
    SetPriority(u32, Priority),
    SetIoPriority(u32, IoPriority),
    SetGpuPriority(u32, GpuPriority),
    Terminate(u32),
    /// `None` = otomatis, `Some(true)` = paksa nyala, `Some(false)` = paksa mati.
    ForceGameMode(Option<bool>),
    /// Catatan dari UI (misalnya optimasi Windows) untuk halaman Aktivitas.
    Log(String),
    Quit,
}

struct Known {
    name_lc: String,
    path: String,
    path_lc: String,
    last_cpu_time: u64,
    rule_checked: bool,
    has_rule: bool,
    fullscreen_ticks: u32,
}

#[derive(Default)]
struct Applied {
    io: bool,
    efficiency: bool,
    cpu_sets: bool,
}

#[derive(Default)]
struct DiscordProc {
    tamed: bool,
    capped: Option<u64>,
}

struct GameMode {
    info: GameInfo,
    game_key: Option<ProcKey>,
    game_priority_set: Option<Priority>,
    original_plan: Option<String>,
    plan_set: Option<String>,
    /// Aplikasi latar yang sedang dibekukan.
    frozen: HashMap<ProcKey, String>,
    last_standby: Instant,
}

pub struct Engine<S: Sys> {
    sys: S,
    cfg: Config,
    shared: Arc<RwLock<Snapshot>>,
    known: HashMap<ProcKey, Known>,
    applied: HashMap<ProcKey, Applied>,
    balancer: Balancer,
    last_times: Option<(u64, u64, u64)>,
    topology: Topology,
    game: Option<GameMode>,
    forced: Option<bool>,
    log: VecDeque<LogEntry>,
    history: VecDeque<f32>,
    start: Instant,
    tick_count: u64,
    timer_ms: Option<f32>,
    log_file: Option<std::fs::File>,
    last_trim: Option<TrimResult>,
    last_trim_at: Instant,
    discord: HashMap<ProcKey, DiscordProc>,
    discord_last_trim: Option<TrimResult>,
    discord_trim_at: Instant,
}

impl<S: Sys> Engine<S> {
    pub fn new(sys: S, cfg: Config, shared: Arc<RwLock<Snapshot>>) -> Self {
        let topology = sys.topology();
        let log_file = if cfg.general.log_to_file { open_log_file() } else { None };
        let mut e = Engine {
            sys,
            cfg,
            shared,
            known: HashMap::new(),
            applied: HashMap::new(),
            balancer: Balancer::default(),
            last_times: None,
            topology,
            game: None,
            forced: None,
            log: VecDeque::new(),
            history: VecDeque::new(),
            start: Instant::now(),
            tick_count: 0,
            timer_ms: None,
            log_file,
            last_trim: None,
            last_trim_at: Instant::now(),
            discord: HashMap::new(),
            discord_last_trim: None,
            discord_trim_at: Instant::now(),
        };
        let t = &e.topology;
        let msg = if t.hybrid() {
            format!(
                "Pacu aktif. CPU: {} ({} thread, {} P-core set + {} E-core set).",
                t.cpu_name, t.logical, t.pcore_sets.len(), t.ecore_sets.len()
            )
        } else {
            format!("Pacu aktif. CPU: {} ({} thread).", t.cpu_name, t.logical)
        };
        e.log(LogKind::Info, msg);
        e
    }

    fn log(&mut self, kind: LogKind, text: String) {
        let (h, m, s) = self.sys.local_time();
        let time = format!("{h:02}:{m:02}:{s:02}");
        if let Some(f) = self.log_file.as_mut() {
            let _ = writeln!(f, "{time} {kind:?} {text}");
        }
        self.log.push_back(LogEntry { time, kind, text });
        while self.log.len() > 500 {
            self.log.pop_front();
        }
    }

    /// Loop utama sampai menerima `Command::Quit`.
    pub fn run(mut self, rx: Receiver<Command>) {
        loop {
            let started = Instant::now();
            self.tick();
            let wait = Duration::from_millis(self.cfg.general.refresh_ms.clamp(250, 5000));
            let mut quit = false;
            loop {
                let left = wait.saturating_sub(started.elapsed());
                match rx.recv_timeout(left) {
                    Ok(Command::Quit) | Err(RecvTimeoutError::Disconnected) => {
                        quit = true;
                        break;
                    }
                    Ok(cmd) => self.handle(cmd),
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            if quit {
                self.shutdown();
                return;
            }
        }
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::UpdateConfig(cfg) => {
                self.cfg = *cfg;
                for k in self.known.values_mut() {
                    k.rule_checked = false;
                }
                if self.cfg.general.log_to_file && self.log_file.is_none() {
                    self.log_file = open_log_file();
                } else if !self.cfg.general.log_to_file {
                    self.log_file = None;
                }
                self.log(LogKind::Info, "Pengaturan disimpan dan diterapkan.".into());
            }
            Command::SetPriority(pid, p) => {
                let ok = self.sys.set_priority(pid, p);
                self.log_result(ok, format!("Prioritas CPU PID {pid} diubah ke {}", p.label()));
            }
            Command::SetIoPriority(pid, p) => {
                let ok = self.sys.set_io_priority(pid, p);
                self.log_result(ok, format!("Prioritas I/O PID {pid} diubah ke {}", p.label()));
            }
            Command::SetGpuPriority(pid, p) => {
                let ok = self.sys.set_gpu_priority(pid, p);
                self.log_result(ok, format!("Prioritas GPU PID {pid} diubah ke {}", p.label()));
            }
            Command::Terminate(pid) => {
                let ok = self.sys.terminate(pid);
                self.log_result(ok, format!("Menghentikan PID {pid}"));
            }
            Command::ForceGameMode(f) => {
                self.forced = f;
                let msg = match f {
                    None => "Mode Game: otomatis.",
                    Some(true) => "Mode Game dinyalakan manual.",
                    Some(false) => "Mode Game dimatikan manual.",
                };
                self.log(LogKind::Game, msg.into());
                self.tick();
            }
            Command::Log(text) => self.log(LogKind::Info, text),
            Command::TrimRam => self.trim_ram(false),
            Command::TrimDiscord => self.trim_discord(false),
            Command::Quit => {}
        }
    }

    fn log_result(&mut self, ok: bool, text: String) {
        if ok {
            self.log(LogKind::Rule, text);
        } else {
            self.log(LogKind::Warn, format!("Gagal: {text} (akses ditolak atau proses sudah keluar)"));
        }
    }

    /// Mengosongkan working set semua proses dan mencatat RAM terpakai sebelum dan sesudahnya.
    fn trim_ram(&mut self, auto: bool) {
        let before = self.sys.memory();
        let ok = self.sys.trim_memory();
        self.last_trim_at = Instant::now();
        if !ok {
            self.log(LogKind::Warn, "Gagal merampingkan RAM (butuh hak administrator).".into());
            return;
        }
        std::thread::sleep(Duration::from_millis(700));
        let after = self.sys.memory();
        let used = |m: &MemInfo| m.total_mb.saturating_sub(m.avail_mb);
        let r = TrimResult { before_mb: used(&before), after_mb: used(&after), auto, at: self.sys.local_time() };
        let gb = |mb: u64| format!("{:.1}", mb as f64 / 1024.0);
        let how = if auto { "otomatis" } else { "manual" };
        self.log(LogKind::Rule, format!("RAM dirampingkan ({how}): terpakai {} GB ke {} GB.", gb(r.before_mb), gb(r.after_mb)));
        self.last_trim = Some(r);
    }

    /// Mengosongkan working set semua proses Discord dan mencatat RAM yang dibebaskan.
    fn trim_discord(&mut self, auto: bool) {
        self.discord_trim_at = Instant::now();
        let ws = |raw: &[RawProc], pids: &HashSet<u32>| -> u64 {
            raw.iter().filter(|p| pids.contains(&p.pid)).map(|p| p.working_set >> 20).sum()
        };
        let raw = self.sys.processes();
        let pids: HashSet<u32> =
            raw.iter().filter(|p| discord::is_discord(&p.name.to_lowercase())).map(|p| p.pid).collect();
        if pids.is_empty() {
            if !auto {
                self.log(LogKind::Info, "Discord tidak sedang berjalan.".into());
            }
            return;
        }
        let before = ws(&raw, &pids);
        let ok = pids.iter().filter(|&&pid| self.sys.trim_process(pid)).count();
        if ok == 0 {
            if !auto {
                self.log(LogKind::Warn, "Gagal merampingkan Discord (akses ditolak).".into());
            }
            return;
        }
        let after = ws(&self.sys.processes(), &pids);
        let r = TrimResult { before_mb: before, after_mb: after.min(before), auto, at: self.sys.local_time() };
        let freed = r.before_mb - r.after_mb;
        // Perampingan otomatis berjalan tiap menit; dicatat hanya bila hasilnya berarti.
        if !auto || freed >= 200 {
            let how = if auto { "otomatis" } else { "manual" };
            self.log(LogKind::Rule, format!("Discord dirampingkan ({how}): {before} MB ke {} MB, hemat {freed} MB.", r.after_mb));
        }
        self.discord_last_trim = Some(r);
    }

    /// Penghemat RAM Discord: perampingan berkala, redam saat Mode Game, dan batas keras RAM.
    /// Semua dilepas selama jendela Discord ada di depan supaya tetap responsif.
    fn discord_maintenance(&mut self, raw: &[RawProc], fg_pid: u32) {
        let dc = self.cfg.discord.clone();
        let procs: Vec<(ProcKey, u32, i32)> = raw
            .iter()
            .filter(|p| self.known.get(&key_of(p)).map(|k| discord::is_discord(&k.name_lc)).unwrap_or(false))
            .map(|p| (key_of(p), p.pid, p.base_priority))
            .collect();
        self.discord.retain(|k, _| procs.iter().any(|p| p.0 == *k));
        if procs.is_empty() {
            return;
        }
        let active = procs.iter().any(|p| p.1 == fg_pid);
        let in_game = self.game.is_some();
        let want_tame = dc.enabled && dc.tame_in_game && in_game && !active;
        let want_cap = (dc.enabled && dc.hard_limit_mb > 0 && !active).then_some(dc.hard_limit_mb);
        let (mut tamed, mut untamed) = (0, 0);
        for &(key, pid, base) in &procs {
            let st = self.discord.entry(key).or_default();
            if want_tame {
                // Diulang bila prioritasnya dinaikkan lagi oleh pihak lain.
                if !st.tamed || base > 6 {
                    self.sys.set_priority(pid, Priority::BelowNormal);
                    self.sys.set_io_priority(pid, IoPriority::Low);
                    self.sys.set_efficiency(pid, Some(true));
                    if !st.tamed {
                        tamed += 1;
                    }
                    st.tamed = true;
                }
            } else if st.tamed {
                self.sys.set_priority(pid, Priority::Normal);
                self.sys.set_io_priority(pid, IoPriority::Normal);
                self.sys.set_efficiency(pid, None);
                st.tamed = false;
                untamed += 1;
            }
            // Batas dipasang ulang tiap kali Discord tidak di depan, jadi tidak dicatat di log.
            if st.capped != want_cap && self.sys.limit_working_set(pid, want_cap) {
                st.capped = want_cap;
            }
        }
        if tamed > 0 {
            self.log(LogKind::Game, format!("Discord diredam selama bermain ({tamed} proses): prioritas rendah dan Efficiency Mode."));
        }
        if untamed > 0 && !active {
            self.log(LogKind::Game, "Discord kembali normal.".into());
        }
        let interval = dc.trim_interval_secs.max(15);
        let due = dc.enabled
            && dc.trim
            && !active
            && (!in_game || dc.trim_in_game)
            && self.discord_trim_at.elapsed().as_secs() >= interval;
        if due {
            self.trim_discord(true);
        }
    }

    /// Perampingan berkala, hanya saat tidak bermain supaya game tidak tersendat.
    fn auto_trim(&mut self) {
        let rc = &self.cfg.ram;
        if !rc.auto_trim || self.game.is_some() || self.last_trim_at.elapsed().as_secs() < rc.interval_min.max(1) * 60 {
            return;
        }
        let m = self.sys.memory();
        if m.total_mb.saturating_sub(m.avail_mb) < rc.min_used_mb {
            self.last_trim_at = Instant::now();
            return;
        }
        self.trim_ram(true);
    }

    fn tick(&mut self) {
        self.tick_count += 1;
        self.auto_trim();
        let now_ms = self.start.elapsed().as_millis() as u64;
        let raw = self.sys.processes();
        let times = self.sys.system_times();
        let (total_cpu, capacity) = match (self.last_times, times) {
            (Some((i0, k0, u0)), Some((i1, k1, u1))) => {
                let busy_total = (k1 - k0) + (u1 - u0);
                let idle = i1 - i0;
                if busy_total == 0 {
                    (0.0, 0)
                } else {
                    (100.0 * (1.0 - idle as f32 / busy_total as f32), busy_total)
                }
            }
            _ => (0.0, 0),
        };
        if times.is_some() {
            self.last_times = times;
        }
        let self_pid = self.sys.self_pid();

        // Data per proses dan CPU %.
        let alive: HashSet<ProcKey> = raw.iter().map(key_of).collect();
        self.known.retain(|k, _| alive.contains(k));
        self.applied.retain(|k, _| alive.contains(k));
        let mut cpu: HashMap<ProcKey, f32> = HashMap::with_capacity(raw.len());
        let mut new_keys = Vec::new();
        for p in &raw {
            let key = key_of(p);
            if p.pid == 0 {
                continue;
            }
            match self.known.get_mut(&key) {
                Some(k) => {
                    let d = p.cpu_time.saturating_sub(k.last_cpu_time);
                    k.last_cpu_time = p.cpu_time;
                    let pct = if capacity > 0 { 100.0 * d as f32 / capacity as f32 } else { 0.0 };
                    cpu.insert(key, pct.min(100.0));
                }
                None => {
                    let path = if p.pid > 4 { self.sys.process_path(p.pid).unwrap_or_default() } else { String::new() };
                    self.known.insert(
                        key,
                        Known {
                            name_lc: p.name.to_lowercase(),
                            path_lc: path.to_lowercase(),
                            path,
                            last_cpu_time: p.cpu_time,
                            rule_checked: false,
                            has_rule: false,
                            fullscreen_ticks: 0,
                        },
                    );
                    cpu.insert(key, 0.0);
                    new_keys.push(key);
                }
            }
        }

        if self.tick_count == 1 {
            // Bila Pacu sebelumnya tertutup paksa saat bermain, aplikasi yang dibekukan dijalankan lagi.
            for p in &raw {
                if let Some(k) = self.known.get(&key_of(p)) {
                    if matcher::matches_any(&self.cfg.game.freeze_apps, &k.name_lc, &k.path_lc) {
                        self.sys.resume(p.pid);
                    }
                }
            }
        }
        self.apply_rules(&raw);

        // Pohon proses foreground.
        let fg = self.sys.foreground();
        let fg_pid = fg.map(|f| f.pid).unwrap_or(0);
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for p in &raw {
            if p.pid != p.ppid {
                children.entry(p.ppid).or_default().push(p.pid);
            }
        }
        let mut fg_tree: HashSet<u32> = HashSet::new();
        if fg_pid != 0 {
            fg_tree.insert(fg_pid);
            if self.cfg.balancer.exclude_foreground_children {
                let mut stack = vec![fg_pid];
                while let Some(pid) = stack.pop() {
                    if let Some(c) = children.get(&pid) {
                        for &c in c {
                            if fg_tree.insert(c) {
                                stack.push(c);
                            }
                        }
                    }
                }
            }
        }

        self.update_game_mode(&raw, fg, &cpu);
        let game_pid = self.game.as_ref().and_then(|g| g.info.pid);
        self.game_maintenance(&raw, &fg_tree, game_pid);
        self.discord_maintenance(&raw, fg_pid);
        let discord_tamed = self.discord.values().any(|d| d.tamed);

        // Penyeimbang CPU.
        let cfgb = self.cfg.balancer.clone();
        let idle_pause = cfgb.pause_when_idle && self.sys.idle_ms() >= cfgb.idle_ms;
        let active = cfgb.enabled && !idle_pause;
        let aggressive = self.game.is_some() && self.cfg.game.aggressive_balancer;
        let th = balancer::effective_thresholds(
            &cfgb,
            self.topology.logical,
            self.topology.ecore_sets.len() as u32,
            aggressive,
        );
        let mut samples = Vec::with_capacity(raw.len());
        for p in &raw {
            if p.pid <= 4 || p.pid == self_pid {
                continue;
            }
            let key = key_of(p);
            let Some(k) = self.known.get(&key) else { continue };
            let protected = (cfgb.exclude_services && p.session == 0)
                || (cfgb.exclude_foreground && fg_tree.contains(&p.pid))
                || Some(p.pid) == game_pid
                || (discord_tamed && discord::is_discord(&k.name_lc))
                || CRITICAL.contains(&k.name_lc.as_str())
                || matcher::matches_any(&cfgb.exclusions, &k.name_lc, &k.path_lc)
                || matcher::matches_any(&self.cfg.game.games, &k.name_lc, &k.path_lc)
                || self.cfg.rule_for(&k.name_lc, &k.path_lc).map(|r| r.never_restrain).unwrap_or(false);
            samples.push(Sample {
                key,
                cpu: cpu.get(&key).copied().unwrap_or(0.0),
                priority: Priority::from_base_priority(p.base_priority),
                protected,
            });
        }
        let actions = if capacity > 0 {
            self.balancer.tick(now_ms, total_cpu, &samples, &cfgb, &th, active)
        } else {
            Vec::new()
        };
        for a in actions {
            self.apply_action(a, &raw, &cpu);
        }

        self.history.push_back(total_cpu);
        while self.history.len() > 120 {
            self.history.pop_front();
        }
        self.publish(&raw, &cpu, total_cpu, th, active, game_pid);
    }

    fn apply_rules(&mut self, raw: &[RawProc]) {
        let recheck = self.tick_count.is_multiple_of(10);
        let mut logs = Vec::new();
        for p in raw {
            if p.pid <= 4 {
                continue;
            }
            let key = key_of(p);
            let restrained = self.balancer.is_restrained(&key);
            let Some(k) = self.known.get_mut(&key) else { continue };
            let first = !k.rule_checked;
            if !first && !(recheck && k.has_rule) {
                continue;
            }
            k.rule_checked = true;
            let Some(rule) = self.cfg.rule_for(&k.name_lc, &k.path_lc).cloned() else {
                k.has_rule = false;
                continue;
            };
            k.has_rule = true;
            let name = p.name.clone();
            if let Some(pr) = rule.priority {
                let current = Priority::from_base_priority(p.base_priority);
                if current != pr && !restrained {
                    let ok = self.sys.set_priority(p.pid, pr);
                    logs.push((ok, format!("Aturan: {name} → prioritas {}", pr.label())));
                }
            }
            if first {
                if let Some(io) = rule.io_priority {
                    let ok = self.sys.set_io_priority(p.pid, io);
                    logs.push((ok, format!("Aturan: {name} → I/O {}", io.label())));
                }
                if let Some(g) = rule.gpu_priority {
                    let ok = self.sys.set_gpu_priority(p.pid, g);
                    logs.push((ok, format!("Aturan: {name} → GPU {}", g.label())));
                }
                if let Some(mask) = rule.affinity {
                    let ok = self.sys.set_affinity(p.pid, mask);
                    logs.push((ok, format!("Aturan: {name} → affinity {mask:#x}")));
                }
                if let Some(e) = rule.efficiency_mode {
                    let ok = self.sys.set_efficiency(p.pid, Some(e));
                    let state = if e { "nyala" } else { "mati" };
                    logs.push((ok, format!("Aturan: {name} → Efficiency Mode {state}")));
                }
            }
        }
        for (ok, text) in logs {
            self.log_result(ok, text);
        }
    }

    fn detect_game(&mut self, raw: &[RawProc], fg: Option<crate::sys::Foreground>, cpu: &HashMap<ProcKey, f32>) -> Option<(ProcKey, String)> {
        let gc = &self.cfg.game;
        let self_pid = self.sys.self_pid();
        let fg_pid = fg.map(|f| f.pid).unwrap_or(0);
        let fullscreen = fg.map(|f| f.fullscreen).unwrap_or(false);
        let mut best: Option<(u8, f32, ProcKey, String)> = None;
        for p in raw {
            if p.pid <= 4 || p.pid == self_pid || p.session == 0 {
                continue;
            }
            let key = key_of(p);
            let Some(k) = self.known.get_mut(&key) else { continue };
            let is_fg = p.pid == fg_pid;
            if is_fg && fullscreen {
                k.fullscreen_ticks += 1;
            } else {
                k.fullscreen_ticks = 0;
            }
            let listed = matcher::matches_any(&gc.games, &k.name_lc, &k.path_lc);
            let not_game = NOT_GAMES.iter().any(|n| matcher::wildcard(n, &k.name_lc))
                || CRITICAL.contains(&k.name_lc.as_str());
            let store = gc.detect_store_folders
                && is_fg
                && !not_game
                && STORE_FOLDERS.iter().any(|f| matcher::wildcard(f, &k.path_lc));
            let full = gc.detect_fullscreen
                && is_fg
                && k.fullscreen_ticks >= 3
                && !not_game
                && k.name_lc != "explorer.exe"
                && !matcher::matches_any(&gc.fullscreen_exclusions, &k.name_lc, &k.path_lc);
            let rank = if listed { 3 } else if store { 2 } else if full { 1 } else { 0 };
            if rank == 0 {
                continue;
            }
            let rank = rank * 2 + is_fg as u8;
            let c = cpu.get(&key).copied().unwrap_or(0.0);
            if best.as_ref().map(|b| (rank, c) > (b.0, b.1)).unwrap_or(true) {
                best = Some((rank, c, key, p.name.clone()));
            }
        }
        best.map(|b| (b.2, b.3))
    }

    fn update_game_mode(&mut self, raw: &[RawProc], fg: Option<crate::sys::Foreground>, cpu: &HashMap<ProcKey, f32>) {
        let alive: HashSet<ProcKey> = raw.iter().map(key_of).collect();
        // Game yang sedang berjalan tetap dipegang walau pemain alt-tab.
        let current = self.game.as_ref().and_then(|g| g.game_key).filter(|k| alive.contains(k));
        let detected = match current {
            Some(k) => Some((k, self.game.as_ref().unwrap().info.name.clone())),
            None => self.detect_game(raw, fg, cpu),
        };
        let want = match self.forced {
            Some(f) => f,
            None => self.cfg.game.auto && detected.is_some(),
        };

        if !want {
            if self.game.is_some() {
                self.exit_game_mode();
            }
            return;
        }
        if self.game.is_none() {
            self.enter_game_mode();
        }
        let gm = self.game.as_ref().unwrap();
        if let Some((key, name)) = detected {
            if gm.game_key != Some(key) {
                self.attach_game(key, name);
            }
        } else if gm.game_key.is_some() {
            let gm = self.game.as_mut().unwrap();
            gm.game_key = None;
            gm.info.pid = None;
            gm.info.name = "Manual".into();
        }
    }

    fn enter_game_mode(&mut self) {
        let gc = self.cfg.game.clone();
        let mut gm = GameMode {
            info: GameInfo { name: "Manual".into(), pid: None, since: Instant::now(), forced: self.forced == Some(true) },
            game_key: None,
            game_priority_set: None,
            original_plan: None,
            plan_set: None,
            frozen: HashMap::new(),
            last_standby: Instant::now(),
        };
        self.log(LogKind::Game, "Mode Game NYALA.".into());

        let target = match &gc.power_plan {
            PowerPlanChoice::Keep => None,
            PowerPlanChoice::Plan(g) => Some(g.clone()),
            PowerPlanChoice::Auto => self.sys.ensure_performance_plan().or_else(|| {
                let plans = self.sys.power_plans();
                [ULTIMATE_PERFORMANCE, HIGH_PERFORMANCE]
                    .iter()
                    .find(|g| plans.iter().any(|p| p.guid.eq_ignore_ascii_case(g)))
                    .map(|g| g.to_string())
            }),
        };
        if let Some(target) = target {
            let original = self.sys.active_power_plan();
            if original.as_deref().map(|o| !o.eq_ignore_ascii_case(&target)).unwrap_or(true) {
                if self.sys.set_power_plan(&target) {
                    let name = self.plan_name(&target);
                    self.log(LogKind::Game, format!("Power plan diganti ke {name}."));
                    gm.original_plan = original;
                    gm.plan_set = Some(target);
                } else {
                    self.log(LogKind::Warn, "Gagal mengganti power plan.".into());
                }
            }
        }
        if gc.timer_resolution {
            if let Some(ms) = self.sys.set_timer_resolution(true) {
                self.timer_ms = Some(ms);
                self.log(LogKind::Game, format!("Resolusi timer sistem {ms:.2} ms."));
            }
        }
        if gc.prevent_sleep {
            self.sys.prevent_sleep(true);
        }
        if gc.clear_standby {
            let mem = self.sys.memory();
            if mem.avail_mb < gc.standby_free_mb_threshold {
                let ok = self.sys.purge_standby();
                self.log_result(ok, format!("Membersihkan RAM standby (RAM bebas {} MB)", mem.avail_mb));
            }
        }
        self.game = Some(gm);
    }

    fn attach_game(&mut self, key: ProcKey, name: String) {
        let gc = self.cfg.game.clone();
        self.restore_game_process();
        if let Some(gm) = self.game.as_mut() {
            gm.game_key = Some(key);
            gm.info.pid = Some(key.pid);
            gm.info.name = name.clone();
            gm.game_priority_set = None;
        }
        self.log(LogKind::Game, format!("Game terdeteksi: {name} (PID {}).", key.pid));
        let pid = key.pid;
        let mut done = Vec::new();
        if let Some(p) = gc.game_priority {
            if self.sys.set_priority(pid, p) {
                if let Some(gm) = self.game.as_mut() {
                    gm.game_priority_set = Some(p);
                }
                done.push(format!("CPU {}", p.label()));
            }
        }
        if let Some(p) = gc.game_io_priority {
            if self.sys.set_io_priority(pid, p) {
                done.push(format!("I/O {}", p.label()));
            }
        }
        if let Some(p) = gc.game_gpu_priority {
            if self.sys.set_gpu_priority(pid, p) {
                done.push(format!("GPU {}", p.label()));
            }
        }
        if gc.disable_efficiency_on_game && self.sys.set_efficiency(pid, Some(false)) {
            done.push("Efficiency Mode mati".into());
        }
        if gc.game_on_pcores && self.topology.hybrid() && self.sys.set_cpu_sets(pid, &self.topology.pcore_sets) {
            done.push("hanya P-core".into());
        }
        if done.is_empty() {
            self.log(LogKind::Warn, format!("Tidak bisa mengatur {name}; mungkin dilindungi anti-cheat."));
        } else {
            self.log(LogKind::Game, format!("{name} dioptimalkan: {}.", done.join(", ")));
        }
    }

    fn restore_game_process(&mut self) {
        let Some(gm) = self.game.as_ref() else { return };
        let Some(key) = gm.game_key else { return };
        if let Some(set) = gm.game_priority_set {
            if self.sys.get_priority(key.pid) == Some(set) {
                self.sys.set_priority(key.pid, Priority::Normal);
            }
        }
        if self.cfg.game.game_on_pcores && self.topology.hybrid() {
            self.sys.set_cpu_sets(key.pid, &[]);
        }
    }

    /// Tugas berkala selama Mode Game: membekukan aplikasi latar dan membersihkan RAM standby.
    fn game_maintenance(&mut self, raw: &[RawProc], fg_tree: &HashSet<u32>, game_pid: Option<u32>) {
        if self.game.is_none() {
            return;
        }
        let gc = self.cfg.game.clone();

        if gc.freeze_enabled && !gc.freeze_apps.is_empty() {
            // Jangan bekukan game itu sendiri, induknya (mis. launcher), atau aplikasi di depan.
            let parents: HashMap<u32, u32> = raw.iter().map(|p| (p.pid, p.ppid)).collect();
            let mut protected: HashSet<u32> = fg_tree.clone();
            if let Some(mut pid) = game_pid {
                for _ in 0..16 {
                    if !protected.insert(pid) {
                        break;
                    }
                    match parents.get(&pid) {
                        Some(&pp) if pp > 4 && pp != pid => pid = pp,
                        _ => break,
                    }
                }
            }
            let self_pid = self.sys.self_pid();
            let mut newly = Vec::new();
            for p in raw {
                if p.pid <= 4 || p.pid == self_pid || protected.contains(&p.pid) {
                    continue;
                }
                let key = key_of(p);
                if self.game.as_ref().unwrap().frozen.contains_key(&key) {
                    continue;
                }
                let Some(k) = self.known.get(&key) else { continue };
                if matcher::matches_any(&gc.freeze_apps, &k.name_lc, &k.path_lc) && self.sys.suspend(p.pid) {
                    newly.push((key, p.name.clone()));
                }
            }
            if !newly.is_empty() {
                let mut names: Vec<String> = newly.iter().map(|n| n.1.clone()).collect();
                names.sort();
                names.dedup();
                self.game.as_mut().unwrap().frozen.extend(newly);
                self.log(LogKind::Game, format!("Membekukan aplikasi latar: {}.", names.join(", ")));
            }
            // Aplikasi yang kemudian dibawa ke depan oleh pemain dijalankan lagi.
            let thawed: Vec<(ProcKey, String)> = self
                .game
                .as_ref()
                .unwrap()
                .frozen
                .iter()
                .filter(|(k, _)| fg_tree.contains(&k.pid))
                .map(|(k, n)| (*k, n.clone()))
                .collect();
            for (k, name) in thawed {
                self.sys.resume(k.pid);
                self.game.as_mut().unwrap().frozen.remove(&k);
                self.log(LogKind::Game, format!("{name} dijalankan lagi karena dibuka."));
            }
        } else if !self.game.as_ref().unwrap().frozen.is_empty() {
            let frozen = std::mem::take(&mut self.game.as_mut().unwrap().frozen);
            for k in frozen.keys() {
                self.sys.resume(k.pid);
            }
        }

        if gc.clear_standby && gc.standby_periodic {
            let due = self.game.as_ref().unwrap().last_standby.elapsed().as_secs() >= gc.standby_check_secs.max(10);
            if due {
                self.game.as_mut().unwrap().last_standby = Instant::now();
                let mem = self.sys.memory();
                if mem.avail_mb < gc.standby_free_mb_threshold {
                    let ok = self.sys.purge_standby();
                    self.log_result(ok, format!("Membersihkan RAM standby (RAM bebas {} MB)", mem.avail_mb));
                }
            }
        }
    }

    fn exit_game_mode(&mut self) {
        self.restore_game_process();
        let Some(gm) = self.game.take() else { return };
        if !gm.frozen.is_empty() {
            let mut names: Vec<String> = Vec::new();
            for (key, name) in &gm.frozen {
                if self.sys.resume(key.pid) {
                    names.push(name.clone());
                }
            }
            names.sort();
            names.dedup();
            self.log(LogKind::Game, format!("Aplikasi latar dijalankan lagi: {}.", names.join(", ")));
        }
        if let (Some(orig), Some(set)) = (gm.original_plan, gm.plan_set) {
            // Kembalikan hanya bila power plan belum diganti pengguna.
            if self.sys.active_power_plan().map(|a| a.eq_ignore_ascii_case(&set)).unwrap_or(false) {
                self.sys.set_power_plan(&orig);
                let name = self.plan_name(&orig);
                self.log(LogKind::Game, format!("Power plan dikembalikan ke {name}."));
            }
        }
        if self.sys.set_timer_resolution(false).is_some() {
            self.timer_ms = None;
        }
        self.sys.prevent_sleep(false);
        let mins = gm.info.since.elapsed().as_secs() / 60;
        self.log(LogKind::Game, format!("Mode Game MATI setelah {mins} menit."));
    }

    fn plan_name(&self, guid: &str) -> String {
        self.sys
            .power_plans()
            .into_iter()
            .find(|p| p.guid.eq_ignore_ascii_case(guid))
            .map(|p| p.name)
            .unwrap_or_else(|| guid.to_string())
    }

    fn apply_action(&mut self, a: Action, raw: &[RawProc], cpu: &HashMap<ProcKey, f32>) {
        let name_of = |key: &ProcKey| {
            raw.iter().find(|p| key_of(p) == *key).map(|p| p.name.clone()).unwrap_or_default()
        };
        let cfgb = self.cfg.balancer.clone();
        match a {
            Action::Restrain { key, to, .. } => {
                let name = name_of(&key);
                let mut ap = Applied::default();
                if let Some(to) = to {
                    self.sys.set_priority(key.pid, to);
                }
                if cfgb.lower_io_priority {
                    ap.io = self.sys.set_io_priority(key.pid, IoPriority::Low);
                }
                if cfgb.use_efficiency_mode {
                    ap.efficiency = self.sys.set_efficiency(key.pid, Some(true));
                }
                if cfgb.move_to_ecores && self.topology.hybrid() {
                    ap.cpu_sets = self.sys.set_cpu_sets(key.pid, &self.topology.ecore_sets);
                }
                self.applied.insert(key, ap);
                let c = cpu.get(&key).copied().unwrap_or(0.0);
                let mut what = vec![];
                if let Some(to) = to {
                    what.push(format!("prioritas {}", to.label()));
                }
                if self.applied.get(&key).map(|a| a.cpu_sets).unwrap_or(false) {
                    what.push("dipindah ke E-core".into());
                }
                self.log(LogKind::Restrain, format!("Meredam {name} (CPU {c:.0}%): {}.", what.join(", ")));
            }
            Action::Restore { key, original, restrained, reason } => {
                let name = name_of(&key);
                if let Some(r) = restrained {
                    if self.sys.get_priority(key.pid) == Some(r) {
                        self.sys.set_priority(key.pid, original);
                    }
                }
                if let Some(ap) = self.applied.remove(&key) {
                    if ap.io {
                        self.sys.set_io_priority(key.pid, IoPriority::Normal);
                    }
                    if ap.efficiency {
                        self.sys.set_efficiency(key.pid, None);
                    }
                    if ap.cpu_sets {
                        self.sys.set_cpu_sets(key.pid, &[]);
                    }
                }
                if !name.is_empty() {
                    self.log(LogKind::Restore, format!("Memulihkan {name}: {}.", reason.label()));
                }
            }
        }
    }

    fn publish(
        &mut self,
        raw: &[RawProc],
        cpu: &HashMap<ProcKey, f32>,
        total_cpu: f32,
        th: balancer::Thresholds,
        active: bool,
        game_pid: Option<u32>,
    ) {
        let mut procs = Vec::with_capacity(raw.len());
        for p in raw {
            if p.pid == 0 {
                continue;
            }
            let key = key_of(p);
            let k = self.known.get(&key);
            let status = if Some(p.pid) == game_pid {
                Status::Game
            } else if self.balancer.is_restrained(&key) {
                Status::Restrained
            } else if k.map(|k| k.has_rule).unwrap_or(false) {
                Status::Ruled
            } else {
                Status::Normal
            };
            procs.push(ProcView {
                pid: p.pid,
                name: p.name.clone(),
                path: k.map(|k| k.path.clone()).unwrap_or_default(),
                cpu: cpu.get(&key).copied().unwrap_or(0.0),
                mem_mb: p.working_set as f32 / 1_048_576.0,
                priority: Priority::from_base_priority(p.base_priority),
                threads: p.threads,
                session: p.session,
                status,
            });
        }
        let refresh_plans = self.tick_count % 30 == 1;
        let mut s = self.shared.write();
        s.procs = procs;
        s.total_cpu = total_cpu;
        s.cpu_history = self.history.clone();
        s.mem = self.sys.memory();
        s.game = self.game.as_ref().map(|g| g.info.clone());
        s.forced = self.forced;
        s.restrained_now = self.balancer.restrained().count();
        s.restraints_total = self.balancer.total_restraints;
        s.topology = self.topology.clone();
        if refresh_plans || s.power_plans.is_empty() {
            s.power_plans = self.sys.power_plans();
        }
        s.active_plan = self.sys.active_power_plan().unwrap_or_default();
        s.timer_ms = self.timer_ms;
        s.thresholds = Some(th);
        s.log = self.log.clone();
        s.balancer_active = active;
        s.frozen = self
            .game
            .as_ref()
            .map(|g| {
                let mut v: Vec<String> = g.frozen.values().cloned().collect();
                v.sort();
                v.dedup();
                v
            })
            .unwrap_or_default();
        s.last_trim = self.last_trim.clone();
        s.discord = DiscordStat {
            procs: 0,
            mem_mb: 0.0,
            tamed: self.discord.values().any(|d| d.tamed),
            capped: self.discord.values().any(|d| d.capped.is_some()),
            last: self.discord_last_trim.clone(),
        };
        for p in raw {
            if discord::is_discord(&p.name.to_lowercase()) {
                s.discord.procs += 1;
                s.discord.mem_mb += (p.working_set >> 20) as f32;
            }
        }
        s.version = self.tick_count;
    }

    fn shutdown(&mut self) {
        let raw = self.sys.processes();
        let cpu = HashMap::new();
        let cfgb = self.cfg.balancer.clone();
        let th = balancer::auto_thresholds(self.topology.logical, 0);
        let samples: Vec<Sample> = raw
            .iter()
            .map(|p| Sample { key: key_of(p), cpu: 0.0, priority: Priority::Normal, protected: false })
            .collect();
        for a in self.balancer.tick(0, 0.0, &samples, &cfgb, &th, false) {
            self.apply_action(a, &raw, &cpu);
        }
        if self.game.is_some() {
            self.exit_game_mode();
        }
        for (key, d) in std::mem::take(&mut self.discord) {
            if d.tamed {
                self.sys.set_priority(key.pid, Priority::Normal);
                self.sys.set_io_priority(key.pid, IoPriority::Normal);
                self.sys.set_efficiency(key.pid, None);
            }
            if d.capped.is_some() {
                self.sys.limit_working_set(key.pid, None);
            }
        }
        self.log(LogKind::Info, "Pacu berhenti; semua perubahan sementara dikembalikan.".into());
    }
}

fn key_of(p: &RawProc) -> ProcKey {
    ProcKey { pid: p.pid, create_time: p.create_time }
}

fn open_log_file() -> Option<std::fs::File> {
    let dir = crate::config::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("pacu.log");
    if std::fs::metadata(&path).map(|m| m.len() > 1 << 20).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("pacu.old.log"));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
}
