//! Polygon and polyline offsetting.
//!
//! The construction is deliberately independent from the boolean kernel. Each
//! input edge is shifted in floating point, joins and caps are constructed
//! explicitly, and the generated outlines are then passed through the exact
//! boolean union. That last step removes the negative slivers and
//! self-overlaps that are unavoidable when offsetting concave paths.

use crate::{
    BooleanRequestD, ClipType, Error, FillRule, Path64, PathD, PathKind, Paths64, PathsD, Point64,
    PointD, boolean_opd, normalize_pathd, validate_pathd,
};

const EPSILON: f64 = 1e-12;
const ARC_TOLERANCE_RATIO: f64 = 0.002;
const MAX_ARC_STEPS: usize = 4096;

/// The treatment of corners when a path is offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum JoinType {
    /// A square corner whose outer extent is capped at roughly two radii.
    Square = 0,
    /// A straight cut between the two offset edges.
    Bevel = 1,
    /// A circular arc around the source vertex.
    Round = 2,
    /// The intersection of the two offset edges, subject to `miter_limit`.
    Miter = 3,
}

/// The treatment of the ends of an open path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EndType {
    /// Treat each path as a closed polygon.
    Polygon = 0,
    /// Join the two sides without treating the path as a stroked line.
    Joined = 1,
    /// Stop at the endpoint without extending it.
    Butt = 2,
    /// Extend the endpoint by one offset radius.
    Square = 3,
    /// Add a semicircular endpoint cap.
    Round = 4,
}

/// Options controlling an offset operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetOptions {
    /// Corner style.
    pub join_type: JoinType,
    /// Endpoint style for open paths.
    pub end_type: EndType,
    /// Maximum miter length divided by the absolute offset distance.
    pub miter_limit: f64,
    /// Maximum deviation of a round join from its ideal circle. Zero selects
    /// a scale-relative default of `abs(delta) / 500`.
    pub arc_tolerance: f64,
    /// Keep collinear vertices in the returned rings.
    pub preserve_collinear: bool,
}

impl Default for OffsetOptions {
    fn default() -> Self {
        Self {
            join_type: JoinType::Round,
            end_type: EndType::Polygon,
            miter_limit: 2.0,
            arc_tolerance: 0.0,
            preserve_collinear: false,
        }
    }
}

/// Offsets integer-coordinate paths and rounds the resulting vertices to
/// integer coordinates.
///
/// Use [`offset_paths_d`] when fractional output is significant. Positive
/// deltas expand closed paths according to their winding direction. Open
/// paths use the absolute delta as their half-width.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for malformed paths, [`Error::InvalidOffset`]
/// for invalid options, and [`Error::ArithmeticOverflow`] when rounded output
/// cannot be represented by `i64`.
pub fn offset_paths64(
    paths: &[Path64],
    delta: f64,
    options: OffsetOptions,
) -> Result<Paths64, Error> {
    let paths_d = paths64_to_d(paths)?;
    offset_paths_d(&paths_d, delta, options)?.into_iter().map(round_path).collect()
}

/// Offsets floating-point paths and returns floating-point polygon outlines.
///
/// [`OffsetOptions::end_type`] determines whether inputs are closed polygons
/// (`Polygon`) or open polylines (`Joined`, `Butt`, `Square`, and `Round`).
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for malformed paths, [`Error::InvalidOffset`]
/// for invalid options, and [`Error::TopologyFailure`] if generated outlines
/// cannot be cleaned into closed polygon rings.
pub fn offset_paths_d(
    paths: &[PathD],
    delta: f64,
    options: OffsetOptions,
) -> Result<PathsD, Error> {
    validate_options(delta, options)?;
    let kind = if options.end_type == EndType::Polygon { PathKind::Closed } else { PathKind::Open };
    for path in paths {
        validate_pathd(path, kind)?;
    }

    let normalized = paths.iter().map(|path| normalize_pathd(path, kind)).collect::<Vec<_>>();
    if delta.abs() <= EPSILON {
        return Ok(if options.end_type == EndType::Polygon {
            normalized
                .into_iter()
                .filter(|path| path.len() >= 3)
                .map(|path| clean_ring(path, options.preserve_collinear))
                .collect()
        } else {
            Vec::new()
        });
    }

    let mut generated = Vec::new();
    for path in &normalized {
        let outline = if options.end_type == EndType::Polygon {
            closed_outline(path, delta, options)?
        } else {
            open_outline(path, delta.abs(), options)?
        };
        if outline.len() >= 3 {
            let outline = clean_ring(outline, options.preserve_collinear);
            if outline.len() >= 3 {
                generated.push(outline);
            }
        }
    }
    if generated.is_empty() {
        return Ok(Vec::new());
    }

    // Concave offsets can contain overlapping lobes and negative slivers. The
    // exact union is the topology cleanup stage and also merges overlapping
    // offsets from multiple input paths.
    let result = boolean_opd(BooleanRequestD {
        subjects: &generated,
        clips: &[],
        clip_type: ClipType::Union,
        fill_rule: FillRule::NonZero,
    })?;
    Ok(result
        .into_iter()
        .filter(|path| path.len() >= 3)
        .map(|path| clean_ring(path, options.preserve_collinear))
        .filter(|path| path.len() >= 3)
        .collect())
}

/// Alias with a name that mirrors [`offset_paths64`].
///
/// # Errors
///
/// Propagates the validation and topology errors from [`offset_paths_d`].
pub fn offset_paths(paths: &[PathD], delta: f64, options: OffsetOptions) -> Result<PathsD, Error> {
    offset_paths_d(paths, delta, options)
}

#[derive(Clone, Copy, Debug)]
struct Vector {
    x: f64,
    y: f64,
}

impl Vector {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };

    fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    fn normalized(self) -> Option<Self> {
        let length = self.length();
        (length > EPSILON).then_some(Self { x: self.x / length, y: self.y / length })
    }

    fn left(self) -> Self {
        Self { x: -self.y, y: self.x }
    }

    fn scale(self, value: f64) -> Self {
        Self { x: self.x * value, y: self.y * value }
    }

    fn sub(self, other: Self) -> Self {
        Self { x: self.x - other.x, y: self.y - other.y }
    }

    fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }
}

impl From<PointD> for Vector {
    fn from(point: PointD) -> Self {
        Self { x: point.x, y: point.y }
    }
}

impl From<Vector> for PointD {
    fn from(vector: Vector) -> Self {
        Self::new(vector.x, vector.y)
    }
}

fn validate_options(delta: f64, options: OffsetOptions) -> Result<(), Error> {
    if !delta.is_finite()
        || !options.miter_limit.is_finite()
        || options.miter_limit < 1.0
        || !options.arc_tolerance.is_finite()
        || options.arc_tolerance < 0.0
    {
        return Err(Error::InvalidOffset);
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn paths64_to_d(paths: &[Path64]) -> Result<Vec<PathD>, Error> {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|point| {
                    Ok(PointD::new(i64_to_exact_f64(point.x)?, i64_to_exact_f64(point.y)?))
                })
                .collect()
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn i64_to_exact_f64(value: i64) -> Result<f64, Error> {
    const MAX_EXACT_INTEGER: u64 = 1 << 53;
    if value.unsigned_abs() > MAX_EXACT_INTEGER {
        return Err(Error::ArithmeticOverflow);
    }
    Ok(value as f64)
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn round_path(path: PathD) -> Result<Path64, Error> {
    path.into_iter()
        .map(|point| {
            let x = point.x.round();
            let y = point.y.round();
            if !x.is_finite()
                || !y.is_finite()
                || x < -2_f64.powi(63)
                || x >= 2_f64.powi(63)
                || y < -2_f64.powi(63)
                || y >= 2_f64.powi(63)
            {
                return Err(Error::ArithmeticOverflow);
            }
            Ok(Point64::new(x as i64, y as i64))
        })
        .collect()
}

fn closed_outline(path: &[PointD], delta: f64, options: OffsetOptions) -> Result<PathD, Error> {
    if path.len() < 3 {
        return Ok(Vec::new());
    }
    let area = signed_area2(path);
    if area.abs() <= EPSILON {
        return Err(Error::InvalidOffset);
    }
    let orientation = if area.is_sign_positive() { 1.0 } else { -1.0 };
    let directions = closed_edge_directions(path)?;
    let normals =
        directions.iter().map(|direction| direction.right().scale(orientation)).collect::<Vec<_>>();
    let mut result = Vec::new();
    for index in 0..path.len() {
        let previous = (index + path.len() - 1) % path.len();
        let previous_point = shifted(path[index], normals[previous], delta);
        let next_point = shifted(path[index], normals[index], delta);
        let turn = directions[previous].cross(directions[index]);
        let outer = turn * orientation * delta > 0.0;
        append_join(
            &mut result,
            path[index],
            previous_point,
            next_point,
            directions[previous],
            directions[index],
            normals[previous],
            normals[index],
            delta,
            outer,
            options,
        );
    }
    ensure_finite(&result)
}

fn open_outline(path: &[PointD], radius: f64, options: OffsetOptions) -> Result<PathD, Error> {
    if path.len() < 2 || radius <= EPSILON {
        return Ok(Vec::new());
    }
    let directions = edge_directions(path)?;
    let left = offset_side(path, &directions, 1.0, radius, options);
    let right = offset_side(path, &directions, -1.0, radius, options);
    let mut result = left.clone();
    let end_style = if options.end_type == EndType::Joined {
        if options.join_type == JoinType::Round { EndType::Round } else { EndType::Square }
    } else {
        options.end_type
    };
    append_cap(
        &mut result,
        path[path.len() - 1],
        left.last().copied().unwrap_or(path[path.len() - 1]),
        right.last().copied().unwrap_or(path[path.len() - 1]),
        directions[directions.len() - 1],
        radius,
        end_style,
        options.arc_tolerance,
    );
    result.extend(right.iter().rev().copied());
    append_cap(
        &mut result,
        path[0],
        right.first().copied().unwrap_or(path[0]),
        left.first().copied().unwrap_or(path[0]),
        directions[0].scale(-1.0),
        radius,
        end_style,
        options.arc_tolerance,
    );
    ensure_finite(&result)
}

fn edge_directions(path: &[PointD]) -> Result<Vec<Vector>, Error> {
    let mut directions = Vec::with_capacity(path.len().saturating_sub(1));
    for pair in path.windows(2) {
        let direction = (Vector::from(pair[1]).sub(Vector::from(pair[0])))
            .normalized()
            .ok_or(Error::InvalidOffset)?;
        directions.push(direction);
    }
    Ok(directions)
}

fn closed_edge_directions(path: &[PointD]) -> Result<Vec<Vector>, Error> {
    let mut directions = Vec::with_capacity(path.len());
    for (first, second) in
        path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len())
    {
        let direction = Vector::from(second)
            .sub(Vector::from(first))
            .normalized()
            .ok_or(Error::InvalidOffset)?;
        directions.push(direction);
    }
    Ok(directions)
}

trait RightNormal {
    fn right(self) -> Self;
}

impl RightNormal for Vector {
    fn right(self) -> Self {
        Self { x: self.y, y: -self.x }
    }
}

fn shifted(point: PointD, normal: Vector, distance: f64) -> PointD {
    PointD::new(point.x + normal.x * distance, point.y + normal.y * distance)
}

fn offset_side(
    path: &[PointD],
    directions: &[Vector],
    side: f64,
    radius: f64,
    options: OffsetOptions,
) -> PathD {
    let normals =
        directions.iter().map(|direction| direction.left().scale(side)).collect::<Vec<_>>();
    let mut result = vec![shifted(path[0], normals[0], radius)];
    for index in 1..path.len() - 1 {
        let previous = shifted(path[index], normals[index - 1], radius);
        let next = shifted(path[index], normals[index], radius);
        let outer = directions[index - 1].cross(directions[index]) * side > 0.0;
        append_join(
            &mut result,
            path[index],
            previous,
            next,
            directions[index - 1],
            directions[index],
            normals[index - 1],
            normals[index],
            radius,
            outer,
            options,
        );
    }
    result.push(shifted(path[path.len() - 1], normals[normals.len() - 1], radius));
    result
}

#[allow(clippy::too_many_arguments)]
fn append_join(
    output: &mut PathD,
    center: PointD,
    previous: PointD,
    next: PointD,
    previous_direction: Vector,
    next_direction: Vector,
    previous_normal: Vector,
    next_normal: Vector,
    delta: f64,
    outer: bool,
    options: OffsetOptions,
) {
    if !outer {
        if let Some(intersection) =
            line_intersection(previous, previous_direction, next, next_direction)
        {
            push_point(output, intersection);
        } else {
            push_point(output, previous);
            push_point(output, next);
        }
        return;
    }

    match options.join_type {
        JoinType::Bevel => {
            push_point(output, previous);
            push_point(output, next);
        }
        JoinType::Round => append_round_join(
            output,
            center,
            previous_normal,
            next_normal,
            delta.abs(),
            options.arc_tolerance,
        ),
        JoinType::Miter | JoinType::Square => {
            let intersection =
                line_intersection(previous, previous_direction, next, next_direction);
            let limit =
                if options.join_type == JoinType::Miter { options.miter_limit } else { 2.0 };
            if let Some(intersection) = intersection
                .filter(|point| distance(*point, center) <= delta.abs() * limit + EPSILON)
            {
                push_point(output, intersection);
            } else {
                push_point(output, previous);
                push_point(output, next);
            }
        }
    }
}

fn append_round_join(
    output: &mut PathD,
    center: PointD,
    previous_normal: Vector,
    next_normal: Vector,
    radius: f64,
    arc_tolerance: f64,
) {
    if radius <= EPSILON {
        push_point(output, center);
        return;
    }
    let start = previous_normal.normalized().unwrap_or(Vector::ZERO);
    let end = next_normal.normalized().unwrap_or(Vector::ZERO);
    let sweep = start.cross(end).atan2(start.dot(end));
    if sweep.abs() <= EPSILON {
        push_point(output, shifted(center, end, radius));
        return;
    }
    append_arc(output, center, start, sweep, radius, arc_tolerance);
}

#[allow(clippy::too_many_arguments)]
fn append_cap(
    output: &mut PathD,
    center: PointD,
    start: PointD,
    end: PointD,
    direction: Vector,
    radius: f64,
    end_type: EndType,
    arc_tolerance: f64,
) {
    match end_type {
        EndType::Butt => {
            push_point(output, end);
        }
        EndType::Square => {
            let extension = direction.normalized().unwrap_or(Vector::ZERO).scale(radius);
            push_point(output, add_point(start, extension));
            push_point(output, add_point(end, extension));
        }
        EndType::Round => {
            let start_vector = Vector::from(start).sub(Vector::from(center));
            let _ = direction;
            append_arc_with_sweep(
                output,
                center,
                start_vector,
                -std::f64::consts::PI,
                radius,
                arc_tolerance,
            );
            push_point(output, end);
        }
        EndType::Polygon | EndType::Joined => push_point(output, end),
    }
}

fn append_arc(
    output: &mut PathD,
    center: PointD,
    start: Vector,
    sweep: f64,
    radius: f64,
    arc_tolerance: f64,
) {
    append_arc_with_sweep(output, center, start, sweep, radius, arc_tolerance);
    let end_angle = start.y.atan2(start.x) + sweep;
    push_point(
        output,
        PointD::new(center.x + radius * end_angle.cos(), center.y + radius * end_angle.sin()),
    );
}

#[allow(clippy::cast_precision_loss)]
fn append_arc_with_sweep(
    output: &mut PathD,
    center: PointD,
    start: Vector,
    sweep: f64,
    radius: f64,
    arc_tolerance: f64,
) {
    let start_angle = start.y.atan2(start.x);
    let steps = arc_steps(radius, sweep.abs(), arc_tolerance);
    for step in 0..steps {
        let angle = start_angle + sweep * step as f64 / steps as f64;
        push_point(
            output,
            PointD::new(center.x + radius * angle.cos(), center.y + radius * angle.sin()),
        );
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn arc_steps(radius: f64, sweep: f64, arc_tolerance: f64) -> usize {
    let tolerance = if arc_tolerance > EPSILON {
        arc_tolerance.min(radius)
    } else {
        radius * ARC_TOLERANCE_RATIO
    };
    let cosine = (1.0 - tolerance / radius).clamp(-1.0, 1.0);
    let max_step = (2.0 * cosine.acos()).max(0.01);
    ((sweep / max_step).ceil() as usize).clamp(1, MAX_ARC_STEPS)
}

fn line_intersection(
    first: PointD,
    first_direction: Vector,
    second: PointD,
    second_direction: Vector,
) -> Option<PointD> {
    let denominator = first_direction.cross(second_direction);
    if denominator.abs() <= EPSILON {
        return None;
    }
    let between = Vector::from(second).sub(Vector::from(first));
    let parameter = between.cross(second_direction) / denominator;
    Some(add_point(first, first_direction.scale(parameter)))
}

fn add_point(point: PointD, vector: Vector) -> PointD {
    PointD::new(point.x + vector.x, point.y + vector.y)
}

fn distance(first: PointD, second: PointD) -> f64 {
    Vector::from(first).sub(Vector::from(second)).length()
}

fn push_point(output: &mut PathD, point: PointD) {
    if output.last().is_none_or(|last| distance(*last, point) > EPSILON) {
        output.push(point);
    }
}

fn ensure_finite(path: &PathD) -> Result<PathD, Error> {
    if path.iter().all(|point| point.x.is_finite() && point.y.is_finite()) {
        Ok(path.clone())
    } else {
        Err(Error::ArithmeticOverflow)
    }
}

fn signed_area2(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(first, second)| first.x * second.y - first.y * second.x)
        .sum()
}

fn clean_ring(mut path: PathD, preserve_collinear: bool) -> PathD {
    if preserve_collinear || path.len() < 3 {
        return path;
    }
    let mut changed = true;
    while changed && path.len() >= 3 {
        changed = false;
        let mut cleaned = Vec::with_capacity(path.len());
        for index in 0..path.len() {
            let previous = path[(index + path.len() - 1) % path.len()];
            let current = path[index];
            let next = path[(index + 1) % path.len()];
            let first = Vector::from(current).sub(Vector::from(previous));
            let second = Vector::from(next).sub(Vector::from(current));
            if first.cross(second).abs() <= EPSILON && first.dot(second) >= -EPSILON {
                changed = true;
            } else {
                cleaned.push(current);
            }
        }
        path = cleaned;
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(left: f64, bottom: f64, right: f64, top: f64) -> PathD {
        vec![
            PointD::new(left, bottom),
            PointD::new(right, bottom),
            PointD::new(right, top),
            PointD::new(left, top),
        ]
    }

    fn bounds(paths: &[PathD]) -> (f64, f64, f64, f64) {
        paths.iter().flatten().fold(
            (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            |(min_x, min_y, max_x, max_y), point| {
                (min_x.min(point.x), min_y.min(point.y), max_x.max(point.x), max_y.max(point.y))
            },
        )
    }

    fn area2(path: &[PointD]) -> f64 {
        path.iter()
            .zip(path.iter().cycle().skip(1))
            .take(path.len())
            .map(|(first, second)| first.x * second.y - first.y * second.x)
            .sum()
    }

    #[test]
    fn miter_offset_expands_and_contracts_closed_rectangle() {
        let options = OffsetOptions { join_type: JoinType::Miter, ..OffsetOptions::default() };
        let expanded = offset_paths_d(&[rectangle(0.0, 0.0, 10.0, 10.0)], 1.0, options).unwrap();
        assert_eq!(expanded.len(), 1);
        let (min_x, min_y, max_x, max_y) = bounds(&expanded);
        assert!((min_x + 1.0).abs() < 1e-9);
        assert!((min_y + 1.0).abs() < 1e-9);
        assert!((max_x - 11.0).abs() < 1e-9);
        assert!((max_y - 11.0).abs() < 1e-9);
        assert!((area2(&expanded[0]).abs() - 288.0).abs() < 1e-8);

        let contracted = offset_paths_d(&[rectangle(0.0, 0.0, 10.0, 10.0)], -1.0, options).unwrap();
        assert_eq!(contracted.len(), 1);
        assert!((area2(&contracted[0]).abs() - 128.0).abs() < 1e-8);
    }

    #[test]
    fn round_and_bevel_joins_have_distinct_vertex_contracts() {
        let path = rectangle(0.0, 0.0, 10.0, 10.0);
        let round = offset_paths_d(
            std::slice::from_ref(&path),
            1.0,
            OffsetOptions { join_type: JoinType::Round, ..OffsetOptions::default() },
        )
        .unwrap();
        let bevel = offset_paths_d(
            &[path],
            1.0,
            OffsetOptions { join_type: JoinType::Bevel, ..OffsetOptions::default() },
        )
        .unwrap();
        assert!(round[0].len() > bevel[0].len());
        assert!(area2(&round[0]).abs() > area2(&bevel[0]).abs());
    }

    #[test]
    fn open_paths_support_caps_and_joined_style() {
        let line = vec![PointD::new(0.0, 0.0), PointD::new(10.0, 0.0)];
        let square = offset_paths_d(
            std::slice::from_ref(&line),
            2.0,
            OffsetOptions {
                end_type: EndType::Square,
                join_type: JoinType::Miter,
                ..OffsetOptions::default()
            },
        )
        .unwrap();
        assert_eq!(square.len(), 1);
        let (min_x, min_y, max_x, max_y) = bounds(&square);
        assert!((min_x + 2.0).abs() < 1e-9);
        assert!((min_y + 2.0).abs() < 1e-9);
        assert!((max_x - 12.0).abs() < 1e-9);
        assert!((max_y - 2.0).abs() < 1e-9);

        let joined = offset_paths_d(
            &[line],
            2.0,
            OffsetOptions {
                end_type: EndType::Joined,
                join_type: JoinType::Round,
                ..OffsetOptions::default()
            },
        )
        .unwrap();
        assert_eq!(joined.len(), 1);
        assert!(joined[0].len() > 4);
    }

    #[test]
    fn validates_options_and_integer_rounding() {
        assert_eq!(
            offset_paths_d(
                &[rectangle(0.0, 0.0, 1.0, 1.0)],
                1.0,
                OffsetOptions { miter_limit: 0.5, ..OffsetOptions::default() },
            ),
            Err(Error::InvalidOffset)
        );
        assert_eq!(
            offset_paths_d(&[rectangle(0.0, 0.0, 1.0, 1.0)], f64::NAN, OffsetOptions::default(),),
            Err(Error::InvalidOffset)
        );
        let integer = offset_paths64(
            &[vec![
                Point64::new(0, 0),
                Point64::new(10, 0),
                Point64::new(10, 10),
                Point64::new(0, 10),
            ]],
            1.0,
            OffsetOptions { join_type: JoinType::Miter, ..OffsetOptions::default() },
        )
        .unwrap();
        assert!(integer[0].contains(&Point64::new(-1, -1)));
    }
}
