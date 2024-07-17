.PHONY: help

EXTRA_DIRS := fuzz examples/demo-rollup/provers/risc0/guest-mock examples/demo-rollup/provers/risc0/guest-celestia

help: ## Display this help message
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build the project
	@cargo build

clean: ## Cleans compiled
	@cargo clean

total-clean: clean
total-clean:
	$(MAKE) -C examples/demo-rollup clean;
	$(MAKE) -C examples/demo-rollup clean-wallet;
	@for dir in $(EXTRA_DIRS); do \
    	echo "Running cargo clean in $$dir"; \
    	(cd $$dir && cargo clean); \
    done;
	rm -rf "examples/demo-rollup/tests/evm/uniswap/node_modules"


test-legacy: ## Runs test suite with output from tests printed
	@cargo test -- --nocapture -Zunstable-options --report-time

test:  ## Runs test suite using next test
	@cargo nextest run --workspace --all-features --status-level skip

test-default-features:  ## Runs test suite using default features
	@cargo nextest run --workspace --status-level skip

install-dev-tools:  ## Installs all necessary cargo helpers
	## Backup VS Code settings to `.vscode/settings.json.bak`.
	cp .vscode/settings.json .vscode/settings.json.bak || true
	## Install the default suggested VS Code settings.
	cp .vscode/settings.default.json .vscode/settings.json
	cargo install cargo-llvm-cov
	cargo install cargo-hack
	cargo install cargo-udeps
	cargo install flaky-finder
	cargo install cargo-nextest --locked
	cargo install cargo-risczero
	cargo risczero install
	cargo install zepter
	rustup target add thumbv6m-none-eabi
	rustup target add wasm32-unknown-unknown

install-risc0-toolchain:
	cargo risczero install --version r0.1.78.0
	@echo "Risc0 toolchain version:"
	cargo +risc0 --version

lint:  ## cargo check and clippy. Skip clippy on guest code since it's not supported by risc0
	## fmt first, because it's the cheapest
	cargo +nightly fmt --all --check
	cargo check --all-targets --all-features
	## Invokes Zepter multiple times because fixes sometimes unveal more underlying issues.
	zepter
	zepter
	zepter
	$(MAKE) check-fuzz
	SKIP_GUEST_BUILD=1 cargo clippy --all-targets --all-features

extra-check:   ## cargo check in non attached crates
	cargo check
	@for dir in $(EXTRA_DIRS); do \
		echo "Running cargo check in $$dir"; \
		(cd $$dir && cargo check); \
	done

lint-fix:  ## cargo fmt, fix and clippy. Skip clippy on guest code since it's not supported by risc0
	cargo +nightly fmt --all
	cargo fix --allow-dirty
	SKIP_GUEST_BUILD=1 cargo clippy --fix --allow-dirty

check-features: ## Checks that project compiles with all combinations of features.
	cargo hack check --workspace --feature-powerset --exclude-features default --all-targets

check-features-default-targets:
	cargo hack check --workspace --feature-powerset --exclude-features default

check-fuzz: ## Checks that fuzz member compiles
	$(MAKE) -C crates/fuzz check

find-unused-deps: ## Prints unused dependencies for project. Note: requires nightly
	cargo +nightly udeps --all-targets --all-features

find-flaky-tests:  ## Runs tests over and over to find if there's flaky tests
	flaky-finder -j16 -r320 --continue "cargo test -- --nocapture"

coverage: ## Coverage in lcov format
	cargo llvm-cov --locked --lcov --output-path lcov.info

coverage-html: ## Coverage in HTML format
	cargo llvm-cov --locked --all-features --html

dry-run-publish: 
	yq '.[]' packages_to_publish.yml | xargs -I _ cargo publish --allow-dirty --dry-run -p _

docs:  ## Generates documentation locally
	cargo doc --open
