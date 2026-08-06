#!/usr/bin/env python3
"""Generate deterministic multi-outline workloads for scaling comparisons."""

import argparse
import json
from pathlib import Path


def rectangle(index: int) -> list[list[float]]:
    x = float(index * 6)
    y = float((index % 3) * 2)
    return [[x, y], [x + 10.0, y], [x + 10.0, y + 12.0], [x, y + 12.0]]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    cases = []
    for count in (4, 8, 16, 32):
        cases.append(
            {
                "id": f"overlap-chain-{count}-union",
                "clip_type": "union",
                "fill_rule": "non_zero",
                "subjects": [rectangle(index) for index in range(count)],
                "clips": [],
            }
        )
    workload = {"schema": "knipsa-workload-v1", "coordinate_type": "f64", "cases": cases}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(workload, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
