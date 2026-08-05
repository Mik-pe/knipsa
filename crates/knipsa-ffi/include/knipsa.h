#ifndef KNIPSA_H
#define KNIPSA_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KnipsaPoint64 {
  int64_t x;
  int64_t y;
} KnipsaPoint64;

typedef struct KnipsaPointD {
  double x;
  double y;
} KnipsaPointD;

typedef struct KnipsaPath64 {
  const KnipsaPoint64 *points;
  size_t point_count;
} KnipsaPath64;

typedef struct KnipsaPathD {
  const KnipsaPointD *points;
  size_t point_count;
} KnipsaPathD;

typedef struct KnipsaPaths64 {
  KnipsaPath64 *paths;
  size_t path_count;
} KnipsaPaths64;

typedef struct KnipsaPathsD {
  KnipsaPathD *paths;
  size_t path_count;
} KnipsaPathsD;

typedef enum KnipsaJoinType {
  KNIPSA_JOIN_SQUARE = 0,
  KNIPSA_JOIN_BEVEL = 1,
  KNIPSA_JOIN_ROUND = 2,
  KNIPSA_JOIN_MITER = 3,
} KnipsaJoinType;

typedef enum KnipsaEndType {
  KNIPSA_END_POLYGON = 0,
  KNIPSA_END_JOINED = 1,
  KNIPSA_END_BUTT = 2,
  KNIPSA_END_SQUARE = 3,
  KNIPSA_END_ROUND = 4,
} KnipsaEndType;

typedef enum KnipsaPathKind {
  KNIPSA_PATH_CLOSED = 0,
  KNIPSA_PATH_OPEN = 1,
} KnipsaPathKind;

typedef enum KnipsaStatus {
  KNIPSA_STATUS_OK = 0,
  KNIPSA_STATUS_NULL_POINTER = 1,
  KNIPSA_STATUS_INVALID_PATH = 2,
  KNIPSA_STATUS_ARITHMETIC_OVERFLOW = 3,
  KNIPSA_STATUS_NON_INTEGRAL_RESULT = 4,
  KNIPSA_STATUS_TOPOLOGY_FAILURE = 5,
  KNIPSA_STATUS_INVALID_ARGUMENT = 6,
  KNIPSA_STATUS_INTERNAL_ERROR = 7,
  KNIPSA_STATUS_INTERSECTING_PATHS = 8,
} KnipsaStatus;

typedef enum KnipsaClipType {
  KNIPSA_CLIP_INTERSECTION = 1,
  KNIPSA_CLIP_UNION = 2,
  KNIPSA_CLIP_DIFFERENCE = 3,
  KNIPSA_CLIP_XOR = 4,
} KnipsaClipType;

typedef enum KnipsaFillRule {
  KNIPSA_FILL_EVEN_ODD = 0,
  KNIPSA_FILL_NON_ZERO = 1,
  KNIPSA_FILL_POSITIVE = 2,
  KNIPSA_FILL_NEGATIVE = 3,
} KnipsaFillRule;

typedef enum KnipsaLocation {
  KNIPSA_LOCATION_OUTSIDE = 0,
  KNIPSA_LOCATION_INSIDE = 1,
  KNIPSA_LOCATION_BOUNDARY = 2,
} KnipsaLocation;

const char *knipsa_version(void);
const char *knipsa_status_message(KnipsaStatus status);

KnipsaStatus knipsa_validate_paths64(const KnipsaPath64 *paths,
                                     size_t path_count,
                                     KnipsaPathKind kind);

KnipsaStatus knipsa_point_in_polygon64(KnipsaPath64 path,
                                       KnipsaPoint64 point,
                                       KnipsaLocation *location);

KnipsaStatus knipsa_boolean64(const KnipsaPath64 *subjects,
                              size_t subject_count,
                              const KnipsaPath64 *clips,
                              size_t clip_count,
                              uint8_t clip_type,
                              uint8_t fill_rule,
                              KnipsaPaths64 *result);

KnipsaStatus knipsa_boolean_d(const KnipsaPathD *subjects,
                              size_t subject_count,
                              const KnipsaPathD *clips,
                              size_t clip_count,
                              uint8_t clip_type,
                              uint8_t fill_rule,
                              KnipsaPathsD *result);

KnipsaStatus knipsa_offset64(const KnipsaPath64 *paths,
                             size_t path_count,
                             double delta,
                             uint8_t join_type,
                             uint8_t end_type,
                             double miter_limit,
                             double arc_tolerance,
                             uint8_t preserve_collinear,
                             KnipsaPathsD *result);

KnipsaStatus knipsa_offset_d(const KnipsaPathD *paths,
                             size_t path_count,
                             double delta,
                             uint8_t join_type,
                             uint8_t end_type,
                             double miter_limit,
                             double arc_tolerance,
                             uint8_t preserve_collinear,
                             KnipsaPathsD *result);

KnipsaStatus knipsa_triangulate64(const KnipsaPath64 *paths,
                                  size_t path_count,
                                  uint8_t fill_rule,
                                  KnipsaPaths64 *result);

KnipsaStatus knipsa_triangulate_d(const KnipsaPathD *paths,
                                  size_t path_count,
                                  uint8_t fill_rule,
                                  KnipsaPathsD *result);

void knipsa_free_paths64(KnipsaPaths64 *result);
void knipsa_free_paths_d(KnipsaPathsD *result);

#ifdef __cplusplus
}
#endif

#endif /* KNIPSA_H */
