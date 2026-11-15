"""Summarize TODO 126 SimpleTest/Sequence exposure and errors, including repeated IDs.

Usage: python -B research/access_geometry/summarize_screen.py RESULTS.json [CONFIG.json]
Counts are TMR's reported errors, not independent physical fault events. Throughput
and error rates across different geometries do not establish detection superiority.
"""
import collections
import json
from pathlib import Path
import sys


def summarize(path, source_path=None):
    data = json.loads(path.read_text(encoding="utf-8-sig"))
    assert not data["overall_stats"]["in_progress"], "Wait for the final saved result"
    config = data["run_config"]
    if source_path:
        source = json.loads(source_path.read_text(encoding="utf-8-sig"))
        definitions = {test["id"]: test for test in source["test_sequence"]}
        order = source.get("cycle_order") or list(definitions)
        assert [test["id"] for test in config["tests"]] == order
        for test in config["tests"]:
            wanted = definitions[test["id"]]
            assert test["access"] == {
                "period_lines": wanted["period_lines"], "group_lines": wanted["group_lines"]
            }, test["id"]
            for field in ("program", "verify_reps", "write_read_cycles"):
                assert test[field] == wanted[field], (test["id"], field)
            assert test["cycle_limit"] == wanted["cycles"], test["id"]
        for cycle in data["cycles"]:
            actual_order = [test["id"] for test in cycle["tests"]]
            assert actual_order == order[:len(actual_order)]  # A halted cycle can be incomplete.
    plan = {test["id"]: test for test in config["tests"]}
    rows = {}
    for cycle in data["cycles"]:
        for test in cycle["tests"]:
            row = rows.setdefault(test["id"], {
                "visits": 0, "complete_visits": 0, "bytes": 0, "timed_ms": 0,
                "errors": 0, "alarms": 0, "not_reproduced": 0, "seal_errors": 0,
                "test_bits": collections.Counter(), "seal_bits": collections.Counter(),
            })
            row["visits"] += 1
            planned = plan[test["id"]]
            wanted = planned["cycle_limit"]
            sequence = planned.get("sequence", "").split(" (")[0]
            if sequence:
                passes = {"W 2R": 3, "W 2R RW 2R": 7}[sequence]
            else:
                passes = 1 + planned["verify_reps"]
            expected_bytes = (planned["extent_bytes"] * config["threads"]["thread_count"]
                              * planned["write_read_cycles"] * passes * wanted)
            row["complete_visits"] += int(
                test["stopped_by"] == "cycles"
                and test["cycles_completed_min"] == test["cycles_completed_max"] == wanted
                and test["bytes_processed"] == expected_bytes
            )
            row["bytes"] += test["bytes_processed"]
            row["timed_ms"] += test["timed_ms"]
            for key in ("errors", "alarms", "not_reproduced", "seal_errors"):
                row[key] += test.get(key, 0)
            for source, target in (("failing_bits", "test_bits"), ("seal_failing_bits", "seal_bits")):
                for bit in test.get(source, []):
                    row[target][bit["bit"]] += bit["checks"]
    for row in rows.values():
        row["gib"] = row["bytes"] / 1024**3
        row["mib_s"] = row["bytes"] / 1024**2 / (row["timed_ms"] / 1000) if row["timed_ms"] else None
        for key in ("test_bits", "seal_bits"):
            row[key] = dict(row[key])
    return {
        "result": str(path.resolve()), "build": data["metadata"]["build"],
        "memory": config["memory"], "execution": config["execution"],
        "outcome": data["overall_stats"]["outcome"], "status": data["overall_stats"]["status"],
        "runtime_secs": data["overall_stats"]["total_runtime_secs"],
        "overall_test_errors": sum(row["errors"] for row in rows.values()),
        "overall_seal_errors": data["overall_stats"]["seal_errors"],
        "whea_total": data["overall_stats"]["whea_total"], "rows": rows,
    }


def main():
    path = Path(sys.argv[1])
    result = summarize(path, Path(sys.argv[2]) if len(sys.argv) > 2 else None)
    print(f"{result['outcome']}, {result['status']}, {result['runtime_secs']} s; "
          f"{result['overall_test_errors']} test / {result['overall_seal_errors']} seal / "
          f"{result['whea_total']} WHEA errors")
    for name, row in result["rows"].items():
        print(f"{name:26} {row['complete_visits']:3}/{row['visits']:<3} completed, "
              f"{row['gib']:10,.2f} GiB, {row['errors']:5} test / {row['seal_errors']:5} seal, "
              f"test bits {row['test_bits']}, seal bits {row['seal_bits']}")
    (path.parent / "exposure.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
