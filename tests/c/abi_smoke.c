#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "knipsa.h"

int main(void) {
  const KnipsaPoint64 triangle[] = {{0, 0}, {10, 0}, {0, 10}};
  const KnipsaPath64 path = {triangle, 3};
  KnipsaLocation location = KNIPSA_LOCATION_OUTSIDE;

  assert(KNIPSA_VERSION_MAJOR == 0);
  assert(KNIPSA_VERSION_MINOR == 2);
  assert(KNIPSA_VERSION_PATCH == 1);
  assert(strcmp(knipsa_version(), "0.2.1") == 0);
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

  KnipsaPaths64 simplified = KNIPSA_PATHS64_INIT;
  assert(knipsa_simplify64(&path, 1, KNIPSA_FILL_EVEN_ODD, &simplified) ==
         KNIPSA_STATUS_OK);
  assert(simplified.path_count == 1);
  knipsa_free_paths64(&simplified);

  KnipsaPaths64 clipped = KNIPSA_PATHS64_INIT;
  assert(knipsa_clip_to_rect64(&path, 1, (KnipsaRect64){0, 0, 5, 5},
                               KNIPSA_FILL_EVEN_ODD, &clipped) == KNIPSA_STATUS_OK);
  assert(clipped.path_count == 1);
  knipsa_free_paths64(&clipped);

  const KnipsaPointD triangle_d[] = {{0.0, 0.0}, {10.0, 0.0}, {0.0, 10.0}};
  const KnipsaPathD path_d = {triangle_d, 3};
  assert(knipsa_validate_paths_d(&path_d, 1, KNIPSA_PATH_CLOSED) ==
         KNIPSA_STATUS_OK);
  KnipsaPathsD result_d = {0};
  assert(knipsa_boolean_d(&path_d, 1, NULL, 0, KNIPSA_CLIP_UNION,
                          KNIPSA_FILL_EVEN_ODD, &result_d) == KNIPSA_STATUS_OK);
  assert(result_d.path_count == 1);
  assert(result_d.paths[0].point_count == 3);
  knipsa_free_paths_d(&result_d);
  assert(result_d.path_count == 0);

  KnipsaPathsD simplified_d = KNIPSA_PATHS_D_INIT;
  assert(knipsa_simplify_d(&path_d, 1, KNIPSA_FILL_EVEN_ODD, &simplified_d) ==
         KNIPSA_STATUS_OK);
  assert(simplified_d.path_count == 1);
  knipsa_free_paths_d(&simplified_d);

  KnipsaPathsD clipped_d = KNIPSA_PATHS_D_INIT;
  assert(knipsa_clip_to_rect_d(&path_d, 1, (KnipsaRectD){0.0, 0.0, 5.0, 5.0},
                               KNIPSA_FILL_EVEN_ODD, &clipped_d) == KNIPSA_STATUS_OK);
  assert(clipped_d.path_count == 1);
  knipsa_free_paths_d(&clipped_d);

  KnipsaPathsD offset = {0};
  KnipsaOffsetOptions offset_options = KNIPSA_OFFSET_OPTIONS_INIT;
  offset_options.join_type = KNIPSA_JOIN_MITER;
  assert(knipsa_offset64(&path, 1, 1.0, &offset_options, &offset) ==
         KNIPSA_STATUS_OK);
  assert(offset.path_count == 1);
  knipsa_free_paths_d(&offset);

  KnipsaPaths64 triangles = {0};
  assert(knipsa_triangulate64(&path, 1, KNIPSA_FILL_NON_ZERO, &triangles) ==
         KNIPSA_STATUS_OK);
  assert(triangles.path_count == 1);
  assert(triangles.paths[0].point_count == 3);
  knipsa_free_paths64(&triangles);

  return 0;
}
