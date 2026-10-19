//! Semua yang dibutuhkan mesin pencari selain GPU: aplikasi beban 3D, pemeriksa error komputasi,
//! pemantau log driver, dan jam. Dipisah sebagai trait supaya algoritma bisa diuji dengan simulasi.

use crate::volt::gpu::Gpu;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadStatus {
    /// Aplikasi beban yang dijalankan Pacu Volt masih hidup.
    Running,
    /// Aplikasi beban tertutup; berisi keterangan kode keluar.
    Exited(String),
    /// Pengguna menjalankan aplikasi bebannya sendiri; dipantau lewat pemakaian GPU.
    External,
}

pub trait Load: Send {
    fn start(&mut self) -> Result<(), String>;
    fn status(&mut self) -> LoadStatus;
    fn stop(&mut self);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CheckStatus {
    pub runs: u64,
    pub errors: u64,
    pub device_lost: bool,
}

/// Pemeriksa error: menjalankan perhitungan GPU yang hasilnya pasti, lalu membandingkannya dengan
/// hasil acuan yang direkam saat GPU masih bawaan. Hasil yang berbeda berarti GPU salah hitung,
/// tanda awal undervolt tidak stabil sebelum terjadi crash.
pub trait Checker: Send {
    fn available(&self) -> bool;
    /// Merekam hasil acuan. Harus dipanggil saat kurva masih bawaan.
    fn prepare(&mut self) -> Result<(), String>;
    fn start(&mut self);
    /// Status sejak panggilan terakhir.
    fn take(&mut self) -> CheckStatus;
    fn stop(&mut self);
}

/// Pemantau log sistem untuk reset driver (TDR) dan error GPU.
pub trait Watchdog: Send {
    fn mark(&mut self);
    fn events_since_mark(&mut self) -> Vec<String>;
}

pub trait Clock: Send {
    fn sleep(&self, d: Duration);
}

pub struct Rig {
    pub gpu: Box<dyn Gpu>,
    pub load: Box<dyn Load>,
    pub checker: Box<dyn Checker>,
    pub watchdog: Box<dyn Watchdog>,
    pub clock: Box<dyn Clock>,
}

pub struct RealClock;

impl Clock for RealClock {
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d)
    }
}

/// Beban yang dijalankan sendiri oleh pengguna.
pub struct ExternalLoad;

impl Load for ExternalLoad {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn status(&mut self) -> LoadStatus {
        LoadStatus::External
    }
    fn stop(&mut self) {}
}

/// Menjalankan aplikasi beban 3D pilihan pengguna.
pub struct ProcessLoad {
    exe: String,
    args: String,
    child: Option<std::process::Child>,
}

impl ProcessLoad {
    pub fn new(exe: &str, args: &str) -> Self {
        Self { exe: exe.to_string(), args: args.to_string(), child: None }
    }
}

/// Memecah argumen baris perintah sederhana dengan dukungan tanda kutip.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

impl Load for ProcessLoad {
    fn start(&mut self) -> Result<(), String> {
        if matches!(self.status(), LoadStatus::Running) {
            return Ok(());
        }
        let path = std::path::Path::new(&self.exe);
        let mut cmd = std::process::Command::new(path);
        cmd.args(split_args(&self.args));
        if let Some(dir) = path.parent() {
            cmd.current_dir(dir);
        }
        let child = cmd.spawn().map_err(|e| format!("Gagal menjalankan {}: {e}", self.exe))?;
        self.child = Some(child);
        Ok(())
    }

    fn status(&mut self) -> LoadStatus {
        match &mut self.child {
            None => LoadStatus::Exited("belum dijalankan".into()),
            Some(c) => match c.try_wait() {
                Ok(None) => LoadStatus::Running,
                Ok(Some(st)) => LoadStatus::Exited(match st.code() {
                    Some(code) => format!("kode keluar {code:#x}"),
                    None => "dihentikan".into(),
                }),
                Err(e) => LoadStatus::Exited(e.to_string()),
            },
        }
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for ProcessLoad {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn split_args_quotes() {
        assert_eq!(super::split_args(r#"-a "b c" d"#), vec!["-a", "b c", "d"]);
        assert!(super::split_args("  ").is_empty());
    }
}
