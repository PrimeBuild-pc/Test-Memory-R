# TODO 130: a command and a sound when a run finds an error or ends

Raised by the owner, 2026-11-21: call a script or command when a run finishes or stops, so it can, say, send a message to your
phone while you're away from the desk.

## What it would be

- A setting naming a command that TMR starts once a run has ended and its results file and log are saved: passed, failed,
  halted (`errors=halt`), interrupted (Ctrl+C, the window's Stop) or stopped before testing.
- TMR hands it the outcome in environment variables, so the command needs to parse nothing: `TMR_OUTCOME` (the results file's
  `outcome`: `passed`, `failed`, `not_run`, `interrupted`), `TMR_EXIT_CODE` (0-3, as the exit code), `TMR_ERRORS`,
  `TMR_RUNTIME_SECS`, `TMR_RESULTS` and `TMR_LOG` (the two files' paths).
- TMR then exits with its own code, whatever the command did. The log records the command's start and its exit code, or its
  timeout.
- Examples for the docs: ntfy (`curl -d "TMR %TMR_OUTCOME%, %TMR_ERRORS% errors" https://ntfy.sh/<topic>`), Pushover, a Telegram
  bot, a PowerShell script.

## Sounds (the owner, 2026-11-22)

- **On a run's first error**: test, seal or WHEA, the moment the run's count first leaves 0. One sound a run, not one an error:
  a fault can count thousands a second. Optionally again when a step fails after a clean stretch.
- **At a run's end**: one sound for passed, another for failed, halted or interrupted.
- **Which sound**: a Windows system sound by default (`PlaySoundW` with an alias: `SystemHand` for an error, `SystemNotification`
  or `SystemAsterisk` for the end; `MessageBeep` where no sound plays), or a `.wav` the user names. No sound files shipped.
- **Where it is seen**: the monitor's timer, which already reads the error counts for the ticker and the window, every 250 ms.
  Never the test loops: they stay free of anything but the test (`CLAUDE.md`, Priorities).
- **The same event for the command**: the first error starts the command too (`TMR_EVENT=first_error`, the end's
  `TMR_EVENT=end`), so a run that fails an hour in messages the phone then, not when it ends hours later.
- **Settings**: `sound=off|error|end|both` (or two settings), and the window's Settings; per user, as with the command, never
  from a test config. Open: on or off by default. A sound by default helps a newcomer away from the screen; off by default
  surprises no one in an office or at night.

## What exists today

From a console a wrapper already does it, on the exit code (0 passed, 1 failed or found errors, 2 stopped before testing, 3
interrupted; `dist/docs/guide.md`, "Reading a result"):

```
tmr plan=stability
if ($LASTEXITCODE -ne 0) { curl.exe -d "TMR exit $LASTEXITCODE" https://ntfy.sh/<topic> }
```

The setting matters for the window, which has no wrapper, and gives both one way to do it.

## Open questions

- **Where it can be set.** Not from a test config (`config=`): TMR loads configs people share on forums, and a config that runs
  a command is a way to run code on someone else's PC, often elevated. So the command line (`on-end=`) and the user's own
  settings (`tmr-cfg.json`, the window's Settings), never a TM5 `.cfg` or a test `.json`.
- **Which ends.** Every end, with the outcome in the variables so the script decides, or a filter (`on-end-when=failed`)?
  Every end is simpler and loses nothing.
- **A crash or a frozen PC runs nothing.** A release build aborts on a panic, and an unstable overclock can freeze or restart
  the PC: the case where a phone alert matters most. Two ways to cover it: the command on the crash path too, where the
  process lives long enough (the fatal box's), which covers a panic but not a freeze; and a heartbeat, a command every N
  minutes during the run, pinged to a dead man's switch (healthchecks.io and the like alert when the pings stop). Only the
  heartbeat catches a freeze or a reboot.
- **How it runs.** Started directly (CreateProcess) with its arguments, not through a shell unless the user names one; with a
  timeout (say 60 s) so a stuck command can't hold TMR open; with TMR's own token, nothing more.
- **Privacy.** The README says TMR makes no network connections. TMR still makes none; a command the user sets can. The Privacy
  section would say so in a line.

## Next

The owner's calls on the open questions, then the build: the settings (the command, the sounds) and the command's variables,
the first-error event from the monitor, the log line, the window's fields, and the docs (the guide's "Reading a result", the
README's Privacy).
