#ifndef KNIPSA_BENCHMARK_BOOLEAN_ADAPTER_HPP
#define KNIPSA_BENCHMARK_BOOLEAN_ADAPTER_HPP

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

namespace knipsa_bench {

constexpr std::size_t kWarmups = 3;
constexpr std::size_t kSamples = 25;
constexpr long long kMinimumSampleTimeNs = 2'000'000;
constexpr std::size_t kMaximumIterationsPerSample = 1 << 20;

template <typename Coordinate>
struct BasicPoint {
  Coordinate x;
  Coordinate y;
};

template <typename Coordinate>
using BasicPath = std::vector<BasicPoint<Coordinate>>;

template <typename Coordinate>
using BasicPaths = std::vector<BasicPath<Coordinate>>;

using Point = BasicPoint<double>;
using Path = BasicPath<double>;
using Paths = BasicPaths<double>;
using Point64 = BasicPoint<std::int64_t>;
using Path64 = BasicPath<std::int64_t>;
using Paths64 = BasicPaths<std::int64_t>;

enum class ClipOperation { kIntersection, kUnion, kDifference, kXor };
enum class FillRule { kEvenOdd, kNonZero, kPositive, kNegative };

inline ClipOperation clip_operation(int value) {
  switch (value) {
    case 0: return ClipOperation::kIntersection;
    case 1: return ClipOperation::kUnion;
    case 2: return ClipOperation::kDifference;
    case 3: return ClipOperation::kXor;
    default: throw std::runtime_error("invalid clip type");
  }
}

inline FillRule fill_rule(int value) {
  switch (value) {
    case 0: return FillRule::kEvenOdd;
    case 1: return FillRule::kNonZero;
    case 2: return FillRule::kPositive;
    case 3: return FillRule::kNegative;
    default: throw std::runtime_error("invalid fill rule");
  }
}

template <typename Coordinate>
BasicPaths<Coordinate> read_paths(std::istream& input, std::size_t count) {
  BasicPaths<Coordinate> paths;
  paths.reserve(count);
  for (std::size_t path_index = 0; path_index < count; ++path_index) {
    std::size_t point_count;
    if (!(input >> point_count)) throw std::runtime_error("missing point count");
    BasicPath<Coordinate> path;
    path.reserve(point_count);
    for (std::size_t point_index = 0; point_index < point_count; ++point_index) {
      Coordinate x;
      Coordinate y;
      if (!(input >> x >> y)) throw std::runtime_error("missing point coordinate");
      if constexpr (std::is_floating_point_v<Coordinate>) {
        if (!std::isfinite(x) || !std::isfinite(y)) {
          throw std::runtime_error("non-finite point coordinate");
        }
      }
      path.push_back({x, y});
    }
    paths.push_back(std::move(path));
  }
  return paths;
}

template <typename Coordinate>
struct BasicWorkloadCase {
  std::string id;
  ClipOperation operation;
  FillRule fill_rule;
  BasicPaths<Coordinate> subjects;
  BasicPaths<Coordinate> clips;
};

using WorkloadCase = BasicWorkloadCase<double>;
using WorkloadCase64 = BasicWorkloadCase<std::int64_t>;

template <typename Coordinate>
bool read_case(std::istream& input, BasicWorkloadCase<Coordinate>& test_case) {
  if (!(input >> test_case.id)) return false;
  int operation_value;
  int fill_rule_value;
  std::size_t subject_count;
  if (!(input >> operation_value >> fill_rule_value >> subject_count)) {
    throw std::runtime_error("incomplete workload case header");
  }
  test_case.subjects = read_paths<Coordinate>(input, subject_count);
  std::size_t clip_count;
  if (!(input >> clip_count)) throw std::runtime_error("missing clip path count");
  test_case.clips = read_paths<Coordinate>(input, clip_count);
  // Decode enums only after consuming the complete record so rejected
  // semantics cannot desynchronize the following framed case.
  test_case.operation = clip_operation(operation_value);
  test_case.fill_rule = fill_rule(fill_rule_value);
  return true;
}

inline double area2(const Path& path) {
  double result = 0.0;
  for (std::size_t index = 0; index < path.size(); ++index) {
    const Point& first = path[index];
    const Point& second = path[(index + 1) % path.size()];
    result += first.x * second.y - first.y * second.x;
  }
  return result;
}

template <typename Coordinate>
std::string raw_signature(const BasicPaths<Coordinate>& paths) {
  std::ostringstream output;
  output << std::setprecision(17) << '[';
  for (std::size_t index = 0; index < paths.size(); ++index) {
    if (index != 0) output << ',';
    output << '[';
    for (std::size_t point_index = 0; point_index < paths[index].size(); ++point_index) {
      if (point_index != 0) output << ',';
      const BasicPoint<Coordinate>& point = paths[index][point_index];
      output << '[' << point.x << ',' << point.y << ']';
    }
    output << ']';
  }
  output << ']';
  return output.str();
}

inline std::string json_escape(const std::string& value) {
  std::ostringstream output;
  for (const unsigned char character : value) {
    switch (character) {
      case '"': output << "\\\""; break;
      case '\\': output << "\\\\"; break;
      case '\b': output << "\\b"; break;
      case '\f': output << "\\f"; break;
      case '\n': output << "\\n"; break;
      case '\r': output << "\\r"; break;
      case '\t': output << "\\t"; break;
      default:
        if (character < 0x20) {
          output << "\\u" << std::hex << std::setw(4) << std::setfill('0')
                 << static_cast<unsigned int>(character) << std::dec;
        } else {
          output << static_cast<char>(character);
        }
    }
  }
  return output.str();
}

inline void print_header(const std::string& implementation, const std::string& revision) {
  std::cout << "{\"implementation\":\"" << json_escape(implementation)
            << "\",\"revision\":\"" << json_escape(revision)
            << "\",\"samples\":" << kSamples << ",\"warmups\":" << kWarmups
            << ",\"minimum_sample_time_ns\":" << kMinimumSampleTimeNs << "}\n";
}

inline void print_error(const std::string& id, const std::string& message) {
  std::cout << "{\"id\":\"" << json_escape(id)
            << "\",\"status\":\"error\",\"error\":\"" << json_escape(message)
            << "\",\"median_ns\":0,\"p95_ns\":0,\"iterations_per_sample\":0,"
               "\"ring_count\":0,\"signature\":\"[]\"}\n";
}

template <typename Output>
struct TimedOutput {
  Output output;
  long long median_ns;
  long long p95_ns;
  std::size_t iterations_per_sample;
};

template <typename Run>
auto benchmark(Run&& run) -> TimedOutput<std::invoke_result_t<Run&>> {
  using Output = std::invoke_result_t<Run&>;
  Output output{};
  for (std::size_t run_index = 0; run_index < kWarmups; ++run_index) output = run();

  std::size_t iterations_per_sample = 1;
  while (true) {
    const auto started = std::chrono::steady_clock::now();
    for (std::size_t iteration = 0; iteration < iterations_per_sample; ++iteration) {
      output = run();
    }
    const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::steady_clock::now() - started).count();
    if (elapsed >= kMinimumSampleTimeNs ||
        iterations_per_sample == kMaximumIterationsPerSample) {
      break;
    }
    iterations_per_sample =
        std::min(iterations_per_sample * 2, kMaximumIterationsPerSample);
  }

  std::vector<long long> timings;
  timings.reserve(kSamples);
  for (std::size_t run_index = 0; run_index < kSamples; ++run_index) {
    const auto started = std::chrono::steady_clock::now();
    for (std::size_t iteration = 0; iteration < iterations_per_sample; ++iteration) {
      output = run();
    }
    const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::steady_clock::now() - started).count();
    timings.push_back(elapsed / static_cast<long long>(iterations_per_sample));
  }
  std::sort(timings.begin(), timings.end());
  const std::size_t p95_index = (kSamples * 95 + 99) / 100 - 1;
  return {std::move(output), timings[kSamples / 2], timings[p95_index], iterations_per_sample};
}

template <typename Output, typename Coordinate>
void print_success(const std::string& id, const TimedOutput<Output>& timed,
                   const BasicPaths<Coordinate>& paths) {
  std::cout << "{\"id\":\"" << json_escape(id)
            << "\",\"status\":\"ok\",\"error\":null,\"median_ns\":" << timed.median_ns
            << ",\"p95_ns\":" << timed.p95_ns
            << ",\"iterations_per_sample\":" << timed.iterations_per_sample
            << ",\"ring_count\":" << paths.size() << ",\"signature\":\""
            << json_escape(raw_signature(paths)) << "\"}\n";
}

}  // namespace knipsa_bench

#endif
