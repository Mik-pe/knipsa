#!/usr/bin/env python3
"""Generate the deterministic offset scale/concentration workload."""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path


def regular_polygon(vertices: int, radius: float, center_x: float = 0.0) -> list[list[float]]:
    return [
        [
            round(center_x + radius * math.cos(2.0 * math.pi * index / vertices), 12),
            round(radius * math.sin(2.0 * math.pi * index / vertices), 12),
        ]
        for index in range(vertices)
    ]


def reflex_polygon(vertices: int, outer: float = 100.0, inner: float = 85.0) -> list[list[float]]:
    return [
        [
            round((outer if index % 2 == 0 else inner) * math.cos(2.0 * math.pi * index / vertices), 12),
            round((outer if index % 2 == 0 else inner) * math.sin(2.0 * math.pi * index / vertices), 12),
        ]
        for index in range(vertices)
    ]


def rectangles(count: int, spacing: float) -> list[list[list[float]]]:
    return [
        [[index * spacing, 0.0], [index * spacing + 20.0, 0.0],
         [index * spacing + 20.0, 20.0], [index * spacing, 20.0]]
        for index in range(count)
    ]


def retrace(vertices: int) -> list[list[float]]:
    path = [[-100.0, 0.0], [100.0, 0.0]]
    for index in range(vertices - 2):
        distance = 99.0 - index // 2
        path.append([distance if index % 2 else -distance, 0.0])
    return path


def case(identifier: str, paths: list[list[list[float]]], *, delta: float = 3.0,
         join: str = "miter", end: str = "polygon", arc: float = 0.02,
         boundary: float = 0.000001, area: float = 0.0001) -> dict[str, object]:
    return {
        "id": identifier,
        "paths": paths,
        "delta": delta,
        "join_type": join,
        "end_type": end,
        "miter_limit": 2.0,
        "arc_tolerance": arc,
        "preserve_collinear": False,
        "boundary_tolerance": boundary,
        "area_tolerance": area,
    }


def workload() -> dict[str, object]:
    cases: list[dict[str, object]] = []
    for vertices in (16, 64, 256):
        cases.append(case(f"vertices-convex-{vertices}", [regular_polygon(vertices, 100.0)]))
        cases.append(case(f"vertices-reflex-{vertices}", [reflex_polygon(vertices)], delta=1.0))
    for count in (1, 16, 64):
        cases.append(case(f"contours-disjoint-{count}", rectangles(count, 30.0)))
    for count in (8, 32):
        cases.append(case(f"overlap-sparse-{count}", rectangles(count, 18.0)))
        cases.append(case(f"overlap-dense-{count}", rectangles(count, 4.0)))
    for tolerance in (0.5, 0.05, 0.005):
        label = str(tolerance).replace(".", "p")
        cases.append(case(
            f"arc-tolerance-{label}", [regular_polygon(32, 100.0)], join="round", arc=tolerance,
            boundary=max(0.02, tolerance * 2.5), area=max(0.5, tolerance * 80.0),
        ))
    for vertices in (16, 128, 1024):
        cases.append(case(
            f"retrace-{vertices}", [retrace(vertices)], delta=2.0, join="round", end="round",
            boundary=0.08, area=3.0,
        ))
    return {"schema": "knipsa-offset-workload-v1", "cases": cases}


def known_gaps() -> dict[str, object]:
    return {
        "schema": "knipsa-offset-workload-v1",
        "cases": [case("known-gap-reflex-228", [reflex_polygon(228, inner=65.0)])],
    }


def main() -> None:
    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[1] != "--known-gaps"):
        raise SystemExit(
            "usage: generate-offset-scale-workload.py [--known-gaps] <output.json>"
        )
    output = Path(sys.argv[-1])
    data = known_gaps() if len(sys.argv) == 3 else workload()
    output.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
