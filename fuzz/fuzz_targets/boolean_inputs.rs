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
    let open_subjects = [points.clone()];
    let clips = [points[split..].to_vec()];
    let fill_rule = [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        [usize::from(selector % 4)];
    for clip_type in [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
    {
        let result = boolean_op(BooleanRequest {
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
            let reverse = boolean_op(BooleanRequest::new(&clips, &subjects, clip_type, fill_rule))
                .map(|output| output.closed);
            assert_eq!(result, reverse, "commutative operation changed with operand order");
        }
    }

    let self_xor = boolean_op(BooleanRequest::new(
        &subjects,
        &subjects,
        ClipType::Xor,
        fill_rule,
    ))
    .map(|output| output.closed);
    if let Ok(paths) = self_xor {
        assert!(paths.is_empty(), "self XOR must be empty");
    }
});

fn assert_valid(paths: &Paths64, kind: PathKind) {
    validate_paths64(paths, kind).expect("boolean output must satisfy path contract");
    for path in paths {
        let adjacent_are_distinct = path.windows(2).all(|pair| pair[0] != pair[1]);
        let closing_is_distinct = kind == PathKind::Open || path.first() != path.last();
        assert!(
            adjacent_are_distinct && closing_is_distinct,
            "boolean output must not contain adjacent duplicate vertices"
        );
    }
}
