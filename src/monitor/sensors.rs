//! Daftar sensor bergaya tabel HWiNFO: nilai sekarang, minimum, maksimum, dan rata-rata sejak
//! Pacu dibuka (atau sejak direset).

use super::Reading;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Fps,
    Gpu,
    Cpu,
    Memory,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Fps => "Frame",
            Group::Gpu => "GPU",
            Group::Cpu => "CPU",
            Group::Memory => "Memori",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sensor {
    pub group: Group,
    pub name: &'static str,
    pub unit: &'static str,
    /// Jumlah angka di belakang koma.
    pub decimals: usize,
    pub now: Option<f32>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    sum: f64,
    count: u64,
}

impl Sensor {
    pub fn avg(&self) -> Option<f32> {
        (self.count > 0).then(|| (self.sum / self.count as f64) as f32)
    }

    pub fn fmt(&self, v: Option<f32>) -> String {
        match v {
            Some(v) => format!("{v:.*} {}", self.decimals, self.unit),
            None => "-".into(),
        }
    }
}

/// (kelompok, nama, satuan, desimal, nilai).
fn readings(r: &Reading) -> Vec<(Group, &'static str, &'static str, usize, Option<f32>)> {
    let g = &r.gpu;
    let u = |v: Option<u32>| v.map(|x| x as f32);
    vec![
        (Group::Fps, "Frame rate", "FPS", 0, r.fps),
        (Group::Fps, "1% low", "FPS", 0, r.low1),
        (Group::Fps, "Frame time", "ms", 1, r.frametime_ms),
        (Group::Gpu, "Pemakaian GPU", "%", 0, u(g.util_pct)),
        (Group::Gpu, "Suhu GPU", "°C", 0, u(g.temp_c)),
        (Group::Gpu, "Clock inti", "MHz", 0, u(g.clock_mhz)),
        (Group::Gpu, "Clock memori", "MHz", 0, u(g.mem_clock_mhz)),
        (Group::Gpu, "Tegangan inti", "mV", 0, g.voltage_mv),
        (Group::Gpu, "Daya", "W", 1, g.power_w),
        (Group::Gpu, "Kipas", "%", 0, u(g.fan_pct)),
        (Group::Gpu, "VRAM terpakai", "MB", 0, u(g.vram_used_mb)),
        (Group::Cpu, "Pemakaian CPU", "%", 0, Some(r.cpu_pct)),
        (Group::Cpu, "Suhu paket", "°C", 0, r.cpu.temp_c),
        (Group::Cpu, "Clock efektif", "MHz", 0, r.cpu.mhz),
        (Group::Cpu, "Daya paket", "W", 1, r.cpu.power_w),
        (Group::Memory, "RAM terpakai", "MB", 0, Some(r.ram_used_mb as f32)),
        (Group::Memory, "RAM tersedia", "MB", 0, Some(r.ram_total_mb.saturating_sub(r.ram_used_mb) as f32)),
    ]
}

#[derive(Clone, Debug, Default)]
pub struct Sensors {
    pub list: Vec<Sensor>,
}

impl Sensors {
    pub fn update(&mut self, r: &Reading) {
        let rows = readings(r);
        if self.list.len() != rows.len() {
            self.list = rows
                .iter()
                .map(|&(group, name, unit, decimals, _)| Sensor { group, name, unit, decimals, now: None, min: None, max: None, sum: 0.0, count: 0 })
                .collect();
        }
        for (s, (_, _, _, _, v)) in self.list.iter_mut().zip(rows) {
            s.now = v;
            if let Some(v) = v {
                s.min = Some(s.min.map_or(v, |m| m.min(v)));
                s.max = Some(s.max.map_or(v, |m| m.max(v)));
                s.sum += v as f64;
                s.count += 1;
            }
        }
    }

    pub fn reset(&mut self) {
        self.list.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_avg() {
        let mut s = Sensors::default();
        let mut r = Reading { cpu_pct: 10.0, ..Default::default() };
        s.update(&r);
        r.cpu_pct = 30.0;
        r.fps = Some(100.0);
        s.update(&r);
        let cpu = s.list.iter().find(|x| x.name == "Pemakaian CPU").unwrap();
        assert_eq!((cpu.min, cpu.max, cpu.avg()), (Some(10.0), Some(30.0), Some(20.0)));
        let fps = s.list.iter().find(|x| x.name == "Frame rate").unwrap();
        assert_eq!((fps.min, fps.avg()), (Some(100.0), Some(100.0)));
        assert_eq!(fps.fmt(fps.now), "100 FPS");
        s.reset();
        assert!(s.list.is_empty());
    }
}
