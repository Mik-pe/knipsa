//! Constrained polygon triangulation.
//!
//! Triangulation is kept as a separate operation from clipping. Inputs may
//! contain multiple nested rings; nesting and [`crate::FillRule`] determine
//! which rings are solids and which are holes. The triangulation backend is
//! the independent ISC-licensed `earcutr` crate, while all input validation,
//! nesting, coordinate preservation, and public result shaping live here.

use crate::{
    Error, FillRule, Path64, PathD, Point64, PointD,
    geometry::paths64_to_local_d,
    topology::{EPSILON, collect_rings, cross, filled_groups, point_d_to_64},
};
use core::fmt;

/// A triangle with integer vertices.
///
/// Returned triangles are oriented counter-clockwise and can be consumed as
/// three-point closed paths.
pub type Triangle64 = [Point64; 3];

/// A triangle with floating-point vertices.
///
/// Returned triangles are oriented counter-clockwise and can be consumed as
/// three-point closed paths.
pub type TriangleD = [PointD; 3];

/// Resources that can be bounded before triangulation begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum TriangulationResource {
    /// Number of input paths.
    Paths,
    /// Total number of input vertices.
    Vertices,
    /// Conservative number of non-adjacent edge pairs examined for intersections.
    EdgePairs,
}

/// Explicit input-complexity limits for untrusted triangulation requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TriangulationLimits {
    paths: usize,
    vertices: usize,
    edge_pairs: usize,
}

impl TriangulationLimits {
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

    /// Maximum conservative number of non-adjacent edge pairs.
    #[must_use]
    pub const fn max_edge_pairs(self) -> usize {
        self.edge_pairs
    }
}

impl Default for TriangulationLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Error from a triangulation request.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum TriangulationError {
    /// Normal geometry validation or execution failed.
    Geometry(Error),
    /// Preflight rejected the request before quadratic intersection work began.
    LimitExceeded {
        /// Resource whose required amount exceeded the configured limit.
        resource: TriangulationResource,
        /// Configured maximum.
        limit: usize,
        /// Conservative required amount, saturated at [`usize::MAX`].
        required: usize,
    },
}

impl fmt::Display for TriangulationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(error) => error.fmt(formatter),
            Self::LimitExceeded { resource, limit, required } => {
                write!(
                    formatter,
                    "triangulation requires {required} {resource:?}; limit is {limit}"
                )
            }
        }
    }
}

impl std::error::Error for TriangulationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            Self::LimitExceeded { .. } => None,
        }
    }
}

impl From<Error> for TriangulationError {
    fn from(error: Error) -> Self {
        Self::Geometry(error)
    }
}

/// Triangulates a collection of integer-coordinate rings using a fill rule.
///
/// Rings may be nested to describe holes and islands. Touching or crossing
/// rings are rejected because their filled meaning is ambiguous for
/// triangulation.
/// The returned triangles are counter-clockwise and preserve the integer
/// coordinate type. Computation uses a shared local origin, so absolute
/// coordinates may span the full `i64` range when their deltas from the shared
/// lower-left bounding-box origin are within the exact integer range of `f64`
/// (2^53).
///
/// The preflight runs before integer-to-floating conversion or quadratic edge
/// intersection checks. [`TriangulationLimits::DEFAULT`] is a conservative
/// starting point; callers should tune it for their latency and memory budget.
///
/// # Errors
///
/// Returns [`TriangulationError::LimitExceeded`] when a configured resource
/// budget is insufficient. [`TriangulationError::Geometry`] contains
/// [`Error::InvalidPath`] for malformed rings, [`Error::IntersectingPaths`] for
/// touching or crossing rings, [`Error::TopologyFailure`] for degenerate ring topology,
/// [`Error::ArithmeticOverflow`] when the integer geometry cannot be converted
/// exactly, or [`Error::TriangulationFailure`] when valid topology cannot be
/// triangulated.
pub fn triangulate64(
    paths: &[Path64],
    fill_rule: FillRule,
    limits: TriangulationLimits,
) -> Result<Vec<Triangle64>, TriangulationError> {
    check_limits(paths.iter().map(Vec::len), limits)?;
    let (origin, paths_d) = paths64_to_local_d(paths)?;
    let triangles = triangulate_d_impl(&paths_d, fill_rule)?;
    Ok(triangles64_from_d(triangles, origin)?)
}

fn triangles64_from_d(
    triangles: Vec<TriangleD>,
    origin: Point64,
) -> Result<Vec<Triangle64>, Error> {
    triangles
        .into_iter()
        .map(|triangle| {
            triangle
                .into_iter()
                .map(|point| point_d_to_64(point, origin))
                .collect::<Result<Vec<_>, _>>()
                .and_then(|points| points.try_into().map_err(|_| Error::TriangulationFailure))
        })
        .collect()
}

/// Triangulates a collection of floating-point rings using a fill rule.
///
/// Nested rings are interpreted using `fill_rule`; an even-odd hole does not
/// need to be manually subtracted from its outer ring. Returned triangles are
/// counter-clockwise.
///
/// This is intended for services processing untrusted or adversarially large
/// geometry. Preflight is linear in the number of paths and rejects requests
/// before any quadratic edge-pair work or backend allocation.
///
/// # Errors
///
/// Returns [`TriangulationError::LimitExceeded`] when a configured resource
/// budget is insufficient. [`TriangulationError::Geometry`] contains
/// [`Error::InvalidPath`] for malformed rings, [`Error::IntersectingPaths`] for
/// touching or crossing rings, [`Error::TopologyFailure`] for degenerate ring topology,
/// or [`Error::TriangulationFailure`] when valid topology cannot be
/// triangulated.
pub fn triangulate_d(
    paths: &[PathD],
    fill_rule: FillRule,
    limits: TriangulationLimits,
) -> Result<Vec<TriangleD>, TriangulationError> {
    check_limits(paths.iter().map(Vec::len), limits)?;
    Ok(triangulate_d_impl(paths, fill_rule)?)
}

fn triangulate_d_impl(paths: &[PathD], fill_rule: FillRule) -> Result<Vec<TriangleD>, Error> {
    let rings = collect_rings(paths)?;
    let groups = filled_groups(&rings, fill_rule);
    let mut result = Vec::new();
    for (outer, holes) in groups {
        let group_start = result.len();
        let mut coordinates = Vec::new();
        let mut vertices = Vec::new();
        let mut hole_indices = Vec::new();
        let mut predicate_vertices = Vec::new();
        append_ring(
            &rings[outer].path,
            &rings[outer].vertices,
            &mut coordinates,
            &mut predicate_vertices,
            &mut vertices,
        );
        for hole in holes {
            hole_indices.push(vertices.len());
            append_ring(
                &rings[hole].path,
                &rings[hole].vertices,
                &mut coordinates,
                &mut predicate_vertices,
                &mut vertices,
            );
        }
        let indices = earcutr::earcut(&coordinates, &hole_indices, 2)
            .map_err(|_| Error::TriangulationFailure)?;
        validate_triangle_indices(&indices)?;
        for indices in indices.chunks_exact(3) {
            push_oriented_triangle(&mut result, &vertices, &predicate_vertices, indices);
        }
        ensure_group_result(group_start, result.len(), !rings[outer].path.is_empty())?;
    }
    Ok(result)
}

/// Triangulates one simple integer polygon with the non-zero fill rule.
///
/// This is the convenient entry point when there is exactly one outer ring
/// and no holes.
///
/// # Errors
///
/// Propagates errors from [`triangulate64`].
pub fn triangulate_path64(
    path: &[Point64],
    limits: TriangulationLimits,
) -> Result<Vec<Triangle64>, TriangulationError> {
    triangulate64(&[path.to_vec()], FillRule::NonZero, limits)
}

/// Triangulates one simple floating-point polygon with the non-zero fill rule.
///
/// This is the convenient entry point when there is exactly one outer ring
/// and no holes.
///
/// # Errors
///
/// Propagates errors from [`triangulate_d`].
pub fn triangulate_path_d(
    path: &[PointD],
    limits: TriangulationLimits,
) -> Result<Vec<TriangleD>, TriangulationError> {
    triangulate_d(&[path.to_vec()], FillRule::NonZero, limits)
}

fn check_limits(
    path_lengths: impl Iterator<Item = usize>,
    limits: TriangulationLimits,
) -> Result<(), TriangulationError> {
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
    check_limit(TriangulationResource::Paths, paths, limits.paths)?;
    check_limit(TriangulationResource::Vertices, vertices, limits.vertices)?;
    check_limit(TriangulationResource::EdgePairs, edge_pairs, limits.edge_pairs)
}

fn check_limit(
    resource: TriangulationResource,
    required: usize,
    limit: usize,
) -> Result<(), TriangulationError> {
    if required > limit {
        Err(TriangulationError::LimitExceeded { resource, limit, required })
    } else {
        Ok(())
    }
}

fn append_ring(
    path: &[PointD],
    original: &[PointD],
    coordinates: &mut Vec<f64>,
    predicate_vertices: &mut Vec<PointD>,
    vertices: &mut Vec<PointD>,
) {
    debug_assert_eq!(path.len(), original.len());
    coordinates.reserve(path.len() * 2);
    predicate_vertices.reserve(path.len());
    vertices.reserve(original.len());
    for (&point, &original_point) in path.iter().zip(original) {
        coordinates.extend([point.x, point.y]);
        predicate_vertices.push(point);
        vertices.push(original_point);
    }
}

fn validate_triangle_indices(indices: &[usize]) -> Result<(), Error> {
    if indices.len().is_multiple_of(3) { Ok(()) } else { Err(Error::TriangulationFailure) }
}

fn ensure_group_result(
    group_start: usize,
    result_len: usize,
    input_was_non_empty: bool,
) -> Result<(), Error> {
    if result_len == group_start && input_was_non_empty {
        Err(Error::TriangulationFailure)
    } else {
        Ok(())
    }
}

fn orient_triangle(
    vertices: &[PointD],
    predicate_vertices: &[PointD],
    indices: &[usize],
) -> Option<TriangleD> {
    let mut triangle = [vertices[indices[0]], vertices[indices[1]], vertices[indices[2]]];
    let area = cross(
        predicate_vertices[indices[1]],
        predicate_vertices[indices[2]],
        predicate_vertices[indices[0]],
    );
    if area.abs() <= EPSILON {
        return None;
    }
    if area < 0.0 {
        triangle.swap(1, 2);
    }
    Some(triangle)
}

fn push_oriented_triangle(
    result: &mut Vec<TriangleD>,
    vertices: &[PointD],
    predicate_vertices: &[PointD],
    indices: &[usize],
) {
    if let Some(triangle) = orient_triangle(vertices, predicate_vertices, indices) {
        result.push(triangle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(left: f64, bottom: f64, right: f64, top: f64) -> PathD {
        vec![
            PointD::new(left, bottom),
            PointD::new(right, bottom),
            PointD::new(right, top),
            PointD::new(left, top),
        ]
    }

    fn area2(first: PointD, second: PointD, third: PointD) -> f64 {
        cross(first, second, third)
    }

    #[test]
    fn triangulates_simple_polygon_and_preserves_ccw_winding() {
        let triangles =
            triangulate_path_d(&rectangle(0.0, 0.0, 10.0, 10.0), TriangulationLimits::DEFAULT)
                .unwrap();
        assert_eq!(triangles.len(), 2);
        let area = triangles
            .iter()
            .map(|triangle| area2(triangle[0], triangle[1], triangle[2]).abs())
            .sum::<f64>();
        assert!((area - 200.0).abs() < 1e-9);
        assert!(
            triangles.iter().all(|triangle| area2(triangle[0], triangle[1], triangle[2]) > 0.0)
        );
    }

    #[test]
    fn floating_triangulation_is_scale_and_translation_invariant() {
        for exponent in -12..=12 {
            let scale = 10_f64.powi(exponent);
            let triangles = triangulate_path_d(
                &rectangle(0.0, 0.0, scale, scale),
                TriangulationLimits::DEFAULT,
            )
            .expect("a uniformly scaled square should triangulate");
            assert_eq!(triangles.len(), 2, "scale exponent {exponent}");
            let normalized_area = triangles
                .iter()
                .map(|triangle| area2(triangle[0], triangle[1], triangle[2]).abs())
                .sum::<f64>()
                / scale.powi(2);
            assert!((normalized_area - 2.0).abs() < 1e-12, "scale exponent {exponent}");
        }

        let origin = 1e12;
        let translated = triangulate_path_d(
            &rectangle(origin, origin, origin + 10.0, origin + 10.0),
            TriangulationLimits::DEFAULT,
        )
        .expect("translation should not alter triangulation topology");
        assert_eq!(translated.len(), 2);
        assert!(translated.iter().flatten().all(|point| point.x >= origin));
        assert!(translated.iter().flatten().all(|point| point.y >= origin));
    }

    #[test]
    fn bounded_triangulation_rejects_work_before_quadratic_checks() {
        let rectangle_d = rectangle(0.0, 0.0, 10.0, 10.0);
        let rectangle64 = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
        ];
        let exact = TriangulationLimits::new(1, 4, 2);
        assert_eq!(exact.max_paths(), 1);
        assert_eq!(exact.max_vertices(), 4);
        assert_eq!(exact.max_edge_pairs(), 2);
        assert_eq!(TriangulationLimits::default(), TriangulationLimits::DEFAULT);
        assert_eq!(
            triangulate_d(std::slice::from_ref(&rectangle_d), FillRule::NonZero, exact)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            triangulate64(std::slice::from_ref(&rectangle64), FillRule::NonZero, exact)
                .unwrap()
                .len(),
            2
        );

        let path_error = triangulate_d(
            &[rectangle_d.clone(), rectangle_d.clone()],
            FillRule::NonZero,
            TriangulationLimits::new(1, usize::MAX, usize::MAX),
        )
        .unwrap_err();
        assert_eq!(
            path_error,
            TriangulationError::LimitExceeded {
                resource: TriangulationResource::Paths,
                limit: 1,
                required: 2,
            }
        );
        assert!(path_error.to_string().contains("Paths"));
        assert!(std::error::Error::source(&path_error).is_none());

        assert_eq!(
            check_limits([4].into_iter(), TriangulationLimits::new(1, 3, usize::MAX)),
            Err(TriangulationError::LimitExceeded {
                resource: TriangulationResource::Vertices,
                limit: 3,
                required: 4,
            })
        );
        assert_eq!(
            check_limits([4].into_iter(), TriangulationLimits::new(1, 4, 1)),
            Err(TriangulationError::LimitExceeded {
                resource: TriangulationResource::EdgePairs,
                limit: 1,
                required: 2,
            })
        );
        assert_eq!(
            check_limits(
                [usize::MAX, usize::MAX].into_iter(),
                TriangulationLimits::new(2, usize::MAX, usize::MAX - 1)
            ),
            Err(TriangulationError::LimitExceeded {
                resource: TriangulationResource::EdgePairs,
                limit: usize::MAX - 1,
                required: usize::MAX,
            })
        );

        let geometry_error = triangulate_d(
            &[vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)]],
            FillRule::NonZero,
            TriangulationLimits::DEFAULT,
        )
        .unwrap_err();
        assert!(matches!(geometry_error, TriangulationError::Geometry(_)));
        assert!(geometry_error.to_string().contains("vertices"));
        assert!(std::error::Error::source(&geometry_error).is_some());
    }

    #[test]
    fn removes_redundant_collinear_vertices_before_triangulation() {
        let path = vec![
            PointD::new(0.0, 0.0),
            PointD::new(3.0, 0.0),
            PointD::new(7.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 8.0),
            PointD::new(6.0, 8.0),
            PointD::new(0.0, 8.0),
        ];
        let triangles = triangulate_path_d(&path, TriangulationLimits::DEFAULT).unwrap();
        assert_eq!(triangles.len(), 2);
        let area = triangles
            .iter()
            .map(|triangle| area2(triangle[0], triangle[1], triangle[2]).abs())
            .sum::<f64>();
        assert!((area - 160.0).abs() < EPSILON);
    }

    #[test]
    fn triangulates_holes_and_nested_islands() {
        let outer = rectangle(0.0, 0.0, 20.0, 20.0);
        let mut hole = rectangle(5.0, 5.0, 15.0, 15.0);
        hole.reverse();
        let mut island = rectangle(8.0, 8.0, 12.0, 12.0);
        island.reverse();
        let triangles =
            triangulate_d(&[outer, hole, island], FillRule::EvenOdd, TriangulationLimits::DEFAULT)
                .unwrap();
        let area = triangles
            .iter()
            .map(|triangle| area2(triangle[0], triangle[1], triangle[2]).abs())
            .sum::<f64>();
        assert!((area - 632.0).abs() < 1e-7);
    }

    #[test]
    fn fill_rules_follow_ring_orientation() {
        let mut clockwise = rectangle(0.0, 0.0, 10.0, 10.0);
        clockwise.reverse();
        assert!(
            triangulate_d(&[clockwise.clone()], FillRule::Positive, TriangulationLimits::DEFAULT,)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            triangulate_d(&[clockwise], FillRule::Negative, TriangulationLimits::DEFAULT)
                .unwrap()
                .len(),
            2
        );

        let twice = [rectangle(0.0, 0.0, 10.0, 10.0), rectangle(0.0, 0.0, 10.0, 10.0)];
        assert!(matches!(
            triangulate_d(&twice, FillRule::EvenOdd, TriangulationLimits::DEFAULT),
            Err(TriangulationError::Geometry(Error::IntersectingPaths))
        ));
    }

    #[test]
    fn rejects_invalid_and_intersecting_paths() {
        assert!(matches!(
            triangulate_path_d(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)],
                TriangulationLimits::DEFAULT,
            ),
            Err(TriangulationError::Geometry(Error::InvalidPath { .. }))
        ));
        let bow_tie = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
        ];
        assert_eq!(
            triangulate_path_d(&bow_tie, TriangulationLimits::DEFAULT),
            Err(TriangulationError::Geometry(Error::TopologyFailure))
        );
        let first = rectangle(0.0, 0.0, 10.0, 10.0);
        let second = rectangle(5.0, -1.0, 15.0, 5.0);
        assert_eq!(
            triangulate_d(&[first, second], FillRule::EvenOdd, TriangulationLimits::DEFAULT),
            Err(TriangulationError::Geometry(Error::IntersectingPaths))
        );
    }

    #[test]
    fn integer_api_returns_original_coordinate_type() {
        let path = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
        ];
        let triangles =
            triangulate64(&[path], FillRule::NonZero, TriangulationLimits::DEFAULT).unwrap();
        assert_eq!(triangles.len(), 2);
    }

    #[test]
    fn covers_single_paths_and_numeric_boundaries() {
        let excessive_span =
            vec![vec![Point64::new(0, 0), Point64::new((1_i64 << 53) + 1, 0), Point64::new(0, 1)]];
        assert_eq!(
            triangulate64(&excessive_span, FillRule::EvenOdd, TriangulationLimits::DEFAULT),
            Err(TriangulationError::Geometry(Error::ArithmeticOverflow))
        );
        let integer_path = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
        ];
        assert_eq!(
            triangulate64(
                std::slice::from_ref(&integer_path),
                FillRule::NonZero,
                TriangulationLimits::DEFAULT,
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            triangulate_path64(&integer_path, TriangulationLimits::DEFAULT).unwrap().len(),
            2
        );
        let double_path = rectangle(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            triangulate_d(
                std::slice::from_ref(&double_path),
                FillRule::NonZero,
                TriangulationLimits::DEFAULT,
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            triangulate_path_d(&double_path, TriangulationLimits::DEFAULT).unwrap().len(),
            2
        );
        assert_eq!(
            triangulate_d(&[Vec::new()], FillRule::EvenOdd, TriangulationLimits::DEFAULT),
            Ok(Vec::new())
        );

        let collapsed = vec![PointD::new(0.0, 0.0), PointD::new(0.0, 0.0), PointD::new(0.0, 0.0)];
        assert_eq!(
            triangulate_d(&[collapsed], FillRule::EvenOdd, TriangulationLimits::DEFAULT),
            Err(TriangulationError::Geometry(Error::TopologyFailure))
        );
        let collinear = vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)];
        assert_eq!(
            triangulate_d(&[collinear], FillRule::EvenOdd, TriangulationLimits::DEFAULT),
            Err(TriangulationError::Geometry(Error::TopologyFailure))
        );

        let huge = vec![PointD::new(1e308, 0.0), PointD::new(0.0, 1e308), PointD::new(-1e308, 0.0)];
        assert_eq!(
            triangulate_d(&[huge], FillRule::EvenOdd, TriangulationLimits::DEFAULT).unwrap().len(),
            1
        );

        let nonzero_crossing = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 8.0),
            PointD::new(10.0, 0.0),
            PointD::new(5.0, 2.0),
        ];
        assert_eq!(
            triangulate_d(&[nonzero_crossing], FillRule::EvenOdd, TriangulationLimits::DEFAULT,),
            Err(TriangulationError::Geometry(Error::TopologyFailure))
        );

        let translated_origin = (1_i64 << 53) + 1;
        let translated = vec![vec![
            Point64::new(translated_origin, 0),
            Point64::new(translated_origin + 10, 0),
            Point64::new(translated_origin + 10, 10),
            Point64::new(translated_origin, 10),
        ]];
        let translated_triangles =
            triangulate64(&translated, FillRule::NonZero, TriangulationLimits::DEFAULT)
                .expect("small integer geometry should triangulate independently of its origin");
        assert_eq!(translated_triangles.len(), 2);
        assert!(translated_triangles.iter().flatten().all(|point| point.x >= translated_origin));

        for bad in [
            PointD::new(f64::NAN, 0.0),
            PointD::new(0.0, f64::NAN),
            PointD::new(f64::INFINITY, 0.0),
            PointD::new(-2_f64.powi(63) - 4096.0, 0.0),
            PointD::new(2_f64.powi(63), 0.0),
            PointD::new(0.0, -2_f64.powi(63) - 4096.0),
            PointD::new(0.0, 2_f64.powi(63)),
        ] {
            assert_eq!(point_d_to_64(bad, Point64::new(0, 0)), Err(Error::ArithmeticOverflow));
        }
    }

    #[test]
    fn covers_triangulation_group_and_orientation_helpers() {
        let mut clockwise = rectangle(0.0, 0.0, 10.0, 10.0);
        clockwise.reverse();
        let triangles =
            triangulate_d(&[clockwise], FillRule::Negative, TriangulationLimits::DEFAULT).unwrap();
        assert_eq!(triangles.len(), 2);
        assert!(
            triangles.iter().all(|triangle| cross(triangle[0], triangle[1], triangle[2]) > 0.0)
        );

        let disjoint = triangulate_d(
            &[rectangle(0.0, 0.0, 20.0, 20.0), rectangle(30.0, 30.0, 31.0, 31.0)],
            FillRule::EvenOdd,
            TriangulationLimits::DEFAULT,
        )
        .unwrap();
        assert_eq!(disjoint.len(), 4);
        let reverse_nested = triangulate_d(
            &[
                rectangle(8.0, 8.0, 12.0, 12.0),
                rectangle(5.0, 5.0, 15.0, 15.0),
                rectangle(0.0, 0.0, 20.0, 20.0),
            ],
            FillRule::EvenOdd,
            TriangulationLimits::DEFAULT,
        )
        .unwrap();
        assert!(!reverse_nested.is_empty());

        assert!(validate_triangle_indices(&[0, 1, 2]).is_ok());
        assert_eq!(validate_triangle_indices(&[0]), Err(Error::TriangulationFailure));
        assert!(ensure_group_result(0, 1, true).is_ok());
        assert!(ensure_group_result(0, 0, false).is_ok());
        assert_eq!(ensure_group_result(0, 0, true), Err(Error::TriangulationFailure));

        let vertices = [PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(0.0, 1.0)];
        assert!(orient_triangle(&vertices, &vertices, &[0, 1, 2]).is_some());
        assert!(orient_triangle(&vertices, &vertices, &[0, 2, 1]).is_some());
        assert!(orient_triangle(&vertices, &vertices, &[0, 1, 1]).is_none());
        let mut oriented = Vec::new();
        push_oriented_triangle(&mut oriented, &vertices, &vertices, &[0, 1, 2]);
        push_oriented_triangle(&mut oriented, &vertices, &vertices, &[0, 1, 1]);
        assert_eq!(oriented.len(), 1);
    }
}
