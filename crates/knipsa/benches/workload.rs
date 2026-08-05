#![allow(missing_docs)]

use std::{hint::black_box, time::Instant};

use knipsa::{BooleanRequestD, ClipType, FillRule, PathD, PathsD, PointD, boolean_opd};
use serde::{Deserialize, Serialize};

const WORKLOAD: &str = include_str!("../../../benchmarks/workloads.json");
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
    clip_type: String,
    fill_rule: String,
    subjects: Vec<Vec<[f64; 2]>>,
    clips: Vec<Vec<[f64; 2]>>,
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

#[derive(Serialize)]
struct RingRecord {
    depth: usize,
    area2: f64,
    points: Vec<[f64; 2]>,
}

fn main() {
    let workload: Workload = serde_json::from_str(WORKLOAD).expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-workload-v1");
    println!(
        "{{\"implementation\":\"knipsa\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS}}}"
    );
    for test_case in workload.cases {
        let subjects = paths_from_json(test_case.subjects);
        let clips = paths_from_json(test_case.clips);
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: clip_type(&test_case.clip_type),
            fill_rule: fill_rule(&test_case.fill_rule),
        };
        if let Some(error) = (0..WARMUP_RUNS).find_map(|_| match boolean_opd(request) {
            Ok(result) => {
                black_box(result);
                None
            }
            Err(error) => Some(error.to_string()),
        }) {
            print_error(test_case.id, error);
            continue;
        }
        let mut timings = Vec::with_capacity(SAMPLE_RUNS);
        let mut output = Vec::new();
        for _ in 0..SAMPLE_RUNS {
            let started = Instant::now();
            match boolean_opd(request) {
                Ok(result) => output = black_box(result),
                Err(error) => {
                    print_error(test_case.id.clone(), error.to_string());
                    output.clear();
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
        let median_ns = timings[timings.len() / 2];
        let p95_ns = timings[(timings.len() * 95).div_ceil(100) - 1];
        let result = BenchResult {
            id: test_case.id,
            status: "ok".to_owned(),
            error: None,
            median_ns,
            p95_ns,
            ring_count: output.len(),
            signature: signature(&output),
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

fn paths_from_json(paths: Vec<Vec<[f64; 2]>>) -> Vec<PathD> {
    paths
        .into_iter()
        .map(|path| path.into_iter().map(|[x, y]| PointD::new(x, y)).collect())
        .collect()
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

fn signature(paths: &PathsD) -> String {
    let records = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let points = canonical_ring(path);
            let depth = paths
                .iter()
                .enumerate()
                .filter(|(other_index, other)| *other_index != index && contains(points[0], other))
                .count();
            RingRecord { depth, area2: quantize(area2(path).abs()), points }
        })
        .collect::<Vec<_>>();
    let mut records = records;
    records.sort_by_key(|record| serde_json::to_string(record).expect("serializable ring"));
    serde_json::to_string(&records).expect("serializable signature")
}

fn canonical_ring(path: &[PointD]) -> Vec<[f64; 2]> {
    let points = remove_collinear(
        path.iter().map(|point| [quantize(point.x), quantize(point.y)]).collect::<Vec<_>>(),
    );
    let forward = rotate_to_minimum(points.clone());
    let mut reversed = points;
    reversed.reverse();
    let reversed = rotate_to_minimum(reversed);
    if forward < reversed { forward } else { reversed }
}

fn remove_collinear(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let mut changed = true;
    while changed && points.len() >= 3 {
        changed = false;
        let mut cleaned = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let previous = points[(index + points.len() - 1) % points.len()];
            let current = points[index];
            let next = points[(index + 1) % points.len()];
            let first = [current[0] - previous[0], current[1] - previous[1]];
            let second = [next[0] - current[0], next[1] - current[1]];
            let cross = first[0] * second[1] - first[1] * second[0];
            let dot = first[0] * second[0] + first[1] * second[1];
            if cross.abs() <= 1e-12 && dot >= -1e-12 {
                changed = true;
            } else {
                cleaned.push(current);
            }
        }
        points = cleaned;
    }
    points
}

fn rotate_to_minimum(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if let Some((minimum, _)) = points.iter().enumerate().min_by(|(_, left), (_, right)| {
        left[0].total_cmp(&right[0]).then(left[1].total_cmp(&right[1]))
    }) {
        points.rotate_left(minimum);
    }
    points
}

fn quantize(value: f64) -> f64 {
    let rounded = (value * 1e9).round() / 1e9;
    if rounded.to_bits() == (-0.0_f64).to_bits() { 0.0 } else { rounded }
}

fn area2(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(start, end)| start.x * end.y - start.y * end.x)
        .sum()
}

fn contains(point: [f64; 2], path: &[PointD]) -> bool {
    let mut inside = false;
    for (start, end) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
        if (start.y > point[1]) != (end.y > point[1]) {
            let cross =
                (end.x - start.x) * (point[1] - start.y) - (end.y - start.y) * (point[0] - start.x);
            if (end.y > start.y && cross > 0.0) || (end.y < start.y && cross < 0.0) {
                inside = !inside;
            }
        }
    }
    inside
}
