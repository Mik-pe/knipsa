#include <clipper2/clipper.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <stdexcept>
#include <utility>
#include <vector>

using namespace Clipper2Lib;

namespace {

constexpr std::size_t kWarmups = 3;
constexpr std::size_t kSamples = 25;
constexpr double kQuantization = 1e9;

struct RingRecord {
  std::size_t depth;
  double area2;
  PathD points;
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

PathsD read_paths(std::istream& input, std::size_t count) {
  PathsD paths;
  paths.reserve(count);
  for (std::size_t path_index = 0; path_index < count; ++path_index) {
    std::size_t point_count;
    input >> point_count;
    PathD path;
    path.reserve(point_count);
    for (std::size_t point_index = 0; point_index < point_count; ++point_index) {
      double x, y;
      input >> x >> y;
      path.emplace_back(x, y);
    }
    paths.emplace_back(std::move(path));
  }
  return paths;
}

double quantize(double value) {
  double rounded = std::round(value * kQuantization) / kQuantization;
  return rounded == -0.0 ? 0.0 : rounded;
}

bool path_less(const PathD& left, const PathD& right);

PathD remove_collinear(PathD points) {
  bool changed = true;
  while (changed && points.size() >= 3) {
    changed = false;
    PathD cleaned;
    cleaned.reserve(points.size());
    for (std::size_t index = 0; index < points.size(); ++index) {
      const PointD& previous = points[(index + points.size() - 1) % points.size()];
      const PointD& current = points[index];
      const PointD& next = points[(index + 1) % points.size()];
      const double first_x = current.x - previous.x;
      const double first_y = current.y - previous.y;
      const double second_x = next.x - current.x;
      const double second_y = next.y - current.y;
      const double cross = first_x * second_y - first_y * second_x;
      const double dot = first_x * second_x + first_y * second_y;
      if (std::abs(cross) <= 1e-12 && dot >= -1e-12) {
        changed = true;
      } else {
        cleaned.push_back(current);
      }
    }
    points = std::move(cleaned);
  }
  return points;
}

PathD canonical_ring(const PathD& path) {
  PathD points;
  points.reserve(path.size());
  for (const PointD& point : path) points.emplace_back(quantize(point.x), quantize(point.y));
  points = remove_collinear(std::move(points));
  auto minimum = std::min_element(points.begin(), points.end(), [](const PointD& left, const PointD& right) {
    return left.x < right.x || (left.x == right.x && left.y < right.y);
  });
  if (minimum != points.end()) std::rotate(points.begin(), minimum, points.end());

  PathD reversed = points;
  std::reverse(reversed.begin(), reversed.end());
  auto reversed_minimum = std::min_element(reversed.begin(), reversed.end(), [](const PointD& left, const PointD& right) {
    return left.x < right.x || (left.x == right.x && left.y < right.y);
  });
  if (reversed_minimum != reversed.end()) std::rotate(reversed.begin(), reversed_minimum, reversed.end());
  return path_less(reversed, points) ? reversed : points;
}

bool path_less(const PathD& left, const PathD& right) {
  const std::size_t common = std::min(left.size(), right.size());
  for (std::size_t index = 0; index < common; ++index) {
    if (left[index].x != right[index].x) return left[index].x < right[index].x;
    if (left[index].y != right[index].y) return left[index].y < right[index].y;
  }
  return left.size() < right.size();
}

double area2(const PathD& path) {
  double result = 0.0;
  for (std::size_t index = 0; index < path.size(); ++index) {
    const PointD& first = path[index];
    const PointD& second = path[(index + 1) % path.size()];
    result += first.x * second.y - first.y * second.x;
  }
  return result;
}

bool contains(PointD point, const PathD& path) {
  bool inside = false;
  for (std::size_t index = 0; index < path.size(); ++index) {
    const PointD& first = path[index];
    const PointD& second = path[(index + 1) % path.size()];
    if ((first.y > point.y) != (second.y > point.y)) {
      double cross = (second.x - first.x) * (point.y - first.y) -
                     (second.y - first.y) * (point.x - first.x);
      if ((second.y > first.y && cross > 0.0) || (second.y < first.y && cross < 0.0)) {
        inside = !inside;
      }
    }
  }
  return inside;
}

std::vector<RingRecord> records(const PathsD& paths) {
  std::vector<RingRecord> result;
  result.reserve(paths.size());
  for (std::size_t index = 0; index < paths.size(); ++index) {
    PathD points = canonical_ring(paths[index]);
    std::size_t depth = 0;
    for (std::size_t other = 0; other < paths.size(); ++other) {
      if (index != other && contains(points.front(), paths[other])) ++depth;
    }
    result.push_back({depth, quantize(std::abs(area2(paths[index]))), std::move(points)});
  }
  std::sort(result.begin(), result.end(), [](const RingRecord& left, const RingRecord& right) {
    if (left.depth != right.depth) return left.depth < right.depth;
    if (left.area2 != right.area2) return left.area2 < right.area2;
    return path_less(left.points, right.points);
  });
  return result;
}

std::string signature(const PathsD& paths) {
  const auto result = records(paths);
  std::ostringstream output;
  output << std::setprecision(17) << '[';
  for (std::size_t index = 0; index < result.size(); ++index) {
    if (index != 0) output << ',';
    output << "{\"depth\":" << result[index].depth
           << ",\"area2\":" << result[index].area2 << ",\"points\":[";
    for (std::size_t point_index = 0; point_index < result[index].points.size(); ++point_index) {
      if (point_index != 0) output << ',';
      const PointD& point = result[index].points[point_index];
      output << '[' << point.x << ',' << point.y << ']';
    }
    output << "]}";
  }
  output << ']';
  return output.str();
}

void print_error(const std::string& id, const std::string& message) {
  std::cout << "{\"id\":\"" << id << "\",\"status\":\"error\",\"error\":\""
            << message << "\",\"median_ns\":0,\"p95_ns\":0,\"ring_count\":0,\"signature\":\"[]\"}\n";
}

}  // namespace

int main() {
  std::cout << "{\"implementation\":\"clipper2-native\",\"revision\":\"f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd\",\"samples\":25,\"warmups\":3}\n";
  std::string id;
  int clip_type_value, fill_rule_value;
  std::size_t subject_count;
  while (std::cin >> id >> clip_type_value >> fill_rule_value >> subject_count) {
    try {
      PathsD subjects = read_paths(std::cin, subject_count);
      std::size_t clip_count;
      std::cin >> clip_count;
      PathsD clips = read_paths(std::cin, clip_count);
      const auto operation = clip_type(clip_type_value);
      const auto rule = fill_rule(fill_rule_value);
      PathsD output;
      for (std::size_t run = 0; run < kWarmups; ++run) {
        output = BooleanOp(operation, rule, subjects, clips, 8);
      }
      std::vector<long long> timings;
      timings.reserve(kSamples);
      for (std::size_t run = 0; run < kSamples; ++run) {
        const auto started = std::chrono::steady_clock::now();
        output = BooleanOp(operation, rule, subjects, clips, 8);
        const auto finished = std::chrono::steady_clock::now();
        timings.push_back(std::chrono::duration_cast<std::chrono::nanoseconds>(finished - started).count());
      }
      std::sort(timings.begin(), timings.end());
      const auto p95_index = (kSamples * 95 + 99) / 100 - 1;
      std::cout << "{\"id\":\"" << id << "\",\"status\":\"ok\",\"error\":null,\"median_ns\":"
                << timings[kSamples / 2] << ",\"p95_ns\":" << timings[p95_index]
                << ",\"ring_count\":" << output.size() << ",\"signature\":\"";
      for (const char character : signature(output)) {
        if (character == '"' || character == '\\') std::cout << '\\';
        std::cout << character;
      }
      std::cout << "\"}\n";
    } catch (const std::exception& error) {
      print_error(id, error.what());
    }
  }
}
