//! Public operation contracts for integer and floating-point paths.

use crate::{Error, Path64, PathD, PathKind, Paths64, PathsD, validate_path64, validate_pathd};

/// A boolean operation over filled paths.
///
/// The subject and clip collections may contain multiple rings. Empty
/// collections are valid and make the operation behave like the corresponding
/// set operation with an empty side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Borrowed integer inputs to a boolean operation.
///
/// Every non-empty path is interpreted as a closed ring. The first point does
/// not need to be repeated at the end.
#[derive(Clone, Copy, Debug)]
pub struct BooleanRequest<'a> {
    /// Subject paths, the left-hand side of the operation.
    pub subjects: &'a [Path64],
    /// Clip paths, the right-hand side of the operation.
    pub clips: &'a [Path64],
    /// Boolean operation to perform.
    pub clip_type: ClipType,
    /// Fill rule for both path sets.
    pub fill_rule: FillRule,
}

impl<'a> BooleanRequest<'a> {
    /// Creates a boolean request from borrowed subject and clip paths.
    #[must_use]
    pub const fn new(
        subjects: &'a [Path64],
        clips: &'a [Path64],
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Self {
        Self { subjects, clips, clip_type, fill_rule }
    }
}

/// Borrowed floating-point inputs to a boolean operation.
///
/// This has the same topology contract as [`BooleanRequest`], but accepts
/// finite [`f64`](f64) coordinates.
#[derive(Clone, Copy, Debug)]
pub struct BooleanRequestD<'a> {
    /// Subject paths, the left-hand side of the operation.
    pub subjects: &'a [PathD],
    /// Clip paths, the right-hand side of the operation.
    pub clips: &'a [PathD],
    /// Boolean operation to perform.
    pub clip_type: ClipType,
    /// Fill rule for both path sets.
    pub fill_rule: FillRule,
}

impl<'a> BooleanRequestD<'a> {
    /// Creates a floating-point boolean request from borrowed paths.
    #[must_use]
    pub const fn new(
        subjects: &'a [PathD],
        clips: &'a [PathD],
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Self {
        Self { subjects, clips, clip_type, fill_rule }
    }
}

/// Validates a boolean request without executing it.
///
/// Use this when a caller wants to report malformed input before doing any
/// work. [`boolean_op`] performs the same validation automatically.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] when a non-empty subject or clip path is too
/// short to describe a closed region.
pub fn validate_request(request: &BooleanRequest<'_>) -> Result<(), Error> {
    for path in request.subjects.iter().chain(request.clips) {
        validate_path64(path, PathKind::Closed)?;
    }
    Ok(())
}

/// Validates a floating-point boolean request without executing it.
///
/// In addition to path shape, this rejects NaN and infinite coordinates.
/// [`boolean_opd`] performs the same validation automatically.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for a too-short path and
/// [`Error::NonFiniteCoordinate`] for a non-finite coordinate.
pub fn validate_requestd(request: &BooleanRequestD<'_>) -> Result<(), Error> {
    for path in request.subjects.iter().chain(request.clips) {
        validate_pathd(path, PathKind::Closed)?;
    }
    Ok(())
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
/// assert_eq!(result.len(), 1);
/// assert!(!result[0].is_empty());
/// ```
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for invalid input, [`Error::ArithmeticOverflow`]
/// for checked integer overflow, [`Error::NonIntegralResult`] when an exact
/// result cannot be represented by `i64`, or [`Error::TopologyFailure`] if the
/// arrangement cannot be closed.
pub fn boolean_op(request: BooleanRequest<'_>) -> Result<Paths64, Error> {
    validate_request(&request)?;
    crate::boolean::boolean_op64(request)
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
pub fn boolean_opd(request: BooleanRequestD<'_>) -> Result<PathsD, Error> {
    validate_requestd(&request)?;
    crate::boolean::boolean_opd(request)
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
    boolean_op(BooleanRequest::new(subjects, clips, ClipType::Intersection, FillRule::EvenOdd))
}

/// Unites two integer path collections using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op`].
pub fn union(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(BooleanRequest::new(subjects, clips, ClipType::Union, FillRule::EvenOdd))
}

/// Subtracts integer `clips` from `subjects` using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op`].
pub fn difference(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(BooleanRequest::new(subjects, clips, ClipType::Difference, FillRule::EvenOdd))
}

/// Computes integer symmetric difference using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_op`].
pub fn xor(subjects: &[Path64], clips: &[Path64]) -> Result<Paths64, Error> {
    boolean_op(BooleanRequest::new(subjects, clips, ClipType::Xor, FillRule::EvenOdd))
}

/// Intersects floating-point path collections using [`FillRule::EvenOdd`].
///
/// Use [`boolean_opd`] when a different fill rule is required.
///
/// # Errors
///
/// Propagates validation, arithmetic, and topology errors from [`boolean_opd`].
pub fn intersection_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_opd(BooleanRequestD::new(subjects, clips, ClipType::Intersection, FillRule::EvenOdd))
}

/// Unites two floating-point path collections using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_opd`].
pub fn union_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_opd(BooleanRequestD::new(subjects, clips, ClipType::Union, FillRule::EvenOdd))
}

/// Subtracts floating-point `clips` from `subjects` using
/// [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_opd`].
pub fn difference_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_opd(BooleanRequestD::new(subjects, clips, ClipType::Difference, FillRule::EvenOdd))
}

/// Computes floating-point symmetric difference using [`FillRule::EvenOdd`].
///
/// # Errors
///
/// Propagates errors from [`boolean_opd`].
pub fn xor_d(subjects: &[PathD], clips: &[PathD]) -> Result<PathsD, Error> {
    boolean_opd(BooleanRequestD::new(subjects, clips, ClipType::Xor, FillRule::EvenOdd))
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
        assert!(std::ptr::eq(integer_request.subjects[0].as_ptr(), integer_subject.as_ptr()));
        assert!(std::ptr::eq(integer_request.clips[0].as_ptr(), integer_clip.as_ptr()));
        assert_eq!(integer_request.clip_type, ClipType::Difference);
        assert_eq!(integer_request.fill_rule, FillRule::Positive);

        let double_subject = vec![crate::PointD::new(0.0, 0.0)];
        let double_clip = vec![crate::PointD::new(1.0, 1.0)];
        let double_request = BooleanRequestD::new(
            std::slice::from_ref(&double_subject),
            std::slice::from_ref(&double_clip),
            ClipType::Xor,
            FillRule::Negative,
        );
        assert!(std::ptr::eq(double_request.subjects[0].as_ptr(), double_subject.as_ptr()));
        assert!(std::ptr::eq(double_request.clips[0].as_ptr(), double_clip.as_ptr()));
        assert_eq!(double_request.clip_type, ClipType::Xor);
        assert_eq!(double_request.fill_rule, FillRule::Negative);
    }

    #[test]
    fn validates_then_executes_union_with_empty_clip() {
        let square =
            vec![crate::Point64::new(0, 0), crate::Point64::new(1, 0), crate::Point64::new(1, 1)];
        let request = BooleanRequest {
            subjects: &[square],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        };
        assert_eq!(validate_request(&request), Ok(()));
        assert_eq!(boolean_op(request).expect("valid request").len(), 1);
    }

    #[test]
    fn rejects_invalid_boolean_input_before_kernel() {
        let request = BooleanRequest {
            subjects: &[vec![crate::Point64::new(0, 0)]],
            clips: &[],
            clip_type: ClipType::Intersection,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(matches!(boolean_op(request), Err(Error::InvalidPath { .. })));
    }

    #[test]
    fn validates_double_requests() {
        let square = vec![
            crate::PointD::new(0.0, 0.0),
            crate::PointD::new(1.0, 0.0),
            crate::PointD::new(1.0, 1.0),
        ];
        let request = BooleanRequestD {
            subjects: &[square],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        };
        assert!(validate_requestd(&request).is_ok());
        assert_eq!(crate::boolean_opd(request).expect("valid request").len(), 1);
        let bad = BooleanRequestD {
            subjects: &[vec![crate::PointD::new(0.0, 0.0), crate::PointD::new(f64::NAN, 0.0)]],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::EvenOdd,
        };
        assert!(matches!(validate_requestd(&bad), Err(Error::InvalidPath { .. })));
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
