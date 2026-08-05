//! Errors returned by the safe Rust API.

use crate::PathKind;
use core::fmt;

/// An error produced while validating or executing a geometry operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A path does not contain enough vertices for its declared kind.
    InvalidPath {
        /// Whether the path is closed or open.
        kind: PathKind,
        /// The minimum number of vertices required.
        minimum_vertices: usize,
        /// The number of vertices actually supplied.
        actual_vertices: usize,
    },
    /// A floating-point coordinate is NaN or infinite.
    NonFiniteCoordinate {
        /// The zero-based point index within the path.
        point_index: usize,
    },
    /// A checked integer computation could not be represented by `i128`.
    ArithmeticOverflow,
    /// An exact boolean result cannot be represented by the integer API.
    NonIntegralResult,
    /// The arrangement could not be closed into valid output rings.
    TopologyFailure,
    /// An offset option or offset input is not geometrically meaningful.
    InvalidOffset,
    /// A triangulation input is self-intersecting or cannot be triangulated.
    TriangulationFailure,
    /// A set of paths that must be disjoint contains an intersection.
    IntersectingPaths,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { kind, minimum_vertices, actual_vertices } => write!(
                formatter,
                "{kind:?} path has {actual_vertices} vertices; at least {minimum_vertices} are required"
            ),
            Self::NonFiniteCoordinate { point_index } => {
                write!(formatter, "point {point_index} contains a non-finite coordinate")
            }
            Self::ArithmeticOverflow => {
                formatter.write_str("checked geometry arithmetic overflowed")
            }
            Self::NonIntegralResult => {
                formatter.write_str("the exact result contains a non-integral coordinate")
            }
            Self::TopologyFailure => formatter.write_str("the polygon arrangement did not close"),
            Self::InvalidOffset => formatter.write_str("the offset parameters are invalid"),
            Self::TriangulationFailure => {
                formatter.write_str("the polygon could not be triangulated")
            }
            Self::IntersectingPaths => formatter.write_str("the input paths intersect"),
        }
    }
}

impl std::error::Error for Error {}
