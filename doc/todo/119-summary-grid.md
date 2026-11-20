# TODO 119: a summary grid at a run's end

A grid of bandwidth and latency per tier, like AIDA64's Cache & Memory Benchmark, and below it what AIDA64 doesn't measure: memory
latency under load and the write turnaround (TODO 117). Planned 2026-11-16 for v0.1.2 from a design workflow (four readers, a
designer, a skeptic) over the code and the 2026-11-13 `plan=perf` results of four hosts; revised 2026-11-17 after three verified
investigations (roles and thread layout, the write-latency data, the L3 rule), and again 2026-11-19 after Astra's review of the
proposal (`119-proposal-review-2026-11-19.md`, its arithmetic in `research/latency_proposal/review.py`). Nothing built yet.

## Decided (the owner, 2026-11-16)

- **In v0.1.2, the whole scope:** the grid, the under-load table, the results file's summary block and the window's placement.
- **Show only what ran.** The plan's tests run as ordinary tests. At the report, a row appears only when a test of its tier ran, a
  column only when some row has a figure for it, and the under-load table only when Lat-Turnaround-DRAM ran. A cell whose step ran
  but didn't finish reads n/a. Neither table shows for a `config=` run: a config can give a built-in test's name other settings.
- **Both units, GiB/s and MiB/s,** side by side. Both binary, so they agree exactly; GB/s would be 4.9% off the MiB/s beside it.
  An exception, for this grid, to one form a quantity.
- **The L3 row stays.**
- **Latency both ways, as configured and constrained** (the owner, 2026-11-17: liked "both"; which one a run gives depends on its
  plan or parameter). The rule for the constrained form is below.
- **Beyond AIDA64: a small table, not one line.** Idle, then each load level the step was given, by read latency under NT writes,
  under reads, and the turnaround. The flat-out rows stay in the step's own report. On daddy-petz, 2026-11-13, Lat-Turnaround-DRAM's
  flat-out row under writes read 174.4 ns in run 1 and 222.3 in run 4, and within run 4 Lat-Loaded-DRAM's read 177.3, while the 75%
  rows held within about 1 ns over all four runs. Flat out the writes and the reads also run at different loads, so their difference
  misleads.

## Latency on the shared tiers: as configured, or a clean read (proposed 2026-11-17)

The L3 tests are sized by TODO 22's guards: half of what a thread reaches in the L3 with the run's threads sharing it, never below
its L2 plus 64 KiB (`CacheTarget::L3`, src/tests.rs:171-176; `CalibrationResults::l3_reach`, src/calibration.rs:241). That keeps
them in the L3 and out of DRAM. With every thread running, each thread's block is small beside its L2:

| Host | L2 a core | L3 tests' block | Block / L2 | Spd L3 / L2, read | Lat L3 / L2 | Lat-L2-Read | Lat-L3-Read |
|---|---|---|---|---|---|---|---|
| Xeon 8488C metal (96 threads) | 2 MiB | 2.04 MiB | 1.02x | 0.62 | 1.13 | 4.9 ns | 5.5 ns |
| petz007 (Zen 4, 6) | 1 MiB | 1.27 MiB | 1.27x | 0.75 | 1.68 | 2.7 ns | 4.6 ns |
| desktop-v6v7goc (Zen 4/4c, 6) | 1 MiB | 1.39 MiB | 1.39x | 0.75 | 1.72 | 3.5 ns | 6.1 ns |
| daddy-petz (Zen 3, 8) | 512 KiB | 1.93 MiB | 3.85x | 0.63 | 3.42 | 2.4 ns | 8.3 ns |

- **Bandwidth doesn't follow the block's size** (0.62 at 1.02x, 0.63 at 3.85x). A sequential pass over a block even a little larger
  than the L2 finds none of it there when it comes round again: the L2 keeps the most recent lines, and the next one wanted is
  always the oldest. So the pass comes from the L3, and every thread running is what L3 bandwidth should be: the L3's total.
- **Latency follows it** (1.13 at 1.02x, 3.42 at 3.85x). A random chase finds a line still in the L2 with about the chance L2 /
  block.
- **A sample is the mean of 1000 chained loads** (src/latency_tests.rs:169, :192-194), so the P50 is a blend, `(1-h)*L2 + h*L3` at
  L3 hit share h. It is steady within a run (per-thread P50s within 0.05 ns at L3) and never snaps to the L3 once h passes 50%. At
  50-60% hits it reads 64-72% of the L3's latency. Between runs it moves with the block calibration gives it: daddy-petz read 8.30
  ns at 1.93 MiB and 8.03 at 1.69 MiB, as the blend predicts (0.26 ns predicted, 0.27 measured).

One thread's latency against its block, from calibration anchor A's sweep (one thread, the same chain as Lat-L3-Read), ns:

| Host | L2 | 1.25x L2 | 2x | 4x | 8x | Top of the sweep | L3, fitted |
|---|---|---|---|---|---|---|---|
| petz007 | 2.7 | 4.3 | 6.4 | 8.2 | 9.0-9.1 | 9.3-9.4 | 9.9-10.05 |
| daddy-petz | 2.5 | 3.9 | 6.3 | 8.2 | 9.0 | 9.7-10.1 | 10.0 |
| Xeon 8488C | 4.1 | ~11.9 | 22.1 | 29.7 | 33.2 | 35-36.4 | 37.0 |

A clear read, within about 10% of the L3, takes 85-90% L3 hits: a block 7-9x the thread's L2.

The proposal (2026-11-19, for Astra to validate against data already taken before anything is built):

**Where it runs: inside the normal run, a step at a time.** The run's threads are chosen once, as now (`cputype=`, `cpus=`,
`skip-cores=`, `cpu-stride=`, `core-priority=`), pinned, each with its memory on its own node. A latency step decides which of them
measure; the rest sit out for that step, as Lat-Loaded-DRAM's do. No separate cycle, no reselection mid-run.

**A sit-out is an explicit role in one per-step plan**, built before any worker's hot loop: the eligible workers, each one's role,
its CPU, cache domains and node, its extent, the estimated share, whether the target was met, and any allocation cap. Not an empty
extent: `test_extent` raises a zero extent to one 4 KiB granule when the thread has memory (src/test_scaffolding.rs:201-203), so it
would test 4 KiB. The step's P50 already merges only non-empty histograms (runner.rs:4810-4815), but the rest must follow the plan:
per-thread rows mark a sit-out idle (not 0 ns, not unfinished), cycle minima leave it out (runner.rs:1573-1614, 4827-4847), the
seal-before-step bookkeeping knows it touches nothing (thread_pool.rs:368-399), and extents and sharers come from the chosen
measurers, not the run-wide globals (src/tests.rs:482-502). Other cores asleep can change clocks and uncore state: part of what
the clean figure means, not a code guarantee.

**Three settings,** each a run parameter and a per-test config field; a plan sets its own defaults:
- `latency-threads=all|clean|N`: `all`, every thread (today's, the default); `clean`, as many a shared cache of the test's tier as
  still reach `latency-hits=` hits in that tier, the rest sitting out; `N`, exactly N.
- `latency-hits=88%`: clean's target. A tier whose cache isn't shared (L1, and L2 on most CPUs) gets every thread under `clean`.
- `latency-cores=ranked|performance|efficiency`: an eligibility filter, then the run's order, top-ranked first (`ranked` is
  today's thread 0). Types come from TMR's existing classification (src/cpu_topology.rs:411-428): Windows' efficiency classes, else
  AMD's CPUID core type, else Windows' core priority under `core-priority=hybrid` (the 8500G's Zen 4c cores). A filter that leaves
  no core is refused with the reason, never measured on another type. N is a total, spread over the domains in rank order; zero, or
  more than the eligible cores, is refused. A run parameter overrides a `-Clean` test's fixed setting.

**plan=perf** runs L1 and L2 as now, and the L3 and DRAM read tests both ways: today's (every thread) and a `-Clean` variant with
`latency-threads=clean` fixed, as `-256` and `-Auto` fix a width, since plans list tests by name. The clean DRAM read is one thread
over the loaded tests' block, `max(1 GiB, 32 L3s)`: one thread at Lat-DRAM-Read's 4 L3s has the L3 to itself and reads 18-22% low
(calibration: about 52 ns against 66 every thread on the 8500G, 105 against 135 on the Xeon). It is a step of its own, the 8 B
chain on one thread with the others asleep, its extent as actually allocated recorded (the rule can't promise 1 GiB to a thread
with less memory). Not Lat-Turnaround-DRAM's idle row: that chains one cell per 128 B and its loaders spin with PAUSE in C0
(src/loaded.rs:15-25). The grid shows a latency column a form that ran, headed with its threads and, for clean, the estimated
share.

**Sizing a clean step:** the largest k (measurers a physical L3) whose block, `max(0.5 * l3_reach(k), L2 + 64 KiB)`, gives an
estimated share `1 - L2 / block` of at least the target, L2 being the calibrated L2 the block's floor already uses (A's, on the
measuring core; CPUID's without a calibration). The share is an estimate from the blend model, which fits the sweeps, not a hit
counter: it is shown as "est.". If even one thread falls short, it runs one and the cell says "target unmet" with the estimate:
petz007, k=1, est. 82% (about 8.7 ns from its sweep). About 3 on daddy-petz (2 or 3 by calibration: the physical 512 KiB L2 gives
2 on its last run, the calibrated 483 KiB 3), about 3 a socket on the Xeon. 88% is a heuristic (model shortfall about 9% on
petz007, 11% on the Xeon), not a proven optimum. The 0.5 scale stays until the new step is measured.

**What it can't do:** measure on CPUs the run didn't choose. Latency on an SMT sibling under `cputype=cores`, or on cores a
`skip-cores=` range left out, takes a run set up for them.

**What sitting threads out keeps and loses** (src/cpu_selection.rs:129-317, src/loaded.rs:100-144, :1093-1099):
- **The layout logic picks the measurer.** Thread 0 is the first CPU of the top-ranked core: performance cores first, then
  Windows' scheduling priority, groups taking turns by node. That is petz007's CPU 6 (core 3), daddy-petz's CPU 2 (core 1), the
  8500G's Zen 4 core 0, and the Xeon's CPU 0 on node 0. Calibration anchor A runs on the same core, so `l3_reach(1)` is its own.
- **A sit-out keeps its CPU and its memory** (pinned once, its span allocated on its own node before the pool starts). It skips the
  step and sleeps in an OS wait, so its core can idle.
- **Lost: breadth.** One core, not a median over cores. Small on uniform CPUs (per-core L3 P50s within 0.05 ns, DRAM about 2 ns, the
  Xeon's two sockets equal). Large on the hybrid 8500G, whose Zen 4c cores read L3 at 6.2 ns against its Zen 4 core's 5.3; today's
  merged figure is the Zen 4c cores' (four of six threads). Thread 0's tail (P99) is among the noisiest on petz007, the Xeon and the
  EPYC (inferred: OS work on the preferred core, or CPU 0).
- **Two cases where the ranking doesn't pick it:** `cpu-stride=` (the measurer becomes the lowest-numbered core) and a
  `skip-cores=A-B` range covering the top core.

**Building it needs what the loaded tests don't have:**
- More than one measurer for the ordinary read tests. The loaded tests keep one: choosing it rebuilds their same-node loader set
  around it, its SMT siblings out, and resizes the participant barrier. The ordinary tests' default `all` never means "every
  worker measures" in a loaded test.
- The block sized from k, not from the run's thread count (src/tests.rs:968, :1017; src/session.rs:575).
- The core-type filter (`latency-cores=`) over the existing classification.
- L2 sharers counted too: an Intel E-core cluster shares one L2 between four cores, so `clean` applies at L2 there. No Intel
  hybrid is reachable to check it on.
- Cache domains from the OS's own sharing sets, `GetLogicalProcessorInformationEx(RelationCache)`'s `GroupMasks` (which span
  processor groups), for L1, L2 and L3: not (processor group, LastLevelCacheIndex) (src/cpu_topology.rs:28-33), which shows the
  Xeon's one 105 MB L3 a socket as two groups, and not "a node or socket is one L3" (chiplets, SNC). Calibration's anchor C
  sharer count was taken under the old identities: version it, and recheck rather than reread an old one.

Open, the owner's call:
- On petz007 the 0.5 scale is what binds above 82%. Its one-thread sweeps are clean to 8.4-12.8 MB against a DRAM edge at 14.7-16.9
  MB, so the clean form could take 0.65-0.75 of its reach. A margin still matters: each 1% of loads reaching DRAM costs about 0.55 ns.
- A chain with one node per line (calibration's, `setup_line_chase`) reads closer to the L3 at the same block (petz007 6.8 against
  4.6 ns at about 1.3 MB). It would change every Lat-*-Read figure and break comparison with the baselines: separate from this.

## Write latency: W-nt+sf (ported 2026-11-19 as Lat-DRAM-NTStore-SFence, step 4)

TODO 117's probe (`../write-latency-test/src/main.rs`) timed one thread doing one write at a time to random lines in 2 GiB on large
pages. W-nt+sf (:141-155) is a full-line NT write (two 32 B NT stores), then SFENCE, so each write waits until the previous line is
accepted. Its alternatives: W-nt+mf (MFENCE), W-fl (store, CLFLUSHOPT, SFENCE), W-clwb, RAW-nt and RAW-fl (write the next pointer,
MFENCE, read it back). R is the read chase.

The record (117-write-latency.md:163-164, 2026-11-05) proposed W-nt+sf because the write methods "moved alike", with RAW-nt less R
as its check, and left the choice to the owner. There was no winner on the data: W-nt+sf was chosen on design grounds, having no
read inside it (unlike W-fl and the read-backs) and not paying MFENCE's fixed 14-16 ns. The build took latency under load and the
turnaround instead, and TODO 117 closed on 2026-11-13 without carrying the choice.

daddy-petz (Ryzen 7 5700X, DDR4; 2133 at FCLK 1067, 3600 at FCLK 1800), ns, from the raw probe outputs:

| Method | 2133 | 3600 | tWR 24 | tRDWR 12 | tFAW 24 | tRRD_L 6 | tWTR_L 12 | tWRRD 6 | 10 boots at 3600 |
|---|---|---|---|---|---|---|---|---|---|
| R | 101.7 | 65.4 | 65.5 | 65.4 | 65.2 | 64.8 | 64.8 | 65.2 | 64.8-65.8 |
| W-nt+sf | 123.6 | 91.8 | 91.8 | 91.6 | 91.7 | 90.2 | 90.4 | 91.5 | 90.2-91.8 |
| W-nt+mf | 140.2 | 108.2 | 108.3 | 108.1 | 108.0 | 106.6 | 106.5 | 107.8 | 106.5-108.3 |
| W-fl | 195.0 | 134.6 | 134.3 | 134.1 | 133.9 | 132.0 | 132.1 | 133.8 | 132.0-134.6 |
| RAW-nt | 236.0 | 166.2 | 166.0 | 165.9 | 167.4 | 163.8 | 163.7 | 165.4 | 163.7-167.4 |
| W-nt+sf / R | 1.216 | 1.404 | 1.401 | 1.401 | 1.405 | 1.392 | 1.396 | 1.402 | 1.392-1.413 |
| TMR Lat-DRAM-Write | 125.8 | 78.3 | 78.6 | 78.5 | | | | | |

petz007 (Ryzen 5 8600G, DDR5-6000, FCLK 2000), 7 boots across tFAW, tRRD_L, tWTR_L and tWTR_S: R 65.7-66.5, W-nt+sf 61.8-62.3
(below the read), W-nt+mf 76.8-77.4, W-fl 91.3-91.9, RAW-nt 127.1-128.6. The m8i.metal's Xeon 6975P-C (not the 8488C): R 131.9,
W-nt+sf 181.7.

What it shows:
- **No idle method, the read included, moved with any one timing** beyond the boot-to-boot spread, on either desktop. Every timing
  tried is a gap between two DRAM commands, and one request at a time over random lines in many banks seldom waits on one. So the
  null result says nothing either way about where a write is acknowledged. (117-write-latency.md:51-52's "the controller
  acknowledges a write when it is queued" is supported only on Zen 4, where the write is faster than the read.)
- **W-nt+sf differs from R by platform:** a write takes 26 ns longer than a read on Zen 3, 4 ns less on Zen 4, 50 ns longer on the
  Xeon 6975P-C. And over the whole 2133-to-3600 change it fell 26% where the read fell 36%.
- **Never varied alone:** FCLK, tCWL (on a write's path, not a read's), tCL and tRCD (a read's). Those could separate W-nt+sf from R.
- **TMR's Lat-DRAM-Write** (`write_latency_multi`, src/latency_tests.rs:253-310) is a read chase with a plain store to a second random
  line each step, off the chain's path: reads with a store stream beside them, about 13 ns over Lat-DRAM-Read. Not a write's latency.

Already run: tWR, tRDWR, tFAW, tRRD_L, tWTR_L, tWRRD, tWTR_S, PHYRDL, and 2133 against 3600 (everything at once, FCLK
included). Never alone: FCLK, tCWL, tCL, tRCD.

Recommended (2026-11-19, Astra agreeing): port it now and check its tuning value later, as its own test, named for the operation:
"NT store + SFENCE, ns/64-B line". It times fenced full-line stores, address generation and platform queuing included; it doesn't
time a write reaching DRAM (SFENCE orders and makes stores globally visible, nothing more). The port keeps the probe's two 32 B NT
stores and SFENCE a line, and a fence at the batch's end before the closing RDTSCP, which doesn't wait for stores by itself; its
emitted code is checked, and its figure against the probe's on the same host. A new name, not Lat-DRAM-Write's: old results keep
their meaning, and retiring or renaming the old plan entries is separate from deleting their kernels.

Selecting the method needs no more data: the methods move alike, and W-nt+sf is the one that never reads the line it writes and
pays no MFENCE per line (W-fl reads the line in first; the read-backs pay MFENCE and a read). It still reads an address index each
line, and loop and fence work are in the figure. Replacing today's Lat-DRAM-Write, which a write-latency figure isn't, is a fix in itself. What more data
would add is whether a tuner can move it where the read doesn't, which only the never-run knobs can show.

Options:
- **Check first (no build):** run the existing probe on petz007 with FCLK alone, tCWL alone and tCL alone changed, two boots each:
  mostly reboot time. If W-nt+sf moves where R doesn't, it earns its cell.
- **Port now** as the DRAM write latency on one thread, named for what it times ("NT write + SFENCE round trip"), replacing
  Lat-DRAM-Write and Lat-DRAMFull-Write in plan=perf. Its name, and which read-chase-plus-store tests retire
  (Lat-{L1,L2,L3,DRAM,DRAMFull}-Write and the Lat-V2/V2P Write families), as a list approved before anything is deleted.

## For Astra: what to validate from data already taken (no runs, no builds)

1. The blend model, `(1-h)*L2 + h*L3` with h = `1 - L2 / block`, against calibration anchor A's one-thread sweeps
   (`baselines\2026-11-05-curve\`, and the `*-perf-1.txt` consoles in `baselines\2026-11-13\<host>\`), and the claim that a
   sample is the mean of 1000 chained loads (src/latency_tests.rs:169, :192-194).
2. The clean rule's k and predicted figure for petz007, daddy-petz and the Xeon 8488C, at 88% and at 50-60%, and whether 88% is
   the right default.
3. That the Xeon's one L3 a socket shows as two L3 groups (src/cpu_topology.rs:28-33;
   `baselines\2026-11-13\ec2amaz-6hbdp1e\116-metal-perf-1.txt`), and what counting per physical L3 takes.
4. What one measuring core loses: the per-thread P50s and P99s in the 2026-11-13 results (`cycles[].tests[].threads[]`), the 8500G's
   Zen 4 against Zen 4c, and thread 0's tail.
5. That a sit-out in `read_latency_multi` can be its empty-block early return without changing the measurers' figures.
6. The write-latency reading: the probe outputs in `baselines\2026-11-05-write-latency\` and `baselines\2026-11-05-timings\`, and
   whether porting W-nt+sf now, as an informational figure named for what it times, is sound.

Needs new runs, not validation: the clean figures at k > 1, measuring on efficiency cores (no Intel hybrid reachable), and the write
figure against FCLK, tCWL and tCL.

## The Memory row's latency

Configured: Lat-DRAM-Read, every thread (petz007 63.5 ns). Clean: its own step, the 8 B chain on one thread over the loaded
tests' extent rule, the others asleep; not yet measured. Lat-Turnaround-DRAM's idle row (petz007 67.0 ns, the Xeon 129.9) is a
different experiment (128 B cells, loaders awake) and stays in the under-load table. Each labelled with its thread count; never
one shown as another.

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
Tier    Read GiB/s  Write GiB/s  Copy GiB/s  Read MiB/s  Write MiB/s  Copy MiB/s  Latency, 6 threads*  Latency, clean
L1          1540.5        852.1      1671.5     1577506       872545     1711623               0.8 ns
L2           850.7        824.4       831.4      871126       844195      851371               2.7 ns
L3           634.9        614.3       620.4      650150       629007      635318               4.6 ns  (1 thread, 82%)
Memory        59.5         59.5        73.5       60926        60894       75307              63.5 ns         67.0 ns
Bandwidth: every thread at once. Copy counts the bytes read and written. Memory: write and copy as NT stores.
* 6 threads share the 16 MiB L3: each chases 1.27 MiB, 1.27x its L2, about 80% of the L3 row's reads in the L2.
Clean: the threads that reach 88% L3 hits a physical L3, the rest sitting out; Memory over 1 GiB on one core.

Memory under load: one core's read latency while 5 other cores load memory
Load                Under NT writes  Under reads  Turnaround
idle                        67.0 ns
25% (14936 MiB/s)           76.8 ns      77.8 ns     -0.9 ns
50% (29872 MiB/s)           86.5 ns      90.8 ns     -4.3 ns
75% (44808 MiB/s)           96.9 ns     113.6 ns    -16.6 ns
Load: shares of the others' lower flat-out peak, 59744 MiB/s.
Turnaround: what a read pays when the others write rather than read at the same rate (paired).
```

The clean L3 cell is a placeholder: petz007's clean L3 has not been measured (the one-thread sweep predicts about 8.7 ns). ASCII
cells, no emoji or colour inside them (the table helper counts bytes). About 115 columns.

The turnaround at 75% separates controllers across hosts (TMR's Lat-Turnaround-DRAM, 2026-11-13): +159.6 ns on the Xeon, +16.0 on
Zen 3 (daddy-petz), -16.6 on Zen 4 (petz007). What moves it is thinner than first said. tRDWR was changed once, on daddy-petz with
TODO 117's probe, one boot (2026-11-05 afternoon): its turnaround rose +12.3 ns at 50% and +23.6 at 75% (117-write-latency.md:64-73),
the 50% clear of the probe's spread, the 75% inside the later sweep's. TMR's own Lat-Turnaround-DRAM was never run with tRDWR changed,
nor on petz007. The sweep's tWTR_L, tWTR_S and tWRRD left it flat. So no footer claims a timing moves it.

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

## Build order (the owner, 2026-11-19: build it all, validate here first, the hardware later)

On branch `feat/summary-grid-119`, one commit a step:

1. Done (0fe737b): `PerfSummary` and `perf_summary()`, and the two tables in the final summary, with a golden.
2. Done (1311ba4): the results file's block, `overall_stats.perf`. (8f9aa5b) The window opens its summary on the two tables,
   drawn as headline tables, with a mount test.
3. Done (02587a9): L3 groups from the OS's cache records, so an L3 spanning processor groups is one group. (f7933cc) Per-step
   measurers and sit-outs (`measure.rs`), `latency-threads=`, `latency-hits=`, `latency-cores=`, plan=perf's `-Clean` steps,
   the grid's clean column and its note; a full-memory step keeps its own extent under a count.
4. Done (6ba4a4d): the write port, Lat-DRAM-NTStore-SFence (`latency_tests::nt_store_sfence_multi`): two 32 B NT stores and SFENCE a line, to
   random lines (xorshift, repeats allowed, no index table), MFENCE before the closing RDTSCP; one thread over the clean DRAM
   block. In plan=perf in place of Lat-DRAM-Write and Lat-DRAMFull-Write; the grid's Memory write note. Its asm read.
5. Done 2026-11-19: plan=perf three times on this dev box (the r8i VM, 4 threads) with the release exe, `memory=50% cputype=cores
   cpus=100% skip-cores=0`, 5 min 26 s each. Write 182.3 ns all three (the probe: 181.7 on the m8i metal box's same CPU); clean
   L3 30.4-30.5 ns, every thread 25.4; clean DRAM 137.8-137.9, every thread 103.3-106.0 (its 4 calibrated L3s, 308 MiB in all,
   fit the CPUID 480 MiB L3; the calibration's DRAM: 140.7). The run line and plan table showed a planned step's test extent on
   every thread; fixed (f2cfd4e).
6. Done: docs (5298be9, 51af41f); the spill table against main's, kernel for kernel the same but the new kernel's one gpr reload;
   the batch's full `tools/ci_local.py` (builds\51af41fe3727-1af69b810aa3282b). Merging to main is the owner's call.
   Review 2026-11-20 (`119-implementation-review-2026-11-20.md`), four findings, each checked against the code and fixed: a config's
   latency tests lost their settings (now their registration's, the entry's `latency_threads`/`latency_hits`/`latency_cores` over
   them, the command line's over both; refused on other tests); an L1/L2 measurer's block counted its idle sibling (now sized for the
   siblings measuring, `CacheTarget::size_bytes_sharing`, and the plan records `per_core`); the results file's layout of a planned
   step mixed the plan's extent with the unplanned chunk (now a measurer's planned config, whole); `--compare-results` ignored who
   measured (now a `measurers` setting change; no record is every thread).
7. Later, on the physical boxes: plan=perf on daddy-petz and petz007 (the write figure against the probe's there), the 8500G
   (`latency-cores=`), and the metal box (the L3 groups across processor groups).

Not built, deferred: the loaded tests' choosable measurer (latency-cores= for Lat-Loaded/Turnaround), and L2 sharing (an Intel
E-core cluster's four cores on one L2: clean would apply at L2 there; no Intel hybrid reachable). Open for the owner: which
read-chase-plus-store latency tests retire, as a list approved before anything is deleted.

## Known limits

- On the hybrid 8500G the configured L1/L2 median is the Zen 4c cores' (4 of 6 threads): L1 1.06 ns against 0.85 on its Zen 4 cores.
  The clean form measures one Zen 4 core.
- `cputype=cores` runs one thread a core, so on an SMT CPU the cache bandwidth isn't AIDA64's every-logical-CPU figure.
- `--compare-results` compares steps, not grids: a grid against a grid is new work.
- The Xeon's DRAMSmall copy is 11% below its DRAMFull copy (315987 against 355310 MiB/s), unexplained; not a blocker.
