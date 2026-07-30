.PHONY: all build release run test check fmt fmt-check lint clean install doc hooks precommit ci

all: build

build:
	cargo build

release:
	cargo build --release

run:
	cargo run

test:
	cargo test --all-features

check:
	cargo check --all-targets --all-features

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

clean:
	cargo clean

install:
	cargo install --path .

doc:
	cargo doc --no-deps --open

hooks:
	pre-commit install --install-hooks -t pre-commit -t commit-msg -t pre-push

precommit:
	pre-commit run --all-files

ci: fmt-check lint test release
