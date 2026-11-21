# TODO 133: moving the test threads between cores, as TM5's unpinned threads move

Raised by the owner, 2026-11-22, from forum feedback: "Original TM5 does jump between cores and threads, and CPPC does force it
to be around specifics. Hence the rewrote 0.12.3 differs from the 0.12 even if its mostly just patched."

## What the sources show

- **TM5 0.12** (`TM5/`, the asm, authoritative): its threads are never pinned. `MainThread.asm` pins a thread only under
  `UseThreadAffinity`, which `tm5.inc` leaves commented out; its comment (cp1251) reads "if defined, set the test threads by
  affinity. This helps a little for Q6600 (x4) when using 2-3 threads. For E8400 (x2) no effect". `TM5.asm`'s process affinity
  is likewise under `NeedSetAffinity`. The Hex-Rays decompile (`TM5_Source/`, a 0.12 build) calls no affinity API at all.
- So TM5's "jumping" is the Windows scheduler's: unpinned threads move wherever it puts them, and with CPPC it favours the
  preferred cores. TM5 doesn't rotate anything itself.
- **0.12.3** isn't in the workspace, so whether it differs here is unchecked. It needs the binary (or its source), then a look
  for affinity, ideal-processor or CPU-set calls.

## What TMR does

Each worker is pinned to one CPU for the whole run (`thread_pool.rs`; `--disable-pinning` unpins them, so Windows schedules them
as it does TM5's). A worker tests its own span from its own core every step, so a core only ever reaches its own thread's
memory, and a fault that needs one core's path to one region shows only if they happen to be paired.

## Options

1. **Rotate the pins at step or cycle boundaries** (`pin-rotate=step|cycle`, off by default): the coordinator moves each worker to
   the next CPU in the run's order, within its NUMA node so memory stays local, between steps. Over a run every core reaches every
   thread's memory. Off the hot path (the coordinator, between steps); a step's figures then mix cores, which the per-thread
   tables would show.
2. **Leave it to `--disable-pinning`**, which already gives TM5's behaviour, and document it as such.
3. Both: 2 now, in the docs; 1 if 0.12.3 or a test shows it finds what fixed pins miss.

## Next

The 0.12.3 binary from the owner or the forum poster, to see what it does; then the owner's call between the options.
