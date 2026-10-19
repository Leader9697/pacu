//! Pengukur FPS dan frame time memakai PresentMon (Intel, lisensi MIT) yang dibundel di dalam Pacu.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(windows)]
const PRESENTMON: &[u8] = include_bytes!("../res/PresentMon.exe");
pub const PRESENTMON_VERSION: &str = "2.6.0";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchResult {
    pub label: String,
    pub game: String,
    pub seconds: u32,
    pub frames: usize,
    pub avg_fps: f32,
    pub low1_fps: f32,
    pub low01_fps: f32,
}

#[derive(Default)]
pub struct Stats {
    pub game: String,
    /// Frame time terakhir (ms), maksimal 600 frame.
    pub frame_times: VecDeque<f32>,
    pub last_frame: Option<Instant>,
    pub recording: Option<(Instant, String, Vec<f32>)>,
    pub results: Vec<BenchResult>,
    pub error: Option<String>,
}

impl Stats {
    fn push(&mut self, ms: f32) {
        if !(0.05..2000.0).contains(&ms) {
            return;
        }
        self.frame_times.push_back(ms);
        while self.frame_times.len() > 600 {
            self.frame_times.pop_front();
        }
        self.last_frame = Some(Instant::now());
        if let Some((_, _, frames)) = self.recording.as_mut() {
            frames.push(ms);
        }
    }

    /// (FPS rata-rata, 1% low) dari ±2 detik terakhir.
    pub fn live(&self) -> Option<(f32, f32)> {
        if self.last_frame.map(|t| t.elapsed() > Duration::from_secs(2)).unwrap_or(true) {
            return None;
        }
        let mut sum = 0.0;
        let mut recent = Vec::new();
        for &ft in self.frame_times.iter().rev() {
            recent.push(ft);
            sum += ft;
            if sum >= 2000.0 {
                break;
            }
        }
        summarize(&recent).map(|(avg, low1, _)| (avg, low1))
    }
}

/// (FPS rata-rata, 1% low, 0,1% low) dari daftar frame time (ms).
pub fn summarize(frames: &[f32]) -> Option<(f32, f32, f32)> {
    if frames.len() < 10 {
        return None;
    }
    let total: f32 = frames.iter().sum();
    let avg = frames.len() as f32 * 1000.0 / total;
    let mut sorted = frames.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let low = |pct: f32| {
        let n = ((sorted.len() as f32 * pct).ceil() as usize).max(1);
        let worst: f32 = sorted[..n].iter().sum::<f32>() / n as f32;
        1000.0 / worst
    };
    Some((avg, low(0.01), low(0.001)))
}

pub struct FpsMeter {
    pub stats: Arc<Mutex<Stats>>,
    running_for: Option<String>,
    child: Option<std::process::Child>,
}

impl Default for FpsMeter {
    fn default() -> Self {
        let stats = Stats { results: load_results(), ..Default::default() };
        FpsMeter { stats: Arc::new(Mutex::new(stats)), running_for: None, child: None }
    }
}

fn results_path() -> std::path::PathBuf {
    crate::config::data_dir().join("benchmark.json")
}

fn load_results() -> Vec<BenchResult> {
    std::fs::read_to_string(results_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_results(r: &[BenchResult]) {
    let _ = std::fs::create_dir_all(crate::config::data_dir());
    let _ = std::fs::write(results_path(), serde_json::to_string_pretty(r).unwrap());
}

impl FpsMeter {
    pub fn running_for(&self) -> Option<&str> {
        self.running_for.as_deref()
    }

    /// Mulai mengukur proses `exe` (nama file). Tidak melakukan apa-apa bila sudah berjalan untuk exe itu.
    pub fn start(&mut self, exe: &str) {
        if self.running_for.as_deref() == Some(exe) {
            return;
        }
        self.stop();
        {
            let mut s = self.stats.lock();
            s.game = exe.to_string();
            s.frame_times.clear();
            s.error = None;
        }
        self.running_for = Some(exe.to_string());
        match spawn(exe, self.stats.clone()) {
            Ok(child) => self.child = child,
            Err(e) => self.stats.lock().error = Some(e),
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.running_for = None;
    }

    pub fn start_recording(&self, label: String) {
        self.stats.lock().recording = Some((Instant::now(), label, Vec::new()));
    }

    /// Selesaikan rekaman bila sudah `secs` detik. Mengembalikan true bila baru selesai.
    pub fn poll_recording(&self, secs: u64) -> bool {
        let mut s = self.stats.lock();
        let done = s.recording.as_ref().map(|r| r.0.elapsed().as_secs() >= secs).unwrap_or(false);
        if !done {
            return false;
        }
        let (start, label, frames) = s.recording.take().unwrap();
        if let Some((avg, low1, low01)) = summarize(&frames) {
            let r = BenchResult {
                label,
                game: s.game.clone(),
                seconds: start.elapsed().as_secs() as u32,
                frames: frames.len(),
                avg_fps: avg,
                low1_fps: low1,
                low01_fps: low01,
            };
            s.results.push(r);
            save_results(&s.results);
        } else {
            s.error = Some("Tidak ada frame yang terekam. Pastikan game sedang berjalan di depan.".into());
        }
        true
    }
}

impl Drop for FpsMeter {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Membaca CSV PresentMon dari stdout. Kolom frame time: "FrameTime" (v2) atau "MsBetweenPresents" (v1).
#[cfg_attr(not(windows), allow(dead_code))]
fn reader_loop(out: impl std::io::Read, stats: Arc<Mutex<Stats>>) {
    use std::io::BufRead;
    let mut col: Option<usize> = None;
    for line in std::io::BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        let fields: Vec<&str> = line.split(',').collect();
        match col {
            None => {
                col = fields
                    .iter()
                    .position(|f| f.trim() == "FrameTime")
                    .or_else(|| fields.iter().position(|f| f.trim() == "MsBetweenPresents"));
            }
            Some(i) => {
                if let Some(v) = fields.get(i).and_then(|f| f.trim().parse::<f32>().ok()) {
                    stats.lock().push(v);
                }
            }
        }
    }
}

#[cfg(windows)]
fn spawn(exe: &str, stats: Arc<Mutex<Stats>>) -> Result<Option<std::process::Child>, String> {
    use std::os::windows::process::CommandExt;
    let dir = crate::config::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("PresentMon-{PRESENTMON_VERSION}.exe"));
    if std::fs::metadata(&path).map(|m| m.len() != PRESENTMON.len() as u64).unwrap_or(true) {
        std::fs::write(&path, PRESENTMON).map_err(|e| format!("Gagal menyiapkan PresentMon: {e}"))?;
    }
    let mut child = std::process::Command::new(&path)
        .args([
            "--process_name",
            exe,
            "--output_stdout",
            "--stop_existing_session",
            "--session_name",
            "PacuFPS",
            "--terminate_on_proc_exit",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("Gagal menjalankan PresentMon: {e}"))?;
    let out = child.stdout.take().unwrap();
    std::thread::spawn(move || reader_loop(out, stats));
    Ok(Some(child))
}

/// Di luar Windows: frame palsu ±150 FPS untuk pengembangan UI.
#[cfg(not(windows))]
fn spawn(_exe: &str, stats: Arc<Mutex<Stats>>) -> Result<Option<std::process::Child>, String> {
    std::thread::spawn(move || {
        let mut i = 0u64;
        loop {
            i += 1;
            let ft = 6.6 + ((i as f32) * 0.37).sin() * 0.6 + if i.is_multiple_of(97) { 9.0 } else { 0.0 };
            stats.lock().push(ft);
            std::thread::sleep(Duration::from_micros((ft * 1000.0) as u64));
        }
    });
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_lows() {
        let mut f = vec![10.0f32; 990];
        f.extend(vec![50.0f32; 10]);
        let (avg, low1, low01) = summarize(&f).unwrap();
        assert!((avg - 1000.0 * 1000.0 / (9900.0 + 500.0)).abs() < 0.01);
        assert!((low1 - 20.0).abs() < 0.01);
        assert!((low01 - 20.0).abs() < 0.01);
    }

    #[test]
    fn parses_v2_and_v1_csv() {
        let csv = "Application,ProcessID,FrameTime,CPUBusy\nproject8.exe,1,6.5,3\nproject8.exe,1,NA,3\nproject8.exe,1,7.0,3\n";
        let stats = Arc::new(Mutex::new(Stats::default()));
        reader_loop(csv.as_bytes(), stats.clone());
        assert_eq!(stats.lock().frame_times.iter().copied().collect::<Vec<_>>(), vec![6.5, 7.0]);
        let csv = "Application,MsBetweenPresents\nx,8.0\n";
        let stats = Arc::new(Mutex::new(Stats::default()));
        reader_loop(csv.as_bytes(), stats.clone());
        assert_eq!(stats.lock().frame_times.len(), 1);
    }
}
