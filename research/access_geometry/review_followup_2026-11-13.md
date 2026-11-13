# Access geometry review follow-up, 2026-11-13

Opus's review identified two gaps: the memory-body audit did not expose the standard spill tool's control-loop findings,
and no retained live run exercised G>1. The audit now reports both scopes, and both combined configs have completed
locally through the existing release. Product code and the release binary are unchanged.

## Spill counts: both tools side by side

Re-derived from `target/access-geometry/candidate.s`, compiled from `72ab3b8`:

| Scope | Ranges | GPR round-trip | GPR reload-only | Clean | SIMD spills |
|---|---:|---:|---:|---:|---:|
| Grouped memory bodies | 456 | 0 | 224 | 232 | 0 |
| Grouped control only | 551 | 227 | 182 | 142 | 0 |
| Both grouped scopes | 1,007 | 227 | 406 | 374 | 0 |

The standard `tools/hot_loop_spills.py` total rises from 2 to 229 GPR round-trip ranges. The other two are the same
`exchange::run` and `loaded::LoadedSummary::clone` entries in both reports. All 227 added flags are in the 12 grouped
kernel name groups. No grouped range calls another function or accesses test data at the wrong SIMD width.

The original `audit.py` required a test-memory operand, excluding `lea`, prefetch and RIP-relative operands. That was
appropriate for identifying memory bodies, but omitted traversal-control stack traffic. It is now an explicit
memory/control split, with a combined count matching the standard tool. The old memory-body count is still correct;
it was not sufficient evidence for calling the complete kernels spill-free.

Both tools identify linear backward-branch ranges. These are static occurrences across specialised functions, not
dynamic spill counts, and a range can cross blocks that do not execute together. The complete grouped implementation
adds 228 specialised functions. Adding the 227 flags together does not mean one invocation spills 227 times per line.

Reproduce without a build, preserving the original audit files:

```powershell
python -B tools/hot_loop_spills.py target/access-geometry/baseline.s
python -B tools/hot_loop_spills.py target/access-geometry/candidate.s
python -B research/access_geometry/audit.py target/access-geometry/candidate.s --output target/access-geometry/review-grouped-audit.json
```

The new output is retained as `review-grouped-audit.json` and `review-grouped-audit.txt`. The original standard-tool
tables remain `baseline-spills.txt` and `candidate-spills.txt`.

## Where the cost occurs

`src/tests/grouped.rs` has three levels: offset passes, visits separated by the period, and a bounded body within each
group. For N lines, period P and group G, validated so G divides N and P:

- There are exactly N/G visits across the complete sweep.
- There are min(P,N)/G nonempty offset passes.
- The memory body executes N times at 128 bits (one line per body), or N/2 times at 256/512 (two lines per body).
- Pointer stepping between groups occurs once per visit. Offset-pass counters and tail-prefetch setup occur once per
  offset pass. They are separate from reloads inside the memory body.

For the live check's 256 MiB chunks, N=4,194,304 lines and P=512:

| G | Group visits per sweep | Offset passes per sweep |
|---|---:|---:|
| 2 | 2,097,152 | 256 |
| 4 | 1,048,576 | 128 |
| 8 | 524,288 | 64 |
| 16 | 262,144 | 32 |

There is real stack traffic at the visit frequency. For example, the retained 512-bit Sequence verifier with symbol
`_RINvNvNtCshRTagmXNTEU_3tmr5tests17sequence_read_512sg_7groupedNtNtB4_6access6DirectEB6_`
keeps its group cursor at `32(%rsp)`. Its group-advance block contains:

```asm
movq 48(%rsp), %rcx
movq 32(%rsp), %rax
leaq (%rax,%rcx,8), %rax
movq %rax, 32(%rsp)
```

The memory-body entry loads the cursor again. These accesses occur around each group, not just once per chunk.
Conversely, the division computing the tail-prefetch offset is once per offset pass, even where the tool's linear
branch range also contains it. Source and control flow are needed before assigning an execution count.

For scale, one 8-byte load/store pair per visit amounts to 16/G bytes of stack traffic per visited line:
8, 4, 2 or 1 byte for G2/4/8/16. This illustrates amortisation, not the total traffic of a kernel: there can be other
loads and stores, and memory-body invariant reloads have their own frequency. Stack traffic normally hits L1; these
ratios are neither extra DRAM bytes nor predicted percentage slowdowns.

**Decision:** treat traversal spills as a real implementation cost. Larger groups amortise that control work but also
change the memory workload. The functional runs below do not isolate the cost; the physical comparison must include
the whole traversal. A further kernel rewrite needs timing evidence, not just the aggregate count. This resolves the
missing accounting, not the outstanding performance gate.

## End-to-end grouped checks

The existing `candidate.exe` was used, SHA-256:

```text
165d7eeea894ded3dbf806203350c879f6196d0d8744d2da5c1cf78dd3832099
```

Host: the development EC2 r8i.2xlarge VM, four exposed cores/eight logical CPUs, Windows Server 2025.
Four workers were pinned to logical CPUs 0/2/4/6. The requested `2GiB-target` was raised by the allocator's 1 GiB per
worker minimum to **4 GiB total**, all 2 MiB pages, four 256 MiB chunks per worker. The saved allocation was checked;
these were not 2 GiB runs.

Both configs were used unchanged. Settings: one suite cycle, 256-bit SIMD, beat 32, seed 126, calibration off,
errors halt, seal enabled. A 180-second cap was provided; every step completed its requested cycles before it.

| Config | Completed steps | Runtime | Counted traffic | Test / seal / WHEA errors |
|---|---:|---:|---:|---|
| `combined.json` | 12/12 | 73 s | 1,248 GiB | 0 / 0 / 0 |
| `combined-reverse.json` | 12/12 | 72 s | 1,248 GiB | 0 / 0 / 0 |

Each run includes P512/G1, P513/G1, and P512/G2/G4/G8/G16 in both SimpleTest and Sequence. Across the two runs,
16 step executions used the grouped kernels. Checks of the saved results confirmed:

- Completed/passed outcome, one complete suite cycle, and zero errors.
- Every expected step ID in the config's order, with its requested `access.period_lines`, `access.group_lines` and SIMD width.
- Every step ended on its cycle count, not a duration limit; all workers completed the requested cycles.
- Exactly 96 GiB per SimpleTest step and 112 GiB per Sequence step, matching their configured operations and allocation.

The total is 2,496 GiB of repeated logical traffic, not that much distinct memory or proof of DRAM traffic on every read.
Analysis work ran alongside parts of these checks. They are functional checks, not controlled throughput measurements.
They establish that grouped configs complete through the real allocator, runner, kernels, seal and result reporting at
256 bits. They do not prove error-finding effectiveness or establish performance on AMD, other SIMD widths or other
transfer profiles.

Exact invocations (from the app root):

```powershell
$env:TMR_HOME = Join-Path $PWD 'target/access-geometry/grouped-smoke-2026-11-13/home'
.\target\access-geometry\candidate.exe config=research/access_geometry/configs/combined.json memory=2GiB-target cputype=cores cpus=4 skip-cores=0 cycles=1 duration=180 seed=126 width=256 beat=32 channels=2 calibrate=off minpage=large maxpage=large errors=halt --no-serials --cli
.\target\access-geometry\candidate.exe config=research/access_geometry/configs/combined-reverse.json memory=2GiB-target cputype=cores cpus=4 skip-cores=0 cycles=1 duration=180 seed=126 width=256 beat=32 channels=2 calibrate=off minpage=large maxpage=large errors=halt --no-serials --cli
```

Retained artifacts: `target/access-geometry/grouped-smoke-2026-11-13/combined-256.txt`,
`combined-reverse-256.txt`, `runs.json` (commands, binary/config hashes and parsed results), and `home/results/` and
`home/logs/`. `summarize_ab.read_run` validates completion, errors, cycles and settings from each console's saved result.

## Other review corrections

- The 76% CPU observation has no retained source and has been withdrawn. The original `activity.jsonl` has the 11%
  sample. Concurrent activity is documented; it does not explain every noisy result.
- All four earlier blocks used ABBA with the same arms in the inner/outer slots. That limitation is now explicit.
  Future comparisons should alternate ABBA/BAAB; the comparison kit documents both label orders.
- The compact TODO now retains the 5.61% same-build variation beside the +0.87%/+0.12% G1 figures. The conclusion is
  no repeatable local regression, not equivalent performance or a proven speedup.
- Whole-function fingerprinting reports 230 changed name groups and 24 added, versus the narrower selected-memory-loop
  comparison's 78 unchanged/24 changed. Both scopes are now stated. Setup and inlined code can change outside the
  selected loops; the smaller count was not a whole-executable claim.

The branch remains experimental. These checks close the local execution gap and expose/control the scope of the spill
claim. Physical-machine throughput and detection comparisons remain the gate before selecting the shipped group set.
