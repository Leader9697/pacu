//! Halaman Undervolt GPU (Pacu Volt) di dalam Pacu.

use crate::volt::autostart;
use crate::volt::gpu::{Curve, Telemetry};
use crate::volt::store::{self, Profile, Session, Settings, TestKind};
use crate::volt::tuner::{Cmd, LogKind, Phase, Sample, Shared};
use eframe::egui::{self, Align, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke, Vec2};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::ui::theme::{self, accent, accent_button, banner, card, danger_button, muted, setting_row, strong, subtitle};

const BG: Color32 = theme::SUNKEN;
const CARD: Color32 = theme::CARD;
const CYAN: Color32 = theme::DATA_BLUE;
const GREEN: Color32 = theme::SUCCESS;
const AMBER: Color32 = theme::CAUTION;
const RED: Color32 = theme::CRITICAL;
const VIOLET: Color32 = theme::DATA_PURPLE;
const TEXT: Color32 = theme::TEXT;
const MUTED: Color32 = theme::TEXT_3;
const GRID: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Tune,
    Vram,
    Curve,
    Log,
}

impl Page {
    const ALL: [Page; 5] = [Page::Home, Page::Tune, Page::Vram, Page::Curve, Page::Log];
    fn label(self) -> &'static str {
        match self {
            Page::Home => "Beranda",
            Page::Tune => "Cari Titik Optimal",
            Page::Vram => "Overclock VRAM",
            Page::Curve => "Kurva V/F",
            Page::Log => "Log & Diagnostik",
        }
    }
}

pub struct VoltPanel {
    sh: Arc<Shared>,
    tx: Sender<Cmd>,
    page: Page,
    settings: Settings,
    /// Sesi yang belum selesai dari pemakaian sebelumnya.
    pending: Option<Session>,
    crashed_note: Option<String>,
    autostart: bool,
    toast: Option<(String, f64)>,
    custom_target: bool,
    custom_mhz: u32,
    confirm: bool,
    vram: crate::volt::vram::VramTuner,
    vram_profile: Option<crate::volt::vram::VramProfile>,
    /// Offset memori terakhir yang dibaca dari driver, dan kapan dibaca (detik egui).
    vram_live: Option<Result<i32, String>>,
    vram_live_at: f64,
    /// Pasang ulang undervolt setelah pencarian VRAM selesai (driver bisa menghapus kurva saat
    /// clock memori diubah).
    reapply_uv: bool,
}

impl VoltPanel {
    pub fn new(sh: Arc<Shared>, tx: Sender<Cmd>, pending: Option<Session>) -> Self {
        let mut settings = Settings::load();
        if settings.load_exe.is_empty() {
            if let Some((exe, args)) = crate::volt::presets::detect_heaven() {
                settings.load_exe = exe;
                settings.load_args = args;
            }
        } else if settings.load_args.contains("-project_name Heaven") && settings.load_args != crate::volt::presets::HEAVEN_ARGS {
            // Argumen Heaven dari versi lama: ganti ke preset Extreme dalam jendela.
            settings.load_args = crate::volt::presets::HEAVEN_ARGS.into();
            settings.save();
        }
        let crashed_note = pending.as_ref().and_then(|s| {
            s.results.last().filter(|r| !r.passed && r.secs == 0).map(|r| {
                format!("Uji terakhir ({} MHz @ {:.0} mV) berhenti karena PC crash/restart atau aplikasi ditutup paksa. Titik itu dicatat tidak stabil.", r.target_mhz, r.volt_mv)
            })
        });
        let custom_target = settings.target_mhz.is_some();
        let custom_mhz = settings.target_mhz.unwrap_or(1800);
        Self {
            sh,
            tx,
            page: if pending.is_some() { Page::Tune } else { Page::Home },
            settings,
            pending,
            crashed_note,
            autostart: autostart::is_enabled(),
            toast: None,
            custom_target,
            custom_mhz,
            confirm: false,
            vram: crate::volt::vram::VramTuner::new(),
            vram_profile: crate::volt::vram::VramProfile::load(),
            vram_live: None,
            vram_live_at: f64::NEG_INFINITY,
            reapply_uv: false,
        }
    }

    fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), 0.0));
    }

    /// Membaca ulang offset memori dari driver tiap 3 detik (atau segera bila `force`), kecuali saat
    /// pencarian VRAM sedang mengubah-ubah offset.
    fn refresh_vram_live(&mut self, now: f64, force: bool) {
        if self.vram.running() {
            return;
        }
        if force || now - self.vram_live_at > 3.0 {
            self.vram_live = Some(crate::volt::vram::current_offset());
            self.vram_live_at = now;
            // Kurva juga dibaca ulang, supaya status undervolt ikut berubah bila driver menghapusnya.
            if !self.sh.view.read().phase().busy() {
                self.send(Cmd::RefreshCurve);
            }
        }
    }

    /// Apakah profil undervolt sedang terpasang menurut kurva terakhir yang dibaca.
    fn uv_active(&self) -> bool {
        let v = self.sh.view.read();
        v.profile.as_ref().map(|p| profile_active(&v.curve, p)).unwrap_or(false)
    }

    /// Memasang offset VRAM tersimpan. Mengubah clock memori lewat Pstates20 bisa membuat driver
    /// menghapus offset kurva V/F, jadi undervolt yang tadinya terpasang dipasang ulang sesudahnya.
    fn apply_vram(&mut self) -> Result<Option<i32>, String> {
        let had_uv = self.uv_active();
        let r = crate::volt::vram::apply_saved();
        if had_uv {
            self.send(Cmd::ApplyProfile);
        }
        self.vram_live_at = f64::NEG_INFINITY;
        r
    }

    fn reset_vram(&mut self) -> Result<(), String> {
        let had_uv = self.uv_active();
        let r = crate::volt::vram::reset();
        if had_uv {
            self.send(Cmd::ApplyProfile);
        }
        self.vram_live_at = f64::NEG_INFINITY;
        r
    }
}

impl VoltPanel {
    /// Apakah pencarian sedang berjalan (untuk mempercepat repaint).
    pub fn busy(&self) -> bool {
        self.sh.view.read().phase().busy() || self.vram.running()
    }

    /// Hentikan pekerja saat Pacu benar-benar keluar. Pencarian yang berjalan dikembalikan ke bawaan.
    pub fn shutdown(&self) {
        self.sh.stop.store(true, Ordering::Relaxed);
        self.send(Cmd::Quit);
        if self.vram.running() {
            let _ = crate::volt::vram::reset();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if self.reapply_uv && !self.vram.running() {
            self.reapply_uv = false;
            self.send(Cmd::ApplyProfile);
        }
        let items: Vec<(Page, &str)> = Page::ALL.iter().map(|&p| (p, p.label())).collect();
        theme::pivot(ui, &mut self.page, &items);
        ui.add_space(6.0);
        self.status_bar(ui);
        ui.add_space(10.0);
        egui::ScrollArea::vertical().id_salt("volt-scroll").auto_shrink([false, false]).show(ui, |ui| match self.page {
            Page::Home => self.page_home(ui),
            Page::Tune => self.page_tune(ui),
            Page::Vram => self.page_vram(ui),
            Page::Curve => self.page_curve(ui),
            Page::Log => self.page_log(ui),
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let v = self.sh.view.read();
        let t = &v.telemetry;
        let phase = v.phase();
        let headline = v.headline.clone();
        let tel = format!(
            "{} MHz · {} mV · {} W · {}°C · GPU {}%",
            opt(t.clock_mhz),
            t.voltage_mv.map(|x| format!("{x:.0}")).unwrap_or("-".into()),
            t.power_w.map(|x| format!("{x:.0}")).unwrap_or("-".into()),
            opt(t.temp_c),
            opt(t.util_pct)
        );
        drop(v);
        ui.horizontal(|ui| {
            ui.label(muted(tel));
            ui.separator();
            let (c, s) = phase_style(phase);
            ui.label(RichText::new(if phase == Phase::Idle { "Siap".into() } else { headline }).color(c).size(13.0));
            let _ = s;
            if let Some((msg, t)) = &mut self.toast {
                if *t == 0.0 {
                    *t = ui.input(|i| i.time);
                }
                if ui.input(|i| i.time) - *t < 3.0 {
                    let m = msg.clone();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(m).color(GREEN).size(13.0));
                    });
                } else {
                    self.toast = None;
                }
            }
        });
    }

    // ── Beranda ────────────────────────────────────────────────────────────────

    fn page_home(&mut self, ui: &mut egui::Ui) {
        let (info, tel, history, gpu_error, profile, phase, curve) = {
            let v = self.sh.view.read();
            (v.info.clone(), v.telemetry.clone(), v.history.clone(), v.gpu_error.clone(), v.profile.clone(), v.phase(), v.curve.clone())
        };
        ui.label(subtitle(&info.name));
        ui.label(muted(format!("Driver {} · {}", info.driver, info.backend)));
        ui.add_space(12.0);

        if let Some(e) = gpu_error {
            banner(ui, RED, "GPU tidak bisa diakses", &format!("{e}\nBuka Log & Diagnostik lalu kirim laporannya."));
            ui.add_space(10.0);
        }
        if let Some(note) = &self.crashed_note {
            banner(ui, AMBER, "Sesi sebelumnya terhenti", note);
            ui.add_space(10.0);
        }

        tiles(ui, &tel);
        ui.add_space(12.0);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(strong("Riwayat 5 menit"));
                ui.label(muted("clock (hijau) dan daya (ungu)"));
            });
            history_chart(ui, &history, 120.0);
        });
        ui.add_space(12.0);

        let now = ui.input(|i| i.time);
        self.refresh_vram_live(now, false);
        let vram_saved = self.vram_profile.clone();
        let vram_live = self.vram_live.clone();
        let nvidia = !is_amd(&info);
        card(ui, |ui| {
            ui.label(subtitle("Profil terpasang"));
            ui.add_space(6.0);

            // Baris 1: undervolt (kurva clock inti).
            let uv_active = profile.as_ref().map(|p| profile_active(&curve, p)).unwrap_or(false);
            ui.horizontal(|ui| {
                ui.label(theme::icon_text(theme::icon::FLASH, 18.0).color(accent()));
                ui.vertical(|ui| {
                    ui.label(muted("Undervolt inti"));
                    match &profile {
                        Some(p) => ui.label(RichText::new(format!("{} MHz @ {:.0} mV", p.target_mhz, p.volt_mv)).font(theme::semibold(18.0))),
                        None => ui.label(RichText::new("Belum ada").color(MUTED)),
                    };
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &profile {
                    Some(_) if uv_active => theme::tag(ui, "Terpasang", GREEN),
                    Some(_) => theme::tag(ui, "Tidak terpasang", AMBER),
                    None => {
                        if ui.button("Cari titik optimal").clicked() {
                            self.page = Page::Tune;
                        }
                    }
                });
            });
            if let Some(p) = &profile {
                if let (Some(b), Some(r)) = (&p.baseline, &p.result) {
                    ui.label(savings_text(b, r));
                }
            }
            theme::divider(ui);

            // Baris 2: overclock VRAM (offset clock memori), status dibaca langsung dari driver.
            let live = match &vram_live {
                Some(Ok(o)) => Some(*o),
                _ => None,
            };
            let vram_active = match (&vram_saved, live) {
                (Some(v), Some(o)) => v.offset_mhz != 0 && o == v.offset_mhz,
                _ => false,
            };
            ui.horizontal(|ui| {
                ui.label(theme::icon_text(theme::icon::RAM, 18.0).color(accent()));
                ui.vertical(|ui| {
                    ui.label(muted("Overclock VRAM"));
                    match &vram_saved {
                        Some(v) => ui.label(RichText::new(format!("{:+} MHz memori", v.offset_mhz)).font(theme::semibold(18.0))),
                        None if nvidia => ui.label(RichText::new("Belum ada").color(MUTED)),
                        None => ui.label(RichText::new("Belum tersedia untuk GPU AMD").color(MUTED)),
                    };
                    let detail = match (&vram_live, self.vram.running()) {
                        (_, true) => "Pencarian VRAM sedang berjalan".to_string(),
                        (Some(Ok(o)), _) => format!("Di driver sekarang: {o:+} MHz"),
                        (Some(Err(e)), _) => format!("Offset tidak terbaca: {e}"),
                        (None, _) => String::new(),
                    };
                    if !detail.is_empty() {
                        ui.label(muted(detail));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &vram_saved {
                    Some(_) if vram_active => theme::tag(ui, "Terpasang", GREEN),
                    Some(_) => theme::tag(ui, "Tidak terpasang", AMBER),
                    None if nvidia => {
                        if ui.button("Cari offset VRAM").clicked() {
                            self.page = Page::Vram;
                        }
                    }
                    None => {}
                });
            });
            ui.add_space(8.0);

            // Tombol bersama: memasang / melepas keduanya sekaligus.
            let any = profile.is_some() || vram_saved.is_some();
            ui.horizontal(|ui| {
                let busy = phase.busy() || self.vram.running();
                if ui.add_enabled(any && !busy, accent_button("Pasang semua")).clicked() {
                    // Urutan penting: clock memori dulu, baru kurva V/F (sama seperti saat Windows menyala),
                    // karena mengubah Pstates20 bisa menghapus offset kurva.
                    let mut parts = Vec::new();
                    if vram_saved.is_some() {
                        match crate::volt::vram::apply_saved() {
                            Ok(Some(o)) => parts.push(format!("VRAM {o:+} MHz")),
                            Ok(None) => {}
                            Err(e) => parts.push(format!("VRAM gagal: {e}")),
                        }
                        self.vram_live_at = f64::NEG_INFINITY;
                    }
                    if profile.is_some() {
                        self.send(Cmd::ApplyProfile);
                        parts.insert(0, "undervolt".to_string());
                    }
                    self.toast(format!("Dipasang: {}", parts.join(", ")));
                }
                if ui.add_enabled(any && !busy, egui::Button::new("Kembalikan ke bawaan")).clicked() {
                    let r = crate::volt::vram::reset();
                    self.vram_live_at = f64::NEG_INFINITY;
                    self.send(Cmd::ResetStock);
                    let msg = match r {
                        Ok(()) => "Kurva inti dan clock memori kembali ke bawaan".to_string(),
                        Err(e) if vram_saved.is_some() => format!("Kurva inti dipulihkan; VRAM gagal: {e}"),
                        Err(_) => "Kurva inti dipulihkan".to_string(),
                    };
                    self.toast(msg);
                }
            });
            if profile.is_some() || vram_saved.is_some() {
                ui.add_space(6.0);
                let mut on = self.autostart;
                if setting_row(ui, &mut on, "Pasang otomatis saat Windows menyala", "Undervolt dan overclock VRAM dipasang lagi setiap login lewat Task Scheduler, tanpa prompt UAC. Driver bisa mengembalikan ke bawaan setelah restart atau crash.") {
                    if autostart::set(on) {
                        self.autostart = on;
                        if let Some(p) = &profile {
                            let mut p2 = p.clone();
                            p2.apply_on_login = on;
                            p2.save();
                        }
                        self.toast(if on { "Dipasang saat login" } else { "Tidak dipasang saat login" });
                    } else {
                        self.toast("Gagal mengubah Task Scheduler");
                    }
                }
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(strong("Cara kerja"));
            if is_amd(&info) {
                ui.label(muted(
                    "Di GPU AMD, Pacu mengatur \"Tegangan maksimum\" dan \"Frekuensi maksimum\" lewat ADLX, sama seperti \
                     slider Tuning manual di AMD Adrenalin. Frekuensi maksimum dibiarkan di nilai bawaan, lalu tegangan \
                     maksimum diturunkan sedikit demi sedikit sambil GPU dibebani penuh, dan berhenti saat muncul tanda \
                     tidak stabil. Didukung untuk RX 5000 dan RX 6000. Seri RX 7000/9000 belum didukung.",
                ));
            } else {
                ui.label(muted(
                    "Pacu Volt memakai fungsi NVAPI yang sama dengan editor kurva MSI Afterburner. Undervolt dibuat dengan \
                     meratakan kurva V/F di satu titik tegangan: GPU tetap di frekuensi yang sama, tetapi dengan tegangan \
                     lebih rendah, sehingga daya dan suhu turun. Pencarian menurunkan tegangan sedikit demi sedikit sambil \
                     GPU dibebani penuh, dan berhenti saat muncul tanda tidak stabil.",
                ));
            }
        });
    }

    // ── Overclock VRAM ─────────────────────────────────────────────────────────

    fn page_vram(&mut self, ui: &mut egui::Ui) {
        use crate::volt::vram::{self, Phase as VPhase};
        let info = self.sh.view.read().info.clone();
        let core_busy = self.sh.view.read().phase().busy();
        ui.label(subtitle("Overclock VRAM"));
        ui.label(muted(
            "Menaikkan clock memori GPU sampai batas aman. Memori GDDR6/GDDR6X mengoreksi error sendiri, jadi clock yang terlalu tinggi \
             membuat bandwidth turun sebelum muncul artefak. Pacu mengukur bandwidth nyata dan memeriksa setiap data, lalu memilih offset \
             dengan bandwidth tertinggi dikurangi margin 50 MHz.",
        ));
        ui.add_space(12.0);
        if is_amd(&info) {
            banner(ui, AMBER, "Hanya GPU NVIDIA", "Overclock VRAM otomatis belum tersedia untuk GPU AMD.");
            return;
        }
        let now = ui.input(|i| i.time);
        self.refresh_vram_live(now, false);
        if let Some(p) = self.vram_profile.clone() {
            let live = match &self.vram_live {
                Some(Ok(o)) => Some(*o),
                _ => None,
            };
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(strong("Hasil tersimpan"));
                    match live {
                        Some(o) if o == p.offset_mhz && o != 0 => theme::tag(ui, "Terpasang", GREEN),
                        Some(o) => theme::tag(ui, &format!("Tidak terpasang (driver {o:+} MHz)"), AMBER),
                        None => {}
                    }
                });
                ui.label(RichText::new(format!("Offset memori {:+} MHz", p.offset_mhz)).size(24.0).strong().color(VIOLET));
                if p.base_gbs > 0.0 {
                    ui.label(RichText::new(format!("Bandwidth {:.0} ke {:.0} GB/s ({:+.1}%)", p.base_gbs, p.best_gbs, (p.best_gbs / p.base_gbs - 1.0) * 100.0)).color(GREEN));
                }
                ui.horizontal(|ui| {
                    if ui.button("Pasang").clicked() {
                        match self.apply_vram() {
                            Ok(_) => self.toast(format!("Offset memori {:+} MHz dipasang", p.offset_mhz)),
                            Err(e) => self.toast(format!("Gagal: {e}")),
                        }
                    }
                    if ui.button("Kembalikan ke bawaan").clicked() {
                        match self.reset_vram() {
                            Ok(()) => self.toast("Offset memori dikembalikan ke 0"),
                            Err(e) => self.toast(format!("Gagal: {e}")),
                        }
                    }
                    if ui.button("Hapus hasil").clicked() {
                        let _ = self.reset_vram();
                        vram::VramProfile::clear();
                        self.vram_profile = None;
                    }
                });
                ui.label(muted("Ikut dipasang saat Windows menyala bila \"Pasang otomatis saat Windows menyala\" di Beranda menyala."));
            });
            ui.add_space(12.0);
        }
        let v = self.vram.view.read().clone();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(strong("Pencarian otomatis"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.vram.running() {
                        if ui.button("Hentikan").clicked() {
                            self.vram.stop();
                        }
                    } else {
                        let b = accent_button("Mulai cari offset VRAM");
                        if ui.add_enabled(!core_busy, b).clicked() {
                            self.reapply_uv = self.uv_active();
                            self.vram.start();
                        }
                    }
                });
            });
            ui.label(muted("Sekitar 5 sampai 8 menit. Tutup game dan aplikasi 3D lain. Layar bisa berkedip bila driver pulih dari error; itu normal."));
            if core_busy {
                ui.label(RichText::new("Tunggu pencarian undervolt selesai dulu.").color(AMBER));
            }
            if v.phase != VPhase::Idle {
                ui.add_space(6.0);
                let c = match v.phase {
                    VPhase::Running => CYAN,
                    VPhase::Done => GREEN,
                    VPhase::Failed => RED,
                    VPhase::Idle => MUTED,
                };
                ui.label(RichText::new(&v.msg).color(c).strong());
                let mut best = 0.0f64;
                for (off, gbs, errs, ok) in &v.results {
                    let dropped = *ok && best > 0.0 && *gbs < best * (1.0 - vram::DROP);
                    if *ok && !dropped {
                        best = best.max(*gbs);
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{off:+} MHz")).monospace().color(TEXT));
                        ui.label(RichText::new(format!("{gbs:.1} GB/s")).monospace().color(CYAN));
                        let (t, col) = if dropped {
                            ("bandwidth turun (EDC)".to_string(), AMBER)
                        } else if *ok {
                            ("lolos".to_string(), GREEN)
                        } else if *errs > 0 {
                            (format!("{errs} error data"), RED)
                        } else {
                            ("gagal".to_string(), RED)
                        };
                        ui.label(RichText::new(t).color(col));
                    });
                }
            }
        });
        if v.phase == VPhase::Done && self.vram_profile.as_ref().map(|p| p.created) != vram::VramProfile::load().map(|p| p.created) {
            self.vram_profile = vram::VramProfile::load();
        }
    }

    // ── Cari titik optimal ─────────────────────────────────────────────────────

    fn page_tune(&mut self, ui: &mut egui::Ui) {
        let (phase, headline, session, test, elapsed, total, history, profile) = {
            let v = self.sh.view.read();
            (v.phase(), v.headline.clone(), v.session.clone(), v.test.clone(), v.test_elapsed, v.test_total, v.history.clone(), v.profile.clone())
        };
        ui.label(subtitle("Cari Titik Optimal"));
        ui.label(muted("Mencari tegangan terendah yang tetap stabil di frekuensi target, dengan GPU dibebani 100%."));
        ui.add_space(12.0);

        if phase.busy() {
            self.running_card(ui, phase, &headline, test.as_ref(), elapsed, total, session.as_ref());
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.label(strong("Sensor langsung"));
                tiles_compact(ui, &self.sh.view.read().telemetry.clone());
                history_chart(ui, &history, 110.0);
            });
        } else {
            self.result_card(ui, phase, &headline, session.as_ref(), profile.as_ref());
            if let Some(p) = self.pending.clone() {
                if !phase.busy() && matches!(phase, Phase::Idle) {
                    card(ui, |ui| {
                        ui.label(strong("Sesi sebelumnya belum selesai").color(AMBER));
                        ui.label(muted(format!(
                            "{} uji sudah dijalankan untuk {} MHz. Lanjutkan untuk memakai hasil itu, atau buang dan mulai dari awal.",
                            p.results.len(),
                            p.target_khz / 1000
                        )));
                        if let Some(n) = &self.crashed_note {
                            ui.label(RichText::new(n).color(AMBER).size(13.0));
                        }
                        ui.horizontal(|ui| {
                            if ui.button(strong("Lanjutkan sesi")).clicked() {
                                self.settings.save();
                                self.send(Cmd::Start { settings: self.settings.clone(), resume: Some(p.clone()) });
                                self.pending = None;
                            }
                            if ui.button("Buang").clicked() {
                                Session::discard();
                                self.pending = None;
                            }
                        });
                    });
                    ui.add_space(12.0);
                }
            }
            self.settings_card(ui);
        }
        if let Some(s) = &session {
            if !s.results.is_empty() || s.baseline.is_some() {
                ui.add_space(12.0);
                results_card(ui, s);
            }
        }
    }

    fn running_card(&mut self, ui: &mut egui::Ui, phase: Phase, headline: &str, test: Option<&crate::volt::store::Planned>, elapsed: u32, total: u32, s: Option<&Session>) {
        card(ui, |ui| {
            let (c, label) = phase_style(phase);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(c));
                ui.label(RichText::new(label).color(c).strong());
            });
            ui.label(RichText::new(headline).size(22.0).strong());
            if phase == Phase::WaitingLoad && self.settings.load_exe.is_empty() {
                ui.label(muted("Buka aplikasi 3D kamu sekarang (misalnya Unigine Heaven preset Extreme, boleh dalam jendela). Pengujian dimulai otomatis begitu GPU sibuk."));
            }
            if total > 0 && matches!(phase, Phase::Testing | Phase::Baseline) {
                let frac = elapsed as f32 / total as f32;
                ui.add(egui::ProgressBar::new(frac).text(format!("{} / {}", mmss(elapsed), mmss(total))).fill(accent()));
            }
            if let (Some(t), Some(s)) = (test, s) {
                let stock_v = s.baseline.as_ref().map(|b| b.volt_mv).unwrap_or(0.0);
                if stock_v > 0.0 {
                    ui.label(muted(format!("{:.0} mV lebih rendah dari tegangan bawaan ({:.0} mV)", stock_v - t.volt_mv, stock_v)));
                }
            }
            if let Some(s) = s {
                ui.label(muted(format!("{} uji selesai · perkiraan sisa: {}", s.results.len(), estimate(s))));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(danger_button("Hentikan")).clicked() {
                    self.sh.stop.store(true, Ordering::Relaxed);
                }
                ui.label(muted("Menghentikan mengembalikan kurva ke bawaan. Sesi bisa dilanjutkan nanti."));
            });
        });
    }

    fn result_card(&mut self, ui: &mut egui::Ui, phase: Phase, headline: &str, s: Option<&Session>, profile: Option<&Profile>) {
        match phase {
            Phase::Done => {
                card(ui, |ui| {
                    ui.label(RichText::new("Selesai").color(GREEN).strong());
                    ui.label(RichText::new(headline).size(24.0).strong());
                    if let (Some(p), Some(_)) = (profile, s) {
                        if let (Some(b), Some(r)) = (&p.baseline, &p.result) {
                            ui.label(savings_text(b, r));
                        }
                    }
                    ui.label(muted("Profil sudah terpasang dan disimpan. Nyalakan \"Pasang otomatis saat Windows menyala\" di Beranda supaya tetap dipakai setelah restart."));
                    ui.label(muted("Main game seperti biasa beberapa hari. Bila ada game yang crash atau driver reset, ulangi pencarian dengan margin lebih besar."));
                });
                ui.add_space(12.0);
            }
            Phase::NoUndervolt => {
                banner(ui, AMBER, headline, "Coba frekuensi target yang lebih rendah (misalnya 60 MHz di bawah clock bawaan). Undervolt dengan sedikit penurunan clock sering memberi penurunan daya terbesar.");
                ui.add_space(12.0);
            }
            Phase::Failed => {
                banner(ui, RED, "Pencarian berhenti", headline);
                ui.add_space(12.0);
            }
            Phase::Stopped => {
                banner(ui, MUTED, "Dihentikan", "Kurva sudah dikembalikan ke bawaan. Tekan Mulai dengan sesi yang sama untuk melanjutkan.");
                if let Some(s) = s {
                    if !s.finished {
                        self.pending = Some(s.clone());
                    }
                }
                ui.add_space(12.0);
            }
            _ => {}
        }
    }

    fn settings_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.label(RichText::new("Aplikasi beban 3D").size(17.0).strong());
            ui.label(muted("Dijalankan otomatis dan dijalankan ulang bila crash. Kosongkan bila ingin membukanya sendiri."));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.settings.load_exe).hint_text("C:\\...\\Heaven.exe").desired_width(ui.available_width() - 210.0));
                if ui.button("Pilih...").clicked() {
                    if let Some(p) = pick_exe() {
                        self.settings.load_exe = p;
                    }
                }
                if ui.button("Unigine Heaven").on_hover_text("Isi otomatis jalur dan argumen Heaven 4.0: setara preset Extreme (DirectX 11, Ultra, Tessellation Extreme, AA 8x), jendela 1600x900").clicked() {
                    match crate::volt::presets::detect_heaven() {
                        Some((exe, args)) => {
                            self.settings.load_exe = exe;
                            self.settings.load_args = args;
                        }
                        None => {
                            self.settings.load_args = crate::volt::presets::HEAVEN_ARGS.into();
                            self.toast("Heaven belum terpasang: pilih Heaven.exe di folder bin");
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Argumen");
                ui.add(egui::TextEdit::singleline(&mut self.settings.load_args).desired_width(ui.available_width()));
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("Target").size(17.0).strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.custom_target, false, "Samakan clock bawaan (performa sama, daya turun)");
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.custom_target, true, "Frekuensi tertentu");
                ui.add_enabled(self.custom_target, egui::DragValue::new(&mut self.custom_mhz).range(1200..=2200).speed(15).suffix(" MHz"));
            });
            ui.add_space(8.0);
            egui::CollapsingHeader::new(RichText::new("Pengaturan lanjutan").color(MUTED)).show(ui, |ui| {
                egui::Grid::new("adv").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    let s = &mut self.settings;
                    ui.label("Langkah turun");
                    ui.add(egui::DragValue::new(&mut s.step_mv).range(6..=100).suffix(" mV"));
                    ui.end_row();
                    let mut m = s.step_secs / 60;
                    ui.label("Lama tiap uji");
                    if ui.add(egui::DragValue::new(&mut m).range(1..=30).suffix(" menit")).changed() {
                        s.step_secs = m * 60;
                    }
                    ui.end_row();
                    let mut m = s.validate_secs / 60;
                    ui.label("Lama validasi akhir");
                    if ui.add(egui::DragValue::new(&mut m).range(5..=120).suffix(" menit")).changed() {
                        s.validate_secs = m * 60;
                    }
                    ui.end_row();
                    ui.label("Margin keamanan");
                    ui.add(egui::DragValue::new(&mut s.margin_mv).range(0..=75).suffix(" mV"));
                    ui.end_row();
                    ui.label("Tegangan minimum");
                    ui.add(egui::DragValue::new(&mut s.min_mv).range(650..=950).suffix(" mV"));
                    ui.end_row();
                    ui.label("Offset maksimum");
                    ui.add(egui::DragValue::new(&mut s.max_offset_mhz).range(50..=600).suffix(" MHz"))
                        .on_hover_text("Batas kenaikan frekuensi di satu titik tegangan dibanding bawaan. Mencegah uji ekstrem yang bisa membuat PC hang.");
                    ui.end_row();
                    ui.label("Batas suhu");
                    ui.add(egui::DragValue::new(&mut s.temp_limit_c).range(70..=95).suffix(" °C"));
                    ui.end_row();
                    ui.label("Pengukuran bawaan");
                    ui.add(egui::DragValue::new(&mut s.baseline_secs).range(20..=600).suffix(" detik"));
                    ui.end_row();
                });
                setting_row(ui, &mut self.settings.use_checker, "Pemeriksa error komputasi", "Compute shader kecil yang memeriksa apakah GPU salah hitung. Menangkap ketidakstabilan sebelum game crash.");
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            let est = (self.settings.baseline_secs + self.settings.step_secs * 8 + self.settings.validate_secs) / 60;
            ui.label(strong("Sebelum mulai"));
            ui.label(muted(format!(
                "Perkiraan {est} menit. Tutup game dan aplikasi berat lain, jangan pakai PC selama pengujian, dan pastikan \
                 MSI Afterburner, AMD Adrenalin (Tuning) atau aplikasi overclock lain tidak memasang profilnya. Layar bisa berkedip atau aplikasi 3D \
                 bisa tertutup saat titik tidak stabil ditemukan: itu normal, Pacu Volt akan pulih dan melanjutkan."
            )));
            ui.add_space(6.0);
            ui.checkbox(&mut self.confirm, "Saya paham undervolt bisa membuat game crash selama pengujian, dan semua risikonya saya tanggung");
            ui.add_space(6.0);
            let busy = self.sh.view.read().phase().busy();
            let can = self.confirm && !busy && self.sh.view.read().gpu_error.is_none();
            if ui.add_enabled(can, accent_button("Mulai pencarian").min_size(Vec2::new(200.0, 38.0))).clicked() {
                self.settings.target_mhz = self.custom_target.then_some(self.custom_mhz);
                self.settings.save();
                if self.pending.is_some() {
                    Session::discard();
                    self.pending = None;
                }
                self.crashed_note = None;
                self.send(Cmd::Start { settings: self.settings.clone(), resume: None });
            }
        });
    }

    // ── Kurva ──────────────────────────────────────────────────────────────────

    fn page_curve(&mut self, ui: &mut egui::Ui) {
        let (curve, profile, tel, err) = {
            let v = self.sh.view.read();
            (v.curve.clone(), v.profile.clone(), v.telemetry.clone(), v.gpu_error.clone())
        };
        ui.label(subtitle("Kurva V/F"));
        ui.label(muted("Abu-abu: kurva bawaan pabrik. Ungu: kurva yang sedang terpasang. Titik kuning: posisi GPU sekarang."));
        if is_amd(&self.sh.view.read().info) {
            ui.label(muted(
                "GPU AMD tidak membuka kurva per titik. Kurva ini model dari dua slider Adrenalin: ujung datar ungu adalah \
                 Frekuensi maksimum, dan titik mulai datarnya adalah Tegangan maksimum.",
            ));
        }
        ui.add_space(12.0);
        if let Some(e) = err {
            banner(ui, RED, "Kurva tidak terbaca", &e);
            return;
        }
        card(ui, |ui| {
            let now = tel.voltage_mv.zip(tel.clock_mhz.map(|c| c as f32));
            curve_chart(ui, &curve, now, 360.0);
            ui.horizontal(|ui| {
                if ui.button("Muat ulang").clicked() {
                    self.send(Cmd::RefreshCurve);
                }
                if profile.is_some() && ui.button("Pasang profil").clicked() {
                    self.send(Cmd::ApplyProfile);
                }
                if ui.button("Kembalikan ke bawaan").clicked() {
                    self.send(Cmd::ResetStock);
                }
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new(format!("{} titik grafis", curve.points.len())).strong());
            egui::Grid::new("pts").num_columns(5).striped(true).spacing([24.0, 4.0]).show(ui, |ui| {
                for h in ["#", "Tegangan", "Bawaan", "Offset", "Terpasang"] {
                    ui.label(muted(h));
                }
                ui.end_row();
                for p in &curve.points {
                    ui.label(muted(p.index.to_string()));
                    ui.label(format!("{:.1} mV", p.volt_mv()));
                    ui.label(format!("{:.0} MHz", p.stock_mhz()));
                    let off = p.delta_khz as f32 / 1000.0;
                    ui.label(RichText::new(format!("{off:+.0} MHz")).color(if off > 0.0 { GREEN } else if off < 0.0 { AMBER } else { MUTED }));
                    ui.label(format!("{:.0} MHz", p.effective_mhz()));
                    ui.end_row();
                }
            });
        });
    }

    // ── Log ────────────────────────────────────────────────────────────────────

    fn page_log(&mut self, ui: &mut egui::Ui) {
        ui.label(subtitle("Log & Diagnostik"));
        ui.label(muted(format!("Log juga disimpan di {}", store::dir().join("pacuvolt.log").display())));
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(strong("Laporan diagnostik"));
                if ui.button("Buat laporan").clicked() {
                    self.send(Cmd::Diagnostics);
                }
            });
            ui.label(muted("Berisi nama GPU, versi driver, data mentah NVAPI atau ADLX, dan event driver terakhir. Kirim ini bila ada yang tidak berjalan."));
            let d = self.sh.view.read().diagnostics.clone();
            if let Some(mut d) = d {
                ui.horizontal(|ui| {
                    if ui.button("Salin").clicked() {
                        ui.ctx().copy_text(d.clone());
                        self.toast("Disalin");
                    }
                    if ui.button("Simpan ke file").clicked() {
                        let p = store::dir().join("diagnostik.txt");
                        if std::fs::write(&p, &d).is_ok() {
                            self.toast(format!("Disimpan: {}", p.display()));
                        }
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut d).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(14));
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(strong("Aktivitas"));
            let log = self.sh.view.read().log.clone();
            if log.is_empty() {
                ui.label(muted("Belum ada."));
            }
            for l in log.iter().rev().take(400) {
                let c = match l.kind {
                    LogKind::Info => TEXT,
                    LogKind::Good => GREEN,
                    LogKind::Warn => AMBER,
                    LogKind::Bad => RED,
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(muted(&l.time));
                    ui.label(RichText::new(&l.text).color(c));
                });
            }
        });
    }
}

// ── Komponen ───────────────────────────────────────────────────────────────────

fn is_amd(info: &crate::volt::gpu::GpuInfo) -> bool {
    info.backend.starts_with("ADLX")
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
}

fn mmss(s: u32) -> String {
    format!("{}:{:02}", s / 60, s % 60)
}

fn phase_style(p: Phase) -> (Color32, &'static str) {
    match p {
        Phase::Idle => (MUTED, "Siap"),
        Phase::Preparing => (CYAN, "Menyiapkan"),
        Phase::WaitingLoad => (AMBER, "Menunggu beban"),
        Phase::Baseline => (CYAN, "Mengukur bawaan"),
        Phase::Testing => (accent(), "Menguji"),
        Phase::Recovering => (AMBER, "Memulihkan"),
        Phase::Done => (GREEN, "Selesai"),
        Phase::NoUndervolt => (AMBER, "Tidak ada undervolt"),
        Phase::Failed => (RED, "Gagal"),
        Phase::Stopped => (MUTED, "Dihentikan"),
    }
}

fn tile(ui: &mut egui::Ui, label: &str, value: String, unit: &str, color: Color32) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, theme::STROKE))
        .corner_radius(CornerRadius::same(theme::RADIUS_CARD))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(muted(label));
        ui.horizontal(|ui| {
            let _ = color;
            ui.label(RichText::new(value).font(theme::semibold(22.0)).color(TEXT));
            ui.label(muted(unit));
        });
    });
}

fn tiles(ui: &mut egui::Ui, t: &Telemetry) {
    let lim = if t.limits.is_empty() { "-".to_string() } else { t.limits.join(", ") };
    let f = |v: Option<f32>| v.map(|x| format!("{x:.0}")).unwrap_or_else(|| "-".into());
    ui.columns(6, |c| {
        tile(&mut c[0], "Clock", opt(t.clock_mhz), "MHz", accent());
        tile(&mut c[1], "Tegangan", f(t.voltage_mv), "mV", CYAN);
        tile(&mut c[2], "Daya", f(t.power_w), &format!("W / {}", f(t.power_limit_w)), VIOLET);
        tile(&mut c[3], "Suhu", opt(t.temp_c), "°C", if t.temp_c.unwrap_or(0) >= 80 { AMBER } else { TEXT });
        tile(&mut c[4], "Pemakaian GPU", opt(t.util_pct), "%", TEXT);
        tile(&mut c[5], "Dibatasi oleh", lim, "", if t.limits.is_empty() { MUTED } else { AMBER });
    });
}

fn tiles_compact(ui: &mut egui::Ui, t: &Telemetry) {
    ui.horizontal_wrapped(|ui| {
        let item = |ui: &mut egui::Ui, l: &str, v: String, c: Color32| {
            ui.label(muted(l));
            ui.label(strong(v).color(c));
            ui.add_space(10.0);
        };
        item(ui, "Clock", format!("{} MHz", opt(t.clock_mhz)), accent());
        item(ui, "Tegangan", format!("{} mV", t.voltage_mv.map(|v| format!("{v:.0}")).unwrap_or("-".into())), CYAN);
        item(ui, "Daya", format!("{} W", t.power_w.map(|v| format!("{v:.0}")).unwrap_or("-".into())), VIOLET);
        item(ui, "Suhu", format!("{}°C", opt(t.temp_c)), TEXT);
        item(ui, "GPU", format!("{}%", opt(t.util_pct)), TEXT);
    });
}

fn history_chart(ui: &mut egui::Ui, data: &VecDeque<Sample>, height: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(6), BG);
    for frac in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * frac;
        p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], Stroke::new(1.0, GRID));
    }
    if data.len() < 2 {
        return;
    }
    let n = 300usize;
    let step = rect.width() / (n - 1) as f32;
    let start = n.saturating_sub(data.len());
    let series: [(fn(&Sample) -> f32, Color32, f32, f32); 2] = [(|s| s.clock, GREEN, 0.0, 2200.0), (|s| s.power, VIOLET, 0.0, 400.0)];
    for (f, color, lo, hi) in series {
        let pts: Vec<egui::Pos2> = data
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let v = ((f(s) - lo) / (hi - lo)).clamp(0.0, 1.0);
                egui::pos2(rect.left() + (start + i) as f32 * step, rect.bottom() - 4.0 - v * (rect.height() - 8.0))
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.8, color)));
    }
    let last = data.back().unwrap();
    p.text(rect.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, format!("{:.0} MHz", last.clock), FontId::proportional(12.0), accent());
    p.text(rect.left_top() + Vec2::new(90.0, 6.0), egui::Align2::LEFT_TOP, format!("{:.0} W", last.power), FontId::proportional(12.0), VIOLET);
}

fn curve_chart(ui: &mut egui::Ui, c: &Curve, now: Option<(f32, f32)>, height: f32) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(6), BG);
    if c.points.len() < 2 {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "Kurva belum terbaca", FontId::proportional(14.0), MUTED);
        return;
    }
    let plot = egui::Rect::from_min_max(rect.min + Vec2::new(52.0, 12.0), rect.max - Vec2::new(12.0, 28.0));
    let (v0, v1) = (c.points.first().unwrap().volt_mv().min(700.0), c.points.last().unwrap().volt_mv().max(1100.0));
    let f_max = c.points.iter().map(|p| p.stock_mhz().max(p.effective_mhz())).fold(0.0, f32::max);
    let (f0, f1) = (0.0f32.max(c.points.iter().map(|p| p.stock_mhz().min(p.effective_mhz())).fold(f32::MAX, f32::min) - 100.0), f_max + 100.0);
    let to = |mv: f32, mhz: f32| egui::pos2(plot.left() + (mv - v0) / (v1 - v0) * plot.width(), plot.bottom() - (mhz - f0) / (f1 - f0) * plot.height());
    let mut mv = (v0 / 50.0).ceil() * 50.0;
    while mv <= v1 {
        let x = to(mv, f0).x;
        p.line_segment([egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], Stroke::new(1.0, GRID));
        p.text(egui::pos2(x, plot.bottom() + 4.0), egui::Align2::CENTER_TOP, format!("{mv:.0}"), FontId::proportional(11.0), MUTED);
        mv += 50.0;
    }
    let mut mhz = (f0 / 250.0).ceil() * 250.0;
    while mhz <= f1 {
        let y = to(v0, mhz).y;
        p.line_segment([egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)], Stroke::new(1.0, GRID));
        p.text(egui::pos2(plot.left() - 6.0, y), egui::Align2::RIGHT_CENTER, format!("{mhz:.0}"), FontId::proportional(11.0), MUTED);
        mhz += 250.0;
    }
    p.text(egui::pos2(plot.right(), rect.bottom() - 2.0), egui::Align2::RIGHT_BOTTOM, "mV", FontId::proportional(11.0), MUTED);
    p.text(egui::pos2(rect.left() + 4.0, plot.top()), egui::Align2::LEFT_TOP, "MHz", FontId::proportional(11.0), MUTED);
    let stock: Vec<egui::Pos2> = c.points.iter().map(|q| to(q.volt_mv(), q.stock_mhz())).collect();
    let eff: Vec<egui::Pos2> = c.points.iter().map(|q| to(q.volt_mv(), q.effective_mhz())).collect();
    p.add(egui::Shape::line(stock, Stroke::new(1.5, MUTED)));
    p.add(egui::Shape::line(eff.clone(), Stroke::new(2.2, accent())));
    for q in &eff {
        p.circle_filled(*q, 2.2, accent());
    }
    if let Some((v, f)) = now {
        if v > 0.0 && f > 0.0 {
            p.circle_filled(to(v, f), 5.0, AMBER);
        }
    }
    if let Some(h) = resp.hover_pos() {
        if plot.contains(h) {
            let nearest = c.points.iter().min_by(|a, b| (to(a.volt_mv(), a.effective_mhz()).x - h.x).abs().total_cmp(&(to(b.volt_mv(), b.effective_mhz()).x - h.x).abs()));
            if let Some(q) = nearest {
                let pos = to(q.volt_mv(), q.effective_mhz());
                p.circle_stroke(pos, 6.0, Stroke::new(1.5, Color32::WHITE));
                p.text(
                    pos + Vec2::new(8.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{:.0} mV: {:.0} MHz (bawaan {:.0})", q.volt_mv(), q.effective_mhz(), q.stock_mhz()),
                    FontId::proportional(12.0),
                    TEXT,
                );
            }
        }
    }
}

fn results_card(ui: &mut egui::Ui, s: &Session) {
    card(ui, |ui| {
        ui.label(RichText::new("Hasil uji").size(17.0).strong());
        if let Some(b) = &s.baseline {
            ui.label(muted(format!(
                "Bawaan: {:.0} MHz @ {:.0} mV, {:.0} W, maks {:.0}°C, pemakaian GPU {:.0}%. Target {} MHz.",
                b.clock_mhz,
                b.volt_mv,
                b.power_w,
                b.temp_c,
                b.util_pct,
                s.target_khz / 1000
            )));
        }
        egui::Grid::new("results").num_columns(7).striped(true).spacing([18.0, 4.0]).show(ui, |ui| {
            for h in ["Uji", "Tegangan", "Target", "Hasil", "Daya", "Suhu", "Keterangan"] {
                ui.label(muted(h));
            }
            ui.end_row();
            for r in &s.results {
                ui.label(if r.kind == TestKind::Validate { "Validasi" } else { "Cari" });
                ui.label(format!("{:.0} mV", r.volt_mv));
                ui.label(format!("{} MHz", r.target_mhz));
                ui.label(if r.passed { RichText::new("Stabil").color(GREEN) } else { RichText::new("Gagal").color(RED) });
                match &r.measure {
                    Some(m) => {
                        ui.label(format!("{:.0} W", m.power_w));
                        ui.label(format!("{:.0}°C", m.temp_c));
                    }
                    None => {
                        ui.label("-");
                        ui.label("-");
                    }
                }
                ui.label(muted(if r.passed { format!("clock {:.0} MHz", r.measure.as_ref().map(|m| m.clock_mhz).unwrap_or(0.0)) } else { r.reason.clone() }));
                ui.end_row();
            }
        });
    });
}

fn savings_text(b: &crate::volt::store::Measure, r: &crate::volt::store::Measure) -> RichText {
    let dp = b.power_w - r.power_w;
    let pct = if b.power_w > 0.0 { dp / b.power_w * 100.0 } else { 0.0 };
    RichText::new(format!(
        "Daya {:.0} W ke {:.0} W ({:.0}% lebih hemat) · suhu maks {:.0}°C ke {:.0}°C · clock {:.0} ke {:.0} MHz",
        b.power_w, r.power_w, pct, b.temp_c, r.temp_c, b.clock_mhz, r.clock_mhz
    ))
    .color(GREEN)
}

fn profile_active(c: &Curve, p: &Profile) -> bool {
    !p.deltas.is_empty() && crate::volt::curve::deltas_match(c, &p.deltas, 1_000)
}

fn estimate(s: &Session) -> String {
    let left_search = 4u32.saturating_sub(s.results.iter().filter(|r| r.kind == TestKind::Search).count() as u32 / 2);
    let secs = left_search * s.settings.step_secs + if s.results.iter().any(|r| r.kind == TestKind::Validate && r.passed) { 0 } else { s.settings.validate_secs };
    format!("±{} menit", secs.div_ceil(60))
}

fn pick_exe() -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().add_filter("Program", &["exe"]).pick_file().map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        None
    }
}
