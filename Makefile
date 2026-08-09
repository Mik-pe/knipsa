.PHONY: check test lint docs c-api coverage conformance conformance-integer conformance-open conformance-offset conformance-offset-scale conformance-triangulation conformance-triangulation-d conformance-clipper-issues fuzz-replay release-check

check: test lint c-api

test:
	cargo test --workspace --all-features
	python3 -m unittest discover -s scripts -p 'test_*.py'

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

docs:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

c-api:
	./scripts/check-c-api.sh

coverage:
	./scripts/coverage.sh

conformance:
	./scripts/run-conformance.sh

conformance-integer:
	./scripts/run-conformance.sh benchmarks/integer-workloads.json \
		target/conformance-integer clipper2-integer

conformance-open:
	./scripts/run-open-conformance.sh

conformance-offset:
	./scripts/run-offset-conformance.sh

conformance-offset-scale:
	./scripts/generate-offset-scale-workload.py target/offset-scale-workloads.json
	./scripts/run-offset-conformance.sh target/offset-scale-workloads.json target/conformance-offset-scale

conformance-triangulation:
	./scripts/run-triangulation-conformance.sh

conformance-triangulation-d:
	./scripts/run-triangulation-d-conformance.sh

conformance-clipper-issues:
	python3 ./scripts/run-clipper2-issue-conformance.py

fuzz-replay:
	./scripts/fuzz-replay.sh

release-check:
	./scripts/check-release.sh
