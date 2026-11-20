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

/// A latency test's settings: the tests that take them (Lat-L1/L2/L3/DRAM/DRAMFull-Read, the `-Clean` variants and memory's write
/// latency, Lat-DRAM-NTStore-SFence) have them, every other test none
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

/// Which threads a step's settings choose among, by what its test measures: the test's own, given where it is registered
/// (`TestMemoryConfig::with_latency`), never read off its extent, which a config can write as it likes (TODO 119)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// L1 and L2: a core's own, so every eligible core measures under `clean`
    Private,
    /// The L3: under `clean`, as many to an L3 as reach the target
    L3,
    /// Memory: one thread under `clean`
    Dram,
    /// The whole memory: one thread under `clean`
    DramFull,
}

/// What a step's extent asks of its measurers' blocks, read off its config (`runner::plan_latency_steps`). The extent sizes the block and
/// never chooses who measures: a config's 1 GiB outright and its whole memory of 1 GiB plan alike.
pub enum Block<'a> {
    /// An L1 or L2 share: its size with `n` of a core's threads measuring, a sibling that sits out not sharing it
    Private(&'a dyn Fn(usize) -> usize),
    /// An L3 share: `scale` of what a thread reaches with the measurers on its L3, at least the L2 each has plus 64 KiB
    L3 { scale: f64 },
    /// A few L3s (a DRAM target's): the clean block, max(1 GiB, 32 L3s), as much of it as the measurer's memory holds, a preference
    CleanDram,
    /// A size given outright, a config's: the block, which a measurer must hold whole
    Exactly(usize),
    /// The measurer's whole memory
    Whole,
}

impl Block<'_> {
    fn describe(&self) -> String {
        match self {
            Block::Private(_) => "an L1 or L2 share".to_string(),
            Block::L3 { .. } => "an L3 share".to_string(),
            Block::CleanDram => "a DRAM share".to_string(),
            Block::Exactly(b) => format!("{} outright", bytes(*b as u64)),
            Block::Whole => "the whole memory".to_string(),
        }
    }
}

/// One of the run's threads, as a step's plan sees it
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub thread: usize,
    /// The logical CPU it is pinned to; none unpinned
    pub cpu: Option<usize>,
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
    /// What one of two SMT siblings has of it with both running, as calibrated (TODO 22's anchor B); half the L2 without
    pub l2_smt: Option<usize>,
    /// One L3 instance, CPUID's
    pub l3: usize,
}

impl Sizes {
    /// The L2 each of `n` measurers on one core has, as `CacheTarget::L3`'s floor takes it
    fn l2_each(&self, n: usize) -> usize {
        match n {
            0 | 1 => self.l2,
            2 => self.l2_smt.unwrap_or(self.l2 / 2),
            n => self.l2 / n,
        }
    }
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
    /// The most measurers sharing one L3, and one core: 1 under a count or clean, a core's every thread under `all` with a core filter
    pub per_l3: usize,
    #[serde(default = "one")]
    pub per_core: usize,
    /// The measurers' logical CPUs, in thread order; none unpinned
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cpus: Vec<usize>,
    /// Its measurers' block, the smallest when they differ; none for a step that keeps its own extent
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
    /// Each thread's memory, by thread id: a measurer's layout is resolved over its own (`TestConfigSnapshot::capture`)
    pub allocated: Vec<usize>,
    pub record: Record,
}

/// A size as the reports give it: `1.00 GiB`
fn bytes(b: u64) -> String {
    use crate::reporting::formatters::ReportFormatter;
    crate::reporting::formatters::DefaultFormatter::new().format_bytes(b)
}

/// What a record's two runs must share to have measured a step alike: everything but the estimate, which follows from the rest
type Measuring<'a> = (&'a str, &'a str, usize, usize, usize, &'a [usize], Option<u64>);

impl Record {
    /// What decides whether two runs measured a step alike (`run_context::setting_changes`): the settings, how many measured, where,
    /// and the block to the byte
    pub fn measuring(&self) -> Measuring<'_> {
        (&self.threads, &self.cores, self.measurers, self.per_l3, self.per_core, &self.cpus, self.block_bytes)
    }

    /// Who measured, for a comparison of two runs: `1 thread (latency-threads=clean) on CPU 0, 1 an L3, over 1.00 GiB`, the cores when a
    /// filter chose them, the most on a core when two shared one; with `exact`, the block's bytes too, when two blocks differ by less
    /// than their sizes show
    pub fn describe(&self, exact: bool) -> String {
        let threads = format!("{} thread{}", self.measurers, if self.measurers == 1 { "" } else { "s" });
        let cores = if self.cores == "ranked" { String::new() } else { format!(" latency-cores={}", self.cores) };
        let on = match self.cpus.as_slice() {
            [] => String::new(),
            [cpu] => format!(" on CPU {cpu}"),
            cpus => format!(" on CPUs {}", cpus.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")),
        };
        let per_core = if self.per_core > 1 { format!(", {} a core", self.per_core) } else { String::new() };
        let block = self
            .block_bytes
            .map_or(String::new(), |b| if exact { format!(", over {} ({b} B)", bytes(b)) } else { format!(", over {}", bytes(b)) });
        format!("{threads} (latency-threads={}{cores}){on}, {} an L3{per_core}{block}", self.threads, self.per_l3)
    }
}

impl Plan {
    pub fn role(&self, thread: usize) -> Role {
        self.roles.get(thread).copied().unwrap_or(Role::SitsOut)
    }
}

fn one() -> usize {
    1
}

/// A whole number of 4 KiB granules, the kernels' unit, at least one
fn granules(bytes: f64) -> usize {
    let granule = crate::test_memory::GRANULE;
    ((bytes.max(0.0) as usize) / granule).max(1) * granule
}

/// A step's plan from its settings over `slots`, the run's threads in its order (its top-ranked core first), or why no thread fits. Who
/// measures is its test's `tier`'s and the settings'; `block` is what its extent asks of each measurer's block. `reach(k)` is what a
/// thread reaches of its L3 with `k` threads sharing it (`CalibrationResults::l3_reach`, else the L3 over `k`). The blocks follow the
/// measurers, never the run's threads: an L1 or L2 block counts the siblings measuring on its core, an L3 block the measurers on its L3.
pub fn plan(
    settings: &Settings, tier: Tier, block: &Block, slots: &[Slot], sizes: &Sizes, reach: &dyn Fn(usize) -> f64,
) -> Result<Plan, String> {
    let admitted: Vec<&Slot> = slots.iter().filter(|s| settings.cores.admits(s.core_type)).collect();
    if admitted.is_empty() {
        return Err(format!("latency-cores={}: none of the run's threads is on a {} core", settings.cores, settings.cores));
    }
    // A block given outright is held whole: a thread with less memory is passed over for the next that holds it, never given less
    let need = if let Block::Exactly(b) = block { *b } else { 0 };
    let eligible: Vec<&Slot> = admitted.iter().copied().filter(|s| s.allocated >= need).collect();
    if eligible.is_empty() {
        let most = admitted.iter().map(|s| s.allocated).max().unwrap_or(0);
        return Err(format!(
            "its extent is {} a measurer, and the most memory an eligible thread has is {}: give it a smaller extent, or more memory=",
            bytes(need as u64),
            bytes(most as u64)
        ));
    }
    // One thread a core under a count or clean: a core's other thread shares its L1 and L2, and its chase would be the measurer's
    let mut cores = HashSet::new();
    let one_a_core: Vec<&Slot> = eligible.iter().copied().filter(|s| cores.insert(s.core)).collect();
    // An L3 step's block, `scale` of the reach with `k` measurers on its L3 and `n` on the measurer's core, floored by the L2 each of
    // those has; and the estimated share of its chase in the L3
    let l3_block = |scale: f64, k: usize, n: usize| granules((scale * reach(k)).max((sizes.l2_each(n) + (64 << 10)) as f64));
    let share = |block: usize, n: usize| 1.0 - sizes.l2_each(n) as f64 / block as f64;

    let measurers: Vec<&Slot> = match settings.threads {
        LatencyThreads::All => eligible,
        LatencyThreads::Count(n) if n > one_a_core.len() => {
            return Err(format!("latency-threads={n}: the run has {} eligible cores", one_a_core.len()));
        }
        LatencyThreads::Count(n) => one_a_core[..n].to_vec(),
        LatencyThreads::Clean => match (tier, block) {
            (Tier::Private, _) => one_a_core,
            (Tier::Dram | Tier::DramFull, _) => one_a_core[..1].to_vec(),
            (Tier::L3, Block::L3 { scale }) => by_l3(&one_a_core)
                .into_iter()
                .flat_map(|on| {
                    // On each L3, the most that reach the target, each holding its block whole: a thread with less memory is passed
                    // over, not given a smaller block that would miss it. Else one, the first that holds one thread's block
                    let most = (1..=on.len()).rev().find_map(|k| {
                        let b = l3_block(*scale, k, 1);
                        let holding: Vec<&Slot> = on.iter().copied().filter(|s| s.allocated >= b).take(k).collect();
                        (share(b, 1) >= settings.hits && holding.len() == k).then_some(holding)
                    });
                    most.unwrap_or_else(|| {
                        let b = l3_block(*scale, 1, 1);
                        vec![on.iter().copied().find(|s| s.allocated >= b).unwrap_or(on[0])]
                    })
                })
                .collect(),
            (Tier::L3, other) => {
                return Err(format!(
                    "latency-threads=clean sizes an L3 test's block from its L3, and its extent is {}: give it an L3 share (\"L3/2\"), \
                     or another latency-threads=",
                    other.describe()
                ));
            }
        },
    };

    let on_l3 = by_l3(&measurers);
    let per_l3 = on_l3.iter().map(Vec::len).max().unwrap_or(0);
    // The measurers on a measurer's core, itself among them
    let on_core = |slot: &Slot| measurers.iter().filter(|s| s.core == slot.core).count().max(1);
    let per_core = measurers.iter().map(|s| on_core(s)).max().unwrap_or(1);
    let extent_of = |slot: &Slot| -> Option<usize> {
        match (tier, block) {
            (_, Block::Exactly(b)) => Some(granules(*b as f64)),
            (_, Block::Whole) => Some(granules(slot.allocated as f64)),
            (Tier::Private, Block::Private(size)) => Some(granules(size(on_core(slot)) as f64).min(slot.allocated)),
            (Tier::L3, Block::L3 { scale }) => {
                let k = on_l3.iter().find(|on| on.iter().any(|s| s.thread == slot.thread)).map_or(1, Vec::len);
                Some(l3_block(*scale, k, on_core(slot)).min(slot.allocated))
            }
            (Tier::Dram | Tier::DramFull, Block::CleanDram) => {
                Some(granules(DRAM_MIN_BYTES.max(DRAM_L3S * sizes.l3) as f64).min(slot.allocated))
            }
            // Another tier's share, as a config can give one (an L3 share on a DRAM test): the step's own extent
            _ => None,
        }
    };
    let threads = slots.iter().map(|s| s.thread + 1).max().unwrap_or(0);
    let mut roles = vec![Role::SitsOut; threads];
    let mut allocated = vec![0; threads];
    for slot in slots {
        allocated[slot.thread] = slot.allocated;
    }
    for slot in &measurers {
        roles[slot.thread] = Role::Measures { extent: extent_of(slot) };
    }
    let blocks: Vec<usize> = measurers.iter().filter_map(|s| extent_of(s)).collect();
    let l3_share = (tier == Tier::L3)
        .then(|| measurers.iter().filter_map(|s| Some(share(extent_of(s)?, on_core(s)))).fold(f64::INFINITY, f64::min))
        .filter(|s| s.is_finite());
    let clean = settings.threads == LatencyThreads::Clean;
    let record = Record {
        threads: settings.threads.to_string(),
        cores: settings.cores.to_string(),
        measurers: measurers.len(),
        per_l3,
        per_core,
        cpus: measurers.iter().filter_map(|s| s.cpu).collect(),
        block_bytes: blocks.iter().min().map(|&b| b as u64),
        l2_bytes: (tier == Tier::L3).then_some(sizes.l2 as u64),
        l3_share_estimated: l3_share,
        target: (clean && tier == Tier::L3).then_some(settings.hits),
        target_unmet: clean && tier == Tier::L3 && l3_share.is_some_and(|s| s < settings.hits),
    };
    Ok(Plan { roles, allocated, record })
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
    const GIB: usize = 1 << 30;
    const L3_HALF: Block<'static> = Block::L3 { scale: 0.5 };

    fn slot(thread: usize, core: usize, l3: u8, allocated: usize) -> Slot {
        Slot { thread, cpu: Some(thread), core, core_type: CoreType::Performance(0), l3: (0, l3), allocated }
    }

    /// petz007: six Zen 4 cores on one 16 MiB L3, a 1 MiB L2, each with 4 GiB; reach(k) = 15 MiB / k (no anchor C)
    fn petz007() -> (Vec<Slot>, Sizes) {
        ((0..6).map(|t| slot(t, t, 0, 4 * GIB)).collect(), Sizes { l2: MIB, l2_smt: None, l3: 16 * MIB })
    }

    fn reach_of(r1: usize) -> impl Fn(usize) -> f64 {
        move |k| r1 as f64 / k as f64
    }

    /// Clean on petz007: no count reaches 88% (half of 15 MiB is 7.5 MiB, 87%), so one thread, target unmet; DRAM: one thread over 1 GiB
    #[test]
    fn clean_takes_as_many_as_reach_the_target_else_one() {
        let (slots, sizes) = petz007();
        let reach = reach_of(15 * MIB);
        let l3 = plan(&Settings::CLEAN, Tier::L3, &L3_HALF, &slots, &sizes, &reach).unwrap();
        assert_eq!((l3.record.measurers, l3.record.per_l3, l3.record.target_unmet), (1, 1, true));
        assert_eq!(l3.role(0), Role::Measures { extent: Some(granules(7.5 * MIB as f64)) });
        assert!((1..6).all(|t| l3.role(t) == Role::SitsOut));
        let share = l3.record.l3_share_estimated.unwrap();
        assert!((share - (1.0 - 1.0 / 7.5)).abs() < 1e-3, "{share}");

        // Blocks of 7.5, 3.75 and 2.5 MiB for one, two and three threads: 87%, 73% and 60% in the L3
        let at = |hits: f64| plan(&Settings { hits, ..Settings::CLEAN }, Tier::L3, &L3_HALF, &slots, &sizes, &reach).unwrap().record;
        assert_eq!((at(0.80).measurers, at(0.70).measurers, at(0.50).measurers), (1, 2, 3));
        assert!(!at(0.70).target_unmet);

        let dram = plan(&Settings::CLEAN, Tier::Dram, &Block::CleanDram, &slots, &sizes, &reach).unwrap();
        assert_eq!((dram.record.measurers, dram.role(0), dram.record.cpus.as_slice()), (1, Role::Measures { extent: Some(GIB) }, &[0][..]));
        assert_eq!(dram.record.target, None, "DRAM has no L3 share to reach");
        assert_eq!(dram.allocated, [4 * GIB; 6]);
    }

    /// An L3 step's block is its own scale of the reach: `L3/4` half `L3/2`'s at a count, and under clean the count that scale allows
    #[test]
    fn an_l3_steps_scale_sizes_its_block() {
        let (slots, sizes) = petz007();
        let reach = reach_of(64 * MIB);
        let two = Settings { threads: LatencyThreads::Count(2), ..Settings::default() };
        let block = |scale: f64| plan(&two, Tier::L3, &Block::L3 { scale }, &slots, &sizes, &reach).unwrap().record.block_bytes;
        assert_eq!((block(0.5), block(0.25)), (Some(16 * MIB as u64), Some(8 * MIB as u64)));
        // 32/k MiB at half the reach meets 88% (8.33 MiB) up to three threads; 16/k MiB at a quarter, one
        let clean = |scale: f64| plan(&Settings::CLEAN, Tier::L3, &Block::L3 { scale }, &slots, &sizes, &reach).unwrap().record.measurers;
        assert_eq!((clean(0.5), clean(0.25)), (3, 1));
    }

    /// Clean on two L3s takes its count on each; a count takes the run's first cores; one a core, a sibling sitting out
    #[test]
    fn clean_counts_each_l3_and_a_count_one_a_core() {
        let slots: Vec<Slot> = (0..8).map(|t| slot(t, t % 4, (t % 4 / 2) as u8, GIB)).collect();
        let sizes = Sizes { l2: 512 << 10, l2_smt: None, l3: 32 * MIB };
        let reach = reach_of(32 * MIB);
        let clean = plan(&Settings::CLEAN, Tier::L3, &L3_HALF, &slots, &sizes, &reach).unwrap();
        // Cores 0-1 on one L3, 2-3 on the other; threads 4-7 their siblings
        assert_eq!(clean.record.per_l3, 2);
        assert!((0..4).all(|t| matches!(clean.role(t), Role::Measures { .. })) && (4..8).all(|t| clean.role(t) == Role::SitsOut));
        let quarter_l2 = |n: usize| (256 << 10) / n;
        let three = Settings { threads: LatencyThreads::Count(3), ..Settings::default() };
        let three = plan(&three, Tier::Private, &Block::Private(&quarter_l2), &slots, &sizes, &reach).unwrap();
        assert_eq!((three.record.measurers, three.role(0), three.role(3)), (3, Role::Measures { extent: Some(256 << 10) }, Role::SitsOut));
        let five = Settings { threads: LatencyThreads::Count(5), ..Settings::default() };
        assert!(plan(&five, Tier::Private, &Block::Private(&quarter_l2), &slots, &sizes, &reach).unwrap_err().contains("4 eligible cores"));
    }

    /// `latency-cores=efficiency` keeps to the efficiency cores, in the run's order; with none, the step is refused, never run elsewhere
    #[test]
    fn a_core_type_filter_or_its_refusal() {
        let (mut slots, sizes) = petz007();
        for slot in &mut slots[4..] {
            slot.core_type = CoreType::Efficiency(0);
        }
        let reach = reach_of(15 * MIB);
        let half_l2 = |n: usize| (512 << 10) / n;
        let efficiency = Settings { cores: LatencyCores::Efficiency, ..Settings::default() };
        let all = plan(&efficiency, Tier::Private, &Block::Private(&half_l2), &slots, &sizes, &reach).unwrap();
        let measures = Role::Measures { extent: Some(512 << 10) };
        assert_eq!(all.roles, [Role::SitsOut, Role::SitsOut, Role::SitsOut, Role::SitsOut, measures, measures]);
        assert_eq!(all.record.cpus, [4, 5]);
        let (uniform, _) = petz007();
        assert!(plan(&efficiency, Tier::Private, &Block::Private(&half_l2), &uniform, &sizes, &reach).unwrap_err().contains("efficiency"));
    }

    /// The clean DRAM block is a preference: as much of it as the measurer's memory holds
    #[test]
    fn the_clean_dram_block_is_capped_by_the_threads_memory() {
        let (mut slots, sizes) = petz007();
        slots[0].allocated = 256 * MIB;
        let dram = plan(&Settings::CLEAN, Tier::Dram, &Block::CleanDram, &slots, &sizes, &reach_of(15 * MIB)).unwrap();
        assert_eq!(dram.role(0), Role::Measures { extent: Some(256 * MIB) });
    }

    /// TODO 119 (the review of 2026-11-20): the extent sizes the block and never chooses who measures. The same bytes written two ways,
    /// the whole 1 GiB memory and 1 GiB outright, give a clean DRAM step the same measurer and the same block; a count's measurers the
    /// same on a full-memory test
    #[test]
    fn the_extent_sizes_the_block_and_never_chooses_who_measures() {
        let slots: Vec<Slot> = (0..4).map(|t| slot(t, t, 0, GIB)).collect();
        let sizes = Sizes { l2: MIB, l2_smt: None, l3: 16 * MIB };
        let reach = reach_of(15 * MIB);
        let whole = plan(&Settings::CLEAN, Tier::Dram, &Block::Whole, &slots, &sizes, &reach).unwrap();
        let outright = plan(&Settings::CLEAN, Tier::Dram, &Block::Exactly(GIB), &slots, &sizes, &reach).unwrap();
        assert_eq!(whole.roles, outright.roles);
        assert_eq!(whole.roles, [Role::Measures { extent: Some(GIB) }, Role::SitsOut, Role::SitsOut, Role::SitsOut]);
        assert_eq!((whole.record.measurers, whole.record.block_bytes), (outright.record.measurers, outright.record.block_bytes));
        let two = Settings { threads: LatencyThreads::Count(2), ..Settings::default() };
        let full = plan(&two, Tier::DramFull, &Block::Whole, &slots, &sizes, &reach).unwrap();
        assert_eq!((full.record.measurers, full.role(1), full.role(2)), (2, Role::Measures { extent: Some(GIB) }, Role::SitsOut));
    }

    /// A block given outright is held whole: the first eligible thread that holds it measures, passing over one with less memory; with
    /// none, the step is refused with both sizes, never run on less
    #[test]
    fn a_block_given_outright_must_fit_a_measurer() {
        let mut slots: Vec<Slot> = (0..4).map(|t| slot(t, t, 0, GIB)).collect();
        let sizes = Sizes { l2: MIB, l2_smt: None, l3: 16 * MIB };
        let reach = reach_of(15 * MIB);
        let refused = plan(&Settings::CLEAN, Tier::Dram, &Block::Exactly(2 * GIB), &slots, &sizes, &reach).unwrap_err();
        assert!(refused.contains("2.00 GiB") && refused.contains("1.00 GiB"), "{refused}");
        slots[2].allocated = 4 * GIB;
        let fits = plan(&Settings::CLEAN, Tier::Dram, &Block::Exactly(2 * GIB), &slots, &sizes, &reach).unwrap();
        assert_eq!(
            (fits.role(0), fits.role(2), fits.record.cpus.as_slice()),
            (Role::SitsOut, Role::Measures { extent: Some(2 * GIB) }, &[2][..])
        );
    }

    /// Clean on an L3 passes over a thread that can't hold the block: three of four threads at 90.6%, where taking the first three and
    /// capping the third at its 4 MiB read 75% (the review of 2026-11-20)
    #[test]
    fn clean_passes_over_a_thread_that_cant_hold_its_l3_block() {
        let mut slots: Vec<Slot> = (0..4).map(|t| slot(t, t, 0, GIB)).collect();
        slots[2].allocated = 4 * MIB;
        let sizes = Sizes { l2: MIB, l2_smt: None, l3: 64 * MIB };
        // 32/k MiB: 8 MiB at four threads, 87.5%; 10.67 MiB at three, 90.6%
        let clean = plan(&Settings::CLEAN, Tier::L3, &L3_HALF, &slots, &sizes, &reach_of(64 * MIB)).unwrap();
        assert_eq!(clean.record.cpus, [0, 1, 3]);
        assert!(!clean.record.target_unmet && clean.record.l3_share_estimated.unwrap() > 0.9);
        assert_eq!(clean.role(2), Role::SitsOut);
    }

    /// Clean on an L3 needs an L3 share to size its block: an extent given outright is refused, saying why; a count takes it as it is
    #[test]
    fn clean_refuses_an_l3_test_whose_extent_isnt_an_l3_share() {
        let (slots, sizes) = petz007();
        let reach = reach_of(15 * MIB);
        let refused = plan(&Settings::CLEAN, Tier::L3, &Block::Exactly(2 * MIB), &slots, &sizes, &reach).unwrap_err();
        assert!(refused.contains("2.00 MiB outright") && refused.contains("L3/2"), "{refused}");
        let one = Settings { threads: LatencyThreads::Count(1), ..Settings::default() };
        assert_eq!(plan(&one, Tier::L3, &Block::Exactly(2 * MIB), &slots, &sizes, &reach).unwrap().role(0), Role::Measures {
            extent: Some(2 * MIB)
        });
    }

    /// TODO 119: one measurer a core on an SMT run has its core's L1 and L2 to itself, its sibling sitting out, so its block is the one
    /// thread's; `all` with a core filter keeps both siblings, which share them, and their L3 floor is half the L2 each
    #[test]
    fn a_measurers_block_counts_the_siblings_that_measure_not_the_runs() {
        let slots: Vec<Slot> = (0..8).map(|t| slot(t, t % 4, 0, GIB)).collect();
        let sizes = Sizes { l2: MIB, l2_smt: None, l3: 32 * MIB };
        // Half the L2, split between the core's measurers
        let half_l2 = |n: usize| MIB / 2 / n;
        let one = Settings { threads: LatencyThreads::Count(1), ..Settings::default() };
        let alone = plan(&one, Tier::Private, &Block::Private(&half_l2), &slots, &sizes, &reach_of(32 * MIB)).unwrap();
        assert_eq!((alone.role(0), alone.record.per_core), (Role::Measures { extent: Some(MIB / 2) }, 1));
        let siblings = Settings { cores: LatencyCores::Performance, ..Settings::default() };
        let both = plan(&siblings, Tier::Private, &Block::Private(&half_l2), &slots, &sizes, &reach_of(32 * MIB)).unwrap();
        assert_eq!((both.role(0), both.role(4), both.record.per_core), (Role::Measures { extent: Some(MIB / 4) }, both.role(0), 2));
        // An L3 step's floor: the half L2 a sibling has, plus 64 KiB, above half a small reach
        let l3 = plan(&siblings, Tier::L3, &L3_HALF, &slots, &sizes, &reach_of(MIB / 2)).unwrap();
        assert_eq!(l3.role(0), Role::Measures { extent: Some(MIB / 2 + (64 << 10)) });
        assert!((l3.record.l3_share_estimated.unwrap() - (1.0 - 0.5 / (0.5 + 1.0 / 16.0))).abs() < 1e-9);
    }

    /// The record says who measured, where and over what, the bytes exactly when asked; and two records alike measure alike
    #[test]
    fn a_record_describes_its_measurers() {
        let (slots, sizes) = petz007();
        let dram = plan(&Settings::CLEAN, Tier::Dram, &Block::CleanDram, &slots, &sizes, &reach_of(15 * MIB)).unwrap().record;
        assert_eq!(dram.describe(false), "1 thread (latency-threads=clean) on CPU 0, 1 an L3, over 1.00 GiB");
        assert_eq!(dram.describe(true), "1 thread (latency-threads=clean) on CPU 0, 1 an L3, over 1.00 GiB (1073741824 B)");
        let moved = Record { cpus: vec![2], ..dram.clone() };
        assert_ne!(dram.measuring(), moved.measuring());
        assert_eq!(dram.measuring(), dram.clone().measuring());
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
