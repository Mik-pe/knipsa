#![doc = include_str!("lib.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc(test(attr(deny(warnings))))]

mod boolean;
mod complexity;
mod dispatch;
mod error;
mod fast;
mod fast_dispatch;
#[cfg(feature = "geo-types")]
pub mod geo_types;
mod geometry;
mod offset;
mod request;
mod standard_dispatch;
mod topology;
mod triangulation;

pub use complexity::{ComplexityLimits, ComplexityResource};
pub use error::{Error, PathValidationError};
pub use geometry::{
    Orientation, Path64, PathD, Paths64, PathsD, Point64, PointD, PointLocation, Rect64, RectD,
    clip_to_rect_d, clip_to_rect64, normalize_path_d, normalize_path64, orientation,
    point_in_polygon, reverse_path_d, reverse_path64, signed_area2, simplify_paths_d,
    simplify_paths64, translate_path_d, translate_path64, trim_collinear_d, trim_collinear64,
    validate_path_d, validate_path64, validate_paths_d, validate_paths_d_located, validate_paths64,
    validate_paths64_located,
};
pub use offset::{
    EndType, JoinType, OffsetOptions, offset_path_d, offset_path64, offset_paths_d, offset_paths64,
};
pub use request::{
    BooleanOutput, BooleanRequest, ClipType, FillRule, boolean_op, boolean_op_d, difference,
    difference_d, difference_path, difference_path_d, intersection, intersection_d,
    intersection_path, intersection_path_d, union, union_d, union_path, union_path_d, xor, xor_d,
    xor_path, xor_path_d,
};
pub use topology::{Polygon64, PolygonD, build_polygons_d, build_polygons64};
pub use triangulation::{
    Triangle64, TriangleD, triangulate_d, triangulate_path_d, triangulate_path64, triangulate64,
};

/// Describes whether a path's final point connects back to its first point.
///
/// Polygon operations use [`PathKind::Closed`]. Offset operations use
/// [`PathKind::Open`] for stroked polylines and [`PathKind::Closed`] for
/// filled polygons. A repeated closing point is accepted and removed by the
/// normalization helpers; it is not required by the API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum PathKind {
    /// A path whose edges wrap from the final point to the first.
    Closed,
    /// A path whose final point is not connected back to its first.
    Open,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_contract_is_constructible() {
        assert_eq!(PathKind::Closed, PathKind::Closed);
        let request = BooleanRequest {
            limits: crate::ComplexityLimits::DEFAULT,
            open_subjects: &[],
            closed_subjects: &[],
            clips: &[],
            clip_type: ClipType::Xor,
            fill_rule: FillRule::Positive,
        };
        assert!(boolean_op(request).is_ok());
    }

    #[test]
    fn errors_are_displayable() {
        let errors = [
            Error::InvalidPath { kind: PathKind::Closed, minimum_vertices: 3, actual_vertices: 2 },
            Error::NonFiniteCoordinate { point_index: 4 },
            Error::ArithmeticOverflow,
            Error::NonIntegralResult,
            Error::TopologyFailure,
            Error::InvalidOffset,
            Error::TriangulationFailure,
            Error::IntersectingPaths,
            Error::LimitExceeded { resource: ComplexityResource::Vertices, limit: 4, required: 5 },
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;
    use serde::{Serialize, de::DeserializeOwned};

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + DeserializeOwned,
    {
        serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
    }

    #[test]
    fn public_value_types_round_trip_through_serde() {
        let path = vec![PointD::new(1.25, -2.5), PointD::new(3.75, 4.5)];
        assert_eq!(round_trip(&path), path);
        assert_eq!(round_trip(&Point64::new(i64::MIN, i64::MAX)), Point64::new(i64::MIN, i64::MAX));
        assert_eq!(round_trip(&Rect64::new(5, 6, 1, 2)), Rect64::new(5, 6, 1, 2));
        assert_eq!(round_trip(&RectD::new(5.0, 6.0, 1.0, 2.0)), RectD::new(5.0, 6.0, 1.0, 2.0));
        assert_eq!(round_trip(&Orientation::CounterClockwise), Orientation::CounterClockwise);
        assert_eq!(round_trip(&PointLocation::Boundary), PointLocation::Boundary);
        assert_eq!(round_trip(&PathKind::Open), PathKind::Open);
        assert_eq!(round_trip(&ClipType::Difference), ClipType::Difference);
        assert_eq!(round_trip(&FillRule::Negative), FillRule::Negative);
        assert_eq!(round_trip(&JoinType::Miter), JoinType::Miter);
        assert_eq!(round_trip(&EndType::Round), EndType::Round);
        assert_eq!(round_trip(&OffsetOptions::default()), OffsetOptions::default());
        assert_eq!(round_trip(&ComplexityLimits::DEFAULT), ComplexityLimits::DEFAULT);
        let polygon = PolygonD {
            outer: vec![PointD::new(0.0, 0.0), PointD::new(4.0, 0.0), PointD::new(0.0, 4.0)],
            holes: Vec::new(),
        };
        assert_eq!(round_trip(&polygon), polygon);
        let polygon64 = Polygon64 {
            outer: vec![Point64::new(0, 0), Point64::new(4, 0), Point64::new(0, 4)],
            holes: Vec::new(),
        };
        assert_eq!(round_trip(&polygon64), polygon64);
        let boolean_output = BooleanOutput {
            closed: vec![vec![Point64::new(0, 0), Point64::new(1, 0), Point64::new(0, 1)]],
            open: vec![vec![Point64::new(0, 0), Point64::new(1, 1)]],
        };
        assert_eq!(round_trip(&boolean_output), boolean_output);

        let limit_error =
            Error::LimitExceeded { resource: ComplexityResource::Vertices, limit: 4, required: 5 };
        assert_eq!(round_trip(&limit_error), limit_error);
        assert_eq!(round_trip(&Error::TopologyFailure), Error::TopologyFailure);

        let located = validate_paths_d_located(
            &[vec![PointD::new(0.0, 0.0), PointD::new(f64::NAN, 0.0)]],
            PathKind::Open,
        )
        .unwrap_err();
        assert_eq!(round_trip(&located), located);
    }

    #[test]
    fn public_enum_wire_names_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&Orientation::CounterClockwise).unwrap(),
            "\"counter_clockwise\""
        );
        assert_eq!(serde_json::to_string(&FillRule::EvenOdd).unwrap(), "\"even_odd\"");
        assert_eq!(
            serde_json::to_string(&ComplexityResource::EdgePairs).unwrap(),
            "\"edge_pairs\""
        );
    }
}
