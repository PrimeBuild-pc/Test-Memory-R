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

The first product candidate uses explicit offsets and selection outside the memory loop, preserving the existing group-1
paths. This is a testable starting point, not a demonstrated speed win over the compact alternative. Group 16 remains
experimental because its expansion adds substantially more code. Revisit the choice after product assembly and hardware
comparison; do not infer speed from the instruction counts alone.

The baseline for the product comparison is commit `57369c4`. Its rebuilt EXE, post-LTO assembly, fingerprints and spill details
are kept locally under `target/access-geometry/baseline*`. This uses the repository's selected MSVC 14.52.37025 and Windows SDK
10.0.28000.2957. No live TMR run was performed to obtain it.

## Hardware comparison kit

The files in `configs/` are ordinary TMR JSON configs:

- `baseline-compatible.json`: the existing group-1 period-512 workload, using `parameter=256` and the default `channels=2`.
  Both the baseline and candidate understand it. Includes an unchanged `Spd-DRAMFull-Read-256` reference.
- `combined.json`: P/G 512/1, 513/1, 512/2, 512/4, 512/8 and 512/16, in SimpleTest and Mem-Sequence.
  Every geometry within one test type has the same extent, chunk, pattern program and number of reads/writes.
  All four program entries run as verified polarity pairs. Group 16 is experimental.
- `combined-reverse.json`: the same entries in reverse order, for an order/drift check. Step keys change with position, so
  this changes the data too; it is not a same-image replay. Fixed `seed=` reproduces one config on the same thread/span layout.

Start with an amount of memory and workers appropriate to the host. The examples below request 8 GiB and four physical
cores, one suite cycle, seed 126, at 256 bits. They are commands for the owner to run, not a completed hardware campaign.
Runtime depends on the host and allocation; start with one cycle before scheduling longer comparisons.

```powershell
python tools/bench_ab.py target/access-geometry/ab-g1 "Spd-DRAMFull-Read-256 (reference)" "config=research/access_geometry/configs/baseline-compatible.json memory=8GiB cputype=cores cpus=4 cycles=1 seed=126 width=256" baseline=target/access-geometry/baseline.exe candidate=target/access-geometry/candidate.exe --runs 4

.\target\access-geometry\candidate.exe config=research/access_geometry/configs/combined.json memory=8GiB cputype=cores cpus=4 cycles=1 seed=126 width=256
.\target\access-geometry\candidate.exe config=research/access_geometry/configs/combined-reverse.json memory=8GiB cputype=cores cpus=4 cycles=1 seed=126 width=256
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
