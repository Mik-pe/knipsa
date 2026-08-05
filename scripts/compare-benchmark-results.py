#!/usr/bin/env python3
"""Fail-closed comparison of Knipsa and reference JSONL workloads."""

from __future__ import annotations

import argparse
import json
import math
import sys
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
    try:
        signature = json.loads(record["signature"])
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        return f"{source}:{case_id}: invalid signature: {error}"
    if not isinstance(signature, list):
        return f"{source}:{case_id}: signature is not an array"
    for ring in signature:
        if not isinstance(ring, dict) or not {"depth", "area2", "points"} <= ring.keys():
            return f"{source}:{case_id}: malformed ring record"
        if not isinstance(ring["depth"], int) or ring["depth"] < 0:
            return f"{source}:{case_id}: invalid ring depth"
        if not isinstance(ring["points"], list):
            return f"{source}:{case_id}: invalid ring points"
    return None


def equivalent(left: Any, right: Any, abs_tol: float = 1e-8) -> bool:
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return math.isclose(left, right, rel_tol=0.0, abs_tol=abs_tol)
    if type(left) is not type(right):
        return False
    if isinstance(left, list):
        return len(left) == len(right) and all(
            equivalent(a, b, abs_tol) for a, b in zip(left, right)
        )
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(
            equivalent(left[key], right[key], 1e-6 if key == "area2" else abs_tol)
            for key in left
        )
    return left == right


def canonical_signature(value: Any) -> Any:
    if not isinstance(value, list) or not all(
        isinstance(record, dict) and {"depth", "area2", "points"} <= record.keys()
        for record in value
    ):
        return value

    def record_key(record: dict[str, Any]) -> tuple[Any, ...]:
        return (
            record["depth"],
            round(record["area2"], 6),
            json.dumps(record["points"], sort_keys=True, separators=(",", ":")),
        )

    return sorted(value, key=record_key)


def compare(
    workload: Path | None,
    left_path: Path,
    right_path: Path,
) -> int:
    try:
        left_header, left = load_results(left_path)
        right_header, right = load_results(right_path)
        expected = load_expected_ids(workload) if workload else None
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
            left_signature = canonical_signature(json.loads(left_record["signature"]))
            right_signature = canonical_signature(json.loads(right_record["signature"]))
            same = equivalent(left_signature, right_signature)
            if not same:
                error = "canonical signatures differ"
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
