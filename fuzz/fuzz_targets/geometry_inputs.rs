#![no_main]

use knipsa::{PathKind, Point64, normalize_path64, orientation, point_in_polygon, signed_area2};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let points: Vec<Point64> = data
        .chunks_exact(16)
        .take(256)
        .map(|bytes| {
            let mut x = [0_u8; 8];
            let mut y = [0_u8; 8];
            x.copy_from_slice(&bytes[..8]);
            y.copy_from_slice(&bytes[8..]);
            Point64::new(i64::from_le_bytes(x), i64::from_le_bytes(y))
        })
        .collect();

    let normalized = normalize_path64(&points, PathKind::Closed);
    let _ = signed_area2(&normalized);
    if let Some(window) = normalized.windows(3).next() {
        let _ = orientation(window[0], window[1], window[2]);
    }
    let _ = point_in_polygon(Point64::default(), &normalized);
});
