use knipsa::{
    BooleanRequestD, ClipType, FillRule, JoinType, OffsetOptions, PathKind, PointD,
    ComplexityLimits, boolean_op_d, normalize_path_d, offset_path_d, triangulate_path_d,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ring = vec![
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(10.0, 10.0),
        PointD::new(0.0, 10.0),
        PointD::new(0.0, 0.0),
    ];
    let ring = normalize_path_d(&ring, PathKind::Closed);
    let subjects = vec![ring.clone()];
    let union = boolean_op_d(BooleanRequestD::new(
        &subjects,
        &[],
        ClipType::Union,
        FillRule::EvenOdd,
    ))?;
    let offset = offset_path_d(&ring, 1.0, OffsetOptions::polygon(JoinType::Miter))?;
    let triangles = triangulate_path_d(&ring, ComplexityLimits::DEFAULT)?;

    assert_eq!(union.len(), 1);
    assert_eq!(offset.len(), 1);
    assert_eq!(triangles.len(), 2);
    Ok(())
}
