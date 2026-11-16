# TODO 119: a summary grid at a run's end

A grid of bandwidth and latency per tier, like AIDA64's Cache & Memory Benchmark, and below it what AIDA64 doesn't measure: memory
latency under load and the write turnaround (TODO 117). Planned 2026-11-16 for v0.1.2 from a design workflow (four readers, a
designer, a skeptic) over the code and the 2026-11-13 `plan=perf` results of four hosts. Nothing built yet.

## Decided (the owner, 2026-11-16)

- **In v0.1.2, the whole scope:** the grid, the under-load table, the results file's summary block and the window's placement.
- **Show only what ran.** The plan's tests run as ordinary tests. At the report, a row appears only when a test of its tier ran, a
  column only when some row has a figure for it, and the under-load table only when Lat-Turnaround-DRAM ran. A cell whose step ran
  but didn't finish reads n/a. Neither table shows for a `config=` run: a config can give a built-in test's name other settings.
- **Both units, GiB/s and MiB/s,** side by side. Both binary, so they agree exactly; GB/s would be 4.9% off the MiB/s beside it.
  An exception, for this grid, to one form a quantity.
- **The L3 row stays.**
- **Beyond AIDA64: a small table, not one line.** Idle, then each load level the step was given, by read latency under NT writes,
  under reads, and the turnaround. The flat-out rows stay in the step's own report. On daddy-petz, 2026-11-13, Lat-Turnaround-DRAM's
  flat-out row under writes read 174.4 ns in run 1 and 222.3 in run 4, and within run 4 Lat-Loaded-DRAM's read 177.3, while the 75%
  rows held within about 1 ns over all four runs. Flat out the writes and the reads also run at different loads, so their difference
  misleads.

## To settle before building

### 1. Latency on one thread (proposed)

The L3 tests are sized by TODO 22's guards: half of what a thread reaches in the L3 with the run's threads sharing it, never below
its L2 plus 64 KiB (`CacheTarget::L3`, src/tests.rs:171-176; `CalibrationResults::l3_reach`, src/calibration.rs:241). That keeps
them in the L3 and out of DRAM, and the bandwidth figures show it: L3 reads at 0.62-0.75x L2's on every host. But every thread
chases at once (thread_pool.rs:492), and where each thread's L3 share is small beside its L2, a random chase over that block still
finds most of its lines in the L2:

| Host | L2 a core | L3 test's block | Block / L2 | Lat-L2-Read | Lat-L3-Read |
|---|---|---|---|---|---|
| daddy-petz (Zen 3, 8 threads) | 512 KiB | 1.93 MiB | 3.9x | 2.4 ns | 8.3 ns |
| petz007 (Zen 4, 6) | 1 MiB | 1.27 MiB | 1.27x | 2.7 ns | 4.6 ns |
| desktop-v6v7goc (Zen 4/4c, 6) | 1 MiB | 1.39 MiB | 1.39x | 3.5 ns | 6.1 ns |
| Xeon 8488C metal (96) | 2 MiB | 2.04 MiB | 1.02x | 4.9 ns | 5.5 ns |

Backing the L2 share out puts petz007's L3 near 11 ns. AIDA64 measures latency on one thread. Proposal: Lat-*-Read in the grid run
one thread, the others sitting out as the loaded tests' do (`loaded::Role::SitsOut`), its block half the one-thread reach (anchor
A's). To decide: the L3 only, or the whole latency column, so L1 and L2 are at one core's clock too (all cores busy run a lower
clock). A measurement change either way: before and after `plan=perf` on daddy-petz and petz007 (a few minutes each), and the
results' `--compare-results` across the change shows a moved figure, pre-1.0.

### 2. A write latency (proposed)

TODO 117's probes (`../write-latency-test/`, 117-write-latency.md:19-62) timed five idle write methods: an NT write + SFENCE
(W-nt+sf), a write + CLFLUSHOPT + SFENCE, CLWB, and two read-after-write forms. None moved with tWR (12 to 24): the controller
accepts a write when it is queued, before DRAM. They did move with the memory profile (2133 to 3600: -25 to -26%, mostly FCLK) and
held steady (W-nt+sf 91.8 ns at both tWR). TODO 117 proposed W-nt+sf as the write figure, with RAW-nt less R as its check, and left
the choice to the owner; the build took latency under load and the turnaround instead, and W-nt+sf was never ported. TMR's own
Lat-*-Write tests are a read chase with a store beside it, off the chain's path (latency_tests.rs:251-310): not a write's latency.
Options: leave write latency out (what writes cost a read is in the under-load table), or port W-nt+sf as a test and show it as the
Memory row's write latency, labelled as the time for the controller to accept a write. Its hardware check joins 1's.

### 3. The Memory row's latency

The one-thread idle figure, as AIDA64's: Lat-DRAM-Read once it runs on one thread (1), else Lat-Turnaround-DRAM's idle row (one
core, the others parked). Never today's Lat-DRAM-Read, every thread chasing: it read 63.5 against 67.0 on petz007 and the other way
on the Xeon (134.6 against 129.9).

## What each cell comes from (checked against the code and the 2026-11-13 results)

- **One source:** a pure `perf_summary(&TestRunResult) -> Option<PerfSummary>`, stored in `FinalTestSummaryReport.perf` by the
  converter (reporting/converters.rs:640). Live runs, `--show-results` (commands.rs:462-469) and files from v0.1.1 on (the loaded
  levels came in b4ad1ad, before the tag) all go through it; display never reads a saved block.
- **Finding a step:** by `run_config.tests[step-1].test`, the plan's name (Spd-L1-Read-Auto), never the resolved one
  (Spd-L1-Read-512_A); then its `overall_stats.per_step_averages` entry, steps merged as the per-test table merges them
  (converters.rs:651-690), so the grid equals that table by construction. `runs == 0` reads n/a, never 0. A step with
  `stopped_by == "stopped"` is marked as cut short.
- **Bandwidth:** `avg_throughput_mib_s`, all threads' bytes over the slowest thread's time (results.rs:383, runner.rs:1689). Copy
  counts the bytes read and written (bandwidth_tests.rs:304). The Memory row's write and copy run NT stores (runner.rs:2196-2205),
  the caches' plain stores: the footer says so.
- **The Memory row's bandwidth:** Spd-DRAMSmall-*, else Spd-DRAMFull-* (their read and write agree within 1.2% on every host); the
  footer names which.
- **Under load,** from Lat-Turnaround-DRAM's `cycles[].tests[].loaded` (results.rs:246; loaded.rs:482-505):
  - Levels found by value, `levels.given[i]` to the row `Load::Level(i)`, never by position; labelled as the step's table labels
    them (formatters.rs:1707-1752), so `load-levels=` shows its own.
  - Cells `writes.ns`, `reads.ns`, and `turnaround.ns` as stored, signed (`{:+.1}`). The turnaround is the paired ABBA statistic
    (loaded.rs:637-650), so it can differ from the columns' difference by a few tenths; the footer says it's paired. Negative is
    real: Zen 4's reads cost more than its writes.
  - "not matched" only when both cells were measured and `matched` is false; a level a stop never reached reads n/a. A `partial`
    step marks the title.
  - cycles > 1: each level the mean of the cycles it matched in, the count shown when short; the target as mean and range.
- **Titles:** the width from the Spd steps' `simd_width_bits` ("mixed" if they differ), threads, nodes above one; the under-load
  title names the measurer's node and the idle ones.
- No AIDA64 name in the UI. No repeatability claims in footers.

## Layout (petz007, Ryzen 5 8600G, DDR5-6000, 2026-11-13)

```
Cache & Memory: 512-bit, 6 threads
Tier    Read GiB/s  Write GiB/s  Copy GiB/s  Read MiB/s  Write MiB/s  Copy MiB/s  Latency
L1          1540.5        852.1      1671.5     1577506       872545     1711623   0.8 ns
L2           850.7        824.4       831.4      871126       844195      851371   2.7 ns
L3           634.9        614.3       620.4      650150       629007      635318   (1)
Memory        59.5         59.5        73.5       60926        60894       75307  67.0 ns
Every thread at once. Copy counts the bytes read and written. Memory: write and copy as NT stores. Latency: one thread, the others idle.

Memory under load: one core's read latency while 5 other cores load memory
Load                Under NT writes  Under reads  Turnaround
idle                        67.0 ns
25% (14936 MiB/s)           76.8 ns      77.8 ns     -0.9 ns
50% (29872 MiB/s)           86.5 ns      90.8 ns     -4.3 ns
75% (44808 MiB/s)           96.9 ns     113.6 ns    -16.6 ns
Load: shares of the others' lower flat-out peak, 59744 MiB/s.
Turnaround: what a read pays when the others write rather than read at the same rate (paired); it moves with tRDWR.
```

ASCII cells, no emoji or colour inside them (the table helper counts bytes). About 95 columns. The turnaround at 75% separates
controllers: +159.6 ns on the Xeon, +16.0 on Zen 3 (daddy-petz), -16.6 on Zen 4 (petz007). TODO 117's one-timing-at-a-time sweep
on both desktops moved it with tRDWR (+12 / +24 ns at 50 / 75%) and not with tWTR_L, tWTR_S or tWRRD, likely because the
controller drains writes in batches and seldom turns back to reads (an inference).

## The results file

`OverallStats.perf: Option<PerfSummary>`, `#[serde(default, skip_serializing_if = "Option::is_none")]` (results.rs:333), set in
`finalize()` so mid-run saves carry it, unrounded, absent cells absent keys. For scripts (the hw-pass compare kit, spreadsheets);
display never reads it. Name a level's count `cycles_matched`, not `matched` (the row's own boolean). No result struct denies
unknown fields, so v0.1.1 still loads v0.1.2's files.

## The window

Both tables reach the final summary panel through `Report::of_summary` (crates/tmr-gui/src/report.rs:99-122) with no window code,
at its end. The change: `report_final_summary_with(report, PerfTables::First | Last)`, the console Last, the window First, so the
grid sits under the verdict in the panel that opens at a run's end (model.rs:1145-1148); a `TableData` emphasis flag the console
ignores and the window draws as 14 pt cells, a semi-bold first column and striped rows (window.rs:1469-1511). No new tab: it would
sit empty on every stability run.

## Build order (about 14-18 agent hours with 1 and 2; 9-10 without)

1. `PerfSummary` and `perf_summary()`, with unit tests: the gates, the plan-name join, runs 0, stopped steps, levels by value, not
   matched, partial, the cycle mean, nodes. The only place a wrong number can come from. About 3 h.
2. The two tables in `report_final_summary` (src/reporting/mod.rs:124), after the Intermediate table; a perf golden, and
   `show_results.txt` extended so the round trip covers both. `test_support::loaded_summary(true)` already builds a loaded fixture
   with a not-matched row. About 3 h.
3. Settle 1 and 2: the one-thread latency mode and, if chosen, the W-nt+sf test; hardware before and after. About 4-8 h.
4. The results file's block. About 1 h.
5. The window's order and emphasis, with a mount test. About 2 h.
6. guide.md and docs\tests (reading the summary), closing TODO 119. About 1 h.

Then the batch's full `tools/ci_local.py`, and the release.

## Known limits

- On the hybrid 8500G the L1/L2 median is the Zen 4c cores' (4 of 6 threads): L1 1.06 ns against 0.88 on its Zen 4 cores.
- `cputype=cores` runs one thread a core, so on an SMT CPU the cache bandwidth isn't AIDA64's every-logical-CPU figure.
- `--compare-results` compares steps, not grids: a grid against a grid is new work.
- The Xeon's DRAMSmall copy is 11% below its DRAMFull copy (315987 against 355310 MiB/s), unexplained; not a blocker.
