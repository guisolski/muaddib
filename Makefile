.DEFAULT_GOAL := help

.PHONY: help all build release run test check fmt fmt-check lint clean install doc hooks precommit ci

help: ## Show this help (default target)
	@awk 'BEGIN {FS = ":.*## "; printf "\nmuaddib — make targets\n\n"} /^[a-zA-Z_-]+:.*## / {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2} END {print ""}' $(MAKEFILE_LIST)

all: build ## Alias for build

build: ## Compile the debug binary
	cargo build

release: ## Compile the optimized release binary
	cargo build --release

run: ## Launch the TUI (debug build)
	cargo run

test: ## Run the full test suite with all features
	cargo test --all-features

check: ## Type-check every target without building
	cargo check --all-targets --all-features

fmt: ## Format the whole workspace
	cargo fmt --all

fmt-check: ## Fail if any file is not formatted
	cargo fmt --all -- --check

lint: ## Run clippy on every target, warnings are errors
	cargo clippy --all-targets --all-features -- -D warnings

clean: ## Remove build artifacts
	cargo clean

install: ## Install the muaddib binary with cargo
	cargo install --path .

doc: ## Build and open the API docs
	cargo doc --no-deps --open

hooks: ## Install pre-commit, commit-msg and pre-push hooks
	pre-commit install --install-hooks -t pre-commit -t commit-msg -t pre-push

precommit: ## Run every pre-commit hook against all files
	pre-commit run --all-files

ci: fmt-check lint test release ## Exactly what CI runs
