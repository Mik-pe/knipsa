.PHONY: check test lint docs c-api coverage conformance

check: test lint c-api

test:
	cargo test --workspace --all-features
	python3 -m unittest scripts/test_compare_benchmark_results.py

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
