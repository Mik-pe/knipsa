#!/usr/bin/env python3
"""Fail-closed comparison for separate integer closed/open Boolean outputs."""

from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

from benchmark_result_protocol import (
    ResultFormatError,
    calibrated_timing_error,
    expected_id_errors,
    load_expected_ids,
    load_results,
)


I64_MIN = -(1 << 63)
I64_MAX = (1 << 63) - 1


def validate_open_record(path: Path, record: dict[str, object]) -> None:
    case_id = record["id"]
    if record.get("status") not in {"ok", "error"}:
        raise ResultFormatError(f"{path}: {case_id}: invalid status")
    if record["status"] == "ok":
        error = calibrated_timing_error(record, str(path), case_id)
        if error is not None:
            raise ResultFormatError(error)
        for kind in ("closed", "open"):
            count = record.get(f"{kind}_path_count")
            if not isinstance(count, int) or isinstance(count, bool) or count < 0:
                raise ResultFormatError(f"{path}: {case_id}: invalid {kind}_path_count")
            paths = parse_signature(record.get(f"{kind}_signature"), kind)
            if len(paths) != count:
                raise ResultFormatError(
                    f"{path}: {case_id}: {kind}_path_count disagrees with signature"
                )
    else:
        if not isinstance(record.get("error"), str) or not record["error"]:
            raise ResultFormatError(f"{path}: {case_id}: error record has no message")
        for field in ("median_ns", "p95_ns", "iterations_per_sample"):
            if record.get(field) != 0:
                raise ResultFormatError(f"{path}: {case_id}: error record has nonzero {field}")
        for kind in ("closed", "open"):
            if record.get(f"{kind}_path_count") != 0:
                raise ResultFormatError(
                    f"{path}: {case_id}: error record has nonzero {kind}_path_count"
                )
            if parse_signature(record.get(f"{kind}_signature"), kind):
                raise ResultFormatError(
                    f"{path}: {case_id}: error record has nonempty {kind}_signature"
                )


def validate_open_results(
    path: Path,
) -> tuple[dict[str, object], dict[str, dict[str, object]]]:
    """Load the shared protocol and validate open-profile record payloads."""
    header, records = load_results(path)
    for record in records.values():
        validate_open_record(path, record)
    return header, records


def parse_signature(value: object, kind: str) -> list[list[tuple[int, int]]]:
    if not isinstance(value, str):
        raise ResultFormatError(f"{kind}_signature must be a JSON string")
    try:
        raw_paths = json.loads(value)
    except json.JSONDecodeError as error:
        raise ResultFormatError(f"invalid {kind}_signature JSON: {error}") from error
    if not isinstance(raw_paths, list):
        raise ResultFormatError(f"{kind}_signature must contain an array")
    result = []
    minimum = 3 if kind == "closed" else 2
    for raw_path in raw_paths:
        if not isinstance(raw_path, list):
            raise ResultFormatError(f"{kind} path must be an array")
        path = []
        for raw_point in raw_path:
            if not isinstance(raw_point, list) or len(raw_point) != 2:
                raise ResultFormatError(f"{kind} point must be a coordinate pair")
            if any(not isinstance(value, int) or isinstance(value, bool) for value in raw_point):
                raise ResultFormatError(f"{kind} coordinates must be exact integers")
            point = tuple(raw_point)
            if any(value < I64_MIN or value > I64_MAX for value in point):
                raise ResultFormatError(f"{kind} coordinate is outside i64")
            path.append(point)
        if kind == "closed" and len(path) > 1 and path[0] == path[-1]:
            path.pop()
        if len(path) < minimum:
            raise ResultFormatError(f"{kind} path has fewer than {minimum} distinct vertices")
        if any(start == end for start, end in path_segments(path, kind == "closed")):
            raise ResultFormatError(f"{kind} path contains a zero-length edge")
        result.append(path)
    return result


def path_segments(path: list[tuple[int, int]], closed: bool):
    pairs = zip(path, path[1:])
    if closed:
        return [*pairs, (path[-1], path[0])]
    return list(pairs)


def lies_on_segment(point, start, end):
    cross = ((point[0] - start[0]) * (end[1] - start[1]) -
             (point[1] - start[1]) * (end[0] - start[0]))
    if cross != 0:
        return False
    return (min(start[0], end[0]) <= point[0] <= max(start[0], end[0]) and
            min(start[1], end[1]) <= point[1] <= max(start[1], end[1]))


def atomic_edges(paths, split_points, *, closed, directed):
    edges = collections.Counter()
    for path in paths:
        for start, end in path_segments(path, closed):
            points = [point for point in split_points if lies_on_segment(point, start, end)]
            points.sort(key=lambda point: ((point[0] - start[0]) * (end[0] - start[0]) +
                                           (point[1] - start[1]) * (end[1] - start[1])))
            if points[0] != start or points[-1] != end:
                raise ResultFormatError("atomic edge splitting lost a segment endpoint")
            for edge_start, edge_end in zip(points, points[1:]):
                if edge_start == edge_end:
                    continue
                edge = (edge_start, edge_end)
                if not directed and edge_end < edge_start:
                    edge = (edge_end, edge_start)
                edges[edge] += 1
    return edges


def equivalent_edges(left, right, *, closed, directed):
    split_points = {point for paths in (left, right) for path in paths for point in path}
    left_edges = atomic_edges(left, split_points, closed=closed, directed=directed)
    right_edges = atomic_edges(right, split_points, closed=closed, directed=directed)
    if left_edges == right_edges:
        return True, "ok"
    missing = right_edges - left_edges
    extra = left_edges - right_edges
    return False, f"missing={list(missing.items())[:3]} extra={list(extra.items())[:3]}"


def compare(workload_path: Path, left_path: Path, right_path: Path) -> int:
    try:
        with workload_path.open(encoding="utf-8") as stream:
            workload = json.load(stream)
        expected = load_expected_ids(workload_path)
    except (OSError, json.JSONDecodeError, ResultFormatError) as error:
        print(error, file=sys.stderr)
        return 2
    if workload.get("schema") != "knipsa-open-workload-v1":
        print("invalid workload schema", file=sys.stderr)
        return 2
    if workload.get("coordinate_type") != "i64" or workload.get("comparison") != {
        "coordinate_tolerance": 0
    }:
        print("open workload must declare exact i64 comparison", file=sys.stderr)
        return 2
    cases = workload.get("cases")
    if not isinstance(cases, list) or not cases:
        print("open workload has no cases", file=sys.stderr)
        return 2
    case_ids = [case.get("id") for case in cases if isinstance(case, dict)]
    if len(case_ids) != len(cases) or any(not isinstance(case_id, str) for case_id in case_ids):
        print("open workload contains an invalid case", file=sys.stderr)
        return 2

    try:
        _, left = validate_open_results(left_path)
        _, right = validate_open_results(right_path)
    except ResultFormatError as error:
        print(error, file=sys.stderr)
        return 2
    completeness_errors = expected_id_errors(
        expected, {str(left_path): left, str(right_path): right}
    )
    if completeness_errors:
        print("\n".join(completeness_errors), file=sys.stderr)
        return 2

    matches = 0
    for case_id in case_ids:
        left_record = left[case_id]
        right_record = right[case_id]
        same = left_record["status"] == right_record["status"] == "ok"
        details = []
        if same:
            try:
                left_closed = parse_signature(left_record["closed_signature"], "closed")
                right_closed = parse_signature(right_record["closed_signature"], "closed")
                left_open = parse_signature(left_record["open_signature"], "open")
                right_open = parse_signature(right_record["open_signature"], "open")
                closed_same, closed_detail = equivalent_edges(
                    left_closed, right_closed, closed=True, directed=False
                )
                open_same, open_detail = equivalent_edges(
                    left_open, right_open, closed=False, directed=True
                )
                same = closed_same and open_same
                if not closed_same:
                    details.append(f"closed {closed_detail}")
                if not open_same:
                    details.append(f"open {open_detail}")
            except ResultFormatError as error:
                same = False
                details.append(str(error))
        else:
            details.append(
                f"status={left_record['status']}/{right_record['status']} "
                f"errors={left_record.get('error')!r}/{right_record.get('error')!r}"
            )
        if same:
            matches += 1
        speedup = right_record["median_ns"] / left_record["median_ns"] if (
            left_record["status"] == right_record["status"] == "ok"
        ) else None
        speed = "-" if speedup is None else f"{speedup:.2f}x"
        detail = "" if not details else " " + "; ".join(details)
        print(
            f"{case_id:34} {'MATCH' if same else 'MISMATCH':9} "
            f"reference/knipsa={speed}{detail}"
        )
    print(f"matched={matches}/{len(case_ids)}")
    return 0 if matches == len(case_ids) else 1


def main() -> int:
    if len(sys.argv) != 4:
        raise SystemExit(
            "usage: compare-open-results.py <workload.json> <knipsa.jsonl> <reference.jsonl>"
        )
    return compare(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))


if __name__ == "__main__":
    raise SystemExit(main())
