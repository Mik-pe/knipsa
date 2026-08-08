//! Public operation contracts for integer and floating-point paths.

use crate::complexity::BooleanComplexity;
use crate::{
    ComplexityLimits, Error, Path64, PathD, PathKind, Paths64, PathsD, validate_path_d,
    validate_path64,
};

/// A boolean operation over filled paths.
///
/// The subject and clip collections may contain multiple rings. Empty
/// collections are valid and make the operation behave like the corresponding
/// set operation with an empty side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[repr(u8)]
pub enum ClipType {
    /// Keep the region present in both inputs.
    Intersection = 1,
    /// Keep the region present in either input.
    Union = 2,
    /// Keep the subject region outside the clip region.
    Difference = 3,
    /// Keep the region present in exactly one input.
    Xor = 4,
}

/// A winding/fill rule used to interpret input paths.
///
/// `EvenOdd` depends only on crossing parity. The other rules use the signed
/// winding accumulated by a path, which makes ring orientation meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[repr(u8)]
pub enum FillRule {
    /// Alternate parity at every crossing.
    EvenOdd = 0,
    /// Non-zero winding number.
    NonZero = 1,
    /// Keep only positively wound regions.
    Positive = 2,
    /// Keep only negatively wound regions.
    Negative = 3,
}

/// Borrowed paths and options for one bounded Boolean operation.
///
/// Closed subjects and clips define filled regions; clips are always closed.
/// Open subjects are polylines and never contribute winding or affect the
/// closed result. Intersection retains open fragments inside clips, difference
/// and XOR retain fragments outside clips, and union retains fragments outside
/// every filled closed subject and clip. Open paths do not interact with one
/// another. A fragment collinear with a filled boundary follows Clipper2's
/// adjacent-fragment rule; at a polyline end, the one present adjacent
/// fragment decides whether the boundary run remains.
#[derive(Debug)]
pub struct BooleanRequest<'a, P> {
    /// Closed subject paths, the filled left-hand side of the operation.
    pub closed_subjects: &'a [P],
    /// Open subject polylines to clip.
    pub open_subjects: &'a [P],
    /// Closed clip paths, the right-hand side of the operation.
    pub clips: &'a [P],
    /// Boolean operation to perform.
    pub clip_type: ClipType,
    /// Fill rule for both path sets.
    pub fill_rule: FillRule,
    /// Deterministic input-complexity budget.
    pub limits: ComplexityLimits,
}

impl<P> Copy for BooleanRequest<'_, P> {}

impl<P> Clone for BooleanRequest<'_, P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, P> BooleanRequest<'a, P> {
    /// Creates a closed-subject Boolean request with [`ComplexityLimits::DEFAULT`].
    ///
    /// Set `open_subjects` or `limits` directly when constructing a request
    /// that needs open clipping or a different deterministic budget.
    #[must_use]
    pub const fn new(
        closed_subjects: &'a [P],
        clips: &'a [P],
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Self {
        Self {
            closed_subjects,
            open_subjects: &[],
            clips,
            clip_type,
            fill_rule,
            limits: ComplexityLimits::DEFAULT,
        }
    }
}

/// Separate closed and open outputs from a Boolean operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BooleanOutput<P> {
    /// Closed polygon rings produced by the operation.
    pub closed: Vec<P>,
    /// Clipped open subject polylines.
    pub open: Vec<P>,
}

fn closed_request<'a, P>(
    subjects: &'a [P],
    clips: &'a [P],
    clip_type: ClipType,
    fill_rule: FillRule,
) -> BooleanRequest<'a, P> {
    BooleanRequest::new(subjects, clips, clip_type, fill_rule)
}

/// Executes an exact integer boolean request.
///
/// # Examples
///
/// ```
/// use knipsa::{boolean_op, BooleanRequest, ClipType, FillRule, Point64};
///
/// let subject = vec![
///     Point64::new(0, 0),
///     Point64::new(10, 0),
///     Point64::new(10, 10),
///     Point64::new(0, 10),
/// ];
/// let clip = vec![
///     Point64::new(5, 5),
///     Point64::new(15, 5),
///     Point64::new(15, 15),
///     Point64::new(5, 15),
/// ];
/// let result = boolean_op(BooleanRequest::new(
///     std::slice::from_ref(&subject),
///     std::slice::from_ref(&clip),
///     ClipType::Intersection,
///     FillRule::EvenOdd,
/// ))
/// .expect("valid polygons close");
///
/// assert_eq!(result.closed.len(), 1);
/// assert!(result.open.is_empty());
/// ```
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for invalid input, [`Error::ArithmeticOverflow`]
/// for checked integer overflow, [`Error::NonIntegralResult`] when an exact
/// result cannot be represented by `i64`, or [`Error::TopologyFailure`] if the
/// arrangement cannot be closed.
pub fn boolean_op(request: BooleanRequest<'_, Path64>) -> Result<BooleanOutput<Path64>, Error> {
    validate_bounded64(&request)?;
    crate::boolean::boolean_op64(&request)
}

/// Executes a floating-point boolean request.
///
/// The input binary floating-point values are treated as exact values during
/// arrangement construction; only the returned coordinates are converted back
/// to `f64`. Ordinary well-conditioned convex input uses the fast path and
/// difficult or ambiguous input uses the exact arrangement fallback.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] or [`Error::NonFiniteCoordinate`] for invalid
/// input and [`Error::TopologyFailure`] if the arrangement cannot be closed.
pub fn boolean_op_d(request: BooleanRequest<'_, PathD>) -> Result<BooleanOutput<PathD>, Error> {
    validate_bounded_d(&request)?;
    crate::boolean::boolean_op_d(&request)
}

fn validate_bounded64(request: &BooleanRequest<'_, Path64>) -> Result<(), Error> {
    let mut validation_error = None;
    let mut complexity = BooleanComplexity::default();
    for path in request.closed_subjects {
        complexity.add_closed_subject(path.len());
        if validation_error.is_none() {
            validation_error = validate_path64(path, PathKind::Closed).err();
        }
    }
    for path in request.open_subjects {
        complexity.add_open_subject(path.len());
        if validation_error.is_none() {
            validation_error = validate_path64(path, PathKind::Open).err();
        }
    }
    for path in request.clips {
        complexity.add_clip(path.len());
        if validation_error.is_none() {
            validation_error = validate_path64(path, PathKind::Closed).err();
        }
    }
    complexity.check(request.limits, request.clip_type == ClipType::Union)?;
    validation_error.map_or(Ok(()), Err)
}

fn validate_bounded_d(request: &BooleanRequest<'_, PathD>) -> Result<(), Error> {
    let mut validation_error = None;
    let mut complexity = BooleanComplexity::default();
    for path in request.closed_subjects {
        complexity.add_closed_subject(path.len());
        if validation_error.is_none() {
            validation_error = validate_path_d(path, PathKind::Closed).err();
        }
    }
    for path in request.open_subjects {
        complexity.add_open_subject(path.len());
        if validation_error.is_none() {
            validation_error = validate_path_d(path, PathKind::Open).err();
        }
    }
    for path in request.clips {
        complexity.add_clip(path.len());
        if validation_error.is_none() {
            validation_error = validate_path_d(path, PathKind::Closed).err();
        }
    }
    complexity.check(request.limits, request.clip_type == ClipType::Union)?;
    validation_error.map_or(Ok(()), Err)
}

/// Intersects integer path collections using the orientation-independent
/// [`FillRule::EvenOdd`] rule.
///
/// Use [`boolean_op`] when a different fill rule is required.
///
/// # Errors
///
/// Propagates validation, arithmetic, representation, and topology errors from
/// [`boolean_op`].
pub fn intersection(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(closed_request(subjects, clips, ClipType::Intersection, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Unites two integer path collections using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op`].
pub fn union(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(closed_request(subjects, clips, ClipType::Union, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Subtracts integer `clips` from `subjects` using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op`].
pub fn difference(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(closed_request(subjects, clips, ClipType::Difference, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Computes integer symmetric difference using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op`].
pub fn xor(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(closed_request(subjects, clips, ClipType::Xor, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Intersects floating-point path collections using [`FillRule::EvenOdd`].
///
/// Use [`boolean_op_d`] when a different fill rule is required.
///
/// # Errors
///
/// Propagates validation, arithmetic, and topology errors from [`boolean_op_d`].
pub fn intersection_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_op_d(closed_request(subjects, clips, ClipType::Intersection, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Unites two floating-point path collections using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op_d`].
pub fn union_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_op_d(closed_request(subjects, clips, ClipType::Union, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Subtracts floating-point `clips` from `subjects` using
/// [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op_d`].
pub fn difference_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_op_d(closed_request(subjects, clips, ClipType::Difference, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Computes floating-point symmetric difference using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op_d`].
pub fn xor_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_op_d(closed_request(subjects, clips, ClipType::Xor, FillRule::EvenOdd))
        .map(|output| output.closed)
}

/// Intersects two integer rings using [`FillRule::EvenOdd`].
///
/// This is the shortest zero-allocation entry point when each side contains
/// exactly one ring. The result can still contain multiple rings.
///
/// # Errors
///
/// Propagates errors from [`intersection`].
pub fn intersection_path(subject: &Path64, clip: &Path64) -> Result<Paths64, Error> {
    intersection(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Unites two integer rings using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`union`].
pub fn union_path(subject: &Path64, clip: &Path64) -> Result<Paths64, Error> {
    union(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Subtracts one integer ring from another using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`difference`].
pub fn difference_path(subject: &Path64, clip: &Path64) -> Result<Paths64, Error> {
    difference(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Computes the symmetric difference of two integer rings using
/// [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`xor`].
pub fn xor_path(subject: &Path64, clip: &Path64) -> Result<Paths64, Error> {
    xor(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Intersects two floating-point rings using [`FillRule::EvenOdd`].
///
/// This is the shortest zero-allocation entry point when each side contains
/// exactly one ring. The result can still contain multiple rings.
///
/// # Errors
///
/// Propagates errors from [`intersection_d`].
pub fn intersection_path_d(subject: &PathD, clip: &PathD) -> Result<PathsD, Error> {
    intersection_d(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Unites two floating-point rings using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`union_d`].
pub fn union_path_d(subject: &PathD, clip: &PathD) -> Result<PathsD, Error> {
    union_d(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Subtracts one floating-point ring from another using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`difference_d`].
pub fn difference_path_d(subject: &PathD, clip: &PathD) -> Result<PathsD, Error> {
    difference_d(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

/// Computes the symmetric difference of two floating-point rings using
/// [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`xor_d`].
pub fn xor_path_d(subject: &PathD, clip: &PathD) -> Result<PathsD, Error> {
    xor_d(std::slice::from_ref(subject), std::slice::from_ref(clip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_stable_operation_values() {
        assert_eq!(ClipType::Intersection as u8, 1);
        assert_eq!(ClipType::Union as u8, 2);
        assert_eq!(ClipType::Difference as u8, 3);
        assert_eq!(ClipType::Xor as u8, 4);
        assert_eq!(FillRule::EvenOdd as u8, 0);
        assert_eq!(FillRule::NonZero as u8, 1);
        assert_eq!(FillRule::Positive as u8, 2);
        assert_eq!(FillRule::Negative as u8, 3);
    }

    #[test]
    fn constructors_preserve_borrowed_inputs_and_options() {
        let integer_subject = vec![crate::Point64::new(0, 0)];
        let integer_clip = vec![crate::Point64::new(1, 1)];
        let integer_request = BooleanRequest::new(
            std::slice::from_ref(&integer_subject),
            std::slice::from_ref(&integer_clip),
            ClipType::Difference,
            FillRule::Positive,
        );
        assert!(std::ptr::eq(
            integer_request.closed_subjects[0].as_ptr(),
            integer_subject.as_ptr()
        ));
        assert!(std::ptr::eq(integer_request.clips[0].as_ptr(), integer_clip.as_ptr()));
        assert_eq!(integer_request.clip_type, ClipType::Difference);
        assert_eq!(integer_request.fill_rule, FillRule::Positive);
        let cloned = Clone::clone(&integer_request);
        assert_eq!(cloned.closed_subjects, integer_request.closed_subjects);

        let double_subject = vec![crate::PointD::new(0.0, 0.0)];
        let double_clip = vec![crate::PointD::new(1.0, 1.0)];
        let double_request = BooleanRequest::new(
            std::slice::from_ref(&double_subject),
            std::slice::from_ref(&double_clip),
            ClipType::Xor,
            FillRule::Negative,
        );
        assert!(std::ptr::eq(double_request.closed_subjects[0].as_ptr(), double_subject.as_ptr()));
        assert!(std::ptr::eq(double_request.clips[0].as_ptr(), double_clip.as_ptr()));
        assert_eq!(double_request.clip_type, ClipType::Xor);
        assert_eq!(double_request.fill_rule, FillRule::Negative);
    }

    #[test]
    fn open_subjects_are_charged_only_against_relevant_filled_boundaries() {
        let open = [(0..3_000).map(|x| crate::Point64::new(x, 0)).collect::<Path64>()];
        let output = boolean_op(BooleanRequest {
            closed_subjects: &[],
            open_subjects: &open,
            clips: &[],
            clip_type: ClipType::Difference,
            fill_rule: FillRule::EvenOdd,
            limits: ComplexityLimits::new(1, 3_000, 0),
        })
        .expect("an isolated open polyline has no candidate edge pairs");
        assert_eq!(output.open, open);

        let open_d = [vec![crate::PointD::new(-1.0, 0.5), crate::PointD::new(2.0, 0.5)]];
        let clip_d = [vec![
            crate::PointD::new(0.0, 0.0),
            crate::PointD::new(1.0, 0.0),
            crate::PointD::new(1.0, 1.0),
            crate::PointD::new(0.0, 1.0),
        ]];
        let output = boolean_op_d(BooleanRequest {
            closed_subjects: &[],
            open_subjects: &open_d,
            clips: &clip_d,
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
            limits: ComplexityLimits::DEFAULT,
        })
        .expect("valid floating open request");
        assert_eq!(
            output.open,
            vec![vec![crate::PointD::new(0.0, 0.5), crate::PointD::new(1.0, 0.5)]]
        );
    }

    #[test]
    fn executes_union_with_empty_clip() {
        let square =
            vec![crate::Point64::new(0, 0), crate::Point64::new(1, 0), crate::Point64::new(1, 1)];
        let request = BooleanRequest {
            limits: crate::ComplexityLimits::DEFAULT,
            open_subjects: &[],
            closed_subjects: &[square],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        };
        assert_eq!(boolean_op(request).expect("valid request").closed.len(), 1);
    }

    #[test]
    fn rejects_invalid_boolean_input_before_kernel() {
        let request = BooleanRequest {
            limits: crate::ComplexityLimits::DEFAULT,
            open_subjects: &[],
            closed_subjects: &[vec![crate::Point64::new(0, 0)]],
            clips: &[],
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(matches!(boolean_op(request), Err(Error::InvalidPath { .. })));
    }

    #[test]
    fn validation_keeps_scanning_complexity_after_the_first_error() {
        let integer_closed = [
            vec![crate::Point64::new(0, 0)],
            vec![crate::Point64::new(0, 0), crate::Point64::new(2, 0), crate::Point64::new(0, 2)],
        ];
        let integer_open = [vec![crate::Point64::new(0, 0), crate::Point64::new(1, 1)]];
        assert!(matches!(
            boolean_op(BooleanRequest {
                closed_subjects: &integer_closed,
                open_subjects: &integer_open,
                clips: &[],
                clip_type: ClipType::Intersection,
                fill_rule: FillRule::EvenOdd,
                limits: ComplexityLimits::DEFAULT,
            }),
            Err(Error::InvalidPath { .. })
        ));

        let double_closed = [
            vec![crate::PointD::new(0.0, 0.0)],
            vec![
                crate::PointD::new(0.0, 0.0),
                crate::PointD::new(2.0, 0.0),
                crate::PointD::new(0.0, 2.0),
            ],
        ];
        let double_open = [vec![crate::PointD::new(0.0, 0.0), crate::PointD::new(1.0, 1.0)]];
        assert!(matches!(
            boolean_op_d(BooleanRequest {
                closed_subjects: &double_closed,
                open_subjects: &double_open,
                clips: &[],
                clip_type: ClipType::Intersection,
                fill_rule: FillRule::EvenOdd,
                limits: ComplexityLimits::DEFAULT,
            }),
            Err(Error::InvalidPath { .. })
        ));
    }

    #[test]
    fn open_requests_validate_and_preflight_before_geometry_work() {
        let invalid_open = [vec![crate::Point64::new(0, 0)]];
        let validation = BooleanRequest {
            closed_subjects: &[],
            open_subjects: &invalid_open,
            clips: &[],
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
            limits: ComplexityLimits::DEFAULT,
        };
        assert!(matches!(
            boolean_op(validation),
            Err(Error::InvalidPath { kind: PathKind::Open, .. })
        ));

        let oversized = [vec![crate::Point64::new(0, 0), crate::Point64::new(1, 1)]];
        let limited = BooleanRequest {
            closed_subjects: &[],
            open_subjects: &oversized,
            clips: &[],
            clip_type: ClipType::Difference,
            fill_rule: FillRule::EvenOdd,
            limits: ComplexityLimits::new(0, 0, 0),
        };
        assert!(matches!(
            boolean_op(limited),
            Err(Error::LimitExceeded { resource: crate::ComplexityResource::Paths, .. })
        ));
    }

    #[test]
    fn executes_and_validates_double_requests() {
        let square = vec![
            crate::PointD::new(0.0, 0.0),
            crate::PointD::new(1.0, 0.0),
            crate::PointD::new(1.0, 1.0),
        ];
        let request = BooleanRequest {
            limits: crate::ComplexityLimits::DEFAULT,
            open_subjects: &[],
            closed_subjects: &[square],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        };
        assert_eq!(crate::boolean_op_d(request).expect("valid request").closed.len(), 1);
        let bad = BooleanRequest {
            limits: crate::ComplexityLimits::DEFAULT,
            open_subjects: &[],
            closed_subjects: &[vec![
                crate::PointD::new(0.0, 0.0),
                crate::PointD::new(f64::NAN, 0.0),
            ]],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(matches!(boolean_op_d(bad), Err(Error::InvalidPath { .. })));
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn convenience_operations_use_even_odd_semantics() {
        let subject = vec![
            crate::Point64::new(0, 0),
            crate::Point64::new(10, 0),
            crate::Point64::new(10, 10),
            crate::Point64::new(0, 10),
        ];
        let clip = vec![
            crate::Point64::new(5, 0),
            crate::Point64::new(15, 0),
            crate::Point64::new(15, 10),
            crate::Point64::new(5, 10),
        ];
        let subjects = [subject];
        let clips = [clip];
        assert_eq!(intersection(&subjects, &clips).expect("intersection").len(), 1);
        assert_eq!(union(&subjects, &clips).expect("union").len(), 1);
        assert_eq!(difference(&subjects, &clips).expect("difference").len(), 1);
        assert_eq!(xor(&subjects, &clips).expect("xor").len(), 2);

        let subjects_d = subjects
            .iter()
            .map(|path| {
                path.iter()
                    .map(|point| crate::PointD::new(point.x as f64, point.y as f64))
                    .collect()
            })
            .collect::<Vec<PathD>>();
        let clips_d = clips
            .iter()
            .map(|path| {
                path.iter()
                    .map(|point| crate::PointD::new(point.x as f64, point.y as f64))
                    .collect()
            })
            .collect::<Vec<PathD>>();
        assert_eq!(intersection_d(&subjects_d, &clips_d).expect("intersection_d").len(), 1);
        assert_eq!(union_d(&subjects_d, &clips_d).expect("union_d").len(), 1);
        assert_eq!(difference_d(&subjects_d, &clips_d).expect("difference_d").len(), 1);
        assert_eq!(xor_d(&subjects_d, &clips_d).expect("xor_d").len(), 2);
    }

    #[test]
    fn single_ring_helpers_match_collection_helpers() {
        let integer_subject = vec![
            crate::Point64::new(0, 0),
            crate::Point64::new(10, 0),
            crate::Point64::new(10, 10),
            crate::Point64::new(0, 10),
        ];
        let integer_clip = vec![
            crate::Point64::new(5, 0),
            crate::Point64::new(15, 0),
            crate::Point64::new(15, 10),
            crate::Point64::new(5, 10),
        ];
        let integer_subjects = std::slice::from_ref(&integer_subject);
        let integer_clips = std::slice::from_ref(&integer_clip);
        assert_eq!(
            intersection_path(&integer_subject, &integer_clip),
            intersection(integer_subjects, integer_clips)
        );
        assert_eq!(
            union_path(&integer_subject, &integer_clip),
            union(integer_subjects, integer_clips)
        );
        assert_eq!(
            difference_path(&integer_subject, &integer_clip),
            difference(integer_subjects, integer_clips)
        );
        assert_eq!(xor_path(&integer_subject, &integer_clip), xor(integer_subjects, integer_clips));

        let double_subject = vec![
            crate::PointD::new(0.0, 0.0),
            crate::PointD::new(10.0, 0.0),
            crate::PointD::new(10.0, 10.0),
            crate::PointD::new(0.0, 10.0),
        ];
        let double_clip = vec![
            crate::PointD::new(5.0, 0.0),
            crate::PointD::new(15.0, 0.0),
            crate::PointD::new(15.0, 10.0),
            crate::PointD::new(5.0, 10.0),
        ];
        let double_subjects = std::slice::from_ref(&double_subject);
        let double_clips = std::slice::from_ref(&double_clip);
        assert_eq!(
            intersection_path_d(&double_subject, &double_clip),
            intersection_d(double_subjects, double_clips)
        );
        assert_eq!(
            union_path_d(&double_subject, &double_clip),
            union_d(double_subjects, double_clips)
        );
        assert_eq!(
            difference_path_d(&double_subject, &double_clip),
            difference_d(double_subjects, double_clips)
        );
        assert_eq!(xor_path_d(&double_subject, &double_clip), xor_d(double_subjects, double_clips));
    }
}
