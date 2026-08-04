.PHONY: check test lint c-api coverage

check: test lint c-api

test:
	cargo test --workspace --all-features

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

c-api:
	./scripts/check-c-api.sh

coverage:
	./scripts/coverage.sh
