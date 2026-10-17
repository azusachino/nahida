.DEFAULT_GOAL := help
.PHONY: help run typecheck lint test check validate

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN{FS=":.*## "}{printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

node_modules: bun.lock package.json
	@bun install --frozen-lockfile

## --- Development ---
run: node_modules ## Run the agent (make run ARGS="what does this repo do")
	@bun run src/cli.ts $(ARGS)

## --- Gates ---
typecheck: node_modules ## TypeScript, no emit
	@bun run typecheck

lint: node_modules ## Biome lint and format check
	@bun run lint

test: node_modules ## Run tests (no network)
	@bun test

check: typecheck lint test ## Everything that must pass before a commit

validate: check ## Everything that must pass before a PR
	@bun build src/cli.ts --target bun --outdir .tmp/build
	@bun .tmp/build/cli.js >/dev/null 2>&1; test $$? -eq 2
