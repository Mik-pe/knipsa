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
        Self { edges: [DirectedEdge::default(); MAX_BOUNDARY_EDGES], len: 0 }
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
    if let Some(result) = try_convex_zero_area_contact(request) {
        return Some(Ok(result));
    }
    if let Some(result) = try_even_odd_bow_tie(request) {
        return Some(Ok(result));
    }
    crate::fast_dispatch::try_boolean_opd(request)
}

/// Resolves two strictly convex rings whose bounding boxes meet only on a line
/// or point. Positive-length collinear contact is left to the general kernel.
fn try_convex_zero_area_contact(request: BooleanRequestD<'_>) -> Option<PathsD> {
    if request.subjects.len() != 1
        || request.clips.len() != 1
        || !matches!(request.fill_rule, FillRule::EvenOdd | FillRule::NonZero)
    {
        return None;
    }
    let subject = request.subjects[0].as_slice();
    let clip = request.clips[0].as_slice();
    let (subject_keys, subject_positive) = certified_strict_convex(subject)?;
    let (clip_keys, clip_positive) = certified_strict_convex(clip)?;
    let subject_bounds = key_bounds(&subject_keys)?;
    let clip_bounds = key_bounds(&clip_keys)?;
    let overlap_x = subject_bounds.2.min(clip_bounds.2) - subject_bounds.0.max(clip_bounds.0);
    let overlap_y = subject_bounds.3.min(clip_bounds.3) - subject_bounds.1.max(clip_bounds.1);
    if overlap_x < 0 || overlap_y < 0 || (overlap_x != 0 && overlap_y != 0) {
        return None;
    }
    if has_positive_collinear_overlap(&subject_keys, &clip_keys) {
        return None;
    }

    let subject = direct_path(subject, subject_positive);
    match request.clip_type {
        ClipType::Intersection => Some(Vec::new()),
        ClipType::Difference => Some(vec![subject]),
        ClipType::Union | ClipType::Xor => {
            let clip = direct_path(clip, clip_positive);
            let mut result = vec![subject, clip];
            result.sort_by(compare_paths);
            Some(result)
        }
    }
}

/// Splits a four-edge `EvenOdd` bow tie at its single proper crossing.
fn try_even_odd_bow_tie(request: BooleanRequestD<'_>) -> Option<PathsD> {
    if !request.clips.is_empty()
        || request.subjects.len() != 1
        || request.fill_rule != FillRule::EvenOdd
        || !matches!(request.clip_type, ClipType::Union | ClipType::Difference | ClipType::Xor)
    {
        return None;
    }
    let [first, second, third, fourth] = request.subjects[0].as_slice() else {
        return None;
    };
    let points = [*first, *second, *third, *fourth];
    let keys = [
        exact_key(points[0])?,
        exact_key(points[1])?,
        exact_key(points[2])?,
        exact_key(points[3])?,
    ];
    split_bow_tie(points, keys).or_else(|| {
        split_bow_tie(
            [points[1], points[2], points[3], points[0]],
            [keys[1], keys[2], keys[3], keys[0]],
        )
    })
}

fn split_bow_tie(points: [PointD; 4], keys: [PointKey; 4]) -> Option<PathsD> {
    let intersection = proper_intersection(points[0], points[1], points[2], points[3], keys)?;
    let mut first = vec![intersection, points[1], points[2]];
    let mut second = vec![intersection, points[3], points[0]];
    make_positive_triangle(&mut first)?;
    make_positive_triangle(&mut second)?;
    canonicalize(&mut first);
    canonicalize(&mut second);
    let mut result = vec![first, second];
    result.sort_by(compare_paths);
    Some(result)
}

fn certified_strict_convex(path: &[PointD]) -> Option<(Vec<PointKey>, bool)> {
    if path.len() < 3 {
        return None;
    }
    let keys = path.iter().copied().map(exact_key).collect::<Option<Vec<_>>>()?;
    let mut orientation = 0_i8;
    for index in 0..keys.len() {
        let turn =
            orient(keys[index], keys[(index + 1) % keys.len()], keys[(index + 2) % keys.len()]);
        let sign = match turn.cmp(&0) {
            Ordering::Greater => 1,
            Ordering::Less => -1,
            Ordering::Equal => return None,
        };
        if orientation != 0 && orientation != sign {
            return None;
        }
        orientation = sign;
    }
    for first in 0..keys.len() {
        let first_end = (first + 1) % keys.len();
        for second in first + 1..keys.len() {
            let second_end = (second + 1) % keys.len();
            if first_end == second || second_end == first {
                continue;
            }
            if segments_intersect(keys[first], keys[first_end], keys[second], keys[second_end]) {
                return None;
            }
        }
    }
    Some((keys, orientation > 0))
}

fn key_bounds(keys: &[PointKey]) -> Option<(i64, i64, i64, i64)> {
    let first = *keys.first()?;
    Some(keys.iter().skip(1).fold(
        (first.x, first.y, first.x, first.y),
        |(min_x, min_y, max_x, max_y), point| {
            (min_x.min(point.x), min_y.min(point.y), max_x.max(point.x), max_y.max(point.y))
        },
    ))
}

fn has_positive_collinear_overlap(first: &[PointKey], second: &[PointKey]) -> bool {
    first.iter().zip(first.iter().cycle().skip(1)).take(first.len()).any(|(&a, &b)| {
        second.iter().zip(second.iter().cycle().skip(1)).take(second.len()).any(|(&c, &d)| {
            orient(a, b, c) == 0 && orient(a, b, d) == 0 && projected_overlap(a, b, c, d) > 0
        })
    })
}

fn projected_overlap(a: PointKey, b: PointKey, c: PointKey, d: PointKey) -> i64 {
    let use_x = a.x.abs_diff(b.x) >= a.y.abs_diff(b.y);
    let (a, b, c, d) = if use_x { (a.x, b.x, c.x, d.x) } else { (a.y, b.y, c.y, d.y) };
    a.max(b).min(c.max(d)) - a.min(b).max(c.min(d))
}

fn segments_intersect(a: PointKey, b: PointKey, c: PointKey, d: PointKey) -> bool {
    let ab_c = orient(a, b, c);
    let ab_d = orient(a, b, d);
    let cd_a = orient(c, d, a);
    let cd_b = orient(c, d, b);
    (opposite_signs(ab_c, ab_d) && opposite_signs(cd_a, cd_b))
        || (ab_c == 0 && point_on_segment(c, a, b))
        || (ab_d == 0 && point_on_segment(d, a, b))
        || (cd_a == 0 && point_on_segment(a, c, d))
        || (cd_b == 0 && point_on_segment(b, c, d))
}

fn proper_intersection(
    a: PointD,
    b: PointD,
    c: PointD,
    d: PointD,
    keys: [PointKey; 4],
) -> Option<PointD> {
    let [ak, bk, ck, dk] = keys;
    if !opposite_signs(orient(ak, bk, ck), orient(ak, bk, dk))
        || !opposite_signs(orient(ck, dk, ak), orient(ck, dk, bk))
    {
        return None;
    }
    let ab = subtract(b, a);
    let cd = subtract(d, c);
    let between = subtract(c, a);
    let denominator = ab.x.mul_add(cd.y, -(ab.y * cd.x));
    let parameter = between.x.mul_add(cd.y, -(between.y * cd.x)) / denominator;
    let point = PointD::new(ab.x.mul_add(parameter, a.x), ab.y.mul_add(parameter, a.y));
    exact_key(point)?;
    Some(point)
}

fn make_positive_triangle(path: &mut PathD) -> Option<()> {
    let [a, b, c] = path.as_slice() else { return None };
    let turn = orient(exact_key(*a)?, exact_key(*b)?, exact_key(*c)?);
    if turn == 0 {
        return None;
    }
    if turn < 0 {
        path.reverse();
    }
    Some(())
}

fn direct_path(path: &[PointD], positive: bool) -> PathD {
    let mut result = path.to_vec();
    if !positive {
        result.reverse();
    }
    canonicalize(&mut result);
    result
}

fn opposite_signs(first: i128, second: i128) -> bool {
    (first < 0 && second > 0) || (first > 0 && second < 0)
}

fn point_on_segment(point: PointKey, start: PointKey, end: PointKey) -> bool {
    point.x >= start.x.min(end.x)
        && point.x <= start.x.max(end.x)
        && point.y >= start.y.min(end.y)
        && point.y <= start.y.max(end.y)
}

fn orient(a: PointKey, b: PointKey, c: PointKey) -> i128 {
    let vector = (i128::from(b.x) - i128::from(a.x), i128::from(b.y) - i128::from(a.y));
    let offset = (i128::from(c.x) - i128::from(a.x), i128::from(c.y) - i128::from(a.y));
    vector.0 * offset.1 - vector.1 * offset.0
}

#[allow(clippy::cast_precision_loss)]
fn exact_key(point: PointD) -> Option<PointKey> {
    let key = key(point)?;
    let reconstructed = PointD::new(key.x as f64 / KEY_SCALE, key.y as f64 / KEY_SCALE);
    (reconstructed.x.to_bits() == (point.x + 0.0).to_bits()
        && reconstructed.y.to_bits() == (point.y + 0.0).to_bits())
    .then_some(key)
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
    let x_overlap_start = subject_rectangle.min_x.key.max(clip_rectangle.min_x.key);
    let x_overlap_end = subject_rectangle.max_x.key.min(clip_rectangle.max_x.key);
    let y_overlap_start = subject_rectangle.min_y.key.max(clip_rectangle.min_y.key);
    let y_overlap_end = subject_rectangle.max_y.key.min(clip_rectangle.max_y.key);
    if x_overlap_start == x_overlap_end && y_overlap_start == y_overlap_end {
        return None;
    }
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
            let clip_contains =
                clip_enabled && clip_rectangle.contains_cell(xs[x], xs[x + 1], ys[y], ys[y + 1]);
            current[y] = apply_operation(subject_contains, clip_contains, request.clip_type);
        }
        add_vertical_transitions(&mut boundary, xs[x], ys, &previous[..rows], &current[..rows])?;
        add_horizontal_transitions(&mut boundary, xs[x], xs[x + 1], ys, &current[..rows])?;
        std::mem::swap(&mut previous, &mut current);
    }

    current[..rows].fill(false);
    finish_rectangle_boundary(boundary, xs[x_len - 1], ys, &previous[..rows], &current[..rows])
}

fn finish_rectangle_boundary(
    mut boundary: SmallBoundary,
    x: GridCoordinate,
    ys: &[GridCoordinate],
    left: &[bool],
    right: &[bool],
) -> Option<PathsD> {
    add_vertical_transitions(&mut boundary, x, ys, left, right)?;
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

    let west_key = keys.iter().map(|point| point.x).min()?;
    let east_key = keys.iter().map(|point| point.x).max()?;
    let south_key = keys.iter().map(|point| point.y).min()?;
    let north_key = keys.iter().map(|point| point.y).max()?;
    let mut corners = 0_u8;
    for point in keys {
        let x_bit = u32::from(point.x == east_key);
        let y_bit = u32::from(point.y == north_key);
        let bit = 1_u8 << (x_bit + 2 * y_bit);
        if corners & bit != 0 {
            return None;
        }
        corners |= bit;
    }
    Some(AxisAlignedRectangle {
        min_x: GridCoordinate {
            key: west_key,
            value: keyed_coordinate_value(path, &keys, west_key, true)?,
        },
        min_y: GridCoordinate {
            key: south_key,
            value: keyed_coordinate_value(path, &keys, south_key, false)?,
        },
        max_x: GridCoordinate {
            key: east_key,
            value: keyed_coordinate_value(path, &keys, east_key, true)?,
        },
        max_y: GridCoordinate {
            key: north_key,
            value: keyed_coordinate_value(path, &keys, north_key, false)?,
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
        start_key: PointKey { x: start_x.key, y: start_y.key },
        end_key: PointKey { x: end_x.key, y: end_y.key },
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
        let mut successor: Option<usize> = None;
        for (candidate, outgoing) in edges.iter().enumerate() {
            if outgoing.start_key != edge.end_key {
                continue;
            }
            if let Some(current) = successor {
                let incoming = subtract(edge.end, edge.start);
                let current_vector = subtract(edges[current].end, edges[current].start);
                let candidate_vector = subtract(outgoing.end, outgoing.start);
                match compare_turn(incoming, candidate_vector, current_vector) {
                    Ordering::Less => successor = Some(candidate),
                    Ordering::Equal => return None,
                    Ordering::Greater => {}
                }
            } else {
                successor = Some(candidate);
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

fn compare_turn(incoming: PointD, first: PointD, second: PointD) -> Ordering {
    let first_relative = PointD::new(
        incoming.x.mul_add(first.x, incoming.y * first.y),
        incoming.x * first.y - incoming.y * first.x,
    );
    let second_relative = PointD::new(
        incoming.x.mul_add(second.x, incoming.y * second.y),
        incoming.x * second.y - incoming.y * second.x,
    );
    compare_angle(first_relative, second_relative)
}

fn compare_angle(first: PointD, second: PointD) -> Ordering {
    let first_upper = first.y > 0.0 || (first.y.abs() <= f64::EPSILON && first.x >= 0.0);
    let second_upper = second.y > 0.0 || (second.y.abs() <= f64::EPSILON && second.x >= 0.0);
    if first_upper != second_upper {
        return second_upper.cmp(&first_upper);
    }
    let turn = first.x * second.y - first.y * second.x;
    if turn.abs() > f64::EPSILON {
        return if turn > 0.0 { Ordering::Less } else { Ordering::Greater };
    }
    let first_length = first.x.mul_add(first.x, first.y * first.y);
    let second_length = second.x.mul_add(second.x, second.y * second.y);
    first_length.total_cmp(&second_length)
}

fn subtract(first: PointD, second: PointD) -> PointD {
    PointD::new(first.x - second.x, first.y - second.y)
}

fn signed_area2(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(start, end)| start.x * end.y - start.y * end.x)
        .sum()
}

fn canonicalize(path: &mut [PointD]) {
    if let Some((minimum, _)) = path
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
    {
        path.rotate_left(minimum);
    }
}

fn compare_paths(left: &PathD, right: &PathD) -> Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
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
#[path = "standard_dispatch/tests.rs"]
mod tests;
