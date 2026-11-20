# TODO 119 implementation review — 2026-11-20

Reviewed `feat/summary-grid-119` at `090bfbfdd4eb74f955d37895eb0d7f86a6dd8914` against main
`b0454fb82622c16059903cfd3d55b1d27535a4be`, the amended proposal and retained evidence.
The retained release is `51af41fe3727-1af69b810aa3282b`; its executable hash matches its manifest.
Only the build-order documentation changed between that source revision and the reviewed head.

**Recommendation: keep the design, fix the four integration/reporting issues below before release.**
The config issue should block merging as complete. These findings do not require another hardware
campaign to establish or fix. No production files were changed, and no live memory test was started.

## Findings

### 1. P1 — Config-loaded latency tests lose their measuring settings

Locations: `src/runner.rs:4684-4740`, `src/config.rs:1338-1342`,
`src/runner.rs:322-330`, `src/runner.rs:3009-3017`.

The built-in registrations attach `config.latency`: ordinary reads get `Settings::default()`,
the two `-Clean` tests and `Lat-DRAM-NTStore-SFence` get `Settings::CLEAN`.
The JSON path instead constructs a fresh `TestMemoryConfig`, whose `latency` is `None`, and
looks up only the test function. It inherits selected registration settings for the *loaded*
tests, but not these new tests.

Consequences:

- A JSON entry naming `Lat-L3-Read-Clean` or `Lat-DRAM-Read-Clean` runs on every worker, over the
  configured/default extent, rather than planning the named clean measurement.
- A configured `Lat-DRAM-NTStore-SFence` likewise loses its default single measurer and clean
  DRAM block.
- Adding `latency-threads=1` or `latency-cores=efficiency` on the command line does not repair
  this: `apply_latency_overrides` updates only an existing `Some(settings)`.
- The promised per-test JSON settings are not implemented either. `TestConfig` has no latency
  fields and denies unknown fields, so they are rejected rather than applied.

The existing `a_config_can_name_every_built_in_test` test checks successful lookup/counts, not
that registration semantics survive config loading.

**Fix:** attach the appropriate latency settings when resolving config tests, add the three
validated per-test fields, then apply CLI overrides. Define how the named clean tests inherit
their tier/whole-chunk defaults when a JSON entry supplies no explicit layout. Test config
loading plus overrides for both clean reads, the new write test and an ordinary read.

### 2. P2 — L1/L2 sizing still counts SMT siblings that now sit out

Locations: `src/measure.rs:263-277,292-295`, `src/tests.rs:154-169,218-224`,
`src/tests.rs` `TestMemoryConfig::as_planned`.

Under a count or `clean`, selection keeps one worker per physical core. For `Tier::Private`,
however, it returns `Measures { extent: None }`. The worker therefore retains the original
cache-relative config. L1/L2 sizing reads the run-global `ACTIVE_THREADS_PER_CORE`, including
siblings that are no longer measuring.

Concrete CPUID-only example: start with two selected SMT siblings and request one latency
measurer. On a 32 KiB L1 / 1 MiB L2, the default half-cache targets become 8 KiB / 256 KiB
instead of the 16 KiB / 512 KiB used when that same measurer runs alone. With calibration,
anchor B can likewise remain selected even though the sibling is idle.

This makes the working set depend on idle workers and can change the mix of cache levels in
the reported latency. It contradicts the proposal's per-step sharer sizing.

There is a related reporting assumption at `src/reporting/perf.rs:529-535`: any measuring plan
is treated as one thread per core. `latency-threads=all latency-cores=performance` can retain
both SMT siblings, so that assumption is not always true either.

**Fix:** resolve private-cache sizes using the actual measuring siblings and record their
sharing counts. Keep the change per step, without mutating the run-global sizing used by
other tests. Add a synthetic SMT case comparing one measurer with and without an idle sibling.

### 3. P2 — Result comparison ignores the new measuring plan

Locations: `src/run_context.rs:550-552,1208-1254`.

`TestConfigSnapshot.measure` is saved, but `setting_changes` never compares it. Run-level CPU
assignments/counts are unchanged when only latency measurers change.

I exercised the retained engine's `commands::compare_results` through a small external Rust
harness. Two synthetic copies of a retained result had identical measurements and run-wide
allocation/CPU settings; only clean DRAM's measuring record changed from one to two workers.
The comparison printed:

```text
Plan Changes: none, the same 26 steps in both
```

This is a metadata sensitivity test, not a claim that a two-measurer hardware run occurred.
The same omission matters when switching between performance and efficiency measurers.

**Fix:** compare the effective per-step measuring configuration, including participating CPUs
where recorded, count and resolved working set. Mark changed steps as unlike-for-like.
Old results without a measuring record need an explicit compatibility rule.

### 4. P2 — Saved clean layouts mix planned extents with unplanned chunks

Locations: `src/run_context.rs:749-762,776-785`.

Capture resolves the original config's layout, then replaces only `extent_bytes` with the
measuring plan's block size. `extent_mode`, `chunk_bytes`, chunk count and overlap still come
from the old layout.

All four retained result files demonstrate it, including the final short confirmation run:

| Test | Recorded extent | Recorded chunk | Chunks |
|---|---:|---:|---:|
| Lat-DRAM-Read-Clean | 7,516,192,768 B (7 GiB) | 80,883,712 B (77.14 MiB) | 1 |
| Lat-DRAM-NTStore-SFence | 7,516,192,768 B (7 GiB) | 80,883,712 B (77.14 MiB) | 1 |
| Lat-L3-Read-Clean | 10,108,928 B (9.64 MiB) | 6,230,016 B (5.94 MiB) | 1 |

These are whole-extent tests. The worker uses the planned extent; the error is in the saved
description, which downstream tools cannot interpret consistently. The console run-line and
plan-table correction did not fix this capture path.

**Fix:** capture the resolved layout of an actual measuring worker consistently. If workers
have different blocks, record their sizes or describe the range explicitly; do not combine
the smallest planned block with an unrelated unplanned spread.

## What checked out

- The retained full CI log reports 606 passed tests, six skipped, formatting, all three clippy
  checks and the release build passing. The audit has the already-allowed chrono maintenance
  warning; it is not a new finding in this branch.
- I reran 20 relevant existing tests from the retained test executable, one process per test:
  selection, clean sizing, summary assembly, cross-processor-group L3 grouping and the fake
  runner's sit-out/seal path. All passed. No project rebuild or full CI rerun was needed.
- Sit-outs skip the worker's latency function and pre-step seal work; zero-work results are
  excluded from the cycle minimum. The fake runner test checks the final sealed state.
- L3 uses OS cache sharing masks, including multiple processor groups. This is the appropriate
  API shape, rather than assuming a processor group is an L3:
  [Microsoft CACHE_RELATIONSHIP](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-cache_relationship).
- The new write loop's retained post-LTO assembly contains two `vmovntdq` YMM stores,
  `sfence` per line and `mfence` before closing `rdtscp`. No call replaces the timed store loop.
  Its one GPR reload appears in the spill report; the existing memory kernels' spill counts
  remain unchanged in the supplied before/after comparison.
- The three reported runs' summary figures agree with the stored JSON: write P50 182.3125 ns,
  clean L3 30.4453-30.4922 ns, clean DRAM 137.8125-137.9375 ns.

## Measurement limits and decisions

Keep the new write test's precise name and explanation. It measures fenced full-line NT
stores, including address generation and platform queueing. Fence ordering/visibility is
not a DRAM-cell completion timestamp:
[Intel SDM volume 1, section 10.4.6.4](https://cdrdv2-public.intel.com/843827/253665-sdm-vol-1-dec-24.pdf).

182.3 ns here versus 181.7 ns on a different host with the same CPU model is reassuring, but
does not prove equivalence to the probe. The port changed index-table traversal to generated
addresses with replacement, the extent and the sampling structure. Same-host comparisons
remain appropriate later; they are not needed to fix the issues above.

The ordinary DRAM read's undersized calibrated block predates this branch. Keep that sizing
question separate. Its result and the new clean result should not be presented as equivalent
measurement conditions. Retiring old read-chase-plus-store test names is also a separate
decision, not necessary for this implementation to work.

No new evidence here validates efficiency-core targeting on the desktops, corrected L3
grouping on the metal host, or clean multi-measurer performance. Those remain the recorded
hardware follow-ups, rather than claims supported by this four-worker VM run.

Private reproduction artifacts are outside the repository in `Downloads/scratch_119_review/`:
`check.py`, `compare.rs` and `comparison.txt`. The script reads the retained result files,
creates clearly synthetic copies and links the retained engine; it never starts a memory test.
