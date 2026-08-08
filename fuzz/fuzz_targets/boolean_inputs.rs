#![no_main]

use knipsa::{
    BooleanRequest, ClipType, FillRule, PathKind, Paths64, Point64, boolean_op, validate_paths64,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, coordinates)) = data.split_first() else { return };
    let points: Vec<Point64> = coordinates
        .chunks_exact(4)
        .take(64)
        .map(|bytes| {
            let x = i16::from_le_bytes([bytes[0], bytes[1]]) as i64;
            let y = i16::from_le_bytes([bytes[2], bytes[3]]) as i64;
            Point64::new(x % 128, y % 128)
        })
        .collect();
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
        let request = BooleanRequest { subjects: &subjects, clips: &clips, clip_type, fill_rule };
        let result = boolean_op(request);
        if let Ok(paths) = &result {
            assert_valid(paths);
        }
        if matches!(clip_type, ClipType::Intersection | ClipType::Union | ClipType::Xor) {
            let reverse = boolean_op(BooleanRequest {
                subjects: &clips,
                clips: &subjects,
                clip_type,
                fill_rule,
            });
            assert_eq!(result, reverse, "commutative operation changed with operand order");
        }
    }

    let self_xor = boolean_op(BooleanRequest {
        subjects: &subjects,
        clips: &subjects,
        clip_type: ClipType::Xor,
        fill_rule,
    });
    if let Ok(paths) = self_xor {
        assert!(paths.is_empty(), "self XOR must be empty");
    }
});

fn assert_valid(paths: &Paths64) {
    validate_paths64(paths, PathKind::Closed).expect("boolean output must satisfy path contract");
    for path in paths {
        assert!(
            path.iter().zip(path.iter().cycle().skip(1)).take(path.len()).all(|(a, b)| a != b),
            "boolean output must not contain adjacent duplicate vertices"
        );
    }
}
