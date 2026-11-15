"""Compare G=1/2/4/8/16 when P=G gives identical forward line order.

Usage: python -B research/access_geometry/summarize_same_order.py CAMPAIGN_DIR/same-order
This measures complete kernel paths, including their prefetch/control differences;
it does not isolate stack accesses and does not represent a P=512 workload.
"""
import json
from pathlib import Path
import re
import statistics
import sys

from summarize_ab import read_run


def main():
    root = Path(sys.argv[1]).resolve()
    summary = {}
    for hostdir in sorted(p for p in root.iterdir() if p.is_dir()):
        runs = []
        for folder in sorted(p for p in hostdir.iterdir() if p.is_dir()):
            console = folder / f"{folder.name}.txt"
            if not console.is_file():
                continue
            run = read_run(console)
            raw = json.loads(Path(run["result"]).read_text(encoding="utf-8-sig"))
            config = raw["run_config"]
            assert run["build"]["commit"].startswith("72ab3b8")
            assert config["memory"]["allocated_gib"] == 8
            assert config["memory"]["page_mix"] == {
                "huge_1gib_gib": 0.0, "large_2mib_gib": 8.0, "regular_4kib_gib": 0.0
            }
            assert config["threads"]["thread_count"] == 4
            assert config["execution"]["pattern_seed"] == "0x000000000000007e"
            assert config["execution"]["error_mode"] == "Halt"
            assert config["calibration"] is None
            expected_ids = {f"{kind}-p{g}-g{g}" for kind in ("simple", "sequence") for g in (1, 2, 4, 8, 16)}
            assert set(run["speeds"]) == expected_ids
            for test in config["tests"]:
                group = int(re.fullmatch(r"(?:simple|sequence)-p(\d+)-g\1", test["id"]).group(1))
                assert test["access"] == {"period_lines": group, "group_lines": group}
                assert test["extent_bytes"] == 2 * 1024**3 and test["chunk_bytes"] == 256 * 1024**2
                assert test["chunks"] == 8 and test["overlap_bytes"] == 0
                assert test["cycle_limit"] == 1 and test["write_read_cycles"] == 8 and test["verify_reps"] == 2
                assert test["simd_width_bits"] == config["execution"]["simd_width_bits"]
                assert test["program"] == ["ident", "switch", "random", "switch"]
                assert run["setup"]["bytes"][test["id"]] == 192 * 1024**3
                if test["id"].startswith("sequence"):
                    assert test["sequence"] == "W 2R (order stride)"
            runs.append(run)
        if not runs:
            continue
        if len(runs) == 2:
            assert [t["id"] for t in runs[0]["setup"]["tests"]] == list(
                reversed([t["id"] for t in runs[1]["setup"]["tests"]])
            )
        first = runs[0]["setup"]
        for run in runs[1:]:
            for field in ("memory", "threads", "execution", "calibration", "bytes"):
                assert run["setup"][field] == first[field], field
        comparisons = {}
        print(f"{hostdir.name}: {len(runs)} complete clean runs, {first['execution']['simd_width_bits']} bits")
        for kind in ("simple", "sequence"):
            for group in (1, 2, 4, 8, 16):
                name = f"{kind}-p{group}-g{group}"
                speeds = [r["speeds"][name] for r in runs]
                delta = [100 * (r["speeds"][name] / r["speeds"][f"{kind}-p1-g1"] - 1) for r in runs]
                comparisons[name] = {"mib_s": speeds, "vs_g1_pct": delta}
                print(f"  {name:24} {statistics.mean(speeds):9,.0f} MiB/s, "
                      f"{statistics.mean(delta):+6.2f}% vs G1, runs {', '.join(f'{d:+.2f}%' for d in delta)}")
        summary[hostdir.name] = {"runs": runs, "comparisons": comparisons}
    (root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
