# Exact noding and shared-index measurements

Measured September 6, 2026. Baseline is merged #14,
`ad5383f99df6501b1abfc3e8beb3f1ba57a108d5`. Measured candidate is
`cd015cce120dfef9947a0600a66e03e9165941b5`: shared spatial noding, exact native
bounds before rank compression, and the adjacent-corner certificate. Subsequent
formatting/documentation changes do not change this production logic. These
are gains relative to #14, not another comparison with its older baseline.

## Decision and scope

Adopt the shared two-dimensional CPU index for general closed noding. Horizontal
comb cases improve with input size, including fractional input that requires
rank compression. This does not establish a CPU-versus-GPU winner: no physical
GPU benchmark was available. The [execution design](execution-design.md) specifies
a conditional GPU experiment; no GPU backend is implemented or enabled.

Not every case improves. The 2048-vertex vertical comb is about 6.3% slower with
integral input and 8.1% slower with fractional input by paired medians. Every pair
shows those regressions; they are not dismissed as noise. Smaller integral rotated
cases benefit from skipping unnecessary rank sorting. Fractional rotated input
still pays for ranking. The full table, including regressions, is the basis for
accepting this tradeoff, not a universal speedup claim.

## Method

Five alternating base/head process pairs per suite, 21 calibrated batches per
fixture. The same harness is compiled into both revisions. Every process checks
complete analytic geometry before timing; the script also compares complete
canonical output signatures between revisions. Floating output uses f64 bit
sequences, with signed zeros normalized. Winding and coordinates are retained;
only ring order and start vertex are canonicalized. Integer output compares all
coordinate pairs. Equality is not reduced to area or vertex counts.

Timings include public validation, preparation, allocation, intersections,
topology, and result destruction. Fixture creation and output comparison are
outside the timed interval. Ratios are medians of paired process medians, not
ratios of the displayed time medians. Pair ranges are not confidence intervals;
batch latency is not per-request p99. Allocation counts, candidate-stage fractions,
GPU throughput, real GIS distributions, and cross-library rankings were not measured.

Both runners report AMD EPYC 7763 64-Core Processor, Linux x86_64, Rust 1.98.1,
LLVM 22.1.8, default release profile/features, empty RUSTFLAGS, and identical
Cargo.lock hashes. Each suite compares revisions on the same runner; the two
suites are separate jobs. The CPU model name does not imply parallel execution.
Artifacts retain raw samples, signatures, iterations, execution order, metadata,
and harness/lock hashes.

## Complete Boolean intersection

[Run 34035872277](https://github.com/Mik-pe/knipsa/actions/runs/34035872277).
Comb vertex counts name the subject; each case also has a four-vertex clip.
Coordinates beyond the specialization range force the exact kernel. Fractional
comb inputs translate both operands and their analytic expected result by an
exact binary quarter, exercising rank preparation rather than native extraction.
The triangle separately checks construction of a nonintegral output vertex.

| Case | Base ns/op | Head ns/op | Median base/head | Pair range |
| --- | ---: | ---: | ---: | ---: |
| horizontal_comb_32_vertices | 279379.7 | 258210.4 | 1.083x | 1.074–1.088x |
| vertical_comb_32_vertices | 350924.7 | 340566.1 | 1.033x | 1.024–1.037x |
| horizontal_comb_256_vertices | 3620510.5 | 2293877.0 | 1.579x | 1.574–1.596x |
| vertical_comb_256_vertices | 2823236.5 | 2798251.5 | 1.009x | 1.005–1.014x |
| horizontal_comb_1024_vertices | 31376334.0 | 10554021.0 | 2.974x | 2.966–2.982x |
| vertical_comb_1024_vertices | 12724372.0 | 11543102.0 | 1.104x | 1.093–1.108x |
| horizontal_comb_2048_vertices | 111175391.0 | 22505308.0 | 4.937x | 4.893–4.978x |
| vertical_comb_2048_vertices | 24350174.0 | 25925562.0 | 0.941x | 0.937–0.945x |
| fractional_horizontal_comb_256_vertices | 3548062.0 | 2373035.2 | 1.495x | 1.489–1.497x |
| fractional_vertical_comb_256_vertices | 2856914.0 | 2889388.0 | 0.987x | 0.986–0.994x |
| fractional_horizontal_comb_2048_vertices | 108092167.0 | 23211240.0 | 4.647x | 4.624–4.697x |
| fractional_vertical_comb_2048_vertices | 24601618.0 | 26703068.0 | 0.925x | 0.918–0.929x |
| dense_grid_4 | 467516.1 | 462789.7 | 1.011x | 1.009–1.016x |
| dense_grid_8 | 1868407.0 | 1839877.0 | 1.014x | 1.009–1.020x |
| exact_fractional_triangle | 44876.4 | 44223.2 | 1.014x | 1.007–1.017x |
| exact_rectangle_control | 52923.9 | 52425.3 | 1.011x | 1.006–1.016x |

## Existing integer certificate consumer

[Run 34035872288](https://github.com/Mik-pe/knipsa/actions/runs/34035872288).
The shared index's original consumer is included rather than assuming extraction
has no cost. The unrelated overlap control remains in the suite.

| Case | Base ns/op | Head ns/op | Median base/head | Pair range |
| --- | ---: | ---: | ---: | ---: |
| rectangle_4 | 237.2 | 176.7 | 1.343x | 1.340–1.372x |
| convex_66 | 6335.6 | 4042.7 | 1.571x | 1.553–1.579x |
| convex_258 | 29431.0 | 19381.2 | 1.517x | 1.499–1.530x |
| convex_1026 | 134128.3 | 93131.1 | 1.439x | 1.437–1.463x |
| convex_2050 | 306194.9 | 220712.1 | 1.386x | 1.373–1.401x |
| separated_rectangles_8 | 1697.4 | 1281.1 | 1.328x | 1.314–1.337x |
| separated_rectangles_64 | 15175.1 | 12593.5 | 1.205x | 1.203–1.211x |
| separated_rectangles_256 | 58235.5 | 48382.5 | 1.204x | 1.200–1.207x |
| concave_disjoint_supports_8 | 400.4 | 279.7 | 1.433x | 1.420–1.440x |
| overlapping_rectangle_control | 831.4 | 761.7 | 1.089x | 1.074–1.134x |

## Reproduce and locate evidence

```sh
python3 scripts/benchmark-revisions.py --workload exact-noding \
  --base ad5383f99df6501b1abfc3e8beb3f1ba57a108d5 \
  --head cd015cce120dfef9947a0600a66e03e9165941b5 --pairs 5
python3 scripts/benchmark-revisions.py --workload direct-certification \
  --base ad5383f99df6501b1abfc3e8beb3f1ba57a108d5 \
  --head cd015cce120dfef9947a0600a66e03e9165941b5 --pairs 5
```

- Noding artifact ID `9990164301`, ZIP SHA-256
  `46b5de8891ad18ebddb9ea92d6daa50eae6a5d80807b080f1e0a450a3334a441`.
- Certificate artifact ID `9990149606`, ZIP SHA-256
  `0234448262eb453d0c9bb6d163ab0b097df4fede63a44b8836dd868dcf190d5d`.

Earlier experiments are not pooled with these samples. The initial extraction
at `8126700` improved the large horizontal comb about 5x but slowed separated-ring
certification. The adjacent-corner reduction at `3951d9f` removed that observed
regression. Always-ranking revisions still penalized small exact controls and
some rotated cases; checked native bounds were introduced before this final
integral/fractional comparison. The remaining large rotated regressions above
are explicitly retained.

The unchanged strict coverage, fuzz replay, compiler/MSRV checks, C ABI,
packaged consumers, and all reference matrices remain CI gates. The PR records
the latest head and its full check status separately from these pinned timings.
