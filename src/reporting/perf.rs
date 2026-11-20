//! The run's figures a cache tier, and memory's latency under load (TODO 119), as the final summary's grid and its under-load table show
//! them. Built from the saved steps alone, so a run's end, `--show-results` and an older results file (v0.1.1 on) give the same figures.
//! A row, a column or the under-load table appears only when a step that fills it ran; neither for a `config=` run, whose tests can carry
//! the built-in names with other settings.

use serde::{Deserialize, Serialize};

use crate::loaded::{Load, LoadedSummary};
use crate::results::{TestAverage, TestRunResult};
use crate::run_context::TestConfigSnapshot;

/// A tier of the grid, in its order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    L1,
    L2,
    L3,
    Memory,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::L1, Tier::L2, Tier::L3, Tier::Memory];

    pub fn name(self) -> &'static str {
        match self {
            Tier::L1 => "L1",
            Tier::L2 => "L2",
            Tier::L3 => "L3",
            Tier::Memory => "Memory",
        }
    }

    /// The bandwidth tests' names for the tier, the first that ran taking the row: Memory's the 4-L3s-a-thread DRAMSmall, else DRAMFull
    fn speed_tiers(self) -> &'static [&'static str] {
        match self {
            Tier::L1 => &["L1"],
            Tier::L2 => &["L2"],
            Tier::L3 => &["L3"],
            Tier::Memory => &["DRAMSmall", "DRAMFull"],
        }
    }

    fn latency_test(self) -> &'static str {
        match self {
            Tier::L1 => "Lat-L1-Read",
            Tier::L2 => "Lat-L2-Read",
            Tier::L3 => "Lat-L3-Read",
            Tier::Memory => "Lat-DRAM-Read",
        }
    }

    /// The tier's latency read clean (TODO 119, `measure`): fewer threads to an L3 than the run has, the rest sitting out; none for a
    /// core's own L1 and L2
    fn write_latency_test(self) -> Option<&'static str> {
        (self == Tier::Memory).then_some(WRITE_LATENCY_TEST)
    }

    fn clean_test(self) -> Option<&'static str> {
        match self {
            Tier::L3 => Some("Lat-L3-Read-Clean"),
            Tier::Memory => Some("Lat-DRAM-Read-Clean"),
            Tier::L1 | Tier::L2 => None,
        }
    }
}

/// The bandwidth tests' operations, in the grid's column order
pub const OPS: [&str; 3] = ["Read", "Write", "Copy"];

/// A bandwidth test's names, the one taken first: `-Auto`, else the widest width that ran
const WIDTH_SUFFIXES: [&str; 4] = ["Auto", "512", "256", "128"];

const TURNAROUND_TEST: &str = "Lat-Turnaround-DRAM";
/// Memory's write latency (TODO 119): a whole line NT-stored, then SFENCE
const WRITE_LATENCY_TEST: &str = "Lat-DRAM-NTStore-SFence";

/// A cell's figure
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Figure {
    /// What the step measured; `cut_short` when a stop ended one of its runs before its limit
    Measured { value: f64, cut_short: bool },
    /// The step was in the run, but a stop came before it ran
    NotReached,
}

/// One tier's row
#[derive(Debug, Clone, PartialEq)]
pub struct TierRow {
    pub tier: Tier,
    /// Read, write and copy MiB/s, as `OPS`; `None` when no step of it ran
    pub speed: [Option<Figure>; 3],
    /// The read latency's P50, ns, as the run's threads chased it
    pub latency: Option<Figure>,
    /// Its `-Clean` test's P50: fewer threads to an L3, the rest sitting out (TODO 119)
    pub latency_clean: Option<Figure>,
    /// Memory's write latency, `WRITE_LATENCY_TEST`'s P50, ns a 64 B line (TODO 119); none for a cache tier
    pub write_latency: Option<Figure>,
}

/// The L3 row's latency as the run's threads chased it: each thread's block beside the L2, so how much of the chase the L2 could still
/// serve. A sample is the mean of 1000 chained loads, so the P50 is a blend of the two tiers, not the L3's alone.
#[derive(Debug, Clone, PartialEq)]
pub struct L3Share {
    pub threads: usize,
    pub threads_per_core: usize,
    pub block_bytes: u64,
    /// The calibrated L2 a thread's L3 block is floored by, else CPUID's
    pub l2_bytes: u64,
}

impl L3Share {
    /// The L2's estimated share of the chase (the blend model, not a counter): its capacity over the blocks its core's threads chase
    pub fn l2_share(&self) -> f64 {
        let chased = self.block_bytes as f64 * self.threads_per_core.max(1) as f64;
        if chased > 0.0 { (self.l2_bytes as f64 / chased).min(1.0) } else { 1.0 }
    }
}

/// The grid: a row a tier that ran
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub rows: Vec<TierRow>,
    /// The bandwidth steps' widths, each once, ascending
    pub widths: Vec<u32>,
    pub threads: usize,
    pub nodes: usize,
    pub cycles: usize,
    /// Memory's bandwidth is the full allocation's: no Spd-DRAMSmall step ran
    pub memory_full: bool,
    pub l3_share: Option<L3Share>,
    /// The Latency column's plan, when a setting (`latency-threads=`, `latency-cores=`) had fewer than all threads measure it
    pub latency_plan: Option<crate::measure::Record>,
    /// The clean column's plans: the L3's and Memory's `-Clean` steps'
    pub l3_clean: Option<crate::measure::Record>,
    pub memory_clean: Option<crate::measure::Record>,
    /// Memory's write latency's plan: one thread over the clean read's block
    pub memory_write: Option<crate::measure::Record>,
}

/// A load level's row
#[derive(Debug, Clone, PartialEq)]
pub struct LevelRow {
    /// The level as given, a share (`75%`); `None` for a pinned MiB/s
    pub share: Option<String>,
    /// Its MiB/s, the mean over the cycles that reached it
    pub target_mib_s: f64,
    pub state: LevelState,
    /// The cycles the step ran in, and those whose level was measured and matched
    pub cycles: usize,
    pub cycles_matched: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LevelState {
    /// Both kinds reached the target: the reads' latency under NT writes and under reads, and the paired turnaround, means over the
    /// cycles that matched
    Measured { writes_ns: f64, reads_ns: f64, turnaround_ns: f64 },
    /// Both kinds measured, not both within the match of the target
    NotMatched,
    /// A stop came before both kinds were measured
    NotReached,
}

/// Lat-Turnaround-DRAM's latencies, its flat-out rows left in the step's own report
#[derive(Debug, Clone, PartialEq)]
pub struct UnderLoad {
    pub measurer_node: u32,
    pub loaders: usize,
    /// The run's nodes: above one, the others sit idle
    pub nodes: usize,
    pub idle_ns: Option<f64>,
    /// What a share is of: the lower of the loaders' flat-out MiB/s
    pub base_mib_s: Option<f64>,
    pub levels: Vec<LevelRow>,
    pub partial: bool,
    pub cycles: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnderLoadPart {
    Measured(UnderLoad),
    /// The step was in the run, but a stop came before it ran
    NotReached,
}

/// What the final summary shows of the run's bandwidth and latency
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerfSummary {
    pub grid: Option<Grid>,
    pub under_load: Option<UnderLoadPart>,
}

/// `PerfSummary` as a results file keeps it, for tools (`OverallStats::perf`): figures unrounded, as the file keeps every figure; a cell
/// no step filled absent, one whose step a stop came before named in `not_reached`, one a stop cut short in `cut_short`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<GridRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under_load: Option<UnderLoadRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridRecord {
    pub widths: Vec<u32>,
    pub threads: usize,
    pub nodes: usize,
    pub cycles: usize,
    /// Memory's bandwidth is the whole allocation's (Spd-DRAMFull), no Spd-DRAMSmall step having run
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub memory_full: bool,
    pub tiers: Vec<TierRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l3_share: Option<L3ShareRecord>,
    /// The plans the latency columns ran under (TODO 119): the Latency column's when a setting had fewer threads measure it, and the
    /// L3's and Memory's `-Clean` steps'
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_plan: Option<crate::measure::Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l3_clean: Option<crate::measure::Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_clean: Option<crate::measure::Record>,
    /// Memory's write latency's (TODO 119)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_write: Option<crate::measure::Record>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TierRecord {
    pub tier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_mib_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_mib_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_mib_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_p50_ns: Option<f64>,
    /// The `-Clean` test's (TODO 119)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_clean_p50_ns: Option<f64>,
    /// Memory's write latency, ns a 64 B line NT-stored and fenced (TODO 119, Lat-DRAM-NTStore-SFence)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_latency_p50_ns: Option<f64>,
    /// The cells whose step was in the run but never ran: `read`, `write`, `copy`, `latency`, `latency_clean`, `write_latency`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_reached: Vec<String>,
    /// The cells whose step a stop ended early
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cut_short: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct L3ShareRecord {
    pub threads: usize,
    pub threads_per_core: usize,
    pub block_bytes: u64,
    pub l2_bytes: u64,
    /// The L2's estimated share of the L3 row's chase (`L3Share::l2_share`), a model's, not a counter's
    pub l2_share_estimated: f64,
}

/// The under-load table, `state` `measured` or `not_reached`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UnderLoadRecord {
    Measured {
        measurer_node: u32,
        loaders: usize,
        nodes: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_ns: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_mib_s: Option<f64>,
        levels: Vec<LevelRecord>,
        partial: bool,
        cycles: usize,
    },
    NotReached,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelRecord {
    /// The level as given, a share (`75%`); absent for a pinned MiB/s
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    pub target_mib_s: f64,
    /// `measured`, `not_matched` or `not_reached`
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writes_ns: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reads_ns: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnaround_ns: Option<f64>,
    pub cycles: usize,
    pub cycles_matched: usize,
}

impl From<&PerfSummary> for PerfRecord {
    fn from(summary: &PerfSummary) -> Self {
        let grid = summary.grid.as_ref().map(|g| GridRecord {
            widths: g.widths.clone(),
            threads: g.threads,
            nodes: g.nodes,
            cycles: g.cycles,
            memory_full: g.memory_full,
            tiers: g.rows.iter().map(TierRecord::from).collect(),
            l3_share: g.l3_share.as_ref().map(|s| L3ShareRecord {
                threads: s.threads,
                threads_per_core: s.threads_per_core,
                block_bytes: s.block_bytes,
                l2_bytes: s.l2_bytes,
                l2_share_estimated: s.l2_share(),
            }),
            latency_plan: g.latency_plan.clone(),
            l3_clean: g.l3_clean.clone(),
            memory_clean: g.memory_clean.clone(),
            memory_write: g.memory_write.clone(),
        });
        let under_load = summary.under_load.as_ref().map(|u| match u {
            UnderLoadPart::NotReached => UnderLoadRecord::NotReached,
            UnderLoadPart::Measured(u) => UnderLoadRecord::Measured {
                measurer_node: u.measurer_node,
                loaders: u.loaders,
                nodes: u.nodes,
                idle_ns: u.idle_ns,
                base_mib_s: u.base_mib_s,
                levels: u.levels.iter().map(LevelRecord::from).collect(),
                partial: u.partial,
                cycles: u.cycles,
            },
        });
        PerfRecord { grid, under_load }
    }
}

impl From<&TierRow> for TierRecord {
    fn from(row: &TierRow) -> Self {
        let cells = [
            ("read", row.speed[0]),
            ("write", row.speed[1]),
            ("copy", row.speed[2]),
            ("latency", row.latency),
            ("latency_clean", row.latency_clean),
            ("write_latency", row.write_latency),
        ];
        let value = |figure: Option<Figure>| match figure {
            Some(Figure::Measured { value, .. }) => Some(value),
            _ => None,
        };
        let named = |pick: fn(Option<Figure>) -> bool| cells.iter().filter(|(_, f)| pick(*f)).map(|(n, _)| n.to_string()).collect();
        TierRecord {
            tier: row.tier.name().to_string(),
            read_mib_s: value(row.speed[0]),
            write_mib_s: value(row.speed[1]),
            copy_mib_s: value(row.speed[2]),
            latency_p50_ns: value(row.latency),
            latency_clean_p50_ns: value(row.latency_clean),
            write_latency_p50_ns: value(row.write_latency),
            not_reached: named(|f| f == Some(Figure::NotReached)),
            cut_short: named(|f| matches!(f, Some(Figure::Measured { cut_short: true, .. }))),
        }
    }
}

impl From<&LevelRow> for LevelRecord {
    fn from(level: &LevelRow) -> Self {
        let (state, writes_ns, reads_ns, turnaround_ns) = match level.state {
            LevelState::Measured { writes_ns, reads_ns, turnaround_ns } => {
                ("measured", Some(writes_ns), Some(reads_ns), Some(turnaround_ns))
            }
            LevelState::NotMatched => ("not_matched", None, None, None),
            LevelState::NotReached => ("not_reached", None, None, None),
        };
        LevelRecord {
            level: level.share.clone(),
            target_mib_s: level.target_mib_s,
            state: state.to_string(),
            writes_ns,
            reads_ns,
            turnaround_ns,
            cycles: level.cycles,
            cycles_matched: level.cycles_matched,
        }
    }
}

/// The run's grid and under-load table, `None` when no step fills either, or for a `config=` run
pub fn perf_summary(result: &TestRunResult) -> Option<PerfSummary> {
    if result.run_config.config_name.is_some() {
        return None;
    }
    let steps = Steps::of(result);
    let summary = PerfSummary { grid: grid(result, &steps), under_load: under_load(result, &steps) };
    (summary.grid.is_some() || summary.under_load.is_some()).then_some(summary)
}

/// A step's averages with its config, both saved
struct Step<'a> {
    average: &'a TestAverage,
    config: &'a TestConfigSnapshot,
}

/// The run's steps, by the name its plan gave each
struct Steps<'a> {
    steps: Vec<Step<'a>>,
    result: &'a TestRunResult,
}

impl<'a> Steps<'a> {
    /// Each step's averages joined to its config by step number. A pair whose resolved names disagree is left out: a figure under the
    /// wrong tier is worse than none.
    fn of(result: &'a TestRunResult) -> Self {
        let configs = &result.run_config.tests;
        let steps = result
            .overall_stats
            .per_step_averages
            .iter()
            .filter_map(|average| {
                let config = configs.get(average.step.checked_sub(1)?)?;
                (config.name == average.name).then_some(Step { average, config })
            })
            .collect();
        Steps { steps, result }
    }

    /// The first step of `test`'s plan, when fewer than all threads measured it (TODO 119)
    fn plan(&self, test: &str) -> Option<crate::measure::Record> {
        self.named(test).first().and_then(|s| s.config.measure.clone())
    }

    fn named(&self, test: &str) -> Vec<&Step<'a>> {
        self.steps.iter().filter(|s| s.config.test == test).collect()
    }

    /// Whether a stop ended one of these steps' runs before its limit
    fn cut_short(&self, steps: &[&Step<'a>]) -> bool {
        self.result
            .cycles
            .iter()
            .flat_map(|c| &c.tests)
            .any(|t| t.stopped_by.as_deref() == Some("stopped") && steps.iter().any(|s| s.average.step == t.step))
    }

    /// The steps' MiB/s, their runs' weighted mean as the per-test table takes it, or that none ran
    fn speed(&self, steps: &[&Step<'a>]) -> Option<Figure> {
        if steps.is_empty() {
            return None;
        }
        let runs: u64 = steps.iter().map(|s| s.average.runs).sum();
        if runs == 0 {
            return Some(Figure::NotReached);
        }
        let mib_s = steps.iter().map(|s| s.average.avg_throughput_mib_s * s.average.runs as f64).sum::<f64>() / runs as f64;
        Some(Figure::Measured { value: mib_s, cut_short: self.cut_short(steps) })
    }

    /// The steps' P50, the first that ran's as the per-test table takes it, or that none ran
    fn latency(&self, steps: &[&Step<'a>]) -> Option<Figure> {
        if steps.is_empty() {
            return None;
        }
        match steps.iter().find(|s| s.average.runs > 0).and_then(|s| s.average.latency_p50_ns) {
            Some(ns) => Some(Figure::Measured { value: ns, cut_short: self.cut_short(steps) }),
            None => Some(Figure::NotReached),
        }
    }
}

/// The bandwidth steps of a tier name and an operation that the run took: its `-Auto` test's, else its widest width's
fn speed_steps<'s, 'a>(steps: &'s Steps<'a>, tier: &str, op: &str) -> Vec<&'s Step<'a>> {
    WIDTH_SUFFIXES.iter().map(|w| steps.named(&format!("Spd-{tier}-{op}-{w}"))).find(|found| !found.is_empty()).unwrap_or_default()
}

/// A step's width: what it ran at when its name doesn't fix it, else its name's
fn width(step: &Step<'_>) -> Option<u32> {
    step.config.simd_width_bits.or_else(|| step.config.test.rsplit('-').next()?.parse().ok())
}

fn grid(result: &TestRunResult, steps: &Steps<'_>) -> Option<Grid> {
    let mut widths = Vec::new();
    let mut memory_full = false;
    let mut rows = Vec::new();
    for tier in Tier::ALL {
        // A tier's bandwidth comes from one of its names for every operation, never one name's read beside another's copy
        let name = tier.speed_tiers().iter().find(|t| OPS.iter().any(|op| !speed_steps(steps, t, op).is_empty()));
        let speed = OPS.map(|op| {
            let found = name.map(|t| speed_steps(steps, t, op)).unwrap_or_default();
            widths.extend(found.iter().filter_map(|s| width(s)));
            steps.speed(&found)
        });
        if tier == Tier::Memory && name == Some(&"DRAMFull") {
            memory_full = true;
        }
        let latency = steps.latency(&steps.named(tier.latency_test()));
        let latency_clean = tier.clean_test().and_then(|test| steps.latency(&steps.named(test)));
        let write_latency = tier.write_latency_test().and_then(|test| steps.latency(&steps.named(test)));
        if speed.iter().any(Option::is_some) || latency.is_some() || latency_clean.is_some() || write_latency.is_some() {
            rows.push(TierRow { tier, speed, latency, latency_clean, write_latency });
        }
    }
    if rows.is_empty() {
        return None;
    }
    widths.sort_unstable();
    widths.dedup();
    let threads = &result.run_config.threads;
    let l3_measured = rows.iter().any(|r| r.tier == Tier::L3 && matches!(r.latency, Some(Figure::Measured { .. })));
    let l3_share = l3_measured.then(|| l3_share(result, steps)).flatten();
    Some(Grid {
        rows,
        widths,
        threads: threads.thread_count,
        nodes: threads.numa_distribution.iter().filter(|n| n.threads > 0).count().max(1),
        cycles: result.overall_stats.cycles_completed as usize,
        memory_full,
        l3_share,
        latency_plan: Tier::ALL.iter().find_map(|tier| steps.plan(tier.latency_test())),
        l3_clean: Tier::L3.clean_test().and_then(|test| steps.plan(test)),
        memory_clean: Tier::Memory.clean_test().and_then(|test| steps.plan(test)),
        memory_write: steps.plan(WRITE_LATENCY_TEST),
    })
}

fn l3_share(result: &TestRunResult, steps: &Steps<'_>) -> Option<L3Share> {
    let step = steps.named(Tier::L3.latency_test()).first().copied()?;
    let block_bytes = step.config.extent_bytes;
    // A setting that had fewer threads measure it: its measurers, and the most of them on one core (`measure::plan`)
    let planned = step.config.measure.as_ref().map(|plan| (plan.measurers, plan.per_core));
    let calibrated = result.run_config.calibration.as_ref().and_then(|c| c.tiers.iter().find(|t| t.tier == "L2")).map(|t| t.optimal_size.0);
    let threads = &result.run_config.threads;
    Some(L3Share {
        threads: planned.map_or(threads.thread_count, |(measurers, _)| measurers),
        threads_per_core: planned.map_or(threads.active_threads_per_core, |(_, per_core)| per_core),
        block_bytes,
        l2_bytes: calibrated.unwrap_or(result.identity.l2_cache_per_core.0),
    })
}

fn under_load(result: &TestRunResult, steps: &Steps<'_>) -> Option<UnderLoadPart> {
    let turnaround = steps.named(TURNAROUND_TEST);
    if turnaround.is_empty() {
        return None;
    }
    let summaries: Vec<&LoadedSummary> = result
        .cycles
        .iter()
        .flat_map(|c| &c.tests)
        .filter(|t| turnaround.iter().any(|s| s.average.step == t.step))
        .filter_map(|t| t.loaded.as_deref())
        .collect();
    let Some(first) = summaries.first() else {
        return Some(UnderLoadPart::NotReached);
    };
    let idle: Vec<f64> =
        summaries.iter().flat_map(|s| &s.rows).filter(|r| r.load == Load::Idle).filter_map(|r| Some(r.idle.as_ref()?.ns)).collect();
    let given = first.levels.as_ref().map(|l| l.given.as_slice()).unwrap_or_default();
    let levels = given
        .iter()
        .enumerate()
        .map(|(index, level)| {
            let rows: Vec<_> = summaries.iter().filter_map(|s| s.rows.iter().find(|r| r.load == Load::Level(index))).collect();
            let targets: Vec<f64> = rows.iter().filter_map(|r| r.target_mib_s).collect();
            let measured: Vec<(f64, f64, f64)> = rows
                .iter()
                .filter(|r| r.matched != Some(false))
                .filter_map(|r| Some((r.writes.as_ref()?.ns, r.reads.as_ref()?.ns, r.turnaround.as_ref()?.ns)))
                .collect();
            let not_matched = rows.iter().any(|r| r.matched == Some(false) && r.writes.is_some() && r.reads.is_some());
            let state = match mean3(&measured) {
                Some((writes_ns, reads_ns, turnaround_ns)) => LevelState::Measured { writes_ns, reads_ns, turnaround_ns },
                None if not_matched => LevelState::NotMatched,
                None => LevelState::NotReached,
            };
            LevelRow {
                share: (!level.is_pinned()).then(|| level.to_string()),
                target_mib_s: mean(&targets).unwrap_or(0.0),
                state,
                cycles: summaries.len(),
                cycles_matched: measured.len(),
            }
        })
        .collect();
    let threads = &result.run_config.threads;
    Some(UnderLoadPart::Measured(UnderLoad {
        measurer_node: first.measurer.node,
        loaders: first.loaders.len(),
        nodes: threads.numa_distribution.iter().filter(|n| n.threads > 0).count().max(1),
        idle_ns: mean(&idle),
        base_mib_s: first.levels.as_ref().map(|l| l.base_mib_s),
        levels,
        partial: summaries.iter().any(|s| s.partial),
        cycles: summaries.len(),
    }))
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn mean3(values: &[(f64, f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = values.len() as f64;
    (!values.is_empty()).then(|| {
        let sum = values.iter().fold((0.0, 0.0, 0.0), |a, v| (a.0 + v.0, a.1 + v.1, a.2 + v.2));
        (sum.0 / n, sum.1 / n, sum.2 / n)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{PerfStep, loaded_summary, perf_run, perf_run_of};

    fn grid_of(result: &TestRunResult) -> Grid {
        perf_summary(result).and_then(|p| p.grid).expect("a grid")
    }

    fn measured(figure: Option<Figure>) -> f64 {
        match figure {
            Some(Figure::Measured { value, .. }) => value,
            other => panic!("not measured: {other:?}"),
        }
    }

    /// A plan=perf run: a row a tier, read, write and copy and a latency in each, at the width the `-Auto` steps ran; the L3's note from
    /// the calibrated L2; the under-load table's levels labelled by share and found by index, an unmatched one reading not matched
    #[test]
    fn a_perf_run_fills_the_grid_and_the_under_load_table() {
        let result = perf_run();
        let summary = perf_summary(&result).expect("perf figures");
        let grid = summary.grid.expect("a grid");
        assert_eq!(grid.rows.iter().map(|r| r.tier).collect::<Vec<_>>(), Tier::ALL);
        assert!(grid.rows.iter().all(|r| r.speed.iter().all(Option::is_some) && r.latency.is_some()));
        assert_eq!((grid.widths.as_slice(), grid.threads, grid.nodes, grid.cycles, grid.memory_full), (&[512][..], 6, 1, 1, false));
        let memory = &grid.rows[3];
        assert_eq!((measured(memory.speed[0]), measured(memory.speed[2]), measured(memory.latency)), (60_926.0, 75_307.0, 63.5));
        let share = grid.l3_share.expect("the L3 row's note");
        assert_eq!((share.l2_bytes, share.block_bytes), (1014 << 10, 1_327_104), "the calibrated L2, not CPUID's 1 MiB");
        assert!((share.l2_share() - (1014.0 * 1024.0 / 1_327_104.0)).abs() < 1e-9);

        let Some(UnderLoadPart::Measured(under)) = summary.under_load else { panic!("the under-load table") };
        let fixture = loaded_summary(true);
        let level = |i: usize| fixture.rows.iter().find(|r| r.load == Load::Level(i)).unwrap();
        assert_eq!(under.levels.iter().map(|l| l.share.as_deref()).collect::<Vec<_>>(), [Some("25%"), Some("50%"), Some("75%")]);
        assert_eq!(under.idle_ns, fixture.rows.iter().find(|r| r.load == Load::Idle).and_then(|r| Some(r.idle.as_ref()?.ns)));
        for (i, row) in under.levels.iter().enumerate() {
            let want = level(i);
            assert_eq!(row.target_mib_s, want.target_mib_s.unwrap());
            match (want.matched, row.state) {
                (Some(false), LevelState::NotMatched) => {}
                (_, LevelState::Measured { writes_ns, reads_ns, turnaround_ns }) => assert_eq!(
                    (writes_ns, reads_ns, turnaround_ns),
                    (want.writes.as_ref().unwrap().ns, want.reads.as_ref().unwrap().ns, want.turnaround.as_ref().unwrap().ns)
                ),
                (matched, state) => panic!("level {i}: matched {matched:?} shown as {state:?}"),
            }
        }
        assert!(matches!(under.levels[2].state, LevelState::NotMatched), "the fixture's 75% level isn't matched");
        assert_eq!((under.loaders, under.cycles, under.partial), (fixture.loaders.len(), 1, false));
    }

    /// Show only what ran: a run of one memory test has the Memory row and its one column, no latency, no under-load table
    #[test]
    fn only_what_ran_shows() {
        let result = perf_run_of(&[PerfStep::speed("Spd-DRAMFull-Read-256", "Spd-DRAMFull-Read-256", 44_000.0)], &[]);
        let summary = perf_summary(&result).expect("perf figures");
        assert!(summary.under_load.is_none());
        let grid = summary.grid.unwrap();
        assert_eq!(grid.rows.len(), 1);
        let row = &grid.rows[0];
        assert_eq!((row.tier, row.speed[1], row.speed[2], row.latency), (Tier::Memory, None, None, None));
        assert_eq!((measured(row.speed[0]), grid.memory_full, grid.widths.as_slice()), (44_000.0, true, &[256][..]));
        assert!(grid.l3_share.is_none());
    }

    /// No step that fills the grid or the table: none; a config's run: none, its tests can carry the built-in names with other settings
    #[test]
    fn a_config_run_or_none_of_the_steps_shows_nothing() {
        assert!(perf_summary(&perf_run_of(&[PerfStep::speed("Mem-SimpleV2-Auto", "Mem-SimpleV2-512_A", 9_000.0)], &[])).is_none());
        let mut result = perf_run();
        result.run_config.config_name = Some("1usmus_v3".to_string());
        assert!(perf_summary(&result).is_none());
    }

    /// A tier's figure is its `-Auto` step's, else its widest width's
    #[test]
    fn auto_then_the_widest_width() {
        let widths =
            [PerfStep::speed("Spd-L1-Read-256", "Spd-L1-Read-256", 100.0), PerfStep::speed("Spd-L1-Read-512", "Spd-L1-Read-512", 200.0)];
        let grid = grid_of(&perf_run_of(&widths, &[]));
        assert_eq!((measured(grid.rows[0].speed[0]), grid.widths.as_slice()), (200.0, &[512][..]));
        let mut all = widths.to_vec();
        all.push(PerfStep::speed("Spd-L1-Read-Auto", "Spd-L1-Read-512_A", 300.0));
        assert_eq!(measured(grid_of(&perf_run_of(&all, &[])).rows[0].speed[0]), 300.0);
    }

    /// Memory's row comes from one of its names for every operation: DRAMSmall's read never sits beside DRAMFull's copy
    #[test]
    fn memory_takes_one_name_for_every_operation() {
        let steps = [
            PerfStep::speed("Spd-DRAMSmall-Read-Auto", "Spd-DRAMSmall-Read-512_A", 60_000.0),
            PerfStep::speed("Spd-DRAMFull-Copy-Auto", "Spd-DRAMFull-Copy-512_A", 80_000.0),
        ];
        let grid = grid_of(&perf_run_of(&steps, &[]));
        assert_eq!((measured(grid.rows[0].speed[0]), grid.rows[0].speed[2], grid.memory_full), (60_000.0, None, false));
    }

    /// A step a stop came before reads n/a, never 0; one a stop cut short keeps its figure, marked
    #[test]
    fn a_step_not_reached_and_one_cut_short() {
        let mut cut = PerfStep::speed("Spd-L2-Read-Auto", "Spd-L2-Read-512_A", 800_000.0);
        cut.stopped = true;
        let steps = [PerfStep::speed("Spd-L1-Read-Auto", "Spd-L1-Read-512_A", 0.0), cut];
        let grid = grid_of(&perf_run_of(&steps, &[]));
        assert_eq!(grid.rows[0].speed[0], Some(Figure::NotReached));
        // The results' own average (bytes over timed seconds) comes back to within an ulp or so
        let Some(Figure::Measured { value, cut_short: true }) = grid.rows[1].speed[0] else {
            panic!("cut short: {:?}", grid.rows[1].speed[0])
        };
        assert!((value - 800_000.0).abs() < 1e-6, "{value}");
    }

    /// A step whose saved config isn't the step its averages are (a resolved name that differs) is left out, not shown under the wrong tier
    #[test]
    fn a_misjoined_step_is_left_out() {
        let mut result = perf_run_of(&[PerfStep::speed("Spd-L1-Read-Auto", "Spd-L1-Read-512_A", 100.0)], &[]);
        result.run_config.tests[0].name = "Mem-StuckBit512_A".to_string();
        assert!(perf_summary(&result).is_none());
    }

    /// Two threads a core chase twice the block in one L2: the L2's share halves. Without a calibration, CPUID's L2.
    #[test]
    fn the_l3_note_counts_a_cores_threads_and_falls_back_to_cpuids_l2() {
        let mut result = perf_run();
        result.run_config.threads.active_threads_per_core = 2;
        result.run_config.calibration = None;
        let share = grid_of(&result).l3_share.unwrap();
        assert_eq!(share.l2_bytes, 1 << 20);
        assert!((share.l2_share() - (1u64 << 20) as f64 / (2.0 * 1_327_104.0)).abs() < 1e-9);
    }

    /// The `-Clean` steps fill the L3's and Memory's clean column, the private tiers' none, and the grid keeps their plans; every thread
    /// measured the configured column, so it has no plan
    #[test]
    fn the_clean_steps_fill_their_column_with_their_plans() {
        let grid = grid_of(&perf_run());
        let clean: Vec<Option<f64>> =
            grid.rows.iter().map(|r| r.latency_clean.map(|f| if let Figure::Measured { value, .. } = f { value } else { 0.0 })).collect();
        assert_eq!(clean, [None, None, Some(8.7), Some(67.0)]);
        assert!(grid.latency_plan.is_none());
        let l3 = grid.l3_clean.expect("the L3's clean plan");
        assert_eq!((l3.measurers, l3.per_l3, l3.block_bytes, l3.target_unmet), (1, 1, Some(7_864_320), true));
        let memory = grid.memory_clean.expect("Memory's clean plan");
        assert_eq!((memory.measurers, memory.block_bytes, memory.l3_share_estimated), (1, Some(1 << 30), None));
        // Memory's write latency, its row's alone, and its plan
        let writes: Vec<bool> = grid.rows.iter().map(|r| r.write_latency.is_some()).collect();
        assert_eq!((writes.as_slice(), measured(grid.rows[3].write_latency)), (&[false, false, false, true][..], 62.0));
        assert_eq!(grid.memory_write.map(|p| (p.measurers, p.block_bytes)), Some((1, Some(1 << 30))));
        // The L3 note stays the configured step's: its six threads, not the clean step's one
        assert_eq!(grid.l3_share.map(|s| (s.threads, s.block_bytes)), Some((6, 1_327_104)));
    }

    /// A setting that had fewer threads measure the configured L3 step: the note counts its measurers, one a core, and the grid keeps the
    /// plan for the column's heading
    #[test]
    fn a_thinned_configured_step_counts_its_measurers() {
        let mut result = perf_run();
        result.run_config.threads.active_threads_per_core = 2;
        let at = result.run_config.tests.iter().position(|t| t.test == "Lat-L3-Read").unwrap();
        let mut plan = result.run_config.tests[at + 1].measure.clone().unwrap();
        (plan.threads, plan.measurers) = ("2".to_string(), 2);
        result.run_config.tests[at].measure = Some(plan);
        let grid = grid_of(&result);
        assert_eq!(grid.l3_share.map(|s| (s.threads, s.threads_per_core)), Some((2, 1)));
        assert_eq!(grid.latency_plan.map(|p| p.threads), Some("2".to_string()));
    }

    /// Lat-Turnaround-DRAM in the run but never reached; over two cycles, a level's figures are the mean of the cycles it matched in
    #[test]
    fn under_load_not_reached_and_over_cycles() {
        let not_reached = perf_run_of(&[PerfStep::turnaround()], &[None]);
        assert_eq!(perf_summary(&not_reached).unwrap().under_load, Some(UnderLoadPart::NotReached));

        let first = loaded_summary(true);
        let mut second = first.clone();
        for row in second.rows.iter_mut().filter(|r| matches!(r.load, Load::Level(_))) {
            if let Some(writes) = row.writes.as_mut() {
                writes.ns += 2.0;
            }
        }
        let run = perf_run_of(&[PerfStep::turnaround()], &[Some(first.clone()), Some(second)]);
        let Some(UnderLoadPart::Measured(under)) = perf_summary(&run).unwrap().under_load else { panic!("measured") };
        assert_eq!(under.cycles, 2);
        let want = first.rows.iter().find(|r| r.load == Load::Level(0)).unwrap().writes.as_ref().unwrap().ns + 1.0;
        let LevelState::Measured { writes_ns, .. } = under.levels[0].state else { panic!("25% measured") };
        assert!((writes_ns - want).abs() < 1e-9, "{writes_ns} against {want}");
        assert_eq!((under.levels[0].cycles, under.levels[0].cycles_matched), (2, 2));
        assert_eq!((under.levels[2].cycles_matched, under.levels[2].state), (0, LevelState::NotMatched));
    }

    /// The results file keeps the grid and the under-load table for tools, unrounded, and reads them back; a cell whose step a stop came
    /// before is absent and named in `not_reached`. serde_json reads a float back to within an ulp, so figures are compared near.
    #[test]
    fn the_results_file_keeps_the_figures_for_tools() {
        let result = perf_run();
        let json = serde_json::to_value(&result).unwrap();
        let perf = &json["overall_stats"]["perf"];
        let near = |v: &serde_json::Value, want: f64| assert!((v.as_f64().unwrap() - want).abs() < 1e-6, "{v} against {want}");
        assert_eq!((&perf["grid"]["tiers"][0]["tier"], &perf["grid"]["widths"]), (&serde_json::json!("L1"), &serde_json::json!([512])));
        near(&perf["grid"]["tiers"][0]["read_mib_s"], 1_577_506.0);
        near(&perf["grid"]["tiers"][1]["latency_p50_ns"], 2.74);
        near(&perf["grid"]["l3_share"]["l2_share_estimated"], 1014.0 * 1024.0 / 1_327_104.0);
        near(&perf["grid"]["tiers"][2]["latency_clean_p50_ns"], 8.7);
        assert!(perf["grid"]["tiers"][1].get("latency_clean_p50_ns").is_none(), "the L2 has no clean form");
        assert_eq!((&perf["grid"]["l3_clean"]["per_l3"], &perf["grid"]["l3_clean"]["target_unmet"]), (&1.into(), &true.into()));
        assert_eq!(perf["grid"]["memory_clean"]["block_bytes"], 1u64 << 30);
        near(&perf["grid"]["tiers"][3]["write_latency_p50_ns"], 62.0);
        assert_eq!(perf["grid"]["memory_write"]["measurers"], 1);
        assert!(perf["grid"].get("latency_plan").is_none(), "every thread measured the configured column");
        assert_eq!(perf["under_load"]["state"], "measured");
        assert_eq!(perf["under_load"]["levels"].as_array().unwrap().iter().map(|l| l["state"].as_str()).collect::<Vec<_>>(), [
            Some("measured"),
            Some("measured"),
            Some("not_matched")
        ]);
        assert!(perf["under_load"]["levels"][2].get("writes_ns").is_none(), "an unmatched level keeps no latencies");

        let back: TestRunResult = serde_json::from_value(json.clone()).unwrap();
        let grid = back.overall_stats.perf.and_then(|p| p.grid).expect("read back");
        assert_eq!(grid.tiers.iter().map(|t| t.tier.as_str()).collect::<Vec<_>>(), ["L1", "L2", "L3", "Memory"]);

        let stopped = perf_run_of(&[PerfStep::speed("Spd-L1-Read-Auto", "Spd-L1-Read-512_A", 0.0)], &[]);
        let tier = &stopped.overall_stats.perf.as_ref().and_then(|p| p.grid.as_ref()).unwrap().tiers[0];
        assert_eq!((tier.read_mib_s, tier.not_reached.as_slice()), (None, &["read".to_string()][..]));
        // A results file written before the block loads without it
        let mut older = json;
        older["overall_stats"].as_object_mut().unwrap().remove("perf");
        assert!(serde_json::from_value::<TestRunResult>(older).unwrap().overall_stats.perf.is_none());
    }
}
