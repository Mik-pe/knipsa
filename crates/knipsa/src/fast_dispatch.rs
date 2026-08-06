//! Adaptive dispatch in front of the general floating-point fast path.

#[path = "fast.rs"]
mod base;

use std::collections::{HashMap, hash_map::Entry};

use crate::{BooleanRequestD, ClipType, FillRule, PathD, PathsD, PointD};

const KEY_SCALE: f64 = 1_000_000_000.0;
const MAX_COORDINATE: f64 = 1_000_000.0;
const MAX_ORTHOGONAL_GRID_POINTS: usize = 1_000_000;
const MIN_RECTANGLE_COUNT: usize = 8;
const MIN_FUSED_SPAN_DENOMINATOR: u128 = 8;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PointKey {
    x: i64,
    y: i64,
}

#[derive(Clone, Copy, Debug)]
struct RectangleKey {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}

#[derive(Clone, Copy)]
struct GridCoordinate {
    key: i64,
    value: f64,
}

#[derive(Clone, Copy, Debug)]
struct GridEvent {
    x: usize,
    y0: usize,
    y1: usize,
}

#[derive(Clone, Copy)]
struct DirectedEdge {
    start: PointD,
    end: PointD,
    start_key: PointKey,
    end_key: PointKey,
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
    fn record_rectangle(&mut self, rectangle: RectangleKey) {
        self.edge_count += 2;
        self.total_key_span += u128::from(rectangle.min_x.abs_diff(rectangle.max_x)) * 2;
        self.min_x =
            Some(self.min_x.map_or(rectangle.min_x, |value| value.min(rectangle.min_x)));
        self.max_x =
            Some(self.max_x.map_or(rectangle.max_x, |value| value.max(rectangle.max_x)));
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
    if let Some(result) = try_long_rectangle_xor(request) {
        return Some(Ok(result));
    }
    base::try_boolean_opd(request)
}

fn try_long_rectangle_xor(request: BooleanRequestD<'_>) -> Option<PathsD> {
    if request.clip_type != ClipType::Xor || request.fill_rule != FillRule::EvenOdd {
        return None;
    }

    let capacity = request.subjects.len() + request.clips.len();
    let mut rectangles = Vec::with_capacity(capacity);
    let mut stats = HorizontalSpanStats::default();
    let mut xs = Vec::with_capacity(capacity * 2);
    let mut ys = Vec::with_capacity(capacity * 2);
    for path in request.subjects.iter().chain(request.clips) {
        if path.is_empty() {
            continue;
        }
        let rectangle = rectangle_key(path)?;
        stats.record_rectangle(rectangle);
        rectangles.push(rectangle);
        xs.push(coordinate(rectangle.min_x, path, true)?);
        xs.push(coordinate(rectangle.max_x, path, true)?);
        ys.push(coordinate(rectangle.min_y, path, false)?);
        ys.push(coordinate(rectangle.max_y, path, false)?);
    }

    if rectangles.len() < MIN_RECTANGLE_COUNT || !stats.should_fuse() {
        return None;
    }

    let xs = dedup_coordinates(xs)?;
    let ys = dedup_coordinates(ys)?;
    orthogonal_grid_size(xs.len(), ys.len())?;

    let mut events = Vec::with_capacity(rectangles.len() * 2);
    for rectangle in rectangles {
        let min_x = coordinate_index(&xs, rectangle.min_x)?;
        let max_x = coordinate_index(&xs, rectangle.max_x)?;
        let min_y = coordinate_index(&ys, rectangle.min_y)?;
        let max_y = coordinate_index(&ys, rectangle.max_y)?;
        events.push(GridEvent { x: min_x, y0: min_y, y1: max_y });
        events.push(GridEvent { x: max_x, y0: min_y, y1: max_y });
    }
    events.sort_unstable_by_key(|event| event.x);
    fused_rectangle_xor(&events, &xs, &ys)
}

fn rectangle_key(path: &[PointD]) -> Option<RectangleKey> {
    let [first, second, third, fourth] = path else { return None };
    let points = [key(*first)?, key(*second)?, key(*third)?, key(*fourth)?];
    for (start, end) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        if start == end || (start.x == end.x) == (start.y == end.y) {
            return None;
        }
    }

    let min_x = points.iter().map(|point| point.x).min()?;
    let max_x = points.iter().map(|point| point.x).max()?;
    let min_y = points.iter().map(|point| point.y).min()?;
    let max_y = points.iter().map(|point| point.y).max()?;
    if min_x == max_x || min_y == max_y {
        return None;
    }

    let mut corners = 0_u8;
    for point in points {
        let x_bit = u32::from(point.x == max_x);
        let y_bit = u32::from(point.y == max_y);
        let bit = 1_u8 << (x_bit + 2 * y_bit);
        if corners & bit != 0 {
            return None;
        }
        corners |= bit;
    }
    Some(RectangleKey { min_x, min_y, max_x, max_y })
}

fn coordinate(key: i64, path: &[PointD], x_axis: bool) -> Option<GridCoordinate> {
    path.iter().find_map(|point| {
        let point_key = self::key(*point)?;
        let candidate = if x_axis { point_key.x } else { point_key.y };
        (candidate == key).then_some(GridCoordinate {
            key,
            value: if x_axis { point.x + 0.0 } else { point.y + 0.0 },
        })
    })
}

fn coordinate_index(coordinates: &[GridCoordinate], key: i64) -> Option<usize> {
    coordinates.binary_search_by_key(&key, |coordinate| coordinate.key).ok()
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

fn fused_rectangle_xor(
    events: &[GridEvent],
    xs: &[GridCoordinate],
    ys: &[GridCoordinate],
) -> Option<PathsD> {
    let mut difference = vec![false; ys.len()];
    let mut filled = vec![false; ys.len() - 1];
    let mut horizontal_runs = vec![None; ys.len()];
    let mut boundary = Vec::with_capacity(events.len() * 2);
    let mut event_index = 0;

    for (x, coordinate) in xs.iter().copied().enumerate().take(xs.len() - 1) {
        while event_index < events.len() && events[event_index].x == x {
            let event = events[event_index];
            difference[event.y0] ^= true;
            difference[event.y1] ^= true;
            event_index += 1;
        }
        sweep_column(
            &difference,
            &mut filled,
            coordinate,
            ys,
            &mut horizontal_runs,
            &mut boundary,
        );
    }

    let right = *xs.last()?;
    for (row, is_filled) in filled.iter().copied().enumerate() {
        if is_filled {
            push_grid_edge(&mut boundary, right, ys[row], right, ys[row + 1]);
        }
    }
    flush_horizontal_runs(&mut boundary, &mut horizontal_runs, right, ys);
    stitch_unique(&boundary)
}

fn sweep_column(
    difference: &[bool],
    filled: &mut [bool],
    x: GridCoordinate,
    ys: &[GridCoordinate],
    horizontal_runs: &mut [Option<HorizontalRun>],
    boundary: &mut Vec<DirectedEdge>,
) {
    let mut current = false;
    let mut below = false;
    for (row, (toggle, previous)) in difference.iter().zip(filled.iter_mut()).enumerate() {
        current ^= *toggle;
        if *previous != current {
            if current {
                push_grid_edge(boundary, x, ys[row + 1], x, ys[row]);
            } else {
                push_grid_edge(boundary, x, ys[row], x, ys[row + 1]);
            }
        }
        let direction = if row == 0 {
            i8::from(current)
        } else if below == current {
            0
        } else if current {
            1
        } else {
            -1
        };
        update_horizontal_run(boundary, &mut horizontal_runs[row], direction, x, ys[row]);
        *previous = current;
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

fn stitch_unique(edges: &[DirectedEdge]) -> Option<PathsD> {
    if edges.is_empty() {
        return Some(Vec::new());
    }

    let mut outgoing = HashMap::with_capacity(edges.len());
    let mut incoming = HashMap::with_capacity(edges.len());
    for (index, edge) in edges.iter().enumerate() {
        match outgoing.entry(edge.start_key) {
            Entry::Vacant(entry) => {
                entry.insert(index);
            }
            Entry::Occupied(_) => return None,
        }
        match incoming.entry(edge.end_key) {
            Entry::Vacant(entry) => {
                entry.insert(index);
            }
            Entry::Occupied(_) => return None,
        }
    }
    if outgoing.len() != incoming.len()
        || outgoing.keys().any(|point| !incoming.contains_key(point))
    {
        return None;
    }

    let mut next = Vec::with_capacity(edges.len());
    for edge in edges {
        next.push(*outgoing.get(&edge.end_key)?);
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
                    return None;
                }
                break;
            }
            visited[current] = true;
            path.push(edges[current].start);
            current = next[current];
        }
        if path.len() >= 3 && signed_area2(&path).abs() > f64::EPSILON {
            path = crate::trim_collinear_d(&path, crate::PathKind::Closed).ok()?;
            canonicalize(&mut path);
            paths.push(path);
        }
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
    if let Some((minimum, _)) = path
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
    {
        path.rotate_left(minimum);
    }
}

fn compare_paths(left: &PathD, right: &PathD) -> std::cmp::Ordering {
    left.iter()
        .zip(right)
        .map(|(left, right)| left.x.total_cmp(&right.x).then(left.y.total_cmp(&right.y)))
        .find(|ordering| *ordering != std::cmp::Ordering::Equal)
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

    fn grid_coordinate(key: i32) -> GridCoordinate {
        GridCoordinate { key: i64::from(key), value: f64::from(key) }
    }

    fn directed(start: PointD, end: PointD) -> DirectedEdge {
        DirectedEdge {
            start,
            end,
            start_key: key(start).expect("test point is keyable"),
            end_key: key(end).expect("test point is keyable"),
        }
    }

    fn summary(paths: &PathsD) -> Vec<(usize, u64)> {
        let mut result = paths
            .iter()
            .map(|path| (path.len(), (signed_area2(path).abs() * 1_000_000.0).round().to_bits()))
            .collect::<Vec<_>>();
        result.sort_unstable();
        result
    }

    #[test]
    fn long_rectangle_xor_matches_exact_oracle() {
        let subjects = (0..16)
            .map(|inset| {
                let inset = f64::from(inset);
                rectangle(inset, inset, 120.0 - inset, 120.0 - inset)
            })
            .collect::<Vec<_>>();
        let mut clips = (0..16)
            .map(|inset| {
                let inset = f64::from(inset) + 0.5;
                rectangle(inset, inset, 120.5 - inset, 120.5 - inset)
            })
            .collect::<Vec<_>>();
        clips.iter_mut().step_by(2).for_each(|path| path.reverse());
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: ClipType::Xor,
            fill_rule: FillRule::EvenOdd,
        };
        let specialized = try_long_rectangle_xor(request).expect("long rectangles select fusion");
        let exact = crate::boolean::boolean_opd_exact(request).expect("exact oracle closes");
        assert_eq!(summary(&specialized), summary(&exact));
        assert_eq!(summary(&try_boolean_opd(request).unwrap().unwrap()), summary(&exact));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn rejects_non_specialized_inputs_and_invalid_rectangles() {
        let rectangles = (0..8)
            .map(|index| {
                let x = f64::from(index) * 20.0;
                rectangle(x, 0.0, x + 2.0, 2.0)
            })
            .collect::<Vec<_>>();
        let request = BooleanRequestD {
            subjects: &rectangles,
            clips: &[],
            clip_type: ClipType::Xor,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(try_long_rectangle_xor(request).is_none());
        assert!(try_boolean_opd(request).unwrap().is_ok());
        assert!(try_long_rectangle_xor(BooleanRequestD {
            clip_type: ClipType::Union,
            ..request
        })
        .is_none());
        assert!(try_long_rectangle_xor(BooleanRequestD {
            fill_rule: FillRule::NonZero,
            ..request
        })
        .is_none());
        assert!(try_long_rectangle_xor(BooleanRequestD {
            subjects: &rectangles[..7],
            ..request
        })
        .is_none());

        for invalid in [
            Vec::new(),
            vec![PointD::new(0.0, 0.0); 3],
            vec![
                PointD::new(0.0, 0.0),
                PointD::new(2.0, 1.0),
                PointD::new(2.0, 2.0),
                PointD::new(0.0, 2.0),
            ],
            vec![
                PointD::new(0.0, 0.0),
                PointD::new(2.0, 0.0),
                PointD::new(2.0, 0.0),
                PointD::new(0.0, 2.0),
            ],
            vec![
                PointD::new(0.0, 0.0),
                PointD::new(2.0, 0.0),
                PointD::new(2.0, 2.0),
                PointD::new(2.0, 0.0),
            ],
            vec![
                PointD::new(f64::NAN, 0.0),
                PointD::new(2.0, 0.0),
                PointD::new(2.0, 2.0),
                PointD::new(0.0, 2.0),
            ],
            vec![
                PointD::new(MAX_COORDINATE + 1.0, 0.0),
                PointD::new(MAX_COORDINATE + 2.0, 0.0),
                PointD::new(MAX_COORDINATE + 2.0, 2.0),
                PointD::new(MAX_COORDINATE + 1.0, 2.0),
            ],
        ] {
            assert!(rectangle_key(&invalid).is_none());
        }
        assert!(rectangle_key(&rectangle(0.0, 0.0, 2.0, 2.0)).is_some());
    }

    #[test]
    fn covers_coordinate_grid_and_span_guards() {
        assert!(super::coordinate(0, &[], true).is_none());
        let path = rectangle(0.0, 0.0, 2.0, 2.0);
        assert_eq!(super::coordinate(0, &path, true).unwrap().value, 0.0);
        assert_eq!(super::coordinate(2_000_000_000, &path, false).unwrap().value, 2.0);
        assert_eq!(coordinate_index(&[grid_coordinate(0), grid_coordinate(2)], 2), Some(1));
        assert!(coordinate_index(&[grid_coordinate(0), grid_coordinate(2)], 1).is_none());
        assert_eq!(orthogonal_grid_size(10, 20), Some(200));
        assert!(orthogonal_grid_size(usize::MAX, 2).is_none());
        assert!(orthogonal_grid_size(1_001, 1_000).is_none());

        let same = vec![grid_coordinate(0), grid_coordinate(0), grid_coordinate(2)];
        assert_eq!(dedup_coordinates(same).unwrap().len(), 2);
        assert!(dedup_coordinates(vec![
            GridCoordinate { key: 0, value: 0.0 },
            GridCoordinate { key: 0, value: 0.25 },
        ])
        .is_none());
        assert!(dedup_coordinates(vec![grid_coordinate(0)]).is_none());

        let mut empty = HorizontalSpanStats::default();
        assert!(!empty.should_fuse());
        empty.record_rectangle(RectangleKey { min_x: 0, min_y: 0, max_x: 0, max_y: 1 });
        assert!(!empty.should_fuse());
        let mut short = HorizontalSpanStats::default();
        short.record_rectangle(RectangleKey { min_x: 0, min_y: 0, max_x: 10, max_y: 1 });
        short.record_rectangle(RectangleKey { min_x: 90, min_y: 0, max_x: 100, max_y: 1 });
        assert!(!short.should_fuse());
        let mut long = HorizontalSpanStats::default();
        long.record_rectangle(RectangleKey { min_x: 0, min_y: 0, max_x: 90, max_y: 1 });
        long.record_rectangle(RectangleKey { min_x: 10, min_y: 0, max_x: 100, max_y: 1 });
        assert!(long.should_fuse());
    }

    #[test]
    fn coalesces_runs_and_rejects_ambiguous_stitching() {
        let y = grid_coordinate(7);
        let mut run = None;
        let mut boundary = Vec::new();
        update_horizontal_run(&mut boundary, &mut run, 1, grid_coordinate(0), y);
        update_horizontal_run(&mut boundary, &mut run, 1, grid_coordinate(1), y);
        update_horizontal_run(&mut boundary, &mut run, -1, grid_coordinate(2), y);
        update_horizontal_run(&mut boundary, &mut run, 0, grid_coordinate(3), y);
        update_horizontal_run(&mut boundary, &mut run, 0, grid_coordinate(4), y);
        assert_eq!(boundary.len(), 2);
        assert!(run.is_none());

        assert_eq!(stitch_unique(&[]), Some(Vec::new()));
        let square = [
            directed(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)),
            directed(PointD::new(1.0, 0.0), PointD::new(1.0, 1.0)),
            directed(PointD::new(1.0, 1.0), PointD::new(0.0, 1.0)),
            directed(PointD::new(0.0, 1.0), PointD::new(0.0, 0.0)),
        ];
        assert_eq!(stitch_unique(&square).unwrap().len(), 1);
        assert!(stitch_unique(&[square[0]]).is_none());
        assert!(stitch_unique(&[square[0], square[0]]).is_none());
        let duplicate_incoming =
            [square[0], directed(PointD::new(2.0, 0.0), PointD::new(1.0, 0.0))];
        assert!(stitch_unique(&duplicate_incoming).is_none());
    }

    #[test]
    fn sweeps_empty_and_filled_columns() {
        let xs = [grid_coordinate(0), grid_coordinate(1), grid_coordinate(2)];
        let ys = [grid_coordinate(0), grid_coordinate(1), grid_coordinate(2)];
        let events = [
            GridEvent { x: 0, y0: 0, y1: 2 },
            GridEvent { x: 2, y0: 0, y1: 2 },
        ];
        assert_eq!(fused_rectangle_xor(&events, &xs, &ys).unwrap().len(), 1);
        assert_eq!(fused_rectangle_xor(&[], &xs, &ys), Some(Vec::new()));
    }

    #[test]
    fn key_rejects_invalid_coordinates() {
        assert!(key(PointD::new(f64::NAN, 0.0)).is_none());
        assert!(key(PointD::new(0.0, f64::INFINITY)).is_none());
        assert!(key(PointD::new(MAX_COORDINATE + 1.0, 0.0)).is_none());
        assert!(key(PointD::new(0.0, MAX_COORDINATE + 1.0)).is_none());
        assert_eq!(
            key(PointD::new(1.0, 2.0)),
            Some(PointKey { x: 1_000_000_000, y: 2_000_000_000 })
        );
    }
}
