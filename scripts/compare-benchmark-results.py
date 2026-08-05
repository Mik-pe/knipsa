#!/usr/bin/env python3
"""Compare two JSONL workload adapter outputs."""

import json
import math
import sys


def load(path):
    records = {}
    with open(path, encoding="utf-8") as stream:
        for line in stream:
            if not line.strip() or not line.lstrip().startswith("{"):
                continue
            value = json.loads(line)
            if "id" in value:
                records[value["id"]] = value
    return records


def equivalent(left, right, abs_tol=1e-8):
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return math.isclose(left, right, rel_tol=0.0, abs_tol=abs_tol)
    if type(left) is not type(right):
        return False
    if isinstance(left, list):
        return len(left) == len(right) and all(equivalent(a, b, abs_tol) for a, b in zip(left, right))
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(
            equivalent(left[key], right[key], 1e-6 if key == "area2" else abs_tol)
            for key in left
        )
    return left == right


def canonical_signature(value):
    if not isinstance(value, list) or not all(
        isinstance(record, dict) and {"depth", "area2", "points"} <= record.keys()
        for record in value
    ):
        return value
    def record_key(record):
        return (
            record["depth"],
            round(record["area2"], 6),
            json.dumps(record["points"], sort_keys=True, separators=(",", ":")),
        )

    return sorted(value, key=record_key)


if len(sys.argv) != 3:
    raise SystemExit("usage: compare-benchmark-results.py <knipsa.jsonl> <reference.jsonl>")

knipsa = load(sys.argv[1])
reference = load(sys.argv[2])
ids = sorted(set(knipsa) | set(reference))
matches = 0
for case_id in ids:
    left = knipsa.get(case_id)
    right = reference.get(case_id)
    same = bool(
        left
        and right
        and left.get("status", "ok") == "ok"
        and right.get("status", "ok") == "ok"
        and equivalent(
            canonical_signature(json.loads(left["signature"])),
            canonical_signature(json.loads(right["signature"])),
        )
    )
    if same:
        matches += 1
    speedup = "-"
    if left and right and left.get("median_ns"):
        speedup = f"{right['median_ns'] / left['median_ns']:.2f}x"
    print(f"{case_id:28} {'MATCH' if same else 'MISMATCH':9} "
          f"knipsa={left.get('median_ns', '-') if left else '-':>10}ns "
          f"reference={right.get('median_ns', '-') if right else '-':>10}ns "
          f"reference/knipsa={speedup}")
print(f"matched={matches}/{len(ids)}")
if matches != len(ids):
    raise SystemExit(1)
