//! Public operation contracts. The execution kernel will be added behind this
//! boundary once its state-machine invariants are covered.

use crate::{Error, Path64, PathKind, Paths64, validate_path64};

/// A boolean operation over filled paths.
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

/// A winding/fill rule used to interpret self-intersecting input paths.
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

/// Borrowed inputs to a boolean operation.
#[derive(Clone, Copy, Debug)]
pub struct BooleanRequest<'a> {
    /// Subject paths.
    pub subjects: &'a [Path64],
    /// Clip paths.
    pub clips: &'a [Path64],
    /// Boolean operation to perform.
    pub clip_type: ClipType,
    /// Fill rule for both path sets.
    pub fill_rule: FillRule,
}

/// Validates a boolean request without executing it.
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

/// Executes a boolean request.
///
/// Empty-input short circuits will be added with the first kernel contract.
/// Returning a dedicated error here prevents an incomplete implementation from
/// being mistaken for a correct empty-result implementation.
///
/// # Errors
///
/// Returns [`Error::InvalidPath`] for invalid input, or
/// [`Error::KernelNotReady`] until the clipping kernel is implemented.
pub fn boolean_op(request: BooleanRequest<'_>) -> Result<Paths64, Error> {
    validate_request(&request)?;
    Err(Error::KernelNotReady)
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
    fn validates_then_reports_kernel_state() {
        let square =
            vec![crate::Point64::new(0, 0), crate::Point64::new(1, 0), crate::Point64::new(1, 1)];
        let request = BooleanRequest {
            subjects: &[square],
            clips: &[],
            clip_type: ClipType::Union,
            fill_rule: FillRule::NonZero,
        };
        assert_eq!(validate_request(&request), Ok(()));
        assert_eq!(boolean_op(request), Err(Error::KernelNotReady));
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
}
