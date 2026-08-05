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
}
