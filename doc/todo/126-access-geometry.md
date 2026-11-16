# TODO 126: Explicit access geometry (`period_lines`, `group_lines`) as per-test tunables

## Why (Astra, 2026-11-12, from the Check_absolutnew.cfg validation)

Reviewing TM5's `Channels`/`Parameter` stride formula raised the question of whether TMR should try to target specific DRAM
channels. Neither channel count nor an interleaving toggle establishes the active address mapping. Reliably targeting hardware
destinations also needs physical-address information, which this project has decided to stay out of
(`TMR-APP/CLAUDE.md`, "No physical-address awareness"). Published mappings (AMD BKDG, ZenHammer,
Intel's FSP header and public register docs — see `doc/memory_mapping_research_2026-11-12.md`) confirm the mapping is partly
documented but not a portable formula from CPU name, channel count or a BIOS toggle.

The proposal's reframing: expose SimpleTest/Mem-Sequence's existing access distance and add adjacent-line grouping, without
claiming it selects hardware. Value is access-pattern diversity over the same allocation (several
shapes in one run, sharing memory state and conditions, per the project's existing "Pattern Comparisons In One Run" practice),
not proof of hitting a specific channel.

## Decided (the owner and Opus, 2026-11-12)

- Direction endorsed: build it, after the current round (117, 119, 122).
- Preset naming must not imply hardware targeting ("Spaced", "Paired" etc. need their *displayed* text to carry the caveat,
  not just surrounding prose) — the plan-text format in the proposal's §3 ("Access: 64 B per visit, 32 KiB between visit
  starts") is the right model: no hardware-sounding noun at all.
- The approved importer work and pattern-conversion reporting are complete. The owner declined the full ABSOLUT regression
  fixture and ordinary missing-section compatibility; neither remains a prerequisite.
- The owner requested a hardware-stress rationale and assessment of larger groups before implementation. The proposal now
  includes TM5's stated intention (one DIMM before the next), conditional concentration/distribution effects, and G=2/4/8/16.
  On 2026-11-13 the owner approved evaluating G=1/2/4/8, with 16 as an additional candidate, and starting implementation on
  a separate branch. Odd groups remain deferred.
- On 2026-11-15, after the physical comparison and Opus's review, the owner approved merging the branch as implemented,
  with the additional hardware comparisons deferred to this TODO. G=1/2/4/8/16 remains available.
  Existing plans and TM5 imports remain G1. The merge does not claim improved error detection or select a preferred group.
- The owner subsequently requested removing the G16-only experimental label. All supported groups share the same
  qualification: no group has established a detection advantage. No kernel, supported value or default changes.

## Full proposal and research

- `doc/access_geometry_proposal_2026-11-12.md`: the proposed `period_lines`/`group_lines` fields, named presets, the exact
  walk/coverage proof, configuration contract, hardware-stress hypotheses, macro sketch, and questions for critique.
- `research/access_geometry/models.py`: reproducible additional coverage checks (364 N/P/G cases through G=16), plus six
  illustrative channel selectors over full sweeps and five aligned bases. No hardware traffic or error detection is modeled.
- `doc/memory_mapping_research_2026-11-12.md`: the AMD/Intel sourcing (spot-checked by Opus against the Intel FSP header
  directly — exact match on `ChHashInterleaveBit`/`ChHashMask`'s bit ranges and granularity claims), and the review of importer
  commit `8675f6c` that found the code-0 chunk-doubling regression (independently verified, then fixed by reverting the floor
  rather than reordering it — see commit `3517ed7`).

## Implementation experiment, 2026-11-13

Branch: `experiment/access-geometry`, based on `57369c4`. The baseline was rebuilt before code changes, using the same
compiler/SDK, and retained under `target/access-geometry/baseline*`.

- `d9981d3`: per-test explicit period and group-1 plumbing; 575 tests passed, six opt-in skips, quick CI clean.
  `channels=` still recalculates the legacy `parameter` path, but never changes an explicit period.
- Grouped native SimpleTest kernels at 128/256/512; Mem-Sequence's stride passes at its existing 256/512 widths.
  Dispatch is outside the memory loops. Groups share a bounded body (one line at 128 bits, two at 256/512) and one traversal macro.
  No new pattern generator, physical
  mapping, default test-program change, or TM5-import geometry change.
- G=1/2/4/8/16 accepted. P must be positive, at least G, and divisible
  by G. G>1 with legacy patterns fails validation, including after CLI overrides. A Sequence with explicit geometry must
  actually contain a stride pass.
- Four independent error accumulators. At 512 bits, each two-line tile rotates the two pairs. A failed reduction still
  uses the existing rescan/error/halt path.
- Prefetch remains one per visited line, up to 128 visited lines ahead, rounded to a complete group and limited by the
  shortest offset pass. Empty offset passes are skipped when the period exceeds the chunk. The plan explains when no
  period hop fits; it does not claim a hardware destination.
- Plan notes and results retain the requested geometry. Example configs compare all six shapes, in both test types, with
  equal work within each type; a reversed-order version and a baseline-compatible group-1 config are supplied.

The isolated AVX2 ident probe compares constant bounds, explicit expansion, and a runtime group of two-line tiles. Its
inner loops have no stack accesses, but counts are not timings. The first product candidate (`d71d1d1`) exposed what that
small probe missed: expanding every pattern grew `.text` 51.9%, the longest grouped loop was 1,682 instructions, and the
worst had 178 stack accesses. There were no grouped calls or wrong-width accesses. This prompted the shared bounded body;
the all-two-line version cut `.text` growth to 5.9%, but one SIMD spill at 128 bits prompted a one-line body at that width.
The intermediate candidates are retained for comparison. Full data and commands:
[`research/access_geometry/README.md`](../../research/access_geometry/README.md).

Tests exercise the actual kernels: complete recorded visit order, every pattern family/beat profile/polarity, corruption
and non-cancelling differences, guard words, overlapping writes, periods above chunk length, and grouped read/write/invert
passes. Harness tests extend seal restoration and error-count/halt checks to grouped walks. SIMD execution is limited to
the host's supported widths; compilation and post-LTO inspection cover all emitted widths.

## Development validation

Code commit `72ab3b8`: full local CI passed, including 581 tests (six opt-in skips), all Clippy variants, release build and
dependency audit. The 364 offline coverage/model cases also passed. The release is archived at
`../builds/72ab3b8b87e2-165d7eeea894ded3/` from the app root, and copied to `target/access-geometry/candidate.exe`.

Final post-LTO memory-body audit: 456 grouped ranges, maximum 193 instructions, correct widths, no calls, no SIMD spills or
spill/reload round trips within those bodies. 224 reload invariant GPR values. The original audit omitted control-only
ranges: the standard tool reports 229 GPR round-trip ranges versus baseline's 2, all 227 new flags in grouped traversal.
The corrected audit reports those alongside the memory bodies; these kernels are not spill-free. Group-control work
executes once per G visited lines, with offset-pass setup once per pass. It is a real cost requiring timing, not a
whole-program per-line count. Full reconciliation and bounds:
[`review_followup_2026-11-13.md`](../../research/access_geometry/review_followup_2026-11-13.md).
`.text` is 7,124,126 bytes versus baseline's 6,744,350 (+5.6%). Groups share 228 added specialized functions, rather than
912 for separate G kernels. The normal release's entire `.text` matches the assembly-audited build.

The baseline comparison confirms 78 selected memory-loop name groups unchanged; 24 strided groups differ in generated code.
Whole-function fingerprints instead report 230 changed name groups and 24 added: the narrower loop figures do not
describe all code-generation changes.
The existing G1 paths are retained, but no claim of identical G1 performance is made. Results, scripts, exact counts and
commands are in `research/access_geometry/README.md`; complete local audits/manifest are under `target/access-geometry/`.

The subsequent physical campaign measures G1 comparisons, complete grouped paths and an error screen; its report is
linked below. The owner approved the merge with all five group sizes. P stays divisible by G; odd groups remain deferred.

## Local and physical follow-up

The owner's requested local before/after check is complete (2026-11-13): 16 measured runs and two pilots on the dev VM,
fixed 8 GiB / four workers / 256 bits / 2 MiB pages, all passed with zero errors. The first set was noisy, including
observed concurrent CPU activity; a longer ABBA follow-up measured +0.87% for existing strided SimpleTest and +0.12%
for strided Sequence, while same-build differences reached 5.61%. No repeatable G1 regression was established. Every
block used the same ABBA slot assignment; future comparisons should alternate ABBA/BAAB. The unsupported 76% CPU
observation has been withdrawn from the report; its retained sample shows 11%. Keep the compact implementation for physical testing;
these runs do not establish sub-percent equivalence or grouped-loop cost. Full unfiltered results and per-run CSV:
[`research/access_geometry/local_comparison_2026-11-13.md`](../../research/access_geometry/local_comparison_2026-11-13.md).

Review follow-up: both combined geometry configs now passed end-to-end locally on the retained release, 12 steps each,
with G2/G4/G8/G16 in both test types, 4 GiB/four workers/256 bits/2 MiB pages and zero errors. Saved geometry, cycles and
processed work were checked. These are functional checks, not isolated spill-cost measurements or detection comparisons.

The owner authorized the physical campaign on 2026-11-15. The retained binaries ran on Ryzen 5700X/DDR4 and 8600G/DDR5,
with 8 GiB, four workers and fixed 2 MiB pages. ABBA/BAAB exchanges the inner and outer build slots. Combined geometry runs,
same-forward-order P=G controls and a longer 512-bit screen complement the existing-G1 comparison. Actual hardware errors
and incomplete runs are retained, and excluded from clean speed comparisons. Full results and their limits:
[`hardware_comparison_2026-11-15.md`](../../research/access_geometry/hardware_comparison_2026-11-15.md).

The campaign completed 37 runs. All 20 on the 5700X passed; the 8600G had eight test and five seal errors across five
halted runs, including one baseline run with the same bit-34 symptom seen on the candidate. Its later 17m35s screen
passed all 120 steps at 512 bits. No large G1 regression was established. In the same-forward-order controls, the largest
mean grouped-path penalty was 1.09% at 512 bits; this is a measured small cost, not a zero-overhead claim. All final controls
passed, results were fetched, and both machines are idle with no further runs queued.

## Merged implementation; hardware follow-up

The feature is an optional geometry setting for existing SimpleTest and Mem-Sequence kernels, not a replacement for another
test. The owner chose to merge it as implemented. No new geometry is enabled in default plans or TM5 imports.
Importer work is settled: positive `Channels * Parameter` preserves the Type-1 period and all checked community configs use
Type 1. Type 0 is still ignored; automatic conversion or a diagnostic for it is outside this merge.

Opus's review confirmed the reported numbers but challenged their interpretation. The largest 512-versus-513 throughput
change is a G1 period comparison, not evidence for grouping. The fixed-period grouped comparisons used P=512; the P=G
controls held line order constant but did not compare groups at one common non-power-of-two period. Cache/prefetch effects
could explain much of the measured variation. No particular cause or detection benefit has been established.

These follow-ups were deferred from the merge. The owner authorized the campaign on 2026-11-16; execution is recorded below:

1. **Compare groups away from P=512.** On each desktop, use P=496 with G=1/2/4/8/16, plus 508/1 as a community-config
   reference and 512/1, 512/16 and 513/1 as controls. 496 is divisible by every supported group. 508 is divisible only by
   G=1/2/4, and 504 only by G=1/2/4/8; do not silently round an invalid pair. Use the same allocation, native program,
   rounds, verified work, page size and workers; include both test orders and retain per-step exposure. Two runs per host,
   each containing forward and reverse orders, give four observations per geometry and test type. Previous step timings
   suggest about 15 minutes on the 5700X and 11 minutes on the 8600G for this expanded matrix.
2. **Complete the existing-G1 A/B at 512 bits.** The matched physical regression blocks were at 256 bits. The 512-bit
   baseline diagnostic and candidate screens were not a matched A/B. Build both comparands with the same compiler, SDK
   and settings; alternate ABBA/BAAB with a reference, retain errors, and exclude incomplete runs from clean speed comparisons.
   Eight runs on the 8600G are estimated at nine minutes. Together with item 1, reserve about 30 minutes with both desktops
   running in parallel; these short comparisons measure throughput, not fault-detection effectiveness.
3. **Separate visit shape from prefetch effects.** Record effective lookahead for each P/G. The target is up to 128 visited
   lines, so visits ahead scale with G and are limited by the shortest offset pass. A throughput change alone does not
   establish different DRAM row activity, channel use, electrical stress, or better detection.
4. **Longer detection comparison after warm-up, without reboots.** The owner requested one boot, longer runs and 5–10 minutes
   of memory load before the measured series (2026-11-15). Proposed starting budget: a ten-minute warm-up, then three
   45-minute measured runs back-to-back at fixed settings. Each run repeatedly mixes the comparison geometries on its own
   allocation; alternate forward/reverse ordering and vary the starting geometry between runs. Compare geometries within
   each run first: restarting TMR can change the physical memory allocated.
   Keep allocation size, workers, page size, patterns and verification settings fixed. Retain warm-up errors as failures,
   separately from the warmed comparison. Use continuing error reporting so one failure does not remove the remaining
   geometries' exposure. Record elapsed time, verified work and repeated addresses; repeated checks are not independent
   fault events. Compare exposure-normalized rates as well as time to first error.
   Ten minutes is a conditioning interval, not proof of thermal equilibrium. Record temperatures if existing telemetry
   provides them, and repeat the warm-up after a substantial idle gap. The proposed detection series is 145 minutes per
   desktop before setup and suite-cycle overrun; both can run in parallel. It is an initial screen, not a promise of enough
   independent errors to rank geometries. No reboot or BIOS change is required.

Use those results to improve recommended configurations and assess the value of grouping. Complete-path timings include
real traversal overhead; they do not isolate individual spills or establish the best geometry for detection.

### Campaign started, 2026-11-16

Both desktop queues started around 12:40 AEDT with the retained, matched baseline `57369c4` and candidate `72ab3b8`;
no rebuild or BIOS changes. Each host passed idle/free-memory checks, deployment hash verification and all config previews.
The 5700X runs at 256 bits and the 8600G at 512 bits, both with 8 GiB, four workers, 256 MiB chunks and fixed 2 MiB pages.

The queues have six processes on the 5700X and fourteen on the 8600G: warm-up, two geometry runs, then three long screens,
with eight baseline/candidate runs added on the 8600G before its screens. Each geometry/screen cycle contains all 18
test/geometry pairs in mirrored order (36 steps), with starting positions rotated for the three screens. Warm-up lasts
600 seconds and each screen 2,700 seconds, plus suite-cycle overrun. Errors are logged without truncating exposure.

The private archive `../baselines/2026-11-16-access-geometry/` retains the generator, configs, exact queues, runner snapshot,
SHA-256 manifest, preflight records and automatic result collection. Its README records the effective prefetch distances:
at P496, G1/2/4/8/16 look 128/64/32/16/8 visits ahead, respectively. The chunk is large enough that none is clipped.
Thus this campaign compares complete workloads, including their prefetch differences.

After the queues finish, validate coverage/settings and analyse throughput and error evidence before closing TODO 126.
Fix any concrete implementation defect first. An inconclusive fault-detection ranking alone does not require another campaign.
