"""Inspect grouped memory and control ranges, including those above the standard tool's length cutoff.

Like hot_loop_spills.py, this finds linear backward-branch ranges, not dynamic execution counts.
Keep memory-body and control-only results separate, but never omit the latter from the report.
"""

import argparse
import collections
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import hot_loop_spills as spills


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("asm")
    parser.add_argument("--output", type=Path, default=ROOT / "target/access-geometry/grouped-audit.json")
    args = parser.parse_args()
    path = spills.binary_asm([args.asm])
    kernels = spills.functions(path)
    standard_limit = spills.MAX_INSTRUCTIONS
    # The expanded experiment exceeded the standard tool's cutoff. Classify it too, and mark
    # which rows the standard tool includes so the two totals can still be reconciled.
    spills.MAX_INSTRUCTIONS = sys.maxsize
    rows = []
    for symbol, body in kernels.items():
        name = spills.short_name(symbol)
        match = re.search(r"group(2|4|8|16|ed)(?!\d)", name)
        if not match:
            continue
        for i, loop in enumerate(spills.innermost_loops(body)):
            if any(x.startswith("ret") for x in loop):
                continue
            memory = any(
                spills.OFF_STACK.search(x) and "(%rip)" not in x and not x.startswith(("lea", "prefetch")) for x in loop
            )
            classification = spills.classify(loop)
            if not memory and classification is None:
                continue
            rows.append({
                "name": name, "symbol": symbol, "loop": i, "group": int(match[1]) if match[1] != "ed" else "shared",
                "scope": "memory" if memory else "control",
                "standard_tool_eligible": classification is not None and len(loop) <= standard_limit,
                "instructions": len(loop),
                "stack": [x for x in loop if spills.STACK.search(x) and not x.startswith("lea")],
                "stack_kind": (classification or ("unclassified",))[0],
                "calls": [x for x in loop if x.startswith("call")],
                "wrong_width": spills.off_width(name, loop),
            })
    spills.MAX_INSTRUCTIONS = standard_limit
    if not rows:
        raise RuntimeError("No grouped loops found; inspect symbols before trusting this audit.")
    out = args.output
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
    for g in [2, 4, 8, 16, "shared"]:
        for scope in ("memory", "control"):
            group = [r for r in rows if r["group"] == g and r["scope"] == scope]
            if not group:
                continue
            totals = collections.Counter(
                loops=len(group), stack_loops=sum(bool(r["stack"]) for r in group),
                call_loops=sum(bool(r["calls"]) for r in group), width_loops=sum(r["wrong_width"] for r in group),
                over_600=sum(r["instructions"] > 600 for r in group),
            )
            print(f"G{g} {scope}: {dict(totals)}, max instructions {max(r['instructions'] for r in group)}")
            print(f"  Stack classifications: {dict(collections.Counter(r['stack_kind'] for r in group))}")
    standard = collections.Counter(r["stack_kind"] for r in rows if r["standard_tool_eligible"])
    print(f"Grouped rows also counted by hot_loop_spills.py: {dict(standard)}")
    print(out)


if __name__ == "__main__":
    main()
