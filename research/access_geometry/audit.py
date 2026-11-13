"""Inspect every grouped product loop, including those above hot_loop_spills.py's 600-instruction cutoff."""

import collections
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import hot_loop_spills as spills


def main():
    path = spills.binary_asm(sys.argv[1:])
    kernels = spills.functions(path)
    rows = []
    for symbol, body in kernels.items():
        name = spills.short_name(symbol)
        match = re.search(r"group(2|4|8|16)(?!\d)", name)
        if not match:
            continue
        for i, loop in enumerate(spills.innermost_loops(body)):
            if any(x.startswith("ret") for x in loop) or not any(
                spills.OFF_STACK.search(x) and "(%rip)" not in x and not x.startswith(("lea", "prefetch")) for x in loop
            ):
                continue
            rows.append({
                "name": name, "symbol": symbol, "loop": i, "group": int(match[1]),
                "instructions": len(loop),
                "stack": [x for x in loop if spills.STACK.search(x) and not x.startswith("lea")],
                "calls": [x for x in loop if x.startswith("call")],
                "wrong_width": spills.off_width(name, loop),
            })
    if not rows:
        raise RuntimeError("No grouped loops found; inspect symbols before trusting this audit.")
    out = ROOT / "target/access-geometry/grouped-audit.json"
    out.write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
    for g in [2, 4, 8, 16]:
        group = [r for r in rows if r["group"] == g]
        totals = collections.Counter(
            loops=len(group), stack_loops=sum(bool(r["stack"]) for r in group),
            call_loops=sum(bool(r["calls"]) for r in group), width_loops=sum(r["wrong_width"] for r in group),
            over_600=sum(r["instructions"] > 600 for r in group),
        )
        print(f"G{g}: {dict(totals)}, max instructions {max((r['instructions'] for r in group), default=0)}")
    print(out)


if __name__ == "__main__":
    main()
