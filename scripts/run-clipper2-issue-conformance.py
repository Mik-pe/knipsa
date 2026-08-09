#!/usr/bin/env python3
"""Run issue-derived Knipsa fixtures against the current Clipper2 reference."""

from __future__ import annotations

import json
import pathlib
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests" / "fixtures" / "clipper2-issue-corpus-v1.json"
OUTPUT = ROOT / "target" / "conformance-clipper-issues"


def main() -> None:
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    cases = corpus["cases"]
    profiles = {
        "boolean": boolean_workload(cases),
        "open": open_workload(cases),
        "offset": offset_workload(cases),
        "triangulation": triangulation_workload(cases),
    }

    with tempfile.TemporaryDirectory(prefix="knipsa-clipper-issues-") as directory:
        temporary = pathlib.Path(directory)
        for name, workload in profiles.items():
            path = temporary / f"{name}.json"
            path.write_text(json.dumps(workload, separators=(",", ":")) + "\n", encoding="utf-8")
            run_profile(name, path)


def selected(cases: list[dict[str, object]], operation: str) -> list[dict[str, object]]:
    return [
        case
        for case in cases
        if case["operation"] == operation
        and case.get("reference_comparison") != "invariant_only"
    ]


def public_case(case: dict[str, object], fields: tuple[str, ...]) -> dict[str, object]:
    return {field: case[field] for field in fields}


def boolean_workload(cases: list[dict[str, object]]) -> dict[str, object]:
    fields = ("id", "clip_type", "fill_rule", "subjects", "clips")
    return {
        "schema": "knipsa-workload-v1",
        "coordinate_type": "i64",
        "comparison": {"coordinate_tolerance": 0, "area2_tolerance": 0},
        "cases": [public_case(case, fields) for case in selected(cases, "boolean")],
    }


def open_workload(cases: list[dict[str, object]]) -> dict[str, object]:
    fields = ("id", "clip_type", "fill_rule", "open_subjects", "clips")
    workload_cases = []
    for case in selected(cases, "open_boolean"):
        converted = public_case(case, fields)
        converted["closed_subjects"] = []
        workload_cases.append(converted)
    return {
        "schema": "knipsa-open-workload-v1",
        "coordinate_type": "i64",
        "comparison": {"coordinate_tolerance": 0},
        "cases": workload_cases,
    }


def offset_workload(cases: list[dict[str, object]]) -> dict[str, object]:
    fields = ("id", "paths", "delta", "join_type", "end_type")
    workload_cases = []
    for case in selected(cases, "offset"):
        converted = public_case(case, fields)
        converted.update(
            miter_limit=2.0,
            arc_tolerance=0.02,
            preserve_collinear=False,
            boundary_tolerance=0.05,
            area_tolerance=0.2,
        )
        workload_cases.append(converted)
    return {"schema": "knipsa-offset-workload-v1", "cases": workload_cases}


def triangulation_workload(cases: list[dict[str, object]]) -> dict[str, object]:
    fields = ("id", "paths")
    return {
        "schema": "knipsa-triangulation-workload-v1",
        "coordinate_type": "i64",
        "cases": [public_case(case, fields) for case in selected(cases, "triangulate")],
    }


def run_profile(name: str, workload: pathlib.Path) -> None:
    scripts = {
        "boolean": "run-conformance.sh",
        "open": "run-open-conformance.sh",
        "offset": "run-offset-conformance.sh",
        "triangulation": "run-triangulation-conformance.sh",
    }
    arguments = [
        str(ROOT / "scripts" / scripts[name]),
        str(workload),
        str(OUTPUT / name),
    ]
    if name == "boolean":
        arguments.append("clipper2-integer")
    subprocess.run(arguments, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
