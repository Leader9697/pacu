//! Penyeimbang CPU: meredam proses latar yang memakan CPU berlebihan supaya aplikasi
//! di depan (terutama game) tetap responsif, lalu memulihkannya saat sudah tenang.
//!
//! Logika di sini murni (tanpa panggilan Windows) sehingga bisa diuji di platform apa pun.

use crate::config::{BalancerConfig, Priority};
use std::collections::{HashMap, HashSet};

/// Ambang default menurut jumlah CPU logis (indeks 0 = 1 CPU, lebih dari 32 memakai nilai terakhir).
const TOTAL: [u8; 32] = [
    75, 45, 40, 23, 18, 18, 17, 11, 11, 10, 10, 10, 10, 10, 10, 10, 9, 9, 9, 9, 9, 8, 8, 8, 8, 8,
    7, 7, 7, 7, 7, 7,
];
const PER_PROCESS: [u8; 32] = [
    64, 32, 20, 15, 10, 10, 9, 8, 8, 8, 7, 7, 6, 6, 5, 5, 5, 5, 5, 4, 4, 4, 4, 4, 3, 3, 3, 3, 3, 2,
    2, 2,
];
const RESTORE: [u8; 32] = [
    15, 7, 5, 5, 5, 5, 5, 4, 3, 3, 3, 3, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub total: f32,
    pub per_process: f32,
    pub restore: f32,
    pub time_over_ms: u64,
    pub min_restraint_ms: u64,
    pub max_restraint_ms: u64,
}

fn table(t: &[u8; 32], cpus: u32) -> f32 {
    t[(cpus.clamp(1, 32) - 1) as usize] as f32
}

/// Ambang otomatis. Di CPU hybrid, E-core tidak dihitung, ambang per proses dilonggarkan
/// dua kali dan waktu tunggu tiga kali, karena E-core sudah menampung beban latar.
pub fn auto_thresholds(logical: u32, ecores: u32) -> Thresholds {
    let hybrid = ecores > 0 && ecores < logical;
    let cpus = if hybrid { logical - ecores } else { logical };
    if cpus == 0 {
        return Thresholds {
            total: 70.0,
            per_process: 20.0,
            restore: 5.0,
            time_over_ms: 900,
            min_restraint_ms: 4200,
            max_restraint_ms: 0,
        };
    }
    Thresholds {
        total: if hybrid { 70.0 } else { table(&TOTAL, cpus) },
        per_process: table(&PER_PROCESS, cpus) * if hybrid { 2.0 } else { 1.0 },
        restore: table(&RESTORE, cpus),
        time_over_ms: if hybrid { 2700 } else { 900 },
        min_restraint_ms: if hybrid { 2100 } else { 4200 },
        max_restraint_ms: 0,
    }
}

/// Ambang yang berlaku: setelan pengguna menimpa nilai otomatis; saat Mode Game dengan
/// peredaman agresif, ambang total dan waktu tunggu dipotong setengah.
pub fn effective_thresholds(cfg: &BalancerConfig, logical: u32, ecores: u32, aggressive: bool) -> Thresholds {
    let auto = auto_thresholds(logical, ecores);
    let mut t = Thresholds {
        total: cfg.total_threshold.unwrap_or(auto.total),
        per_process: cfg.per_process_threshold.unwrap_or(auto.per_process),
        restore: cfg.restore_threshold.unwrap_or(auto.restore),
        time_over_ms: cfg.time_over_ms.unwrap_or(auto.time_over_ms),
        min_restraint_ms: cfg.min_restraint_ms.unwrap_or(auto.min_restraint_ms),
        max_restraint_ms: cfg.max_restraint_ms,
    };
    if aggressive {
        t.total *= 0.5;
        t.time_over_ms /= 2;
    }
    t
}

/// Identitas proses yang aman dari daur ulang PID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcKey {
    pub pid: u32,
    pub create_time: u64,
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub key: ProcKey,
    /// Pemakaian CPU sebagai persen dari seluruh CPU (0-100).
    pub cpu: f32,
    pub priority: Priority,
    /// Proses ini tidak boleh diredam (foreground, layanan, pengecualian, game, dsb.).
    pub protected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Restrain { key: ProcKey, from: Priority, to: Option<Priority> },
    Restore { key: ProcKey, original: Priority, restrained: Option<Priority>, reason: RestoreReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreReason {
    Calm,
    Protected,
    MaxTime,
    Disabled,
}

impl RestoreReason {
    pub fn label(self) -> &'static str {
        match self {
            RestoreReason::Calm => "pemakaian CPU sudah turun",
            RestoreReason::Protected => "sekarang di depan atau dilindungi",
            RestoreReason::MaxTime => "batas waktu redam tercapai",
            RestoreReason::Disabled => "penyeimbang dimatikan",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Restraint {
    pub since_ms: u64,
    pub original: Priority,
    pub restrained: Option<Priority>,
}

#[derive(Default)]
pub struct Balancer {
    over_since: HashMap<ProcKey, u64>,
    restrained: HashMap<ProcKey, Restraint>,
    pub total_restraints: u64,
}

impl Balancer {
    pub fn is_restrained(&self, key: &ProcKey) -> bool {
        self.restrained.contains_key(key)
    }

    pub fn restrained(&self) -> impl Iterator<Item = (&ProcKey, &Restraint)> {
        self.restrained.iter()
    }

    /// Satu siklus keputusan. `total_cpu` adalah pemakaian CPU seluruh sistem (%).
    pub fn tick(
        &mut self,
        now_ms: u64,
        total_cpu: f32,
        samples: &[Sample],
        cfg: &BalancerConfig,
        th: &Thresholds,
        active: bool,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let alive: HashSet<ProcKey> = samples.iter().map(|s| s.key).collect();
        self.over_since.retain(|k, _| alive.contains(k));
        self.restrained.retain(|k, _| alive.contains(k));

        if !active {
            self.over_since.clear();
            for (key, r) in self.restrained.drain() {
                actions.push(Action::Restore {
                    key,
                    original: r.original,
                    restrained: r.restrained,
                    reason: RestoreReason::Disabled,
                });
            }
            return actions;
        }

        for s in samples {
            if let Some(r) = self.restrained.get(&s.key) {
                let held = now_ms.saturating_sub(r.since_ms);
                let reason = if s.protected {
                    Some(RestoreReason::Protected)
                } else if th.max_restraint_ms > 0 && held >= th.max_restraint_ms {
                    Some(RestoreReason::MaxTime)
                } else if s.cpu < th.restore && held >= th.min_restraint_ms {
                    Some(RestoreReason::Calm)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    let r = self.restrained.remove(&s.key).unwrap();
                    actions.push(Action::Restore {
                        key: s.key,
                        original: r.original,
                        restrained: r.restrained,
                        reason,
                    });
                }
                continue;
            }

            let eligible = !s.protected
                && (!cfg.only_normal_priority || s.priority == Priority::Normal)
                && s.priority != Priority::Idle;
            if eligible && total_cpu > th.total && s.cpu > th.per_process {
                let since = *self.over_since.entry(s.key).or_insert(now_ms);
                if now_ms.saturating_sub(since) >= th.time_over_ms {
                    self.over_since.remove(&s.key);
                    let to = if cfg.lower_to_idle {
                        Some(Priority::Idle)
                    } else if s.priority > Priority::BelowNormal {
                        Some(Priority::BelowNormal)
                    } else {
                        None
                    };
                    self.restrained.insert(
                        s.key,
                        Restraint { since_ms: now_ms, original: s.priority, restrained: to },
                    );
                    self.total_restraints += 1;
                    actions.push(Action::Restrain { key: s.key, from: s.priority, to });
                }
            } else {
                self.over_since.remove(&s.key);
            }
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pid: u32) -> ProcKey {
        ProcKey { pid, create_time: 1 }
    }

    fn sample(pid: u32, cpu: f32) -> Sample {
        Sample { key: key(pid), cpu, priority: Priority::Normal, protected: false }
    }

    #[test]
    fn auto_table() {
        let t = auto_thresholds(8, 0);
        assert_eq!((t.total, t.per_process, t.restore), (11.0, 8.0, 4.0));
        assert_eq!((t.time_over_ms, t.min_restraint_ms), (900, 4200));
        let t = auto_thresholds(64, 0);
        assert_eq!((t.total, t.per_process, t.restore), (7.0, 2.0, 1.0));
        // 12 thread P-core + 8 E-core.
        let t = auto_thresholds(20, 8);
        assert_eq!((t.total, t.per_process, t.restore), (70.0, 14.0, 3.0));
        assert_eq!((t.time_over_ms, t.min_restraint_ms), (2700, 2100));
    }

    #[test]
    fn restrain_after_time_then_restore_when_calm() {
        let cfg = BalancerConfig::default();
        let th = auto_thresholds(8, 0);
        let mut b = Balancer::default();
        assert!(b.tick(0, 50.0, &[sample(10, 30.0)], &cfg, &th, true).is_empty());
        assert!(b.tick(500, 50.0, &[sample(10, 30.0)], &cfg, &th, true).is_empty());
        let a = b.tick(1000, 50.0, &[sample(10, 30.0)], &cfg, &th, true);
        assert_eq!(
            a,
            vec![Action::Restrain { key: key(10), from: Priority::Normal, to: Some(Priority::BelowNormal) }]
        );
        // Sudah tenang tapi belum lewat waktu minimum.
        assert!(b.tick(2000, 5.0, &[sample(10, 1.0)], &cfg, &th, true).is_empty());
        let a = b.tick(5300, 5.0, &[sample(10, 1.0)], &cfg, &th, true);
        assert!(matches!(a[0], Action::Restore { reason: RestoreReason::Calm, .. }));
    }

    #[test]
    fn spike_resets_counter() {
        let cfg = BalancerConfig::default();
        let th = auto_thresholds(8, 0);
        let mut b = Balancer::default();
        b.tick(0, 50.0, &[sample(10, 30.0)], &cfg, &th, true);
        b.tick(500, 50.0, &[sample(10, 2.0)], &cfg, &th, true);
        assert!(b.tick(1000, 50.0, &[sample(10, 30.0)], &cfg, &th, true).is_empty());
    }

    #[test]
    fn protected_and_non_normal_are_skipped() {
        let cfg = BalancerConfig::default();
        let th = auto_thresholds(8, 0);
        let mut b = Balancer::default();
        let mut p = sample(1, 90.0);
        p.protected = true;
        let mut h = sample(2, 90.0);
        h.priority = Priority::High;
        for t in [0, 1000, 2000] {
            assert!(b.tick(t, 95.0, &[p.clone(), h.clone()], &cfg, &th, true).is_empty());
        }
    }

    #[test]
    fn foreground_restores_immediately() {
        let cfg = BalancerConfig::default();
        let th = auto_thresholds(8, 0);
        let mut b = Balancer::default();
        b.tick(0, 50.0, &[sample(10, 30.0)], &cfg, &th, true);
        b.tick(1000, 50.0, &[sample(10, 30.0)], &cfg, &th, true);
        let mut s = sample(10, 30.0);
        s.protected = true;
        let a = b.tick(1100, 50.0, &[s], &cfg, &th, true);
        assert!(matches!(a[0], Action::Restore { reason: RestoreReason::Protected, .. }));
    }

    #[test]
    fn disabling_restores_everything_and_exited_is_forgotten() {
        let cfg = BalancerConfig::default();
        let th = auto_thresholds(8, 0);
        let mut b = Balancer::default();
        b.tick(0, 50.0, &[sample(10, 30.0), sample(11, 30.0)], &cfg, &th, true);
        b.tick(1000, 50.0, &[sample(10, 30.0), sample(11, 30.0)], &cfg, &th, true);
        // Proses 11 keluar.
        b.tick(1500, 50.0, &[sample(10, 30.0)], &cfg, &th, true);
        let a = b.tick(1600, 50.0, &[sample(10, 30.0)], &cfg, &th, false);
        assert_eq!(a.len(), 1);
        assert!(matches!(a[0], Action::Restore { reason: RestoreReason::Disabled, .. }));
    }

    #[test]
    fn aggressive_mode_halves_total_and_wait() {
        let cfg = BalancerConfig::default();
        let t = effective_thresholds(&cfg, 8, 0, true);
        assert_eq!((t.total, t.time_over_ms), (5.5, 450));
    }
}
