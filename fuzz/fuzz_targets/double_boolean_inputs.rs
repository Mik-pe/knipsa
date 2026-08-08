#![no_main]

use knipsa::{
    BooleanRequest, ClipType, Error, FillRule, PathKind, PathsD, PointD, boolean_op_d,
    validate_paths_d,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, coordinates)) = data.split_first() else { return };
    let points = coordinates
        .chunks_exact(5)
        .take(48)
        .map(|bytes| {
            let x = f64::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 16.0;
            let y = f64::from(i16::from_le_bytes([bytes[2], bytes[3]])) / 16.0;
            let jitter = f64::from(i8::from_le_bytes([bytes[4]])) * 1.0e-10;
            PointD::new(x + jitter, y - jitter)
        })
        .collect::<Vec<_>>();
    let split = points.len() / 2;
    if split < 3 || points.len() - split < 3 {
        return;
    }

    let subjects = [points[..split].to_vec()];
    let open_subjects = [points.clone()];
    let clips = [points[split..].to_vec()];
    let fill_rule = [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        [usize::from(selector % 4)];
    for clip_type in [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
    {
        let result = boolean_op_d(BooleanRequest {
            closed_subjects: &subjects,
            open_subjects: &open_subjects,
            clips: &clips,
            clip_type,
            fill_rule,
            limits: knipsa::ComplexityLimits::DEFAULT,
        });
        if let Ok(output) = &result {
            assert_valid(&output.closed, PathKind::Closed);
            assert_valid(&output.open, PathKind::Open);
        }
        let result = result.map(|output| output.closed);
        if matches!(clip_type, ClipType::Intersection | ClipType::Union | ClipType::Xor) {
            let reverse = boolean_op_d(BooleanRequest::new(&clips, &subjects, clip_type, fill_rule))
                .map(|output| output.closed);
            assert_commutative(&result, &reverse);
        }
    }
});

fn assert_valid(paths: &PathsD, kind: PathKind) {
    validate_paths_d(paths, kind).expect("boolean output must satisfy path contract");
    for path in paths {
        let adjacent_are_distinct = path.windows(2).all(|pair| pair[0] != pair[1]);
        let closing_is_distinct = kind == PathKind::Open || path.first() != path.last();
        assert!(
            adjacent_are_distinct && closing_is_distinct,
            "boolean output must not contain adjacent duplicate vertices"
        );
    }
}

fn assert_commutative(left: &Result<PathsD, Error>, right: &Result<PathsD, Error>) {
    match (left, right) {
        (Ok(left), Ok(right)) => {
            assert_eq!(left.len(), right.len(), "commutative operation changed ring count");
            for (left_path, right_path) in left.iter().zip(right) {
                assert_eq!(
                    left_path.len(),
                    right_path.len(),
                    "commutative operation changed vertex count"
                );
                for (left_point, right_point) in left_path.iter().zip(right_path) {
                    assert!(
                        (left_point.x - right_point.x).abs() <= 1.0e-8
                            && (left_point.y - right_point.y).abs() <= 1.0e-8,
                        "commutative operation changed canonical geometry"
                    );
                }
            }
        }
        (Err(left), Err(right)) => assert_eq!(left, right),
        _ => panic!("commutative operation changed success status"),
    }
}
