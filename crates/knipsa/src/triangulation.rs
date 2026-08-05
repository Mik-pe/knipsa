//! Constrained polygon triangulation.
//!
//! Triangulation is kept as a separate operation from clipping. Inputs may
//! contain multiple nested rings; nesting and [`crate::FillRule`] determine
//! which rings are solids and which are holes. The triangulation backend is
//! the independent ISC-licensed `earcutr` crate, while all input validation,
//! nesting, coordinate preservation, and public result shaping live here.

use crate::{Error, FillRule, Path64, PathD, Point64, PointD, normalize_pathd, validate_pathd};

const EPSILON: f64 = 1e-12;

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

/// Triangulates a collection of integer-coordinate rings using a fill rule.
///
/// Rings may be nested to describe holes and islands. Intersecting rings are
/// rejected because their filled meaning is ambiguous for triangulation.
/// The returned triangles are counter-clockwise and preserve the integer
/// coordinate type. Computation uses a shared local origin, so absolute
/// coordinates may span the full `i64` range when their differences from that
/// origin fit the exact integer range of `f64` (2^53).
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for malformed rings, [`Error::IntersectingPaths`]
/// for crossing rings, and [`Error::TriangulationFailure`] when the topology
/// cannot be triangulated.
pub fn triangulate64(paths: &[Path64], fill_rule: FillRule) -> Result<Vec<Triangle64>, Error> {
    let (origin, paths_d) = paths64_to_local_d(paths)?;
    let triangles = triangulate_d(&paths_d, fill_rule)?;
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
/// # Errors
///
/// Returns [`Error::InvalidPath`] for malformed rings, [`Error::IntersectingPaths`]
/// for crossing rings, and [`Error::TriangulationFailure`] when the topology
/// cannot be triangulated.
pub fn triangulate_d(paths: &[PathD], fill_rule: FillRule) -> Result<Vec<TriangleD>, Error> {
    let rings = collect_rings(paths)?;
    let groups = filled_groups(&rings, fill_rule);
    let mut result = Vec::new();
    for (outer, holes) in groups {
        let group_start = result.len();
        let mut coordinates = Vec::new();
        let mut vertices = Vec::new();
        let mut hole_indices = Vec::new();
        append_ring(&rings[outer].path, &mut coordinates, &mut vertices);
        for hole in holes {
            hole_indices.push(vertices.len());
            append_ring(&rings[hole].path, &mut coordinates, &mut vertices);
        }
        let indices = earcutr::earcut(&coordinates, &hole_indices, 2)
            .map_err(|_| Error::TriangulationFailure)?;
        validate_triangle_indices(&indices)?;
        for indices in indices.chunks_exact(3) {
            push_oriented_triangle(&mut result, &vertices, indices);
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
pub fn triangulate_path64(path: &[Point64]) -> Result<Vec<Triangle64>, Error> {
    triangulate64(&[path.to_vec()], FillRule::NonZero)
}

/// Triangulates one simple floating-point polygon with the non-zero fill rule.
///
/// This is the convenient entry point when there is exactly one outer ring
/// and no holes.
///
/// # Errors
///
/// Propagates errors from [`triangulate_d`].
pub fn triangulate_pathd(path: &[PointD]) -> Result<Vec<TriangleD>, Error> {
    triangulate_d(&[path.to_vec()], FillRule::NonZero)
}

/// Alias for [`triangulate64`] using the plural name used by the path APIs.
///
/// # Errors
///
/// Propagates errors from [`triangulate64`].
pub fn triangulate_paths64(
    paths: &[Path64],
    fill_rule: FillRule,
) -> Result<Vec<Triangle64>, Error> {
    triangulate64(paths, fill_rule)
}

/// Alias for [`triangulate_d`] using the plural name used by the path APIs.
///
/// # Errors
///
/// Propagates errors from [`triangulate_d`].
pub fn triangulate_paths_d(paths: &[PathD], fill_rule: FillRule) -> Result<Vec<TriangleD>, Error> {
    triangulate_d(paths, fill_rule)
}

#[derive(Clone, Debug)]
struct Ring {
    path: PathD,
    area: f64,
    parent: Option<usize>,
    probe: PointD,
}

#[allow(clippy::cast_precision_loss)]
fn paths64_to_local_d(paths: &[Path64]) -> Result<(Point64, Vec<PathD>), Error> {
    let origin = paths.iter().find_map(|path| path.first()).copied().unwrap_or(Point64::new(0, 0));
    let mut local = Vec::with_capacity(paths.len());
    for path in paths {
        let mut local_path = Vec::with_capacity(path.len());
        for point in path {
            local_path.push(PointD::new(
                i128_to_exact_f64(i128::from(point.x) - i128::from(origin.x))?,
                i128_to_exact_f64(i128::from(point.y) - i128::from(origin.y))?,
            ));
        }
        local.push(local_path);
    }
    Ok((origin, local))
}

#[allow(clippy::cast_precision_loss)]
fn i128_to_exact_f64(value: i128) -> Result<f64, Error> {
    const MAX_EXACT_INTEGER: u128 = 1 << 53;
    if value.unsigned_abs() > MAX_EXACT_INTEGER {
        return Err(Error::ArithmeticOverflow);
    }
    Ok(value as f64)
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn point_d_to_64(point: PointD, origin: Point64) -> Result<Point64, Error> {
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

fn collect_rings(paths: &[PathD]) -> Result<Vec<Ring>, Error> {
    let mut rings = Vec::new();
    for path in paths {
        validate_pathd(path, crate::PathKind::Closed)?;
        let path = normalize_pathd(path, crate::PathKind::Closed);
        if path.is_empty() {
            continue;
        }
        if path.len() < 3 {
            return Err(Error::TriangulationFailure);
        }
        let area = signed_area2(&path);
        if !area.is_finite() {
            return Err(Error::ArithmeticOverflow);
        }
        if area.abs() <= EPSILON || self_intersects(&path) {
            return Err(Error::TriangulationFailure);
        }
        let probe = interior_probe(&path, area).ok_or(Error::TriangulationFailure)?;
        rings.push(Ring { path, area, parent: None, probe });
    }

    for index in 0..rings.len() {
        for other in 0..rings.len() {
            if index == other {
                continue;
            }
            if rings_intersect(&rings[index].path, &rings[other].path) {
                return Err(Error::IntersectingPaths);
            }
        }
        let mut parent: Option<usize> = None;
        for other in 0..rings.len() {
            if index == other || rings[other].area.abs() <= rings[index].area.abs() {
                continue;
            }
            if point_in_path(rings[index].probe, &rings[other].path)
                && parent.is_none_or(|current| rings[other].area.abs() < rings[current].area.abs())
            {
                parent = Some(other);
            }
        }
        rings[index].parent = parent;
    }
    Ok(rings)
}

fn filled_groups(rings: &[Ring], fill_rule: FillRule) -> Vec<(usize, Vec<usize>)> {
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
        FillRule::EvenOdd => i32::from(ancestors.len() % 2 != 0),
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
        FillRule::EvenOdd => i32::from(ancestors.len() % 2 == 0),
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

fn append_ring(path: &[PointD], coordinates: &mut Vec<f64>, vertices: &mut Vec<PointD>) {
    coordinates.reserve(path.len() * 2);
    for &point in path {
        coordinates.extend([point.x, point.y]);
        vertices.push(point);
    }
}

fn validate_triangle_indices(indices: &[usize]) -> Result<(), Error> {
    if indices.len() % 3 != 0 { Err(Error::TriangulationFailure) } else { Ok(()) }
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

fn orient_triangle(vertices: &[PointD], indices: &[usize]) -> Option<TriangleD> {
    let mut triangle = [vertices[indices[0]], vertices[indices[1]], vertices[indices[2]]];
    let area = cross(triangle[1], triangle[2], triangle[0]);
    if area.abs() <= EPSILON {
        return None;
    }
    if area < 0.0 {
        triangle.swap(1, 2);
    }
    Some(triangle)
}

fn push_oriented_triangle(result: &mut Vec<TriangleD>, vertices: &[PointD], indices: &[usize]) {
    if let Some(triangle) = orient_triangle(vertices, indices) {
        result.push(triangle);
    }
}

fn signed_area2(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(first, second)| first.x * second.y - first.y * second.x)
        .sum()
}

fn cross(first: PointD, second: PointD, third: PointD) -> f64 {
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
            if first_end == second
                || second_end == first
                || (first == 0 && second_end == edge_count - 1)
            {
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
        let triangles = triangulate_pathd(&rectangle(0.0, 0.0, 10.0, 10.0)).unwrap();
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
    fn triangulates_holes_and_nested_islands() {
        let outer = rectangle(0.0, 0.0, 20.0, 20.0);
        let mut hole = rectangle(5.0, 5.0, 15.0, 15.0);
        hole.reverse();
        let mut island = rectangle(8.0, 8.0, 12.0, 12.0);
        island.reverse();
        let triangles = triangulate_d(&[outer, hole, island], FillRule::EvenOdd).unwrap();
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
        assert!(triangulate_d(&[clockwise.clone()], FillRule::Positive).unwrap().is_empty());
        assert_eq!(triangulate_d(&[clockwise], FillRule::Negative).unwrap().len(), 2);

        let twice = [rectangle(0.0, 0.0, 10.0, 10.0), rectangle(0.0, 0.0, 10.0, 10.0)];
        assert!(matches!(triangulate_d(&twice, FillRule::EvenOdd), Err(Error::IntersectingPaths)));
    }

    #[test]
    fn rejects_invalid_and_intersecting_paths() {
        assert!(matches!(
            triangulate_pathd(&[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)]),
            Err(Error::InvalidPath { .. })
        ));
        let bow_tie = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
        ];
        assert_eq!(triangulate_pathd(&bow_tie), Err(Error::TriangulationFailure));
        let first = rectangle(0.0, 0.0, 10.0, 10.0);
        let second = rectangle(5.0, -1.0, 15.0, 5.0);
        assert_eq!(
            triangulate_d(&[first, second], FillRule::EvenOdd),
            Err(Error::IntersectingPaths)
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
        let triangles = triangulate64(&[path], FillRule::NonZero).unwrap();
        assert_eq!(triangles.len(), 2);
        assert_eq!(triangles.len(), 2);
    }

    #[test]
    fn covers_aliases_and_numeric_boundaries() {
        assert_eq!(i128_to_exact_f64((1_i128 << 53) + 1), Err(Error::ArithmeticOverflow));
        let excessive_span =
            vec![vec![Point64::new(0, 0), Point64::new((1_i64 << 53) + 1, 0), Point64::new(0, 1)]];
        assert_eq!(
            triangulate64(&excessive_span, FillRule::EvenOdd),
            Err(Error::ArithmeticOverflow)
        );
        let integer_path = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
        ];
        assert_eq!(
            triangulate_paths64(std::slice::from_ref(&integer_path), FillRule::NonZero)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(triangulate_path64(&integer_path).unwrap().len(), 2);
        let double_path = rectangle(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            triangulate_paths_d(std::slice::from_ref(&double_path), FillRule::NonZero)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(triangulate_pathd(&double_path).unwrap().len(), 2);
        assert_eq!(triangulate_d(&[Vec::new()], FillRule::EvenOdd), Ok(Vec::new()));

        let collapsed = vec![PointD::new(0.0, 0.0), PointD::new(0.0, 0.0), PointD::new(0.0, 0.0)];
        assert_eq!(
            triangulate_d(&[collapsed], FillRule::EvenOdd),
            Err(Error::TriangulationFailure)
        );
        let collinear = vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)];
        assert_eq!(
            triangulate_d(&[collinear], FillRule::EvenOdd),
            Err(Error::TriangulationFailure)
        );

        let huge = vec![PointD::new(1e308, 0.0), PointD::new(0.0, 1e308), PointD::new(-1e308, 0.0)];
        assert_eq!(triangulate_d(&[huge], FillRule::EvenOdd), Err(Error::ArithmeticOverflow));

        let nonzero_crossing = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 8.0),
            PointD::new(10.0, 0.0),
            PointD::new(5.0, 2.0),
        ];
        assert_eq!(
            triangulate_d(&[nonzero_crossing], FillRule::EvenOdd),
            Err(Error::TriangulationFailure)
        );

        let translated_origin = (1_i64 << 53) + 1;
        let translated = vec![vec![
            Point64::new(translated_origin, 0),
            Point64::new(translated_origin + 10, 0),
            Point64::new(translated_origin + 10, 10),
            Point64::new(translated_origin, 10),
        ]];
        let translated_triangles = triangulate64(&translated, FillRule::NonZero)
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
    fn covers_topology_and_fill_rule_helpers() {
        let mut clockwise = rectangle(0.0, 0.0, 10.0, 10.0);
        clockwise.reverse();
        let triangles = triangulate_d(&[clockwise], FillRule::Negative).unwrap();
        assert_eq!(triangles.len(), 2);
        assert!(
            triangles.iter().all(|triangle| cross(triangle[0], triangle[1], triangle[2]) > 0.0)
        );

        let disjoint = triangulate_d(
            &[rectangle(0.0, 0.0, 20.0, 20.0), rectangle(30.0, 30.0, 31.0, 31.0)],
            FillRule::EvenOdd,
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
        )
        .unwrap();
        assert!(!reverse_nested.is_empty());

        assert!(interior_probe(&[PointD::new(0.0, 0.0), PointD::new(0.0, 0.0)], 1.0).is_none());
        let outward_probe = interior_probe(&rectangle(0.0, 0.0, 100.0, 100.0), -1.0);
        assert!(outward_probe.is_none());
        assert!(point_in_path(PointD::new(0.0, 0.0), &rectangle(0.0, 0.0, 1.0, 1.0)));
        assert!(point_in_path(PointD::new(0.5, 0.5), &rectangle(0.0, 0.0, 1.0, 1.0)));
        assert!(!point_in_path(PointD::new(2.0, 0.5), &rectangle(0.0, 0.0, 1.0, 1.0)));

        let rings = vec![
            Ring { path: Vec::new(), area: 100.0, parent: None, probe: PointD::new(0.0, 0.0) },
            Ring { path: Vec::new(), area: -25.0, parent: Some(0), probe: PointD::new(0.0, 0.0) },
            Ring { path: Vec::new(), area: 4.0, parent: Some(1), probe: PointD::new(0.0, 0.0) },
        ];
        assert_eq!(nearest_outer_ancestor(1, &rings, &[0]), Some(0));
        assert_eq!(nearest_outer_ancestor(2, &rings, &[0]), Some(0));
        assert_eq!(nearest_outer_ancestor(1, &rings, &[]), None);
        assert_eq!(filled_groups(&rings, FillRule::EvenOdd).len(), 2);
        let same_winding = vec![
            Ring { path: Vec::new(), area: 100.0, parent: None, probe: PointD::new(0.0, 0.0) },
            Ring { path: Vec::new(), area: 25.0, parent: Some(0), probe: PointD::new(0.0, 0.0) },
        ];
        assert_eq!(filled_groups(&same_winding, FillRule::NonZero).len(), 1);

        let ancestors = [0_usize, 1];
        assert_eq!(winding_value(&ancestors, &rings, FillRule::EvenOdd), 0);
        assert_eq!(winding_value(&ancestors, &rings, FillRule::NonZero), 0);
        assert_eq!(winding_value(&ancestors, &rings, FillRule::Positive), 0);
        assert_eq!(winding_value(&ancestors, &rings, FillRule::Negative), 0);
        assert_eq!(winding_value_with_current(&ancestors, 2, &rings, FillRule::EvenOdd), 1);
        assert_eq!(winding_value_with_current(&ancestors, 2, &rings, FillRule::NonZero), 1);
        assert_eq!(winding_value_with_current(&ancestors, 2, &rings, FillRule::Positive), 1);
        assert_eq!(winding_value_with_current(&ancestors, 2, &rings, FillRule::Negative), 1);
        assert!(is_filled(1, FillRule::EvenOdd));
        assert!(is_filled(1, FillRule::NonZero));
        assert!(is_filled(1, FillRule::Positive));
        assert!(!is_filled(1, FillRule::Negative));
        assert!(!is_filled(-1, FillRule::Positive));
        assert!(is_filled(-1, FillRule::Negative));
        assert_eq!(sign(1.0), 1);
        assert_eq!(sign(-1.0), -1);

        assert!(validate_triangle_indices(&[0, 1, 2]).is_ok());
        assert_eq!(validate_triangle_indices(&[0]), Err(Error::TriangulationFailure));
        assert!(ensure_group_result(0, 1, true).is_ok());
        assert!(ensure_group_result(0, 0, false).is_ok());
        assert_eq!(ensure_group_result(0, 0, true), Err(Error::TriangulationFailure));

        let vertices = [PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(0.0, 1.0)];
        assert!(orient_triangle(&vertices, &[0, 1, 2]).is_some());
        assert!(orient_triangle(&vertices, &[0, 2, 1]).is_some());
        assert!(orient_triangle(&vertices, &[0, 1, 1]).is_none());
        let mut oriented = Vec::new();
        push_oriented_triangle(&mut oriented, &vertices, &[0, 1, 2]);
        push_oriented_triangle(&mut oriented, &vertices, &[0, 1, 1]);
        assert_eq!(oriented.len(), 1);
    }

    #[test]
    fn covers_segment_predicates() {
        let horizontal_start = PointD::new(0.0, 0.0);
        let horizontal_end = PointD::new(10.0, 0.0);
        assert!(segments_intersect(
            horizontal_start,
            horizontal_end,
            PointD::new(2.0, 0.0),
            PointD::new(2.0, 1.0),
        ));
        assert!(segments_intersect(
            horizontal_start,
            horizontal_end,
            PointD::new(2.0, 1.0),
            PointD::new(2.0, 0.0),
        ));
        assert!(segments_intersect(
            PointD::new(2.0, 0.0),
            PointD::new(2.0, 1.0),
            horizontal_start,
            horizontal_end,
        ));
        assert!(segments_intersect(
            PointD::new(2.0, 1.0),
            PointD::new(2.0, 0.0),
            horizontal_start,
            horizontal_end,
        ));
        assert!(segments_intersect(
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
        ));
        assert!(!segments_intersect(
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, 1.0),
            PointD::new(1.0, 1.0),
        ));
        assert!(on_segment(PointD::new(5.0, 0.0), horizontal_start, horizontal_end));
        assert!(!on_segment(PointD::new(11.0, 0.0), horizontal_start, horizontal_end));
        assert!(!on_segment(PointD::new(-1.0, 0.0), horizontal_start, horizontal_end));
        assert!(!on_segment(PointD::new(5.0, 1.0), horizontal_start, horizontal_end));
        assert!(!on_segment(PointD::new(5.0, -1.0), horizontal_start, horizontal_end));
        let vertical_start = PointD::new(0.0, 0.0);
        let vertical_end = PointD::new(0.0, 10.0);
        assert!(!on_segment(PointD::new(0.0, -1.0), vertical_start, vertical_end));
        assert!(!on_segment(PointD::new(0.0, 11.0), vertical_start, vertical_end));
        assert!(!segments_intersect(
            horizontal_start,
            horizontal_end,
            PointD::new(20.0, 0.0),
            PointD::new(20.0, 1.0),
        ));
        assert!(!segments_intersect(
            horizontal_start,
            horizontal_end,
            PointD::new(20.0, 1.0),
            PointD::new(20.0, 0.0),
        ));
        assert!(!segments_intersect(
            PointD::new(20.0, 0.0),
            PointD::new(20.0, 1.0),
            horizontal_start,
            horizontal_end,
        ));
        assert!(!segments_intersect(
            PointD::new(20.0, 1.0),
            PointD::new(20.0, 0.0),
            horizontal_start,
            horizontal_end,
        ));
        assert!(!rings_intersect(&rectangle(0.0, 0.0, 1.0, 1.0), &rectangle(2.0, 2.0, 3.0, 3.0),));
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
