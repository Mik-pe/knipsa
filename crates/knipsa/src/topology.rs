//! Validated ring nesting and strongly typed polygon output.

use crate::{
    ComplexityLimits, Error, FillRule, Path64, PathD, Paths64, PathsD, Point64, PointD,
    geometry::{cross_ordering, signed_area2_d},
    trim_collinear_d, trim_collinear64,
};
use core::cmp::Ordering;
use num_bigint::BigInt;
use num_traits::Signed;

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
/// Returns [`Error::LimitExceeded`] before quadratic topology
/// work when the configured budget is insufficient,
/// [`Error::InvalidPath`] for malformed rings,
/// [`Error::IntersectingPaths`] for touching or crossing rings,
/// and [`Error::TopologyFailure`] for degenerate topology. All topology
/// decisions are exact across the complete `i64` coordinate domain.
pub fn build_polygons64(
    paths: &[Path64],
    fill_rule: FillRule,
    limits: ComplexityLimits,
) -> Result<Vec<Polygon64>, Error> {
    limits.check(paths.iter().map(Vec::len))?;
    let rings = collect_rings64(paths)?;
    Ok(polygons64_from_rings(&rings, fill_rule))
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
    let rings = collect_rings_d(paths)?;
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

#[derive(Clone, Debug)]
pub(crate) struct Ring64 {
    pub(crate) vertices: Path64,
    area: ExactArea,
    sign: i32,
    parent: Option<usize>,
}

#[derive(Clone, Debug)]
enum ExactArea {
    Small(i128),
    Big(BigInt),
}

fn polygons64_from_rings(rings: &[Ring64], fill_rule: FillRule) -> Vec<Polygon64> {
    let mut polygons = filled_groups(rings, fill_rule)
        .into_iter()
        .map(|(outer, holes)| {
            let mut holes = holes
                .into_iter()
                .map(|hole| canonical_ring64(&rings[hole].vertices, rings[hole].sign, false))
                .collect::<Paths64>();
            holes.sort_by(|left, right| compare_paths64(left, right));
            Polygon64 {
                outer: canonical_ring64(&rings[outer].vertices, rings[outer].sign, true),
                holes,
            }
        })
        .collect::<Vec<_>>();
    polygons.sort_by(|left, right| compare_paths64(&left.outer, &right.outer));
    polygons
}

fn compare_paths64(left: &[Point64], right: &[Point64]) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.x.cmp(&right.x).then_with(|| left.y.cmp(&right.y)))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

fn canonical_ring64(path: &[Point64], sign: i32, outer: bool) -> Path64 {
    let mut canonical = if (outer && sign > 0) || (!outer && sign < 0) {
        path.to_vec()
    } else {
        path.iter().rev().copied().collect()
    };
    let start = canonical
        .iter()
        .enumerate()
        .min_by_key(|(_, point)| (point.x, point.y))
        .map_or(0, |(index, _)| index);
    canonical.rotate_left(start);
    canonical
}

pub(crate) fn collect_rings64(paths: &[Path64]) -> Result<Vec<Ring64>, Error> {
    let mut rings = Vec::new();
    for path in paths {
        let vertices = trim_collinear64(path, crate::PathKind::Closed)?;
        if vertices.is_empty() {
            continue;
        }
        if vertices.len() < 3 {
            return Err(Error::TopologyFailure);
        }
        let (area, sign) = exact_area(&vertices);
        if sign == 0 || self_intersects_by(&vertices, segments_intersect64) {
            return Err(Error::TopologyFailure);
        }
        rings.push(Ring64 { vertices, area, sign, parent: None });
    }
    assign_parents(
        &mut rings,
        |left, right| compare_exact_area(&left.area, &right.area),
        |child, parent| point_in_path64(child.vertices[0], &parent.vertices),
        |left, right| rings_intersect_by(&left.vertices, &right.vertices, segments_intersect64),
    )?;
    Ok(rings)
}

pub(crate) fn collect_rings_d(paths: &[PathD]) -> Result<Vec<Ring>, Error> {
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
    for vertices in cleaned_paths {
        let path = frame.normalize_path(&vertices);
        let area = signed_area2_d(&path);
        if area.abs() <= EPSILON || self_intersects_by(&path, segments_intersect) {
            return Err(Error::TopologyFailure);
        }
        rings.push(Ring { path, vertices, area, parent: None });
    }
    assign_parents(
        &mut rings,
        |left, right| left.area.abs().partial_cmp(&right.area.abs()).expect("finite area"),
        |child, parent| point_in_path(child.path[0], &parent.path),
        |left, right| rings_intersect_by(&left.path, &right.path, segments_intersect),
    )?;
    Ok(rings)
}

pub(crate) trait NestedRing {
    fn parent(&self) -> Option<usize>;
    fn set_parent(&mut self, parent: Option<usize>);
    fn sign(&self) -> i32;
}

impl NestedRing for Ring {
    fn parent(&self) -> Option<usize> {
        self.parent
    }
    fn set_parent(&mut self, parent: Option<usize>) {
        self.parent = parent;
    }
    fn sign(&self) -> i32 {
        if self.area > 0.0 { 1 } else { -1 }
    }
}

impl NestedRing for Ring64 {
    fn parent(&self) -> Option<usize> {
        self.parent
    }
    fn set_parent(&mut self, parent: Option<usize>) {
        self.parent = parent;
    }
    fn sign(&self) -> i32 {
        self.sign
    }
}

fn assign_parents<R>(
    rings: &mut [R],
    compare_area: impl Fn(&R, &R) -> Ordering,
    contains: impl Fn(&R, &R) -> bool,
    intersects: impl Fn(&R, &R) -> bool,
) -> Result<(), Error>
where
    R: NestedRing,
{
    for index in 0..rings.len() {
        for other in (index + 1)..rings.len() {
            if intersects(&rings[index], &rings[other]) {
                return Err(Error::IntersectingPaths);
            }
        }
    }
    for index in 0..rings.len() {
        let mut parent = None;
        for other in 0..rings.len() {
            if index == other || compare_area(&rings[other], &rings[index]) != Ordering::Greater {
                continue;
            }
            if contains(&rings[index], &rings[other])
                && parent.is_none_or(|current| {
                    compare_area(&rings[other], &rings[current]) == Ordering::Less
                })
            {
                parent = Some(other);
            }
        }
        rings[index].set_parent(parent);
    }
    Ok(())
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

pub(crate) fn filled_groups<R: NestedRing>(
    rings: &[R],
    fill_rule: FillRule,
) -> Vec<(usize, Vec<usize>)> {
    let mut outer_rings = Vec::new();
    let mut hole_rings = Vec::new();
    for index in 0..rings.len() {
        let mut ancestors = Vec::new();
        let mut current = rings[index].parent();
        while let Some(parent) = current {
            ancestors.push(parent);
            current = rings[parent].parent();
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

fn nearest_outer_ancestor<R: NestedRing>(
    ring: usize,
    rings: &[R],
    outer_rings: &[usize],
) -> Option<usize> {
    let mut current = rings[ring].parent();
    while let Some(index) = current {
        if outer_rings.contains(&index) {
            return Some(index);
        }
        current = rings[index].parent();
    }
    None
}

fn winding_value<R: NestedRing>(ancestors: &[usize], rings: &[R], fill_rule: FillRule) -> i32 {
    match fill_rule {
        FillRule::EvenOdd => i32::from(!ancestors.len().is_multiple_of(2)),
        FillRule::NonZero | FillRule::Positive | FillRule::Negative => {
            ancestors.iter().map(|index| rings[*index].sign()).sum()
        }
    }
}

fn winding_value_with_current<R: NestedRing>(
    ancestors: &[usize],
    current: usize,
    rings: &[R],
    fill_rule: FillRule,
) -> i32 {
    match fill_rule {
        FillRule::EvenOdd => i32::from(ancestors.len().is_multiple_of(2)),
        FillRule::NonZero | FillRule::Positive | FillRule::Negative => {
            winding_value(ancestors, rings, fill_rule) + rings[current].sign()
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

pub(crate) fn cross(first: PointD, second: PointD, third: PointD) -> f64 {
    (second.x - first.x) * (third.y - first.y) - (second.y - first.y) * (third.x - first.x)
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

fn self_intersects_by<P: Copy, F>(path: &[P], intersects: F) -> bool
where
    F: Fn(P, P, P, P) -> bool + Copy,
{
    let edge_count = path.len();
    for first in 0..edge_count {
        let first_end = (first + 1) % edge_count;
        for second in (first + 1)..edge_count {
            let second_end = (second + 1) % edge_count;
            if first_end == second || second_end == first {
                continue;
            }
            if intersects(path[first], path[first_end], path[second], path[second_end]) {
                return true;
            }
        }
    }
    false
}

fn rings_intersect_by<P: Copy, F>(first: &[P], second: &[P], intersects: F) -> bool
where
    F: Fn(P, P, P, P) -> bool + Copy,
{
    for first_index in 0..first.len() {
        let first_end = (first_index + 1) % first.len();
        for second_index in 0..second.len() {
            let second_end = (second_index + 1) % second.len();
            if intersects(
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

fn exact_area(path: &[Point64]) -> (ExactArea, i32) {
    let mut area = 0_i128;
    let small = path
        .iter()
        .copied()
        .zip(path.iter().copied().cycle().skip(1))
        .take(path.len())
        .try_for_each(|(a, b)| {
            let term = i128::from(a.x)
                .checked_mul(i128::from(b.y))?
                .checked_sub(i128::from(a.y).checked_mul(i128::from(b.x))?)?;
            area = area.checked_add(term)?;
            Some(())
        });
    if small.is_some() {
        let sign = match area.cmp(&0) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        };
        return (ExactArea::Small(area), sign);
    }
    let area =
        path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len()).fold(
            BigInt::from(0),
            |sum, (a, b)| {
                sum + BigInt::from(a.x) * BigInt::from(b.y) - BigInt::from(a.y) * BigInt::from(b.x)
            },
        );
    let sign = match area.cmp(&BigInt::from(0)) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    (ExactArea::Big(area), sign)
}

fn compare_exact_area(left: &ExactArea, right: &ExactArea) -> Ordering {
    match (left, right) {
        (ExactArea::Small(left), ExactArea::Small(right)) => {
            left.unsigned_abs().cmp(&right.unsigned_abs())
        }
        _ => exact_area_big(left).abs().cmp(&exact_area_big(right).abs()),
    }
}

fn exact_area_big(area: &ExactArea) -> BigInt {
    match area {
        ExactArea::Small(value) => BigInt::from(*value),
        ExactArea::Big(value) => value.clone(),
    }
}

fn on_segment64(point: Point64, a: Point64, b: Point64) -> bool {
    point.x >= a.x.min(b.x)
        && point.x <= a.x.max(b.x)
        && point.y >= a.y.min(b.y)
        && point.y <= a.y.max(b.y)
}

fn segments_intersect64(a: Point64, b: Point64, c: Point64, d: Point64) -> bool {
    let ab_c = cross_ordering(a, b, c);
    let ab_d = cross_ordering(a, b, d);
    let cd_a = cross_ordering(c, d, a);
    let cd_b = cross_ordering(c, d, b);
    if ab_c == Ordering::Equal && on_segment64(c, a, b)
        || ab_d == Ordering::Equal && on_segment64(d, a, b)
        || cd_a == Ordering::Equal && on_segment64(a, c, d)
        || cd_b == Ordering::Equal && on_segment64(b, c, d)
    {
        return true;
    }
    ((ab_c == Ordering::Less && ab_d == Ordering::Greater)
        || (ab_c == Ordering::Greater && ab_d == Ordering::Less))
        && ((cd_a == Ordering::Less && cd_b == Ordering::Greater)
            || (cd_a == Ordering::Greater && cd_b == Ordering::Less))
}

fn point_in_path64(point: Point64, path: &[Point64]) -> bool {
    let mut inside = false;
    for (a, b) in path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len()) {
        if (a.y > point.y) != (b.y > point.y) {
            let orientation = cross_ordering(a, b, point);
            if (b.y > a.y && orientation == Ordering::Greater)
                || (b.y < a.y && orientation == Ordering::Less)
            {
                inside = !inside;
            }
        }
    }
    inside
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
        let expected =
            build_polygons64(std::slice::from_ref(&integer_ring), FillRule::EvenOdd).unwrap();
        for start in 0..integer_ring.len() {
            let mut rotated = integer_ring.clone();
            rotated.rotate_left(start);
            assert_eq!(
                build_polygons64(&[rotated], FillRule::EvenOdd).unwrap(),
                expected,
                "ring start {start}"
            );
        }
    }

    fn rectangle64(left: i64, bottom: i64, right: i64, top: i64) -> Path64 {
        vec![
            Point64::new(left, bottom),
            Point64::new(right, bottom),
            Point64::new(right, top),
            Point64::new(left, top),
        ]
    }

    #[test]
    fn integer_topology_is_exact_across_full_domain() {
        let outer = rectangle64(i64::MIN, i64::MIN, i64::MAX, i64::MAX);
        let hole = rectangle64(-1, -1, 1, 1);
        let expected = build_polygons64(&[outer.clone(), hole.clone()], FillRule::EvenOdd).unwrap();
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].outer, outer);
        assert_eq!(
            expected[0].holes,
            vec![vec![
                Point64::new(-1, -1),
                Point64::new(-1, 1),
                Point64::new(1, 1),
                Point64::new(1, -1),
            ]]
        );

        for reverse_mask in 0..4 {
            let mut paths = vec![outer.clone(), hole.clone()];
            if reverse_mask & 1 != 0 {
                paths[0].reverse();
            }
            if reverse_mask & 2 != 0 {
                paths[1].reverse();
            }
            paths.reverse();
            for path in &mut paths {
                path.rotate_left(1);
            }
            assert_eq!(build_polygons64(&paths, FillRule::EvenOdd).unwrap(), expected);
        }

        let thin =
            vec![Point64::new(i64::MIN, 0), Point64::new(i64::MAX, 0), Point64::new(i64::MAX, 1)];
        assert_eq!(build_polygons64(&[thin], FillRule::EvenOdd).unwrap().len(), 1);

        let local = vec![rectangle64(0, 0, 100, 100), rectangle64(20, 20, 80, 80)];
        let translated = vec![
            rectangle64(i64::MAX - 100, i64::MAX - 100, i64::MAX, i64::MAX),
            rectangle64(i64::MAX - 80, i64::MAX - 80, i64::MAX - 20, i64::MAX - 20),
        ];
        let local_result = build_polygons64(&local, FillRule::EvenOdd).unwrap();
        let translated_result = build_polygons64(&translated, FillRule::EvenOdd).unwrap();
        assert_eq!(local_result.len(), translated_result.len());
        assert_eq!(local_result[0].holes.len(), translated_result[0].holes.len());

        assert_eq!(
            build_polygons64(std::slice::from_ref(&outer), FillRule::Positive).unwrap().len(),
            1
        );
        let mut clockwise = outer;
        clockwise.reverse();
        assert_eq!(
            build_polygons64(std::slice::from_ref(&clockwise), FillRule::Negative).unwrap().len(),
            1
        );

        let sorted = build_polygons64(
            &[
                rectangle64(0, 0, 100, 100),
                rectangle64(60, 20, 80, 40),
                rectangle64(20, 20, 40, 40),
                rectangle64(200, 0, 210, 10),
            ],
            FillRule::EvenOdd,
        )
        .unwrap();
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].holes.len(), 2);
        assert_eq!(compare_paths64(&sorted[0].outer, &sorted[0].outer), Ordering::Equal);
    }

    #[test]
    fn integer_topology_distinguishes_gaps_contacts_and_crossings() {
        assert!(build_polygons64(&[Vec::new()], FillRule::EvenOdd).unwrap().is_empty());
        assert_eq!(
            build_polygons64(
                &[vec![Point64::new(0, 0), Point64::new(1, 0), Point64::new(2, 0)]],
                FillRule::EvenOdd,
            ),
            Err(Error::TopologyFailure)
        );
        let left = rectangle64(0, 0, 10, 10);
        assert_eq!(
            build_polygons64(&[left.clone(), rectangle64(11, 0, 20, 10)], FillRule::EvenOdd)
                .unwrap()
                .len(),
            2
        );
        for invalid in [rectangle64(10, 0, 20, 10), rectangle64(5, -1, 15, 5), left.clone()] {
            assert_eq!(
                build_polygons64(&[left.clone(), invalid], FillRule::EvenOdd),
                Err(Error::IntersectingPaths)
            );
        }
        let bow_tie = vec![
            Point64::new(i64::MIN, i64::MIN),
            Point64::new(i64::MAX, i64::MAX),
            Point64::new(i64::MIN, i64::MAX),
            Point64::new(i64::MAX, i64::MIN),
        ];
        assert_eq!(build_polygons64(&[bow_tie], FillRule::EvenOdd), Err(Error::TopologyFailure));
        let nonzero_crossing = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, -10),
            Point64::new(5, -5),
            Point64::new(5, 5),
        ];
        assert_eq!(
            build_polygons64(&[nonzero_crossing], FillRule::EvenOdd),
            Err(Error::TopologyFailure)
        );
        let (area, sign) = exact_area(&[
            Point64::new(i64::MIN, i64::MIN),
            Point64::new(i64::MAX, i64::MIN),
            Point64::new(i64::MAX, i64::MAX),
            Point64::new(i64::MIN, i64::MAX),
            Point64::new(i64::MIN, i64::MIN),
            Point64::new(i64::MIN, i64::MAX),
            Point64::new(i64::MAX, i64::MAX),
            Point64::new(i64::MAX, i64::MIN),
        ]);
        assert!(matches!(area, ExactArea::Big(_)));
        assert_eq!(sign, 0);
    }

    #[test]
    fn integer_limits_precede_exact_topology_work() {
        let invalid = vec![
            vec![Point64::new(i64::MIN, i64::MIN); 4],
            rectangle64(i64::MIN, i64::MIN, i64::MAX, i64::MAX),
        ];
        assert_eq!(
            super::build_polygons64(
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

    fn ring(area: f64, parent: Option<usize>) -> Ring {
        Ring { path: Vec::new(), vertices: Vec::new(), area, parent }
    }

    #[test]
    fn covers_nesting_predicates_and_coordinate_frames() {
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
        let horizontal_start64 = Point64::new(0, 0);
        let horizontal_end64 = Point64::new(10, 0);
        for (first, first_end, second, second_end) in [
            (horizontal_start64, horizontal_end64, Point64::new(2, 0), Point64::new(2, 1)),
            (horizontal_start64, horizontal_end64, Point64::new(2, 1), Point64::new(2, 0)),
            (Point64::new(2, 0), Point64::new(2, 1), horizontal_start64, horizontal_end64),
            (Point64::new(2, 1), Point64::new(2, 0), horizontal_start64, horizontal_end64),
        ] {
            assert!(segments_intersect64(first, first_end, second, second_end));
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
        assert!(!rings_intersect_by(
            &rectangle(0.0, 0.0, 1.0, 1.0),
            &rectangle(2.0, 2.0, 3.0, 3.0),
            segments_intersect,
        ));
        assert!(!self_intersects_by(&rectangle(0.0, 0.0, 1.0, 1.0), segments_intersect,));
        assert!(self_intersects_by(
            &[
                PointD::new(0.0, 0.0),
                PointD::new(10.0, 10.0),
                PointD::new(0.0, 8.0),
                PointD::new(10.0, 0.0),
                PointD::new(5.0, 2.0),
            ],
            segments_intersect
        ));
    }
}
