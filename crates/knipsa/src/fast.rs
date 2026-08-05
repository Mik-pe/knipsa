//! Conservative floating-point fast path for ordinary, well-conditioned input.
//!
//! The exact arrangement remains the fallback. This path is deliberately
//! bounded to coordinates that can be keyed without loss at the output
//! resolution and to segment configurations whose predicates are comfortably
//! away from zero.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use crate::{BooleanRequestD, ClipType, Error, FillRule, PathD, PathsD, PointD, normalize_pathd};

const KEY_SCALE: f64 = 1_000_000_000.0;
const MAX_COORDINATE: f64 = 1_000_000.0;
const PREDICATE_TOLERANCE: f64 = 1.0e-12;
const SAMPLE_SCALE: f64 = 1.0e-9;
const CONTAINMENT_BUCKETS: usize = 32;

#[derive(Default)]
struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(u64::from_ne_bytes(value.to_ne_bytes()));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 ^= value.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.0 = self.0.rotate_left(27).wrapping_mul(0x3c79_ac49_2ba7_b653);
    }
}

// These tables are private and keyed only by quantized geometry. A fixed
// lightweight hasher avoids paying SipHash's general-purpose cost in the hot
// arrangement path while keeping the public API independent of the choice.
type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
type FastSet<T> = HashSet<T, BuildHasherDefault<FastHasher>>;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PointKey {
    x: i64,
    y: i64,
}

type Point = PointD;

trait PathSlice {
    fn points(&self) -> &[Point];
}

impl PathSlice for Vec<Point> {
    fn points(&self) -> &[Point] {
        self
    }
}

enum FastPath<'a> {
    Borrowed(&'a [Point]),
    Owned(Vec<Point>),
}

impl PathSlice for FastPath<'_> {
    fn points(&self) -> &[Point] {
        match self {
            Self::Borrowed(points) => points,
            Self::Owned(points) => points,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    start: Point,
    end: Point,
    start_key: PointKey,
    end_key: PointKey,
    path_id: usize,
    subject: bool,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    min_x_key: u64,
    max_x_key: u64,
}

#[derive(Clone, Copy, Debug)]
struct DirectedEdge {
    start: Point,
    end: Point,
    start_key: PointKey,
    end_key: PointKey,
}

#[derive(Clone, Copy, Debug)]
struct ContainmentEdge {
    start: Point,
    delta_x: f64,
    delta_y: f64,
    intercept: f64,
    upward: bool,
}

#[derive(Clone, Debug)]
struct ContainmentPath {
    bounds: (f64, f64, f64, f64),
    buckets: Vec<Vec<ContainmentEdge>>,
    convex_points: Option<Vec<Point>>,
    convex_winding: Option<i32>,
}

#[derive(Clone, Copy, Debug)]
struct LocalSides {
    left: bool,
    right: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct PathProperties {
    simple: bool,
    convex: bool,
}

pub(crate) fn try_boolean_opd(request: BooleanRequestD<'_>) -> Option<Result<PathsD, ()>> {
    let subjects = fast_paths(request.subjects)?;
    let clips = fast_paths(request.clips)?;
    if !eligible(&subjects, &clips) {
        return None;
    }
    Some(run(&subjects, &clips, request.clip_type, request.fill_rule).map_err(|_| ()))
}

fn fast_paths(paths: &[PathD]) -> Option<Vec<FastPath<'_>>> {
    paths
        .iter()
        .map(|path| {
            let points = if path.windows(2).any(|window| window[0] == window[1])
                || (path.len() > 1 && path.first() == path.last())
            {
                FastPath::Owned(normalize_pathd(path, crate::PathKind::Closed))
            } else {
                FastPath::Borrowed(path)
            };
            if points.points().len() >= 3 && !keyable_path(points.points()) {
                None
            } else {
                Some(points)
            }
        })
        .collect::<Option<Vec<_>>>()
        .map(|paths| paths.into_iter().filter(|path| path.points().len() >= 3).collect())
}

fn keyable_path(points: &[Point]) -> bool {
    let Some(first) = points.first().copied().and_then(key) else { return false };
    let mut previous = first;
    for point in points.iter().skip(1).copied() {
        let Some(current) = key(point) else { return false };
        if current == previous {
            return false;
        }
        previous = current;
    }
    previous != first
}

fn eligible<P: PathSlice>(subjects: &[P], clips: &[P]) -> bool {
    for path in subjects.iter().chain(clips) {
        let points = path.points();
        if points.len() < 3 {
            continue;
        }
        for (start, end) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
            if !start.x.is_finite()
                || !start.y.is_finite()
                || !end.x.is_finite()
                || !end.y.is_finite()
                || start.x.abs() > MAX_COORDINATE
                || start.y.abs() > MAX_COORDINATE
                || end.x.abs() > MAX_COORDINATE
                || end.y.abs() > MAX_COORDINATE
            {
                return false;
            }
        }
    }
    true
}

fn boxes_disjoint(first: &Edge, second: &Edge) -> bool {
    first.max_x < second.min_x
        || second.max_x < first.min_x
        || first.max_y < second.min_y
        || second.max_y < first.min_y
}

#[allow(clippy::too_many_lines)]
fn run<P: PathSlice>(
    subjects: &[P],
    clips: &[P],
    clip_type: ClipType,
    fill_rule: FillRule,
) -> Result<PathsD, Error> {
    if let Some(result) = short_circuit(subjects, clips, clip_type, fill_rule) {
        return Ok(result);
    }
    let mut edges = Vec::new();
    let mut path_properties = Vec::with_capacity(subjects.len() + clips.len());
    for (path_id, path) in subjects.iter().enumerate() {
        let points = path.points();
        path_properties.push(classify_path(points));
        edges.extend(path_edges(points).into_iter().map(|mut edge| {
            edge.path_id = path_id;
            edge.subject = true;
            edge
        }));
    }
    for (clip_index, path) in clips.iter().enumerate() {
        let path_id = subjects.len() + clip_index;
        let points = path.points();
        path_properties.push(classify_path(points));
        edges.extend(path_edges(points).into_iter().map(|mut edge| {
            edge.path_id = path_id;
            edge
        }));
    }
    if edges.is_empty() {
        return Ok(Vec::new());
    }

    let mut parameters = vec![vec![0.0, 1.0]; edges.len()];
    if !split_pairs(&edges, &path_properties, &mut parameters) {
        return Err(Error::TopologyFailure);
    }

    let scale = maximum_coordinate(subjects, clips).max(1.0);
    let sample = SAMPLE_SCALE / scale;
    let subject_sides = simple_local_sides_with_hint(
        subjects,
        fill_rule,
        path_properties.first().map(|properties| properties.simple),
    );
    let clip_sides = simple_local_sides_with_hint(
        clips,
        fill_rule,
        path_properties.get(subjects.len()).map(|properties| properties.simple),
    );
    let subject_paths = if clips.is_empty() && subject_sides.is_some() {
        Vec::new()
    } else {
        containment_paths_with_properties(subjects, &path_properties[..subjects.len()])
    };
    let clip_paths = if subjects.is_empty() && clip_sides.is_some() {
        Vec::new()
    } else {
        containment_paths_with_properties(clips, &path_properties[subjects.len()..])
    };
    let mut directed = Vec::with_capacity(edges.len());
    let mut seen: FastSet<(PointKey, PointKey)> =
        FastSet::with_capacity_and_hasher(edges.len(), BuildHasherDefault::default());
    for (edge, values) in edges.iter().zip(parameters.iter_mut()) {
        if values.len() > 2 {
            values.sort_unstable_by(f64::total_cmp);
            values.dedup_by(|left, right| (*left - *right).abs() <= 1.0e-12);
        }
        for pair in values.windows(2) {
            let start = point_at(*edge, pair[0]);
            let end = point_at(*edge, pair[1]);
            let start_key = key(start).expect("eligible split point is keyable");
            let end_key = key(end).expect("eligible split point is keyable");
            if start_key == end_key {
                continue;
            }
            let midpoint = Point { x: (start.x + end.x) * 0.5, y: (start.y + end.y) * 0.5 };
            let vector = subtract(end, start);
            let left_sample =
                Point { x: midpoint.x - vector.y * sample, y: midpoint.y + vector.x * sample };
            let right_sample =
                Point { x: midpoint.x + vector.y * sample, y: midpoint.y - vector.x * sample };
            let (subject_left, subject_right) = containment_sides(
                left_sample,
                right_sample,
                &subject_paths,
                edge.subject.then_some(subject_sides).flatten(),
                fill_rule,
            );
            let (clip_left, clip_right) = containment_sides(
                left_sample,
                right_sample,
                &clip_paths,
                (!edge.subject).then_some(clip_sides).flatten(),
                fill_rule,
            );
            let left = apply_operation(subject_left, clip_left, clip_type);
            let right = apply_operation(subject_right, clip_right, clip_type);
            if left == right {
                continue;
            }
            let edge = if left {
                DirectedEdge { start, end, start_key, end_key }
            } else {
                DirectedEdge { start: end, end: start, start_key: end_key, end_key: start_key }
            };
            if seen.insert((edge.start_key, edge.end_key)) {
                directed.push(edge);
            }
        }
    }
    stitch(&directed)
}

/// Calls the exact floating-point pair predicate only for edges whose X
/// intervals overlap. The active list is kept in sweep order, so disjoint
/// edge pairs never reach the comparatively expensive intersection math.
fn split_pairs(
    edges: &[Edge],
    path_properties: &[PathProperties],
    parameters: &mut [Vec<f64>],
) -> bool {
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|index| (edges[*index].min_x_key, edges[*index].max_x_key, *index));

    let mut active: Vec<usize> = Vec::with_capacity(edges.len());
    for &index in &order {
        let min_x = edges[index].min_x;
        active.retain(|other| edges[*other].max_x >= min_x);
        for &other in &active {
            let (first, second) = if other < index { (other, index) } else { (index, other) };
            if edges[first].path_id == edges[second].path_id
                && path_properties[edges[first].path_id].simple
            {
                continue;
            }
            let (before, after) = parameters.split_at_mut(second);
            if !split_pair(&edges[first], &edges[second], &mut before[first], &mut after[0]) {
                return false;
            }
        }
        active.push(index);
    }
    true
}

fn classify_path(path: &[Point]) -> PathProperties {
    if path.len() < 3 {
        return PathProperties::default();
    }
    if is_convex_simple(path) {
        return PathProperties { simple: true, convex: true };
    }
    let edges = path_edges(path);
    for (first, edge) in edges.iter().enumerate() {
        for (other_index, other) in edges.iter().enumerate().skip(first + 1) {
            if other_index == first + 1 || (first == 0 && other_index + 1 == edges.len()) {
                continue;
            }
            if edges_intersect(edge, other) {
                return PathProperties::default();
            }
        }
    }
    PathProperties { simple: true, convex: false }
}

fn short_circuit<P: PathSlice>(
    subjects: &[P],
    clips: &[P],
    clip_type: ClipType,
    fill_rule: FillRule,
) -> Option<PathsD> {
    if !matches!(fill_rule, FillRule::EvenOdd | FillRule::NonZero) {
        return None;
    }
    if subjects.is_empty() && clips.is_empty() {
        return Some(Vec::new());
    }
    if clips.is_empty() {
        return match clip_type {
            ClipType::Intersection => Some(Vec::new()),
            ClipType::Difference | ClipType::Union | ClipType::Xor => {
                direct_if_simple_and_disjoint(subjects)
            }
        };
    }
    if subjects.is_empty() {
        return match clip_type {
            ClipType::Intersection | ClipType::Difference => Some(Vec::new()),
            ClipType::Union | ClipType::Xor => direct_if_simple_and_disjoint(clips),
        };
    }
    let subjects_box = bbox(subjects)?;
    let clips_box = bbox(clips)?;
    if subjects_box.2 < clips_box.0
        || clips_box.2 < subjects_box.0
        || subjects_box.3 < clips_box.1
        || clips_box.3 < subjects_box.1
    {
        if !paths_are_simple_and_disjoint(subjects) || !paths_are_simple_and_disjoint(clips) {
            return None;
        }
        let mut result = direct_paths(subjects);
        if matches!(clip_type, ClipType::Union | ClipType::Xor) {
            result.extend(direct_paths(clips));
        }
        return Some(result);
    }
    None
}

fn direct_if_simple_and_disjoint<P: PathSlice>(paths: &[P]) -> Option<PathsD> {
    paths_are_simple_and_disjoint(paths).then(|| direct_paths(paths))
}

fn direct_paths<P: PathSlice>(paths: &[P]) -> PathsD {
    let mut result = paths
        .iter()
        .map(|path| {
            let mut points = path.points().to_vec();
            if area2(&points) < 0.0 {
                points.reverse();
            }
            canonicalize(&mut points);
            points.into_iter().map(|point| PointD::new(point.x, point.y)).collect()
        })
        .collect::<PathsD>();
    result.sort_by(compare_paths);
    result
}

#[rustfmt::skip]
fn paths_are_simple_and_disjoint<P: PathSlice>(paths: &[P]) -> bool {
    for (index, path) in paths.iter().enumerate() {
        let points = path.points();
        if !is_convex_simple(points) {
            let edges = path_edges(points);
            for (first, edge) in edges.iter().enumerate() {
                for (other_index, other) in edges.iter().enumerate().skip(first + 1) {
                    if other_index == first + 1 || (first == 0 && other_index + 1 == edges.len()) {
                        continue;
                    }
                    if edges_intersect(edge, other) { return false; }
                }
            }
        }
        let Some(path_box) = bbox(std::slice::from_ref(path)) else { continue };
        for other in paths.iter().skip(index + 1) {
            let Some(other_box) = bbox(std::slice::from_ref(other)) else { continue };
            if !(path_box.2 < other_box.0 || other_box.2 < path_box.0 || path_box.3 < other_box.1 || other_box.3 < path_box.1) { return false; }
        }
    }
    true
}

fn edges_intersect(first: &Edge, second: &Edge) -> bool {
    if boxes_disjoint(first, second) {
        return false;
    }
    let first_vector = subtract(first.end, first.start);
    let second_vector = subtract(second.end, second.start);
    let between = subtract(second.start, first.start);
    let denominator = cross(first_vector, second_vector);
    if (key_cross(first, second) != 0) & (denominator.abs() > f64::EPSILON) {
        let first_t = cross(between, second_vector) / denominator;
        let second_t = cross(between, first_vector) / denominator;
        return in_unit_interval(first_t) & in_unit_interval(second_t);
    }
    cross(between, first_vector).abs() <= f64::EPSILON
}

fn bbox<P: PathSlice>(paths: &[P]) -> Option<(f64, f64, f64, f64)> {
    paths.iter().flat_map(|path| path.points().iter()).fold(None, |bounds, point| {
        Some(match bounds {
            None => (point.x, point.y, point.x, point.y),
            Some((min_x, min_y, max_x, max_y)) => {
                (min_x.min(point.x), min_y.min(point.y), max_x.max(point.x), max_y.max(point.y))
            }
        })
    })
}

fn path_edges(path: &[Point]) -> Vec<Edge> {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .filter_map(|(start, end)| {
            let start_key = key(*start)?;
            let end_key = key(*end)?;
            (start_key != end_key).then_some(Edge {
                start: *start,
                end: *end,
                start_key,
                end_key,
                path_id: 0,
                subject: false,
                min_x: start.x.min(end.x),
                min_y: start.y.min(end.y),
                max_x: start.x.max(end.x),
                max_y: start.y.max(end.y),
                min_x_key: total_order_key(start.x.min(end.x)),
                max_x_key: total_order_key(start.x.max(end.x)),
            })
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn key(point: Point) -> Option<PointKey> {
    let x = (point.x * KEY_SCALE).round();
    let y = (point.y * KEY_SCALE).round();
    if !x.is_finite() || !y.is_finite() || x.abs() > i64::MAX as f64 || y.abs() > i64::MAX as f64 {
        None
    } else {
        Some(PointKey { x: x as i64, y: y as i64 })
    }
}

#[inline]
fn total_order_key(value: f64) -> u64 {
    let bits = value.to_bits();
    let signed = i64::from_ne_bytes(bits.to_ne_bytes());
    let sign_mask = u64::from_ne_bytes((signed >> 63).to_ne_bytes()) >> 1;
    bits ^ sign_mask ^ (1_u64 << 63)
}

fn split_pair(
    first: &Edge,
    second: &Edge,
    first_values: &mut Vec<f64>,
    second_values: &mut Vec<f64>,
) -> bool {
    if boxes_disjoint(first, second) {
        return true;
    }
    let first_vector = subtract(first.end, first.start);
    let second_vector = subtract(second.end, second.start);
    let denominator = cross(first_vector, second_vector);
    let between = subtract(second.start, first.start);
    if denominator != 0.0 {
        let scale = first_vector.x.abs().max(first_vector.y.abs())
            * second_vector.x.abs().max(second_vector.y.abs());
        if denominator.abs() <= PREDICATE_TOLERANCE * scale.max(1.0)
            && key_cross(first, second) != 0
        {
            return false;
        }
    }
    if (key_cross(first, second) != 0) & (denominator.abs() > f64::EPSILON) {
        let first_t = cross(between, second_vector) / denominator;
        let second_t = cross(between, first_vector) / denominator;
        if in_unit_interval(first_t) & in_unit_interval(second_t) {
            first_values.push(first_t.clamp(0.0, 1.0));
            second_values.push(second_t.clamp(0.0, 1.0));
        }
    } else if cross(between, first_vector).abs() <= f64::EPSILON {
        for point in [second.start, second.end] {
            if on_segment(point, first.start, first.end) {
                first_values.push(parameter(point, first));
            }
        }
        for point in [first.start, first.end] {
            if on_segment(point, second.start, second.end) {
                second_values.push(parameter(point, second));
            }
        }
    }
    true
}

fn in_unit_interval(value: f64) -> bool {
    (-f64::EPSILON..=1.0 + f64::EPSILON).contains(&value)
}

fn parameter(point: Point, edge: &Edge) -> f64 {
    let vector = subtract(edge.end, edge.start);
    if vector.x.abs() >= vector.y.abs() {
        (point.x - edge.start.x) / vector.x
    } else {
        (point.y - edge.start.y) / vector.y
    }
}

fn point_at(edge: Edge, value: f64) -> Point {
    Point {
        x: edge.start.x + (edge.end.x - edge.start.x) * value,
        y: edge.start.y + (edge.end.y - edge.start.y) * value,
    }
}

fn subtract(left: Point, right: Point) -> Point {
    Point { x: left.x - right.x, y: left.y - right.y }
}

fn cross(left: Point, right: Point) -> f64 {
    left.x * right.y - left.y * right.x
}

fn key_cross(first: &Edge, second: &Edge) -> i128 {
    let first_x = i128::from(first.end_key.x) - i128::from(first.start_key.x);
    let first_y = i128::from(first.end_key.y) - i128::from(first.start_key.y);
    let second_x = i128::from(second.end_key.x) - i128::from(second.start_key.x);
    let second_y = i128::from(second.end_key.y) - i128::from(second.start_key.y);
    first_x * second_y - first_y * second_x
}

fn on_segment(point: Point, start: Point, end: Point) -> bool {
    cross(subtract(point, start), subtract(end, start)).abs() <= f64::EPSILON
        && point.x >= start.x.min(end.x) - f64::EPSILON
        && point.x <= start.x.max(end.x) + f64::EPSILON
        && point.y >= start.y.min(end.y) - f64::EPSILON
        && point.y <= start.y.max(end.y) + f64::EPSILON
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

#[inline]
fn containment_sides(
    left: Point,
    right: Point,
    paths: &[ContainmentPath],
    known: Option<LocalSides>,
    fill_rule: FillRule,
) -> (bool, bool) {
    if let Some(sides) = known {
        (sides.left, sides.right)
    } else {
        paths_contain_pair(left, right, paths, fill_rule)
    }
}

fn simple_local_sides_with_hint<P: PathSlice>(
    paths: &[P],
    fill_rule: FillRule,
    simple_hint: Option<bool>,
) -> Option<LocalSides> {
    if paths.len() != 1 || paths[0].points().len() < 3 {
        return None;
    }
    let points = paths[0].points();
    if !simple_hint.unwrap_or_else(|| classify_path(points).simple) {
        return None;
    }
    let positive_winding_on_left = area2(points) > 0.0;
    Some(match fill_rule {
        FillRule::EvenOdd | FillRule::NonZero => {
            LocalSides { left: positive_winding_on_left, right: !positive_winding_on_left }
        }
        FillRule::Positive => LocalSides { left: positive_winding_on_left, right: false },
        FillRule::Negative => LocalSides { left: false, right: !positive_winding_on_left },
    })
}

fn is_convex_simple(path: &[Point]) -> bool {
    let mut keys: FastSet<PointKey> = FastSet::default();
    for point in path {
        let Some(point_key) = key(*point) else { return false };
        if !keys.insert(point_key) {
            return false;
        }
    }
    let mut turn = None;
    for index in 0..path.len() {
        let previous = path[(index + path.len() - 1) % path.len()];
        let current = path[index];
        let next = path[(index + 1) % path.len()];
        let cross_value = cross(subtract(current, previous), subtract(next, current));
        if cross_value.abs() <= f64::EPSILON {
            continue;
        }
        let positive = cross_value > 0.0;
        if turn.is_some_and(|known| known != positive) {
            return false;
        }
        turn = Some(positive);
    }
    turn.is_some()
}

fn paths_contain_pair(
    left: Point,
    right: Point,
    paths: &[ContainmentPath],
    fill_rule: FillRule,
) -> (bool, bool) {
    let mut left_state = WindingState::default();
    let mut right_state = WindingState::default();
    for path in paths {
        if let (Some(points), Some(winding)) = (&path.convex_points, path.convex_winding) {
            update_convex_winding(&mut left_state, left, points, winding);
            update_convex_winding(&mut right_state, right, points, winding);
            continue;
        }
        let (min_x, min_y, max_x, max_y) = path.bounds;
        let left_bucket = containment_bucket_if_inside(left, min_x, min_y, max_x, max_y);
        let right_bucket = containment_bucket_if_inside(right, min_x, min_y, max_x, max_y);
        match (left_bucket, right_bucket) {
            (None, None) => {}
            (Some(bucket), None) => {
                for edge in &path.buckets[bucket] {
                    update_winding(&mut left_state, left, edge);
                }
            }
            (None, Some(bucket)) => {
                for edge in &path.buckets[bucket] {
                    update_winding(&mut right_state, right, edge);
                }
            }
            (Some(left_bucket), Some(right_bucket)) if left_bucket == right_bucket => {
                for edge in &path.buckets[left_bucket] {
                    update_winding(&mut left_state, left, edge);
                    update_winding(&mut right_state, right, edge);
                }
            }
            (Some(left_bucket), Some(right_bucket)) => {
                for edge in &path.buckets[left_bucket] {
                    update_winding(&mut left_state, left, edge);
                }
                for edge in &path.buckets[right_bucket] {
                    update_winding(&mut right_state, right, edge);
                }
            }
        }
    }
    (left_state.contains(fill_rule), right_state.contains(fill_rule))
}

fn update_convex_winding(state: &mut WindingState, point: Point, points: &[Point], winding: i32) {
    if convex_contains(point, points, winding > 0) {
        state.parity = !state.parity;
        state.winding += winding;
    }
}

fn convex_contains(point: Point, points: &[Point], positive: bool) -> bool {
    let origin = points[0];
    let target = subtract(point, origin);
    let first = cross(subtract(points[1], origin), target);
    let last = cross(subtract(points[points.len() - 1], origin), target);
    if positive {
        if first < 0.0 || last > 0.0 {
            return false;
        }
    } else if first > 0.0 || last < 0.0 {
        return false;
    }

    let mut lower = 1;
    let mut upper = points.len() - 1;
    while upper - lower > 1 {
        let middle = lower.midpoint(upper);
        let cross_value = cross(subtract(points[middle], origin), target);
        if (positive && cross_value >= 0.0) || (!positive && cross_value <= 0.0) {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    let edge = subtract(points[(lower + 1) % points.len()], points[lower]);
    let to_point = subtract(point, points[lower]);
    let boundary = cross(edge, to_point);
    if positive { boundary >= 0.0 } else { boundary <= 0.0 }
}

#[derive(Clone, Copy, Debug, Default)]
struct WindingState {
    parity: bool,
    winding: i32,
}

impl WindingState {
    fn contains(self, fill_rule: FillRule) -> bool {
        match fill_rule {
            FillRule::EvenOdd => self.parity,
            FillRule::NonZero => self.winding != 0,
            FillRule::Positive => self.winding > 0,
            FillRule::Negative => self.winding < 0,
        }
    }
}

fn update_winding(state: &mut WindingState, point: Point, edge: &ContainmentEdge) {
    if (edge.start.y > point.y) != (edge.start.y + edge.delta_y > point.y) {
        let direction =
            edge.delta_x.mul_add(point.y, (-edge.delta_y).mul_add(point.x, edge.intercept));
        if edge.upward && direction > 0.0 {
            state.parity = !state.parity;
            state.winding += 1;
        } else if !edge.upward && direction < 0.0 {
            state.parity = !state.parity;
            state.winding -= 1;
        }
    }
}

fn containment_bucket_if_inside(
    point: Point,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Option<usize> {
    (point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y)
        .then(|| containment_bucket(point.y, min_y, max_y))
}

#[cfg(test)]
fn containment_paths<P: PathSlice>(paths: &[P]) -> Vec<ContainmentPath> {
    containment_paths_with_properties(paths, &[])
}

fn containment_paths_with_properties<P: PathSlice>(
    paths: &[P],
    properties: &[PathProperties],
) -> Vec<ContainmentPath> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let points = path.points();
            let bounds = points.iter().fold(
                (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                |(min_x, min_y, max_x, max_y), point| {
                    (min_x.min(point.x), min_y.min(point.y), max_x.max(point.x), max_y.max(point.y))
                },
            );
            let edges: Vec<ContainmentEdge> = points
                .iter()
                .zip(points.iter().cycle().skip(1))
                .take(points.len())
                .map(|(start, end)| {
                    let delta_x = end.x - start.x;
                    let delta_y = end.y - start.y;
                    ContainmentEdge {
                        start: *start,
                        delta_x,
                        delta_y,
                        intercept: delta_y.mul_add(start.x, -delta_x * start.y),
                        upward: delta_y > 0.0,
                    }
                })
                .collect();
            if properties.get(index).is_some_and(|properties| properties.convex) {
                return ContainmentPath {
                    bounds,
                    buckets: Vec::new(),
                    convex_points: Some(points.to_vec()),
                    convex_winding: Some(if area2(points) > 0.0 { 1 } else { -1 }),
                };
            }
            let mut buckets = vec![Vec::new(); CONTAINMENT_BUCKETS];
            for edge in edges {
                let start_y = edge.start.y;
                let end_y = edge.start.y + edge.delta_y;
                let lower = containment_bucket(start_y.min(end_y), bounds.1, bounds.3);
                let upper = containment_bucket(start_y.max(end_y), bounds.1, bounds.3);
                for bucket in buckets.iter_mut().take(upper + 1).skip(lower) {
                    bucket.push(edge);
                }
            }
            ContainmentPath { bounds, buckets, convex_points: None, convex_winding: None }
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn containment_bucket(value: f64, min_y: f64, max_y: f64) -> usize {
    if max_y <= min_y {
        return 0;
    }
    let scaled = (value - min_y) / (max_y - min_y) * CONTAINMENT_BUCKETS as f64;
    scaled.floor().clamp(0.0, CONTAINMENT_BUCKETS as f64 - 1.0) as usize
}

fn stitch(edges: &[DirectedEdge]) -> Result<PathsD, Error> {
    if edges.is_empty() {
        return Ok(Vec::new());
    }
    let mut single_outgoing: FastMap<PointKey, usize> =
        FastMap::with_capacity_and_hasher(edges.len(), BuildHasherDefault::default());
    let mut has_branch = false;
    for (index, edge) in edges.iter().enumerate() {
        if single_outgoing.insert(edge.start_key, index).is_some() {
            has_branch = true;
        }
    }
    let mut next = vec![0; edges.len()];
    if has_branch {
        let mut outgoing: FastMap<PointKey, Vec<usize>> =
            FastMap::with_capacity_and_hasher(edges.len(), BuildHasherDefault::default());
        for (index, edge) in edges.iter().enumerate() {
            outgoing.entry(edge.start_key).or_default().push(index);
        }
        for indices in outgoing.values_mut() {
            if indices.len() < 2 {
                continue;
            }
            let origin_point = edges[*indices.first().ok_or(Error::TopologyFailure)?].start;
            indices.sort_by(|left, right| {
                compare_angle(
                    subtract(edges[*left].end, origin_point),
                    subtract(edges[*right].end, origin_point),
                )
            });
        }
        for (index, edge) in edges.iter().enumerate() {
            let candidates = outgoing.get(&edge.end_key).ok_or(Error::TopologyFailure)?;
            if candidates.len() == 1 {
                next[index] = candidates[0];
                continue;
            }
            let reverse = subtract(edge.start, edge.end);
            let insertion = candidates
                .iter()
                .position(|candidate| {
                    compare_angle(subtract(edges[*candidate].end, edges[*candidate].start), reverse)
                        != Ordering::Less
                })
                .unwrap_or(candidates.len());
            next[index] = candidates[(insertion + candidates.len() - 1) % candidates.len()];
        }
    } else {
        for (index, edge) in edges.iter().enumerate() {
            next[index] = *single_outgoing.get(&edge.end_key).ok_or(Error::TopologyFailure)?;
        }
    }
    let mut visited = vec![false; edges.len()];
    let mut paths = Vec::new();
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        let mut path = Vec::new();
        let mut current = start;
        loop {
            if visited[current] {
                if current != start {
                    return Err(Error::TopologyFailure);
                }
                break;
            }
            visited[current] = true;
            path.push(edges[current].start);
            current = next[current];
        }
        if path.len() >= 3 && area2(&path).abs() > f64::EPSILON {
            canonicalize(&mut path);
            paths.push(path.into_iter().map(|point| PointD::new(point.x, point.y)).collect());
        }
    }
    paths.sort_by(compare_paths);
    Ok(paths)
}

fn compare_angle(first: Point, second: Point) -> Ordering {
    let first_upper = first.y > 0.0 || (first.y.abs() <= f64::EPSILON && first.x >= 0.0);
    let second_upper = second.y > 0.0 || (second.y.abs() <= f64::EPSILON && second.x >= 0.0);
    if first_upper != second_upper {
        return second_upper.cmp(&first_upper);
    }
    let cross = cross(first, second);
    if cross.abs() > f64::EPSILON {
        return if cross > 0.0 { Ordering::Less } else { Ordering::Greater };
    }
    let first_length = first.x.mul_add(first.x, first.y * first.y);
    let second_length = second.x.mul_add(second.x, second.y * second.y);
    first_length.total_cmp(&second_length)
}

fn area2(path: &[Point]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(start, end)| start.x * end.y - start.y * end.x)
        .sum()
}

fn canonicalize(path: &mut [Point]) {
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

fn maximum_coordinate<P: PathSlice>(subjects: &[P], clips: &[P]) -> f64 {
    subjects
        .iter()
        .chain(clips)
        .flat_map(|path| path.points().iter())
        .flat_map(|point| [point.x.abs(), point.y.abs()])
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_precision_loss)]
    fn circle(center_x: f64, radius: f64, vertices: usize) -> PathD {
        (0..vertices)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / vertices as f64;
                PointD::new(center_x + radius * angle.cos(), radius * angle.sin())
            })
            .collect()
    }

    #[test]
    fn high_vertex_overlap_stays_on_fast_path() {
        let subject = circle(0.0, 100.0, 64);
        let clip = circle(30.0, 100.0, 64);
        let subjects = [subject];
        let clips = [clip];
        let subject_points = fast_paths(&subjects).expect("finite high-vertex path");
        let clip_points = fast_paths(&clips).expect("finite high-vertex clip");
        assert!(eligible(&subject_points, &clip_points));
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
        };
        let result = try_boolean_opd(request).expect("high-vertex input is eligible");
        assert!(result.is_ok(), "fast path failed: {result:?}");
    }

    fn point(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn edge(start: Point, end: Point) -> Edge {
        Edge {
            start,
            end,
            start_key: key(start).expect("test point key"),
            end_key: key(end).expect("test point key"),
            path_id: 0,
            subject: false,
            min_x: start.x.min(end.x),
            min_y: start.y.min(end.y),
            max_x: start.x.max(end.x),
            max_y: start.y.max(end.y),
            min_x_key: total_order_key(start.x.min(end.x)),
            max_x_key: total_order_key(start.x.max(end.x)),
        }
    }

    fn directed(start: Point, end: Point) -> DirectedEdge {
        DirectedEdge {
            start,
            end,
            start_key: key(start).expect("test point key"),
            end_key: key(end).expect("test point key"),
        }
    }

    #[test]
    #[allow(clippy::cloned_ref_to_slice_refs, clippy::float_cmp, clippy::too_many_lines)]
    fn exercises_fast_path_predicates_and_shortcuts() {
        let rectangle =
            vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0), point(0.0, 10.0)];
        let clockwise =
            vec![point(0.0, 0.0), point(0.0, 10.0), point(10.0, 10.0), point(10.0, 0.0)];
        assert!(fast_paths(&[]).unwrap().is_empty());
        assert!(fast_paths(&[Vec::new()]).unwrap().is_empty());
        assert!(
            fast_paths(&[vec![
                PointD::new(f64::NAN, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(0.0, 1.0),
            ]])
            .is_none()
        );
        assert!(
            fast_paths(&[vec![PointD::new(f64::INFINITY, 0.0), PointD::new(1.0, 0.0)]]).is_some()
        );
        let borrowed = fast_paths(std::slice::from_ref(&rectangle)).expect("borrowed path");
        assert!(matches!(borrowed.first(), Some(FastPath::Borrowed(_))));
        let duplicated = vec![point(0.0, 0.0), point(1.0, 0.0), point(1.0, 0.0), point(0.0, 1.0)];
        let owned = fast_paths(std::slice::from_ref(&duplicated)).expect("normalized path");
        assert!(matches!(owned.first(), Some(FastPath::Owned(_))));
        let no_paths: Vec<Vec<Point>> = Vec::new();
        assert!(eligible(&no_paths, &[]));
        assert!(eligible(std::slice::from_ref(&rectangle), &[]));
        assert!(!eligible(
            [vec![point(2_000_000.0, 0.0), point(2_000_001.0, 0.0), point(2_000_000.0, 1.0)]]
                .as_slice(),
            &[],
        ));
        assert!(!eligible(
            [vec![point(0.0, 2_000_000.0), point(1.0, 2_000_000.0), point(0.0, 2_000_001.0)]]
                .as_slice(),
            &[],
        ));
        assert!(
            !eligible(&[vec![point(0.0, 0.0), point(2_000_000.0, 0.0), point(0.0, 1.0)]], &[],)
        );
        assert!(!eligible(&[vec![point(f64::NAN, 0.0), point(1.0, 0.0), point(0.0, 1.0)]], &[],));
        assert!(eligible(&[vec![point(0.0, 0.0), point(0.0, 1.0)]], &[]));
        assert!(keyable_path(&rectangle));
        assert!(!keyable_path(&[point(0.0, 0.0), point(0.0, 0.0), point(1.0, 0.0),]));
        assert!(!keyable_path(&[point(0.0, 0.0), point(1.0, 0.0), point(0.0, 0.0),]));
        assert!(!keyable_path(&[point(f64::NAN, 0.0), point(1.0, 0.0), point(0.0, 1.0),]));
        assert!(total_order_key(-1.0) < total_order_key(0.0));
        assert!(total_order_key(-0.0) < total_order_key(0.0));
        let mut bytes_hasher = FastHasher::default();
        bytes_hasher.write(&[1, 2, 3]);
        let mut integer_hasher = FastHasher::default();
        integer_hasher.write_i64(1);
        integer_hasher.write_i64(2);
        integer_hasher.write_i64(3);
        assert_eq!(bytes_hasher.finish(), integer_hasher.finish());
        let empty_paths: Vec<Vec<Point>> = Vec::new();
        assert!(bbox(&empty_paths).is_none());
        assert_eq!(bbox(std::slice::from_ref(&rectangle)), Some((0.0, 0.0, 10.0, 10.0)));
        assert!(path_edges(&[]).is_empty());
        assert_eq!(path_edges(&[point(0.0, 0.0), point(0.0, 0.0)]).len(), 0);
        assert_eq!(path_edges(&rectangle).len(), 4);
        assert!(key(point(f64::NAN, 0.0)).is_none());
        assert!(key(point(0.0, f64::NAN)).is_none());
        assert!(key(point(f64::MAX, 0.0)).is_none());
        assert!(key(point(0.0, f64::MAX)).is_none());
        assert!(key(point(10_000_000_000.0, 0.0)).is_none());
        assert!(key(point(0.0, 10_000_000_000.0)).is_none());
        assert!(boxes_disjoint(
            &edge(point(0.0, 0.0), point(1.0, 1.0)),
            &edge(point(2.0, 0.0), point(3.0, 1.0))
        ));
        assert!(!boxes_disjoint(
            &edge(point(0.0, 0.0), point(1.0, 1.0)),
            &edge(point(1.0, 0.0), point(2.0, 1.0))
        ));

        let crossing_first = edge(point(0.0, 0.0), point(10.0, 10.0));
        let crossing_second = edge(point(0.0, 10.0), point(10.0, 0.0));
        let mut first_values = vec![0.0, 1.0];
        let mut second_values = vec![0.0, 1.0];
        assert!(split_pair(
            &crossing_first,
            &crossing_second,
            &mut first_values,
            &mut second_values
        ));
        assert!(first_values.iter().any(|value| (*value - 0.5).abs() < f64::EPSILON));
        assert!(second_values.iter().any(|value| (*value - 0.5).abs() < f64::EPSILON));
        assert!(split_pair(
            &edge(point(0.0, 0.0), point(1.0, 0.0)),
            &edge(point(0.0, 1.0), point(1.0, 1.0)),
            &mut Vec::new(),
            &mut Vec::new()
        ));
        let mut overlap_first = Vec::new();
        let mut overlap_second = Vec::new();
        assert!(split_pair(
            &edge(point(0.0, 0.0), point(10.0, 0.0)),
            &edge(point(5.0, 0.0), point(15.0, 0.0)),
            &mut overlap_first,
            &mut overlap_second
        ));
        assert_eq!(overlap_first.len(), 1);
        assert_eq!(overlap_second.len(), 1);
        let mut ill_conditioned_first = Vec::new();
        let mut ill_conditioned_second = Vec::new();
        assert!(!split_pair(
            &edge(point(0.0, 0.0), point(1_000_000.0, 1_000_000.0)),
            &edge(point(0.0, 0.0), point(1_000_000.0, 1_000_000.000_000_001)),
            &mut ill_conditioned_first,
            &mut ill_conditioned_second
        ));
        assert!(in_unit_interval(-f64::EPSILON));
        assert!(in_unit_interval(1.0 + f64::EPSILON));
        assert!(!in_unit_interval(2.0));
        assert!(
            (parameter(point(5.0, 1.0), &edge(point(0.0, 1.0), point(10.0, 1.0))) - 0.5).abs()
                < f64::EPSILON
        );
        assert!(
            (parameter(point(1.0, 5.0), &edge(point(1.0, 0.0), point(1.0, 10.0))) - 0.5).abs()
                < f64::EPSILON
        );
        assert_eq!(point_at(edge(point(0.0, 0.0), point(10.0, 10.0)), 0.5).x, 5.0);
        assert_eq!(subtract(point(3.0, 4.0), point(1.0, 2.0)).x, 2.0);
        assert_eq!(cross(point(1.0, 0.0), point(0.0, 1.0)), 1.0);
        assert_ne!(key_cross(&crossing_first, &crossing_second), 0);
        assert!(on_segment(point(5.0, 0.0), point(0.0, 0.0), point(10.0, 0.0)));
        assert!(!on_segment(point(-1.0, 0.0), point(0.0, 0.0), point(10.0, 0.0)));
        assert!(!on_segment(point(11.0, 0.0), point(0.0, 0.0), point(10.0, 0.0)));
        assert!(!on_segment(point(5.0, 1.0), point(0.0, 0.0), point(10.0, 0.0)));
        assert!(!on_segment(point(5.0, -1.0), point(0.0, 0.0), point(10.0, 0.0)));
        assert!(!on_segment(point(5.0, 11.0), point(0.0, 0.0), point(10.0, 0.0)));
        assert!(!on_segment(point(0.0, -1.0), point(0.0, 0.0), point(0.0, 10.0)));
        assert!(!on_segment(point(0.0, 11.0), point(0.0, 0.0), point(0.0, 10.0)));

        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            let _ = apply_operation(true, false, clip_type);
        }
        assert!(
            short_circuit(&empty_paths, &empty_paths, ClipType::Union, FillRule::EvenOdd).is_some()
        );
        assert!(
            short_circuit(&[rectangle.clone()], &[], ClipType::Intersection, FillRule::EvenOdd)
                .is_some()
        );
        assert!(
            short_circuit(&[], &[rectangle.clone()], ClipType::Difference, FillRule::EvenOdd)
                .is_some()
        );
        assert!(
            short_circuit(&[rectangle.clone()], &[], ClipType::Union, FillRule::Positive).is_none()
        );
        assert!(
            short_circuit(&[], &[rectangle.clone()], ClipType::Union, FillRule::EvenOdd).is_some()
        );
        assert!(
            short_circuit(&[], &[rectangle.clone()], ClipType::Xor, FillRule::EvenOdd).is_some()
        );
        assert_eq!(
            run(&[Vec::new()], &[Vec::new()], ClipType::Union, FillRule::EvenOdd).unwrap(),
            Vec::<PathD>::new()
        );
        assert_eq!(
            run(
                &[vec![point(0.0, 0.0), point(0.0, 0.0)]],
                &[vec![point(0.0, 0.0), point(0.0, 0.0)]],
                ClipType::Union,
                FillRule::EvenOdd,
            )
            .unwrap(),
            Vec::<PathD>::new()
        );
        let disjoint =
            vec![point(20.0, 0.0), point(30.0, 0.0), point(30.0, 10.0), point(20.0, 10.0)];
        let left_disjoint =
            vec![point(-30.0, 0.0), point(-20.0, 0.0), point(-20.0, 10.0), point(-30.0, 10.0)];
        let above_disjoint =
            vec![point(0.0, 20.0), point(10.0, 20.0), point(10.0, 30.0), point(0.0, 30.0)];
        let below_disjoint =
            vec![point(0.0, -30.0), point(10.0, -30.0), point(10.0, -20.0), point(0.0, -20.0)];
        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            for other in [&disjoint, &left_disjoint, &above_disjoint, &below_disjoint] {
                assert!(
                    run(
                        &[rectangle.clone()],
                        std::slice::from_ref(other),
                        clip_type,
                        FillRule::EvenOdd
                    )
                    .is_ok()
                );
            }
        }
        let self_crossing =
            vec![point(0.0, 0.0), point(10.0, 10.0), point(0.0, 10.0), point(10.0, 0.0)];
        let far_self_crossing =
            vec![point(20.0, 20.0), point(30.0, 30.0), point(20.0, 30.0), point(30.0, 20.0)];
        assert!(
            short_circuit(
                &[self_crossing.clone()],
                &[disjoint.clone()],
                ClipType::Union,
                FillRule::EvenOdd
            )
            .is_none()
        );
        assert!(
            short_circuit(
                &[rectangle.clone()],
                &[far_self_crossing],
                ClipType::Union,
                FillRule::EvenOdd
            )
            .is_none()
        );
        assert!(
            run(
                &[vec![
                    point(0.0, 0.0),
                    point(1_000_000.0, 1_000_000.0),
                    point(1_000_000.0, 999_999.0),
                ]],
                &[vec![
                    point(0.0, 0.0),
                    point(1_000_000.0, 1_000_000.000_000_001),
                    point(1_000_000.0, 999_998.0),
                ]],
                ClipType::Intersection,
                FillRule::EvenOdd,
            )
            .is_err()
        );
        assert!(
            run(
                &[rectangle.clone()],
                &[rectangle.clone()],
                ClipType::Intersection,
                FillRule::EvenOdd
            )
            .is_ok()
        );
        let _ = run(
            &[vec![point(0.0, 0.0), point(1.0, 0.0)]],
            &[
                vec![point(0.5, -1.0), point(0.5, 1.0)],
                vec![point(0.500_000_000_002, -1.0), point(0.500_000_000_002, 1.0)],
            ],
            ClipType::Union,
            FillRule::EvenOdd,
        );
        assert!(!paths_are_simple_and_disjoint(std::slice::from_ref(&self_crossing)));
        assert!(paths_are_simple_and_disjoint(&empty_paths));
        assert!(paths_are_simple_and_disjoint(&[Vec::new()]));
        assert!(paths_are_simple_and_disjoint(&[rectangle.clone(), disjoint.clone()]));
        assert!(paths_are_simple_and_disjoint(&[rectangle.clone(), left_disjoint.clone()]));
        assert!(paths_are_simple_and_disjoint(&[rectangle.clone(), above_disjoint.clone()]));
        assert!(paths_are_simple_and_disjoint(&[rectangle.clone(), below_disjoint.clone()]));
        assert!(paths_are_simple_and_disjoint(&[rectangle.clone(), Vec::new()]));
        assert!(!paths_are_simple_and_disjoint(&[rectangle.clone(), rectangle.clone()]));
        assert!(edges_intersect(&crossing_first, &crossing_second));
        assert!(edges_intersect(
            &edge(point(0.0, 0.0), point(10.0, 0.0)),
            &edge(point(5.0, 0.0), point(15.0, 0.0))
        ));
        assert!(!edges_intersect(
            &edge(point(0.0, 0.0), point(1.0, 0.0)),
            &edge(point(2.0, 0.0), point(3.0, 0.0))
        ));

        let reversed = direct_paths(&[clockwise.clone()]);
        assert!(reversed[0][1].x >= reversed[0][0].x);
        assert!(direct_if_simple_and_disjoint(&[rectangle.clone()]).is_some());
        assert!(direct_if_simple_and_disjoint(&[self_crossing.clone()]).is_none());
        assert!(simple_local_sides_with_hint(&empty_paths, FillRule::EvenOdd, None).is_none());
        assert!(
            simple_local_sides_with_hint(&[rectangle.clone()], FillRule::EvenOdd, None).is_some()
        );
        assert!(simple_local_sides_with_hint(&[clockwise], FillRule::Positive, None).is_some());
        let concave = vec![
            point(0.0, 0.0),
            point(4.0, 0.0),
            point(4.0, 4.0),
            point(3.0, 4.0),
            point(3.0, 1.0),
            point(1.0, 1.0),
            point(1.0, 4.0),
            point(0.0, 4.0),
        ];
        assert!(paths_are_simple_and_disjoint(&[concave.clone()]));
        assert!(
            simple_local_sides_with_hint(&[concave.clone()], FillRule::Negative, None).is_some()
        );
        assert!(simple_local_sides_with_hint(&[self_crossing], FillRule::EvenOdd, None).is_none());
        assert!(is_convex_simple(&rectangle));
        assert!(!is_convex_simple(&[]));
        assert!(!is_convex_simple(&[point(0.0, 0.0), point(0.0, 0.0), point(1.0, 0.0)]));
        assert!(!is_convex_simple(&[point(f64::NAN, 0.0), point(1.0, 0.0), point(0.0, 1.0),]));
        assert!(is_convex_simple(&[
            point(0.0, 0.0),
            point(1.0, 0.0),
            point(2.0, 0.0),
            point(2.0, 1.0),
            point(0.0, 1.0)
        ]));
        assert!(!is_convex_simple(&concave));

        let containment = containment_paths(&[rectangle.clone()]);
        let convex_containment = containment_paths_with_properties(
            &[rectangle.clone()],
            &[PathProperties { simple: true, convex: true }],
        );
        assert_eq!(
            paths_contain_pair(
                point(-1.0, -1.0),
                point(-2.0, -2.0),
                &containment,
                FillRule::EvenOdd
            ),
            (false, false)
        );
        for fill_rule in
            [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        {
            assert_eq!(
                paths_contain_pair(point(5.0, 5.0), point(-2.0, -2.0), &containment, fill_rule),
                paths_contain_pair(
                    point(5.0, 5.0),
                    point(-2.0, -2.0),
                    &convex_containment,
                    fill_rule,
                )
            );
        }
        let clockwise_containment = containment_paths_with_properties(
            &[vec![point(0.0, 0.0), point(0.0, 10.0), point(10.0, 10.0), point(10.0, 0.0)]],
            &[PathProperties { simple: true, convex: true }],
        );
        assert_eq!(
            paths_contain_pair(
                point(1.0, 1.0),
                point(-1.0, -1.0),
                &clockwise_containment,
                FillRule::Negative,
            ),
            (true, false)
        );
        assert_eq!(
            paths_contain_pair(point(5.0, 5.0), point(-2.0, -2.0), &containment, FillRule::EvenOdd),
            (true, false)
        );
        assert_eq!(
            paths_contain_pair(point(-2.0, -2.0), point(5.0, 5.0), &containment, FillRule::EvenOdd),
            (false, true)
        );
        assert_eq!(
            paths_contain_pair(point(1.0, 1.0), point(9.0, 9.0), &containment, FillRule::NonZero),
            (true, true)
        );
        assert_eq!(
            paths_contain_pair(point(1.0, 1.0), point(9.0, 9.0), &containment, FillRule::Positive),
            (true, true)
        );
        assert_eq!(
            paths_contain_pair(point(1.0, 1.0), point(9.0, 9.0), &containment, FillRule::Negative),
            (false, false)
        );
        assert_eq!(containment_bucket(0.0, 1.0, 1.0), 0);
        assert_eq!(containment_bucket(-100.0, 0.0, 10.0), 0);
        assert_eq!(containment_bucket(100.0, 0.0, 10.0), CONTAINMENT_BUCKETS - 1);
        assert!(containment_bucket_if_inside(point(-1.0, 5.0), 0.0, 0.0, 10.0, 10.0).is_none());
        assert!(containment_bucket_if_inside(point(5.0, 5.0), 0.0, 0.0, 10.0, 10.0).is_some());
        let mut state = WindingState::default();
        let upward = ContainmentEdge {
            start: point(0.0, 0.0),
            delta_x: 0.0,
            delta_y: 10.0,
            intercept: 0.0,
            upward: true,
        };
        let downward = ContainmentEdge {
            start: point(0.0, 10.0),
            delta_x: 0.0,
            delta_y: -10.0,
            intercept: 100.0,
            upward: false,
        };
        update_winding(&mut state, point(-1.0, 5.0), &upward);
        update_winding(&mut state, point(1.0, 5.0), &downward);
        assert!(state.parity);

        assert!(stitch(&[]).unwrap().is_empty());
        let square_edges = [
            directed(point(0.0, 0.0), point(10.0, 0.0)),
            directed(point(10.0, 0.0), point(10.0, 10.0)),
            directed(point(10.0, 10.0), point(0.0, 10.0)),
            directed(point(0.0, 10.0), point(0.0, 0.0)),
        ];
        assert_eq!(stitch(&square_edges).unwrap().len(), 1);
        assert!(stitch(&[directed(point(0.0, 0.0), point(1.0, 0.0))]).is_err());
        let branched = [
            directed(point(0.0, 0.0), point(1.0, 0.0)),
            directed(point(0.0, 0.0), point(0.0, 1.0)),
            directed(point(1.0, 0.0), point(0.0, 0.0)),
            directed(point(0.0, 1.0), point(0.0, 0.0)),
        ];
        assert!(stitch(&branched).is_ok());
        let non_start_cycle = [
            directed(point(0.0, 0.0), point(1.0, 0.0)),
            directed(point(1.0, 0.0), point(2.0, 0.0)),
            directed(point(2.0, 0.0), point(1.0, 0.0)),
        ];
        assert!(stitch(&non_start_cycle).is_err());
        let short_cycle = [
            directed(point(0.0, 0.0), point(1.0, 0.0)),
            directed(point(1.0, 0.0), point(0.0, 0.0)),
        ];
        assert!(stitch(&short_cycle).unwrap().is_empty());
        let collinear_cycle = [
            directed(point(0.0, 0.0), point(1.0, 0.0)),
            directed(point(1.0, 0.0), point(2.0, 0.0)),
            directed(point(2.0, 0.0), point(0.0, 0.0)),
        ];
        assert!(stitch(&collinear_cycle).unwrap().is_empty());
        assert_eq!(compare_angle(point(1.0, 0.0), point(0.0, 1.0)), Ordering::Less);
        assert_eq!(compare_angle(point(0.0, -1.0), point(1.0, 0.0)), Ordering::Greater);
        assert_eq!(compare_angle(point(1.0, 0.0), point(2.0, 0.0)), Ordering::Less);
        assert_eq!(area2(&[]), 0.0);
        let mut canonical = vec![point(2.0, 2.0), point(0.0, 0.0), point(1.0, 1.0)];
        canonicalize(&mut canonical);
        assert_eq!(canonical[0].x, 0.0);
        let mut empty_canonical = Vec::new();
        canonicalize(&mut empty_canonical);
        let path_a: PathD = vec![PointD::new(0.0, 0.0)];
        let path_b: PathD = vec![PointD::new(1.0, 0.0)];
        assert_eq!(compare_paths(&path_a, &path_b), Ordering::Less);
        assert_eq!(compare_paths(&path_a, &path_a), Ordering::Equal);
        assert_eq!(maximum_coordinate(&[rectangle], &[]), 10.0);
    }
}
