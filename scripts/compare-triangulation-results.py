#!/usr/bin/env python3
"""Validate triangulations against their input region and compare timings."""

import json
import math
import sys
from fractions import Fraction


def load_results(path):
    records = {}
    metadata = None
    with open(path, encoding="utf-8") as stream:
        for line in stream:
            if line.lstrip().startswith("{"):
                value = json.loads(line)
                if "id" in value:
                    if value["id"] in records:
                        raise ValueError(f"{path}: duplicate case {value['id']}")
                    records[value["id"]] = value
                elif "implementation" in value:
                    if metadata is not None:
                        raise ValueError(f"{path}: duplicate metadata header")
                    metadata = value
    expected_metadata = {"samples": 25, "warmups": 3, "minimum_sample_time_ns": 2_000_000}
    if metadata is None or any(metadata.get(key) != value for key, value in expected_metadata.items()):
        raise ValueError(f"{path}: missing or incompatible metadata header")
    return records


def area2(ring):
    return sum(cross(start, end) for start, end in edges(ring))


def cross(left, right):
    return left[0] * right[1] - left[1] * right[0]


def edges(ring):
    return zip(ring, ring[1:] + ring[:1])


def point_on_segment(point, segment, tolerance):
    start, end = segment
    if tolerance == 0:
        dx, dy = end[0] - start[0], end[1] - start[1]
        point_dx, point_dy = point[0] - start[0], point[1] - start[1]
        return (dx * point_dy - dy * point_dx == 0
                and min(start[0], end[0]) <= point[0] <= max(start[0], end[0])
                and min(start[1], end[1]) <= point[1] <= max(start[1], end[1]))
    scale = max(1.0, math.hypot(end[0] - start[0], end[1] - start[1]))
    return point_segment_distance(point, segment) <= tolerance * scale


def segment_parameter(point, segment, exact):
    start, end = segment
    dx, dy = end[0] - start[0], end[1] - start[1]
    length2 = dx * dx + dy * dy
    numerator = (point[0] - start[0]) * dx + (point[1] - start[1]) * dy
    if not length2:
        return Fraction(0) if exact else 0.0
    return Fraction(numerator, length2) if exact else numerator / length2


def triangle_boundaries(triangles, tolerance):
    """Return the covered region boundary independent of edge subdivision."""
    triangle_edges = [segment for triangle in triangles for segment in edges(triangle)]
    vertices = [point for triangle in triangles for point in triangle]
    exact = tolerance == 0
    boundaries = []
    for segment in triangle_edges:
        start, end = segment
        parameters = {
            Fraction(0) if exact else 0.0: tuple(start),
            Fraction(1) if exact else 1.0: tuple(end),
        }
        for point in vertices:
            if point_on_segment(point, segment, tolerance):
                amount = segment_parameter(point, segment, exact)
                if -tolerance <= amount <= 1.0 + tolerance:
                    parameters[max(0, min(1, amount))] = tuple(point)
        ordered = sorted(parameters)
        for lower, upper in zip(ordered, ordered[1:]):
            if upper - lower <= tolerance:
                continue
            lower_point, upper_point = parameters[lower], parameters[upper]
            midpoint = ((lower_point[0] + upper_point[0]) / 2,
                        (lower_point[1] + upper_point[1]) / 2)
            if exact:
                midpoint = (Fraction(lower_point[0] + upper_point[0], 2),
                            Fraction(lower_point[1] + upper_point[1], 2))
            coverage = sum(point_on_segment(midpoint, candidate, tolerance)
                           for candidate in triangle_edges)
            if coverage > 2:
                return None
            if coverage == 1:
                boundaries.append((lower_point, upper_point))
    return boundaries


def point_segment_distance(point, segment):
    start, end = segment
    dx, dy = end[0] - start[0], end[1] - start[1]
    length2 = dx * dx + dy * dy
    if not length2:
        return math.hypot(point[0] - start[0], point[1] - start[1])
    amount = max(0.0, min(1.0, ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length2))
    return math.hypot(point[0] - start[0] - amount * dx, point[1] - start[1] - amount * dy)


def segment_samples(segment):
    start, end = segment
    return (start, end, ((start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5))


def boundary_distance(left, right):
    return max((min(point_segment_distance(point, segment) for segment in right)
                for segment in left for point in segment_samples(segment)), default=0.0)


def boundary_length(segments):
    return sum(math.hypot(end[0] - start[0], end[1] - start[1]) for start, end in segments)


def projection(triangle, axis):
    values = [point[0] * axis[0] + point[1] * axis[1] for point in triangle]
    return min(values), max(values)


def interiors_overlap(left, right, tolerance=0.0):
    for triangle in (left, right):
        for start, end in edges(triangle):
            axis = (start[1] - end[1], end[0] - start[0])
            left_min, left_max = projection(left, axis)
            right_min, right_max = projection(right, axis)
            threshold = 0 if tolerance == 0 else tolerance * max(
                1.0, abs(left_min), abs(left_max), abs(right_min), abs(right_max)
            )
            if min(left_max, right_max) <= max(left_min, right_min) + threshold:
                return False
    return True


def normalize_geometry(paths, triangles):
    points = [point for path in paths for point in path]
    min_x = min(point[0] for point in points)
    max_x = max(point[0] for point in points)
    min_y = min(point[1] for point in points)
    max_y = max(point[1] for point in points)
    origin = (min_x * 0.5, min_y * 0.5)
    scale = max(max_x * 0.5 - origin[0], max_y * 0.5 - origin[1])
    if not math.isfinite(scale) or scale == 0.0:
        raise ValueError("invalid normalization frame")

    def normalize_point(point):
        return ((point[0] * 0.5 - origin[0]) / scale,
                (point[1] * 0.5 - origin[1]) / scale)

    return (
        [[normalize_point(point) for point in path] for path in paths],
        [[normalize_point(point) for point in triangle] for triangle in triangles],
    )


def localize_integer_geometry(paths, triangles):
    origin_x = min(point[0] for path in paths for point in path)
    origin_y = min(point[1] for path in paths for point in path)

    def localize_point(point):
        return (point[0] - origin_x, point[1] - origin_y)

    return (
        [[localize_point(point) for point in path] for path in paths],
        [[localize_point(point) for point in triangle] for triangle in triangles],
    )


def validate(case, record, coordinate_type="i64"):
    if not record or record.get("status") != "ok":
        return False, "missing or errored result"
    if not isinstance(record.get("iterations_per_sample"), int) or record["iterations_per_sample"] < 1:
        return False, "uncalibrated result"
    try:
        triangles = json.loads(record["signature"])
    except (KeyError, TypeError, json.JSONDecodeError):
        return False, "invalid signature"
    if record.get("triangle_count") != len(triangles) or not triangles:
        return False, "invalid triangle count"
    if any(len(triangle) != 3 or any(len(point) != 2 for point in triangle) for triangle in triangles):
        return False, "non-triangle output"
    coordinates = [coordinate for triangle in triangles for point in triangle for coordinate in point]
    if coordinate_type == "i64":
        if any(not isinstance(coordinate, int) or isinstance(coordinate, bool)
               for coordinate in coordinates):
            return False, "non-integer triangulate64 output"
        paths, triangles = localize_integer_geometry(case["paths"], triangles)
        area_tolerance = 0
        boundary_tolerance = 0.0
        overlap_tolerance = 0.0
    elif coordinate_type == "f64":
        if any(isinstance(coordinate, bool) or not isinstance(coordinate, (int, float))
               or not math.isfinite(coordinate) for coordinate in coordinates):
            return False, "non-finite triangulate_d output"
        try:
            paths, triangles = normalize_geometry(case["paths"], triangles)
        except (IndexError, ValueError) as error:
            return False, str(error)
        area_tolerance = 1e-10
        boundary_tolerance = 1e-9
        overlap_tolerance = 1e-12
    else:
        return False, "unsupported coordinate type"
    triangle_areas = [abs(area2(triangle)) for triangle in triangles]
    if any(area <= area_tolerance for area in triangle_areas):
        return False, "degenerate triangle"
    expected_area2 = abs(sum(area2(path) for path in paths))
    actual_area2 = sum(triangle_areas)
    if abs(actual_area2 - expected_area2) > max(area_tolerance, expected_area2 * area_tolerance):
        return False, f"area2={sum(triangle_areas)} expected={expected_area2}"
    for index, left in enumerate(triangles):
        if any(interiors_overlap(left, right, overlap_tolerance)
               for right in triangles[index + 1:]):
            return False, "triangle interiors overlap"
    actual_boundary = triangle_boundaries(triangles, overlap_tolerance)
    if actual_boundary is None:
        return False, "non-manifold triangle edge multiplicity"
    expected_boundary = [(tuple(start), tuple(end)) for path in paths for start, end in edges(path)]
    distance = max(boundary_distance(actual_boundary, expected_boundary),
                   boundary_distance(expected_boundary, actual_boundary))
    perimeter_error = abs(boundary_length(actual_boundary) - boundary_length(expected_boundary))
    perimeter_tolerance = max(boundary_tolerance, boundary_length(expected_boundary) * 1e-12)
    if distance > boundary_tolerance or perimeter_error > perimeter_tolerance:
        return False, f"boundary_distance={distance:.6g} perimeter_error={perimeter_error:.6g}"
    return True, f"triangles={len(triangles)} area2={expected_area2}"


def compare(workload_path, knipsa_path, reference_path):
    with open(workload_path, encoding="utf-8") as stream:
        workload = json.load(stream)
    coordinate_type = workload.get("coordinate_type")
    schemas = {
        "knipsa-triangulation-workload-v1": "i64",
        "knipsa-triangulation-d-workload-v1": "f64",
    }
    if schemas.get(workload.get("schema")) != coordinate_type:
        raise ValueError("unsupported triangulation workload schema")
    knipsa = load_results(knipsa_path)
    reference = load_results(reference_path)
    print(f"reference={reference_path}")
    expected_ids = {case["id"] for case in workload["cases"]}
    if set(knipsa) != expected_ids or set(reference) != expected_ids:
        raise ValueError("result case IDs do not exactly match the triangulation workload")
    matches = 0
    for case in workload["cases"]:
        case_id = case["id"]
        left, right = knipsa.get(case_id), reference.get(case_id)
        left_ok, left_detail = validate(case, left, coordinate_type)
        right_ok, right_detail = validate(case, right, coordinate_type)
        same = left_ok and right_ok
        if same:
            matches += 1
        speedup = "-"
        if left_ok and right_ok and left.get("median_ns"):
            speedup = f"{right['median_ns'] / left['median_ns']:.2f}x"
        detail = left_detail if not left_ok else right_detail if not right_ok else left_detail
        print(f"{case_id:28} {'MATCH' if same else 'MISMATCH':9} {detail} reference/knipsa={speedup}")
    print(f"matched={matches}/{len(workload['cases'])}")
    return matches == len(workload["cases"])


def main(argv):
    if len(argv) < 4:
        raise SystemExit(
            "usage: compare-triangulation-results.py "
            "<workload.json> <knipsa.jsonl> <reference.jsonl> [reference.jsonl ...]"
        )
    if not all(compare(argv[1], argv[2], reference) for reference in argv[3:]):
        raise SystemExit(1)


if __name__ == "__main__":
    main(sys.argv)
