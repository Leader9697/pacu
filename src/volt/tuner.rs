//! Mesin pencari undervolt yang berjalan di thread pekerja. Thread ini juga satu-satunya pemilik
//! akses GPU, jadi pemantauan sensor saat diam juga dilakukan di sini.

use crate::volt::curve;
use crate::volt::gpu::{Curve, GpuInfo, Telemetry};
use crate::volt::plan::{self, Next};
use crate::volt::rig::{LoadStatus, Rig};
use crate::volt::store::{self, Measure, Planned, Profile, Session, Settings, TestKind, TestResult};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Preparing,
    WaitingLoad,
    Baseline,
    Testing,
    Recovering,
    Done,
    NoUndervolt,
    Failed,
    Stopped,
}

impl Phase {
    pub fn busy(self) -> bool {
        matches!(self, Phase::Preparing | Phase::WaitingLoad | Phase::Baseline | Phase::Testing | Phase::Recovering)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Good,
    Warn,
    Bad,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: String,
    pub kind: LogKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub clock: f32,
    pub volt: f32,
    pub power: f32,
    pub temp: f32,
}

#[derive(Default)]
pub struct View {
    pub phase: Option<Phase>,
    pub headline: String,
    pub info: GpuInfo,
    pub gpu_error: Option<String>,
    pub telemetry: Telemetry,
    pub history: VecDeque<Sample>,
    pub curve: Curve,
    pub test: Option<Planned>,
    pub test_elapsed: u32,
    pub test_total: u32,
    pub session: Option<Session>,
    pub profile: Option<Profile>,
    pub log: Vec<LogLine>,
    pub diagnostics: Option<String>,
}

impl View {
    pub fn phase(&self) -> Phase {
        self.phase.unwrap_or(Phase::Idle)
    }
}

pub struct Shared {
    pub view: RwLock<View>,
    pub stop: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { view: RwLock::new(View::default()), stop: AtomicBool::new(false) })
    }

    pub fn log(&self, kind: LogKind, text: impl Into<String>) {
        let text = text.into();
        let time = store::stamp();
        store::append_log(&format!("{time} {kind:?} {text}"));
        let mut v = self.view.write();
        v.log.push(LogLine { time, kind, text });
        if v.log.len() > 2000 {
            v.log.drain(..500);
        }
    }

    fn set_phase(&self, p: Phase, headline: impl Into<String>) {
        let mut v = self.view.write();
        v.phase = Some(p);
        v.headline = headline.into();
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

pub enum Cmd {
    Start { settings: Settings, resume: Option<Session> },
    ApplyProfile,
    ResetStock,
    RefreshCurve,
    Diagnostics,
    Quit,
}

/// Loop utama thread pekerja.
pub fn worker(mut rig: Rig, sh: Arc<Shared>, rx: Receiver<Cmd>, make_load: impl Fn(&Settings) -> Box<dyn crate::volt::rig::Load>) {
    {
        let mut v = sh.view.write();
        v.info = rig.gpu.info();
        v.profile = Profile::load_for(&v.info.name);
    }
    refresh_curve(&mut rig, &sh);
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Cmd::Start { settings, resume }) => {
                sh.stop.store(false, Ordering::Relaxed);
                rig.load = make_load(&settings);
                tune(&mut rig, &sh, settings, resume);
                rig.load.stop();
                rig.checker.stop();
                refresh_curve(&mut rig, &sh);
            }
            Ok(Cmd::ApplyProfile) => {
                match Profile::load_for(&rig.gpu.info().name) {
                    Some(p) => match rig.gpu.set_deltas(&p.deltas) {
                        Ok(()) => sh.log(LogKind::Good, format!("Profil {} MHz @ {:.0} mV dipasang", p.target_mhz, p.volt_mv)),
                        Err(e) => sh.log(LogKind::Bad, format!("Gagal memasang profil: {e}")),
                    },
                    None => sh.log(LogKind::Warn, "Belum ada profil tersimpan"),
                }
                refresh_curve(&mut rig, &sh);
            }
            Ok(Cmd::ResetStock) => {
                match rig.gpu.reset() {
                    Ok(()) => sh.log(LogKind::Info, "Kurva V/F dikembalikan ke bawaan"),
                    Err(e) => sh.log(LogKind::Bad, format!("Gagal mengembalikan kurva: {e}")),
                }
                refresh_curve(&mut rig, &sh);
            }
            Ok(Cmd::RefreshCurve) => refresh_curve(&mut rig, &sh),
            Ok(Cmd::Diagnostics) => {
                let d = rig.gpu.diagnostics();
                store::append_log(&d);
                sh.view.write().diagnostics = Some(d);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let t = rig.gpu.telemetry();
        push_telemetry(&sh, t);
    }
    rig.load.stop();
    rig.checker.stop();
}

fn refresh_curve(rig: &mut Rig, sh: &Shared) {
    match rig.gpu.curve() {
        Ok(c) => {
            let mut v = sh.view.write();
            v.curve = c;
            v.gpu_error = None;
        }
        Err(e) => sh.view.write().gpu_error = Some(e),
    }
}

fn push_telemetry(sh: &Shared, t: Telemetry) {
    let mut v = sh.view.write();
    let s = Sample {
        clock: t.clock_mhz.unwrap_or(0) as f32,
        volt: t.voltage_mv.unwrap_or(0.0),
        power: t.power_w.unwrap_or(0.0),
        temp: t.temp_c.unwrap_or(0) as f32,
    };
    v.history.push_back(s);
    while v.history.len() > 300 {
        v.history.pop_front();
    }
    v.telemetry = t;
}

enum Outcome {
    Pass(Measure),
    Fail(String),
    /// Uji dihentikan karena alasan yang bukan ketidakstabilan (suhu, beban hilang, dll).
    Abort(String),
    Stopped,
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Menjalankan seluruh pencarian. Selalu mengembalikan GPU ke keadaan aman (bawaan atau profil
/// yang sudah lolos) sebelum selesai.
fn tune(rig: &mut Rig, sh: &Shared, settings: Settings, resume: Option<Session>) {
    sh.view.write().log.clear();
    sh.set_phase(Phase::Preparing, "Menyiapkan");
    let info = rig.gpu.info();

    let mut s = match resume {
        Some(mut s) => {
            sh.log(LogKind::Info, format!("Melanjutkan sesi: {} uji sudah selesai", s.results.len()));
            s.settings.load_exe = settings.load_exe.clone();
            s.settings.load_args = settings.load_args.clone();
            s.settings.use_checker = settings.use_checker;
            s
        }
        None => {
            let mut c = match rig.gpu.curve() {
                Ok(c) => c,
                Err(e) => return fail(sh, format!("Kurva V/F tidak bisa dibaca: {e}")),
            };
            for p in &mut c.points {
                p.delta_khz = 0;
            }
            Session { gpu_name: info.name.clone(), started: store::now_secs(), settings: settings.clone(), curve: c, ..Default::default() }
        }
    };
    if s.curve.points.len() < 8 {
        return fail(sh, "Kurva V/F terlalu pendek atau kosong. Kirim laporan Diagnostik.".into());
    }
    sh.view.write().session = Some(s.clone());

    if other_oc_tool_running() {
        sh.log(LogKind::Warn, "MSI Afterburner sedang berjalan. Tutup dulu, atau pastikan tidak memasang profil, supaya kurva tidak saling menimpa.");
    }
    if let Err(e) = rig.gpu.reset() {
        return fail(sh, format!("Tidak bisa menulis kurva V/F: {e}"));
    }
    sh.log(LogKind::Info, "Kurva dikembalikan ke bawaan sebelum mulai");

    if let Err(e) = start_load(rig, sh) {
        return fail(sh, e);
    }
    match wait_load(rig, sh) {
        Ok(()) => {}
        Err(Outcome::Stopped) => return stopped(rig, sh, &mut s),
        Err(Outcome::Abort(e)) | Err(Outcome::Fail(e)) => return fail_reset(rig, sh, e),
        Err(Outcome::Pass(_)) => unreachable!(),
    }

    if s.settings.use_checker && rig.checker.available() {
        sh.set_phase(Phase::Preparing, "Merekam hasil acuan pemeriksa error");
        match rig.checker.prepare() {
            Ok(()) => {
                rig.checker.start();
                sh.log(LogKind::Info, "Pemeriksa error komputasi aktif");
            }
            Err(e) => sh.log(LogKind::Warn, format!("Pemeriksa error tidak bisa dipakai: {e}")),
        }
    }

    if s.baseline.is_none() {
        sh.set_phase(Phase::Baseline, "Mengukur kondisi bawaan");
        rig.watchdog.mark();
        rig.checker.take();
        let secs = s.settings.baseline_secs.max(20);
        match monitor(rig, sh, secs, None) {
            Outcome::Pass(m) => {
                let target = match s.settings.target_mhz {
                    Some(mhz) => mhz * 1000,
                    None => rig.gpu.default_target_khz().unwrap_or_else(|| curve::snap_down_khz((m.clock_mhz * 1000.0) as u32)),
                };
                sh.log(
                    LogKind::Good,
                    format!(
                        "Bawaan: {:.0} MHz @ {:.0} mV, {:.0} W, {:.0}°C. Target: {} MHz",
                        m.clock_mhz,
                        m.volt_mv,
                        m.power_w,
                        m.temp_c,
                        target / 1000
                    ),
                );
                if m.util_pct < 90.0 {
                    sh.log(LogKind::Warn, format!("Pemakaian GPU rata-rata hanya {:.0}%. Beban yang lebih berat memberi hasil yang lebih bisa dipercaya.", m.util_pct));
                }
                s.baseline = Some(m);
                s.target_khz = target;
                s.save();
            }
            Outcome::Fail(e) => return fail_reset(rig, sh, format!("GPU sudah tidak stabil di pengaturan bawaan: {e}")),
            Outcome::Abort(e) => return fail_reset(rig, sh, e),
            Outcome::Stopped => return stopped(rig, sh, &mut s),
        }
    }

    loop {
        sh.view.write().session = Some(s.clone());
        match plan::next(&s, info.min_delta_khz, info.max_delta_khz) {
            Next::Baseline => return fail_reset(rig, sh, "Pengukuran bawaan hilang".into()),
            Next::Test(p) => match run_test(rig, sh, &mut s, p) {
                Ok(()) => {}
                Err(Outcome::Stopped) => return stopped(rig, sh, &mut s),
                Err(Outcome::Abort(e)) | Err(Outcome::Fail(e)) => {
                    s.testing = None;
                    s.save();
                    return fail_reset(rig, sh, e);
                }
                Err(Outcome::Pass(_)) => unreachable!(),
            },
            Next::Done(pos) => return finish(rig, sh, &mut s, pos),
            Next::NoUndervolt => {
                let _ = rig.gpu.reset();
                s.finished = true;
                s.save();
                sh.view.write().session = Some(s.clone());
                sh.log(LogKind::Warn, "Tidak ada titik di bawah tegangan bawaan yang stabil untuk frekuensi ini");
                sh.set_phase(Phase::NoUndervolt, "Tidak ada undervolt yang stabil");
                return;
            }
            Next::GiveUp(e) => {
                s.finished = true;
                s.save();
                sh.view.write().session = Some(s.clone());
                return fail_reset(rig, sh, e);
            }
        }
    }
}

fn other_oc_tool_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq MSIAfterburner.exe", "/NH"])
            .creation_flags(0x0800_0000)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("msiafterburner.exe"))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn start_load(rig: &mut Rig, sh: &Shared) -> Result<(), String> {
    match rig.load.status() {
        LoadStatus::Running | LoadStatus::External => {}
        LoadStatus::Exited(_) => {
            rig.load.start()?;
            sh.log(LogKind::Info, "Aplikasi beban 3D dijalankan");
        }
    }
    Ok(())
}

/// Menunggu GPU benar-benar sibuk (pemakaian >= 85% selama 5 detik berturut-turut).
fn wait_load(rig: &mut Rig, sh: &Shared) -> Result<(), Outcome> {
    let external = rig.load.status() == LoadStatus::External;
    sh.set_phase(
        Phase::WaitingLoad,
        if external { "Jalankan aplikasi 3D kamu sekarang (tanpa V-Sync; boleh dalam jendela)" } else { "Menunggu aplikasi 3D membebani GPU" },
    );
    let limit = if external { 1800 } else { 180 };
    let mut busy = 0;
    for _ in 0..limit {
        if sh.stopped() {
            return Err(Outcome::Stopped);
        }
        rig.clock.sleep(Duration::from_secs(1));
        let t = rig.gpu.telemetry();
        let util = t.util_pct.unwrap_or(0);
        push_telemetry(sh, t);
        if let LoadStatus::Exited(c) = rig.load.status() {
            return Err(Outcome::Abort(format!("Aplikasi beban tertutup sebelum uji dimulai ({c})")));
        }
        busy = if util >= 85 { busy + 1 } else { 0 };
        if busy >= 5 {
            // Biarkan clock dan suhu sedikit naik dulu.
            for _ in 0..10 {
                if sh.stopped() {
                    return Err(Outcome::Stopped);
                }
                rig.clock.sleep(Duration::from_secs(1));
                let t = rig.gpu.telemetry();
                push_telemetry(sh, t);
            }
            return Ok(());
        }
    }
    Err(Outcome::Abort("GPU tidak pernah sibuk di atas 85%. Pastikan aplikasi 3D berjalan tanpa V-Sync atau batas FPS.".into()))
}

/// Memantau satu uji. `expected` adalah offset yang sedang dipasang, untuk mendeteksi reset driver.
fn monitor(rig: &mut Rig, sh: &Shared, secs: u32, expected: Option<&[(usize, i32)]>) -> Outcome {
    let warm = (secs / 6).clamp(5, 20);
    let (mut clock, mut volt, mut power, mut temp, mut util) = (vec![], vec![], vec![], vec![], vec![]);
    let mut limited = 0u32;
    let mut idle_streak = 0;
    let temp_limit = sh.view.read().session.as_ref().map(|s| s.settings.temp_limit_c).unwrap_or(87);
    let nonzero = expected.is_some_and(|d| d.iter().any(|&(_, x)| x != 0));
    {
        let mut v = sh.view.write();
        v.test_elapsed = 0;
        v.test_total = secs;
    }
    for sec in 1..=secs {
        if sh.stopped() {
            return Outcome::Stopped;
        }
        rig.clock.sleep(Duration::from_secs(1));
        let t = rig.gpu.telemetry();
        let tel = t.clone();
        push_telemetry(sh, t);
        sh.view.write().test_elapsed = sec;

        let ev = rig.watchdog.events_since_mark();
        if let Some(e) = ev.first() {
            return Outcome::Fail(format!("Driver GPU melaporkan error: {e}"));
        }
        if let LoadStatus::Exited(c) = rig.load.status() {
            return Outcome::Fail(format!("Aplikasi beban 3D tertutup atau crash ({c})"));
        }
        let ck = rig.checker.take();
        if ck.device_lost {
            return Outcome::Fail("Perangkat GPU hilang di tengah uji (driver di-reset)".into());
        }
        if ck.errors > 0 {
            return Outcome::Fail(format!("GPU salah hitung: {} hasil berbeda dari acuan", ck.errors));
        }
        if nonzero && sec % 5 == 0 {
            match rig.gpu.curve() {
                Ok(c) if curve::all_zero(&c.points) => {
                    return Outcome::Fail("Offset kurva hilang: driver di-reset (TDR)".into());
                }
                Ok(_) => {}
                Err(e) => return Outcome::Fail(format!("Driver tidak merespons: {e}")),
            }
        }
        let u = tel.util_pct.unwrap_or(0);
        idle_streak = if u < 60 { idle_streak + 1 } else { 0 };
        if idle_streak >= 10 {
            return Outcome::Fail("Beban GPU berhenti (aplikasi 3D kemungkinan crash atau macet)".into());
        }
        if let Some(tc) = tel.temp_c {
            if tc >= temp_limit {
                return Outcome::Abort(format!("Suhu GPU mencapai {tc}°C (batas {temp_limit}°C). Perbaiki pendinginan atau naikkan batas suhu."));
            }
        }
        if sec > warm {
            clock.push(tel.clock_mhz.unwrap_or(0) as f32);
            if let Some(v) = tel.voltage_mv {
                volt.push(v);
            }
            power.push(tel.power_w.unwrap_or(0.0));
            temp.push(tel.temp_c.unwrap_or(0) as f32);
            util.push(u as f32);
            if tel.limits.contains(&"daya") {
                limited += 1;
            }
        }
    }
    let n = clock.len().max(1) as f32;
    Outcome::Pass(Measure {
        clock_mhz: median(&mut clock),
        volt_mv: median(&mut volt),
        power_w: power.iter().sum::<f32>() / n,
        temp_c: temp.iter().cloned().fold(0.0, f32::max),
        util_pct: util.iter().sum::<f32>() / n,
        samples: clock.len() as u32,
        power_limited_pct: limited as f32 * 100.0 / n,
    })
}

fn run_test(rig: &mut Rig, sh: &Shared, s: &mut Session, p: Planned) -> Result<(), Outcome> {
    let deltas = curve::flatten(&s.curve, p.pos, s.target_khz);
    let label = match p.kind {
        TestKind::Search => "Menguji",
        TestKind::Validate => "Validasi panjang",
    };
    sh.set_phase(Phase::Testing, format!("{label}: {} MHz @ {:.0} mV", p.target_mhz, p.volt_mv));
    sh.log(LogKind::Info, format!("{label} {} MHz @ {:.0} mV", p.target_mhz, p.volt_mv));
    s.testing = Some(p.clone());
    s.save();
    sh.view.write().test = Some(p.clone());

    rig.gpu.set_deltas(&deltas).map_err(|e| Outcome::Abort(format!("Driver menolak penulisan kurva: {e}")))?;
    match rig.gpu.curve() {
        Ok(c) => {
            if !curve::deltas_match(&c, &deltas, 1_000) {
                return Err(Outcome::Abort("Kurva yang terbaca berbeda dengan yang ditulis. Kirim laporan Diagnostik.".into()));
            }
            sh.view.write().curve = c;
        }
        Err(e) => return Err(Outcome::Abort(format!("Kurva tidak bisa dibaca ulang: {e}"))),
    }
    rig.watchdog.mark();
    rig.checker.take();

    let secs = match p.kind {
        TestKind::Search => s.settings.step_secs,
        TestKind::Validate => s.settings.validate_secs,
    }
    .max(20);
    let out = monitor(rig, sh, secs, Some(&deltas));
    sh.view.write().test = None;
    let mut result = TestResult {
        kind: p.kind,
        pos: p.pos,
        volt_mv: p.volt_mv,
        target_mhz: p.target_mhz,
        passed: false,
        reason: String::new(),
        secs,
        measure: None,
    };
    match out {
        Outcome::Pass(m) => {
            let off = m.clock_mhz - p.target_mhz as f32;
            if off.abs() > 45.0 && m.power_limited_pct < 30.0 {
                sh.log(LogKind::Warn, format!("Clock rata-rata {:.0} MHz, berbeda dari target {} MHz", m.clock_mhz, p.target_mhz));
            }
            sh.log(LogKind::Good, format!("Lolos: {:.0} MHz, {:.0} W, {:.0}°C", m.clock_mhz, m.power_w, m.temp_c));
            result.passed = true;
            result.reason = "Stabil".into();
            result.measure = Some(m);
            s.results.push(result);
            s.testing = None;
            s.save();
            Ok(())
        }
        Outcome::Fail(reason) => {
            sh.log(LogKind::Bad, format!("Gagal di {:.0} mV: {reason}", p.volt_mv));
            result.reason = reason;
            s.results.push(result);
            s.testing = None;
            s.save();
            sh.view.write().session = Some(s.clone());
            recover(rig, sh)
        }
        Outcome::Abort(e) => Err(Outcome::Abort(e)),
        Outcome::Stopped => Err(Outcome::Stopped),
        #[allow(unreachable_patterns)]
        _ => unreachable!(),
    }
}

/// Setelah uji gagal: kembali ke bawaan, beri waktu driver pulih, nyalakan lagi beban.
fn recover(rig: &mut Rig, sh: &Shared) -> Result<(), Outcome> {
    sh.set_phase(Phase::Recovering, "Memulihkan setelah uji gagal");
    let mut ok = false;
    for _ in 0..15 {
        if rig.gpu.reset().is_ok() {
            ok = true;
            break;
        }
        rig.clock.sleep(Duration::from_secs(2));
    }
    if !ok {
        return Err(Outcome::Abort("Driver GPU tidak merespons setelah uji gagal. Restart PC.".into()));
    }
    rig.clock.sleep(Duration::from_secs(10));
    let lost = rig.checker.take().device_lost;
    if lost {
        rig.checker.stop();
        rig.checker.start();
    }
    if let LoadStatus::Exited(_) = rig.load.status() {
        rig.load.start().map_err(Outcome::Abort)?;
        sh.log(LogKind::Info, "Aplikasi beban 3D dijalankan ulang");
    }
    wait_load(rig, sh)?;
    rig.checker.take();
    Ok(())
}

fn finish(rig: &mut Rig, sh: &Shared, s: &mut Session, pos: usize) {
    let deltas = curve::flatten(&s.curve, pos, s.target_khz);
    if let Err(e) = rig.gpu.set_deltas(&deltas) {
        return fail_reset(rig, sh, format!("Gagal memasang hasil: {e}"));
    }
    let result = s.results.iter().rev().find(|r| r.kind == TestKind::Validate && r.passed && r.pos == pos).and_then(|r| r.measure.clone());
    let old = Profile::load();
    let profile = Profile {
        gpu_name: s.gpu_name.clone(),
        created: store::now_secs(),
        volt_mv: s.curve.points[pos].volt_mv(),
        target_mhz: s.target_khz / 1000,
        deltas,
        baseline: s.baseline.clone(),
        result,
        apply_on_login: old.map(|o| o.apply_on_login).unwrap_or(false),
    };
    profile.save();
    s.finished = true;
    s.testing = None;
    s.save();
    let msg = format!("Titik optimal: {} MHz @ {:.0} mV", profile.target_mhz, profile.volt_mv);
    sh.log(LogKind::Good, format!("{msg}. Profil disimpan dan sedang terpasang."));
    {
        let mut v = sh.view.write();
        v.session = Some(s.clone());
        v.profile = Some(profile);
    }
    rig.load.stop();
    rig.checker.stop();
    sh.set_phase(Phase::Done, msg);
}

fn fail(sh: &Shared, e: String) {
    sh.log(LogKind::Bad, e.clone());
    sh.set_phase(Phase::Failed, e);
}

fn fail_reset(rig: &mut Rig, sh: &Shared, e: String) {
    let _ = rig.gpu.reset();
    sh.log(LogKind::Info, "Kurva dikembalikan ke bawaan");
    fail(sh, e);
}

fn stopped(rig: &mut Rig, sh: &Shared, s: &mut Session) {
    let _ = rig.gpu.reset();
    s.testing = None;
    s.save();
    sh.view.write().session = Some(s.clone());
    sh.log(LogKind::Info, "Dihentikan. Kurva dikembalikan ke bawaan; sesi bisa dilanjutkan nanti.");
    sh.set_phase(Phase::Stopped, "Dihentikan");
}

/// Dipanggil saat aplikasi dibuka: bila sesi terakhir berhenti di tengah uji, berarti PC crash,
/// restart, atau aplikasi ditutup paksa saat uji itu. Uji tersebut dicatat gagal.
pub fn recover_crashed_session() -> Option<Session> {
    let mut s = Session::load()?;
    if s.finished {
        return None;
    }
    if let Some(p) = s.testing.take() {
        s.results.push(TestResult {
            kind: p.kind,
            pos: p.pos,
            volt_mv: p.volt_mv,
            target_mhz: p.target_mhz,
            passed: false,
            reason: "PC crash/restart atau aplikasi ditutup paksa saat uji".into(),
            secs: 0,
            measure: None,
        });
        s.save();
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volt::gpu::sim;

    fn run(seed: u64, bonus: f32) -> (Arc<Shared>, Session) {
        let (mut rig, _w) = sim::rig(seed, bonus, 0.0);
        let sh = Shared::new();
        let dir = std::env::temp_dir().join(format!("pacuvolt-test-{seed}"));
        std::env::set_var("APPDATA", &dir);
        tune(&mut rig, &sh, Settings::default(), None);
        let s = sh.view.read().session.clone().unwrap();
        (sh, s)
    }

    #[test]
    fn sim_search_finds_stable_undervolt() {
        let (sh, s) = run(11, 150.0);
        let v = sh.view.read();
        for l in &v.log {
            eprintln!("{} {:?} {}", l.time, l.kind, l.text);
        }
        assert_eq!(v.phase(), Phase::Done, "{}", v.headline);
        let p = v.profile.clone().unwrap();
        let base = s.baseline.clone().unwrap();
        assert!(p.volt_mv < base.volt_mv - 40.0, "undervolt terlalu kecil: {} vs {}", p.volt_mv, base.volt_mv);
        assert!(p.result.unwrap().power_w < base.power_w);
    }
}
