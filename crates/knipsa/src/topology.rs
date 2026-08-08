//! Validated ring nesting and strongly typed polygon output.

use crate::{
    ComplexityLimits, Error, FillRule, Path64, PathD, Paths64, PathsD, Point64, PointD,
    geometry::{paths64_to_local_d, signed_area2_d},
    trim_collinear_d,
};
use core::cmp::Ordering;

pub(crate) const EPSILON: f64 = 1e-12;

/// One integer polygon with explicit ownership of its holes.
///
/// Values returned by [`build_polygons64`] are canonical: the outer ring is
/// counter-clockwise, holes are clockwise, and each unclosed ring starts at its
/// lexicographically smallest vertex. Directly constructed or deserialized
/// values are plain data and are not implicitly validated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Polygon64 {
    /// Outer boundary. [`build_polygons64`] returns it counter-clockwise.
    pub outer: Path64,
    /// Holes owned by this outer boundary. [`build_polygons64`] returns them
    /// clockwise.
    pub holes: Paths64,
}

/// One floating-point polygon with explicit ownership of its holes.
///
/// Values returned by [`build_polygons_d`] are canonical: the outer ring is
/// counter-clockwise, holes are clockwise, and each unclosed ring starts at its
/// lexicographically smallest vertex. Directly constructed or deserialized
/// values are plain data and are not implicitly validated.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PolygonD {
    /// Outer boundary. [`build_polygons_d`] returns it counter-clockwise.
    pub outer: PathD,
    /// Holes owned by this outer boundary. [`build_polygons_d`] returns them
    /// clockwise.
    pub holes: PathsD,
}

/// Builds polygons with explicit hole ownership from integer rings.
///
/// Arbitrarily nested islands become separate polygons. `fill_rule` decides
/// which nesting transitions create filled outer boundaries and holes. Output
/// winding, ring starts, hole order, and polygon order are canonical regardless
/// of input winding or order.
///
/// # Errors
///
/// Returns [`Error::LimitExceeded`] before conversion or quadratic topology
/// work when the configured budget is insufficient,
/// [`Error::InvalidPath`] for malformed rings,
/// [`Error::IntersectingPaths`] for touching or crossing rings,
/// [`Error::ArithmeticOverflow`] when coordinates do not fit the shared exact
/// integer-to-floating conversion frame, and [`Error::TopologyFailure`] for
/// degenerate topology. Integer coordinates are converted exactly before the
/// shared normalized floating-point topology predicates run.
pub fn build_polygons64(
    paths: &[Path64],
    fill_rule: FillRule,
    limits: ComplexityLimits,
) -> Result<Vec<Polygon64>, Error> {
    limits.check(paths.iter().map(Vec::len))?;
    let (origin, paths_d) = paths64_to_local_d(paths)?;
    let rings = collect_rings(&paths_d)?;
    polygons64_from_d(polygons_from_rings(&rings, fill_rule), origin)
}

/// Builds polygons with explicit hole ownership from floating-point rings.
///
/// Arbitrarily nested islands become separate polygons. `fill_rule` decides
/// which nesting transitions create filled outer boundaries and holes. Output
/// winding, ring starts, hole order, and polygon order are canonical regardless
/// of input winding or order, while coordinates remain in the caller's original
/// frame.
///
/// # Errors
///
/// Returns [`Error::LimitExceeded`] before normalization or quadratic topology
/// work when the configured budget is insufficient,
/// [`Error::InvalidPath`] for malformed rings,
/// [`Error::IntersectingPaths`] for touching or crossing rings,
/// and [`Error::TopologyFailure`] for degenerate topology.
pub fn build_polygons_d(
    paths: &[PathD],
    fill_rule: FillRule,
    limits: ComplexityLimits,
) -> Result<Vec<PolygonD>, Error> {
    limits.check(paths.iter().map(Vec::len))?;
    let rings = collect_rings(paths)?;
    Ok(polygons_from_rings(&rings, fill_rule))
}

#[derive(Clone, Debug)]
pub(crate) struct Ring {
    /// Coordinates in the shared, translation-free unit frame used by every
    /// topology predicate and by triangulation.
    pub(crate) path: PathD,
    /// Original caller coordinates returned through the public API.
    pub(crate) vertices: PathD,
    pub(crate) area: f64,
    pub(crate) parent: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct CoordinateFrame {
    /// Half of the deterministic lower-left bounding-box corner. Halving before
    /// subtraction keeps normalization finite across the full `f64` range.
    origin: PointD,
    /// Half of the longest bounding-box span.
    scale: f64,
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(crate) fn point_d_to_64(point: PointD, origin: Point64) -> Result<Point64, Error> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x < -2_f64.powi(63)
        || point.x >= 2_f64.powi(63)
        || point.y < -2_f64.powi(63)
        || point.y >= 2_f64.powi(63)
    {
        return Err(Error::ArithmeticOverflow);
    }
    let x = i128::from(point.x as i64) + i128::from(origin.x);
    let y = i128::from(point.y as i64) + i128::from(origin.y);
    Ok(Point64::new(
        i64::try_from(x).map_err(|_| Error::ArithmeticOverflow)?,
        i64::try_from(y).map_err(|_| Error::ArithmeticOverflow)?,
    ))
}

fn polygons_from_rings(rings: &[Ring], fill_rule: FillRule) -> Vec<PolygonD> {
    let mut polygons = filled_groups(rings, fill_rule)
        .into_iter()
        .map(|(outer, holes)| {
            let mut holes = holes
                .into_iter()
                .map(|hole| canonical_ring(&rings[hole].vertices, rings[hole].area, false))
                .collect::<PathsD>();
            holes.sort_by(|left, right| compare_paths(left, right));
            PolygonD {
                outer: canonical_ring(&rings[outer].vertices, rings[outer].area, true),
                holes,
            }
        })
        .collect::<Vec<_>>();
    polygons.sort_by(|left, right| compare_paths(&left.outer, &right.outer));
    polygons
}

fn canonical_ring(path: &[PointD], area: f64, outer: bool) -> PathD {
    let already_canonical = if outer { area > 0.0 } else { area < 0.0 };
    let mut canonical =
        if already_canonical { path.to_vec() } else { path.iter().rev().copied().collect() };
    let start = canonical
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| compare_points(**left, **right))
        .map_or(0, |(index, _)| index);
    canonical.rotate_left(start);
    canonical
}

fn compare_paths(left: &[PointD], right: &[PointD]) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(&left, &right)| compare_points(left, right))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

fn compare_points(left: PointD, right: PointD) -> Ordering {
    left.x
        .partial_cmp(&right.x)
        .expect("validated coordinates are finite")
        .then_with(|| left.y.partial_cmp(&right.y).expect("validated coordinates are finite"))
}

fn polygons64_from_d(polygons: Vec<PolygonD>, origin: Point64) -> Result<Vec<Polygon64>, Error> {
    polygons
        .into_iter()
        .map(|polygon| {
            Ok(Polygon64 {
                outer: path64_from_d(polygon.outer, origin)?,
                holes: polygon
                    .holes
                    .into_iter()
                    .map(|path| path64_from_d(path, origin))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

fn path64_from_d(path: PathD, origin: Point64) -> Result<Path64, Error> {
    path.into_iter().map(|point| point_d_to_64(point, origin)).collect()
}

pub(crate) fn collect_rings(paths: &[PathD]) -> Result<Vec<Ring>, Error> {
    let mut cleaned_paths = Vec::new();
    for path in paths {
        let path = trim_collinear_d(path, crate::PathKind::Closed)?;
        if path.is_empty() {
            continue;
        }
        if path.len() < 3 {
            return Err(Error::TopologyFailure);
        }
        cleaned_paths.push(path);
    }
    if cleaned_paths.is_empty() {
        return Ok(Vec::new());
    }

    let frame = CoordinateFrame::from_paths(&cleaned_paths)?;
    let mut rings = Vec::with_capacity(cleaned_paths.len());
    let mut probes = Vec::with_capacity(cleaned_paths.len());
    for vertices in cleaned_paths {
        let path = frame.normalize_path(&vertices);
        let area = signed_area2_d(&path);
        if area.abs() <= EPSILON || self_intersects(&path) {
            return Err(Error::TopologyFailure);
        }
        probes.push(interior_probe(&path, area).ok_or(Error::TopologyFailure)?);
        rings.push(Ring { path, vertices, area, parent: None });
    }

    for index in 0..rings.len() {
        for other in (index + 1)..rings.len() {
            if rings_intersect(&rings[index].path, &rings[other].path) {
                return Err(Error::IntersectingPaths);
            }
        }
    }
    for index in 0..rings.len() {
        let mut parent: Option<usize> = None;
        for other in 0..rings.len() {
            if index == other || rings[other].area.abs() <= rings[index].area.abs() {
                continue;
            }
            if point_in_path(probes[index], &rings[other].path)
                && parent.is_none_or(|current| rings[other].area.abs() < rings[current].area.abs())
            {
                parent = Some(other);
            }
        }
        rings[index].parent = parent;
    }
    Ok(rings)
}

impl CoordinateFrame {
    fn from_paths(paths: &[PathD]) -> Result<Self, Error> {
        let first = paths.iter().flatten().next().copied().ok_or(Error::TopologyFailure)?;
        let (mut min_x, mut max_x) = (first.x, first.x);
        let (mut min_y, mut max_y) = (first.y, first.y);
        for point in paths.iter().flatten() {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }
        let origin = PointD::new(min_x * 0.5, min_y * 0.5);
        let scale = (max_x * 0.5 - origin.x).max(max_y * 0.5 - origin.y);
        if scale == 0.0 {
            return Err(Error::TopologyFailure);
        }
        Ok(Self { origin, scale })
    }

    fn normalize_path(self, path: &[PointD]) -> PathD {
        path.iter()
            .map(|point| {
                PointD::new(
                    (point.x * 0.5 - self.origin.x) / self.scale,
                    (point.y * 0.5 - self.origin.y) / self.scale,
                )
            })
            .collect()
    }
}

pub(crate) fn filled_groups(rings: &[Ring], fill_rule: FillRule) -> Vec<(usize, Vec<usize>)> {
    let mut outer_rings = Vec::new();
    let mut hole_rings = Vec::new();
    for index in 0..rings.len() {
        let mut ancestors = Vec::new();
        let mut current = rings[index].parent;
        while let Some(parent) = current {
            ancestors.push(parent);
            current = rings[parent].parent;
        }
        ancestors.reverse();
        let before = winding_value(&ancestors, rings, fill_rule);
        let after = winding_value_with_current(&ancestors, index, rings, fill_rule);
        let before_filled = is_filled(before, fill_rule);
        let after_filled = is_filled(after, fill_rule);
        if !before_filled && after_filled {
            outer_rings.push(index);
        } else if before_filled && !after_filled {
            hole_rings.push(index);
        }
    }

    outer_rings
        .iter()
        .copied()
        .map(|outer| {
            let holes = hole_rings
                .iter()
                .copied()
                .filter(|hole| nearest_outer_ancestor(*hole, rings, &outer_rings) == Some(outer))
                .collect();
            (outer, holes)
        })
        .collect()
}

fn nearest_outer_ancestor(ring: usize, rings: &[Ring], outer_rings: &[usize]) -> Option<usize> {
    let mut current = rings[ring].parent;
    while let Some(index) = current {
        if outer_rings.contains(&index) {
            return Some(index);
        }
        current = rings[index].parent;
    }
    None
}

fn winding_value(ancestors: &[usize], rings: &[Ring], fill_rule: FillRule) -> i32 {
    match fill_rule {
        FillRule::EvenOdd => i32::from(!ancestors.len().is_multiple_of(2)),
        FillRule::NonZero | FillRule::Positive | FillRule::Negative => {
            ancestors.iter().map(|index| sign(rings[*index].area)).sum()
        }
    }
}

fn winding_value_with_current(
    ancestors: &[usize],
    current: usize,
    rings: &[Ring],
    fill_rule: FillRule,
) -> i32 {
    match fill_rule {
        FillRule::EvenOdd => i32::from(ancestors.len().is_multiple_of(2)),
        FillRule::NonZero | FillRule::Positive | FillRule::Negative => {
            winding_value(ancestors, rings, fill_rule) + sign(rings[current].area)
        }
    }
}

fn is_filled(value: i32, fill_rule: FillRule) -> bool {
    match fill_rule {
        FillRule::EvenOdd | FillRule::NonZero => value != 0,
        FillRule::Positive => value > 0,
        FillRule::Negative => value < 0,
    }
}

fn sign(value: f64) -> i32 {
    if value.is_sign_positive() { 1 } else { -1 }
}

pub(crate) fn cross(first: PointD, second: PointD, third: PointD) -> f64 {
    (second.x - first.x) * (third.y - first.y) - (second.y - first.y) * (third.x - first.x)
}

fn interior_probe(path: &[PointD], area: f64) -> Option<PointD> {
    let scale =
        path.iter().flat_map(|point| [point.x.abs(), point.y.abs()]).fold(1.0_f64, f64::max);
    let inward_sign = if area > 0.0 { 1.0 } else { -1.0 };
    for pair in path.windows(2).chain(std::iter::once(&[path[path.len() - 1], path[0]][..])) {
        let direction = PointD::new(pair[1].x - pair[0].x, pair[1].y - pair[0].y);
        let length = direction.x.hypot(direction.y);
        if length <= EPSILON {
            continue;
        }
        let normal =
            PointD::new(-direction.y / length * inward_sign, direction.x / length * inward_sign);
        let midpoint = PointD::new((pair[0].x + pair[1].x) * 0.5, (pair[0].y + pair[1].y) * 0.5);
        for exponent in [7_i32, 9, 11, 13] {
            let distance = scale * 10_f64.powi(-exponent);
            let candidate =
                PointD::new(midpoint.x + normal.x * distance, midpoint.y + normal.y * distance);
            if point_in_path(candidate, path) {
                return Some(candidate);
            }
        }
    }
    None
}

fn point_in_path(point: PointD, path: &[PointD]) -> bool {
    let mut inside = false;
    for (first, second) in
        path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len())
    {
        if on_segment(point, first, second) {
            return true;
        }
        if (first.y > point.y) != (second.y > point.y) {
            let cross = cross(first, second, point);
            if (second.y > first.y && cross > 0.0) || (second.y < first.y && cross < 0.0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn self_intersects(path: &[PointD]) -> bool {
    let edge_count = path.len();
    for first in 0..edge_count {
        let first_end = (first + 1) % edge_count;
        for second in (first + 1)..edge_count {
            let second_end = (second + 1) % edge_count;
            if first_end == second || second_end == first {
                continue;
            }
            if segments_intersect(path[first], path[first_end], path[second], path[second_end]) {
                return true;
            }
        }
    }
    false
}

fn rings_intersect(first: &[PointD], second: &[PointD]) -> bool {
    for first_index in 0..first.len() {
        let first_end = (first_index + 1) % first.len();
        for second_index in 0..second.len() {
            let second_end = (second_index + 1) % second.len();
            if segments_intersect(
                first[first_index],
                first[first_end],
                second[second_index],
                second[second_end],
            ) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(
    first: PointD,
    first_end: PointD,
    second: PointD,
    second_end: PointD,
) -> bool {
    let ab_c = cross(first, first_end, second);
    let ab_d = cross(first, first_end, second_end);
    let cd_a = cross(second, second_end, first);
    let cd_b = cross(second, second_end, first_end);
    if ab_c.abs() <= EPSILON && on_segment(second, first, first_end)
        || ab_d.abs() <= EPSILON && on_segment(second_end, first, first_end)
        || cd_a.abs() <= EPSILON && on_segment(first, second, second_end)
        || cd_b.abs() <= EPSILON && on_segment(first_end, second, second_end)
    {
        return true;
    }
    (ab_c > 0.0) != (ab_d > 0.0) && (cd_a > 0.0) != (cd_b > 0.0)
}

fn on_segment(point: PointD, first: PointD, second: PointD) -> bool {
    cross(first, second, point).abs() <= EPSILON
        && point.x >= first.x.min(second.x) - EPSILON
        && point.x <= first.x.max(second.x) + EPSILON
        && point.y >= first.y.min(second.y) - EPSILON
        && point.y <= first.y.max(second.y) + EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComplexityResource;

    fn build_polygons64(paths: &[Path64], fill_rule: FillRule) -> Result<Vec<Polygon64>, Error> {
        super::build_polygons64(paths, fill_rule, ComplexityLimits::DEFAULT)
    }

    fn build_polygons_d(paths: &[PathD], fill_rule: FillRule) -> Result<Vec<PolygonD>, Error> {
        super::build_polygons_d(paths, fill_rule, ComplexityLimits::DEFAULT)
    }

    fn rectangle(left: f64, bottom: f64, right: f64, top: f64) -> PathD {
        vec![
            PointD::new(left, bottom),
            PointD::new(right, bottom),
            PointD::new(right, top),
            PointD::new(left, top),
        ]
    }

    #[test]
    fn complexity_limits_are_deterministic_and_precede_geometry_work() {
        let exact = ComplexityLimits::new(2, 8, 20);
        assert_eq!(exact.max_paths(), 2);
        assert_eq!(exact.max_vertices(), 8);
        assert_eq!(exact.max_edge_pairs(), 20);
        assert_eq!(ComplexityLimits::default(), ComplexityLimits::DEFAULT);
        assert_eq!(exact.check([4, 4]), Ok(()));

        for (limits, expected) in [
            (
                ComplexityLimits::new(1, 0, 0),
                Error::LimitExceeded { resource: ComplexityResource::Paths, limit: 1, required: 2 },
            ),
            (
                ComplexityLimits::new(2, 7, 0),
                Error::LimitExceeded {
                    resource: ComplexityResource::Vertices,
                    limit: 7,
                    required: 8,
                },
            ),
            (
                ComplexityLimits::new(2, 8, 19),
                Error::LimitExceeded {
                    resource: ComplexityResource::EdgePairs,
                    limit: 19,
                    required: 20,
                },
            ),
        ] {
            assert_eq!(limits.check([4, 4]), Err(expected));
        }
        assert_eq!(
            ComplexityLimits::new(2, usize::MAX, usize::MAX - 1).check([usize::MAX, usize::MAX]),
            Err(Error::LimitExceeded {
                resource: ComplexityResource::EdgePairs,
                limit: usize::MAX - 1,
                required: usize::MAX,
            })
        );

        let invalid = vec![vec![PointD::new(f64::NAN, 0.0); 4], rectangle(0.0, 0.0, 1.0, 1.0)];
        assert_eq!(
            super::build_polygons_d(
                &invalid,
                FillRule::EvenOdd,
                ComplexityLimits::new(1, usize::MAX, usize::MAX),
            ),
            Err(Error::LimitExceeded {
                resource: ComplexityResource::Paths,
                limit: 1,
                required: 2,
            })
        );
    }

    #[test]
    fn assigns_holes_islands_and_canonical_winding() {
        let outer = rectangle(0.0, 0.0, 10.0, 10.0);
        let hole = rectangle(2.0, 2.0, 8.0, 8.0);
        let island = rectangle(4.0, 4.0, 6.0, 6.0);
        let polygons =
            build_polygons_d(&[outer.clone(), hole.clone(), island.clone()], FillRule::EvenOdd)
                .unwrap();
        assert_eq!(polygons.len(), 2);
        assert_eq!(polygons[0].outer, outer);
        assert_eq!(
            polygons[0].holes,
            vec![vec![
                PointD::new(2.0, 2.0),
                PointD::new(2.0, 8.0),
                PointD::new(8.0, 8.0),
                PointD::new(8.0, 2.0),
            ]]
        );
        assert_eq!(polygons[1].outer, island);
        assert!(polygons[1].holes.is_empty());
        assert!(signed_area2_d(&polygons[0].outer) > 0.0);
        assert!(signed_area2_d(&polygons[0].holes[0]) < 0.0);

        let two_holes = build_polygons_d(
            &[outer.clone(), rectangle(6.0, 2.0, 8.0, 4.0), rectangle(2.0, 2.0, 4.0, 4.0)],
            FillRule::EvenOdd,
        )
        .unwrap();
        assert_eq!(two_holes[0].holes[0][0], PointD::new(2.0, 2.0));
        assert_eq!(two_holes[0].holes[1][0], PointD::new(6.0, 2.0));
        assert_eq!(
            compare_paths(&[PointD::new(0.0, 0.0)], &rectangle(0.0, 0.0, 1.0, 1.0)),
            Ordering::Less
        );

        let base = [outer.clone(), hole, island.clone()];
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            for reverse_mask in 0_u8..8 {
                let paths = order
                    .into_iter()
                    .enumerate()
                    .map(|(position, index)| {
                        let mut path = base[index].clone();
                        if reverse_mask & (1 << position) != 0 {
                            path.reverse();
                        }
                        path
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    build_polygons_d(&paths, FillRule::EvenOdd).unwrap(),
                    polygons,
                    "order={order:?} reverse_mask={reverse_mask}"
                );
            }
        }

        let integer_paths = vec![
            vec![
                Point64::new(i64::MAX - 10, i64::MAX - 10),
                Point64::new(i64::MAX, i64::MAX - 10),
                Point64::new(i64::MAX, i64::MAX),
                Point64::new(i64::MAX - 10, i64::MAX),
            ],
            vec![
                Point64::new(i64::MAX - 8, i64::MAX - 8),
                Point64::new(i64::MAX - 2, i64::MAX - 8),
                Point64::new(i64::MAX - 2, i64::MAX - 2),
                Point64::new(i64::MAX - 8, i64::MAX - 2),
            ],
        ];
        let integer = build_polygons64(&integer_paths, FillRule::EvenOdd).unwrap();
        assert_eq!(integer.len(), 1);
        assert_eq!(integer[0].outer, integer_paths[0]);
        assert_eq!(integer[0].holes.len(), 1);
        assert!(crate::signed_area2(&integer[0].outer).unwrap() > 0);
        assert!(crate::signed_area2(&integer[0].holes[0]).unwrap() < 0);
    }

    #[test]
    fn follows_fill_rules_and_reports_topology_errors() {
        let outer = rectangle(0.0, 0.0, 10.0, 10.0);
        let nested = rectangle(2.0, 2.0, 8.0, 8.0);
        let non_zero = build_polygons_d(&[outer, nested], FillRule::NonZero).unwrap();
        assert_eq!(non_zero.len(), 1);
        assert!(non_zero[0].holes.is_empty());

        let bow_tie = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
        ];
        assert_eq!(build_polygons_d(&[bow_tie], FillRule::EvenOdd), Err(Error::TopologyFailure));

        let late_crossing = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, -10.0),
            PointD::new(5.0, -5.0),
            PointD::new(5.0, 5.0),
        ];
        assert_eq!(
            build_polygons_d(&[late_crossing], FillRule::EvenOdd),
            Err(Error::TopologyFailure)
        );

        assert_eq!(
            build_polygons_d(
                &[rectangle(0.0, 0.0, 10.0, 10.0), rectangle(5.0, -1.0, 15.0, 5.0)],
                FillRule::EvenOdd,
            ),
            Err(Error::IntersectingPaths)
        );
    }

    #[test]
    fn floating_nesting_is_scale_and_translation_invariant() {
        for exponent in -12..=12 {
            let scale = 10_f64.powi(exponent);
            let polygons = build_polygons_d(
                &[
                    rectangle(0.0, 0.0, scale, scale),
                    rectangle(0.2 * scale, 0.2 * scale, 0.8 * scale, 0.8 * scale),
                    rectangle(0.4 * scale, 0.4 * scale, 0.6 * scale, 0.6 * scale),
                ],
                FillRule::EvenOdd,
            )
            .unwrap();
            assert_eq!(polygons.len(), 2, "scale exponent {exponent}");
            assert_eq!(polygons[0].holes.len(), 1, "scale exponent {exponent}");
        }

        let origin = 1e12;
        let polygons = build_polygons_d(
            &[
                rectangle(origin, origin, origin + 100.0, origin + 100.0),
                rectangle(origin + 20.0, origin + 20.0, origin + 80.0, origin + 80.0),
            ],
            FillRule::EvenOdd,
        )
        .unwrap();
        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].holes.len(), 1);
        assert_eq!(polygons[0].outer[0], PointD::new(origin, origin));

        let exact_limit = 1_i64 << 53;
        let integer_ring = vec![
            Point64::new(-exact_limit, 0),
            Point64::new(0, exact_limit),
            Point64::new(exact_limit, 0),
        ];
        for start in 0..integer_ring.len() {
            let mut rotated = integer_ring.clone();
            rotated.rotate_left(start);
            assert_eq!(
                build_polygons64(&[rotated], FillRule::EvenOdd),
                Err(Error::ArithmeticOverflow),
                "ring start {start}"
            );
        }
    }

    fn ring(area: f64, parent: Option<usize>) -> Ring {
        Ring { path: Vec::new(), vertices: Vec::new(), area, parent }
    }

    #[test]
    fn covers_nesting_predicates_and_coordinate_frames() {
        assert!(interior_probe(&[PointD::new(0.0, 0.0), PointD::new(0.0, 0.0)], 1.0).is_none());
        assert!(interior_probe(&rectangle(0.0, 0.0, 100.0, 100.0), -1.0).is_none());
        assert!(point_in_path(PointD::new(0.0, 0.0), &rectangle(0.0, 0.0, 1.0, 1.0)));
        assert!(point_in_path(PointD::new(0.5, 0.5), &rectangle(0.0, 0.0, 1.0, 1.0)));
        assert!(!point_in_path(PointD::new(2.0, 0.5), &rectangle(0.0, 0.0, 1.0, 1.0)));

        let rings = vec![ring(100.0, None), ring(-25.0, Some(0)), ring(4.0, Some(1))];
        assert_eq!(nearest_outer_ancestor(1, &rings, &[0]), Some(0));
        assert_eq!(nearest_outer_ancestor(2, &rings, &[0]), Some(0));
        assert_eq!(nearest_outer_ancestor(1, &rings, &[]), None);
        assert_eq!(filled_groups(&rings, FillRule::EvenOdd).len(), 2);
        let same_winding = vec![ring(100.0, None), ring(25.0, Some(0))];
        assert_eq!(filled_groups(&same_winding, FillRule::NonZero).len(), 1);

        let ancestors = [0_usize, 1];
        for fill_rule in
            [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        {
            assert_eq!(winding_value(&ancestors, &rings, fill_rule), 0);
            assert_eq!(winding_value_with_current(&ancestors, 2, &rings, fill_rule), 1);
        }
        assert!(is_filled(1, FillRule::EvenOdd));
        assert!(is_filled(1, FillRule::NonZero));
        assert!(is_filled(1, FillRule::Positive));
        assert!(!is_filled(1, FillRule::Negative));
        assert!(!is_filled(-1, FillRule::Positive));
        assert!(is_filled(-1, FillRule::Negative));
        assert_eq!(sign(1.0), 1);
        assert_eq!(sign(-1.0), -1);

        let degenerate = vec![vec![PointD::new(1.0, 1.0); 3]];
        assert_eq!(CoordinateFrame::from_paths(&degenerate).unwrap_err(), Error::TopologyFailure);
        let extreme = vec![vec![
            PointD::new(f64::MAX, 0.0),
            PointD::new(-f64::MAX, 0.0),
            PointD::new(0.0, 1.0),
        ]];
        let frame = CoordinateFrame::from_paths(&extreme).unwrap();
        assert_eq!(frame.origin.x.to_bits(), (-f64::MAX * 0.5).to_bits());
        assert_eq!(frame.scale.to_bits(), f64::MAX.to_bits());
    }

    #[test]
    fn covers_segment_predicates() {
        let horizontal_start = PointD::new(0.0, 0.0);
        let horizontal_end = PointD::new(10.0, 0.0);
        for (first, first_end, second, second_end) in [
            (horizontal_start, horizontal_end, PointD::new(2.0, 0.0), PointD::new(2.0, 1.0)),
            (horizontal_start, horizontal_end, PointD::new(2.0, 1.0), PointD::new(2.0, 0.0)),
            (PointD::new(2.0, 0.0), PointD::new(2.0, 1.0), horizontal_start, horizontal_end),
            (PointD::new(2.0, 1.0), PointD::new(2.0, 0.0), horizontal_start, horizontal_end),
            (
                PointD::new(0.0, 0.0),
                PointD::new(10.0, 10.0),
                PointD::new(0.0, 10.0),
                PointD::new(10.0, 0.0),
            ),
        ] {
            assert!(segments_intersect(first, first_end, second, second_end));
        }
        assert!(!segments_intersect(
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, 1.0),
            PointD::new(1.0, 1.0),
        ));
        assert!(on_segment(PointD::new(5.0, 0.0), horizontal_start, horizontal_end));
        for point in [
            PointD::new(11.0, 0.0),
            PointD::new(-1.0, 0.0),
            PointD::new(5.0, 1.0),
            PointD::new(5.0, -1.0),
        ] {
            assert!(!on_segment(point, horizontal_start, horizontal_end));
        }
        let vertical_start = PointD::new(0.0, 0.0);
        let vertical_end = PointD::new(0.0, 10.0);
        assert!(!on_segment(PointD::new(0.0, -1.0), vertical_start, vertical_end));
        assert!(!on_segment(PointD::new(0.0, 11.0), vertical_start, vertical_end));
        for (first, first_end, second, second_end) in [
            (horizontal_start, horizontal_end, PointD::new(20.0, 0.0), PointD::new(20.0, 1.0)),
            (horizontal_start, horizontal_end, PointD::new(20.0, 1.0), PointD::new(20.0, 0.0)),
            (PointD::new(20.0, 0.0), PointD::new(20.0, 1.0), horizontal_start, horizontal_end),
            (PointD::new(20.0, 1.0), PointD::new(20.0, 0.0), horizontal_start, horizontal_end),
        ] {
            assert!(!segments_intersect(first, first_end, second, second_end));
        }
        assert!(!rings_intersect(&rectangle(0.0, 0.0, 1.0, 1.0), &rectangle(2.0, 2.0, 3.0, 3.0)));
        assert!(!self_intersects(&rectangle(0.0, 0.0, 1.0, 1.0)));
        assert!(self_intersects(&[
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 8.0),
            PointD::new(10.0, 0.0),
            PointD::new(5.0, 2.0),
        ]));
    }
}
