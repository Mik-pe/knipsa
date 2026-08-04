//! C-compatible, language-neutral entry points for the safe `knipsa` API.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

use std::{ffi::c_char, slice};

use knipsa::{Error, PathKind, Point64, PointLocation, point_in_polygon, validate_path64};

/// A fixed-layout integer point for FFI callers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct KnipsaPoint64 {
    /// Horizontal coordinate.
    pub x: i64,
    /// Vertical coordinate.
    pub y: i64,
}

impl From<KnipsaPoint64> for Point64 {
    fn from(point: KnipsaPoint64) -> Self {
        Self::new(point.x, point.y)
    }
}

/// A borrowed point slice. A null `points` pointer is valid only when
/// `point_count` is zero.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct KnipsaPath64 {
    /// Pointer to the first point.
    pub points: *const KnipsaPoint64,
    /// Number of points at `points`.
    pub point_count: usize,
}

/// Whether an FFI path is closed or open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaPathKind {
    /// The final point connects back to the first.
    Closed = 0,
    /// The final point does not connect back to the first.
    Open = 1,
}

impl From<KnipsaPathKind> for PathKind {
    fn from(kind: KnipsaPathKind) -> Self {
        match kind {
            KnipsaPathKind::Closed => Self::Closed,
            KnipsaPathKind::Open => Self::Open,
        }
    }
}

/// Stable status codes returned by exported functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaStatus {
    /// The call succeeded.
    Ok = 0,
    /// A required pointer was null.
    NullPointer = 1,
    /// A path did not satisfy the requested shape contract.
    InvalidPath = 2,
    /// A checked integer computation overflowed.
    ArithmeticOverflow = 3,
    /// The requested operation is not implemented in this release.
    KernelNotReady = 4,
}

/// Point classification returned by `knipsa_point_in_polygon64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaLocation {
    /// The point is outside the path.
    Outside = 0,
    /// The point is inside the path.
    Inside = 1,
    /// The point lies on the path boundary.
    Boundary = 2,
}

const VERSION: &[u8] = b"0.0.0\0";
const STATUS_OK: &[u8] = b"ok\0";
const STATUS_NULL: &[u8] = b"required pointer is null\0";
const STATUS_PATH: &[u8] = b"invalid path\0";
const STATUS_OVERFLOW: &[u8] = b"checked arithmetic overflow\0";
const STATUS_KERNEL: &[u8] = b"polygon clipping kernel is not ready\0";

/// Returns the knipsa FFI version as a static NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn knipsa_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

/// Returns a static NUL-terminated description for a status code.
#[unsafe(no_mangle)]
pub extern "C" fn knipsa_status_message(status: KnipsaStatus) -> *const c_char {
    match status {
        KnipsaStatus::Ok => STATUS_OK.as_ptr().cast(),
        KnipsaStatus::NullPointer => STATUS_NULL.as_ptr().cast(),
        KnipsaStatus::InvalidPath => STATUS_PATH.as_ptr().cast(),
        KnipsaStatus::ArithmeticOverflow => STATUS_OVERFLOW.as_ptr().cast(),
        KnipsaStatus::KernelNotReady => STATUS_KERNEL.as_ptr().cast(),
    }
}

/// Validates a borrowed array of paths.
///
/// A null `paths` pointer is accepted when `path_count` is zero. Each path's
/// point pointer follows the same rule. Inputs remain owned by the caller.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_validate_paths64(
    paths: *const KnipsaPath64,
    path_count: usize,
    kind: KnipsaPathKind,
) -> KnipsaStatus {
    let paths = if path_count == 0 {
        &[]
    } else if paths.is_null() {
        return KnipsaStatus::NullPointer;
    } else {
        // SAFETY: A non-null pointer and its element count are supplied by
        // the caller according to the documented borrowed-slice contract.
        unsafe { slice::from_raw_parts(paths, path_count) }
    };

    for path in paths {
        let points = if path.point_count == 0 {
            &[]
        } else if path.points.is_null() {
            return KnipsaStatus::NullPointer;
        } else {
            // SAFETY: The path descriptor has a non-null point pointer and
            // the caller owns the borrowed memory for this call.
            unsafe { slice::from_raw_parts(path.points, path.point_count) }
        };
        let rust_points: Vec<Point64> = points.iter().copied().map(Into::into).collect();
        if let Err(error) = validate_path64(&rust_points, kind.into()) {
            return status_from_error(&error);
        }
    }
    KnipsaStatus::Ok
}

/// Classifies one point against a borrowed closed path.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_point_in_polygon64(
    path: KnipsaPath64,
    point: KnipsaPoint64,
    location: *mut KnipsaLocation,
) -> KnipsaStatus {
    if location.is_null() {
        return KnipsaStatus::NullPointer;
    }
    let points = if path.point_count == 0 {
        &[]
    } else if path.points.is_null() {
        return KnipsaStatus::NullPointer;
    } else {
        // SAFETY: The path descriptor has a non-null point pointer and the
        // caller owns the borrowed memory for this call.
        unsafe { slice::from_raw_parts(path.points, path.point_count) }
    };
    let rust_points: Vec<Point64> = points.iter().copied().map(Into::into).collect();
    match point_in_polygon(point.into(), &rust_points) {
        Ok(result) => {
            // SAFETY: The null pointer was rejected above and the caller owns
            // the writable output location for the duration of this call.
            unsafe {
                *location = result.into();
            }
            KnipsaStatus::Ok
        }
        Err(error) => status_from_error(&error),
    }
}

fn status_from_error(error: &Error) -> KnipsaStatus {
    match error {
        Error::InvalidPath { .. } | Error::NonFiniteCoordinate { .. } => KnipsaStatus::InvalidPath,
        Error::ArithmeticOverflow => KnipsaStatus::ArithmeticOverflow,
        Error::KernelNotReady => KnipsaStatus::KernelNotReady,
    }
}

impl From<PointLocation> for KnipsaLocation {
    fn from(location: PointLocation) -> Self {
        match location {
            PointLocation::Outside => Self::Outside,
            PointLocation::Inside => Self::Inside,
            PointLocation::Boundary => Self::Boundary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    const TRIANGLE: [KnipsaPoint64; 3] = [
        KnipsaPoint64 { x: 0, y: 0 },
        KnipsaPoint64 { x: 10, y: 0 },
        KnipsaPoint64 { x: 0, y: 10 },
    ];

    #[test]
    fn exposes_version_and_status_messages() {
        // SAFETY: Both pointers refer to static NUL-terminated strings.
        unsafe {
            assert_eq!(CStr::from_ptr(knipsa_version()).to_str(), Ok("0.0.0"));
            assert_eq!(CStr::from_ptr(knipsa_status_message(KnipsaStatus::Ok)).to_str(), Ok("ok"));
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::KernelNotReady)).to_str(),
                Ok("polygon clipping kernel is not ready")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::NullPointer)).to_str(),
                Ok("required pointer is null")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::InvalidPath)).to_str(),
                Ok("invalid path")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::ArithmeticOverflow)).to_str(),
                Ok("checked arithmetic overflow")
            );
        }
    }

    #[test]
    fn validates_null_empty_valid_and_invalid_inputs() {
        assert_eq!(
            knipsa_validate_paths64(std::ptr::null(), 0, KnipsaPathKind::Closed),
            KnipsaStatus::Ok
        );
        assert_eq!(
            knipsa_validate_paths64(std::ptr::null(), 1, KnipsaPathKind::Closed),
            KnipsaStatus::NullPointer
        );
        let empty = KnipsaPath64 { points: std::ptr::null(), point_count: 0 };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&empty), 1, KnipsaPathKind::Closed),
            KnipsaStatus::Ok
        );
        let valid = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: TRIANGLE.len() };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&valid), 1, KnipsaPathKind::Closed),
            KnipsaStatus::Ok
        );
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&valid), 1, KnipsaPathKind::Open),
            KnipsaStatus::Ok
        );
        let invalid = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: 1 };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&invalid), 1, KnipsaPathKind::Closed),
            KnipsaStatus::InvalidPath
        );
        let bad_pointer = KnipsaPath64 { points: std::ptr::null(), point_count: 1 };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&bad_pointer), 1, KnipsaPathKind::Closed),
            KnipsaStatus::NullPointer
        );
    }

    #[test]
    fn classifies_points_and_rejects_bad_output_pointer() {
        let path = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: TRIANGLE.len() };
        assert_eq!(
            knipsa_point_in_polygon64(path, KnipsaPoint64 { x: 1, y: 1 }, std::ptr::null_mut()),
            KnipsaStatus::NullPointer
        );
        let mut location = KnipsaLocation::Outside;
        assert_eq!(
            knipsa_point_in_polygon64(
                path,
                KnipsaPoint64 { x: 1, y: 1 },
                std::ptr::from_mut(&mut location)
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(location, KnipsaLocation::Inside);
        assert_eq!(
            knipsa_point_in_polygon64(
                path,
                KnipsaPoint64 { x: 10, y: 10 },
                std::ptr::from_mut(&mut location)
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(location, KnipsaLocation::Outside);
        let empty = KnipsaPath64 { points: std::ptr::null(), point_count: 0 };
        assert_eq!(
            knipsa_point_in_polygon64(
                empty,
                KnipsaPoint64 { x: 0, y: 0 },
                std::ptr::from_mut(&mut location)
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(location, KnipsaLocation::Outside);
        assert_eq!(
            knipsa_point_in_polygon64(
                path,
                KnipsaPoint64 { x: 0, y: 0 },
                std::ptr::from_mut(&mut location)
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(location, KnipsaLocation::Boundary);
        let invalid = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: 2 };
        assert_eq!(
            knipsa_point_in_polygon64(
                invalid,
                KnipsaPoint64 { x: 0, y: 0 },
                std::ptr::from_mut(&mut location)
            ),
            KnipsaStatus::InvalidPath
        );
        let bad_pointer = KnipsaPath64 { points: std::ptr::null(), point_count: 1 };
        assert_eq!(
            knipsa_point_in_polygon64(
                bad_pointer,
                KnipsaPoint64 { x: 0, y: 0 },
                std::ptr::from_mut(&mut location)
            ),
            KnipsaStatus::NullPointer
        );
        assert_eq!(status_from_error(&Error::ArithmeticOverflow), KnipsaStatus::ArithmeticOverflow);
        assert_eq!(status_from_error(&Error::KernelNotReady), KnipsaStatus::KernelNotReady);
    }
}
