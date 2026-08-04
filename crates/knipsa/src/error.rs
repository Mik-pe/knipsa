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
    /// The scanbeam kernel has not landed in the public tree yet.
    KernelNotReady,
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
            Self::KernelNotReady => formatter.write_str("the polygon clipping kernel is not ready"),
        }
    }
}

impl std::error::Error for Error {}
