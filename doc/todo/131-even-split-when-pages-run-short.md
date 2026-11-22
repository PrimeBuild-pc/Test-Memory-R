# TODO 131: allocation short of its target: stop before testing, and keep headroom for large pages

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

**With it, headroom for large-page runs.** A TM5 `.cfg`'s reserve was chosen for TM5's 4 KiB AWE pages (`callDLL.asm`'s
`MEM_LARGE_PAGES*0`, `awe.asm`'s `AllocateUserPhysicalPages`), which any free page can serve; TMR's 1 GiB and 2 MiB pages need
unbroken physical runs, which the last few hundred MiB of a nearly full machine can't give. So:
- a floor under the reserve when the run allows large pages (about 1-2% of what's free, or 512 MiB, whichever is larger), for a
  `.cfg`'s reserve too, said in the plan's table when it raises one;
- the share's rounding never takes the reserve below what was asked (round down, not up, when up would cut into it). Today
  `ShareRounding::fit` stops at what's free (TODO 75 B's reasoning: "available" counts the standby list), so a reserve is soft.

Together they make the stop rare, and when it fires it points at a real problem.

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

The build: the stop and its message (stitched.rs's shortfall path), the headroom floor and the reserve-respecting rounding
(allocation_strategy.rs), their tests (the fake VM's exhaustion paths), and the docs (the guide's memory settings,
test_parameters.md).
