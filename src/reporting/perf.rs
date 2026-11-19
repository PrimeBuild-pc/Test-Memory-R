//! The run's figures a cache tier, and memory's latency under load (TODO 119), as the final summary's grid and its under-load table show
//! them. Built from the saved steps alone, so a run's end, `--show-results` and an older results file (v0.1.1 on) give the same figures.
//! A row, a column or the under-load table appears only when a step that fills it ran; neither for a `config=` run, whose tests can carry
//! the built-in names with other settings.

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
}

/// The bandwidth tests' operations, in the grid's column order
pub const OPS: [&str; 3] = ["Read", "Write", "Copy"];

/// A bandwidth test's names, the one taken first: `-Auto`, else the widest width that ran
const WIDTH_SUFFIXES: [&str; 4] = ["Auto", "512", "256", "128"];

const TURNAROUND_TEST: &str = "Lat-Turnaround-DRAM";

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
        if speed.iter().any(Option::is_some) || latency.is_some() {
            rows.push(TierRow { tier, speed, latency });
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
    })
}

fn l3_share(result: &TestRunResult, steps: &Steps<'_>) -> Option<L3Share> {
    let block_bytes = steps.named(Tier::L3.latency_test()).first()?.config.extent_bytes;
    let calibrated = result.run_config.calibration.as_ref().and_then(|c| c.tiers.iter().find(|t| t.tier == "L2")).map(|t| t.optimal_size.0);
    let threads = &result.run_config.threads;
    Some(L3Share {
        threads: threads.thread_count,
        threads_per_core: threads.active_threads_per_core,
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
}
