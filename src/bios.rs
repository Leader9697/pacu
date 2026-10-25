//! Pemeriksa BIOS: membaca identitas motherboard, BIOS, CPU, dan RAM dari tabel SMBIOS, status
//! Resizable BAR, mode firmware, Secure Boot, dan Memory Integrity, lalu memberi rekomendasi
//! pengaturan yang perlu diubah sendiri di layar BIOS. Pacu tidak pernah menulis ke BIOS/NVRAM.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dimm {
    pub slot: String,
    pub size_mb: u64,
    /// Kode tipe memori SMBIOS (0x1A = DDR4, 0x22 = DDR5).
    pub mem_type: u8,
    /// Kecepatan maksimal yang dilaporkan modul (MT/s).
    pub rated: u32,
    /// Kecepatan yang sedang dipakai (MT/s).
    pub configured: u32,
    pub maker: String,
    pub part: String,
}

impl Dimm {
    pub fn type_name(&self) -> &'static str {
        match self.mem_type {
            0x18 => "DDR3",
            0x1A => "DDR4",
            0x22 => "DDR5",
            0x1E => "LPDDR4",
            0x23 => "LPDDR5",
            _ => "RAM",
        }
    }

    /// Batas atas kecepatan bawaan yang umum dipakai motherboard bila XMP/EXPO mati.
    fn jedec_max(&self) -> u32 {
        match self.mem_type {
            0x18 => 1333,
            0x1A => 2666,
            0x22 => 4800,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smbios {
    pub bios_vendor: String,
    pub bios_version: String,
    /// Format SMBIOS mm/dd/yyyy.
    pub bios_date: String,
    pub system_maker: String,
    pub system_product: String,
    pub board_maker: String,
    pub board_product: String,
    pub cpu: String,
    pub dimms: Vec<Dimm>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rebar {
    On { bar_mb: u32, vram_mb: u32 },
    Off { bar_mb: u32, vram_mb: u32 },
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub smbios: Smbios,
    /// true = UEFI, false = Legacy/CSM, None = tidak terbaca.
    pub uefi: Option<bool>,
    pub secure_boot: Option<bool>,
    pub hvci: Option<bool>,
    pub rebar: Rebar,
    pub gpu: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Pengaturan dengan dampak FPS paling besar yang masih belum aktif.
    Penting,
    Saran,
    Opsional,
    Oke,
}

#[derive(Clone, Debug)]
pub struct Advice {
    pub level: Level,
    pub title: String,
    pub body: String,
    /// Letak menu di BIOS vendor ini (perkiraan; nama bisa sedikit berbeda antar seri).
    pub menu: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Asus,
    Msi,
    Gigabyte,
    Asrock,
    Other,
}

pub fn vendor(maker: &str) -> Vendor {
    let m = maker.to_lowercase();
    if m.contains("asus") {
        Vendor::Asus
    } else if m.contains("micro-star") || m.contains("msi") {
        Vendor::Msi
    } else if m.contains("gigabyte") {
        Vendor::Gigabyte
    } else if m.contains("asrock") {
        Vendor::Asrock
    } else {
        Vendor::Other
    }
}

// ---------------------------------------------------------------------------------------------
// SMBIOS
// ---------------------------------------------------------------------------------------------

fn word(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn dword(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Mengurai tabel struktur SMBIOS (tanpa header RawSMBIOSData).
pub fn parse_smbios(table: &[u8]) -> Smbios {
    let mut out = Smbios::default();
    let mut i = 0;
    while i + 4 <= table.len() {
        let ty = table[i];
        let len = table[i + 1] as usize;
        if len < 4 || i + len > table.len() {
            break;
        }
        let f = &table[i..i + len];
        // Kumpulan string setelah bagian berformat, diakhiri dua byte nol.
        let mut j = i + len;
        let mut strings: Vec<String> = Vec::new();
        loop {
            let start = j;
            while j < table.len() && table[j] != 0 {
                j += 1;
            }
            if j >= table.len() {
                break;
            }
            if j == start {
                j += 1;
                if strings.is_empty() && j < table.len() && table[j] == 0 {
                    j += 1;
                }
                break;
            }
            strings.push(String::from_utf8_lossy(&table[start..j]).trim().to_string());
            j += 1;
        }
        let s = |off: usize| -> String {
            match f.get(off).copied() {
                Some(n) if n > 0 => strings.get(n as usize - 1).cloned().unwrap_or_default(),
                _ => String::new(),
            }
        };
        match ty {
            0 => {
                out.bios_vendor = s(0x04);
                out.bios_version = s(0x05);
                out.bios_date = s(0x08);
            }
            1 => {
                out.system_maker = s(0x04);
                out.system_product = s(0x05);
            }
            2 if out.board_product.is_empty() => {
                out.board_maker = s(0x04);
                out.board_product = s(0x05);
            }
            4 if out.cpu.is_empty() => out.cpu = s(0x10),
            17 => {
                let size_mb = match word(f, 0x0C).unwrap_or(0) {
                    0 | 0xFFFF => 0,
                    0x7FFF => dword(f, 0x1C).unwrap_or(0) as u64 & 0x7FFF_FFFF,
                    v if v & 0x8000 != 0 => (v & 0x7FFF) as u64 / 1024,
                    v => v as u64,
                };
                if size_mb > 0 {
                    let speed = |off: usize, ext: usize| match word(f, off).unwrap_or(0) {
                        0xFFFF => dword(f, ext).unwrap_or(0),
                        v => v as u32,
                    };
                    out.dimms.push(Dimm {
                        slot: s(0x10),
                        size_mb,
                        mem_type: f.get(0x12).copied().unwrap_or(0),
                        rated: speed(0x15, 0x54),
                        configured: speed(0x20, 0x58),
                        maker: s(0x17),
                        part: s(0x1A),
                    });
                }
            }
            127 => break,
            _ => {}
        }
        i = j;
    }
    out
}

/// Nilai "BAR1 Memory Usage > Total" dan "FB Memory Usage > Total" dari `nvidia-smi -q -d MEMORY`.
pub fn parse_nvidia_smi(text: &str) -> Rebar {
    let mut section = "";
    let (mut bar, mut fb) = (None, None);
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("FB Memory Usage") {
            section = "fb";
        } else if t.starts_with("BAR1 Memory Usage") {
            section = "bar";
        } else if t.ends_with("Memory Usage") {
            section = "";
        } else if let Some(rest) = t.strip_prefix("Total") {
            let mb = rest.trim_start_matches([' ', ':']).split_whitespace().next().and_then(|v| v.parse::<u32>().ok());
            match section {
                "fb" if fb.is_none() => fb = mb,
                "bar" if bar.is_none() => bar = mb,
                _ => {}
            }
        }
    }
    match (bar, fb) {
        // Tanpa ReBAR, jendela BAR1 hanya 256 MB; dengan ReBAR ukurannya setara (atau melebihi) VRAM.
        (Some(b), Some(v)) if b > 256 && b as u64 * 10 >= v as u64 * 9 => Rebar::On { bar_mb: b, vram_mb: v },
        (Some(b), Some(v)) => Rebar::Off { bar_mb: b, vram_mb: v },
        _ => Rebar::Unknown,
    }
}

/// Tahun dari tanggal BIOS mm/dd/yyyy.
fn bios_year(date: &str) -> Option<u32> {
    date.rsplit('/').next().and_then(|y| y.trim().parse().ok()).filter(|y| *y > 1990)
}

// ---------------------------------------------------------------------------------------------
// Rekomendasi
// ---------------------------------------------------------------------------------------------

fn menu_xmp(v: Vendor, amd: bool, ddr5: bool) -> String {
    let prof = if amd && ddr5 { "EXPO" } else { "XMP" };
    match v {
        Vendor::Asus => format!("Ai Tweaker > Ai Overclock Tuner > {prof} I"),
        Vendor::Msi => format!("OC > Extreme Memory Profile ({prof}) > Profile 1 (atau tombol {prof} di halaman depan)"),
        Vendor::Gigabyte => format!("Tweaker > Extreme Memory Profile ({prof}) > Profile1"),
        Vendor::Asrock => format!("OC Tweaker > DRAM Configuration > Load {prof} Setting > Profile 1"),
        Vendor::Other => format!("Menu Overclocking/Memory > {prof} atau DOCP/EXPO > Profile 1"),
    }
}

fn menu_rebar(v: Vendor) -> String {
    match v {
        Vendor::Asus => "Advanced > PCI Subsystem Settings > Above 4G Decoding = Enabled, Re-Size BAR Support = Auto".into(),
        Vendor::Msi => "Settings > Advanced > PCI Subsystem Settings > Above 4G memory/Crypto Currency mining = Enabled, Re-Size BAR Support = Enabled".into(),
        Vendor::Gigabyte => "Settings > IO Ports > Above 4G Decoding = Enabled, Re-Size BAR Support = Auto".into(),
        Vendor::Asrock => "Advanced > Chipset Configuration > Above 4G Decoding = Enabled, Re-Size BAR Support = Enabled".into(),
        Vendor::Other => "Cari Above 4G Decoding dan Re-Size BAR Support (biasanya di Advanced > PCI), nyalakan keduanya".into(),
    }
}

fn menu_csm(v: Vendor) -> String {
    match v {
        Vendor::Asus => "Boot > CSM (Compatibility Support Module) > Launch CSM = Disabled".into(),
        Vendor::Msi => "Settings > Advanced > Windows OS Configuration > BIOS UEFI/CSM Mode = UEFI".into(),
        Vendor::Gigabyte => "Boot > CSM Support = Disabled".into(),
        Vendor::Asrock => "Boot > CSM (Compatibility Support Module) > CSM = Disabled".into(),
        Vendor::Other => "Menu Boot > CSM / Legacy = Disabled (UEFI saja)".into(),
    }
}

fn menu_pbo(v: Vendor) -> String {
    match v {
        Vendor::Asus => "Ai Tweaker > Precision Boost Overdrive > Curve Optimizer".into(),
        Vendor::Msi => "OC > Advanced CPU Configuration > AMD Overclocking > Precision Boost Overdrive".into(),
        Vendor::Gigabyte => "Tweaker > Advanced CPU Settings > Precision Boost Overdrive".into(),
        Vendor::Asrock => "Advanced > AMD Overclocking > Precision Boost Overdrive".into(),
        Vendor::Other => "Menu AMD Overclocking > Precision Boost Overdrive > Curve Optimizer".into(),
    }
}

pub fn advice(r: &Report, current_year: u32) -> Vec<Advice> {
    let s = &r.smbios;
    let v = vendor(if s.board_maker.is_empty() { &s.system_maker } else { &s.board_maker });
    let cpu = s.cpu.to_lowercase();
    let amd = cpu.contains("amd") || cpu.contains("ryzen");
    let mut out = Vec::new();

    // XMP / EXPO
    if let Some(d) = s.dimms.iter().find(|d| d.configured > 0) {
        let ddr5 = d.mem_type == 0x22;
        let prof = if amd && ddr5 { "EXPO" } else { "XMP" };
        let slow = s.dimms.iter().any(|d| d.rated > 0 && d.configured > 0 && d.configured + 50 < d.rated);
        let at_jedec = d.jedec_max() > 0 && d.configured <= d.jedec_max();
        if slow {
            let rated = s.dimms.iter().map(|d| d.rated).max().unwrap_or(0);
            out.push(Advice {
                level: Level::Penting,
                title: format!("Nyalakan {prof}: RAM jalan di {} MT/s, padahal modul mampu {rated}", d.configured),
                body: format!("RAM yang lambat langsung menurunkan FPS minimum di game berat CPU seperti Deadlock (sering 10 sampai 20 persen). {prof} hanya memuat profil pabrik kit RAM, jadi aman dan bisa dikembalikan dengan clear CMOS."),
                menu: Some(menu_xmp(v, amd, ddr5)),
            });
        } else if at_jedec {
            out.push(Advice {
                level: Level::Saran,
                title: format!("Kemungkinan {prof} belum menyala ({} {} MT/s)", d.type_name(), d.configured),
                body: format!("{} MT/s adalah kecepatan standar tanpa profil. Cek label kit RAM (kode {}); bila tertulis lebih tinggi (misalnya 3600 atau 6000), nyalakan {prof}.", d.configured, if d.part.is_empty() { "tidak terbaca" } else { &d.part }),
                menu: Some(menu_xmp(v, amd, ddr5)),
            });
        } else {
            out.push(Advice {
                level: Level::Oke,
                title: format!("RAM sudah di {} MT/s ({prof} aktif)", d.configured),
                body: String::new(),
                menu: None,
            });
        }
        if s.dimms.len() == 1 {
            out.push(Advice {
                level: Level::Saran,
                title: "RAM hanya satu keping (single channel)".into(),
                body: "Dengan satu keping, lebar jalur memori hanya setengah. Dua keping identik di slot A2 dan B2 biasanya menaikkan FPS cukup jauh di game berat CPU.".into(),
                menu: None,
            });
        }
    }

    // Resizable BAR
    match r.rebar {
        Rebar::On { .. } => out.push(Advice { level: Level::Oke, title: "Resizable BAR aktif".into(), body: String::new(), menu: None }),
        Rebar::Off { .. } => {
            let mut body = String::from("Resizable BAR membuat CPU bisa mengakses seluruh VRAM sekaligus. Di RTX 30, driver NVIDIA hanya memakainya untuk game yang ada di daftar putihnya, jadi hasilnya 0 sampai 10 persen tergantung game.");
            if r.uefi == Some(false) {
                body.push_str(" Windows kamu boot dalam mode Legacy, padahal ReBAR butuh UEFI dan CSM mati. Ubah disk ke GPT dulu (mbr2gpt /convert /allowFullOS) sebelum mematikan CSM, kalau tidak Windows tidak bisa boot.");
            }
            out.push(Advice { level: Level::Saran, title: "Resizable BAR belum aktif".into(), body, menu: Some(menu_rebar(v)) });
        }
        Rebar::Unknown => {}
    }

    // Mode firmware
    if r.uefi == Some(false) {
        out.push(Advice {
            level: Level::Saran,
            title: "Windows boot dalam mode Legacy (CSM)".into(),
            body: "Mode UEFI dibutuhkan untuk Resizable BAR dan Secure Boot (beberapa anti-cheat memintanya). Konversi disk ke GPT dulu dengan mbr2gpt, baru matikan CSM.".into(),
            menu: Some(menu_csm(v)),
        });
    }

    // Umur BIOS
    if let Some(y) = bios_year(&s.bios_date) {
        if current_year >= y + 2 {
            out.push(Advice {
                level: Level::Saran,
                title: format!("BIOS dari tahun {y}, sudah ada versi lebih baru"),
                body: format!("Versi baru biasanya memperbaiki stabilitas RAM/XMP, dukungan ReBAR, dan microcode CPU{}. Unduh hanya dari situs resmi untuk {} {}, dan jangan matikan PC saat proses flash. Setelah update, pengaturan BIOS kembali ke bawaan, jadi nyalakan lagi XMP dan ReBAR.", if amd { " (AGESA)" } else { "" }, s.board_maker, s.board_product),
                menu: None,
            });
        }
    }

    if amd {
        out.push(Advice {
            level: Level::Opsional,
            title: "Precision Boost Overdrive dan Curve Optimizer".into(),
            body: "Ini undervolt CPU Ryzen: mulai dari All Core Negative 10, uji stabil dengan game dan OCCT, naikkan bertahap sampai 20 atau 30. Membuat boost lebih tinggi dan suhu lebih rendah. Kalau tidak stabil, PC bisa restart atau error acak; kembalikan ke Auto.".into(),
            menu: Some(menu_pbo(v)),
        });
    }
    out.push(Advice {
        level: Level::Opsional,
        title: "C-states dan fitur hemat daya CPU".into(),
        body: "Mematikan C-states (Global C-state Control di AMD, CPU C-states di Intel) sedikit menurunkan latensi, tetapi CPU lebih panas dan pada Ryzen boost satu inti bisa turun. Untuk game, profil daya Pacu Performa di Windows biasanya sudah cukup; ubah ini hanya bila ingin bereksperimen.".into(),
        menu: None,
    });

    if r.hvci == Some(true) {
        out.push(Advice {
            level: Level::Opsional,
            title: "Memory Integrity (HVCI) menyala".into(),
            body: "Ini bukan pengaturan BIOS, tetapi berbiaya beberapa persen FPS. Bisa dimatikan di Optimasi Windows > Berisiko, dengan konsekuensi keamanan yang dijelaskan di sana.".into(),
            menu: None,
        });
    }

    out.sort_by_key(|a| a.level);
    out
}

/// Laporan teks untuk disalin ke clipboard (misalnya untuk dikirim ke Claude).
pub fn report_text(r: &Report, tips: &[Advice]) -> String {
    let s = &r.smbios;
    let yn = |b: Option<bool>| match b {
        Some(true) => "ya",
        Some(false) => "tidak",
        None => "tidak terbaca",
    };
    let mut t = String::from("Laporan BIOS Pacu\n");
    t += &format!("Motherboard: {} {}\n", s.board_maker, s.board_product);
    if !s.system_product.is_empty() && s.system_product != s.board_product {
        t += &format!("Sistem: {} {}\n", s.system_maker, s.system_product);
    }
    t += &format!("BIOS: {} versi {} ({})\n", s.bios_vendor, s.bios_version, s.bios_date);
    t += &format!("CPU: {}\n", s.cpu);
    t += &format!("GPU: {}\n", r.gpu);
    for d in &s.dimms {
        t += &format!(
            "RAM {}: {} GB {} {} / {} MT/s (jalan/maks) {} {}\n",
            d.slot,
            d.size_mb / 1024,
            d.type_name(),
            d.configured,
            d.rated,
            d.maker,
            d.part
        );
    }
    t += &format!("UEFI: {}, Secure Boot: {}, Memory Integrity: {}\n", yn(r.uefi), yn(r.secure_boot), yn(r.hvci));
    t += &match r.rebar {
        Rebar::On { bar_mb, vram_mb } => format!("Resizable BAR: aktif (BAR1 {bar_mb} MB, VRAM {vram_mb} MB)\n"),
        Rebar::Off { bar_mb, vram_mb } => format!("Resizable BAR: mati (BAR1 {bar_mb} MB, VRAM {vram_mb} MB)\n"),
        Rebar::Unknown => "Resizable BAR: tidak terbaca\n".into(),
    };
    t += "\nRekomendasi:\n";
    for a in tips.iter().filter(|a| a.level != Level::Oke) {
        t += &format!("- [{:?}] {}\n", a.level, a.title);
        if let Some(m) = &a.menu {
            t += &format!("  Menu: {m}\n");
        }
    }
    t
}

// ---------------------------------------------------------------------------------------------
// Pengambilan data
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
mod native {
    use super::*;
    use crate::tweaks::{reg, Root, Saved};

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemFirmwareTable(provider: u32, id: u32, buf: *mut u8, size: u32) -> u32;
        fn GetFirmwareType(ty: *mut u32) -> i32;
    }

    const RSMB: u32 = u32::from_be_bytes(*b"RSMB");

    pub fn smbios() -> Smbios {
        unsafe {
            let n = GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0);
            if n < 8 {
                return Smbios::default();
            }
            let mut buf = vec![0u8; n as usize];
            if GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr(), n) != n {
                return Smbios::default();
            }
            // RawSMBIOSData: 4 byte versi lalu panjang tabel (u32), baru tabelnya.
            let len = (u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize).min(buf.len() - 8);
            parse_smbios(&buf[8..8 + len])
        }
    }

    pub fn uefi() -> Option<bool> {
        let mut ty = 0u32;
        // FirmwareTypeBios = 1, FirmwareTypeUefi = 2.
        (unsafe { GetFirmwareType(&mut ty) } != 0).then_some(ty == 2).filter(|_| ty == 1 || ty == 2)
    }

    fn dword(path: &str, name: &str) -> Option<u32> {
        match reg::read(Root::Hklm, path, name) {
            Ok(Some(Saved::Dword(d))) => Some(d),
            _ => None,
        }
    }

    pub fn secure_boot() -> Option<bool> {
        dword("SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State", "UEFISecureBootEnabled").map(|d| d != 0)
    }

    pub fn hvci() -> Option<bool> {
        Some(dword("SYSTEM\\CurrentControlSet\\Control\\DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity", "Enabled").unwrap_or(0) != 0)
    }

    fn smi(args: &[&str]) -> Option<String> {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("nvidia-smi").args(args).creation_flags(0x0800_0000).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn rebar() -> Rebar {
        smi(&["-q", "-d", "MEMORY"]).map(|t| parse_nvidia_smi(&t)).unwrap_or(Rebar::Unknown)
    }

    pub fn gpu() -> String {
        smi(&["--query-gpu=name", "--format=csv,noheader"])
            .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "tidak terbaca (bukan NVIDIA atau nvidia-smi tidak ada)".into())
    }
}

#[cfg(windows)]
pub fn collect() -> Report {
    Report {
        smbios: native::smbios(),
        uefi: native::uefi(),
        secure_boot: native::secure_boot(),
        hvci: native::hvci(),
        rebar: native::rebar(),
        gpu: native::gpu(),
    }
}

/// Data tiruan untuk pengembangan di Linux: PC dengan XMP dan ReBAR masih mati.
#[cfg(not(windows))]
pub fn collect() -> Report {
    let dimm = |slot: &str| Dimm {
        slot: slot.into(),
        size_mb: 16384,
        mem_type: 0x1A,
        rated: 3600,
        configured: 2133,
        maker: "G Skill Intl".into(),
        part: "F4-3600C16-16GVKC".into(),
    };
    Report {
        smbios: Smbios {
            bios_vendor: "American Megatrends International, LLC.".into(),
            bios_version: "2801".into(),
            bios_date: "03/14/2023".into(),
            system_maker: "ASUS".into(),
            system_product: "System Product Name".into(),
            board_maker: "ASUSTeK COMPUTER INC.".into(),
            board_product: "TUF GAMING B550-PLUS".into(),
            cpu: "AMD Ryzen 7 5800X 8-Core Processor".into(),
            dimms: vec![dimm("DIMM_A2"), dimm("DIMM_B2")],
        },
        uefi: Some(true),
        secure_boot: Some(true),
        hvci: Some(true),
        rebar: Rebar::Off { bar_mb: 256, vram_mb: 8192 },
        gpu: "NVIDIA GeForce RTX 3070 Ti".into(),
    }
}

/// Tahun sekarang (UTC) dari jam sistem.
pub fn this_year() -> u32 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Cukup akurat untuk membandingkan umur BIOS dalam tahun.
    1970 + (secs / 31_556_952) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(ty: u8, formatted: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut v = vec![ty, (formatted.len() + 4) as u8, 0, 0];
        v.extend_from_slice(formatted);
        if strings.is_empty() {
            v.extend_from_slice(&[0, 0]);
        } else {
            for s in strings {
                v.extend_from_slice(s.as_bytes());
                v.push(0);
            }
            v.push(0);
        }
        v
    }

    fn sample() -> Vec<u8> {
        let mut t = Vec::new();
        // Tipe 0: vendor=1 versi=2 tanggal=3 (offset 0x04, 0x05, 0x08)
        let mut f0 = vec![0u8; 0x14];
        f0[0x00] = 1;
        f0[0x01] = 2;
        f0[0x04] = 3;
        t.extend(structure(0, &f0, &["American Megatrends Inc.", "F15", "05/12/2021"]));
        let mut f2 = vec![0u8; 0x0B];
        f2[0x00] = 1;
        f2[0x01] = 2;
        t.extend(structure(2, &f2, &["Gigabyte Technology Co., Ltd.", "B560M AORUS ELITE"]));
        let mut f4 = vec![0u8; 0x2C];
        f4[0x10 - 4] = 1;
        t.extend(structure(4, &f4, &["11th Gen Intel(R) Core(TM) i5-11400F @ 2.60GHz"]));
        // Tipe 17 dengan panjang 0x28.
        let mut f17 = vec![0u8; 0x28 - 4];
        f17[0x0C - 4..0x0E - 4].copy_from_slice(&8192u16.to_le_bytes());
        f17[0x10 - 4] = 1;
        f17[0x12 - 4] = 0x1A;
        f17[0x15 - 4..0x17 - 4].copy_from_slice(&3200u16.to_le_bytes());
        f17[0x17 - 4] = 2;
        f17[0x1A - 4] = 3;
        f17[0x20 - 4..0x22 - 4].copy_from_slice(&2666u16.to_le_bytes());
        t.extend(structure(17, &f17, &["DIMM 1", "Kingston", "KF432C16BB/8"]));
        // Slot kosong
        let f17e = vec![0u8; 0x28 - 4];
        t.extend(structure(17, &f17e, &[]));
        t.extend(structure(127, &[], &[]));
        t
    }

    #[test]
    fn parses_smbios() {
        let s = parse_smbios(&sample());
        assert_eq!(s.bios_vendor, "American Megatrends Inc.");
        assert_eq!(s.bios_version, "F15");
        assert_eq!(s.bios_date, "05/12/2021");
        assert_eq!(s.board_maker, "Gigabyte Technology Co., Ltd.");
        assert_eq!(s.board_product, "B560M AORUS ELITE");
        assert!(s.cpu.contains("i5-11400F"));
        assert_eq!(s.dimms.len(), 1);
        let d = &s.dimms[0];
        assert_eq!((d.size_mb, d.rated, d.configured), (8192, 3200, 2666));
        assert_eq!(d.part, "KF432C16BB/8");
        assert_eq!(d.type_name(), "DDR4");
    }

    #[test]
    fn parses_nvidia_smi() {
        let off = "==============NVSMI LOG==============\n\nAttached GPUs : 1\nGPU 00000000:01:00.0\n    FB Memory Usage\n        Total                             : 8192 MiB\n        Reserved                          : 195 MiB\n        Used                              : 900 MiB\n    BAR1 Memory Usage\n        Total                             : 256 MiB\n        Used                              : 3 MiB\n    Conf Compute Protected Memory Usage\n        Total                             : 0 MiB\n";
        assert_eq!(parse_nvidia_smi(off), Rebar::Off { bar_mb: 256, vram_mb: 8192 });
        let on = off.replace("256 MiB", "8192 MiB");
        assert_eq!(parse_nvidia_smi(&on), Rebar::On { bar_mb: 8192, vram_mb: 8192 });
        assert_eq!(parse_nvidia_smi("garbage"), Rebar::Unknown);
    }

    #[test]
    fn advice_flags_slow_ram_and_rebar() {
        let r = collect_sim();
        let tips = advice(&r, 2026);
        assert_eq!(tips[0].level, Level::Penting);
        assert!(tips[0].title.contains("XMP"));
        assert!(tips[0].menu.as_deref().unwrap().contains("Ai Overclock Tuner"));
        assert!(tips.iter().any(|a| a.title.contains("Resizable BAR belum aktif")));
        assert!(tips.iter().any(|a| a.title.contains("tahun 2021")));
        assert!(!tips.iter().any(|a| a.title.contains("Precision Boost")));
        let text = report_text(&r, &tips);
        assert!(text.contains("B560M AORUS ELITE"));
    }

    #[test]
    fn advice_ok_when_tuned() {
        let mut r = collect_sim();
        for d in &mut r.smbios.dimms {
            d.configured = 3200;
        }
        r.rebar = Rebar::On { bar_mb: 8192, vram_mb: 8192 };
        r.smbios.bios_date = "01/01/2026".into();
        let tips = advice(&r, 2026);
        assert!(tips.iter().all(|a| a.level >= Level::Opsional));
    }

    #[test]
    fn jedec_speed_hints_xmp_off() {
        let mut r = collect_sim();
        for d in &mut r.smbios.dimms {
            d.rated = 2133;
            d.configured = 2133;
        }
        let tips = advice(&r, 2026);
        assert_eq!(tips[0].level, Level::Saran);
        assert!(tips.iter().any(|a| a.title.contains("Kemungkinan XMP")));
    }

    #[test]
    fn legacy_boot_warns_about_gpt() {
        let mut r = collect_sim();
        r.uefi = Some(false);
        let tips = advice(&r, 2026);
        assert!(tips.iter().any(|a| a.body.contains("mbr2gpt")));
    }

    fn collect_sim() -> Report {
        let mut s = parse_smbios(&sample());
        s.board_maker = "ASUSTeK COMPUTER INC.".into();
        s.dimms.push(s.dimms[0].clone());
        Report { smbios: s, uefi: Some(true), secure_boot: Some(true), hvci: Some(false), rebar: Rebar::Off { bar_mb: 256, vram_mb: 8192 }, gpu: "RTX 3070 Ti".into() }
    }
}
