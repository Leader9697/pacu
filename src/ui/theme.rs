//! Tema mengikuti sistem desain Fluent Windows 11 (mode gelap): warna lapisan, tipografi Segoe UI,
//! sudut membulat 4/8 px, dan warna aksen dari pengaturan Personalisasi Windows.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Vec2};
use std::sync::{Arc, OnceLock};

/// Latar jendela dan panel navigasi.
pub const BG: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
/// Area konten (lapisan di atas latar).
pub const LAYER: Color32 = Color32::from_rgb(0x27, 0x27, 0x27);
/// Kartu.
pub const CARD: Color32 = Color32::from_rgb(0x2D, 0x2D, 0x2D);
/// Garis tepi kartu dan pemisah.
pub const STROKE: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
pub const DIVIDER: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
/// Isi kontrol (tombol, kotak isian).
pub const CONTROL: Color32 = Color32::from_rgb(0x37, 0x37, 0x37);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x3D, 0x3D, 0x3D);
pub const CONTROL_STROKE: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
/// Kotak isian dan lintasan grafik.
pub const SUNKEN: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);

pub const TEXT: Color32 = Color32::WHITE;
pub const TEXT_2: Color32 = Color32::from_rgb(0xC5, 0xC5, 0xC5);
pub const TEXT_3: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
/// Teks di atas warna aksen.
pub const ON_ACCENT: Color32 = Color32::BLACK;

pub const SUCCESS: Color32 = Color32::from_rgb(0x6C, 0xCB, 0x5F);
pub const CAUTION: Color32 = Color32::from_rgb(0xFC, 0xE1, 0x00);
pub const CRITICAL: Color32 = Color32::from_rgb(0xFF, 0x99, 0xA4);

/// Warna data grafik (selaras dengan grafik Task Manager).
pub const DATA_BLUE: Color32 = Color32::from_rgb(0x4C, 0xC2, 0xFF);
pub const DATA_ORANGE: Color32 = Color32::from_rgb(0xF7, 0xA2, 0x4F);
pub const DATA_PURPLE: Color32 = Color32::from_rgb(0xB6, 0x9C, 0xFF);

pub const RADIUS_CONTROL: u8 = 4;
pub const RADIUS_CARD: u8 = 8;

pub const FONT_SEMIBOLD: &str = "semibold";
pub const FONT_ICONS: &str = "icons";

/// Ikon Fluent System Icons (MIT), subset di res/fonts/FluentIcons.ttf.
#[allow(dead_code)]
pub mod icon {
    pub const HOME: &str = "\u{F480}";
    pub const PULSE: &str = "\u{E9DC}";
    pub const GAMES: &str = "\u{E68A}";
    pub const SPEED: &str = "\u{ED9B}";
    pub const FLASH: &str = "\u{E618}";
    pub const WRENCH: &str = "\u{EE87}";
    pub const RAM: &str = "\u{E9F3}";
    pub const APPS: &str = "\u{F8CB}";
    pub const RULES: &str = "\u{EC92}";
    pub const HISTORY: &str = "\u{F47E}";
    pub const SETTINGS: &str = "\u{F6A9}";
    pub const WARNING: &str = "\u{F869}";
    pub const INFO: &str = "\u{F4A3}";
    pub const CHECK: &str = "\u{F298}";
    pub const PLAY: &str = "\u{F605}";
    pub const STOP: &str = "\u{F72A}";
    pub const REFRESH: &str = "\u{F13D}";
    pub const DELETE: &str = "\u{F34C}";
    pub const ADD: &str = "\u{F109}";
    pub const SEARCH: &str = "\u{F68F}";
    pub const TEMP: &str = "\u{F78F}";
    pub const BOARD: &str = "\u{E4AC}";
    pub const SHIELD: &str = "\u{F6BE}";
    pub const DISMISS: &str = "\u{F369}";
    pub const CHEVRON: &str = "\u{F2B0}";
}

/// Warna aksen Windows (HKCU\Software\Microsoft\Windows\DWM\AccentColor, format ABGR), dicerahkan
/// seperti varian "Light 2" yang dipakai Windows di mode gelap. Bila tidak terbaca: biru bawaan.
pub fn accent() -> Color32 {
    static A: OnceLock<Color32> = OnceLock::new();
    *A.get_or_init(|| {
        use crate::tweaks::{reg, Root, Saved};
        match reg::read(Root::Hkcu, "Software\\Microsoft\\Windows\\DWM", "AccentColor") {
            Ok(Some(Saved::Dword(abgr))) => dark_mode_accent(abgr),
            _ => Color32::from_rgb(0x60, 0xCD, 0xFF),
        }
    })
}

/// ABGR ke warna yang cukup terang untuk latar gelap (luminans minimal ~0.45).
pub fn dark_mode_accent(abgr: u32) -> Color32 {
    let (r, g, b) = ((abgr & 0xFF) as f32, ((abgr >> 8) & 0xFF) as f32, ((abgr >> 16) & 0xFF) as f32);
    let lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
    let t = if lum < 0.45 { ((0.45 - lum) / (1.0 - lum)).clamp(0.0, 1.0) + 0.25 } else { 0.0 };
    let mix = |c: f32| (c + (255.0 - c) * t.min(0.8)).round() as u8;
    Color32::from_rgb(mix(r), mix(g), mix(b))
}

fn windows_font(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
    std::fs::read(dir.join("Fonts").join(name)).ok()
}

/// Segoe UI (Variable / klasik) dari folder Fonts Windows, dengan font bawaan egui sebagai cadangan,
/// plus ikon Fluent di urutan terakhir supaya ikon bisa ditulis langsung di dalam teks.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut regular: Vec<String> = Vec::new();
    let mut semibold: Vec<String> = Vec::new();
    if let Some(b) = windows_font("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(b)));
        regular.push("segoe".into());
    }
    if let Some(b) = windows_font("seguisb.ttf") {
        fonts.font_data.insert("segoe-sb".into(), Arc::new(FontData::from_owned(b)));
        semibold.push("segoe-sb".into());
    }
    fonts.font_data.insert("fluent-icons".into(), Arc::new(FontData::from_static(include_bytes!("../../res/fonts/FluentIcons.ttf"))));

    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // Ikon sebelum font bawaan egui, karena emoji-icon-font egui juga memakai area Private Use.
    let mut prop = regular.clone();
    prop.push("fluent-icons".into());
    prop.extend(base.iter().cloned());
    let mut sb = semibold;
    sb.extend(regular);
    sb.push("fluent-icons".into());
    sb.extend(base);
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name(FONT_SEMIBOLD.into()), sb);
    fonts.families.insert(FontFamily::Name(FONT_ICONS.into()), vec!["fluent-icons".into()]);
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.push("fluent-icons".into());
    }
    ctx.set_fonts(fonts);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_SEMIBOLD.into()))
}

/// Font khusus ikon Fluent (tanpa font teks, supaya glyph Private Use font lain tidak ikut terpakai).
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_ICONS.into()))
}

/// Teks ikon Fluent.
pub fn icon_text(ic: &str, size: f32) -> RichText {
    RichText::new(ic).font(icon_font(size))
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let a = accent();
    let r = CornerRadius::same(RADIUS_CONTROL);
    let mut v = egui::Visuals::dark();
    v.panel_fill = LAYER;
    v.window_fill = Color32::from_rgb(0x2C, 0x2C, 0x2C);
    v.window_stroke = Stroke::new(1.0, DIVIDER);
    v.window_corner_radius = CornerRadius::same(RADIUS_CARD);
    v.menu_corner_radius = CornerRadius::same(RADIUS_CARD);
    v.extreme_bg_color = SUNKEN;
    v.faint_bg_color = Color32::from_rgb(0x2A, 0x2A, 0x2A);
    v.code_bg_color = SUNKEN;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = a.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, a);
    v.hyperlink_color = a;
    v.warn_fg_color = CAUTION;
    v.error_fg_color = CRITICAL;
    v.striped = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CARD;
    w.noninteractive.weak_bg_fill = CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_2);
    w.inactive.bg_fill = CONTROL;
    w.inactive.weak_bg_fill = CONTROL;
    w.inactive.bg_stroke = Stroke::new(1.0, CONTROL_STROKE);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.bg_fill = CONTROL_HOVER;
    w.hovered.weak_bg_fill = CONTROL_HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, CONTROL_STROKE);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.expansion = 0.0;
    w.active.bg_fill = Color32::from_rgb(0x32, 0x32, 0x32);
    w.active.weak_bg_fill = Color32::from_rgb(0x32, 0x32, 0x32);
    w.active.bg_stroke = Stroke::new(1.0, CONTROL_STROKE);
    w.active.fg_stroke = Stroke::new(1.0, TEXT_2);
    w.active.expansion = 0.0;
    w.open = w.active;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = r;
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(11.0, 5.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.combo_height = 300.0;
        s.spacing.slider_rail_height = 4.0;
        s.spacing.icon_width = 18.0;
        s.spacing.icon_width_inner = 10.0;
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.scroll.floating = true;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Heading, semibold(28.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_is_readable_on_dark() {
        // Biru bawaan Windows (#0078D4) disimpan sebagai ABGR 0xFFD47800.
        let c = dark_mode_accent(0xFFD4_7800);
        let lum = (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0;
        assert!(lum > 0.45, "{c:?}");
        assert!(c.b() > c.r());
        // Warna yang sudah terang tidak diubah.
        assert_eq!(dark_mode_accent(0xFF60_CDFF), Color32::from_rgb(0xFF, 0xCD, 0x60));
    }
}

// ── Komponen bersama ────────────────────────────────────────────────────────────

use eframe::egui::{Align, Layout, Margin, RichText, Sense};

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Kartu Fluent: lapisan #2D2D2D, tepi tipis, sudut 8 px, lebar penuh.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, STROKE))
        .corner_radius(CornerRadius::same(RADIUS_CARD))
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Teks keterangan (caption 12 px, warna sekunder).
pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(TEXT_3).size(12.5)
}

/// Judul halaman (Title, 28 px semibold).
pub fn title(text: &str) -> RichText {
    RichText::new(text).font(semibold(28.0)).color(TEXT)
}

/// Subjudul (Subtitle, 20 px semibold).
pub fn subtitle(text: &str) -> RichText {
    RichText::new(text).font(semibold(20.0)).color(TEXT)
}

/// Judul bagian (Body strong, 14 px semibold), seperti "Related settings" di aplikasi Settings.
pub fn section(text: &str) -> RichText {
    RichText::new(text).font(semibold(14.0)).color(TEXT)
}

/// Teks tebal ukuran biasa.
pub fn strong(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(semibold(14.0))
}

/// Header halaman: judul 28 px dan keterangan satu baris di bawahnya.
pub fn page_header(ui: &mut egui::Ui, head: &str, sub: &str) {
    ui.label(title(head));
    if !sub.is_empty() {
        ui.add_space(-2.0);
        ui.label(RichText::new(sub).color(TEXT_2).size(13.0));
    }
    ui.add_space(14.0);
}

/// Tombol utama (accent button): latar aksen, teks hitam.
pub fn accent_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(ON_ACCENT)).fill(accent()).stroke(Stroke::new(1.0, mix(accent(), Color32::WHITE, 0.15)))
}

/// Tombol berbahaya (menghentikan / menghapus).
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(Color32::BLACK)).fill(CRITICAL).stroke(Stroke::NONE)
}

/// ToggleSwitch WinUI: 40x20, mati = garis tepi, nyala = isi aksen dengan kenop gelap.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(40.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let hover = resp.hovered();
    let p = ui.painter();
    let r = CornerRadius::same(10);
    if *on {
        let a = accent();
        p.rect_filled(rect, r, if hover { mix(a, Color32::WHITE, 0.1) } else { a });
    } else {
        p.rect_filled(rect, r, if hover { Color32::from_rgb(0x34, 0x34, 0x34) } else { Color32::from_rgb(0x27, 0x27, 0x27) });
        p.rect_stroke(rect.shrink(0.5), r, Stroke::new(1.0, TEXT_2), egui::StrokeKind::Inside);
    }
    let knob = if hover { 7.0 } else { 6.0 };
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    p.circle_filled(egui::pos2(x, rect.center().y), knob, if *on { ON_ACCENT } else { TEXT_2 });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, true, *on, ""));
    resp
}

/// Baris pengaturan (SettingsCard): judul dan keterangan di kiri, "Aktif/Nonaktif" dan sakelar
/// di kanan.
pub fn setting_row(ui: &mut egui::Ui, on: &mut bool, label: &str, help: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed = toggle(ui, on).changed();
            ui.add_space(4.0);
            ui.label(RichText::new(if *on { "Aktif" } else { "Nonaktif" }).color(TEXT_2));
            ui.add_space(16.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.label(RichText::new(label).color(TEXT));
                if !help.is_empty() {
                    ui.label(muted(help));
                }
            });
        });
    });
    ui.add_space(2.0);
    changed
}

/// Garis pemisah tipis di dalam kartu.
pub fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment([egui::pos2(rect.left() - 16.0, y), egui::pos2(rect.right() + 16.0, y)], Stroke::new(1.0, STROKE));
}

/// InfoBar WinUI: ikon, judul tebal dan pesan; warna mengikuti tingkat (sukses, peringatan, kritis,
/// atau informasi).
pub fn banner(ui: &mut egui::Ui, color: Color32, head: &str, body: &str) {
    let (bg, ic) = if color == SUCCESS {
        (Color32::from_rgb(0x39, 0x3D, 0x1B), icon::CHECK)
    } else if color == CRITICAL {
        (Color32::from_rgb(0x44, 0x27, 0x26), icon::WARNING)
    } else if color == CAUTION {
        (Color32::from_rgb(0x43, 0x35, 0x19), icon::WARNING)
    } else {
        (CARD, icon::INFO)
    };
    let ic_color = if color == CAUTION || color == SUCCESS || color == CRITICAL { color } else { accent() };
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, STROKE))
        .corner_radius(CornerRadius::same(RADIUS_CONTROL))
        .inner_margin(Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(icon_text(ic, 16.0).color(ic_color));
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(strong(head));
                    if !body.is_empty() {
                        ui.label(RichText::new(body).color(TEXT_2).size(13.0));
                    }
                });
            });
        });
}

/// Tab bergaya Pivot / SelectorBar: teks, dengan garis aksen di bawah tab terpilih.
pub fn pivot<T: Copy + PartialEq>(ui: &mut egui::Ui, current: &mut T, items: &[(T, &str)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for &(v, label) in items {
            let sel = *current == v;
            let font = if sel { semibold(14.0) } else { FontId::proportional(14.0) };
            let galley = ui.painter().layout_no_wrap(label.to_string(), font, if sel { TEXT } else { TEXT_2 });
            let size = Vec2::new(galley.size().x + 24.0, 38.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let p = ui.painter();
            if resp.hovered() && !sel {
                p.rect_filled(rect.shrink2(Vec2::new(0.0, 3.0)), CornerRadius::same(RADIUS_CONTROL), Color32::from_rgb(0x32, 0x32, 0x32));
            }
            p.galley(rect.center() - galley.size() / 2.0, galley, TEXT);
            if sel {
                let w = 16.0f32.max(size.x - 28.0);
                let bar = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.bottom() - 2.0), Vec2::new(w, 3.0));
                p.rect_filled(bar, CornerRadius::same(2), accent());
            }
            if resp.clicked() {
                *current = v;
            }
        }
    });
}

/// SegmentedControl: pilihan berdampingan dalam satu bingkai.
pub fn segmented<T: Copy + PartialEq>(ui: &mut egui::Ui, current: T, items: &[(T, &str)]) -> Option<T> {
    let mut picked = None;
    egui::Frame::new()
        .fill(SUNKEN)
        .stroke(Stroke::new(1.0, CONTROL_STROKE))
        .corner_radius(CornerRadius::same(RADIUS_CONTROL + 1))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Di dalam layout kanan-ke-kiri, urutan item dibalik supaya tetap terbaca kiri ke kanan.
                let rtl = ui.layout().prefer_right_to_left();
                let order: Vec<_> = if rtl { items.iter().rev().collect() } else { items.iter().collect() };
                for &&(v, label) in &order {
                    let sel = current == v;
                    let b = if sel {
                        egui::Button::new(RichText::new(label).color(ON_ACCENT)).fill(accent()).stroke(Stroke::NONE)
                    } else {
                        egui::Button::new(RichText::new(label).color(TEXT_2)).fill(Color32::TRANSPARENT).stroke(Stroke::NONE)
                    };
                    if ui.add(b.min_size(Vec2::new(84.0, 28.0))).clicked() {
                        picked = Some(v);
                    }
                }
            });
        });
    picked
}

/// Label status kecil (pill), ukurannya pas dengan teks di layout apa pun.
pub fn tag(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.0), color);
    let size = galley.size() + Vec2::new(12.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), mix(CARD, color, 0.18));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}
