.PHONY: check
check: ## dev: do we compile?
	@$(call log,"checking compilation")
	@cargo check --all-targets --all-features

.PHONY: clean
clean: ## dev: purge build artifacts
	@$(call log,"cleaning build artifacts")
	@cargo clean

.PHONY: lint
lint: ## dev: are we idiomatic?
	@$(call log,"linting the crate")
	@cargo clippy --all-targets --all-features -- -D warnings

.PHONY: format
format: ## dev: format the code
	@$(call log,"formatting the crate")
	@cargo fmt --verbose --all

.PHONY: format-check
format-check: ## dev: are we pretty?
	@$(call log,"checking code formatting")
	@cargo fmt --verbose --all -- --check

.PHONY: test
test: ## dev: do we profess to work?
	@$(call log,"running all tests")
	@cargo nextest run --profile ci --all-features --no-tests=pass

.PHONY: install
install: ## dev: put this build of senzu in ~/.cargo/bin
	@$(call log,"installing senzu")
	@cargo install --path . --locked

.PHONY: deny
deny: ## dev: any known holes, odd licenses or stray sources in our deps?
	@$(call log,"checking dependencies")
	@cargo deny check
