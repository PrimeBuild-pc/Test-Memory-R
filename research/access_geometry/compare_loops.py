"""Compare existing memory-loop instructions and constants, separately from changed chunk setup/dispatch."""

import collections
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import asm_fingerprint as fingerprints
import hot_loop_spills as spills


def loops(path):
    data = fingerprints.label_data(Path(path).read_text(encoding="utf-8").splitlines())
    result = collections.defaultdict(list)
    for symbol, body in spills.functions(path).items():
        name = spills.short_name(symbol)
        if not (
            "simple_native_" in name or "sequence_read_" in name or "sequence_write_" in name or "sequence_rw_" in name
            or name.startswith("bandwidth_tests::")
        ) or "group" in name:
            continue
        for loop in spills.innermost_loops(body):
            if any(x.startswith(("call", "ret")) for x in loop) or not any(
                spills.OFF_STACK.search(x) and "(%rip)" not in x and not x.startswith(("lea", "prefetch")) for x in loop
            ):
                continue
            normalized = fingerprints.normalise(loop, data)
            result[name].append(fingerprints.digest(normalized))
    return {name: sorted(hashes) for name, hashes in result.items()}


def main():
    before, after = map(loops, sys.argv[1:3])
    result = {kind: [] for kind in ("same", "changed", "missing", "new")}
    for name in sorted(before.keys() | after.keys()):
        kind = "new" if name not in before else "missing" if name not in after else "same" if before[name] == after[name] else "changed"
        result[kind].append(name)
    out = ROOT / "target/access-geometry/existing-loop-diff.json"
    out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for kind, names in result.items():
        print(f"{kind}: {len(names)}")
        if kind != "same":
            for name in names:
                print(f"  {name}")


if __name__ == "__main__":
    main()
