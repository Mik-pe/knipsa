#![doc = include_str!("lib.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc(test(attr(deny(warnings))))]

mod boolean;
mod error;
#[path = "standard_dispatch.rs"]
mod standard_dispatch;
#[path = "performance_dispatch.rs"]
mod fast;
mod fast_dispatch;
mod geometry;
mod offset;
mod request;
mod triangulation;

pub use error::Error;
pub use geometry::{
    Orientation, Path64, PathD, Paths64, PathsD, Point64, PointD, PointLocation, Rect64, RectD,
    clip_to_rect_d, clip_to_rect64, normalize_path64, normalize_pathd, orientation,
    point_in_polygon, reverse_path_d, reverse_path64, signed_area2, simplify_paths_d,
    simplify_paths64, translate_path_d, translate_path64, trim_collinear_d, trim_collinear64,
    validate_path64, validate_pathd, validate_paths_d, validate_paths64,
};
pub use offset::{
    EndType, JoinType, OffsetOptions, offset_path_d, offset_path64, offset_paths, offset_paths_d,
    offset_paths64,
};
pub use request::{
    BooleanRequest, BooleanRequestD, ClipType, FillRule, boolean_op, boolean_opd, difference,
    difference_d, difference_path, difference_path_d, intersection, intersection_d,
    intersection_path, intersection_path_d, union, union_d, union_path, union_path_d,
    validate_request, validate_requestd, xor, xor_d, xor_path, xor_path_d,
};
pub use triangulation::{
    Triangle64, TriangleD, triangulate_d, triangulate_path64, triangulate_pathd,
    triangulate_paths_d, triangulate_paths64, triangulate64,
};

/// Describes whether a path's final point connects back to its first point.
///
/// Polygon operations use [`PathKind::Closed`]. Offset operations use
/// [`PathKind::Open`] for stroked polylines and [`PathKind::Closed`] for
/// filled polygons. A repeated closing point is accepted and removed by the
/// normalization helpers; it is not required by the API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// A path whose edges wrap from the final point to the first.
    Closed,
    /// A path whose final point is not connected back to its first.
    Open,
}

/// The current semantic version of the safe Rust API.
///
/// This is the crate version compiled into the library. It is useful for
/// logging or for checking that a dynamically loaded companion library is the
/// version an application expects.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_contract_is_constructible() {
        assert_eq!(API_VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(PathKind::Closed, PathKind::Closed);
        let request = BooleanRequest {
            subjects: &[],
            clips: &[],
            clip_type: ClipType::Xor,
            fill_rule: FillRule::Positive,
        };
        assert!(validate_request(&request).is_ok());
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
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }
}
