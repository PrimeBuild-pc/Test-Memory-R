# Access geometry: final hardware follow-up, 2026-11-16

All 20 queued runs completed and their results were collected: six on the Ryzen 5700X/DDR4
and fourteen on the Ryzen 8600G/DDR5, covering 2,469 completed steps. The 5700X reported
29 test errors during its long screens; every other run passed. Neither host reported
seal errors or WHEA events. No specific implementation defect was identified.

TODO 126 can close. Grouping changes throughput away from period 512 too, the existing
512-bit strided paths show no repeatable regression, and both ordinary and grouped walks
detected mismatches. This does not establish a best geometry for finding errors.

## Method and validation

The retained baseline `57369c4` and candidate `72ab3b8` were used without rebuilding.
Their hashes match the preceding campaign. Both hosts used 8 GiB, four pinned physical-core
workers, 256 MiB chunks, only 2 MiB pages, automatic transfer-width detection, rescan on and
continuing error reporting. The 5700X used 256-bit SIMD and the 8600G used 512-bit SIMD.
No reboot, BIOS change or additional physical-address mapping was performed.

Each queue began with a ten-minute sequential native-pattern warm-up. Then two geometry
runs exercised P496/G1/2/4/8/16, P508/G1, P512/G1, P512/G16 and P513/G1 in both SimpleTest
and Sequence. Each run contained forward and reverse orders, giving four observations
per test/geometry pair across the two runs. The 8600G also ran eight existing-G1 comparisons
in ABBA/BAAB order, including unchanged sequential and bandwidth controls.

Finally, each host ran three mixed-geometry screens requested for 45 minutes each.
TMR completed its current suite cycle, so these lasted about 49–50 minutes. Each screen
repeated all 36 steps, with starting positions rotated between runs and both directions
within each cycle. Seeds were recorded. Each process retained its allocation; separate
processes could receive different physical memory.

The archive's `analyse.py` checked all 20 outputs against their configs and queues:
binary identity, allocation/page mix, worker count, width, seed, duration, geometry,
patterns, complete step order, per-worker/per-step byte counts and total errors.
All checks passed. The 512-bit A/B additionally had identical recorded setup and work,
including CPU assignments. Rates below use test time excluding setup/sealing.

Raw consoles, JSON, logs, exact configs, queues, hashes, preflight records and the analysis
script are retained outside Git in `../baselines/2026-11-16-access-geometry/` from the app root.
Reproduce with:

```text
python -B ../baselines/2026-11-16-access-geometry/analyse.py
```

It writes `analysis.json`, `steps.csv` and `error-episodes.csv` into that archive.

## Grouping at period 496

Mean MiB/s across four observations, using only the clean, short geometry comparisons:

| Group | 5700X SimpleTest | 5700X Sequence | 8600G SimpleTest | 8600G Sequence |
|---|---:|---:|---:|---:|
| 1 | 26,481 | 30,934 | 22,280 | 25,959 |
| 2 | 28,189 | 32,839 | 25,852 | 30,213 |
| 4 | 20,485 | 23,613 | 26,687 | 31,035 |
| 8 | 21,120 | 24,017 | 30,716 | 35,818 |
| 16 | 19,626 | 22,105 | 29,928 | 34,463 |

Grouping still has a substantial effect without P512. On the 5700X, G2 was about 6% faster
than G1, while G4–G16 were about 20–29% slower. On the 8600G, every grouped setting was
faster than G1 here; G8 led by about 38%. The fastest choice is therefore platform-dependent
even within this small comparison. These are complete-workload rates, not isolated kernel
overhead or proof of greater electrical stress.

Prefetching changes with grouping: all these chunks permit the full target of 128 visited
lines ahead, which becomes 128/G visits. At P496, the in-pass virtual-address distance is
approximately 3.875 MiB for G1 and 0.242 MiB for G16. Grouping, locality, cache effects and
prefetch distance are not isolated from one another by this campaign.

## Existing G1 paths at 512 bits

Candidate versus baseline, each block averaging two runs per binary:

| Test | ABBA change | BAAB change | Mean change |
|---|---:|---:|---:|
| Strided SimpleTest | −0.19% | +0.09% | −0.05% |
| Strided Sequence | −0.71% | +0.38% | −0.17% |
| Sequential SimpleTest control | −0.14% | −0.64% | −0.39% |
| Sequential Sequence control | −0.18% | −0.37% | −0.27% |
| Bandwidth reference | −0.04% | +0.02% | −0.01% |

All eight runs passed. The two strided comparisons change sign between blocks, and
same-binary changes reached 2.35%. Reference normalization leaves their conclusion
unchanged. No repeatable strided regression was established; this does not prove exact
equivalence or exclude a small change. The small negative sequential-control differences
are retained rather than rounded into a claim of zero cost.

## Error evidence

| Host | Screen 1 | Screen 2 | Screen 3 | Warm-up and short comparisons |
|---|---:|---:|---:|---:|
| 5700X/DDR4 | 7 errors, 49m19s | 11 errors, 49m20s | 11 errors, 49m47s | 0 errors |
| 8600G/DDR5 | 0 errors, 49m13s | 0 errors, 49m13s | 0 errors, 49m16s | 0 errors |

The 5700X's 29 reported errors are not 29 independent corruption events:

- The logs identify **13 distinct bad-value episodes**, each observed by two verify passes:
  26 word detections. An episode here groups the same run, step, suite cycle, thread,
  address, expected value and actual value. It is an observation grouping, not proof
  of independent hardware faults.
- All 26 word detections survived four immediate rereads and the subsequent flush/recheck.
  A flush may write a bad cached value back, so this does not localize the origin to DRAM.
- Three additional Sequence RW alarms followed already observed bad values. The post-write
  rescan was clean. Calling these three independent transient faults would misrepresent
  the read/write sequence.
- Twelve episodes involved bit 23; one involved bit 28. All were in worker 3's span
  (logical CPU 14), word 5 of a cache line. Each run had two affected virtual addresses,
  with repeated failures at one of them. Virtual addresses do not identify physical
  cells across separate runs.

Failures appeared in SimpleTest and Sequence, at P496, P508, P512 and P513, using G1,
G2, G4 and G16. They included ident and random backgrounds. G1 failures demonstrate that
the observations are not confined to the new grouped traversal. No failures were observed
with G8 in this campaign; that alone does not establish weaker detection.

Every geometry/test pair completed 48 measured screen steps on the 5700X and 66 on the
8600G. Per pair, that is 9/12.375 TiB of logical SimpleTest work and 10.5/14.4375 TiB of
Sequence work, respectively. Elapsed exposure differs with throughput and is preserved
in `analysis.json`. The errors cluster by address and time, patterns have different keys
between steps, and no independent fault source was controlled. Ranking geometries from
these small, correlated counts would not be justified.

The warmed runs found persistent mismatches on the previously mostly clean 5700X.
They do not establish that heat caused them: no memory-temperature time series was
recorded. The 8600G's clean session likewise does not prove its earlier fault is resolved.

## Recommendation

Keep all five supported groups and the existing defaults. P496 is a useful common period
for comparing groups; P512 and P513 remain complementary spacing controls. Select a mix
when testing stability rather than treating the fastest setting as the strongest detector.

Close TODO 126 with this evidence and its limits. Further investigation of the 5700X's
localized failures is separate from shipping the geometry feature; no new hardware
campaign is required to close this implementation item.
