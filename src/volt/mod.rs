//! Pacu Volt: pencari titik undervolt GPU NVIDIA otomatis, digabung ke dalam Pacu.
//! Data tetap disimpan di `%APPDATA%\PacuVolt` supaya profil dari Pacu Volt versi lama terbawa.

pub mod autostart;
pub mod checker;
pub mod curve;
pub mod gpu;
pub mod panel;
pub mod plan;
pub mod presets;
pub mod rig;
pub mod store;
pub mod tuner;
pub mod watchdog;
pub mod vram;

use gpu::sim;
use rig::{ExternalLoad, Load, ProcessLoad, Rig};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use store::{Profile, Settings};
use tuner::{Cmd, Shared};

/// Pekerja undervolt yang berjalan di thread sendiri.
pub struct Volt {
    pub shared: Arc<Shared>,
    pub tx: Sender<Cmd>,
    pub pending: Option<store::Session>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Volt {
    /// `simulate`: pakai GPU simulasi (selalu di luar Windows).
    pub fn start(simulate: bool) -> Volt {
        let simulate = simulate || !cfg!(windows);
        let (rig, world) = if simulate {
            let (r, w) = sim::rig(store::now_secs(), 150.0, 30.0);
            (r, Some(w))
        } else {
            (native_rig(), None)
        };
        let make_load = move |s: &Settings| -> Box<dyn Load> {
            if let Some(w) = &world {
                return Box::new(sim::SimLoad(w.clone()));
            }
            if s.load_exe.trim().is_empty() {
                Box::new(ExternalLoad)
            } else {
                Box::new(ProcessLoad::new(s.load_exe.trim(), &s.load_args))
            }
        };
        let pending = tuner::recover_crashed_session();
        let shared = Shared::new();
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let sh = shared.clone();
        let worker = std::thread::Builder::new()
            .name("pacu-volt".into())
            .spawn(move || tuner::worker(rig, sh, rx, make_load))
            .expect("thread undervolt");
        Volt { shared, tx, pending, worker: Some(worker) }
    }

    /// Hentikan pekerja. Pencarian yang sedang berjalan dikembalikan ke kurva bawaan.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self.tx.send(Cmd::Quit);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(windows)]
fn native_rig() -> Rig {
    let gpu: Box<dyn gpu::Gpu> = match gpu::open() {
        Ok(g) => g,
        Err(e) => Box::new(gpu::native::DeadGpu(e)),
    };
    Rig {
        gpu,
        load: Box::new(ExternalLoad),
        checker: Box::new(checker::D3dChecker::new()),
        watchdog: Box::new(watchdog::EventLogWatchdog::new()),
        clock: Box::new(rig::RealClock),
    }
}

#[cfg(not(windows))]
fn native_rig() -> Rig {
    sim::rig(1, 150.0, 1.0).0
}

/// Mode tanpa jendela untuk Task Scheduler (`--volt-apply`): pasang profil tersimpan lalu keluar.
pub fn apply_profile() -> i32 {
    let log = |s: String| store::append_log(&format!("{} {s}", store::stamp()));
    if vram::VramProfile::load().is_some() {
        for attempt in 0..6 {
            match vram::apply_saved() {
                Ok(Some(o)) => {
                    log(format!("--volt-apply: offset memori {o:+} MHz dipasang"));
                    break;
                }
                Ok(None) => break,
                Err(e) if attempt == 5 => log(format!("--volt-apply: offset memori gagal: {e}")),
                Err(_) => std::thread::sleep(std::time::Duration::from_secs(5)),
            }
        }
    }
    let Some(p) = Profile::load() else {
        log("--volt-apply: belum ada profil".into());
        return 1;
    };
    // Driver kadang belum siap tepat setelah login.
    for attempt in 0..6 {
        match gpu::open() {
            Ok(mut g) => {
                let name = g.info().name;
                if !p.gpu_name.is_empty() && name != p.gpu_name {
                    log(format!("--volt-apply: GPU berbeda ({name} vs {}), profil tidak dipasang", p.gpu_name));
                    return 2;
                }
                return match g.set_deltas(&p.deltas) {
                    Ok(()) => {
                        log(format!("--volt-apply: profil {} MHz @ {:.0} mV dipasang", p.target_mhz, p.volt_mv));
                        0
                    }
                    Err(e) => {
                        log(format!("--volt-apply: gagal: {e}"));
                        1
                    }
                };
            }
            Err(e) if attempt == 5 => {
                log(format!("--volt-apply: GPU tidak bisa dibuka: {e}"));
                return 1;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_secs(5)),
        }
    }
    1
}

/// `--volt-reset`: kembalikan kurva ke bawaan lalu keluar.
pub fn reset_curve() -> i32 {
    match gpu::open().and_then(|mut g| g.reset()) {
        Ok(()) => 0,
        Err(e) => {
            store::append_log(&format!("{} --volt-reset gagal: {e}", store::stamp()));
            1
        }
    }
}
