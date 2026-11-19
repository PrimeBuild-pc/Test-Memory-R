//! Which of a run's threads measure a latency step (TODO 119). Every thread the run was set up with, as ever (`latency-threads=all`, the
//! default); `clean`, as many to a physical L3 as still leave the chase an estimated `latency-hits=` share in the L3 (one thread for
//! DRAM), the rest sitting out; or a count. `latency-cores=` keeps the measurers to one core type. The run's threads, their CPUs, pinning
//! and memory stay as the run chose them: a step's plan only picks among them, once, before its workers start.
//!
//! Why: a latency sample is the mean of 1000 chained loads, so its P50 is a blend of the tiers the chase lands in. With every thread
//! chasing, each thread's share of a shared L3 is small beside its L2 (1.0-1.4x on Zen 4 and the Xeon), and Lat-L3-Read reads mostly
//! the L2. Fewer threads to an L3 give each a block far past its L2 (`doc/todo/119-summary-grid.md`).

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::cpu_topology::{CoreType, L3Group};

/// `latency-hits=`'s default: an estimated 88% of the chase in the L3 reads within about 10% of the L3's own latency (the blend model)
pub const DEFAULT_HITS: f64 = 0.88;

/// The DRAM read's clean block: the loaded tests' rule (`loaded::extent_bytes`), so no line of it stays in the L3
const DRAM_MIN_BYTES: usize = 1 << 30;
const DRAM_L3S: usize = 32;

/// How many threads measure a latency step
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LatencyThreads {
    /// Every thread the run was set up with (today's)
    All,
    /// As many to a physical L3 as reach the target share, one for DRAM
    Clean,
    /// Exactly this many, in the run's order
    Count(usize),
}

impl LatencyThreads {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "clean" => Ok(Self::Clean),
            n => match n.parse::<usize>() {
                Ok(n) if n > 0 => Ok(Self::Count(n)),
                _ => Err(format!("Invalid latency-threads '{s}'. Valid: all, clean, or a count from 1")),
            },
        }
    }
}

impl fmt::Display for LatencyThreads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "all"),
            Self::Clean => write!(f, "clean"),
            Self::Count(n) => write!(f, "{n}"),
        }
    }
}

/// Which cores measure first when fewer than all threads do
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyCores {
    /// The run's order, its top-ranked core first
    Ranked,
    /// Performance cores only, as TMR's core classification types them
    Performance,
    /// Efficiency cores only
    Efficiency,
}

impl LatencyCores {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ranked" => Ok(Self::Ranked),
            "performance" => Ok(Self::Performance),
            "efficiency" => Ok(Self::Efficiency),
            _ => Err(format!("Invalid latency-cores '{s}'. Valid: ranked, performance, efficiency")),
        }
    }

    fn admits(self, core_type: CoreType) -> bool {
        match self {
            Self::Ranked => true,
            Self::Performance => matches!(core_type, CoreType::Performance(_)),
            Self::Efficiency => matches!(core_type, CoreType::Efficiency(_)),
        }
    }
}

impl fmt::Display for LatencyCores {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ranked => "ranked",
            Self::Performance => "performance",
            Self::Efficiency => "efficiency",
        })
    }
}

/// A latency read test's settings: the tests that take them (Lat-L1/L2/L3/DRAM/DRAMFull-Read and the `-Clean` variants) have them, every
/// other test none
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub threads: LatencyThreads,
    /// `clean`'s target: the estimated share of the chase in the L3
    pub hits: f64,
    pub cores: LatencyCores,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { threads: LatencyThreads::All, hits: DEFAULT_HITS, cores: LatencyCores::Ranked }
    }
}

impl Settings {
    pub const CLEAN: Settings = Settings { threads: LatencyThreads::Clean, hits: DEFAULT_HITS, cores: LatencyCores::Ranked };

    /// Whether fewer than all of the run's threads might measure: a plan is needed
    pub fn constrains(&self) -> bool {
        self.threads != LatencyThreads::All || self.cores != LatencyCores::Ranked
    }
}

/// The command line's `latency-threads=`, `latency-hits=` and `latency-cores=`, each if given: over a `-Clean` test's own setting too
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Overrides {
    pub threads: Option<LatencyThreads>,
    pub hits: Option<f64>,
    pub cores: Option<LatencyCores>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        self.threads.is_none() && self.hits.is_none() && self.cores.is_none()
    }

    pub fn apply(&self, settings: Settings) -> Settings {
        Settings {
            threads: self.threads.unwrap_or(settings.threads),
            hits: self.hits.unwrap_or(settings.hits),
            cores: self.cores.unwrap_or(settings.cores),
        }
    }
}

/// `latency-hits=`: a share, `88%` or `0.88`, above 0 and below 100%
pub fn parse_hits(s: &str) -> Result<f64, String> {
    let t = s.trim();
    let value = match t.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f64>().map(|p| p / 100.0),
        None => t.parse::<f64>(),
    };
    match value {
        Ok(v) if v > 0.0 && v < 1.0 => Ok(v),
        _ => Err(format!("Invalid latency-hits '{s}'. Valid: a share above 0 and below 100%, as 88% or 0.88")),
    }
}

/// What a step's chain covers, which sizes its measurers' blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// L1 and L2: a core's own, so every eligible thread measures under `clean`, its block the test's own
    Private,
    L3,
    /// A few L3s of memory (Lat-DRAM-Read's): a thinned step's measurers chase the clean block, so the L3 they no longer share holds
    /// none of it
    Dram,
    /// The whole allocation (Lat-DRAMFull-Read's): already past any L3, each measurer keeps the test's own extent
    DramFull,
}

impl Tier {
    pub fn of(extent: &crate::tests::ExtentMode) -> Tier {
        use crate::tests::{CacheTarget, ExtentMode};
        match extent {
            ExtentMode::Cache { target: CacheTarget::L3 { .. } } => Tier::L3,
            ExtentMode::Cache { target: CacheTarget::Dram { .. } } => Tier::Dram,
            ExtentMode::Cache { target: CacheTarget::DRAMFull } | ExtentMode::FullAllocation => Tier::DramFull,
            _ => Tier::Private,
        }
    }
}

/// One of the run's threads, as a step's plan sees it
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub thread: usize,
    pub core: usize,
    pub core_type: CoreType,
    pub l3: L3Group,
    /// Its memory: no block is larger
    pub allocated: usize,
}

/// The sizes a plan sizes blocks by
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The L2 the L3 tests' blocks are floored by: the calibrated one, else CPUID's
    pub l2: usize,
    /// One L3 instance, CPUID's
    pub l3: usize,
}

/// A thread's part in a step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// It measures: over `extent` bytes, or over the test's own extent
    Measures { extent: Option<usize> },
    /// It skips the step: no seal check, no test, its memory untouched
    SitsOut,
}

/// What a results file keeps of a step's plan
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The settings it ran under: `clean`, `all` or a count; and the cores
    pub threads: String,
    pub cores: String,
    pub measurers: usize,
    /// The most measurers sharing one L3
    pub per_l3: usize,
    /// An L3 or DRAM step's: its measurers' block, the smallest when they differ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_bytes: Option<u64>,
    /// An L3 step's: the L2 its estimate is against, and the estimated share of the chase in the L3, the lowest an L3 has
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l2_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l3_share_estimated: Option<f64>,
    /// `clean`'s target, and whether an L3 fell short of it even with one thread
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub target_unmet: bool,
}

/// A step's measurers, built before its workers start
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Each thread's role, by thread id
    pub roles: Vec<Role>,
    pub record: Record,
}

impl Plan {
    pub fn role(&self, thread: usize) -> Role {
        self.roles.get(thread).copied().unwrap_or(Role::SitsOut)
    }
}

/// A whole number of 4 KiB granules, the kernels' unit, at least one
fn granules(bytes: f64) -> usize {
    let granule = crate::test_memory::GRANULE;
    ((bytes.max(0.0) as usize) / granule).max(1) * granule
}

/// A step's plan from its settings over `slots`, the run's threads in its order (its top-ranked core first), or why no thread fits.
/// `reach(k)` is what a thread reaches of its L3 with `k` threads sharing it (`CalibrationResults::l3_reach`, else the L3 over `k`).
pub fn plan(settings: &Settings, tier: Tier, slots: &[Slot], sizes: &Sizes, reach: &dyn Fn(usize) -> f64) -> Result<Plan, String> {
    let admitted: Vec<&Slot> = slots.iter().filter(|s| settings.cores.admits(s.core_type)).collect();
    if admitted.is_empty() {
        return Err(format!("latency-cores={}: none of the run's threads is on a {} core", settings.cores, settings.cores));
    }
    // One thread a core under a count or clean: a core's other thread shares its L1 and L2, and its chase would be the measurer's
    let mut cores = HashSet::new();
    let one_a_core: Vec<&Slot> = admitted.iter().copied().filter(|s| cores.insert(s.core)).collect();
    // A block of an L3 step, with `k` measurers on its L3
    let block = |k: usize| granules((0.5 * reach(k)).max((sizes.l2 + (64 << 10)) as f64));
    let share = |block: usize| 1.0 - sizes.l2 as f64 / block as f64;

    let measurers: Vec<&Slot> = match settings.threads {
        LatencyThreads::All => admitted,
        LatencyThreads::Count(n) if n > one_a_core.len() => {
            return Err(format!("latency-threads={n}: the run has {} eligible cores", one_a_core.len()));
        }
        LatencyThreads::Count(n) => one_a_core[..n].to_vec(),
        LatencyThreads::Clean => match tier {
            Tier::Private => one_a_core,
            Tier::Dram | Tier::DramFull => one_a_core[..1].to_vec(),
            // On each L3, the most that still reach the target, else one
            Tier::L3 => by_l3(&one_a_core)
                .into_iter()
                .flat_map(|on| {
                    let k = (1..=on.len()).rev().find(|&k| share(block(k)) >= settings.hits).unwrap_or(1);
                    on.into_iter().take(k)
                })
                .collect(),
        },
    };

    let on_l3 = by_l3(&measurers);
    let per_l3 = on_l3.iter().map(Vec::len).max().unwrap_or(0);
    let extent_of = |slot: &Slot| -> Option<usize> {
        match tier {
            Tier::Private | Tier::DramFull => None,
            Tier::L3 => {
                let k = on_l3.iter().find(|on| on.iter().any(|s| s.thread == slot.thread)).map_or(1, Vec::len);
                Some(block(k).min(slot.allocated))
            }
            Tier::Dram => Some(granules(DRAM_MIN_BYTES.max(DRAM_L3S * sizes.l3) as f64).min(slot.allocated)),
        }
    };
    let mut roles = vec![Role::SitsOut; slots.iter().map(|s| s.thread + 1).max().unwrap_or(0)];
    for slot in &measurers {
        roles[slot.thread] = Role::Measures { extent: extent_of(slot) };
    }
    let blocks: Vec<usize> = measurers.iter().filter_map(|s| extent_of(s)).collect();
    let l3_share = (tier == Tier::L3).then(|| blocks.iter().map(|&b| share(b)).fold(f64::INFINITY, f64::min)).filter(|s| s.is_finite());
    let clean = settings.threads == LatencyThreads::Clean;
    let record = Record {
        threads: settings.threads.to_string(),
        cores: settings.cores.to_string(),
        measurers: measurers.len(),
        per_l3,
        block_bytes: blocks.iter().min().map(|&b| b as u64),
        l2_bytes: (tier == Tier::L3).then_some(sizes.l2 as u64),
        l3_share_estimated: l3_share,
        target: (clean && tier == Tier::L3).then_some(settings.hits),
        target_unmet: clean && tier == Tier::L3 && l3_share.is_some_and(|s| s < settings.hits),
    };
    Ok(Plan { roles, record })
}

/// `slots` by their L3, each L3's in the run's order, the L3s by their first thread
fn by_l3<'a>(slots: &[&'a Slot]) -> Vec<Vec<&'a Slot>> {
    let mut groups: Vec<(L3Group, Vec<&Slot>)> = Vec::new();
    for slot in slots {
        match groups.iter_mut().find(|(l3, _)| *l3 == slot.l3) {
            Some((_, on)) => on.push(slot),
            None => groups.push((slot.l3, vec![slot])),
        }
    }
    groups.into_iter().map(|(_, on)| on).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1 << 20;

    /// petz007: six Zen 4 cores on one 16 MiB L3, a 1 MiB L2, each with 4 GiB; reach(k) = 15 MiB / k (no anchor C)
    fn petz007() -> (Vec<Slot>, Sizes) {
        let slots = (0..6)
            .map(|thread| Slot { thread, core: thread, core_type: CoreType::Performance(0), l3: (0, 0), allocated: 4 << 30 })
            .collect();
        (slots, Sizes { l2: MIB, l3: 16 * MIB })
    }

    fn reach_of(r1: usize) -> impl Fn(usize) -> f64 {
        move |k| r1 as f64 / k as f64
    }

    /// A full-memory step thinned by a count keeps its own extent, the whole allocation, where a few-L3s step's measurers take the clean
    /// block; both are one thread under clean
    #[test]
    fn a_full_memory_step_keeps_its_own_extent() {
        let ((slots, sizes), reach) = (petz007(), reach_of(15 * MIB));
        let two = Settings { threads: LatencyThreads::Count(2), ..Settings::default() };
        let full = plan(&two, Tier::DramFull, &slots, &sizes, &reach).unwrap();
        assert_eq!(
            (full.role(0), full.role(1), full.role(2)),
            (Role::Measures { extent: None }, Role::Measures { extent: None }, Role::SitsOut)
        );
        assert_eq!(full.record.block_bytes, None);
        // 1 GiB, more than 32 of its 16 MiB L3s
        let few = plan(&two, Tier::Dram, &slots, &sizes, &reach).unwrap();
        assert_eq!(few.role(0), Role::Measures { extent: Some(1 << 30) });
        assert_eq!(plan(&Settings::CLEAN, Tier::DramFull, &slots, &sizes, &reach).unwrap().record.measurers, 1);
        assert_eq!(Tier::of(&crate::tests::ExtentMode::Cache { target: crate::tests::CacheTarget::DRAMFull }), Tier::DramFull);
    }

    /// Clean on petz007: no count reaches 88% (half of 15 MiB is 7.5 MiB, 87%), so one thread, target unmet; DRAM: one thread over 1 GiB
    #[test]
    fn clean_takes_as_many_as_reach_the_target_else_one() {
        let (slots, sizes) = petz007();
        let reach = reach_of(15 * MIB);
        let l3 = plan(&Settings::CLEAN, Tier::L3, &slots, &sizes, &reach).unwrap();
        assert_eq!((l3.record.measurers, l3.record.per_l3, l3.record.target_unmet), (1, 1, true));
        assert_eq!(l3.role(0), Role::Measures { extent: Some(granules(7.5 * MIB as f64)) });
        assert!((1..6).all(|t| l3.role(t) == Role::SitsOut));
        let share = l3.record.l3_share_estimated.unwrap();
        assert!((share - (1.0 - 1.0 / 7.5)).abs() < 1e-3, "{share}");

        // Blocks of 7.5, 3.75 and 2.5 MiB for one, two and three threads: 87%, 73% and 60% in the L3
        let at = |hits: f64| plan(&Settings { hits, ..Settings::CLEAN }, Tier::L3, &slots, &sizes, &reach).unwrap().record;
        assert_eq!((at(0.80).measurers, at(0.70).measurers, at(0.50).measurers), (1, 2, 3));
        assert!(!at(0.70).target_unmet);

        let dram = plan(&Settings::CLEAN, Tier::Dram, &slots, &sizes, &reach).unwrap();
        assert_eq!((dram.record.measurers, dram.role(0)), (1, Role::Measures { extent: Some(1 << 30) }));
        assert_eq!(dram.record.target, None, "DRAM has no L3 share to reach");
    }

    /// Clean on two L3s takes its count on each; a count takes the run's first cores; one a core, a sibling sitting out
    #[test]
    fn clean_counts_each_l3_and_a_count_one_a_core() {
        let slots: Vec<Slot> = (0..8)
            .map(|thread| Slot {
                thread,
                core: thread % 4,
                core_type: CoreType::Performance(0),
                l3: (0, (thread % 4 / 2) as u8),
                allocated: 1 << 30,
            })
            .collect();
        let sizes = Sizes { l2: 512 << 10, l3: 32 * MIB };
        let reach = reach_of(32 * MIB);
        let clean = plan(&Settings::CLEAN, Tier::L3, &slots, &sizes, &reach).unwrap();
        // Cores 0-1 on one L3, 2-3 on the other; threads 4-7 their siblings
        assert_eq!(clean.record.per_l3, 2);
        assert!((0..4).all(|t| matches!(clean.role(t), Role::Measures { .. })) && (4..8).all(|t| clean.role(t) == Role::SitsOut));
        let three =
            plan(&Settings { threads: LatencyThreads::Count(3), ..Settings::default() }, Tier::Private, &slots, &sizes, &reach).unwrap();
        assert_eq!((three.record.measurers, three.role(0), three.role(3)), (3, Role::Measures { extent: None }, Role::SitsOut));
        let too_many = plan(&Settings { threads: LatencyThreads::Count(5), ..Settings::default() }, Tier::Private, &slots, &sizes, &reach);
        assert!(too_many.unwrap_err().contains("4 eligible cores"));
    }

    /// `latency-cores=efficiency` keeps to the efficiency cores, in the run's order; with none, the step is refused, never run elsewhere
    #[test]
    fn a_core_type_filter_or_its_refusal() {
        let (mut slots, sizes) = petz007();
        for slot in &mut slots[4..] {
            slot.core_type = CoreType::Efficiency(0);
        }
        let reach = reach_of(15 * MIB);
        let efficiency = Settings { cores: LatencyCores::Efficiency, ..Settings::default() };
        let all = plan(&efficiency, Tier::Private, &slots, &sizes, &reach).unwrap();
        assert_eq!(all.roles, [
            Role::SitsOut,
            Role::SitsOut,
            Role::SitsOut,
            Role::SitsOut,
            Role::Measures { extent: None },
            Role::Measures { extent: None }
        ]);
        let (uniform, _) = petz007();
        assert!(plan(&efficiency, Tier::Private, &uniform, &sizes, &reach).unwrap_err().contains("efficiency"));
    }

    /// A block never outgrows the thread's memory
    #[test]
    fn a_block_is_capped_by_the_threads_memory() {
        let (mut slots, sizes) = petz007();
        slots[0].allocated = 256 * MIB;
        let dram = plan(&Settings::CLEAN, Tier::Dram, &slots, &sizes, &reach_of(15 * MIB)).unwrap();
        assert_eq!(dram.role(0), Role::Measures { extent: Some(256 * MIB) });
    }

    #[test]
    fn the_settings_parse() {
        assert_eq!(LatencyThreads::parse("Clean"), Ok(LatencyThreads::Clean));
        assert_eq!(LatencyThreads::parse("3"), Ok(LatencyThreads::Count(3)));
        assert!(LatencyThreads::parse("0").is_err());
        assert_eq!(parse_hits("88%"), Ok(0.88));
        assert_eq!(parse_hits("0.5"), Ok(0.5));
        assert!(parse_hits("100%").is_err() && parse_hits("0").is_err());
        assert_eq!(LatencyCores::parse("efficiency"), Ok(LatencyCores::Efficiency));
        let over = Overrides { hits: Some(0.9), ..Default::default() }.apply(Settings::CLEAN);
        assert_eq!((over.threads, over.hits), (LatencyThreads::Clean, 0.9));
    }
}
