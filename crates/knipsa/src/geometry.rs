//! Checked, allocation-transparent geometry primitives.

use crate::{Error, PathKind};

/// An integer point used by the exact-coordinate API.
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

/// A double-precision point used by the scaled-coordinate API.
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

/// A closed or open sequence of points.
pub type Path64 = Vec<Point64>;

/// A collection of integer paths.
pub type Paths64 = Vec<Path64>;

/// A floating-point sequence of points.
pub type PathD = Vec<PointD>;

/// A collection of floating-point paths.
pub type PathsD = Vec<PathD>;

/// The orientation of three points in Cartesian coordinates.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointLocation {
    /// The point is outside the filled path.
    Outside,
    /// The point is inside the filled path.
    Inside,
    /// The point lies on the path boundary.
    Boundary,
}

/// Validates an integer path's minimum shape contract.
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
/// # Errors
///
/// Returns [`Error::InvalidPath`] for a too-short path or
/// [`Error::NonFiniteCoordinate`] for NaN/infinite coordinates.
pub fn validate_pathd(path: &[PointD], kind: PathKind) -> Result<(), Error> {
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

/// Removes consecutive duplicates and, for closed paths, a repeated closing
/// point. The input is never modified.
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
#[must_use]
pub fn normalize_pathd(path: &[PointD], kind: PathKind) -> PathD {
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

/// Returns the orientation of three integer points.
///
/// # Errors
///
/// Returns [`Error::ArithmeticOverflow`] if the exact cross product does not
/// fit in an `i128`.
pub fn orientation(a: Point64, b: Point64, c: Point64) -> Result<Orientation, Error> {
    let cross = checked_cross(a, b, c)?;
    Ok(match cross.cmp(&0) {
        core::cmp::Ordering::Less => Orientation::Clockwise,
        core::cmp::Ordering::Equal => Orientation::Collinear,
        core::cmp::Ordering::Greater => Orientation::CounterClockwise,
    })
}

/// Classifies an integer point against a closed path.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for a non-empty path with fewer than three
/// points or [`Error::ArithmeticOverflow`] if a cross product overflows.
pub fn point_in_polygon(point: Point64, path: &[Point64]) -> Result<PointLocation, Error> {
    validate_path64(path, PathKind::Closed)?;
    if path.is_empty() {
        return Ok(PointLocation::Outside);
    }

    let mut inside = false;
    for (a, b) in path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len()) {
        if point_on_segment(point, a, b)? {
            return Ok(PointLocation::Boundary);
        }

        if (a.y > point.y) != (b.y > point.y) {
            let cross = checked_cross(a, b, point)?;
            let crosses_right = if b.y > a.y { cross > 0 } else { cross < 0 };
            if crosses_right {
                inside = !inside;
            }
        }
    }
    Ok(if inside { PointLocation::Inside } else { PointLocation::Outside })
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

fn point_on_segment(point: Point64, a: Point64, b: Point64) -> Result<bool, Error> {
    if checked_cross(a, b, point)? != 0 {
        return Ok(false);
    }
    let px = i128::from(point.x);
    let py = i128::from(point.y);
    let min_x = i128::from(a.x).min(i128::from(b.x));
    let max_x = i128::from(a.x).max(i128::from(b.x));
    let min_y = i128::from(a.y).min(i128::from(b.y));
    let max_y = i128::from(a.y).max(i128::from(b.y));
    Ok(px >= min_x && px <= max_x && py >= min_y && py <= max_y)
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
    }

    #[test]
    fn validates_double_finiteness() {
        assert!(validate_pathd(&[], PathKind::Closed).is_ok());
        assert!(validate_pathd(&[PointD::new(0.0, 0.0)], PathKind::Closed).is_err());
        assert!(
            validate_pathd(&[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)], PathKind::Open).is_ok()
        );
        let error = validate_pathd(
            &[PointD::new(0.0, 0.0), PointD::new(f64::NAN, 0.0), PointD::new(1.0, 1.0)],
            PathKind::Closed,
        )
        .expect_err("NaN must be rejected");
        assert_eq!(error, Error::NonFiniteCoordinate { point_index: 1 });
        assert!(
            validate_pathd(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(f64::INFINITY, 1.0),],
                PathKind::Closed,
            )
            .is_err()
        );
        assert!(
            validate_pathd(
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
        assert_eq!(normalize_pathd(&doubles, PathKind::Open).len(), 2);
        let closed_doubles = [
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, 1.0),
            PointD::new(0.0, 0.0),
        ];
        assert_eq!(normalize_pathd(&closed_doubles, PathKind::Closed).len(), 3);
        assert_eq!(
            normalize_pathd(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(0.0, 1.0)],
                PathKind::Closed,
            )
            .len(),
            3
        );
        assert_eq!(normalize_pathd(&[], PathKind::Closed), Vec::<PointD>::new());
    }

    #[test]
    fn computes_area_and_orientation() {
        let square = vec![A, B, C, Point64::new(0, 10)];
        assert_eq!(signed_area2(&square), Ok(200));
        let mut reverse = square.clone();
        reverse.reverse();
        assert_eq!(signed_area2(&reverse), Ok(-200));
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
            Err(Error::ArithmeticOverflow)
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
    }
}
