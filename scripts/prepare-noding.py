"""One-off source transformation; removed from the published candidate tree."""
from pathlib import Path
import json
import os
import subprocess
import urllib.request

p = Path('crates/knipsa/src/dispatch/broad_phase.rs')
s = p.read_text()
core = s[s.index('#[derive(Clone, Copy, Debug)]'):s.index('#[cfg(test)]')]
core = core.replace('fn visit_pairs(', 'pub(crate) fn visit_pairs(', 1)
tests = s[s.index('#[cfg(test)]'):s.index('    #[test]\n    fn non_adjacent_contacts')]
spatial = '''//! Deterministic closed-box pair traversal shared by geometry certificates and noding.

pub(crate) type Bounds = (i64, i64, i64, i64);
const LEAF_CAPACITY: usize = 8;

#[inline]
pub(crate) fn boxes_touch_or_overlap64(first: Bounds, second: Bounds) -> bool {
    !(first.2 < second.0 || second.2 < first.0 || first.3 < second.1 || second.3 < first.1)
}

''' + core + tests + '}\n'
Path('crates/knipsa/src/spatial.rs').write_text(spatial)
cert = s[:s.index('#[derive(Clone, Copy, Debug)]')]
cert = cert.replace('use super::{boxes_touch_or_overlap64, edges_intersect64, path_bounds64};', 'use super::{edges_intersect64, path_bounds64};\nuse crate::spatial::{Bounds, visit_pairs};')
cert = cert.replace('type Bounds = (i64, i64, i64, i64);\nconst LEAF_CAPACITY: usize = 8;\n\n', '')
cert += '#[cfg(test)]\nmod tests {\n    use super::*;\n\n' + s[s.index('    #[test]\n    fn non_adjacent_contacts'):]
p.write_text(cert)
p = Path('crates/knipsa/src/dispatch.rs')
s = p.read_text()
old = '''fn boxes_touch_or_overlap64(first: (i64, i64, i64, i64), second: (i64, i64, i64, i64)) -> bool {
    !(first.2 < second.0 || second.2 < first.0 || first.3 < second.1 || second.3 < first.1)
}

'''
assert s.count(old) == 1
s = s.replace(old, '')
s = s.replace('    use super::*;\n', '    use super::*;\n    use crate::spatial::boxes_touch_or_overlap64;\n', 1)
p.write_text(s)
p = Path('crates/knipsa/src/lib.rs')
s = p.read_text().replace('mod standard_dispatch;', 'mod spatial;\nmod standard_dispatch;')
p.write_text(s)
p = Path('crates/knipsa/src/boolean.rs')
s = p.read_text().replace('use std::cmp::Ordering;', 'mod noding;\n\nuse std::cmp::Ordering;', 1)
a = s.index('    let mut edge_order: Vec<usize> = (0..edges.len()).collect();', s.index('fn run_boolean('))
b = s.index('\n    let atomic_edges', a)
s = s[:a] + '    noding::node_edges(&edges, &mut split_parameters);\n' + s[b:]
p.write_text(s)
p = Path('scripts/benchmark-direct-certification.py')
s = p.read_text()
s = s.replace('parser.add_argument("--head", default="HEAD")', 'parser.add_argument("--head", default="HEAD")\n    parser.add_argument("--workload", choices=["direct-certification", "exact-noding"], default="direct-certification")')
s = s.replace('default=Path("target/direct-certification.json")', 'default=None')
s = s.replace('root = Path(__file__).resolve().parents[1]\n    source = root / "benchmarks/direct-certification.rs"', 'root = Path(__file__).resolve().parents[1]\n    source = root / "benchmarks" / (args.workload + ".rs")\n    if args.output is None:\n        args.output = Path("target") / (args.workload + ".json")')
s = s.replace('"revisions": revisions,', '"revisions": revisions,\n        "workload": args.workload,')
s = s.replace('knipsa-certification-', 'knipsa-paired-').replace('direct_certification_bench', 'paired_revision_bench')
s = s.replace('print("## Direct integer certification: paired public-API benchmark\\n")', 'print(f"## {args.workload}: paired public-API benchmark\\n")')
Path('scripts/benchmark-revisions.py').write_text(s)
p.unlink()
for name in ['.github/workflows/direct-certification.yml', 'docs/spatial-certification-results.md']:
    p = Path(name)
    p.write_text(p.read_text().replace('benchmark-direct-certification.py', 'benchmark-revisions.py'))
Path('.github/workflows/noding-evaluation.yml').write_text('''name: Exact noding benchmark

on:
  pull_request:
    paths:
      - 'crates/knipsa/src/**'
      - 'benchmarks/exact-noding.rs'
      - 'scripts/benchmark-revisions.py'
      - '.github/workflows/noding-evaluation.yml'

permissions:
  contents: read

jobs:
  paired:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
      - name: Compare full Boolean results and paired latency
        env:
          BASE_SHA: ${{ github.event.pull_request.base.sha }}
          HEAD_SHA: ${{ github.event.pull_request.head.sha }}
        run: |
          set -o pipefail
          python3 scripts/benchmark-revisions.py --workload exact-noding --base "$BASE_SHA" --head "$HEAD_SHA" --pairs 5 --output target/exact-noding.json | tee noding-summary.md
          cat noding-summary.md >> "$GITHUB_STEP_SUMMARY"
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: exact-noding-${{ github.event.pull_request.head.sha }}
          path: |
            target/exact-noding.json
            noding-summary.md
          if-no-files-found: warn
''')
output = Path('target/noding-preparation')
output.mkdir(parents=True, exist_ok=True)
commands = [
    ['cargo', 'fmt', '--all'],
    ['rustfmt', '--edition', '2024', '--config-path', 'rustfmt.toml', 'benchmarks/exact-noding.rs'],
    ['cargo', 'test', '--workspace', '--all-features'],
    ['cargo', 'clippy', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings'],
    ['git', 'diff', '--check'],
]
with (output / 'checks.log').open('w') as log:
    for command in commands:
        result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        log.write('$ ' + ' '.join(command) + '\n' + result.stdout)
        log.flush()
        print(result.stdout, flush=True)
        if result.returncode:
            raise SystemExit(result.returncode)
files = [
    'crates/knipsa/src/boolean.rs', 'crates/knipsa/src/boolean/noding.rs',
    'crates/knipsa/src/dispatch.rs', 'crates/knipsa/src/dispatch/broad_phase.rs',
    'crates/knipsa/src/lib.rs', 'crates/knipsa/src/spatial.rs',
    'benchmarks/exact-noding.rs', 'scripts/benchmark-revisions.py',
    '.github/workflows/direct-certification.yml', '.github/workflows/noding-evaluation.yml',
    'docs/spatial-certification-results.md',
]
removed = ['scripts/benchmark-direct-certification.py', 'scripts/prepare-noding.py', '.github/workflows/prepare-noding.yml']
entries = [{'path': name, 'mode': '100644', 'type': 'blob', 'content': Path(name).read_text()} for name in files]
entries.extend({'path': name, 'mode': '100644', 'type': 'blob', 'sha': None} for name in removed)
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
base_tree = subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], text=True).strip()
request = urllib.request.Request(
    'https://api.github.com/repos/' + os.environ['GITHUB_REPOSITORY'] + '/git/trees',
    data=json.dumps({'base_tree': base_tree, 'tree': entries}).encode(),
    headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'},
    method='POST',
)
with urllib.request.urlopen(request, timeout=60) as response:
    tree = json.load(response)
report = {'source_head': head, 'tree_sha': tree['sha'], 'files': files, 'removed': removed}
(output / 'tree.json').write_text(json.dumps(report, indent=2) + '\n')
subprocess.run(['git', 'add', '--', *files], check=True)
(output / 'changes.patch').write_bytes(subprocess.check_output(['git', 'diff', '--cached']))
print(json.dumps(report, indent=2), flush=True)
