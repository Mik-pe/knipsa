//! Optional conversions between Knipsa paths and `geo-types` geometry.
//!
//! Enable the `geo-types` Cargo feature. Polygon conversions preserve explicit
//! outer-ring and hole ownership through [`crate::Polygon64`] and
//! [`crate::PolygonD`]. They do not infer nesting across multiple independent
//! polygons.

use ::geo_types::{Coord, CoordNum, LineString, Polygon};

use crate::{Path64, PathD, Point64, PointD, Polygon64, PolygonD};

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

/// Converts one floating-point `geo-types` polygon into Knipsa's owned polygon
/// representation.
///
/// Ring winding is preserved and the result is not validated. Use
/// [`crate::build_polygons_d`] when canonical winding or topology validation is
/// required.
#[must_use]
pub fn polygon_d_from_geo(polygon: &Polygon<f64>) -> PolygonD {
    PolygonD {
        outer: path_d_from_line_string(polygon.exterior()),
        holes: polygon.interiors().iter().map(path_d_from_line_string).collect(),
    }
}

/// Converts one integer `geo-types` polygon into Knipsa's owned polygon
/// representation.
///
/// Ring winding is preserved and the result is not validated. Use
/// [`crate::build_polygons64`] when canonical winding or topology validation is
/// required.
#[must_use]
pub fn polygon64_from_geo(polygon: &Polygon<i64>) -> Polygon64 {
    Polygon64 {
        outer: path64_from_line_string(polygon.exterior()),
        holes: polygon.interiors().iter().map(path64_from_line_string).collect(),
    }
}

/// Converts Knipsa's owned floating-point polygon into a `geo-types` polygon.
#[must_use]
pub fn geo_polygon_from_polygon_d(polygon: &PolygonD) -> Polygon<f64> {
    Polygon::new(
        line_string_from_path_d(&polygon.outer),
        polygon.holes.iter().map(|path| line_string_from_path_d(path)).collect(),
    )
}

/// Converts Knipsa's owned integer polygon into a `geo-types` polygon.
#[must_use]
pub fn geo_polygon_from_polygon64(polygon: &Polygon64) -> Polygon<i64> {
    Polygon::new(
        line_string_from_path64(&polygon.outer),
        polygon.holes.iter().map(|path| line_string_from_path64(path)).collect(),
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
        let owned = PolygonD { outer: exterior.clone(), holes: vec![hole.clone()] };
        let polygon = geo_polygon_from_polygon_d(&owned);
        assert_eq!(polygon.exterior().0.len(), exterior.len() + 1);
        assert_eq!(polygon_d_from_geo(&polygon), owned);
    }

    #[test]
    fn integer_polygon_preserves_winding_and_lines_handle_closure() {
        let exterior =
            vec![Point64::new(0, 0), Point64::new(4, 0), Point64::new(4, 4), Point64::new(0, 4)];
        let hole =
            vec![Point64::new(1, 1), Point64::new(1, 3), Point64::new(3, 3), Point64::new(3, 1)];
        let owned = Polygon64 { outer: exterior, holes: vec![hole] };
        let polygon = geo_polygon_from_polygon64(&owned);
        assert_eq!(polygon64_from_geo(&polygon), owned);

        let reversed = Polygon64 {
            outer: owned.outer.iter().copied().rev().collect(),
            holes: vec![owned.holes[0].iter().copied().rev().collect()],
        };
        assert_eq!(polygon64_from_geo(&geo_polygon_from_polygon64(&reversed)), reversed);

        let open = LineString::new(vec![Coord { x: 0_i64, y: 0 }, Coord { x: 1, y: 1 }]);
        assert_eq!(path64_from_line_string(&open).len(), 2);
        assert!(path64_from_line_string(&LineString::new(Vec::new())).is_empty());
        assert!(line_string_from_path64(&[]).0.is_empty());
        assert_eq!(line_string_from_path64(&[Point64::new(1, 1)]).0.len(), 1);
        let already_closed = [Point64::new(0, 0), Point64::new(1, 0), Point64::new(0, 0)];
        assert_eq!(line_string_from_path64(&already_closed).0.len(), 3);
    }
}
