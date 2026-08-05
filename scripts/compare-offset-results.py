#!/usr/bin/env python3
"""Compare offset regions using topology, area, and boundary distance."""

import json
import math
import sys


def load_results(path):
    records = {}
    with open(path, encoding="utf-8") as stream:
        for line in stream:
            if line.lstrip().startswith("{"):
                value = json.loads(line)
                if "id" in value:
                    records[value["id"]] = value
    return records


def area2(ring):
    return sum(a[0] * b[1] - a[1] * b[0] for a, b in zip(ring, ring[1:] + ring[:1]))


def point_segment_distance(point, start, end):
    dx, dy = end[0] - start[0], end[1] - start[1]
    length2 = dx * dx + dy * dy
    if length2 == 0:
        return math.hypot(point[0] - start[0], point[1] - start[1])
    amount = max(0.0, min(1.0, ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length2))
    return math.hypot(point[0] - start[0] - amount * dx, point[1] - start[1] - amount * dy)


def directed_boundary_distance(left, right):
    segments = [(ring[i], ring[(i + 1) % len(ring)]) for ring in right for i in range(len(ring))]
    return max((min(point_segment_distance(point, *segment) for segment in segments)
                for ring in left for point in ring), default=0.0)


def region_area(rings):
    # Clipper-style output winding makes outer rings positive and holes negative;
    # absolute total is invariant under reversing every ring.
    return abs(sum(area2(ring) for ring in rings)) * 0.5


if len(sys.argv) != 4:
    raise SystemExit("usage: compare-offset-results.py <workload.json> <knipsa.jsonl> <reference.jsonl>")

with open(sys.argv[1], encoding="utf-8") as stream:
    cases = {case["id"]: case for case in json.load(stream)["cases"]}
knipsa, reference = load_results(sys.argv[2]), load_results(sys.argv[3])
matches = 0
for case_id, case in cases.items():
    left, right = knipsa.get(case_id), reference.get(case_id)
    boundary = area_error = math.inf
    same = bool(left and right and left.get("status") == right.get("status") == "ok")
    if same:
        left_paths, right_paths = json.loads(left["signature"]), json.loads(right["signature"])
        if len(left_paths) == len(right_paths):
            boundary = max(directed_boundary_distance(left_paths, right_paths),
                           directed_boundary_distance(right_paths, left_paths))
            area_error = abs(region_area(left_paths) - region_area(right_paths))
            same = boundary <= case["boundary_tolerance"] and area_error <= case["area_tolerance"]
        else:
            same = False
    if same:
        matches += 1
    speedup = "-" if not left or not right or not left.get("median_ns") else f"{right['median_ns'] / left['median_ns']:.2f}x"
    print(f"{case_id:28} {'MATCH' if same else 'MISMATCH':9} boundary={boundary:.6g} area_error={area_error:.6g} reference/knipsa={speedup}")
print(f"matched={matches}/{len(cases)}")
if matches != len(cases):
    raise SystemExit(1)
