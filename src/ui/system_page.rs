//! Bagian Pengaturan untuk pembaruan otomatis dan perangkat yang boleh membangunkan PC.

use super::*;
use crate::update::{self, Phase};
use crate::wake;

/// Pemeriksaan otomatis tiap 6 jam selama Pacu menyala.
const CHECK_EVERY_SECS: f64 = 6.0 * 3600.0;

fn mb(b: u64) -> String {
    format!("{:.1} MB", b as f64 / 1_048_576.0)
}

impl PacuApp {
    /// Dipanggil tiap frame: pemeriksaan otomatis saat menyala dan berkala.
    pub(super) fn update_tick(&mut self, now: f64) {
        if !self.cfg.update.auto_check || self.updater.busy() {
            return;
        }
        if self.update_next_check <= now {
            self.update_next_check = now + CHECK_EVERY_SECS;
            let idle = matches!(self.updater.state.lock().phase, Phase::Idle | Phase::UpToDate | Phase::Failed(_));
            if idle {
                self.updater.check(&self.cfg.update.repo);
            }
        }
    }

    fn restart_into_update(&mut self, ctx: &egui::Context) {
        let _ = update::RESTART.set(());
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// InfoBar di Beranda saat ada versi baru atau pembaruan siap dipasang.
    pub(super) fn update_banner(&mut self, ui: &mut egui::Ui) {
        let phase = self.updater.state.lock().phase.clone();
        match phase {
            Phase::Available(rel) => {
                banner(ui, theme::TEXT_2, &format!("Pacu {} tersedia", rel.version), &format!("Kamu memakai versi {}.", update::current_version()));
                ui.horizontal(|ui| {
                    if ui.add(accent_button("Perbarui sekarang")).clicked() {
                        self.updater.install(rel.clone());
                    }
                    if ui.button("Lihat detail").clicked() {
                        self.page = Page::Settings;
                    }
                });
                ui.add_space(8.0);
            }
            Phase::Downloading { got, total } => {
                banner(ui, theme::TEXT_2, "Mengunduh pembaruan", &format!("{} dari {}", mb(got), mb(total)));
                ui.add_space(8.0);
            }
            Phase::Ready(v) => {
                banner(ui, GREEN, &format!("Pacu {v} siap dipakai"), "Mulai ulang Pacu untuk memakai versi baru. Pengaturan dan profil tetap tersimpan.");
                if ui.add(accent_button("Mulai ulang sekarang")).clicked() {
                    let ctx = ui.ctx().clone();
                    self.restart_into_update(&ctx);
                }
                ui.add_space(8.0);
            }
            _ => {}
        }
    }

    pub(super) fn update_card(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let (phase, last) = {
            let s = self.updater.state.lock();
            (s.phase.clone(), s.last_check)
        };
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon_text(theme::icon::REFRESH, 18.0).color(accent()));
                ui.vertical(|ui| {
                    ui.label(section("Pembaruan Pacu"));
                    ui.label(muted(format!("Versi terpasang {}", update::current_version())));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let busy = self.updater.busy();
                    if ui.add_enabled(!busy, egui::Button::new("Periksa pembaruan")).clicked() {
                        self.updater.check(&self.cfg.update.repo);
                    }
                });
            });
            ui.add_space(6.0);
            match &phase {
                Phase::Idle => {
                    ui.label(muted("Belum diperiksa."));
                }
                Phase::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(muted("Memeriksa GitHub..."));
                    });
                }
                Phase::UpToDate => {
                    ui.horizontal(|ui| {
                        theme::tag(ui, "Terbaru", GREEN);
                        ui.label(muted("Kamu sudah memakai versi terbaru."));
                    });
                }
                Phase::Available(rel) => {
                    ui.horizontal(|ui| {
                        theme::tag(ui, &format!("Versi {} tersedia", rel.version), accent());
                        ui.label(muted(mb(rel.size)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(accent_button("Unduh dan pasang")).clicked() {
                                self.updater.install(rel.clone());
                            }
                        });
                    });
                    if !rel.notes.is_empty() {
                        ui.add_space(4.0);
                        ui.label(strong("Yang baru"));
                        egui::ScrollArea::vertical().id_salt("release-notes").max_height(160.0).show(ui, |ui| {
                            ui.label(RichText::new(&rel.notes).color(theme::TEXT_2).size(13.0));
                        });
                    }
                }
                Phase::Downloading { got, total } => {
                    let frac = if *total > 0 { *got as f32 / *total as f32 } else { 0.0 };
                    ui.label(muted(format!("Mengunduh {} dari {}", mb(*got), mb(*total))));
                    bar(ui, frac, accent());
                }
                Phase::Ready(v) => {
                    ui.horizontal(|ui| {
                        theme::tag(ui, &format!("Versi {v} terpasang"), GREEN);
                        ui.label(muted("SHA-256 cocok. Mulai ulang untuk memakainya."));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(accent_button("Mulai ulang sekarang")).clicked() {
                                let ctx = ui.ctx().clone();
                                self.restart_into_update(&ctx);
                            }
                        });
                    });
                }
                Phase::Failed(e) => {
                    ui.horizontal(|ui| {
                        theme::tag(ui, "Gagal", RED);
                        ui.label(muted(e.clone()));
                    });
                }
            }
            if last > 0 {
                let ago = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().saturating_sub(last))
                    .unwrap_or(0);
                ui.label(muted(format!("Diperiksa {} lalu dari github.com/{}", fmt_duration(Duration::from_secs(ago)), self.cfg.update.repo)));
            }
            divider(ui);
            changed |= setting_row(
                ui,
                &mut self.cfg.update.auto_check,
                "Periksa pembaruan otomatis",
                "Saat Pacu menyala dan tiap 6 jam. Pacu tidak memasang apa pun tanpa kamu menekan tombol.",
            );
        });
        changed
    }

    pub(super) fn wake_card(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if self.wake_list.is_none() {
            self.wake_list = Some(wake::list());
        }
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon_text(theme::icon::SHIELD, 18.0).color(accent()));
                ui.vertical(|ui| {
                    ui.label(section("Bangun dari sleep"));
                    ui.label(muted("Pilih perangkat yang boleh membangunkan PC. Sama dengan pengaturan Power Management di Device Manager."));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Muat ulang").clicked() {
                        self.wake_list = Some(wake::list());
                    }
                });
            });
            ui.add_space(6.0);
            let list = match self.wake_list.clone() {
                Some(Ok(l)) => l,
                Some(Err(e)) => {
                    banner(ui, RED, "Daftar perangkat tidak terbaca", &e);
                    return;
                }
                None => Vec::new(),
            };
            if list.is_empty() {
                ui.label(muted("Tidak ada perangkat yang bisa membangunkan PC."));
                return;
            }
            let peripherals: Vec<String> = list.iter().filter(|d| matches!(d.kind, wake::Kind::Mouse | wake::Kind::Keyboard)).map(|d| d.name.clone()).collect();
            let all: Vec<String> = list.iter().map(|d| d.name.clone()).collect();
            let mut bulk: Option<(Vec<String>, bool)> = None;
            ui.horizontal(|ui| {
                if ui.add_enabled(!peripherals.is_empty(), accent_button("Mouse dan keyboard tidak membangunkan")).clicked() {
                    bulk = Some((peripherals.clone(), false));
                }
                if ui.button("Hanya tombol power").on_hover_text("Semua perangkat di daftar ini tidak bisa membangunkan PC").clicked() {
                    bulk = Some((all.clone(), false));
                }
                if ui.button("Izinkan semua").clicked() {
                    bulk = Some((all.clone(), true));
                }
            });
            ui.add_space(4.0);
            let mut single: Option<(String, bool)> = None;
            for d in &list {
                let mut on = d.armed;
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    theme::tag(ui, d.kind.label(), theme::TEXT_2);
                    ui.add(egui::Label::new(RichText::new(&d.name).color(TEXT)).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if toggle(ui, &mut on).changed() {
                            single = Some((d.name.clone(), on));
                        }
                        ui.label(RichText::new(if d.armed { "Boleh membangunkan" } else { "Tidak membangunkan" }).color(theme::TEXT_2).size(13.0));
                    });
                });
            }
            let mut todo: Vec<(String, bool)> = Vec::new();
            if let Some((names, armed)) = bulk {
                todo.extend(names.into_iter().map(|n| (n, armed)));
            }
            if let Some(s) = single {
                todo.push(s);
            }
            if !todo.is_empty() {
                let mut errors = Vec::new();
                for (name, armed) in &todo {
                    match wake::set(name, *armed) {
                        Ok(()) => {
                            let b = &mut self.cfg.wake.blocked;
                            b.retain(|x| x != name);
                            if !armed {
                                b.push(name.clone());
                            }
                            changed = true;
                        }
                        Err(e) => errors.push(format!("{name}: {e}")),
                    }
                }
                self.wake_list = Some(wake::list());
                self.status = Some((if errors.is_empty() { "Pengaturan bangun dari sleep diperbarui".into() } else { errors.join("; ") }, 0.0));
            }
            ui.add_space(4.0);
            ui.label(muted("Pilihan ini dipasang ulang setiap Pacu menyala, karena update driver kadang menyalakannya kembali. Tombol power tetap selalu bisa membangunkan PC."));
        });
        changed
    }
}
