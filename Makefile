.PHONY: help build clean package-all package-% install test

# Default target
help:
	@echo "NeoMind Extensions - Build Commands"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build           - Build a specific extension (EXTENSION=name required)"
	@echo "  package-all     - Build all extensions into .nep packages"
	@echo "  package-<name>  - Build a specific extension (e.g., package-weather-forecast-wasm)"
	@echo "  clean           - Remove build artifacts"
	@echo "  test            - Test a package"
	@echo "  install         - Install a package locally (for testing)"
	@echo ""
	@echo "Examples:"
	@echo "  make package-all"
	@echo "  make package-weather-forecast-wasm"
	@echo "  make package-weather-forecast-wasm EXTENSION=weather-forecast-wasm"

# Build all packages
package-all:
	@echo "Building all extensions..."
	@bash scripts/build-all.sh

# Build specific extension
package-%:
	@echo "Building extension: $*"
	@bash scripts/package.sh -d extensions/$*

# Generic package target
package:
	@if [ -z "$(EXTENSION)" ]; then \
		echo "Error: EXTENSION parameter required"; \
		echo "Usage: make package EXTENSION=extension-name"; \
		exit 1; \
	fi
	@bash scripts/package.sh -d extensions/$(EXTENSION)

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@rm -rf dist/*.nep dist/checksums.txt
	@rm -f extensions/*/target/release/*.so
	@rm -f extensions/*/target/release/*.dylib
	@rm -f extensions/*/target/release/*.dll
	@rm -f extensions/*/target/wasm32-unknown-unknown/release/*.wasm
	@echo "✓ Clean complete"

# Test a package
test:
	@if [ -z "$(PACKAGE)" ]; then \
		echo "Error: PACKAGE parameter required"; \
		echo "Usage: make test PACKAGE=dist/package.nep"; \
		exit 1; \
	fi
	@echo "Testing package: $(PACKAGE)"
	@unzip -l "$(PACKAGE)"
	@echo ""
	@if command -v python3 >/dev/null 2>&1; then \
		python3 -c "import zipfile; zf = zipfile.ZipFile('$(PACKAGE)'); print('✓ Valid ZIP archive'); mf = zf.open('manifest.json'); import json; m = json.load(mf); print(f'✓ Extension: {m.get(\"name\")} ({m.get(\"version\")})')"; \
	fi

# Install locally (for testing)
install:
	@if [ -z "$(PACKAGE)" ]; then \
		echo "Error: PACKAGE parameter required"; \
		echo "Usage: make install PACKAGE=dist/package.nep"; \
		exit 1; \
	fi
	@echo "Installing $(PACKAGE) to local NeoMind..."
	@curl -X POST http://localhost:9375/api/extensions/upload/file \
		-H "Content-Type: application/octet-stream" \
		--data-binary @"$(PACKAGE)"
	@echo "✓ Install complete"

# List extensions
list:
	@echo "Available extensions:"
	@for dir in extensions/*/; do \
		if [ -f "$$dir/manifest.json" ]; then \
			name=$$(basename "$$dir"); \
			version=$$(grep '"version"' "$$dir/manifest.json" | head -1 | cut -d'"' -f4); \
			echo "  - $$name (v$$version)"; \
		fi \
	done
package-weather-forecast:
	@echo "Building weather-forecast WASM package..."
	@bash scripts/package.sh -d extensions/weather-forecast
