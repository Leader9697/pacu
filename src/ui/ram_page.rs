//! Halaman RAM Ekstrem: merampingkan RAM terpakai, mematikan layanan dan aplikasi latar, dan
//! mengatur aplikasi yang ikut berjalan saat Windows menyala.

use super::*;
use crate::startup;
use std::collections::HashMap;

fn gb(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1024.0)
}

const INTERVALS: [u64; 6] = [1, 3, 5, 10, 15, 30];

impl PacuApp {
    pub(super) fn page_ram(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "RAM Ekstrem", "Menekan pemakaian RAM saat tidak bermain: merampingkan proses, mematikan layanan latar, dan menyaring aplikasi startup.");
        self.ram_now_card(ui);
        ui.add_space(10.0);
        self.ram_target_note(ui);
        ui.add_space(14.0);

        self.tweak_header(ui, "Layanan dan aplikasi latar", tweaks::Group::Ram);
        ui.label(muted(tweaks::RAM_TWEAKS_NOTE));
        ui.add_space(4.0);
        self.tweak_cards(ui, tweaks::Group::Ram);
        ui.add_space(14.0);

        self.startup_section(ui);
        ui.add_space(14.0);
        self.top_ram_card(ui);
        ui.add_space(4.0);
        ui.label(muted("Pacu tidak mematikan Defender, Firewall, Windows Update, atau VBS / Memory Integrity."));
    }

    fn ram_now_card(&mut self, ui: &mut egui::Ui) {
        let mem = self.snap.mem.clone();
        let used = mem.total_mb.saturating_sub(mem.avail_mb);
        let last = self.snap.last_trim.clone();
        let mut changed = false;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(muted("RAM terpakai"));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(gb(used)).font(theme::semibold(28.0)));
                        ui.label(muted(format!("GB dari {} GB", gb(mem.total_mb))));
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let b = accent_button("Rampingkan sekarang");
                    if ui.add(b).clicked() {
                        self.send(Command::TrimRam);
                        self.status = Some(("Merampingkan RAM...".into(), 0.0));
                    }
                });
            });
            let frac = if mem.total_mb > 0 { used as f32 / mem.total_mb as f32 } else { 0.0 };
            bar(ui, frac, accent());
            ui.add_space(4.0);
            match &last {
                Some(r) => {
                    let how = if r.auto { "otomatis" } else { "manual" };
                    ui.label(RichText::new(format!(
                        "Terakhir ({how}, {:02}:{:02}): {} GB ke {} GB, hemat {} GB",
                        r.at.0,
                        r.at.1,
                        gb(r.before_mb),
                        gb(r.after_mb),
                        gb(r.before_mb.saturating_sub(r.after_mb))
                    ))
                    .color(GREEN));
                }
                None => {
                    ui.label(muted("Belum dirampingkan sejak Pacu dibuka."));
                }
            }
            ui.add_space(8.0);
            let rc = &mut self.cfg.ram;
            changed |= setting_row(
                ui,
                &mut rc.auto_trim,
                "Rampingkan otomatis saat tidak bermain",
                &format!("Tiap {} menit bila RAM terpakai di atas {} GB. Mati selama Mode Game supaya game tidak tersendat.", rc.interval_min, gb(rc.min_used_mb)),
            );
            ui.horizontal(|ui| {
                ui.label(muted("Interval"));
                egui::ComboBox::from_id_salt("ram_interval").selected_text(format!("{} menit", rc.interval_min)).width(100.0).show_ui(ui, |ui| {
                    for m in INTERVALS {
                        changed |= ui.selectable_value(&mut rc.interval_min, m, format!("{m} menit")).changed();
                    }
                });
                ui.add_space(12.0);
                ui.label(muted("Bila terpakai di atas"));
                egui::ComboBox::from_id_salt("ram_min").selected_text(format!("{} GB", gb(rc.min_used_mb))).width(90.0).show_ui(ui, |ui| {
                    for mb in [1024u64, 1536, 2048, 3072, 4096] {
                        changed |= ui.selectable_value(&mut rc.min_used_mb, mb, format!("{} GB", gb(mb))).changed();
                    }
                });
            });
        });
        if changed {
            self.apply_config();
        }
    }

    fn ram_target_note(&mut self, ui: &mut egui::Ui) {
        banner(
            ui,
            theme::TEXT_2,
            "Soal target 1 GB",
                    "Windows 11 sendiri (kernel, Explorer, Defender, driver GPU, dan layanan inti) biasanya sudah memakai lebih dari 1 GB. \
                     Angka di bawah 1 GB hanya bisa didapat dengan membuang Defender dan komponen inti, dan itu tidak dilakukan Pacu. \
                     Setelah dirampingkan, angkanya langsung turun, lalu naik lagi pelan-pelan saat aplikasi dipakai. Itu normal: \
                     halaman yang dibutuhkan dibaca balik dari RAM standby, bukan dari disk.",
        );
    }

    fn startup_section(&mut self, ui: &mut egui::Ui) {
        if !self.startup_loaded {
            self.startup = startup::list();
            self.startup_loaded = true;
        }
        let on = self.startup.iter().filter(|e| e.enabled).count();
        ui.horizontal(|ui| {
            ui.label(section("Aplikasi saat Windows menyala"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Muat ulang").clicked() {
                    self.startup = startup::list();
                }
            });
        });
        ui.label(muted(format!(
            "{on} dari {} aplikasi ikut berjalan saat login. Matikan yang tidak perlu (Discord, Steam, OneDrive, Teams, launcher game): \
             aplikasinya tetap terpasang dan bisa dibuka manual. Sama seperti tab Startup di Task Manager.",
            self.startup.len()
        )));
        ui.add_space(8.0);
        let mut toggled: Option<(usize, bool)> = None;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.startup.is_empty() {
                ui.label(muted("Tidak ada aplikasi startup yang terbaca."));
            }
            for (i, e) in self.startup.iter().enumerate() {
                ui.horizontal(|ui| {
                    let mut v = e.enabled;
                    if toggle(ui, &mut v).changed() {
                        toggled = Some((i, v));
                    }
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(strong(&e.name).color(if e.enabled { TEXT } else { MUTED }));
                            ui.label(RichText::new(e.source).color(MUTED).size(12.0));
                        });
                        ui.add(egui::Label::new(RichText::new(&e.command).color(MUTED).size(12.0)).truncate());
                    });
                });
                ui.add_space(4.0);
            }
        });
        if let Some((i, v)) = toggled {
            let e = self.startup[i].clone();
            match startup::set_enabled(&e, v) {
                Ok(()) => {
                    let verb = if v { "Dinyalakan" } else { "Dimatikan" };
                    self.status = Some((format!("{verb} saat startup: {}", e.name), 0.0));
                    self.send(Command::Log(format!("Startup {}: {}", if v { "nyala" } else { "mati" }, e.name)));
                    self.startup = startup::list();
                }
                Err(err) => self.status = Some((format!("Gagal: {err}"), 0.0)),
            }
        }
    }

    fn top_ram_card(&mut self, ui: &mut egui::Ui) {
        let mut by_name: HashMap<String, (f32, u32)> = HashMap::new();
        for p in &self.snap.procs {
            let e = by_name.entry(p.name.clone()).or_default();
            e.0 += p.mem_mb;
            e.1 += 1;
        }
        let mut top: Vec<(String, f32, u32)> = by_name.into_iter().map(|(n, (m, c))| (n, m, c)).collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        top.truncate(10);
        ui.label(section("Pemakan RAM terbesar"));
        ui.label(muted("Working set per aplikasi, digabung per nama. Tutup atau bekukan yang tidak dipakai saat bermain (halaman Mode Game, bagian Bekukan aplikasi latar)."));
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let max = top.first().map(|t| t.1).unwrap_or(1.0).max(1.0);
            for (name, mb, count) in &top {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(220.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_width(220.0);
                        let label = if *count > 1 { format!("{name} ({count})") } else { name.clone() };
                        ui.add(egui::Label::new(RichText::new(label).color(TEXT)).truncate());
                    });
                    ui.allocate_ui_with_layout(Vec2::new(90.0, 20.0), Layout::right_to_left(Align::Center), |ui| {
                        ui.set_width(90.0);
                        ui.label(RichText::new(format!("{mb:.0} MB")).color(theme::TEXT_2));
                    });
                    ui.add_space(8.0);
                    bar(ui, mb / max, accent());
                });
            }
        });
    }
}
