.PHONY: check test lint docs c-api coverage

check: test lint c-api

test:
	cargo test --workspace --all-features

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features -- -D warnings

docs:
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

c-api:
	./scripts/check-c-api.sh

coverage:
	./scripts/coverage.sh
