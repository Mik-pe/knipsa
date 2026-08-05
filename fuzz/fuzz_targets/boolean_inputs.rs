#![no_main]

use knipsa::{BooleanRequest, ClipType, FillRule, Point64, boolean_op};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let points: Vec<Point64> = data
        .chunks_exact(8)
        .take(32)
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
    for clip_type in
        [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
    {
        let request = BooleanRequest {
            subjects: &subjects,
            clips: &clips,
            clip_type,
            fill_rule: FillRule::EvenOdd,
        };
        let _ = boolean_op(request);
    }
});
