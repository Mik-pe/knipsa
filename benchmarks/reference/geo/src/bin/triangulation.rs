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
    paths: Vec<Vec<[i64; 2]>>,
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
    let path = env::args().nth(1).expect("usage: knipsa-geo-triangulation <workloads.json>");
    let workload: Workload =
        serde_json::from_str(&fs::read_to_string(path).expect("read workload JSON"))
            .expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-triangulation-workload-v1");
    assert_eq!(workload.coordinate_type, "i64");
    println!(
        "{{\"implementation\":\"geo-spade-cdt\",\"revision\":\"geo-0.33.1-spade-2.15.1\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );

    for case in workload.cases {
        let paths = case
            .paths
            .iter()
            .map(|path| path.iter().map(|[x, y]| [*x as f64, *y as f64]).collect())
            .collect::<Vec<_>>();
        let result = CoordinateFrame::translation_only(&paths).and_then(|frame| {
            let normalized = frame.normalize_paths(&paths);
            polygons(&normalized).and_then(|polygons| {
                measure(|| {
                    triangulate(&polygons).and_then(|triangles| {
                        frame
                            .restore_triangles(triangles)
                            .into_iter()
                            .map(integer_triangle)
                            .collect::<Result<Vec<_>, _>>()
                    })
                })
                .map_err(|error| error.to_string())
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

fn integer_triangle(triangle: [[f64; 2]; 3]) -> Result<[[i64; 2]; 3], String> {
    Ok([
        integer_coordinate(triangle[0])?,
        integer_coordinate(triangle[1])?,
        integer_coordinate(triangle[2])?,
    ])
}

fn integer_coordinate([x, y]: [f64; 2]) -> Result<[i64; 2], String> {
    if !x.is_finite()
        || !y.is_finite()
        || x.fract() != 0.0
        || y.fract() != 0.0
        || x < i64::MIN as f64
        || x > i64::MAX as f64
        || y < i64::MIN as f64
        || y > i64::MAX as f64
    {
        return Err("triangulator returned a non-integer coordinate".to_string());
    }
    Ok([x as i64, y as i64])
}
