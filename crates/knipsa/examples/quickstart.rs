//! A small tour of the safe Rust API.

use knipsa::{
    BooleanRequestD, ClipType, FillRule, OffsetOptions, PathD, Point64, PointD, PointLocation,
    boolean_opd, offset_paths_d, point_in_polygon, triangulate_pathd,
};

fn square(left: f64, bottom: f64, right: f64, top: f64) -> PathD {
    vec![
        PointD::new(left, bottom),
        PointD::new(right, bottom),
        PointD::new(right, top),
        PointD::new(left, top),
    ]
}

fn main() -> Result<(), knipsa::Error> {
    let subject = square(0.0, 0.0, 10.0, 10.0);
    let clip = square(5.0, 5.0, 15.0, 15.0);

    let intersection = boolean_opd(BooleanRequestD {
        subjects: std::slice::from_ref(&subject),
        clips: std::slice::from_ref(&clip),
        clip_type: ClipType::Intersection,
        fill_rule: FillRule::EvenOdd,
    })?;
    println!("intersection rings: {}", intersection.len());

    let outline = offset_paths_d(std::slice::from_ref(&subject), 1.0, OffsetOptions::default())?;
    println!("offset rings: {}", outline.len());

    let triangles = triangulate_pathd(&subject)?;
    println!("triangles: {}", triangles.len());

    let integer_square =
        [Point64::new(0, 0), Point64::new(10, 0), Point64::new(10, 10), Point64::new(0, 10)];
    let location = point_in_polygon(Point64::new(4, 4), &integer_square)?;
    assert_eq!(location, PointLocation::Inside);

    Ok(())
}
