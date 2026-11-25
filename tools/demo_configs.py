"""The two demo configs the installer ships, made by `tmr.exe --create-demo-configs` into a throwaway home and copied into a folder.

release.yml stages them with it, and Rust-nightly-build.yml runs it on every push, so a tag finds that step working. Run as
administrator, as GitHub's runners are, TMR takes only a TMR_HOME that only administrators can change (src/home/admin.rs), so the home
is a new folder in Program Files, which TMR makes with its own permissions; otherwise one in the temp folder. Removed after, made or not.

    python tools/demo_configs.py <tmr.exe> <folder>
"""

import argparse
import ctypes
import os
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

DEMOS = ("demo_comprehensive_test.json", "demo_tm5_compatible.json")


def elevated():
    """Whether this process runs as administrator, as TMR reads it: an elevated token (or UAC off, the built-in Administrator)"""
    return bool(ctypes.windll.shell32.IsUserAnAdmin())


def throwaway_home(admin):
    """A new folder's path, not made: TMR makes it, with its own permissions when run as administrator"""
    parent = Path(os.environ["ProgramFiles"]) if admin else Path(tempfile.gettempdir())
    return parent / f"TMR-demo-configs-{uuid.uuid4()}"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("exe", type=Path, help="the tmr.exe to make them")
    parser.add_argument("folder", type=Path, help="where the two configs go")
    args = parser.parse_args()
    home = throwaway_home(elevated())
    try:
        made = subprocess.run([str(args.exe), "--cli", "--create-demo-configs"], env=dict(os.environ, TMR_HOME=str(home)))
        if made.returncode != 0:
            print(f"tmr.exe --create-demo-configs failed (exit {made.returncode}), TMR_HOME={home}", file=sys.stderr)
            return 1
        args.folder.mkdir(parents=True, exist_ok=True)
        for name in DEMOS:
            shutil.copy2(home / "configs" / name, args.folder / name)
            print(f"{args.folder / name}")
    finally:
        shutil.rmtree(home, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
