//! A small tour of the safe Rust API.

use knipsa::{
    ComplexityLimits, FillRule, OffsetOptions, PathD, Point64, PointD, PointLocation,
    build_polygons_d, intersection_path_d, offset_path_d, point_in_polygon, triangulate_path_d,
};

fn square(left: f64, bottom: f64, right: f64, top: f64) -> PathD {
    vec![
        PointD::new(left, bottom),
        PointD::new(right, bottom),
        PointD::new(right, top),
        PointD::new(left, top),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subject = square(0.0, 0.0, 10.0, 10.0);
    let clip = square(5.0, 5.0, 15.0, 15.0);

    let intersection = intersection_path_d(&subject, &clip)?;
    println!("intersection rings: {}", intersection.len());

    let outline = offset_path_d(&subject, 1.0, OffsetOptions::default())?;
    println!("offset rings: {}", outline.len());

    let triangles = triangulate_path_d(&subject, ComplexityLimits::DEFAULT)?;
    println!("triangles: {}", triangles.len());

    let polygons = build_polygons_d(
        &[subject.clone(), square(2.0, 2.0, 8.0, 8.0)],
        FillRule::EvenOdd,
        ComplexityLimits::DEFAULT,
    )?;
    assert_eq!(polygons[0].holes.len(), 1);

    let integer_square =
        [Point64::new(0, 0), Point64::new(10, 0), Point64::new(10, 10), Point64::new(0, 10)];
    let location = point_in_polygon(Point64::new(4, 4), &integer_square)?;
    assert_eq!(location, PointLocation::Inside);

    Ok(())
}
