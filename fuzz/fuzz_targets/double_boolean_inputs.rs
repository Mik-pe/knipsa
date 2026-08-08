#![no_main]

use knipsa::{
    BooleanRequestD, ClipType, Error, FillRule, PathKind, PathsD, PointD, boolean_op_d,
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
    let clips = [points[split..].to_vec()];
    let fill_rule = [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        [usize::from(selector % 4)];
    for clip_type in [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
    {
        let result = boolean_op_d(BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type,
            fill_rule,
        });
        if let Ok(paths) = &result {
            assert_valid(paths);
        }
        if matches!(clip_type, ClipType::Intersection | ClipType::Union | ClipType::Xor) {
            let reverse = boolean_op_d(BooleanRequestD {
                subjects: &clips,
                clips: &subjects,
                clip_type,
                fill_rule,
            });
            assert_commutative(&result, &reverse);
        }
    }
});

fn assert_valid(paths: &PathsD) {
    validate_paths_d(paths, PathKind::Closed).expect("boolean output must satisfy path contract");
    for path in paths {
        assert!(
            path.iter().zip(path.iter().cycle().skip(1)).take(path.len()).all(|(a, b)| a != b),
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
