//! Allocation-light dispatch for the smallest, most common polygon pairs.

use std::cmp::Ordering;

use crate::{
    BooleanRequest, ClipType, FillRule, PathD, PathsD, PointD,
    dispatch::{
        AxisAlignedRectangle, DirectedEdge, GridCoordinate, apply_operation,
        axis_aligned_rectangle, canonicalize, compare_paths, exact_key, fill_rule_accepts_ring,
    },
    geometry::signed_area2_d,
};

const MAX_BOUNDARY_EDGES: usize = 24;

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

pub(crate) fn try_apply(request: &BooleanRequest<'_, PathD>) -> Option<PathsD> {
    if let Some(result) = try_rectangle_pair(request) {
        return Some(result);
    }
    if let Some(result) = try_nested_rectangle_xor(request) {
        return Some(result);
    }
    if let Some(result) = try_convex_zero_area_contact(request) {
        return Some(result);
    }
    if let Some(result) = try_even_odd_bow_tie(request) {
        return Some(result);
    }
    None
}

/// Resolves XOR over strictly nested or disjoint exact-grid rectangles. Under
/// `EvenOdd` semantics every surviving rectangle toggles the combined parity,
/// so its complete boundary can be emitted without an arrangement or stitch.
fn try_nested_rectangle_xor(request: &BooleanRequest<'_, PathD>) -> Option<PathsD> {
    if request.clip_type != ClipType::Xor || request.fill_rule != FillRule::EvenOdd {
        return None;
    }
    let paths = request.closed_subjects.iter().chain(request.clips).filter(|path| !path.is_empty());
    let path_count = paths.clone().count();
    if !(3..=64).contains(&path_count) {
        return None;
    }

    let mut rectangles = Vec::with_capacity(path_count);
    for path in paths {
        if !path.iter().copied().all(|point| exact_key(point).is_some()) {
            return None;
        }
        let rectangle = axis_aligned_rectangle(path)?;
        if let Some(index) = rectangles.iter().position(|known| same_rectangle(*known, rectangle)) {
            rectangles.swap_remove(index);
        } else {
            rectangles.push(rectangle);
        }
    }

    for (index, rectangle) in rectangles.iter().copied().enumerate() {
        for other in rectangles.iter().copied().skip(index + 1) {
            if !rectangles_strictly_disjoint(rectangle, other)
                && !strictly_contains(rectangle, other)
                && !strictly_contains(other, rectangle)
            {
                return None;
            }
        }
    }

    let mut result = rectangles
        .iter()
        .copied()
        .map(|rectangle| {
            let depth = rectangles
                .iter()
                .copied()
                .filter(|other| strictly_contains(*other, rectangle))
                .count();
            rectangle_path(rectangle, depth % 2 == 0)
        })
        .collect::<PathsD>();
    result.sort_by(compare_paths);
    Some(result)
}

fn same_rectangle(first: AxisAlignedRectangle, second: AxisAlignedRectangle) -> bool {
    first.min_x.key == second.min_x.key
        && first.min_y.key == second.min_y.key
        && first.max_x.key == second.max_x.key
        && first.max_y.key == second.max_y.key
}

fn rectangles_strictly_disjoint(first: AxisAlignedRectangle, second: AxisAlignedRectangle) -> bool {
    first.max_x.key < second.min_x.key
        || second.max_x.key < first.min_x.key
        || first.max_y.key < second.min_y.key
        || second.max_y.key < first.min_y.key
}

fn strictly_contains(outer: AxisAlignedRectangle, inner: AxisAlignedRectangle) -> bool {
    outer.min_x.key < inner.min_x.key
        && outer.min_y.key < inner.min_y.key
        && outer.max_x.key > inner.max_x.key
        && outer.max_y.key > inner.max_y.key
}

fn rectangle_path(rectangle: AxisAlignedRectangle, positive: bool) -> PathD {
    let mut path = vec![
        PointD::new(rectangle.min_x.value, rectangle.min_y.value),
        PointD::new(rectangle.max_x.value, rectangle.min_y.value),
        PointD::new(rectangle.max_x.value, rectangle.max_y.value),
        PointD::new(rectangle.min_x.value, rectangle.max_y.value),
    ];
    if !positive {
        path.reverse();
    }
    canonicalize(&mut path);
    path
}

/// Resolves strictly convex rings whose bounding boxes meet only on a line or
/// point. Positive-length collinear contact stays on the general path.
fn try_convex_zero_area_contact(request: &BooleanRequest<'_, PathD>) -> Option<PathsD> {
    if request.closed_subjects.len() != 1
        || request.clips.len() != 1
        || !matches!(request.fill_rule, FillRule::EvenOdd | FillRule::NonZero)
    {
        return None;
    }

    let subject = request.closed_subjects[0].as_slice();
    let clip = request.clips[0].as_slice();
    if !bounds_have_zero_area_contact(path_bounds(subject)?, path_bounds(clip)?) {
        return None;
    }
    let subject_keys = exact_path_keys(subject)?;
    let clip_keys = exact_path_keys(clip)?;

    let subject_positive = certified_strict_convex(&subject_keys)?;
    let clip_positive = certified_strict_convex(&clip_keys)?;
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

fn path_bounds(path: &[PointD]) -> Option<(f64, f64, f64, f64)> {
    let first = *path.first()?;
    if !first.x.is_finite() || !first.y.is_finite() {
        return None;
    }
    path.iter().copied().skip(1).try_fold(
        (first.x, first.y, first.x, first.y),
        |(x_minimum, y_minimum, x_maximum, y_maximum), point| {
            (point.x.is_finite() && point.y.is_finite()).then_some((
                x_minimum.min(point.x),
                y_minimum.min(point.y),
                x_maximum.max(point.x),
                y_maximum.max(point.y),
            ))
        },
    )
}

fn bounds_have_zero_area_contact(
    first: (f64, f64, f64, f64),
    second: (f64, f64, f64, f64),
) -> bool {
    let overlap_x = first.2.min(second.2) - first.0.max(second.0);
    let overlap_y = first.3.min(second.3) - first.1.max(second.1);
    overlap_x >= 0.0 && overlap_y >= 0.0 && (overlap_x == 0.0 || overlap_y == 0.0)
}

/// Splits a four-edge `EvenOdd` bow tie at its single proper crossing.
fn try_even_odd_bow_tie(request: &BooleanRequest<'_, PathD>) -> Option<PathsD> {
    if !request.clips.is_empty()
        || request.closed_subjects.len() != 1
        || request.fill_rule != FillRule::EvenOdd
        || !matches!(request.clip_type, ClipType::Union | ClipType::Difference | ClipType::Xor)
    {
        return None;
    }
    let [first, second, third, fourth] = request.closed_subjects[0].as_slice() else {
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

fn split_bow_tie(points: [PointD; 4], keys: [crate::dispatch::PointKey; 4]) -> Option<PathsD> {
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

fn exact_path_keys(path: &[PointD]) -> Option<Vec<crate::dispatch::PointKey>> {
    (path.len() >= 3).then(|| path.iter().copied().map(exact_key).collect::<Option<Vec<_>>>())?
}

fn certified_strict_convex(keys: &[crate::dispatch::PointKey]) -> Option<bool> {
    let mut direction = None;
    for index in 0..keys.len() {
        let turn =
            orient(keys[index], keys[(index + 1) % keys.len()], keys[(index + 2) % keys.len()]);
        let positive = match turn.cmp(&0) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => return None,
        };
        if direction.is_some_and(|known| known != positive) {
            return None;
        }
        direction = Some(positive);
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
    direction
}

fn has_positive_collinear_overlap(
    first: &[crate::dispatch::PointKey],
    second: &[crate::dispatch::PointKey],
) -> bool {
    first.iter().zip(first.iter().cycle().skip(1)).take(first.len()).any(|(&a, &b)| {
        second.iter().zip(second.iter().cycle().skip(1)).take(second.len()).any(|(&c, &d)| {
            orient(a, b, c) == 0 && orient(a, b, d) == 0 && projected_overlap(a, b, c, d) > 0
        })
    })
}

fn projected_overlap(
    a: crate::dispatch::PointKey,
    b: crate::dispatch::PointKey,
    c: crate::dispatch::PointKey,
    d: crate::dispatch::PointKey,
) -> i64 {
    let use_x = a.x.abs_diff(b.x) >= a.y.abs_diff(b.y);
    let (a, b, c, d) = if use_x { (a.x, b.x, c.x, d.x) } else { (a.y, b.y, c.y, d.y) };
    a.max(b).min(c.max(d)) - a.min(b).max(c.min(d))
}

fn segments_intersect(
    a: crate::dispatch::PointKey,
    b: crate::dispatch::PointKey,
    c: crate::dispatch::PointKey,
    d: crate::dispatch::PointKey,
) -> bool {
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
    keys: [crate::dispatch::PointKey; 4],
) -> Option<PointD> {
    let [a_key, b_key, c_key, d_key] = keys;
    if !opposite_signs(orient(a_key, b_key, c_key), orient(a_key, b_key, d_key))
        || !opposite_signs(orient(c_key, d_key, a_key), orient(c_key, d_key, b_key))
    {
        return None;
    }
    let first = subtract(b, a);
    let second = subtract(d, c);
    let between = subtract(c, a);
    let denominator = first.x.mul_add(second.y, -(first.y * second.x));
    let parameter = between.x.mul_add(second.y, -(between.y * second.x)) / denominator;
    let point = PointD::new(first.x.mul_add(parameter, a.x), first.y.mul_add(parameter, a.y));
    exact_key(point)?;
    Some(point)
}

fn make_positive_triangle(path: &mut Vec<PointD>) -> Option<()> {
    let [a, b, c] = path.as_slice() else {
        return None;
    };
    let turn = orient(exact_key(*a)?, exact_key(*b)?, exact_key(*c)?);
    if turn == 0 {
        return None;
    }
    if turn < 0 {
        path.reverse();
    }
    Some(())
}

fn direct_path(path: &[PointD], positive: bool) -> Vec<PointD> {
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

fn point_on_segment(
    point: crate::dispatch::PointKey,
    start: crate::dispatch::PointKey,
    end: crate::dispatch::PointKey,
) -> bool {
    point.x >= start.x.min(end.x)
        && point.x <= start.x.max(end.x)
        && point.y >= start.y.min(end.y)
        && point.y <= start.y.max(end.y)
}

fn orient(
    a: crate::dispatch::PointKey,
    b: crate::dispatch::PointKey,
    c: crate::dispatch::PointKey,
) -> i128 {
    let vector = (i128::from(b.x) - i128::from(a.x), i128::from(b.y) - i128::from(a.y));
    let offset = (i128::from(c.x) - i128::from(a.x), i128::from(c.y) - i128::from(a.y));
    vector.0 * offset.1 - vector.1 * offset.0
}

fn subtract(first: PointD, second: PointD) -> PointD {
    PointD::new(first.x - second.x, first.y - second.y)
}

/// Handles one axis-aligned rectangle per side without coordinate vectors,
/// event streams, winding buffers, or hash-based boundary stitching.
fn try_rectangle_pair(request: &BooleanRequest<'_, PathD>) -> Option<PathsD> {
    if request.closed_subjects.len() != 1 || request.clips.len() != 1 {
        return None;
    }
    let subject = request.closed_subjects[0].as_slice();
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
            let clip_contains =
                clip_enabled && clip_rectangle.contains_cell(xs[x], xs[x + 1], ys[y], ys[y + 1]);
            current[y] = apply_operation(subject_contains, clip_contains, request.clip_type);
        }
        add_vertical_transitions(&mut boundary, xs[x], ys, &previous[..rows], &current[..rows])?;
        add_horizontal_transitions(&mut boundary, xs[x], xs[x + 1], ys, &current[..rows])?;
        std::mem::swap(&mut previous, &mut current);
    }

    finish_rectangle_grid(&mut boundary, xs[x_len - 1], ys, &previous[..rows], &mut current[..rows])
}

fn finish_rectangle_grid(
    boundary: &mut SmallBoundary,
    final_x: GridCoordinate,
    ys: &[GridCoordinate],
    previous: &[bool],
    current: &mut [bool],
) -> Option<PathsD> {
    current.fill(false);
    add_vertical_transitions(boundary, final_x, ys, previous, current)?;
    stitch_small(boundary.as_slice())
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
    boundary.push(DirectedEdge::from_grid(start_x, start_y, end_x, end_y))
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
        if path.len() < 3 || signed_area2_d(&path).abs() <= f64::EPSILON {
            return None;
        }
        path = crate::trim_collinear_d(&path, crate::PathKind::Closed).ok()?;
        canonicalize(&mut path);
        paths.push(path);
    }
    paths.sort_by(compare_paths);
    Some(paths)
}

#[cfg(test)]
mod tests;
