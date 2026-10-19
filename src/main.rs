#![cfg_attr(windows, windows_subsystem = "windows")]

use pacu::config::Config;
use pacu::engine::{Command, Engine, Snapshot};
use pacu::sys::{self, NativeSys};
use pacu::ui::{Extras, PacuApp, UiEvent, UiHooks};
use parking_lot::RwLock;
use std::sync::{mpsc, Arc};

#[cfg(windows)]
mod tray;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    // Mode tanpa jendela untuk undervolt GPU (dipakai Task Scheduler).
    if has("--volt-apply") || has("--apply") {
        std::process::exit(pacu::volt::apply_profile());
    }
    if has("--volt-reset") {
        std::process::exit(pacu::volt::reset_curve());
    }
    #[cfg(windows)]
    if has("--selftest-checker") {
        use pacu::volt::rig::Checker;
        let mut c = pacu::volt::checker::D3dChecker::new();
        let r = c.prepare();
        c.start();
        std::thread::sleep(std::time::Duration::from_secs(5));
        let st = c.take();
        c.stop();
        let s = pacu::volt::store::stamp();
        pacu::volt::store::append_log(&format!("{s} selftest pemeriksa: prepare {r:?}, {} putaran, {} error, hilang {}", st.runs, st.errors, st.device_lost));
        std::process::exit(0);
    }

    // Dibuka oleh pembaruan: tunggu proses lama keluar dulu.
    if let Some(pid) = args.iter().position(|a| a == "--wait-pid").and_then(|i| args.get(i + 1)).and_then(|p| p.parse().ok()) {
        pacu::update::wait_for_pid(pid);
    }
    let Some(instance) = sys::instance::acquire() else {
        // Pacu sudah berjalan; jendelanya sudah diminta tampil.
        return;
    };
    let minimized = std::env::args().any(|a| a == "--minimized");
    let mut cfg = Config::load();
    if minimized {
        cfg.general.start_minimized = true;
    }

    pacu::update::cleanup_old();
    if !cfg.wake.blocked.is_empty() {
        let blocked = cfg.wake.blocked.clone();
        std::thread::spawn(move || pacu::wake::enforce(&blocked));
    }

    let shared = Arc::new(RwLock::new(Snapshot::default()));
    let (tx, rx) = mpsc::channel::<Command>();
    let engine = Engine::new(NativeSys::new(), cfg.clone(), shared.clone());
    let worker = std::thread::Builder::new()
        .name("pacu-engine".into())
        .spawn(move || engine.run(rx))
        .expect("thread mesin");

    // Undervolt GPU (Pacu Volt) dan monitor sensor/overlay, masing-masing di thread sendiri.
    let mut volt = pacu::volt::Volt::start(has("--sim-volt"));
    pacu::volt::autostart::refresh();
    let mut monitor = pacu::monitor::Monitor::start(shared.clone(), volt.shared.clone(), cfg.overlay.clone(), cfg.game.fps_meter);
    let extras = Extras {
        volt: pacu::volt::panel::VoltPanel::new(volt.shared.clone(), volt.tx.clone(), volt.pending.take()),
        mon: monitor.view.clone(),
        fps: monitor.fps.clone(),
        overlay_cfg: monitor.overlay_cfg.clone(),
        fps_enabled: monitor.fps_enabled.clone(),
    };

    let icon = egui_icon();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Pacu")
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([920.0, 620.0])
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };

    let tx_app = tx.clone();
    let start_hidden = cfg.general.start_minimized;
    let result = eframe::run_native(
        "Pacu",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let (ev_tx, ev_rx) = mpsc::channel::<UiEvent>();

            #[cfg(windows)]
            let tray = tray::create(ev_tx.clone(), ctx.clone());
            #[cfg(windows)]
            let has_tray = tray.is_some();
            #[cfg(not(windows))]
            let has_tray = false;

            if start_hidden && has_tray {
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Visible(false));
            }
            let _ = &ev_tx;
            let hooks = UiHooks {
                poll: Box::new(move || {
                    #[cfg(windows)]
                    let _keep_tray_alive = &tray;
                    let mut out: Vec<UiEvent> = ev_rx.try_iter().collect();
                    if instance.show_requested() {
                        out.push(UiEvent::Show);
                    }
                    out
                }),
                tray: has_tray,
            };
            Ok(Box::new(PacuApp::new(cc, shared, tx_app, cfg, hooks, extras)))
        }),
    );
    monitor.stop();
    volt.stop();
    let _ = tx.send(Command::Quit);
    let _ = worker.join();
    if pacu::update::RESTART.get().is_some() {
        pacu::update::spawn_new_instance();
    }
    if let Err(e) = result {
        eprintln!("Pacu gagal membuka jendela: {e}");
    }
}

fn egui_icon() -> eframe::egui::IconData {
    let size = 64;
    eframe::egui::IconData { rgba: pacu::icon::icon_rgba(size), width: size, height: size }
}
