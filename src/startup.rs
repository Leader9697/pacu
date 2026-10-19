//! Aplikasi yang ikut berjalan saat Windows menyala. Menyalakan/mematikannya memakai cara yang sama
//! dengan tab Startup di Task Manager (key `StartupApproved` dan status startup task aplikasi Store),
//! jadi entri aslinya tidak dihapus dan bisa dinyalakan lagi dari Pacu maupun Task Manager.

use crate::tweaks::reg;
use crate::tweaks::Root;

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN32: &str = "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";
const APPDATA_PKGS: &str = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\SystemAppData";

#[derive(Clone, Debug, PartialEq)]
pub enum Toggle {
    /// Nilai biner di `StartupApproved\<sub>`.
    Approved { root: Root, sub: &'static str, value: String },
    /// Startup task aplikasi Store: DWORD `State` di key ini (HKCU).
    StoreTask { path: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub command: String,
    pub source: &'static str,
    pub enabled: bool,
    /// Berlaku untuk semua pengguna (HKLM / folder Startup bersama).
    pub all_users: bool,
    pub toggle: Toggle,
}

/// Byte pertama genap (02, 06) = nyala, ganjil (03) = dimatikan. Tanpa nilai = nyala.
pub fn approved_enabled(bytes: Option<&[u8]>) -> bool {
    bytes.and_then(|b| b.first()).map(|x| x & 1 == 0).unwrap_or(true)
}

/// Nilai yang ditulis Task Manager: 12 byte, 02 = nyala, 03 + waktu (FILETIME) = dimatikan.
pub fn approved_bytes(enabled: bool, filetime: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0] = if enabled { 2 } else { 3 };
    if !enabled {
        b[4..].copy_from_slice(&filetime.to_le_bytes());
    }
    b
}

/// Nama pendek dari package family name, misalnya "MSTeams_8wekyb3d8bbwe" jadi "MSTeams".
pub fn package_label(family: &str) -> String {
    let base = family.split('_').next().unwrap_or(family);
    base.rsplit('.').next().unwrap_or(base).to_string()
}

fn filetime_now() -> u64 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (unix.as_secs() + 11_644_473_600) * 10_000_000 + unix.subsec_nanos() as u64 / 100
}

fn approved(root: Root, sub: &'static str, value: &str) -> bool {
    approved_enabled(reg::read_bin(root, &format!("{APPROVED}\\{sub}"), value).as_deref())
}

fn run_entries(out: &mut Vec<Entry>, root: Root, path: &str, sub: &'static str, source: &'static str) {
    for (name, command) in reg::values(root, path) {
        out.push(Entry {
            enabled: approved(root, sub, &name),
            toggle: Toggle::Approved { root, sub, value: name.clone() },
            name,
            command,
            source,
            all_users: root == Root::Hklm,
        });
    }
}

fn folder_entries(out: &mut Vec<Entry>, dir: Option<std::path::PathBuf>, root: Root) {
    let Some(dir) = dir else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        if file.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let name = file.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or_else(|| file.clone());
        out.push(Entry {
            name,
            command: e.path().display().to_string(),
            source: "Folder Startup",
            enabled: approved(root, "StartupFolder", &file),
            all_users: root == Root::Hklm,
            toggle: Toggle::Approved { root, sub: "StartupFolder", value: file },
        });
    }
}

fn store_entries(out: &mut Vec<Entry>) {
    for pkg in reg::subkeys(Root::Hkcu, APPDATA_PKGS) {
        let pkg_path = format!("{APPDATA_PKGS}\\{pkg}");
        for task in reg::subkeys(Root::Hkcu, &pkg_path) {
            let path = format!("{pkg_path}\\{task}");
            let Ok(Some(crate::tweaks::Saved::Dword(state))) = reg::read(Root::Hkcu, &path, "State") else { continue };
            // 3/4 = diatur kebijakan, tidak bisa diubah pengguna.
            if state > 2 {
                continue;
            }
            out.push(Entry {
                name: package_label(&pkg),
                command: pkg.clone(),
                source: "Aplikasi Store",
                enabled: state == 2,
                all_users: false,
                toggle: Toggle::StoreTask { path },
            });
        }
    }
}

fn env_dir(var: &str, rest: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(var).map(|v| std::path::PathBuf::from(v).join(rest))
}

/// Semua aplikasi startup, urut nama.
pub fn list() -> Vec<Entry> {
    let mut out = Vec::new();
    run_entries(&mut out, Root::Hkcu, RUN, "Run", "Registry (pengguna)");
    run_entries(&mut out, Root::Hklm, RUN, "Run", "Registry (semua pengguna)");
    run_entries(&mut out, Root::Hklm, RUN32, "Run32", "Registry (semua pengguna)");
    let folder = "Microsoft\\Windows\\Start Menu\\Programs\\Startup";
    folder_entries(&mut out, env_dir("APPDATA", folder), Root::Hkcu);
    folder_entries(&mut out, env_dir("ProgramData", folder), Root::Hklm);
    store_entries(&mut out);
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

pub fn set_enabled(e: &Entry, enabled: bool) -> Result<(), String> {
    match &e.toggle {
        Toggle::Approved { root, sub, value } => reg::write_bin(*root, &format!("{APPROVED}\\{sub}"), value, &approved_bytes(enabled, filetime_now())),
        Toggle::StoreTask { path } => reg::write(Root::Hkcu, path, "State", &crate::tweaks::Saved::Dword(if enabled { 2 } else { 1 })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_flags() {
        assert!(approved_enabled(None));
        assert!(approved_enabled(Some(&[2, 0, 0, 0])));
        assert!(approved_enabled(Some(&[6, 0, 0, 0])));
        assert!(!approved_enabled(Some(&[3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])));
        let b = approved_bytes(false, 0x0102030405060708);
        assert_eq!(b, [3, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(approved_bytes(true, 99), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn package_labels() {
        assert_eq!(package_label("MSTeams_8wekyb3d8bbwe"), "MSTeams");
        assert_eq!(package_label("SpotifyAB.SpotifyMusic_zpdnekdrzrea0"), "SpotifyMusic");
        assert_eq!(package_label("Discord"), "Discord");
    }
}
