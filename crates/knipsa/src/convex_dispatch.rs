//! Certified front-end kernels for large, ordinary convex pairs.

use std::cmp::Ordering;

use crate::{BooleanRequestD, ClipType, FillRule, PathD, PathsD, PointD};

const KEY_SCALE: f64 = 1_000_000_000.0;
const MAX_COORDINATE: f64 = 1_000_000.0;
const MIN_VERTEX_COUNT: usize = 16;
const PREDICATE_TOLERANCE: f64 = 1.0e-12;
const MAX_CHAINS: usize = 4;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PointKey {
    x: i64,
    y: i64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Crossing {
    subject_edge: usize,
    clip_edge: usize,
    subject_parameter: f64,
    clip_parameter: f64,
    point: PointD,
    key: PointKey,
}

#[derive(Clone, Copy, Debug, Default)]
struct Position {
    edge: usize,
    parameter: f64,
    crossing: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct Arc {
    subject: bool,
    start: Position,
    end: Position,
    wraps: bool,
    inside_other: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Chain {
    arc: Arc,
    reversed: bool,
    start_crossing: usize,
    end_crossing: usize,
}

/// Resolves large, strictly convex pairs with exactly two proper crossings.
///
/// Any ambiguity is reported as `None`, preserving the conservative dispatcher
/// and exact arrangement as the source of truth for unsupported topology.
pub(super) fn try_boolean_opd(request: BooleanRequestD<'_>) -> Option<PathsD> {
    if request.subjects.len() != 1
        || request.clips.len() != 1
        || !matches!(request.fill_rule, FillRule::EvenOdd | FillRule::NonZero | FillRule::Positive)
    {
        return None;
    }

    let subject = request.subjects[0].as_slice();
    let clip = request.clips[0].as_slice();
    if !certified_positive_convex(subject) || !certified_positive_convex(clip) {
        return None;
    }

    let crossings = find_two_crossings(subject, clip)?;
    let subject_arcs = build_arcs(subject, clip, &crossings, true)?;
    let clip_arcs = build_arcs(clip, subject, &crossings, false)?;

    let mut chains = [Chain::default(); MAX_CHAINS];
    let mut chain_count = 0;
    for arc in subject_arcs {
        select_chain(arc, request.clip_type, &mut chains, &mut chain_count)?;
    }
    for arc in clip_arcs {
        select_chain(arc, request.clip_type, &mut chains, &mut chain_count)?;
    }

    stitch_chains(&chains[..chain_count], subject, clip, &crossings)
}

fn certified_positive_convex(path: &[PointD]) -> bool {
    if path.len() < MIN_VERTEX_COUNT {
        return false;
    }

    let Some(mut previous_key) = key(path[path.len() - 1]) else {
        return false;
    };
    for index in 0..path.len() {
        let previous = path[(index + path.len() - 1) % path.len()];
        let current = path[index];
        let next = path[(index + 1) % path.len()];
        let Some(current_key) = key(current) else {
            return false;
        };
        let duplicate_key = previous_key == current_key;
        previous_key = current_key;

        let incoming = subtract(current, previous);
        let outgoing = subtract(next, current);
        if duplicate_key || robust_cross_order(incoming, outgoing) != Some(Ordering::Greater) {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_lines)]
fn find_two_crossings(subject: &[PointD], clip: &[PointD]) -> Option<[Crossing; 2]> {
    let mut crossings = [Crossing::default(); 2];
    let mut crossing_count = 0;
    let mut subject_index = 0;
    let mut clip_index = 0;
    let mut subject_previous = subject[subject.len() - 1];
    let mut clip_previous = clip[clip.len() - 1];
    let mut subject_vector = subtract(subject[subject_index], subject_previous);
    let mut clip_vector = subtract(clip[clip_index], clip_previous);
    let mut subject_steps = 0;
    let mut clip_steps = 0;
    let limit = 2 * (subject.len() + clip.len()) + 1;

    for _ in 0..limit {
        let subject_edge = (subject_index + subject.len() - 1) % subject.len();
        let clip_edge = (clip_index + clip.len() - 1) % clip.len();
        if collinear_edges_overlap(
            subject_previous,
            subject[subject_index],
            clip_previous,
            clip[clip_index],
        ) {
            return None;
        }

        if let Some((point, subject_parameter, clip_parameter)) = segment_intersection(
            subject_previous,
            subject[subject_index],
            clip_previous,
            clip[clip_index],
        ) {
            let endpoint_distance = subject_parameter
                .min(1.0 - subject_parameter)
                .min(clip_parameter.min(1.0 - clip_parameter));
            if endpoint_distance <= PREDICATE_TOLERANCE {
                return None;
            }
            let point_key = key(point)?;
            let crossing_index =
                (!crossings[..crossing_count].iter().any(|crossing| crossing.key == point_key))
                    .then_some(crossing_count)?;
            let crossing = crossings.get_mut(crossing_index)?;
            *crossing = Crossing {
                subject_edge,
                clip_edge,
                subject_parameter,
                clip_parameter,
                point,
                key: point_key,
            };
            crossing_count += 1;
        }

        if cross(clip_vector, subject_vector) > 0.0 {
            if cross(clip_vector, subtract(subject[subject_index], clip_previous)) >= 0.0 {
                clip_previous = clip[clip_index];
                clip_index = (clip_index + 1) % clip.len();
                clip_vector = subtract(clip[clip_index], clip_previous);
                clip_steps += 1;
            } else {
                subject_previous = subject[subject_index];
                subject_index = (subject_index + 1) % subject.len();
                subject_vector = subtract(subject[subject_index], subject_previous);
                subject_steps += 1;
            }
        } else if cross(subject_vector, subtract(clip[clip_index], subject_previous)) >= 0.0 {
            subject_previous = subject[subject_index];
            subject_index = (subject_index + 1) % subject.len();
            subject_vector = subtract(subject[subject_index], subject_previous);
            subject_steps += 1;
        } else {
            clip_previous = clip[clip_index];
            clip_index = (clip_index + 1) % clip.len();
            clip_vector = subtract(clip[clip_index], clip_previous);
            clip_steps += 1;
        }

        if subject_steps >= subject.len() && clip_steps >= clip.len() {
            break;
        }
    }

    (crossing_count == crossings.len()).then_some(crossings)
}

fn crossing_position(crossing: Crossing, crossing_index: usize, subject: bool) -> Position {
    if subject {
        Position {
            edge: crossing.subject_edge,
            parameter: crossing.subject_parameter,
            crossing: crossing_index,
        }
    } else {
        Position {
            edge: crossing.clip_edge,
            parameter: crossing.clip_parameter,
            crossing: crossing_index,
        }
    }
}

fn position_after(first: Position, second: Position) -> bool {
    first.edge > second.edge || (first.edge == second.edge && first.parameter > second.parameter)
}

fn build_arcs(
    path: &[PointD],
    other: &[PointD],
    crossings: &[Crossing; 2],
    subject: bool,
) -> Option<[Arc; 2]> {
    let mut positions =
        [crossing_position(crossings[0], 0, subject), crossing_position(crossings[1], 1, subject)];
    if position_after(positions[0], positions[1]) {
        positions.swap(0, 1);
    }

    let inside_other = forward_arc_enters_other(positions[0], path, other, crossings, subject)?;
    Some([
        Arc { subject, start: positions[0], end: positions[1], wraps: false, inside_other },
        Arc {
            subject,
            start: positions[1],
            end: positions[0],
            wraps: true,
            inside_other: !inside_other,
        },
    ])
}

fn forward_arc_enters_other(
    start: Position,
    path: &[PointD],
    other: &[PointD],
    crossings: &[Crossing; 2],
    subject: bool,
) -> Option<bool> {
    let crossing = crossings[start.crossing];
    let other_edge = if subject { crossing.clip_edge } else { crossing.subject_edge };
    let path_vector = subtract(path[(start.edge + 1) % path.len()], path[start.edge]);
    let other_vector = subtract(other[(other_edge + 1) % other.len()], other[other_edge]);
    robust_cross_order(other_vector, path_vector).map(|order| order == Ordering::Greater)
}

fn select_chain(
    arc: Arc,
    clip_type: ClipType,
    chains: &mut [Chain; MAX_CHAINS],
    chain_count: &mut usize,
) -> Option<()> {
    let (left, right) = if arc.subject {
        (
            apply_operation(true, arc.inside_other, clip_type),
            apply_operation(false, arc.inside_other, clip_type),
        )
    } else {
        (
            apply_operation(arc.inside_other, true, clip_type),
            apply_operation(arc.inside_other, false, clip_type),
        )
    };
    if left == right {
        return Some(());
    }

    let reversed = !left;
    let chain = chains.get_mut(*chain_count)?;
    *chain = Chain {
        arc,
        reversed,
        start_crossing: if reversed { arc.end.crossing } else { arc.start.crossing },
        end_crossing: if reversed { arc.start.crossing } else { arc.end.crossing },
    };
    *chain_count += 1;
    Some(())
}

fn stitch_chains(
    chains: &[Chain],
    subject: &[PointD],
    clip: &[PointD],
    crossings: &[Crossing; 2],
) -> Option<PathsD> {
    if chains.is_empty() {
        return None;
    }

    let mut next = [0_usize; MAX_CHAINS];
    for (index, chain) in chains.iter().enumerate() {
        let mut candidates = [0_usize; MAX_CHAINS];
        let mut candidate_count = 0;
        for (candidate, outgoing) in chains.iter().enumerate() {
            if outgoing.start_crossing == chain.end_crossing {
                candidates[candidate_count] = candidate;
                candidate_count += 1;
            }
        }
        if candidate_count == 0 {
            return None;
        }
        if candidate_count == 1 {
            next[index] = candidates[0];
            continue;
        }

        candidates[..candidate_count].sort_unstable_by(|left, right| {
            compare_angle(
                first_vector(chains[*left], subject, clip, crossings),
                first_vector(chains[*right], subject, clip, crossings),
            )
        });
        let reverse = reverse_incoming_vector(*chain, subject, clip, crossings);
        let insertion = candidates[..candidate_count]
            .iter()
            .position(|candidate| {
                compare_angle(first_vector(chains[*candidate], subject, clip, crossings), reverse)
                    != Ordering::Less
            })
            .unwrap_or(candidate_count);
        next[index] = candidates[(insertion + candidate_count - 1) % candidate_count];
    }

    let mut visited = [false; MAX_CHAINS];
    let mut paths = Vec::with_capacity(2);
    for start in 0..chains.len() {
        if visited[start] {
            continue;
        }
        let mut path = Vec::with_capacity(subject.len() + clip.len() + 2);
        let mut current = start;
        loop {
            if visited[current] {
                if current != start {
                    return None;
                }
                break;
            }
            visited[current] = true;
            append_chain(&mut path, chains[current], subject, clip, crossings);
            current = next[current];
        }
        if path.len() < 3 {
            return None;
        }
        if area2(&path) <= f64::EPSILON {
            return None;
        }
        canonicalize(&mut path);
        paths.push(path);
    }
    paths.sort_by(compare_paths);
    Some(paths)
}

fn append_chain(
    output: &mut PathD,
    chain: Chain,
    subject: &[PointD],
    clip: &[PointD],
    crossings: &[Crossing; 2],
) {
    let path = if chain.arc.subject { subject } else { clip };
    if chain.reversed {
        append_reverse(output, chain.arc, path, crossings);
    } else {
        append_forward(output, chain.arc, path, crossings);
    }
}

fn append_forward(output: &mut PathD, arc: Arc, path: &[PointD], crossings: &[Crossing; 2]) {
    output.push(crossings[arc.start.crossing].point);
    let vertex_count = forward_vertex_count(arc, path.len());
    for offset in 1..=vertex_count {
        output.push(path[(arc.start.edge + offset) % path.len()]);
    }
}

fn append_reverse(output: &mut PathD, arc: Arc, path: &[PointD], crossings: &[Crossing; 2]) {
    output.push(crossings[arc.end.crossing].point);
    let vertex_count = forward_vertex_count(arc, path.len());
    for offset in 0..vertex_count {
        output.push(path[(arc.end.edge + path.len() - offset) % path.len()]);
    }
}

fn first_vector(
    chain: Chain,
    subject: &[PointD],
    clip: &[PointD],
    crossings: &[Crossing; 2],
) -> PointD {
    let path = if chain.arc.subject { subject } else { clip };
    if chain.reversed {
        subtract(
            previous_forward_point(chain.arc, path, crossings),
            crossings[chain.arc.end.crossing].point,
        )
    } else {
        subtract(
            next_forward_point(chain.arc, path, crossings),
            crossings[chain.arc.start.crossing].point,
        )
    }
}

fn reverse_incoming_vector(
    chain: Chain,
    subject: &[PointD],
    clip: &[PointD],
    crossings: &[Crossing; 2],
) -> PointD {
    let path = if chain.arc.subject { subject } else { clip };
    if chain.reversed {
        subtract(
            next_forward_point(chain.arc, path, crossings),
            crossings[chain.end_crossing].point,
        )
    } else {
        subtract(
            previous_forward_point(chain.arc, path, crossings),
            crossings[chain.end_crossing].point,
        )
    }
}

fn next_forward_point(arc: Arc, path: &[PointD], crossings: &[Crossing; 2]) -> PointD {
    if forward_vertex_count(arc, path.len()) == 0 {
        crossings[arc.end.crossing].point
    } else {
        path[(arc.start.edge + 1) % path.len()]
    }
}

fn previous_forward_point(arc: Arc, path: &[PointD], crossings: &[Crossing; 2]) -> PointD {
    if forward_vertex_count(arc, path.len()) == 0 {
        crossings[arc.start.crossing].point
    } else {
        path[arc.end.edge]
    }
}

fn forward_vertex_count(arc: Arc, path_len: usize) -> usize {
    match arc.start.edge.cmp(&arc.end.edge) {
        Ordering::Equal => usize::from(arc.wraps) * path_len,
        Ordering::Less => arc.end.edge - arc.start.edge,
        Ordering::Greater => path_len - arc.start.edge + arc.end.edge,
    }
}

fn segment_intersection(
    first_start: PointD,
    first_end: PointD,
    second_start: PointD,
    second_end: PointD,
) -> Option<(PointD, f64, f64)> {
    let first_vector = subtract(first_end, first_start);
    let second_vector = subtract(second_end, second_start);
    let denominator = cross(first_vector, second_vector);
    robust_cross_order(first_vector, second_vector)?;
    let between = subtract(second_start, first_start);
    let first_parameter = cross(between, second_vector) / denominator;
    let second_parameter = cross(between, first_vector) / denominator;
    if !in_unit_interval(first_parameter) || !in_unit_interval(second_parameter) {
        return None;
    }
    Some((
        PointD::new(
            first_vector.x.mul_add(first_parameter, first_start.x),
            first_vector.y.mul_add(first_parameter, first_start.y),
        ),
        first_parameter,
        second_parameter,
    ))
}

fn collinear_edges_overlap(
    first_start: PointD,
    first_end: PointD,
    second_start: PointD,
    second_end: PointD,
) -> bool {
    robust_cross_order(subtract(first_end, first_start), subtract(second_end, second_start))
        .is_none()
        && robust_cross_order(subtract(second_start, first_start), subtract(first_end, first_start))
            .is_none()
        && first_start.x.min(first_end.x) <= second_start.x.max(second_end.x)
        && second_start.x.min(second_end.x) <= first_start.x.max(first_end.x)
        && first_start.y.min(first_end.y) <= second_start.y.max(second_end.y)
        && second_start.y.min(second_end.y) <= first_start.y.max(first_end.y)
}

fn in_unit_interval(value: f64) -> bool {
    (-PREDICATE_TOLERANCE..=(1.0 + PREDICATE_TOLERANCE)).contains(&value)
}

fn apply_operation(subject: bool, clip: bool, clip_type: ClipType) -> bool {
    match clip_type {
        ClipType::Intersection => subject && clip,
        ClipType::Union => subject || clip,
        ClipType::Difference => subject && !clip,
        ClipType::Xor => subject != clip,
    }
}

fn compare_angle(first: PointD, second: PointD) -> Ordering {
    let first_upper = first.y > 0.0 || (first.y.abs() <= f64::EPSILON && first.x >= 0.0);
    let second_upper = second.y > 0.0 || (second.y.abs() <= f64::EPSILON && second.x >= 0.0);
    if first_upper != second_upper {
        return second_upper.cmp(&first_upper);
    }
    let turn = cross(first, second);
    if turn.abs() > f64::EPSILON {
        return if turn > 0.0 { Ordering::Less } else { Ordering::Greater };
    }
    let first_length = first.x.mul_add(first.x, first.y * first.y);
    let second_length = second.x.mul_add(second.x, second.y * second.y);
    first_length.total_cmp(&second_length)
}

fn robust_cross_order(first: PointD, second: PointD) -> Option<Ordering> {
    let value = cross(first, second);
    let scale = first.x.abs() * second.y.abs() + first.y.abs() * second.x.abs();
    let tolerance = PREDICATE_TOLERANCE * scale.max(1.0);
    if value.abs() <= tolerance { None } else { Some(value.total_cmp(&0.0)) }
}

fn subtract(first: PointD, second: PointD) -> PointD {
    PointD::new(first.x - second.x, first.y - second.y)
}

fn cross(first: PointD, second: PointD) -> f64 {
    first.x * second.y - first.y * second.x
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
        .unwrap_or(left.len().cmp(&right.len()))
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
#[path = "convex_dispatch/tests.rs"]
mod tests;
