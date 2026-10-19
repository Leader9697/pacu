//! Halaman Monitor & Overlay: semua sensor di satu tempat dan pengaturan overlay di dalam game.

use super::*;
use crate::config::{Corner, OverlayMode};
use crate::monitor::{OverlayState, Point};


/// Tabel sensor bergaya HWiNFO: baris per sensor dikelompokkan, kolom Sekarang / Min / Maks /
/// Rata-rata rata kanan, baris kelompok dengan ikon.
fn sensor_table(ui: &mut egui::Ui, list: &[monitor::sensors::Sensor]) {
    use monitor::sensors::Group;
    let w = ui.available_width();
    let cols = [w * 0.34, w * 0.165, w * 0.165, w * 0.165, w * 0.165];
    let row_h = 26.0;
    let text = |p: &egui::Painter, x: f32, y: f32, right: bool, s: &str, font: FontId, c: Color32| {
        let align = if right { egui::Align2::RIGHT_CENTER } else { egui::Align2::LEFT_CENTER };
        p.text(egui::pos2(x, y), align, s, font, c);
    };
    // Header kolom.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::hover());
    let p = ui.painter();
    let y = rect.center().y;
    let mut x = rect.left();
    for (i, head) in ["Sensor", "Sekarang", "Minimum", "Maksimum", "Rata-rata"].iter().enumerate() {
        if i == 0 {
            text(p, x + 10.0, y, false, head, theme::semibold(12.5), theme::TEXT_2);
        } else {
            text(p, x + cols[i] - 10.0, y, true, head, theme::semibold(12.5), theme::TEXT_2);
        }
        x += cols[i];
    }
    p.line_segment([egui::pos2(rect.left(), rect.bottom()), egui::pos2(rect.right(), rect.bottom())], Stroke::new(1.0, theme::DIVIDER));

    let mut last: Option<Group> = None;
    let mut stripe = false;
    for s in list {
        if last != Some(s.group) {
            last = Some(s.group);
            stripe = false;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, row_h + 4.0), Sense::hover());
            let p = ui.painter();
            let ic = match s.group {
                Group::Fps => theme::icon::SPEED,
                Group::Gpu => theme::icon::BOARD,
                Group::Cpu => theme::icon::BOARD,
                Group::Memory => theme::icon::RAM,
            };
            let y = rect.center().y + 2.0;
            text(p, rect.left() + 10.0, y, false, ic, theme::icon_font(14.0), accent());
            text(p, rect.left() + 32.0, y, false, s.group.label(), theme::semibold(13.0), TEXT);
        }
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, row_h), Sense::hover());
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(theme::RADIUS_CONTROL), Color32::from_rgb(0x36, 0x36, 0x36));
        } else if stripe {
            p.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(0x30, 0x30, 0x30));
        }
        stripe = !stripe;
        let y = rect.center().y;
        let dim = s.now.is_none();
        text(p, rect.left() + 32.0, y, false, s.name, FontId::proportional(13.0), if dim { theme::TEXT_3 } else { TEXT });
        let vals = [s.now, s.min, s.max, s.avg()];
        let mut x = rect.left() + cols[0];
        for (i, v) in vals.iter().enumerate() {
            let c = if v.is_none() { theme::TEXT_3 } else if i == 0 { TEXT } else { theme::TEXT_2 };
            text(p, x + cols[i + 1] - 10.0, y, true, &s.fmt(*v), FontId::proportional(13.0), c);
            x += cols[i + 1];
        }
    }
}

/// Grafik beberapa garis dengan skala 0..max. Nilai None memutus garis.
fn multi_chart(ui: &mut egui::Ui, series: &[(Vec<Option<f32>>, Color32, &str)], max: f32, unit: &str, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_CONTROL), theme::SUNKEN);
    for frac in [0.25, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * frac;
        p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], Stroke::new(1.0, theme::DIVIDER.gamma_multiply(0.5)));
    }
    let n = crate::monitor::HISTORY;
    let dx = rect.width() / (n - 1) as f32;
    for (data, color, _) in series {
        let start = n.saturating_sub(data.len());
        let mut run: Vec<egui::Pos2> = Vec::new();
        for (i, v) in data.iter().enumerate() {
            match v {
                Some(v) => run.push(egui::pos2(
                    rect.left() + (start + i) as f32 * dx,
                    rect.bottom() - 3.0 - (rect.height() - 6.0) * (v / max).clamp(0.0, 1.0),
                )),
                None => {
                    if run.len() > 1 {
                        p.add(egui::Shape::line(std::mem::take(&mut run), Stroke::new(1.8, *color)));
                    }
                    run.clear();
                }
            }
        }
        if run.len() > 1 {
            p.add(egui::Shape::line(run, Stroke::new(1.8, *color)));
        }
    }
    p.text(rect.left_top() + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, format!("{max:.0}{unit}"), FontId::proportional(11.0), MUTED);
    let mut x = rect.right() - 8.0;
    for (_, color, name) in series.iter().rev() {
        let g = p.text(egui::pos2(x, rect.top() + 4.0), egui::Align2::RIGHT_TOP, *name, FontId::proportional(11.0), *color);
        x = g.left() - 12.0;
    }
}

fn series(h: &std::collections::VecDeque<Point>, f: impl Fn(&Point) -> Option<f32>) -> Vec<Option<f32>> {
    h.iter().map(f).collect()
}

/// Menjalankan perintah lewat cmd di jendela baru (untuk winget / membuka situs).
fn shell(args: &[&str]) {
    let _ = std::process::Command::new("cmd").arg("/c").arg("start").arg("").args(args).spawn();
}

impl PacuApp {
    pub(super) fn page_monitor(&mut self, ui: &mut egui::Ui) {
        let v = self.mon.read().clone();
        let r = &v.now;
        let sub = format!("{}  |  {}", if v.cpu_name.is_empty() { "CPU" } else { &v.cpu_name }, if v.gpu_name.is_empty() { "GPU" } else { &v.gpu_name });
        page_header(ui, "Monitor & Overlay", &sub);
        let _ = r;

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(section("Sensor"));
                ui.label(muted("Min, maks, dan rata-rata dihitung sejak Pacu dibuka."));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Reset statistik").clicked() {
                        self.mon.write().sensors.reset();
                    }
                });
            });
            ui.add_space(2.0);
            sensor_table(ui, &v.sensors.list);
        });
        ui.add_space(8.0);

        if let Some(note) = &v.cpu_note {
            if v.pawnio_missing {
                banner(
                    ui,
                    AMBER,
                    "Suhu dan daya CPU butuh PawnIO",
                    "Sensor CPU dibaca lewat PawnIO, driver bertanda tangan yang juga dipakai LibreHardwareMonitor dan FanControl. \
                     WinRing0 tidak dipakai karena diblokir Windows Defender. Setelah terpasang, suhu CPU muncul dalam satu menit.",
                );
                ui.add_space(4.0);
                {
                    ui.horizontal(|ui| {
                        if ui.button("Pasang PawnIO (winget)").clicked() {
                            shell(&["winget", "install", "-e", "--id", "namazso.PawnIO"]);
                        }
                        if ui.button("Buka pawnio.eu").clicked() {
                            shell(&["https://pawnio.eu"]);
                        }
                    });
                }
            } else {
                banner(ui, theme::TEXT_2, "Suhu dan daya CPU tidak tersedia", note);
            }
            ui.add_space(8.0);
        }

        card(ui, |ui| {
            ui.label(section("Riwayat 2 menit"));
            let h = &v.history;
            let fps_max = h.iter().filter_map(|p| p.fps).fold(60.0f32, f32::max) * 1.15;
            multi_chart(ui, &[(series(h, |p| p.fps), GREEN, "FPS")], fps_max, " fps", 90.0);
            ui.add_space(6.0);
            multi_chart(ui, &[(series(h, |p| p.gpu_temp), CYAN, "GPU °C"), (series(h, |p| p.cpu_temp), theme::DATA_ORANGE, "CPU °C")], 100.0, "°C", 90.0);
            ui.add_space(6.0);
            multi_chart(ui, &[(series(h, |p| p.gpu_pct), CYAN, "GPU %"), (series(h, |p| Some(p.cpu_pct)), theme::DATA_ORANGE, "CPU %")], 100.0, "%", 90.0);
        });

        ui.add_space(20.0);
        ui.label(section("Overlay di dalam game"));
        ui.add_space(4.0);
        self.overlay_card(ui, &v);
    }

    fn overlay_card(&mut self, ui: &mut egui::Ui, v: &monitor::View) {
        let mut changed = false;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let o = &mut self.cfg.overlay;
            changed |= setting_row(
                ui,
                &mut o.enabled,
                "Overlay di dalam game",
                "Menampilkan FPS, GPU, CPU dan RAM di pojok layar saat bermain, seperti OSD RivaTuner. Ctrl+Shift+F10 untuk sembunyikan / tampilkan.",
            );
            let (color, text) = match v.overlay {
                OverlayState::Off => (MUTED, "Mati".to_string()),
                OverlayState::Waiting => (MUTED, "Menunggu game".into()),
                OverlayState::Rtss => (GREEN, "Tampil lewat RTSS (di dalam game, semua mode layar)".into()),
                OverlayState::Window => (GREEN, "Tampil sebagai jendela Pacu di atas game".into()),
                OverlayState::HiddenByHotkey => (AMBER, "Disembunyikan dengan Ctrl+Shift+F10".into()),
                OverlayState::RtssMissing => (AMBER, "RTSS tidak berjalan atau semua slot OSD terpakai".into()),
            };
            ui.horizontal(|ui| {
                ui.label(muted("Status"));
                tag(ui, &text, color);
            });
            divider(ui);

            ui.label(RichText::new("Cara menampilkan"));
            changed |= ui.radio_value(&mut o.mode, OverlayMode::Auto, "Otomatis: lewat RTSS bila sedang berjalan, selain itu jendela Pacu").changed();
            changed |= ui.radio_value(&mut o.mode, OverlayMode::Rtss, "Selalu lewat RTSS").changed();
            changed |= ui.radio_value(&mut o.mode, OverlayMode::Window, "Selalu jendela Pacu").changed();
            ui.label(muted(if v.rtss_running {
                "RTSS sedang berjalan. Lewat RTSS, overlay digambar di dalam game sehingga tampil juga di Fullscreen eksklusif."
            } else {
                "RTSS tidak berjalan. Jendela Pacu hanya terlihat di mode Windowed / Borderless. Untuk Fullscreen eksklusif, pasang RTSS."
            }));
            changed |= setting_row(
                ui,
                &mut o.launch_rtss,
                "Jalankan RTSS otomatis",
                "Pacu membuka RTSS (ke tray) bila belum berjalan. Buka game setelah RTSS jalan; game yang sudah terbuka perlu dibuka ulang.",
            );
            if let Some(m) = &v.rtss_launch {
                ui.label(muted(m.clone()));
            }
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Posisi");
                egui::ComboBox::from_id_salt("overlay-corner").selected_text(o.corner.label()).show_ui(ui, |ui| {
                    for c in Corner::ALL {
                        changed |= ui.selectable_value(&mut o.corner, c, c.label()).changed();
                    }
                });
                ui.add_space(16.0);
                ui.label("Ukuran");
                changed |= ui.add(egui::Slider::new(&mut o.scale, 0.7..=2.0).step_by(0.1).suffix("x")).drag_stopped();
            });
            ui.label(muted("Posisi dan ukuran untuk mode RTSS diatur dari RTSS sendiri."));
            divider(ui);
            ui.label(RichText::new("Isi overlay"));
            ui.horizontal_wrapped(|ui| {
                changed |= ui.checkbox(&mut o.show_fps, "FPS dan 1% low").changed();
                changed |= ui.checkbox(&mut o.show_frametime, "Frame time").changed();
                changed |= ui.checkbox(&mut o.show_gpu, "GPU").changed();
                changed |= ui.checkbox(&mut o.show_vram, "VRAM").changed();
                changed |= ui.checkbox(&mut o.show_cpu, "CPU").changed();
                changed |= ui.checkbox(&mut o.show_ram, "RAM").changed();
            });
            ui.add_space(6.0);
            changed |= setting_row(ui, &mut o.always, "Tampilkan walau tidak ada game", "Berguna untuk mencoba posisi dan ukuran (mode jendela Pacu).");
        });
        if changed {
            self.apply_config();
        }
    }
}
