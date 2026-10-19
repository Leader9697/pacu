//! Preset game populer: nama exe dan setelan yang cocok.

use crate::config::{Config, GpuPriority, IoPriority, Priority};

pub struct Preset {
    pub name: &'static str,
    pub exes: &'static [&'static str],
    /// Game berat CPU yang diuntungkan bila hanya berjalan di P-core (CPU hybrid).
    pub pcores: bool,
    pub note: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Deadlock",
        exes: &["project8.exe"],
        pcores: true,
        note: "Berat di CPU (Source 2). Tidak perlu opsi peluncuran -high karena Pacu sudah mengatur prioritasnya.",
    },
    Preset { name: "Counter-Strike 2", exes: &["cs2.exe"], pcores: true, note: "" },
    Preset { name: "Dota 2", exes: &["dota2.exe"], pcores: true, note: "" },
    Preset { name: "Valorant", exes: &["VALORANT-Win64-Shipping.exe"], pcores: false, note: "Anti-cheat Vanguard bisa menolak perubahan prioritas." },
    Preset { name: "Apex Legends", exes: &["r5apex.exe", "r5apex_dx12.exe"], pcores: false, note: "" },
    Preset { name: "Fortnite", exes: &["FortniteClient-Win64-Shipping.exe"], pcores: false, note: "" },
];

/// Menerapkan preset ke konfigurasi. `hybrid` = CPU punya P-core dan E-core.
pub fn apply(cfg: &mut Config, preset: &Preset, hybrid: bool) {
    for exe in preset.exes {
        if !cfg.game.games.iter().any(|g| g.eq_ignore_ascii_case(exe)) {
            cfg.game.games.push(exe.to_string());
        }
    }
    let g = &mut cfg.game;
    g.auto = true;
    g.game_priority = Some(Priority::High);
    g.game_io_priority = Some(IoPriority::High);
    g.game_gpu_priority = Some(GpuPriority::High);
    g.disable_efficiency_on_game = true;
    g.aggressive_balancer = true;
    g.timer_resolution = true;
    g.clear_standby = true;
    g.standby_periodic = true;
    g.freeze_enabled = true;
    if hybrid && preset.pcores {
        g.game_on_pcores = true;
    }
}

pub fn is_applied(cfg: &Config, preset: &Preset) -> bool {
    preset.exes.iter().all(|e| cfg.game.games.iter().any(|g| g.eq_ignore_ascii_case(e)))
}
