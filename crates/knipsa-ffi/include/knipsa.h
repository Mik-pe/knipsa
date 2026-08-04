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

typedef struct KnipsaPath64 {
  const KnipsaPoint64 *points;
  size_t point_count;
} KnipsaPath64;

typedef enum KnipsaPathKind {
  KNIPSA_PATH_CLOSED = 0,
  KNIPSA_PATH_OPEN = 1,
} KnipsaPathKind;

typedef enum KnipsaStatus {
  KNIPSA_STATUS_OK = 0,
  KNIPSA_STATUS_NULL_POINTER = 1,
  KNIPSA_STATUS_INVALID_PATH = 2,
  KNIPSA_STATUS_ARITHMETIC_OVERFLOW = 3,
  KNIPSA_STATUS_KERNEL_NOT_READY = 4,
} KnipsaStatus;

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

#ifdef __cplusplus
}
#endif

#endif /* KNIPSA_H */
