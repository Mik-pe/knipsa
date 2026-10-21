#!/usr/bin/env python3
"""Compare two trusted revisions using identical, correctness-checked fixtures."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile


def command(args, cwd):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--workload", choices=["direct-certification", "exact-noding"], default="direct-certification")
    parser.add_argument("--pairs", type=int, default=5)
    parser.add_argument("--output", type=Path, default=None)
    args = parser.parse_args()
    if args.pairs < 1:
        parser.error("--pairs must be positive")
    root = Path(__file__).resolve().parents[1]
    source = root / "benchmarks" / (args.workload + ".rs")
    if args.output is None:
        args.output = Path("target") / (args.workload + ".json")
    revisions = {
        name: command(["git", "rev-parse", "--verify", "--end-of-options", ref + "^{commit}"], root)
        for name, ref in [("base", args.base), ("head", args.head)]
    }
    report = {
        "revisions": revisions,
        "workload": args.workload,
        "rustc": command(["rustc", "-Vv"], root),
        "platform": platform.platform(),
        "cpu": next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                     if line.startswith("model name")), "unknown") if Path("/proc/cpuinfo").exists() else platform.processor(),
        "rustflags": os.environ.get("RUSTFLAGS", ""),
        "profile": "cargo build --locked --release (default features)",
        "harness_sha256": digest(source),
        "lock_sha256": {},
        "process_pairs": [],
    }
    worktrees = []
    with tempfile.TemporaryDirectory(prefix="knipsa-paired-") as temporary:
        temporary = Path(temporary)
        binaries = {}
        try:
            for name, revision in revisions.items():
                tree = temporary / name
                subprocess.run(["git", "worktree", "add", "--detach", "--quiet", str(tree), revision],
                               cwd=root, check=True)
                worktrees.append(tree)
                example = tree / "crates/knipsa/examples/paired_revision_bench.rs"
                if example.exists():
                    raise RuntimeError(f"refusing to overwrite {example}")
                example.parent.mkdir(parents=True, exist_ok=True)
                example.write_bytes(source.read_bytes())
                report["lock_sha256"][name] = digest(tree / "Cargo.lock")
                target = temporary / (name + "-target")
                subprocess.run(["cargo", "build", "--locked", "--release", "-p", "knipsa", "--example",
                                "paired_revision_bench", "--target-dir", str(target)], cwd=tree, check=True)
                binaries[name] = target / "release/examples/paired_revision_bench"
            for pair in range(args.pairs):
                order = ["base", "head"] if pair % 2 == 0 else ["head", "base"]
                results = {}
                for name in order:
                    rows = [json.loads(line) for line in command([str(binaries[name])], root).splitlines()]
                    if not rows or len({row["case"] for row in rows}) != len(rows):
                        raise RuntimeError("missing or duplicate benchmark cases")
                    for row in rows:
                        if len(row["samples_ns"]) != 21 or any(not math.isfinite(x) or x <= 0 for x in row["samples_ns"]):
                            raise RuntimeError("invalid timing samples")
                    results[name] = {row["case"]: row for row in rows}
                if results["base"].keys() != results["head"].keys():
                    raise RuntimeError("different benchmark cases")
                for name, row in results["base"].items():
                    other = results["head"][name]
                    if row["signature"] != other["signature"] or row["input_vertices"] != other["input_vertices"]:
                        raise RuntimeError(f"geometry differs for {name}; no speed claim is valid")
                report["process_pairs"].append({"order": order, "results": results})
                print(f"Verified process pair {pair + 1}/{args.pairs}", file=sys.stderr, flush=True)
        finally:
            for tree in reversed(worktrees):
                subprocess.run(["git", "worktree", "remove", "--force", str(tree)], cwd=root, check=True)
    summary = []
    for case in report["process_pairs"][0]["results"]["base"]:
        base, head, ratios = [], [], []
        for pair in report["process_pairs"]:
            a = statistics.median(pair["results"]["base"][case]["samples_ns"])
            b = statistics.median(pair["results"]["head"][case]["samples_ns"])
            base.append(a)
            head.append(b)
            ratios.append(a / b)
        summary.append({"case": case, "base_ns": statistics.median(base), "head_ns": statistics.median(head),
                        "paired_speedup": statistics.median(ratios), "paired_min": min(ratios), "paired_max": max(ratios)})
    report["summary"] = summary
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"## {args.workload}: paired public-API benchmark\n")
    print(f"Base `{revisions['base']}`; head `{revisions['head']}`.\n")
    print(f"{args.pairs} alternating process pairs; 21 batches per case; exact output equality checked.\n")
    print("| Case | Base ns/op | Head ns/op | Median base/head | Pair range |")
    print("| --- | ---: | ---: | ---: | ---: |")
    for row in summary:
        print(f"| {row['case']} | {row['base_ns']:.1f} | {row['head_ns']:.1f} | {row['paired_speedup']:.3f}x | "
              f"{row['paired_min']:.3f}–{row['paired_max']:.3f}x |")
    print("\nIncludes validation, allocations, index construction, and output destruction. "
          "Synthetic targeted workloads, not a cross-library or universal performance claim.")


if __name__ == "__main__":
    main()
