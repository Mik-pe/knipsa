import fs from 'node:fs';
import process from 'node:process';
import * as polygonClipping from 'martinez-polygon-clipping';

const workloadPath = process.argv[2];
if (!workloadPath) {
  throw new Error('usage: node adapter.mjs <workloads.json>');
}

const workload = JSON.parse(fs.readFileSync(workloadPath, 'utf8'));
const operations = {
  intersection: (subjects, clips) => execute('intersection', subjects, clips),
  union: (subjects, clips) => execute('union', subjects, clips),
  difference: (subjects, clips) => execute('difference', subjects, clips),
  xor: (subjects, clips) => execute('xor', subjects, clips),
};

function execute(operation, subjects, clips) {
  const left = toGeometry(subjects);
  const right = toGeometry(clips);
  if (subjects.length === 0 || clips.length === 0) {
    if (operation === 'intersection') return [];
    if (operation === 'difference' && subjects.length === 0) return [];
    const nonEmpty = subjects.length === 0 ? right : left;
    return polygonClipping.union(nonEmpty, nonEmpty);
  }
  return polygonClipping[operation === 'difference' ? 'diff' : operation](left, right);
}

function toGeometry(paths) {
  return paths.map((path) => {
    const first = path[0];
    const last = path.at(-1);
    const ring = last && first[0] === last[0] && first[1] === last[1]
      ? path
      : path.concat([first]);
    return [ring];
  });
}

function quantize(value) {
  const rounded = Math.round(value * 1e9) / 1e9;
  return Object.is(rounded, -0) ? 0 : rounded;
}

function area2(ring) {
  let result = 0;
  for (let index = 0; index < ring.length; index += 1) {
    const point = ring[index];
    const next = ring[(index + 1) % ring.length];
    result += point[0] * next[1] - point[1] * next[0];
  }
  return result;
}

function normalizeRing(ring) {
  const points = ring.slice();
  if (points.length > 1 && points[0][0] === points.at(-1)[0] && points[0][1] === points.at(-1)[1]) {
    points.pop();
  }
  const candidates = [points, points.slice().reverse()];
  const rotated = candidates.map((candidate) => {
    let minimum = 0;
    for (let index = 1; index < candidate.length; index += 1) {
      if (candidate[index][0] < candidate[minimum][0] ||
          (candidate[index][0] === candidate[minimum][0] && candidate[index][1] < candidate[minimum][1])) {
        minimum = index;
      }
    }
    return candidate.slice(minimum).concat(candidate.slice(0, minimum));
  });
  rotated.sort(comparePointArrays);
  return rotated[0].map(([x, y]) => [quantize(x), quantize(y)]);
}

function comparePointArrays(left, right) {
  for (let index = 0; index < Math.min(left.length, right.length); index += 1) {
    if (left[index][0] !== right[index][0]) return left[index][0] - right[index][0];
    if (left[index][1] !== right[index][1]) return left[index][1] - right[index][1];
  }
  return left.length - right.length;
}

function contains(point, ring) {
  let inside = false;
  for (let index = 0; index < ring.length; index += 1) {
    const a = ring[index];
    const b = ring[(index + 1) % ring.length];
    if ((a[1] > point[1]) !== (b[1] > point[1])) {
      const cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
      if ((b[1] > a[1] && cross > 0) || (b[1] < a[1] && cross < 0)) inside = !inside;
    }
  }
  return inside;
}

function signature(result) {
  const polygons = result ?? [];
  const rings = canonicalBoundaryRings(polygons);
  const records = rings.map((ring) => {
    const normalized = normalizeRing(ring);
    const point = interiorProbe(ring);
    const depth = rings.filter((other) => other !== ring && contains(point, other)).length;
    return { depth, area2: quantize(Math.abs(area2(ring))), points: normalized };
  });
  records.sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)));
  return JSON.stringify(records);
}

function canonicalBoundaryRings(polygons) {
  const sourceRings = polygons.flatMap((polygon) => polygon).map((ring) => {
    const points = ring.slice();
    if (points.length > 1 && points[0][0] === points.at(-1)[0] && points[0][1] === points.at(-1)[1]) {
      points.pop();
    }
    return points.map(([x, y]) => [quantize(x), quantize(y)]);
  });
  const edges = [];
  for (const ring of sourceRings) {
    for (let index = 0; index < ring.length; index += 1) {
      const start = ring[index];
      const end = ring[(index + 1) % ring.length];
      if (start[0] !== end[0] || start[1] !== end[1]) edges.push({ start, end });
    }
  }

  const directed = [];
  for (const edge of edges) {
    const dx = edge.end[0] - edge.start[0];
    const dy = edge.end[1] - edge.start[1];
    const length = Math.hypot(dx, dy);
    const midpoint = [(edge.start[0] + edge.end[0]) / 2, (edge.start[1] + edge.end[1]) / 2];
    const epsilon = Math.min(length * 1e-4, 1e-9 * Math.max(1, ...edge.start.map(Math.abs), ...edge.end.map(Math.abs)));
    const left = [midpoint[0] - (dy / length) * epsilon, midpoint[1] + (dx / length) * epsilon];
    const right = [midpoint[0] + (dy / length) * epsilon, midpoint[1] - (dx / length) * epsilon];
    const leftInside = sourceRings.reduce((inside, ring) => inside !== contains(left, ring), false);
    const rightInside = sourceRings.reduce((inside, ring) => inside !== contains(right, ring), false);
    if (leftInside !== rightInside) directed.push(leftInside ? edge : { start: edge.end, end: edge.start });
  }

  const outgoing = new Map();
  directed.forEach((edge, index) => {
    const key = pointKey(edge.start);
    if (!outgoing.has(key)) outgoing.set(key, []);
    outgoing.get(key).push(index);
  });
  for (const indices of outgoing.values()) {
    indices.sort((left, right) => compareAngle(vector(directed[left]), vector(directed[right])));
  }
  const next = directed.map((edge) => {
    const candidates = outgoing.get(pointKey(edge.end));
    if (!candidates) return -1;
    const reverse = [edge.start[0] - edge.end[0], edge.start[1] - edge.end[1]];
    let insertion = candidates.findIndex((candidate) => compareAngle(vector(directed[candidate]), reverse) >= 0);
    if (insertion < 0) insertion = candidates.length;
    return candidates[(insertion + candidates.length - 1) % candidates.length];
  });

  const visited = new Set();
  const rings = [];
  for (let start = 0; start < directed.length; start += 1) {
    if (visited.has(start)) continue;
    const ring = [];
    let current = start;
    while (current >= 0 && !visited.has(current)) {
      visited.add(current);
      ring.push(directed[current].start);
      current = next[current];
    }
    if (current === start && ring.length >= 3 && Math.abs(area2(ring)) > 1e-12) {
      rings.push(ring);
    }
  }
  return rings;
}

function pointKey([x, y]) {
  return `${x},${y}`;
}

function vector(edge) {
  if (Array.isArray(edge)) return edge;
  return [edge.end[0] - edge.start[0], edge.end[1] - edge.start[1]];
}

function compareAngle(left, right) {
  const leftAngle = Math.atan2(left[1], left[0]);
  const rightAngle = Math.atan2(right[1], right[0]);
  const normalizedLeft = leftAngle < 0 ? leftAngle + Math.PI * 2 : leftAngle;
  const normalizedRight = rightAngle < 0 ? rightAngle + Math.PI * 2 : rightAngle;
  if (normalizedLeft !== normalizedRight) return normalizedLeft - normalizedRight;
  return Math.hypot(left[0], left[1]) - Math.hypot(right[0], right[1]);
}

function interiorProbe(ring) {
  const orientation = area2(ring) >= 0 ? 1 : -1;
  for (let index = 0; index < ring.length; index += 1) {
    const start = ring[index];
    const end = ring[(index + 1) % ring.length];
    const dx = end[0] - start[0];
    const dy = end[1] - start[1];
    const length = Math.hypot(dx, dy);
    if (length === 0) continue;
    const scale = Math.max(1, ...start.map(Math.abs), ...end.map(Math.abs));
    const epsilon = Math.min(length * 1e-4, 1e-9 * scale);
    return [(start[0] + end[0]) / 2 - orientation * (dy / length) * epsilon,
      (start[1] + end[1]) / 2 + orientation * (dx / length) * epsilon];
  }
  return ring[0];
}
for (const testCase of workload.cases) {
  const operation = operations[testCase.clip_type];
  for (let index = 0; index < 3; index += 1) operation(testCase.subjects, testCase.clips);
  const timings = [];
  let result = [];
  for (let index = 0; index < 25; index += 1) {
    const started = process.hrtime.bigint();
    result = operation(testCase.subjects, testCase.clips);
    timings.push(Number(process.hrtime.bigint() - started));
  }
  timings.sort((left, right) => left - right);
  const medianNs = timings[Math.floor(timings.length / 2)];
  const p95Ns = timings[Math.ceil(timings.length * 0.95) - 1];
  process.stdout.write(`${JSON.stringify({
    id: testCase.id,
    status: 'ok',
    error: null,
    median_ns: medianNs,
    p95_ns: p95Ns,
    signature: signature(result),
    ring_count: (result ?? []).flat().length,
  })}\n`);
}
