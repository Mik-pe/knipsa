#![allow(missing_docs)]

use std::{hint::black_box, time::Instant};

use knipsa::{EndType, JoinType, OffsetOptions, PathD, PointD, offset_paths_d};
use serde::{Deserialize, Serialize};

const WORKLOAD: &str = include_str!("../../../benchmarks/offset-workloads.json");
const WARMUP_RUNS: usize = 3;
const SAMPLE_RUNS: usize = 25;

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
    ring_count: usize,
    signature: String,
}

fn main() {
    let workload: Workload = serde_json::from_str(WORKLOAD).expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-offset-workload-v1");
    println!(
        "{{\"implementation\":\"knipsa-offset\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS}}}"
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
        if let Some(error) = (0..WARMUP_RUNS).find_map(|_| {
            offset_paths_d(&paths, case.delta, options).err().map(|error| error.to_string())
        }) {
            print_error(case.id, error);
            continue;
        }
        let mut timings = Vec::with_capacity(SAMPLE_RUNS);
        let mut output = Vec::new();
        for _ in 0..SAMPLE_RUNS {
            let started = Instant::now();
            match offset_paths_d(&paths, case.delta, options) {
                Ok(result) => output = black_box(result),
                Err(error) => {
                    print_error(case.id.clone(), error.to_string());
                    timings.clear();
                    break;
                }
            }
            timings.push(started.elapsed().as_nanos());
        }
        if timings.is_empty() {
            continue;
        }
        timings.sort_unstable();
        let result = BenchResult {
            id: case.id,
            status: "ok".to_owned(),
            error: None,
            median_ns: timings[timings.len() / 2],
            p95_ns: timings[(timings.len() * 95).div_ceil(100) - 1],
            ring_count: output.len(),
            signature: serde_json::to_string(
                &output
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
