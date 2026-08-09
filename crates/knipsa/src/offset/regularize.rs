//! Certified signed-contour regularization for generated offset outlines.

use std::{cmp::Ordering, collections::VecDeque};

use super::{
    BooleanRequest, BoundsD, ClipType, Error, FillRule, MAX_CERTIFIED_CONTOUR_SEGMENTS,
    MAX_CERTIFIED_SEGMENT_CANDIDATES, PathD, PathsD, PointD, SweepSegment, Vector, add_point,
    boolean_op_d, certified_area_sign, certified_orientation, certify_non_zero_contours,
    clean_ring, line_intersection, segments_are_certifiably_disjoint, signed_area2_d,
    sweep_segments_are_adjacent,
};

pub(super) fn generated_contours(
    generated: &[PathD],
    preserve_collinear: bool,
    assume_signed: bool,
) -> Result<PathsD, Error> {
    let mut regularized = Vec::new();
    for path in generated {
        if !assume_signed && certify_non_zero_contours(std::slice::from_ref(path)).is_some() {
            regularized.push(path.clone());
            continue;
        }
        let fill_rule = if signed_area2_d(path).is_sign_positive() {
            FillRule::Positive
        } else {
            FillRule::Negative
        };
        if let Some(rings) = signed_contour(path, fill_rule, preserve_collinear) {
            regularized.extend(rings);
            continue;
        }
        let result = boolean_op_d(BooleanRequest {
            closed_subjects: std::slice::from_ref(path),
            open_subjects: &[],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule,
            limits: crate::ComplexityLimits::DEFAULT,
        })?;
        for ring in result.closed {
            regularized.push(clean_ring(ring, preserve_collinear));
        }
    }
    Ok(regularized)
}

#[derive(Clone, Copy)]
struct CrossingNode {
    parameter: f64,
    point: PointD,
    node: usize,
}

#[derive(Clone, Copy)]
struct BoundaryAtom {
    start: PointD,
    end: PointD,
    source_edge: usize,
    start_node: usize,
    end_node: usize,
}

#[derive(Clone, Copy)]
struct FaceLink {
    neighbor: usize,
    winding_delta: i32,
}

#[derive(Clone, Copy)]
struct NodeFan {
    half_edges: [usize; 4],
    len: usize,
}

impl NodeFan {
    const EMPTY: Self = Self { half_edges: [usize::MAX; 4], len: 0 };

    fn push(&mut self, half_edge: usize) -> Option<()> {
        let slot = self.half_edges.get_mut(self.len)?;
        *slot = half_edge;
        self.len += 1;
        Some(())
    }

    fn as_slice(&self) -> &[usize] {
        &self.half_edges[..self.len]
    }

    fn as_mut_slice(&mut self) -> &mut [usize] {
        &mut self.half_edges[..self.len]
    }
}

/// Extracts the boundary of a single self-crossing contour for its signed fill.
///
/// This deliberately accepts only proper, numerically certified crossings.
/// Contacts, overlaps, multiple crossings at one point, ambiguous predicates,
/// and non-manifold output all defer to the exact arrangement kernel.
pub(super) fn signed_contour(
    path: &[PointD],
    fill_rule: FillRule,
    preserve_collinear: bool,
) -> Option<PathsD> {
    if path.len() < 3 || path.len() > MAX_CERTIFIED_CONTOUR_SEGMENTS {
        return None;
    }
    let (mut nodes, crossing_count) = node_signed_contour(path)?;
    if crossing_count == 0 {
        return None;
    }

    let arrangement = build_arrangement(&mut nodes, crossing_count)?;
    let boundary = select_signed_boundary(&arrangement, path, fill_rule)?;
    stitch_boundary_atoms(&boundary, preserve_collinear)
}

fn build_arrangement(
    nodes: &mut [Vec<CrossingNode>],
    crossing_count: usize,
) -> Option<Vec<BoundaryAtom>> {
    let mut arrangement = Vec::with_capacity(nodes.len() + crossing_count * 2);
    for (edge, edge_nodes) in nodes.iter_mut().enumerate() {
        edge_nodes.sort_unstable_by(|first, second| first.parameter.total_cmp(&second.parameter));
        if edge_nodes
            .windows(2)
            .any(|pair| (pair[1].parameter - pair[0].parameter).abs() <= 32.0 * f64::EPSILON)
        {
            return None;
        }
        for pair in edge_nodes.windows(2) {
            arrangement.push(BoundaryAtom {
                start: pair[0].point,
                end: pair[1].point,
                source_edge: edge,
                start_node: pair[0].node,
                end_node: pair[1].node,
            });
        }
    }
    Some(arrangement)
}

fn node_signed_contour(path: &[PointD]) -> Option<(Vec<Vec<CrossingNode>>, usize)> {
    node_signed_contour_with_limit(path, MAX_CERTIFIED_SEGMENT_CANDIDATES)
}

fn node_signed_contour_with_limit(
    path: &[PointD],
    candidate_limit: usize,
) -> Option<(Vec<Vec<CrossingNode>>, usize)> {
    let mut nodes = (0..path.len())
        .map(|edge| {
            let mut edge_nodes = Vec::with_capacity(6);
            edge_nodes.push(CrossingNode { parameter: 0.0, point: path[edge], node: edge });
            edge_nodes.push(CrossingNode {
                parameter: 1.0,
                point: path[(edge + 1) % path.len()],
                node: (edge + 1) % path.len(),
            });
            edge_nodes
        })
        .collect::<Vec<_>>();
    let mut segments = (0..path.len())
        .map(|edge| {
            let start = path[edge];
            let end = path[(edge + 1) % path.len()];
            SweepSegment {
                ring: 0,
                edge,
                ring_len: path.len(),
                start,
                end,
                bounds: BoundsD::from_segment(start, end),
            }
        })
        .collect::<Vec<_>>();
    segments.sort_unstable_by(compare_sweep_segments);
    let mut active: Vec<usize> = Vec::new();
    let mut candidates = 0_usize;
    let mut crossing_count = 0_usize;
    for current_index in 0..segments.len() {
        let current = segments[current_index];
        active.retain(|index| segments[*index].bounds.max_x >= current.bounds.min_x);
        for other_index in active.iter().copied() {
            let other = segments[other_index];
            if !current.bounds.overlaps(other.bounds) || sweep_segments_are_adjacent(current, other)
            {
                continue;
            }
            let (first, second) = if current.edge < other.edge {
                (current.edge, other.edge)
            } else {
                (other.edge, current.edge)
            };
            candidates += 1;
            if candidates > candidate_limit {
                return None;
            }
            crossing_count += usize::from(add_proper_crossing(
                path,
                first,
                second,
                path.len() + crossing_count,
                &mut nodes,
            )?);
        }
        active.push(current_index);
    }
    Some((nodes, crossing_count))
}

fn compare_sweep_segments(first: &SweepSegment, second: &SweepSegment) -> Ordering {
    for ordering in [
        first.bounds.min_x.total_cmp(&second.bounds.min_x),
        first.bounds.min_y.total_cmp(&second.bounds.min_y),
        first.bounds.max_x.total_cmp(&second.bounds.max_x),
        first.bounds.max_y.total_cmp(&second.bounds.max_y),
        first.edge.cmp(&second.edge),
    ] {
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn add_proper_crossing(
    path: &[PointD],
    first: usize,
    second: usize,
    node: usize,
    nodes: &mut [Vec<CrossingNode>],
) -> Option<bool> {
    let first_end = (first + 1) % path.len();
    let second_end = (second + 1) % path.len();
    let ab_c = certified_orientation(path[first], path[first_end], path[second])?;
    let ab_d = certified_orientation(path[first], path[first_end], path[second_end])?;
    if ab_c == ab_d {
        return Some(false);
    }
    let cd_a = certified_orientation(path[second], path[second_end], path[first])?;
    let cd_b = certified_orientation(path[second], path[second_end], path[first_end])?;
    if cd_a == cd_b {
        return Some(false);
    }
    let first_direction = Vector::from(path[first_end]).sub(Vector::from(path[first]));
    let second_direction = Vector::from(path[second_end]).sub(Vector::from(path[second]));
    let point = line_intersection(path[first], first_direction, path[second], second_direction)?;
    record_crossing(path, first, second, node, point, nodes)
}

fn record_crossing(
    path: &[PointD],
    first: usize,
    second: usize,
    node: usize,
    point: PointD,
    nodes: &mut [Vec<CrossingNode>],
) -> Option<bool> {
    let first_end = (first + 1) % path.len();
    let second_end = (second + 1) % path.len();
    let (first_parameter, second_parameter) = validated_intersection_parameters(
        path[first],
        path[first_end],
        path[second],
        path[second_end],
        point,
    )?;
    nodes[first].push(CrossingNode { parameter: first_parameter, point, node });
    nodes[second].push(CrossingNode { parameter: second_parameter, point, node });
    Some(true)
}

fn parameters_are_strictly_internal(first: f64, second: f64) -> bool {
    0.0 < first && first < 1.0 && 0.0 < second && second < 1.0
}

fn validated_intersection_parameters(
    first: PointD,
    first_end: PointD,
    second: PointD,
    second_end: PointD,
    point: PointD,
) -> Option<(f64, f64)> {
    let first_parameter = segment_parameter(first, first_end, point)?;
    let second_parameter = segment_parameter(second, second_end, point)?;
    parameters_are_strictly_internal(first_parameter, second_parameter)
        .then_some((first_parameter, second_parameter))
}

enum ProbeWinding {
    Retry,
    Accepted(i32, i32),
}

fn classify_probe_winding(left: Option<i32>, right: Option<i32>) -> ProbeWinding {
    let Some(left) = left else {
        return ProbeWinding::Retry;
    };
    let Some(right) = right else {
        return ProbeWinding::Retry;
    };
    if right.checked_add(1) != Some(left) {
        return ProbeWinding::Retry;
    }
    ProbeWinding::Accepted(left, right)
}

fn segment_parameter(start: PointD, end: PointD, point: PointD) -> Option<f64> {
    let x_span = end.x - start.x;
    let y_span = end.y - start.y;
    let parameter = if x_span.abs() >= y_span.abs() {
        (point.x - start.x) / x_span
    } else {
        (point.y - start.y) / y_span
    };
    parameter.is_finite().then_some(parameter)
}

fn certified_winding_pair_across_atom(path: &[PointD], atom: BoundaryAtom) -> Option<(i32, i32)> {
    let direction = Vector::from(atom.end).sub(Vector::from(atom.start));
    let length = direction.length();
    let normal = direction.normalized()?.left();
    for fraction in [0.5, 0.381_966_011_250_105_1, 0.618_033_988_749_894_9] {
        let midpoint = PointD::new(
            atom.start.x + (atom.end.x - atom.start.x) * fraction,
            atom.start.y + (atom.end.y - atom.start.y) * fraction,
        );
        for scale in [0.25, 0.0625, 0.015_625, 0.003_906_25, 0.000_976_562_5] {
            let probe = normal.scale(length * scale);
            let left = add_point(midpoint, probe);
            let right = add_point(midpoint, probe.scale(-1.0));
            if !probe_crosses_only_edge(right, left, path, atom.source_edge) {
                continue;
            }
            if let ProbeWinding::Accepted(left, right) = classify_probe_winding(
                certified_winding(left, path),
                certified_winding(right, path),
            ) {
                return Some((left, right));
            }
        }
    }
    None
}

fn select_signed_boundary(
    arrangement: &[BoundaryAtom],
    path: &[PointD],
    fill_rule: FillRule,
) -> Option<Vec<BoundaryAtom>> {
    let (face_of, face_seeds) = build_float_face_cycles(arrangement)?;
    let mut degrees = vec![0_usize; face_seeds.len()];
    for index in 0..arrangement.len() {
        let left = face_of[index * 2];
        let right = face_of[index * 2 + 1];
        if left == right {
            return None;
        }
        degrees[left] = degrees[left].checked_add(1)?;
        degrees[right] = degrees[right].checked_add(1)?;
    }
    let mut offsets = Vec::with_capacity(degrees.len() + 1);
    offsets.push(0_usize);
    for degree in degrees {
        offsets.push(offsets.last()?.checked_add(degree)?);
    }
    let mut links = vec![FaceLink { neighbor: 0, winding_delta: 0 }; *offsets.last()?];
    let mut cursors = offsets[..offsets.len() - 1].to_vec();
    for index in 0..arrangement.len() {
        let left = face_of[index * 2];
        let right = face_of[index * 2 + 1];
        links[cursors[left]] = FaceLink { neighbor: right, winding_delta: -1 };
        cursors[left] += 1;
        links[cursors[right]] = FaceLink { neighbor: left, winding_delta: 1 };
        cursors[right] += 1;
    }

    let mut labels = vec![None; face_seeds.len()];
    for &seed_half_edge in &face_seeds {
        if labels[face_of[seed_half_edge]].is_some() {
            continue;
        }
        let atom_index = seed_half_edge / 2;
        let (left_winding, right_winding) =
            certified_winding_pair_across_atom(path, arrangement[atom_index])?;
        let left = face_of[atom_index * 2];
        let right = face_of[atom_index * 2 + 1];
        let mut queue = VecDeque::new();
        assign_float_face_label(&mut labels, left, left_winding, &mut queue)?;
        assign_float_face_label(&mut labels, right, right_winding, &mut queue)?;
        while let Some(face) = queue.pop_front() {
            let winding = labels[face]?;
            for link in &links[offsets[face]..offsets[face + 1]] {
                propagate_face_link(&mut labels, winding, *link, &mut queue)?;
            }
        }
    }

    let included = |winding: i32| match fill_rule {
        FillRule::Positive => Some(winding > 0),
        FillRule::Negative => Some(winding < 0),
        FillRule::EvenOdd | FillRule::NonZero => None,
    };
    let mut boundary = Vec::new();
    for (index, atom) in arrangement.iter().copied().enumerate() {
        let left = included(labels[face_of[index * 2]]?)?;
        let right = included(labels[face_of[index * 2 + 1]]?)?;
        if left != right {
            boundary.push(if left {
                atom
            } else {
                BoundaryAtom {
                    start: atom.end,
                    end: atom.start,
                    source_edge: atom.source_edge,
                    start_node: atom.end_node,
                    end_node: atom.start_node,
                }
            });
        }
    }
    Some(boundary)
}

fn propagate_face_link(
    labels: &mut [Option<i32>],
    winding: i32,
    link: FaceLink,
    queue: &mut VecDeque<usize>,
) -> Option<()> {
    assign_float_face_label(labels, link.neighbor, winding.checked_add(link.winding_delta)?, queue)
}

fn assign_float_face_label(
    labels: &mut [Option<i32>],
    face: usize,
    winding: i32,
    queue: &mut VecDeque<usize>,
) -> Option<()> {
    if let Some(current) = labels[face] {
        (current == winding).then_some(())
    } else {
        labels[face] = Some(winding);
        queue.push_back(face);
        Some(())
    }
}

fn build_float_face_cycles(edges: &[BoundaryAtom]) -> Option<(Vec<usize>, Vec<usize>)> {
    let half_edge_count = edges.len().checked_mul(2)?;
    let node_count =
        edges.iter().map(|edge| edge.start_node.max(edge.end_node)).max()?.checked_add(1)?;
    let mut fans = vec![NodeFan::EMPTY; node_count];
    for (index, edge) in edges.iter().enumerate() {
        for (node, half_edge) in [(edge.start_node, index * 2), (edge.end_node, index * 2 + 1)] {
            fans[node].push(half_edge)?;
        }
    }
    let mut fan_position = vec![usize::MAX; half_edge_count];
    for fan in &mut fans {
        fan.as_mut_slice()
            .sort_unstable_by(|left, right| compare_float_half_edge_angle(edges, *left, *right));
        for pair in fan.as_slice().windows(2) {
            if float_half_edges_share_ray(edges, pair[0], pair[1]) {
                return None;
            }
        }
        for (position, &half_edge) in fan.as_slice().iter().enumerate() {
            fan_position[half_edge] = position;
        }
    }

    let mut next = vec![usize::MAX; half_edge_count];
    for (half_edge, next_edge) in next.iter_mut().enumerate() {
        let edge = edges[half_edge / 2];
        let node = if half_edge & 1 == 0 { edge.end_node } else { edge.start_node };
        let candidates = fans[node].as_slice();
        let twin_position = fan_position[half_edge ^ 1];
        *next_edge = candidates[(twin_position + candidates.len() - 1) % candidates.len()];
    }
    let mut face_of = vec![usize::MAX; half_edge_count];
    let mut face_seeds = Vec::new();
    for start in 0..half_edge_count {
        if face_of[start] != usize::MAX {
            continue;
        }
        let face = face_seeds.len();
        face_seeds.push(start);
        let mut current = start;
        loop {
            if face_of[current] != usize::MAX {
                debug_assert_eq!(current, start);
                break;
            }
            face_of[current] = face;
            current = next[current];
        }
    }
    Some((face_of, face_seeds))
}

fn float_half_edge_start(edges: &[BoundaryAtom], half_edge: usize) -> PointD {
    let edge = edges[half_edge / 2];
    if half_edge & 1 == 0 { edge.start } else { edge.end }
}

fn float_half_edge_end(edges: &[BoundaryAtom], half_edge: usize) -> PointD {
    let edge = edges[half_edge / 2];
    if half_edge & 1 == 0 { edge.end } else { edge.start }
}

fn float_half_edge_direction(edges: &[BoundaryAtom], half_edge: usize) -> Vector {
    Vector::from(float_half_edge_end(edges, half_edge))
        .sub(Vector::from(float_half_edge_start(edges, half_edge)))
}

fn compare_float_half_edge_angle(edges: &[BoundaryAtom], first: usize, second: usize) -> Ordering {
    let first = float_half_edge_direction(edges, first);
    let second = float_half_edge_direction(edges, second);
    let upper = |direction: Vector| direction.y > 0.0 || (direction.y == 0.0 && direction.x >= 0.0);
    let hemisphere = upper(second).cmp(&upper(first));
    if hemisphere != Ordering::Equal {
        return hemisphere;
    }
    let cross = second.cross(first).total_cmp(&0.0);
    if cross != Ordering::Equal {
        return cross;
    }
    first.length().total_cmp(&second.length())
}

fn float_half_edges_share_ray(edges: &[BoundaryAtom], first: usize, second: usize) -> bool {
    let origin = float_half_edge_start(edges, first);
    let first_end = float_half_edge_end(edges, first);
    let second_end = float_half_edge_end(edges, second);
    Vector::from(first_end)
        .sub(Vector::from(origin))
        .dot(Vector::from(second_end).sub(Vector::from(origin)))
        > 0.0
        && certified_orientation(origin, first_end, second_end).is_none()
}

fn probe_crosses_only_edge(start: PointD, end: PointD, path: &[PointD], crossed: usize) -> bool {
    (0..path.len()).filter(|edge| *edge != crossed).all(|edge| {
        segments_are_certifiably_disjoint(start, end, path[edge], path[(edge + 1) % path.len()])
    })
}

fn certified_winding(point: PointD, path: &[PointD]) -> Option<i32> {
    let mut winding = 0_i32;
    for edge in 0..path.len() {
        let start = path[edge];
        let end = path[(edge + 1) % path.len()];
        if start.y <= point.y {
            if end.y > point.y && certified_orientation(start, end, point)? > 0 {
                winding = winding.checked_add(1)?;
            }
        } else if end.y <= point.y && certified_orientation(start, end, point)? < 0 {
            winding = winding.checked_sub(1)?;
        }
    }
    Some(winding)
}

fn stitch_boundary_atoms(atoms: &[BoundaryAtom], preserve_collinear: bool) -> Option<PathsD> {
    if atoms.is_empty() {
        return Some(Vec::new());
    }
    let node_count =
        atoms.iter().map(|atom| atom.start_node.max(atom.end_node)).max()?.checked_add(1)?;
    let mut outgoing = vec![usize::MAX; node_count];
    let mut incoming = vec![usize::MAX; node_count];
    for (index, atom) in atoms.iter().enumerate() {
        if atom.start_node == atom.end_node
            || outgoing[atom.start_node] != usize::MAX
            || incoming[atom.end_node] != usize::MAX
        {
            return None;
        }
        outgoing[atom.start_node] = index;
        incoming[atom.end_node] = index;
    }
    let mut visited = vec![false; atoms.len()];
    let mut rings = Vec::new();
    for seed in 0..atoms.len() {
        if visited[seed] {
            continue;
        }
        let first_node = atoms[seed].start_node;
        let mut ring = Vec::new();
        let mut current = seed;
        loop {
            debug_assert!(!visited[current]);
            visited[current] = true;
            ring.push(atoms[current].start);
            let end_node = atoms[current].end_node;
            if end_node == first_node {
                break;
            }
            let next = outgoing[end_node];
            if next == usize::MAX {
                return None;
            }
            current = next;
        }
        let ring = clean_ring(ring, preserve_collinear);
        if ring.len() < 3 || certified_area_sign(&ring).is_none() {
            return None;
        }
        rings.push(ring);
    }
    Some(rings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle() -> PathD {
        vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
        ]
    }

    fn atom(
        start: PointD,
        end: PointD,
        source_edge: usize,
        start_node: usize,
        end_node: usize,
    ) -> BoundaryAtom {
        BoundaryAtom { start, end, source_edge, start_node, end_node }
    }

    fn rectangle_atoms() -> Vec<BoundaryAtom> {
        let path = rectangle();
        (0..path.len())
            .map(|edge| {
                atom(path[edge], path[(edge + 1) % path.len()], edge, edge, (edge + 1) % path.len())
            })
            .collect()
    }

    #[test]
    fn generated_contours_cover_simple_and_exact_fallbacks() {
        let path = rectangle();
        assert_eq!(
            generated_contours(std::slice::from_ref(&path), false, false),
            Ok(vec![path.clone()])
        );
        assert_eq!(
            generated_contours(std::slice::from_ref(&path), false, true),
            Ok(vec![path.clone()])
        );

        let mut reversed = path.clone();
        reversed.reverse();
        let negative = generated_contours(std::slice::from_ref(&reversed), true, true).unwrap();
        assert_eq!(negative.len(), 1);
        assert_eq!(negative[0].len(), 4);
    }

    #[test]
    fn signed_contour_and_noding_fail_closed() {
        assert_eq!(signed_contour(&[], FillRule::Positive, false), None);
        assert_eq!(signed_contour(&rectangle(), FillRule::Positive, false), None);
        let oversized = vec![PointD::new(0.0, 0.0); MAX_CERTIFIED_CONTOUR_SEGMENTS + 1];
        assert_eq!(signed_contour(&oversized, FillRule::Positive, false), None);

        let bow_tie = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
        ];
        assert!(node_signed_contour_with_limit(&bow_tie, 0).is_none());
        assert!(node_signed_contour(&bow_tie).is_some());

        let touching = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(5.0, 0.0),
            PointD::new(5.0, 5.0),
        ];
        assert!(node_signed_contour(&touching).is_none());
    }

    #[test]
    fn graph_helpers_reject_non_manifold_inputs() {
        let mut fan = NodeFan::EMPTY;
        for half_edge in 0..4 {
            assert_eq!(fan.push(half_edge), Some(()));
        }
        assert_eq!(fan.push(4), None);

        let segment = SweepSegment {
            ring: 0,
            edge: 0,
            ring_len: 4,
            start: PointD::new(0.0, 0.0),
            end: PointD::new(1.0, 0.0),
            bounds: BoundsD::from_segment(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)),
        };
        assert_eq!(compare_sweep_segments(&segment, &segment), Ordering::Equal);
        assert_eq!(segment_parameter(segment.start, segment.end, PointD::new(0.5, 0.0)), Some(0.5));
        assert_eq!(
            segment_parameter(PointD::new(0.0, 0.0), PointD::new(0.0, 2.0), PointD::new(0.0, 1.0)),
            Some(0.5)
        );
        assert_eq!(
            segment_parameter(PointD::new(0.0, 0.0), PointD::new(0.0, 0.0), PointD::new(0.0, 0.0)),
            None
        );
        assert!(!parameters_are_strictly_internal(-0.1, 0.5));
        assert!(!parameters_are_strictly_internal(1.1, 0.5));
        assert!(!parameters_are_strictly_internal(0.5, -0.1));
        assert!(!parameters_are_strictly_internal(0.5, 1.1));
        assert_eq!(
            validated_intersection_parameters(
                PointD::new(0.0, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(0.0, -1.0),
                PointD::new(0.0, 1.0),
                PointD::new(2.0, 0.0),
            ),
            None
        );
        let crossing_path = vec![
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, -1.0),
            PointD::new(0.0, 1.0),
        ];
        let mut crossing_nodes = vec![Vec::new(); crossing_path.len()];
        assert_eq!(
            record_crossing(&crossing_path, 0, 2, 4, PointD::new(2.0, 0.0), &mut crossing_nodes,),
            None
        );

        let duplicate_node = CrossingNode { parameter: 0.5, point: PointD::new(0.5, 0.0), node: 2 };
        let mut duplicate_nodes = vec![vec![
            CrossingNode { parameter: 0.0, point: segment.start, node: 0 },
            duplicate_node,
            duplicate_node,
            CrossingNode { parameter: 1.0, point: segment.end, node: 1 },
        ]];
        assert!(build_arrangement(&mut duplicate_nodes, 1).is_none());

        let shared_ray = vec![
            atom(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), 0, 0, 1),
            atom(PointD::new(0.0, 0.0), PointD::new(2.0, 0.0), 1, 0, 2),
        ];
        assert!(build_float_face_cycles(&shared_ray).is_none());
        assert_eq!(compare_float_half_edge_angle(&shared_ray, 0, 2), Ordering::Less);
        assert!(
            select_signed_boundary(&shared_ray[..1], &rectangle(), FillRule::Positive).is_none()
        );

        assert_eq!(stitch_boundary_atoms(&[], false), Some(Vec::new()));
        let self_loop = atom(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), 0, 0, 0);
        assert!(stitch_boundary_atoms(&[self_loop], false).is_none());
        let duplicate = vec![shared_ray[0], shared_ray[1]];
        assert!(stitch_boundary_atoms(&duplicate, false).is_none());
        let duplicate_incoming = vec![
            atom(PointD::new(0.0, 0.0), PointD::new(2.0, 0.0), 0, 0, 2),
            atom(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0), 1, 1, 2),
        ];
        assert!(stitch_boundary_atoms(&duplicate_incoming, false).is_none());
        assert!(stitch_boundary_atoms(&shared_ray[..1], false).is_none());
        let two_edge_cycle =
            vec![shared_ray[0], atom(PointD::new(1.0, 0.0), PointD::new(0.0, 0.0), 1, 1, 0)];
        assert!(stitch_boundary_atoms(&two_edge_cycle, false).is_none());
        let collinear_cycle = vec![
            atom(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), 0, 0, 1),
            atom(PointD::new(1.0, 0.0), PointD::new(2.0, 0.0), 1, 1, 2),
            atom(PointD::new(2.0, 0.0), PointD::new(0.0, 0.0), 2, 2, 0),
        ];
        assert!(stitch_boundary_atoms(&collinear_cycle, true).is_none());
    }

    #[test]
    fn face_labels_cover_supported_and_rejected_fill_rules() {
        let path = rectangle();
        let arrangement = rectangle_atoms();
        assert!(select_signed_boundary(&arrangement, &path, FillRule::Positive).is_some());
        assert!(select_signed_boundary(&arrangement, &path, FillRule::EvenOdd).is_none());

        assert!(matches!(classify_probe_winding(None, Some(0)), ProbeWinding::Retry));
        assert!(matches!(classify_probe_winding(Some(1), None), ProbeWinding::Retry));
        assert!(matches!(classify_probe_winding(Some(3), Some(0)), ProbeWinding::Retry));
        assert!(matches!(classify_probe_winding(Some(1), Some(0)), ProbeWinding::Accepted(1, 0)));
        assert!(matches!(classify_probe_winding(Some(0), Some(i32::MAX)), ProbeWinding::Retry));

        let blocked_path = vec![
            PointD::new(-1.0, -0.01),
            PointD::new(1.0, -0.01),
            PointD::new(1.0, 0.01),
            PointD::new(-1.0, 0.01),
        ];
        let blocked_atom = atom(PointD::new(-50.0, 0.0), PointD::new(50.0, 0.0), 0, 0, 1);
        assert_eq!(certified_winding_pair_across_atom(&blocked_path, blocked_atom), None);

        let mut labels = [None, None];
        let mut queue = VecDeque::new();
        assert_eq!(assign_float_face_label(&mut labels, 0, 1, &mut queue), Some(()));
        assert_eq!(assign_float_face_label(&mut labels, 0, 1, &mut queue), Some(()));
        assert_eq!(assign_float_face_label(&mut labels, 0, 2, &mut queue), None);
        assert_eq!(
            propagate_face_link(
                &mut labels,
                0,
                FaceLink { neighbor: 0, winding_delta: 1 },
                &mut queue,
            ),
            Some(())
        );
        assert_eq!(
            propagate_face_link(
                &mut labels,
                0,
                FaceLink { neighbor: 0, winding_delta: 2 },
                &mut queue,
            ),
            None
        );
    }
}
