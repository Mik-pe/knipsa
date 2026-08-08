//! Certified fused sweep for large rectangle XOR workloads.

use std::collections::{HashMap, hash_map::Entry};

use crate::{
    BooleanRequestD, ClipType, FillRule, PathsD,
    dispatch::{
        AxisAlignedRectangle, DirectedEdge, GridCoordinate, axis_aligned_rectangle, canonicalize,
        compare_paths, dedup_grid_coordinates, orthogonal_grid_size,
    },
    geometry::signed_area2_d,
};

const MIN_RECTANGLE_COUNT: usize = 8;
const MIN_FUSED_SPAN_DENOMINATOR: u128 = 8;

#[derive(Clone, Copy, Debug)]
struct GridEvent {
    x: usize,
    y0: usize,
    y1: usize,
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
    fn record_rectangle(&mut self, rectangle: AxisAlignedRectangle) {
        self.edge_count += 2;
        self.total_key_span += u128::from(rectangle.min_x.key.abs_diff(rectangle.max_x.key)) * 2;
        self.min_x =
            Some(self.min_x.map_or(rectangle.min_x.key, |value| value.min(rectangle.min_x.key)));
        self.max_x =
            Some(self.max_x.map_or(rectangle.max_x.key, |value| value.max(rectangle.max_x.key)));
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

pub(crate) fn try_apply(request: BooleanRequestD<'_>) -> Option<PathsD> {
    try_long_rectangle_xor(request)
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
        let rectangle = axis_aligned_rectangle(path)?;
        stats.record_rectangle(rectangle);
        rectangles.push(rectangle);
        xs.push(rectangle.min_x);
        xs.push(rectangle.max_x);
        ys.push(rectangle.min_y);
        ys.push(rectangle.max_y);
    }

    if rectangles.len() < MIN_RECTANGLE_COUNT || !stats.should_fuse() {
        return None;
    }

    let xs = dedup_grid_coordinates(xs)?;
    let ys = dedup_grid_coordinates(ys)?;
    orthogonal_grid_size(xs.len(), ys.len())?;

    let mut events = Vec::with_capacity(rectangles.len() * 2);
    for rectangle in rectangles {
        let min_x = coordinate_index(&xs, rectangle.min_x.key)?;
        let max_x = coordinate_index(&xs, rectangle.max_x.key)?;
        let min_y = coordinate_index(&ys, rectangle.min_y.key)?;
        let max_y = coordinate_index(&ys, rectangle.max_y.key)?;
        events.push(GridEvent { x: min_x, y0: min_y, y1: max_y });
        events.push(GridEvent { x: max_x, y0: min_y, y1: max_y });
    }
    events.sort_unstable_by_key(|event| event.x);
    fused_rectangle_xor(&events, &xs, &ys)
}

fn coordinate_index(coordinates: &[GridCoordinate], key: i64) -> Option<usize> {
    coordinates.binary_search_by_key(&key, |coordinate| coordinate.key).ok()
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
        sweep_column(&difference, &mut filled, coordinate, ys, &mut horizontal_runs, &mut boundary);
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
    boundary.push(DirectedEdge::from_grid(start_x, start_y, end_x, end_y));
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
    if outgoing.keys().any(|point| !incoming.contains_key(point)) {
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
                debug_assert_eq!(current, start, "validated one-to-one graph must close at start");
                break;
            }
            visited[current] = true;
            path.push(edges[current].start);
            current = next[current];
        }
        if path.len() < 3 || signed_area2_d(&path).abs() <= f64::EPSILON {
            continue;
        }
        path = crate::trim_collinear_d(&path, crate::PathKind::Closed).ok()?;
        canonicalize(&mut path);
        paths.push(path);
    }
    paths.sort_by(compare_paths);
    Some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PathD, PointD,
        dispatch::{MAX_COORDINATE, PointKey, key},
    };

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

    fn grid_rectangle(min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> AxisAlignedRectangle {
        AxisAlignedRectangle {
            min_x: grid_coordinate(min_x),
            min_y: grid_coordinate(min_y),
            max_x: grid_coordinate(max_x),
            max_y: grid_coordinate(max_y),
        }
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
            .map(|path| (path.len(), (signed_area2_d(path).abs() * 1_000_000.0).round().to_bits()))
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
        let exact = crate::boolean::boolean_op_d_exact(request).expect("exact oracle closes");
        assert_eq!(summary(&specialized), summary(&exact));
        assert_eq!(summary(&try_apply(request).unwrap()), summary(&exact));
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
        assert!(try_apply(request).is_none());
        assert!(crate::dispatch::try_boolean_op_d(request).is_some());
        assert!(
            try_long_rectangle_xor(BooleanRequestD { clip_type: ClipType::Union, ..request })
                .is_none()
        );
        assert!(
            try_long_rectangle_xor(BooleanRequestD { fill_rule: FillRule::NonZero, ..request })
                .is_none()
        );
        assert!(
            try_long_rectangle_xor(BooleanRequestD { subjects: &rectangles[..7], ..request })
                .is_none()
        );
        let mut with_empty = rectangles.clone();
        with_empty.push(Vec::new());
        assert!(
            try_long_rectangle_xor(BooleanRequestD { subjects: &with_empty, ..request }).is_none()
        );

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
            assert!(axis_aligned_rectangle(&invalid).is_none());
        }
        assert!(axis_aligned_rectangle(&rectangle(0.0, 0.0, 2.0, 2.0)).is_some());
    }

    #[test]
    fn covers_coordinate_grid_and_span_guards() {
        let path = rectangle(0.0, 0.0, 2.0, 2.0);
        let bounds = axis_aligned_rectangle(&path).unwrap();
        assert_eq!(bounds.min_x.value.to_bits(), 0.0_f64.to_bits());
        assert_eq!(bounds.max_y.value.to_bits(), 2.0_f64.to_bits(),);
        assert_eq!(coordinate_index(&[grid_coordinate(0), grid_coordinate(2)], 2), Some(1));
        assert!(coordinate_index(&[grid_coordinate(0), grid_coordinate(2)], 1).is_none());
        assert_eq!(orthogonal_grid_size(10, 20), Some(200));
        assert!(orthogonal_grid_size(usize::MAX, 2).is_none());
        assert!(orthogonal_grid_size(1_001, 1_000).is_none());

        let same = vec![grid_coordinate(0), grid_coordinate(0), grid_coordinate(2)];
        assert_eq!(dedup_grid_coordinates(same).unwrap().len(), 2);
        assert!(
            dedup_grid_coordinates(vec![
                GridCoordinate { key: 0, value: 0.0 },
                GridCoordinate { key: 0, value: 0.25 },
            ])
            .is_none()
        );
        assert!(dedup_grid_coordinates(vec![grid_coordinate(0)]).is_none());

        let mut empty = HorizontalSpanStats::default();
        assert!(!empty.should_fuse());
        let inconsistent = HorizontalSpanStats {
            edge_count: 0,
            total_key_span: 1,
            min_x: Some(0),
            max_x: Some(1),
        };
        assert!(!inconsistent.should_fuse());
        empty.record_rectangle(grid_rectangle(0, 0, 0, 1));
        assert!(!empty.should_fuse());
        let mut short = HorizontalSpanStats::default();
        short.record_rectangle(grid_rectangle(0, 0, 10, 1));
        short.record_rectangle(grid_rectangle(90, 0, 100, 1));
        assert!(!short.should_fuse());
        let mut long = HorizontalSpanStats::default();
        long.record_rectangle(grid_rectangle(0, 0, 90, 1));
        long.record_rectangle(grid_rectangle(10, 0, 100, 1));
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
        let two_edge_cycle = [square[0], directed(PointD::new(1.0, 0.0), PointD::new(0.0, 0.0))];
        assert_eq!(stitch_unique(&two_edge_cycle), Some(Vec::new()));
        let collinear_cycle = [
            directed(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)),
            directed(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)),
            directed(PointD::new(2.0, 0.0), PointD::new(0.0, 0.0)),
        ];
        assert_eq!(stitch_unique(&collinear_cycle), Some(Vec::new()));
        assert!(stitch_unique(&[square[0]]).is_none());
        assert!(stitch_unique(&[square[0], square[0]]).is_none());
        let duplicate_incoming =
            [square[0], directed(PointD::new(2.0, 0.0), PointD::new(1.0, 0.0))];
        assert!(stitch_unique(&duplicate_incoming).is_none());
        let revisited_non_start = [
            directed(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)),
            directed(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)),
            directed(PointD::new(2.0, 0.0), PointD::new(1.0, 0.0)),
        ];
        assert!(stitch_unique(&revisited_non_start).is_none());
    }

    #[test]
    fn sweeps_empty_and_filled_columns() {
        let xs = [grid_coordinate(0), grid_coordinate(1), grid_coordinate(2)];
        let ys = [grid_coordinate(0), grid_coordinate(1), grid_coordinate(2)];
        let events = [GridEvent { x: 0, y0: 0, y1: 2 }, GridEvent { x: 2, y0: 0, y1: 2 }];
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
