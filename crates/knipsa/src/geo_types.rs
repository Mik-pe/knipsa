//! Optional conversions between Knipsa paths and `geo-types` geometry.
//!
//! Enable the `geo-types` Cargo feature. Polygon conversions preserve the
//! exterior-first, interiors-after ordering used by [`::geo_types::Polygon`].
//! They do not infer nesting across multiple independent polygons.

use ::geo_types::{Coord, CoordNum, LineString, Polygon};

use crate::{Path64, PathD, Paths64, PathsD, Point64, PointD};

/// Converts a floating-point `geo-types` line string into a Knipsa path.
///
/// A repeated closing coordinate is removed because Knipsa closes polygon
/// rings implicitly.
#[must_use]
pub fn path_d_from_line_string(line: &LineString<f64>) -> PathD {
    trim_closing_point(line.0.iter().map(|point| PointD::new(point.x, point.y)).collect())
}

/// Converts an integer `geo-types` line string into a Knipsa path.
#[must_use]
pub fn path64_from_line_string(line: &LineString<i64>) -> Path64 {
    trim_closing_point(line.0.iter().map(|point| Point64::new(point.x, point.y)).collect())
}

/// Converts a Knipsa floating-point path into an explicitly closed line string.
#[must_use]
pub fn line_string_from_path_d(path: &[PointD]) -> LineString<f64> {
    closed_line_string(path.iter().map(|point| Coord { x: point.x, y: point.y }).collect())
}

/// Converts a Knipsa integer path into an explicitly closed line string.
#[must_use]
pub fn line_string_from_path64(path: &[Point64]) -> LineString<i64> {
    closed_line_string(path.iter().map(|point| Coord { x: point.x, y: point.y }).collect())
}

/// Converts one floating-point polygon into exterior-first Knipsa paths.
#[must_use]
pub fn paths_d_from_polygon(polygon: &Polygon<f64>) -> PathsD {
    std::iter::once(path_d_from_line_string(polygon.exterior()))
        .chain(polygon.interiors().iter().map(path_d_from_line_string))
        .collect()
}

/// Converts one integer polygon into exterior-first Knipsa paths.
#[must_use]
pub fn paths64_from_polygon(polygon: &Polygon<i64>) -> Paths64 {
    std::iter::once(path64_from_line_string(polygon.exterior()))
        .chain(polygon.interiors().iter().map(path64_from_line_string))
        .collect()
}

/// Builds a floating-point polygon from one exterior and zero or more holes.
#[must_use]
pub fn polygon_from_paths_d(exterior: &[PointD], interiors: &[PathD]) -> Polygon<f64> {
    Polygon::new(
        line_string_from_path_d(exterior),
        interiors.iter().map(|path| line_string_from_path_d(path)).collect(),
    )
}

/// Builds an integer polygon from one exterior and zero or more holes.
#[must_use]
pub fn polygon_from_paths64(exterior: &[Point64], interiors: &[Path64]) -> Polygon<i64> {
    Polygon::new(
        line_string_from_path64(exterior),
        interiors.iter().map(|path| line_string_from_path64(path)).collect(),
    )
}

fn trim_closing_point<Point: PartialEq>(mut path: Vec<Point>) -> Vec<Point> {
    if path.len() > 1 && path.first() == path.last() {
        path.pop();
    }
    path
}

fn closed_line_string<T: CoordNum>(mut coordinates: Vec<Coord<T>>) -> LineString<T> {
    if coordinates.len() > 1 && coordinates.first() != coordinates.last() {
        coordinates.push(coordinates[0]);
    }
    LineString::new(coordinates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_polygon_round_trips_without_duplicate_closure() {
        let exterior = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
        ];
        let hole = vec![
            PointD::new(2.0, 2.0),
            PointD::new(2.0, 8.0),
            PointD::new(8.0, 8.0),
            PointD::new(8.0, 2.0),
        ];
        let polygon = polygon_from_paths_d(&exterior, std::slice::from_ref(&hole));
        assert_eq!(polygon.exterior().0.len(), exterior.len() + 1);
        assert_eq!(paths_d_from_polygon(&polygon), vec![exterior, hole]);
    }

    #[test]
    fn integer_polygon_and_open_or_empty_lines_convert_canonically() {
        let exterior =
            vec![Point64::new(0, 0), Point64::new(4, 0), Point64::new(4, 4), Point64::new(0, 4)];
        let hole =
            vec![Point64::new(1, 1), Point64::new(1, 3), Point64::new(3, 3), Point64::new(3, 1)];
        let polygon = polygon_from_paths64(&exterior, std::slice::from_ref(&hole));
        assert_eq!(paths64_from_polygon(&polygon), vec![exterior.clone(), hole]);

        let open = LineString::new(vec![Coord { x: 0_i64, y: 0 }, Coord { x: 1, y: 1 }]);
        assert_eq!(path64_from_line_string(&open).len(), 2);
        assert!(path64_from_line_string(&LineString::new(Vec::new())).is_empty());
        assert!(line_string_from_path64(&[]).0.is_empty());
        assert_eq!(line_string_from_path64(&[Point64::new(1, 1)]).0.len(), 1);
        let already_closed = [Point64::new(0, 0), Point64::new(1, 0), Point64::new(0, 0)];
        assert_eq!(line_string_from_path64(&already_closed).0.len(), 3);
    }
}
