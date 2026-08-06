//! Adaptive dispatch in front of the general floating-point fast path.

#[path = "fast.rs"]
mod base;

use std::cmp::Ordering;
use std::collections::{HashMap, hash_map::Entry};
use std::hash::{BuildHasherDefault, Hasher};

use crate::{BooleanRequestD, ClipType, Error, FillRule, PathD, PathsD, PointD};

const KEY_SCALE: f64 = 1_000_000_000.0;
const MAX_COORDINATE: f64 = 1_000_000.0;
const MAX_ORTHOGONAL_GRID_POINTS: usize = 1_000_000;
const MIN_FUSED_SPAN_DENOMINATOR: u128 = 8;

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

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PointKey {
    x: i64,
    y: i64,
}

#[derive(Clone, Copy)]
struct GridCoordinate {
    key: i64,
    value: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct WindingDifference {
    subject: i32,
    clip: i32,
}

#[derive(Clone, Copy, Debug)]
struct GridEvent {
    x: usize,
    y0: usize,
    y1: usize,
    subject_delta: i32,
    clip_delta: i32,
}

#[derive(Clone, Copy)]
struct DirectedEdge {
    start: PointD,
    end: PointD,
    start_key: PointKey,
    end_key: PointKey,
}

enum Outgoing {
    Single(usize),
    Multiple(Vec<usize>),
}

#[derive(Clone, Copy)]
struct HorizontalRun {
    start: GridCoordinate,
    direction: i8,
}

#[derive(Clone, Copy, Debug, Default)]
struct HorizontalSpanStats {
    edge_count: usize,
    total_key_span: u128,
    min_x: Option<i64>,
    max_x: Option<i64>,
}

impl HorizontalSpanStats {
    fn record_point(&mut self, point: PointKey) {
        self.min_x = Some(self.min_x.map_or(point.x, |value| value.min(point.x)));
        self.max_x = Some(self.max_x.map_or(point.x, |value| value.max(point.x)));
    }

    fn record_horizontal(&mut self, start: PointKey, end: PointKey) {
        self.edge_count += 1;
        self.total_key_span += u128::from(start.x.abs_diff(end.x));
    }

    fn should_fuse(self) -> bool {
        let (Some(min_x), Some(max_x)) = (self.min_x, self.max_x) else {
            return false;
        };
        if self.edge_count == 0 || min_x == max_x {
            return false;
        }
        let coordinate_span = u128::from(min_x.abs_diff(max_x));
        let edge_count = u128::try_from(self.edge_count).expect("edge count fits u128");
        self.total_key_span.saturating_mul(MIN_FUSED_SPAN_DENOMINATOR)
            >= coordinate_span.saturating_mul(edge_count)
    }
}

pub(crate) fn try_boolean_opd(request: BooleanRequestD<'_>) -> Option<Result<PathsD, ()>> {
    if request.subjects.len() + request.clips.len() >= 8
        && let Some(result) = try_adaptive_orthogonal(request)
    {
        return Some(result.map_err(|_| ()));
    }
    base::try_boolean_opd(request)
}

fn try_adaptive_orthogonal(request: BooleanRequestD<'_>) -> Option<Result<PathsD, Error>> {
    if request.subjects.is_empty() && request.clips.is_empty() {
        return None;
    }

    let mut stats = HorizontalSpanStats::default();
    let mut point_count = 0_usize;
    for path in request.subjects.iter().chain(request.clips) {
        if path.is_empty() {
            continue;
        }
        if path.len() < 3 {
            return None;
        }
        point_count = point_count.checked_add(path.len())?;
        for (start, end) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
            let start_key = key(*start)?;
            let end_key = key(*end)?;
            stats.record_point(start_key);
            if start_key == end_key || (start_key.x == end_key.x) == (start_key.y == end_key.y) {
                return None;
            }
            if start_key.y == end_key.y {
                stats.record_horizontal(start_key, end_key);
            }
        }
    }
    if !stats.should_fuse() {
        return None;
    }

    let mut xs = Vec::with_capacity(point_count);
    let mut ys = Vec::with_capacity(point_count);
    for point in request.subjects.iter().chain(request.clips).flatten() {
        let point_key = key(*point)?;
        xs.push(GridCoordinate { key: point_key.x, value: point.x + 0.0 });
        ys.push(GridCoordinate { key: point_key.y, value: point.y + 0.0 });
    }
    let xs = dedup_coordinates(xs)?;
    let ys = dedup_coordinates(ys)?;
    let grid_size = orthogonal_grid_size(xs.len(), ys.len())?;

    let mut events = Vec::with_capacity(point_count / 2);
    let mut difference = vec![WindingDifference::default(); ys.len()];
    for path in &request.subjects {
        add_orthogonal_path(&mut events, &mut difference, &xs, &ys, path, true)?;
    }
    for path in &request.clips {
        add_orthogonal_path(&mut events, &mut difference, &xs, &ys, path, false)?;
    }
    events.sort_unstable_by_key(|event| event.x);

    Some(fused_orthogonal_sweep(
        &events,
        difference,
        &xs,
        &ys,
        request.fill_rule,
        request.clip_type,
        point_count,
        grid_size,
    ))
}

fn orthogonal_grid_size(width: usize, height: usize) -> Option<usize> {
    width.checked_mul(height).filter(|grid_size| *grid_size <= MAX_ORTHOGONAL_GRID_POINTS)
}

fn dedup_coordinates(mut coordinates: Vec<GridCoordinate>) -> Option<Vec<GridCoordinate>> {
    coordinates.sort_unstable_by_key(|coordinate| coordinate.key);
    let mut result: Vec<GridCoordinate> = Vec::with_capacity(coordinates.len());
    for coordinate in coordinates {
        if let Some(previous) = result.last() {
            if previous.key == coordinate.key {
                if previous.value.to_bits() != coordinate.value.to_bits() {
                    return None;
                }
                continue;
            }
        }
        result.push(coordinate);
    }
    (result.len() >= 2).then_some(result)
}

fn add_orthogonal_path(
    events: &mut Vec<GridEvent>,
    difference: &mut [WindingDifference],
    xs: &[GridCoordinate],
    ys: &[GridCoordinate],
    path: &[PointD],
    subject: bool,
) -> Option<()> {
    for (start, end) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
        let start_key = key(*start)?;
        let end_key = key(*end)?;
        if start_key.x != end_key.x {
            continue;
        }
        let x = xs.binary_search_by_key(&start_key.x, |coordinate| coordinate.key).ok()?;
        let start_y = ys.binary_search_by_key(&start_key.y, |coordinate| coordinate.key).ok()?;
        let end_y = ys.binary_search_by_key(&end_key.y, |coordinate| coordinate.key).ok()?;
        let (y0, y1, winding) =
            if start_y < end_y { (start_y, end_y, 1) } else { (end_y, start_y, -1) };
        let (subject_delta, clip_delta) = if subject { (winding, 0) } else { (0, winding) };
        add_winding_range(difference, y0, y1, subject_delta, clip_delta);
        events.push(GridEvent { x, y0, y1, subject_delta, clip_delta });
    }
    Some(())
}

fn add_winding_range(
    values: &mut [WindingDifference],
    y0: usize,
    y1: usize,
    subject_delta: i32,
    clip_delta: i32,
) {
    values[y0].subject += subject_delta;
    values[y0].clip += clip_delta;
    values[y1].subject -= subject_delta;
    values[y1].clip -= clip_delta;
}

#[allow(clippy::too_many_arguments)]
fn fused_orthogonal_sweep(
    events: &[GridEvent],
    mut difference: Vec<WindingDifference>,
    xs: &[GridCoordinate],
    ys: &[GridCoordinate],
    fill_rule: FillRule,
    clip_type: ClipType,
    edge_count: usize,
    grid_size: usize,
) -> Result<PathsD, Error> {
    let mut filled = vec![false; ys.len() - 1];
    let mut horizontal_runs = vec![None; ys.len()];
    let mut event_index = 0;
    let mut boundary = Vec::with_capacity(edge_count.saturating_mul(2).min(grid_size));
    for (x, coordinate) in xs.iter().copied().enumerate().take(xs.len() - 1) {
        while event_index < events.len() && events[event_index].x == x {
            let event = events[event_index];
            add_winding_range(
                &mut difference,
                event.y0,
                event.y1,
                -event.subject_delta,
                -event.clip_delta,
            );
            event_index += 1;
        }
        sweep_orthogonal_column(
            &difference,
            &mut filled,
            fill_rule,
            clip_type,
            coordinate,
            ys,
            &mut horizontal_runs,
            &mut boundary,
        );
    }

    let right = *xs.last().expect("orthogonal grid has at least two columns");
    for (y, is_filled) in filled.iter().copied().enumerate() {
        if is_filled {
            push_grid_edge(&mut boundary, right, ys[y], right, ys[y + 1]);
        }
    }
    flush_horizontal_runs(&mut boundary, &mut horizontal_runs, right, ys);
    finish_orthogonal_boundary(&boundary)
}

#[allow(clippy::too_many_arguments)]
fn sweep_orthogonal_column(
    difference: &[WindingDifference],
    filled: &mut [bool],
    fill_rule: FillRule,
    clip_type: ClipType,
    x: GridCoordinate,
    ys: &[GridCoordinate],
    horizontal_runs: &mut [Option<HorizontalRun>],
    boundary: &mut Vec<DirectedEdge>,
) {
    match fill_rule {
        FillRule::EvenOdd => sweep_orthogonal_column_with(
            difference,
            filled,
            clip_type,
            x,
            ys,
            horizontal_runs,
            boundary,
            |winding| (winding & 1) != 0,
        ),
        FillRule::NonZero => sweep_orthogonal_column_with(
            difference,
            filled,
            clip_type,
            x,
            ys,
            horizontal_runs,
            boundary,
            |winding| winding != 0,
        ),
        FillRule::Positive => sweep_orthogonal_column_with(
            difference,
            filled,
            clip_type,
            x,
            ys,
            horizontal_runs,
            boundary,
            |winding| winding > 0,
        ),
        FillRule::Negative => sweep_orthogonal_column_with(
            difference,
            filled,
            clip_type,
            x,
            ys,
            horizontal_runs,
            boundary,
            |winding| winding < 0,
        ),
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn sweep_orthogonal_column_with(
    difference: &[WindingDifference],
    filled: &mut [bool],
    clip_type: ClipType,
    x: GridCoordinate,
    ys: &[GridCoordinate],
    horizontal_runs: &mut [Option<HorizontalRun>],
    boundary: &mut Vec<DirectedEdge>,
    contains: impl Fn(i32) -> bool,
) {
    match clip_type {
        ClipType::Intersection => sweep_orthogonal_cells(
            difference,
            filled,
            x,
            ys,
            horizontal_runs,
            boundary,
            &contains,
            |subject, clip| subject && clip,
        ),
        ClipType::Union => sweep_orthogonal_cells(
            difference,
            filled,
            x,
            ys,
            horizontal_runs,
            boundary,
            &contains,
            |subject, clip| subject || clip,
        ),
        ClipType::Difference => sweep_orthogonal_cells(
            difference,
            filled,
            x,
            ys,
            horizontal_runs,
            boundary,
            &contains,
            |subject, clip| subject && !clip,
        ),
        ClipType::Xor => sweep_orthogonal_cells(
            difference,
            filled,
            x,
            ys,
            horizontal_runs,
            boundary,
            &contains,
            |subject, clip| subject != clip,
        ),
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn sweep_orthogonal_cells(
    difference: &[WindingDifference],
    filled: &mut [bool],
    x: GridCoordinate,
    ys: &[GridCoordinate],
    horizontal_runs: &mut [Option<HorizontalRun>],
    boundary: &mut Vec<DirectedEdge>,
    contains: &impl Fn(i32) -> bool,
    operation: impl Fn(bool, bool) -> bool,
) {
    let mut subject_winding = 0;
    let mut clip_winding = 0;
    let mut below = false;
    for (y, (delta, cell)) in difference.iter().zip(filled.iter_mut()).enumerate() {
        subject_winding += delta.subject;
        clip_winding += delta.clip;
        let current = operation(contains(subject_winding), contains(clip_winding));
        if *cell != current {
            if current {
                push_grid_edge(boundary, x, ys[y + 1], x, ys[y]);
            } else {
                push_grid_edge(boundary, x, ys[y], x, ys[y + 1]);
            }
        }
        let direction = if y == 0 {
            i8::from(current)
        } else if below == current {
            0
        } else if current {
            1
        } else {
            -1
        };
        update_horizontal_run(boundary, &mut horizontal_runs[y], direction, x, ys[y]);
        *cell = current;
        below = current;
    }
    let top = filled.len();
    update_horizontal_run(
        boundary,
        &mut horizontal_runs[top],
        if below { -1 } else { 0 },
        x,
        ys[top],
    );
}

#[inline]
fn update_horizontal_run(
    boundary: &mut Vec<DirectedEdge>,
    run: &mut Option<HorizontalRun>,
    direction: i8,
    x: GridCoordinate,
    y: GridCoordinate,
) {
    match *run {
        Some(active) if active.direction == direction => return,
        Some(active) => {
            if active.direction > 0 {
                push_grid_edge(boundary, active.start, y, x, y);
            } else {
                push_grid_edge(boundary, x, y, active.start, y);
            }
            *run = None;
        }
        None => {}
    }
    if direction != 0 {
        *run = Some(HorizontalRun { start: x, direction });
    }
}

fn flush_horizontal_runs(
    boundary: &mut Vec<DirectedEdge>,
    horizontal_runs: &mut [Option<HorizontalRun>],
    x: GridCoordinate,
    ys: &[GridCoordinate],
) {
    for (run, y) in horizontal_runs.iter_mut().zip(ys.iter().copied()) {
        update_horizontal_run(boundary, run, 0, x, y);
    }
}

#[inline]
fn push_grid_edge(
    boundary: &mut Vec<DirectedEdge>,
    start_x: GridCoordinate,
    start_y: GridCoordinate,
    end_x: GridCoordinate,
    end_y: GridCoordinate,
) {
    boundary.push(DirectedEdge {
        start: PointD::new(start_x.value, start_y.value),
        end: PointD::new(end_x.value, end_y.value),
        start_key: PointKey { x: start_x.key, y: start_y.key },
        end_key: PointKey { x: end_x.key, y: end_y.key },
    });
}

fn finish_orthogonal_boundary(boundary: &[DirectedEdge]) -> Result<PathsD, Error> {
    stitch(boundary)?
        .into_iter()
        .map(|path| crate::trim_collinear_d(&path, crate::PathKind::Closed))
        .collect()
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
            let origin = edges[indices[0]].start;
            indices.sort_by(|left, right| {
                compare_angle(subtract(edges[*left].end, origin), subtract(edges[*right].end, origin))
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

fn compare_angle(first: PointD, second: PointD) -> Ordering {
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

fn area2(path: &[PointD]) -> f64 {
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

fn subtract(left: PointD, right: PointD) -> PointD {
    PointD::new(left.x - right.x, left.y - right.y)
}

fn cross(left: PointD, right: PointD) -> f64 {
    left.x * right.y - left.y * right.x
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
    let x = (point.x * KEY_SCALE).round();
    let y = (point.y * KEY_SCALE).round();
    Some(PointKey { x: x as i64, y: y as i64 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> PathD {
        vec![
            PointD::new(min_x, min_y),
            PointD::new(max_x, min_y),
            PointD::new(max_x, max_y),
            PointD::new(min_x, max_y),
        ]
    }

    fn summary(paths: &PathsD) -> Vec<(usize, u64)> {
        let mut result = paths
            .iter()
            .map(|path| (path.len(), (area2(path).abs() * 1_000_000.0).round().to_bits()))
            .collect::<Vec<_>>();
        result.sort_unstable();
        result
    }

    #[test]
    fn span_gate_selects_long_edges_only() {
        let mut short = HorizontalSpanStats::default();
        short.record_point(PointKey { x: 0, y: 0 });
        short.record_point(PointKey { x: 100, y: 0 });
        short.record_horizontal(PointKey { x: 0, y: 0 }, PointKey { x: 10, y: 0 });
        assert!(!short.should_fuse());

        let mut long = HorizontalSpanStats::default();
        long.record_point(PointKey { x: 0, y: 0 });
        long.record_point(PointKey { x: 100, y: 0 });
        long.record_horizontal(PointKey { x: 0, y: 0 }, PointKey { x: 80, y: 0 });
        assert!(long.should_fuse());
        assert!(!HorizontalSpanStats::default().should_fuse());
    }

    #[test]
    fn adaptive_kernel_matches_exact_for_all_operations_and_fill_rules() {
        let subjects = (0..12)
            .map(|inset| {
                let inset = f64::from(inset);
                rectangle(inset, inset, 80.0 - inset, 80.0 - inset)
            })
            .collect::<Vec<_>>();
        let clips = (0..12)
            .map(|inset| {
                let inset = f64::from(inset) + 0.5;
                rectangle(inset, inset, 80.5 - inset, 80.5 - inset)
            })
            .collect::<Vec<_>>();

        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            for fill_rule in
                [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
            {
                let request = BooleanRequestD {
                    subjects: &subjects,
                    clips: &clips,
                    clip_type,
                    fill_rule,
                };
                let adaptive = try_adaptive_orthogonal(request)
                    .expect("long orthogonal spans should select the adaptive kernel")
                    .expect("adaptive boundary should close");
                let exact = crate::boolean::boolean_opd_exact(request).expect("exact oracle");
                assert_eq!(summary(&adaptive), summary(&exact), "{clip_type:?} {fill_rule:?}");
            }
        }
    }

    #[test]
    fn short_spans_fall_through_to_the_existing_kernel() {
        let subjects = (0..8)
            .map(|index| {
                let x = f64::from(index) * 20.0;
                rectangle(x, 0.0, x + 2.0, 2.0)
            })
            .collect::<Vec<_>>();
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(try_adaptive_orthogonal(request).is_none());
        assert!(try_boolean_opd(request).expect("existing fast path handles the request").is_ok());
    }
}
