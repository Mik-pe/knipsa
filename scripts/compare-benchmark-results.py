#!/usr/bin/env python3
"""Fail-closed comparison of Knipsa and reference JSONL workloads."""

from __future__ import annotations

import argparse
import json
import math
import sys
from fractions import Fraction
from pathlib import Path
from typing import Any


class ResultFormatError(ValueError):
    """Raised when an adapter output is incomplete or malformed."""


def load_results(path: Path) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    header: dict[str, Any] | None = None
    records: dict[str, dict[str, Any]] = {}
    saw_record = False

    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as error:
            raise ResultFormatError(f"{path}:{line_number}: invalid JSON: {error}") from error
        if not isinstance(value, dict):
            raise ResultFormatError(f"{path}:{line_number}: JSON value is not an object")
        if "implementation" in value:
            if saw_record:
                raise ResultFormatError(f"{path}:{line_number}: header appears after a case")
            if header is not None:
                raise ResultFormatError(f"{path}:{line_number}: duplicate adapter header")
            header = value
            continue
        saw_record = True
        case_id = value.get("id")
        if not isinstance(case_id, str) or not case_id:
            raise ResultFormatError(f"{path}:{line_number}: case has no non-empty string id")
        if case_id in records:
            raise ResultFormatError(f"{path}:{line_number}: duplicate case id {case_id!r}")
        records[case_id] = value

    if header is None:
        raise ResultFormatError(f"{path}: missing adapter header")
    if not records:
        raise ResultFormatError(f"{path}: contains no case records")
    return header, records


def load_expected_ids(path: Path) -> set[str]:
    try:
        workload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ResultFormatError(f"{path}: invalid workload JSON: {error}") from error
    if not isinstance(workload, dict) or not isinstance(workload.get("cases"), list):
        raise ResultFormatError(f"{path}: workload must contain a cases array")

    ids: set[str] = set()
    for index, case in enumerate(workload["cases"]):
        if not isinstance(case, dict) or not isinstance(case.get("id"), str) or not case["id"]:
            raise ResultFormatError(f"{path}: case {index} has no non-empty string id")
        case_id = case["id"]
        if case_id in ids:
            raise ResultFormatError(f"{path}: duplicate case id {case_id!r}")
        ids.add(case_id)
    if not ids:
        raise ResultFormatError(f"{path}: workload contains no cases")
    return ids


def load_tolerances(path: Path) -> tuple[float, float]:
    try:
        workload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ResultFormatError(f"{path}: invalid workload JSON: {error}") from error
    comparison = workload.get("comparison", {})
    if not isinstance(comparison, dict):
        raise ResultFormatError(f"{path}: comparison must be an object")

    def tolerance(name: str, default: float) -> float:
        value = comparison.get(name, default)
        if (
            not isinstance(value, (int, float))
            or isinstance(value, bool)
            or not math.isfinite(value)
            or value < 0
        ):
            raise ResultFormatError(f"{path}: invalid non-negative {name}={value!r}")
        return float(value)

    return tolerance("coordinate_tolerance", 1e-8), tolerance("area2_tolerance", 1e-6)


def load_coordinate_type(path: Path) -> str:
    try:
        workload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ResultFormatError(f"{path}: invalid workload JSON: {error}") from error
    coordinate_type = workload.get("coordinate_type")
    if coordinate_type not in {"f64", "i64"}:
        raise ResultFormatError(f"{path}: unsupported coordinate_type {coordinate_type!r}")
    return coordinate_type


def validate_record(record: dict[str, Any], source: str, case_id: str) -> str | None:
    status = record.get("status")
    if status not in {"ok", "error"}:
        return f"{source}:{case_id}: invalid status {status!r}"
    if status != "ok":
        return f"{source}:{case_id}: adapter reported {record.get('error')!r}"

    for key in ("median_ns", "p95_ns", "ring_count"):
        value = record.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            return f"{source}:{case_id}: invalid non-negative integer {key}={value!r}"
    iterations = record.get("iterations_per_sample")
    if not isinstance(iterations, int) or isinstance(iterations, bool) or iterations < 1:
        return f"{source}:{case_id}: invalid positive integer iterations_per_sample={iterations!r}"
    try:
        signature = json.loads(record["signature"])
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        return f"{source}:{case_id}: invalid signature: {error}"
    if not isinstance(signature, list):
        return f"{source}:{case_id}: signature is not an array"
    if record["ring_count"] != len(signature):
        return (
            f"{source}:{case_id}: ring_count={record['ring_count']} "
            f"does not match signature length {len(signature)}"
        )
    for ring in signature:
        points = ring.get("points") if isinstance(ring, dict) else ring
        if not isinstance(points, list) or len(points) < 3:
            return f"{source}:{case_id}: invalid ring points"
        if not all(
            isinstance(point, list)
            and len(point) == 2
            and all(
                isinstance(coordinate, (int, float))
                and not isinstance(coordinate, bool)
                and math.isfinite(coordinate)
                for coordinate in point
            )
            for point in points
        ):
            return f"{source}:{case_id}: invalid ring coordinate"
    return None


def equivalent(
    left: Any,
    right: Any,
    coordinate_tolerance: float = 1e-8,
    area2_tolerance: float = 1e-6,
) -> bool:
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        if (
            isinstance(left, int)
            and not isinstance(left, bool)
            and isinstance(right, int)
            and not isinstance(right, bool)
            and coordinate_tolerance == 0
        ):
            return left == right
        return math.isclose(left, right, rel_tol=0.0, abs_tol=coordinate_tolerance)
    if type(left) is not type(right):
        return False
    if isinstance(left, list):
        return len(left) == len(right) and all(
            equivalent(a, b, coordinate_tolerance, area2_tolerance)
            for a, b in zip(left, right)
        )
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(
            equivalent(
                left[key],
                right[key],
                area2_tolerance if key == "area2" else coordinate_tolerance,
                area2_tolerance,
            )
            for key in left
        )
    return left == right


def quantize(value: float) -> float:
    """Apply the adapter protocol's fixed 1e-9 output normalization."""
    scaled = value * 1e9
    if not math.isfinite(scaled):
        raise ResultFormatError("signature coordinate exceeds the canonicalization range")
    rounded = math.floor(scaled + 0.5) if scaled >= 0 else math.ceil(scaled - 0.5)
    result = rounded / 1e9
    return 0.0 if result == 0.0 else result


def ring_area2(points: list[list[float]]) -> float:
    return sum(
        start[0] * end[1] - start[1] * end[0]
        for start, end in zip(points, points[1:] + points[:1])
    )


def canonical_ring(
    raw_points: list[list[float]],
    coordinate_tolerance: float = 1e-8,
    integer_coordinates: bool = False,
) -> list[list[float]]:
    if integer_coordinates:
        if not all(
            isinstance(value, int) and not isinstance(value, bool)
            for point in raw_points
            for value in point
        ):
            raise ResultFormatError("integer signature contains a non-integer coordinate")
        points = [[x, y] for x, y in raw_points]
    else:
        points = [[quantize(float(x)), quantize(float(y))] for x, y in raw_points]
    if len(points) > 1 and points[0] == points[-1]:
        points.pop()
    points = [point for index, point in enumerate(points) if index == 0 or point != points[index - 1]]

    # A repeated non-adjacent vertex is a graph junction in a self-touching
    # boundary. Removing a locally collinear occurrence can splice two cycles
    # together and change the edge multiset, so simplify only simple rings.
    changed = len({tuple(point) for point in points}) == len(points)
    while changed and len(points) >= 3:
        changed = False
        cleaned: list[list[float]] = []
        for index, current in enumerate(points):
            previous = points[(index - 1) % len(points)]
            following = points[(index + 1) % len(points)]
            first = (current[0] - previous[0], current[1] - previous[1])
            second = (following[0] - current[0], following[1] - current[1])
            cross = first[0] * second[1] - first[1] * second[0]
            dot = first[0] * second[0] + first[1] * second[1]
            if abs(cross) <= 1e-12 and dot >= -1e-12:
                changed = True
            else:
                cleaned.append(current)
        points = cleaned
    if len(points) < 3:
        raise ResultFormatError("signature contains a degenerate ring")

    def coordinate_bucket(value: float) -> int | float:
        if coordinate_tolerance == 0:
            return value
        return round(value / coordinate_tolerance)

    def point_key(point: list[float]) -> tuple[Any, ...]:
        return (
            coordinate_bucket(point[0]),
            coordinate_bucket(point[1]),
            point[0],
            point[1],
        )

    def rotate_to_minimum(path: list[list[float]]) -> list[list[float]]:
        minimum = min(range(len(path)), key=lambda index: point_key(path[index]))
        return path[minimum:] + path[:minimum]

    forward = rotate_to_minimum(points)
    reverse = rotate_to_minimum(list(reversed(points)))
    return min(forward, reverse, key=lambda path: tuple(point_key(point) for point in path))


def point_location(point: list[float], ring: list[list[float]], tolerance: float) -> int:
    """Return -1 outside, 0 on the boundary, or 1 inside a simple ring."""
    inside = False
    for start, end in zip(ring, ring[1:] + ring[:1]):
        dx = end[0] - start[0]
        dy = end[1] - start[1]
        offset_x = point[0] - start[0]
        offset_y = point[1] - start[1]
        cross = dx * offset_y - dy * offset_x
        cross_budget = tolerance * max(1.0, abs(dx) + abs(dy))
        within_segment = (
            min(start[0], end[0]) <= point[0] <= max(start[0], end[0])
            and min(start[1], end[1]) <= point[1] <= max(start[1], end[1])
            if tolerance == 0
            else min(start[0], end[0]) - tolerance
            <= point[0]
            <= max(start[0], end[0]) + tolerance
            and min(start[1], end[1]) - tolerance
            <= point[1]
            <= max(start[1], end[1]) + tolerance
        )
        if (
            abs(cross) <= cross_budget
            and within_segment
        ):
            return 0
        if (start[1] > point[1]) != (end[1] > point[1]):
            if (dy > 0 and cross > 0) or (dy < 0 and cross < 0):
                inside = not inside
    return 1 if inside else -1


def ring_relation(subject: list[list[float]], container: list[list[float]], tolerance: float) -> int:
    """Classify a non-crossing output ring without choosing a boundary vertex."""
    locations = {
        location
        for point in subject
        if (location := point_location(point, container, tolerance)) != 0
    }
    if not locations:
        integer_ring = all(
            isinstance(value, int) and not isinstance(value, bool)
            for point in subject
            for value in point
        )
        midpoints = [
            [
                Fraction(start[0] + end[0], 2)
                if integer_ring
                else (start[0] + end[0]) / 2.0,
                Fraction(start[1] + end[1], 2)
                if integer_ring
                else (start[1] + end[1]) / 2.0,
            ]
            for start, end in zip(subject, subject[1:] + subject[:1])
        ]
        locations = {
            location
            for point in midpoints
            if (location := point_location(point, container, tolerance)) != 0
        }
    if len(locations) != 1:
        reason = "coincident" if not locations else "crossing"
        raise ResultFormatError(f"signature contains {reason} output rings")
    return locations.pop()


def canonical_signature(
    value: Any,
    coordinate_tolerance: float = 1e-8,
    integer_coordinates: bool = False,
) -> Any:
    if not isinstance(value, list):
        return value
    raw_rings = [record.get("points") if isinstance(record, dict) else record for record in value]
    if not all(isinstance(points, list) for points in raw_rings):
        return value
    rings = [
        canonical_ring(points, coordinate_tolerance, integer_coordinates)
        for points in raw_rings
    ]
    doubled_areas = [abs(ring_area2(points)) for points in rings]
    if not all(math.isfinite(area) for area in doubled_areas):
        raise ResultFormatError("signature ring area is not finite")
    boundary_tolerance = (
        0.0 if integer_coordinates else max(coordinate_tolerance, 1e-9)
    )
    records = []
    for index, points in enumerate(rings):
        depth = sum(
            ring_relation(points, other, boundary_tolerance) == 1
            for other_index, other in enumerate(rings)
            if other_index != index
        )
        records.append({"depth": depth, "area2": doubled_areas[index], "points": points})
    return sorted(
        records,
        key=lambda record: (
            record["depth"],
            round(record["area2"], 6),
            json.dumps(record["points"], separators=(",", ":")),
        ),
    )


def canonical_boundary(value: Any, coordinate_tolerance: float = 1e-8) -> Any:
    """Normalize equivalent simple-ring and self-touching-ring decompositions."""
    if not isinstance(value, list) or not all(
        isinstance(record, dict) and {"depth", "area2", "points"} <= record.keys()
        for record in value
    ):
        return value

    edges: list[list[Any]] = []
    all_points = sorted({tuple(point) for record in value for point in record["points"]})
    for record in value:
        points = record["points"]
        if not isinstance(points, list):
            return value
        for start, end in zip(points, points[1:] + points[:1]):
            dx = end[0] - start[0]
            dy = end[1] - start[1]

            def lies_on_edge(point: tuple[float, float]) -> bool:
                cross = dx * (point[1] - start[1]) - dy * (point[0] - start[0])
                tolerance = 0.0 if coordinate_tolerance == 0 else 1e-9
                within_segment = (
                    min(start[0], end[0]) <= point[0] <= max(start[0], end[0])
                    and min(start[1], end[1]) <= point[1] <= max(start[1], end[1])
                    if tolerance == 0
                    else min(start[0], end[0]) - tolerance
                    <= point[0]
                    <= max(start[0], end[0]) + tolerance
                    and min(start[1], end[1]) - tolerance
                    <= point[1]
                    <= max(start[1], end[1]) + tolerance
                )
                return (
                    abs(cross) <= tolerance * max(1.0, abs(dx) + abs(dy))
                    and within_segment
                )

            split_points = [list(point) for point in all_points if lies_on_edge(point)]
            if abs(dx) >= abs(dy):
                split_points.sort(key=lambda point: (point[0], point[1]), reverse=dx < 0)
            else:
                split_points.sort(key=lambda point: (point[1], point[0]), reverse=dy < 0)
            for split_start, split_end in zip(split_points, split_points[1:]):
                if split_start != split_end:
                    edges.append(
                        [split_start, split_end]
                        if split_start <= split_end
                        else [split_end, split_start]
                    )
    edges.sort(key=lambda edge: (*edge[0], *edge[1]))
    # For the EvenOdd decomposition fallback, an identical complete
    # undirected edge multiset defines the same parity boundary regardless of
    # how a self-touching traversal is split into rings. Multiplicity remains
    # significant because `edges` is a list, not a set.
    return {"edges": edges}


def compare(
    workload: Path | None,
    left_path: Path,
    right_path: Path,
) -> int:
    try:
        left_header, left = load_results(left_path)
        right_header, right = load_results(right_path)
        expected = load_expected_ids(workload) if workload else None
        coordinate_tolerance, area2_tolerance = (
            load_tolerances(workload) if workload else (1e-8, 1e-6)
        )
        coordinate_type = load_coordinate_type(workload) if workload else "f64"
    except ResultFormatError as error:
        print(f"FORMAT ERROR: {error}", file=sys.stderr)
        return 2

    failures: list[str] = []
    if expected is not None:
        for name, records in (("left", left), ("right", right)):
            missing = sorted(expected - records.keys())
            extra = sorted(records.keys() - expected)
            if missing:
                failures.append(f"{name}: missing cases: {', '.join(missing)}")
            if extra:
                failures.append(f"{name}: unexpected cases: {', '.join(extra)}")

    ids = sorted(set(left) | set(right))
    matches = 0
    left_name = str(left_header.get("implementation", left_path))
    right_name = str(right_header.get("implementation", right_path))
    for case_id in ids:
        left_record = left.get(case_id)
        right_record = right.get(case_id)
        error = None
        if left_record is None or right_record is None:
            error = "missing record"
        else:
            error = validate_record(left_record, left_name, case_id)
            if error is None:
                error = validate_record(right_record, right_name, case_id)
        same = False
        if error is None and left_record is not None and right_record is not None:
            try:
                left_signature = canonical_signature(
                    json.loads(left_record["signature"]),
                    coordinate_tolerance,
                    coordinate_type == "i64",
                )
                right_signature = canonical_signature(
                    json.loads(right_record["signature"]),
                    coordinate_tolerance,
                    coordinate_type == "i64",
                )
                same = equivalent(
                    left_signature,
                    right_signature,
                    coordinate_tolerance,
                    area2_tolerance,
                )
                if not same:
                    same = equivalent(
                        canonical_boundary(left_signature, coordinate_tolerance),
                        canonical_boundary(right_signature, coordinate_tolerance),
                        coordinate_tolerance,
                        area2_tolerance,
                    )
                if not same:
                    error = "canonical filled boundaries differ"
            except ResultFormatError as format_error:
                error = str(format_error)
        if same:
            matches += 1
        else:
            failures.append(f"{case_id}: {error or 'mismatch'}")
        speedup = "-"
        if left_record and right_record and left_record.get("median_ns"):
            speedup = f"{right_record.get('median_ns', 0) / left_record['median_ns']:.2f}x"
        print(
            f"{case_id:28} {'MATCH' if same else 'MISMATCH':9} "
            f"left={left_record.get('median_ns', '-') if left_record else '-':>10}ns "
            f"right={right_record.get('median_ns', '-') if right_record else '-':>10}ns "
            f"right/left={speedup}"
        )

    print(f"matched={matches}/{len(ids)}")
    if failures:
        print("conformance failures:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("left", type=Path, help="Knipsa JSONL output")
    parser.add_argument("right", type=Path, help="reference JSONL output")
    parser.add_argument(
        "--workload",
        type=Path,
        help="require both outputs to contain exactly the workload's case IDs",
    )
    args = parser.parse_args()
    return compare(args.workload, args.left, args.right)


if __name__ == "__main__":
    raise SystemExit(main())
