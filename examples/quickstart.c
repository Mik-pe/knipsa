#include <stdio.h>

#include "knipsa.h"

static int report_failure(KnipsaStatus status) {
  fprintf(stderr, "knipsa failed: %s\n", knipsa_status_message(status));
  return 1;
}

int main(void) {
  const KnipsaPoint64 points[] = {
      {0, 0}, {10, 0}, {10, 10}, {0, 10},
  };
  const KnipsaPath64 path = {points, 4};

  KnipsaStatus status =
      knipsa_validate_paths64(&path, 1, KNIPSA_PATH_CLOSED);
  if (status != KNIPSA_STATUS_OK) {
    return report_failure(status);
  }

  KnipsaLocation location = KNIPSA_LOCATION_OUTSIDE;
  status = knipsa_point_in_polygon64(
      path, (KnipsaPoint64){5, 5}, &location);
  if (status != KNIPSA_STATUS_OK) {
    return report_failure(status);
  }
  printf("point location: %d\n", (int)location);

  KnipsaPaths64 union_result = KNIPSA_PATHS64_INIT;
  status = knipsa_boolean64(
      &path, 1, NULL, 0, KNIPSA_CLIP_UNION, KNIPSA_FILL_EVEN_ODD,
      &union_result);
  if (status != KNIPSA_STATUS_OK) {
    return report_failure(status);
  }
  printf("union rings: %zu\n", union_result.path_count);
  knipsa_free_paths64(&union_result);

  KnipsaPathsD offset = KNIPSA_PATHS_D_INIT;
  KnipsaOffsetOptions offset_options = KNIPSA_OFFSET_OPTIONS_INIT;
  offset_options.join_type = KNIPSA_JOIN_ROUND;
  status = knipsa_offset64(
      &path, 1, 1.0, &offset_options, &offset);
  if (status != KNIPSA_STATUS_OK) {
    return report_failure(status);
  }
  printf("offset rings: %zu\n", offset.path_count);
  knipsa_free_paths_d(&offset);

  KnipsaPaths64 triangles = KNIPSA_PATHS64_INIT;
  status = knipsa_triangulate64(
      &path, 1, KNIPSA_FILL_NON_ZERO, &triangles);
  if (status != KNIPSA_STATUS_OK) {
    return report_failure(status);
  }
  printf("triangles: %zu\n", triangles.path_count);
  knipsa_free_paths64(&triangles);

  return 0;
}
