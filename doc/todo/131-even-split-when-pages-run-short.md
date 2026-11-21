# TODO 131: an even split when large pages run short

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

## Options (none releases memory)

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

Recommendation: 1 and 2 together. 1 bounds any shortfall's unevenness; 2 makes a shortfall rarer. Check both against the fill
tests (`memory/fill.rs`'s ladder tests) with a pool that runs dry mid-round.

## Next

The owner's go on options 1 and 2, then the build and the fill tests.
