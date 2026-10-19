//! Sensor CPU: clock efektif (PDH, tanpa driver), serta suhu dan daya paket lewat PawnIO.
//!
//! Suhu dan daya CPU hanya bisa dibaca dari register MSR/SMN, yang butuh driver kernel. Pacu
//! tidak membawa driver sendiri: ia memakai PawnIO (driver bertanda tangan yang juga dipakai
//! LibreHardwareMonitor dan FanControl) bila sudah terpasang. RTSS 7.3.7 sendiri memakai
//! LibreHardwareMonitor dengan driver WinRing0, yang kini diblokir Windows Defender.

#[derive(Clone, Debug, Default)]
pub struct CpuReading {
    pub mhz: Option<f32>,
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
}

/// Mengubah nilai register suhu Intel (IA32_PACKAGE_THERM_STATUS) menjadi °C.
pub fn intel_temp(therm_status: u64, temp_target: u64) -> Option<f32> {
    if therm_status & 0x8000_0000 == 0 {
        return None;
    }
    let tjmax = ((temp_target >> 16) & 0xFF) as f32;
    let tjmax = if (60.0..=130.0).contains(&tjmax) { tjmax } else { 100.0 };
    Some(tjmax - ((therm_status >> 16) & 0x7F) as f32)
}

/// Mengubah nilai SMN THM_TCON_CUR_TMP (AMD Zen) menjadi °C (Tctl).
pub fn amd_temp(raw: u32) -> f32 {
    let offset = raw & 0x80000 != 0 || raw & 0x30000 == 0x30000;
    let t = (raw >> 21) as f32 * 0.125;
    if offset {
        t - 49.0
    } else {
        t
    }
}

/// Daya dari dua bacaan penghitung energi 32-bit. `unit_bits` dari bit 12:8 register unit.
pub fn rapl_watts(prev: u32, now: u32, unit_bits: u32, secs: f32) -> Option<f32> {
    if secs <= 0.0 {
        return None;
    }
    let delta = now.wrapping_sub(prev) as f32;
    let joules = delta / (1u64 << unit_bits) as f32;
    let w = joules / secs;
    (0.0..1000.0).contains(&w).then_some(w)
}

#[cfg(windows)]
pub use imp::CpuSensors;

#[cfg(not(windows))]
pub struct CpuSensors {
    t: f32,
}

#[cfg(not(windows))]
impl CpuSensors {
    pub fn new() -> Self {
        CpuSensors { t: 0.0 }
    }
    pub fn name(&self) -> String {
        "CPU simulasi".into()
    }
    /// Keterangan sumber suhu CPU untuk ditampilkan.
    pub fn note(&self) -> Option<String> {
        None
    }
    pub fn pawnio_missing(&self) -> bool {
        false
    }
    pub fn read(&mut self, load_pct: f32) -> CpuReading {
        self.t += 0.1;
        CpuReading {
            mhz: Some(4500.0 + (self.t.sin() * 150.0)),
            temp_c: Some(45.0 + load_pct * 0.3 + self.t.cos() * 2.0),
            power_w: Some(25.0 + load_pct * 0.9),
        }
    }
}

#[cfg(not(windows))]
impl Default for CpuSensors {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::time::Instant;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER,
        PDH_HQUERY,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    // Modul PawnIO bertanda tangan hanya disertakan bila build memakai fitur "pawnio"
    // (file res/pawnio/*.bin), lihat README.
    #[cfg(feature = "pawnio")]
    const INTEL_MSR: &[u8] = include_bytes!("../../res/pawnio/IntelMSR.bin");
    #[cfg(feature = "pawnio")]
    const AMD_17: &[u8] = include_bytes!("../../res/pawnio/AMDFamily17.bin");
    #[cfg(not(feature = "pawnio"))]
    const INTEL_MSR: &[u8] = &[];
    #[cfg(not(feature = "pawnio"))]
    const AMD_17: &[u8] = &[];
    const IOCTL_LOAD: u32 = 0xA1B2_0000 | (0x821 << 2);
    const IOCTL_EXEC: u32 = 0xA1B2_0000 | (0x841 << 2);

    /// Satu modul PawnIO yang sudah dimuat.
    struct Pawn(HANDLE);

    unsafe impl Send for Pawn {}

    impl Pawn {
        fn open(blob: &[u8]) -> Result<Pawn, String> {
            if blob.is_empty() {
                return Err("modul sensor CPU (PawnIO) belum disertakan di versi ini".into());
            }
            unsafe {
                let h = CreateFileW(
                    w!("\\\\?\\GLOBALROOT\\Device\\PawnIO"),
                    (GENERIC_READ | GENERIC_WRITE).0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
                .map_err(|_| "PawnIO belum terpasang".to_string())?;
                if DeviceIoControl(h, IOCTL_LOAD, Some(blob.as_ptr() as _), blob.len() as u32, None, 0, None, None).is_err() {
                    let _ = CloseHandle(h);
                    return Err("PawnIO menolak modul".into());
                }
                Ok(Pawn(h))
            }
        }

        fn exec(&self, name: &str, args: &[u64]) -> Option<u64> {
            let mut input = vec![0u8; 32 + args.len() * 8];
            input[..name.len().min(31)].copy_from_slice(&name.as_bytes()[..name.len().min(31)]);
            for (i, a) in args.iter().enumerate() {
                input[32 + i * 8..40 + i * 8].copy_from_slice(&a.to_le_bytes());
            }
            let mut out = [0u8; 8];
            let mut got = 0u32;
            unsafe {
                DeviceIoControl(
                    self.0,
                    IOCTL_EXEC,
                    Some(input.as_ptr() as _),
                    input.len() as u32,
                    Some(out.as_mut_ptr() as _),
                    8,
                    Some(&mut got),
                    None,
                )
                .ok()?;
            }
            (got >= 8).then(|| u64::from_le_bytes(out))
        }

        fn msr(&self, index: u32) -> Option<u64> {
            self.exec("ioctl_read_msr", &[index as u64])
        }
    }

    impl Drop for Pawn {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Vendor {
        Intel,
        Amd,
        Other,
    }

    struct Pdh {
        query: PDH_HQUERY,
        perf: PDH_HCOUNTER,
        freq: PDH_HCOUNTER,
    }

    impl Pdh {
        fn open() -> Option<Pdh> {
            unsafe {
                let mut query = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                    return None;
                }
                let mut perf = PDH_HCOUNTER::default();
                let mut freq = PDH_HCOUNTER::default();
                if PdhAddEnglishCounterW(query, w!("\\Processor Information(_Total)\\% Processor Performance"), 0, &mut perf) != 0
                    || PdhAddEnglishCounterW(query, w!("\\Processor Information(_Total)\\Processor Frequency"), 0, &mut freq) != 0
                {
                    return None;
                }
                PdhCollectQueryData(query);
                Some(Pdh { query, perf, freq })
            }
        }

        fn mhz(&self) -> Option<f32> {
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let get = |c: PDH_HCOUNTER| {
                    let mut v = PDH_FMT_COUNTERVALUE::default();
                    (PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) == 0).then(|| v.Anonymous.doubleValue)
                };
                let perf = get(self.perf)?;
                let base = get(self.freq)?;
                let mhz = (base * perf / 100.0) as f32;
                (mhz > 100.0).then_some(mhz)
            }
        }
    }

    unsafe impl Send for Pdh {}

    pub struct CpuSensors {
        name: String,
        vendor: Vendor,
        pawn: Result<Pawn, String>,
        pdh: Option<Pdh>,
        unit_bits: Option<u32>,
        last_energy: Option<(u32, Instant)>,
        tj_target: u64,
    }

    fn cpuid_info() -> (String, Vendor, u32) {
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::x86_64::__cpuid;
            let v = __cpuid(0);
            let mut vendor = Vec::new();
            for r in [v.ebx, v.edx, v.ecx] {
                vendor.extend_from_slice(&r.to_le_bytes());
            }
            let vendor = match &vendor[..] {
                b"GenuineIntel" => Vendor::Intel,
                b"AuthenticAMD" => Vendor::Amd,
                _ => Vendor::Other,
            };
            let f = __cpuid(1).eax;
            let family = ((f >> 8) & 0xF) + if (f >> 8) & 0xF == 0xF { (f >> 20) & 0xFF } else { 0 };
            let mut name = Vec::new();
            for leaf in 0x8000_0002u32..=0x8000_0004 {
                let r = __cpuid(leaf);
                for x in [r.eax, r.ebx, r.ecx, r.edx] {
                    name.extend_from_slice(&x.to_le_bytes());
                }
            }
            let name = String::from_utf8_lossy(&name).trim_matches(char::from(0)).trim().to_string();
            (name, vendor, family)
        }
        #[cfg(not(target_arch = "x86_64"))]
        ("CPU".into(), Vendor::Other, 0)
    }

    impl CpuSensors {
        pub fn new() -> Self {
            let (name, vendor, family) = cpuid_info();
            let pawn = match vendor {
                Vendor::Intel => Pawn::open(INTEL_MSR),
                Vendor::Amd if family >= 0x17 => Pawn::open(AMD_17),
                Vendor::Amd => Err("CPU AMD sebelum Ryzen belum didukung".into()),
                Vendor::Other => Err("Merek CPU tidak dikenal".into()),
            };
            let (unit_bits, tj_target) = match &pawn {
                Ok(p) => {
                    let unit_msr = if vendor == Vendor::Intel { 0x606 } else { 0xC001_0299 };
                    let unit = p.msr(unit_msr).map(|u| ((u >> 8) & 0x1F) as u32);
                    let tj = if vendor == Vendor::Intel { p.msr(0x1A2).unwrap_or(0) } else { 0 };
                    (unit, tj)
                }
                Err(_) => (None, 0),
            };
            CpuSensors { name, vendor, pawn, pdh: Pdh::open(), unit_bits, last_energy: None, tj_target }
        }

        pub fn name(&self) -> String {
            self.name.clone()
        }

        pub fn note(&self) -> Option<String> {
            self.pawn.as_ref().err().cloned()
        }

        pub fn pawnio_missing(&self) -> bool {
            matches!(&self.pawn, Err(e) if e.starts_with("PawnIO belum"))
        }

        pub fn read(&mut self, _load_pct: f32) -> CpuReading {
            let mut r = CpuReading { mhz: self.pdh.as_ref().and_then(|p| p.mhz()), ..Default::default() };
            let Ok(p) = &self.pawn else { return r };
            match self.vendor {
                Vendor::Intel => {
                    r.temp_c = p.msr(0x1B1).and_then(|v| intel_temp(v, self.tj_target));
                }
                Vendor::Amd => {
                    r.temp_c = p.exec("ioctl_read_smn", &[0x0005_9800]).map(|v| amd_temp(v as u32));
                }
                Vendor::Other => {}
            }
            let energy_msr = if self.vendor == Vendor::Intel { 0x611 } else { 0xC001_029B };
            if let (Some(bits), Some(e)) = (self.unit_bits, p.msr(energy_msr)) {
                let now = (e as u32, Instant::now());
                if let Some((prev, t)) = self.last_energy {
                    r.power_w = rapl_watts(prev, now.0, bits, t.elapsed().as_secs_f32());
                }
                self.last_energy = Some(now);
            }
            r
        }
    }

    impl Default for CpuSensors {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_package_temp() {
        // TjMax 100, readout 38 => 62 °C.
        assert_eq!(intel_temp(0x8026_0000, 100 << 16), Some(62.0));
        assert_eq!(intel_temp(0x0026_0000, 100 << 16), None);
        // TjMax tidak masuk akal => anggap 100.
        assert_eq!(intel_temp(0x8000_0000 | (10 << 16), 0), Some(90.0));
    }

    #[test]
    fn amd_tctl() {
        // 60 °C = 480 * 0.125.
        assert_eq!(amd_temp(480 << 21), 60.0);
        assert_eq!(amd_temp((480 << 21) | 0x80000), 11.0);
    }

    #[test]
    fn rapl_wraps() {
        // Unit 14 bit: 1 J = 16384.
        let w = rapl_watts(u32::MAX - 16383, 16384, 14, 1.0).unwrap();
        assert!((w - 2.0).abs() < 0.01);
    }
}
