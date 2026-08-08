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

#[derive(Default)]
pub(crate) struct BooleanComplexity {
    paths: usize,
    vertices: usize,
    filled_vertices: usize,
    subject_edges: usize,
    clip_edges: usize,
    open_edges: usize,
    filled_edge_pairs: usize,
}

impl BooleanComplexity {
    pub(crate) fn add_closed_subject(&mut self, vertices: usize) {
        self.add_filled(vertices);
        self.subject_edges = self.subject_edges.saturating_add(vertices);
    }

    pub(crate) fn add_open_subject(&mut self, vertices: usize) {
        self.add_path(vertices);
        self.open_edges = self.open_edges.saturating_add(vertices.saturating_sub(1));
    }

    pub(crate) fn add_clip(&mut self, vertices: usize) {
        self.add_filled(vertices);
        self.clip_edges = self.clip_edges.saturating_add(vertices);
    }

    pub(crate) fn check(
        self,
        limits: ComplexityLimits,
        open_against_closed_subjects: bool,
    ) -> Result<(), Error> {
        let open_boundaries = if open_against_closed_subjects {
            self.clip_edges.saturating_add(self.subject_edges)
        } else {
            self.clip_edges
        };
        let edge_pairs =
            self.filled_edge_pairs.saturating_add(self.open_edges.saturating_mul(open_boundaries));
        check_limit(ComplexityResource::Paths, self.paths, limits.paths)?;
        check_limit(ComplexityResource::Vertices, self.vertices, limits.vertices)?;
        check_limit(ComplexityResource::EdgePairs, edge_pairs, limits.edge_pairs)
    }

    fn add_filled(&mut self, vertices: usize) {
        self.add_path(vertices);
        self.filled_edge_pairs =
            self.filled_edge_pairs.saturating_add(self.filled_vertices.saturating_mul(vertices));
        if vertices >= 3 {
            self.filled_edge_pairs =
                self.filled_edge_pairs.saturating_add(vertices.saturating_mul(vertices - 3) / 2);
        }
        self.filled_vertices = self.filled_vertices.saturating_add(vertices);
    }

    fn add_path(&mut self, vertices: usize) {
        self.paths = self.paths.saturating_add(1);
        self.vertices = self.vertices.saturating_add(vertices);
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
