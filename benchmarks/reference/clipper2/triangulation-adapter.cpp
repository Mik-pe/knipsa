#include <clipper2/clipper.h>

#include <algorithm>
#include <chrono>
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

Paths64 read_paths(std::istream& input, std::size_t count) {
  Paths64 paths;
  paths.reserve(count);
  for (std::size_t i = 0; i < count; ++i) {
    std::size_t point_count;
    input >> point_count;
    Path64 path;
    path.reserve(point_count);
    for (std::size_t j = 0; j < point_count; ++j) {
      int64_t x, y;
      input >> x >> y;
      path.emplace_back(x, y);
    }
    paths.emplace_back(std::move(path));
  }
  return paths;
}

std::string signature(const Paths64& triangles) {
  std::ostringstream output;
  output << '[';
  for (std::size_t i = 0; i < triangles.size(); ++i) {
    if (i) output << ',';
    output << '[';
    for (std::size_t j = 0; j < triangles[i].size(); ++j) {
      if (j) output << ',';
      output << '[' << triangles[i][j].x << ',' << triangles[i][j].y << ']';
    }
    output << ']';
  }
  return output.str() + ']';
}

const char* result_name(TriangulateResult result) {
  switch (result) {
    case TriangulateResult::success: return "success";
    case TriangulateResult::fail: return "triangulation failed";
    case TriangulateResult::no_polygons: return "no polygons";
    case TriangulateResult::paths_intersect: return "paths intersect";
  }
  return "unknown triangulation result";
}

Paths64 triangulate(const Paths64& paths) {
  Paths64 triangles;
  const TriangulateResult result = Triangulate(paths, triangles, true);
  if (result != TriangulateResult::success) throw std::runtime_error(result_name(result));
  return triangles;
}
}  // namespace

int main() {
  std::cout << "{\"implementation\":\"clipper2-triangulate64\",\"revision\":\"f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd\",\"samples\":25,\"warmups\":3,\"minimum_sample_time_ns\":"
            << kMinimumSampleTimeNs << "}\n";
  std::string id;
  std::size_t path_count;
  while (std::cin >> id >> path_count) {
    try {
      const Paths64 paths = read_paths(std::cin, path_count);
      Paths64 triangles;
      for (std::size_t i = 0; i < kWarmups; ++i) triangles = triangulate(paths);
      std::size_t iterations_per_sample = 1;
      while (true) {
        const auto started = std::chrono::steady_clock::now();
        for (std::size_t i = 0; i < iterations_per_sample; ++i) triangles = triangulate(paths);
        const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - started).count();
        if (elapsed >= kMinimumSampleTimeNs ||
            iterations_per_sample == kMaximumIterationsPerSample) break;
        iterations_per_sample = std::min(iterations_per_sample * 2,
                                         kMaximumIterationsPerSample);
      }
      std::vector<long long> timings;
      timings.reserve(kSamples);
      for (std::size_t sample = 0; sample < kSamples; ++sample) {
        const auto started = std::chrono::steady_clock::now();
        for (std::size_t i = 0; i < iterations_per_sample; ++i) triangles = triangulate(paths);
        timings.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(
                              std::chrono::steady_clock::now() - started).count() /
                          static_cast<long long>(iterations_per_sample));
      }
      std::sort(timings.begin(), timings.end());
      std::cout << "{\"id\":\"" << id << "\",\"status\":\"ok\",\"error\":null,\"median_ns\":"
                << timings[kSamples / 2] << ",\"p95_ns\":"
                << timings[(kSamples * 95 + 99) / 100 - 1]
                << ",\"iterations_per_sample\":" << iterations_per_sample
                << ",\"triangle_count\":" << triangles.size()
                << ",\"signature\":\"" << signature(triangles) << "\"}\n";
    } catch (const std::exception& error) {
      std::cout << "{\"id\":\"" << id << "\",\"status\":\"error\",\"error\":\""
                << error.what() << "\",\"median_ns\":0,\"p95_ns\":0,"
                << "\"iterations_per_sample\":0,\"triangle_count\":0,\"signature\":\"[]\"}\n";
    }
  }
}
