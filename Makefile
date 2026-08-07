.DEFAULT_GOAL := help
.PHONY: help fmt fmt-check lint test check validate run build eval tutorial tutorial-build

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN{FS=":.*## "}{printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

## --- Development ---
run: ## Run the agent (make run ARGS="what does this repo do")
	@cargo run --quiet -p nahida-cli -- $(ARGS)

build: ## Release build
	@cargo build --release

## --- Gates ---
fmt: ## Format
	@cargo fmt --all

fmt-check: ## Check formatting
	@cargo fmt --all --check

lint: ## Clippy, warnings are failures
	@cargo clippy --workspace --all-targets -- -D warnings

test: ## Run tests
	@cargo test --workspace

check: fmt-check lint test ## Everything that must pass before a commit

validate: check ## Everything that must pass before a PR
	@cargo build --release

eval: ## Real-provider evals -- costs tokens, never run from check/CI
	@cargo test -p nahida-cli --test evals -- --ignored --nocapture

## --- Tutorial (docs/, mise+uv-managed, kept apart from the Rust toolchain) ---
tutorial: ## Serve the 0-to-hero tutorial locally with live reload
	@cd docs && mise exec -- uv run mkdocs serve --dev-addr 0.0.0.0:1314

tutorial-build: ## Build the tutorial site (strict)
	@cd docs && mise exec -- uv run mkdocs build --strict
