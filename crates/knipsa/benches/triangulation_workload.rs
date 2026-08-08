#![allow(missing_docs)]

use std::{env, fs};

use knipsa::{ComplexityLimits, FillRule, Path64, Point64, triangulate64};
use serde::{Deserialize, Serialize};

#[path = "support/benchmark_protocol.rs"]
mod benchmark_protocol;
use benchmark_protocol::{MIN_SAMPLE_TIME_NS, SAMPLE_RUNS, WARMUP_RUNS, measure};

const WORKLOAD: &str = include_str!("../../../benchmarks/triangulation-workloads.json");

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
    status: String,
    error: Option<String>,
    median_ns: u128,
    p95_ns: u128,
    iterations_per_sample: usize,
    triangle_count: usize,
    signature: String,
}

fn main() {
    let workload_json = env::var_os("KNIPSA_TRIANGULATION_WORKLOAD").map_or_else(
        || WORKLOAD.to_owned(),
        |path| fs::read_to_string(path).expect("read KNIPSA_TRIANGULATION_WORKLOAD"),
    );
    let workload: Workload = serde_json::from_str(&workload_json).expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-triangulation-workload-v1");
    assert_eq!(workload.coordinate_type, "i64");
    println!(
        "{{\"implementation\":\"knipsa-triangulate64\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );

    for case in workload.cases {
        let paths = case
            .paths
            .into_iter()
            .map(|path| path.into_iter().map(|[x, y]| Point64::new(x, y)).collect::<Path64>())
            .collect::<Vec<_>>();
        match measure(|| triangulate64(&paths, FillRule::NonZero, ComplexityLimits::DEFAULT)) {
            Ok(measured) => {
                let signature = measured
                    .output
                    .iter()
                    .map(|triangle| triangle.map(|point| [point.x, point.y]))
                    .collect::<Vec<_>>();
                print_result(&BenchResult {
                    id: case.id,
                    status: "ok".to_owned(),
                    error: None,
                    median_ns: measured.median_ns,
                    p95_ns: measured.p95_ns,
                    iterations_per_sample: measured.iterations_per_sample,
                    triangle_count: signature.len(),
                    signature: serde_json::to_string(&signature)
                        .expect("serializable triangle signature"),
                });
            }
            Err(error) => print_result(&BenchResult {
                id: case.id,
                status: "error".to_owned(),
                error: Some(error.to_string()),
                median_ns: 0,
                p95_ns: 0,
                iterations_per_sample: 0,
                triangle_count: 0,
                signature: "[]".to_owned(),
            }),
        }
    }
}

fn print_result(result: &BenchResult) {
    println!("{}", serde_json::to_string(&result).expect("serializable result"));
}
