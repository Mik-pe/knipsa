#![allow(missing_docs)]

use std::{env, fs};

use knipsa::{
    BooleanRequest, ClipType, Error, FillRule, Path64, PathD, Point64, PointD, boolean_op,
    boolean_op_d,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[path = "support/benchmark_protocol.rs"]
mod benchmark_protocol;
use benchmark_protocol::{MIN_SAMPLE_TIME_NS, SAMPLE_RUNS, WARMUP_RUNS, measure};

const WORKLOAD: &str = include_str!("../../../benchmarks/workloads.json");

#[derive(Deserialize)]
struct WorkloadMetadata {
    schema: String,
    coordinate_type: String,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "Coordinate: Deserialize<'de>"))]
struct Workload<Coordinate> {
    schema: String,
    #[serde(rename = "coordinate_type")]
    _coordinate_type: String,
    cases: Vec<WorkloadCase<Coordinate>>,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "Coordinate: Deserialize<'de>"))]
struct WorkloadCase<Coordinate> {
    id: String,
    clip_type: String,
    fill_rule: String,
    subjects: Vec<Vec<[Coordinate; 2]>>,
    clips: Vec<Vec<[Coordinate; 2]>>,
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

trait BenchmarkCoordinate: Copy + DeserializeOwned + Serialize {
    type Point;

    fn point(coordinates: [Self; 2]) -> Self::Point;
    fn coordinates(point: &Self::Point) -> [Self; 2];
    fn boolean_op(
        subjects: &[Vec<Self::Point>],
        clips: &[Vec<Self::Point>],
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<Vec<Vec<Self::Point>>, Error>;
}

impl BenchmarkCoordinate for f64 {
    type Point = PointD;

    fn point([x, y]: [Self; 2]) -> Self::Point {
        PointD::new(x, y)
    }

    fn coordinates(point: &Self::Point) -> [Self; 2] {
        [point.x, point.y]
    }

    fn boolean_op(
        subjects: &[PathD],
        clips: &[PathD],
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<Vec<PathD>, Error> {
        boolean_op_d(BooleanRequest::new(subjects, clips, clip_type, fill_rule))
            .map(|output| output.closed)
    }
}

impl BenchmarkCoordinate for i64 {
    type Point = Point64;

    fn point([x, y]: [Self; 2]) -> Self::Point {
        Point64::new(x, y)
    }

    fn coordinates(point: &Self::Point) -> [Self; 2] {
        [point.x, point.y]
    }

    fn boolean_op(
        subjects: &[Path64],
        clips: &[Path64],
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<Vec<Path64>, Error> {
        boolean_op(BooleanRequest::new(subjects, clips, clip_type, fill_rule))
            .map(|output| output.closed)
    }
}

fn main() {
    let workload_json = env::var_os("KNIPSA_WORKLOAD").map_or_else(
        || WORKLOAD.to_owned(),
        |path| fs::read_to_string(path).expect("read KNIPSA_WORKLOAD"),
    );
    let metadata: WorkloadMetadata =
        serde_json::from_str(&workload_json).expect("valid workload metadata");
    assert_eq!(metadata.schema, "knipsa-workload-v1");
    println!(
        "{{\"implementation\":\"knipsa\",\"samples\":{SAMPLE_RUNS},\"warmups\":{WARMUP_RUNS},\"minimum_sample_time_ns\":{MIN_SAMPLE_TIME_NS}}}"
    );
    match metadata.coordinate_type.as_str() {
        "f64" => run_workload::<f64>(&workload_json),
        "i64" => run_workload::<i64>(&workload_json),
        other => panic!("unsupported coordinate_type {other}"),
    }
}

fn run_workload<Coordinate: BenchmarkCoordinate>(workload_json: &str) {
    let workload: Workload<Coordinate> =
        serde_json::from_str(workload_json).expect("valid workload JSON");
    assert_eq!(workload.schema, "knipsa-workload-v1");
    for test_case in workload.cases {
        let subjects = paths_from_json::<Coordinate>(test_case.subjects);
        let clips = paths_from_json::<Coordinate>(test_case.clips);
        let operation = clip_type(&test_case.clip_type);
        let rule = fill_rule(&test_case.fill_rule);
        let id = test_case.id;
        if let Err(error) = benchmark_case::<Coordinate, _>(&id, || {
            Coordinate::boolean_op(&subjects, &clips, operation, rule)
        }) {
            print_error(id, error);
        }
    }
}

fn benchmark_case<Coordinate, Run>(id: &str, mut run: Run) -> Result<(), String>
where
    Coordinate: BenchmarkCoordinate,
    Run: FnMut() -> Result<Vec<Vec<Coordinate::Point>>, Error>,
{
    let measured = measure(|| run().map_err(|error| error.to_string()))?;
    let result = BenchResult {
        id: id.to_owned(),
        status: "ok".to_owned(),
        error: None,
        median_ns: measured.median_ns,
        p95_ns: measured.p95_ns,
        iterations_per_sample: measured.iterations_per_sample,
        ring_count: measured.output.len(),
        signature: signature::<Coordinate>(&measured.output),
    };
    println!("{}", serde_json::to_string(&result).expect("serializable result"));
    Ok(())
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

fn paths_from_json<Coordinate: BenchmarkCoordinate>(
    paths: Vec<Vec<[Coordinate; 2]>>,
) -> Vec<Vec<Coordinate::Point>> {
    paths.into_iter().map(|path| path.into_iter().map(Coordinate::point).collect()).collect()
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

fn signature<Coordinate: BenchmarkCoordinate>(paths: &[Vec<Coordinate::Point>]) -> String {
    let rings = paths
        .iter()
        .map(|path| path.iter().map(Coordinate::coordinates).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    serde_json::to_string(&rings).expect("serializable signature")
}
