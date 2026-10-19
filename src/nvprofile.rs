//! Profil driver NVIDIA per game (sama dengan "Program Settings" di NVIDIA Control Panel), lewat
//! fungsi NvAPI_DRS_* yang terdokumentasi di NVAPI SDK publik (lisensi MIT).
//!
//! Pacu hanya menulis nilai pengguna di profil game; "Kembalikan" menghapus nilai itu sehingga
//! driver kembali memakai nilai bawaannya.

/// (ID setting, nilai, keterangan). ID dan nilai dari NvApiDriverSettings.h.
pub const SETTINGS: &[(u32, u32, &str)] = &[
    // PREFERRED_PSTATE_ID = PREFERRED_PSTATE_PREFER_MAX
    (0x1057_EB71, 1, "Power management: Prefer maximum performance"),
    // PRERENDERLIMIT_ID = 1 ("Low Latency Mode: On")
    (0x007B_A09E, 1, "Low Latency Mode: On"),
    // OGL_THREAD_CONTROL_ID = OGL_THREAD_CONTROL_ENABLE
    (0x20C1_221E, 1, "Threaded optimization: On"),
    // PS_SHADERDISKCACHE_ID = PS_SHADERDISKCACHE_ON
    (0x0019_8FFF, 1, "Shader Cache: On"),
];

#[cfg(windows)]
mod imp {
    use super::SETTINGS;
    use crate::volt::gpu::nvapi::{err, NvApi};
    use std::ffi::c_void;

    const DRS_CREATE_SESSION: u32 = 0x0694_D52E;
    const DRS_DESTROY_SESSION: u32 = 0xDAD9_CFF8;
    const DRS_LOAD_SETTINGS: u32 = 0x375D_BD6B;
    const DRS_SAVE_SETTINGS: u32 = 0xFCBC_7E14;
    const DRS_CREATE_PROFILE: u32 = 0xCC17_6068;
    const DRS_FIND_PROFILE_BY_NAME: u32 = 0x7E4A_9A0B;
    const DRS_CREATE_APPLICATION: u32 = 0x4347_A9DE;
    const DRS_FIND_APPLICATION_BY_NAME: u32 = 0xEEE5_66B2;
    const DRS_SET_SETTING: u32 = 0x577D_D202;
    const DRS_GET_SETTING: u32 = 0x73BF_8338;
    const DRS_DELETE_PROFILE_SETTING: u32 = 0xE4A2_6362;

    const STR: usize = 2048 * 2;
    // NVDRS_SETTING_V1 (pack 4): nama di 4, id di 4100, tipe 4104, lokasi 4108, nilai sekarang di 8220.
    const SETTING_SIZE: usize = 12320;
    const SETTING_ID: usize = 4 + STR;
    const SETTING_LOCATION: usize = SETTING_ID + 8;
    const SETTING_CURRENT: usize = 8220;
    // NVDRS_PROFILE_V1: version, nama, gpuSupport, isPredefined, numOfApps, numOfSettings.
    const PROFILE_SIZE: usize = 4 + STR + 16;
    // NVDRS_APPLICATION_V4: version, isPredefined, appName, userFriendlyName, launcher, fileInFolder, flags, commandLine.
    const APP_SIZE: usize = 8 + STR * 4 + 4 + STR;
    const NVAPI_EXECUTABLE_NOT_FOUND: i32 = -166;
    const NVAPI_PROFILE_NOT_FOUND: i32 = -163;
    const NVAPI_SETTING_NOT_FOUND: i32 = -160;

    type Session = *mut c_void;
    type Profile = *mut c_void;

    fn versioned(size: usize, ver: u32) -> Vec<u8> {
        let mut b = vec![0u8; size];
        b[..4].copy_from_slice(&((size as u32) | (ver << 16)).to_le_bytes());
        b
    }

    fn wstr(s: &str) -> Vec<u16> {
        let mut w = vec![0u16; 2048];
        for (i, c) in s.encode_utf16().take(2047).enumerate() {
            w[i] = c;
        }
        w
    }

    fn put_wstr(b: &mut [u8], off: usize, s: &str) {
        for (i, c) in s.encode_utf16().take(2047).enumerate() {
            b[off + i * 2..off + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
        }
    }

    struct Drs {
        api: NvApi,
        s: Session,
    }

    impl Drs {
        fn f<T: Copy>(&self, id: u32) -> Result<T, String> {
            let p = self.api.func(id)?;
            Ok(unsafe { std::mem::transmute_copy(&p) })
        }

        fn open() -> Result<Drs, String> {
            let api = NvApi::load()?;
            let mut d = Drs { api, s: std::ptr::null_mut() };
            unsafe {
                let create: unsafe extern "C" fn(*mut Session) -> i32 = d.f(DRS_CREATE_SESSION)?;
                let r = create(&mut d.s);
                if r != 0 {
                    return Err(err("NvAPI_DRS_CreateSession", r));
                }
                let load: unsafe extern "C" fn(Session) -> i32 = d.f(DRS_LOAD_SETTINGS)?;
                let r = load(d.s);
                if r != 0 {
                    return Err(err("NvAPI_DRS_LoadSettings", r));
                }
            }
            Ok(d)
        }

        fn save(&self) -> Result<(), String> {
            let save: unsafe extern "C" fn(Session) -> i32 = self.f(DRS_SAVE_SETTINGS)?;
            let r = unsafe { save(self.s) };
            if r != 0 {
                return Err(err("NvAPI_DRS_SaveSettings", r));
            }
            Ok(())
        }

        /// Profil tempat `exe` terdaftar. Bila belum ada dan `create`, dibuat profil "Pacu - exe".
        fn profile_for(&self, exe: &str, create: bool) -> Result<Option<Profile>, String> {
            unsafe {
                let find: unsafe extern "C" fn(Session, *const u16, *mut Profile, *mut u8) -> i32 = self.f(DRS_FIND_APPLICATION_BY_NAME)?;
                let mut app = versioned(APP_SIZE, 4);
                let mut p: Profile = std::ptr::null_mut();
                let r = find(self.s, wstr(exe).as_ptr(), &mut p, app.as_mut_ptr());
                if r == 0 {
                    return Ok(Some(p));
                }
                if r != NVAPI_EXECUTABLE_NOT_FOUND {
                    return Err(err("NvAPI_DRS_FindApplicationByName", r));
                }
                if !create {
                    return Ok(None);
                }
                let name = format!("Pacu - {exe}");
                let find_p: unsafe extern "C" fn(Session, *const u16, *mut Profile) -> i32 = self.f(DRS_FIND_PROFILE_BY_NAME)?;
                let r = find_p(self.s, wstr(&name).as_ptr(), &mut p);
                if r == NVAPI_PROFILE_NOT_FOUND {
                    let cp: unsafe extern "C" fn(Session, *mut u8, *mut Profile) -> i32 = self.f(DRS_CREATE_PROFILE)?;
                    let mut prof = versioned(PROFILE_SIZE, 1);
                    put_wstr(&mut prof, 4, &name);
                    let r = cp(self.s, prof.as_mut_ptr(), &mut p);
                    if r != 0 {
                        return Err(err("NvAPI_DRS_CreateProfile", r));
                    }
                } else if r != 0 {
                    return Err(err("NvAPI_DRS_FindProfileByName", r));
                }
                let ca: unsafe extern "C" fn(Session, Profile, *mut u8) -> i32 = self.f(DRS_CREATE_APPLICATION)?;
                let mut app = versioned(APP_SIZE, 4);
                put_wstr(&mut app, 8, exe);
                let r = ca(self.s, p, app.as_mut_ptr());
                if r != 0 {
                    return Err(err("NvAPI_DRS_CreateApplication", r));
                }
                Ok(Some(p))
            }
        }

        /// Nilai DWORD yang ditulis pengguna di profil ini (None bila memakai bawaan driver).
        fn user_value(&self, p: Profile, id: u32) -> Result<Option<u32>, String> {
            unsafe {
                let get: unsafe extern "C" fn(Session, Profile, u32, *mut u8) -> i32 = self.f(DRS_GET_SETTING)?;
                let mut b = versioned(SETTING_SIZE, 1);
                let r = get(self.s, p, id, b.as_mut_ptr());
                if r == NVAPI_SETTING_NOT_FOUND {
                    return Ok(None);
                }
                if r != 0 {
                    return Err(err("NvAPI_DRS_GetSetting", r));
                }
                let loc = u32::from_le_bytes(b[SETTING_LOCATION..SETTING_LOCATION + 4].try_into().unwrap());
                let val = u32::from_le_bytes(b[SETTING_CURRENT..SETTING_CURRENT + 4].try_into().unwrap());
                Ok((loc == 0).then_some(val))
            }
        }

        fn set(&self, p: Profile, id: u32, v: u32) -> Result<(), String> {
            unsafe {
                let set: unsafe extern "C" fn(Session, Profile, *mut u8) -> i32 = self.f(DRS_SET_SETTING)?;
                let mut b = versioned(SETTING_SIZE, 1);
                b[SETTING_ID..SETTING_ID + 4].copy_from_slice(&id.to_le_bytes());
                // Tipe DWORD = 0, sudah nol.
                b[SETTING_CURRENT..SETTING_CURRENT + 4].copy_from_slice(&v.to_le_bytes());
                let r = set(self.s, p, b.as_mut_ptr());
                if r != 0 {
                    return Err(err(&format!("NvAPI_DRS_SetSetting({id:#010X})"), r));
                }
                Ok(())
            }
        }

        fn delete(&self, p: Profile, id: u32) -> Result<(), String> {
            let del: unsafe extern "C" fn(Session, Profile, u32) -> i32 = self.f(DRS_DELETE_PROFILE_SETTING)?;
            let r = unsafe { del(self.s, p, id) };
            if r != 0 && r != NVAPI_SETTING_NOT_FOUND {
                return Err(err("NvAPI_DRS_DeleteProfileSetting", r));
            }
            Ok(())
        }
    }

    impl Drop for Drs {
        fn drop(&mut self) {
            if let Ok(d) = self.f::<unsafe extern "C" fn(Session) -> i32>(DRS_DESTROY_SESSION) {
                unsafe { d(self.s) };
            }
        }
    }

    pub fn status(exes: &[&str]) -> Result<bool, String> {
        let d = Drs::open()?;
        for exe in exes {
            let Some(p) = d.profile_for(exe, false)? else { return Ok(false) };
            for &(id, v, _) in SETTINGS {
                if d.user_value(p, id)? != Some(v) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    pub fn apply(exes: &[&str]) -> Result<(), String> {
        let d = Drs::open()?;
        for exe in exes {
            let p = d.profile_for(exe, true)?.ok_or("Profil tidak bisa dibuat")?;
            for &(id, v, _) in SETTINGS {
                d.set(p, id, v)?;
            }
        }
        d.save()
    }

    pub fn revert(exes: &[&str]) -> Result<(), String> {
        let d = Drs::open()?;
        for exe in exes {
            if let Some(p) = d.profile_for(exe, false)? {
                for &(id, _, _) in SETTINGS {
                    d.delete(p, id)?;
                }
            }
        }
        d.save()
    }
}

#[cfg(windows)]
pub use imp::{apply, revert, status};

#[cfg(not(windows))]
pub fn status(_: &[&str]) -> Result<bool, String> {
    Err("Hanya di Windows".into())
}
#[cfg(not(windows))]
pub fn apply(_: &[&str]) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn revert(_: &[&str]) -> Result<(), String> {
    Ok(())
}
