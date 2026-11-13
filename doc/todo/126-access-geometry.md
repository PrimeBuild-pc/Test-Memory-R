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

## Validation complete; hardware gate remains

Code commit `72ab3b8`: full local CI passed, including 581 tests (six opt-in skips), all Clippy variants, release build and
dependency audit. The 364 offline coverage/model cases also passed. The release is archived at
`../builds/72ab3b8b87e2-165d7eeea894ded3/` from the app root, and copied to `target/access-geometry/candidate.exe`.

Final post-LTO audit: 456 grouped memory loops, maximum 193 instructions, correct widths, no calls, no SIMD spills or
spill/reload round trips. 224 loops still reload invariant GPR values; outer visit/pass loops have their own control cost.
`.text` is 7,124,126 bytes versus baseline's 6,744,350 (+5.6%). Groups share 228 added specialized functions, rather than
912 for separate G kernels. The normal release's entire `.text` matches the assembly-audited build.

The baseline comparison confirms 78 existing loop-name groups unchanged; 24 strided groups differ in generated code.
The existing G1 paths are retained, but no claim of identical G1 performance is made. Results, scripts, exact counts and
commands are in `research/access_geometry/README.md`; complete local audits/manifest are under `target/access-geometry/`.

The owner still needs to measure G1 regressions, grouped prefetch/control cost, and error detection on hardware. This is
why G16 remains experimental and why the branch has not merged. P stays divisible by G; odd groups remain deferred.

## Next

The owner's requested local before/after check is complete (2026-11-13): 16 measured runs and two pilots on the dev VM,
fixed 8 GiB / four workers / 256 bits / 2 MiB pages, all passed with zero errors. The first set was noisy, including
observed concurrent CPU activity; a longer ABBA follow-up measured +0.87% for existing strided SimpleTest and +0.12%
for strided Sequence. No repeatable G1 regression was established. Keep the compact implementation for physical testing;
these runs do not establish sub-percent equivalence or grouped-loop cost. Full unfiltered results and per-run CSV:
[`research/access_geometry/local_comparison_2026-11-13.md`](../../research/access_geometry/local_comparison_2026-11-13.md).

The owner runs the baseline-compatible A/B and combined/reversed geometry configs. Use those results to choose the shipped
group set and assess throughput/detection before merging `experiment/access-geometry`. Importer work is settled.
