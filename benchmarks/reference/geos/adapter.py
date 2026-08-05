"""GEOS-backed reference adapter through the pinned Shapely wheel."""

from __future__ import annotations

import json
import math
import sys
import time
from pathlib import Path

from shapely.geometry import GeometryCollection, Polygon
from shapely.ops import unary_union
from shapely.validation import make_valid


def close_ring(path):
    points = [tuple(point) for point in path]
    if points and points[0] != points[-1]:
        points.append(points[0])
    return points


def region(paths):
    geometries = []
    for path in paths:
        ring = close_ring(path)
        if len(ring) >= 4:
            geometries.append(make_valid(Polygon(ring)))
    return unary_union(geometries) if geometries else GeometryCollection()


def operation(name, subjects, clips):
    left = region(subjects)
    right = region(clips)
    if name == "intersection":
        return left.intersection(right)
    if name == "union":
        return left.union(right)
    if name == "difference":
        return left.difference(right)
    if name == "xor":
        return left.symmetric_difference(right)
    raise ValueError(f"unknown operation: {name}")


def area2(ring):
    return sum(
        point[0] * following[1] - point[1] * following[0]
        for point, following in zip(ring, ring[1:] + ring[:1])
    )


def quantize(value):
    rounded = round(value, 9)
    return 0.0 if rounded == 0.0 else rounded


def compare_points(left, right):
    for a, b in zip(left, right):
        if a != b:
            return -1 if a < b else 1
    return (len(left) > len(right)) - (len(left) < len(right))


def canonical_ring(ring):
    points = [[quantize(point[0]), quantize(point[1])] for point in ring[:-1]]
    candidates = []
    for candidate in (points, list(reversed(points))):
        minimum = min(range(len(candidate)), key=lambda index: candidate[index])
        candidates.append(candidate[minimum:] + candidate[:minimum])
    return min(candidates, key=lambda candidate: PointKey(candidate))


class PointKey(list):
    def __lt__(self, other):
        return compare_points(self, other) < 0


def contains(point, ring):
    inside = False
    for start, end in zip(ring, ring[1:] + ring[:1]):
        if (start[1] > point[1]) != (end[1] > point[1]):
            cross = (end[0] - start[0]) * (point[1] - start[1]) - (end[1] - start[1]) * (point[0] - start[0])
            if (end[1] > start[1] and cross > 0) or (end[1] < start[1] and cross < 0):
                inside = not inside
    return inside


def rings(geometry):
    if geometry.geom_type == "Polygon":
        return [list(geometry.exterior.coords)] + [list(interior.coords) for interior in geometry.interiors]
    if hasattr(geometry, "geoms"):
        result = []
        for child in geometry.geoms:
            result.extend(rings(child))
        return result
    return []


def signature(geometry):
    raw_rings = rings(geometry)
    records = []
    for index, ring in enumerate(raw_rings):
        normalized = canonical_ring(ring)
        point = normalized[0]
        depth = sum(
            other_index != index and contains(point, other[:-1])
            for other_index, other in enumerate(raw_rings)
        )
        records.append({"depth": depth, "area2": quantize(abs(area2(ring[:-1]))), "points": normalized})
    return json.dumps(sorted(records, key=lambda value: json.dumps(value, sort_keys=True)), separators=(",", ":"))


def main(workload_path):
    workload = json.loads(Path(workload_path).read_text(encoding="utf-8"))
    print(json.dumps({"implementation": "geos-shapely", "samples": 25, "warmups": 3}))
    for test_case in workload["cases"]:
        for _ in range(3):
            operation(test_case["clip_type"], test_case["subjects"], test_case["clips"])
        timings = []
        result = GeometryCollection()
        for _ in range(25):
            started = time.perf_counter_ns()
            result = operation(test_case["clip_type"], test_case["subjects"], test_case["clips"])
            timings.append(time.perf_counter_ns() - started)
        timings.sort()
        print(json.dumps({
            "id": test_case["id"],
            "status": "ok",
            "error": None,
            "median_ns": timings[len(timings) // 2],
            "p95_ns": timings[math.ceil(len(timings) * 0.95) - 1],
            "ring_count": len(rings(result)),
            "signature": signature(result),
        }, separators=(",", ":")))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "benchmarks/workloads.json")
