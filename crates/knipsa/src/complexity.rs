//! Deterministic budgets for operations that analyze polygon topology.

use crate::Error;

/// Resources bounded before polygon topology analysis begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ComplexityResource {
    /// Number of input paths.
    Paths,
    /// Total number of input vertices.
    Vertices,
    /// Conservative number of candidate edge pairs checked for intersections.
    EdgePairs,
}

/// Explicit input-complexity limits for polygon topology analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ComplexityLimits {
    paths: usize,
    vertices: usize,
    edge_pairs: usize,
}

impl ComplexityLimits {
    /// A production-oriented starting point for untrusted requests.
    pub const DEFAULT: Self = Self::new(1_024, 1_000_000, 4_000_000);

    /// Creates a limit set. A zero limit rejects any use of that resource.
    #[must_use]
    pub const fn new(max_paths: usize, max_vertices: usize, max_edge_pairs: usize) -> Self {
        Self { paths: max_paths, vertices: max_vertices, edge_pairs: max_edge_pairs }
    }

    /// Maximum number of input paths.
    #[must_use]
    pub const fn max_paths(self) -> usize {
        self.paths
    }

    /// Maximum total number of input vertices.
    #[must_use]
    pub const fn max_vertices(self) -> usize {
        self.vertices
    }

    /// Maximum conservative number of candidate intersection edge pairs.
    #[must_use]
    pub const fn max_edge_pairs(self) -> usize {
        self.edge_pairs
    }

    /// Checks path lengths without reading or allocating their vertices.
    ///
    /// Adapters can call this before copying foreign input. Geometry operations
    /// still require the same limits and repeat this cheap linear preflight.
    ///
    /// # Errors
    ///
    /// Returns [`Error::LimitExceeded`] in deterministic `Paths`, `Vertices`,
    /// then `EdgePairs` priority order.
    pub fn check(self, path_lengths: impl IntoIterator<Item = usize>) -> Result<(), Error> {
        let mut paths = 0_usize;
        let mut vertices = 0_usize;
        let mut edge_pairs = 0_usize;
        for length in path_lengths {
            paths = paths.saturating_add(1);
            edge_pairs = edge_pairs.saturating_add(vertices.saturating_mul(length));
            if length >= 3 {
                edge_pairs = edge_pairs.saturating_add(length.saturating_mul(length - 3) / 2);
            }
            vertices = vertices.saturating_add(length);
        }
        check_limit(ComplexityResource::Paths, paths, self.paths)?;
        check_limit(ComplexityResource::Vertices, vertices, self.vertices)?;
        check_limit(ComplexityResource::EdgePairs, edge_pairs, self.edge_pairs)
    }
}

impl Default for ComplexityLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn check_limit(resource: ComplexityResource, required: usize, limit: usize) -> Result<(), Error> {
    if required > limit { Err(Error::LimitExceeded { resource, limit, required }) } else { Ok(()) }
}
