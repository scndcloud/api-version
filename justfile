set shell := ["bash", "-uc"]

check:
	cargo check --tests

fmt toolchain="+nightly":
	cargo {{toolchain}} fmt

fmt-check toolchain="+nightly":
	cargo {{toolchain}} fmt --check

lint:
	cargo clippy --no-deps -- -D warnings

test:
	cargo test

coverage:
	cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info

fix:
	cargo fix --allow-dirty --allow-staged

all: check fmt lint test
