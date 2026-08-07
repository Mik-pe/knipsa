//! Polygon and polyline offsetting.
//!
//! Input edges are shifted in floating point and joins and caps are constructed
//! explicitly. Simple generated contours with no boundary contact are reduced
//! through a certified containment forest. Touching, overlapping, self-crossing,
//! oversized, or numerically ambiguous contours fall back to the exact non-zero
//! Boolean union, which removes negative slivers and merges overlapping lobes.

use crate::{
    BooleanRequestD, ClipType, Error, FillRule, Path64, PathD, PathKind, Paths64, PathsD, Point64,
    PointD, boolean_opd, normalize_pathd, validate_pathd,
};

const EPSILON: f64 = 1e-12;
const ARC_TOLERANCE_RATIO: f64 = 0.002;
const MAX_ARC_STEPS: usize = 4096;
const SMALL_CONTOUR_CERTIFICATION_LIMIT: usize = 256;
const MAX_CERTIFIED_CONTOURS: usize = 1024;
const MAX_CERTIFIED_CONTOUR_SEGMENTS: usize = 65_536;
const MAX_CERTIFIED_SEGMENT_CANDIDATES: usize = 1_048_576;
const ORIENTATION_ERROR_BOUND: f64 = (3.0 + 16.0 * f64::EPSILON) * f64::EPSILON;

/// The treatment of corners when a path is offset.
///
/// The selected join is applied to the outer side of each generated outline.
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
///
/// [`EndType::Polygon`] is the only closed-path mode. The other variants
/// create a stroked outline around an open polyline.
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
///
/// `OffsetOptions::default()` produces round joins for closed polygons. For
/// an open path, choose one of the non-`Polygon` [`EndType`] variants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetOptions {
    /// Corner style for generated outlines.
    pub join_type: JoinType,
    /// Endpoint style; `Polygon` means that input paths are closed regions.
    pub end_type: EndType,
    /// Maximum miter length divided by the absolute offset distance.
    ///
    /// Values below `1.0` are rejected. This field only affects
    /// [`JoinType::Miter`].
    pub miter_limit: f64,
    /// Maximum deviation of a round join from its ideal circle in input units.
    /// Zero selects a scale-relative default of `abs(delta) / 500`.
    pub arc_tolerance: f64,
    /// Keep collinear vertices in the returned rings instead of cleaning them.
    pub preserve_collinear: bool,
}

impl Default for OffsetOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl OffsetOptions {
    /// Creates options for closed polygons with the selected corner style.
    #[must_use]
    pub const fn polygon(join_type: JoinType) -> Self {
        Self { join_type, ..Self::DEFAULT }
    }

    /// Creates options for open polylines with the selected join and cap.
    ///
    /// Passing [`EndType::Polygon`] selects closed-polygon behavior, so open
    /// callers should use `Joined`, `Butt`, `Square`, or `Round`.
    #[must_use]
    pub const fn polyline(join_type: JoinType, end_type: EndType) -> Self {
        Self { join_type, end_type, ..Self::DEFAULT }
    }

    /// Returns these options with a different miter limit.
    #[must_use]
    pub const fn with_miter_limit(mut self, miter_limit: f64) -> Self {
        self.miter_limit = miter_limit;
        self
    }

    /// Returns these options with a different round-curve tolerance.
    #[must_use]
    pub const fn with_arc_tolerance(mut self, arc_tolerance: f64) -> Self {
        self.arc_tolerance = arc_tolerance;
        self
    }

    /// Returns these options with collinear output vertices preserved.
    #[must_use]
    pub const fn with_preserve_collinear(mut self, preserve_collinear: bool) -> Self {
        self.preserve_collinear = preserve_collinear;
        self
    }

    const DEFAULT: Self = Self {
        join_type: JoinType::Round,
        end_type: EndType::Polygon,
        miter_limit: 2.0,
        arc_tolerance: 0.0,
        preserve_collinear: false,
    };
}

/// Offsets integer-coordinate paths and rounds the resulting vertices to
/// integer coordinates.
///
/// Use [`offset_paths_d`] when fractional output is significant. Positive
/// deltas expand closed paths according to their winding direction. Open
/// paths use the absolute delta as their half-width. Integer coordinates are
/// rounded after the floating-point outline has been cleaned. Computation is
/// translated to a local origin, so absolute coordinates may span the full
/// `i64` range as long as all coordinate differences from that origin fit the
/// exact integer range of `f64` (2^53).
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
    let (origin, paths_d) = paths64_to_local_d(paths)?;
    offset_paths_d(&paths_d, delta, options)?
        .into_iter()
        .map(|path| round_path_with_origin(path, origin))
        .collect()
}

/// Offsets one integer-coordinate path.
///
/// The result remains a path collection because an inset may split a concave
/// polygon and a stroked polyline always produces a closed outline.
///
/// # Errors
///
/// Propagates validation, offset, topology, and conversion errors from
/// [`offset_paths64`].
pub fn offset_path64(path: &Path64, delta: f64, options: OffsetOptions) -> Result<Paths64, Error> {
    offset_paths64(std::slice::from_ref(path), delta, options)
}

/// Offsets floating-point paths and returns floating-point polygon outlines.
///
/// [`OffsetOptions::end_type`] determines whether inputs are closed polygons
/// (`Polygon`) or open polylines (`Joined`, `Butt`, `Square`, and `Round`).
/// A positive delta expands a closed ring according to its winding; a negative
/// delta contracts it. Open paths always use the absolute value as their
/// half-width.
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
        add_generated_outline(&mut generated, outline, options.preserve_collinear);
    }
    if generated.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(selection) = certify_non_zero_contours(&generated) {
        return Ok(generated
            .into_iter()
            .zip(selection)
            .filter_map(|(path, keep)| keep.then_some(path))
            .collect());
    }

    merge_generated_contours(&generated, options.preserve_collinear)
}

fn merge_generated_contours(
    generated: &[PathD],
    preserve_collinear: bool,
) -> Result<PathsD, Error> {
    if let Some(result) = try_merge_rectangle_pair(generated, preserve_collinear) {
        return result;
    }
    // Concave offsets can contain overlapping lobes and negative slivers. The
    // exact non-zero union is the topology cleanup stage and also merges
    // overlapping offsets from multiple input paths. The boolean kernel only
    // emits non-degenerate rings, so cleaning cannot collapse a result below
    // the closed-path minimum.
    let result = boolean_opd(BooleanRequestD {
        subjects: generated,
        clips: &[],
        clip_type: ClipType::Union,
        fill_rule: FillRule::NonZero,
    })?;
    Ok(result.into_iter().map(|path| clean_ring(path, preserve_collinear)).collect())
}

/// Routes two same-winding rectangles through the allocation-light pair
/// dispatcher. Opposite winding can cancel under `NonZero` and must retain the
/// general cleanup path.
fn try_merge_rectangle_pair(
    generated: &[PathD],
    preserve_collinear: bool,
) -> Option<Result<PathsD, Error>> {
    let [first, second] = generated else { return None };
    if !is_axis_aligned_rectangle(first) || !is_axis_aligned_rectangle(second) {
        return None;
    }
    let first_winding = certified_area_sign(first)?;
    if certified_area_sign(second)? != first_winding {
        return None;
    }
    Some(
        boolean_opd(BooleanRequestD {
            subjects: std::slice::from_ref(first),
            clips: std::slice::from_ref(second),
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        })
        .map(|paths| paths.into_iter().map(|path| clean_ring(path, preserve_collinear)).collect()),
    )
}

#[allow(clippy::float_cmp)]
fn is_axis_aligned_rectangle(path: &[PointD]) -> bool {
    let [first, second, third, fourth] = path else { return false };
    let points = [first, second, third, fourth];
    if points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .any(|(start, end)| start == end || (start.x == end.x) == (start.y == end.y))
    {
        return false;
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
    for point in [second, third, fourth] {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    min_x < max_x && min_y < max_y
}

/// Offsets one floating-point path.
///
/// The result remains a path collection because an inset may split a concave
/// polygon and a stroked polyline always produces a closed outline.
///
/// # Errors
///
/// Propagates validation and topology errors from [`offset_paths_d`].
pub fn offset_path_d(path: &PathD, delta: f64, options: OffsetOptions) -> Result<PathsD, Error> {
    offset_paths_d(std::slice::from_ref(path), delta, options)
}

/// Offsets floating-point paths; an ergonomic alias for [`offset_paths_d`].
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

    fn add(self, other: Self) -> Self {
        Self { x: self.x + other.x, y: self.y + other.y }
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
fn paths64_to_local_d(paths: &[Path64]) -> Result<(Point64, Vec<PathD>), Error> {
    let origin = paths.iter().find_map(|path| path.first()).copied().unwrap_or(Point64::new(0, 0));
    let mut local = Vec::with_capacity(paths.len());
    for path in paths {
        let mut local_path = Vec::with_capacity(path.len());
        for point in path {
            local_path.push(PointD::new(
                i128_to_exact_f64(i128::from(point.x) - i128::from(origin.x))?,
                i128_to_exact_f64(i128::from(point.y) - i128::from(origin.y))?,
            ));
        }
        local.push(local_path);
    }
    Ok((origin, local))
}

#[allow(clippy::cast_precision_loss)]
fn i128_to_exact_f64(value: i128) -> Result<f64, Error> {
    const MAX_EXACT_INTEGER: u128 = 1 << 53;
    if value.unsigned_abs() > MAX_EXACT_INTEGER {
        return Err(Error::ArithmeticOverflow);
    }
    Ok(value as f64)
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn round_path_with_origin(path: PathD, origin: Point64) -> Result<Path64, Error> {
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
            let x = i128::from(x as i64) + i128::from(origin.x);
            let y = i128::from(y as i64) + i128::from(origin.y);
            Ok(Point64::new(
                i64::try_from(x).map_err(|_| Error::ArithmeticOverflow)?,
                i64::try_from(y).map_err(|_| Error::ArithmeticOverflow)?,
            ))
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
    if delta * orientation < 0.0 && is_convex(path, orientation) {
        return Ok(convex_inset(path, delta.abs(), orientation));
    }
    // A positive delta always moves to the right of the directed boundary.
    // Consequently, correctly wound holes contract while outer rings expand.
    let normals = directions.iter().map(|direction| direction.right()).collect::<Vec<_>>();
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
    ensure_finite(result)
}

fn is_convex(path: &[PointD], orientation: f64) -> bool {
    path.iter()
        .copied()
        .zip(path.iter().copied().cycle().skip(1))
        .zip(path.iter().copied().cycle().skip(2))
        .take(path.len())
        .all(|((previous, current), next)| {
            Vector::from(current)
                .sub(Vector::from(previous))
                .cross(Vector::from(next).sub(Vector::from(current)))
                * orientation
                >= -EPSILON
        })
}

fn convex_inset(path: &[PointD], radius: f64, orientation: f64) -> PathD {
    let mut output = path.to_vec();
    for (start, end) in
        path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len())
    {
        if output.is_empty() {
            break;
        }
        let edge = Vector::from(end).sub(Vector::from(start));
        let threshold = radius * edge.length();
        let signed_distance = |point: PointD| {
            edge.cross(Vector::from(point).sub(Vector::from(start))) * orientation - threshold
        };
        let input = std::mem::take(&mut output);
        let mut previous = input[input.len() - 1];
        let mut previous_distance = signed_distance(previous);
        for current in input {
            let current_distance = signed_distance(current);
            let previous_inside = previous_distance >= -EPSILON;
            let current_inside = current_distance >= -EPSILON;
            if previous_inside != current_inside {
                let amount = previous_distance / (previous_distance - current_distance);
                push_point(
                    &mut output,
                    PointD::new(
                        previous.x + (current.x - previous.x) * amount,
                        previous.y + (current.y - previous.y) * amount,
                    ),
                );
            }
            if current_inside {
                push_point(&mut output, current);
            }
            previous = current;
            previous_distance = current_distance;
        }
    }
    clean_ring(output, false)
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
    ensure_finite(result)
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
        let outer = directions[index - 1].cross(directions[index]) * side < 0.0;
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
        JoinType::Miter => {
            let intersection =
                line_intersection(previous, previous_direction, next, next_direction);
            if let Some(intersection) = intersection.filter(|point| {
                distance(*point, center) <= delta.abs() * options.miter_limit + EPSILON
            }) {
                push_point(output, intersection);
            } else {
                append_square_join(
                    output,
                    center,
                    previous,
                    next,
                    previous_direction,
                    next_direction,
                    delta,
                );
            }
        }
        JoinType::Square => append_square_join(
            output,
            center,
            previous,
            next,
            previous_direction,
            next_direction,
            delta,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn append_square_join(
    output: &mut PathD,
    center: PointD,
    previous: PointD,
    next: PointD,
    previous_direction: Vector,
    next_direction: Vector,
    delta: f64,
) {
    let radial = Vector::from(previous)
        .sub(Vector::from(center))
        .add(Vector::from(next).sub(Vector::from(center)))
        .normalized();
    let Some(radial) = radial else {
        push_point(output, previous);
        push_point(output, next);
        return;
    };
    let cut_center = add_point(center, radial.scale(delta.abs()));
    let cut_direction = radial.left();
    let first = line_intersection(previous, previous_direction, cut_center, cut_direction);
    let second = line_intersection(next, next_direction, cut_center, cut_direction);
    if let (Some(first), Some(second)) = (first, second) {
        push_point(output, first);
        push_point(output, second);
    } else {
        push_point(output, previous);
        push_point(output, next);
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
    let steps = arc_steps(radius, sweep.abs(), arc_tolerance);
    let Some(unit) = start.normalized() else {
        return;
    };
    let angle_step = sweep / steps as f64;
    let (step_sine, step_cosine) = angle_step.sin_cos();
    let mut radial = unit.scale(radius);
    for _ in 0..steps {
        push_point(output, add_point(center, radial));
        radial = Vector {
            x: radial.x * step_cosine - radial.y * step_sine,
            y: radial.x * step_sine + radial.y * step_cosine,
        };
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

fn ensure_finite(path: PathD) -> Result<PathD, Error> {
    if path.iter().all(|point| point.x.is_finite() && point.y.is_finite()) {
        Ok(path)
    } else {
        Err(Error::ArithmeticOverflow)
    }
}

fn add_generated_outline(generated: &mut Vec<PathD>, outline: PathD, preserve_collinear: bool) {
    if outline.len() < 3 {
        return;
    }
    let outline = clean_ring(outline, preserve_collinear);
    if outline.len() >= 3 {
        generated.push(outline);
    }
}

#[derive(Clone, Copy, Debug)]
struct BoundsD {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl BoundsD {
    fn from_path(path: &[PointD]) -> Option<Self> {
        let first = *path.first()?;
        Some(path.iter().copied().skip(1).fold(
            Self { min_x: first.x, min_y: first.y, max_x: first.x, max_y: first.y },
            |bounds, point| Self {
                min_x: bounds.min_x.min(point.x),
                min_y: bounds.min_y.min(point.y),
                max_x: bounds.max_x.max(point.x),
                max_y: bounds.max_y.max(point.y),
            },
        ))
    }

    const fn from_segment(start: PointD, end: PointD) -> Self {
        Self {
            min_x: if start.x <= end.x { start.x } else { end.x },
            min_y: if start.y <= end.y { start.y } else { end.y },
            max_x: if start.x >= end.x { start.x } else { end.x },
            max_y: if start.y >= end.y { start.y } else { end.y },
        }
    }

    fn contains(self, other: Self) -> bool {
        self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && self.max_x >= other.max_x
            && self.max_y >= other.max_y
    }

    fn overlaps(self, other: Self) -> bool {
        self.max_x >= other.min_x
            && other.max_x >= self.min_x
            && self.max_y >= other.min_y
            && other.max_y >= self.min_y
    }
}

#[derive(Clone, Copy)]
struct SweepSegment {
    ring: usize,
    edge: usize,
    ring_len: usize,
    start: PointD,
    end: PointD,
    bounds: BoundsD,
}

/// Certifies when the non-zero union of generated contours can be recovered
/// without running the full arrangement kernel. Individually simple contours
/// with no boundary contact form a laminar containment forest. Accumulating
/// their winding signs through that forest identifies exactly which rings
/// separate zero winding from non-zero winding; redundant nested boundaries
/// disappear without constructing intersections or sampling with an epsilon.
fn certify_non_zero_contours(paths: &[PathD]) -> Option<Vec<bool>> {
    let mut path_iter = paths.iter();
    let Some(first) = path_iter.next() else {
        return Some(Vec::new());
    };
    if paths.len() > MAX_CERTIFIED_CONTOURS {
        return None;
    }
    let containment_cells = paths.len() * paths.len();

    let mut bounds = Vec::with_capacity(paths.len());
    let mut winding_signs = Vec::with_capacity(paths.len());
    let mut segment_count = 0_usize;
    push_certified_contour_metadata(first, &mut bounds, &mut winding_signs, &mut segment_count)?;
    for path in path_iter {
        push_certified_contour_metadata(path, &mut bounds, &mut winding_signs, &mut segment_count)?;
    }

    if paths.len() == 1 && paths[0].len() <= SMALL_CONTOUR_CERTIFICATION_LIMIT {
        return if ring_self_intersects(&paths[0]) { None } else { Some(vec![true]) };
    }
    certify_boundaries_do_not_touch(paths, segment_count)?;

    let mut contains = vec![false; containment_cells];
    for outer in 0..paths.len() {
        for inner in 0..paths.len() {
            if outer == inner || !bounds[outer].contains(bounds[inner]) {
                continue;
            }
            contains[outer * paths.len() + inner] =
                certified_point_in_ring(paths[inner][0], &paths[outer])?;
        }
    }

    let parents = contour_parents(&contains, paths.len())?;
    let depths = contour_depths(&parents)?;
    let mut order = (0..paths.len()).collect::<Vec<_>>();
    order.sort_unstable_by_key(|index| depths[*index]);

    let mut inside_winding = vec![0_i32; paths.len()];
    let mut keep = vec![false; paths.len()];
    for index in order {
        let outside = parents[index].map_or(0, |parent| inside_winding[parent]);
        let inside = outside + winding_signs[index];
        inside_winding[index] = inside;
        keep[index] = (outside == 0) != (inside == 0);
    }
    Some(keep)
}

fn push_certified_contour_metadata(
    path: &PathD,
    bounds: &mut Vec<BoundsD>,
    winding_signs: &mut Vec<i32>,
    segment_count: &mut usize,
) -> Option<()> {
    if path.len() < 3 || path.len() > MAX_CERTIFIED_CONTOUR_SEGMENTS - *segment_count {
        return None;
    }
    *segment_count += path.len();
    bounds.push(BoundsD::from_path(path)?);
    winding_signs.push(certified_area_sign(path)?);
    Some(())
}

fn contour_parents(contains: &[bool], count: usize) -> Option<Vec<Option<usize>>> {
    let mut parents = vec![None; count];
    for inner in 0..count {
        for candidate in 0..count {
            if !contains[candidate * count + inner] {
                continue;
            }
            let Some(current) = parents[inner] else {
                parents[inner] = Some(candidate);
                continue;
            };
            if contains[current * count + candidate] {
                parents[inner] = Some(candidate);
            } else if !contains[candidate * count + current] {
                // Certified disjoint simple contours are laminar. Two incomparable
                // containers therefore prove that the relation matrix is invalid.
                return None;
            }
        }
    }
    Some(parents)
}

fn contour_depths(parents: &[Option<usize>]) -> Option<Vec<usize>> {
    let mut depths = vec![0_usize; parents.len()];
    for (index, depth) in depths.iter_mut().enumerate() {
        let mut current = index;
        while let Some(parent) = parents[current] {
            *depth += 1;
            if *depth > parents.len() {
                return None;
            }
            current = parent;
        }
    }
    Some(depths)
}

fn certify_boundaries_do_not_touch(paths: &[PathD], segment_count: usize) -> Option<()> {
    certify_boundaries_do_not_touch_with_limit(
        paths,
        segment_count,
        MAX_CERTIFIED_SEGMENT_CANDIDATES,
    )
}

fn certify_boundaries_do_not_touch_with_limit(
    paths: &[PathD],
    segment_count: usize,
    candidate_limit: usize,
) -> Option<()> {
    let mut segments = Vec::with_capacity(segment_count);
    for (ring, path) in paths.iter().enumerate() {
        for edge in 0..path.len() {
            let start = path[edge];
            let end = path[(edge + 1) % path.len()];
            if start == end {
                return None;
            }
            segments.push(SweepSegment {
                ring,
                edge,
                ring_len: path.len(),
                start,
                end,
                bounds: BoundsD::from_segment(start, end),
            });
        }
    }
    segments.sort_unstable_by(|first, second| {
        first
            .bounds
            .min_x
            .total_cmp(&second.bounds.min_x)
            .then_with(|| first.bounds.min_y.total_cmp(&second.bounds.min_y))
            .then_with(|| first.bounds.max_x.total_cmp(&second.bounds.max_x))
            .then_with(|| first.bounds.max_y.total_cmp(&second.bounds.max_y))
            .then_with(|| first.ring.cmp(&second.ring))
            .then_with(|| first.edge.cmp(&second.edge))
    });

    let mut active: Vec<usize> = Vec::new();
    let mut candidates = 0_usize;
    for current_index in 0..segments.len() {
        let current = segments[current_index];
        active.retain(|index| segments[*index].bounds.max_x >= current.bounds.min_x);
        for other_index in active.iter().copied() {
            let other = segments[other_index];
            if !current.bounds.overlaps(other.bounds) || sweep_segments_are_adjacent(current, other)
            {
                continue;
            }
            candidates += 1;
            if candidates > candidate_limit
                || !segments_are_certifiably_disjoint(
                    current.start,
                    current.end,
                    other.start,
                    other.end,
                )
            {
                return None;
            }
        }
        active.push(current_index);
    }
    Some(())
}

fn sweep_segments_are_adjacent(first: SweepSegment, second: SweepSegment) -> bool {
    first.ring == second.ring
        && ((first.edge + 1) % first.ring_len == second.edge
            || (second.edge + 1) % second.ring_len == first.edge)
}

#[allow(clippy::cast_precision_loss)]
fn certified_area_sign(path: &[PointD]) -> Option<i32> {
    let origin = path[0];
    let mut sum = 0.0;
    let mut compensation = 0.0;
    let mut magnitude = 0.0;
    for index in 1..path.len() - 1 {
        let first = Vector::from(path[index]).sub(Vector::from(origin));
        let second = Vector::from(path[index + 1]).sub(Vector::from(origin));
        let left = first.x * second.y;
        let right = first.y * second.x;
        let term = left - right;
        if !left.is_finite() || !right.is_finite() || !term.is_finite() {
            return None;
        }
        magnitude += left.abs() + right.abs();
        let next = sum + term;
        compensation +=
            if sum.abs() >= term.abs() { (sum - next) + term } else { (term - next) + sum };
        sum = next;
    }
    let area = sum + compensation;
    let summation_error = (path.len() as f64 + 2.0) * f64::EPSILON * magnitude;
    let error = ORIENTATION_ERROR_BOUND * magnitude + summation_error;
    if !area.is_finite() || !magnitude.is_finite() || area.abs() <= error {
        None
    } else {
        Some(if area.is_sign_positive() { 1 } else { -1 })
    }
}

fn certified_point_in_ring(point: PointD, path: &[PointD]) -> Option<bool> {
    let mut winding = 0_i32;
    for (start, end) in
        path.iter().copied().zip(path.iter().copied().cycle().skip(1)).take(path.len())
    {
        if start.y <= point.y {
            if end.y > point.y && certified_orientation(start, end, point)? > 0 {
                winding += 1;
            }
        } else if end.y <= point.y && certified_orientation(start, end, point)? < 0 {
            winding -= 1;
        }
    }
    Some(winding != 0)
}

fn certified_orientation(first: PointD, second: PointD, third: PointD) -> Option<i8> {
    let first_x = first.x - third.x;
    let first_y = first.y - third.y;
    let second_x = second.x - third.x;
    let second_y = second.y - third.y;
    let left = first_x * second_y;
    let right = first_y * second_x;
    let determinant = left - right;
    let magnitude = left.abs() + right.abs();
    if !determinant.is_finite() || !magnitude.is_finite() {
        return None;
    }
    let error = ORIENTATION_ERROR_BOUND * magnitude;
    if determinant > error {
        Some(1)
    } else if determinant < -error {
        Some(-1)
    } else {
        None
    }
}

fn segments_are_certifiably_disjoint(
    first: PointD,
    first_end: PointD,
    second: PointD,
    second_end: PointD,
) -> bool {
    if !BoundsD::from_segment(first, first_end).overlaps(BoundsD::from_segment(second, second_end))
    {
        return true;
    }
    let Some(ab_c) = certified_orientation(first, first_end, second) else {
        return false;
    };
    let Some(ab_d) = certified_orientation(first, first_end, second_end) else {
        return false;
    };
    if ab_c == ab_d {
        return true;
    }
    let Some(cd_a) = certified_orientation(second, second_end, first) else {
        return false;
    };
    let Some(cd_b) = certified_orientation(second, second_end, first_end) else {
        return false;
    };
    cd_a == cd_b
}

fn signed_area2(path: &[PointD]) -> f64 {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .map(|(first, second)| first.x * second.y - first.y * second.x)
        .sum()
}

fn ring_self_intersects(path: &[PointD]) -> bool {
    for first in 0..path.len() {
        let first_end = (first + 1) % path.len();
        for second in (first + 1)..path.len() {
            let second_end = (second + 1) % path.len();
            if first_end == second || second_end == first {
                continue;
            }
            if segments_intersect(path[first], path[first_end], path[second], path[second_end]) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(
    first: PointD,
    first_end: PointD,
    second: PointD,
    second_end: PointD,
) -> bool {
    !segments_are_certifiably_disjoint(first, first_end, second, second_end)
}

#[cfg(test)]
fn point_on_segment(point: PointD, start: PointD, end: PointD) -> bool {
    point.x >= start.x.min(end.x) - EPSILON
        && point.x <= start.x.max(end.x) + EPSILON
        && point.y >= start.y.min(end.y) - EPSILON
        && point.y <= start.y.max(end.y) + EPSILON
}

fn clean_ring(mut path: PathD, preserve_collinear: bool) -> PathD {
    path.dedup();
    if path.len() > 1 && path.first() == path.last() {
        path.pop();
    }
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

    #[allow(clippy::cast_precision_loss)]
    fn regular_polygon(vertices: usize, radius: f64) -> PathD {
        (0..vertices)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / vertices as f64;
                PointD::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect()
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
    fn reference_regressions_for_square_holes_collapse_and_open_turns() {
        let square = offset_paths_d(
            &[rectangle(0.0, 0.0, 100.0, 100.0)],
            10.0,
            OffsetOptions { join_type: JoinType::Square, ..OffsetOptions::default() },
        )
        .unwrap();
        assert_eq!(square[0].len(), 8);
        assert!((square[0][0].x + 10.0).abs() < 1e-9);
        assert!((square[0][0].y + 4.142_135_624).abs() < 1e-9);

        let hole = vec![
            PointD::new(30.0, 30.0),
            PointD::new(30.0, 90.0),
            PointD::new(90.0, 90.0),
            PointD::new(90.0, 30.0),
        ];
        let donut = offset_paths_d(
            &[rectangle(0.0, 0.0, 120.0, 120.0), hole],
            8.0,
            OffsetOptions { join_type: JoinType::Miter, ..OffsetOptions::default() },
        )
        .unwrap();
        assert_eq!(donut.len(), 2);
        assert!(donut.iter().flatten().any(|point| *point == PointD::new(38.0, 38.0)));

        let collapsed = offset_paths_d(
            &[rectangle(0.0, 0.0, 20.0, 20.0)],
            -11.0,
            OffsetOptions { join_type: JoinType::Miter, ..OffsetOptions::default() },
        )
        .unwrap();
        assert!(collapsed.is_empty());

        let line = vec![
            PointD::new(0.0, 0.0),
            PointD::new(40.0, 0.0),
            PointD::new(55.0, 30.0),
            PointD::new(90.0, 30.0),
        ];
        let raw_stroke = open_outline(
            &line,
            6.0,
            OffsetOptions {
                join_type: JoinType::Bevel,
                end_type: EndType::Butt,
                ..OffsetOptions::default()
            },
        )
        .unwrap();
        let raw_stroke = clean_ring(raw_stroke, false);
        assert_eq!(raw_stroke.len(), 10, "raw stroke: {raw_stroke:?}");
        assert!(!ring_self_intersects(&raw_stroke), "raw stroke: {raw_stroke:?}");
        let stroke = offset_paths_d(
            &[line],
            6.0,
            OffsetOptions {
                join_type: JoinType::Bevel,
                end_type: EndType::Butt,
                ..OffsetOptions::default()
            },
        )
        .unwrap();
        assert_eq!(stroke[0].len(), 10);
        assert!(stroke[0].iter().any(|point| *point == PointD::new(90.0, 24.0)));
        assert!(stroke[0].iter().any(|point| {
            (point.x - 45.366_563_146).abs() < 1e-9 && (point.y + 2.683_281_573).abs() < 1e-9
        }));

        let concave = vec![
            PointD::new(0.0, 0.0),
            PointD::new(20.0, 0.0),
            PointD::new(20.0, 8.0),
            PointD::new(8.0, 8.0),
            PointD::new(8.0, 20.0),
            PointD::new(0.0, 20.0),
        ];
        assert!(offset_paths_d(&[concave], -1.0, OffsetOptions::polygon(JoinType::Miter),).is_ok());
    }

    #[test]
    fn single_path_offset_helpers_match_collection_helpers() {
        let integer = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
        ];
        let integer_options = OffsetOptions::polygon(JoinType::Miter);
        assert_eq!(
            offset_path64(&integer, 2.0, integer_options),
            offset_paths64(std::slice::from_ref(&integer), 2.0, integer_options)
        );

        let floating = rectangle(0.0, 0.0, 10.0, 10.0);
        let floating_options = OffsetOptions::polygon(JoinType::Round);
        let collection = offset_paths_d(std::slice::from_ref(&floating), 2.0, floating_options);
        assert_eq!(offset_path_d(&floating, 2.0, floating_options), collection);
    }

    #[test]
    fn overlapping_offsets_merge_and_preserve_general_fallback() {
        let first = rectangle(0.0, 0.0, 10.0, 10.0);
        let second = rectangle(5.0, 0.0, 15.0, 10.0);
        let options = OffsetOptions::polygon(JoinType::Round).with_arc_tolerance(0.05);
        let result = offset_paths_d(&[first.clone(), second.clone()], 2.0, options).unwrap();
        assert_eq!(result.len(), 1);
        assert!(area2(&result[0]).abs() > 250.0);

        let merged = merge_generated_contours(&[first, second], false).unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(bounds(&merged), (0.0, 0.0, 15.0, 10.0));

        let first = rectangle(0.0, 0.0, 10.0, 10.0);
        let mut opposite = rectangle(5.0, 0.0, 15.0, 10.0);
        opposite.reverse();
        assert!(try_merge_rectangle_pair(&[first.clone(), opposite.clone()], false).is_none());
        assert!(!merge_generated_contours(&[first, opposite], false).unwrap().is_empty());

        assert!(try_merge_rectangle_pair(&[], false).is_none());
        assert!(!is_axis_aligned_rectangle(&[]));
        assert!(
            try_merge_rectangle_pair(
                &[
                    rectangle(0.0, 0.0, 1.0, 1.0),
                    vec![
                        PointD::new(0.0, 0.0),
                        PointD::new(1.0, 1.0),
                        PointD::new(0.0, 1.0),
                        PointD::new(1.0, 0.0),
                    ],
                ],
                false,
            )
            .is_none()
        );
        assert!(!is_axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 1.0),
            PointD::new(0.0, 1.0),
            PointD::new(1.0, 0.0),
        ]));
        assert!(!is_axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 1.0),
            PointD::new(1.0, 0.0),
        ]));
        assert!(!is_axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(0.0, 1.0),
            PointD::new(0.0, 2.0),
            PointD::new(0.0, 3.0),
        ]));
        assert!(!is_axis_aligned_rectangle(&[
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(2.0, 0.0),
            PointD::new(3.0, 0.0),
        ]));
    }

    #[test]
    fn offset_option_constructors_are_chainable() {
        assert_eq!(OffsetOptions::polygon(JoinType::Bevel).join_type, JoinType::Bevel);
        let options = OffsetOptions::polyline(JoinType::Miter, EndType::Square)
            .with_miter_limit(4.0)
            .with_arc_tolerance(0.01)
            .with_preserve_collinear(true);
        assert_eq!(options.join_type, JoinType::Miter);
        assert_eq!(options.end_type, EndType::Square);
        assert!((options.miter_limit - 4.0).abs() < f64::EPSILON);
        assert!((options.arc_tolerance - 0.01).abs() < f64::EPSILON);
        assert!(options.preserve_collinear);
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

    #[test]
    #[allow(clippy::too_many_lines)]
    fn covers_zero_delta_empty_and_degenerate_inputs() {
        let path = rectangle(0.0, 0.0, 10.0, 10.0);
        assert_eq!(offset_paths_d(&[], 1.0, OffsetOptions::default()), Ok(Vec::new()));
        assert_eq!(
            offset_paths_d(std::slice::from_ref(&path), 0.0, OffsetOptions::default()),
            Ok(vec![path.clone()])
        );

        let line = vec![PointD::new(0.0, 0.0), PointD::new(10.0, 0.0)];
        assert_eq!(
            offset_paths_d(
                std::slice::from_ref(&line),
                0.0,
                OffsetOptions { end_type: EndType::Butt, ..OffsetOptions::default() },
            ),
            Ok(Vec::new())
        );
        assert_eq!(
            offset_paths(std::slice::from_ref(&path), 0.0, OffsetOptions::default()).unwrap(),
            vec![path.clone()]
        );

        let translated_origin = (1_i64 << 53) + 1;
        let translated = vec![vec![
            Point64::new(translated_origin, 0),
            Point64::new(translated_origin + 10, 0),
            Point64::new(translated_origin + 10, 10),
            Point64::new(translated_origin, 10),
        ]];
        let translated_offset = offset_paths64(
            &translated,
            1.0,
            OffsetOptions { join_type: JoinType::Miter, ..OffsetOptions::default() },
        )
        .expect("small integer geometry should not depend on its absolute origin");
        assert!(translated_offset[0].contains(&Point64::new(translated_origin - 1, -1)));

        let options = OffsetOptions { miter_limit: f64::NAN, ..OffsetOptions::default() };
        assert_eq!(validate_options(1.0, options), Err(Error::InvalidOffset));
        let options = OffsetOptions { arc_tolerance: f64::NAN, ..OffsetOptions::default() };
        assert_eq!(validate_options(1.0, options), Err(Error::InvalidOffset));
        let options = OffsetOptions { arc_tolerance: -1.0, ..OffsetOptions::default() };
        assert_eq!(validate_options(1.0, options), Err(Error::InvalidOffset));
        assert_eq!(
            validate_options(f64::INFINITY, OffsetOptions::default()),
            Err(Error::InvalidOffset)
        );

        assert_eq!(closed_outline(&[], 1.0, OffsetOptions::default()), Ok(Vec::new()));
        assert_eq!(
            closed_outline(
                &[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0),],
                1.0,
                OffsetOptions::default(),
            ),
            Err(Error::InvalidOffset)
        );
        assert_eq!(open_outline(&[], 1.0, OffsetOptions::default()), Ok(Vec::new()));
        assert_eq!(
            open_outline(&[PointD::new(0.0, 0.0)], 1.0, OffsetOptions::default()),
            Ok(Vec::new())
        );
        assert_eq!(open_outline(&line, 0.0, OffsetOptions::default()), Ok(Vec::new()));
        let bent = vec![PointD::new(0.0, 0.0), PointD::new(10.0, 0.0), PointD::new(10.0, 10.0)];
        assert!(
            open_outline(
                &bent,
                1.0,
                OffsetOptions { end_type: EndType::Butt, ..OffsetOptions::default() },
            )
            .unwrap()
            .len()
                >= 3
        );
        assert!(
            open_outline(
                &bent,
                1.0,
                OffsetOptions {
                    end_type: EndType::Joined,
                    join_type: JoinType::Miter,
                    ..OffsetOptions::default()
                },
            )
            .is_ok()
        );
        assert_eq!(
            offset_paths_d(
                &[Vec::new()],
                1.0,
                OffsetOptions { end_type: EndType::Butt, ..OffsetOptions::default() },
            ),
            Ok(Vec::new())
        );
        let mut reversed = path.clone();
        reversed.reverse();
        assert_eq!(offset_paths_d(&[reversed], 1.0, OffsetOptions::default()).unwrap().len(), 1);
        assert!(matches!(
            edge_directions(&[PointD::new(0.0, 0.0), PointD::new(0.0, 0.0)]),
            Err(Error::InvalidOffset)
        ));
        assert!(matches!(
            closed_edge_directions(&[
                PointD::new(0.0, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(0.0, 0.0),
            ]),
            Err(Error::InvalidOffset)
        ));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn certified_contour_forest_handles_holes_redundancy_and_large_rounds() {
        let outer = rectangle(0.0, 0.0, 100.0, 100.0);
        let inner = rectangle(20.0, 20.0, 80.0, 80.0);
        assert_eq!(
            certify_non_zero_contours(&[outer.clone(), inner.clone()]),
            Some(vec![true, false])
        );

        let mut hole = inner.clone();
        hole.reverse();
        assert_eq!(
            certify_non_zero_contours(&[outer.clone(), hole.clone()]),
            Some(vec![true, true])
        );
        let island = rectangle(35.0, 35.0, 65.0, 65.0);
        assert_eq!(
            certify_non_zero_contours(&[outer.clone(), hole.clone(), island.clone()]),
            Some(vec![true, true, true])
        );

        let middle = rectangle(15.0, 15.0, 85.0, 85.0);
        let mut cancelling_inner = rectangle(30.0, 30.0, 70.0, 70.0);
        cancelling_inner.reverse();
        assert_eq!(
            certify_non_zero_contours(&[outer.clone(), middle, cancelling_inner]),
            Some(vec![true, false, false])
        );

        let disjoint = rectangle(120.0, 0.0, 150.0, 30.0);
        assert_eq!(certify_non_zero_contours(&[outer.clone(), disjoint]), Some(vec![true, true]));
        assert_eq!(
            certify_non_zero_contours(&[
                rectangle(0.0, 0.0, 10.0, 10.0),
                rectangle(5.0, 5.0, 15.0, 15.0),
            ]),
            None
        );
        assert_eq!(
            certify_non_zero_contours(&[
                rectangle(0.0, 0.0, 10.0, 10.0),
                rectangle(10.0, 0.0, 20.0, 10.0),
            ]),
            None
        );

        let large = regular_polygon(SMALL_CONTOUR_CERTIFICATION_LIMIT + 44, 20.0);
        assert_eq!(certify_non_zero_contours(std::slice::from_ref(&large)), Some(vec![true]));

        let options = OffsetOptions::polygon(JoinType::Round).with_arc_tolerance(0.000_01);
        let raw = clean_ring(closed_outline(&outer, 10.0, options).unwrap(), false);
        assert!(raw.len() > SMALL_CONTOUR_CERTIFICATION_LIMIT);
        assert_eq!(certify_non_zero_contours(std::slice::from_ref(&raw)), Some(vec![true]));
        let rounded = offset_paths_d(&[outer], 10.0, options).unwrap();
        assert_eq!(rounded.len(), 1);
        assert!(rounded[0].len() > SMALL_CONTOUR_CERTIFICATION_LIMIT);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn covers_certified_contour_predicates_and_budgets() {
        let empty_bounds = BoundsD::from_path(&[]);
        assert!(empty_bounds.is_none());
        let bounds = BoundsD::from_path(&rectangle(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert!(
            bounds.contains(BoundsD::from_segment(PointD::new(2.0, 2.0), PointD::new(8.0, 8.0)))
        );
        assert!(
            !bounds.contains(BoundsD::from_segment(PointD::new(-1.0, 2.0), PointD::new(8.0, 8.0)))
        );
        assert!(
            !bounds.contains(BoundsD::from_segment(PointD::new(2.0, -1.0), PointD::new(8.0, 8.0)))
        );
        assert!(
            !bounds.contains(BoundsD::from_segment(PointD::new(2.0, 2.0), PointD::new(11.0, 8.0)))
        );
        assert!(
            !bounds.contains(BoundsD::from_segment(PointD::new(2.0, 2.0), PointD::new(8.0, 11.0)))
        );
        assert!(
            bounds.overlaps(BoundsD::from_segment(PointD::new(10.0, 2.0), PointD::new(12.0, 8.0)))
        );
        assert!(
            !bounds.overlaps(BoundsD::from_segment(PointD::new(11.0, 2.0), PointD::new(12.0, 8.0)))
        );

        let positive = rectangle(0.0, 0.0, 10.0, 10.0);
        let mut negative = positive.clone();
        negative.reverse();
        assert_eq!(certified_area_sign(&positive), Some(1));
        assert_eq!(certified_area_sign(&negative), Some(-1));
        assert_eq!(certified_area_sign(&[PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)]), None);

        let large = 1.0e154;
        assert_eq!(
            certified_area_sign(&[
                PointD::new(0.0, 0.0),
                PointD::new(large, 0.0),
                PointD::new(0.0, large),
                PointD::new(-large, 0.0),
            ]),
            None
        );
        assert_eq!(
            certified_area_sign(&[
                PointD::new(0.0, 0.0),
                PointD::new(large, 0.0),
                PointD::new(0.0, large),
                PointD::new(large, 0.0),
            ]),
            None
        );
        assert_eq!(
            certified_area_sign(&[
                PointD::new(0.0, 0.0),
                PointD::new(f64::MAX, 1.0),
                PointD::new(1.0, f64::MAX),
            ]),
            None
        );
        assert_eq!(
            certify_non_zero_contours(&[vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)]]),
            None
        );
        assert_eq!(
            certified_area_sign(&[
                PointD::new(0.0, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(2.0, 0.0),
            ]),
            None
        );
        assert_eq!(
            certified_area_sign(&[
                PointD::new(f64::MAX, f64::MAX),
                PointD::new(-f64::MAX, f64::MAX),
                PointD::new(f64::MAX, -f64::MAX),
            ]),
            None
        );
        assert_eq!(
            certified_area_sign(&[
                PointD::new(0.0, 0.0),
                PointD::new(0.0, f64::MAX),
                PointD::new(f64::MAX, 0.0),
            ]),
            None
        );
        assert_eq!(
            certified_area_sign(&[
                PointD::new(0.0, 0.0),
                PointD::new(f64::MAX, f64::MAX),
                PointD::new(-1.0, 1.0),
            ]),
            None
        );

        assert_eq!(certified_point_in_ring(PointD::new(5.0, 5.0), &positive), Some(true));
        assert_eq!(certified_point_in_ring(PointD::new(15.0, 5.0), &positive), Some(false));
        assert_eq!(certified_point_in_ring(PointD::new(0.0, 5.0), &positive), None);
        assert_eq!(
            certified_orientation(
                PointD::new(0.0, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(0.0, 1.0)
            ),
            Some(1)
        );
        assert_eq!(
            certified_orientation(
                PointD::new(0.0, 0.0),
                PointD::new(0.0, 1.0),
                PointD::new(1.0, 0.0)
            ),
            Some(-1)
        );
        assert_eq!(
            certified_orientation(
                PointD::new(0.0, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(2.0, 0.0)
            ),
            None
        );
        assert_eq!(
            certified_orientation(
                PointD::new(f64::MAX, 0.0),
                PointD::new(-f64::MAX, 0.0),
                PointD::new(0.0, f64::MAX)
            ),
            None
        );
        assert_eq!(
            certified_orientation(
                PointD::new(f64::MAX, 1.0),
                PointD::new(f64::MAX, 1.0),
                PointD::new(0.0, 0.0)
            ),
            None
        );

        assert!(segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(2.0, 0.0),
            PointD::new(3.0, 0.0)
        ));
        assert!(segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(2.0, 2.0),
            PointD::new(0.0, 1.0),
            PointD::new(1.0, 2.0)
        ));
        assert!(!segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(2.0, 2.0),
            PointD::new(0.0, 2.0),
            PointD::new(2.0, 0.0)
        ));
        assert!(!segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(2.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(3.0, 0.0)
        ));
        assert!(!segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(-1.0, -1.0),
            PointD::new(1.0, 1.0)
        ));
        assert!(!segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(9.0, -1.0),
            PointD::new(11.0, 1.0)
        ));
        assert!(segments_are_certifiably_disjoint(
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 0.0),
            PointD::new(9.0, -1.0),
            PointD::new(20.0, 1.0)
        ));

        let mut contains = vec![false; 9];
        contains[1] = true;
        contains[2] = true;
        contains[5] = true;
        assert_eq!(contour_parents(&contains, 3), Some(vec![None, Some(0), Some(1)]));
        assert_eq!(contour_depths(&[None, Some(0), Some(1)]), Some(vec![0, 1, 2]));
        let mut reverse_order = vec![false; 9];
        reverse_order[2] = true;
        reverse_order[3] = true;
        reverse_order[5] = true;
        assert_eq!(contour_parents(&reverse_order, 3), Some(vec![Some(1), None, Some(0)]));
        let mut invalid_contains = vec![false; 9];
        invalid_contains[2] = true;
        invalid_contains[5] = true;
        assert_eq!(contour_parents(&invalid_contains, 3), None);
        assert_eq!(contour_depths(&[Some(1), Some(0)]), None);

        let outer_diamond = vec![
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
            PointD::new(0.0, -10.0),
            PointD::new(-10.0, 0.0),
        ];
        let inner_diamond = vec![
            PointD::new(0.0, 5.0),
            PointD::new(5.0, 0.0),
            PointD::new(0.0, -5.0),
            PointD::new(-5.0, 0.0),
        ];
        assert_eq!(
            certify_boundaries_do_not_touch_with_limit(
                &[outer_diamond.clone(), inner_diamond.clone()],
                8,
                0
            ),
            None
        );
        assert_eq!(certify_boundaries_do_not_touch(&[outer_diamond, inner_diamond], 8), Some(()));
        assert_eq!(
            certify_boundaries_do_not_touch(
                &[vec![PointD::new(0.0, 0.0), PointD::new(0.0, 0.0), PointD::new(1.0, 1.0),]],
                3
            ),
            None
        );

        let first_segment = SweepSegment {
            ring: 0,
            edge: 0,
            ring_len: 4,
            start: PointD::new(0.0, 0.0),
            end: PointD::new(1.0, 0.0),
            bounds: BoundsD::from_segment(PointD::new(0.0, 0.0), PointD::new(1.0, 0.0)),
        };
        let adjacent = SweepSegment { edge: 1, ..first_segment };
        let separate = SweepSegment { ring: 1, ..first_segment };
        assert!(sweep_segments_are_adjacent(first_segment, adjacent));
        assert!(!sweep_segments_are_adjacent(first_segment, separate));

        let bow_tie = vec![
            PointD::new(0.0, 0.0),
            PointD::new(10.0, 10.0),
            PointD::new(0.0, 10.0),
            PointD::new(10.0, 0.0),
        ];
        assert_eq!(certify_non_zero_contours(&[bow_tie]), None);

        let too_many_contours = vec![positive.clone(); MAX_CERTIFIED_CONTOURS + 1];
        assert_eq!(certify_non_zero_contours(&too_many_contours), None);
        assert_eq!(
            certify_non_zero_contours(&[vec![
                PointD::new(0.0, 0.0);
                MAX_CERTIFIED_CONTOUR_SEGMENTS + 1
            ]]),
            None
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn covers_offset_helper_branches() {
        let center = PointD::new(0.0, 0.0);
        let horizontal = Vector { x: 1.0, y: 0.0 };
        let vertical = Vector { x: 0.0, y: 1.0 };
        let converted: PointD = horizontal.into();
        assert_eq!(converted, PointD::new(1.0, 0.0));
        let previous = PointD::new(0.0, 1.0);
        let next = PointD::new(1.0, 0.0);
        let mut output = Vec::new();
        let mut generated = Vec::new();
        add_generated_outline(&mut generated, Vec::new(), false);
        add_generated_outline(
            &mut generated,
            vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0)],
            false,
        );
        assert!(generated.is_empty());

        append_join(
            &mut output,
            center,
            previous,
            next,
            horizontal,
            vertical,
            horizontal,
            vertical,
            1.0,
            false,
            OffsetOptions::default(),
        );
        assert_eq!(output.len(), 1);
        output.clear();
        append_join(
            &mut output,
            center,
            previous,
            PointD::new(1.0, 1.0),
            horizontal,
            horizontal,
            horizontal,
            horizontal,
            1.0,
            false,
            OffsetOptions::default(),
        );
        assert_eq!(output.len(), 2);

        for join_type in [JoinType::Bevel, JoinType::Round, JoinType::Miter, JoinType::Square] {
            output.clear();
            append_join(
                &mut output,
                center,
                previous,
                next,
                horizontal,
                vertical,
                horizontal,
                vertical,
                1.0,
                true,
                OffsetOptions { join_type, arc_tolerance: 0.5, ..OffsetOptions::default() },
            );
            assert!(!output.is_empty());
        }
        output.clear();
        append_join(
            &mut output,
            center,
            PointD::new(0.0, 1.0),
            PointD::new(10.0, 0.0),
            horizontal,
            vertical,
            horizontal,
            vertical,
            1.0,
            true,
            OffsetOptions {
                join_type: JoinType::Miter,
                miter_limit: 1.0,
                ..OffsetOptions::default()
            },
        );
        assert_eq!(output.len(), 2);
        output.clear();
        append_join(
            &mut output,
            center,
            PointD::new(0.0, 1.0),
            PointD::new(10.0, 0.0),
            horizontal,
            vertical,
            horizontal,
            vertical,
            1.0,
            true,
            OffsetOptions { join_type: JoinType::Square, ..OffsetOptions::default() },
        );
        assert_eq!(output.len(), 2);

        output.clear();
        append_round_join(&mut output, center, horizontal, horizontal, 1.0, 0.0);
        assert_eq!(output.len(), 1);
        output.clear();
        append_round_join(&mut output, center, horizontal, vertical, 0.0, 0.0);
        assert_eq!(output, vec![center]);

        output.clear();
        append_cap(&mut output, center, previous, next, horizontal, 1.0, EndType::Butt, 0.0);
        append_cap(&mut output, center, previous, next, Vector::ZERO, 1.0, EndType::Square, 0.0);
        append_cap(&mut output, center, previous, next, horizontal, 1.0, EndType::Round, 0.5);
        append_cap(&mut output, center, previous, next, horizontal, 1.0, EndType::Polygon, 0.0);
        append_cap(&mut output, center, previous, next, horizontal, 1.0, EndType::Joined, 0.0);
        assert!(!output.is_empty());

        assert_eq!(arc_steps(1.0, 0.0, 0.0), 1);
        assert!(arc_steps(1.0, 100.0, 1e-20) <= MAX_ARC_STEPS);
        assert_eq!(line_intersection(previous, horizontal, next, horizontal), None);
        assert!(line_intersection(previous, horizontal, next, vertical).is_some());
        assert!((distance(center, PointD::new(3.0, 4.0)) - 5.0).abs() < f64::EPSILON);
        assert_eq!(i128_to_exact_f64((1_i128 << 53) + 1), Err(Error::ArithmeticOverflow));
        let excessive_span =
            vec![vec![Point64::new(0, 0), Point64::new((1_i64 << 53) + 1, 0), Point64::new(0, 1)]];
        assert_eq!(
            offset_paths64(&excessive_span, 1.0, OffsetOptions::polygon(JoinType::Miter)),
            Err(Error::ArithmeticOverflow)
        );

        output.clear();
        append_square_join(
            &mut output,
            center,
            PointD::new(0.0, 1.0),
            PointD::new(0.0, -1.0),
            horizontal,
            horizontal,
            1.0,
        );
        assert_eq!(output.len(), 2);
        output.clear();
        let diagonal = Vector { x: -1.0, y: 1.0 }.normalized().unwrap();
        append_square_join(
            &mut output,
            center,
            PointD::new(1.0, 0.0),
            PointD::new(0.0, 1.0),
            diagonal,
            diagonal,
            1.0,
        );
        assert_eq!(output.len(), 2);
        append_arc_with_sweep(&mut output, center, Vector::ZERO, 1.0, 1.0, 0.1);

        assert!(ring_self_intersects(&[
            PointD::new(0.0, 0.0),
            PointD::new(2.0, 2.0),
            PointD::new(0.0, 2.0),
            PointD::new(2.0, 0.0),
        ]));
        assert!(segments_intersect(
            PointD::new(0.0, 0.0),
            PointD::new(2.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(3.0, 0.0),
        ));
        assert!(point_on_segment(PointD::new(1.0, 0.0), center, PointD::new(2.0, 0.0)));
        assert!(!point_on_segment(PointD::new(-1.0, 0.0), center, PointD::new(2.0, 0.0)));
        assert!(!point_on_segment(PointD::new(3.0, 0.0), center, PointD::new(2.0, 0.0)));
        assert!(!point_on_segment(PointD::new(1.0, -1.0), center, PointD::new(2.0, 0.0)));
        assert!(!point_on_segment(PointD::new(1.0, 1.0), center, PointD::new(2.0, 0.0)));

        let simple = rectangle(0.0, 0.0, 2.0, 2.0);
        assert_eq!(certify_non_zero_contours(&[]), Some(Vec::new()));
        assert_eq!(certify_non_zero_contours(std::slice::from_ref(&simple)), Some(vec![true]));

        // Repeated, oppositely directed edges force the sweep ordering all the
        // way through its deterministic edge-index tie breaker. The geometry is
        // intentionally uncertifiable because non-adjacent copies overlap.
        let repeated_edge_bounds = vec![
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
            PointD::new(0.0, 0.0),
            PointD::new(1.0, 0.0),
        ];
        assert_eq!(
            certify_boundaries_do_not_touch_with_limit(
                std::slice::from_ref(&repeated_edge_bounds),
                repeated_edge_bounds.len(),
                usize::MAX,
            ),
            None
        );
        let lopsided_bow_tie = vec![
            PointD::new(0.0, 0.0),
            PointD::new(4.0, 4.0),
            PointD::new(0.0, 4.0),
            PointD::new(3.0, 0.0),
        ];
        assert_eq!(certified_area_sign(&lopsided_bow_tie), Some(1));
        assert_eq!(certify_non_zero_contours(std::slice::from_ref(&lopsided_bow_tie)), None);
        assert_eq!(certify_non_zero_contours(&[vec![center, next]]), None);
        assert_eq!(certify_non_zero_contours(&[vec![center, next, PointD::new(2.0, 0.0)]]), None);
        assert_eq!(
            certify_non_zero_contours(&[vec![
                PointD::new(0.0, 0.0),
                PointD::new(2.0, 2.0),
                PointD::new(0.0, 2.0),
                PointD::new(2.0, 0.0),
            ]]),
            None
        );

        let mut points = vec![center];
        push_point(&mut points, center);
        push_point(&mut points, next);
        assert_eq!(points.len(), 2);
        assert_eq!(ensure_finite(points.clone()), Ok(points));
        assert_eq!(ensure_finite(vec![PointD::new(f64::NAN, 0.0)]), Err(Error::ArithmeticOverflow));

        assert_eq!(clean_ring(vec![center, next], false), vec![center, next]);
        assert_eq!(clean_ring(vec![center, next, PointD::new(2.0, 0.0)], true).len(), 3);
        let cleaned = clean_ring(
            vec![
                PointD::new(0.0, 0.0),
                PointD::new(1.0, 0.0),
                PointD::new(2.0, 0.0),
                PointD::new(2.0, 1.0),
                PointD::new(0.0, 1.0),
            ],
            false,
        );
        assert_eq!(cleaned.len(), 4);
        assert_eq!(
            clean_ring(
                vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(2.0, 0.0),],
                false,
            )
            .len(),
            2
        );
        assert_eq!(
            clean_ring(
                vec![PointD::new(0.0, 0.0), PointD::new(1.0, 0.0), PointD::new(0.0, 0.0),],
                false,
            )
            .len(),
            2
        );

        for bad in [
            PointD::new(f64::NAN, 0.0),
            PointD::new(0.0, f64::NAN),
            PointD::new(-2_f64.powi(63) - 4096.0, 0.0),
            PointD::new(2_f64.powi(63), 0.0),
            PointD::new(0.0, -2_f64.powi(63) - 4096.0),
            PointD::new(0.0, 2_f64.powi(63)),
        ] {
            assert_eq!(
                round_path_with_origin(vec![bad], Point64::new(0, 0)),
                Err(Error::ArithmeticOverflow)
            );
        }
    }
}
