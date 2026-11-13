"""Summarize local bench_ab.py ABBA blocks, including completion and setup checks.

Usage: python -B research/access_geometry/summarize_ab.py OUTDIR
Labels must be base_first, new_first, new_last, base_last. Incomplete blocks are
reported but excluded. A block, not an individual step or thread, is the repeat.
"""
import csv
import json
import pathlib
import re
import statistics
import sys


LABELS = ("base_first", "new_first", "new_last", "base_last")
APP = pathlib.Path(__file__).resolve().parents[2]


def read_run(path):
    match = re.search(r"Results saved: (.+\.json)", path.read_text(encoding="utf-8", errors="replace"))
    if not match:
        raise ValueError(f"No results in {path}")
    result_path = pathlib.Path(match[1].strip())
    if not result_path.is_absolute():
        result_path = APP / result_path
    result = json.loads(result_path.read_text(encoding="utf-8"))
    config = result["run_config"]
    overall = result["overall_stats"]
    assert overall["outcome"] == "completed" and overall["status"] == "passed", (path, overall)
    assert not any(overall[k] for k in ("total_errors", "seal_errors", "whea_total")), path
    assert len(result["cycles"]) == 1, path
    tests = result["cycles"][0]["tests"]
    expected = {t["id"]: t["cycle_limit"] for t in config["tests"]}
    assert {t["id"] for t in tests} == set(expected), path
    for test in tests:
        assert test["stopped_by"] == "cycles", (path, test["id"])
        assert test["cycles_completed_min"] == test["cycles_completed_max"] == expected[test["id"]], (path, test["id"])
        assert not any(test[k] for k in ("errors", "seal_errors", "whea_total")), (path, test["id"])

    memory = config["memory"]
    setup = {
        "memory": {k: memory[k] for k in ("spec", "allocated_gib", "per_thread_gib", "page_mix", "commit_method")},
        "threads": config["threads"],
        "execution": config["execution"],
        "calibration": config["calibration"],
        "tests": [
            {k: t.get(k) for k in (
                "id", "function", "extent_bytes", "chunk_bytes", "chunks", "overlap_bytes", "cycle_limit",
                "verify_reps", "test_reps", "write_read_cycles", "program", "pattern_mode", "parameter",
                "sequence", "simd_width_bits", "flush_before_verify", "skip_init",
            )}
            for t in config["tests"]
        ],
        "bytes": {t["id"]: t["bytes_processed"] for t in tests},
    }
    return {
        "console": str(path.resolve()),
        "result": str(result_path),
        "build": result["metadata"]["build"],
        "started_utc": result["metadata"]["start_time_utc"],
        "runtime_secs": overall["total_runtime_secs"],
        "setup": setup,
        "speeds": {t["id"]: t["throughput_mib_s"] for t in tests},
        "timed_ms": {t["id"]: t["timed_ms"] for t in tests},
    }


def main():
    outdir = pathlib.Path(sys.argv[1])
    numbers = sorted({int(p.stem.rsplit("_", 1)[1]) for p in outdir.glob("base_first_*.txt")})
    blocks = []
    for number in numbers:
        paths = {label: outdir / f"{label}_{number}.txt" for label in LABELS}
        if not all(p.exists() for p in paths.values()):
            print(f"Block {number} incomplete; excluded")
            continue
        blocks.append({label: read_run(path) for label, path in paths.items()})
    if not blocks:
        return
    setup = blocks[0]["base_first"]["setup"]
    for block in blocks:
        for label, run in block.items():
            assert run["setup"] == setup, f"Setup/work changed: {run['console']}"
            expected_commit = "57369c4" if label.startswith("base") else "72ab3b8"
            assert run["build"]["commit"].startswith(expected_commit), run["build"]

    rows = {}
    print(f"{len(blocks)} complete ABBA blocks; {2 * len(blocks)} runs per build; identical setup/work; all passed, zero errors")
    print(f"{'Test':24} {'Baseline mean (range)':>29} {'Candidate mean (range)':>29} {'Raw %':>9} {'Ref %':>9}")
    for test in blocks[0]["base_first"]["speeds"]:
        values = {}
        for arm in ("base", "new"):
            values[arm] = [block[f"{arm}_{pos}"]["speeds"][test] for block in blocks for pos in ("first", "last")]
        raw = []
        relative = []
        same_build = {"base": [], "new": []}
        for block in blocks:
            means = {}
            normalized = {}
            for arm in values:
                runs = [block[f"{arm}_{pos}"] for pos in ("first", "last")]
                means[arm] = statistics.mean(r["speeds"][test] for r in runs)
                normalized[arm] = statistics.mean(r["speeds"][test] / r["speeds"]["reference"] for r in runs)
                same_build[arm].append(100 * (runs[1]["speeds"][test] / runs[0]["speeds"][test] - 1))
            raw.append(100 * (means["new"] / means["base"] - 1))
            relative.append(100 * (normalized["new"] / normalized["base"] - 1))
        rows[test] = {
            "baseline_mib_s": values["base"],
            "candidate_mib_s": values["new"],
            "block_raw_pct": raw,
            "block_reference_normalized_pct": relative,
            "same_build_last_vs_first_pct": same_build,
        }
        columns = []
        for arm in values:
            v = values[arm]
            columns.append(f"{statistics.mean(v):,.0f} ({min(v):,.0f}-{max(v):,.0f})")
        print(f"{test:24} {columns[0]:>29} {columns[1]:>29} {statistics.mean(raw):+8.2f} {statistics.mean(relative):+8.2f}")
        print("  blocks raw:", ", ".join(f"{v:+.2f}%" for v in raw))
        print("  same binary last/first:", "; ".join(f"{arm}: " + ", ".join(f"{v:+.2f}%" for v in deltas)
                                                    for arm, deltas in same_build.items()))
    summary = {"blocks": blocks, "setup": setup, "comparisons": rows}
    (outdir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    with (outdir / "summary.csv").open("w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        writer.writerow(["campaign", "block", "arm", "commit", "started_utc", "runtime_secs", *rows])
        for block in blocks:
            number = int(pathlib.Path(block["base_first"]["console"]).stem.rsplit("_", 1)[1])
            for label, run in block.items():
                writer.writerow([outdir.name, number, label, run["build"]["commit"], run["started_utc"],
                                 run["runtime_secs"], *(run["speeds"][test] for test in rows)])


if __name__ == "__main__":
    main()
