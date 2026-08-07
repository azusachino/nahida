.PHONY: help local build

.DEFAULT_GOAL := help

export NO_MKDOCS_2_WARNING := true

help: ## show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-8s\033[0m %s\n", $$1, $$2}'

local: ## serve the tutorial locally with live reload
	uv run mkdocs serve --dev-addr 0.0.0.0:1314

build: ## build the static site (strict -- broken links/nav fail the build)
	uv run mkdocs build --strict
