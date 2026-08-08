use std::{convert::Infallible, env, fs};

use geo::algorithm::bool_ops::{FillRule, OpType};
use geo::{BooleanOps, Coord, LineString, MultiPolygon, Polygon};
use knipsa_geo_reference::benchmark_protocol::{
    MIN_SAMPLE_TIME_NS, SAMPLE_RUNS, WARMUP_RUNS, measure,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Workload {
    schema: String,
    cases: Vec<WorkloadCase>,
}

#[derive(Deserialize)]
struct WorkloadCase {
    id: String,
    clip_type: String,
    fill_rule: String,
    subjects: Vec<Vec<[f64; 2]>>,
    clips: Vec<Vec<[f64; 2]>>,
}

#[derive(Serialize)]
struct BenchResult {
    id: String,
    status: &'static str,
    error: Option<String>,
    median_ns: u128,
    p95_ns: u128,
    iterations_per_sample: usize,
    ring_count: usize,
    signature: String,
}

fn main() {
    let path = env::args().nth(1).expect("usage: knipsa-geo-reference <workloads.json>");
    let workload: Workload =
        serde_json::from_str(&fs::read_to_string(path).expect("read workload JSON"))
            .expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-workload-v1");
    println!(
        "{{\"implementation\":\"geo-i-overlay\",\"revision\":\"geo-0.33.1\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );

    for case in workload.cases {
        let subjects = multi_polygon(case.subjects);
        let clips = multi_polygon(case.clips);
        let operation = operation(&case.clip_type);
        let fill_rule = fill_rule(&case.fill_rule);

        let measured = measure(|| {
            Ok::<_, Infallible>(subjects.boolean_op_with_fill_rule(&clips, operation, fill_rule))
        })
        .expect("the geo Boolean operation is infallible");

        println!(
            "{}",
            serde_json::to_string(&BenchResult {
                id: case.id,
                status: "ok",
                error: None,
                median_ns: measured.median_ns,
                p95_ns: measured.p95_ns,
                iterations_per_sample: measured.iterations_per_sample,
                ring_count: ring_count(&measured.output),
                signature: signature(&measured.output),
            })
            .expect("serializable benchmark result")
        );
    }
}

fn multi_polygon(paths: Vec<Vec<[f64; 2]>>) -> MultiPolygon<f64> {
    MultiPolygon::new(
        paths
            .into_iter()
            .filter(|path| path.len() >= 3)
            .map(|path| {
                let mut coordinates =
                    path.into_iter().map(|[x, y]| Coord { x, y }).collect::<Vec<_>>();
                if coordinates.first() != coordinates.last() {
                    coordinates.push(coordinates[0]);
                }
                Polygon::new(LineString::new(coordinates), Vec::new())
            })
            .collect(),
    )
}

fn operation(value: &str) -> OpType {
    match value {
        "intersection" => OpType::Intersection,
        "union" => OpType::Union,
        "difference" => OpType::Difference,
        "xor" => OpType::Xor,
        other => panic!("unknown clip type {other}"),
    }
}

fn fill_rule(value: &str) -> FillRule {
    match value {
        "even_odd" => FillRule::EvenOdd,
        "non_zero" => FillRule::NonZero,
        "positive" => FillRule::Positive,
        "negative" => FillRule::Negative,
        other => panic!("unknown fill rule {other}"),
    }
}

fn rings(paths: &MultiPolygon<f64>) -> Vec<Vec<[f64; 2]>> {
    paths
        .iter()
        .flat_map(|polygon| std::iter::once(polygon.exterior()).chain(polygon.interiors()))
        .map(|ring| {
            let mut points = ring.coords().map(|point| [point.x, point.y]).collect::<Vec<_>>();
            if points.len() > 1 && points.first() == points.last() {
                points.pop();
            }
            points
        })
        .filter(|ring| !ring.is_empty())
        .collect()
}

fn ring_count(paths: &MultiPolygon<f64>) -> usize {
    paths.iter().map(|polygon| 1 + polygon.interiors().len()).sum()
}

fn signature(paths: &MultiPolygon<f64>) -> String {
    serde_json::to_string(&rings(paths)).expect("serializable signature")
}
