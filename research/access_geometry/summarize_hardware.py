"""Validate and summarize the TODO 126 hardware queues.

Usage: python -B research/access_geometry/summarize_hardware.py CAMPAIGN_DIR
The external campaign directory contains <host>.txt queues and <host>/<run>/ results.
Incomplete or failed runs are reported and excluded from speed comparisons.
"""
import csv
import json
from pathlib import Path
import statistics
import sys

from summarize_ab import read_run


APP = Path(__file__).resolve().parents[2]
LABELS = ("base_first", "new_first", "new_last", "base_last")


def validate_plan(run, raw, args, grouped):
    config = raw["run_config"]
    memory = config["memory"]
    assert memory["allocated_gib"] == 8 and memory["per_thread_gib"] == 2, memory
    assert memory["page_mix"] == {
        "huge_1gib_gib": 0.0, "large_2mib_gib": 8.0, "regular_4kib_gib": 0.0
    }, memory
    assert config["threads"]["thread_count"] == 4 and config["threads"]["pinned"]
    assert config["calibration"] is None
    execution = config["execution"]
    width = int(next(a.split("=")[1] for a in args.split() if a.startswith("width=")))
    assert execution["simd_width_bits"] == width
    assert execution["pattern_seed"] == "0x000000000000007e"
    assert execution["error_mode"] == "Halt" and execution["rescan"]
    assert execution["channels"] == 2 and execution["cycles_requested"] == 1

    if grouped:
        source = "combined-reverse.json" if "config=126-reverse.json" in args else "combined.json"
    else:
        source = "local-regression-long.json"
    expected = json.loads((APP / "research/access_geometry/configs" / source).read_text())["test_sequence"]
    assert [t["id"] for t in config["tests"]] == [t["id"] for t in expected]
    for actual, wanted in zip(config["tests"], expected):
        assert actual["extent_bytes"] == 2 * 1024**3
        assert actual["chunk_bytes"] == 256 * 1024**2
        assert actual["chunks"] == 8 and actual["overlap_bytes"] == 0
        assert actual["cycle_limit"] == wanted["cycles"]
        if actual["id"] == "reference":
            assert actual["function"] == "Spd-DRAMFull-Read-256"
            expected_bytes = 8 * 1024**3 * 64
        else:
            assert actual["simd_width_bits"] == width
            for field in ("program", "verify_reps", "write_read_cycles"):
                assert actual[field] == wanted[field], (actual["id"], field)
            if grouped:
                assert actual["access"] == {
                    "period_lines": wanted["period_lines"], "group_lines": wanted["group_lines"]
                }, actual
            reads_writes = 7 if grouped and actual["id"].startswith("sequence") else 3
            expected_bytes = 8 * 1024**3 * wanted["cycles"] * wanted["write_read_cycles"] * reads_writes
        assert run["setup"]["bytes"][actual["id"]] == expected_bytes, actual["id"]


def compare(blocks):
    rows = {}
    if not blocks:
        return rows
    setup = blocks[0]["base_first"]["setup"]
    for block in blocks:
        assert all(run["setup"] == setup for run in block.values()), "Comparison settings or work changed"
    for test in setup["bytes"]:
        arms = {
            arm: [b[f"{arm}_{pos}"]["speeds"][test] for b in blocks for pos in ("first", "last")]
            for arm in ("base", "new")
        }
        deltas, normalized = [], []
        same_build = {arm: [] for arm in arms}
        for block in blocks:
            means, ratios = {}, {}
            for arm in arms:
                runs = [block[f"{arm}_{pos}"] for pos in ("first", "last")]
                means[arm] = statistics.mean(r["speeds"][test] for r in runs)
                ratios[arm] = statistics.mean(r["speeds"][test] / r["speeds"]["reference"] for r in runs)
                same_build[arm].append(100 * (runs[1]["speeds"][test] / runs[0]["speeds"][test] - 1))
            deltas.append(100 * (means["new"] / means["base"] - 1))
            normalized.append(100 * (ratios["new"] / ratios["base"] - 1))
        rows[test] = {
            "baseline_mib_s": arms["base"], "candidate_mib_s": arms["new"],
            "block_raw_pct": deltas, "block_normalized_pct": normalized,
            "same_build_last_vs_first_pct": same_build,
        }
    return rows


def ordered_blocks(runs, prefix):
    blocks = []
    for order in ("abba", "baab"):
        names = [f"{prefix}-{order}-{label}" for label in LABELS]
        if all(name in runs for name in names):
            block = {label: runs[name] for label, name in zip(LABELS, names)}
            chronological = sorted(LABELS, key=lambda label: block[label]["started_utc"])
            wanted = list(LABELS) if order == "abba" else ["new_first", "base_first", "base_last", "new_last"]
            assert chronological == wanted, (order, chronological)
            blocks.append(block)
    return blocks


def main():
    root = Path(sys.argv[1]).resolve()
    report = {}
    csv_rows = []
    for queue in sorted(root.glob("*.txt")):
        hostdir = root / queue.stem
        if not hostdir.is_dir():
            continue
        host = {"runs": {}, "failures": {}, "unavailable": {}, "comparison": {}, "grouped": {}}
        for line in queue.read_text().splitlines():
            if not line.strip() or line.startswith("#"):
                continue
            name, exe, args, _ = [s.strip() for s in line.split("|")]
            path = hostdir / name / f"{name}.txt"
            if not path.is_file():
                host["unavailable"][name] = "pending console capture"
                continue
            result_files = list(path.parent.glob("TMR_*.json"))
            if len(result_files) == 1:
                try:
                    raw = json.loads(result_files[0].read_text(encoding="utf-8-sig"))
                except json.JSONDecodeError:
                    host["unavailable"][name] = "result capture not complete"
                    continue
                stats = raw["overall_stats"]
                if stats["status"] != "passed":
                    host["failures"][name] = {
                        "result": str(result_files[0]), "build": raw["metadata"]["build"],
                        "overall": {k: stats.get(k) for k in (
                            "outcome", "status", "total_runtime_secs", "total_errors", "seal_errors",
                            "whea_total", "failing_bits", "seal_failing_bits",
                        )},
                        "steps": [
                            {k: t.get(k) for k in (
                                "id", "errors", "alarms", "not_reproduced", "seal_errors", "whea_total",
                                "failing_bits", "seal_failing_bits", "bytes_processed", "timed_ms",
                            )}
                            for cycle in raw["cycles"] for t in cycle["tests"]
                            if t["errors"] or t["seal_errors"] or t["whea_total"]
                        ],
                    }
            try:
                run = read_run(path)
                raw = json.loads(Path(run["result"]).read_text(encoding="utf-8-sig"))
                validate_plan(run, raw, args, "-group-" in name)
                commit = "57369c4" if exe.endswith("-base.exe") else "72ab3b8"
                assert run["build"]["commit"].startswith(commit), run["build"]
            except (ValueError, AssertionError, FileNotFoundError, json.JSONDecodeError) as error:
                host["unavailable"][name] = str(error)
                continue
            host["runs"][name] = run
            for test, speed in run["speeds"].items():
                csv_rows.append([queue.stem, name, run["build"]["commit"], run["started_utc"],
                                 test, speed, run["timed_ms"][test], run["setup"]["bytes"][test]])
            if "-group-" in name:
                host["grouped"][name] = run["speeds"]
        blocks = ordered_blocks(host["runs"], "126")
        host["comparison"] = compare(blocks)
        report[queue.stem] = host
        print(f"{queue.stem}: {len(host['runs'])} complete clean runs, {len(host['unavailable'])} unavailable; "
              f"{len(blocks)} complete comparison blocks")
        for test, values in host["comparison"].items():
            print(f"  {test:24} raw blocks {values['block_raw_pct']}, normalized {values['block_normalized_pct']}")
        for name, why in host["unavailable"].items():
            print(f"  {name}: {why[:400]}")
        reference_runs = {}
        for folder in sorted((root / "reference-only" / queue.stem).glob("126-ref-*")):
            console = folder / f"{folder.name}.txt"
            if not console.is_file():
                continue
            try:
                run = read_run(console)
            except (FileNotFoundError, json.JSONDecodeError):
                continue  # The driver may still be fetching this run's JSON.
            expected_commit = "57369c4" if "-base_" in folder.name else "72ab3b8"
            assert run["build"]["commit"].startswith(expected_commit)
            assert run["setup"]["bytes"] == {"reference": 512 * 1024**3}
            original_setup = next(iter(host["runs"].values()))["setup"]
            for field in ("memory", "threads", "calibration"):
                assert run["setup"][field] == original_setup[field], (folder.name, field)
            execution = run["setup"]["execution"]
            assert execution["seal"] is None and execution["seal_width_source"] is None
            # A benchmark-only plan has no seal. The original mixed plan has correctness steps.
            for field, value in original_setup["execution"].items():
                if field not in ("seal", "seal_width_source"):
                    assert execution[field] == value, (folder.name, field)
            reference_runs[folder.name] = run
        if reference_runs:
            reference_blocks = ordered_blocks(reference_runs, "126-ref")
            host["reference_only"] = {"runs": reference_runs, "comparison": compare(reference_blocks)}
            print(f"  reference-only: {len(reference_runs)} clean runs, {len(reference_blocks)} complete blocks")
            for values in host["reference_only"]["comparison"].values():
                print(f"  reference-only raw blocks: {values['block_raw_pct']}")
    (root / "summary.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    with (root / "summary.csv").open("w", newline="", encoding="utf-8") as out:
        writer = csv.writer(out)
        writer.writerow(["host", "run", "commit", "started_utc", "test", "mib_s", "timed_ms", "bytes"])
        writer.writerows(csv_rows)


if __name__ == "__main__":
    main()
