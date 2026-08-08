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
    raw_rings = [
        [[point[0], point[1]] for point in ring[:-1] if len(ring) > 1]
        for ring in rings(geometry)
    ]
    return json.dumps(raw_rings, separators=(",", ":"))


def main(workload_path):
    workload = json.loads(Path(workload_path).read_text(encoding="utf-8"))
    minimum_sample_time_ns = 2_000_000
    maximum_iterations_per_sample = 1 << 20
    print(json.dumps({
        "implementation": "geos-shapely",
        "samples": 25,
        "warmups": 3,
        "minimum_sample_time_ns": minimum_sample_time_ns,
    }))
    for test_case in workload["cases"]:
        for _ in range(3):
            operation(test_case["clip_type"], test_case["subjects"], test_case["clips"])
        result = GeometryCollection()
        iterations_per_sample = 1
        while True:
            started = time.perf_counter_ns()
            for _ in range(iterations_per_sample):
                result = operation(test_case["clip_type"], test_case["subjects"], test_case["clips"])
            elapsed = time.perf_counter_ns() - started
            if elapsed >= minimum_sample_time_ns or iterations_per_sample == maximum_iterations_per_sample:
                break
            iterations_per_sample = min(iterations_per_sample * 2, maximum_iterations_per_sample)
        timings = []
        for _ in range(25):
            started = time.perf_counter_ns()
            for _ in range(iterations_per_sample):
                result = operation(test_case["clip_type"], test_case["subjects"], test_case["clips"])
            timings.append((time.perf_counter_ns() - started) // iterations_per_sample)
        timings.sort()
        print(json.dumps({
            "id": test_case["id"],
            "status": "ok",
            "error": None,
            "median_ns": timings[len(timings) // 2],
            "p95_ns": timings[math.ceil(len(timings) * 0.95) - 1],
            "iterations_per_sample": iterations_per_sample,
            "ring_count": len(rings(result)),
            "signature": signature(result),
        }, separators=(",", ":")))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "benchmarks/workloads.json")
