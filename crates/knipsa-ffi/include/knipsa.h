#ifndef KNIPSA_H
#define KNIPSA_H

/* The public C ABI for knipsa. The library owns output buffers; callers own
 * all input buffers and must release outputs with the matching free function.
 */

#include <stddef.h>
#include <stdint.h>

/** Semantic version of the headers and matching knipsa library. */
#define KNIPSA_VERSION_MAJOR 0
#define KNIPSA_VERSION_MINOR 2
#define KNIPSA_VERSION_PATCH 1

#ifdef __cplusplus
extern "C" {
#endif

/** An exact integer-coordinate point. */
typedef struct KnipsaPoint64 {
  /** Horizontal coordinate. */
  int64_t x;
  /** Vertical coordinate. */
  int64_t y;
} KnipsaPoint64;

/** A double-precision point. Coordinates must be finite. */
typedef struct KnipsaPointD {
  /** Horizontal coordinate. */
  double x;
  /** Vertical coordinate. */
  double y;
} KnipsaPointD;

/** An axis-aligned integer clipping rectangle. */
typedef struct KnipsaRect64 {
  /** Lower horizontal bound. */
  int64_t min_x;
  /** Lower vertical bound. */
  int64_t min_y;
  /** Upper horizontal bound. */
  int64_t max_x;
  /** Upper vertical bound. */
  int64_t max_y;
} KnipsaRect64;

/** An axis-aligned floating-point clipping rectangle. Bounds must be finite. */
typedef struct KnipsaRectD {
  /** Lower horizontal bound. */
  double min_x;
  /** Lower vertical bound. */
  double min_y;
  /** Upper horizontal bound. */
  double max_x;
  /** Upper vertical bound. */
  double max_y;
} KnipsaRectD;

/** A borrowed integer path.
 *
 * `points` may be NULL only when `point_count` is zero. The caller keeps
 * ownership of the pointed-to memory for the whole API call.
 */
typedef struct KnipsaPath64 {
  /** Borrowed point array. */
  const KnipsaPoint64 *points;
  /** Number of points in `points`. */
  size_t point_count;
} KnipsaPath64;

/** A borrowed floating-point path.
 *
 * `points` may be NULL only when `point_count` is zero. The caller keeps
 * ownership of the pointed-to memory for the whole API call.
 */
typedef struct KnipsaPathD {
  /** Borrowed point array. */
  const KnipsaPointD *points;
  /** Number of points in `points`. */
  size_t point_count;
} KnipsaPathD;

/** An owned integer result returned by knipsa.
 *
 * Initialize with `KNIPSA_PATHS64_INIT` before the first operation. Release
 * with `knipsa_free_paths64`; that function clears the descriptor and is
 * idempotent for an initialized result. Treat the returned descriptors and
 * points as read-only.
 */
typedef struct KnipsaPaths64 {
  /** Library-owned path descriptors. */
  KnipsaPath64 *paths;
  /** Number of descriptors in `paths`. */
  size_t path_count;
} KnipsaPaths64;

/** An owned floating-point result returned by knipsa.
 *
 * Initialize with `KNIPSA_PATHS_D_INIT` before the first operation. Release
 * with `knipsa_free_paths_d`; that function clears the descriptor and is
 * idempotent for an initialized result. Treat the returned descriptors and
 * points as read-only.
 */
typedef struct KnipsaPathsD {
  /** Library-owned path descriptors. */
  KnipsaPathD *paths;
  /** Number of descriptors in `paths`. */
  size_t path_count;
} KnipsaPathsD;

/** Options shared by polygon and polyline offset operations.
 *
 * Use `KNIPSA_OFFSET_OPTIONS_INIT` as a starting point. The enum-valued
 * fields are stored as `uint8_t` deliberately so invalid foreign values can
 * be rejected with `KNIPSA_STATUS_INVALID_ARGUMENT` instead of becoming an
 * invalid Rust enum.
 */
typedef struct KnipsaOffsetOptions {
  /** `KNIPSA_JOIN_*` value. */
  uint8_t join_type;
  /** `KNIPSA_END_*` value. */
  uint8_t end_type;
  /** Zero to clean collinear vertices; non-zero to preserve them. */
  uint8_t preserve_collinear;
  /** Reserved; must be zero. */
  uint8_t reserved;
  /** Maximum miter length divided by the absolute offset distance. */
  double miter_limit;
  /** Maximum round-join deviation; zero selects the default. */
  double arc_tolerance;
} KnipsaOffsetOptions;

/** Initial value for an empty integer result slot. */
#define KNIPSA_PATHS64_INIT { NULL, 0 }

/** Initial value for an empty floating-point result slot. */
#define KNIPSA_PATHS_D_INIT { NULL, 0 }

/** Corner style used by offset operations. */
typedef enum KnipsaJoinType {
  /** Square outer corner. */
  KNIPSA_JOIN_SQUARE = 0,
  /** Straight bevel between offset edges. */
  KNIPSA_JOIN_BEVEL = 1,
  /** Circular outer corner. */
  KNIPSA_JOIN_ROUND = 2,
  /** Intersected offset edges, limited by the miter limit. */
  KNIPSA_JOIN_MITER = 3,
} KnipsaJoinType;

/** Endpoint style used by offset operations. */
typedef enum KnipsaEndType {
  /** Treat paths as closed polygons. */
  KNIPSA_END_POLYGON = 0,
  /** Join the sides of an open path. */
  KNIPSA_END_JOINED = 1,
  /** Butt cap with no extension. */
  KNIPSA_END_BUTT = 2,
  /** Square cap extended by one offset radius. */
  KNIPSA_END_SQUARE = 3,
  /** Semicircular endpoint cap. */
  KNIPSA_END_ROUND = 4,
} KnipsaEndType;

/** Initial value for round-join, closed-polygon offsets. */
#define KNIPSA_OFFSET_OPTIONS_INIT \
  { KNIPSA_JOIN_ROUND, KNIPSA_END_POLYGON, 0, 0, 2.0, 0.0 }

/** Shape contract used by path validation. */
typedef enum KnipsaPathKind {
  /** The final point connects back to the first. */
  KNIPSA_PATH_CLOSED = 0,
  /** The final point is not connected to the first. */
  KNIPSA_PATH_OPEN = 1,
} KnipsaPathKind;

/** Status returned by an operation. Values are stable ABI values. */
typedef enum KnipsaStatus {
  /** The operation succeeded. */
  KNIPSA_STATUS_OK = 0,
  /** A required pointer was NULL. */
  KNIPSA_STATUS_NULL_POINTER = 1,
  /** A path has an invalid shape. */
  KNIPSA_STATUS_INVALID_PATH = 2,
  /** A checked integer computation overflowed. */
  KNIPSA_STATUS_ARITHMETIC_OVERFLOW = 3,
  /** An exact integer result contains a fractional coordinate. */
  KNIPSA_STATUS_NON_INTEGRAL_RESULT = 4,
  /** A polygon arrangement could not be closed. */
  KNIPSA_STATUS_TOPOLOGY_FAILURE = 5,
  /** An operation, enum value, or output slot is invalid. */
  KNIPSA_STATUS_INVALID_ARGUMENT = 6,
  /** A panic was contained at the ABI boundary. */
  KNIPSA_STATUS_INTERNAL_ERROR = 7,
  /** Input rings intersect where disjoint rings are required. */
  KNIPSA_STATUS_INTERSECTING_PATHS = 8,
  /** A double coordinate is NaN or infinite. */
  KNIPSA_STATUS_NON_FINITE_COORDINATE = 9,
  /** Offset parameters are not geometrically meaningful. */
  KNIPSA_STATUS_INVALID_OFFSET = 10,
  /** Triangulation could not produce valid triangles. */
  KNIPSA_STATUS_TRIANGULATION_FAILURE = 11,
} KnipsaStatus;

/** Boolean operation applied to subject and clip paths. */
typedef enum KnipsaClipType {
  /** Keep the region present in both inputs. */
  KNIPSA_CLIP_INTERSECTION = 1,
  /** Keep the region present in either input. */
  KNIPSA_CLIP_UNION = 2,
  /** Keep subject regions outside clip regions. */
  KNIPSA_CLIP_DIFFERENCE = 3,
  /** Keep regions present in exactly one input. */
  KNIPSA_CLIP_XOR = 4,
} KnipsaClipType;

/** Winding rule used to interpret nested input rings. */
typedef enum KnipsaFillRule {
  /** Alternate filled state at every crossing. */
  KNIPSA_FILL_EVEN_ODD = 0,
  /** Fill where the winding number is non-zero. */
  KNIPSA_FILL_NON_ZERO = 1,
  /** Fill only positively wound regions. */
  KNIPSA_FILL_POSITIVE = 2,
  /** Fill only negatively wound regions. */
  KNIPSA_FILL_NEGATIVE = 3,
} KnipsaFillRule;

/** Result of a point-in-polygon query. */
typedef enum KnipsaLocation {
  /** The point is outside the filled path. */
  KNIPSA_LOCATION_OUTSIDE = 0,
  /** The point is inside the filled path. */
  KNIPSA_LOCATION_INSIDE = 1,
  /** The point lies on the path boundary. */
  KNIPSA_LOCATION_BOUNDARY = 2,
} KnipsaLocation;

/** Returns a static, NUL-terminated library version string.
 *
 * The returned pointer remains valid for the lifetime of the library and must
 * not be freed.
 */
const char *knipsa_version(void);

/** Returns a static message for a status value.
 *
 * Unknown numeric values return `"unknown status"`. The returned pointer
 * remains valid for the lifetime of the library and must not be freed.
 */
const char *knipsa_status_message(uint8_t status);

/** Validates borrowed integer paths.
 *
 * Empty paths are valid. A closed non-empty path needs at least three points;
 * an open non-empty path needs at least two. Pass one of the
 * `KNIPSA_PATH_*` values as `kind`.
 *
 * @param paths Borrowed path descriptors, or NULL when `path_count` is zero.
 * @param path_count Number of descriptors in `paths`.
 * @param kind `KNIPSA_PATH_CLOSED` or `KNIPSA_PATH_OPEN`.
 * @return A status code; no output allocation occurs.
 */
KnipsaStatus knipsa_validate_paths64(const KnipsaPath64 *paths,
                                     size_t path_count,
                                     uint8_t kind);

/** Validates borrowed floating-point paths, including coordinate finiteness.
 *
 * The pointer, count, and `kind` rules are the same as for
 * `knipsa_validate_paths64`.
 */
KnipsaStatus knipsa_validate_paths_d(const KnipsaPathD *paths,
                                     size_t path_count,
                                     uint8_t kind);

/** Classifies one point against a closed integer path using even-odd fill.
 *
 * `location` must point to writable storage. A NULL path pointer is valid when
 * `path.point_count` is zero.
 */
KnipsaStatus knipsa_point_in_polygon64(KnipsaPath64 path,
                                       KnipsaPoint64 point,
                                       KnipsaLocation *location);

/** Executes an exact integer boolean operation.
 *
 * `clip_type` and `fill_rule` accept the corresponding enum constants. The
 * result slot must be initialized with `KNIPSA_PATHS64_INIT` or have been
 * cleared by `knipsa_free_paths64`. On success, release the result with that
 * matching function. Inputs remain owned by the caller.
 */
KnipsaStatus knipsa_boolean64(const KnipsaPath64 *subjects,
                              size_t subject_count,
                              const KnipsaPath64 *clips,
                              size_t clip_count,
                              uint8_t clip_type,
                              uint8_t fill_rule,
                              KnipsaPaths64 *result);

/** Executes a floating-point boolean operation over finite double paths.
 *
 * The result slot must be initialized with `KNIPSA_PATHS_D_INIT` or have been
 * cleared by `knipsa_free_paths_d`. On success, release it with that matching
 * function. Inputs remain owned by the caller.
 */
KnipsaStatus knipsa_boolean_d(const KnipsaPathD *subjects,
                              size_t subject_count,
                              const KnipsaPathD *clips,
                              size_t clip_count,
                              uint8_t clip_type,
                              uint8_t fill_rule,
                              KnipsaPathsD *result);

/** Simplifies integer paths with a union using the selected fill rule. */
KnipsaStatus knipsa_simplify64(const KnipsaPath64 *paths,
                               size_t path_count,
                               uint8_t fill_rule,
                               KnipsaPaths64 *result);

/** Simplifies finite floating-point paths with a union using the selected
 * fill rule. */
KnipsaStatus knipsa_simplify_d(const KnipsaPathD *paths,
                               size_t path_count,
                               uint8_t fill_rule,
                               KnipsaPathsD *result);

/** Clips integer paths to an axis-aligned rectangle. Bounds are normalized if
 * the caller supplies opposite corners in reverse order. */
KnipsaStatus knipsa_clip_to_rect64(const KnipsaPath64 *paths,
                                   size_t path_count,
                                   KnipsaRect64 rectangle,
                                   uint8_t fill_rule,
                                   KnipsaPaths64 *result);

/** Clips finite floating-point paths to an axis-aligned rectangle. Bounds are
 * normalized if the caller supplies opposite corners in reverse order. */
KnipsaStatus knipsa_clip_to_rect_d(const KnipsaPathD *paths,
                                   size_t path_count,
                                   KnipsaRectD rectangle,
                                   uint8_t fill_rule,
                                   KnipsaPathsD *result);

/** Offsets integer paths and returns an unrounded floating-point outline.
 *
 * Integer coordinates must fit exactly in a double. `delta` is the signed
 * offset for closed polygons and the absolute half-width for open polylines.
 * The options descriptor is normally initialized with
 * `KNIPSA_OFFSET_OPTIONS_INIT`.
 */
KnipsaStatus knipsa_offset64(const KnipsaPath64 *paths,
                             size_t path_count,
                             double delta,
                             const KnipsaOffsetOptions *options,
                             KnipsaPathsD *result);

/** Offsets floating-point polygon or polyline paths.
 *
 * The result slot must be initialized with `KNIPSA_PATHS_D_INIT` or have been
 * cleared by `knipsa_free_paths_d`.
 */
KnipsaStatus knipsa_offset_d(const KnipsaPathD *paths,
                             size_t path_count,
                             double delta,
                             const KnipsaOffsetOptions *options,
                             KnipsaPathsD *result);

/** Triangulates integer-coordinate rings and returns one three-point path per
 * counter-clockwise triangle. Before topology validation, the function rejects
 * more than 1024 paths, 1000000 vertices, or 4000000 conservative edge pairs
 * with KNIPSA_STATUS_INVALID_ARGUMENT.
 */
KnipsaStatus knipsa_triangulate64(const KnipsaPath64 *paths,
                                  size_t path_count,
                                  uint8_t fill_rule,
                                  KnipsaPaths64 *result);

/** Triangulates finite floating-point rings and returns one three-point path
 * per counter-clockwise triangle. Before topology validation, the function
 * rejects more than 1024 paths, 1000000 vertices, or 4000000 conservative edge
 * pairs with KNIPSA_STATUS_INVALID_ARGUMENT.
 */
KnipsaStatus knipsa_triangulate_d(const KnipsaPathD *paths,
                                  size_t path_count,
                                  uint8_t fill_rule,
                                  KnipsaPathsD *result);

/** Releases a result returned through the integer-coordinate API.
 *
 * Safe for NULL and for an already-cleared result descriptor. Only pass a
 * descriptor initialized by `KNIPSA_PATHS64_INIT` or returned by knipsa.
 */
void knipsa_free_paths64(KnipsaPaths64 *result);

/** Releases a result returned through the floating-point API.
 *
 * Safe for NULL and for an already-cleared result descriptor. Only pass a
 * descriptor initialized by `KNIPSA_PATHS_D_INIT` or returned by knipsa.
 */
void knipsa_free_paths_d(KnipsaPathsD *result);

#ifdef __cplusplus
}
#endif

#endif /* KNIPSA_H */
