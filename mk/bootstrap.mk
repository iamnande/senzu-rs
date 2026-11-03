.PHONY: bootstrap
bootstrap: ## setup: install mise (if missing) and the pinned toolchain
	@command -v mise >/dev/null 2>&1 || curl -fsSL https://mise.run | sh
	@mise install
	@sh=$${SHELL##*/}; \
		case $$sh in fish) hook="mise activate fish | source" ;; *) hook="eval \"\$$(mise activate $$sh)\"" ;; esac; \
		$(call log,"toolchain ready - new shell? run: $$hook")

.PHONY: doctor
doctor: ## setup: does the toolchain match mise.toml?
	@mise install
	@mise ls --current
