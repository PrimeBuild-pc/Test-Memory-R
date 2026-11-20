# TODO 128. Show repeated error observations in saved results

On daddy-petz, three runs reported 29 errors. The logs showed 13 bad values, each observed by two verify passes, plus three read/write alarms whose post-write rescans came back clean. That distinction currently takes manual reconstruction. [Hardware follow-up](../../research/access_geometry/hardware_followup_2026-11-16.md)

Add a short evidence summary to `--show-results`, alongside the existing counts. Group repeat observations only when the records tie them to the same write, worker, address, and expected and observed values. Distinguish checks reproduced on a reread, checks the rescan could not reproduce, and checks run with rescan off. Keep test and seal evidence identifiable. A grouped count describes what TMR observed; the number of independent hardware faults remains unknown.

Start by comparing the saved JSON and matching logs from those runs. Where older or sampled records lack the identity or ordering needed for grouping, show the raw totals and say how much detail was available. If saving more evidence is necessary, capture it on the existing failure path. Show elapsed time to the first observed failure when [TODO 122's timestamps](122-timings-and-temperatures.md) are available.
