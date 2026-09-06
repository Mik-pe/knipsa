# Exact noding and shared-index measurements

Measured September 6, 2026. This report compares the merged #14 baseline
`ad5383f99df6501b1abfc3e8beb3f1ba57a108d5` with code commit
`3951d9ff223bfaa9d72dc1f7a6f998897da8b614` from #17. Documentation-only
follow-ups do not change the measured source. These are new gains relative to
#14, not another comparison with the older quadratic direct-ring certificate.

## Decision and limits

Adopt the shared two-dimensional CPU index for general closed noding. The
large horizontal-comb pathology improves about 5x end to end. This does not
establish that the CPU wins against GPU hardware: no physical GPU benchmark was
available. GPU remains a proposed candidate-generation stage under the admission
criteria in [the execution design](execution-design.md), not an implemented or
enabled backend.

The change is not uniformly faster. The vertical 256-vertex comb has a median
base/head ratio of 0.960x, or about 4.2% more latency, with all five pairs showing
the regression. This is an accepted, explicit tradeoff for two-axis pruning,
not dismissed as noise. The 2048-vertex rotated control is approximately flat.
Dense intersections and tiny exact calls are roughly unchanged in this run.
Integer certification benefits from the shared-index layout and the checked
adjacent-corner reduction. No broad claim about unrelated workloads follows.

## Method

Each suite runs five alternating base/head process pairs. Each process verifies
the complete analytic output and measures 21 calibrated batches per fixture.
The same source harness is compiled into both revisions. Floating noding outputs
are compared using full canonical f64 bit sequences, not area or vertex counts;
integer outputs compare full coordinate sequences. Ring order and start vertex
are canonicalized, but winding and coordinates are retained.

Timing includes public validation, allocations, exact rank preparation, index
construction, exact intersections, topology, and result destruction. Fixture
construction and output verification are outside the measured interval. Ratios
are medians of paired process medians; the displayed ranges are not confidence
intervals. Batch latency is not per-request p99. Allocation counts, resident GPU
throughput, real-world GIS distributions, and cross-library rankings were not
measured here.

Both runners report AMD EPYC 7763 64-Core Processor, Linux x86_64, Rust 1.98.1,
LLVM 22.1.8, default release profile/features, empty RUSTFLAGS, and identical
Cargo.lock hashes. Each comparison is within one runner; the two suites are
separate jobs. The CPU model name does not imply 64-core parallel execution.
The full machine/compiler metadata, samples, signatures, iteration counts,
execution order, and hashes are retained in the artifacts.

## Full Boolean intersection

[Actions run 34035265586](https://github.com/Mik-pe/knipsa/actions/runs/34035265586).
Comb vertex counts name the subject only; each case also has a four-vertex clip.
Both axes are tested. Original coordinates beyond the specialization range force
the exact kernel. The fractional triangle checks a constructed nonintegral vertex.

| Case | Base ns/op | Head ns/op | Median base/head | Pair range |
| --- | ---: | ---: | ---: | ---: |
| horizontal_comb_32_vertices | 270049.0 | 257542.2 | 1.050x | 1.043–1.060x |
| vertical_comb_32_vertices | 345054.8 | 350226.4 | 0.987x | 0.983–0.996x |
| horizontal_comb_256_vertices | 3464173.5 | 2460697.2 | 1.416x | 1.404–1.422x |
| vertical_comb_256_vertices | 2804667.5 | 2929123.0 | 0.960x | 0.947–0.963x |
| horizontal_comb_1024_vertices | 30463103.0 | 11076333.0 | 2.768x | 2.708–2.778x |
| vertical_comb_1024_vertices | 12580885.0 | 12732183.0 | 0.993x | 0.980–1.000x |
| horizontal_comb_2048_vertices | 112492340.0 | 22152156.0 | 5.078x | 5.002–5.215x |
| vertical_comb_2048_vertices | 26454016.0 | 26354671.0 | 0.997x | 0.995–1.009x |
| dense_grid_4 | 453794.1 | 453932.4 | 0.996x | 0.994–1.006x |
| dense_grid_8 | 1774791.0 | 1735974.0 | 1.022x | 1.019–1.040x |
| exact_fractional_triangle | 44800.5 | 44752.2 | 1.004x | 0.989–1.018x |
| exact_rectangle_control | 52116.0 | 52641.2 | 0.993x | 0.984–1.015x |

## Existing integer certificate consumer

[Actions run 34035265550](https://github.com/Mik-pe/knipsa/actions/runs/34035265550).
This guards the actual existing user of the extracted index. The initial extraction
at `8126700` slowed separated-ring cases; the corner reduction at `3951d9f`
removed that observed regression. The unrelated overlap control is retained.

| Case | Base ns/op | Head ns/op | Median base/head | Pair range |
| --- | ---: | ---: | ---: | ---: |
| rectangle_4 | 262.3 | 231.0 | 1.148x | 1.113–1.172x |
| convex_66 | 6780.6 | 4184.0 | 1.622x | 1.615–1.625x |
| convex_258 | 31395.5 | 20020.0 | 1.568x | 1.559–1.575x |
| convex_1026 | 142712.9 | 96095.4 | 1.488x | 1.480–1.488x |
| convex_2050 | 326792.4 | 228775.0 | 1.429x | 1.385–1.430x |
| separated_rectangles_8 | 1792.3 | 1401.5 | 1.273x | 1.225–1.296x |
| separated_rectangles_64 | 16275.7 | 13579.7 | 1.201x | 1.176–1.205x |
| separated_rectangles_256 | 62884.6 | 52603.8 | 1.197x | 1.176–1.200x |
| concave_disjoint_supports_8 | 435.6 | 317.5 | 1.373x | 1.366–1.398x |
| overlapping_rectangle_control | 791.8 | 791.7 | 1.007x | 0.942–1.012x |

## Reproduce and locate evidence

```sh
python3 scripts/benchmark-revisions.py --workload exact-noding \
  --base ad5383f99df6501b1abfc3e8beb3f1ba57a108d5 \
  --head 3951d9ff223bfaa9d72dc1f7a6f998897da8b614 --pairs 5
python3 scripts/benchmark-revisions.py --workload direct-certification \
  --base ad5383f99df6501b1abfc3e8beb3f1ba57a108d5 \
  --head 3951d9ff223bfaa9d72dc1f7a6f998897da8b614 --pairs 5
```

- Noding artifact ID `9989970331`, ZIP SHA-256
  `0fc4574865e064ae4bed80dbf209f8c395a588c77f4087b9a5ffce213370cd30`.
- Certificate artifact ID `9989962020`, ZIP SHA-256
  `f00a5ad3468ef47e064985d1165dc279fb1529bf22b35d21e764c1f3d829cbf1`.
- Earlier noding comparison: run `34035046724`, head `8126700`, about 5.028x
  on the large horizontal comb. It is a separate earlier result, not pooled
  with the final code's process pairs.

The unchanged strict coverage, fuzz replay, compiler/MSRV checks, C ABI,
packaged consumers, and all reference conformance matrices remain CI gates.
The complete run for the measured source is
[34035265563](https://github.com/Mik-pe/knipsa/actions/runs/34035265563).
The PR records the status of the latest documentation-inclusive head.
