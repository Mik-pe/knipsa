#include "boolean_adapter.hpp"

#include <clipper2/clipper.h>

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <utility>

namespace {

namespace bench = knipsa_bench;

template <typename Coordinate>
struct AdapterTraits;

template <>
struct AdapterTraits<double> {
  using ClipperPoint = Clipper2Lib::PointD;
  using ClipperPath = Clipper2Lib::PathD;
  using ClipperPaths = Clipper2Lib::PathsD;

  static ClipperPaths boolean_op(Clipper2Lib::ClipType operation,
                                 Clipper2Lib::FillRule rule,
                                 const ClipperPaths& subjects,
                                 const ClipperPaths& clips) {
    return Clipper2Lib::BooleanOp(operation, rule, subjects, clips, 8);
  }
};

template <>
struct AdapterTraits<std::int64_t> {
  using ClipperPoint = Clipper2Lib::Point64;
  using ClipperPath = Clipper2Lib::Path64;
  using ClipperPaths = Clipper2Lib::Paths64;

  static ClipperPaths boolean_op(Clipper2Lib::ClipType operation,
                                 Clipper2Lib::FillRule rule,
                                 const ClipperPaths& subjects,
                                 const ClipperPaths& clips) {
    return Clipper2Lib::BooleanOp(operation, rule, subjects, clips);
  }
};

template <typename Coordinate>
typename AdapterTraits<Coordinate>::ClipperPaths to_clipper(
    const bench::BasicPaths<Coordinate>& paths) {
  using Traits = AdapterTraits<Coordinate>;
  typename Traits::ClipperPaths result;
  result.reserve(paths.size());
  for (const bench::BasicPath<Coordinate>& path : paths) {
    typename Traits::ClipperPath converted;
    converted.reserve(path.size());
    for (const bench::BasicPoint<Coordinate> point : path) {
      converted.emplace_back(point.x, point.y);
    }
    result.push_back(std::move(converted));
  }
  return result;
}

template <typename Coordinate>
bench::BasicPaths<Coordinate> from_clipper(
    const typename AdapterTraits<Coordinate>::ClipperPaths& paths) {
  using Traits = AdapterTraits<Coordinate>;
  bench::BasicPaths<Coordinate> result;
  result.reserve(paths.size());
  for (const typename Traits::ClipperPath& path : paths) {
    bench::BasicPath<Coordinate> converted;
    converted.reserve(path.size());
    for (const typename Traits::ClipperPoint point : path) {
      converted.push_back({point.x, point.y});
    }
    result.push_back(std::move(converted));
  }
  return result;
}

Clipper2Lib::ClipType clip_type(bench::ClipOperation operation) {
  switch (operation) {
    case bench::ClipOperation::kIntersection: return Clipper2Lib::ClipType::Intersection;
    case bench::ClipOperation::kUnion: return Clipper2Lib::ClipType::Union;
    case bench::ClipOperation::kDifference: return Clipper2Lib::ClipType::Difference;
    case bench::ClipOperation::kXor: return Clipper2Lib::ClipType::Xor;
  }
  throw std::runtime_error("invalid clip operation");
}

Clipper2Lib::FillRule fill_rule(bench::FillRule rule) {
  switch (rule) {
    case bench::FillRule::kEvenOdd: return Clipper2Lib::FillRule::EvenOdd;
    case bench::FillRule::kNonZero: return Clipper2Lib::FillRule::NonZero;
    case bench::FillRule::kPositive: return Clipper2Lib::FillRule::Positive;
    case bench::FillRule::kNegative: return Clipper2Lib::FillRule::Negative;
  }
  throw std::runtime_error("invalid fill rule");
}

}  // namespace

template <typename Coordinate>
int run(const std::string& implementation) {
  using Traits = AdapterTraits<Coordinate>;
  bench::print_header(implementation, "f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd");
  while (true) {
    bench::BasicWorkloadCase<Coordinate> test_case;
    try {
      if (!bench::read_case(std::cin, test_case)) break;
      const auto subjects = to_clipper(test_case.subjects);
      const auto clips = to_clipper(test_case.clips);
      const auto operation = clip_type(test_case.operation);
      const auto rule = fill_rule(test_case.fill_rule);
      const auto timed = bench::benchmark([&] {
        return Traits::boolean_op(operation, rule, subjects, clips);
      });
      bench::print_success(test_case.id, timed,
                           from_clipper<Coordinate>(timed.output));
    } catch (const std::exception& error) {
      bench::print_error(test_case.id, error.what());
    }
  }
  return 0;
}

int main() {
  const char* coordinate_type = std::getenv("KNIPSA_COORDINATE_TYPE");
  if (coordinate_type == nullptr || std::string(coordinate_type) == "f64") {
    return run<double>("clipper2-native");
  }
  if (std::string(coordinate_type) == "i64") {
    return run<std::int64_t>("clipper2-native-i64");
  }
  throw std::runtime_error("KNIPSA_COORDINATE_TYPE must be f64 or i64");
}
