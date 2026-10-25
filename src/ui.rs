//! Antarmuka Pacu (egui). Ringan: hanya menggambar ulang sekali per detik atau saat ada input,
//! dan berhenti menggambar saat jendela disembunyikan ke tray.

mod bios_page;
mod monitor_page;
mod ram_page;
mod system_page;
pub(crate) mod theme;

use crate::autostart;
use crate::config::{Config, GpuPriority, IoPriority, PowerPlanChoice, Priority, ProcessRule};
use crate::engine::{Command, LogKind, ProcView, Snapshot, Status};
use crate::fps::{FpsMeter, Stats};
use crate::monitor;
use crate::volt::panel::VoltPanel;
use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;
use crate::{presets, tweaks};
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, Vec2,
};
use egui_extras::{Column, TableBuilder};
use parking_lot::RwLock;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use theme::{accent, accent_button, banner, card, divider, muted, page_header, section, segmented, setting_row, strong, tag, toggle};

const BG: Color32 = theme::BG;
const PANEL: Color32 = theme::LAYER;
const CARD: Color32 = theme::CARD;
const CARD_HI: Color32 = theme::CONTROL;
const CYAN: Color32 = theme::DATA_BLUE;
const GREEN: Color32 = theme::SUCCESS;
const AMBER: Color32 = theme::CAUTION;
const RED: Color32 = theme::CRITICAL;
const TEXT: Color32 = theme::TEXT;
const MUTED: Color32 = theme::TEXT_3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Monitor,
    Game,
    Fps,
    Volt,
    Tweaks,
    Ram,
    Processes,
    Rules,
    Activity,
    Settings,
    Bios,
}

impl Page {
    const ALL: [Page; 12] = [
        Page::Home,
        Page::Monitor,
        Page::Game,
        Page::Fps,
        Page::Volt,
        Page::Tweaks,
        Page::Ram,
        Page::Processes,
        Page::Rules,
        Page::Activity,
        Page::Settings,
        Page::Bios,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Home => "Beranda",
            Page::Game => "Mode Game",
            Page::Monitor => "Monitor & Overlay",
            Page::Fps => "FPS & Benchmark",
            Page::Volt => "Undervolt GPU",
            Page::Tweaks => "Optimasi Windows",
            Page::Ram => "RAM Ekstrem",
            Page::Processes => "Proses",
            Page::Rules => "Aturan",
            Page::Activity => "Aktivitas",
            Page::Settings => "Pengaturan",
            Page::Bios => "BIOS & Hardware",
        }
    }

    fn icon(self) -> &'static str {
        use theme::icon;
        match self {
            Page::Home => icon::HOME,
            Page::Monitor => icon::PULSE,
            Page::Game => icon::GAMES,
            Page::Fps => icon::SPEED,
            Page::Volt => icon::FLASH,
            Page::Tweaks => icon::WRENCH,
            Page::Ram => icon::RAM,
            Page::Processes => icon::APPS,
            Page::Rules => icon::RULES,
            Page::Activity => icon::HISTORY,
            Page::Settings => icon::SETTINGS,
            Page::Bios => icon::BOARD,
        }
    }
}

/// Susunan menu samping: (judul kelompok, halaman). Aktivitas dan Pengaturan ada di bawah.
const NAV: [(&str, &[Page]); 3] = [
    ("", &[Page::Home, Page::Monitor]),
    ("Performa", &[Page::Game, Page::Fps, Page::Volt]),
    ("Sistem", &[Page::Tweaks, Page::Bios, Page::Ram, Page::Processes, Page::Rules]),
];
const NAV_FOOTER: [Page; 2] = [Page::Activity, Page::Settings];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Cpu,
    Mem,
    Name,
}

/// Bagian Pacu yang berjalan di thread lain: monitor/overlay dan undervolt GPU.
pub struct Extras {
    pub volt: VoltPanel,
    pub mon: Arc<RwLock<monitor::View>>,
    pub fps: Arc<Mutex<FpsMeter>>,
    pub overlay_cfg: Arc<RwLock<crate::config::OverlayConfig>>,
    pub fps_enabled: Arc<AtomicBool>,
}

pub struct UiHooks {
    /// Dipanggil tiap `logic`; mengembalikan perintah dari tray atau instance lain.
    pub poll: Box<dyn FnMut() -> Vec<UiEvent>>,
    pub tray: bool,
}

pub enum UiEvent {
    Show,
    Quit,
    GameMode(Option<bool>),
}

pub struct PacuApp {
    shared: Arc<RwLock<Snapshot>>,
    tx: Sender<Command>,
    snap: Snapshot,
    cfg: Config,
    page: Page,
    search: String,
    sort: SortBy,
    new_game: String,
    new_exclusion: String,
    log_filter: Option<LogKind>,
    autostart: bool,
    hooks: UiHooks,
    quitting: bool,
    hidden: bool,
    status: Option<(String, f64)>,
    logo: Option<egui::TextureHandle>,
    fps: Arc<Mutex<FpsMeter>>,
    fps_stats: Arc<Mutex<Stats>>,
    mon: Arc<RwLock<monitor::View>>,
    overlay_cfg: Arc<RwLock<crate::config::OverlayConfig>>,
    fps_enabled: Arc<AtomicBool>,
    volt: VoltPanel,
    bench_label: String,
    bench_secs: u64,
    new_freeze: String,
    tweak_states: Vec<tweaks::State>,
    tweak_backup: tweaks::Backup,
    reboot_needed: bool,
    startup: Vec<crate::startup::Entry>,
    startup_loaded: bool,
    discord_clients: Vec<crate::discord::Client>,
    discord_loaded: bool,
    updater: crate::update::Updater,
    update_next_check: f64,
    wake_list: Option<Result<Vec<crate::wake::Device>, String>>,
    bios: Arc<Mutex<Option<crate::bios::Report>>>,
    bios_scanning: Arc<AtomicBool>,
}

impl PacuApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Arc<RwLock<Snapshot>>,
        tx: Sender<Command>,
        cfg: Config,
        hooks: UiHooks,
        extras: Extras,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        let rgba = crate::icon::icon_rgba(64);
        let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], &rgba);
        let logo = Some(cc.egui_ctx.load_texture("logo", img, egui::TextureOptions::LINEAR));
        let hidden = cfg.general.start_minimized && hooks.tray;
        let fps_stats = extras.fps.lock().stats.clone();
        PacuApp {
            shared,
            tx,
            snap: Snapshot::default(),
            cfg,
            page: std::env::var("PACU_PAGE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| Page::ALL.get(i).copied())
                .unwrap_or(Page::Home),
            search: String::new(),
            sort: SortBy::Cpu,
            new_game: String::new(),
            new_exclusion: String::new(),
            log_filter: None,
            autostart: autostart::is_enabled(),
            hooks,
            quitting: false,
            hidden,
            status: None,
            logo,
            fps_stats,
            fps: extras.fps,
            mon: extras.mon,
            overlay_cfg: extras.overlay_cfg,
            fps_enabled: extras.fps_enabled,
            volt: extras.volt,
            bench_label: String::new(),
            bench_secs: 60,
            new_freeze: String::new(),
            tweak_states: tweaks::all().map(tweaks::status).collect(),
            tweak_backup: tweaks::Backup::load(),
            reboot_needed: false,
            startup: Vec::new(),
            startup_loaded: false,
            discord_clients: Vec::new(),
            discord_loaded: false,
            updater: crate::update::Updater::default(),
            // Pemeriksaan pertama beberapa detik setelah menyala, supaya tidak menunda jendela.
            update_next_check: 5.0,
            wake_list: None,
            bios: Arc::new(Mutex::new(None)),
            bios_scanning: Arc::new(AtomicBool::new(false)),
        }
    }

    fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }

    fn apply_config(&mut self) {
        let msg = match self.cfg.save() {
            Ok(()) => "Tersimpan".to_string(),
            Err(e) => format!("Gagal menyimpan: {e}"),
        };
        self.send(Command::UpdateConfig(Box::new(self.cfg.clone())));
        *self.overlay_cfg.write() = self.cfg.overlay.clone();
        self.fps_enabled.store(self.cfg.game.fps_meter, std::sync::atomic::Ordering::Relaxed);
        self.status = Some((msg, 0.0));
    }

    fn add_game(&mut self, name: &str) {
        let n = name.trim().to_string();
        if !n.is_empty() && !self.cfg.game.games.iter().any(|g| g.eq_ignore_ascii_case(&n)) {
            self.cfg.game.games.push(n);
            self.apply_config();
        }
    }

    fn rule_mut(&mut self, name: &str) -> &mut ProcessRule {
        let idx = match self.cfg.rules.iter().position(|r| r.pattern.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                self.cfg.rules.push(ProcessRule { pattern: name.to_string(), ..Default::default() });
                self.cfg.rules.len() - 1
            }
        };
        &mut self.cfg.rules[idx]
    }
}

fn opt_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut Option<T>,
    all: &[T],
    label: fn(T) -> &'static str,
    none: &str,
) -> bool {
    let mut changed = false;
    let text = value.map(label).unwrap_or(none);
    egui::ComboBox::from_id_salt(id).selected_text(text).width(150.0).show_ui(ui, |ui| {
        changed |= ui.selectable_value(value, None, none).changed();
        for v in all {
            changed |= ui.selectable_value(value, Some(*v), label(*v)).changed();
        }
    });
    changed
}

fn status_color(s: Status) -> Color32 {
    match s {
        Status::Game => GREEN,
        Status::Restrained => AMBER,
        Status::Ruled => CYAN,
        Status::Normal => MUTED,
    }
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::Game => "Game",
        Status::Restrained => "Diredam",
        Status::Ruled => "Aturan",
        Status::Normal => "",
    }
}

fn sparkline(ui: &mut egui::Ui, data: &std::collections::VecDeque<f32>, height: f32, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_CONTROL), theme::SUNKEN);
    for frac in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * frac;
        p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], Stroke::new(1.0, theme::DIVIDER.gamma_multiply(0.5)));
    }
    if data.len() < 2 {
        return;
    }
    let n = 120usize;
    let step = rect.width() / (n - 1) as f32;
    let start = n.saturating_sub(data.len());
    let pts: Vec<egui::Pos2> = data
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(rect.left() + (start + i) as f32 * step, rect.bottom() - rect.height() * (v.clamp(0.0, 100.0) / 100.0))
        })
        .collect();
    let mut fill = pts.clone();
    fill.push(egui::pos2(pts.last().unwrap().x, rect.bottom()));
    fill.push(egui::pos2(pts[0].x, rect.bottom()));
    for w in pts.windows(2) {
        let quad = vec![w[0], w[1], egui::pos2(w[1].x, rect.bottom()), egui::pos2(w[0].x, rect.bottom())];
        p.add(egui::Shape::convex_polygon(quad, color.gamma_multiply(0.18), Stroke::NONE));
    }
    p.add(egui::Shape::line(pts, Stroke::new(2.0, color)));
}

/// Grafik frame time: skala otomatis dari median, lonjakan di atas 2x median diberi warna merah.
fn frame_graph(ui: &mut egui::Ui, frames: &[f32], height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_CONTROL), theme::SUNKEN);
    let mut sorted = frames.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2].max(0.1);
    let top = (median * 3.0).max(sorted[sorted.len() * 99 / 100] * 1.1);
    let y = |v: f32| rect.bottom() - (v / top).min(1.0) * (rect.height() - 6.0) - 3.0;
    let dx = rect.width() / (frames.len().max(2) - 1) as f32;
    for (k, w) in frames.windows(2).enumerate() {
        let x0 = rect.left() + k as f32 * dx;
        let c = if w[1] > median * 2.0 { RED } else { CYAN };
        p.line_segment([egui::pos2(x0, y(w[0])), egui::pos2(x0 + dx, y(w[1]))], Stroke::new(1.3, c));
    }
    p.text(
        rect.left_top() + Vec2::new(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!("{top:.1} ms"),
        FontId::proportional(11.0),
        MUTED,
    );
}

fn bar(ui: &mut egui::Ui, frac: f32, color: Color32) {
    let width = ui.available_width();
    // ProgressBar WinUI: lintasan tipis 1 px, isi 3 px membulat.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), Sense::hover());
    let p = ui.painter();
    let track = egui::Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 1.0));
    p.rect_filled(track, CornerRadius::ZERO, theme::TEXT_3.gamma_multiply(0.6));
    let mut fill = egui::Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 3.0));
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
    p.rect_filled(fill, CornerRadius::same(2), color);
}

fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{} j {} m", s / 3600, (s / 60) % 60)
    } else if s >= 60 {
        format!("{} m {} d", s / 60, s % 60)
    } else {
        format!("{s} d")
    }
}

impl eframe::App for PacuApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for ev in (self.hooks.poll)() {
            match ev {
                UiEvent::Show => {
                    self.hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                UiEvent::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                UiEvent::GameMode(m) => self.send(Command::ForceGameMode(m)),
            }
        }
        {
            let s = self.shared.read();
            if s.version != self.snap.version {
                self.snap = s.clone();
            }
        }
        let now = ctx.input(|i| i.time);
        self.update_tick(now);
        // Pengukur FPS dijalankan thread monitor (ikut Mode Game), supaya tetap jalan saat jendela di tray.
        if self.fps.lock().poll_recording(self.bench_secs) {
            self.status = Some(("Benchmark selesai".into(), 0.0));
        }
        let fast = self.updater.busy() || !self.hidden
            && ((self.page == Page::Fps && self.fps.lock().running_for().is_some())
                || self.page == Page::Monitor
                || (self.page == Page::Volt && self.volt.busy()));
        ctx.request_repaint_after(Duration::from_millis(if fast { 250 } else { 1000 }));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.hooks.tray && self.cfg.general.close_to_tray {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
        }

        egui::Panel::left("nav")
            .exact_size(236.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin { left: 6, right: 6, top: 10, bottom: 8 }))
            .show(ui, |ui| self.nav(ui));

        egui::Panel::bottom("status")
            .exact_size(28.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::symmetric(14, 5)))
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(Stroke::new(1.0, theme::STROKE))
                    .corner_radius(CornerRadius { nw: 8, ne: 0, sw: 8, se: 0 })
                    .inner_margin(Margin { left: 32, right: 20, top: 26, bottom: 4 }),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(1060.0));
                    ui.add_space(0.0);
                    match self.page {
                    Page::Home => self.page_home(ui),
                    Page::Game => self.page_game(ui),
                    Page::Monitor => self.page_monitor(ui),
                    Page::Fps => self.page_fps(ui),
                    Page::Volt => {
                        page_header(ui, "Undervolt GPU", "Mencari tegangan terendah yang stabil di clock yang sama, lalu menerapkannya otomatis.");
                        self.volt.show(ui)
                    }
                    Page::Tweaks => self.page_tweaks(ui),
                    Page::Ram => self.page_ram(ui),
                    Page::Processes => self.page_processes(ui),
                    Page::Rules => self.page_rules(ui),
                    Page::Activity => self.page_activity(ui),
                    Page::Settings => self.page_settings(ui),
                    Page::Bios => self.page_bios(ui),
                    }
                    ui.add_space(24.0);
                });
            });
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.send(Command::Quit);
    }
}

impl PacuApp {
    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), Vec2::splat(20.0))));
            }
            ui.label(RichText::new("Pacu").font(theme::semibold(14.0)));
            ui.label(RichText::new("Game Booster").size(12.0).color(theme::TEXT_3));
        });
        ui.add_space(14.0);
        for (head, pages) in NAV {
            if !head.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new(head).font(theme::semibold(12.0)).color(theme::TEXT_3));
                });
            }
            for &p in pages {
                self.nav_item(ui, p);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            for p in NAV_FOOTER.iter().rev() {
                self.nav_item(ui, *p);
            }
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
            ui.painter().line_segment([egui::pos2(rect.left() + 8.0, rect.center().y), egui::pos2(rect.right() - 8.0, rect.center().y)], Stroke::new(1.0, theme::DIVIDER));
        });
    }

    /// Item NavigationView: ikon 16 px, label, latar saat dipilih / disorot, dan garis aksen
    /// vertikal di kiri item terpilih.
    fn nav_item(&mut self, ui: &mut egui::Ui, p: Page) {
        let selected = self.page == p;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
        let rect = rect.shrink2(Vec2::new(0.0, 2.0));
        let painter = ui.painter();
        let r = CornerRadius::same(theme::RADIUS_CONTROL);
        if selected {
            painter.rect_filled(rect, r, Color32::from_rgb(0x2D, 0x2D, 0x2D));
            let pill = egui::Rect::from_center_size(egui::pos2(rect.left() + 1.5, rect.center().y), Vec2::new(3.0, 16.0));
            painter.rect_filled(pill, CornerRadius::same(2), accent());
        } else if resp.hovered() {
            painter.rect_filled(rect, r, Color32::from_rgb(0x2A, 0x2A, 0x2A));
        }
        let y = rect.center().y;
        painter.text(egui::pos2(rect.left() + 16.0, y), egui::Align2::LEFT_CENTER, p.icon(), theme::icon_font(16.0), TEXT);
        painter.text(egui::pos2(rect.left() + 46.0, y), egui::Align2::LEFT_CENTER, p.label(), FontId::proportional(14.0), TEXT);
        if resp.clicked() {
            self.page = p;
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let s = &self.snap;
        let dim = |t: String| RichText::new(t).size(12.0).color(theme::TEXT_3);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            ui.label(dim(format!("CPU {:.0}%", s.total_cpu)));
            ui.label(dim(format!("RAM {}%", s.mem.load)));
            match &s.game {
                Some(g) => ui.label(RichText::new(format!("\u{25CF} Mode Game: {}", g.name)).size(12.0).color(GREEN)),
                None => ui.label(dim("Mode Game siaga".into())),
            };
            let bal = if s.balancer_active { format!("Penyeimbang aktif, {} diredam", s.restrained_now) } else { "Penyeimbang jeda".into() };
            ui.label(dim(bal));
            if let Some((msg, t)) = &mut self.status {
                if *t == 0.0 {
                    *t = ui.input(|i| i.time);
                }
                let shown = ui.input(|i| i.time) - *t < 3.0;
                if shown {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(msg.clone()).size(12.0).color(theme::TEXT_2));
                    });
                } else {
                    self.status = None;
                }
            }
        });
    }

    fn game_mode_buttons(&mut self, ui: &mut egui::Ui) {
        let forced = self.snap.forced;
        if let Some(val) = segmented(ui, forced, &[(None, "Otomatis"), (Some(true), "Nyala"), (Some(false), "Mati")]) {
            self.send(Command::ForceGameMode(val));
            self.snap.forced = val;
        }
    }

    fn page_home(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Beranda", "Ringkasan Mode Game dan beban sistem saat ini.");
        self.update_banner(ui);

        // Kartu Mode Game.
        let game = self.snap.game.clone();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                let on = game.is_some();
                ui.painter().rect_filled(icon_rect, CornerRadius::same(theme::RADIUS_CARD), if on { theme::mix(CARD, GREEN, 0.22) } else { CARD_HI });
                ui.painter().text(icon_rect.center(), egui::Align2::CENTER_CENTER, theme::icon::GAMES, theme::icon_font(22.0), if on { GREEN } else { TEXT });
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Mode Game").font(theme::semibold(20.0)));
                        if on {
                            tag(ui, "Aktif", GREEN);
                        } else {
                            tag(ui, "Siaga", theme::TEXT_2);
                        }
                    });
                    match &game {
                        Some(g) => {
                            let pid = g.pid.map(|p| format!(" (PID {p})")).unwrap_or_default();
                            ui.label(RichText::new(format!("{}{pid}, berjalan {}", g.name, fmt_duration(g.since.elapsed()))).color(theme::TEXT_2));
                        }
                        None => {
                            ui.label(RichText::new("Menyala otomatis saat game terdeteksi.").color(theme::TEXT_2));
                        }
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.game_mode_buttons(ui));
            });
            divider(ui);
            let plan = self
                .snap
                .power_plans
                .iter()
                .find(|p| p.guid.eq_ignore_ascii_case(&self.snap.active_plan))
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "-".into());
            let timer = self.snap.timer_ms.map(|m| format!("{m:.1} ms")).unwrap_or_else(|| "Bawaan".into());
            let fps = self.fps_stats.lock().live().map(|(avg, low)| format!("{avg:.0} FPS, 1% low {low:.0}")).unwrap_or_else(|| "-".into());
            let frozen = if self.snap.frozen.is_empty() { "Tidak ada".to_string() } else { self.snap.frozen.join(", ") };
            ui.columns(4, |cols| {
                for (ui, (k, v)) in cols.iter_mut().zip([("Power plan", plan), ("Timer resolution", timer), ("Frame rate", fps), ("Dibekukan", frozen)]) {
                    ui.label(muted(k));
                    ui.add(egui::Label::new(RichText::new(v).color(TEXT)).truncate());
                }
            });
        });
        ui.add_space(8.0);

        // Statistik.
        let s = self.snap.clone();
        let big = |t: String| RichText::new(t).font(theme::semibold(26.0)).color(TEXT);
        ui.columns(3, |cols| {
            card(&mut cols[0], |ui| {
                ui.label(muted("Pemakaian CPU"));
                ui.label(big(format!("{:.0}%", s.total_cpu)));
                sparkline(ui, &s.cpu_history, 52.0, CYAN);
            });
            card(&mut cols[1], |ui| {
                ui.label(muted("Memori"));
                let used = s.mem.total_mb.saturating_sub(s.mem.avail_mb);
                ui.horizontal(|ui| {
                    ui.label(big(format!("{:.1}", used as f32 / 1024.0)));
                    ui.label(muted(format!("/ {:.1} GB", s.mem.total_mb as f32 / 1024.0)));
                });
                let c = if s.mem.load > 85 { RED } else if s.mem.load > 70 { AMBER } else { accent() };
                ui.add_space(6.0);
                bar(ui, s.mem.load as f32 / 100.0, c);
                ui.label(muted(format!("{}% terpakai, {} MB bebas", s.mem.load, s.mem.avail_mb)));
            });
            card(&mut cols[2], |ui| {
                ui.label(muted("Penyeimbang CPU"));
                ui.horizontal(|ui| {
                    ui.label(big(s.restrained_now.to_string()));
                    ui.label(muted("proses diredam"));
                });
                ui.label(muted(format!("{} kali sejak Pacu dibuka", s.restraints_total)));
                if let Some(t) = s.thresholds {
                    ui.label(muted(format!("Aktif bila CPU > {:.0}% dan satu proses > {:.0}%", t.total, t.per_process)));
                }
            });
        });
        ui.add_space(8.0);

        // Proses teratas.
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(section("Paling berat sekarang"));
            ui.add_space(4.0);
            let mut top: Vec<&ProcView> = s.procs.iter().filter(|p| p.pid > 4).collect();
            top.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
            for p in top.into_iter().take(6) {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(220.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_width(220.0);
                        ui.add(egui::Label::new(strong(&p.name)).truncate());
                    });
                    let w = (ui.available_width() - 190.0).max(40.0);
                    ui.allocate_ui(Vec2::new(w, 20.0), |ui| bar(ui, p.cpu / 100.0, if p.status == Status::Restrained { AMBER } else { accent() }));
                    ui.label(format!("{:>5.1}%", p.cpu));
                    let st = status_label(p.status);
                    if !st.is_empty() {
                        ui.label(RichText::new(st).color(status_color(p.status)).size(12.5));
                    }
                });
            }
        });

        if s.topology.hybrid() {
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(strong("CPU hybrid terdeteksi").color(CYAN));
                ui.label(muted(format!(
                    "{} P-core dan {} E-core. Proses latar yang diredam dipindah ke E-core supaya P-core bebas untuk game.",
                    s.topology.pcore_sets.len(),
                    s.topology.ecore_sets.len()
                )));
            });
        }
    }

    fn page_game(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Mode Game", "Saat game berjalan, Pacu memberi game prioritas penuh dan menenangkan aplikasi lain.");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(strong("Status"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.game_mode_buttons(ui));
            });
        });
        ui.add_space(10.0);

        let mut changed = false;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Preset sekali klik"));
            ui.label(muted("Menambahkan game ke daftar dan menyalakan semua optimasi yang cocok untuknya."));
            ui.add_space(6.0);
            let hybrid = self.snap.topology.hybrid();
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                for (i, p) in presets::PRESETS.iter().enumerate() {
                    let applied = presets::is_applied(&self.cfg, p);
                    let label = if applied { format!("{} (aktif)", p.name) } else { p.name.to_string() };
                    let b = if i == 0 { accent_button(label) } else { egui::Button::new(label) }.min_size(Vec2::new(0.0, 32.0));
                    let r = ui.add(b);
                    let r = if p.note.is_empty() { r } else { r.on_hover_text(p.note) };
                    if r.clicked() {
                        chosen = Some(p);
                    }
                }
            });
            if let Some(p) = chosen {
                presets::apply(&mut self.cfg, p, hybrid);
                changed = true;
                self.status = Some((format!("Preset {} diterapkan", p.name), 0.0));
            }
        });
        ui.add_space(10.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Daftar game"));
            ui.label(muted("Nama exe atau pola, misalnya valorant*.exe. Game di folder Steam, Epic, Riot, dan Xbox terdeteksi otomatis."));
            ui.add_space(4.0);
            let mut remove = None;
            for (i, g) in self.cfg.game.games.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(g.clone()).size(15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Hapus").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                self.cfg.game.games.remove(i);
                changed = true;
            }
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.new_game).hint_text("contoh: cs2.exe").desired_width(240.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Tambah").clicked() || enter {
                    let g = std::mem::take(&mut self.new_game);
                    self.add_game(&g);
                }
                let mut running: Vec<String> = self
                    .snap
                    .procs
                    .iter()
                    .filter(|p| p.session != 0 && p.pid > 4)
                    .map(|p| p.name.clone())
                    .collect();
                running.sort_by_key(|n| n.to_lowercase());
                running.dedup();
                let mut pick = None;
                egui::ComboBox::from_id_salt("pick_game").selected_text("Pilih dari aplikasi yang berjalan").width(260.0).show_ui(ui, |ui| {
                    for n in &running {
                        if ui.selectable_label(false, n).clicked() {
                            pick = Some(n.clone());
                        }
                    }
                });
                if let Some(n) = pick {
                    self.add_game(&n);
                }
            });
        });
        ui.add_space(10.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Deteksi"));
            ui.add_space(4.0);
            let g = &mut self.cfg.game;
            changed |= setting_row(ui, &mut g.auto, "Nyalakan otomatis", "Mode Game menyala sendiri saat game terdeteksi dan mati saat game ditutup.");
            changed |= setting_row(ui, &mut g.detect_store_folders, "Kenali game dari folder toko", "Steam, Epic, Riot, Xbox, GOG, Ubisoft, EA, Battle.net.");
            changed |= setting_row(ui, &mut g.detect_fullscreen, "Kenali aplikasi layar penuh", "Browser dan pemutar video dikecualikan.");
        });
        ui.add_space(10.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Optimasi saat bermain"));
            ui.add_space(4.0);
            let plans = self.snap.power_plans.clone();
            let hybrid = self.snap.topology.hybrid();
            let g = &mut self.cfg.game;
            ui.horizontal(|ui| {
                ui.label("Prioritas CPU game");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    changed |= opt_combo(ui, "gp", &mut g.game_priority, &Priority::ALL, Priority::label, "Tidak diubah");
                });
            });
            ui.horizontal(|ui| {
                ui.label("Prioritas I/O game (disk)");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    changed |= opt_combo(ui, "gio", &mut g.game_io_priority, &IoPriority::ALL, IoPriority::label, "Tidak diubah");
                });
            });
            ui.horizontal(|ui| {
                ui.label("Prioritas GPU game");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    changed |= opt_combo(ui, "ggpu", &mut g.game_gpu_priority, &GpuPriority::ALL, GpuPriority::label, "Tidak diubah");
                });
            });
            ui.horizontal(|ui| {
                ui.label("Power plan saat bermain");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = match &g.power_plan {
                        PowerPlanChoice::Auto => "Otomatis (performa tertinggi)".to_string(),
                        PowerPlanChoice::Keep => "Jangan diubah".to_string(),
                        PowerPlanChoice::Plan(guid) => plans.iter().find(|p| &p.guid == guid).map(|p| p.name.clone()).unwrap_or_else(|| guid.clone()),
                    };
                    egui::ComboBox::from_id_salt("plan").selected_text(text).width(240.0).show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut g.power_plan, PowerPlanChoice::Auto, "Otomatis (performa tertinggi)").changed();
                        changed |= ui.selectable_value(&mut g.power_plan, PowerPlanChoice::Keep, "Jangan diubah").changed();
                        for p in &plans {
                            changed |= ui.selectable_value(&mut g.power_plan, PowerPlanChoice::Plan(p.guid.clone()), &p.name).changed();
                        }
                    });
                });
            });
            ui.add_space(6.0);
            changed |= setting_row(ui, &mut g.aggressive_balancer, "Redam aplikasi latar lebih cepat", "Ambang penyeimbang CPU diturunkan setengah selama bermain.");
            changed |= setting_row(ui, &mut g.timer_resolution, "Resolusi timer 0,5 ms", "Frame time lebih stabil di banyak game. Sedikit menambah pemakaian daya.");
            changed |= setting_row(ui, &mut g.disable_efficiency_on_game, "Matikan Efficiency Mode pada game", "Mencegah Windows memperlambat game.");
            changed |= setting_row(ui, &mut g.prevent_sleep, "Cegah layar mati dan sleep", "");
            changed |= setting_row(ui, &mut g.clear_standby, "Bersihkan RAM standby saat mulai bermain", &format!("Hanya bila RAM bebas di bawah {} MB.", g.standby_free_mb_threshold));
            changed |= setting_row(ui, &mut g.standby_periodic, "Bersihkan RAM standby berkala", &format!("Dicek tiap {} detik selama bermain, dibersihkan hanya bila RAM bebas menipis.", g.standby_check_secs));
            changed |= setting_row(ui, &mut g.fps_meter, "Ukur FPS otomatis", "Menjalankan PresentMon untuk game yang terdeteksi. Lihat halaman FPS & Benchmark.");
            if hybrid {
                changed |= setting_row(ui, &mut g.game_on_pcores, "Jalankan game hanya di P-core", "Bisa menambah FPS di CPU Intel hybrid; matikan bila game malah tersendat.");
            }
        });
        ui.add_space(10.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let g = &mut self.cfg.game;
            changed |= setting_row(
                ui,
                &mut g.freeze_enabled,
                "Bekukan aplikasi latar selama bermain",
                "Aplikasi di bawah ini dijeda total (0% CPU) dan dilanjutkan saat game ditutup. Bila salah satunya kamu buka ke depan, ia dilanjutkan otomatis.",
            );
            ui.add_space(4.0);
            let mut remove = None;
            for (i, a) in g.freeze_apps.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(a.clone());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Hapus").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                g.freeze_apps.remove(i);
                changed = true;
            }
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.new_freeze).hint_text("contoh: Discord.exe").desired_width(240.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Tambah").clicked() || enter {
                    let n = std::mem::take(&mut self.new_freeze).trim().to_string();
                    if !n.is_empty() && !g.freeze_apps.iter().any(|a| a.eq_ignore_ascii_case(&n)) {
                        g.freeze_apps.push(n);
                        changed = true;
                    }
                }
            });
            ui.label(muted("Jangan masukkan aplikasi yang kamu pakai saat bermain, misalnya Discord bila kamu voice chat."));
        });
        if changed {
            self.apply_config();
        }
    }

    fn page_fps(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "FPS & Benchmark",
            &format!("Diukur dengan PresentMon {} yang sudah dibundel, tanpa injeksi ke game.", crate::fps::PRESENTMON_VERSION),
        );

        let running = self.fps.lock().running_for().map(str::to_string);
        let (live, frames, error, recording, results) = {
            let st = self.fps_stats.lock();
            (
                st.live(),
                st.frame_times.iter().copied().collect::<Vec<f32>>(),
                st.error.clone(),
                st.recording.as_ref().map(|r| (r.0.elapsed().as_secs(), r.1.clone())),
                st.results.clone(),
            )
        };

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            match (&running, live) {
                (Some(exe), Some((avg, low1))) => {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(muted("FPS rata-rata"));
                            ui.label(RichText::new(format!("{avg:.0}")).size(40.0).strong().color(CYAN));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label(muted("1% low"));
                            let c = if low1 < avg * 0.5 { AMBER } else { GREEN };
                            ui.label(RichText::new(format!("{low1:.0}")).size(40.0).strong().color(c));
                        });
                        ui.add_space(30.0);
                        ui.vertical(|ui| {
                            ui.label(muted("Frame time"));
                            ui.label(RichText::new(format!("{:.2} ms", 1000.0 / avg)).size(40.0).strong());
                        });
                    });
                    ui.label(muted(format!("Mengukur {exe}")));
                }
                (Some(exe), None) => {
                    ui.label(RichText::new(format!("Menunggu frame dari {exe}...")).size(18.0).strong());
                    ui.label(muted("Angka muncul begitu game menggambar frame. Bila tetap kosong, pastikan Pacu berjalan sebagai admin."));
                }
                (None, _) => {
                    ui.label(RichText::new("Belum ada game yang diukur").size(18.0).strong());
                    let why = if self.cfg.game.fps_meter {
                        "Pengukuran mulai otomatis saat Mode Game mendeteksi game."
                    } else {
                        "Pengukuran FPS otomatis mati. Nyalakan di halaman Mode Game."
                    };
                    ui.label(muted(why));
                }
            }
            if let Some(e) = &error {
                ui.label(RichText::new(e).color(RED));
            }
            if frames.len() > 2 {
                ui.add_space(8.0);
                ui.label(muted("Frame time (ms), 600 frame terakhir. Lonjakan tinggi = stutter."));
                frame_graph(ui, &frames, 110.0);
            }
        });
        ui.add_space(10.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Rekam benchmark"));
            ui.label(muted("Main di tempat yang sama tiap kali (misalnya sandbox atau lane yang sama), lalu bandingkan hasil sebelum dan sesudah mengubah pengaturan."));
            ui.add_space(6.0);
            match &recording {
                Some((el, label)) => {
                    ui.label(RichText::new(format!("Merekam \"{label}\"... {el} / {} detik", self.bench_secs)).color(AMBER).strong());
                    bar(ui, *el as f32 / self.bench_secs as f32, AMBER);
                    if ui.button("Batalkan").clicked() {
                        self.fps_stats.lock().recording = None;
                    }
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.bench_label).hint_text("Nama, misalnya: sesudah optimasi").desired_width(260.0));
                        egui::ComboBox::from_id_salt("bench_secs").selected_text(format!("{} detik", self.bench_secs)).width(100.0).show_ui(ui, |ui| {
                            for v in [30u64, 60, 120, 300] {
                                ui.selectable_value(&mut self.bench_secs, v, format!("{v} detik"));
                            }
                        });
                        let can = running.is_some();
                        let b = accent_button("Mulai rekam");
                        let r = ui.add_enabled(can, b);
                        let r = if can { r } else { r.on_disabled_hover_text("Jalankan game dulu") };
                        if r.clicked() {
                            let n = results.len() + 1;
                            let label = match self.bench_label.trim() {
                                "" => format!("Rekaman {n}"),
                                l => l.to_string(),
                            };
                            self.fps.lock().start_recording(label);
                            self.bench_label.clear();
                        }
                    });
                }
            }
        });
        ui.add_space(10.0);

        if !results.is_empty() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(strong("Hasil"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Hapus semua").clicked() {
                            let mut st = self.fps_stats.lock();
                            st.results.clear();
                            crate::fps::save_results(&st.results);
                        }
                    });
                });
                ui.label(muted("Persentase dibandingkan dengan rekaman pertama untuk game yang sama."));
                ui.add_space(4.0);
                let mut remove = None;
                egui::Grid::new("bench").striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
                    for h in ["Nama", "Game", "Durasi", "FPS rata-rata", "1% low", "0,1% low", ""] {
                        ui.label(RichText::new(h).color(MUTED));
                    }
                    ui.end_row();
                    for (i, r) in results.iter().enumerate() {
                        let base = results.iter().find(|b| b.game.eq_ignore_ascii_case(&r.game)).filter(|b| !std::ptr::eq(*b, r));
                        let cell = |ui: &mut egui::Ui, v: f32, b: Option<f32>| {
                            ui.horizontal(|ui| {
                                ui.label(strong(format!("{v:.0}")));
                                if let Some(b) = b.filter(|b| *b > 0.0) {
                                    let d = (v / b - 1.0) * 100.0;
                                    let c = if d >= 0.5 { GREEN } else if d <= -0.5 { RED } else { MUTED };
                                    ui.label(RichText::new(format!("{d:+.1}%")).color(c).size(12.5));
                                }
                            });
                        };
                        ui.label(&r.label);
                        ui.label(muted(r.game.clone()));
                        ui.label(format!("{} d", r.seconds));
                        cell(ui, r.avg_fps, base.map(|b| b.avg_fps));
                        cell(ui, r.low1_fps, base.map(|b| b.low1_fps));
                        cell(ui, r.low01_fps, base.map(|b| b.low01_fps));
                        if ui.small_button("Hapus").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = remove {
                    let mut st = self.fps_stats.lock();
                    st.results.remove(i);
                    crate::fps::save_results(&st.results);
                }
            });
        }
    }

    fn page_tweaks(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Optimasi Windows", "Pengaturan Windows 11 yang membantu game. Nilai lama disimpan, jadi semuanya bisa dikembalikan.");
        self.tweak_header(ui, "Game", tweaks::Group::Game);
        self.tweak_cards(ui, tweaks::Group::Game);
        ui.add_space(18.0);
        self.tweak_header(ui, "Ekstrem", tweaks::Group::Extreme);
        ui.label(muted("Diadaptasi dari AtlasOS. Lebih agresif, tetapi tidak mengurangi keamanan."));
        ui.add_space(4.0);
        self.tweak_cards(ui, tweaks::Group::Extreme);
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(theme::icon_text(theme::icon::SHIELD, 16.0).color(RED));
            ui.label(section("Berisiko: mengurangi keamanan"));
        });
        ui.label(muted("Tidak ikut \"Terapkan semua\". Terapkan satu per satu bila kamu menerima risikonya."));
        ui.add_space(4.0);
        self.tweak_cards(ui, tweaks::Group::Risky);
        ui.add_space(4.0);
        ui.label(muted("Defender dan Firewall tidak disentuh Pacu."));
    }

    /// Judul halaman dengan tombol "Cek ulang" dan "Terapkan semua" untuk satu kelompok tweak.
    fn tweak_header(&mut self, ui: &mut egui::Ui, head: &str, group: tweaks::Group) {
        let idx: Vec<usize> = tweaks::all().enumerate().filter(|(_, t)| t.group == group).map(|(i, _)| i).collect();
        let done = idx.iter().filter(|&&i| self.tweak_states[i] == tweaks::State::Applied).count();
        ui.horizontal(|ui| {
            ui.label(section(head));
            ui.label(muted(format!("{done} dari {} sudah optimal", idx.len())));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cek ulang").clicked() {
                    self.tweak_states = tweaks::all().map(tweaks::status).collect();
                }
                let pending: Vec<usize> = tweaks::all()
                    .enumerate()
                    .filter(|(i, t)| t.group == group && self.tweak_states[*i] == tweaks::State::NotApplied)
                    .map(|(i, _)| i)
                    .collect();
                let b = accent_button("Terapkan semua");
                if ui.add_enabled(!pending.is_empty(), b).clicked() {
                    for i in pending {
                        let t = tweaks::all().nth(i).unwrap();
                        self.run_tweak(i, t, true);
                    }
                }
            });
        });
    }

    fn tweak_cards(&mut self, ui: &mut egui::Ui, group: tweaks::Group) {
        if self.reboot_needed && matches!(group, tweaks::Group::Game | tweaks::Group::Ram) {
            banner(ui, AMBER, "Restart diperlukan", "Restart PC supaya semua perubahan berlaku.");
            ui.add_space(6.0);
        }

        let idx: Vec<usize> = tweaks::all().enumerate().filter(|(_, t)| t.group == group).map(|(i, _)| i).collect();
        // Satu kartu per tweak, rapat seperti daftar SettingsCard di aplikasi Settings.
        for i in idx {
            let t = tweaks::all().nth(i).unwrap();
            let state = self.tweak_states[i];
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match state {
                            tweaks::State::NotApplied => {
                                if ui.add(egui::Button::new("Terapkan").min_size(Vec2::new(104.0, 32.0))).clicked() {
                                    self.run_tweak(i, t, true);
                                }
                            }
                            tweaks::State::Applied if tweaks::has_backup(t, &self.tweak_backup) => {
                                if ui.add(egui::Button::new("Kembalikan").min_size(Vec2::new(104.0, 32.0))).clicked() {
                                    self.run_tweak(i, t, false);
                                }
                            }
                            _ => {
                                ui.add_space(104.0);
                            }
                        }
                        ui.add_space(4.0);
                        let (txt, c) = match state {
                            tweaks::State::Applied => ("Optimal", GREEN),
                            tweaks::State::NotApplied => ("Belum", AMBER),
                            tweaks::State::Unknown => ("Tidak terbaca", theme::TEXT_2),
                        };
                        tag(ui, txt, c);
                        ui.add_space(12.0);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(t.title).color(TEXT));
                                if t.reboot {
                                    ui.label(RichText::new("perlu restart").color(theme::TEXT_3).size(12.0));
                                }
                            });
                            ui.label(muted(t.desc));
                        });
                    });
                });
            });
            ui.add_space(3.0);
        }
    }

    fn run_tweak(&mut self, i: usize, t: &tweaks::Tweak, apply: bool) {
        let r = if apply { tweaks::apply(t, &mut self.tweak_backup) } else { tweaks::revert(t, &mut self.tweak_backup) };
        self.tweak_states[i] = tweaks::status(t);
        match r {
            Ok(()) => {
                self.reboot_needed |= t.reboot;
                let verb = if apply { "Diterapkan" } else { "Dikembalikan" };
                self.status = Some((format!("{verb}: {}", t.title), 0.0));
                self.send(Command::Log(format!("{verb}: {}", t.title)));
            }
            Err(e) => self.status = Some((format!("Gagal: {e}"), 0.0)),
        }
    }

    fn page_processes(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Proses", "Klik kanan sebuah proses untuk mengatur prioritas, menjadikannya game, atau membuat aturan.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(format!("{}  Cari proses", theme::icon::SEARCH)).desired_width(280.0));
            ui.add_space(8.0);
            ui.label(muted("Urutkan"));
            if let Some(v) = segmented(ui, self.sort, &[(SortBy::Cpu, "CPU"), (SortBy::Mem, "RAM"), (SortBy::Name, "Nama")]) {
                self.sort = v;
            }
        });
        ui.add_space(6.0);

        let q = self.search.to_lowercase();
        let mut procs: Vec<ProcView> = self
            .snap
            .procs
            .iter()
            .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.pid.to_string() == q)
            .cloned()
            .collect();
        match self.sort {
            SortBy::Cpu => procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.mem_mb.total_cmp(&a.mem_mb))),
            SortBy::Mem => procs.sort_by(|a, b| b.mem_mb.total_cmp(&a.mem_mb)),
            SortBy::Name => procs.sort_by_key(|p| p.name.to_lowercase()),
        }

        let mut action: Option<(ProcView, ProcAction)> = None;
        let height = ui.available_height().max(300.0);
        TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::remainder().at_least(200.0))
            .column(Column::exact(70.0))
            .column(Column::exact(70.0))
            .column(Column::exact(90.0))
            .column(Column::exact(130.0))
            .column(Column::exact(90.0))
            .max_scroll_height(height)
            .header(26.0, |mut h| {
                for t in ["Nama", "PID", "CPU", "RAM", "Prioritas", "Status"] {
                    h.col(|ui| {
                        ui.label(RichText::new(t).color(MUTED).strong());
                    });
                }
            })
            .body(|body| {
                body.rows(26.0, procs.len(), |mut row| {
                    let p = &procs[row.index()];
                    row.col(|ui| {
                        ui.add(egui::Label::new(RichText::new(&p.name)).truncate());
                    });
                    row.col(|ui| {
                        ui.label(muted(p.pid.to_string()));
                    });
                    row.col(|ui| {
                        let c = if p.cpu > 20.0 { AMBER } else if p.cpu > 0.5 { TEXT } else { MUTED };
                        ui.label(RichText::new(format!("{:.1}%", p.cpu)).color(c));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0} MB", p.mem_mb));
                    });
                    row.col(|ui| {
                        ui.label(p.priority.label());
                    });
                    row.col(|ui| {
                        let st = status_label(p.status);
                        if !st.is_empty() {
                            ui.label(RichText::new(st).color(status_color(p.status)));
                        }
                    });
                    row.response().context_menu(|ui| {
                        if let Some(a) = proc_menu(ui, p) {
                            action = Some((p.clone(), a));
                        }
                    });
                });
            });
        if let Some((p, a)) = action {
            self.handle_proc_action(&p, a);
        }
    }

    fn handle_proc_action(&mut self, p: &ProcView, a: ProcAction) {
        match a {
            ProcAction::Priority(pr) => self.send(Command::SetPriority(p.pid, pr)),
            ProcAction::Io(io) => self.send(Command::SetIoPriority(p.pid, io)),
            ProcAction::Gpu(g) => self.send(Command::SetGpuPriority(p.pid, g)),
            ProcAction::MakeGame => {
                self.add_game(&p.name);
                self.status = Some((format!("{} ditambahkan ke daftar game", p.name), 0.0));
            }
            ProcAction::NeverRestrain => {
                self.rule_mut(&p.name.clone()).never_restrain = true;
                self.apply_config();
            }
            ProcAction::RulePriority(pr) => {
                self.rule_mut(&p.name.clone()).priority = Some(pr);
                self.apply_config();
            }
            ProcAction::OpenRules => {
                self.rule_mut(&p.name.clone());
                self.apply_config();
                self.page = Page::Rules;
            }
            ProcAction::Terminate => self.send(Command::Terminate(p.pid)),
        }
    }

    fn page_rules(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Aturan proses", "Pengaturan yang selalu diterapkan setiap kali aplikasi tertentu berjalan.");
        let mut changed = false;
        let mut remove = None;
        for (i, r) in self.cfg.rules.iter_mut().enumerate() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut r.pattern).hint_text("nama.exe atau pola").desired_width(260.0)).lost_focus();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Hapus").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                egui::Grid::new(format!("rule{i}")).num_columns(4).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("CPU");
                    changed |= opt_combo(ui, &format!("rp{i}"), &mut r.priority, &Priority::ALL, Priority::label, "Tidak diubah");
                    ui.label("I/O");
                    changed |= opt_combo(ui, &format!("ri{i}"), &mut r.io_priority, &IoPriority::ALL, IoPriority::label, "Tidak diubah");
                    ui.end_row();
                    ui.label("GPU");
                    changed |= opt_combo(ui, &format!("rg{i}"), &mut r.gpu_priority, &GpuPriority::ALL, GpuPriority::label, "Tidak diubah");
                    ui.label("Efficiency Mode");
                    let mut eff = r.efficiency_mode;
                    changed |= opt_combo(ui, &format!("re{i}"), &mut eff, &[true, false], |b| if b { "Nyala" } else { "Mati" }, "Tidak diubah");
                    r.efficiency_mode = eff;
                    ui.end_row();
                    ui.label("Affinity");
                    let mut aff = r.affinity.map(|m| format!("{m:#x}")).unwrap_or_default();
                    let resp = ui.add(egui::TextEdit::singleline(&mut aff).hint_text("mis. 0xFF (kosong = semua)").desired_width(150.0));
                    if resp.changed() {
                        let t = aff.trim().trim_start_matches("0x").trim_start_matches("0X");
                        r.affinity = if t.is_empty() { None } else { u64::from_str_radix(t, 16).ok().or(r.affinity) };
                    }
                    changed |= resp.lost_focus();
                    ui.label("Jangan diredam");
                    changed |= toggle(ui, &mut r.never_restrain).changed();
                    ui.end_row();
                });
            });
            ui.add_space(8.0);
        }
        if let Some(i) = remove {
            self.cfg.rules.remove(i);
            changed = true;
        }
        if ui.add(egui::Button::new(strong("+ Aturan baru")).fill(accent())).clicked() {
            self.cfg.rules.push(ProcessRule::default());
        }
        if changed {
            self.apply_config();
        }
    }

    fn page_activity(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Aktivitas", "Apa saja yang dilakukan Pacu sejak dibuka.");
        theme::pivot(
            ui,
            &mut self.log_filter,
            &[(None, "Semua"), (Some(LogKind::Game), "Game"), (Some(LogKind::Restrain), "Peredaman"), (Some(LogKind::Rule), "Aturan"), (Some(LogKind::Warn), "Peringatan")],
        );
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let entries: Vec<_> = self
                .snap
                .log
                .iter()
                .rev()
                .filter(|e| match self.log_filter {
                    None => true,
                    Some(LogKind::Restrain) => matches!(e.kind, LogKind::Restrain | LogKind::Restore),
                    Some(k) => e.kind == k,
                })
                .take(300)
                .collect();
            if entries.is_empty() {
                ui.label(muted("Belum ada aktivitas."));
            }
            for e in entries {
                let (icon, c) = match e.kind {
                    LogKind::Info => ("INFO", MUTED),
                    LogKind::Restrain => ("REDAM", AMBER),
                    LogKind::Restore => ("PULIH", GREEN),
                    LogKind::Game => ("GAME", accent()),
                    LogKind::Rule => ("ATURAN", CYAN),
                    LogKind::Warn => ("GAGAL", RED),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(muted(e.time.clone()));
                    ui.add_sized([56.0, 18.0], egui::Label::new(RichText::new(icon).color(c).size(11.5).strong()));
                    ui.label(&e.text);
                });
            }
        });
    }

    fn page_settings(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Pengaturan", "Penyeimbang CPU, perilaku jendela, dan data Pacu.");
        let mut changed = false;
        let auto = self.snap.thresholds;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Penyeimbang CPU"));
            ui.label(muted("Menurunkan prioritas sementara aplikasi latar yang memakan CPU, supaya aplikasi di depan tetap lancar."));
            ui.add_space(6.0);
            let b = &mut self.cfg.balancer;
            changed |= setting_row(ui, &mut b.enabled, "Aktifkan penyeimbang CPU", "");
            let mut custom = b.total_threshold.is_some();
            if setting_row(ui, &mut custom, "Atur ambang sendiri", "Bila mati, ambang dihitung otomatis dari jumlah inti CPU.") {
                changed = true;
                if custom {
                    let topo = &self.snap.topology;
                    let t = crate::balancer::auto_thresholds(topo.logical.max(1), topo.ecore_sets.len() as u32);
                    b.total_threshold = Some(t.total);
                    b.per_process_threshold = Some(t.per_process);
                    b.restore_threshold = Some(t.restore);
                    b.time_over_ms = Some(t.time_over_ms);
                } else {
                    b.total_threshold = None;
                    b.per_process_threshold = None;
                    b.restore_threshold = None;
                    b.time_over_ms = None;
                }
            }
            if custom {
                egui::Grid::new("thr").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    let slider = |ui: &mut egui::Ui, label: &str, v: &mut Option<f32>, max: f32| {
                        ui.label(label);
                        let mut x = v.unwrap_or(10.0);
                        let r = ui.add(egui::Slider::new(&mut x, 1.0..=max).suffix("%"));
                        *v = Some(x);
                        ui.end_row();
                        r.drag_stopped() || (r.changed() && !r.dragged())
                    };
                    changed |= slider(ui, "CPU total sistem di atas", &mut b.total_threshold, 100.0);
                    changed |= slider(ui, "CPU satu proses di atas", &mut b.per_process_threshold, 100.0);
                    changed |= slider(ui, "Pulihkan bila CPU proses di bawah", &mut b.restore_threshold, 50.0);
                    ui.label("Selama (ms)");
                    let mut ms = b.time_over_ms.unwrap_or(900);
                    let r = ui.add(egui::Slider::new(&mut ms, 100..=10_000));
                    b.time_over_ms = Some(ms);
                    changed |= r.drag_stopped() || (r.changed() && !r.dragged());
                    ui.end_row();
                });
            } else if let Some(t) = auto {
                let note = if self.snap.game.is_some() && self.cfg.game.aggressive_balancer { " (dipercepat selama Mode Game)" } else { "" };
                ui.label(muted(format!(
                    "Otomatis: CPU total > {:.0}%, satu proses > {:.0}% selama {} ms, pulih di bawah {:.0}%{note}.",
                    t.total, t.per_process, t.time_over_ms, t.restore
                )));
            }
            ui.add_space(6.0);
            changed |= setting_row(ui, &mut b.lower_to_idle, "Turunkan ke Idle", "Lebih tegas daripada \"di bawah normal\".");
            changed |= setting_row(ui, &mut b.lower_io_priority, "Turunkan juga prioritas disk", "");
            changed |= setting_row(ui, &mut b.use_efficiency_mode, "Pakai Efficiency Mode Windows", "Proses yang diredam berjalan hemat daya.");
            changed |= setting_row(ui, &mut b.move_to_ecores, "Pindahkan ke E-core (CPU hybrid)", "");
            changed |= setting_row(ui, &mut b.only_normal_priority, "Hanya proses berprioritas normal", "");
            changed |= setting_row(ui, &mut b.exclude_services, "Jangan sentuh layanan Windows", "");
            changed |= setting_row(ui, &mut b.exclude_foreground, "Jangan sentuh aplikasi di depan", "");
            changed |= setting_row(ui, &mut b.exclude_foreground_children, "Jangan sentuh anak proses aplikasi di depan", "");
            changed |= setting_row(ui, &mut b.pause_when_idle, "Jeda saat PC tidak dipakai", &format!("Setelah {} detik tanpa input.", b.idle_ms / 1000));

            ui.add_space(6.0);
            ui.label(strong("Pengecualian"));
            let mut remove = None;
            for (i, e) in b.exclusions.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(e);
                    if ui.small_button("Hapus").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                b.exclusions.remove(i);
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_exclusion).hint_text("nama.exe").desired_width(200.0));
                if ui.button("Tambah").clicked() && !self.new_exclusion.trim().is_empty() {
                    b.exclusions.push(std::mem::take(&mut self.new_exclusion).trim().to_string());
                    changed = true;
                }
            });
        });
        ui.add_space(10.0);

        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(strong("Umum"));
            ui.add_space(4.0);
            let mut auto_on = self.autostart;
            if setting_row(ui, &mut auto_on, "Jalankan saat Windows menyala", "Lewat Task Scheduler dengan hak admin, tanpa prompt UAC.") {
                if autostart::set(auto_on) {
                    self.autostart = auto_on;
                    self.status = Some(("Jalan otomatis diperbarui".into(), 0.0));
                } else {
                    self.status = Some(("Gagal mengatur jalan otomatis".into(), 0.0));
                }
            }
            let g = &mut self.cfg.general;
            changed |= setting_row(ui, &mut g.start_minimized, "Mulai tersembunyi di tray", "");
            changed |= setting_row(ui, &mut g.close_to_tray, "Tombol tutup menyembunyikan ke tray", "Pacu tetap bekerja di latar.");
            changed |= setting_row(ui, &mut g.log_to_file, "Simpan log ke file", "");
            ui.horizontal(|ui| {
                ui.label("Interval pemeriksaan");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("refresh").selected_text(format!("{} ms", g.refresh_ms)).show_ui(ui, |ui| {
                        for ms in [500u64, 1000, 2000] {
                            changed |= ui.selectable_value(&mut g.refresh_ms, ms, format!("{ms} ms")).changed();
                        }
                    });
                });
            });
            ui.add_space(4.0);
            ui.label(muted(format!("Data dan log: {}", crate::config::data_dir().display())));
            if ui.button("Kembalikan semua pengaturan ke default").clicked() {
                self.cfg = Config::default();
                changed = true;
            }
        });
        ui.add_space(10.0);
        changed |= self.wake_card(ui);
        ui.add_space(10.0);
        changed |= self.update_card(ui);
        if changed {
            self.apply_config();
        }
    }
}

#[derive(Clone, Copy)]
enum ProcAction {
    Priority(Priority),
    Io(IoPriority),
    Gpu(GpuPriority),
    MakeGame,
    NeverRestrain,
    RulePriority(Priority),
    OpenRules,
    Terminate,
}

fn proc_menu(ui: &mut egui::Ui, p: &ProcView) -> Option<ProcAction> {
    let mut out = None;
    ui.label(strong(&p.name));
    if !p.path.is_empty() {
        ui.label(muted(p.path.clone()));
    }
    ui.separator();
    ui.menu_button("Prioritas CPU (sekarang)", |ui| {
        for pr in Priority::ALL {
            if ui.button(pr.label()).clicked() {
                out = Some(ProcAction::Priority(pr));
                ui.close();
            }
        }
    });
    ui.menu_button("Prioritas CPU (selalu)", |ui| {
        for pr in Priority::ALL {
            if ui.button(pr.label()).clicked() {
                out = Some(ProcAction::RulePriority(pr));
                ui.close();
            }
        }
    });
    ui.menu_button("Prioritas I/O", |ui| {
        for io in IoPriority::ALL {
            if ui.button(io.label()).clicked() {
                out = Some(ProcAction::Io(io));
                ui.close();
            }
        }
    });
    ui.menu_button("Prioritas GPU", |ui| {
        for g in GpuPriority::ALL {
            if ui.button(g.label()).clicked() {
                out = Some(ProcAction::Gpu(g));
                ui.close();
            }
        }
    });
    ui.separator();
    if ui.button("Jadikan game").clicked() {
        out = Some(ProcAction::MakeGame);
        ui.close();
    }
    if ui.button("Jangan pernah diredam").clicked() {
        out = Some(ProcAction::NeverRestrain);
        ui.close();
    }
    if ui.button("Buat aturan...").clicked() {
        out = Some(ProcAction::OpenRules);
        ui.close();
    }
    ui.separator();
    if ui.button(RichText::new("Hentikan proses").color(RED)).clicked() {
        out = Some(ProcAction::Terminate);
        ui.close();
    }
    out
}
