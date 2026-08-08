//! Errors returned by the safe Rust API.

use crate::PathKind;
use core::fmt;

/// An error produced while validating or executing a geometry operation.
///
/// The variants are intentionally operation-independent across booleans,
/// offsets, topology building, and checked geometry helpers. Triangulation
/// wraps this type in [`crate::TriangulationError`] to add resource failures.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    ///
    /// Use the floating-point API when an intersection creates fractional
    /// coordinates.
    NonIntegralResult,
    /// The arrangement could not be closed into valid output rings.
    TopologyFailure,
    /// An offset option is not geometrically meaningful.
    InvalidOffset,
    /// A triangulation input cannot be triangulated.
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

/// A path-collection validation error with stable input coordinates.
///
/// Call the `validate_paths*_located` helpers before an operation when an
/// application needs to identify the failing path and, for coordinate errors,
/// vertex.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PathValidationError {
    path_index: usize,
    point_index: Option<usize>,
    error: Error,
}

impl PathValidationError {
    pub(crate) fn new(path_index: usize, error: Error) -> Self {
        let point_index = match &error {
            Error::NonFiniteCoordinate { point_index } => Some(*point_index),
            _ => None,
        };
        Self { path_index, point_index, error }
    }

    /// Zero-based index of the failing path.
    #[must_use]
    pub const fn path_index(&self) -> usize {
        self.path_index
    }

    /// Zero-based vertex index when the underlying error identifies one.
    #[must_use]
    pub const fn point_index(&self) -> Option<usize> {
        self.point_index
    }

    /// Original operation-independent geometry error.
    #[must_use]
    pub const fn error(&self) -> &Error {
        &self.error
    }

    /// Consumes this value and returns the original geometry error.
    #[must_use]
    pub fn into_error(self) -> Error {
        self.error
    }
}

impl fmt::Display for PathValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(point_index) = self.point_index {
            write!(formatter, "path {}, point {point_index}: {}", self.path_index, self.error)
        } else {
            write!(formatter, "path {}: {}", self.path_index, self.error)
        }
    }
}

impl std::error::Error for PathValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
