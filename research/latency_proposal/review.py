"""Read-only arithmetic for the review of TODO 119 at 8a7d8e3. No TMR execution or hardware access.

Run from any directory: python research/latency_proposal/review.py
Requires the private ../baselines archive; prints only selected measurement fields, not machine IDs.
"""

import json
import math
import re
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[3]
BASELINES = WORKSPACE / "baselines"
PERF = BASELINES / "2026-11-13"
HOSTS = ("petz007", "daddy-petz", "desktop-v6v7goc", "ec2amaz-6hbdp1e")


def text(path):
    return path.read_text(encoding="utf-8-sig")


def mib(value):
    number, unit = value.split()
    return float(number) * {"B": 1 / 1048576, "KiB": 1 / 1024, "MiB": 1, "GiB": 1024}[unit]


def perf_records():
    for host in HOSTS:
        for path in sorted((PERF / host).glob("*.json")):
            data = json.loads(text(path))
            if any(t["name"] == "Lat-L3-Read" for t in data["cycles"][0]["tests"]):
                yield host, path.name, data


def core_ranges():
    out = []
    for host, file, data in perf_records():
        for test in data["cycles"][0]["tests"]:
            if test["name"] not in ("Lat-L2-Read", "Lat-L3-Read", "Lat-DRAM-Read"):
                continue
            threads = test["threads"]
            t0 = next(t for t in threads if t["thread"] == 0)
            out.append({
                "host": host, "file": file, "test": test["name"],
                "merged_p50_ns": test["latency_p50_ns"],
                "thread_p50_range_ns": [
                    min(t["latency"]["p50_ns"] for t in threads),
                    max(t["latency"]["p50_ns"] for t in threads),
                ],
                "thread_0_p99_rank_worst_first": 1 + sum(
                    t["latency"]["p99_ns"] > t0["latency"]["p99_ns"] for t in threads
                ),
                "threads": len(threads),
            })
    return out


def blend_fits():
    # Fixed L2 plateau values and physical capacities; fit L3 only. These are empirical fits,
    # not measured hit counters. Use coarse one-thread probes from 2x through 8x L2.
    sources = (
        ("petz007", BASELINES / "2026-11-05-curve/petz007-curve-1.txt", 1, 2.7),
        ("daddy-petz", BASELINES / "2026-11-05-curve/daddy-petz-curve-1.txt", 0.5, 2.5),
        ("ec2amaz-6hbdp1e", PERF / "ec2amaz-6hbdp1e/116-metal-perf-1.txt", 2, 4.1),
    )
    out = []
    for host, path, capacity, l2_ns in sources:
        sweep = text(path).split("Sweep, one thread:", 1)[1].split("Sweep complete:", 1)[0]
        points = []
        for size, unit, ns in re.findall(r"\d+:\s+([\d.]+)\s+(KB|MB)\s+([\d.]+)ns", sweep):
            size = float(size) / (1024 if unit == "KB" else 1)
            if 2 * capacity <= size <= 8 * capacity:
                points.append((size, float(ns)))
        shares = [1 - capacity / size for size, _ in points]
        fitted = sum(h * (ns - (1 - h) * l2_ns) for h, (_, ns) in zip(shares, points)) / sum(h*h for h in shares)
        rmse = math.sqrt(sum(
            (ns - ((1 - h) * l2_ns + h * fitted)) ** 2 for h, (_, ns) in zip(shares, points)
        ) / len(points))
        out.append({
            "host": host, "source": str(path.relative_to(BASELINES)),
            "physical_l2_mib": capacity, "assumed_l2_ns": l2_ns,
            "points": points, "fitted_l3_ns": fitted, "rmse_ns": rmse,
        })
    return out


def clean_counts():
    out = []
    for host, file, data in perf_records():
        if host == "desktop-v6v7goc":
            continue
        cal = {t["tier"]: t for t in data["run_config"]["calibration"]["tiers"]}
        r1 = mib(cal["L3"]["optimal_size"])
        physical_l2 = mib(data["identity"]["l2_cache_per_core"])
        calibrated_l2 = mib(cal["L2"]["optimal_size"])
        k_run = {"petz007": 6, "daddy-petz": 8, "ec2amaz-6hbdp1e": 48}[host]
        block = next(t["extent_bytes"] for t in data["run_config"]["tests"] if t["test"] == "Lat-L3-Read") / 1048576
        # The saved JSON omits C. On AMD infer own+split/k from A and the observed rounded
        # half-reach extent (above its L2 floor). These are approximate, not exact new extents.
        # Xeon's console says C found no edge; l3_reach therefore uses R1/k.
        split = r1 if host == "ec2amaz-6hbdp1e" else (r1 - 2 * block) / (1 - 1 / k_run)
        own = r1 - split
        for l2_kind, capacity in (("physical", physical_l2), ("calibrated", calibrated_l2)):
            for target in (0.50, 0.60, 0.88):
                candidates = []
                for k in range(1, k_run + 1):
                    b = max(0.5 * (own + split / k), capacity + 0.0625)
                    h = 1 - capacity / b
                    if h >= target:
                        candidates.append((k, b, h))
                b1 = max(0.5 * r1, capacity + 0.0625)
                k, b, h = candidates[-1] if candidates else (1, b1, 1 - capacity / b1)
                out.append({
                    "host": host, "file": file, "l2_capacity": l2_kind, "target": target,
                    "k_per_physical_l3": k, "block_mib_approx": b,
                    "estimated_share": h, "target_met_in_model": bool(candidates),
                })
    return out


def write_probes():
    out = []
    for campaign in ("2026-11-05-write-latency", "2026-11-05-timings"):
        for path in sorted((BASELINES / campaign).rglob("*.txt")):
            if not re.fullmatch(r"probe[_-]v3_\d+\.txt", path.name):
                continue
            rows = {}
            for name, median, low, high in re.findall(
                r"^(R|W-nt\+sf|W-nt\+mf|W-fl|RAW-nt)\s+([\d.]+) ns\s+\[([\d.]+)\.\.([\d.]+)\]",
                text(path), re.MULTILINE,
            ):
                rows[name] = {"median": float(median), "low": float(low), "high": float(high)}
            out.append({"file": str(path.relative_to(BASELINES)), "ns": rows})
    return out


if __name__ == "__main__":
    print(json.dumps({
        "blend_fits": blend_fits(),
        "clean_counts_approx": clean_counts(),
        "per_core": core_ranges(),
        "write_probes_v3": write_probes(),
    }, indent=2))
