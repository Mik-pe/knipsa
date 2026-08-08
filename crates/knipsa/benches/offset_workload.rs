#![allow(missing_docs)]

use std::{env, fs};

use knipsa::{EndType, JoinType, OffsetOptions, PathD, PointD, offset_paths_d};
use serde::{Deserialize, Serialize};

#[path = "support/benchmark_protocol.rs"]
mod benchmark_protocol;
use benchmark_protocol::{MIN_SAMPLE_TIME_NS, SAMPLE_RUNS, WARMUP_RUNS, measure};

const WORKLOAD: &str = include_str!("../../../benchmarks/offset-workloads.json");

#[derive(Deserialize)]
struct Workload {
    schema: String,
    cases: Vec<WorkloadCase>,
}

#[derive(Deserialize)]
struct WorkloadCase {
    id: String,
    paths: Vec<Vec<[f64; 2]>>,
    delta: f64,
    join_type: String,
    end_type: String,
    miter_limit: f64,
    arc_tolerance: f64,
    preserve_collinear: bool,
}

#[derive(Serialize)]
struct BenchResult {
    id: String,
    status: String,
    error: Option<String>,
    median_ns: u128,
    p95_ns: u128,
    iterations_per_sample: usize,
    ring_count: usize,
    signature: String,
}

fn main() {
    let workload_json = env::var_os("KNIPSA_OFFSET_WORKLOAD").map_or_else(
        || WORKLOAD.to_owned(),
        |path| fs::read_to_string(path).expect("read KNIPSA_OFFSET_WORKLOAD"),
    );
    let workload: Workload = serde_json::from_str(&workload_json).expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-offset-workload-v1");
    println!(
        "{{\"implementation\":\"knipsa-offset\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );
    for case in workload.cases {
        let paths = case
            .paths
            .into_iter()
            .map(|path| path.into_iter().map(|[x, y]| PointD::new(x, y)).collect::<PathD>())
            .collect::<Vec<_>>();
        let options = OffsetOptions {
            join_type: join_type(&case.join_type),
            end_type: end_type(&case.end_type),
            miter_limit: case.miter_limit,
            arc_tolerance: case.arc_tolerance,
            preserve_collinear: case.preserve_collinear,
        };
        let measured = match measure(|| {
            offset_paths_d(&paths, case.delta, options).map_err(|error| error.to_string())
        }) {
            Ok(measured) => measured,
            Err(error) => {
                print_error(case.id, error);
                continue;
            }
        };
        let result = BenchResult {
            id: case.id,
            status: "ok".to_owned(),
            error: None,
            median_ns: measured.median_ns,
            p95_ns: measured.p95_ns,
            iterations_per_sample: measured.iterations_per_sample,
            ring_count: measured.output.len(),
            signature: serde_json::to_string(
                &measured
                    .output
                    .iter()
                    .map(|path| path.iter().map(|point| [point.x, point.y]).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            )
            .expect("serializable signature"),
        };
        println!("{}", serde_json::to_string(&result).expect("serializable result"));
    }
}

fn print_error(id: String, error: String) {
    let result = BenchResult {
        id,
        status: "error".to_owned(),
        error: Some(error),
        median_ns: 0,
        p95_ns: 0,
        iterations_per_sample: 0,
        ring_count: 0,
        signature: "[]".to_owned(),
    };
    println!("{}", serde_json::to_string(&result).expect("serializable error result"));
}

fn join_type(value: &str) -> JoinType {
    match value {
        "square" => JoinType::Square,
        "bevel" => JoinType::Bevel,
        "round" => JoinType::Round,
        "miter" => JoinType::Miter,
        other => panic!("unknown join type {other}"),
    }
}

fn end_type(value: &str) -> EndType {
    match value {
        "polygon" => EndType::Polygon,
        "joined" => EndType::Joined,
        "butt" => EndType::Butt,
        "square" => EndType::Square,
        "round" => EndType::Round,
        other => panic!("unknown end type {other}"),
    }
}
