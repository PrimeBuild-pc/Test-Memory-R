# Access spacing and grouping

SimpleTest (`Mem-SimpleV2`) and Mem-Sequence can vary how far they jump between visits and how
many neighbouring cache lines they access before each jump. These are optional settings on
existing tests, not a new test or a replacement for the stability plan. They change the order
of accesses while keeping the test's patterns, verification and seal checks.

## The two settings

Put these fields in a test entry in a TMR JSON config:

| Field | Meaning | Values |
|---|---|---|
| `period_lines` | Distance between visit starts, in 64-byte lines | Positive integer, at least the group size and divisible by it |
| `group_lines` | Consecutive lines accessed in one visit | 1 (default), 2, 4, 8 or 16 |

For example, `"period_lines": 512, "group_lines": 2` accesses 128 bytes per visit, with
32 KiB between visit starts. One offset pass visits lines `(0,1)`, then `(512,513)`, then
`(1024,1025)`, and so on. Other offset passes cover the remaining lines.

**Every sweep covers each line of the chunk once.** The example above illustrates offset zero;
the actual sweep starts with the highest applicable offset group and works downwards, with
lines inside each group accessed upwards. It does not leave the gaps between visits unchecked.
Chunks can still overlap, as they do in other TMR tests.

These settings are independent of:

- **`width=`**, the CPU's SIMD load/store width;
- **`beat=`**, the transfer profile used to generate switching data;
- **`channels=`**, the multiplier used by the older TM5-style stride setting.

They describe addresses in the thread's allocation. Neither setting selects a physical memory
channel, bank or row, or changes the BIOS's interleaving configuration.

## What to try, and why

| Period / group | Accesses | Reason to compare it |
|---|---|---|
| 512 / 1 | 64 B per visit, 32 KiB between starts | Repeats the same offset in successive 32 KiB intervals |
| 513 / 1 | 64 B per visit, 32,832 B between starts | Changes low address bits between visits |
| 512 / 2 | 128 B per visit, 32 KiB between starts | Adds the adjacent line before each jump |
| 512 / 4 or 8 | 256 or 512 B per visit, 32 KiB between starts | Does more work at nearby addresses before jumping |
| 512 / 16 | 1 KiB per visit, 32 KiB between starts | A longer local run before each jump |

These are starting points for comparison, not ranked presets. Repeated low address bits may
concentrate requests on particular cache sets or memory resources; changing those bits may
distribute them differently. Larger groups add local accesses between jumps and can change
prefetching and the number of requests the CPU keeps outstanding. Which effect dominates
depends on the processor and its address mapping.

Faster throughput alone does not establish stronger electrical stress or better error
detection. The physical comparisons so far show throughput differences, but have not identified
a best geometry for finding faults. In particular, the large difference between periods 512
and 513 is a spacing result, not proof of a grouping benefit.

## Run the comparison

The installer includes [`access_geometry.json`](../../configs/access_geometry.json). It runs
twelve steps on the same allocation: the six geometries above in SimpleTest, then the same
six in Mem-Sequence. Each uses the native `ident, switch, random, switch` program. Within
each test type, the rounds and checks are equal across geometries. Mem-Sequence uses
`W 2R RW 2R`, all in stride order.

Preview it, then run it:

```text
tmr config=access_geometry.json --startup-debug
tmr config=access_geometry.json memory=8GiB-target cpus=4 cycles=1 errors=halt
```

Adjust the memory and CPU count for your machine. The example requests 256 MiB chunks over
each thread's full allocation; smaller allocations cap the chunks. Its duration depends on
the allocation and machine, rather than a fixed time limit. A bare-exe installation can
download the config from the link above and place it in TMR's `configs\` folder.

The plan describes bytes per visit and bytes between visit starts, and reports when a chunk
is too small for any period hop. Results retain the requested
geometry. Compare the same test type with itself: SimpleTest and Mem-Sequence perform different
amounts of work.

One run keeps the allocation, but its steps still run at different times and use different
pattern keys. For closer comparisons, repeat with the test entries in reverse order, keep
memory, workers, page size and SIMD width the same, and retain runs that report errors.

## Add it to your own config

These are test entries to insert in an existing config's `test_sequence`:

```json
{
  "id": "simple-p512-g2",
  "enabled": true,
  "function": "Mem-SimpleV2-Auto",
  "cycles": 1,
  "write_read_cycles": 8,
  "verify_reps": 2,
  "program": ["ident", "switch", "random", "switch"],
  "period_lines": 512,
  "group_lines": 2
}
```

```json
{
  "id": "sequence-p512-g4",
  "enabled": true,
  "function": "Mem-Sequence",
  "cycles": 1,
  "write_read_cycles": 4,
  "verify_reps": 2,
  "program": ["ident", "switch", "random", "switch"],
  "sequence": "W 2R RW 2R",
  "order": "stride",
  "period_lines": 512,
  "group_lines": 4
}
```

The rules are:

- These are **per-test JSON fields**, not command-line overrides. SimpleTest supports its
  128/256/512/Auto variants; Mem-Sequence uses its usual 256/512-bit dispatch.
- Use `period_lines` or `parameter`, never both. `group_lines` requires `period_lines`.
- The period must divide into complete groups: 513/1 works, 513/2 is rejected. Period 496
  accepts every supported group, including 16, and is useful for comparing groups at a common
  spacing that is not a power of two.
- Groups above 1 require native patterns. A legacy pattern override is rejected.
- Mem-Sequence must have at least one `stride` pass. Its forward, reverse and random passes
  keep their own order.
- These fields do not apply to SimpleNT, Mem-Stride, CacheBust or the mirror tests.

## TM5 imports and defaults

For a positive TM5 SimpleTest `Parameter`, the imported visit period remains
`Channels × Parameter` lines, with one line per visit. Thus `Channels=2, Parameter=256`
gives period 512 and group 1. An explicit JSON period expresses that distance directly,
without depending on the run's channel setting.

The checked community configs all use `Interleave Type=1`, matching the one-line visit.
`Interleave Type=0` on two or more channels is TM5's 128-bit mode: each visit is 128 bytes,
two lines, and the period is `2 × Parameter` lines whatever the Channels. An import translates
it into that geometry, `period_lines` 2 × Parameter and `group_lines` 2 (a Parameter of 1 or
less is sequential); with `patterns=legacy` it keeps group 1, with a warning.

Existing plans and TM5 imports retain their current walks. Even `plan=all` runs each
registered test with its settings, rather than generating every period/group combination.
Use a JSON config to compare geometries.

The [completed hardware follow-up](https://github.com/jlpetz/Test-Memory-R/blob/main/research/access_geometry/hardware_followup_2026-11-16.md)
also compared all groups at period 496. The fastest group differed between the two CPUs;
both ordinary and grouped walks detected errors, without establishing a best detector.
Use a mix of geometries when comparing stability. No group is recommended as a universal
hardware target, and the existing defaults remain unchanged.

Back to [the stability tests](stability.md) or [all tests](README.md).
