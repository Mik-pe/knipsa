//! Public-API workloads with analytically known output, including exact fallbacks.

use std::hint::black_box;
use std::time::{Duration, Instant};

use knipsa::{BooleanRequest, ClipType, FillRule, PathD, PathsD, PointD, boolean_op_d};

struct Case {
    name: String,
    subjects: PathsD,
    clips: PathsD,
    expected: PathsD,
}

const ORIGIN: f64 = 2_000_000.0;

fn point(x: f64, y: f64) -> PointD {
    PointD::new(ORIGIN + x, y)
}

fn rectangle(x: f64, y: f64, width: f64, height: f64) -> PathD {
    vec![point(x, y), point(x + width, y), point(x + width, y + height), point(x, y + height)]
}

fn comb(teeth: u32, width: f64) -> PathD {
    let mut path = vec![point(0.0, 0.0)];
    for tooth in 0..teeth {
        let y = f64::from(tooth) * 2.0;
        if tooth > 0 {
            path.push(point(1.0, y));
        }
        path.push(point(width, y));
        path.push(point(width, y + 1.0));
        if tooth + 1 < teeth {
            path.push(point(1.0, y + 1.0));
        }
    }
    path.push(point(0.0, f64::from(teeth) * 2.0 - 1.0));
    path
}

fn transpose(path: &mut PathD) {
    for p in path.iter_mut() {
        *p = point(p.y, p.x - ORIGIN);
    }
    path.reverse();
}

fn canonical(paths: &PathsD) -> Vec<Vec<(u64, u64)>> {
    let mut result = paths
        .iter()
        .map(|path| {
            let mut points = path
                .iter()
                .map(|p| ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits()))
                .collect::<Vec<_>>();
            if let Some((index, _)) = points.iter().enumerate().min_by_key(|(_, p)| *p) {
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
    for (teeth, fractional) in [
        (8, false),
        (64, false),
        (256, false),
        (512, false),
        (64, true),
        (512, true),
    ] {
        for vertical in [false, true] {
            let mut subject = comb(teeth, 100.0);
            let mut clip = rectangle(-1.0, -1.0, 51.0, f64::from(teeth) * 2.0 + 1.0);
            let mut expected = comb(teeth, 50.0);
            if vertical {
                transpose(&mut subject);
                transpose(&mut clip);
                transpose(&mut expected);
            }
            if fractional {
                for path in [&mut subject, &mut clip, &mut expected] {
                    for p in path {
                        p.x += 0.25;
                        p.y += 0.25;
                    }
                }
            }
            let prefix = if fractional { "fractional_" } else { "" };
            let axis = if vertical { "vertical" } else { "horizontal" };
            cases.push(Case {
                name: format!("{prefix}{axis}_comb_{}_vertices", subject.len()),
                subjects: vec![subject],
                clips: vec![clip],
                expected: vec![expected],
            });
        }
    }
    for count in [4, 8] {
        let width = f64::from(count) * 3.0;
        let subjects = (0..count).map(|i| rectangle(0.0, f64::from(i) * 3.0, width, 1.0)).collect();
        let clips = (0..count).map(|i| rectangle(f64::from(i) * 3.0, 0.0, 1.0, width)).collect();
        let expected = (0..count)
            .flat_map(|i| {
                (0..count).map(move |j| rectangle(f64::from(i) * 3.0, f64::from(j) * 3.0, 1.0, 1.0))
            })
            .collect();
        cases.push(Case { name: format!("dense_grid_{count}"), subjects, clips, expected });
    }
    cases.push(Case {
        name: "exact_fractional_triangle".into(),
        subjects: vec![vec![point(0.0, 0.0), point(3.0, 0.0), point(0.0, 2.0)]],
        clips: vec![rectangle(1.0, -1.0, 3.0, 4.0)],
        expected: vec![vec![point(1.0, 0.0), point(3.0, 0.0), point(1.0, 4.0 / 3.0)]],
    });
    cases.push(Case {
        name: "exact_rectangle_control".into(),
        subjects: vec![rectangle(0.0, 0.0, 10.0, 10.0)],
        clips: vec![rectangle(5.0, 5.0, 10.0, 10.0)],
        expected: vec![rectangle(5.0, 5.0, 5.0, 5.0)],
    });
    cases
}

fn main() {
    for case in fixtures() {
        let request = BooleanRequest::new(
            &case.subjects,
            &case.clips,
            ClipType::Intersection,
            FillRule::NonZero,
        );
        let output = boolean_op_d(request).expect("benchmark input must succeed");
        assert!(output.open.is_empty());
        let signature = canonical(&output.closed);
        assert_eq!(signature, canonical(&case.expected), "{}", case.name);
        let batch = |iterations: u32| {
            let started = Instant::now();
            for _ in 0..iterations {
                drop(black_box(boolean_op_d(black_box(request)).expect("verified fixture")));
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
