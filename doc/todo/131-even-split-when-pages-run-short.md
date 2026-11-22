# TODO 131: allocation short of its target: a 4 KiB tail, TM5's rounding for imports, else stop

Raised by the owner, 2026-11-22, from a forum report: a TM5 import (anta777's DDR5 Ryzen3D config) on a 32 GiB Ryzen, 12
threads, TMR 0.1.2. The run started and passed its checks, but the threads' memory came out uneven. The reporter's log is kept
outside git (it names their account).

## What happened (from the log)

- **The plan took nearly all free memory.** 26.94 GiB free, the config's 256 MiB reserve, 12 threads: 2.224 GiB raw a thread.
  `blkround=up` to 1 GiB would have needed 36 GiB, so the step fell to 16 MiB, rounding up: 2.234 GiB a thread, 26.81 GiB in
  all. The rounding took 122 MiB of the reserve, leaving 134 MiB (0.5%).
- **1 GiB pages ran out one short.** 21 granted, the 22nd (for thread 9) refused with 1450. Threads 0-8 got two each, 9-11 one.
- **2 MiB pages ran out 0.70 GiB short.** `fill_ladder` (memory/fill.rs) hands each request to the thread holding the fewest
  bytes, 128 MiB at a time: threads 9-11 first, back to 2 GiB, then a 128 MiB round to all 12, then the last 112 MiB each. That
  last round reached threads 9-11, 0 and 1; thread 2's 112 MiB was refused, then 64 MiB granted; thread 3's 64, 32 and 16 MiB
  were refused, and the pool was dry. Threads 4-8 never got their last request. The smallest page allowed was 2 MiB (the
  default), so no 4 KiB fallback.
- **The result:** threads 0, 1 and 9-11 at 2.23 GiB, thread 2 at 2.19, threads 3-8 at 2.12 (each 112 MiB short); 26.11 of
  26.81 GiB. The fairness table flagged it. Every allocated byte was tested; seven threads just had less to test.

This is `fill_ladder` as designed ("lets the threads end about one `top` apart"), and nothing granted is ever released to even
it out: a settled rule (CLAUDE.md, "Never release committed memory to rebalance or re-split it").

## Decided (the owner, 2026-11-22)

**When the smallest page size the run allows can't complete every thread's target, stop before testing.** Exit code 2 (stopped
before testing), and a message with the shortfall, the page size that ran out and what to change: a bigger reserve (`memory=`,
or the `.cfg`'s "Reserved Memory for Windows (Mb)"), `minpage=regular` to let 4 KiB pages finish, or fewer threads. The memory
committed so far goes back as the arena drops (a stop, not a rebalance: the never-release rule is about a running test).

Why: TMR already stops on the last tier's failure when that tier is 4 KiB (a refused 4 KiB commit), but ran on short when the
last tier allowed was 2 MiB. One rule for both. Running short hides a config that asks too much; stopping makes the user (or
TMR) fix it. `stitched.rs`'s `min_large_leaves_threads_short_but_not_empty` test becomes a test of the stop.

**Decided with it (the owner, 2026-11-22), in place of a general headroom floor:**

- **Imported `.cfg`s round down, as TM5 does.** TM5 (`MainThread.asm` ~153-262) takes what's free less the `.cfg`'s
  "Reserved Memory for Windows (Mb)", divides by the threads and rounds each share down to "Lock Memory Granularity (Mb)" (16
  MiB by default), never up, so the reserve asked is kept. A TMR import does the same with the settings TMR already has: the
  conversion sets `blkround=down` and `blkroundtarget=` to the granularity (at least 16 MiB, `largefloor`, which the step must
  be a multiple of). TMR's own plans and configs keep `blkround=up` at 1 GiB. Until it's built, a user can give the same on
  the command line: `blkround=down blkroundtarget=16MiB` (and `minpage=regular` for a 4 KiB tail, pageable in 0.1.2). Here it would have asked 26.62 GiB and kept 326 MiB, 190 MiB back: on its own not enough for
  this run (its 2 MiB pages came 0.70 GiB short), so with:
- **A capped 4 KiB tail.** When the large-page tiers run short, each thread's rest on 4 KiB pages, up to about 5% of its target
  (more stops the run, rule 1). 4 KiB pages need no unbroken runs, so the free memory large pages can't use serves them: here
  about 0.8 GiB. Its cost is on the tail alone, more TLB misses there (an estimate, not measured: a percent or two slower over
  it for the streaming tests, more for the random ones, well under 1% of a run).
  - **Both forms, a setting (the owner, 2026-11-22):**
    - **Locked**, the proposed default: TMR raises its working-set minimum by the tail's size (`SetProcessWorkingSetSizeEx`)
      and locks it (`VirtualLock`), so Windows can't page it out. That needs "Increase a process working set", which Windows
      gives every user by default, not the large pages' "Lock pages in memory". If the minimum or the lock is refused, the run
      stops (rule 1: no silent gap; a page Windows trims is read back from the page file, not tested in RAM).
    - **Pageable**, opted into: the tail as ordinary memory, with a warning, as TM5's runs without AWE are.
  - **Phased (the owner, 2026-11-22):** phase 1 locks the capped tail only. A run whose 4 KiB part isn't a tail
    (`minpage=regular`, or no "Lock pages in memory" so everything is 4 KiB) stays pageable, as in 0.1.2, until phase 2: a
    live check that locking tens of GiB of 4 KiB pages (a working-set minimum that size) holds up on a desktop, then locked
    there too.
  - The threads table shows a thread's 4 KiB part, locked or pageable. (Planned as "`minpage=large` given outright means no
    tail", which can't work: `large` is the default. Built as `tail4k=off` instead; see "Built" below.)
  - **TM5's own locking**, for comparison (the owner asked, 2026-11-22): AWE is on by default (`tm5.inc`'s `bEnableAWE db 1`),
    locked physical pages, and is turned off without admin rights (`mt_ini.asm` checks with `LsaOpenPolicy`), by the `.cfg`'s
    `Capable` bits 1, 2 or 4, or when a thread's share comes out too small. Without AWE TM5 tests ordinary memory, at most one
    Testing Window a thread, and calls `VirtualLock` only under `Capable` bit 8 (`Capable_UseVirtualLock`), not checking its
    result (`function.asm`). The common `.cfg`s have `Capable=0x0`, so a TM5 run without admin is pageable and can page when
    pushed: the owner's recollection. AWE also needs "Lock pages in memory", and its pages can't sit in a stitched span.
- **Not taken:** TM5's bigger reserve when the page file is off (TM5 detects it from `GlobalMemoryStatusEx`'s commit limit
  against the RAM, and reserves up to 1 GB). The owner: the reserve is the user's and the config's to choose, not a heuristic's.
  The reporter had a page file (a 36.57 GiB commit limit on 31.57 GiB), so it wouldn't have applied here anyway.

**Dropped: the finer last round** (option 1 below). With a short run stopped, unevenness never reaches a test.

## The options weighed before the decision

### Options (none releases memory)

1. **A finer last round.** Once each thread's remaining need is under `top`, hand it out in floor-sized (16 MiB) requests,
   round-robin, so a pool that runs dry mid-round leaves every thread within 16 MiB of the others instead of 128 MiB. Here
   the 624 MiB granted in the last round (five 112 MiB and one 64 MiB) would have spread about 52 MiB a thread, each about 60
   MiB short, where five were short none, one 48 MiB and six 112 MiB. The cost is more requests in that round only (about 7
   a thread here, in milliseconds).
2. **More headroom in the plan.** Large pages need contiguous physical memory, so "available" overstates what they can be cut
   from: standby and cached pages count as available but don't all come back as 2 MiB pages. Round the per-thread share down,
   not up, when rounding up would take the reserve below what was asked; or keep a floor of headroom for large-page runs (a
   few hundred MiB, or 1-2%). Here rounding down would have asked 26.62 GiB and kept the 256 MiB.
3. **Even test extents (no).** Capping every thread's full-allocation extent at the smallest thread's would even the work, but
   leaves granted memory untested; TMR's point is to test it all.

Recommended before the decision (superseded by it, above): 1 and 2 together. 1 bounds any shortfall's unevenness; 2 makes a shortfall rarer. Check both against the fill
tests (`memory/fill.rs`'s ladder tests) with a pool that runs dry mid-round.

## Next

Phase 1, the build: the stop and its message (stitched.rs's shortfall path), the import's round-down (config.rs's `.cfg`
conversion and allocation_strategy.rs), the capped 4 KiB tail with its locked and pageable forms (fill.rs, stitched.rs), their
tests (the fake VM's exhaustion paths), and the docs (the guide's memory settings, test_parameters.md, the README's page-size
notes). Then an A/B on the dev box against 0.1.2, with physical memory fragmented (AWE pages held one per 2 MiB region) so 2
MiB pages run short as on the reporter's desktop: 0.1.2 granted 51.67 of 56.75 GiB there, threads 6.38-6.50 of 7.09 GiB, and
ran.

Phase 2: the live check of locking an all-4 KiB run, then its lock.

## Built: phase 1 (2026-11-22, c5f66b5)

- **The setting**: `tail4k=locked|pageable|off` (JSON `regular_tail`), default `locked`; only with `minpage=` above 4 KiB. The
  cap is `stitched::TAIL_PERCENT` (5%) of each thread's target; every short thread must be within it, else no tail is taken.
- **Locking**: `VmOps` gained `raise_working_set`, `lower_working_set` and `lock`. The tail raises the working-set minimum
  (and maximum) by the whole shortfall plus 16 MiB of slack, soft limits, after enabling `SeIncreaseWorkingSetPrivilege` (held
  by every user, disabled by default, as `whoami /priv` shows on the dev box); each 4 KiB commit is then `VirtualLock`ed as it
  is made. The arena lowers the minimum again on drop, since Windows charges it until then and the window runs sessions in one
  process. Either refusal stops the run.
- **The stop**: `stitched: 1 GiB and 2 MiB pages ran 5.53 GiB short of the 57.62 GiB asked for: 8 of 8 threads short, thread 2
  the most (6.50 of 7.20 GiB, 9.8% short), more than the 4 KiB tail takes (5% of a thread's share). A run tests every thread's
  whole share or stops here: free more memory, by a bigger reserve (...), fewer threads (cpus=) or a restart (...), or
  minpage=regular ...`, exit 2.
- **The report**: one footer line under the threads table (`4 KiB: the rest where the larger pages ran short, locked in RAM
  (tail4k=locked).`). The Status column's fairness check also flags each tail thread 🟡 `4 KiB 64.00 MiB`, repeating the
  column.
- **The import**: `LegacyConfig::memory_allocation` sets `blkround=down` and `blkroundtarget=` to the Lock Memory Granularity,
  at least `largefloor` and a multiple of it.

## The A/B (dev box, 2026-11-22)

`pressure256.cfg` (1usmus_v3 with Reserved 256 MiB, test 1 once), 8 threads; 0.1.2 (66a34db) and the build (c5f66b5) back to
back under one fragmenter: AWE pages taken, one kept in each 2 MiB physical region they fell in, the rest freed, held while
both ran (`scratch_119b/ab131/`, outside git). How far a fragmenter's pages spread varied run to run (2 GiB of them blocked
3.5 to 18.2 GiB of 2 MiB room), so each level is one pair, not a sweep.

| Fragmenter (2 MiB room blocked) | 0.1.2: asked, granted, reserve left | The build: asked, reserve left, outcome |
|---|---|---|
| none | 57.75, all, 201 MiB | 58.62, 338 MiB, ran full |
| 512 MiB (1.0 GiB) | 59.75, 59.16 (8 threads, up to 96 MiB short), 35 MiB | 59.50, 268 MiB, 0.34 GiB short over 6 threads: a locked tail (6 locks, 1 working-set raise), ran full |
| 1 GiB (9.0 GiB) | 57.75, 51.94, 195 MiB | 57.62, 297 MiB, stopped: 5.69 GiB short, thread 7 10.6% |
| 2 + 6 GiB (10.9 GiB) | 57.75, 52.09 (threads 9-10% short), 74 MiB | 57.62, 288 MiB, stopped: 5.53 GiB short, thread 2 9.8% |

0.1.2 ran every short case and passed, testing less than it planned. The tail run's test speed matched 0.1.2's (82,034 against
81,998 MiB/s, one run each). Not checked: a standard (non-admin) account's raise; a tail of several GiB.
