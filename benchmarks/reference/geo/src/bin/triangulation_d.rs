use std::{env, fs};

use knipsa_geo_reference::benchmark_protocol::{
    MIN_SAMPLE_TIME_NS, SAMPLE_RUNS, WARMUP_RUNS, measure,
};
use knipsa_geo_reference::triangulation_reference::{CoordinateFrame, polygons, triangulate};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Workload {
    schema: String,
    coordinate_type: String,
    cases: Vec<WorkloadCase>,
}

#[derive(Deserialize)]
struct WorkloadCase {
    id: String,
    paths: Vec<Vec<[f64; 2]>>,
}

#[derive(Serialize)]
struct BenchResult {
    id: String,
    status: &'static str,
    error: Option<String>,
    median_ns: u128,
    p95_ns: u128,
    iterations_per_sample: usize,
    triangle_count: usize,
    signature: String,
}

fn main() {
    let path = env::args().nth(1).expect("usage: knipsa-geo-triangulation-d <workloads.json>");
    let workload: Workload =
        serde_json::from_str(&fs::read_to_string(path).expect("read workload JSON"))
            .expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-triangulation-d-workload-v1");
    assert_eq!(workload.coordinate_type, "f64");
    println!(
        "{{\"implementation\":\"geo-spade-cdt-f64\",\"revision\":\"geo-0.33.1-spade-2.15.1\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );

    for case in workload.cases {
        let result = CoordinateFrame::from_paths(&case.paths).and_then(|frame| {
            let normalized = frame.normalize_paths(&case.paths);
            polygons(&normalized).and_then(|polygons| {
                measure(|| {
                    triangulate(&polygons).map(|triangles| frame.restore_triangles(triangles))
                })
            })
        });
        let record = match result {
            Ok(measured) => BenchResult {
                id: case.id,
                status: "ok",
                error: None,
                median_ns: measured.median_ns,
                p95_ns: measured.p95_ns,
                iterations_per_sample: measured.iterations_per_sample,
                triangle_count: measured.output.len(),
                signature: serde_json::to_string(&measured.output)
                    .expect("triangles are serializable"),
            },
            Err(error) => BenchResult {
                id: case.id,
                status: "error",
                error: Some(error),
                median_ns: 0,
                p95_ns: 0,
                iterations_per_sample: 0,
                triangle_count: 0,
                signature: "[]".to_string(),
            },
        };
        println!("{}", serde_json::to_string(&record).expect("record is serializable"));
    }
}
