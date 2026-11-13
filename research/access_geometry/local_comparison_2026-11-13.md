# Local before/after comparison, 2026-11-13

The owner requested this check before the physical-machine campaign: does adding grouped access slow the existing tests?
This compares the same G1 workloads across builds. It does not compare G1 against G2/G4/G8/G16 or rank error detection.

## Method

- Development VM: Amazon EC2 r8i.2xlarge, Intel Xeon 6975P-C, four exposed cores/eight logical CPUs, 64 GiB installed.
  Windows Server 2025 build 26100.33822. No concurrent build or other TMR run.
- Baseline `57369c4`, candidate code `72ab3b8`; the previously retained release binaries, compiled with the same
  Rust 1.102.0-nightly (`9c5648833`), MSVC 14.52.37025 and Windows SDK 10.0.28000.2957.
- Fixed 8 GiB target, four pinned workers on logical CPUs 0/2/4/6, 2 GiB per worker. Both page limits are `large`
  (2 MiB). Calibration is off; SIMD width 256, transfer profile 32 bits, seed 126, seal enabled, errors halt.
- Each correctness step covers the full span in 256 MiB chunks, eight chunks per worker with no overlap.
  Eight write rounds execute `ident, switch, random, switch` as verified P/~P pairs, with two reads per round.
  This is 192 GiB of counted reads/writes per step across four workers. The reference reads 128 GiB.
- One initial run per build used `baseline-compatible.json` to check setup and runtime. These pilot results are excluded.
  Its reference lasted only about 0.14 seconds, so the main config adds sequential controls and uses 16 reference sweeps.
- Three ABBA blocks, six runs per build, using `configs/local-regression.json`. Same configuration and data inputs
  for each build; physical placement can change between allocations. Each run has a 180-second cap.
- All blocks, including the longer follow-up, put the baseline in the outer slots and the candidate in the inner
  slots. ABBA cancels linear drift, but repeated use of this order does not balance a repeatable slot-dependent effect.
- Throughput uses each test's internal time, excluding setup/seal work. Absolute rates and within-run ratios to the
  unchanged bandwidth reference are retained. Normalization is a cross-check: sequential and strided walks need not
  respond identically to host contention.
- `summarize_ab.py` verifies completion, error totals, page mix, worker placement, test settings and processed bytes
  before comparing complete blocks. First/last repeats of each binary reveal variation and order effects. Three
  blocks do not establish a narrow confidence interval or rule out a small architecture-dependent regression.

Exact executable SHA-256:

```text
baseline   7885c01225c8cff9e2cf4e796fc33394a30cd470ade8f32ca5edac7231773f38
candidate  165d7eeea894ded3dbf806203350c879f6196d0d8744d2da5c1cf78dd3832099
```

Command from the app root:

```powershell
$env:TMR_HOME = Join-Path $PWD 'target\access-geometry\local-home'
python -B tools/bench_ab.py target/access-geometry/local-abba "Spd-DRAMFull-Read-256 (reference)" "config=research/access_geometry/configs/local-regression.json memory=8GiB-target cputype=cores cpus=4 skip-cores=0 cycles=1 duration=180 seed=126 width=256 beat=32 channels=2 calibrate=off minpage=large maxpage=large errors=halt" base_first=target/access-geometry/baseline.exe new_first=target/access-geometry/candidate.exe new_last=target/access-geometry/candidate.exe base_last=target/access-geometry/baseline.exe --runs 3
python -B research/access_geometry/summarize_ab.py target/access-geometry/local-abba
```

Raw console files and the derived summary are under `target/access-geometry/local-abba/`; original result JSON and
logs are under `target/access-geometry/local-home/`. The pilot is under `target/access-geometry/local-pilot/`.
These local artifacts are retained outside Git. The config and summarizer are tracked.

## Original twelve-run set

All twelve runs completed with identical settings/work, exactly 8 GiB of 2 MiB pages, and zero test, seal or WHEA errors.
Main test runtime was about a minute per run. Rates below are means across six runs per build; percentage changes are
the means of the three block-level changes, rather than treating individual workers as independent samples.

| Existing workload | Baseline, MiB/s | Candidate, MiB/s | Raw change | Reference-normalized change |
|---|---:|---:|---:|---:|
| SimpleTest, P512/G1 | 8,135 | 8,066 | -0.85% | +1.49% |
| Sequence, P512/G1 | 8,195 | 8,162 | -0.39% | +1.96% |
| SimpleTest, sequential | 45,200 | 43,936 | -2.71% | -0.51% |
| Sequence, forward | 45,894 | 43,611 | -5.08% | -2.95% |
| Unchanged bandwidth reference | 58,927 | 57,581 | -2.29% | — |

The blocks disagree:

| Workload | Block 1 | Block 2 | Block 3 |
|---|---:|---:|---:|
| SimpleTest, P512/G1 | -0.06% | -4.43% | +1.95% |
| Sequence, P512/G1 | +0.09% | -3.84% | +2.56% |
| SimpleTest, sequential | +0.56% | -10.60% | +1.90% |
| Sequence, forward | +0.37% | -14.13% | -1.47% |
| Unchanged bandwidth reference | +0.05% | -6.21% | -0.72% |

During block 2 the retained `activity.jsonl` sample records a concurrent Claude process using 11% of one logical CPU.
The earlier report of about 76% has no retained measurement and is withdrawn. The saved sample establishes concurrent
activity, not the cause of the slow block. Later CPU-time samples showed that process largely idle, but step-to-step
variation continued. Both binaries' sequential controls varied substantially:
the same candidate's sequential SimpleTest changed by +11.88% between consecutive runs, and the same baseline changed
by -9.65% between the first and last runs of block 3. The middle block is retained, not silently discarded.

This set alone cannot assign a small cost to the feature. In particular, normalization does not reliably cancel an
interruption that affects a four-second sequential step differently from a twenty-four-second strided step.

## Longer control steps

Because the first set was noisy, the follow-up uses `configs/local-regression-long.json`: unchanged strided steps,
four cycles of each sequential control and 64 reference sweeps. That increases their expected measured times from
about 4 to 16 seconds and 2 to 9 seconds, respectively. One additional ABBA block uses otherwise identical arguments,
with `duration=240`. CPU-time counters for Claude and Defender are sampled before and after each run, outside its timing.
The follow-up is kept separate because its work differs from the original set.

Its raw files are under `target/access-geometry/local-abba-long/`; the same summarizer reads that directory.

All four follow-up runs completed with the same allocation/settings/work and zero errors:

| Existing workload | Baseline, MiB/s | Candidate, MiB/s | Raw change | Reference-normalized change |
|---|---:|---:|---:|---:|
| SimpleTest, P512/G1 | 8,232 | 8,304 | +0.87% | +0.48% |
| Sequence, P512/G1 | 8,213 | 8,223 | +0.12% | -0.26% |
| SimpleTest, sequential | 45,831 | 45,925 | +0.21% | -0.17% |
| Sequence, forward | 45,293 | 46,222 | +2.05% | +1.66% |
| Unchanged bandwidth reference | 58,402 | 58,631 | +0.39% | — |

This was quieter, not idle. Claude's average CPU use was 10.18%, 1.14%, 1.67% and 5.97% of one logical CPU across
the four runs; Defender's was 2.51%, 5.10%, 1.74% and 0.91%. Those averages can hide bursts and say nothing about other
guests on the physical host. Same-build sequential differences still reached 5.61%, so the +2.05% row is not evidence
of a reliable speedup. Per-run measurements for both sets are preserved in `local_comparison_2026-11-13.csv`.

## Decision

Keep the compact implementation unchanged for physical testing. This local comparison found no repeatable regression
in the existing G1 paths, and no evidence of a uniform runtime penalty corresponding to the 5.6% growth in `.text`.
Additional functions increase executable size; code that is not executed does not add an instruction to every access.
Layout and dispatch can still affect performance, which is why timings were needed.

The scope is limited: one VM, four workers, 256-bit SIMD, a 32-bit switching profile, and the existing G1/sequential
paths. The original noisy set does not justify a clean performance claim, and one longer block cannot establish
sub-percent equivalence. The fixed arm-to-slot order is another limitation: subsequent comparisons should alternate
ABBA and BAAB, preserving each arm's first/last labels, rather than merely repeating ABBA.
Other SIMD widths, AMD CPUs, grouped-loop overhead and error-finding effectiveness remain
for the physical-machine campaign. G16 stays experimental.

In total, 16 measured runs and two setup pilots passed with zero test, seal or guest-reported WHEA errors. The 16
measured runs counted 20 TiB of reads/writes; that is repeated logical traffic, not 20 TiB of unique memory or proof
that every read reached DRAM. Both binaries were the already-built releases; no product source or compiler changed.

The separate [review follow-up](review_followup_2026-11-13.md) reconciles control-loop spills and records the later
G2/G4/G8/G16 functional runs. Those runs do not change this G1 comparison's scope or conclusions.
