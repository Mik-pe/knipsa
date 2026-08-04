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
  return 0;
}
