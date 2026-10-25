//! Halaman BIOS & Hardware: identitas motherboard/BIOS, status XMP dan Resizable BAR, dan daftar
//! pengaturan BIOS yang disarankan. Hanya membaca; perubahan dilakukan sendiri di layar BIOS.

use super::*;
use crate::bios::{self, Level, Rebar};
use std::sync::atomic::Ordering;

fn level_tag(l: Level) -> (&'static str, Color32) {
    match l {
        Level::Penting => ("Penting", RED),
        Level::Saran => ("Saran", AMBER),
        Level::Opsional => ("Opsional", CYAN),
        Level::Oke => ("Sudah oke", GREEN),
    }
}

fn yes_no(v: Option<bool>, yes: &str, no: &str) -> String {
    match v {
        Some(true) => yes.into(),
        Some(false) => no.into(),
        None => "Tidak terbaca".into(),
    }
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(150.0, 18.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(150.0);
            ui.label(muted(label));
        });
        ui.label(RichText::new(value).color(TEXT));
    });
}

impl PacuApp {
    /// Pemindaian di thread lain karena nvidia-smi butuh waktu hingga satu detik.
    fn bios_scan(&self, ctx: &egui::Context) {
        if self.bios_scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        let (slot, flag, ctx) = (self.bios.clone(), self.bios_scanning.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = bios::collect();
            *slot.lock() = Some(r);
            flag.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    pub(super) fn page_bios(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "BIOS & Hardware", "Memeriksa pengaturan BIOS yang paling berpengaruh ke FPS. Pacu hanya membaca; perubahan kamu lakukan sendiri di layar BIOS.");
        let report = self.bios.lock().clone();
        let Some(r) = report else {
            self.bios_scan(ui.ctx());
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(muted("Membaca SMBIOS dan status GPU..."));
            });
            return;
        };
        let tips = bios::advice(&r, bios::this_year());
        let s = &r.smbios;

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon_text(theme::icon::BOARD, 18.0).color(accent()));
                ui.vertical(|ui| {
                    ui.label(section(&format!("{} {}", s.board_maker, s.board_product)));
                    ui.label(muted(format!("BIOS {} versi {} tanggal {}", s.bios_vendor, s.bios_version, s.bios_date)));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(!self.bios_scanning.load(Ordering::SeqCst), egui::Button::new("Pindai ulang")).clicked() {
                        *self.bios.lock() = None;
                    }
                    if ui.button("Salin laporan").on_hover_text("Salin ringkasan ini untuk dikirim ke Claude atau forum").clicked() {
                        ui.ctx().copy_text(bios::report_text(&r, &tips));
                        self.status = Some(("Laporan BIOS disalin".into(), 0.0));
                    }
                });
            });
            ui.add_space(8.0);
            divider(ui);
            ui.add_space(6.0);
            info_row(ui, "CPU", &s.cpu);
            info_row(ui, "GPU", &r.gpu);
            for d in &s.dimms {
                info_row(
                    ui,
                    &format!("RAM {}", d.slot),
                    &format!("{} GB {} jalan {} MT/s (maks {}), {} {}", d.size_mb / 1024, d.type_name(), d.configured, d.rated, d.maker, d.part),
                );
            }
            if s.dimms.is_empty() {
                info_row(ui, "RAM", "Tidak terbaca");
            }
            let rebar = match r.rebar {
                Rebar::On { bar_mb, .. } => format!("Aktif (jendela BAR {bar_mb} MB)"),
                Rebar::Off { bar_mb, .. } => format!("Mati (jendela BAR hanya {bar_mb} MB)"),
                Rebar::Unknown => "Tidak terbaca (hanya untuk GPU NVIDIA)".into(),
            };
            info_row(ui, "Resizable BAR", &rebar);
            info_row(ui, "Mode boot", &yes_no(r.uefi, "UEFI", "Legacy (CSM)"));
            info_row(ui, "Secure Boot", &yes_no(r.secure_boot, "Menyala", "Mati"));
            info_row(ui, "Memory Integrity", &yes_no(r.hvci, "Menyala", "Mati"));
        });

        ui.add_space(14.0);
        ui.label(section("Rekomendasi"));
        ui.label(muted("Masuk BIOS dengan menekan Del atau F2 saat PC menyala. Nama menu bisa sedikit berbeda antar seri motherboard."));
        ui.add_space(4.0);
        for a in &tips {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    let (t, c) = level_tag(a.level);
                    tag(ui, t, c);
                    ui.label(strong(&a.title));
                });
                if !a.body.is_empty() {
                    ui.label(RichText::new(&a.body).color(theme::TEXT_2).size(13.0));
                }
                if let Some(m) = &a.menu {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(muted("Di BIOS:"));
                        ui.label(RichText::new(m).color(CYAN).size(12.5));
                    });
                }
            });
            ui.add_space(6.0);
        }
        ui.add_space(4.0);
        banner(
            ui,
            theme::TEXT_2,
            "Kalau PC tidak mau menyala setelah mengubah BIOS",
            "Matikan PC, cabut listrik, lalu tekan tombol Clear CMOS di panel belakang atau lepas baterai CMOS selama satu menit. Semua pengaturan BIOS kembali ke bawaan pabrik.",
        );
    }
}
