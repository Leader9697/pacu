//! Perangkat yang boleh membangunkan PC dari sleep (mouse, keyboard, kartu jaringan, USB hub).
//! Sama dengan centang "Allow this device to wake the computer" di Device Manager, lewat
//! `powercfg /devicequery` dan `/deviceenablewake` / `/devicedisablewake` (butuh admin; Pacu
//! sudah berjalan sebagai admin).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mouse,
    Keyboard,
    Network,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Mouse => "Mouse",
            Kind::Keyboard => "Keyboard",
            Kind::Network => "Jaringan",
            Kind::Other => "Lainnya",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub kind: Kind,
    /// Saat ini boleh membangunkan PC.
    pub armed: bool,
}

pub fn classify(name: &str) -> Kind {
    let n = name.to_lowercase();
    if n.contains("mouse") || n.contains("pointing") || n.contains("touchpad") {
        Kind::Mouse
    } else if n.contains("keyboard") || n.contains("keypad") {
        Kind::Keyboard
    } else if ["ethernet", "wi-fi", "wifi", "wireless", "wlan", "network", "gbe", "lan", "802.11", "bluetooth"].iter().any(|k| n.contains(k)) {
        Kind::Network
    } else {
        Kind::Other
    }
}

/// Baris keluaran `powercfg /devicequery` jadi daftar nama. "NONE" (atau kosong) berarti tidak ada.
pub fn parse_query(out: &str) -> Vec<String> {
    out.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("none"))
        .map(|l| l.to_string())
        .collect()
}

pub fn build_list(programmable: Vec<String>, armed: &[String]) -> Vec<Device> {
    let mut v: Vec<Device> = programmable
        .into_iter()
        .map(|name| Device { kind: classify(&name), armed: armed.iter().any(|a| a == &name), name })
        .collect();
    v.sort_by(|a, b| (a.kind as u8, a.name.to_lowercase()).cmp(&(b.kind as u8, b.name.to_lowercase())));
    v.dedup_by(|a, b| a.name == b.name);
    v
}

#[cfg(windows)]
fn powercfg(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powercfg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powercfg tidak bisa dijalankan: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("powercfg gagal: {}", if err.trim().is_empty() { text.trim() } else { err.trim() }));
    }
    Ok(text)
}

#[cfg(not(windows))]
fn powercfg(args: &[&str]) -> Result<String, String> {
    // Daftar tiruan untuk pengembangan di Linux.
    match args.get(1).copied() {
        Some("wake_programmable") => Ok("HID-compliant mouse\nHID Keyboard Device\nIntel(R) Ethernet Controller I225-V\nUSB Root Hub (USB 3.0)\n".into()),
        Some("wake_armed") => Ok("HID-compliant mouse\nHID Keyboard Device\n".into()),
        _ => Ok(String::new()),
    }
}

pub fn list() -> Result<Vec<Device>, String> {
    let prog = parse_query(&powercfg(&["/devicequery", "wake_programmable"])?);
    let armed = parse_query(&powercfg(&["/devicequery", "wake_armed"])?);
    Ok(build_list(prog, &armed))
}

pub fn set(name: &str, armed: bool) -> Result<(), String> {
    powercfg(&[if armed { "/deviceenablewake" } else { "/devicedisablewake" }, name]).map(|_| ())
}

/// Saat Pacu menyala: matikan lagi wake untuk perangkat yang diblokir pengguna (update driver
/// atau Windows Update kadang menyalakannya kembali). Mengembalikan jumlah yang diubah.
pub fn enforce(blocked: &[String]) -> usize {
    if blocked.is_empty() {
        return 0;
    }
    let Ok(armed) = powercfg(&["/devicequery", "wake_armed"]).map(|o| parse_query(&o)) else { return 0 };
    blocked.iter().filter(|b| armed.contains(b)).filter(|b| set(b, false).is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_classify() {
        assert!(parse_query("NONE\r\n").is_empty());
        let prog = parse_query("HID-compliant mouse\r\nHID Keyboard Device\r\nRealtek PCIe GbE Family Controller\r\nUSB Root Hub (USB 3.0)\r\n\r\n");
        let armed = parse_query("HID-compliant mouse\r\nRealtek PCIe GbE Family Controller\r\n");
        let l = build_list(prog, &armed);
        assert_eq!(l.len(), 4);
        assert_eq!((l[0].kind, l[0].armed), (Kind::Mouse, true));
        assert_eq!((l[1].kind, l[1].armed), (Kind::Keyboard, false));
        assert_eq!(l[2].kind, Kind::Network);
        assert_eq!(l[3].kind, Kind::Other);
        assert_eq!(classify("Logitech USB Receiver Keyboard"), Kind::Keyboard);
    }
}
