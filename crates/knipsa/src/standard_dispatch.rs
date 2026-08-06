//! Allocation-light dispatch for the smallest, most common polygon pairs.

use std::cmp::Ordering;

use crate::{BooleanRequestD, ClipType, FillRule, PathD, PathsD, PointD};

const KEY_SCALE: f64 = 1_000_000_000.0;
const MAX_COORDINATE: f64 = 1_000_000.0;
const MAX_BOUNDARY_EDGES: usize = 24;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct PointKey {
    x: i64,
    y: i64,
}

#[derive(Clone, Copy, Debug)]
struct GridCoordinate {
    key: i64,
    value: f64,
}

#[derive(Clone, Copy, Debug)]
struct AxisAlignedRectangle {
    min_x: GridCoordinate,
    min_y: GridCoordinate,
    max_x: GridCoordinate,
    max_y: GridCoordinate,
}

impl AxisAlignedRectangle {
    #[inline]
    fn contains_cell(
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
struct DirectedEdge {
    start: PointD,
    end: PointD,
    start_key: PointKey,
    end_key: PointKey,
}

struct SmallBoundary {
    edges: [DirectedEdge; MAX_BOUNDARY_EDGES],
    len: usize,
}

impl SmallBoundary {
    fn new() -> Self {
        Self {
            edges: [DirectedEdge::default(); MAX_BOUNDARY_EDGES],
            len: 0,
        }
    }

    #[inline]
    fn push(&mut self, edge: DirectedEdge) -> Option<()> {
        *self.edges.get_mut(self.len)? = edge;
        self.len += 1;
        Some(())
    }

    fn as_slice(&self) -> &[DirectedEdge] {
        &self.edges[..self.len]
    }
}

pub(crate) fn try_boolean_opd(request: BooleanRequestD<'_>) -> Option<Result<PathsD, ()>> {
    if let Some(result) = try_rectangle_pair(request) {
        return Some(Ok(result));
    }
    crate::fast_dispatch::try_boolean_opd(request)
}

/// Handles one axis-aligned rectangle per side without coordinate vectors,
/// event streams, winding buffers, or hash-based boundary stitching.
fn try_rectangle_pair(request: BooleanRequestD<'_>) -> Option<PathsD> {
    if request.subjects.len() != 1 || request.clips.len() != 1 {
        return None;
    }
    let subject = request.subjects[0].as_slice();
    let clip = request.clips[0].as_slice();
    let subject_rectangle = axis_aligned_rectangle(subject)?;
    let clip_rectangle = axis_aligned_rectangle(clip)?;
    let (xs, x_len) = tiny_coordinates(
        subject_rectangle.min_x,
        subject_rectangle.max_x,
        clip_rectangle.min_x,
        clip_rectangle.max_x,
    )?;
    let (ys, y_len) = tiny_coordinates(
        subject_rectangle.min_y,
        subject_rectangle.max_y,
        clip_rectangle.min_y,
        clip_rectangle.max_y,
    )?;
    let xs = &xs[..x_len];
    let ys = &ys[..y_len];
    let rows = y_len - 1;
    let subject_enabled = fill_rule_accepts_ring(subject, request.fill_rule);
    let clip_enabled = fill_rule_accepts_ring(clip, request.fill_rule);
    let mut previous = [false; 3];
    let mut current = [false; 3];
    let mut boundary = SmallBoundary::new();

    for x in 0..x_len - 1 {
        for y in 0..rows {
            let subject_contains = subject_enabled
                && subject_rectangle.contains_cell(xs[x], xs[x + 1], ys[y], ys[y + 1]);
            let clip_contains = clip_enabled
                && clip_rectangle.contains_cell(xs[x], xs[x + 1], ys[y], ys[y + 1]);
            current[y] = apply_operation(subject_contains, clip_contains, request.clip_type);
        }
        add_vertical_transitions(
            &mut boundary,
            xs[x],
            ys,
            &previous[..rows],
            &current[..rows],
        )?;
        add_horizontal_transitions(
            &mut boundary,
            xs[x],
            xs[x + 1],
            ys,
            &current[..rows],
        )?;
        std::mem::swap(&mut previous, &mut current);
    }

    current[..rows].fill(false);
    add_vertical_transitions(
        &mut boundary,
        xs[x_len - 1],
        ys,
        &previous[..rows],
        &current[..rows],
    )?;
    stitch_small(boundary.as_slice())
}

fn axis_aligned_rectangle(path: &[PointD]) -> Option<AxisAlignedRectangle> {
    let [first, second, third, fourth] = path else {
        return None;
    };
    let keys = [key(*first)?, key(*second)?, key(*third)?, key(*fourth)?];
    for (start, end) in keys.iter().zip(keys.iter().cycle().skip(1)).take(keys.len()) {
        if start == end || (start.x == end.x) == (start.y == end.y) {
            return None;
        }
    }

    let min_x_key = keys.iter().map(|point| point.x).min()?;
    let max_x_key = keys.iter().map(|point| point.x).max()?;
    let min_y_key = keys.iter().map(|point| point.y).min()?;
    let max_y_key = keys.iter().map(|point| point.y).max()?;
    if min_x_key == max_x_key || min_y_key == max_y_key {
        return None;
    }

    let mut corners = 0_u8;
    for point in keys {
        let x_bit = u32::from(point.x == max_x_key);
        let y_bit = u32::from(point.y == max_y_key);
        let bit = 1_u8 << (x_bit + 2 * y_bit);
        if corners & bit != 0 {
            return None;
        }
        corners |= bit;
    }
    Some(AxisAlignedRectangle {
        min_x: GridCoordinate {
            key: min_x_key,
            value: keyed_coordinate_value(path, &keys, min_x_key, true)?,
        },
        min_y: GridCoordinate {
            key: min_y_key,
            value: keyed_coordinate_value(path, &keys, min_y_key, false)?,
        },
        max_x: GridCoordinate {
            key: max_x_key,
            value: keyed_coordinate_value(path, &keys, max_x_key, true)?,
        },
        max_y: GridCoordinate {
            key: max_y_key,
            value: keyed_coordinate_value(path, &keys, max_y_key, false)?,
        },
    })
}

fn keyed_coordinate_value(
    path: &[PointD],
    keys: &[PointKey; 4],
    target: i64,
    x_axis: bool,
) -> Option<f64> {
    let mut value: Option<f64> = None;
    for (point, point_key) in path.iter().zip(keys) {
        let (candidate_key, candidate) = if x_axis {
            (point_key.x, point.x + 0.0)
        } else {
            (point_key.y, point.y + 0.0)
        };
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

fn tiny_coordinates(
    first_min: GridCoordinate,
    first_max: GridCoordinate,
    second_min: GridCoordinate,
    second_max: GridCoordinate,
) -> Option<([GridCoordinate; 4], usize)> {
    let mut coordinates = [first_min, first_max, second_min, second_max];
    coordinates.sort_unstable_by_key(|coordinate| coordinate.key);
    let mut len = 1;
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
    Some((coordinates, len))
}

#[inline]
fn fill_rule_accepts_ring(path: &[PointD], fill_rule: FillRule) -> bool {
    match fill_rule {
        FillRule::EvenOdd | FillRule::NonZero => true,
        FillRule::Positive => signed_area2(path) > 0.0,
        FillRule::Negative => signed_area2(path) < 0.0,
    }
}

#[inline]
fn apply_operation(subject: bool, clip: bool, clip_type: ClipType) -> bool {
    match clip_type {
        ClipType::Intersection => subject && clip,
        ClipType::Union => subject || clip,
        ClipType::Difference => subject && !clip,
        ClipType::Xor => subject != clip,
    }
}

fn add_vertical_transitions(
    boundary: &mut SmallBoundary,
    x: GridCoordinate,
    ys: &[GridCoordinate],
    left: &[bool],
    right: &[bool],
) -> Option<()> {
    for y in 0..left.len() {
        if left[y] == right[y] {
            continue;
        }
        if right[y] {
            push_grid_edge(boundary, x, ys[y + 1], x, ys[y])?;
        } else {
            push_grid_edge(boundary, x, ys[y], x, ys[y + 1])?;
        }
    }
    Some(())
}

fn add_horizontal_transitions(
    boundary: &mut SmallBoundary,
    min_x: GridCoordinate,
    max_x: GridCoordinate,
    ys: &[GridCoordinate],
    filled: &[bool],
) -> Option<()> {
    if filled[0] {
        push_grid_edge(boundary, min_x, ys[0], max_x, ys[0])?;
    }
    for y in 1..filled.len() {
        if filled[y - 1] == filled[y] {
            continue;
        }
        if filled[y] {
            push_grid_edge(boundary, min_x, ys[y], max_x, ys[y])?;
        } else {
            push_grid_edge(boundary, max_x, ys[y], min_x, ys[y])?;
        }
    }
    if filled[filled.len() - 1] {
        let top = ys[filled.len()];
        push_grid_edge(boundary, max_x, top, min_x, top)?;
    }
    Some(())
}

#[inline]
fn push_grid_edge(
    boundary: &mut SmallBoundary,
    start_x: GridCoordinate,
    start_y: GridCoordinate,
    end_x: GridCoordinate,
    end_y: GridCoordinate,
) -> Option<()> {
    boundary.push(DirectedEdge {
        start: PointD::new(start_x.value, start_y.value),
        end: PointD::new(end_x.value, end_y.value),
        start_key: PointKey {
            x: start_x.key,
            y: start_y.key,
        },
        end_key: PointKey {
            x: end_x.key,
            y: end_y.key,
        },
    })
}

fn stitch_small(edges: &[DirectedEdge]) -> Option<PathsD> {
    if edges.is_empty() {
        return Some(Vec::new());
    }
    if edges.len() > MAX_BOUNDARY_EDGES {
        return None;
    }

    let mut next = [0_usize; MAX_BOUNDARY_EDGES];
    for (index, edge) in edges.iter().enumerate() {
        let mut successor = None;
        for (candidate, outgoing) in edges.iter().enumerate() {
            if outgoing.start_key != edge.end_key {
                continue;
            }
            if successor.replace(candidate).is_some() {
                return None;
            }
        }
        next[index] = successor?;
    }

    let mut visited = [false; MAX_BOUNDARY_EDGES];
    let mut paths = Vec::with_capacity(2);
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        let mut path = Vec::with_capacity(8);
        let mut current = start;
        loop {
            if visited[current] {
                if current != start {
                    return None;
                }
                break;
            }
            visited[current] = true;
            path.push(edges[current].start);
            current = next[current];
        }
        if path.len() < 3 || signed_area2(&path).abs() <= f64::EPSILON {
            return None;
        }
        path = crate::trim_collinear_d(&path, crate::PathKind::Closed).ok()?;
        canonicalize(&mut path);
        paths.push(path);
    }
    paths.sort_by(compare_paths);
    Some(paths)
}

fn signed_area2(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(start, end)| start.x * end.y - start.y * end.x)
        .sum()
}

fn canonicalize(path: &mut [PointD]) {
    if let Some((minimum, _)) = path.iter().enumerate().min_by(|(_, left), (_, right)| {
        left.x
            .total_cmp(&right.x)
            .then(left.y.total_cmp(&right.y))
    }) {
        path.rotate_left(minimum);
    }
}

fn compare_paths(left: &PathD, right: &PathD) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| {
            left.x
                .total_cmp(&right.x)
                .then(left.y.total_cmp(&right.y))
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn key(point: PointD) -> Option<PointKey> {
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


#[cfg(test)]
mod tests;
