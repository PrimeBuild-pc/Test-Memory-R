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
- G=1/2/4/8/16 accepted on the experiment branch; G16 labeled experimental. P must be positive, at least G, and divisible
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

## Gates for accepting the experiment

1. Macro cost: compare constant-offset expansion against a bounded runtime/microtile candidate at one width. Macros keep the
   source shared but may emit specialized code across family/beat/operation/group/ISA. Auto dispatches the three real widths
   (128/256/512); it is not a fourth width. A separate kernel per G is an implementation choice, not a consequence of the rule.
2. Grouped prefetch lookahead and speed must be measured (`bench_ab.py`, `hot_loop_spills.py`), not assumed free.
3. Whether `period_lines`/`group_lines` reuse the group-1 hot path unchanged for the direct-period case (should be yes; confirm
   via post-LTO asm per this repo's asm-check rule).
4. Select the shipped group set after the bounded experiment. G=2/4/8 are the primary candidates; G=16 differs under coarser
   interleave models. Keep P divisible by G. Defer odd G: different per-visit imbalance is possible, but adds tail/period cases
   without evidence of a required error-detection benefit.

## Next

Complete the release assembly/spill review and full local CI, retain the candidate binary, then have the owner run the
comparison kit. Hardware throughput and detection results determine whether the expanded group set should merge or be
reshaped. No live TMR run is authorized as part of these development checks; importer work is settled.
