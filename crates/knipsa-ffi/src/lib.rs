#![doc = include_str!("lib.md")]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![doc(test(attr(deny(warnings))))]

use std::{
    ffi::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

#[cfg(test)]
use std::cell::Cell;

use knipsa::{
    BooleanRequest, BooleanRequestD, ClipType, EndType, Error, FillRule, JoinType, OffsetOptions,
    Path64, PathD, PathKind, Point64, PointD, PointLocation, boolean_op, boolean_opd,
    offset_paths_d, point_in_polygon, triangulate_d, triangulate64, validate_paths_d,
    validate_paths64,
};

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

/// A fixed-layout floating-point point for FFI callers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct KnipsaPointD {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl From<KnipsaPointD> for PointD {
    fn from(point: KnipsaPointD) -> Self {
        Self::new(point.x, point.y)
    }
}

/// A borrowed integer point slice.
///
/// A null `points` pointer is valid only when `point_count` is zero. The
/// pointer must remain readable for the duration of the call that receives
/// this descriptor; ownership stays with the caller.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct KnipsaPath64 {
    /// Pointer to the first point.
    pub points: *const KnipsaPoint64,
    /// Number of points at `points`.
    pub point_count: usize,
}

/// A borrowed floating-point point slice.
///
/// A null `points` pointer is valid only when `point_count` is zero. The
/// pointer must remain readable for the duration of the call that receives
/// this descriptor; ownership stays with the caller.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct KnipsaPathD {
    /// Pointer to the first point.
    pub points: *const KnipsaPointD,
    /// Number of points at `points`.
    pub point_count: usize,
}

/// An owned integer result returned by a Rust operation.
///
/// Release both the descriptor array and its point arrays with
/// [`knipsa_free_paths64`]. A zero-path result has `paths == NULL` and
/// `path_count == 0`. Treat the returned descriptors and points as read-only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct KnipsaPaths64 {
    /// Pointer to the returned path descriptors.
    pub paths: *mut KnipsaPath64,
    /// Number of descriptors at `paths`.
    pub path_count: usize,
}

/// An owned floating-point result returned by a Rust operation.
///
/// Release both the descriptor array and its point arrays with
/// [`knipsa_free_paths_d`]. A zero-path result has `paths == NULL` and
/// `path_count == 0`. Treat the returned descriptors and points as read-only.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct KnipsaPathsD {
    /// Pointer to the returned path descriptors.
    pub paths: *mut KnipsaPathD,
    /// Number of descriptors at `paths`.
    pub path_count: usize,
}

/// Options shared by the integer and floating-point offset entry points.
///
/// The enum fields use the numeric values documented by the C header so that
/// unknown values can be rejected without constructing an invalid Rust enum at
/// the ABI boundary. Use [`Default::default`] for round polygon offsets, then
/// change the fields that matter to the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct KnipsaOffsetOptions {
    /// Corner style: `0` square, `1` bevel, `2` round, or `3` miter.
    pub join_type: u8,
    /// Endpoint style: `0` polygon, `1` joined, `2` butt, `3` square, or `4`
    /// round.
    pub end_type: u8,
    /// Non-zero keeps collinear vertices in output rings.
    pub preserve_collinear: u8,
    /// Reserved padding; set this to zero.
    pub reserved: u8,
    /// Maximum miter length divided by the absolute offset distance.
    pub miter_limit: f64,
    /// Maximum round-join deviation in input units; zero selects the default.
    pub arc_tolerance: f64,
}

impl KnipsaOffsetOptions {
    /// Creates the default round-join polygon options.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            join_type: 2,
            end_type: 0,
            preserve_collinear: 0,
            reserved: 0,
            miter_limit: 2.0,
            arc_tolerance: 0.0,
        }
    }
}

impl Default for KnipsaOffsetOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The corner style accepted by offset operations.
///
/// The numeric values are part of the C ABI and mirror the generated header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaJoinType {
    /// A square corner.
    Square = 0,
    /// A bevelled corner.
    Bevel = 1,
    /// A circular corner.
    Round = 2,
    /// A mitered corner.
    Miter = 3,
}

/// The endpoint style accepted by offset operations.
///
/// `Polygon` is for closed rings; all other values describe open polylines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaEndType {
    /// Treat paths as closed polygons.
    Polygon = 0,
    /// Join the two sides of an open path.
    Joined = 1,
    /// Butt cap.
    Butt = 2,
    /// Square cap.
    Square = 3,
    /// Round cap.
    Round = 4,
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

/// Boolean operation values used by the C ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaClipType {
    /// Keep the region present in both inputs.
    Intersection = 1,
    /// Keep the region present in either input.
    Union = 2,
    /// Keep subject regions outside clip regions.
    Difference = 3,
    /// Keep regions present in exactly one input.
    Xor = 4,
}

/// Fill-rule values used by the C ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum KnipsaFillRule {
    /// Alternate filled state at every crossing.
    EvenOdd = 0,
    /// Fill where the winding number is non-zero.
    NonZero = 1,
    /// Fill only positively wound regions.
    Positive = 2,
    /// Fill only negatively wound regions.
    Negative = 3,
}

/// Stable status codes returned by exported functions.
///
/// Functions return a status instead of allowing a Rust panic or exception to
/// cross the ABI boundary. The output descriptor is reset before operations
/// that produce an owned result.
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
    /// An exact result could not be represented by the integer API.
    NonIntegralResult = 4,
    /// The arrangement could not be closed into output rings.
    TopologyFailure = 5,
    /// An operation or fill-rule value was not recognized.
    InvalidArgument = 6,
    /// A Rust panic was contained at the ABI boundary.
    InternalError = 7,
    /// Input paths intersect where a triangulation requires disjoint rings.
    IntersectingPaths = 8,
    /// A floating-point coordinate is NaN or infinite.
    NonFiniteCoordinate = 9,
    /// Offset parameters are not geometrically meaningful.
    InvalidOffset = 10,
    /// The input topology could not be triangulated.
    TriangulationFailure = 11,
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

const VERSION: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();
const STATUS_OK: &[u8] = b"ok\0";
const STATUS_NULL: &[u8] = b"required pointer is null\0";
const STATUS_PATH: &[u8] = b"invalid path\0";
const STATUS_OVERFLOW: &[u8] = b"checked arithmetic overflow\0";
const STATUS_NON_INTEGRAL: &[u8] = b"result contains a non-integral coordinate\0";
const STATUS_TOPOLOGY: &[u8] = b"polygon arrangement did not close\0";
const STATUS_ARGUMENT: &[u8] = b"invalid operation or fill rule\0";
const STATUS_INTERNAL: &[u8] = b"internal error\0";
const STATUS_INTERSECTING: &[u8] = b"input paths intersect\0";
const STATUS_NON_FINITE: &[u8] = b"coordinate is not finite\0";
const STATUS_INVALID_OFFSET: &[u8] = b"invalid offset parameters\0";
const STATUS_TRIANGULATION: &[u8] = b"triangulation failed\0";
const STATUS_UNKNOWN: &[u8] = b"unknown status\0";

#[cfg(test)]
thread_local! {
    static FORCE_BOOLEAN_PANIC: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
fn test_panic_if_requested() {
    assert!(!FORCE_BOOLEAN_PANIC.with(Cell::get), "test panic at the ABI boundary");
}

/// Returns the knipsa FFI version as a static NUL-terminated string.
///
/// The returned pointer is owned by the library and must not be freed.
#[unsafe(no_mangle)]
pub extern "C" fn knipsa_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

/// Returns a static NUL-terminated description for a status code.
///
/// The returned pointer is owned by the library and must not be freed.
#[unsafe(no_mangle)]
pub extern "C" fn knipsa_status_message(status: u8) -> *const c_char {
    match status {
        0 => STATUS_OK.as_ptr().cast(),
        1 => STATUS_NULL.as_ptr().cast(),
        2 => STATUS_PATH.as_ptr().cast(),
        3 => STATUS_OVERFLOW.as_ptr().cast(),
        4 => STATUS_NON_INTEGRAL.as_ptr().cast(),
        5 => STATUS_TOPOLOGY.as_ptr().cast(),
        6 => STATUS_ARGUMENT.as_ptr().cast(),
        7 => STATUS_INTERNAL.as_ptr().cast(),
        8 => STATUS_INTERSECTING.as_ptr().cast(),
        9 => STATUS_NON_FINITE.as_ptr().cast(),
        10 => STATUS_INVALID_OFFSET.as_ptr().cast(),
        11 => STATUS_TRIANGULATION.as_ptr().cast(),
        _ => STATUS_UNKNOWN.as_ptr().cast(),
    }
}

/// Validates a borrowed array of paths.
///
/// A null `paths` pointer is accepted when `path_count` is zero. Each path's
/// point pointer follows the same rule. Inputs remain owned by the caller.
///
/// # Safety
///
/// When either count is non-zero, the corresponding pointer and every
/// non-empty point pointer must refer to readable memory for the duration of
/// the call.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_validate_paths64(
    paths: *const KnipsaPath64,
    path_count: usize,
    kind: u8,
) -> KnipsaStatus {
    let Some(kind) = path_kind_from_u8(kind) else {
        return KnipsaStatus::InvalidArgument;
    };
    let paths = match copy_paths64(paths, path_count) {
        Ok(paths) => paths,
        Err(status) => return status,
    };
    match validate_paths64(&paths, kind) {
        Ok(()) => KnipsaStatus::Ok,
        Err(error) => status_from_error(&error),
    }
}

/// Validates a borrowed array of floating-point paths.
///
/// A null `paths` pointer is accepted when `path_count` is zero. Coordinates
/// must be finite. The `kind` argument accepts `KNIPSA_PATH_CLOSED` or
/// `KNIPSA_PATH_OPEN`.
///
/// # Safety
///
/// When either count is non-zero, the corresponding pointer and every
/// non-empty point pointer must refer to readable memory for the duration of
/// the call.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_validate_paths_d(
    paths: *const KnipsaPathD,
    path_count: usize,
    kind: u8,
) -> KnipsaStatus {
    let Some(kind) = path_kind_from_u8(kind) else {
        return KnipsaStatus::InvalidArgument;
    };
    let paths = match copy_paths_d(paths, path_count) {
        Ok(paths) => paths,
        Err(status) => return status,
    };
    match validate_paths_d(&paths, kind) {
        Ok(()) => KnipsaStatus::Ok,
        Err(error) => status_from_error(&error),
    }
}

/// Executes an integer boolean operation and allocates the result for the
/// caller. The operation and fill-rule arguments use the values from
/// `KnipsaClipType` and `KnipsaFillRule`.
///
/// On success, release `result` with [`knipsa_free_paths64`]. The input arrays
/// remain owned by the caller. A null input array is valid when its count is
/// zero.
///
/// # Safety
///
/// `result` must point to an empty, zero-initialized `KnipsaPaths64` storage
/// slot, or to a slot whose previous result has been released. Non-empty input
/// arrays and their point buffers must be readable for the duration of the
/// call. Passing a live result without releasing it first returns
/// [`KnipsaStatus::InvalidArgument`] and leaves that result untouched.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_boolean64(
    subjects: *const KnipsaPath64,
    subject_count: usize,
    clips: *const KnipsaPath64,
    clip_count: usize,
    clip_type: u8,
    fill_rule: u8,
    result: *mut KnipsaPaths64,
) -> KnipsaStatus {
    if result.is_null() {
        return KnipsaStatus::NullPointer;
    }
    if !result_is_empty64(result) {
        return KnipsaStatus::InvalidArgument;
    }
    // SAFETY: `result` was checked for null and is writable for this call.
    unsafe {
        *result = KnipsaPaths64::default();
    }
    let Some(clip_type) = clip_type_from_u8(clip_type) else {
        return KnipsaStatus::InvalidArgument;
    };
    let Some(fill_rule) = fill_rule_from_u8(fill_rule) else {
        return KnipsaStatus::InvalidArgument;
    };

    let operation = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        test_panic_if_requested();
        let subjects = copy_paths64(subjects, subject_count)?;
        let clips = copy_paths64(clips, clip_count)?;
        boolean_op(BooleanRequest { subjects: &subjects, clips: &clips, clip_type, fill_rule })
            .map_err(|error| status_from_error(&error))
    }));
    match operation {
        Ok(Ok(paths)) => {
            write_paths64(paths, result);
            KnipsaStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => KnipsaStatus::InternalError,
    }
}

/// Executes a floating-point boolean operation and allocates the result for
/// the caller. Coordinates are accepted as IEEE-754 `double` values and are
/// checked for finiteness by the Rust API.
///
/// On success, release `result` with [`knipsa_free_paths_d`]. The input arrays
/// remain owned by the caller. A null input array is valid when its count is
/// zero.
///
/// # Safety
///
/// `result` must point to an empty, zero-initialized `KnipsaPathsD` storage
/// slot, or to a slot whose previous result has been released. Non-empty input
/// arrays and their point buffers must be readable for the duration of the
/// call. Passing a live result without releasing it first returns
/// [`KnipsaStatus::InvalidArgument`] and leaves that result untouched.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_boolean_d(
    subjects: *const KnipsaPathD,
    subject_count: usize,
    clips: *const KnipsaPathD,
    clip_count: usize,
    clip_type: u8,
    fill_rule: u8,
    result: *mut KnipsaPathsD,
) -> KnipsaStatus {
    if result.is_null() {
        return KnipsaStatus::NullPointer;
    }
    if !result_is_empty_d(result) {
        return KnipsaStatus::InvalidArgument;
    }
    // SAFETY: `result` was checked for null and is writable for this call.
    unsafe {
        *result = KnipsaPathsD::default();
    }
    let Some(clip_type) = clip_type_from_u8(clip_type) else {
        return KnipsaStatus::InvalidArgument;
    };
    let Some(fill_rule) = fill_rule_from_u8(fill_rule) else {
        return KnipsaStatus::InvalidArgument;
    };

    let operation = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        test_panic_if_requested();
        let subjects = copy_paths_d(subjects, subject_count)?;
        let clips = copy_paths_d(clips, clip_count)?;
        boolean_opd(BooleanRequestD { subjects: &subjects, clips: &clips, clip_type, fill_rule })
            .map_err(|error| status_from_error(&error))
    }));
    match operation {
        Ok(Ok(paths)) => {
            write_paths_d(paths, result);
            KnipsaStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => KnipsaStatus::InternalError,
    }
}

/// Offsets integer-coordinate paths and returns the floating-point outline.
/// Integer input coordinates are converted exactly when they fit in the
/// exact `f64` integer range; joins and round caps are not rounded to integers
/// by this entry point.
///
/// On success, release `result` with [`knipsa_free_paths_d`]. The offset style
/// and tolerances are supplied through [`KnipsaOffsetOptions`].
///
/// # Safety
///
/// `options` must point to a readable [`KnipsaOffsetOptions`] value. `result`
/// must point to an empty, zero-initialized `KnipsaPathsD` storage slot, or to
/// a slot whose previous result has been released. Non-empty input arrays and
/// their point buffers must be readable for the duration of the call. Passing
/// a live result without releasing it first returns
/// [`KnipsaStatus::InvalidArgument`] and leaves that result untouched.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_offset64(
    paths: *const KnipsaPath64,
    path_count: usize,
    delta: f64,
    options: *const KnipsaOffsetOptions,
    result: *mut KnipsaPathsD,
) -> KnipsaStatus {
    if result.is_null() {
        return KnipsaStatus::NullPointer;
    }
    if !result_is_empty_d(result) {
        return KnipsaStatus::InvalidArgument;
    }
    // SAFETY: `result` was checked for null and is writable for this call.
    unsafe {
        *result = KnipsaPathsD::default();
    }
    let options = match copy_offset_options(options) {
        Ok(options) => options,
        Err(status) => return status,
    };
    let operation = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        test_panic_if_requested();
        let paths = copy_paths64(paths, path_count)?;
        let paths = paths64_to_d(&paths)?;
        offset_paths_d(&paths, delta, options).map_err(|error| status_from_error(&error))
    }));
    match operation {
        Ok(Ok(paths)) => {
            write_paths_d(paths, result);
            KnipsaStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => KnipsaStatus::InternalError,
    }
}

/// Offsets floating-point paths and returns floating-point polygon outlines.
///
/// On success, release `result` with [`knipsa_free_paths_d`]. The offset style
/// and tolerances are supplied through [`KnipsaOffsetOptions`].
///
/// # Safety
///
/// `options` must point to a readable [`KnipsaOffsetOptions`] value. `result`
/// must point to an empty, zero-initialized `KnipsaPathsD` storage slot, or to
/// a slot whose previous result has been released. Non-empty input arrays and
/// their point buffers must be readable for the duration of the call. Passing
/// a live result without releasing it first returns
/// [`KnipsaStatus::InvalidArgument`] and leaves that result untouched.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_offset_d(
    paths: *const KnipsaPathD,
    path_count: usize,
    delta: f64,
    options: *const KnipsaOffsetOptions,
    result: *mut KnipsaPathsD,
) -> KnipsaStatus {
    if result.is_null() {
        return KnipsaStatus::NullPointer;
    }
    if !result_is_empty_d(result) {
        return KnipsaStatus::InvalidArgument;
    }
    // SAFETY: `result` was checked for null and is writable for this call.
    unsafe {
        *result = KnipsaPathsD::default();
    }
    let options = match copy_offset_options(options) {
        Ok(options) => options,
        Err(status) => return status,
    };
    let operation = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        test_panic_if_requested();
        let paths = copy_paths_d(paths, path_count)?;
        offset_paths_d(&paths, delta, options).map_err(|error| status_from_error(&error))
    }));
    match operation {
        Ok(Ok(paths)) => {
            write_paths_d(paths, result);
            KnipsaStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => KnipsaStatus::InternalError,
    }
}

/// Triangulates integer-coordinate rings. Each returned triangle is exposed
/// as a three-point path in `KnipsaPaths64`.
///
/// On success, release `result` with [`knipsa_free_paths64`].
///
/// # Safety
///
/// `result` must point to an empty, zero-initialized `KnipsaPaths64` storage
/// slot, or to a slot whose previous result has been released. Non-empty input
/// arrays and their point buffers must be readable for the duration of the
/// call. Passing a live result without releasing it first returns
/// [`KnipsaStatus::InvalidArgument`] and leaves that result untouched.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_triangulate64(
    paths: *const KnipsaPath64,
    path_count: usize,
    fill_rule: u8,
    result: *mut KnipsaPaths64,
) -> KnipsaStatus {
    if result.is_null() {
        return KnipsaStatus::NullPointer;
    }
    if !result_is_empty64(result) {
        return KnipsaStatus::InvalidArgument;
    }
    // SAFETY: `result` was checked for null and is writable for this call.
    unsafe {
        *result = KnipsaPaths64::default();
    }
    let Some(fill_rule) = fill_rule_from_u8(fill_rule) else {
        return KnipsaStatus::InvalidArgument;
    };
    let operation = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        test_panic_if_requested();
        let paths = copy_paths64(paths, path_count)?;
        triangulate64(&paths, fill_rule)
            .map(|triangles| {
                triangles.into_iter().map(|triangle| triangle.into_iter().collect()).collect()
            })
            .map_err(|error| status_from_error(&error))
    }));
    match operation {
        Ok(Ok(paths)) => {
            write_paths64(paths, result);
            KnipsaStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => KnipsaStatus::InternalError,
    }
}

/// Triangulates floating-point rings. Each returned triangle is exposed as a
/// three-point path in `KnipsaPathsD`.
///
/// On success, release `result` with [`knipsa_free_paths_d`].
///
/// # Safety
///
/// `result` must point to an empty, zero-initialized `KnipsaPathsD` storage
/// slot, or to a slot whose previous result has been released. Non-empty input
/// arrays and their point buffers must be readable for the duration of the
/// call. Passing a live result without releasing it first returns
/// [`KnipsaStatus::InvalidArgument`] and leaves that result untouched.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_triangulate_d(
    paths: *const KnipsaPathD,
    path_count: usize,
    fill_rule: u8,
    result: *mut KnipsaPathsD,
) -> KnipsaStatus {
    if result.is_null() {
        return KnipsaStatus::NullPointer;
    }
    if !result_is_empty_d(result) {
        return KnipsaStatus::InvalidArgument;
    }
    // SAFETY: `result` was checked for null and is writable for this call.
    unsafe {
        *result = KnipsaPathsD::default();
    }
    let Some(fill_rule) = fill_rule_from_u8(fill_rule) else {
        return KnipsaStatus::InvalidArgument;
    };
    let operation = catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        test_panic_if_requested();
        let paths = copy_paths_d(paths, path_count)?;
        triangulate_d(&paths, fill_rule)
            .map(|triangles| {
                triangles.into_iter().map(|triangle| triangle.into_iter().collect()).collect()
            })
            .map_err(|error| status_from_error(&error))
    }));
    match operation {
        Ok(Ok(paths)) => {
            write_paths_d(paths, result);
            KnipsaStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => KnipsaStatus::InternalError,
    }
}

/// Releases a result returned by [`knipsa_boolean64`] or
/// [`knipsa_triangulate64`]. It is safe to pass a null pointer or to call this
/// function again after it zeroes the result.
///
/// The pointer must be a valid pointer to a result descriptor previously
/// initialized by this library or a zeroed descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_free_paths64(result: *mut KnipsaPaths64) {
    if result.is_null() {
        return;
    }
    // SAFETY: The pointer is supplied by the caller and is checked above.
    let owned = unsafe { *result };
    if owned.paths.is_null() {
        // SAFETY: `result` is non-null and points to the caller's result slot.
        unsafe {
            *result = KnipsaPaths64::default();
        }
        return;
    }
    // SAFETY: `owned.paths` and each point allocation were produced by
    // `write_paths64`, with exactly these lengths.
    unsafe {
        let descriptors = slice::from_raw_parts_mut(owned.paths, owned.path_count);
        for path in descriptors {
            if !path.points.is_null() {
                let points =
                    ptr::slice_from_raw_parts_mut(path.points.cast_mut(), path.point_count);
                drop(Box::from_raw(points));
            }
            *path = KnipsaPath64 { points: ptr::null(), point_count: 0 };
        }
        let descriptor_slice = ptr::slice_from_raw_parts_mut(owned.paths, owned.path_count);
        drop(Box::from_raw(descriptor_slice));
        *result = KnipsaPaths64::default();
    }
}

/// Releases a result returned by [`knipsa_boolean_d`], [`knipsa_offset64`],
/// [`knipsa_offset_d`], or [`knipsa_triangulate_d`]. It is safe to pass a null
/// pointer or to call this function again after it zeroes the result.
///
/// The pointer must be a valid pointer to a result descriptor previously
/// initialized by this library or a zeroed descriptor.
#[unsafe(no_mangle)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn knipsa_free_paths_d(result: *mut KnipsaPathsD) {
    if result.is_null() {
        return;
    }
    // SAFETY: The pointer is supplied by the caller and is checked above.
    let owned = unsafe { *result };
    if owned.paths.is_null() {
        // SAFETY: `result` is non-null and points to the caller's result slot.
        unsafe {
            *result = KnipsaPathsD::default();
        }
        return;
    }
    // SAFETY: `owned.paths` and each point allocation were produced by
    // `write_paths_d`, with exactly these lengths.
    unsafe {
        let descriptors = slice::from_raw_parts_mut(owned.paths, owned.path_count);
        for path in descriptors {
            if !path.points.is_null() {
                let points =
                    ptr::slice_from_raw_parts_mut(path.points.cast_mut(), path.point_count);
                drop(Box::from_raw(points));
            }
            *path = KnipsaPathD { points: ptr::null(), point_count: 0 };
        }
        let descriptor_slice = ptr::slice_from_raw_parts_mut(owned.paths, owned.path_count);
        drop(Box::from_raw(descriptor_slice));
        *result = KnipsaPathsD::default();
    }
}

/// Classifies one point against a borrowed closed path using the even-odd rule.
///
/// # Safety
///
/// `location` must point to writable `KnipsaLocation` storage. If
/// `path.point_count` is non-zero, `path.points` must point to readable
/// `KnipsaPoint64` values for the duration of the call.
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
        Error::InvalidPath { .. } => KnipsaStatus::InvalidPath,
        Error::NonFiniteCoordinate { .. } => KnipsaStatus::NonFiniteCoordinate,
        Error::ArithmeticOverflow => KnipsaStatus::ArithmeticOverflow,
        Error::NonIntegralResult => KnipsaStatus::NonIntegralResult,
        Error::TopologyFailure => KnipsaStatus::TopologyFailure,
        Error::InvalidOffset => KnipsaStatus::InvalidOffset,
        Error::TriangulationFailure => KnipsaStatus::TriangulationFailure,
        Error::IntersectingPaths => KnipsaStatus::IntersectingPaths,
    }
}

fn path_kind_from_u8(value: u8) -> Option<PathKind> {
    match value {
        0 => Some(PathKind::Closed),
        1 => Some(PathKind::Open),
        _ => None,
    }
}

fn copy_offset_options(options: *const KnipsaOffsetOptions) -> Result<OffsetOptions, KnipsaStatus> {
    if options.is_null() {
        return Err(KnipsaStatus::NullPointer);
    }
    // SAFETY: The public functions require a readable options descriptor and
    // reject a null pointer above.
    let options = unsafe { &*options };
    if options.reserved != 0 {
        return Err(KnipsaStatus::InvalidArgument);
    }
    offset_options_from_u8(
        options.join_type,
        options.end_type,
        options.miter_limit,
        options.arc_tolerance,
        options.preserve_collinear,
    )
    .ok_or(KnipsaStatus::InvalidArgument)
}

fn offset_options_from_u8(
    join_type: u8,
    end_type: u8,
    miter_limit: f64,
    arc_tolerance: f64,
    preserve_collinear: u8,
) -> Option<OffsetOptions> {
    let join_type = match join_type {
        0 => JoinType::Square,
        1 => JoinType::Bevel,
        2 => JoinType::Round,
        3 => JoinType::Miter,
        _ => return None,
    };
    let end_type = match end_type {
        0 => EndType::Polygon,
        1 => EndType::Joined,
        2 => EndType::Butt,
        3 => EndType::Square,
        4 => EndType::Round,
        _ => return None,
    };
    Some(OffsetOptions {
        join_type,
        end_type,
        miter_limit,
        arc_tolerance,
        preserve_collinear: preserve_collinear != 0,
    })
}

#[allow(clippy::cast_precision_loss)]
fn paths64_to_d(paths: &[Path64]) -> Result<Vec<PathD>, KnipsaStatus> {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|point| {
                    if point.x.unsigned_abs() > (1_u64 << 53)
                        || point.y.unsigned_abs() > (1_u64 << 53)
                    {
                        return Err(KnipsaStatus::ArithmeticOverflow);
                    }
                    Ok(PointD::new(point.x as f64, point.y as f64))
                })
                .collect()
        })
        .collect()
}

fn clip_type_from_u8(value: u8) -> Option<ClipType> {
    match value {
        1 => Some(ClipType::Intersection),
        2 => Some(ClipType::Union),
        3 => Some(ClipType::Difference),
        4 => Some(ClipType::Xor),
        _ => None,
    }
}

fn fill_rule_from_u8(value: u8) -> Option<FillRule> {
    match value {
        0 => Some(FillRule::EvenOdd),
        1 => Some(FillRule::NonZero),
        2 => Some(FillRule::Positive),
        3 => Some(FillRule::Negative),
        _ => None,
    }
}

fn result_is_empty64(result: *const KnipsaPaths64) -> bool {
    // SAFETY: Public callers check this pointer for null before calling the
    // helper, and the descriptor points to caller-owned writable storage.
    let result = unsafe { &*result };
    result.paths.is_null() && result.path_count == 0
}

fn result_is_empty_d(result: *const KnipsaPathsD) -> bool {
    // SAFETY: Public callers check this pointer for null before calling the
    // helper, and the descriptor points to caller-owned writable storage.
    let result = unsafe { &*result };
    result.paths.is_null() && result.path_count == 0
}

fn copy_paths64(
    paths: *const KnipsaPath64,
    path_count: usize,
) -> Result<Vec<Path64>, KnipsaStatus> {
    let descriptors = if path_count == 0 {
        &[]
    } else if paths.is_null() {
        return Err(KnipsaStatus::NullPointer);
    } else {
        // SAFETY: A non-null pointer and element count are supplied according
        // to the borrowed-slice contract of this ABI.
        unsafe { slice::from_raw_parts(paths, path_count) }
    };
    descriptors
        .iter()
        .map(|path| {
            let points = if path.point_count == 0 {
                &[]
            } else if path.points.is_null() {
                return Err(KnipsaStatus::NullPointer);
            } else {
                // SAFETY: The descriptor contains a non-null point pointer
                // and the caller owns that memory for this call.
                unsafe { slice::from_raw_parts(path.points, path.point_count) }
            };
            Ok(points.iter().copied().map(Into::into).collect())
        })
        .collect()
}

fn copy_paths_d(paths: *const KnipsaPathD, path_count: usize) -> Result<Vec<PathD>, KnipsaStatus> {
    let descriptors = if path_count == 0 {
        &[]
    } else if paths.is_null() {
        return Err(KnipsaStatus::NullPointer);
    } else {
        // SAFETY: A non-null pointer and element count are supplied according
        // to the borrowed-slice contract of this ABI.
        unsafe { slice::from_raw_parts(paths, path_count) }
    };
    descriptors
        .iter()
        .map(|path| {
            let points = if path.point_count == 0 {
                &[]
            } else if path.points.is_null() {
                return Err(KnipsaStatus::NullPointer);
            } else {
                // SAFETY: The descriptor contains a non-null point pointer
                // and the caller owns that memory for this call.
                unsafe { slice::from_raw_parts(path.points, path.point_count) }
            };
            Ok(points.iter().copied().map(Into::into).collect())
        })
        .collect()
}

fn write_paths64(paths: Vec<Path64>, result: *mut KnipsaPaths64) {
    let descriptors: Vec<KnipsaPath64> = paths
        .into_iter()
        .map(|path| {
            let points: Vec<KnipsaPoint64> =
                path.into_iter().map(|point| KnipsaPoint64 { x: point.x, y: point.y }).collect();
            let point_count = points.len();
            let points = points.into_boxed_slice();
            let points = Box::into_raw(points).cast::<KnipsaPoint64>();
            KnipsaPath64 { points, point_count }
        })
        .collect();
    let descriptors = descriptors.into_boxed_slice();
    let path_count = descriptors.len();
    let paths = if descriptors.is_empty() {
        ptr::null_mut()
    } else {
        Box::into_raw(descriptors).cast::<KnipsaPath64>()
    };
    // SAFETY: `result` was checked by the public function before this helper
    // was called.
    unsafe {
        (*result).paths = paths;
        (*result).path_count = path_count;
    }
}

fn write_paths_d(paths: Vec<PathD>, result: *mut KnipsaPathsD) {
    let descriptors: Vec<KnipsaPathD> = paths
        .into_iter()
        .map(|path| {
            let points: Vec<KnipsaPointD> =
                path.into_iter().map(|point| KnipsaPointD { x: point.x, y: point.y }).collect();
            let point_count = points.len();
            let points = points.into_boxed_slice();
            let points = Box::into_raw(points).cast::<KnipsaPointD>();
            KnipsaPathD { points, point_count }
        })
        .collect();
    let descriptors = descriptors.into_boxed_slice();
    let path_count = descriptors.len();
    let paths = if descriptors.is_empty() {
        ptr::null_mut()
    } else {
        Box::into_raw(descriptors).cast::<KnipsaPathD>()
    };
    // SAFETY: `result` was checked by the public function before this helper
    // was called.
    unsafe {
        (*result).paths = paths;
        (*result).path_count = path_count;
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

    const TRIANGLE_D: [KnipsaPointD; 3] = [
        KnipsaPointD { x: 0.0, y: 0.0 },
        KnipsaPointD { x: 10.0, y: 0.0 },
        KnipsaPointD { x: 0.0, y: 10.0 },
    ];

    #[test]
    fn exposes_version_and_status_messages() {
        // SAFETY: Both pointers refer to static NUL-terminated strings.
        unsafe {
            assert_eq!(CStr::from_ptr(knipsa_version()).to_str(), Ok("0.1.0"));
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::Ok as u8)).to_str(),
                Ok("ok")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::NullPointer as u8)).to_str(),
                Ok("required pointer is null")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::InvalidPath as u8)).to_str(),
                Ok("invalid path")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::ArithmeticOverflow as u8))
                    .to_str(),
                Ok("checked arithmetic overflow")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::NonIntegralResult as u8))
                    .to_str(),
                Ok("result contains a non-integral coordinate")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::TopologyFailure as u8)).to_str(),
                Ok("polygon arrangement did not close")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::InvalidArgument as u8)).to_str(),
                Ok("invalid operation or fill rule")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::InternalError as u8)).to_str(),
                Ok("internal error")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::IntersectingPaths as u8))
                    .to_str(),
                Ok("input paths intersect")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::NonFiniteCoordinate as u8))
                    .to_str(),
                Ok("coordinate is not finite")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::InvalidOffset as u8)).to_str(),
                Ok("invalid offset parameters")
            );
            assert_eq!(
                CStr::from_ptr(knipsa_status_message(KnipsaStatus::TriangulationFailure as u8))
                    .to_str(),
                Ok("triangulation failed")
            );
            assert_eq!(CStr::from_ptr(knipsa_status_message(255)).to_str(), Ok("unknown status"));
        }
    }

    #[test]
    fn validates_null_empty_valid_and_invalid_inputs() {
        assert_eq!(
            knipsa_validate_paths64(std::ptr::null(), 0, KnipsaPathKind::Closed as u8),
            KnipsaStatus::Ok
        );
        assert_eq!(
            knipsa_validate_paths64(std::ptr::null(), 1, KnipsaPathKind::Closed as u8),
            KnipsaStatus::NullPointer
        );
        let empty = KnipsaPath64 { points: std::ptr::null(), point_count: 0 };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&empty), 1, KnipsaPathKind::Closed as u8),
            KnipsaStatus::Ok
        );
        let valid = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: TRIANGLE.len() };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&valid), 1, KnipsaPathKind::Closed as u8),
            KnipsaStatus::Ok
        );
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&valid), 1, KnipsaPathKind::Open as u8),
            KnipsaStatus::Ok
        );
        let invalid = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: 1 };
        assert_eq!(
            knipsa_validate_paths64(std::ptr::from_ref(&invalid), 1, KnipsaPathKind::Closed as u8),
            KnipsaStatus::InvalidPath
        );
        let bad_pointer = KnipsaPath64 { points: std::ptr::null(), point_count: 1 };
        assert_eq!(
            knipsa_validate_paths64(
                std::ptr::from_ref(&bad_pointer),
                1,
                KnipsaPathKind::Closed as u8
            ),
            KnipsaStatus::NullPointer
        );
        assert_eq!(knipsa_validate_paths64(std::ptr::null(), 0, 99), KnipsaStatus::InvalidArgument);

        let valid_d = KnipsaPathD { points: TRIANGLE_D.as_ptr(), point_count: TRIANGLE_D.len() };
        assert_eq!(
            knipsa_validate_paths_d(std::ptr::from_ref(&valid_d), 1, KnipsaPathKind::Closed as u8,),
            KnipsaStatus::Ok
        );
        let invalid_d_points = [
            KnipsaPointD { x: 0.0, y: 0.0 },
            KnipsaPointD { x: f64::NAN, y: 0.0 },
            KnipsaPointD { x: 0.0, y: 1.0 },
        ];
        let invalid_d =
            KnipsaPathD { points: invalid_d_points.as_ptr(), point_count: invalid_d_points.len() };
        assert_eq!(
            knipsa_validate_paths_d(
                std::ptr::from_ref(&invalid_d),
                1,
                KnipsaPathKind::Closed as u8,
            ),
            KnipsaStatus::NonFiniteCoordinate
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
        assert_eq!(status_from_error(&Error::NonIntegralResult), KnipsaStatus::NonIntegralResult);
        assert_eq!(status_from_error(&Error::TopologyFailure), KnipsaStatus::TopologyFailure);
        assert_eq!(status_from_error(&Error::InvalidOffset), KnipsaStatus::InvalidOffset);
        assert_eq!(
            status_from_error(&Error::TriangulationFailure),
            KnipsaStatus::TriangulationFailure
        );
        assert_eq!(status_from_error(&Error::IntersectingPaths), KnipsaStatus::IntersectingPaths);
    }

    #[test]
    fn executes_and_releases_boolean_result() {
        let square = [
            KnipsaPoint64 { x: 0, y: 0 },
            KnipsaPoint64 { x: 10, y: 0 },
            KnipsaPoint64 { x: 10, y: 10 },
            KnipsaPoint64 { x: 0, y: 10 },
        ];
        let subject = KnipsaPath64 { points: square.as_ptr(), point_count: square.len() };
        let mut result = KnipsaPaths64::default();
        assert_eq!(
            knipsa_boolean64(
                std::ptr::from_ref(&subject),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::NonZero as u8,
                std::ptr::from_mut(&mut result),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(result.path_count, 1);
        assert_eq!(
            knipsa_boolean64(
                std::ptr::from_ref(&subject),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::NonZero as u8,
                std::ptr::from_mut(&mut result),
            ),
            KnipsaStatus::InvalidArgument
        );
        // SAFETY: The result belongs to this test and was returned by the
        // boolean function with the reported descriptor count.
        unsafe {
            let paths = slice::from_raw_parts(result.paths, result.path_count);
            assert_eq!(paths[0].point_count, 4);
            assert_eq!((*paths[0].points).x, 0);
        }
        knipsa_free_paths64(std::ptr::from_mut(&mut result));
        assert_eq!(result, KnipsaPaths64::default());
        knipsa_free_paths64(std::ptr::from_mut(&mut result));
        knipsa_free_paths64(std::ptr::null_mut());

        let subject_d = KnipsaPathD { points: TRIANGLE_D.as_ptr(), point_count: 3 };
        let mut result_d = KnipsaPathsD::default();
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::from_ref(&subject_d),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(result_d.path_count, 1);
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::from_ref(&subject_d),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidArgument
        );
        // SAFETY: The result belongs to this test and was returned by the
        // floating-point boolean function with the reported descriptor count.
        unsafe {
            let paths = slice::from_raw_parts(result_d.paths, result_d.path_count);
            assert_eq!(paths[0].point_count, 3);
            assert_eq!((*paths[0].points).x.to_bits(), 0.0_f64.to_bits());
        }
        knipsa_free_paths_d(std::ptr::from_mut(&mut result_d));
        assert_eq!(result_d, KnipsaPathsD::default());
        knipsa_free_paths_d(std::ptr::from_mut(&mut result_d));
        knipsa_free_paths_d(std::ptr::null_mut());
    }

    #[test]
    fn executes_offsets_and_triangulation() {
        let square = [
            KnipsaPointD { x: 0.0, y: 0.0 },
            KnipsaPointD { x: 10.0, y: 0.0 },
            KnipsaPointD { x: 10.0, y: 10.0 },
            KnipsaPointD { x: 0.0, y: 10.0 },
        ];
        let square_path = KnipsaPathD { points: square.as_ptr(), point_count: square.len() };
        let mut offset = KnipsaPathsD::default();
        let offset_options = KnipsaOffsetOptions {
            join_type: KnipsaJoinType::Miter as u8,
            ..KnipsaOffsetOptions::default()
        };
        assert_eq!(
            knipsa_offset_d(
                std::ptr::from_ref(&square_path),
                1,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut offset),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(offset.path_count, 1);
        // SAFETY: The output was allocated by the offset function and the
        // descriptor count was checked above.
        unsafe {
            assert!(slice::from_raw_parts(offset.paths, offset.path_count)[0].point_count >= 4);
        }
        knipsa_free_paths_d(std::ptr::from_mut(&mut offset));

        let integer_path = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: TRIANGLE.len() };
        let mut integer_offset = KnipsaPathsD::default();
        let offset_options = KnipsaOffsetOptions {
            join_type: KnipsaJoinType::Round as u8,
            ..KnipsaOffsetOptions::default()
        };
        assert_eq!(
            knipsa_offset64(
                std::ptr::from_ref(&integer_path),
                1,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut integer_offset),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(integer_offset.path_count, 1);
        // `knipsa_offset64` returns the floating-point outline directly; a
        // round join must retain at least one non-integral vertex.
        // SAFETY: The output was allocated by the offset function and the
        // descriptor count was checked above.
        unsafe {
            let descriptor =
                &slice::from_raw_parts(integer_offset.paths, integer_offset.path_count)[0];
            let points = slice::from_raw_parts(descriptor.points, descriptor.point_count);
            assert!(
                points
                    .iter()
                    .any(|point| { point.x.fract().abs() > 1e-9 || point.y.fract().abs() > 1e-9 })
            );
        }
        knipsa_free_paths_d(std::ptr::from_mut(&mut integer_offset));

        let mut triangles = KnipsaPaths64::default();
        assert_eq!(
            knipsa_triangulate64(
                std::ptr::from_ref(&integer_path),
                1,
                FillRule::NonZero as u8,
                std::ptr::from_mut(&mut triangles),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(triangles.path_count, 1);
        // SAFETY: The output was allocated by the triangulation function.
        unsafe {
            assert_eq!(
                slice::from_raw_parts(triangles.paths, triangles.path_count)[0].point_count,
                3
            );
        }
        knipsa_free_paths64(std::ptr::from_mut(&mut triangles));

        let mut triangles_d = KnipsaPathsD::default();
        assert_eq!(
            knipsa_triangulate_d(
                std::ptr::from_ref(&square_path),
                1,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut triangles_d),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(triangles_d.path_count, 2);
        knipsa_free_paths_d(std::ptr::from_mut(&mut triangles_d));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn covers_offset_and_triangulation_error_paths() {
        for join_type in 0..=3 {
            for end_type in 0..=4 {
                assert!(offset_options_from_u8(join_type, end_type, 2.0, 0.0, 1).is_some());
            }
        }

        let invalid_64 = KnipsaPath64 { points: TRIANGLE.as_ptr(), point_count: 2 };
        let invalid_d = KnipsaPathD { points: TRIANGLE_D.as_ptr(), point_count: 2 };
        let mut result_d = KnipsaPathsD::default();
        let mut offset_options = KnipsaOffsetOptions::default();
        assert_eq!(
            knipsa_offset64(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::null_mut(),
            ),
            KnipsaStatus::NullPointer
        );
        offset_options.join_type = 99;
        assert_eq!(
            knipsa_offset64(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidArgument
        );
        offset_options.join_type = KnipsaJoinType::Round as u8;
        assert_eq!(
            knipsa_offset64(
                std::ptr::from_ref(&invalid_64),
                1,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidPath
        );
        assert_eq!(
            knipsa_offset_d(
                std::ptr::from_ref(&invalid_d),
                1,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidPath
        );

        let mut result_64 = KnipsaPaths64::default();
        assert_eq!(
            knipsa_triangulate64(
                std::ptr::null(),
                0,
                FillRule::EvenOdd as u8,
                std::ptr::null_mut()
            ),
            KnipsaStatus::NullPointer
        );
        assert_eq!(
            knipsa_triangulate64(
                std::ptr::from_ref(&invalid_64),
                1,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_64),
            ),
            KnipsaStatus::InvalidPath
        );
        assert_eq!(
            knipsa_triangulate_d(std::ptr::null(), 0, 99, std::ptr::from_mut(&mut result_d),),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_triangulate_d(
                std::ptr::from_ref(&invalid_d),
                1,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidPath
        );

        FORCE_BOOLEAN_PANIC.with(|panic| panic.set(true));
        assert_eq!(
            knipsa_offset64(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InternalError
        );
        assert_eq!(
            knipsa_offset_d(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&offset_options),
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InternalError
        );
        assert_eq!(
            knipsa_triangulate64(
                std::ptr::null(),
                0,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_64),
            ),
            KnipsaStatus::InternalError
        );
        assert_eq!(
            knipsa_triangulate_d(
                std::ptr::null(),
                0,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InternalError
        );
        FORCE_BOOLEAN_PANIC.with(|panic| panic.set(false));
    }

    #[test]
    fn rejects_bad_offset_and_triangulation_arguments() {
        assert_eq!(offset_options_from_u8(99, 0, 2.0, 0.0, 0), None);
        assert_eq!(offset_options_from_u8(0, 99, 2.0, 0.0, 0), None);
        assert!(offset_options_from_u8(0, 0, 2.0, 0.0, 1).is_some());
        let invalid_options =
            KnipsaOffsetOptions { join_type: 99, ..KnipsaOffsetOptions::default() };
        let reserved_options =
            KnipsaOffsetOptions { reserved: 1, ..KnipsaOffsetOptions::default() };
        let mut offset = KnipsaPathsD::default();
        assert_eq!(
            knipsa_offset_d(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&invalid_options),
                std::ptr::from_mut(&mut offset),
            ),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_offset_d(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::null(),
                std::ptr::from_mut(&mut offset),
            ),
            KnipsaStatus::NullPointer
        );
        assert_eq!(
            knipsa_offset_d(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&reserved_options),
                std::ptr::from_mut(&mut offset),
            ),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_offset_d(
                std::ptr::null(),
                0,
                1.0,
                std::ptr::from_ref(&KnipsaOffsetOptions::default()),
                std::ptr::null_mut(),
            ),
            KnipsaStatus::NullPointer
        );
        let mut triangles = KnipsaPaths64::default();
        assert_eq!(
            knipsa_triangulate64(std::ptr::null(), 0, 99, std::ptr::from_mut(&mut triangles),),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_triangulate_d(
                std::ptr::null(),
                0,
                FillRule::EvenOdd as u8,
                std::ptr::null_mut(),
            ),
            KnipsaStatus::NullPointer
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn rejects_bad_boolean_arguments_and_pointers() {
        assert_eq!(clip_type_from_u8(1), Some(ClipType::Intersection));
        assert_eq!(clip_type_from_u8(2), Some(ClipType::Union));
        assert_eq!(clip_type_from_u8(3), Some(ClipType::Difference));
        assert_eq!(clip_type_from_u8(4), Some(ClipType::Xor));
        assert_eq!(fill_rule_from_u8(0), Some(FillRule::EvenOdd));
        assert_eq!(fill_rule_from_u8(1), Some(FillRule::NonZero));
        assert_eq!(fill_rule_from_u8(2), Some(FillRule::Positive));
        assert_eq!(fill_rule_from_u8(3), Some(FillRule::Negative));

        let mut result = KnipsaPaths64::default();
        assert_eq!(
            knipsa_boolean64(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                99,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result),
            ),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_boolean64(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                99,
                std::ptr::from_mut(&mut result),
            ),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_boolean64(
                std::ptr::null(),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result),
            ),
            KnipsaStatus::NullPointer
        );

        let mut result_d = KnipsaPathsD::default();
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                99,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                99,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::InvalidArgument
        );
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::null(),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::NullPointer
        );
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::null_mut(),
            ),
            KnipsaStatus::NullPointer
        );
        let bad_point = [
            KnipsaPointD { x: f64::NAN, y: 0.0 },
            KnipsaPointD { x: 1.0, y: 0.0 },
            KnipsaPointD { x: 0.0, y: 1.0 },
        ];
        let bad_path = KnipsaPathD { points: bad_point.as_ptr(), point_count: 3 };
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::from_ref(&bad_path),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result_d),
            ),
            KnipsaStatus::NonFiniteCoordinate
        );
        let bad_integer_point = [KnipsaPoint64 { x: 0, y: 0 }];
        let bad_integer_path = KnipsaPath64 {
            points: bad_integer_point.as_ptr(),
            point_count: bad_integer_point.len(),
        };
        assert_eq!(
            knipsa_boolean64(
                std::ptr::from_ref(&bad_integer_path),
                1,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut result),
            ),
            KnipsaStatus::InvalidPath
        );
        assert_eq!(
            knipsa_boolean64(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::null_mut(),
            ),
            KnipsaStatus::NullPointer
        );

        let empty_d = KnipsaPathD { points: std::ptr::null(), point_count: 0 };
        assert_eq!(copy_paths_d(std::ptr::from_ref(&empty_d), 1).unwrap().len(), 1);
        assert_eq!(
            copy_paths_d(
                std::ptr::from_ref(&KnipsaPathD { points: std::ptr::null(), point_count: 1 }),
                1,
            ),
            Err(KnipsaStatus::NullPointer)
        );
        let empty_64_result = {
            let mut output = KnipsaPaths64::default();
            let status = knipsa_boolean64(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut output),
            );
            assert_eq!(status, KnipsaStatus::Ok);
            output
        };
        assert_eq!(empty_64_result, KnipsaPaths64::default());
        let mut empty_d_result = KnipsaPathsD::default();
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut empty_d_result),
            ),
            KnipsaStatus::Ok
        );
        assert_eq!(empty_d_result, KnipsaPathsD::default());

        FORCE_BOOLEAN_PANIC.with(|panic| panic.set(true));
        let mut panic_result = KnipsaPaths64::default();
        assert_eq!(
            knipsa_boolean64(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut panic_result),
            ),
            KnipsaStatus::InternalError
        );
        let mut panic_result_d = KnipsaPathsD::default();
        assert_eq!(
            knipsa_boolean_d(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                ClipType::Union as u8,
                FillRule::EvenOdd as u8,
                std::ptr::from_mut(&mut panic_result_d),
            ),
            KnipsaStatus::InternalError
        );
        FORCE_BOOLEAN_PANIC.with(|panic| panic.set(false));

        let descriptors_64 =
            vec![KnipsaPath64 { points: std::ptr::null(), point_count: 0 }].into_boxed_slice();
        let mut owned_64 = KnipsaPaths64 {
            paths: Box::into_raw(descriptors_64).cast::<KnipsaPath64>(),
            path_count: 1,
        };
        knipsa_free_paths64(std::ptr::from_mut(&mut owned_64));
        assert_eq!(owned_64, KnipsaPaths64::default());
        let descriptors_d =
            vec![KnipsaPathD { points: std::ptr::null(), point_count: 0 }].into_boxed_slice();
        let mut owned_d = KnipsaPathsD {
            paths: Box::into_raw(descriptors_d).cast::<KnipsaPathD>(),
            path_count: 1,
        };
        knipsa_free_paths_d(std::ptr::from_mut(&mut owned_d));
        assert_eq!(owned_d, KnipsaPathsD::default());
    }
}
