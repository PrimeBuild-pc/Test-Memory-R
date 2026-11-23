# TODO 135: forum feedback still to decide (overclock.net, 2026-11-20 to 23)

The thread "TMR (Test Memory R): an open-source TM5-style memory tester, looking for testers" drew feedback from three users
(igor_kavinski, Veii, Bam_Bam). Each suggestion was checked against the code on 2026-11-23, every claim re-checked by a second agent.
This file keeps the ones still waiting on the owner's call; what was built or decided is listed at the end.

## Still to decide

| # | Suggestion (who) | Today | Proposal | Size | The owner's call |
|---|---|---|---|---|---|
| 1 | A hash of the loaded config (Veii: "basic hashing as first validator") | Not recorded: the results keep the `config=` path and the config's declared name only | SHA-256 of the bytes `load_config` reads, with the size, in the results (`RunConfigSnapshot`), the setup line and the log; `--compare-results` flags two runs whose names match but hashes differ. Optional: a table of the common community `.cfg`s' published hashes ("matches the published 1usmus_v3") | Small | The `sha2` crate (a new dependency: Cargo.lock, `cargo audit`) or CNG `BCryptHash` through the `windows` crate (one feature). Whether to keep a known-config table, and who keeps it current |
| 2 | Stop after about 7-8 errors by default (Veii: "2 is too little, 10 has a high likelihood of a BSOD") | `errors=log` by default; `errors=halt:N` stops at N or more (the monitor checks twice a second). An error is one bad 64-bit word, so one failing check can add hundreds | An `errors` default on the built-in plans, or a session default; or count failing checks rather than words for N (TODO 128) | Small | Which plans, if any (it changes "run the whole plan", and a halted short run's results are partial); what N counts |
| 3 | A memory slider (Veii, RunMemtestPro's mouse-drag; the owner on the thread: "wouldn't be too hard") | The window has only the free-text parameter box; Reactor 0.100.0 has a `Slider` | A row under the command bar, "Memory tested [slider] 90% of available, about 28.4 of 31.6 GiB", writing `memory=10%-from-available` into the box (as `config=` is set today); another `memory=` form greys it ("set in the box") | Medium | What it measures: the share of available memory to test (recommended), GiB to test, or GiB to leave free |
| 4 | A denser, narrower window (Veii: "a bit wide for screenshotting") | Resizable, remembers its size and place; opens at 860x960 DIPs, minimum 720x680. The header (title plus the system panel) sets the default, the Results table the minimum | Open at about 760x820, minimum about 600; below about 760 the system panel moves under the title; below about 640 drop Seal and WHEA from the Results table; tighter spacing. Needs a look at 100%, 125% and 150% scaling | Small to medium | The narrowest width; what moves or hides; one denser look or a setting |
| 5 | Select and copy several lines in the Log tab (Bam_Bam) | One TextBlock a line, so a selection can't cross lines; the three-dot menu's "Open the log" is the workaround | Now: "Copy the log" in the three-dot menu (the clipboard through the `windows` crate). Later: one selectable text block (losing the red WARN/ERROR lines), or a read-only TextBox once Reactor has `is_read_only` (TODO 111) | Small | Whether the menu item is enough until Reactor's next release |
| 6 | Keep a config's test size and add testers, opt-in (Veii: config-makers' intent first) | The import matches TM5 run as admin with AWE, as the shipped configs (Capable=0x0) run: one tester a CPU, each walking its whole share a window at a time. TM5's other mode (Capable bit 0x2, or no admin rights) runs about available / (window - window/8) testers, each capped at the window | An opt-in `testers=window` (or the `.cfg`'s own Capable=0x2) splitting each thread's span into window-sized testers: TM5's AWE-off block sizes, not more coverage. Smaller fidelity fixes either way: an out-of-range value reset to TM5's default as TM5 does (not clamped), code 0 floored to the Lock Granularity as TM5 does, a missing window taken as 1024, a warning when Cores exceeds the CPUs | Medium (the fixes small) | Whether to follow the `.cfg`'s Capable bits; whether the opt-in mode is worth building, since it changes block sizes, not coverage |
| 7 | Desktop screenshots on a timer during a run (Veii, as DangWang does) | None | An opt-in engine thread beside the monitor: a GDI capture of the virtual desktop every N minutes into the home's `screenshots\`, written whole, a few kept, never on a test thread | Medium | Whether to build it or let TODO 130's heartbeat run an outside tool; the interval, how many to keep, PNG or JPEG, one image or one a monitor, a shot at the first error |
| 8 | A per-user key so only the owner sees their serials in shared data (igor_kavinski) | Serials, the UUID and both fingerprints are left out by default (742a707); `--sanitize` removes what a file still has | HMAC-SHA256 pseudonyms under a per-install key in `tmr-cfg.json`, so the owner can match their own DIMMs across shared files | Medium | Whether at all, given the default; stable pseudonyms (match across files, but a forum reader can link one person's posts) or one per file |
| 9 | Read temperatures and timings natively rather than through HWiNFO, ZenTimings or CPU-Z (Veii) | Nothing yet; TODO 122 plans ZenTimings and LibreHardwareMonitor | Feeds TODO 122's decision: temperatures look doable natively; timings need a driver for the memory controller's registers, which ZenTimings has | (TODO 122) | TODO 122's approach |
| 10 | A misleading compare line (found while checking, not raised on the forum) | `--compare-results` says "(virtualized) · instance retype is the likely cause" on Hyper-V/VBS desktops too, where it isn't | Say it only on a cloud VM (the hypervisor's vendor), not under Hyper-V on a desktop | Small | None: a fix |

## Built or decided (2026-11-20 to 23)

- Serials left out by default, `--serials` keeps them (742a707); `tmr --sanitize` for a results file and its log, `tmr --sanitize-single`
  for one (1b5d450, a14c600, ebe6bd9, 3789afa): igor_kavinski.
- The reporter's uneven allocation: the import rounds down, a locked 4 KiB tail, or the run stops (TODO 131): Bam_Bam.
- LPDDR5X named in the docs and `beat=` (TODO 132): Veii.
- INFO in the log: by design; the duplicates are gone (5b460a5): Bam_Bam.
- The window remembers its position (53e5b77): Bam_Bam.
- `errors=halt:N` reaches a run from the command line and the window (f716809); it had silently become `log`: Veii's point about stopping.
- Crash safety: `tmr-cfg.json` and `window.json` written whole in `settings\` with a `.bak` (9cd30fa, 697e4dd, dd2b0a7, 7bda8ab); a
  results file from the end of setup (761a799); `.cfg` damage warned of, read as TM5 reads it (7896b1e, 7e0ac10): Veii.
- The large-page check reads as success when it succeeds (644fd88, e15a303): Bam_Bam's "ASSIGNED but DISABLED".
- DIMMs show their configured speed and a slot (5c4bdfa, 6f33633): from Bam_Bam's log.
- Not taken (the owner): forcing the log to disk (the risk an overclocker runs); more for comparing runs with different allocations (a
  clean boot gives the same memory; the allocation fixes are enough).
- No action: bundling the runtimes (winget installs them); the theme (the colour system exists); a second maintainer (not code); the
  rotation of threads over cores is TODO 133, and Veii attached TM5 0.12.3 to post #11 of the thread.
