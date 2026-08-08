//! Checked, allocation-transparent geometry primitives.

use core::cmp::Ordering;
use num_bigint::BigInt;

use crate::{
    BooleanRequest, BooleanRequestD, ClipType, Error, FillRule, PathKind, boolean_op, boolean_op_d,
};

/// An integer point used by the exact-coordinate API.
///
/// Integer operations preserve these coordinates exactly. Paths are ordinary
/// Rust vectors, so they can be built with `vec![]` and passed by slice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Point64 {
    /// Horizontal coordinate.
    pub x: i64,
    /// Vertical coordinate.
    pub y: i64,
}

impl Point64 {
    /// Creates an integer point.
    #[must_use]
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

/// A double-precision point used by the floating-point API.
///
/// [`crate::boolean_op_d`] accepts finite `f64` coordinates. The arrangement
/// kernel keeps the input values exact while it computes intersections, then
/// converts the result back to `f64`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct PointD {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl PointD {
    /// Creates a floating-point point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A sequence of points describing one path.
///
/// The path does not need to repeat its first point at the end. Whether the
/// final point connects to the first is selected with [`PathKind`]. Boolean
/// and triangulation inputs are closed paths; open paths are used by line
/// offsets.
pub type Path64 = Vec<Point64>;

/// A collection of integer paths, usually polygon rings.
pub type Paths64 = Vec<Path64>;

/// A floating-point sequence of points.
pub type PathD = Vec<PointD>;

/// A collection of floating-point paths, usually polygon rings.
pub type PathsD = Vec<PathD>;

#[allow(clippy::cast_precision_loss)]
pub(crate) fn paths64_to_local_d(paths: &[Path64]) -> Result<(Point64, PathsD), Error> {
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

/// The orientation of three points in Cartesian coordinates.
///
/// For points `a`, `b`, and `c`, the result describes the turn from `a -> b`
/// to `b -> c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// The points turn clockwise.
    Clockwise,
    /// The points are collinear.
    Collinear,
    /// The points turn counter-clockwise.
    CounterClockwise,
}

/// The result of testing a point against a closed path.
///
/// [`point_in_polygon`] uses the even-odd rule for this classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointLocation {
    /// The point is outside the filled path.
    Outside,
    /// The point is inside the filled path.
    Inside,
    /// The point lies on the path boundary.
    Boundary,
}

/// An axis-aligned integer rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Rect64 {
    /// Inclusive lower horizontal bound.
    pub min_x: i64,
    /// Inclusive lower vertical bound.
    pub min_y: i64,
    /// Inclusive upper horizontal bound.
    pub max_x: i64,
    /// Inclusive upper vertical bound.
    pub max_y: i64,
}

impl Rect64 {
    /// Creates a rectangle from two opposite corners.
    ///
    /// The bounds are normalized, so the corner order does not matter.
    #[must_use]
    pub const fn new(first_x: i64, first_y: i64, second_x: i64, second_y: i64) -> Self {
        let (min_x, max_x) =
            if first_x <= second_x { (first_x, second_x) } else { (second_x, first_x) };
        let (min_y, max_y) =
            if first_y <= second_y { (first_y, second_y) } else { (second_y, first_y) };
        Self { min_x, min_y, max_x, max_y }
    }

    fn path(self) -> Path64 {
        vec![
            Point64::new(self.min_x, self.min_y),
            Point64::new(self.max_x, self.min_y),
            Point64::new(self.max_x, self.max_y),
            Point64::new(self.min_x, self.max_y),
        ]
    }
}

/// An axis-aligned floating-point rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct RectD {
    /// Inclusive lower horizontal bound.
    pub min_x: f64,
    /// Inclusive lower vertical bound.
    pub min_y: f64,
    /// Inclusive upper horizontal bound.
    pub max_x: f64,
    /// Inclusive upper vertical bound.
    pub max_y: f64,
}

impl RectD {
    /// Creates a rectangle from two opposite corners.
    ///
    /// The bounds are normalized, so the corner order does not matter.
    #[must_use]
    pub const fn new(first_x: f64, first_y: f64, second_x: f64, second_y: f64) -> Self {
        let (min_x, max_x) =
            if first_x <= second_x { (first_x, second_x) } else { (second_x, first_x) };
        let (min_y, max_y) =
            if first_y <= second_y { (first_y, second_y) } else { (second_y, first_y) };
        Self { min_x, min_y, max_x, max_y }
    }

    fn path(self) -> PathD {
        vec![
            PointD::new(self.min_x, self.min_y),
            PointD::new(self.max_x, self.min_y),
            PointD::new(self.max_x, self.max_y),
            PointD::new(self.min_x, self.max_y),
        ]
    }
}

/// Validates an integer path's shape contract and coordinate type.
///
/// Empty paths are valid and represent no geometry. A closed path requires at
/// least three points; an open path requires at least two.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] when a non-empty path is shorter than the
/// minimum for its kind.
pub fn validate_path64(path: &[Point64], kind: PathKind) -> Result<(), Error> {
    let minimum_vertices = match kind {
        PathKind::Closed => 3,
        PathKind::Open => 2,
    };
    if path.is_empty() || path.len() >= minimum_vertices {
        Ok(())
    } else {
        Err(Error::InvalidPath { kind, minimum_vertices, actual_vertices: path.len() })
    }
}

/// Validates every integer path in a collection.
///
/// # Errors
///
/// Returns the first path-shape error found.
pub fn validate_paths64(paths: &[Path64], kind: PathKind) -> Result<(), Error> {
    for path in paths {
        validate_path64(path, kind)?;
    }
    Ok(())
}

/// Validates a floating-point path's shape and finiteness.
///
/// Empty paths are valid. Closed paths require three points and open paths
/// require two. Consecutive duplicate points are accepted here and can be
/// removed with [`normalize_path_d`].
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for a too-short path or
/// [`Error::NonFiniteCoordinate`] for NaN/infinite coordinates.
pub fn validate_path_d(path: &[PointD], kind: PathKind) -> Result<(), Error> {
    let minimum_vertices = match kind {
        PathKind::Closed => 3,
        PathKind::Open => 2,
    };
    if !path.is_empty() && path.len() < minimum_vertices {
        return Err(Error::InvalidPath { kind, minimum_vertices, actual_vertices: path.len() });
    }
    for (point_index, point) in path.iter().enumerate() {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(Error::NonFiniteCoordinate { point_index });
        }
    }
    Ok(())
}

/// Validates every floating-point path in a collection.
///
/// This is the collection counterpart to [`validate_path_d`]. Empty paths are
/// valid and every coordinate in a non-empty path must be finite.
///
/// # Errors
///
/// Returns the first path-shape or coordinate-finiteness error found.
pub fn validate_paths_d(paths: &[PathD], kind: PathKind) -> Result<(), Error> {
    for path in paths {
        validate_path_d(path, kind)?;
    }
    Ok(())
}

/// Removes consecutive duplicates and, for closed paths, a repeated closing
/// point. The input is never modified.
///
/// This is a shape normalization helper, not a validity check: a normalized
/// path can still be too short or collinear for a particular operation.
#[must_use]
pub fn normalize_path64(path: &[Point64], kind: PathKind) -> Path64 {
    let mut normalized = Vec::with_capacity(path.len());
    for &point in path {
        if normalized.last().copied() != Some(point) {
            normalized.push(point);
        }
    }
    if kind == PathKind::Closed && normalized.len() > 1 && normalized.first() == normalized.last() {
        normalized.pop();
    }
    normalized
}

/// Removes consecutive duplicate floating-point points and a repeated closing
/// point for closed paths.
///
/// This is a shape normalization helper, not a validity check: a normalized
/// path can still be too short or collinear for a particular operation.
#[must_use]
pub fn normalize_path_d(path: &[PointD], kind: PathKind) -> PathD {
    let mut normalized = Vec::with_capacity(path.len());
    for &point in path {
        if normalized.last().copied() != Some(point) {
            normalized.push(point);
        }
    }
    if kind == PathKind::Closed && normalized.len() > 1 && normalized.first() == normalized.last() {
        normalized.pop();
    }
    normalized
}

/// Returns twice the signed area of an integer path using checked `i128`
/// arithmetic.
///
/// A positive result means counter-clockwise winding in the usual Cartesian
/// coordinate system; a negative result means clockwise winding. Paths with
/// fewer than three points have area zero.
///
/// # Errors
///
/// Returns [`Error::ArithmeticOverflow`] if the exact sum does not fit in an
/// `i128`.
pub fn signed_area2(path: &[Point64]) -> Result<i128, Error> {
    if path.len() < 3 {
        return Ok(0);
    }
    let mut area = 0_i128;
    for (a, b) in path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len()) {
        let term = i128::from(a.x)
            .checked_mul(i128::from(b.y))
            .and_then(|left| {
                i128::from(a.y)
                    .checked_mul(i128::from(b.x))
                    .and_then(|right| left.checked_sub(right))
            })
            .ok_or(Error::ArithmeticOverflow)?;
        area = area.checked_add(term).ok_or(Error::ArithmeticOverflow)?;
    }
    Ok(area)
}

pub(crate) fn signed_area2_d(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(start, end)| start.x * end.y - start.y * end.x)
        .sum()
}

/// Returns the orientation of three integer points.
///
/// # Errors
///
/// The common path uses checked `i128` arithmetic. Coordinates spanning more
/// than that representation can hold are compared with arbitrary-precision
/// integers, so every `i64` input has an exact answer.
pub fn orientation(a: Point64, b: Point64, c: Point64) -> Result<Orientation, Error> {
    Ok(match cross_ordering(a, b, c) {
        Ordering::Less => Orientation::Clockwise,
        Ordering::Equal => Orientation::Collinear,
        Ordering::Greater => Orientation::CounterClockwise,
    })
}

/// Classifies an integer point against a closed path using the even-odd rule.
///
/// An empty path has no filled region, so every point is [`PointLocation::Outside`].
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for a non-empty path with fewer than three
/// points. Predicates are exact across the complete `i64` coordinate range.
pub fn point_in_polygon(point: Point64, path: &[Point64]) -> Result<PointLocation, Error> {
    validate_path64(path, PathKind::Closed)?;
    if path.is_empty() {
        return Ok(PointLocation::Outside);
    }

    let mut inside = false;
    for (a, b) in path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len()) {
        if point_on_segment(point, a, b) {
            return Ok(PointLocation::Boundary);
        }

        if (a.y > point.y) != (b.y > point.y) {
            let cross = cross_ordering(a, b, point);
            let crosses_right =
                if b.y > a.y { cross == Ordering::Greater } else { cross == Ordering::Less };
            if crosses_right {
                inside = !inside;
            }
        }
    }
    Ok(if inside { PointLocation::Inside } else { PointLocation::Outside })
}

/// Returns a reversed copy of an integer path.
#[must_use]
pub fn reverse_path64(path: &[Point64]) -> Path64 {
    path.iter().copied().rev().collect()
}

/// Returns a reversed copy of a floating-point path.
#[must_use]
pub fn reverse_path_d(path: &[PointD]) -> PathD {
    path.iter().copied().rev().collect()
}

/// Translates an integer path with checked coordinate arithmetic.
///
/// # Errors
///
/// Returns [`Error::ArithmeticOverflow`] if any translated coordinate does not
/// fit in `i64`.
pub fn translate_path64(path: &[Point64], dx: i64, dy: i64) -> Result<Path64, Error> {
    path.iter()
        .map(|point| {
            Ok(Point64::new(
                point.x.checked_add(dx).ok_or(Error::ArithmeticOverflow)?,
                point.y.checked_add(dy).ok_or(Error::ArithmeticOverflow)?,
            ))
        })
        .collect()
}

/// Translates a floating-point path.
///
/// # Errors
///
/// Returns [`Error::NonFiniteCoordinate`] when an input or translated
/// coordinate is not finite.
pub fn translate_path_d(path: &[PointD], dx: f64, dy: f64) -> Result<PathD, Error> {
    if !dx.is_finite() || !dy.is_finite() {
        return Err(Error::NonFiniteCoordinate { point_index: 0 });
    }
    path.iter()
        .enumerate()
        .map(|(point_index, point)| {
            let translated = PointD::new(point.x + dx, point.y + dy);
            if point.x.is_finite()
                && point.y.is_finite()
                && translated.x.is_finite()
                && translated.y.is_finite()
            {
                Ok(translated)
            } else {
                Err(Error::NonFiniteCoordinate { point_index })
            }
        })
        .collect()
}

/// Removes redundant collinear vertices from an integer path.
///
/// Consecutive duplicates and a repeated closing point are removed first. For
/// closed paths the first and last vertices are treated as adjacent; for open
/// paths the two endpoints are preserved.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for a non-empty path shorter than the
/// minimum for `kind`, or [`Error::ArithmeticOverflow`] for a checked cross or
/// dot product that does not fit in `i128`.
pub fn trim_collinear64(path: &[Point64], kind: PathKind) -> Result<Path64, Error> {
    validate_path64(path, kind)?;
    let mut points = normalize_path64(path, kind);
    loop {
        if points.len() < 3 {
            return Ok(points);
        }
        let mut changed = false;
        let mut cleaned = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let removable = match kind {
                PathKind::Closed => {
                    let previous = points[(index + points.len() - 1) % points.len()];
                    let current = points[index];
                    let next = points[(index + 1) % points.len()];
                    collinear_between64(previous, current, next)?
                }
                PathKind::Open if index > 0 && index + 1 < points.len() => {
                    collinear_between64(points[index - 1], points[index], points[index + 1])?
                }
                PathKind::Open => false,
            };
            if removable {
                changed = true;
            } else {
                cleaned.push(points[index]);
            }
        }
        points = cleaned;
        if !changed {
            return Ok(points);
        }
    }
}

/// Removes redundant collinear vertices from a floating-point path.
///
/// The test is exact in the input `f64` values; nearly-collinear vertices are
/// retained rather than silently changing the shape.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] or [`Error::NonFiniteCoordinate`] when the
/// input violates the selected path contract.
pub fn trim_collinear_d(path: &[PointD], kind: PathKind) -> Result<PathD, Error> {
    validate_path_d(path, kind)?;
    let mut points = normalize_path_d(path, kind);
    loop {
        if points.len() < 3 {
            return Ok(points);
        }
        let mut changed = false;
        let mut cleaned = Vec::with_capacity(points.len());
        for index in 0..points.len() {
            let removable = match kind {
                PathKind::Closed => {
                    let previous = points[(index + points.len() - 1) % points.len()];
                    let current = points[index];
                    let next = points[(index + 1) % points.len()];
                    collinear_between_d(previous, current, next)
                }
                PathKind::Open if index > 0 && index + 1 < points.len() => {
                    collinear_between_d(points[index - 1], points[index], points[index + 1])
                }
                PathKind::Open => false,
            };
            if removable {
                changed = true;
            } else {
                cleaned.push(points[index]);
            }
        }
        points = cleaned;
        if !changed {
            return Ok(points);
        }
    }
}

/// Simplifies integer paths by applying a union with the selected fill rule.
///
/// This resolves self-intersections and removes internal boundaries. Use
/// [`trim_collinear64`] instead when only vertex cleanup is desired.
///
/// # Errors
///
/// Returns the same validation, arithmetic, and topology errors as
/// [`crate::boolean_op`].
pub fn simplify_paths64(paths: &[Path64], fill_rule: FillRule) -> Result<Paths64, Error> {
    boolean_op(BooleanRequest::new(paths, &[], ClipType::Union, fill_rule))
}

/// Simplifies floating-point paths by applying a union with the selected fill
/// rule.
///
/// # Errors
///
/// Returns the same validation, non-finite-coordinate, and topology errors as
/// [`crate::boolean_op_d`].
pub fn simplify_paths_d(paths: &[PathD], fill_rule: FillRule) -> Result<PathsD, Error> {
    boolean_op_d(BooleanRequestD::new(paths, &[], ClipType::Union, fill_rule))
}

/// Clips integer paths to an axis-aligned rectangle.
///
/// # Errors
///
/// Returns the same validation, arithmetic, and topology errors as
/// [`crate::boolean_op`].
pub fn clip_to_rect64(
    paths: &[Path64],
    rectangle: Rect64,
    fill_rule: FillRule,
) -> Result<Paths64, Error> {
    let clip = rectangle.path();
    boolean_op(BooleanRequest::new(
        paths,
        std::slice::from_ref(&clip),
        ClipType::Intersection,
        fill_rule,
    ))
}

/// Clips floating-point paths to an axis-aligned rectangle.
///
/// # Errors
///
/// Returns [`Error::NonFiniteCoordinate`] when any rectangle bound is not
/// finite, in addition to the usual boolean-operation errors.
pub fn clip_to_rect_d(
    paths: &[PathD],
    rectangle: RectD,
    fill_rule: FillRule,
) -> Result<PathsD, Error> {
    if !rectangle.min_x.is_finite()
        || !rectangle.min_y.is_finite()
        || !rectangle.max_x.is_finite()
        || !rectangle.max_y.is_finite()
    {
        return Err(Error::NonFiniteCoordinate { point_index: 0 });
    }
    let clip = rectangle.path();
    boolean_op_d(BooleanRequestD::new(
        paths,
        std::slice::from_ref(&clip),
        ClipType::Intersection,
        fill_rule,
    ))
}

fn collinear_between64(previous: Point64, current: Point64, next: Point64) -> Result<bool, Error> {
    if checked_cross(previous, current, next)? != 0 {
        return Ok(false);
    }
    let first_x = i128::from(current.x) - i128::from(previous.x);
    let first_y = i128::from(current.y) - i128::from(previous.y);
    let second_x = i128::from(next.x) - i128::from(current.x);
    let second_y = i128::from(next.y) - i128::from(current.y);
    let dot = first_x
        .checked_mul(second_x)
        .and_then(|left| first_y.checked_mul(second_y).and_then(|right| left.checked_add(right)))
        .ok_or(Error::ArithmeticOverflow)?;
    Ok(dot >= 0)
}

fn collinear_between_d(previous: PointD, current: PointD, next: PointD) -> bool {
    let first_x = current.x - previous.x;
    let first_y = current.y - previous.y;
    let second_x = next.x - current.x;
    let second_y = next.y - current.y;
    let cross = first_x * second_y - first_y * second_x;
    let dot = first_x * second_x + first_y * second_y;
    cross == 0.0 && dot >= 0.0
}

fn checked_cross(a: Point64, b: Point64, c: Point64) -> Result<i128, Error> {
    let first_x = i128::from(b.x) - i128::from(a.x);
    let first_y = i128::from(b.y) - i128::from(a.y);
    let second_x = i128::from(c.x) - i128::from(a.x);
    let second_y = i128::from(c.y) - i128::from(a.y);
    first_x
        .checked_mul(second_y)
        .and_then(|left| first_y.checked_mul(second_x).and_then(|right| left.checked_sub(right)))
        .ok_or(Error::ArithmeticOverflow)
}

fn cross_ordering(a: Point64, b: Point64, c: Point64) -> Ordering {
    if let Ok(cross) = checked_cross(a, b, c) {
        return cross.cmp(&0);
    }
    let first_x = BigInt::from(i128::from(b.x) - i128::from(a.x));
    let first_y = BigInt::from(i128::from(b.y) - i128::from(a.y));
    let second_x = BigInt::from(i128::from(c.x) - i128::from(a.x));
    let second_y = BigInt::from(i128::from(c.y) - i128::from(a.y));
    (first_x * second_y - first_y * second_x).cmp(&BigInt::from(0))
}

fn point_on_segment(point: Point64, a: Point64, b: Point64) -> bool {
    if cross_ordering(a, b, point) != Ordering::Equal {
        return false;
    }
    let px = i128::from(point.x);
    let py = i128::from(point.y);
    let min_x = i128::from(a.x).min(i128::from(b.x));
    let max_x = i128::from(a.x).max(i128::from(b.x));
    let min_y = i128::from(a.y).min(i128::from(b.y));
    let max_y = i128::from(a.y).max(i128::from(b.y));
    px >= min_x && px <= max_x && py >= min_y && py <= max_y
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Point64 = Point64::new(0, 0);
    const B: Point64 = Point64::new(10, 0);
    const C: Point64 = Point64::new(10, 10);

    #[test]
    fn validates_empty_and_minimum_shapes() {
        assert!(validate_path64(&[], PathKind::Closed).is_ok());
        assert!(validate_path64(&[A], PathKind::Closed).is_err());
        assert!(validate_path64(&[A, B], PathKind::Closed).is_err());
        assert!(validate_path64(&[A, B], PathKind::Open).is_ok());
        assert!(validate_path64(&[A], PathKind::Open).is_err());
        assert!(validate_paths64(&[vec![A, B]], PathKind::Open).is_ok());
        assert!(validate_paths64(&[vec![A]], PathKind::Closed).is_err());

        assert!(validate_paths_d(&[], PathKind::Closed).is_ok());
        assert!(
            validate_paths_d(&[vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)]], PathKind::Open)
                .is_ok()
        );
        assert!(validate_paths_d(&[vec![PointD::new(0.0, 0.0)]], PathKind::Closed).is_err());
    }

    #[test]
    fn converts_integer_paths_in_one_exact_local_coordinate_frame() {
        let origin = i64::MAX - 100;
        let paths = [vec![
            Point64::new(origin, origin),
            Point64::new(origin + 10, origin),
            Point64::new(origin + 10, origin + 20),
        ]];
        let (actual_origin, local) = paths64_to_local_d(&paths).unwrap();
        assert_eq!(actual_origin, Point64::new(origin, origin));
        assert_eq!(local[0][2], PointD::new(10.0, 20.0));

        let excessive_span =
            [vec![Point64::new(i64::MIN, 0), Point64::new(i64::MAX, 0), Point64::new(0, 1)]];
        assert_eq!(paths64_to_local_d(&excessive_span), Err(Error::ArithmeticOverflow));
    }

    #[test]
    fn validates_double_finiteness() {
        assert!(validate_path_d(&[], PathKind::Closed).is_ok());
        assert!(validate_path_d(&[PointD::new(0.0, 0.0)], PathKind::Closed).is_err());
        assert!(
            validate_path_d(&[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)], PathKind::Open)
                .is_ok()
        );
        let error = validate_path_d(
            &[PointD::new(0.0, 0.0), PointD::new(f64::NAN, 0.0), PointD::new(1.0, 1.0)],
            PathKind::Closed,
        )
        .expect_err("NaN must be rejected");
        assert_eq!(error, Error::NonFiniteCoordinate { point_index: 1 });
        assert!(
            validate_path_d(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(f64::INFINITY, 1.0),],
                PathKind::Closed,
            )
            .is_err()
        );
        assert!(
            validate_path_d(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(0.0, f64::INFINITY)],
                PathKind::Closed,
            )
            .is_err()
        );
    }

    #[test]
    fn normalizes_integer_and_double_paths() {
        let input = [A, A, B, B, C, A, A];
        assert_eq!(normalize_path64(&input, PathKind::Closed), vec![A, B, C]);
        assert_eq!(normalize_path64(&[A, B, C], PathKind::Closed), vec![A, B, C]);
        assert_eq!(normalize_path64(&input, PathKind::Open), vec![A, B, C, A]);
        assert!(normalize_path64(&[], PathKind::Closed).is_empty());

        let doubles = [PointD::new(0.0, 0.0), PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)];
        assert_eq!(normalize_path_d(&doubles, PathKind::Open).len(), 2);
        let closed_doubles = [
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, 1.0),
            PointD::new(0.0, 0.0),
        ];
        assert_eq!(normalize_path_d(&closed_doubles, PathKind::Closed).len(), 3);
        assert_eq!(
            normalize_path_d(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(0.0, 1.0)],
                PathKind::Closed,
            )
            .len(),
            3
        );
        assert_eq!(normalize_path_d(&[], PathKind::Closed), Vec::<PointD>::new());
    }

    #[test]
    fn computes_area_and_orientation() {
        let square = vec![A, B, C, Point64::new(0, 10)];
        assert_eq!(signed_area2(&square), Ok(200));
        let mut reverse = square.clone();
        reverse.reverse();
        assert_eq!(signed_area2(&reverse), Ok(-200));
        let square_d = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
        ];
        assert!((signed_area2_d(&square_d) - 200.0).abs() < f64::EPSILON);
        assert_eq!(signed_area2(&[A, B]), Ok(0));
        assert_eq!(orientation(A, B, C), Ok(Orientation::CounterClockwise));
        assert_eq!(orientation(C, B, A), Ok(Orientation::Clockwise));
        assert_eq!(orientation(A, B, Point64::new(20, 0)), Ok(Orientation::Collinear));
    }

    #[test]
    fn reports_arithmetic_overflow() {
        let extreme = vec![
            Point64::new(i64::MAX, i64::MAX),
            Point64::new(i64::MIN, i64::MAX),
            Point64::new(i64::MIN, i64::MIN),
            Point64::new(i64::MAX, i64::MIN),
        ];
        assert_eq!(signed_area2(&extreme), Err(Error::ArithmeticOverflow));
        assert_eq!(
            signed_area2(&[
                Point64::new(i64::MIN, i64::MIN),
                Point64::new(i64::MIN, 1),
                Point64::new(i64::MAX, i64::MIN),
            ]),
            Err(Error::ArithmeticOverflow)
        );
        assert_eq!(
            orientation(
                Point64::new(i64::MIN, i64::MIN),
                Point64::new(i64::MAX, i64::MAX),
                Point64::new(i64::MIN, i64::MAX)
            ),
            Ok(Orientation::CounterClockwise)
        );
        assert_eq!(
            checked_cross(
                Point64::new(i64::MIN, i64::MIN),
                Point64::new(i64::MIN, i64::MAX),
                Point64::new(i64::MAX, i64::MIN),
            ),
            Err(Error::ArithmeticOverflow)
        );
        assert_eq!(
            checked_cross(
                Point64::new(i64::MIN, -1),
                Point64::new(-1, i64::MIN),
                Point64::new(i64::MAX, i64::MAX),
            ),
            Err(Error::ArithmeticOverflow)
        );
    }

    #[test]
    fn classifies_points_and_empty_paths() {
        let square = vec![A, B, C, Point64::new(0, 10)];
        assert_eq!(point_in_polygon(Point64::new(5, 5), &square), Ok(PointLocation::Inside));
        assert_eq!(point_in_polygon(Point64::new(15, 5), &square), Ok(PointLocation::Outside));
        assert_eq!(point_in_polygon(Point64::new(20, 0), &square), Ok(PointLocation::Outside));
        assert_eq!(point_in_polygon(Point64::new(-1, 0), &square), Ok(PointLocation::Outside));
        assert_eq!(point_in_polygon(Point64::new(10, 20), &square), Ok(PointLocation::Outside));
        assert_eq!(point_in_polygon(Point64::new(10, -1), &square), Ok(PointLocation::Outside));
        assert_eq!(point_in_polygon(Point64::new(10, 5), &square), Ok(PointLocation::Boundary));
        assert_eq!(point_in_polygon(A, &[]), Ok(PointLocation::Outside));
        assert!(point_in_polygon(A, &[A, B]).is_err());

        let extreme = vec![
            Point64::new(i64::MAX, i64::MAX),
            Point64::new(i64::MIN, i64::MAX),
            Point64::new(i64::MIN, i64::MIN),
            Point64::new(i64::MAX, i64::MIN),
        ];
        assert_eq!(point_in_polygon(Point64::new(0, 0), &extreme), Ok(PointLocation::Inside));
        assert_eq!(
            point_in_polygon(Point64::new(i64::MIN, 0), &extreme),
            Ok(PointLocation::Boundary)
        );
    }

    #[test]
    fn transforms_and_trims_paths() {
        let square = vec![A, B, C, Point64::new(0, 10)];
        let with_collinear = vec![A, Point64::new(5, 0), B, C, Point64::new(0, 10), A];
        assert_eq!(trim_collinear64(&with_collinear, PathKind::Closed), Ok(square.clone()));
        assert_eq!(
            trim_collinear64(&[A, Point64::new(5, 0), B, C], PathKind::Open,),
            Ok(vec![A, B, C])
        );
        assert_eq!(reverse_path64(&square), vec![Point64::new(0, 10), C, B, A]);
        assert_eq!(
            translate_path64(&square, 5, -2),
            Ok(vec![
                Point64::new(5, -2),
                Point64::new(15, -2),
                Point64::new(15, 8),
                Point64::new(5, 8),
            ])
        );
        assert_eq!(
            translate_path64(&[Point64::new(i64::MAX, 0)], 1, 0),
            Err(Error::ArithmeticOverflow)
        );

        let double_path = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
        ];
        assert_eq!(
            reverse_path_d(&double_path),
            double_path.iter().copied().rev().collect::<Vec<_>>()
        );
        assert_eq!(
            trim_collinear_d(
                &[
                    PointD::new(0.0, 0.0),
                    PointD::new(5.0, 0.0),
                    PointD::new(10.0, 0.0),
                    PointD::new(10.0, 10.0),
                ],
                PathKind::Open,
            )
            .unwrap()
            .len(),
            3
        );
        assert_eq!(
            translate_path_d(&[PointD::new(1.0, 2.0)], 3.0, -1.0),
            Ok(vec![PointD::new(4.0, 1.0)])
        );
        assert!(translate_path_d(&[PointD::new(1.0, 2.0)], f64::INFINITY, 0.0).is_err());
        assert!(translate_path_d(&[PointD::new(1.0, 2.0)], 0.0, f64::INFINITY).is_err());
        assert!(translate_path_d(&[PointD::new(f64::INFINITY, 2.0)], 0.0, 0.0).is_err());
        assert!(translate_path_d(&[PointD::new(1.0, f64::INFINITY)], 0.0, 0.0).is_err());
        assert!(translate_path_d(&[PointD::new(f64::MAX, 0.0)], f64::MAX, 0.0).is_err());
        assert!(translate_path_d(&[PointD::new(0.0, f64::MAX)], 0.0, f64::MAX).is_err());
        let collinear_closed64 = [A, Point64::new(5, 0), B];
        assert_eq!(trim_collinear64(&collinear_closed64, PathKind::Closed), Ok(vec![A, B]));
        let collinear_closed_d =
            [PointD::new(0.0, 0.0), PointD::new(5.0, 0.0), PointD::new(10.0, 0.0)];
        assert_eq!(
            trim_collinear_d(&collinear_closed_d, PathKind::Closed),
            Ok(vec![PointD::new(0.0, 0.0), PointD::new(10.0, 0.0)])
        );
    }

    #[test]
    fn simplifies_and_clips_to_rectangles() {
        let bow_tie = vec![
            Point64::new(0, 0),
            Point64::new(20, 20),
            Point64::new(0, 20),
            Point64::new(20, 0),
        ];
        let simplified = simplify_paths64(&[bow_tie], FillRule::EvenOdd).unwrap();
        assert_eq!(simplified.len(), 2);
        assert_eq!(
            simplified.iter().map(|path| signed_area2(path).unwrap().abs()).sum::<i128>(),
            400
        );

        let subject = vec![
            Point64::new(0, 0),
            Point64::new(20, 0),
            Point64::new(20, 20),
            Point64::new(0, 20),
        ];
        let clipped =
            clip_to_rect64(&[subject], Rect64::new(5, 5, 15, 15), FillRule::EvenOdd).unwrap();
        assert_eq!(clipped.len(), 1);
        assert_eq!(signed_area2(&clipped[0]), Ok(200));
        assert_eq!(Rect64::new(15, 15, 5, 5), Rect64::new(5, 5, 15, 15));

        let subject_d = vec![
            PointD::new(0.0, 0.0),
            PointD::new(20.0, 0.0),
            PointD::new(20.0, 20.0),
            PointD::new(0.0, 20.0),
        ];
        let clipped_d =
            clip_to_rect_d(&[subject_d], RectD::new(5.5, 5.5, 15.5, 15.5), FillRule::EvenOdd)
                .unwrap();
        assert_eq!(clipped_d.len(), 1);
        assert!((signed_area2_d(&clipped_d[0]).abs() - 200.0).abs() < f64::EPSILON);
        assert!(
            clip_to_rect_d(&[], RectD::new(f64::NAN, 0.0, 1.0, 1.0), FillRule::EvenOdd,).is_err()
        );
        for rectangle in [
            RectD::new(0.0, f64::NAN, 1.0, 1.0),
            RectD::new(0.0, 0.0, f64::NAN, 1.0),
            RectD::new(0.0, 0.0, 1.0, f64::NAN),
        ] {
            assert!(clip_to_rect_d(&[], rectangle, FillRule::EvenOdd).is_err());
        }
    }
}
