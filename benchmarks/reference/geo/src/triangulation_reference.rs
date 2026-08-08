//! Shared geometry construction and Spade triangulation for reference binaries.

use geo::TriangulateDelaunay;
use geo::algorithm::triangulate_delaunay::DelaunayTriangulationConfig;
use geo::{Contains, Coord, LineString, MultiPolygon, Point, Polygon, Triangle};

#[derive(Clone, Copy)]
pub struct CoordinateFrame {
    origin: [f64; 2],
    scale: f64,
}

impl CoordinateFrame {
    pub fn translation_only(paths: &[Vec<[f64; 2]>]) -> Result<Self, String> {
        let origin = *paths
            .iter()
            .find_map(|path| path.first())
            .ok_or_else(|| "workload has no coordinates".to_string())?;
        if paths
            .iter()
            .flatten()
            .any(|point| !(point[0] - origin[0]).is_finite() || !(point[1] - origin[1]).is_finite())
        {
            return Err("workload has no finite coordinate frame".to_string());
        }
        Ok(Self { origin, scale: 1.0 })
    }

    pub fn from_paths(paths: &[Vec<[f64; 2]>]) -> Result<Self, String> {
        let origin = *paths
            .iter()
            .find_map(|path| path.first())
            .ok_or_else(|| "workload has no coordinates".to_string())?;
        let scale = paths
            .iter()
            .flatten()
            .flat_map(|point| [(point[0] - origin[0]).abs(), (point[1] - origin[1]).abs()])
            .fold(0.0_f64, f64::max);
        if scale == 0.0 || !scale.is_finite() {
            return Err("workload has no finite coordinate span".to_string());
        }
        Ok(Self { origin, scale })
    }

    pub fn normalize_paths(self, paths: &[Vec<[f64; 2]>]) -> Vec<Vec<[f64; 2]>> {
        paths
            .iter()
            .map(|path| {
                path.iter()
                    .map(|point| {
                        [
                            (point[0] - self.origin[0]) / self.scale,
                            (point[1] - self.origin[1]) / self.scale,
                        ]
                    })
                    .collect()
            })
            .collect()
    }

    pub fn restore_triangles(self, triangles: Vec<[[f64; 2]; 3]>) -> Vec<[[f64; 2]; 3]> {
        triangles
            .into_iter()
            .map(|triangle| {
                triangle.map(|point| {
                    [
                        point[0].mul_add(self.scale, self.origin[0]),
                        point[1].mul_add(self.scale, self.origin[1]),
                    ]
                })
            })
            .collect()
    }
}

pub fn polygons(paths: &[Vec<[f64; 2]>]) -> Result<MultiPolygon<f64>, String> {
    let mut outers = Vec::new();
    let mut holes = Vec::new();
    for path in paths {
        if path.len() < 3 || path.iter().flatten().any(|coordinate| !coordinate.is_finite()) {
            return Err("path is malformed".to_string());
        }
        let area = signed_area2(path);
        if area == 0.0 || !area.is_finite() {
            return Err("path has invalid area".to_string());
        }
        let ring = line_string(path);
        if area > 0.0 {
            outers.push((area, ring, Vec::new()));
        } else {
            holes.push(ring);
        }
    }

    for hole in holes {
        let point = Point::from(hole.0[0]);
        let owner = outers
            .iter()
            .enumerate()
            .filter(|(_, (_, outer, _))| Polygon::new(outer.clone(), Vec::new()).contains(&point))
            .min_by(|(_, (left, _, _)), (_, (right, _, _))| left.total_cmp(right))
            .map(|(index, _)| index)
            .ok_or_else(|| "hole has no containing outer ring".to_string())?;
        outers[owner].2.push(hole);
    }

    Ok(MultiPolygon::new(
        outers
            .into_iter()
            .map(|(_, exterior, interiors)| Polygon::new(exterior, interiors))
            .collect(),
    ))
}

pub fn triangulate(polygons: &MultiPolygon<f64>) -> Result<Vec<[[f64; 2]; 3]>, String> {
    let mut output = Vec::new();
    for polygon in polygons {
        let triangles = polygon
            .constrained_triangulation(DelaunayTriangulationConfig::default())
            .map_err(|error| error.to_string())?;
        output.extend(triangles.into_iter().map(triangle_coordinates));
    }
    Ok(output)
}

fn signed_area2(path: &[[f64; 2]]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|([x1, y1], [x2, y2])| x1 * y2 - y1 * x2)
        .sum()
}

fn line_string(path: &[[f64; 2]]) -> LineString<f64> {
    let mut coordinates = path.iter().map(|[x, y]| Coord { x: *x, y: *y }).collect::<Vec<_>>();
    coordinates.push(coordinates[0]);
    LineString::new(coordinates)
}

fn triangle_coordinates(triangle: Triangle<f64>) -> [[f64; 2]; 3] {
    [
        [triangle.v1().x, triangle.v1().y],
        [triangle.v2().x, triangle.v2().y],
        [triangle.v3().x, triangle.v3().y],
    ]
}
