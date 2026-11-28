# Review of TODO 119's proposal at 8a7d8e3

Reviewed 2026-11-19 against the current sources and retained measurements. No builds, live runs, or product changes.

**Recommendation:** keep per-step measurer selection, the configured/constrained comparison, and the proposed NT-store
measurement. Amend the selection contract and measurement claims before implementing them. No separate allocation or cycle
is needed. The changes below are design corrections, not reasons to abandon the summary grid.

The arithmetic can be repeated with `python research/latency_proposal/review.py`. It reads the private `../baselines` archive
and prints selected measurements without hardware identifiers. It does not start TMR or contact a host.

## Corrections before implementation

1. **An estimated cache share must not be presented as measured hits.** `1 - L2 / block` is a useful approximation for the
   existing word-granular chase, supported by the sweeps below. It is not a hardware counter or a guarantee. If the one-thread
   fallback reaches only an estimated 82% against a target of 88%, explicitly show that the target was not met. Do not silently
   label that result a successful clean measurement.
2. **The loaded idle row is not interchangeable with the proposed clean DRAM step.** Its pointer layout and helper state differ.
   Using the same extent does not make the tests identical. Measure a separate step, or deliberately reuse the loaded idle row
   with its actual method and helper state in the label.
3. **Use the existing core classification, not an Intel/Windows-only restriction.** TMR already supports AMD core-type CPUID and
   the owner's explicit `core-priority=hybrid` interpretation on the 8500G. Keep the detector's provenance; do not infer types
   from preferred-core rankings by default.
4. **An idle worker needs an explicit role in scheduling and reporting.** Empty histograms merge safely, but that alone does not
   exclude idle workers from thread counts, cycle minima, per-thread latency rows, or seal bookkeeping. Setting an extent to zero
   with a nonempty allocation does not skip the test: the scaffold raises it to 4 KiB.
5. **Count real cache-sharing domains, including SMT and shared L2.** Processor-group-relative L3 IDs are not sufficient. Fix or
   qualify calibration provenance as well as the new selector. Do not assume a NUMA node or socket is always one L3.
6. **Name the write metric for the executed operation.** `NT store + SFENCE, ns/64-B line` is defensible. A DRAM-cell write
   completion time or an isolated write-command latency is not established by it. The Intel 181.7 ns result is also attributed
   to the wrong CPU in the proposal.

## 1. Blend model: useful, with limits

Confirmed in `src/latency_tests.rs:110-143,169-194`: the current read test traverses a random cycle through every u64 and records
the average cost of up to 1,000 dependent loads as each histogram sample. P50 is the median of those batch averages, not the
median individual load. It does not abruptly become an L3 latency when more than half the accesses miss L2.

Anchor A uses the same kind of word-granular cycle, though a different construction (`src/calibration.rs:627-658,684-707`).
The shared B/C probes use one node per 64-byte line (`src/calibration.rs:988-1009`); those are a different reuse pattern.
Consequently, neither a line-chase result nor a change to its layout can inherit the word-chase hit estimate unchanged.

For a reproducible check, I fitted only the L3 endpoint of
`T = (C/B)*T_L2 + (1-C/B)*T_L3`, with physical L2 capacity C, using the coarse one-thread sweep points from 2x through 8x L2.
The assumed L2 plateau and fitted endpoint are explicit:

| Source | L2 plateau | Fitted L3 endpoint | RMS residual |
|---|---:|---:|---:|
| petz007, Nov 5 curve | 2.7 ns | 10.08 ns | 0.095 ns |
| daddy-petz, Nov 5 curve | 2.5 ns | 10.16 ns | 0.111 ns |
| Xeon 8488C, Nov 13 perf calibration | 4.1 ns | 38.42 ns | 0.587 ns |

These are empirical fits to rounded console measurements, not independently measured pure-L3 latencies. They support the
mixture explanation. They do not establish exact hit percentages, replacement policy, or accuracy at the DRAM transition.
L1 service, cache replacement, prefetch, translations, and clocks can also contribute.

Under this two-level approximation, relative shortfall from L3 is `(1-h)*(1-T_L2/T_L3)`. At h=88% it is about 8.8% on petz007
and 10.7% on the Xeon. Thus 88% is a reasonable heuristic, not a generally established 10%-accuracy threshold. A 90% target
would bound this particular model's shortfall to 10%, but would cost measuring-core breadth and would still not certify real
hardware hit rates. There is no evidence that specifically makes 88% the optimal default.

The argument about sequential bandwidth is plausible as a capacity argument, but saying every access necessarily comes from
L3 goes too far: prefetch and real replacement policies can put lines in closer caches. The measured bandwidth ratios are not
cache-hit counters either.

## 2. Clean counts and the meaning of 88%

Using the Nov 13 saved A and the observed half-reach extents to reconstruct the AMD reach curve approximately:

| Host / calibration | At 50% | At 60% | At 88%, using physical L2 |
|---|---:|---:|---|
| petz007 | 3 | 2 | 1; target unmet, estimated 81.6% at 5.44 MiB |
| daddy-petz, first perf run | 8 | 8 | 3; estimated 88.3% at 4.28 MiB |
| daddy-petz, last perf run | 8 | 8 | 2; three would estimate only 87.8% |
| Xeon 8488C, per physical socket L3 | 12 | 10 | 3; estimated 88.5% at 17.32 MiB |

Counts are bounded by the cores selected in those runs. AMD reconstruction inherits the saved extent's granule rounding.
The Xeon console says C found no edge, so its current implementation falls back to A/k; the three-core result is a prediction.

Using calibrated usable L2 instead gives three at 88% on the last daddy-petz run: estimated 88.5%, because its calibrated L2
is about 483 KiB rather than the physical 512 KiB. It gives thirteen instead of twelve on the Xeon at 50%.
The proposal must specify which capacity it uses and how SMT/shared-L2 activity changes it. Those are consequential choices.

The 5.44 MiB petz007 block is consistent with roughly 8.7 ns in its retained sweep; it has not been measured as the new clean
step. Keep the 0.5 safety factor initially, reporting an unmet target when necessary. Raising it to 0.65-0.75 should be validated
against the actual new workload, rather than justified from a single sweep alone.

There is already some multi-thread reach evidence: the Nov 5 curve reports include k sweeps, using the line-granular shared
probe. For example, petz007's reach model overestimated the measured reach by 16% at k=1 and 9% at k=2; daddy-petz's was
conservative at k=2 and k=3. This supports retaining a margin. It is not an end-to-end measurement of the proposed clean test.

## 3. Xeon cache domains

The Nov 13 8488C console has four L3 labels across two NUMA nodes. On each node, one label covers 64 logical CPUs and another
covers 32; the perf run chooses 32 and 16 physical cores respectively. Calibration C actually runs 96 logical CPUs on node 0
while reporting a maximum of 64 sharers. This confirms the grouping problem described in the proposal.

`CpuTopologyInfo::l3` is explicitly `(processor group, LastLevelCacheIndex)` (`src/cpu_topology.rs:18-33`).
Use OS cache relationship membership where available, including masks spanning processor groups, to construct a physical cache
domain. Windows exposes this through `GetLogicalProcessorInformationEx(RelationCache)` and
[`CACHE_RELATIONSHIP.GroupCount/GroupMasks`](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-cache_relationship).
Do not merge IDs solely because their NUMA node or socket matches: chiplets and SNC need different treatment.

The planner also needs actual L1/L2 sharing sets. L1 is shared by SMT siblings; L2 can be shared by an E-core cluster.
Using one global maximum sharer count cannot describe uneven selections or dissimilar cache capacities.
A's calibration is from one core/node, not a measured curve for every core type and cache domain.

Changing these identities affects the interpretation of saved C and its sharer count. Version/recheck that interpretation;
do not merely update the new test's k while treating an old C measurement as though it used the corrected topology.

## 4. What one measuring core loses

The saved per-thread data confirms the main point about breadth, with slightly wider ranges than the proposal states:

| Host, first Nov 13 perf run | L3 thread P50 range | DRAM thread P50 range |
|---|---:|---:|
| petz007 | 4.580-4.619 ns | 62.391-64.594 ns |
| daddy-petz | 8.277-8.332 ns | 63.859-65.281 ns |
| 8500G | 5.311-6.209 ns | 65.969-67.719 ns |
| Xeon 8488C | 5.467-5.557 ns | 133.063-136.188 ns |

The 8500G distinction is real. The merged histogram is sample-weighted, not an equal-weight median over cores: faster cores
can contribute more 1,000-load batches within the same timed duration.
Thread 0 is second-worst for petz007's L3 P99 and sixth-worst of 96 for the Xeon's L3 P99. This supports keeping per-core
evidence; it does not identify OS work as the cause.

Correct the claim that an efficiency-core selector only works on Intel. `src/cpu_topology.rs:411-425` already cascades from
Windows efficiency classes to AMD CPUID, then explicitly opted-in priority-based hybrid typing.
For the 8500G, `core-priority=hybrid` can supply the classification to the new per-step selector. There is no need to invent a
second detector or force every test onto those cores. Reject an efficiency selection with no eligible typed core, with a clear
explanation; do not silently measure on a performance core.

## 5. Sitting out: safe aggregation is not measurement invariance

The histogram part checks out: `merged_latencies` adds counts, so an empty histogram does not change the active histogram's
percentiles (`src/runner.rs:4810-4815`). A returning worker blocks in `receiver.recv()` (`src/thread_pool.rs:467-470`).

But an empty extent is not a complete implementation:

- `test_extent` changes zero to 4 KiB if memory exists (`src/test_scaffolding.rs:198-204`). To reach the existing empty path,
  one must pass an empty view or explicitly dispatch a sit-out role, retaining the worker's actual allocation.
- The runner still includes the worker in per-thread rows and cycle minima (`src/runner.rs:1573-1614,4827-4847`).
  Mark it as idle, not a zero-nanosecond measurement or an unfinished measuring thread.
- The seal-before-step machinery resolves extents separately (`src/thread_pool.rs:368-399`). It must know the same plan:
  otherwise a nominally idle worker can touch memory and mark an untouched extent unsealed.
- Compute per-step extents and cache sharers from the chosen measurers. Avoid changing run-wide globals to represent a
  heterogeneous step (`src/tests.rs:482-502`). Retain the existing run allocation, pinning, and later test selection.

There is no code-only proof that measured nanoseconds stay unchanged when other cores sleep. Cache occupancy, CPU/package
power, boost, and uncore state can change. That is part of the new measurement's definition.

In particular, the loaded idle row is a different experiment:

| Property | Ordinary Lat-DRAM-Read | Loaded idle row |
|---|---|---|
| Pointer nodes | Every 8-byte word | One per 128 bytes |
| Extent rule | Four calibrated L3 reaches | max(1 GiB, 32 CPUID L3 sizes), allocation-capped |
| Other participating cores during idle | Other read workers in the ordinary test | Loader cores spin with PAUSE in C0 |

Sources: `src/latency_tests.rs:167-182`; `src/loaded.rs:15-25,62,91-94,854-858`.
A sleeping-worker clean read can reuse code after its semantics are specified; it cannot simply reuse that idle number.
Also record the actual capped extent: the rule cannot guarantee a cache-exceeding block when the selected worker owns too little
memory.

## 6. Write method and evidence

The current `write_latency_multi` follows a read dependency chain while issuing stores to a parallel region
(`src/latency_tests.rs:267-310`). Renaming/reclassifying it is justified. Replacing its perf-plan entry with the new method is
reasonable; do not silently reuse its historical identity or reinterpret old saved results as fenced-store measurements.

The probe's W-nt+sf writes a whole line using two 32-byte NT stores, SFENCE after each line, and one final MFENCE after the batch
(`../write-latency-test/src/main.rs:142-155`). Its timer averages 400,000 iterations; each iteration also reads an address index.
It avoids a dependent read-back of the payload and the temporal store's usual ownership acquisition. It is not literally
"only a write": address generation, fencing, loop work and platform queuing are included.

Intel specifies ordering/global visibility for SFENCE, not arrival at a DRAM cell or a named controller queue.
[Intel's optimization manual](https://www.intel.com/content/dam/doc/manual/64-ia-32-architectures-optimization-manual.pdf)
describes the store ordering; the [SFENCE reference](https://www.felixcloutier.com/x86/sfence) states the same rule.
For a TSC-based port, keep an explicit end-of-batch completion boundary: RDTSCP by itself does not wait for prior stores to
become globally visible ([RDTSCP reference](https://www.felixcloutier.com/x86/rdtscp)).
Paying one batch-ending fence is different from inserting MFENCE after every line.

Averages of the two **v3** run medians per setting, ns:

| Host / setting | R | NT + SFENCE | NT + MFENCE | Store + flush |
|---|---:|---:|---:|---:|
| daddy-petz, 2133 | 102.80 | 125.30 | 141.80 | 197.40 |
| daddy-petz, 3600 | 65.25 | 91.75 | 107.95 | 133.95 |
| petz007, initial 6000 | 66.15 | 62.20 | 77.30 | 91.95 |
| m8i.metal, Xeon 6975P-C | 131.85 | 181.70 | 195.55 | 188.20 |

Files are under `../baselines/2026-11-05-write-latency/{slow-2133,oc-3600,petz007-6000,metal-m8i-7200}/`,
named `probe-v3_*.txt` or `probe_v3_*.txt`.
The proposal's 181.7 ns figure is these m8i measurements, not the 8488C used on Nov 13.
Its earlier-version and v3 results should be distinguished if summarizing exact scatter or small differences.

The v3 timing-sweep ranges also support the limited conclusion: at 3600, the setting means for daddy-petz range from
90.25-91.80 ns for W-nt+sf; petz007's range from 61.80-62.20 ns across the retained settings.
They do not establish sensitivity to an individual timing beyond variation between boots.
The write being faster than a read on Zen 4 does not locate its acknowledgment point either.

Porting this as an informational fenced-store metric needs no additional BIOS experiments first. Preserve its full-line
stores and timing boundaries, validate its emitted code, and check a port against the probe. FCLK/tCWL/tCL experiments answer
whether it is useful for tuning, not whether software can time the operation.

## Suggested implementation contract and order

1. Define one immutable per-step plan: eligible workers, roles, actual CPUs/cache domains/nodes, extents, estimated share,
   whether the target was met, and any allocation limit. Build it before the worker's hot loop.
2. Make `latency-cores` an eligibility filter using existing classification, then apply ranked selection. Define N as a total
   count and state how it is distributed across domains. Reject zero, impossible exact counts, and an empty filtered set.
   Specify parameter precedence when a named Clean test meets an explicit run override.
3. Ordinary read tests may have several measurers. Loaded/turnaround tests still have one: allow choosing that measurer, then
   rebuild its same-node loader set, exclude its SMT siblings, and size the participant barrier accordingly.
   Do not accidentally apply the ordinary test's default `all` as "all workers measure" to a loaded test.
4. Keep configured results and add genuinely measured constrained results. Identify the chase layout and helper state;
   display estimated shares and unmet targets honestly. Do not erase old methods from saved-result interpretation.
5. Add the fenced-store metric under a distinct name; retire or rename old plan entries separately from deleting kernels.
6. Verify selection, empty/idle accounting, capped extents, topology fixtures, and old-result loading locally. Then a small
   hardware check covers k>1 and the implemented idle state. Intel-hybrid placement and timing-tuning claims remain separate
   hardware validation; neither needs to hold up the pure summary formatter.
