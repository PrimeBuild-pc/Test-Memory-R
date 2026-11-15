# Access geometry, TODO 126

`models.py` checks the proposed walk and illustrates several address selectors. It does not access hardware mappings.

`probe.py` compiles `group_probe.rs` to assembly without running it. This compares constant-loop bounds, explicit macro
expansion and a runtime group made of two-line tiles, using the same AVX2 identity verifier and four accumulators.

Run from the repository root:

```text
python -B research/access_geometry/models.py
python -B research/access_geometry/probe.py
```

The probe's assembly and machine-readable counts go in `target/access-geometry/`. These are isolated compiler experiments;
the product still needs post-LTO inspection and hardware A/B measurements.

## First comparison, 2026-11-13

Rust 1.102.0-nightly (9c5648833, 2026-11-10), LLVM 23.1.5, Windows MSVC, x86-64-v3:

| Form | Whole-function instructions | Innermost memory-loop instructions |
|---|---:|---:|
| Constant group 2 | 188 | 40 / 40 |
| Constant group 4 | 260 | 76 / 76 |
| Constant group 8 | 412 | 148 / 148 |
| Constant group 16 | 212 | 42 / 43, with indexed addressing |
| Explicitly expanded group 16 | 701 | 292 / 291 |
| Runtime group, two-line tiles | 243 | 39 / 40 |

The constant group-16 bound did not fully unroll. Its inner loop processes two lines, so its counts cannot be compared with
expanded-16's as equal amounts of work. Likewise, runtime-group's figures exclude its enclosing group-control loop. None of
these counts is a throughput measurement. The innermost memory loops have no stack accesses in this probe.

The first product candidate (`d71d1d1`) used explicit offsets and selection outside the memory loop. Its post-LTO audit
rejected this as the final implementation: code grew substantially and many expanded loops spilled.

| Expanded group | Memory loops | Loops with stack accesses | Longest loop, instructions |
|---|---:|---:|---:|
| 2 | 456 | 162 | 214 |
| 4 | 456 | 210 | 429 |
| 8 | 454 | 267 | 853 |
| 16 | 450 | 289 | 1,682 |

No grouped loop had calls or a wrong-width memory access. The worst loop had 178 stack-access instructions per iteration;
these were not merely a larger counter or cold-path spill. The executable's `.text` grew from 6,744,350 to 10,243,502 bytes
(51.9%). These product results supersede what the small ident probe alone suggested.

The revised candidate shares a bounded inner body across groups 2/4/8/16: one line at 128 bits (already four SIMD vectors),
two lines at 256/512. A visit repeats that body before the period jump. Selection remains outside the memory loop, four
accumulators remain independent, and the requested visit order and prefetch targets are unchanged. This adds bounded loop
control in exchange for smaller code and less register pressure. Hardware comparisons,
rather than instruction counts, must establish the throughput cost, including outer-loop control and invariant reloads.

The intermediate all-two-line version (`5cb54c9`, retained as `two-line-*`) reduced `.text` to 7,139,918 bytes (+5.9% over
baseline), and its longest memory loop to 220 instructions. One 128-bit loop still had a SIMD spill/reload, prompting the
one-line inner body at that width. This does not change G's meaning or introduce separate kernels per group.

The baseline for the product comparison is commit `57369c4`. Its rebuilt EXE, post-LTO assembly, fingerprints and spill details
are kept locally under `target/access-geometry/baseline*`. This uses the repository's selected MSVC 14.52.37025 and Windows SDK
10.0.28000.2957. No live TMR run was performed to obtain it.

The first product candidate's artifacts and audit are retained as `target/access-geometry/expanded-*`; the final candidate
uses `candidate.*`. `audit.py FILE.s` examines grouped backward-branch ranges with no length cutoff. It now reports
memory-body and control-only ranges separately; the initial audit filtered out the latter. `compare_loops.py BEFORE.s AFTER.s`
compares existing SimpleTest, Sequence and bandwidth memory-loop instructions/constants independently of chunk setup.
Neither tool's memory-body counts establish that the enclosing visit/pass loops are free of stack traffic.

## Final development validation, 2026-11-13

Code commit `72ab3b8`, same compiler/SDK as the baseline:

- Full `python tools/ci_local.py` passed: formatting, build-tool checks, all three Clippy runs, release build, 581 tests
  (six existing opt-in tests skipped), and `cargo audit`.
- All 456 grouped memory loops use the intended SIMD width and contain no calls. None has a SIMD spill or a spill/reload
  round trip. 232 have no stack accesses; 224 reload loop-invariant GPR values. The longest is 193 instructions. Those
  counts exclude control-only ranges. The standard `hot_loop_spills.py` total is 229 GPR round-trip ranges, against
  baseline's 2: all 227 added flags belong to grouped traversal control. The corrected audit reports 551 grouped
  control ranges: 227 round-trip, 182 reload-only, 142 clean. This is real stack traffic, not a spill-free kernel.
  These are static ranges across specialisations, not spills per memory access. Their frequency and an example are in
  [the review follow-up](review_followup_2026-11-13.md); subsequent complete-path timings are in
  [the physical comparison](hardware_comparison_2026-11-15.md).
- `.text`: 7,124,126 bytes, 5.6% above baseline; full expansion had added 51.9%. The grouped implementation adds 228
  specialized functions across patterns/operations/widths, shared across G=2/4/8/16, rather than 912 functions for four
  separate group kernels.
- Existing loop comparison: 78 name groups match instruction/constant fingerprints; 24 strided name groups differ.
  Sequential/reverse/random and bandwidth loops match. Strided code is not claimed byte-identical, which is why the
  baseline-compatible G1 A/B remains part of the kit.
- Whole-function fingerprints have a wider scope: 230 name groups changed and 24 were added, including changes in
  modules whose source was untouched. The 78/24 figures describe selected memory loops, not the entire executable.
- The normal CI release executable's entire `.text` section matches the assembly-audited executable byte for byte.
  `candidate.exe`/`candidate.pdb` are the normal release copies; `candidate.build.json` retains the original build manifest
  (whose artifact keys are `tmr.exe`/`tmr.pdb`). `validation.json` records both executable hashes and the common code hash.
  The standard archive is `../builds/72ab3b8b87e2-165d7eeea894ded3/`.

The original validation above did not run TMR live. Later G1 measurements, grouped functional checks and the physical
comparison are recorded below. On 2026-11-15 the owner approved merging the implementation as an optional setting,
with further hardware comparisons deferred to [TODO 126](../../doc/todo/126-access-geometry.md#merged-implementation-deferred-hardware-follow-up).
All five group sizes are supported; default plans and TM5 imports remain G1.

## Hardware comparison kit

The files in `configs/` are ordinary TMR JSON configs:

- `baseline-compatible.json`: the existing group-1 period-512 workload, using `parameter=256` and the default `channels=2`.
  Both the baseline and candidate understand it. Includes an unchanged `Spd-DRAMFull-Read-256` reference.
- `local-regression.json`: the same two strided steps plus sequential controls, followed by 16 sweeps of the unchanged
  bandwidth reference. This makes the reference long enough to compare locally without relying on a single short sweep.
- `local-regression-long.json`: the follow-up with four cycles of each sequential control and 64 reference sweeps,
  introduced after the local VM's short control steps showed substantial variation.
- `combined.json`: P/G 512/1, 513/1, 512/2, 512/4, 512/8 and 512/16, in SimpleTest and Mem-Sequence.
  Every geometry within one test type has the same extent, chunk, pattern program and number of reads/writes.
  All four program entries run as verified polarity pairs.
- `combined-reverse.json`: the same entries in reverse order, for an order/drift check. Step keys change with position, so
  this changes the data too; it is not a same-image replay. Fixed `seed=` reproduces one config on the same thread/span layout.

Start with an amount of memory and workers appropriate to the host. The examples below request 8 GiB and four physical
cores, one suite cycle, seed 126, at 256 bits. `-target` requests that allocation; plain `memory=8GiB` reserves 8 GiB for
the system and tests the remainder. `skip-cores=0` allows all four cores on a four-core host. These commands describe the kit;
completed measurements are recorded separately below.
Runtime depends on the host and allocation; start with one cycle before scheduling longer comparisons.

```powershell
python tools/bench_ab.py target/access-geometry/ab-g1 "Spd-DRAMFull-Read-256 (reference)" "config=research/access_geometry/configs/baseline-compatible.json memory=8GiB-target cputype=cores cpus=4 skip-cores=0 cycles=1 seed=126 width=256" baseline=target/access-geometry/baseline.exe candidate=target/access-geometry/candidate.exe --runs 4

.\target\access-geometry\candidate.exe config=research/access_geometry/configs/combined.json memory=8GiB-target cputype=cores cpus=4 skip-cores=0 cycles=1 seed=126 width=256
.\target\access-geometry\candidate.exe config=research/access_geometry/configs/combined-reverse.json memory=8GiB-target cputype=cores cpus=4 skip-cores=0 cycles=1 seed=126 width=256
```

Check the printed allocation, page sizes and geometry before interpreting speeds. Keep the machine otherwise idle and do not
build during the run. A/A runs with the same binary under two labels establish noise. Repeat at the host's normal thread count
and 512 bits where available; SimpleTest also has 128 bits, while Mem-Sequence has 256/512.

The baseline-compatible A/B tests for regressions in existing code. The combined runs compare different workloads: lower
bandwidth can reflect more conflicts, so speed alone does not rank their detection ability. Compare errors and their first
occurrence, repeated bad addresses, elapsed exposure, verified bytes and whether rescans reproduce them. Cache-sized and
DRAM-sized chunks are separate experiments; repeated cached reads are not additional DRAM coverage.

Do not infer the best geometry from one quiet or noisy run. A stable host checks false alarms and cost; a marginal host is
needed to establish detection benefit. No BIOS mapping, channel-count or interleaving claim is assumed by these examples.

`summarize_ab.py OUTDIR` reads `bench_ab.py` output with labels `base_first`, `new_first`, `new_last`, `base_last`.
It checks successful completion, errors, allocation/page mix, worker placement, test settings and processed bytes before
comparing complete ABBA blocks. It reports raw and reference-normalized changes, plus same-build first/last differences.
Those same-build differences help identify order effects and run-to-run variation; they are not a confidence interval.
For subsequent comparisons, alternate ABBA and BAAB blocks. `bench_ab.py` uses the labels' command-line order:
use `base_first, new_first, new_last, base_last` for ABBA and `new_first, base_first, base_last, new_last` for BAAB,
with their respective executable paths as above. Use a separate output directory for each invocation to preserve every
block. Reversing the four ABBA labels alone still gives ABBA and does not exchange the arms' inner/outer slots.

## Local before/after measurements, 2026-11-13

At the owner's request, both retained releases were run here before the physical-machine campaign: 8 GiB target, four
workers, 256-bit SIMD, 2 MiB pages, fixed seed and transfer width. Six runs per build were followed by a longer ABBA block.
All 16 measured runs and two pilots passed. The first set had a disrupted middle block and substantial VM variation;
the longer follow-up measured +0.87% for existing strided SimpleTest and +0.12% for strided Sequence, with no sequential
control slower on average. This supports keeping the implementation for physical testing, not sub-percent equivalence
or a detection claim.

The full results, including the noisy block and background activity, are in
[`local_comparison_2026-11-13.md`](local_comparison_2026-11-13.md); per-run rates are in the adjacent CSV.
The scripts validate matching settings/work and preserve the raw local logs. Grouped costs and the other CPUs/widths
remain unmeasured by this comparison.

## Review follow-up

Both `combined.json` and `combined-reverse.json` subsequently completed through the retained `candidate.exe` locally:
four workers, 4 GiB actually allocated, 256-bit SIMD, 2 MiB pages, all 12 steps in each, and zero test/seal/WHEA errors.
The saved plans confirm every requested P/G pair, and the results have the expected work and completed cycles.
This closes the missing end-to-end grouped sanity check at 256 bits, not the physical detection or performance gate.
See [the full follow-up](review_followup_2026-11-13.md) for commands, artifact locations and the spill reconciliation.

## Physical comparison, 2026-11-15

The retained baseline and candidate were run on Ryzen 5700X/DDR4 and 8600G/DDR5, with fixed 8 GiB allocations,
four workers and 2 MiB pages. Regression blocks alternate ABBA/BAAB. Grouped tests cover both visit orders at
256 bits, with additional 512-bit screening on the 8600G. P=G controls compare the complete grouped paths while
holding forward line order constant; they include control/prefetch costs but do not isolate individual spills.

See [the hardware report](hardware_comparison_2026-11-15.md) for results, errors and limits. Raw results and
deployment manifests are retained outside Git in `../baselines/2026-11-15-access-geometry/`. The three
`summarize_hardware.py`, `summarize_same_order.py` and `summarize_screen.py` tools reproduce the comparisons and
check completed work. Hardware errors and incomplete runs are retained, and excluded from clean speed comparisons.

The follow-up will compare all groups at P=496, include the 508/1 and 512/513 controls, and complete the matched
512-bit existing-G1 A/B. The 512-versus-513 result is a period comparison, not proof of grouping's value. These
experiments are deferred work, not a claim that the current measurements demonstrate better error detection.
