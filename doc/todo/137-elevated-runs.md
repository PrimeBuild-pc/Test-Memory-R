# TODO 137: elevated runs, a home of their own and a question before testing

Found 2026-11-24 in Astra's cross-review of the 0.1.3 batch. Run as administrator, TMR read and wrote its settings, results and logs in
folders other accounts can change: an install for all users makes its four folders writable by users, and a portable copy is often in
such a folder too. The owner chose the full change over narrower fixes to each file operation (2026-11-24). Astra built it; reviewed here.

## Decided (the owner, with Astra, 2026-11-24)

- **Administrator rights are for the large-page grant and removal only.** `--setup-large-pages`, `--remove-large-pages` and the
  session's own grant question keep their prompts and need no flag. When the admin home can't be used they still run, with no log file.
- **An elevated run has a home only administrators can change.** By default `%ProgramFiles%\TMR Admin`, which TMR makes with its own
  permissions (SYSTEM and Administrators full control, owner rights read-only, nothing inherited) and checks at every start; changed
  permissions stop it, and the message says to rename the folder. Or a `TMR_HOME`: an absolute path on a fixed local drive that only
  administrators can change, under folders nobody else can move or replace; TMR makes its four folders with its own permissions. A
  folder in the user's profile fails, as the account itself can change it. On the dev box the checkout also gives Modify to
  `CodexSandboxUsers` and three other accounts (checked 2026-11-24), so its elevated runs take `$env:ProgramFiles\TMR Dev`.
- **What differs there.** No settings file is taken from an older version's place; a damaged one is reported and left, not set aside
  as `.unreadable`. Window settings are one file for each account (`window-<SID>.json`); `tmr-cfg.json` is shared by its
  administrators. A config is found by name in the current folder and the home's `configs\`, not in `examples\`.
- **The question comes last**, after the large-page check, before anything is allocated: "Run memory tests as Administrator using
  <folder>? [y/N]" on the console (on stderr when stdout is piped), a dialog in the window. Only yes runs. `--allow-admin`, on the
  command line only (a config can't set it), skips it; a run with no console to answer from exits 2 without it. The window's Settings
  switch, "Allow administrator runs without asking", keeps the yes for the account in that home, as a file of its own in the protected
  `settings\`, written only by the switch. Calibrations ask too; `--startup-debug`, which tests nothing, doesn't.
- **The installer** gives the four folders users-modify only when it makes them in an install for all users (a per-user install's are
  plain, and an upgrade keeps a folder's permissions). The demo configs go to `examples\`, which only administrators can write. The
  installer moves no settings: the first normal run copies them in.
- **Tests** take a throwaway home per process, removed at exit: under `%TEMP%` normally; elevated, `C:\Windows\Temp\tmr-test-home-<GUID>`,
  since the elevated check refuses a folder in the profile and the tests are to run the elevated code as it runs. `release.yml`
  generates the demo configs in such a home on its elevated runner.
- **The hardware kit** (`hw-pass\`, the test-host skill's scripts) adds `--allow-admin` and fetches from the admin home.

## Commits

6ce46dc (the home, the question, the switch, the installer), 99628fc (the question with stdout piped), f85a3e5 (a protected
`TMR_HOME`, the setup without a home, the test homes), 4bb9e46 (every test's scratch files in its home), and the docs after them.
Alongside, from the same review: 5865311, 1f45f17, ec2bb7f, 92b7aaf and 325e7ea.

## Not verified

By hand: the window's question and switch, a save from a standard account, an installer upgrade over 0.1.2. The release workflow's
elevated step runs only on GitHub, at the next tag.
