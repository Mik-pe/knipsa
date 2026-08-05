#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "knipsa.h"

int main(void) {
  const KnipsaPoint64 triangle[] = {{0, 0}, {10, 0}, {0, 10}};
  const KnipsaPath64 path = {triangle, 3};
  KnipsaLocation location = KNIPSA_LOCATION_OUTSIDE;

  assert(strcmp(knipsa_version(), "0.0.0") == 0);
  assert(knipsa_validate_paths64(&path, 1, KNIPSA_PATH_CLOSED) == KNIPSA_STATUS_OK);
  assert(knipsa_point_in_polygon64(path, (KnipsaPoint64){1, 1}, &location) ==
         KNIPSA_STATUS_OK);
  assert(location == KNIPSA_LOCATION_INSIDE);

  KnipsaPaths64 result = {0};
  assert(knipsa_boolean64(&path, 1, NULL, 0, KNIPSA_CLIP_UNION,
                         KNIPSA_FILL_NON_ZERO, &result) == KNIPSA_STATUS_OK);
  assert(result.path_count == 1);
  assert(result.paths[0].point_count == 3);
  knipsa_free_paths64(&result);
  assert(result.path_count == 0);

  const KnipsaPointD triangle_d[] = {{0.0, 0.0}, {10.0, 0.0}, {0.0, 10.0}};
  const KnipsaPathD path_d = {triangle_d, 3};
  KnipsaPathsD result_d = {0};
  assert(knipsa_boolean_d(&path_d, 1, NULL, 0, KNIPSA_CLIP_UNION,
                          KNIPSA_FILL_EVEN_ODD, &result_d) == KNIPSA_STATUS_OK);
  assert(result_d.path_count == 1);
  assert(result_d.paths[0].point_count == 3);
  knipsa_free_paths_d(&result_d);
  assert(result_d.path_count == 0);
  return 0;
}
