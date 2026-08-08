"""Shared fail-closed transport validation for benchmark JSONL adapters."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Mapping


class ResultFormatError(ValueError):
    """Raised when a workload or adapter result violates the transport protocol."""


def load_results(path: Path) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    """Load one adapter header and uniquely identified case records."""
    header: dict[str, Any] | None = None
    records: dict[str, dict[str, Any]] = {}
    saw_record = False

    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise ResultFormatError(f"{path}: cannot read adapter output: {error}") from error
    for line_number, line in enumerate(lines, 1):
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
    validate_header(header, path)
    if not records:
        raise ResultFormatError(f"{path}: contains no case records")
    return header, records


def validate_header(header: Mapping[str, Any], source: object) -> None:
    """Require an identified adapter and a calibrated sampling declaration."""
    implementation = header.get("implementation")
    if not isinstance(implementation, str) or not implementation:
        raise ResultFormatError(f"{source}: invalid adapter implementation")
    for field, minimum in (("samples", 1), ("warmups", 0), ("minimum_sample_time_ns", 1)):
        value = header.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
            raise ResultFormatError(f"{source}: invalid header field {field}={value!r}")


def load_expected_ids(path: Path) -> set[str]:
    """Load and validate the unique, non-empty case IDs owned by a workload."""
    try:
        workload = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ResultFormatError(f"{path}: cannot read workload: {error}") from error
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


def expected_id_errors(
    expected: set[str], named_records: Mapping[str, Mapping[str, object]]
) -> list[str]:
    """Describe missing and unexpected records for every adapter."""
    errors = []
    for name, records in named_records.items():
        actual = set(records)
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        if missing:
            errors.append(f"{name}: missing cases: {', '.join(missing)}")
        if extra:
            errors.append(f"{name}: unexpected cases: {', '.join(extra)}")
    return errors


def calibrated_timing_error(record: Mapping[str, Any], source: str, case_id: str) -> str | None:
    """Validate the common per-operation timing fields of a successful record."""
    for key in ("median_ns", "p95_ns"):
        value = record.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            return f"{source}:{case_id}: invalid non-negative integer {key}={value!r}"
    iterations = record.get("iterations_per_sample")
    if not isinstance(iterations, int) or isinstance(iterations, bool) or iterations < 1:
        return f"{source}:{case_id}: invalid positive integer iterations_per_sample={iterations!r}"
    if record["p95_ns"] < record["median_ns"]:
        return f"{source}:{case_id}: p95_ns is smaller than median_ns"
    return None
