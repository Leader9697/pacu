//! Membaca log System Windows untuk tanda driver GPU bermasalah:
//! - Display 4101: TDR, driver tampilan berhenti merespons lalu dipulihkan.
//! - nvlddmkm (driver NVIDIA): error seperti 13 (Graphics Exception), 14, 153 (GPU di-reset).
//! - amdkmdag (driver AMD): error apa pun dari driver kernel AMD.

use crate::volt::rig::Watchdog;

#[cfg(windows)]
mod imp {
    use windows::core::PCWSTR;
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender, EvtRenderEventXml, EVT_HANDLE,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn render(ev: EVT_HANDLE) -> Option<String> {
        unsafe {
            let mut used = 0u32;
            let mut props = 0u32;
            let _ = EvtRender(None, ev, EvtRenderEventXml.0, 0, None, &mut used, &mut props);
            if used == 0 {
                return None;
            }
            let mut buf = vec![0u16; used as usize / 2 + 1];
            EvtRender(None, ev, EvtRenderEventXml.0, (buf.len() * 2) as u32, Some(buf.as_mut_ptr() as _), &mut used, &mut props).ok()?;
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }

    /// Menjalankan kueri XPath di log System, mengembalikan XML tiap event (maksimal `max`).
    pub fn query(xpath: &str, reverse: bool, max: usize) -> Vec<String> {
        let mut out = Vec::new();
        unsafe {
            let ch = wide("System");
            let q = wide(xpath);
            let flags = EvtQueryChannelPath.0 | if reverse { EvtQueryReverseDirection.0 } else { 0 };
            let Ok(rs) = EvtQuery(None, PCWSTR(ch.as_ptr()), PCWSTR(q.as_ptr()), flags) else { return out };
            loop {
                let mut evs = [0isize; 16];
                let mut n = 0u32;
                if EvtNext(rs, &mut evs, 500, 0, &mut n).is_err() || n == 0 {
                    break;
                }
                for &e in &evs[..n as usize] {
                    let h = EVT_HANDLE(e);
                    if out.len() < max {
                        if let Some(x) = render(h) {
                            out.push(x);
                        }
                    }
                    let _ = EvtClose(h);
                }
                if out.len() >= max {
                    break;
                }
            }
            let _ = EvtClose(rs);
        }
        out
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn tag<'a>(xml: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let i = xml.find(open)? + open.len();
    let j = xml[i..].find(close)? + i;
    Some(&xml[i..j])
}

#[cfg_attr(not(windows), allow(dead_code))]
fn record_id(xml: &str) -> Option<u64> {
    tag(xml, "<EventRecordID>", "</EventRecordID>")?.trim().parse().ok()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn describe(xml: &str) -> String {
    let provider = tag(xml, "Provider Name='", "'").or_else(|| tag(xml, "Provider Name=\"", "\"")).unwrap_or("?");
    let id = tag(xml, "<EventID", "</EventID>").and_then(|s| s.rsplit('>').next()).unwrap_or("?").trim().to_string();
    let what = match (provider, id.as_str()) {
        ("Display", "4101") => "driver tampilan berhenti merespons lalu dipulihkan (TDR)",
        ("nvlddmkm", "13") => "Graphics Exception (unit shader salah hitung)",
        ("nvlddmkm", "14") => "Graphics Exception",
        ("nvlddmkm", "153") => "GPU di-reset karena error",
        ("nvlddmkm", "0") => "error driver NVIDIA",
        ("amdkmdag", _) => "error driver AMD",
        _ => "error driver",
    };
    format!("{provider} {id}: {what}")
}

pub struct EventLogWatchdog {
    mark: u64,
}

impl EventLogWatchdog {
    pub fn new() -> Self {
        let mut w = Self { mark: 0 };
        w.mark();
        w
    }
}

impl Default for EventLogWatchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl Watchdog for EventLogWatchdog {
    fn mark(&mut self) {
        #[cfg(windows)]
        {
            if let Some(id) = imp::query("*", true, 1).first().and_then(|x| record_id(x)) {
                self.mark = id;
            }
        }
    }

    fn events_since_mark(&mut self) -> Vec<String> {
        #[cfg(windows)]
        {
            let xp = format!(
                "*[System[(Provider[@Name='nvlddmkm'] or Provider[@Name='amdkmdag'] or Provider[@Name='Display']) and (Level=1 or Level=2 or Level=3) and (EventRecordID > {})]]",
                self.mark
            );
            imp::query(&xp, false, 8).iter().map(|x| describe(x)).collect()
        }
        #[cfg(not(windows))]
        {
            let _ = self.mark;
            Vec::new()
        }
    }
}

/// Riwayat TDR/error driver terakhir, untuk laporan diagnostik.
pub fn recent_driver_events(max: usize) -> Vec<String> {
    #[cfg(windows)]
    {
        imp::query("*[System[(Provider[@Name='nvlddmkm'] or Provider[@Name='amdkmdag'] or Provider[@Name='Display']) and (Level=1 or Level=2 or Level=3)]]", true, max)
            .iter()
            .map(|x| {
                let t = tag(x, "SystemTime='", "'").or_else(|| tag(x, "SystemTime=\"", "\"")).unwrap_or("").chars().take(19).collect::<String>();
                format!("{t} {}", describe(x))
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = max;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_event_xml() {
        let x = "<Event><System><Provider Name='Display'/><EventID Qualifiers='0'>4101</EventID><EventRecordID>9876</EventRecordID></System></Event>";
        assert_eq!(super::record_id(x), Some(9876));
        assert!(super::describe(x).starts_with("Display 4101: driver tampilan"));
    }
}
