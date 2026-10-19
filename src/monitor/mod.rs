//! Monitor sistem: FPS, CPU, GPU, VRAM dan RAM di satu tempat, plus overlay di atas game.
//!
//! Berjalan di thread sendiri (dua kali per detik) supaya overlay tetap diperbarui walaupun
//! jendela Pacu tersembunyi di tray.

pub mod cpu;
pub mod osd;
pub mod overlay;
pub mod rtss;
pub mod sensors;

use crate::config::{OverlayConfig, OverlayMode};
use crate::engine::Snapshot;
use crate::fps::FpsMeter;
use crate::volt::gpu::Telemetry;
use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const TICK_MS: u64 = 500;
/// Riwayat 2 menit.
pub const HISTORY: usize = 240;

#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub cpu_pct: f32,
    pub cpu: cpu::CpuReading,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpu: Telemetry,
    pub fps: Option<f32>,
    pub low1: Option<f32>,
    pub frametime_ms: Option<f32>,
    /// Aplikasi yang FPS-nya diukur.
    pub fps_app: Option<String>,
    /// "PresentMon" atau "RTSS".
    pub fps_source: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub fps: Option<f32>,
    pub cpu_pct: f32,
    pub cpu_temp: Option<f32>,
    pub gpu_pct: Option<f32>,
    pub gpu_temp: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum OverlayState {
    #[default]
    Off,
    /// Menyala, menunggu game.
    Waiting,
    Rtss,
    Window,
    HiddenByHotkey,
    /// Mode RTSS dipilih tetapi RTSS tidak berjalan / slot penuh.
    RtssMissing,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub now: Reading,
    pub history: VecDeque<Point>,
    pub cpu_name: String,
    pub gpu_name: String,
    pub cpu_note: Option<String>,
    pub pawnio_missing: bool,
    pub rtss_running: bool,
    pub overlay: OverlayState,
    /// Hasil percobaan terakhir menjalankan RTSS otomatis.
    pub rtss_launch: Option<String>,
    /// Tabel sensor dengan min / maks / rata-rata sesi.
    pub sensors: sensors::Sensors,
}

pub struct Monitor {
    pub view: Arc<RwLock<View>>,
    pub fps: Arc<Mutex<FpsMeter>>,
    pub overlay_cfg: Arc<RwLock<OverlayConfig>>,
    /// Pengukur FPS ikut Mode Game (pengaturan "Ukur FPS").
    pub fps_enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Monitor {
    pub fn start(engine: Arc<RwLock<Snapshot>>, volt: Arc<crate::volt::tuner::Shared>, overlay_cfg: OverlayConfig, fps_enabled: bool) -> Monitor {
        let m = Monitor {
            view: Arc::new(RwLock::new(View::default())),
            fps: Arc::new(Mutex::new(FpsMeter::default())),
            overlay_cfg: Arc::new(RwLock::new(overlay_cfg)),
            fps_enabled: Arc::new(AtomicBool::new(fps_enabled)),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        };
        let (view, fps, cfg, fps_on, stop) = (m.view.clone(), m.fps.clone(), m.overlay_cfg.clone(), m.fps_enabled.clone(), m.stop.clone());
        let thread = std::thread::Builder::new()
            .name("pacu-monitor".into())
            .spawn(move || run(engine, volt, view, fps, cfg, fps_on, stop))
            .expect("thread monitor");
        Monitor { thread: Some(thread), ..m }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    engine: Arc<RwLock<Snapshot>>,
    volt: Arc<crate::volt::tuner::Shared>,
    view: Arc<RwLock<View>>,
    fps: Arc<Mutex<FpsMeter>>,
    cfg: Arc<RwLock<OverlayConfig>>,
    fps_on: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    let mut cpu = cpu::CpuSensors::new();
    {
        let mut v = view.write();
        v.cpu_name = cpu.name();
        v.cpu_note = cpu.note();
        v.pawnio_missing = cpu.pawnio_missing();
    }
    let overlay = overlay::Overlay::new();
    let mut rtss_owned = false;
    let mut rtss_tried: Option<std::time::Instant> = None;
    let mut tick = 0u64;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(TICK_MS));
        tick += 1;
        // PawnIO mungkin baru dipasang dari halaman Monitor: coba lagi tiap 30 detik.
        if tick % 60 == 0 && cpu.pawnio_missing() {
            cpu = cpu::CpuSensors::new();
            let mut v = view.write();
            v.cpu_note = cpu.note();
            v.pawnio_missing = cpu.pawnio_missing();
        }
        let (cpu_pct, mem, game) = {
            let s = engine.read();
            (s.total_cpu, s.mem.clone(), s.game.clone())
        };

        // FPS: PresentMon untuk game yang dideteksi Mode Game, RTSS untuk aplikasi 3D lain.
        let want = game.as_ref().filter(|g| g.pid.is_some() && fps_on.load(Ordering::Relaxed)).map(|g| g.name.clone());
        let live = {
            let mut m = fps.lock();
            match &want {
                Some(exe) => m.start(exe),
                None if m.running_for().is_some() => m.stop(),
                None => {}
            }
            let st = m.stats.lock();
            st.live().map(|(avg, low)| (avg, low, st.game.clone()))
        };
        let rtss_running = rtss::running();
        let rtss_app = if rtss_running { rtss::app(game.as_ref().and_then(|g| g.pid).unwrap_or(0)).or_else(|| rtss::app(0)) } else { None };

        let mut r = Reading {
            cpu_pct,
            ram_total_mb: mem.total_mb,
            ram_used_mb: mem.total_mb.saturating_sub(mem.avail_mb),
            gpu: volt.view.read().telemetry.clone(),
            ..Default::default()
        };
        r.cpu = cpu.read(cpu_pct);
        if let Some((avg, low, name)) = live {
            r.fps = Some(avg);
            r.low1 = Some(low);
            r.frametime_ms = Some(1000.0 / avg);
            r.fps_app = Some(name);
            r.fps_source = Some("PresentMon");
        } else if let Some(a) = &rtss_app {
            r.fps = Some(a.fps);
            r.frametime_ms = a.frametime_ms.or(Some(1000.0 / a.fps));
            r.fps_app = Some(a.name.clone());
            r.fps_source = Some("RTSS");
        }

        // Overlay.
        let c = cfg.read().clone();
        // RTSS harus sudah berjalan sebelum game dibuka supaya bisa menggambar OSD di dalam game.
        if c.enabled && c.launch_rtss && c.mode != OverlayMode::Window && !rtss_running && rtss_tried.map(|t| t.elapsed().as_secs() >= 120).unwrap_or(true) {
            rtss_tried = Some(std::time::Instant::now());
            let msg = match rtss::launch() {
                Ok(exe) => format!("RTSS dijalankan otomatis ({})", exe.display()),
                Err(e) => e,
            };
            view.write().rtss_launch = Some(msg);
        }
        let in_game = game.is_some() || rtss_app.is_some() || r.fps.is_some();
        let state = if !c.enabled {
            OverlayState::Off
        } else if overlay.hidden_by_user() {
            OverlayState::HiddenByHotkey
        } else {
            match c.mode {
                OverlayMode::Rtss | OverlayMode::Auto if rtss_running => OverlayState::Rtss,
                OverlayMode::Rtss => OverlayState::RtssMissing,
                _ if c.always || in_game => OverlayState::Window,
                _ => OverlayState::Waiting,
            }
        };
        let state = if state == OverlayState::Rtss {
            if rtss::show(&osd::rtss_text(&osd::lines(&r, &c, true))) {
                rtss_owned = true;
                OverlayState::Rtss
            } else {
                OverlayState::RtssMissing
            }
        } else {
            if rtss_owned {
                rtss::release();
                rtss_owned = false;
            }
            state
        };
        overlay.update(osd::lines(&r, &c, false), c.corner, c.scale, state == OverlayState::Window);

        let p = Point {
            fps: r.fps,
            cpu_pct: r.cpu_pct,
            cpu_temp: r.cpu.temp_c,
            gpu_pct: r.gpu.util_pct.map(|x| x as f32),
            gpu_temp: r.gpu.temp_c.map(|x| x as f32),
        };
        let gpu_name = volt.view.read().info.name.clone();
        let mut v = view.write();
        v.sensors.update(&r);
        v.now = r;
        v.history.push_back(p);
        while v.history.len() > HISTORY {
            v.history.pop_front();
        }
        v.gpu_name = gpu_name;
        v.rtss_running = rtss_running;
        v.overlay = state;
    }
    if rtss_owned {
        rtss::release();
    }
    overlay.update(vec![], crate::config::Corner::TopLeft, 1.0, false);
    fps.lock().stop();
}
