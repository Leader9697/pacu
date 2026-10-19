//! Penghemat RAM khusus Discord. Bagian proses (merampingkan, meredam saat bermain, batas keras
//! working set) dijalankan mesin; modul ini menyimpan daftar nama proses dan mengubah
//! `settings.json` milik Discord (akselerasi hardware, ikut menyala saat login).

use serde_json::{Map, Value};
use std::path::PathBuf;

/// Nama exe Discord (huruf kecil): klien resmi, PTB, Canary, dan Vesktop.
pub const PROCESS_NAMES: &[&str] = &["discord.exe", "discordptb.exe", "discordcanary.exe", "vesktop.exe"];

pub fn is_discord(name_lc: &str) -> bool {
    PROCESS_NAMES.contains(&name_lc)
}

/// Kunci di settings.json Discord.
pub const HW_ACCEL: &str = "enableHardwareAcceleration";
pub const OPEN_ON_STARTUP: &str = "OPEN_ON_STARTUP";

#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub label: &'static str,
    pub exe: &'static str,
    pub settings: PathBuf,
    /// `%LOCALAPPDATA%\<folder>\Update.exe`, untuk membuka Discord lagi setelah pengaturan diubah.
    pub launcher: Option<PathBuf>,
    pub hw_accel: bool,
    pub open_on_startup: bool,
}

/// (label, exe, folder data di %APPDATA%, folder instalasi di %LOCALAPPDATA%)
const CLIENTS: &[(&str, &str, &str, &str)] = &[
    ("Discord", "Discord.exe", "discord", "Discord"),
    ("Discord PTB", "DiscordPTB.exe", "discordptb", "DiscordPTB"),
    ("Discord Canary", "DiscordCanary.exe", "discordcanary", "DiscordCanary"),
];

fn roaming() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        // Discord untuk Linux menyimpan pengaturannya di ~/.config/discord.
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
    }
}

/// Klien Discord yang terpasang (folder datanya ada).
pub fn clients() -> Vec<Client> {
    let Some(base) = roaming() else { return Vec::new() };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    CLIENTS
        .iter()
        .filter_map(|&(label, exe, data, install)| {
            let dir = base.join(data);
            if !dir.is_dir() {
                return None;
            }
            let settings = dir.join("settings.json");
            let obj = read(&settings);
            let get = |k: &str| obj.get(k).and_then(Value::as_bool).unwrap_or(true);
            let launcher = local.as_ref().map(|l| l.join(install).join("Update.exe")).filter(|p| p.is_file());
            Some(Client { label, exe, hw_accel: get(HW_ACCEL), open_on_startup: get(OPEN_ON_STARTUP), settings, launcher })
        })
        .collect()
}

fn read(path: &std::path::Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Mengubah satu kunci dan mempertahankan kunci lain apa adanya.
pub fn set_key(text: &str, key: &str, value: bool) -> Result<String, String> {
    let mut v: Value = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(text).map_err(|e| format!("settings.json Discord rusak: {e}"))?
    };
    let obj = v.as_object_mut().ok_or("settings.json Discord bukan objek JSON")?;
    obj.insert(key.to_string(), Value::Bool(value));
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// Menutup Discord (bila berjalan), menulis pengaturan, lalu membukanya lagi. Discord menimpa
/// settings.json saat keluar, jadi pengaturan tidak bisa ditulis selama Discord masih jalan.
/// Mengembalikan `true` bila Discord dibuka ulang.
pub fn apply(client: &Client, key: &str, value: bool) -> Result<bool, String> {
    let was_running = kill(client.exe);
    if was_running {
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    let text = std::fs::read_to_string(&client.settings).unwrap_or_default();
    let out = set_key(&text, key, value)?;
    if client.settings.is_file() {
        let _ = std::fs::copy(&client.settings, client.settings.with_extension("json.pacu-bak"));
    }
    std::fs::write(&client.settings, out).map_err(|e| format!("Gagal menulis {}: {e}", client.settings.display()))?;
    if was_running {
        if let Some(l) = &client.launcher {
            return Ok(launch(l, client.exe));
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn kill(exe: &str) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("taskkill")
        .args(["/F", "/T", "/IM", exe])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn launch(update_exe: &std::path::Path, exe: &str) -> bool {
    std::process::Command::new(update_exe).args(["--processStart", exe]).spawn().is_ok()
}

#[cfg(not(windows))]
fn kill(_: &str) -> bool {
    false
}

#[cfg(not(windows))]
fn launch(_: &std::path::Path, _: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_key_keeps_other_keys() {
        let src = r##"{"BACKGROUND_COLOR":"#202225","enableHardwareAcceleration":true,"chromiumSwitches":{}}"##;
        let out = set_key(src, HW_ACCEL, false).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["enableHardwareAcceleration"], Value::Bool(false));
        assert_eq!(v["BACKGROUND_COLOR"], "#202225");
        assert!(v["chromiumSwitches"].is_object());
    }

    #[test]
    fn set_key_on_missing_file() {
        let out = set_key("", OPEN_ON_STARTUP, false).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()[OPEN_ON_STARTUP], Value::Bool(false));
        assert!(set_key("[1,2]", HW_ACCEL, false).is_err());
    }

    #[test]
    fn names() {
        assert!(is_discord("discord.exe"));
        assert!(is_discord("discordcanary.exe"));
        assert!(!is_discord("discordhelper.exe"));
    }
}
