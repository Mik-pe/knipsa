#![doc = include_str!("../../../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod geometry;
mod request;

pub use error::Error;
pub use geometry::{
    Orientation, Path64, PathD, Paths64, PathsD, Point64, PointD, PointLocation, normalize_path64,
    normalize_pathd, orientation, point_in_polygon, signed_area2, validate_path64, validate_pathd,
    validate_paths64,
};
pub use request::{BooleanRequest, ClipType, FillRule, boolean_op, validate_request};

/// Whether a path describes a closed region or an open line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// A path whose edges wrap from the final point to the first.
    Closed,
    /// A path whose final point is not connected back to the first.
    Open,
}

/// The current semantic version of the safe Rust API.
pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_contract_is_constructible() {
        assert_eq!(API_VERSION, "0.0.0");
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
            Error::KernelNotReady,
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }
}
