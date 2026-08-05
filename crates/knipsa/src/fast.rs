//! Conservative floating-point fast path for ordinary, well-conditioned input.
//!
//! The exact arrangement remains the fallback. This path is deliberately
//! bounded to coordinates that can be keyed without loss at the output
//! resolution and to segment configurations whose predicates are comfortably
//! away from zero.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::hash::{BuildHasherDefault, Hasher};

use crate::{BooleanRequestD, ClipType, Error, FillRule, PathD, PathsD, PointD, normalize_pathd};

const KEY_SCALE: f64 = 1_000_000_000.0;
const MAX_COORDINATE: f64 = 1_000_000.0;
const PREDICATE_TOLERANCE: f64 = 1.0e-12;
const SAMPLE_SCALE: f64 = 1.0e-9;
const MAX_CONTAINMENT_BUCKETS: usize = 64;

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

enum Outgoing {
    Single(usize),
    Multiple(Vec<usize>),
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
struct ContainmentPath<'a> {
    bounds: (f64, f64, f64, f64),
    buckets: Vec<Vec<ContainmentEdge>>,
    convex_points: Option<&'a [Point]>,
    convex_winding: Option<i32>,
}

const INLINE_SPLIT_CAPACITY: usize = 4;

#[derive(Clone, Debug)]
struct SplitParameters {
    inline: [f64; INLINE_SPLIT_CAPACITY],
    len: usize,
    overflow: Option<Vec<f64>>,
}

impl SplitParameters {
    fn new() -> Self {
        Self { inline: [0.0, 1.0, 0.0, 0.0], len: 2, overflow: None }
    }

    fn len(&self) -> usize {
        self.overflow.as_ref().map_or(self.len, Vec::len)
    }

    fn push(&mut self, value: f64) {
        if let Some(overflow) = &mut self.overflow {
            overflow.push(value);
        } else if self.len < INLINE_SPLIT_CAPACITY {
            self.inline[self.len] = value;
            self.len += 1;
        } else {
            let mut overflow = Vec::with_capacity(INLINE_SPLIT_CAPACITY * 2);
            overflow.extend_from_slice(&self.inline);
            overflow.push(value);
            self.overflow = Some(overflow);
            self.len += 1;
        }
    }

    fn sort_dedup(&mut self) {
        if let Some(overflow) = &mut self.overflow {
            overflow.sort_unstable_by(f64::total_cmp);
            overflow.dedup_by(|left, right| (*left - *right).abs() <= 1.0e-12);
            self.len = overflow.len();
        } else {
            let old_len = self.len;
            let values = &mut self.inline[..old_len];
            values.sort_unstable_by(f64::total_cmp);
            let mut write = 1;
            for read in 1..old_len {
                if (values[read] - values[write - 1]).abs() > 1.0e-12 {
                    values[write] = values[read];
                    write += 1;
                }
            }
            self.len = write;
        }
    }

    fn values(&self) -> &[f64] {
        match self.overflow.as_deref() {
            Some(values) => values,
            None => &self.inline[..self.len],
        }
    }
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

struct ConvexIndex<'a> {
    points: &'a [Point],
    positive: bool,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

struct ConvexWalk {
    output: PathsD,
    subject_splits: Vec<SplitParameters>,
    clip_splits: Vec<SplitParameters>,
    subject_inside: Vec<Option<bool>>,
    clip_inside: Vec<Option<bool>>,
    degenerate: bool,
}

impl ConvexIndex<'_> {
    #[inline]
    fn new(points: &[Point]) -> ConvexIndex<'_> {
        let (min_x, min_y, max_x, max_y) = point_bounds(points);
        ConvexIndex { points, positive: area2(points) > 0.0, min_x, min_y, max_x, max_y }
    }

    #[inline]
    fn contains(&self, point: Point) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
            && convex_contains(point, self.points, self.positive)
    }
}

pub(crate) fn try_boolean_opd(request: BooleanRequestD<'_>) -> Option<Result<PathsD, ()>> {
    if let Some(result) = try_single_convex_boolean(request) {
        return Some(result.map_err(|_| ()));
    }
    let subjects = fast_paths(request.subjects)?;
    let clips = fast_paths(request.clips)?;
    if !eligible(&subjects, &clips) {
        return None;
    }
    Some(run(&subjects, &clips, request.clip_type, request.fill_rule).map_err(|_| ()))
}

fn try_single_convex_boolean(request: BooleanRequestD<'_>) -> Option<Result<PathsD, Error>> {
    if request.subjects.len() != 1
        || request.clips.len() != 1
        || !eligible(request.subjects, request.clips)
    {
        return None;
    }
    let subject = request.subjects[0].as_slice();
    let clip = request.clips[0].as_slice();
    if subject.len() >= 3 && clip.len() >= 3 && strict_convex(subject) && strict_convex(clip) {
        if !keyable_path(subject)
            || !keyable_path(clip)
            || !fill_rule_accepts_ring(subject, request.fill_rule)
            || !fill_rule_accepts_ring(clip, request.fill_rule)
        {
            return None;
        }
        return convex_boolean(subject, clip, request.clip_type, Some((true, true))).map(Ok);
    }
    let subject_properties = classify_path(subject);
    let clip_properties = classify_path(clip);
    if subject.len() < 3
        || clip.len() < 3
        || !subject_properties.convex
        || !clip_properties.convex
        || !fill_rule_accepts_ring(subject, request.fill_rule)
        || !fill_rule_accepts_ring(clip, request.fill_rule)
    {
        return None;
    }
    convex_boolean(subject, clip, request.clip_type, None).map(Ok)
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
    let edge_capacity = subjects.iter().chain(clips).map(|path| path.points().len()).sum();
    let mut edges = Vec::with_capacity(edge_capacity);
    let mut path_properties = Vec::with_capacity(subjects.len() + clips.len());
    for (path_id, path) in subjects.iter().enumerate() {
        let points = path.points();
        path_properties.push(classify_path(points));
        append_path_edges(&mut edges, points, path_id, true);
    }
    for (clip_index, path) in clips.iter().enumerate() {
        let path_id = subjects.len() + clip_index;
        let points = path.points();
        path_properties.push(classify_path(points));
        if path_properties[path_id].convex
            && subjects.len() == 1
            && clips.len() == 1
            && path_properties[0].convex
            && fill_rule_accepts_ring(subjects[0].points(), fill_rule)
            && fill_rule_accepts_ring(points, fill_rule)
        {
            if let Some(result) = convex_boolean(subjects[0].points(), points, clip_type, None) {
                return Ok(result);
            }
        }
        append_path_edges(&mut edges, points, path_id, false);
    }
    if edges.is_empty() {
        return Ok(Vec::new());
    }

    let mut parameters = vec![SplitParameters::new(); edges.len()];
    if !split_pairs(&edges, &path_properties, &mut parameters, false) {
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
            values.sort_dedup();
        }
        for pair in values.values().windows(2) {
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
    parameters: &mut [SplitParameters],
    reject_collinear: bool,
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
            if !split_pair(
                &edges[first],
                &edges[second],
                &mut before[first],
                &mut after[0],
                reject_collinear,
            ) {
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

fn fill_rule_accepts_ring(path: &[Point], fill_rule: FillRule) -> bool {
    match fill_rule {
        FillRule::EvenOdd | FillRule::NonZero => true,
        FillRule::Positive => area2(path) > 0.0,
        FillRule::Negative => area2(path) < 0.0,
    }
}

fn convex_boolean(
    subject: &[Point],
    clip: &[Point],
    clip_type: ClipType,
    strict_hint: Option<(bool, bool)>,
) -> Option<PathsD> {
    let (subject_strict, clip_strict) =
        strict_hint.unwrap_or_else(|| (strict_convex(subject), strict_convex(clip)));
    let linear_walk = if area2(subject) > 0.0 && area2(clip) > 0.0 && subject_strict && clip_strict
    {
        Some(convex_boundary_walk(
            subject,
            clip,
            clip_type == ClipType::Intersection,
            clip_type != ClipType::Intersection,
        ))
    } else {
        None
    };
    if clip_type == ClipType::Intersection
        && let Some(walk) = linear_walk.as_ref()
        && (!walk.degenerate || valid_convex_intersection(&walk.output, subject, clip))
    {
        return Some(walk.output.clone());
    }
    let use_linear_splits = linear_walk.as_ref().is_some_and(|walk| !walk.degenerate);
    if use_linear_splits {
        let ConvexWalk { subject_splits, clip_splits, subject_inside, clip_inside, .. } =
            linear_walk.expect("linear walk was checked above");
        let use_inside_hints =
            subject_splits.iter().chain(&clip_splits).any(|values| values.len() > 2);
        return convex_boolean_from_splits(
            subject,
            clip,
            clip_type,
            subject_splits,
            clip_splits,
            if use_inside_hints { &subject_inside } else { &[] },
            if use_inside_hints { &clip_inside } else { &[] },
        );
    }
    let mut edges = Vec::with_capacity(subject.len() + clip.len());
    append_path_edges(&mut edges, subject, 0, true);
    append_path_edges(&mut edges, clip, 1, false);
    let properties = [PathProperties { simple: true, convex: true }; 2];
    let mut parameters = vec![SplitParameters::new(); edges.len()];
    if !split_pairs(&edges, &properties, &mut parameters, true) {
        return None;
    }

    let subject_index = ConvexIndex::new(subject);
    let clip_index = ConvexIndex::new(clip);
    let mut directed = Vec::with_capacity(edges.len());
    for (edge, values) in edges.iter().zip(parameters.iter_mut()) {
        if values.len() > 2 {
            values.sort_dedup();
        }
        for pair in values.values().windows(2) {
            let start = point_at(*edge, pair[0]);
            let end = point_at(*edge, pair[1]);
            let start_key = key(start)?;
            let end_key = key(end)?;
            if start_key == end_key {
                continue;
            }
            let midpoint = Point { x: (start.x + end.x) * 0.5, y: (start.y + end.y) * 0.5 };
            let other_inside = if edge.subject {
                clip_index.contains(midpoint)
            } else {
                subject_index.contains(midpoint)
            };
            let (left, right) = if edge.subject {
                (
                    apply_operation(subject_index.positive, other_inside, clip_type),
                    apply_operation(!subject_index.positive, other_inside, clip_type),
                )
            } else {
                (
                    apply_operation(other_inside, clip_index.positive, clip_type),
                    apply_operation(other_inside, !clip_index.positive, clip_type),
                )
            };
            if left == right {
                continue;
            }
            let directed_edge = if left {
                DirectedEdge { start, end, start_key, end_key }
            } else {
                DirectedEdge { start: end, end: start, start_key: end_key, end_key: start_key }
            };
            directed.push(directed_edge);
        }
    }
    stitch(&directed).ok()
}

fn convex_boolean_from_splits(
    subject: &[Point],
    clip: &[Point],
    clip_type: ClipType,
    mut subject_splits: Vec<SplitParameters>,
    mut clip_splits: Vec<SplitParameters>,
    subject_inside: &[Option<bool>],
    clip_inside: &[Option<bool>],
) -> Option<PathsD> {
    let subject_index = ConvexIndex::new(subject);
    let clip_index = ConvexIndex::new(clip);
    let mut directed = Vec::with_capacity(subject.len() + clip.len());
    let subject_count = append_convex_operation_edges(
        subject,
        &mut subject_splits,
        subject_inside,
        true,
        clip_type,
        &subject_index,
        &clip_index,
        &mut directed,
    )?;
    let clip_start = directed.len();
    let clip_count = append_convex_operation_edges(
        clip,
        &mut clip_splits,
        clip_inside,
        false,
        clip_type,
        &subject_index,
        &clip_index,
        &mut directed,
    )?;
    if clip_type == ClipType::Xor {
        return match stitch_ordered_convex(
            &directed,
            (0, subject_count),
            (clip_start, clip_start + clip_count),
        ) {
            Ok(result) => Some(result),
            Err(_) => stitch(&directed).ok(),
        };
    }
    stitch(&directed).ok()
}

#[allow(clippy::too_many_arguments)]
fn append_convex_operation_edges(
    points: &[Point],
    parameters: &mut [SplitParameters],
    inside_hints: &[Option<bool>],
    subject: bool,
    clip_type: ClipType,
    subject_index: &ConvexIndex<'_>,
    clip_index: &ConvexIndex<'_>,
    directed: &mut Vec<DirectedEdge>,
) -> Option<usize> {
    let start_len = directed.len();
    for (index, values) in parameters.iter_mut().enumerate() {
        if values.len() > 2 {
            values.sort_dedup();
        }
        let has_split = values.len() > 2;
        let start_point = points[index];
        let end_point = points[(index + 1) % points.len()];
        let vector = subtract(end_point, start_point);
        for pair in values.values().windows(2) {
            let start = Point {
                x: start_point.x + vector.x * pair[0],
                y: start_point.y + vector.y * pair[0],
            };
            let end = Point {
                x: start_point.x + vector.x * pair[1],
                y: start_point.y + vector.y * pair[1],
            };
            let start_key = key(start)?;
            let end_key = key(end)?;
            if start_key == end_key {
                continue;
            }
            let midpoint = Point { x: (start.x + end.x) * 0.5, y: (start.y + end.y) * 0.5 };
            let other_inside = if !has_split {
                inside_hints.get(index).copied().flatten().unwrap_or_else(|| {
                    if subject {
                        clip_index.contains(midpoint)
                    } else {
                        subject_index.contains(midpoint)
                    }
                })
            } else if subject {
                clip_index.contains(midpoint)
            } else {
                subject_index.contains(midpoint)
            };
            let (left, right) = if subject {
                (
                    apply_operation(subject_index.positive, other_inside, clip_type),
                    apply_operation(!subject_index.positive, other_inside, clip_type),
                )
            } else {
                (
                    apply_operation(other_inside, clip_index.positive, clip_type),
                    apply_operation(other_inside, !clip_index.positive, clip_type),
                )
            };
            if left == right {
                continue;
            }
            if left {
                directed.push(DirectedEdge { start, end, start_key, end_key });
            } else {
                directed.push(DirectedEdge {
                    start: end,
                    end: start,
                    start_key: end_key,
                    end_key: start_key,
                });
            }
        }
    }
    Some(directed.len() - start_len)
}

fn valid_convex_intersection(result: &PathsD, subject: &[Point], clip: &[Point]) -> bool {
    let Some(path) = result.first() else {
        let subject_bounds = point_bounds(subject);
        let clip_bounds = point_bounds(clip);
        return subject_bounds.2 < clip_bounds.0
            || clip_bounds.2 < subject_bounds.0
            || subject_bounds.3 < clip_bounds.1
            || clip_bounds.3 < subject_bounds.1;
    };
    if result.len() != 1 || path.len() < 3 {
        return false;
    }
    path.iter()
        .copied()
        .all(|point| convex_contains(point, subject, true) && convex_contains(point, clip, true))
        && area2(path) > 0.0
}

fn strict_convex(path: &[Point]) -> bool {
    if path.len() < 3 {
        return false;
    }
    let mut direction = None;
    for index in 0..path.len() {
        let previous = path[(index + path.len() - 1) % path.len()];
        let current = path[index];
        let next = path[(index + 1) % path.len()];
        let turn = cross(subtract(current, previous), subtract(next, current));
        if turn.abs() <= f64::EPSILON {
            return false;
        }
        let positive = turn > 0.0;
        if direction.is_some_and(|known| known != positive) {
            return false;
        }
        direction = Some(positive);
    }
    true
}

#[allow(clippy::too_many_lines)]
fn convex_boundary_walk(
    subject: &[Point],
    clip: &[Point],
    collect_output: bool,
    collect_splits: bool,
) -> ConvexWalk {
    let mut subject_index = 0;
    let mut clip_index = 0;
    let mut subject_previous = subject[subject.len() - 1];
    let mut clip_previous = clip[clip.len() - 1];
    let mut subject_vector = subtract(subject[subject_index], subject_previous);
    let mut clip_vector = subtract(clip[clip_index], clip_previous);
    let initial_subject_midpoint = Point {
        x: (subject_previous.x + subject[subject_index].x) * 0.5,
        y: (subject_previous.y + subject[subject_index].y) * 0.5,
    };
    // Split-only walks need the initial side of the subject boundary so their
    // per-edge hints remain valid before the first crossing is encountered.
    let mut inside =
        u8::from(!collect_output && convex_contains(initial_subject_midpoint, clip, true));
    let mut output =
        if collect_output { Vec::with_capacity(subject.len() + clip.len()) } else { Vec::new() };
    let mut subject_splits =
        if collect_splits { vec![SplitParameters::new(); subject.len()] } else { Vec::new() };
    let mut clip_splits =
        if collect_splits { vec![SplitParameters::new(); clip.len()] } else { Vec::new() };
    let mut subject_inside = if collect_splits { vec![None; subject.len()] } else { Vec::new() };
    let mut clip_inside = if collect_splits { vec![None; clip.len()] } else { Vec::new() };
    let mut first_intersection = None;
    let limit = 2 * (subject.len() + clip.len()) + 1;
    let mut degenerate = false;
    let mut subject_steps = 0;
    let mut clip_steps = 0;

    for _ in 0..limit {
        let subject_edge = (subject_index + subject.len() - 1) % subject.len();
        let clip_edge = (clip_index + clip.len() - 1) % clip.len();
        if collinear_edges_overlap(
            subject_previous,
            subject[subject_index],
            clip_previous,
            clip[clip_index],
        ) {
            degenerate = true;
        }
        if let Some((intersection, first_parameter, second_parameter)) = segment_intersection(
            subject_previous,
            subject[subject_index],
            clip_previous,
            clip[clip_index],
        ) {
            if first_parameter <= f64::EPSILON
                || first_parameter >= 1.0 - f64::EPSILON
                || second_parameter <= f64::EPSILON
                || second_parameter >= 1.0 - f64::EPSILON
            {
                degenerate = true;
            }
            if first_intersection.is_some_and(|first| key(first) == key(intersection)) {
                break;
            }
            if first_intersection.is_none() {
                first_intersection = Some(intersection);
            }
            if collect_splits {
                subject_splits[subject_edge].push(first_parameter.clamp(0.0, 1.0));
                clip_splits[clip_edge].push(second_parameter.clamp(0.0, 1.0));
            }
            if collect_output {
                output.push(intersection);
            }
            inside = if cross(clip_vector, subtract(subject[subject_index], clip_previous)) >= 0.0 {
                1
            } else {
                2
            };
        }

        if cross(clip_vector, subject_vector) > 0.0 {
            if cross(clip_vector, subtract(subject[subject_index], clip_previous)) >= 0.0 {
                if collect_splits && clip_inside[clip_edge].is_none() {
                    clip_inside[clip_edge] = Some(inside == 2);
                }
                clip_previous = clip[clip_index];
                clip_index = (clip_index + 1) % clip.len();
                clip_vector = subtract(clip[clip_index], clip_previous);
                clip_steps += 1;
                if collect_output && inside == 2 {
                    output.push(clip_previous);
                }
            } else {
                if collect_splits && subject_inside[subject_edge].is_none() {
                    subject_inside[subject_edge] = Some(inside == 1);
                }
                subject_previous = subject[subject_index];
                subject_index = (subject_index + 1) % subject.len();
                subject_vector = subtract(subject[subject_index], subject_previous);
                subject_steps += 1;
                if collect_output && inside == 1 {
                    output.push(subject_previous);
                }
            }
        } else if cross(subject_vector, subtract(clip[clip_index], subject_previous)) >= 0.0 {
            if collect_splits && subject_inside[subject_edge].is_none() {
                subject_inside[subject_edge] = Some(inside == 1);
            }
            subject_previous = subject[subject_index];
            subject_index = (subject_index + 1) % subject.len();
            subject_vector = subtract(subject[subject_index], subject_previous);
            subject_steps += 1;
            if collect_output && inside == 1 {
                output.push(subject_previous);
            }
        } else {
            if collect_splits && clip_inside[clip_edge].is_none() {
                clip_inside[clip_edge] = Some(inside == 2);
            }
            clip_previous = clip[clip_index];
            clip_index = (clip_index + 1) % clip.len();
            clip_vector = subtract(clip[clip_index], clip_previous);
            clip_steps += 1;
            if collect_output && inside == 2 {
                output.push(clip_previous);
            }
        }
        if !collect_output && subject_steps >= subject.len() && clip_steps >= clip.len() {
            break;
        }
    }

    if !collect_output {
        return ConvexWalk {
            output: Vec::new(),
            subject_splits,
            clip_splits,
            subject_inside,
            clip_inside,
            degenerate,
        };
    }
    if output.is_empty() {
        if convex_contains(subject[0], clip, true) {
            output.extend_from_slice(subject);
        } else if convex_contains(clip[0], subject, true) {
            output.extend_from_slice(clip);
        } else {
            return ConvexWalk {
                output: Vec::new(),
                subject_splits,
                clip_splits,
                subject_inside,
                clip_inside,
                degenerate,
            };
        }
    }
    output.dedup_by(|left, right| *left == *right);
    if output.len() > 1 && output.first() == output.last() {
        output.pop();
    }
    if !has_nonzero_area(&output) {
        return ConvexWalk {
            output: Vec::new(),
            subject_splits,
            clip_splits,
            subject_inside,
            clip_inside,
            degenerate,
        };
    }
    canonicalize(&mut output);
    ConvexWalk {
        output: vec![output],
        subject_splits,
        clip_splits,
        subject_inside,
        clip_inside,
        degenerate,
    }
}

fn segment_intersection(
    first_start: Point,
    first_end: Point,
    second_start: Point,
    second_end: Point,
) -> Option<(Point, f64, f64)> {
    let first_vector = subtract(first_end, first_start);
    let second_vector = subtract(second_end, second_start);
    let denominator = cross(first_vector, second_vector);
    if denominator.abs() <= f64::EPSILON {
        return None;
    }
    let between = subtract(second_start, first_start);
    let first_parameter = cross(between, second_vector) / denominator;
    let second_parameter = cross(between, first_vector) / denominator;
    if in_unit_interval(first_parameter) && in_unit_interval(second_parameter) {
        Some((
            Point {
                x: first_start.x + first_vector.x * first_parameter,
                y: first_start.y + first_vector.y * first_parameter,
            },
            first_parameter,
            second_parameter,
        ))
    } else {
        None
    }
}

fn collinear_segments_overlap(
    first_start: Point,
    first_end: Point,
    second_start: Point,
    second_end: Point,
) -> bool {
    let first_min_x = first_start.x.min(first_end.x);
    let first_max_x = first_start.x.max(first_end.x);
    let second_min_x = second_start.x.min(second_end.x);
    let second_max_x = second_start.x.max(second_end.x);
    let first_min_y = first_start.y.min(first_end.y);
    let first_max_y = first_start.y.max(first_end.y);
    let second_min_y = second_start.y.min(second_end.y);
    let second_max_y = second_start.y.max(second_end.y);
    first_min_x <= second_max_x
        && second_min_x <= first_max_x
        && first_min_y <= second_max_y
        && second_min_y <= first_max_y
}

fn collinear_edges_overlap(
    first_start: Point,
    first_end: Point,
    second_start: Point,
    second_end: Point,
) -> bool {
    cross(subtract(first_end, first_start), subtract(second_end, second_start)).abs()
        <= f64::EPSILON
        && cross(subtract(second_start, first_start), subtract(first_end, first_start)).abs()
            <= f64::EPSILON
        && collinear_segments_overlap(first_start, first_end, second_start, second_end)
}

fn has_nonzero_area(points: &[Point]) -> bool {
    points.len() >= 3 && area2(points).abs() > f64::EPSILON
}

fn point_bounds(points: &[Point]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        |(min_x, min_y, max_x, max_y), point| {
            (min_x.min(point.x), min_y.min(point.y), max_x.max(point.x), max_y.max(point.y))
        },
    )
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
    let mut edges = Vec::with_capacity(path.len());
    append_path_edges(&mut edges, path, 0, false);
    edges
}

fn append_path_edges(edges: &mut Vec<Edge>, path: &[Point], path_id: usize, subject: bool) {
    path.iter().zip(path.iter().cycle().skip(1)).take(path.len()).for_each(|(start, end)| {
        if let (Some(start_key), Some(end_key)) = (key(*start), key(*end)) {
            if start_key != end_key {
                edges.push(Edge {
                    start: *start,
                    end: *end,
                    start_key,
                    end_key,
                    path_id,
                    subject,
                    min_x: start.x.min(end.x),
                    min_y: start.y.min(end.y),
                    max_x: start.x.max(end.x),
                    max_y: start.y.max(end.y),
                    min_x_key: total_order_key(start.x.min(end.x)),
                    max_x_key: total_order_key(start.x.max(end.x)),
                });
            }
        }
    });
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
    first_values: &mut SplitParameters,
    second_values: &mut SplitParameters,
    reject_collinear: bool,
) -> bool {
    if !well_conditioned_pair(first, second) {
        return false;
    }
    if boxes_disjoint(first, second) {
        return true;
    }
    let first_vector = subtract(first.end, first.start);
    let second_vector = subtract(second.end, second.start);
    let denominator = cross(first_vector, second_vector);
    let between = subtract(second.start, first.start);
    if reject_collinear
        && key_cross(first, second) == 0
        && cross(between, first_vector).abs() <= f64::EPSILON
    {
        return false;
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

fn well_conditioned_pair(first: &Edge, second: &Edge) -> bool {
    if boxes_disjoint(first, second) {
        return true;
    }
    let first_vector = subtract(first.end, first.start);
    let second_vector = subtract(second.end, second.start);
    let denominator = cross(first_vector, second_vector);
    if denominator == 0.0 {
        return true;
    }
    let scale = first_vector.x.abs().max(first_vector.y.abs())
        * second_vector.x.abs().max(second_vector.y.abs());
    denominator.abs() > PREDICATE_TOLERANCE * scale.max(1.0) || key_cross(first, second) == 0
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
    paths: &[ContainmentPath<'_>],
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
        let bucket_count = path.buckets.len();
        let left_bucket =
            containment_bucket_if_inside(left, min_x, min_y, max_x, max_y, bucket_count);
        let right_bucket =
            containment_bucket_if_inside(right, min_x, min_y, max_x, max_y, bucket_count);
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

#[inline]
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
    bucket_count: usize,
) -> Option<usize> {
    (point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y)
        .then(|| containment_bucket(point.y, min_y, max_y, bucket_count))
}

#[cfg(test)]
fn containment_paths<P: PathSlice>(paths: &[P]) -> Vec<ContainmentPath<'_>> {
    containment_paths_with_properties(paths, &[])
}

fn containment_paths_with_properties<'a, P: PathSlice>(
    paths: &'a [P],
    properties: &[PathProperties],
) -> Vec<ContainmentPath<'a>> {
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
            if properties.get(index).is_some_and(|properties| properties.convex) {
                return ContainmentPath {
                    bounds,
                    buckets: Vec::new(),
                    convex_points: Some(points),
                    convex_winding: Some(if area2(points) > 0.0 { 1 } else { -1 }),
                };
            }
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
            let bucket_count = containment_bucket_count(edges.len());
            let mut buckets = vec![Vec::new(); bucket_count];
            for edge in edges {
                let start_y = edge.start.y;
                let end_y = edge.start.y + edge.delta_y;
                let lower =
                    containment_bucket(start_y.min(end_y), bounds.1, bounds.3, bucket_count);
                let upper =
                    containment_bucket(start_y.max(end_y), bounds.1, bounds.3, bucket_count);
                for bucket in buckets.iter_mut().take(upper + 1).skip(lower) {
                    bucket.push(edge);
                }
            }
            ContainmentPath { bounds, buckets, convex_points: None, convex_winding: None }
        })
        .collect()
}

fn containment_bucket_count(edge_count: usize) -> usize {
    // Tiny rings should not pay for dozens of empty Vec allocations. Keeping
    // roughly two edges per power-of-two bucket preserves cheap indexing while
    // capping construction and memory for large paths.
    edge_count.div_ceil(2).clamp(1, MAX_CONTAINMENT_BUCKETS).next_power_of_two()
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn containment_bucket(value: f64, min_y: f64, max_y: f64, bucket_count: usize) -> usize {
    if max_y <= min_y || bucket_count <= 1 {
        return 0;
    }
    let scaled = (value - min_y) / (max_y - min_y) * bucket_count as f64;
    scaled.floor().clamp(0.0, bucket_count as f64 - 1.0) as usize
}

fn stitch_ordered_convex(
    edges: &[DirectedEdge],
    subject_range: (usize, usize),
    clip_range: (usize, usize),
) -> Result<PathsD, Error> {
    // Convex XOR emits each operand's boundary in cyclic source order, so
    // most successor links can be resolved without a point-key hash table.
    if edges.is_empty() {
        return Ok(Vec::new());
    }
    let mut next = vec![usize::MAX; edges.len()];
    for (start, end) in [subject_range, clip_range] {
        if start > end || end > edges.len() {
            return Err(Error::TopologyFailure);
        }
        if start == end {
            continue;
        }
        for index in start..end {
            let following = if index + 1 == end { start } else { index + 1 };
            if edges[index].end_key == edges[following].start_key {
                if next[index] != usize::MAX && next[index] != following {
                    return Err(Error::TopologyFailure);
                }
                next[index] = following;
            }
            if edges[following].end_key == edges[index].start_key {
                if next[following] != usize::MAX && next[following] != index {
                    return Err(Error::TopologyFailure);
                }
                next[following] = index;
            }
        }
    }
    for (index, edge) in edges.iter().enumerate() {
        if next[index] != usize::MAX {
            continue;
        }
        let mut candidates = Vec::with_capacity(2);
        for (candidate, outgoing) in edges.iter().enumerate() {
            if outgoing.start_key == edge.end_key {
                candidates.push(candidate);
            }
        }
        if candidates.is_empty() {
            return Err(Error::TopologyFailure);
        }
        if candidates.len() == 1 {
            next[index] = candidates[0];
            continue;
        }
        let origin_point = edges[candidates[0]].start;
        candidates.sort_unstable_by(|left, right| {
            compare_angle(
                subtract(edges[*left].end, origin_point),
                subtract(edges[*right].end, origin_point),
            )
        });
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
    stitch_next(edges, &next)
}

fn stitch(edges: &[DirectedEdge]) -> Result<PathsD, Error> {
    if edges.is_empty() {
        return Ok(Vec::new());
    }
    let mut outgoing: FastMap<PointKey, Outgoing> =
        FastMap::with_capacity_and_hasher(edges.len(), BuildHasherDefault::default());
    for (index, edge) in edges.iter().enumerate() {
        match outgoing.entry(edge.start_key) {
            Entry::Vacant(entry) => {
                entry.insert(Outgoing::Single(index));
            }
            Entry::Occupied(mut entry) => match entry.get_mut() {
                Outgoing::Single(previous) => {
                    *entry.get_mut() = Outgoing::Multiple(vec![*previous, index]);
                }
                Outgoing::Multiple(indices) => indices.push(index),
            },
        }
    }
    let mut next = vec![0; edges.len()];
    for candidates in outgoing.values_mut() {
        if let Outgoing::Multiple(indices) = candidates {
            let origin_point = edges[*indices.first().ok_or(Error::TopologyFailure)?].start;
            indices.sort_by(|left, right| {
                compare_angle(
                    subtract(edges[*left].end, origin_point),
                    subtract(edges[*right].end, origin_point),
                )
            });
        }
    }
    for (index, edge) in edges.iter().enumerate() {
        let Some(candidates) = outgoing.get(&edge.end_key) else {
            return Err(Error::TopologyFailure);
        };
        match candidates {
            Outgoing::Single(next_index) => next[index] = *next_index,
            Outgoing::Multiple(candidates) => {
                let reverse = subtract(edge.start, edge.end);
                let insertion = candidates
                    .iter()
                    .position(|candidate| {
                        compare_angle(
                            subtract(edges[*candidate].end, edges[*candidate].start),
                            reverse,
                        ) != Ordering::Less
                    })
                    .unwrap_or(candidates.len());
                next[index] = candidates[(insertion + candidates.len() - 1) % candidates.len()];
            }
        }
    }
    stitch_next(edges, &next)
}

fn stitch_next(edges: &[DirectedEdge], next: &[usize]) -> Result<PathsD, Error> {
    let mut visited = vec![false; edges.len()];
    let mut paths = Vec::new();
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        let mut path = Vec::with_capacity(edges.len());
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
            paths.push(path);
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
        let xor_result = try_boolean_opd(BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: ClipType::Xor,
            fill_rule: FillRule::EvenOdd,
        })
        .expect("high-vertex xor input is eligible");
        assert!(xor_result.is_ok(), "fast xor path failed: {xor_result:?}");
    }

    #[test]
    fn rounded_high_vertex_xor_stays_on_linear_walk() {
        #[allow(clippy::cast_precision_loss)]
        fn rounded_circle(center_x: f64) -> PathD {
            (0..64)
                .map(|index| {
                    let angle = std::f64::consts::TAU * f64::from(index) / 64.0;
                    PointD::new(
                        ((center_x + 100.0 * angle.cos()) * 1000.0).round() / 1000.0,
                        ((100.0 * angle.sin()) * 1000.0).round() / 1000.0,
                    )
                })
                .collect()
        }

        let subject = rounded_circle(0.0);
        let clip = rounded_circle(30.0);
        let subject_points = fast_paths(std::slice::from_ref(&subject)).expect("finite subject");
        let clip_points = fast_paths(std::slice::from_ref(&clip)).expect("finite clip");
        assert!(strict_convex(subject_points[0].points()));
        assert!(strict_convex(clip_points[0].points()));
        let walk =
            convex_boundary_walk(subject_points[0].points(), clip_points[0].points(), false, true);
        assert!(!walk.degenerate);
        let fast = convex_boolean(
            subject_points[0].points(),
            clip_points[0].points(),
            ClipType::Xor,
            Some((true, true)),
        )
        .expect("linear convex xor should close");
        let request = BooleanRequestD {
            subjects: std::slice::from_ref(&subject),
            clips: std::slice::from_ref(&clip),
            clip_type: ClipType::Xor,
            fill_rule: FillRule::EvenOdd,
        };
        let exact = crate::boolean::boolean_opd_exact(request).expect("exact oracle should close");
        let summary = |paths: &PathsD| {
            let mut values = paths
                .iter()
                .map(|path| (path.len(), (area2(path).abs() * 1_000_000.0).round().to_bits()))
                .collect::<Vec<_>>();
            values.sort_unstable();
            values
        };
        assert_eq!(summary(&fast), summary(&exact));
        assert!(try_boolean_opd(request).expect("rounded input is eligible").is_ok());
    }

    fn point(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    #[test]
    fn simple_operand_sides_are_classified_across_crossing_edges() {
        let subject = vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0), point(0.0, 10.0)];
        let clip = vec![point(5.0, 0.0), point(15.0, 0.0), point(15.0, 10.0), point(5.0, 10.0)];

        let result = run(
            std::slice::from_ref(&subject),
            std::slice::from_ref(&clip),
            ClipType::Intersection,
            FillRule::EvenOdd,
        )
        .expect("well-conditioned rectangles should remain on the fast path");

        assert_eq!(result.len(), 1);
        assert!((area2(&result[0]).abs() - 100.0).abs() <= f64::EPSILON);
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
        let mut first_values = SplitParameters::new();
        let mut second_values = SplitParameters::new();
        assert!(split_pair(
            &crossing_first,
            &crossing_second,
            &mut first_values,
            &mut second_values,
            false,
        ));
        assert!(first_values.values().iter().any(|value| (*value - 0.5).abs() < f64::EPSILON));
        assert!(second_values.values().iter().any(|value| (*value - 0.5).abs() < f64::EPSILON));
        let mut disjoint_first = SplitParameters::new();
        let mut disjoint_second = SplitParameters::new();
        assert!(split_pair(
            &edge(point(0.0, 0.0), point(1.0, 0.0)),
            &edge(point(0.0, 1.0), point(1.0, 1.0)),
            &mut disjoint_first,
            &mut disjoint_second,
            false,
        ));
        let mut overlap_first = SplitParameters::new();
        let mut overlap_second = SplitParameters::new();
        assert!(split_pair(
            &edge(point(0.0, 0.0), point(10.0, 0.0)),
            &edge(point(5.0, 0.0), point(15.0, 0.0)),
            &mut overlap_first,
            &mut overlap_second,
            false,
        ));
        assert_eq!(overlap_first.len(), 3);
        assert_eq!(overlap_second.len(), 3);
        let mut ill_conditioned_first = SplitParameters::new();
        let mut ill_conditioned_second = SplitParameters::new();
        assert!(!split_pair(
            &edge(point(0.0, 0.0), point(1_000_000.0, 1_000_000.0)),
            &edge(point(0.0, 0.0), point(1_000_000.0, 1_000_000.000_000_001)),
            &mut ill_conditioned_first,
            &mut ill_conditioned_second,
            false,
        ));
        let mut inline_values = SplitParameters::new();
        inline_values.push(0.5);
        inline_values.sort_dedup();
        assert_eq!(inline_values.values(), &[0.0, 0.5, 1.0]);
        let mut overflow_values = SplitParameters::new();
        overflow_values.push(0.5);
        overflow_values.push(0.500_000_000_000_001);
        overflow_values.push(0.25);
        overflow_values.push(0.75);
        overflow_values.sort_dedup();
        assert_eq!(overflow_values.values(), &[0.0, 0.25, 0.5, 0.75, 1.0]);
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

        let rectangle_paths = [rectangle.clone()];
        let containment = containment_paths(&rectangle_paths);
        assert_eq!(containment[0].buckets.len(), 2);
        let convex_rectangle_paths = [rectangle.clone()];
        let convex_containment = containment_paths_with_properties(
            &convex_rectangle_paths,
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
        let clockwise_paths =
            [vec![point(0.0, 0.0), point(0.0, 10.0), point(10.0, 10.0), point(10.0, 0.0)]];
        let clockwise_containment = containment_paths_with_properties(
            &clockwise_paths,
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
        assert_eq!(containment_bucket_count(0), 1);
        assert_eq!(containment_bucket_count(4), 2);
        assert_eq!(containment_bucket_count(usize::MAX), MAX_CONTAINMENT_BUCKETS);
        assert_eq!(containment_bucket(0.0, 1.0, 1.0, 2), 0);
        assert_eq!(containment_bucket(-100.0, 0.0, 10.0, 2), 0);
        assert_eq!(containment_bucket(100.0, 0.0, 10.0, 2), 1);
        assert!(containment_bucket_if_inside(point(-1.0, 5.0), 0.0, 0.0, 10.0, 10.0, 2).is_none());
        assert!(containment_bucket_if_inside(point(5.0, 5.0), 0.0, 0.0, 10.0, 10.0, 2).is_some());
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

    #[test]
    #[allow(clippy::too_many_lines)]
    fn covers_convex_walk_and_conservative_fallbacks() {
        let rectangle =
            vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0), point(0.0, 10.0)];
        let overlap = vec![point(5.0, -1.0), point(15.0, -1.0), point(15.0, 9.0), point(5.0, 9.0)];
        let collinear_convex = vec![
            point(0.0, 0.0),
            point(5.0, 0.0),
            point(10.0, 0.0),
            point(10.0, 10.0),
            point(0.0, 10.0),
        ];
        let concave = vec![
            point(0.0, 0.0),
            point(3.0, 0.0),
            point(1.0, 1.0),
            point(3.0, 3.0),
            point(0.0, 3.0),
        ];
        assert!(!strict_convex(&rectangle[..2]));
        assert!(!strict_convex(&collinear_convex));
        assert!(!strict_convex(&concave));

        let subjects = [collinear_convex.clone()];
        let clips = [rectangle.clone()];
        let result = try_boolean_opd(BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        })
        .expect("collinear convex paths remain eligible");
        assert!(result.is_ok(), "conservative convex fallback failed: {result:?}");
        let non_convex_subjects = [concave.clone()];
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: &non_convex_subjects,
                clips: &clips,
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );

        assert!(
            run(std::slice::from_ref(&rectangle), &[], ClipType::Union, FillRule::Positive).is_ok()
        );
        assert!(
            run(&[], std::slice::from_ref(&rectangle), ClipType::Union, FillRule::Positive).is_ok()
        );
        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            assert!(
                convex_boolean(&rectangle, &overlap, clip_type, Some((false, false))).is_some()
            );
        }

        let narrow = vec![
            point(5.0, -1.0),
            point(5.000_000_000_1, -1.0),
            point(5.000_000_000_1, 1.0),
            point(5.0, 1.0),
        ];
        assert!(convex_boolean(&rectangle, &narrow, ClipType::Xor, Some((false, false))).is_none());

        let huge = vec![
            point(10_000_000_000.0, 0.0),
            point(10_000_000_010.0, 0.0),
            point(10_000_000_010.0, 10.0),
            point(10_000_000_000.0, 10.0),
        ];
        let huge_splits = vec![SplitParameters::new(); huge.len()];
        let rectangle_splits = vec![SplitParameters::new(); rectangle.len()];
        assert!(
            convex_boolean_from_splits(
                &huge,
                &rectangle,
                ClipType::Union,
                huge_splits.clone(),
                rectangle_splits.clone(),
                &[],
                &[],
            )
            .is_none()
        );
        assert!(
            convex_boolean_from_splits(
                &rectangle,
                &huge,
                ClipType::Union,
                rectangle_splits,
                huge_splits,
                &[],
                &[],
            )
            .is_none()
        );

        let tiny = vec![point(0.0, 0.0), point(0.000_000_000_1, 0.0), point(0.0, 1.0)];
        let tiny_index = ConvexIndex::new(&tiny);
        let rectangle_index = ConvexIndex::new(&rectangle);
        let mut tiny_splits = vec![SplitParameters::new(); tiny.len()];
        let mut tiny_directed = Vec::new();
        assert!(
            append_convex_operation_edges(
                &tiny,
                &mut tiny_splits,
                &[],
                true,
                ClipType::Union,
                &tiny_index,
                &rectangle_index,
                &mut tiny_directed,
            )
            .is_some()
        );

        let mut bad_edges = Vec::new();
        append_path_edges(
            &mut bad_edges,
            &[point(f64::NAN, 0.0), point(1.0, 0.0), point(0.0, 1.0)],
            0,
            false,
        );
        assert_eq!(bad_edges.len(), 1);

        let mut collinear_first = SplitParameters::new();
        let mut collinear_second = SplitParameters::new();
        assert!(!split_pair(
            &edge(point(0.0, 0.0), point(10.0, 0.0)),
            &edge(point(5.0, 0.0), point(15.0, 0.0)),
            &mut collinear_first,
            &mut collinear_second,
            true,
        ));
        assert!(collinear_segments_overlap(
            point(0.0, 0.0),
            point(10.0, 0.0),
            point(5.0, 0.0),
            point(15.0, 0.0),
        ));
        assert!(!collinear_segments_overlap(
            point(0.0, 0.0),
            point(1.0, 0.0),
            point(2.0, 0.0),
            point(3.0, 0.0),
        ));
        assert!(collinear_segments_overlap(
            point(0.0, 0.0),
            point(0.0, 10.0),
            point(0.0, 5.0),
            point(0.0, 15.0),
        ));

        let far_right =
            vec![point(20.0, 0.0), point(30.0, 0.0), point(30.0, 10.0), point(20.0, 10.0)];
        let far_left =
            vec![point(-30.0, 0.0), point(-20.0, 0.0), point(-20.0, 10.0), point(-30.0, 10.0)];
        let far_above =
            vec![point(0.0, 20.0), point(10.0, 20.0), point(10.0, 30.0), point(0.0, 30.0)];
        let far_below =
            vec![point(0.0, -30.0), point(10.0, -30.0), point(10.0, -20.0), point(0.0, -20.0)];
        let empty_result: PathsD = Vec::new();
        for far in [&far_right, &far_left, &far_above, &far_below] {
            assert!(valid_convex_intersection(&empty_result, &rectangle, far));
        }
        let short_result: PathsD = vec![vec![point(0.0, 0.0), point(1.0, 0.0)]];
        assert!(!valid_convex_intersection(&short_result, &rectangle, &overlap,));
        let multiple_result: PathsD = vec![rectangle.clone(), overlap.clone()];
        assert!(!valid_convex_intersection(&multiple_result, &rectangle, &overlap,));

        let contained = vec![point(2.0, 2.0), point(4.0, 2.0), point(4.0, 4.0), point(2.0, 4.0)];
        let contained_walk = convex_boundary_walk(&contained, &rectangle, true, false);
        assert_eq!(contained_walk.output.len(), 1);
        assert_eq!(contained_walk.output[0].len(), contained.len());

        let touching =
            vec![point(10.0, 0.0), point(20.0, 0.0), point(20.0, 10.0), point(10.0, 10.0)];
        let touching_walk = convex_boundary_walk(&rectangle, &touching, true, false);
        assert!(touching_walk.degenerate);
        assert!(touching_walk.output.is_empty());

        let overlap_walk = convex_boundary_walk(&rectangle, &overlap, true, false);
        let reversed_overlap_walk = convex_boundary_walk(&overlap, &rectangle, true, false);
        assert_ne!(overlap_walk.output.len() + reversed_overlap_walk.output.len(), 0);
        let rectangle_reversed = rectangle.iter().copied().rev().collect::<Vec<_>>();
        let overlap_reversed = overlap.iter().copied().rev().collect::<Vec<_>>();
        for subject in [&rectangle, &rectangle_reversed, &overlap, &overlap_reversed] {
            for clip in [&rectangle, &rectangle_reversed, &overlap, &overlap_reversed] {
                if !std::ptr::eq(subject, clip) {
                    let _ = convex_boundary_walk(subject, clip, true, false);
                }
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn covers_remaining_selection_and_stitch_branches() {
        let rectangle =
            vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0), point(0.0, 10.0)];
        let clockwise = rectangle.iter().copied().rev().collect::<Vec<_>>();
        let collinear = vec![
            point(0.0, 0.0),
            point(5.0, 0.0),
            point(10.0, 0.0),
            point(10.0, 10.0),
            point(0.0, 10.0),
        ];
        let concave = vec![
            point(0.0, 0.0),
            point(4.0, 0.0),
            point(4.0, 4.0),
            point(2.0, 1.0),
            point(0.0, 4.0),
        ];
        let key_collision =
            vec![point(0.0, 0.0), point(0.25e-9, 0.0), point(1.0, 1.0), point(0.0, 1.0)];
        let short = vec![point(0.0, 0.0), point(1.0, 0.0)];
        let collinear_clockwise = collinear.iter().copied().rev().collect::<Vec<_>>();

        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: &[],
                clips: std::slice::from_ref(&rectangle),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );
        let _ = try_single_convex_boolean(BooleanRequestD {
            subjects: std::slice::from_ref(&rectangle),
            clips: std::slice::from_ref(&collinear),
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        });
        let _ = try_single_convex_boolean(BooleanRequestD {
            subjects: std::slice::from_ref(&collinear),
            clips: std::slice::from_ref(&rectangle),
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        });
        assert!(strict_convex(&key_collision));
        assert!(!keyable_path(&key_collision));
        assert!(!keyable_path(&[point(0.0, 0.0), point(f64::NAN, 1.0)]));
        let huge = vec![
            point(MAX_COORDINATE + 1.0, 0.0),
            point(MAX_COORDINATE + 2.0, 0.0),
            point(MAX_COORDINATE + 1.0, 1.0),
        ];
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&huge),
                clips: std::slice::from_ref(&rectangle),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&key_collision),
                clips: std::slice::from_ref(&rectangle),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&rectangle),
                clips: std::slice::from_ref(&key_collision),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&rectangle),
                clips: std::slice::from_ref(&clockwise),
                clip_type: ClipType::Union,
                fill_rule: FillRule::Positive,
            })
            .is_none()
        );
        let _ = try_single_convex_boolean(BooleanRequestD {
            subjects: std::slice::from_ref(&collinear),
            clips: std::slice::from_ref(&rectangle),
            clip_type: ClipType::Union,
            fill_rule: FillRule::Negative,
        });
        let _ = try_single_convex_boolean(BooleanRequestD {
            subjects: std::slice::from_ref(&collinear_clockwise),
            clips: std::slice::from_ref(&collinear),
            clip_type: ClipType::Union,
            fill_rule: FillRule::Negative,
        });
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&clockwise),
                clips: std::slice::from_ref(&rectangle),
                clip_type: ClipType::Union,
                fill_rule: FillRule::Negative,
            })
            .is_none()
        );
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&short),
                clips: std::slice::from_ref(&rectangle),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&rectangle),
                clips: std::slice::from_ref(&short),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );
        assert!(
            try_single_convex_boolean(BooleanRequestD {
                subjects: std::slice::from_ref(&rectangle),
                clips: std::slice::from_ref(&concave),
                clip_type: ClipType::Union,
                fill_rule: FillRule::EvenOdd,
            })
            .is_none()
        );

        let closed = vec![point(0.0, 0.0), point(1.0, 0.0), point(0.0, 1.0), point(0.0, 0.0)];
        let closed_paths = fast_paths(std::slice::from_ref(&closed)).expect("closed path");
        assert!(matches!(closed_paths.first(), Some(FastPath::Owned(_))));

        let invalid_paths = [
            vec![point(f64::NAN, 0.0), point(1.0, 0.0), point(0.0, 1.0)],
            vec![point(0.0, f64::NAN), point(1.0, 0.0), point(0.0, 1.0)],
            vec![point(0.0, 0.0), point(f64::NAN, 0.0), point(0.0, 1.0)],
            vec![point(0.0, 0.0), point(0.0, f64::NAN), point(0.0, 1.0)],
            vec![point(MAX_COORDINATE + 1.0, 0.0), point(1.0, 0.0), point(0.0, 1.0)],
            vec![point(0.0, MAX_COORDINATE + 1.0), point(1.0, 0.0), point(0.0, 1.0)],
            vec![point(0.0, 0.0), point(MAX_COORDINATE + 1.0, 0.0), point(0.0, 1.0)],
            vec![point(0.0, 0.0), point(0.0, MAX_COORDINATE + 1.0), point(0.0, 1.0)],
        ];
        for path in invalid_paths {
            assert!(!eligible(&[path], &[]));
        }

        for subject in [concave.clone(), clockwise.clone()] {
            for clip in [rectangle.clone(), collinear.clone()] {
                for clip_type in
                    [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
                {
                    let _ = run(
                        std::slice::from_ref(&subject),
                        std::slice::from_ref(&clip),
                        clip_type,
                        FillRule::EvenOdd,
                    );
                }
            }
        }
        let _ = run(
            &[concave.clone(), rectangle.clone()],
            std::slice::from_ref(&collinear),
            ClipType::Union,
            FillRule::EvenOdd,
        );
        let _ = run(
            std::slice::from_ref(&concave),
            &[collinear.clone(), rectangle.clone()],
            ClipType::Union,
            FillRule::EvenOdd,
        );
        let _ = run(std::slice::from_ref(&concave), &[], ClipType::Union, FillRule::EvenOdd);
        let _ = run(&[], std::slice::from_ref(&concave), ClipType::Union, FillRule::EvenOdd);
        let _ =
            run(&[rectangle.clone(), clockwise.clone()], &[], ClipType::Union, FillRule::Positive);
        let _ =
            run(&[], &[rectangle.clone(), clockwise.clone()], ClipType::Union, FillRule::Positive);

        let touching =
            vec![point(10.0, 0.0), point(20.0, 0.0), point(20.0, 10.0), point(10.0, 10.0)];
        let overlap = vec![point(5.0, -1.0), point(15.0, -1.0), point(15.0, 9.0), point(5.0, 9.0)];
        let far_right =
            vec![point(20.0, 0.0), point(30.0, 0.0), point(30.0, 10.0), point(20.0, 10.0)];
        let _ = convex_boolean(&rectangle, &touching, ClipType::Intersection, Some((true, true)));
        let _ = convex_boundary_walk(&rectangle, &far_right, true, false);
        let _ = convex_boundary_walk(&rectangle, &far_right, false, true);
        let _ = convex_boundary_walk(&touching, &rectangle, true, false);
        let _ = convex_boundary_walk(&rectangle, &collinear, true, false);

        let triangle_touch = vec![point(5.0, 0.0), point(15.0, -5.0), point(15.0, 5.0)];
        let triangle_cross = vec![point(10.0, -5.0), point(15.0, 5.0), point(5.0, 5.0)];
        let diamond = vec![point(5.0, -5.0), point(15.0, 5.0), point(5.0, 15.0), point(-5.0, 5.0)];
        for subject in [&rectangle, &overlap, &triangle_touch, &triangle_cross, &diamond] {
            for clip in [&rectangle, &overlap, &triangle_touch, &triangle_cross, &diamond] {
                let _ = convex_boundary_walk(subject, clip, true, true);
                let _ = convex_boundary_walk(subject, clip, false, true);
            }
        }
        let overlap_reversed = overlap.iter().copied().rev().collect::<Vec<_>>();
        let triangle_touch_reversed = triangle_touch.iter().copied().rev().collect::<Vec<_>>();
        let triangle_cross_reversed = triangle_cross.iter().copied().rev().collect::<Vec<_>>();
        let diamond_reversed = diamond.iter().copied().rev().collect::<Vec<_>>();
        for subject in [
            &rectangle,
            &clockwise,
            &overlap,
            &overlap_reversed,
            &triangle_touch,
            &triangle_touch_reversed,
            &triangle_cross,
            &triangle_cross_reversed,
            &diamond,
            &diamond_reversed,
        ] {
            for clip in [
                &rectangle,
                &clockwise,
                &overlap,
                &overlap_reversed,
                &triangle_touch,
                &triangle_touch_reversed,
                &triangle_cross,
                &triangle_cross_reversed,
                &diamond,
                &diamond_reversed,
            ] {
                let _ = convex_boundary_walk(subject, clip, true, false);
            }
        }
        let nested = vec![point(2.0, 2.0), point(4.0, 2.0), point(4.0, 4.0), point(2.0, 4.0)];
        let nested_reversed = nested.iter().copied().rev().collect::<Vec<_>>();
        let _ = convex_boundary_walk(&rectangle, &rectangle, true, false);
        let _ = convex_boundary_walk(&rectangle, &clockwise, true, false);
        let _ = convex_boundary_walk(&nested, &rectangle, true, false);
        let _ = convex_boundary_walk(&nested_reversed, &rectangle, true, false);
        let _ = convex_boundary_walk(&rectangle, &nested, true, false);
        let _ = convex_boundary_walk(&rectangle, &nested_reversed, true, false);
        let line_triangle = vec![point(-1.0, 0.0), point(1.0, 0.0), point(3.0, 0.0)];
        let _ = convex_boundary_walk(&line_triangle, &collinear, true, true);
        let _ = convex_boundary_walk(&collinear, &line_triangle, true, true);

        let mut fake_subject_splits = vec![SplitParameters::new(); rectangle.len()];
        fake_subject_splits[0].push(0.5);
        let _ = convex_boolean_from_splits(
            &rectangle,
            &overlap,
            ClipType::Xor,
            fake_subject_splits,
            vec![SplitParameters::new(); overlap.len()],
            &[],
            &[],
        );

        let mut first_values = SplitParameters::new();
        let mut second_values = SplitParameters::new();
        assert!(split_pair(
            &edge(point(0.0, 0.0), point(10.0, 10.0)),
            &edge(point(0.0, 10.0), point(10.0, 20.0)),
            &mut first_values,
            &mut second_values,
            true,
        ));
        assert!(!collinear_segments_overlap(
            point(2.0, 0.0),
            point(3.0, 0.0),
            point(0.0, 0.0),
            point(1.0, 0.0),
        ));
        assert!(!collinear_segments_overlap(
            point(0.0, 0.0),
            point(0.0, 1.0),
            point(0.0, 2.0),
            point(0.0, 3.0),
        ));
        assert!(!collinear_segments_overlap(
            point(0.0, 2.0),
            point(0.0, 3.0),
            point(0.0, 0.0),
            point(0.0, 1.0),
        ));
        assert!(collinear_edges_overlap(
            point(0.0, 0.0),
            point(10.0, 0.0),
            point(5.0, 0.0),
            point(15.0, 0.0),
        ));
        assert!(!collinear_edges_overlap(
            point(0.0, 0.0),
            point(10.0, 0.0),
            point(20.0, 0.0),
            point(30.0, 0.0),
        ));
        assert!(!has_nonzero_area(&[]));
        assert!(!has_nonzero_area(&line_triangle));
        assert!(has_nonzero_area(&rectangle));
        let mut crossing_first_values = SplitParameters::new();
        let mut crossing_second_values = SplitParameters::new();
        assert!(split_pair(
            &edge(point(0.0, 0.0), point(10.0, 1.0)),
            &edge(point(0.0, 0.5), point(1.0, 1.5)),
            &mut crossing_first_values,
            &mut crossing_second_values,
            false,
        ));

        let mut state = WindingState::default();
        let downward = ContainmentEdge {
            start: point(0.0, 10.0),
            delta_x: 0.0,
            delta_y: -10.0,
            intercept: 0.0,
            upward: false,
        };
        update_winding(&mut state, point(-1.0, 20.0), &downward);
        assert!(!state.parity);
        assert_eq!(state.winding, 0);
        let upward = ContainmentEdge {
            start: point(0.0, 0.0),
            delta_x: 0.0,
            delta_y: 10.0,
            intercept: 0.0,
            upward: true,
        };
        update_winding(&mut state, point(1.0, 5.0), &upward);
        update_winding(&mut state, point(-1.0, 5.0), &downward);

        let outside_result = vec![vec![point(-1.0, -1.0), point(-2.0, -1.0), point(-1.0, -2.0)]];
        assert!(!valid_convex_intersection(&outside_result, &rectangle, &collinear));
        let clockwise_result = vec![clockwise.clone()];
        assert!(!valid_convex_intersection(&clockwise_result, &rectangle, &rectangle));

        let square_edges = [
            directed(point(0.0, 0.0), point(10.0, 0.0)),
            directed(point(10.0, 0.0), point(10.0, 10.0)),
            directed(point(10.0, 10.0), point(0.0, 10.0)),
            directed(point(0.0, 10.0), point(0.0, 0.0)),
        ];
        assert_eq!(stitch_ordered_convex(&[], (0, 0), (0, 0)), Ok(Vec::new()));
        assert!(stitch_ordered_convex(&square_edges, (2, 1), (0, 0)).is_err());
        assert!(stitch_ordered_convex(&square_edges, (0, 5), (0, 0)).is_err());
        assert!(
            stitch_ordered_convex(&[directed(point(0.0, 0.0), point(1.0, 0.0))], (0, 0), (0, 0))
                .is_err()
        );
        assert!(
            stitch_ordered_convex(
                &[
                    directed(point(0.0, 0.0), point(1.0, 0.0)),
                    directed(point(1.0, 0.0), point(0.0, 0.0)),
                ],
                (0, 0),
                (0, 0),
            )
            .is_ok()
        );
        assert!(
            stitch_ordered_convex(
                &[
                    directed(point(0.0, 0.0), point(1.0, 0.0)),
                    directed(point(0.0, 1.0), point(0.0, 0.0)),
                    directed(point(0.0, 0.0), point(1.0, 1.0)),
                ],
                (0, 3),
                (0, 0),
            )
            .is_err()
        );
        assert!(
            stitch_ordered_convex(
                &[
                    directed(point(0.0, 0.0), point(1.0, 0.0)),
                    directed(point(1.0, 0.0), point(2.0, 0.0)),
                    directed(point(3.0, 0.0), point(4.0, 0.0)),
                    directed(point(1.0, 0.0), point(5.0, 0.0)),
                ],
                (0, 2),
                (0, 4),
            )
            .is_err()
        );
    }

    #[test]
    fn covers_containment_bucket_and_downward_winding_branches() {
        let rectangle =
            vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0), point(0.0, 10.0)];
        let containment = containment_paths(std::slice::from_ref(&rectangle));
        assert_eq!(
            paths_contain_pair(point(1.0, 1.0), point(1.2, 1.2), &containment, FillRule::EvenOdd),
            (true, true)
        );

        let downward = ContainmentEdge {
            start: point(0.0, 10.0),
            delta_x: 0.0,
            delta_y: -10.0,
            intercept: 0.0,
            upward: false,
        };
        let mut state = WindingState::default();
        update_winding(&mut state, point(-1.0, 5.0), &downward);
        assert!(state.parity);
        assert_eq!(state.winding, -1);
    }
}
