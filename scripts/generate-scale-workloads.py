#!/usr/bin/env python3
"""Generate deterministic outline-count and vertex-count scaling profiles."""

import argparse
import json
import math
from pathlib import Path


OUTLINE_COUNTS = (4, 8, 16, 32, 64, 128)
VERTEX_COUNTS = (32, 64, 128, 256)


def overlap_rectangle(index: int) -> list[list[float]]:
    x = float(index * 6)
    y = float((index % 3) * 2)
    return [[x, y], [x + 10.0, y], [x + 10.0, y + 12.0], [x, y + 12.0]]


def disjoint_rectangle(index: int) -> list[list[float]]:
    columns = 16
    x = float((index % columns) * 14)
    y = float((index // columns) * 14)
    return [[x, y], [x + 10.0, y], [x + 10.0, y + 10.0], [x, y + 10.0]]


def regular_polygon(vertices: int, center_x: float, center_y: float) -> list[list[float]]:
    return [
        [
            round(center_x + 100.0 * math.cos(2.0 * math.pi * index / vertices), 9),
            round(center_y + 100.0 * math.sin(2.0 * math.pi * index / vertices), 9),
        ]
        for index in range(vertices)
    ]


def generate_cases() -> list[dict[str, object]]:
    cases: list[dict[str, object]] = []
    for count in OUTLINE_COUNTS:
        cases.extend(
            [
                {
                    "id": f"overlap-chain-{count}-union",
                    "clip_type": "union",
                    "fill_rule": "non_zero",
                    "subjects": [overlap_rectangle(index) for index in range(count)],
                    "clips": [],
                },
                {
                    "id": f"disjoint-grid-{count}-union",
                    "clip_type": "union",
                    "fill_rule": "non_zero",
                    "subjects": [disjoint_rectangle(index) for index in range(count)],
                    "clips": [],
                },
            ]
        )
    for vertices in VERTEX_COUNTS:
        subjects = [regular_polygon(vertices, 0.0, 0.0)]
        clips = [regular_polygon(vertices, 35.0, 5.0)]
        for operation in ("intersection", "xor"):
            cases.append(
                {
                    "id": f"convex-{vertices}-{operation}",
                    "clip_type": operation,
                    "fill_rule": "even_odd",
                    "subjects": subjects,
                    "clips": clips,
                }
            )
    return cases


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    workload = {
        "schema": "knipsa-workload-v1",
        "coordinate_type": "f64",
        "comparison": {"coordinate_tolerance": 1e-6, "area2_tolerance": 1e-4},
        "cases": generate_cases(),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(workload, separators=(",", ":")) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
