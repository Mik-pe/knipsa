#include <clipper2/clipper.h>

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

using namespace Clipper2Lib;

namespace {

constexpr std::size_t kWarmups = 3;
constexpr std::size_t kSamples = 25;
constexpr long long kMinimumSampleTimeNs = 2'000'000;
constexpr std::size_t kMaximumIterationsPerSample = 1 << 20;

struct WorkloadCase {
  std::string id;
  ClipType operation;
  FillRule fill_rule;
  Paths64 closed_subjects;
  Paths64 open_subjects;
  Paths64 clips;
};

struct Output {
  Paths64 closed;
  Paths64 open;
};

struct TimedOutput {
  Output output;
  long long median_ns;
  long long p95_ns;
  std::size_t iterations_per_sample;
};

ClipType clip_type(int value) {
  switch (value) {
    case 0: return ClipType::Intersection;
    case 1: return ClipType::Union;
    case 2: return ClipType::Difference;
    case 3: return ClipType::Xor;
    default: throw std::runtime_error("invalid clip type");
  }
}

FillRule fill_rule(int value) {
  switch (value) {
    case 0: return FillRule::EvenOdd;
    case 1: return FillRule::NonZero;
    case 2: return FillRule::Positive;
    case 3: return FillRule::Negative;
    default: throw std::runtime_error("invalid fill rule");
  }
}

Paths64 read_paths(std::istream& input) {
  std::size_t path_count;
  if (!(input >> path_count)) throw std::runtime_error("missing path count");
  Paths64 paths;
  paths.reserve(path_count);
  for (std::size_t i = 0; i < path_count; ++i) {
    std::size_t point_count;
    input >> point_count;
    Path64 path;
    path.reserve(point_count);
    for (std::size_t j = 0; j < point_count; ++j) {
      std::int64_t x;
      std::int64_t y;
      input >> x >> y;
      if (!input) throw std::runtime_error("malformed path coordinates");
      path.emplace_back(x, y);
    }
    paths.push_back(std::move(path));
  }
  return paths;
}

bool read_case(std::istream& input, WorkloadCase& test_case) {
  int operation;
  int rule;
  if (!(input >> test_case.id)) return false;
  if (!(input >> operation >> rule)) throw std::runtime_error("malformed case header");
  test_case.operation = clip_type(operation);
  test_case.fill_rule = fill_rule(rule);
  test_case.closed_subjects = read_paths(input);
  test_case.open_subjects = read_paths(input);
  test_case.clips = read_paths(input);
  return true;
}

Output execute(const WorkloadCase& test_case) {
  Clipper64 clipper;
  clipper.AddSubject(test_case.closed_subjects);
  clipper.AddOpenSubject(test_case.open_subjects);
  clipper.AddClip(test_case.clips);
  Output output;
  if (!clipper.Execute(test_case.operation, test_case.fill_rule, output.closed, output.open)) {
    throw std::runtime_error("Clipper2 Execute returned false");
  }
  return output;
}

TimedOutput benchmark(const WorkloadCase& test_case) {
  Output output = execute(test_case);
  for (std::size_t i = 1; i < kWarmups; ++i) output = execute(test_case);

  std::size_t iterations_per_sample = 1;
  while (true) {
    const auto start = std::chrono::steady_clock::now();
    for (std::size_t i = 0; i < iterations_per_sample; ++i) output = execute(test_case);
    const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::steady_clock::now() - start).count();
    if (elapsed >= kMinimumSampleTimeNs ||
        iterations_per_sample == kMaximumIterationsPerSample) {
      break;
    }
    iterations_per_sample = std::min(iterations_per_sample * 2,
                                     kMaximumIterationsPerSample);
  }

  std::vector<long long> timings;
  timings.reserve(kSamples);
  for (std::size_t sample = 0; sample < kSamples; ++sample) {
    const auto start = std::chrono::steady_clock::now();
    for (std::size_t i = 0; i < iterations_per_sample; ++i) output = execute(test_case);
    timings.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(
                          std::chrono::steady_clock::now() - start).count() /
                      static_cast<long long>(iterations_per_sample));
  }
  std::sort(timings.begin(), timings.end());
  return {std::move(output), timings[kSamples / 2],
          timings[(kSamples * 95 + 99) / 100 - 1], iterations_per_sample};
}

std::string signature(const Paths64& paths) {
  std::ostringstream output;
  output << '[';
  for (std::size_t i = 0; i < paths.size(); ++i) {
    if (i != 0) output << ',';
    output << '[';
    for (std::size_t j = 0; j < paths[i].size(); ++j) {
      if (j != 0) output << ',';
      output << '[' << paths[i][j].x << ',' << paths[i][j].y << ']';
    }
    output << ']';
  }
  return output.str() + ']';
}

void print_success(const WorkloadCase& test_case, const TimedOutput& timed) {
  std::cout << "{\"id\":\"" << test_case.id
            << "\",\"status\":\"ok\",\"error\":null,\"median_ns\":" << timed.median_ns
            << ",\"p95_ns\":" << timed.p95_ns
            << ",\"iterations_per_sample\":" << timed.iterations_per_sample
            << ",\"closed_path_count\":" << timed.output.closed.size()
            << ",\"open_path_count\":" << timed.output.open.size()
            << ",\"closed_signature\":\"" << signature(timed.output.closed)
            << "\",\"open_signature\":\"" << signature(timed.output.open) << "\"}\n";
}

void print_error(const std::string& id, const std::string& error) {
  std::cout << "{\"id\":\"" << id << "\",\"status\":\"error\",\"error\":\""
            << error
            << "\",\"median_ns\":0,\"p95_ns\":0,\"iterations_per_sample\":0,"
               "\"closed_path_count\":0,\"open_path_count\":0,"
               "\"closed_signature\":\"[]\",\"open_signature\":\"[]\"}\n";
}

}  // namespace

int main() {
  std::cout << "{\"implementation\":\"clipper2-native-open-i64\","
               "\"revision\":\"f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd\","
               "\"samples\":25,\"warmups\":3,\"minimum_sample_time_ns\":"
            << kMinimumSampleTimeNs << "}\n";
  while (true) {
    WorkloadCase test_case;
    try {
      if (!read_case(std::cin, test_case)) break;
      print_success(test_case, benchmark(test_case));
    } catch (const std::exception& error) {
      print_error(test_case.id, error.what());
    }
  }
}
