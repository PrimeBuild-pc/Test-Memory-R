# Access geometry: physical-machine comparison, 2026-11-15

Completed: 37 runs across both machines. All 20 runs on the 5700X passed. Of 17 on the 8600G,
12 passed and five halted with errors; one failing run used the baseline. Both machines are idle
and all artifacts have been fetched. No large existing-G1 regression was established. The controlled
grouped-path comparisons show small differences, including about 1% slower cases at 512 bits.

## Builds and method

The retained release binaries were deployed without rebuilding: baseline `57369c4`, candidate `72ab3b8`.
Their SHA-256 hashes match the earlier development validation. Later documentation/tool edits and concurrent
working-tree changes are not in these binaries.

Both machines used an 8 GiB allocation, four pinned physical-core workers, 256 MiB chunks, fixed 2 MiB pages,
seed 126, no calibration, and automatic transfer-width detection. The Ryzen 5700X had DDR4-3600 and the
8600G DDR5-6000, both running Windows build 26300 with VBS. BIOS and power settings were left unchanged.
The 5700X used High performance; the 8600G Balanced. These are within-machine comparisons.

Each original regression comparison ran ABBA then BAAB: four runs per binary, swapping which binary
occupies the inner and outer slots. The config includes strided SimpleTest/Sequence, longer sequential
controls, and an unchanged bandwidth reference. Matching settings, CPU placements, completed cycles,
allocation/page mix and processed bytes were checked from the saved results.

Failed or incomplete runs are retained, but their blocks are excluded from the clean speed comparison.
Separate processes can receive different physical memory. No physical-address mapping was performed.
Byte counts are logical read/write work, not measured DRAM traffic.

Raw consoles, JSON, logs, deployment hashes, exact queues and additional configs are retained outside Git
in `../baselines/2026-11-15-access-geometry/` from the app root. Host details stay in that private archive.
`summarize_hardware.py` reproduces the main comparison; `summarize_same_order.py` checks the same-order
controls; `summarize_screen.py` counts exposure and errors in the longer screen.

## Existing G1 paths

Change is candidate versus baseline, averaging each complete block's percentage change:

| Test | 5700X, two clean blocks | 8600G, one clean BAAB block |
|---|---:|---:|
| Strided SimpleTest | +2.38% | −1.06% |
| Strided Sequence | +3.72% | +0.10% |
| Sequential SimpleTest | +0.92% | +0.59% |
| Sequential Sequence | +0.10% | −0.79% |
| Bandwidth reference | −0.96% | −0.14% |

All eight 5700X runs passed. On the 8600G, seven passed and one candidate run halted with two test
errors and one seal error, invalidating its ABBA block for this comparison.

The 5700X's strided candidate deltas were positive in both orders: SimpleTest +3.33%/+1.43%, Sequence
+4.21%/+3.23%. However, same-binary strided results changed by as much as 15.09% within a block.
The reference did not follow that drift, so dividing by it does not remove all variation.
The 8600G's clean block had same-binary strided changes up to 1.96%.

These measurements show no large G1 slowdown. They do not establish sub-percent equivalence or a
precise speedup. The small 5700X reference decrease prompted an additional reference-only comparison.

That reference-only ABBA/BAAB comparison completed eight clean runs, each timing 512 GiB of reads.
The candidate changes were +1.05% and −2.10%; same-binary changes reached +3.91% for baseline and
−2.26% for candidate. The direction reversed between blocks. This does not establish a repeatable
reference regression or resolve a difference below that variation. The benchmark-only plan has no
seal; unlike the original mixed plan, it contains no correctness steps.

## Grouped walks

The original configs compare P/G = 512/1, 513/1, 512/2, 512/4, 512/8 and 512/16, in both SimpleTest
and Sequence. Each SimpleTest step processes 192 GiB; each Sequence step 224 GiB.

Both orders passed on the 5700X at 256 bits. The 8600G's forward 256-bit run also passed all 12 steps.
Its reverse 256-bit and initial 512-bit runs halted with errors, detailed below; they are not full
coverage passes or clean performance samples.

Clean rates in MiB/s:

| P/G | 5700X SimpleTest, mean of both orders | 5700X Sequence, mean of both orders | 8600G SimpleTest, forward only | 8600G Sequence, forward only |
|---|---:|---:|---:|---:|
| 512/1 | 9,199 | 10,830 | 18,597 | 22,808 |
| 513/1 | 29,158 | 34,023 | 49,531 | 58,083 |
| 512/2 | 18,981 | 21,999 | 26,722 | 30,845 |
| 512/4 | 17,581 | 19,804 | 17,163 | 18,761 |
| 512/8 | 16,885 | 18,955 | 17,677 | 19,406 |
| 512/16 | 19,320 | 21,521 | 22,853 | 24,328 |

The rates are not monotonic in group size. These are different memory workloads; neither higher
bandwidth nor lower bandwidth establishes better error detection. This table does not identify
channels, banks, or the cause of the differences.

## Same-order control: measuring the complete grouped path

The new control sets P=G for G=1/2/4/8/16. There is one offset pass and the visited line numbers
are exactly `0,1,2,3,...` for every group. SimpleTest and Sequence both use eight `W 2R` rounds,
the same four-entry pattern program, and 192 GiB of work per step. Both test orders are run.

This exercises the grouped cursor updates once per G lines while holding the memory visit order
constant. It compares complete generated paths, including prefetch and control differences, not
an isolated spill instruction. It does not reproduce P=512's offset passes and tail scheduling.
Step-derived keys also change with position; forward/reverse runs are not identical-image replays.

5700X, four workers, 256 bits, both runs clean:

| G | SimpleTest mean vs G1 | Sequence mean vs G1 |
|---|---:|---:|
| 2 | +1.13% | +1.06% |
| 4 | +1.33% | +1.05% |
| 8 | −0.18% | −0.05% |
| 16 | +0.10% | +0.29% |

G1 averaged 34,532 MiB/s for SimpleTest and 34,540 MiB/s for Sequence. There is no large grouped-path
penalty in this DRAM-sized four-worker control. This does not make the 227 added static GPR
spill/reload flags imaginary or prove they are free on every processor, width, or working-set size.

8600G, four workers, 512 bits, both runs clean:

| G | SimpleTest mean vs G1 | Sequence mean vs G1 |
|---|---:|---:|
| 2 | −1.00% | −0.01% |
| 4 | −0.57% | −1.09% |
| 8 | −0.21% | −0.30% |
| 16 | −0.84% | −0.19% |

G1 averaged 53,076 MiB/s for SimpleTest and 52,894 MiB/s for Sequence. SimpleTest G2 was slower in
both orders (−1.11%/−0.88%), as was Sequence G4 (−1.49%/−0.69%). These small measured costs should
not be described as zero overhead. With only two orders, neither their exact size nor their cause
is isolated. The largest mean reduction was 1.09%, substantially smaller than the differences
between the P=512/513 workloads.

## Errors and diagnostic controls

The original 8600G campaign reported 12 errors: seven test and five seal errors, with no WHEA events:

| Run | Result |
|---|---|
| Candidate G1 ABBA run | Two sequential SimpleTest errors and one seal error, all bit 34; not reproduced on rescan |
| Grouped 256-bit reverse | Sequence G8, random phase: one bit-39 error; not reproduced on rescan |
| Grouped 512-bit forward | Four bit-34 seal errors before the first step's chunks; not reproduced on rescan |
| Grouped 512-bit reverse | Sequence G16, random phase: four bit-39 errors; one bad word was found on rescan |

For the reproduced word, the expected value was `0x49e7a6efaa3c0ad7`, actual `0x49e7a66faa3c0ad7`,
XOR `0x0000008000000000`. Its four immediate rereads and the read after flushing were correct.
That is a captured mismatch, not proof of a permanently faulty cell or a particular physical pin.

Two additional baseline controls were run immediately afterward. The 512-bit baseline completed
cleanly; the 256-bit baseline detected bit 34 in sequential Sequence's random phase, also not
reproduced on rescan. Thus the bit-34 symptom is not unique to the candidate. The normalized
assembly fingerprints of the seal fill/check routines and their check wrappers also match between
the retained binaries. None of this isolates the physical cause or proves the same cause for bit 39.

Counts are TMR's reported errors, not independent fault events or unique physical addresses.
The early halts prevented equal exposure across geometries. A further 512-bit screen therefore used
error logging, interleaved SimpleTest/Sequence by geometry, then reversed that order, with all steps
sharing one allocation.

That screen passed with zero test, seal or WHEA errors. Its requested 15-minute limit finished at
the suite-cycle boundary: actual runtime was **17m35s**, five full cycles and 120 complete steps.
Every geometry completed ten visits in each test type: 1,920 GiB per SimpleTest geometry and
2,240 GiB per Sequence geometry, 24,960 GiB (24.375 TiB) of logical work in total. Saved geometry,
order, patterns, widths, worker count, page mix and expected work were checked. No last step was
partial. This supplies full 512-bit execution coverage on the same allocation; its quiet interval
does not rank geometries or invalidate the earlier mismatches.

Including the baseline diagnostic run, the campaign reported eight test errors and five seal
errors on the 8600G, with no WHEA events. All 20 runs on the 5700X and both final same-order
512-bit controls on the 8600G were clean.

## Scope of the conclusion

The hardware tests measure the complete kernel paths. They do not isolate the cost of the added
GPR spills, or test every working-set size, worker count and SIMD width. In particular, 128-bit
SimpleTest has development tests and assembly checks, but no physical timing in this campaign.
All live allocations here are DRAM-sized; four-worker bandwidth can hide costs that matter with
one worker or cache-sized chunks.

Keep the compact shared-body implementation for review, with G1 as the unchanged default and G16
still experimental. The tested geometries produce substantially different workloads, which is the
reason to offer them. These results do not select a best group for error detection, prove channel
targeting, or justify changing imported TM5 geometry automatically.
