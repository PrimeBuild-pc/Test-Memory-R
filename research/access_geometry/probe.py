"""Compile the isolated grouping alternatives and record code shape, without executing a memory test."""

import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from hot_loop_spills import classify, innermost_loops


def main():
    out = ROOT / "target/access-geometry"
    out.mkdir(parents=True, exist_ok=True)
    asm = out / "group-probe.s"
    command = ["rustc", "+nightly", "-O", "-C", "target-cpu=x86-64-v3", "--edition", "2024",
               "--crate-type", "lib", "--emit", "asm", str(Path(__file__).with_name("group_probe.rs")), "-o", str(asm)]
    subprocess.run(command, check=True, cwd=ROOT)
    results = {
        "compiler": subprocess.check_output(["rustc", "+nightly", "-Vv"], text=True),
        "scope": "Isolated ident verify, AVX2; no execution, timings, or post-LTO product claims.",
        "functions": {},
    }
    for match in re.finditer(r"^(group\d+|expanded16|runtime_group):\n(.*?)(?=\.seh_endproc)", asm.read_text(), re.M | re.S):
        name, body = match.groups()
        loops = []
        for loop in innermost_loops(body.splitlines()):
            if not any("vpxor" in line for line in loop):
                continue
            found = classify(loop)
            loops.append({
                "instructions": len(loop),
                "stack_accesses": len(found[1]) if found else None,
                "review_flags": found[2] if found else ["not classified"],
            })
        count = sum(line.startswith("\t") and not line.strip().startswith((".", "#")) for line in body.splitlines())
        results["functions"][name] = {"function_instructions": count, "innermost_memory_loops": loops}
        print(f"{name:14} {count:4} instructions; loops {loops}")
    if set(results["functions"]) != {"group2", "group4", "group8", "group16", "expanded16", "runtime_group"}:
        raise RuntimeError("Missing an exported probe function; inspect the compiler output.")
    (out / "probe-results.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
