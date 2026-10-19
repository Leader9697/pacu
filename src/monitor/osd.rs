//! Menyusun isi overlay dari bacaan sensor, untuk jendela Pacu maupun OSD RTSS.

use super::Reading;
use crate::config::OverlayConfig;

pub const GREEN: u32 = 0x4ADE80;
pub const CYAN: u32 = 0x22D3EE;
pub const AMBER: u32 = 0xF59E0B;
pub const VIOLET: u32 = 0xA78BFA;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub label: &'static str,
    pub color: u32,
    pub value: String,
}

fn n<T: std::fmt::Display>(v: Option<T>, unit: &str) -> Option<String> {
    v.map(|x| format!("{x}{unit}"))
}

fn join(parts: Vec<Option<String>>) -> String {
    let p: Vec<String> = parts.into_iter().flatten().collect();
    if p.is_empty() {
        "-".into()
    } else {
        p.join("  ")
    }
}

fn gb(mb: f32) -> String {
    format!("{:.1}", mb / 1024.0)
}

/// Baris overlay. Bila `rtss_fps` true, FPS dan frame time memakai makro RTSS (<FR>, <FT>)
/// sehingga angkanya dihitung RTSS untuk game tempat OSD digambar.
pub fn lines(r: &Reading, cfg: &OverlayConfig, rtss_fps: bool) -> Vec<Line> {
    let mut out = Vec::new();
    if cfg.show_fps && (rtss_fps || r.fps.is_some()) {
        let fps = if rtss_fps { Some("<FR>".to_string()) } else { r.fps.map(|f| format!("{f:.0}")) };
        let ft = if !cfg.show_frametime {
            None
        } else if rtss_fps {
            Some("<FT> ms".to_string())
        } else {
            r.frametime_ms.map(|f| format!("{f:.1} ms"))
        };
        let low = r.low1.map(|l| format!("1% {l:.0}"));
        out.push(Line { label: "FPS", color: GREEN, value: join(vec![fps, low, ft]) });
    }
    let g = &r.gpu;
    if cfg.show_gpu {
        out.push(Line {
            label: "GPU",
            color: CYAN,
            value: join(vec![
                n(g.util_pct, "%"),
                n(g.temp_c, "°C"),
                n(g.clock_mhz, " MHz"),
                g.power_w.map(|p| format!("{p:.0} W")),
                g.voltage_mv.map(|v| format!("{v:.0} mV")),
            ]),
        });
    }
    if cfg.show_vram {
        if let (Some(u), Some(t)) = (g.vram_used_mb, g.vram_total_mb) {
            out.push(Line { label: "VRAM", color: CYAN, value: format!("{} / {} GB", gb(u as f32), gb(t as f32)) });
        }
    }
    if cfg.show_cpu {
        out.push(Line {
            label: "CPU",
            color: AMBER,
            value: join(vec![
                Some(format!("{:.0}%", r.cpu_pct)),
                r.cpu.temp_c.map(|t| format!("{t:.0}°C")),
                r.cpu.mhz.map(|m| format!("{:.2} GHz", m / 1000.0)),
                r.cpu.power_w.map(|p| format!("{p:.0} W")),
            ]),
        });
    }
    if cfg.show_ram && r.ram_total_mb > 0 {
        out.push(Line {
            label: "RAM",
            color: VIOLET,
            value: format!("{} / {} GB", gb(r.ram_used_mb as f32), gb(r.ram_total_mb as f32)),
        });
    }
    out
}

/// Teks dengan tag format RTSS: warna didefinisikan sekali (C0..C3) lalu dipakai per label.
pub fn rtss_text(lines: &[Line]) -> String {
    let palette = [GREEN, CYAN, AMBER, VIOLET];
    let mut s = String::new();
    for (i, c) in palette.iter().enumerate() {
        s.push_str(&format!("<C{i}={c:06X}>"));
    }
    s.push_str("<C4=FFFFFF>");
    for l in lines {
        let idx = palette.iter().position(|&c| c == l.color).unwrap_or(0);
        s.push_str(&format!("<C{idx}>{:<5}<C><C4>{}<C>\n", l.label, l.value));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volt::gpu::Telemetry;

    fn reading() -> Reading {
        Reading {
            cpu_pct: 23.4,
            ram_used_mb: 9830,
            ram_total_mb: 16384,
            fps: Some(143.6),
            low1: Some(98.2),
            frametime_ms: Some(6.96),
            gpu: Telemetry { util_pct: Some(97), temp_c: Some(65), clock_mhz: Some(1920), power_w: Some(151.6), vram_used_mb: Some(5325), vram_total_mb: Some(8192), ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn builds_lines() {
        let l = lines(&reading(), &OverlayConfig::default(), false);
        assert_eq!(l[0].value, "144  1% 98  7.0 ms");
        assert_eq!(l[1].value, "97%  65°C  1920 MHz  152 W");
        assert_eq!(l[2].value, "5.2 / 8.0 GB");
        assert_eq!(l[3].value, "23%");
        assert_eq!(l[4].value, "9.6 / 16.0 GB");
    }

    #[test]
    fn rtss_uses_macros_and_tags() {
        let cfg = OverlayConfig { show_gpu: false, show_vram: false, show_cpu: false, show_ram: false, ..Default::default() };
        let t = rtss_text(&lines(&reading(), &cfg, true));
        assert!(t.starts_with("<C0=4ADE80><C1=22D3EE><C2=F59E0B><C3=A78BFA><C4=FFFFFF>"));
        assert!(t.ends_with("<C0>FPS  <C><C4><FR>  1% 98  <FT> ms<C>\n"));
    }
}
