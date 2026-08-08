#![allow(missing_docs)]

use std::{env, fs};

use knipsa::{BooleanOutput, BooleanRequest, ClipType, FillRule, Path64, Point64, boolean_op};
use serde::{Deserialize, Serialize};

#[path = "support/benchmark_protocol.rs"]
mod benchmark_protocol;
use benchmark_protocol::{MIN_SAMPLE_TIME_NS, SAMPLE_RUNS, WARMUP_RUNS, measure};

const WORKLOAD: &str = include_str!("../../../benchmarks/open-workloads.json");

#[derive(Deserialize)]
struct Workload {
    schema: String,
    coordinate_type: String,
    cases: Vec<WorkloadCase>,
}

#[derive(Deserialize)]
struct WorkloadCase {
    id: String,
    clip_type: String,
    fill_rule: String,
    closed_subjects: Vec<Vec<[i64; 2]>>,
    open_subjects: Vec<Vec<[i64; 2]>>,
    clips: Vec<Vec<[i64; 2]>>,
}

#[derive(Serialize)]
struct BenchResult {
    id: String,
    status: String,
    error: Option<String>,
    median_ns: u128,
    p95_ns: u128,
    iterations_per_sample: usize,
    closed_path_count: usize,
    open_path_count: usize,
    closed_signature: String,
    open_signature: String,
}

fn main() {
    let workload_json = env::var_os("KNIPSA_OPEN_WORKLOAD").map_or_else(
        || WORKLOAD.to_owned(),
        |path| fs::read_to_string(path).expect("read KNIPSA_OPEN_WORKLOAD"),
    );
    let workload: Workload = serde_json::from_str(&workload_json).expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-open-workload-v1");
    assert_eq!(workload.coordinate_type, "i64");
    println!(
        "{{\"implementation\":\"knipsa-open-i64\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );

    for case in workload.cases {
        let closed_subjects = paths(case.closed_subjects);
        let open_subjects = paths(case.open_subjects);
        let clips = paths(case.clips);
        let request = BooleanRequest {
            closed_subjects: &closed_subjects,
            open_subjects: &open_subjects,
            clips: &clips,
            clip_type: clip_type(&case.clip_type),
            fill_rule: fill_rule(&case.fill_rule),
            limits: knipsa::ComplexityLimits::DEFAULT,
        };
        let measured = match measure(|| boolean_op(request).map_err(|error| error.to_string())) {
            Ok(measured) => measured,
            Err(error) => {
                print_error(case.id, error);
                continue;
            }
        };
        print_success(case.id, &measured);
    }
}

fn paths(input: Vec<Vec<[i64; 2]>>) -> Vec<Path64> {
    input
        .into_iter()
        .map(|path| path.into_iter().map(|[x, y]| Point64::new(x, y)).collect())
        .collect()
}

fn print_success(id: String, measured: &benchmark_protocol::Measurement<BooleanOutput<Path64>>) {
    let result = BenchResult {
        id,
        status: "ok".to_owned(),
        error: None,
        median_ns: measured.median_ns,
        p95_ns: measured.p95_ns,
        iterations_per_sample: measured.iterations_per_sample,
        closed_path_count: measured.output.closed.len(),
        open_path_count: measured.output.open.len(),
        closed_signature: signature(&measured.output.closed),
        open_signature: signature(&measured.output.open),
    };
    println!("{}", serde_json::to_string(&result).expect("serializable result"));
}

fn print_error(id: String, error: String) {
    let result = BenchResult {
        id,
        status: "error".to_owned(),
        error: Some(error),
        median_ns: 0,
        p95_ns: 0,
        iterations_per_sample: 0,
        closed_path_count: 0,
        open_path_count: 0,
        closed_signature: "[]".to_owned(),
        open_signature: "[]".to_owned(),
    };
    println!("{}", serde_json::to_string(&result).expect("serializable error result"));
}

fn signature(paths: &[Path64]) -> String {
    serde_json::to_string(
        &paths
            .iter()
            .map(|path| path.iter().map(|point| [point.x, point.y]).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    )
    .expect("serializable signature")
}

fn clip_type(value: &str) -> ClipType {
    match value {
        "intersection" => ClipType::Intersection,
        "union" => ClipType::Union,
        "difference" => ClipType::Difference,
        "xor" => ClipType::Xor,
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
