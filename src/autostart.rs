//! Jalan otomatis saat login lewat Task Scheduler dengan hak tertinggi, supaya tidak ada
//! prompt UAC setiap kali Windows menyala.

const TASK: &str = "Pacu Game Booster";

#[cfg(windows)]
fn schtasks(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("schtasks")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn schtasks(_args: &[&str]) -> bool {
    false
}

pub fn is_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK])
}

pub fn set(enabled: bool) -> bool {
    if enabled {
        let Ok(exe) = std::env::current_exe() else { return false };
        let tr = format!("\"{}\" --minimized", exe.display());
        schtasks(&["/Create", "/F", "/TN", TASK, "/TR", &tr, "/SC", "ONLOGON", "/RL", "HIGHEST", "/DELAY", "0000:15"])
    } else {
        schtasks(&["/Delete", "/F", "/TN", TASK])
    }
}
