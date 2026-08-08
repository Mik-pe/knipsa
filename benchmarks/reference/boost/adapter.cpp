#include "boolean_adapter.hpp"

#include <boost/geometry.hpp>
#include <boost/geometry/geometries/geometries.hpp>

#include <cstddef>
#include <stdexcept>
#include <string>
#include <utility>

namespace {

namespace bench = knipsa_bench;
namespace bg = boost::geometry;
using BoostPoint = bg::model::d2::point_xy<double>;
using BoostPolygon = bg::model::polygon<BoostPoint, false, false>;
using BoostMultiPolygon = bg::model::multi_polygon<BoostPolygon>;

BoostPolygon to_polygon(const bench::Path& path) {
  if (path.size() < 3) throw std::runtime_error("Boost.Geometry requires at least three points");
  BoostPolygon polygon;
  polygon.outer().reserve(path.size());
  for (const bench::Point point : path) polygon.outer().emplace_back(point.x, point.y);
  bg::correct(polygon);
  std::string reason;
  if (!bg::is_valid(polygon, reason)) {
    throw std::runtime_error("input is outside the OGC-valid profile: " + reason);
  }
  return polygon;
}

BoostMultiPolygon combine_paths(const bench::Paths& paths, bool use_parity) {
  BoostMultiPolygon region;
  for (const bench::Path& path : paths) {
    BoostMultiPolygon single;
    single.push_back(to_polygon(path));
    if (region.empty()) {
      region = std::move(single);
    } else {
      BoostMultiPolygon combined;
      if (use_parity) {
        bg::sym_difference(region, single, combined);
      } else {
        bg::union_(region, single, combined);
      }
      region = std::move(combined);
    }
  }
  return region;
}

BoostMultiPolygon to_region(const bench::Paths& paths, bench::FillRule rule) {
  if (rule == bench::FillRule::kEvenOdd) return combine_paths(paths, true);

  bool have_winding = false;
  bool positive_winding = false;
  for (const bench::Path& path : paths) {
    const double doubled_area = bench::area2(path);
    if (!std::isfinite(doubled_area) || doubled_area == 0.0) {
      throw std::runtime_error("non-EvenOdd profile requires non-zero finite ring area");
    }
    const bool positive = doubled_area > 0.0;
    if (have_winding && positive != positive_winding) {
      throw std::runtime_error("Boost.Geometry adapter rejects mixed-winding fill input");
    }
    have_winding = true;
    positive_winding = positive;
  }

  if (rule == bench::FillRule::kPositive && have_winding && !positive_winding) return {};
  if (rule == bench::FillRule::kNegative && have_winding && positive_winding) return {};
  return combine_paths(paths, false);
}

BoostMultiPolygon overlay(bench::ClipOperation operation, const BoostMultiPolygon& subjects,
                          const BoostMultiPolygon& clips) {
  BoostMultiPolygon output;
  switch (operation) {
    case bench::ClipOperation::kIntersection:
      bg::intersection(subjects, clips, output);
      break;
    case bench::ClipOperation::kUnion:
      bg::union_(subjects, clips, output);
      break;
    case bench::ClipOperation::kDifference:
      bg::difference(subjects, clips, output);
      break;
    case bench::ClipOperation::kXor:
      bg::sym_difference(subjects, clips, output);
      break;
  }
  return output;
}

template <typename Ring>
bench::Path from_ring(const Ring& ring) {
  bench::Path result;
  result.reserve(ring.size());
  for (const BoostPoint& point : ring) {
    result.push_back({bg::get<0>(point), bg::get<1>(point)});
  }
  if (result.size() > 1 && result.front().x == result.back().x &&
      result.front().y == result.back().y) {
    result.pop_back();
  }
  return result;
}

bench::Paths from_boost(const BoostMultiPolygon& polygons) {
  std::string reason;
  if (!bg::is_valid(polygons, reason)) {
    throw std::runtime_error("Boost.Geometry returned invalid output: " + reason);
  }
  bench::Paths result;
  for (const BoostPolygon& polygon : polygons) {
    result.push_back(from_ring(polygon.outer()));
    for (const auto& inner : polygon.inners()) result.push_back(from_ring(inner));
  }
  return result;
}

}  // namespace

int main() {
  bench::print_header("boost-geometry-native", "1.91.0");
  while (true) {
    bench::WorkloadCase test_case;
    try {
      if (!bench::read_case(std::cin, test_case)) break;
      if (test_case.subjects.size() > 1 || test_case.clips.size() > 1) {
        // Boost accepts already-formed polygons, not a contour collection plus
        // fill rule. Resolving multiple contours is therefore part of the
        // comparable public operation and must stay inside the timed region.
        const auto timed = bench::benchmark([&] {
          return overlay(test_case.operation,
                         to_region(test_case.subjects, test_case.fill_rule),
                         to_region(test_case.clips, test_case.fill_rule));
        });
        bench::print_success(test_case.id, timed, from_boost(timed.output));
      } else {
        const BoostMultiPolygon subjects = to_region(test_case.subjects, test_case.fill_rule);
        const BoostMultiPolygon clips = to_region(test_case.clips, test_case.fill_rule);
        const auto timed =
            bench::benchmark([&] { return overlay(test_case.operation, subjects, clips); });
        bench::print_success(test_case.id, timed, from_boost(timed.output));
      }
    } catch (const std::exception& error) {
      bench::print_error(test_case.id, error.what());
    }
  }
}
