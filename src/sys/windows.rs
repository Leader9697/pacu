//! Implementasi Windows. Fungsi API dideklarasikan langsung supaya ringan dan tidak bergantung
//! pada crate binding besar.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use super::{Foreground, MemInfo, PowerPlan, RawProc, Sys, Topology};
use crate::config::{GpuPriority, IoPriority, Priority};
use std::ffi::c_void;

type HANDLE = *mut c_void;
type HWND = *mut c_void;
type BOOL = i32;
type NTSTATUS = i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct GUID {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

#[repr(C)]
#[derive(Default)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct MONITORINFO {
    cb_size: u32,
    rc_monitor: RECT,
    rc_work: RECT,
    flags: u32,
}

#[repr(C)]
#[derive(Default)]
struct LASTINPUTINFO {
    cb_size: u32,
    dw_time: u32,
}

#[repr(C)]
#[derive(Default)]
struct MEMORYSTATUSEX {
    dw_length: u32,
    dw_memory_load: u32,
    ull_total_phys: u64,
    ull_avail_phys: u64,
    ull_total_page_file: u64,
    ull_avail_page_file: u64,
    ull_total_virtual: u64,
    ull_avail_virtual: u64,
    ull_avail_extended_virtual: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FILETIME {
    low: u32,
    high: u32,
}

impl FILETIME {
    fn get(self) -> u64 {
        ((self.high as u64) << 32) | self.low as u64
    }
}

#[repr(C)]
struct LUID {
    low: u32,
    high: i32,
}

#[repr(C)]
struct TOKEN_PRIVILEGES {
    count: u32,
    luid: LUID,
    attributes: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: BOOL, pid: u32) -> HANDLE;
    fn CloseHandle(h: HANDLE) -> BOOL;
    fn GetCurrentProcess() -> HANDLE;
    fn GetCurrentProcessId() -> u32;
    fn GetPriorityClass(h: HANDLE) -> u32;
    fn SetPriorityClass(h: HANDLE, class: u32) -> BOOL;
    fn SetProcessAffinityMask(h: HANDLE, mask: usize) -> BOOL;
    fn TerminateProcess(h: HANDLE, code: u32) -> BOOL;
    fn QueryFullProcessImageNameW(h: HANDLE, flags: u32, buf: *mut u16, size: *mut u32) -> BOOL;
    fn GetSystemTimes(idle: *mut FILETIME, kernel: *mut FILETIME, user: *mut FILETIME) -> BOOL;
    fn GetActiveProcessorCount(group: u16) -> u32;
    fn GetSystemCpuSetInformation(info: *mut u8, len: u32, ret: *mut u32, process: HANDLE, flags: u32) -> BOOL;
    fn SetProcessDefaultCpuSets(h: HANDLE, ids: *const u32, count: u32) -> BOOL;
    fn SetProcessInformation(h: HANDLE, class: i32, info: *const c_void, size: u32) -> BOOL;
    fn GlobalMemoryStatusEx(m: *mut MEMORYSTATUSEX) -> BOOL;
    fn GetTickCount() -> u32;
    fn SetThreadExecutionState(flags: u32) -> u32;
    fn LocalFree(p: *mut c_void) -> *mut c_void;
    fn GetLocalTime(t: *mut [u16; 8]);
}

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> HWND;
    fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
    fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    fn MonitorFromWindow(hwnd: HWND, flags: u32) -> *mut c_void;
    fn GetMonitorInfoW(mon: *mut c_void, info: *mut MONITORINFO) -> BOOL;
    fn GetShellWindow() -> HWND;
    fn GetDesktopWindow() -> HWND;
    fn GetLastInputInfo(info: *mut LASTINPUTINFO) -> BOOL;
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(h: HANDLE, access: u32, token: *mut HANDLE) -> BOOL;
    fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut LUID) -> BOOL;
    fn AdjustTokenPrivileges(
        token: HANDLE,
        disable_all: BOOL,
        new: *const TOKEN_PRIVILEGES,
        len: u32,
        prev: *mut c_void,
        ret: *mut u32,
    ) -> BOOL;
    fn RegOpenKeyExW(key: isize, sub: *const u16, opts: u32, sam: u32, out: *mut isize) -> i32;
    fn RegQueryValueExW(key: isize, name: *const u16, res: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> NTSTATUS;
    fn NtSetSystemInformation(class: u32, info: *const c_void, len: u32) -> NTSTATUS;
    fn NtSetInformationProcess(h: HANDLE, class: u32, info: *const c_void, len: u32) -> NTSTATUS;
    fn NtSetTimerResolution(desired: u32, set: u8, current: *mut u32) -> NTSTATUS;
    fn NtSuspendProcess(h: HANDLE) -> NTSTATUS;
    fn NtResumeProcess(h: HANDLE) -> NTSTATUS;
}

#[link(name = "gdi32")]
extern "system" {
    fn D3DKMTSetProcessSchedulingPriorityClass(h: HANDLE, class: i32) -> NTSTATUS;
}

#[link(name = "powrprof")]
extern "system" {
    fn PowerGetActiveScheme(root: *mut c_void, guid: *mut *mut GUID) -> u32;
    fn PowerSetActiveScheme(root: *mut c_void, guid: *const GUID) -> u32;
    fn PowerEnumerate(
        root: *mut c_void,
        scheme: *const GUID,
        sub: *const GUID,
        flags: u32,
        index: u32,
        buf: *mut u8,
        size: *mut u32,
    ) -> u32;
    fn PowerDuplicateScheme(root: *mut c_void, src: *const GUID, dst: *mut *mut GUID) -> u32;
    fn PowerWriteFriendlyName(root: *mut c_void, scheme: *const GUID, sub: *const GUID, setting: *const GUID, buf: *const u8, size: u32) -> u32;
    fn PowerWriteDescription(root: *mut c_void, scheme: *const GUID, sub: *const GUID, setting: *const GUID, buf: *const u8, size: u32) -> u32;
    fn PowerWriteACValueIndex(root: *mut c_void, scheme: *const GUID, sub: *const GUID, setting: *const GUID, value: u32) -> u32;
    fn PowerWriteDCValueIndex(root: *mut c_void, scheme: *const GUID, sub: *const GUID, setting: *const GUID, value: u32) -> u32;
    fn PowerReadFriendlyName(
        root: *mut c_void,
        scheme: *const GUID,
        sub: *const GUID,
        setting: *const GUID,
        buf: *mut u8,
        size: *mut u32,
    ) -> u32;
}

const PROCESS_TERMINATE: u32 = 0x0001;
const PROCESS_SUSPEND_RESUME: u32 = 0x0800;

/// (subgrup, setelan, nilai) untuk power plan "Pacu Performa". Nilai sama untuk AC dan baterai.
const PERFORMANCE_SETTINGS: &[(&str, &str, u32)] = &[
    // Prosesor: core parking mati (min. core 100%), state minimum dan maksimum 100%,
    // boost agresif, preferensi energi = performa penuh.
    ("54533251-82be-4824-96c1-47b60b740d00", "0cc5b647-c1df-4637-891a-dec35c318583", 100),
    ("54533251-82be-4824-96c1-47b60b740d00", "ea062031-0e34-4ff1-9b6d-eb1059334028", 100),
    ("54533251-82be-4824-96c1-47b60b740d00", "893dee8e-2bef-41e0-89c6-b55d0929964c", 100),
    ("54533251-82be-4824-96c1-47b60b740d00", "bc5038f7-23e0-4960-96da-33abaf5935ec", 100),
    ("54533251-82be-4824-96c1-47b60b740d00", "be337238-0d82-4146-a960-4f3749d470c7", 2),
    ("54533251-82be-4824-96c1-47b60b740d00", "36687f9e-e3a5-4dbf-b1dc-15eb381c6863", 0),
    // PCI Express: Link State Power Management mati.
    ("501a4d13-42af-4429-9fd1-a8218c268e20", "ee12f906-d277-404b-b6da-e5fa1a576df5", 0),
    // USB selective suspend mati.
    ("2a737441-1930-4402-8d77-b2bebba308a3", "48e6b7a6-50f5-4782-a5d4-53bb8f07e226", 0),
    // Disk tidak pernah dimatikan.
    ("0012ee47-9041-4b5d-9b77-535fba8b1442", "6738e2c4-e8a5-4a42-b16a-e040e769756e", 0),
    // Prosesor: CPU tidak boleh masuk throttle state, cek performa tiap 200 ms (lebih sedikit DPC).
    ("54533251-82be-4824-96c1-47b60b740d00", "3b04d4fd-1cc7-4f23-ab1c-d1337819c4bb", 0),
    ("54533251-82be-4824-96c1-47b60b740d00", "4d2b0152-7d5c-498b-88e2-34345392a2c5", 200),
    // NVMe tidak pernah idle (timeout primer/sekunder 0, NOPPME mati). Ide dari AtlasOS.
    ("0012ee47-9041-4b5d-9b77-535fba8b1442", "d639518a-e56d-4345-8af2-b9f32fb26109", 0),
    ("0012ee47-9041-4b5d-9b77-535fba8b1442", "d3d55efd-c1ff-424e-9dc3-441be7833010", 0),
    ("0012ee47-9041-4b5d-9b77-535fba8b1442", "fc7372b6-ab2d-43ee-8797-15e9841f2cca", 0),
    // USB hub tidak di-suspend dan USB 3 link power management mati.
    ("2a737441-1930-4402-8d77-b2bebba308a3", "0853a681-27c8-4100-a2fd-82013e970683", 0),
    ("2a737441-1930-4402-8d77-b2bebba308a3", "d4e98f31-5ffe-4ce1-be31-1b38b384c009", 0),
    // Sleep dan hibernasi otomatis mati.
    ("238c9fa8-0aad-41ed-83f4-97be242c8f20", "29f6c1db-86da-48c5-9fdb-f2b67b1f44da", 0),
    ("238c9fa8-0aad-41ed-83f4-97be242c8f20", "9d7815a6-7ee4-497e-8888-515a05f02364", 0),
];
const PROCESS_SET_INFORMATION: u32 = 0x0200;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const PROCESS_SET_LIMITED_INFORMATION: u32 = 0x2000;
const STATUS_INFO_LENGTH_MISMATCH: NTSTATUS = 0xC000_0004u32 as i32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(HANDLE);

impl Handle {
    fn open(pid: u32, access: u32) -> Option<Handle> {
        let h = unsafe { OpenProcess(access, 0, pid) };
        if h.is_null() {
            None
        } else {
            Some(Handle(h))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn guid_to_string(g: &GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.d1, g.d2, g.d3, g.d4[0], g.d4[1], g.d4[2], g.d4[3], g.d4[4], g.d4[5], g.d4[6], g.d4[7]
    )
}

fn guid_from_string(s: &str) -> Option<GUID> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if h.len() != 32 {
        return None;
    }
    let b = |i: usize, n: usize| u64::from_str_radix(&h[i..i + n], 16).ok();
    let mut d4 = [0u8; 8];
    for (k, v) in d4.iter_mut().enumerate() {
        *v = b(16 + k * 2, 2)? as u8;
    }
    Some(GUID { d1: b(0, 8)? as u32, d2: b(8, 4)? as u16, d3: b(12, 4)? as u16, d4 })
}

fn enable_privilege(name: &str) -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), 0x0020 | 0x0008, &mut token) == 0 {
            return false;
        }
        let mut luid = LUID { low: 0, high: 0 };
        let ok = LookupPrivilegeValueW(std::ptr::null(), wide(name).as_ptr(), &mut luid) != 0
            && {
                let tp = TOKEN_PRIVILEGES { count: 1, luid, attributes: 0x2 };
                AdjustTokenPrivileges(token, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0
            };
        CloseHandle(token);
        ok
    }
}

fn read_cpu_name() -> String {
    unsafe {
        let mut key: isize = 0;
        let sub = wide("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
        // HKEY_LOCAL_MACHINE = 0x80000002, KEY_READ = 0x20019
        if RegOpenKeyExW(0x8000_0002u32 as i32 as isize, sub.as_ptr(), 0, 0x20019, &mut key) != 0 {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        let name = wide("ProcessorNameString");
        let r = RegQueryValueExW(key, name.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), buf.as_mut_ptr() as *mut u8, &mut len);
        RegCloseKey(key);
        if r != 0 {
            return String::new();
        }
        let n = (len as usize / 2).min(buf.len());
        String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').trim().to_string()
    }
}

pub struct WinSys {
    buf: Vec<u8>,
    self_pid: u32,
    topology: Topology,
    timer_on: bool,
    sleep_blocked: bool,
}

impl WinSys {
    pub fn new() -> WinSys {
        for p in [
            "SeDebugPrivilege",
            "SeIncreaseBasePriorityPrivilege",
            "SeProfileSingleProcessPrivilege",
            "SeIncreaseQuotaPrivilege",
        ] {
            enable_privilege(p);
        }
        let topology = Self::read_topology();
        WinSys {
            buf: vec![0; 1 << 20],
            self_pid: unsafe { GetCurrentProcessId() },
            topology,
            timer_on: false,
            sleep_blocked: false,
        }
    }

    fn read_topology() -> Topology {
        let logical = unsafe { GetActiveProcessorCount(0xFFFF) };
        let mut sets: Vec<(u32, u8)> = Vec::new();
        unsafe {
            let mut len = 0u32;
            GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut len, std::ptr::null_mut(), 0);
            if len > 0 {
                let mut buf = vec![0u8; len as usize];
                if GetSystemCpuSetInformation(buf.as_mut_ptr(), len, &mut len, std::ptr::null_mut(), 0) != 0 {
                    let mut off = 0usize;
                    while off + 24 <= len as usize {
                        let size = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
                        let id = u32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap());
                        let class = buf[off + 18];
                        sets.push((id, class));
                        if size == 0 {
                            break;
                        }
                        off += size;
                    }
                }
            }
        }
        let max_class = sets.iter().map(|s| s.1).max().unwrap_or(0);
        let (mut pcore_sets, mut ecore_sets) = (Vec::new(), Vec::new());
        if max_class > 0 {
            for (id, class) in sets {
                if class == max_class {
                    pcore_sets.push(id);
                } else {
                    ecore_sets.push(id);
                }
            }
        }
        Topology { logical, pcore_sets, ecore_sets, cpu_name: read_cpu_name() }
    }

    fn priority_class(p: Priority) -> u32 {
        match p {
            Priority::Idle => 0x40,
            Priority::BelowNormal => 0x4000,
            Priority::Normal => 0x20,
            Priority::AboveNormal => 0x8000,
            Priority::High => 0x80,
            Priority::Realtime => 0x100,
        }
    }
}

impl Default for WinSys {
    fn default() -> Self {
        Self::new()
    }
}

fn rd<T: Copy>(b: &[u8], off: usize) -> T {
    unsafe { std::ptr::read_unaligned(b.as_ptr().add(off) as *const T) }
}

impl Sys for WinSys {
    fn processes(&mut self) -> Vec<RawProc> {
        // SystemProcessInformation = 5
        loop {
            let mut ret = 0u32;
            let st = unsafe {
                NtQuerySystemInformation(5, self.buf.as_mut_ptr() as *mut c_void, self.buf.len() as u32, &mut ret)
            };
            if st == STATUS_INFO_LENGTH_MISMATCH {
                let want = (ret as usize).max(self.buf.len() * 2) + 64 * 1024;
                self.buf.resize(want, 0);
                continue;
            }
            if st < 0 {
                return Vec::new();
            }
            break;
        }
        let b = &self.buf;
        let mut out = Vec::with_capacity(400);
        let mut off = 0usize;
        loop {
            let next: u32 = rd(b, off);
            let threads: u32 = rd(b, off + 0x04);
            let create: i64 = rd(b, off + 0x20);
            let user: i64 = rd(b, off + 0x28);
            let kernel: i64 = rd(b, off + 0x30);
            let name_len: u16 = rd(b, off + 0x38);
            let name_ptr: *const u16 = rd(b, off + 0x40);
            let base_priority: i32 = rd(b, off + 0x48);
            let pid: usize = rd(b, off + 0x50);
            let ppid: usize = rd(b, off + 0x58);
            let session: u32 = rd(b, off + 0x64);
            let working_set: usize = rd(b, off + 0x90);
            let name = if name_ptr.is_null() || name_len == 0 {
                if pid == 0 { "System Idle Process".to_string() } else { "System".to_string() }
            } else {
                let s = unsafe { std::slice::from_raw_parts(name_ptr, name_len as usize / 2) };
                String::from_utf16_lossy(s)
            };
            out.push(RawProc {
                pid: pid as u32,
                ppid: ppid as u32,
                create_time: create as u64,
                name,
                session,
                cpu_time: (user + kernel) as u64,
                working_set: working_set as u64,
                base_priority,
                threads,
            });
            if next == 0 {
                break;
            }
            off += next as usize;
        }
        out
    }

    fn system_times(&mut self) -> Option<(u64, u64, u64)> {
        let (mut i, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        (unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } != 0).then(|| (i.get(), k.get(), u.get()))
    }

    fn topology(&self) -> Topology {
        self.topology.clone()
    }

    fn process_path(&mut self, pid: u32) -> Option<String> {
        let h = Handle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        (unsafe { QueryFullProcessImageNameW(h.0, 0, buf.as_mut_ptr(), &mut len) } != 0)
            .then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }

    fn foreground(&self) -> Option<Foreground> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let mut fullscreen = false;
            if hwnd != GetShellWindow() && hwnd != GetDesktopWindow() {
                let mut r = RECT::default();
                let mon = MonitorFromWindow(hwnd, 2);
                let mut mi = MONITORINFO { cb_size: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                if GetWindowRect(hwnd, &mut r) != 0 && GetMonitorInfoW(mon, &mut mi) != 0 {
                    let m = &mi.rc_monitor;
                    fullscreen = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
                }
            }
            Some(Foreground { pid, fullscreen })
        }
    }

    fn idle_ms(&self) -> u64 {
        let mut li = LASTINPUTINFO { cb_size: 8, dw_time: 0 };
        unsafe {
            if GetLastInputInfo(&mut li) == 0 {
                return 0;
            }
            GetTickCount().wrapping_sub(li.dw_time) as u64
        }
    }

    fn memory(&self) -> MemInfo {
        let mut m = MEMORYSTATUSEX { dw_length: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        unsafe { GlobalMemoryStatusEx(&mut m) };
        MemInfo { total_mb: m.ull_total_phys >> 20, avail_mb: m.ull_avail_phys >> 20, load: m.dw_memory_load }
    }

    fn self_pid(&self) -> u32 {
        self.self_pid
    }

    fn local_time(&self) -> (u16, u16, u16) {
        let mut t = [0u16; 8];
        unsafe { GetLocalTime(&mut t) };
        (t[4], t[5], t[6])
    }

    fn get_priority(&self, pid: u32) -> Option<Priority> {
        let h = Handle::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        Some(match unsafe { GetPriorityClass(h.0) } {
            0x40 => Priority::Idle,
            0x4000 => Priority::BelowNormal,
            0x20 => Priority::Normal,
            0x8000 => Priority::AboveNormal,
            0x80 => Priority::High,
            0x100 => Priority::Realtime,
            _ => return None,
        })
    }

    fn set_priority(&self, pid: u32, p: Priority) -> bool {
        Handle::open(pid, PROCESS_SET_INFORMATION)
            .map(|h| unsafe { SetPriorityClass(h.0, Self::priority_class(p)) } != 0)
            .unwrap_or(false)
    }

    fn set_io_priority(&self, pid: u32, p: IoPriority) -> bool {
        let v: u32 = match p {
            IoPriority::VeryLow => 0,
            IoPriority::Low => 1,
            IoPriority::Normal => 2,
            IoPriority::High => 3,
        };
        // ProcessIoPriority = 33
        Handle::open(pid, PROCESS_SET_INFORMATION)
            .map(|h| unsafe { NtSetInformationProcess(h.0, 33, &v as *const u32 as *const c_void, 4) } >= 0)
            .unwrap_or(false)
    }

    fn set_gpu_priority(&self, pid: u32, p: GpuPriority) -> bool {
        let v = match p {
            GpuPriority::Idle => 0,
            GpuPriority::BelowNormal => 1,
            GpuPriority::Normal => 2,
            GpuPriority::AboveNormal => 3,
            GpuPriority::High => 4,
            GpuPriority::Realtime => 5,
        };
        Handle::open(pid, PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION)
            .map(|h| unsafe { D3DKMTSetProcessSchedulingPriorityClass(h.0, v) } >= 0)
            .unwrap_or(false)
    }

    fn set_efficiency(&self, pid: u32, on: Option<bool>) -> bool {
        // PROCESS_POWER_THROTTLING_STATE { Version = 1, ControlMask, StateMask }, kelas ProcessPowerThrottling = 4.
        let state: [u32; 3] = match on {
            Some(true) => [1, 1, 1],
            Some(false) => [1, 1, 0],
            None => [1, 0, 0],
        };
        Handle::open(pid, PROCESS_SET_INFORMATION)
            .map(|h| unsafe { SetProcessInformation(h.0, 4, state.as_ptr() as *const c_void, 12) } != 0)
            .unwrap_or(false)
    }

    fn set_cpu_sets(&self, pid: u32, sets: &[u32]) -> bool {
        Handle::open(pid, PROCESS_SET_LIMITED_INFORMATION)
            .map(|h| unsafe {
                let ptr = if sets.is_empty() { std::ptr::null() } else { sets.as_ptr() };
                SetProcessDefaultCpuSets(h.0, ptr, sets.len() as u32) != 0
            })
            .unwrap_or(false)
    }

    fn set_affinity(&self, pid: u32, mask: u64) -> bool {
        mask != 0
            && Handle::open(pid, PROCESS_SET_INFORMATION)
                .map(|h| unsafe { SetProcessAffinityMask(h.0, mask as usize) } != 0)
                .unwrap_or(false)
    }

    fn terminate(&self, pid: u32) -> bool {
        Handle::open(pid, PROCESS_TERMINATE).map(|h| unsafe { TerminateProcess(h.0, 1) } != 0).unwrap_or(false)
    }

    fn suspend(&self, pid: u32) -> bool {
        Handle::open(pid, PROCESS_SUSPEND_RESUME).map(|h| unsafe { NtSuspendProcess(h.0) } >= 0).unwrap_or(false)
    }

    fn resume(&self, pid: u32) -> bool {
        Handle::open(pid, PROCESS_SUSPEND_RESUME).map(|h| unsafe { NtResumeProcess(h.0) } >= 0).unwrap_or(false)
    }

    fn ensure_performance_plan(&self) -> Option<String> {
        let plans = self.power_plans();
        let guid = match plans.iter().find(|p| p.name == super::PACU_PLAN_NAME) {
            Some(p) => guid_from_string(&p.guid)?,
            None => {
                let mut created = None;
                for src in [super::ULTIMATE_PERFORMANCE, super::HIGH_PERFORMANCE, super::BALANCED] {
                    let Some(src) = guid_from_string(src) else { continue };
                    let mut out: *mut GUID = std::ptr::null_mut();
                    if unsafe { PowerDuplicateScheme(std::ptr::null_mut(), &src, &mut out) } == 0 && !out.is_null() {
                        created = Some(unsafe { *out });
                        unsafe { LocalFree(out as *mut c_void) };
                        break;
                    }
                }
                let g = created?;
                let name: Vec<u8> = wide(super::PACU_PLAN_NAME).iter().flat_map(|c| c.to_le_bytes()).collect();
                let desc: Vec<u8> = wide("Dibuat oleh Pacu untuk gaming: core parking mati, CPU selalu di clock penuh, hemat daya USB/PCIe mati.")
                    .iter()
                    .flat_map(|c| c.to_le_bytes())
                    .collect();
                unsafe {
                    PowerWriteFriendlyName(std::ptr::null_mut(), &g, std::ptr::null(), std::ptr::null(), name.as_ptr(), name.len() as u32);
                    PowerWriteDescription(std::ptr::null_mut(), &g, std::ptr::null(), std::ptr::null(), desc.as_ptr(), desc.len() as u32);
                }
                g
            }
        };
        for (sub, setting, value) in PERFORMANCE_SETTINGS {
            let (Some(sub), Some(setting)) = (guid_from_string(sub), guid_from_string(setting)) else { continue };
            unsafe {
                PowerWriteACValueIndex(std::ptr::null_mut(), &guid, &sub, &setting, *value);
                PowerWriteDCValueIndex(std::ptr::null_mut(), &guid, &sub, &setting, *value);
            }
        }
        Some(guid_to_string(&guid))
    }

    fn power_plans(&self) -> Vec<PowerPlan> {
        let mut out = Vec::new();
        for index in 0..64 {
            let mut g = GUID::default();
            let mut size = std::mem::size_of::<GUID>() as u32;
            // ACCESS_SCHEME = 16
            let r = unsafe {
                PowerEnumerate(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), 16, index, &mut g as *mut GUID as *mut u8, &mut size)
            };
            if r != 0 {
                break;
            }
            let mut name = [0u16; 256];
            let mut nsize = (name.len() * 2) as u32;
            let nr = unsafe {
                PowerReadFriendlyName(std::ptr::null_mut(), &g, std::ptr::null(), std::ptr::null(), name.as_mut_ptr() as *mut u8, &mut nsize)
            };
            let guid = guid_to_string(&g);
            let name = if nr == 0 {
                let n = (nsize as usize / 2).min(name.len());
                String::from_utf16_lossy(&name[..n]).trim_end_matches('\0').to_string()
            } else {
                guid.clone()
            };
            out.push(PowerPlan { guid, name });
        }
        out
    }

    fn active_power_plan(&self) -> Option<String> {
        unsafe {
            let mut p: *mut GUID = std::ptr::null_mut();
            if PowerGetActiveScheme(std::ptr::null_mut(), &mut p) != 0 || p.is_null() {
                return None;
            }
            let s = guid_to_string(&*p);
            LocalFree(p as *mut c_void);
            Some(s)
        }
    }

    fn set_power_plan(&self, guid: &str) -> bool {
        guid_from_string(guid)
            .map(|g| unsafe { PowerSetActiveScheme(std::ptr::null_mut(), &g) } == 0)
            .unwrap_or(false)
    }

    fn set_timer_resolution(&mut self, on: bool) -> Option<f32> {
        if on == self.timer_on {
            return None;
        }
        let mut cur = 0u32;
        let st = unsafe { NtSetTimerResolution(5000, on as u8, &mut cur) };
        if st < 0 {
            return None;
        }
        self.timer_on = on;
        Some(cur as f32 / 10_000.0)
    }

    fn prevent_sleep(&mut self, on: bool) {
        if on == self.sleep_blocked {
            return;
        }
        // ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        let flags = if on { 0x8000_0000 | 0x1 | 0x2 } else { 0x8000_0000 };
        unsafe { SetThreadExecutionState(flags) };
        self.sleep_blocked = on;
    }

    fn purge_standby(&mut self) -> bool {
        // SystemMemoryListInformation = 80, MemoryPurgeStandbyList = 4
        let cmd: u32 = 4;
        unsafe { NtSetSystemInformation(80, &cmd as *const u32 as *const c_void, 4) >= 0 }
    }

    fn trim_memory(&mut self) -> bool {
        // MemoryEmptyWorkingSets = 2, MemoryFlushModifiedList = 3
        let empty: u32 = 2;
        let flush: u32 = 3;
        unsafe {
            let ok = NtSetSystemInformation(80, &empty as *const u32 as *const c_void, 4) >= 0;
            NtSetSystemInformation(80, &flush as *const u32 as *const c_void, 4);
            ok
        }
    }
}

/// Mutex satu instance plus event untuk memunculkan jendela instance yang sudah berjalan.
pub mod instance {
    use super::{wide, HANDLE};
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(attr: *mut c_void, owner: i32, name: *const u16) -> HANDLE;
        fn CreateEventW(attr: *mut c_void, manual: i32, initial: i32, name: *const u16) -> HANDLE;
        fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> HANDLE;
        fn SetEvent(h: HANDLE) -> i32;
        fn WaitForSingleObject(h: HANDLE, ms: u32) -> u32;
        fn GetLastError() -> u32;
    }

    pub struct Instance {
        show_event: HANDLE,
    }

    unsafe impl Send for Instance {}
    unsafe impl Sync for Instance {}

    const MUTEX: &str = "Local\\PacuGameBooster.Instance";
    const EVENT: &str = "Local\\PacuGameBooster.Show";

    /// `None` bila Pacu sudah berjalan (instance lama diminta menampilkan jendelanya).
    pub fn acquire() -> Option<Instance> {
        unsafe {
            CreateMutexW(std::ptr::null_mut(), 0, wide(MUTEX).as_ptr());
            if GetLastError() == 183 {
                let ev = OpenEventW(0x0002, 0, wide(EVENT).as_ptr());
                if !ev.is_null() {
                    SetEvent(ev);
                }
                return None;
            }
            let show_event = CreateEventW(std::ptr::null_mut(), 0, 0, wide(EVENT).as_ptr());
            Some(Instance { show_event })
        }
    }

    impl Instance {
        pub fn show_requested(&self) -> bool {
            !self.show_event.is_null() && unsafe { WaitForSingleObject(self.show_event, 0) } == 0
        }
    }
}
