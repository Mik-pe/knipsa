//! Central Boolean specialization router and its shared conservative geometry.
//!
//! The routing order is deliberately explicit: allocation-light standard
//! cases, fused large-rectangle XOR, then the general certified floating-point
//! kernel. Each specialization is independent and returns `None` when its
//! proof obligations are not met. The caller in `boolean` owns the one exact
//! arrangement fallback.

use std::cmp::Ordering;

use num_traits::ToPrimitive;

use crate::{
    BooleanRequest, BooleanRequestD, ClipType, FillRule, Path64, PathD, PathKind, Paths64, PathsD,
    Point64, PointD, geometry::signed_area2_d, trim_collinear64,
};

pub(crate) const KEY_SCALE: f64 = 1_000_000_000.0;
pub(crate) const MAX_COORDINATE: f64 = 1_000_000.0;
pub(crate) const MAX_ORTHOGONAL_GRID_POINTS: usize = 1_000_000;
const MAX_EXACT_F64_INTEGER: u64 = 1_u64 << 53;

/// Tries every certified specialization for an integer request.
///
/// Integer inputs are translated into one exact local `f64` frame. Any
/// inexact input or output conversion defers to the exact integer kernel.
pub(crate) fn try_boolean_op64(request: BooleanRequest<'_>) -> Option<Paths64> {
    let origin = request
        .subjects
        .iter()
        .chain(request.clips)
        .find_map(|path| path.first())
        .copied()
        .unwrap_or(Point64::new(0, 0));
    let subjects = paths64_to_d(request.subjects, origin)?;
    let clips = paths64_to_d(request.clips, origin)?;
    let request_d = BooleanRequestD::new(&subjects, &clips, request.clip_type, request.fill_rule);
    let paths = try_boolean_op_d(request_d)?;
    paths_d_to_64(&paths, origin)
}

/// Tries each certified floating-point specialization in one explicit order.
///
/// Specializations do not call one another. Returning `None` delegates the
/// request to the exact arrangement kernel owned by `boolean`.
pub(crate) fn try_boolean_op_d(request: BooleanRequestD<'_>) -> Option<PathsD> {
    crate::standard_dispatch::try_apply(request)
        .or_else(|| crate::fast_dispatch::try_apply(request))
        .or_else(|| crate::fast::try_apply(request))
}

fn paths64_to_d(paths: &[Path64], origin: Point64) -> Option<PathsD> {
    paths
        .iter()
        .map(|path| {
            let trimmed = if path.len() > 4 {
                Some(trim_collinear64(path, PathKind::Closed).ok()?)
            } else {
                None
            };
            trimmed
                .as_deref()
                .unwrap_or(path)
                .iter()
                .map(|point| {
                    let x = i128::from(point.x) - i128::from(origin.x);
                    let y = i128::from(point.y) - i128::from(origin.y);
                    if x.unsigned_abs() > u128::from(MAX_EXACT_F64_INTEGER)
                        || y.unsigned_abs() > u128::from(MAX_EXACT_F64_INTEGER)
                    {
                        return None;
                    }
                    Some(PointD::new(x.to_f64()?, y.to_f64()?))
                })
                .collect()
        })
        .collect()
}

fn paths_d_to_64(paths: &[PathD], origin: Point64) -> Option<Paths64> {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|point| {
                    let x = i128::from(exact_f64_integer(point.x)?) + i128::from(origin.x);
                    let y = i128::from(exact_f64_integer(point.y)?) + i128::from(origin.y);
                    Some(Point64::new(x.try_into().ok()?, y.try_into().ok()?))
                })
                .collect()
        })
        .collect()
}

fn exact_f64_integer(value: f64) -> Option<i64> {
    let integer = value.to_i64()?;
    let expected = integer.to_f64()?;
    (expected.to_bits() == value.to_bits()
        || (integer == 0 && value.to_bits() == (-0.0_f64).to_bits()))
    .then_some(integer)
}

pub(crate) fn orthogonal_grid_size(width: usize, height: usize) -> Option<usize> {
    width.checked_mul(height).filter(|grid_size| *grid_size <= MAX_ORTHOGONAL_GRID_POINTS)
}

pub(crate) fn dedup_grid_coordinates(
    mut coordinates: Vec<GridCoordinate>,
) -> Option<Vec<GridCoordinate>> {
    coordinates.sort_unstable_by_key(|coordinate| coordinate.key);
    let mut len = usize::from(!coordinates.is_empty());
    for read in 1..coordinates.len() {
        let coordinate = coordinates[read];
        if coordinate.key == coordinates[len - 1].key {
            if coordinate.value.to_bits() != coordinates[len - 1].value.to_bits() {
                return None;
            }
        } else {
            coordinates[len] = coordinate;
            len += 1;
        }
    }
    coordinates.truncate(len);
    (coordinates.len() >= 2).then_some(coordinates)
}

pub(crate) fn fill_rule_accepts_ring(path: &[PointD], fill_rule: FillRule) -> bool {
    match fill_rule {
        FillRule::EvenOdd | FillRule::NonZero => true,
        FillRule::Positive => signed_area2_d(path) > 0.0,
        FillRule::Negative => signed_area2_d(path) < 0.0,
    }
}

#[inline]
pub(crate) fn apply_operation(subject: bool, clip: bool, clip_type: ClipType) -> bool {
    match clip_type {
        ClipType::Intersection => subject && clip,
        ClipType::Union => subject || clip,
        ClipType::Difference => subject && !clip,
        ClipType::Xor => subject != clip,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct PointKey {
    pub(crate) x: i64,
    pub(crate) y: i64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GridCoordinate {
    pub(crate) key: i64,
    pub(crate) value: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AxisAlignedRectangle {
    pub(crate) min_x: GridCoordinate,
    pub(crate) min_y: GridCoordinate,
    pub(crate) max_x: GridCoordinate,
    pub(crate) max_y: GridCoordinate,
}

impl AxisAlignedRectangle {
    #[inline]
    pub(crate) fn contains_cell(
        self,
        min_x: GridCoordinate,
        max_x: GridCoordinate,
        min_y: GridCoordinate,
        max_y: GridCoordinate,
    ) -> bool {
        min_x.key >= self.min_x.key
            && max_x.key <= self.max_x.key
            && min_y.key >= self.min_y.key
            && max_y.key <= self.max_y.key
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DirectedEdge {
    pub(crate) start: PointD,
    pub(crate) end: PointD,
    pub(crate) start_key: PointKey,
    pub(crate) end_key: PointKey,
}

impl DirectedEdge {
    pub(crate) fn from_grid(
        start_x: GridCoordinate,
        start_y: GridCoordinate,
        end_x: GridCoordinate,
        end_y: GridCoordinate,
    ) -> Self {
        Self {
            start: PointD::new(start_x.value, start_y.value),
            end: PointD::new(end_x.value, end_y.value),
            start_key: PointKey { x: start_x.key, y: start_y.key },
            end_key: PointKey { x: end_x.key, y: end_y.key },
        }
    }
}

pub(crate) fn axis_aligned_rectangle(path: &[PointD]) -> Option<AxisAlignedRectangle> {
    let [first, second, third, fourth] = path else {
        return None;
    };
    let keys = [key(*first)?, key(*second)?, key(*third)?, key(*fourth)?];
    for (start, end) in keys.iter().zip(keys.iter().cycle().skip(1)).take(keys.len()) {
        if start == end || (start.x == end.x) == (start.y == end.y) {
            return None;
        }
    }

    let x_minimum = keys.iter().map(|point| point.x).min()?;
    let x_maximum = keys.iter().map(|point| point.x).max()?;
    let y_minimum = keys.iter().map(|point| point.y).min()?;
    let y_maximum = keys.iter().map(|point| point.y).max()?;
    if x_minimum == x_maximum || y_minimum == y_maximum {
        return None;
    }

    let mut corners = 0_u8;
    for point in keys {
        let x_bit = u32::from(point.x == x_maximum);
        let y_bit = u32::from(point.y == y_maximum);
        let bit = 1_u8 << (x_bit + 2 * y_bit);
        if corners & bit != 0 {
            return None;
        }
        corners |= bit;
    }

    Some(AxisAlignedRectangle {
        min_x: GridCoordinate {
            key: x_minimum,
            value: keyed_coordinate_value(path, &keys, x_minimum, true)?,
        },
        min_y: GridCoordinate {
            key: y_minimum,
            value: keyed_coordinate_value(path, &keys, y_minimum, false)?,
        },
        max_x: GridCoordinate {
            key: x_maximum,
            value: keyed_coordinate_value(path, &keys, x_maximum, true)?,
        },
        max_y: GridCoordinate {
            key: y_maximum,
            value: keyed_coordinate_value(path, &keys, y_maximum, false)?,
        },
    })
}

pub(crate) fn keyed_coordinate_value(
    path: &[PointD],
    keys: &[PointKey; 4],
    target: i64,
    x_axis: bool,
) -> Option<f64> {
    let mut value: Option<f64> = None;
    for (point, point_key) in path.iter().zip(keys) {
        let (candidate_key, candidate) =
            if x_axis { (point_key.x, point.x + 0.0) } else { (point_key.y, point.y + 0.0) };
        if candidate_key != target {
            continue;
        }
        if value.is_some_and(|known| known.to_bits() != candidate.to_bits()) {
            return None;
        }
        value = Some(candidate);
    }
    value
}

pub(crate) fn canonicalize(path: &mut [PointD]) {
    if let Some((minimum, _)) = path
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
    {
        path.rotate_left(minimum);
    }
}

pub(crate) fn compare_paths(left: &PathD, right: &PathD) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(crate) fn key(point: PointD) -> Option<PointKey> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x.abs() > MAX_COORDINATE
        || point.y.abs() > MAX_COORDINATE
    {
        return None;
    }
    Some(PointKey {
        x: (point.x * KEY_SCALE).round() as i64,
        y: (point.y * KEY_SCALE).round() as i64,
    })
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn exact_key(point: PointD) -> Option<PointKey> {
    let key = key(point)?;
    let reconstructed = PointD::new(key.x as f64 / KEY_SCALE, key.y as f64 / KEY_SCALE);
    (reconstructed.x.to_bits() == (point.x + 0.0).to_bits()
        && reconstructed.y.to_bits() == (point.y + 0.0).to_bits())
    .then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BooleanRequestD, ClipType, FillRule};

    #[test]
    fn central_dispatch_routes_supported_inputs_and_defers_unsafe_coordinates() {
        let subjects = [vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
        ]];
        let clips = [vec![
            PointD::new(5.0, 5.0),
            PointD::new(15.0, 5.0),
            PointD::new(15.0, 15.0),
            PointD::new(5.0, 15.0),
        ]];
        let request =
            BooleanRequestD::new(&subjects, &clips, ClipType::Intersection, FillRule::EvenOdd);
        assert_eq!(try_boolean_op_d(request).map(|paths| paths.len()), Some(1));

        let unsafe_subjects = [subjects[0]
            .iter()
            .map(|point| PointD::new(point.x + MAX_COORDINATE * 2.0, point.y))
            .collect()];
        let unsafe_request = BooleanRequestD::new(
            &unsafe_subjects,
            &clips,
            ClipType::Intersection,
            FillRule::EvenOdd,
        );
        assert!(try_boolean_op_d(unsafe_request).is_none());

        assert!(paths_d_to_64(&[vec![PointD::new(0.5, 0.0)]], Point64::new(0, 0)).is_none());
        assert!(paths_d_to_64(&[vec![PointD::new(f64::NAN, 0.0)]], Point64::new(0, 0)).is_none());
    }
}
