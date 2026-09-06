//! Identical public-API workload compiled against both benchmark revisions.

use std::hint::black_box;
use std::time::{Duration, Instant};

use knipsa::{BooleanRequest, ClipType, FillRule, Path64, Paths64, Point64, boolean_op};

struct Case {
    name: String,
    subjects: Paths64,
    clips: Paths64,
    operation: ClipType,
    expected: Paths64,
}

fn rectangle(x: i64, y: i64, width: i64, height: i64) -> Path64 {
    vec![
        Point64::new(x, y),
        Point64::new(x + width, y),
        Point64::new(x + width, y + height),
        Point64::new(x, y + height),
    ]
}

fn lens(radius: i64) -> Path64 {
    (-radius..=radius)
        .map(|x| Point64::new(x, x * x))
        .chain((-radius..=radius).rev().map(|x| Point64::new(x, 4 * radius * radius - x * x)))
        .collect()
}

fn canonical(paths: &Paths64) -> Vec<Vec<(i64, i64)>> {
    let mut result = paths
        .iter()
        .map(|path| {
            let mut points = path.iter().map(|point| (point.x, point.y)).collect::<Vec<_>>();
            if let Some((index, _)) = points.iter().enumerate().min_by_key(|(_, point)| *point) {
                points.rotate_left(index);
            }
            points
        })
        .collect::<Vec<_>>();
    result.sort_unstable();
    result
}

fn fixtures() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut identity = |name: String, subjects: Paths64| {
        cases.push(Case {
            name,
            expected: subjects.clone(),
            subjects,
            clips: Vec::new(),
            operation: ClipType::Union,
        });
    };
    identity("rectangle_4".into(), vec![rectangle(0, 0, 10, 10)]);
    for radius in [16, 64, 256, 512] {
        let path = lens(radius);
        identity(format!("convex_{}", path.len()), vec![path]);
    }
    for count in [8, 64, 256] {
        identity(
            format!("separated_rectangles_{count}"),
            (0..count).map(|index| rectangle(index * 4, 0, 2, 2)).collect(),
        );
    }
    identity(
        "concave_disjoint_supports_8".into(),
        vec![vec![
            Point64::new(0, 0),
            Point64::new(4, 0),
            Point64::new(4, 4),
            Point64::new(3, 4),
            Point64::new(3, 1),
            Point64::new(1, 1),
            Point64::new(1, 4),
            Point64::new(0, 4),
        ]],
    );
    cases.push(Case {
        name: "overlapping_rectangle_control".into(),
        subjects: vec![rectangle(0, 0, 10, 10)],
        clips: vec![rectangle(5, 5, 10, 10)],
        operation: ClipType::Intersection,
        expected: vec![rectangle(5, 5, 5, 5)],
    });
    cases
}

fn main() {
    for case in fixtures() {
        let request = BooleanRequest::new(
            &case.subjects,
            &case.clips,
            case.operation,
            FillRule::EvenOdd,
        );
        let output = boolean_op(request).expect("benchmark input must succeed");
        assert!(output.open.is_empty());
        let signature = canonical(&output.closed);
        assert_eq!(signature, canonical(&case.expected), "{}", case.name);
        let batch = |iterations: u32| {
            let started = Instant::now();
            for _ in 0..iterations {
                drop(black_box(boolean_op(black_box(request)).expect("verified fixture")));
            }
            started.elapsed()
        };
        batch(3);
        let mut iterations = 1_u32;
        while batch(iterations) < Duration::from_millis(5) && iterations < 1 << 20 {
            iterations *= 2;
        }
        let samples = (0..21)
            .map(|_| batch(iterations).as_secs_f64() * 1e9 / f64::from(iterations))
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({
                "case": case.name,
                "iterations": iterations,
                "samples_ns": samples,
                "signature": signature,
                "input_vertices": case.subjects.iter().chain(&case.clips).map(Vec::len).sum::<usize>(),
            })
        );
    }
}
