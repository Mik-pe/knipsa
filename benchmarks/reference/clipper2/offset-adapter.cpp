#include <clipper2/clipper.h>

#include <algorithm>
#include <chrono>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using namespace Clipper2Lib;

namespace {
constexpr std::size_t kWarmups = 3;
constexpr std::size_t kSamples = 25;
constexpr long long kMinimumSampleTimeNs = 2'000'000;
constexpr std::size_t kMaximumIterationsPerSample = 1 << 20;

JoinType join_type(int value) {
  switch (value) {
    case 0: return JoinType::Square; case 1: return JoinType::Bevel;
    case 2: return JoinType::Round; case 3: return JoinType::Miter;
    default: throw std::runtime_error("invalid join type");
  }
}

EndType end_type(int value) {
  switch (value) {
    case 0: return EndType::Polygon; case 1: return EndType::Joined;
    case 2: return EndType::Butt; case 3: return EndType::Square;
    case 4: return EndType::Round;
    default: throw std::runtime_error("invalid end type");
  }
}

PathsD read_paths(std::istream& input, std::size_t count) {
  PathsD paths;
  for (std::size_t i = 0; i < count; ++i) {
    std::size_t point_count; input >> point_count;
    PathD path; path.reserve(point_count);
    for (std::size_t j = 0; j < point_count; ++j) {
      double x, y; input >> x >> y; path.emplace_back(x, y);
    }
    paths.emplace_back(std::move(path));
  }
  return paths;
}

std::string signature(const PathsD& paths) {
  std::ostringstream output; output << std::setprecision(17) << '[';
  for (std::size_t i = 0; i < paths.size(); ++i) {
    if (i) output << ','; output << '[';
    for (std::size_t j = 0; j < paths[i].size(); ++j) {
      if (j) output << ','; output << '[' << paths[i][j].x << ',' << paths[i][j].y << ']';
    }
    output << ']';
  }
  return output.str() + ']';
}

PathsD offset_paths(const PathsD& paths, double delta, JoinType join, EndType end,
                    double miter_limit, double arc_tolerance, bool preserve_collinear) {
  constexpr double scale = 1e8;
  int error_code = 0;
  Paths64 scaled = ScalePaths<int64_t, double>(paths, scale, error_code);
  if (error_code) throw std::runtime_error("coordinate scaling failed");
  ClipperOffset offset(miter_limit, arc_tolerance * scale, preserve_collinear);
  offset.AddPaths(scaled, join, end);
  Paths64 result;
  offset.Execute(delta * scale, result);
  PathsD output = ScalePaths<double, int64_t>(result, 1.0 / scale, error_code);
  if (error_code) throw std::runtime_error("result scaling failed");
  return output;
}
}

int main() {
  std::cout << "{\"implementation\":\"clipper2-native-offset\",\"revision\":\"f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd\",\"samples\":25,\"warmups\":3,\"minimum_sample_time_ns\":"
            << kMinimumSampleTimeNs << "}\n";
  std::string id; double delta, miter_limit, arc_tolerance; int join, end, preserve;
  std::size_t path_count;
  while (std::cin >> id >> delta >> join >> end >> miter_limit >> arc_tolerance >> preserve >> path_count) {
    try {
      const PathsD paths = read_paths(std::cin, path_count);
      const auto run = [&] {
        return offset_paths(paths, delta, join_type(join), end_type(end), miter_limit,
                            arc_tolerance, preserve != 0);
      };
      PathsD result;
      for (std::size_t i = 0; i < kWarmups; ++i) result = run();
      std::size_t iterations_per_sample = 1;
      while (true) {
        const auto start = std::chrono::steady_clock::now();
        for (std::size_t i = 0; i < iterations_per_sample; ++i) result = run();
        const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - start).count();
        if (elapsed >= kMinimumSampleTimeNs ||
            iterations_per_sample == kMaximumIterationsPerSample) break;
        iterations_per_sample = std::min(iterations_per_sample * 2,
                                         kMaximumIterationsPerSample);
      }
      std::vector<long long> timings; timings.reserve(kSamples);
      for (std::size_t i = 0; i < kSamples; ++i) {
        const auto start = std::chrono::steady_clock::now();
        for (std::size_t iteration = 0; iteration < iterations_per_sample; ++iteration) {
          result = run();
        }
        timings.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(
                              std::chrono::steady_clock::now() - start).count() /
                          static_cast<long long>(iterations_per_sample));
      }
      std::sort(timings.begin(), timings.end());
      std::cout << "{\"id\":\"" << id << "\",\"status\":\"ok\",\"error\":null,\"median_ns\":"
                << timings[kSamples / 2] << ",\"p95_ns\":" << timings[(kSamples * 95 + 99) / 100 - 1]
                << ",\"iterations_per_sample\":" << iterations_per_sample
                << ",\"ring_count\":" << result.size() << ",\"signature\":\"" << signature(result) << "\"}\n";
    } catch (const std::exception& error) {
      std::cout << "{\"id\":\"" << id << "\",\"status\":\"error\",\"error\":\"" << error.what()
                << "\",\"median_ns\":0,\"p95_ns\":0,\"iterations_per_sample\":0,\"ring_count\":0,\"signature\":\"[]\"}\n";
    }
  }
}
