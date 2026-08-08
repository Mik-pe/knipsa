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

function signature(result) {
  const rings = (result ?? []).flatMap((polygon) => polygon).map((ring) => {
    const points = ring.slice();
    if (points.length > 1 && points[0][0] === points.at(-1)[0] && points[0][1] === points.at(-1)[1]) {
      points.pop();
    }
    return points;
  });
  return JSON.stringify(rings);
}

const minimumSampleTimeNs = 2_000_000n;
const maximumIterationsPerSample = 1 << 20;

process.stdout.write(`${JSON.stringify({
  implementation: 'martinez-polygon-clipping',
  revision: '0.8.1',
  samples: 25,
  warmups: 3,
  minimum_sample_time_ns: Number(minimumSampleTimeNs),
})}\n`);

for (const testCase of workload.cases) {
  const operation = operations[testCase.clip_type];
  for (let index = 0; index < 3; index += 1) operation(testCase.subjects, testCase.clips);
  let result = [];
  let iterationsPerSample = 1;
  while (true) {
    const started = process.hrtime.bigint();
    for (let index = 0; index < iterationsPerSample; index += 1) {
      result = operation(testCase.subjects, testCase.clips);
    }
    const elapsed = process.hrtime.bigint() - started;
    if (elapsed >= minimumSampleTimeNs || iterationsPerSample === maximumIterationsPerSample) break;
    iterationsPerSample = Math.min(iterationsPerSample * 2, maximumIterationsPerSample);
  }
  const timings = [];
  for (let index = 0; index < 25; index += 1) {
    const started = process.hrtime.bigint();
    for (let iteration = 0; iteration < iterationsPerSample; iteration += 1) {
      result = operation(testCase.subjects, testCase.clips);
    }
    timings.push(Number((process.hrtime.bigint() - started) / BigInt(iterationsPerSample)));
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
    iterations_per_sample: iterationsPerSample,
    signature: signature(result),
    ring_count: (result ?? []).flat().length,
  })}\n`);
}
