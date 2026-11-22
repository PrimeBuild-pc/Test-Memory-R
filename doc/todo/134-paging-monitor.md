# TODO 134: a paging monitor (pages read in, written out, or both)

Raised by the owner, 2026-11-22, from TODO 131's discussion: TM5 0.13 added a paging-per-second monitor, to show when the system
was slowing down from paging and could be hurting the test's speed. TMR has nothing like it.

## Why

- **The test's own memory doesn't page.** TMR's 1 GiB and 2 MiB pages are non-pageable, and so would a locked 4 KiB tail be
  (TODO 131). TM5's AWE pages are the same.
- **Everything else can.** A run takes nearly all free memory, leaving the reserve (a TM5 `.cfg`'s is often 128-256 MiB).
  Windows makes room by trimming other processes and the file cache, writing their modified pages out; when they touch that
  memory again it's read back in. That disk traffic and its faults compete with the test for CPU time and memory bandwidth, and
  slow the machine.
- **So it's evidence for two things a run can't show today:** whether a step's speed was hurt by paging (a figure to distrust,
  or a reason a run is slower than the last), and whether the reserve was too small (TODO 131's case).

## What to read: in, out, or both

| Counter (`\Memory\...`) | What it is | What it says about the run |
|---|---|---|
| Pages Input/sec | pages read from disk to resolve hard faults | others (or a pageable tail) faulting back in: the slowdown itself |
| Pages Output/sec | pages written to disk to free memory | Windows making room: pressure, highest while TMR allocates |
| Pages/sec | the two summed | the classic single figure, TM5 0.13's kind |

Proposed: both, kept apart in the results file (they mean different things: output is pressure, input is its cost), and their
sum shown live. Output during allocation is worth its own line: how much Windows wrote out to satisfy TMR's request, which says
the reserve was too small before a test runs.

## How

- **Reading it**: PDH (`PdhOpenQuery`, `PdhAddEnglishCounterW` so the counter path works in any language, `PdhCollectQueryData`;
  the `windows` crate's `Win32_System_Performance` feature). A documented, cheap read of system-wide counters. Undocumented
  `NtQuerySystemInformation` classes would avoid PDH but aren't worth it.
- **Where**: the reporting thread's cadence, which already polls WHEA (`whea.rs`'s `poll`) and publishes progress, every 250 ms.
  Never the test loops.
- **Showing it**: a figure on the ticker and the window (pages/s, or MiB/s), a step's mean and peak in its report, the run's in
  the summary and the results file, and a warning when paging passes a threshold during a step (open: what threshold; Microsoft's
  old rule of thumb is sustained Pages/sec in the hundreds on a disk, far more on an SSD, so it should be a setting with a
  conservative default).
- **Privacy**: system-wide counters only, nothing about other processes.

## Open questions

- In, out or both (proposed: both, apart, their sum live).
- The warning's threshold and whether it should ever fail a step (proposed: never; a speed caveat, not an error).
- A per-step figure or only the run's (proposed: per step, as WHEA is).

## Next

The owner's calls on the open questions, then the build.
