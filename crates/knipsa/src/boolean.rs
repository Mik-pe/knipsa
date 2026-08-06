//! Exact planar arrangement kernel used by the public boolean operations.
//!
//! The implementation deliberately keeps the numerical model separate from
//! the public coordinate types. Input coordinates become reduced arbitrary
//! precision rationals, all crossings are inserted into the arrangement, and
//! the two sides of every atomic edge are classified before output rings are
//! traced. This makes the first kernel easy to audit and gives the later
//! sweep-line optimization a differential oracle.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use num_bigint::{BigInt, Sign};
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::{
    BooleanRequest, BooleanRequestD, ClipType, Error, FillRule, Path64, PathD, Paths64, PathsD,
    Point64, PointD, normalize_path64, normalize_pathd,
};

const INTEGER_SAMPLE_BITS: usize = 120;
const DOUBLE_SAMPLE_BITS: usize = 120;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum Rational {
    Small { numerator: i128, denominator: i128 },
    Big { numerator: BigInt, denominator: BigInt },
}

impl Rational {
    fn zero() -> Self {
        Self::Small { numerator: 0, denominator: 1 }
    }

    fn one() -> Self {
        Self::Small { numerator: 1, denominator: 1 }
    }

    fn from_i64(value: i64) -> Self {
        Self::Small { numerator: i128::from(value), denominator: 1 }
    }

    fn from_f64(value: f64) -> Result<Self, Error> {
        if !value.is_finite() {
            return Err(Error::NonFiniteCoordinate { point_index: 0 });
        }
        let bits = value.to_bits();
        let negative = (bits >> 63) != 0;
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1_u64 << 52) - 1);
        let (mantissa, exponent) = if exponent_bits == 0 {
            (fraction, -1022 - 52)
        } else {
            ((1_u64 << 52) | fraction, exponent_bits - 1023 - 52)
        };
        let signed_mantissa = if negative { -i128::from(mantissa) } else { i128::from(mantissa) };
        if exponent >= 0 {
            let shift = u32::try_from(exponent).expect("f64 exponent is non-negative here");
            if let Some(numerator) = signed_mantissa.checked_shl(shift) {
                return Ok(Self::from_i128(numerator, 1));
            }
        } else {
            let shift = u32::try_from(-exponent).expect("f64 exponent fits in u32");
            if let Some(denominator) = 1_i128.checked_shl(shift) {
                return Ok(Self::new_small(signed_mantissa, denominator)
                    .expect("a positive power-of-two denominator fits the small rational"));
            }
        }
        let mut numerator = BigInt::from(signed_mantissa);
        if exponent >= 0 {
            numerator <<= usize::try_from(exponent).expect("f64 exponent fits in usize");
            Ok(Self::new(numerator, BigInt::one()))
        } else {
            let denominator =
                BigInt::one() << usize::try_from(-exponent).expect("f64 exponent fits in usize");
            Ok(Self::new(numerator, denominator))
        }
    }

    fn new(mut numerator: BigInt, mut denominator: BigInt) -> Self {
        debug_assert!(!denominator.is_zero());
        if denominator.sign() == Sign::Minus {
            numerator = -numerator;
            denominator = -denominator;
        }
        if numerator.is_zero() {
            return Self::zero();
        }
        let divisor = gcd(numerator.abs(), denominator.clone());
        numerator /= &divisor;
        denominator /= divisor;
        if let (Some(numerator), Some(denominator)) = (numerator.to_i128(), denominator.to_i128()) {
            return Self::new_small(numerator, denominator)
                .expect("a normalized i128 rational must fit the small representation");
        }
        Self::Big { numerator, denominator }
    }

    fn new_small(mut numerator: i128, mut denominator: i128) -> Option<Self> {
        if denominator == 0 || denominator == i128::MIN {
            return None;
        }
        if denominator < 0 {
            numerator = numerator.checked_neg()?;
            denominator = -denominator;
        }
        if numerator == 0 {
            return Some(Self::zero());
        }
        let denominator_abs = u128::try_from(denominator).ok()?;
        let divisor = i128::try_from(gcd_u128(numerator.unsigned_abs(), denominator_abs)).ok()?;
        Some(Self::Small { numerator: numerator / divisor, denominator: denominator / divisor })
    }

    fn from_i128(numerator: i128, denominator: i128) -> Self {
        Self::new_small(numerator, denominator)
            .unwrap_or_else(|| Self::new(BigInt::from(numerator), BigInt::from(denominator)))
    }

    fn big_parts(&self) -> (BigInt, BigInt) {
        match self {
            Self::Small { numerator, denominator } => {
                (BigInt::from(*numerator), BigInt::from(*denominator))
            }
            Self::Big { numerator, denominator } => (numerator.clone(), denominator.clone()),
        }
    }

    fn neg(&self) -> Self {
        match self {
            Self::Small { numerator, denominator } => numerator.checked_neg().map_or_else(
                || Self::new(-BigInt::from(*numerator), BigInt::from(*denominator)),
                |numerator| Self::Small { numerator, denominator: *denominator },
            ),
            Self::Big { numerator, denominator } => {
                Self::Big { numerator: -numerator, denominator: denominator.clone() }
            }
        }
    }

    fn add(&self, other: &Self) -> Self {
        if let (
            Self::Small { numerator: left_numerator, denominator: left_denominator },
            Self::Small { numerator: right_numerator, denominator: right_denominator },
        ) = (self, other)
        {
            let numerator = left_numerator.checked_mul(*right_denominator).and_then(|left| {
                right_numerator
                    .checked_mul(*left_denominator)
                    .and_then(|right| left.checked_add(right))
            });
            let denominator = left_denominator.checked_mul(*right_denominator);
            if let (Some(numerator), Some(denominator)) = (numerator, denominator) {
                return Self::from_i128(numerator, denominator);
            }
        }
        let (left_numerator, left_denominator) = self.big_parts();
        let (right_numerator, right_denominator) = other.big_parts();
        Self::new(
            left_numerator * &right_denominator + right_numerator * &left_denominator,
            left_denominator * right_denominator,
        )
    }

    fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    fn mul(&self, other: &Self) -> Self {
        if let (
            Self::Small { numerator: left_numerator, denominator: left_denominator },
            Self::Small { numerator: right_numerator, denominator: right_denominator },
        ) = (self, other)
        {
            if let (Some(numerator), Some(denominator)) = (
                left_numerator.checked_mul(*right_numerator),
                left_denominator.checked_mul(*right_denominator),
            ) {
                return Self::from_i128(numerator, denominator);
            }
        }
        let (left_numerator, left_denominator) = self.big_parts();
        let (right_numerator, right_denominator) = other.big_parts();
        Self::new(left_numerator * right_numerator, left_denominator * right_denominator)
    }

    fn div(&self, other: &Self) -> Self {
        debug_assert!(!other.is_zero());
        if let (
            Self::Small { numerator: left_numerator, denominator: left_denominator },
            Self::Small { numerator: right_numerator, denominator: right_denominator },
        ) = (self, other)
        {
            if let (Some(numerator), Some(denominator)) = (
                left_numerator.checked_mul(*right_denominator),
                left_denominator.checked_mul(*right_numerator),
            ) {
                return Self::from_i128(numerator, denominator);
            }
        }
        let (left_numerator, left_denominator) = self.big_parts();
        let (right_numerator, right_denominator) = other.big_parts();
        Self::new(left_numerator * right_denominator, left_denominator * right_numerator)
    }

    fn is_zero(&self) -> bool {
        match self {
            Self::Small { numerator, .. } => *numerator == 0,
            Self::Big { numerator, .. } => numerator.is_zero(),
        }
    }

    fn is_negative(&self) -> bool {
        match self {
            Self::Small { numerator, .. } => *numerator < 0,
            Self::Big { numerator, .. } => numerator.sign() == Sign::Minus,
        }
    }

    fn is_positive(&self) -> bool {
        !self.is_zero() && !self.is_negative()
    }

    fn to_i64(&self) -> Option<i64> {
        match self {
            Self::Small { numerator, denominator } => {
                (*denominator == 1).then(|| i64::try_from(*numerator).ok()).flatten()
            }
            Self::Big { numerator, denominator } => {
                if denominator == &BigInt::one() {
                    numerator.to_i64()
                } else {
                    None
                }
            }
        }
    }

    fn to_f64(&self) -> Option<f64> {
        let (numerator, denominator) = match self {
            Self::Small { numerator, denominator } => (numerator.to_f64()?, denominator.to_f64()?),
            Self::Big { numerator, denominator } => (numerator.to_f64()?, denominator.to_f64()?),
        };
        let value = numerator / denominator;
        value.is_finite().then_some(value)
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        if let (
            Self::Small { numerator: left_numerator, denominator: left_denominator },
            Self::Small { numerator: right_numerator, denominator: right_denominator },
        ) = (self, other)
        {
            if let (Some(left), Some(right)) = (
                left_numerator.checked_mul(*right_denominator),
                right_numerator.checked_mul(*left_denominator),
            ) {
                return left.cmp(&right);
            }
        }
        let (left_numerator, left_denominator) = self.big_parts();
        let (right_numerator, right_denominator) = other.big_parts();
        (left_numerator * right_denominator).cmp(&(right_numerator * left_denominator))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn gcd(mut left: BigInt, mut right: BigInt) -> BigInt {
    while !right.is_zero() {
        let remainder = left % &right;
        left = right;
        right = remainder;
    }
    left
}

fn gcd_u128(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
struct ExactPoint {
    x: Rational,
    y: Rational,
}

impl ExactPoint {
    fn new(x: Rational, y: Rational) -> Self {
        Self { x, y }
    }

    fn sub(&self, other: &Self) -> Self {
        Self::new(self.x.sub(&other.x), self.y.sub(&other.y))
    }
}

type ExactPath = Vec<ExactPoint>;

#[derive(Clone)]
struct Edge {
    start: ExactPoint,
    end: ExactPoint,
    min_x: Rational,
    max_x: Rational,
    min_y: Rational,
    max_y: Rational,
}

impl Edge {
    fn new(start: ExactPoint, end: ExactPoint) -> Self {
        let min_x = start.x.clone().min(end.x.clone());
        let max_x = start.x.clone().max(end.x.clone());
        let min_y = start.y.clone().min(end.y.clone());
        let max_y = start.y.clone().max(end.y.clone());
        Self { start, end, min_x, max_x, min_y, max_y }
    }
}

#[derive(Clone)]
struct DirectedEdge {
    start: ExactPoint,
    end: ExactPoint,
}

pub(crate) fn boolean_op64(request: BooleanRequest<'_>) -> Result<Paths64, Error> {
    let subjects = exact_paths64(request.subjects);
    let clips = exact_paths64(request.clips);
    let result =
        run_boolean(&subjects, &clips, request.clip_type, request.fill_rule, INTEGER_SAMPLE_BITS)?;
    exact_paths_to_i64(&result)
}

pub(crate) fn boolean_opd(request: BooleanRequestD<'_>) -> Result<PathsD, Error> {
    if let Some(Ok(result)) = crate::fast::try_boolean_opd(request) {
        return Ok(result);
    }
    boolean_opd_exact(request)
}

pub(crate) fn boolean_opd_exact(request: BooleanRequestD<'_>) -> Result<PathsD, Error> {
    let subjects = exact_paths_d(request.subjects)?;
    let clips = exact_paths_d(request.clips)?;
    let result =
        run_boolean(&subjects, &clips, request.clip_type, request.fill_rule, DOUBLE_SAMPLE_BITS)?;
    exact_paths_to_f64(&result)
}

fn exact_paths64(paths: &[Path64]) -> Vec<ExactPath> {
    paths
        .iter()
        .map(|path| {
            normalize_path64(path, crate::PathKind::Closed)
                .into_iter()
                .map(|point| {
                    ExactPoint::new(Rational::from_i64(point.x), Rational::from_i64(point.y))
                })
                .collect::<ExactPath>()
        })
        .filter(|path| path.len() >= 3)
        .collect()
}

fn exact_paths_d(paths: &[PathD]) -> Result<Vec<ExactPath>, Error> {
    paths
        .iter()
        .map(|path| {
            normalize_pathd(path, crate::PathKind::Closed)
                .into_iter()
                .map(|point| {
                    let x = Rational::from_f64(point.x)?;
                    let y = Rational::from_f64(point.y)?;
                    Ok(ExactPoint::new(x, y))
                })
                .collect::<Result<ExactPath, Error>>()
        })
        .filter_map(|path| match path {
            Ok(path) if path.len() >= 3 => Some(Ok(path)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn run_boolean(
    subjects: &[ExactPath],
    clips: &[ExactPath],
    clip_type: ClipType,
    fill_rule: FillRule,
    sample_bits: usize,
) -> Result<Vec<ExactPath>, Error> {
    if let Some(result) = short_circuit(subjects, clips, clip_type, fill_rule) {
        return Ok(result);
    }
    let mut edges = Vec::new();
    for path in subjects.iter().chain(clips) {
        for (start, end) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
            if start != end {
                edges.push(Edge::new(start.clone(), end.clone()));
            }
        }
    }
    if edges.is_empty() {
        return Ok(Vec::new());
    }

    let mut split_parameters = vec![vec![Rational::zero(), Rational::one()]; edges.len()];
    let mut edge_order: Vec<usize> = (0..edges.len()).collect();
    edge_order.sort_unstable_by(|&first, &second| {
        edges[first]
            .min_x
            .cmp(&edges[second].min_x)
            .then_with(|| edges[first].max_x.cmp(&edges[second].max_x))
    });
    let mut active: Vec<usize> = Vec::new();
    for &current in &edge_order {
        let current_min_x = &edges[current].min_x;
        active.retain(|&candidate| edges[candidate].max_x >= *current_min_x);
        for &previous in &active {
            let (first, second) =
                if previous < current { (previous, current) } else { (current, previous) };
            let (before, after) = split_parameters.split_at_mut(second);
            split_edge_pair(&edges[first], &edges[second], &mut before[first], &mut after[0]);
        }
        active.push(current);
    }

    let atomic_edges = split_edges(&edges, &mut split_parameters);
    let epsilon = Rational::new(BigInt::one(), BigInt::one() << sample_bits);
    let mut directed = BTreeSet::new();
    for edge in atomic_edges {
        let midpoint =
            point_at(&edge.start, &edge.end, &Rational::new(BigInt::from(1), BigInt::from(2)));
        let vector = edge.end.sub(&edge.start);
        let left_sample = ExactPoint::new(
            midpoint.x.sub(&vector.y.mul(&epsilon)),
            midpoint.y.add(&vector.x.mul(&epsilon)),
        );
        let right_sample = ExactPoint::new(
            midpoint.x.add(&vector.y.mul(&epsilon)),
            midpoint.y.sub(&vector.x.mul(&epsilon)),
        );
        let left = operation_contains(&left_sample, subjects, clips, clip_type, fill_rule);
        let right = operation_contains(&right_sample, subjects, clips, clip_type, fill_rule);
        if left != right {
            if left {
                directed.insert((edge.start, edge.end));
            } else {
                directed.insert((edge.end, edge.start));
            }
        }
    }

    let directed: Vec<DirectedEdge> =
        directed.into_iter().map(|(start, end)| DirectedEdge { start, end }).collect();
    stitch_directed_edges(&directed)
}

fn short_circuit(
    subjects: &[ExactPath],
    clips: &[ExactPath],
    clip_type: ClipType,
    fill_rule: FillRule,
) -> Option<Vec<ExactPath>> {
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
    let subjects_box = paths_bbox(subjects)?;
    let clips_box = paths_bbox(clips)?;
    let disjoint = subjects_box.2 < clips_box.0
        || clips_box.2 < subjects_box.0
        || subjects_box.3 < clips_box.1
        || clips_box.3 < subjects_box.1;
    if disjoint {
        if !paths_are_simple_and_disjoint(subjects) || !paths_are_simple_and_disjoint(clips) {
            return None;
        }
        return match clip_type {
            ClipType::Intersection => Some(Vec::new()),
            ClipType::Difference | ClipType::Union | ClipType::Xor => {
                let mut result = direct_paths(subjects);
                if matches!(clip_type, ClipType::Union | ClipType::Xor) {
                    result.extend(direct_paths(clips));
                }
                result.sort();
                Some(result)
            }
        };
    }
    let width = subjects_box.2.min(clips_box.2).sub(&subjects_box.0.max(clips_box.0));
    let height = subjects_box.3.min(clips_box.3).sub(&subjects_box.1.max(clips_box.1));
    if (width.is_zero() || height.is_zero()) && !sets_have_segment_contact(subjects, clips) {
        if !paths_are_simple_and_disjoint(subjects) || !paths_are_simple_and_disjoint(clips) {
            return None;
        }
        return match clip_type {
            ClipType::Intersection => Some(Vec::new()),
            ClipType::Difference | ClipType::Union | ClipType::Xor => {
                let mut result = direct_paths(subjects);
                if matches!(clip_type, ClipType::Union | ClipType::Xor) {
                    result.extend(direct_paths(clips));
                }
                result.sort();
                Some(result)
            }
        };
    }
    None
}

fn sets_have_segment_contact(subjects: &[ExactPath], clips: &[ExactPath]) -> bool {
    subjects.iter().flat_map(path_edges).any(|subject_edge| {
        clips.iter().flat_map(path_edges).any(|clip_edge| {
            let first_vector = subject_edge.end.sub(&subject_edge.start);
            let second_vector = clip_edge.end.sub(&clip_edge.start);
            let between = clip_edge.start.sub(&subject_edge.start);
            if !cross_vectors(&first_vector, &second_vector).is_zero()
                || !cross_vectors(&between, &first_vector).is_zero()
            {
                return false;
            }
            let shared = [
                subject_edge.start.clone(),
                subject_edge.end.clone(),
                clip_edge.start.clone(),
                clip_edge.end.clone(),
            ]
            .into_iter()
            .filter(|point| {
                point_on_segment_exact(point, &subject_edge.start, &subject_edge.end)
                    && point_on_segment_exact(point, &clip_edge.start, &clip_edge.end)
            })
            .collect::<BTreeSet<_>>();
            shared.len() >= 2
        })
    })
}

fn direct_if_simple_and_disjoint(paths: &[ExactPath]) -> Option<Vec<ExactPath>> {
    paths_are_simple_and_disjoint(paths).then(|| direct_paths(paths))
}

fn direct_paths(paths: &[ExactPath]) -> Vec<ExactPath> {
    paths
        .iter()
        .cloned()
        .map(|mut path| {
            if exact_area2(&path).is_negative() {
                path.reverse();
            }
            canonicalize_ring(&mut path);
            path
        })
        .collect()
}

#[rustfmt::skip]
fn paths_are_simple_and_disjoint(paths: &[ExactPath]) -> bool {
    for (index, path) in paths.iter().enumerate() {
        if path.len() < 3 {
            continue;
        }
        let edges = path_edges(path);
        for (first, edge) in edges.iter().enumerate() {
            for other in edges.iter().skip(first + 1) {
                if edges_intersect(edge, other) {
                    return false;
                }
            }
        }
        let path_box = path_bbox(path).expect("paths with at least three points have a bbox");
        for other in paths.iter().skip(index + 1) {
            let Some(other_box) = path_bbox(other) else { continue };
            if boxes_touch_or_overlap(&path_box, &other_box) { return false; }
        }
    }
    true
}

fn path_edges(path: &ExactPath) -> Vec<Edge> {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .filter(|(start, end)| start != end)
        .map(|(start, end)| Edge::new(start.clone(), end.clone()))
        .collect()
}

fn edges_intersect(first: &Edge, second: &Edge) -> bool {
    let first_vector = first.end.sub(&first.start);
    let second_vector = second.end.sub(&second.start);
    let between = second.start.sub(&first.start);
    if cross_vectors(&first_vector, &second_vector).is_zero()
        && cross_vectors(&between, &first_vector).is_zero()
    {
        return true;
    }
    let mut first_params = vec![Rational::zero(), Rational::one()];
    let mut second_params = vec![Rational::zero(), Rational::one()];
    split_edge_pair(first, second, &mut first_params, &mut second_params);
    first_params.len() > 2 || second_params.len() > 2
}

fn paths_bbox(paths: &[ExactPath]) -> Option<(Rational, Rational, Rational, Rational)> {
    paths.iter().filter_map(path_bbox).fold(None, |current, next| {
        Some(match current {
            None => next,
            Some((min_x, min_y, max_x, max_y)) => {
                (min_x.min(next.0), min_y.min(next.1), max_x.max(next.2), max_y.max(next.3))
            }
        })
    })
}

fn path_bbox(path: &ExactPath) -> Option<(Rational, Rational, Rational, Rational)> {
    let first = path.first()?;
    let mut bounds = (first.x.clone(), first.y.clone(), first.x.clone(), first.y.clone());
    for point in path.iter().skip(1) {
        bounds.0 = bounds.0.min(point.x.clone());
        bounds.1 = bounds.1.min(point.y.clone());
        bounds.2 = bounds.2.max(point.x.clone());
        bounds.3 = bounds.3.max(point.y.clone());
    }
    Some(bounds)
}

fn boxes_touch_or_overlap(
    first: &(Rational, Rational, Rational, Rational),
    second: &(Rational, Rational, Rational, Rational),
) -> bool {
    !(first.2 < second.0 || second.2 < first.0 || first.3 < second.1 || second.3 < first.1)
}

fn split_edge_pair(
    first: &Edge,
    second: &Edge,
    first_params: &mut Vec<Rational>,
    second_params: &mut Vec<Rational>,
) {
    if first.max_x < second.min_x
        || second.max_x < first.min_x
        || first.max_y < second.min_y
        || second.max_y < first.min_y
    {
        return;
    }
    let first_vector = first.end.sub(&first.start);
    let second_vector = second.end.sub(&second.start);
    let denominator = cross_vectors(&first_vector, &second_vector);
    let between = second.start.sub(&first.start);
    if !denominator.is_zero() {
        let first_t = cross_vectors(&between, &second_vector).div(&denominator);
        let second_t = cross_vectors(&between, &first_vector).div(&denominator);
        if is_unit_interval(&first_t) && is_unit_interval(&second_t) {
            push_unique(first_params, first_t.clone());
            push_unique(second_params, second_t);
        }
    } else if cross_vectors(&between, &first_vector).is_zero() {
        for point in [&second.start, &second.end] {
            if point_on_segment_exact(point, &first.start, &first.end) {
                push_unique(first_params, parameter_on_segment(point, &first.start, &first.end));
            }
        }
        for point in [&first.start, &first.end] {
            if point_on_segment_exact(point, &second.start, &second.end) {
                push_unique(second_params, parameter_on_segment(point, &second.start, &second.end));
            }
        }
    }
}

fn push_unique(values: &mut Vec<Rational>, value: Rational) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn is_unit_interval(value: &Rational) -> bool {
    value >= &Rational::zero() && value <= &Rational::one()
}

fn parameter_on_segment(point: &ExactPoint, start: &ExactPoint, end: &ExactPoint) -> Rational {
    let vector = end.sub(start);
    if vector.x.is_zero() {
        point.y.sub(&start.y).div(&vector.y)
    } else {
        point.x.sub(&start.x).div(&vector.x)
    }
}

fn point_at(start: &ExactPoint, end: &ExactPoint, parameter: &Rational) -> ExactPoint {
    let vector = end.sub(start);
    ExactPoint::new(start.x.add(&vector.x.mul(parameter)), start.y.add(&vector.y.mul(parameter)))
}

fn split_edges(edges: &[Edge], parameters: &mut [Vec<Rational>]) -> Vec<Edge> {
    let mut result = Vec::new();
    for (edge, values) in edges.iter().zip(parameters.iter_mut()) {
        values.sort();
        values.dedup();
        for pair in values.windows(2) {
            let start = point_at(&edge.start, &edge.end, &pair[0]);
            let end = point_at(&edge.start, &edge.end, &pair[1]);
            if start != end {
                result.push(Edge::new(start, end));
            }
        }
    }
    result
}

fn cross_vectors(first: &ExactPoint, second: &ExactPoint) -> Rational {
    first.x.mul(&second.y).sub(&first.y.mul(&second.x))
}

fn point_on_segment_exact(point: &ExactPoint, start: &ExactPoint, end: &ExactPoint) -> bool {
    if !cross_vectors(&point.sub(start), &end.sub(start)).is_zero() {
        return false;
    }
    let x_min = if start.x <= end.x { start.x.clone() } else { end.x.clone() };
    let x_max = if start.x >= end.x { start.x.clone() } else { end.x.clone() };
    let y_min = if start.y <= end.y { start.y.clone() } else { end.y.clone() };
    let y_max = if start.y >= end.y { start.y.clone() } else { end.y.clone() };
    let x_in_range = point.x >= x_min && point.x <= x_max;
    let y_in_range = point.y >= y_min && point.y <= y_max;
    x_in_range && y_in_range
}

fn operation_contains(
    point: &ExactPoint,
    subjects: &[ExactPath],
    clips: &[ExactPath],
    clip_type: ClipType,
    fill_rule: FillRule,
) -> bool {
    let subject = paths_contain(point, subjects, fill_rule);
    let clip = paths_contain(point, clips, fill_rule);
    match clip_type {
        ClipType::Intersection => subject && clip,
        ClipType::Union => subject || clip,
        ClipType::Difference => subject && !clip,
        ClipType::Xor => subject != clip,
    }
}

fn paths_contain(point: &ExactPoint, paths: &[ExactPath], fill_rule: FillRule) -> bool {
    let mut parity = false;
    let mut winding = 0_i128;
    for path in paths {
        for (start, end) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
            if point_on_segment_exact(point, start, end) {
                return true;
            }
            if (start.y > point.y) != (end.y > point.y) {
                let cross = cross_vectors(&end.sub(start), &point.sub(start));
                if end.y > start.y {
                    if cross.is_positive() {
                        parity = !parity;
                        winding += 1;
                    }
                } else if cross.is_negative() {
                    parity = !parity;
                    winding -= 1;
                }
            }
        }
    }
    match fill_rule {
        FillRule::EvenOdd => parity,
        FillRule::NonZero => winding != 0,
        FillRule::Positive => winding > 0,
        FillRule::Negative => winding < 0,
    }
}

fn stitch_directed_edges(edges: &[DirectedEdge]) -> Result<Vec<ExactPath>, Error> {
    if edges.is_empty() {
        return Ok(Vec::new());
    }
    let mut outgoing: BTreeMap<ExactPoint, Vec<usize>> = BTreeMap::new();
    for (index, edge) in edges.iter().enumerate() {
        outgoing.entry(edge.start.clone()).or_default().push(index);
    }
    for (origin, indices) in &mut outgoing {
        indices.sort_by(|left, right| {
            let left_direction = edges[*left].end.sub(origin);
            let right_direction = edges[*right].end.sub(origin);
            compare_angle(&left_direction, &right_direction)
        });
    }

    let mut next = vec![0_usize; edges.len()];
    for (index, edge) in edges.iter().enumerate() {
        let candidates = outgoing.get(&edge.end).ok_or(Error::TopologyFailure)?;
        let reverse_direction = edge.start.sub(&edge.end);
        let insertion = candidates
            .iter()
            .position(|candidate| {
                let direction = edges[*candidate].end.sub(&edges[*candidate].start);
                compare_angle(&direction, &reverse_direction) != Ordering::Less
            })
            .unwrap_or(candidates.len());
        next[index] = candidates[(insertion + candidates.len() - 1) % candidates.len()];
    }

    let mut visited = vec![false; edges.len()];
    let mut rings = Vec::new();
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        let mut ring = Vec::new();
        let mut current = start;
        loop {
            if visited[current] {
                if current != start {
                    return Err(Error::TopologyFailure);
                }
                break;
            }
            visited[current] = true;
            ring.push(edges[current].start.clone());
            current = next[current];
        }
        if ring.len() >= 3 && !exact_area2(&ring).is_zero() {
            canonicalize_ring(&mut ring);
            rings.push(ring);
        }
    }
    rings.sort();
    Ok(rings)
}

fn compare_angle(first: &ExactPoint, second: &ExactPoint) -> Ordering {
    let first_upper =
        first.y > Rational::zero() || (first.y.is_zero() && first.x >= Rational::zero());
    let second_upper =
        second.y > Rational::zero() || (second.y.is_zero() && second.x >= Rational::zero());
    if first_upper != second_upper {
        return second_upper.cmp(&first_upper);
    }
    let cross = cross_vectors(first, second);
    if !cross.is_zero() {
        return if cross.is_positive() { Ordering::Less } else { Ordering::Greater };
    }
    let first_length = first.x.mul(&first.x).add(&first.y.mul(&first.y));
    let second_length = second.x.mul(&second.x).add(&second.y.mul(&second.y));
    first_length.cmp(&second_length)
}

fn exact_area2(path: &[ExactPoint]) -> Rational {
    path.iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
        .fold(Rational::zero(), |area, (start, end)| {
            area.add(&start.x.mul(&end.y).sub(&start.y.mul(&end.x)))
        })
}

fn canonicalize_ring(ring: &mut ExactPath) {
    if let Some((minimum, _)) =
        ring.iter().enumerate().min_by(|(_, left), (_, right)| left.cmp(right))
    {
        ring.rotate_left(minimum);
    }
}

fn exact_paths_to_i64(paths: &[ExactPath]) -> Result<Paths64, Error> {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|point| {
                    Ok(Point64::new(
                        point.x.to_i64().ok_or(Error::NonIntegralResult)?,
                        point.y.to_i64().ok_or(Error::NonIntegralResult)?,
                    ))
                })
                .collect::<Result<Path64, Error>>()
        })
        .collect()
}

fn exact_paths_to_f64(paths: &[ExactPath]) -> Result<PathsD, Error> {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|point| {
                    Ok(PointD::new(
                        point.x.to_f64().ok_or(Error::ArithmeticOverflow)?,
                        point.y.to_f64().ok_or(Error::ArithmeticOverflow)?,
                    ))
                })
                .collect::<Result<PathD, Error>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PointLocation, boolean_op, point_in_polygon, signed_area2};

    fn rectangle(left: i64, bottom: i64, right: i64, top: i64) -> Path64 {
        vec![
            Point64::new(left, bottom),
            Point64::new(right, bottom),
            Point64::new(right, top),
            Point64::new(left, top),
        ]
    }

    fn request<'a>(
        subjects: &'a [Path64],
        clips: &'a [Path64],
        clip_type: ClipType,
    ) -> BooleanRequest<'a> {
        BooleanRequest { subjects, clips, clip_type, fill_rule: FillRule::EvenOdd }
    }

    fn area_sum(paths: &[Path64]) -> i128 {
        paths.iter().map(|path| signed_area2(path).expect("test area")).sum()
    }

    fn canonical_summary(paths: &[Path64]) -> Vec<(i128, Vec<(i64, i64)>)> {
        let mut summary = paths
            .iter()
            .map(|path| {
                let area = signed_area2(path).expect("test area");
                let points = path.iter().map(|point| (point.x, point.y)).collect::<Vec<_>>();
                let forward = rotate_to_minimum(points.clone());
                let mut reversed = points;
                reversed.reverse();
                let reversed = rotate_to_minimum(reversed);
                (area, if forward < reversed { forward } else { reversed })
            })
            .collect::<Vec<_>>();
        summary.sort();
        summary
    }

    fn rotate_to_minimum(mut points: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
        if let Some((minimum, _)) = points.iter().enumerate().min_by_key(|(_, point)| *point) {
            points.rotate_left(minimum);
        }
        points
    }

    #[test]
    fn all_boolean_operations_have_expected_area_and_topology() {
        let subject = rectangle(0, 0, 10, 10);
        let clip = rectangle(5, 0, 15, 10);
        let subjects = [subject.clone()];
        let clips = [clip.clone()];

        let intersection = boolean_op(request(&subjects, &clips, ClipType::Intersection)).unwrap();
        assert_eq!(area_sum(&intersection), 100);
        assert_eq!(intersection.len(), 1);
        assert_eq!(
            point_in_polygon(Point64::new(7, 5), &intersection[0]),
            Ok(PointLocation::Inside)
        );

        let union = boolean_op(request(&subjects, &clips, ClipType::Union)).unwrap();
        assert_eq!(area_sum(&union), 300);
        assert_eq!(union.len(), 1);
        assert_eq!(point_in_polygon(Point64::new(2, 5), &union[0]), Ok(PointLocation::Inside));
        assert_eq!(point_in_polygon(Point64::new(13, 5), &union[0]), Ok(PointLocation::Inside));

        let difference = boolean_op(request(&subjects, &clips, ClipType::Difference)).unwrap();
        assert_eq!(area_sum(&difference), 100);
        assert_eq!(difference.len(), 1);
        assert_eq!(point_in_polygon(Point64::new(2, 5), &difference[0]), Ok(PointLocation::Inside));

        let xor = boolean_op(request(&subjects, &clips, ClipType::Xor)).unwrap();
        assert_eq!(area_sum(&xor), 200);
        assert_eq!(xor.len(), 2);
    }

    #[test]
    fn handles_holes_containment_and_reversed_input() {
        let outer = rectangle(0, 0, 20, 20);
        let mut hole = rectangle(5, 5, 15, 15);
        hole.reverse();
        let subjects = [outer.clone()];
        let clips = [hole.clone()];
        let result = boolean_op(request(&subjects, &clips, ClipType::Difference)).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(area_sum(&result), 600);
        assert!(result.iter().any(|path| signed_area2(path).unwrap() > 0));
        assert!(result.iter().any(|path| signed_area2(path).unwrap() < 0));

        let union = boolean_op(request(&[outer], &[], ClipType::Union)).unwrap();
        assert_eq!(union.len(), 1);
        assert_eq!(area_sum(&union), 800);
    }

    #[test]
    fn handles_empty_disjoint_touching_and_identical_inputs() {
        let subject = rectangle(0, 0, 10, 10);
        let disjoint = rectangle(20, 0, 30, 10);
        let subjects = [subject.clone()];
        let clips = [disjoint.clone()];
        assert!(boolean_op(request(&[], &clips, ClipType::Intersection)).unwrap().is_empty());
        assert_eq!(boolean_op(request(&subjects, &[], ClipType::Difference)).unwrap().len(), 1);
        assert_eq!(boolean_op(request(&subjects, &clips, ClipType::Union)).unwrap().len(), 2);
        assert!(boolean_op(request(&subjects, &subjects, ClipType::Xor)).unwrap().is_empty());

        let touching = rectangle(10, 0, 20, 10);
        let touching_paths = [touching];
        let union = boolean_op(request(&subjects, &touching_paths, ClipType::Union)).unwrap();
        assert_eq!(area_sum(&union), 400);
        assert_eq!(union.len(), 1);
    }

    #[test]
    fn boolean_algebra_properties_hold_for_deterministic_rectangles() {
        for offset in -3..=3 {
            let subject = rectangle(0, 0, 10, 10);
            let clip = rectangle(offset * 3, 2, offset * 3 + 8, 12);
            let subjects = [subject.clone()];
            let clips = [clip.clone()];

            let union = boolean_op(request(&subjects, &clips, ClipType::Union)).unwrap();
            let reverse_union = boolean_op(request(&clips, &subjects, ClipType::Union)).unwrap();
            assert_eq!(canonical_summary(&union), canonical_summary(&reverse_union));

            let intersection =
                boolean_op(request(&subjects, &clips, ClipType::Intersection)).unwrap();
            let reverse_intersection =
                boolean_op(request(&clips, &subjects, ClipType::Intersection)).unwrap();
            assert_eq!(canonical_summary(&intersection), canonical_summary(&reverse_intersection));

            let xor = boolean_op(request(&subjects, &clips, ClipType::Xor)).unwrap();
            let reverse_xor = boolean_op(request(&clips, &subjects, ClipType::Xor)).unwrap();
            assert_eq!(canonical_summary(&xor), canonical_summary(&reverse_xor));

            assert_eq!(
                canonical_summary(
                    &boolean_op(request(&subjects, &subjects, ClipType::Union)).unwrap()
                ),
                canonical_summary(&subjects)
            );
            assert_eq!(
                canonical_summary(
                    &boolean_op(request(&subjects, &subjects, ClipType::Intersection)).unwrap()
                ),
                canonical_summary(&subjects)
            );
            assert!(boolean_op(request(&subjects, &subjects, ClipType::Xor)).unwrap().is_empty());
            assert_eq!(
                canonical_summary(
                    &boolean_op(request(&subjects, &[], ClipType::Difference)).unwrap()
                ),
                canonical_summary(&subjects)
            );
            assert!(
                boolean_op(request(&subjects, &[], ClipType::Intersection)).unwrap().is_empty()
            );
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn exercises_rational_numeric_boundaries() {
        assert!(matches!(Rational::from_f64(f64::NAN), Err(Error::NonFiniteCoordinate { .. })));
        let _ = Rational::from_f64(0.5).expect("normal negative exponent");
        let _ = Rational::from_f64(-0.5).expect("negative mantissa");
        let _ = Rational::from_f64(4_503_599_627_370_496.0).expect("zero exponent");
        let _ = Rational::from_f64(2.0_f64.powi(127)).expect("big positive exponent");
        let _ = Rational::from_f64(f64::from_bits(1)).expect("subnormal exponent");

        assert!(Rational::new_small(1, 0).is_none());
        assert!(Rational::new_small(1, i128::MIN).is_none());
        assert!(Rational::new_small(i128::MIN, -1).is_none());
        assert!(Rational::new_small(0, -7).is_some());
        let negative = Rational::new_small(2, -4).expect("negative denominator");
        assert_eq!(negative.to_f64(), Some(-0.5));
        let min = Rational::Small { numerator: i128::MIN, denominator: 1 };
        let _ = min.neg();
        let big = Rational::Big { numerator: BigInt::from(1), denominator: BigInt::from(2) };
        assert_eq!(big.to_i64(), None);
        assert!(big.to_f64().is_some());
        let big_integer = Rational::Big { numerator: BigInt::from(7), denominator: BigInt::one() };
        assert_eq!(big_integer.to_i64(), Some(7));
        let _ = Rational::from_i128(i128::MIN, -1);
        let _ = Rational::new(BigInt::from(1), BigInt::from(-2));
        let huge = Rational::new(BigInt::from(1) << 200, BigInt::from(3));
        assert!(matches!(huge, Rational::Big { .. }));

        let left = Rational::Small { numerator: i128::MAX, denominator: 1 };
        let right = Rational::Small { numerator: 2, denominator: 1 };
        let _ = left.add(&right);
        let _ = left.mul(&right);
        let _ = left.div(&right);
        let _ = left.cmp(&right);
        assert!(Rational::zero().is_zero());
        assert!(!Rational::zero().is_positive());
        assert!(Rational::from_i64(1).is_positive());
        assert!(Rational::from_i64(-1).is_negative());

        let nan_path = [vec![PointD::new(f64::NAN, 0.0), PointD::new(1.0, 0.0)]];
        assert!(exact_paths_d(&nan_path).is_err());
        assert!(exact_paths_d(&[Vec::new()]).unwrap().is_empty());
        let empty = run_boolean(&[], &[], ClipType::Union, FillRule::EvenOdd, 8).unwrap();
        assert!(empty.is_empty());
    }

    fn exact_point(x: i64, y: i64) -> ExactPoint {
        ExactPoint::new(Rational::from_i64(x), Rational::from_i64(y))
    }

    fn exact_edge(start: (i64, i64), end: (i64, i64)) -> Edge {
        Edge::new(exact_point(start.0, start.1), exact_point(end.0, end.1))
    }

    fn exact_directed(start: (i64, i64), end: (i64, i64)) -> DirectedEdge {
        DirectedEdge { start: exact_point(start.0, start.1), end: exact_point(end.0, end.1) }
    }

    #[test]
    #[allow(
        clippy::bool_assert_comparison,
        clippy::cloned_ref_to_slice_refs,
        clippy::too_many_lines
    )]
    fn exercises_exact_kernel_predicates_and_fallbacks() {
        let subject = exact_paths64(&[rectangle(0, 0, 10, 10)]);
        let clip = exact_paths64(&[rectangle(5, 0, 15, 10)]);
        assert_eq!(
            exact_paths64(&[Vec::new(), vec![Point64::new(0, 0), Point64::new(1, 0)]]).len(),
            0
        );
        assert!(exact_paths_d(&[Vec::new()]).unwrap().is_empty());
        assert!(
            exact_paths_d(&[[
                PointD::new(0.0, 0.0),
                PointD::new(f64::NAN, 0.0),
                PointD::new(1.0, 1.0),
            ]
            .to_vec()])
            .is_err()
        );

        let huge = Rational::from_f64(f64::MAX).expect("finite f64 becomes a big rational");
        assert!(matches!(huge, Rational::Big { .. }));
        assert!(Rational::Small { numerator: 3, denominator: 2 }.to_i64().is_none());
        assert!(
            Rational::Big { numerator: BigInt::one() << 2_000, denominator: BigInt::one() }
                .to_f64()
                .is_none()
        );
        let big_a =
            Rational::Big { numerator: BigInt::from(1) << 200, denominator: BigInt::from(3) };
        let big_b =
            Rational::Big { numerator: BigInt::from(2) << 200, denominator: BigInt::from(5) };
        let _ = big_a.add(&big_b);
        let _ = big_a.mul(&big_b);
        let _ = big_a.div(&big_b);
        assert!(big_a > Rational::from_i64(1));

        assert!(short_circuit(&[], &[], ClipType::Union, FillRule::EvenOdd).is_some());
        assert!(short_circuit(&subject, &[], ClipType::Intersection, FillRule::EvenOdd).is_some());
        assert!(short_circuit(&subject, &[], ClipType::Difference, FillRule::EvenOdd).is_some());
        assert!(short_circuit(&subject, &[], ClipType::Union, FillRule::EvenOdd).is_some());
        assert!(short_circuit(&subject, &[], ClipType::Xor, FillRule::EvenOdd).is_some());
        for clip_type in
            [ClipType::Intersection, ClipType::Difference, ClipType::Union, ClipType::Xor]
        {
            assert!(short_circuit(&[], &clip, clip_type, FillRule::EvenOdd).is_some());
        }
        assert!(short_circuit(&subject, &clip, ClipType::Union, FillRule::Positive).is_none());

        let disjoint = exact_paths64(&[rectangle(20, 0, 30, 10)]);
        let vertical_disjoint = exact_paths64(&[rectangle(0, 20, 10, 30)]);
        let reverse_vertical_disjoint = exact_paths64(&[rectangle(0, -30, 10, -20)]);
        for clip_type in
            [ClipType::Intersection, ClipType::Difference, ClipType::Union, ClipType::Xor]
        {
            assert!(short_circuit(&subject, &disjoint, clip_type, FillRule::EvenOdd).is_some());
            assert!(
                short_circuit(&subject, &vertical_disjoint, clip_type, FillRule::EvenOdd).is_some()
            );
            assert!(
                short_circuit(&subject, &reverse_vertical_disjoint, clip_type, FillRule::EvenOdd)
                    .is_some()
            );
        }
        let invalid =
            vec![exact_point(0, 0), exact_point(10, 10), exact_point(0, 10), exact_point(10, 0)];
        assert!(
            short_circuit(&[invalid.clone()], &disjoint, ClipType::Union, FillRule::EvenOdd)
                .is_none()
        );
        let corner_touch = exact_paths64(&[rectangle(10, 10, 20, 20)]);
        assert!(
            short_circuit(&[invalid.clone()], &corner_touch, ClipType::Union, FillRule::EvenOdd)
                .is_none()
        );
        let invalid_far = vec![
            exact_point(20, 20),
            exact_point(30, 30),
            exact_point(20, 30),
            exact_point(30, 20),
        ];
        assert!(
            short_circuit(&subject, &[invalid_far], ClipType::Union, FillRule::EvenOdd).is_none()
        );
        let invalid_touch = vec![
            exact_point(10, 10),
            exact_point(20, 20),
            exact_point(10, 20),
            exact_point(20, 10),
        ];
        assert!(
            short_circuit(&subject, &[invalid_touch], ClipType::Union, FillRule::EvenOdd).is_none()
        );
        let height_only_touch = vec![exact_point(2, 10), exact_point(8, 20), exact_point(2, 20)];
        assert!(
            short_circuit(&subject, &[height_only_touch], ClipType::Union, FillRule::EvenOdd)
                .is_some()
        );
        for clip_type in
            [ClipType::Intersection, ClipType::Difference, ClipType::Union, ClipType::Xor]
        {
            assert!(short_circuit(&subject, &corner_touch, clip_type, FillRule::EvenOdd).is_some());
        }
        assert!(sets_have_segment_contact(&subject, &clip));
        assert!(!sets_have_segment_contact(&subject, &corner_touch));

        let clockwise =
            vec![exact_point(0, 0), exact_point(0, 10), exact_point(10, 10), exact_point(10, 0)];
        assert_eq!(direct_paths(&[clockwise])[0][0], exact_point(0, 0));
        assert!(paths_are_simple_and_disjoint(&[Vec::new()]));
        assert!(paths_are_simple_and_disjoint(&[subject[0].clone(), Vec::new()]));
        assert!(paths_are_simple_and_disjoint(&[subject[0].clone(), disjoint[0].clone()]));
        assert!(!paths_are_simple_and_disjoint(&[subject[0].clone(), subject[0].clone()]));
        assert!(paths_bbox(&[]).is_none());
        assert!(paths_bbox(&[subject[0].clone(), clip[0].clone()]).is_some());
        assert!(boxes_touch_or_overlap(
            &(
                Rational::from_i64(0),
                Rational::from_i64(0),
                Rational::from_i64(1),
                Rational::from_i64(1)
            ),
            &(
                Rational::from_i64(1),
                Rational::from_i64(1),
                Rational::from_i64(2),
                Rational::from_i64(2)
            ),
        ));
        assert!(!boxes_touch_or_overlap(
            &(
                Rational::from_i64(0),
                Rational::from_i64(0),
                Rational::from_i64(1),
                Rational::from_i64(1)
            ),
            &(
                Rational::from_i64(2),
                Rational::from_i64(2),
                Rational::from_i64(3),
                Rational::from_i64(3)
            ),
        ));
        assert!(!boxes_touch_or_overlap(
            &(
                Rational::from_i64(2),
                Rational::from_i64(0),
                Rational::from_i64(3),
                Rational::from_i64(1)
            ),
            &(
                Rational::from_i64(0),
                Rational::from_i64(0),
                Rational::from_i64(1),
                Rational::from_i64(1)
            ),
        ));
        assert!(!boxes_touch_or_overlap(
            &(
                Rational::from_i64(0),
                Rational::from_i64(2),
                Rational::from_i64(1),
                Rational::from_i64(3)
            ),
            &(
                Rational::from_i64(0),
                Rational::from_i64(0),
                Rational::from_i64(1),
                Rational::from_i64(1)
            ),
        ));
        assert!(!boxes_touch_or_overlap(
            &(
                Rational::from_i64(0),
                Rational::from_i64(0),
                Rational::from_i64(1),
                Rational::from_i64(1)
            ),
            &(
                Rational::from_i64(0),
                Rational::from_i64(2),
                Rational::from_i64(1),
                Rational::from_i64(3)
            ),
        ));

        let crossing_first = exact_edge((0, 0), (10, 10));
        let crossing_second = exact_edge((0, 10), (10, 0));
        assert!(edges_intersect(&crossing_first, &crossing_second));
        assert!(edges_intersect(&exact_edge((0, 0), (10, 0)), &exact_edge((5, 0), (15, 0))));
        assert!(!edges_intersect(&exact_edge((0, 0), (1, 0)), &exact_edge((2, 1), (3, 1))));
        let mut first_parameters = vec![Rational::zero(), Rational::one()];
        let mut second_parameters = vec![Rational::zero(), Rational::one()];
        split_edge_pair(
            &crossing_first,
            &crossing_second,
            &mut first_parameters,
            &mut second_parameters,
        );
        assert_eq!(first_parameters.len(), 3);
        split_edge_pair(
            &exact_edge((0, 0), (1, 0)),
            &exact_edge((0, 1), (1, 1)),
            &mut Vec::new(),
            &mut Vec::new(),
        );
        split_edge_pair(
            &exact_edge((0, 0), (10, 0)),
            &exact_edge((15, -1), (14, 1)),
            &mut Vec::new(),
            &mut Vec::new(),
        );
        split_edge_pair(
            &exact_edge((0, 0), (10, 0)),
            &exact_edge((5, 1), (4, 2)),
            &mut Vec::new(),
            &mut Vec::new(),
        );
        let mut collinear_first = vec![Rational::zero(), Rational::one()];
        let mut collinear_second = vec![Rational::zero(), Rational::one()];
        split_edge_pair(
            &exact_edge((0, 0), (10, 0)),
            &exact_edge((5, 0), (15, 0)),
            &mut collinear_first,
            &mut collinear_second,
        );
        assert_eq!(collinear_first.len(), 3);
        assert_eq!(collinear_second.len(), 3);
        assert_eq!(
            parameter_on_segment(&exact_point(5, 0), &exact_point(0, 0), &exact_point(10, 0)),
            Rational::from_i64(1).div(&Rational::from_i64(2))
        );
        assert_eq!(
            parameter_on_segment(&exact_point(0, 5), &exact_point(0, 0), &exact_point(0, 10)),
            Rational::from_i64(1).div(&Rational::from_i64(2))
        );
        assert_eq!(
            point_at(
                &exact_point(0, 0),
                &exact_point(10, 10),
                &Rational::from_i64(1).div(&Rational::from_i64(2))
            ),
            exact_point(5, 5)
        );
        let mut duplicate = vec![Rational::zero()];
        push_unique(&mut duplicate, Rational::zero());
        assert_eq!(duplicate.len(), 1);
        assert!(is_unit_interval(&Rational::zero()));
        assert!(!is_unit_interval(&Rational::from_i64(2)));
        assert!(!is_unit_interval(&Rational::from_i64(-1)));
        let split = split_edges(
            &[exact_edge((0, 0), (10, 0))],
            &mut [vec![Rational::zero(), Rational::from_i64(1), Rational::one()]],
        );
        assert_eq!(split.len(), 1);
        assert!(
            split_edges(
                &[exact_edge((0, 0), (0, 0))],
                &mut [vec![Rational::zero(), Rational::one()]],
            )
            .is_empty()
        );
        assert!(!point_on_segment_exact(
            &exact_point(1, 1),
            &exact_point(0, 0),
            &exact_point(10, 0)
        ));
        assert!(!point_on_segment_exact(
            &exact_point(11, 0),
            &exact_point(0, 0),
            &exact_point(10, 0)
        ));
        assert!(point_on_segment_exact(
            &exact_point(5, 0),
            &exact_point(10, 0),
            &exact_point(0, 0)
        ));

        let inside = exact_point(5, 5);
        let outside = exact_point(20, 20);
        assert!(paths_contain(&inside, &subject, FillRule::EvenOdd));
        assert!(paths_contain(&inside, &subject, FillRule::NonZero));
        assert!(paths_contain(&inside, &subject, FillRule::Positive));
        assert!(!paths_contain(&inside, &subject, FillRule::Negative));
        assert!(paths_contain(&exact_point(0, 0), &subject, FillRule::EvenOdd));
        assert!(!paths_contain(&outside, &subject, FillRule::EvenOdd));
        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            let _ = operation_contains(&inside, &subject, &disjoint, clip_type, FillRule::EvenOdd);
        }
        assert_eq!(
            operation_contains(&inside, &subject, &[], ClipType::Difference, FillRule::EvenOdd),
            true
        );

        assert!(stitch_directed_edges(&[]).unwrap().is_empty());
        let square = [
            exact_directed((0, 0), (10, 0)),
            exact_directed((10, 0), (10, 10)),
            exact_directed((10, 10), (0, 10)),
            exact_directed((0, 10), (0, 0)),
        ];
        assert_eq!(stitch_directed_edges(&square).unwrap().len(), 1);
        assert!(stitch_directed_edges(&[exact_directed((0, 0), (1, 0))]).is_err());
        let non_start_cycle = [
            exact_directed((0, 0), (1, 0)),
            exact_directed((1, 0), (2, 0)),
            exact_directed((2, 0), (1, 0)),
        ];
        assert!(stitch_directed_edges(&non_start_cycle).is_err());
        let collinear_cycle = [
            exact_directed((0, 0), (1, 0)),
            exact_directed((1, 0), (2, 0)),
            exact_directed((2, 0), (0, 0)),
        ];
        assert!(stitch_directed_edges(&collinear_cycle).unwrap().is_empty());
        let short_cycle = [exact_directed((0, 0), (1, 0)), exact_directed((1, 0), (0, 0))];
        assert!(stitch_directed_edges(&short_cycle).unwrap().is_empty());
        assert_eq!(compare_angle(&exact_point(1, 0), &exact_point(0, 1)), Ordering::Less);
        assert_eq!(compare_angle(&exact_point(0, -1), &exact_point(1, 0)), Ordering::Greater);
        assert_eq!(compare_angle(&exact_point(1, 0), &exact_point(2, 0)), Ordering::Less);
        assert_eq!(exact_area2(&[]), Rational::zero());
        let mut ring = vec![exact_point(2, 2), exact_point(0, 0), exact_point(1, 1)];
        canonicalize_ring(&mut ring);
        assert_eq!(ring[0], exact_point(0, 0));
        canonicalize_ring(&mut Vec::new());

        assert!(
            run_boolean(&[vec![]], &[vec![]], ClipType::Union, FillRule::EvenOdd, 8)
                .unwrap()
                .is_empty()
        );
        assert!(run_boolean(&subject, &clip, ClipType::Intersection, FillRule::EvenOdd, 8).is_ok());
        let repeated =
            vec![exact_point(0, 0), exact_point(0, 0), exact_point(10, 0), exact_point(10, 10)];
        let _ = run_boolean(&[repeated], &clip, ClipType::Union, FillRule::EvenOdd, 8);
        assert!(
            exact_paths_to_i64(&[vec![ExactPoint::new(
                Rational::from_i64(1).div(&Rational::from_i64(2)),
                Rational::from_i64(0),
            )]])
            .is_err()
        );
        assert!(exact_paths_to_f64(&[vec![exact_point(1, 2)]]).is_ok());
        let mut reversed_rectangle = rectangle(0, 0, 10, 10);
        reversed_rectangle.reverse();
        assert!(!canonical_summary(&[rectangle(0, 0, 10, 10)]).is_empty());
        assert!(!canonical_summary(&[reversed_rectangle]).is_empty());
        assert!(rotate_to_minimum(Vec::new()).is_empty());
    }

    #[test]
    fn supports_non_integral_intersections_through_double_api() {
        let subject = rectangle_d(0.0, 0.0, 10.0, 10.0);
        let clip = vec![PointD::new(3.0, -1.0), PointD::new(13.0, 4.0), PointD::new(3.0, 9.0)];
        let result = boolean_opd(BooleanRequestD {
            subjects: &[subject],
            clips: &[clip],
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
        })
        .unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].iter().any(|point| {
            (point.x - 10.0).abs() < f64::EPSILON && (point.y - 2.5).abs() < f64::EPSILON
        }));
        let integer_result = boolean_op(request(
            &[rectangle(0, 0, 10, 10)],
            &[vec![Point64::new(3, -1), Point64::new(13, 4), Point64::new(3, 9)]],
            ClipType::Intersection,
        ));
        assert!(matches!(integer_result, Err(Error::NonIntegralResult)));
    }

    #[test]
    fn fill_rules_distinguish_winding() {
        let path = vec![
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
            Point64::new(0, 0),
            Point64::new(10, 0),
            Point64::new(10, 10),
            Point64::new(0, 10),
        ];
        let subjects = [path];
        let even_odd = boolean_op(BooleanRequest {
            subjects: &subjects,
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        })
        .unwrap();
        assert!(even_odd.is_empty());
        let non_zero = boolean_op(BooleanRequest {
            subjects: &subjects,
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        })
        .unwrap();
        assert_eq!(area_sum(&non_zero), 200);
    }

    #[test]
    fn rejects_integer_result_with_fractional_coordinate() {
        let subject = rectangle(0, 0, 10, 10);
        let clip = vec![Point64::new(3, -1), Point64::new(13, 4), Point64::new(3, 9)];
        let result = boolean_op(BooleanRequest {
            subjects: &[subject],
            clips: &[clip],
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
        });
        assert!(matches!(result, Err(Error::NonIntegralResult)));
    }

    #[test]
    fn fast_double_path_matches_exact_oracle() {
        let subject = rectangle_d(0.0, 0.0, 10.0, 10.0);
        let clip = vec![PointD::new(3.0, -1.0), PointD::new(13.0, 4.0), PointD::new(3.0, 9.0)];
        let subjects = [subject];
        let clips = [clip];
        for clip_type in
            [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
        {
            let request = BooleanRequestD {
                subjects: &subjects,
                clips: &clips,
                clip_type,
                fill_rule: FillRule::EvenOdd,
            };
            let fast = crate::fast::try_boolean_opd(request)
                .expect("well-conditioned input should use fast path")
                .expect("fast path should close");
            let exact = boolean_opd_exact(request).expect("exact oracle should close");
            assert_eq!(double_summary(&fast), double_summary(&exact));
        }
    }

    #[test]
    fn floating_rectangle_workload_matches_set_operations() {
        let subject = rectangle_d(0.0, 0.0, 10.0, 10.0);
        let clip = rectangle_d(5.0, 0.0, 15.0, 10.0);
        let subjects = [subject];
        let clips = [clip];
        let expected = [
            (ClipType::Intersection, 100.0),
            (ClipType::Union, 300.0),
            (ClipType::Difference, 100.0),
            (ClipType::Xor, 200.0),
        ];

        for (clip_type, expected_area) in expected {
            let request = BooleanRequestD {
                subjects: &subjects,
                clips: &clips,
                clip_type,
                fill_rule: FillRule::EvenOdd,
            };
            let exact = boolean_opd_exact(request).expect("exact rectangle operation should close");
            let result = boolean_opd(request).expect("public rectangle operation should close");
            assert!((double_area_sum(&exact) - expected_area).abs() < f64::EPSILON);
            assert!((double_area_sum(&result) - expected_area).abs() < f64::EPSILON);
        }

        let touching = rectangle_d(10.0, 0.0, 20.0, 10.0);
        let result = boolean_opd(BooleanRequestD {
            subjects: &subjects,
            clips: std::slice::from_ref(&touching),
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        })
        .expect("touching rectangle union should close");
        assert_eq!(result.len(), 1);
        assert!((double_area_sum(&result) - 400.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rectilinear_operations_match_exact_oracle() {
        let subject = vec![
            PointD::new(0.0, 0.0),
            PointD::new(30.0, 0.0),
            PointD::new(30.0, 30.0),
            PointD::new(20.0, 30.0),
            PointD::new(20.0, 10.0),
            PointD::new(10.0, 10.0),
            PointD::new(10.0, 30.0),
            PointD::new(0.0, 30.0),
        ];
        let clip = vec![
            PointD::new(5.0, -5.0),
            PointD::new(25.0, -5.0),
            PointD::new(25.0, 5.0),
            PointD::new(25.0, 35.0),
            PointD::new(5.0, 35.0),
        ];
        let subjects = [subject];
        let clips = [clip];
        for fill_rule in
            [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        {
            for clip_type in
                [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
            {
                let request =
                    BooleanRequestD { subjects: &subjects, clips: &clips, clip_type, fill_rule };
                let exact = boolean_opd_exact(request).expect("exact rectilinear oracle");
                let result = boolean_opd(request).expect("public rectilinear operation");
                let exact = exact
                    .iter()
                    .map(|path| crate::trim_collinear_d(path, crate::PathKind::Closed))
                    .collect::<Result<PathsD, Error>>()
                    .expect("exact result is finite");
                assert_eq!(
                    double_summary(&result),
                    double_summary(&exact),
                    "{clip_type:?} {fill_rule:?}"
                );
            }
        }
    }

    #[test]
    fn fast_double_path_matches_exact_for_all_fill_rules() {
        let subject = rectangle_d(0.0, 0.0, 10.0, 10.0);
        let mut reversed = rectangle_d(2.0, 2.0, 8.0, 8.0);
        reversed.reverse();
        let subjects = [subject];
        let clips = [reversed];
        for fill_rule in
            [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative]
        {
            let request = BooleanRequestD {
                subjects: &subjects,
                clips: &clips,
                clip_type: ClipType::Difference,
                fill_rule,
            };
            let fast = crate::fast::try_boolean_opd(request)
                .expect("well-conditioned input should use fast path")
                .expect("fast path should close");
            let exact = boolean_opd_exact(request).expect("exact oracle should close");
            assert_eq!(double_summary(&fast), double_summary(&exact), "fill rule: {fill_rule:?}");
        }
    }

    #[test]
    fn fast_double_path_matches_exact_on_high_vertex_input() {
        let subject = regular_polygon(0.0, 40.0, 16);
        let clip = regular_polygon(12.0, 40.0, 16);
        let subjects = [subject];
        let clips = [clip];
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &clips,
            clip_type: ClipType::Xor,
            fill_rule: FillRule::EvenOdd,
        };
        let fast = crate::fast::try_boolean_opd(request)
            .expect("high-vertex input should use fast path")
            .expect("fast path should close");
        let exact = boolean_opd_exact(request).expect("exact oracle should close");
        assert_eq!(double_summary(&fast), double_summary(&exact));
    }

    #[test]
    fn fast_double_path_matches_exact_for_convex_variants() {
        let cases = [(0.0, 40.0, 12.0, 40.0), (0.0, 20.0, 60.0, 20.0), (0.0, 40.0, 0.0, 20.0)];
        for (subject_x, subject_radius, clip_x, clip_radius) in cases {
            let subject = regular_polygon(subject_x, subject_radius, 8);
            let clip = regular_polygon(clip_x, clip_radius, 8);
            let subjects = [subject];
            let clips = [clip];
            for clip_type in
                [ClipType::Intersection, ClipType::Union, ClipType::Difference, ClipType::Xor]
            {
                let request = BooleanRequestD {
                    subjects: &subjects,
                    clips: &clips,
                    clip_type,
                    fill_rule: FillRule::EvenOdd,
                };
                let fast = crate::fast::try_boolean_opd(request)
                    .expect("convex input should use fast path")
                    .expect("fast path should close");
                let exact = boolean_opd_exact(request).expect("exact oracle should close");
                assert_eq!(double_summary(&fast), double_summary(&exact), "case: {clip_type:?}");
            }
        }
    }

    #[test]
    fn fast_double_path_defers_large_coordinates_to_exact_oracle() {
        let path = rectangle_d(2_000_000.0, 2_000_000.0, 2_000_010.0, 2_000_010.0);
        let subjects = [path];
        let request = BooleanRequestD {
            subjects: &subjects,
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(crate::fast::try_boolean_opd(request).is_none());
        assert_eq!(boolean_opd(request).expect("exact fallback should close").len(), 1);
    }

    fn double_summary(paths: &PathsD) -> Vec<(usize, u64)> {
        let mut summary = paths
            .iter()
            .map(|path| (path.len(), (double_area2(path).abs() * 1_000_000.0).round().to_bits()))
            .collect::<Vec<_>>();
        summary.sort_unstable();
        summary
    }

    fn double_area2(path: &[PointD]) -> f64 {
        path.iter()
            .zip(path.iter().cycle().skip(1))
            .take(path.len())
            .map(|(start, end)| start.x * end.y - start.y * end.x)
            .sum()
    }

    fn double_area_sum(paths: &[PathD]) -> f64 {
        paths.iter().map(|path| double_area2(path).abs()).sum()
    }

    fn rectangle_d(left: f64, bottom: f64, right: f64, top: f64) -> PathD {
        vec![
            PointD::new(left, bottom),
            PointD::new(right, bottom),
            PointD::new(right, top),
            PointD::new(left, top),
        ]
    }

    #[allow(clippy::cast_precision_loss)]
    fn regular_polygon(center_x: f64, radius: f64, vertices: usize) -> PathD {
        (0..vertices)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / vertices as f64;
                PointD::new(center_x + radius * angle.cos(), radius * angle.sin())
            })
            .collect()
    }
}
