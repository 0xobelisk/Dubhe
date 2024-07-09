PROJECT := "sov-wasm"
FLAGS := RUSTFLAGS="$(RUSTFLAGS) --remap-path-prefix $(HOME)= -C link-args=-zstack-size=65536"
WASM := "target/wasm32-wasi/release/sov_wasm.wasm"
WAT := "assets/$(PROJECT).wat"
WAT_THIN := "assets/$(PROJECT)-thin.wat"
PACKAGE := "assets/$(PROJECT).wasm"
PACKAGE_OPTIMIZED := "assets/$(PROJECT)-initial-optimized.wasm"
PACKAGE_INITIAL_OPTIMIZED := "assets/$(PROJECT)-initial-optimized.wasm"
PACKAGE_INITIAL_THIN := "assets/$(PROJECT)-initial-thin.wasm"
PACKAGE_RESTORED := "assets/$(PROJECT)-restored.wasm"
PACKAGE_RESTORED_OPTIMIZED := "assets/$(PROJECT)-restored-optimized.wasm"
PACKAGE_THIN := "assets/$(PROJECT)-thin.wasm"

wasm: ## Build the WASM files
	@$(FLAGS) cargo build --release --target wasm32-wasi
	@mkdir -p assets
	@cp $(WASM) $(PACKAGE)
	@wasm-opt -O4 $(PACKAGE) -o $(PACKAGE_INITIAL_OPTIMIZED)
	@wasm-opt -Oz $(PACKAGE_INITIAL_OPTIMIZED) -o $(PACKAGE_INITIAL_THIN)
	@wasm2wat $(PACKAGE_INITIAL_THIN) -o $(WAT)
	@wat2wasm $(WAT) -o $(PACKAGE_RESTORED)
	@wasm-opt --enable-bulk-memory -O4 $(PACKAGE_RESTORED) -o $(PACKAGE_RESTORED_OPTIMIZED)
	@wasm-opt --enable-bulk-memory -Oz $(PACKAGE_RESTORED_OPTIMIZED) -o $(PACKAGE_THIN)
	@rm $(PACKAGE_INITIAL_OPTIMIZED) \
		$(PACKAGE_INITIAL_THIN) \
		$(WAT) \
		$(PACKAGE_RESTORED) \
		$(PACKAGE_RESTORED_OPTIMIZED)
	@mv $(PACKAGE_THIN) $(PACKAGE)
	@echo "Package created: $(PACKAGE)"

help: ## Display this help screen
	@grep -h \
		-E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

.PHONY: wasm help
