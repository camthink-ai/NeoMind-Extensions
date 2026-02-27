.PHONY: help build clean install

# Default target
help:
	@echo "NeoMind Extensions V2 - Build Commands"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build           - Build all V2 extensions and create .nep packages"
	@echo "  build-debug     - Build in debug mode (no .nep packages)"
	@echo "  install         - Install extensions locally"
	@echo "  clean           - Remove build artifacts"
	@echo ""
	@echo "Examples:"
	@echo "  make build"
	@echo "  make install"

# Build all V2 extensions with .nep packages
build:
	@bash build.sh --skip-install

# Build in debug mode
build-debug:
	@bash build.sh --debug --skip-package

# Install extensions locally
install:
	@bash build.sh --yes --skip-frontend

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@rm -rf target/release target/debug
	@rm -rf dist/*.nep dist/checksums.txt
	@rm -rf extensions/*/node_modules extensions/*/frontend/node_modules
	@rm -rf extensions/*/frontend/dist
	@echo "✓ Clean complete"

# Deep clean
clean-all:
	@echo "Deep cleaning..."
	@rm -rf target
	@rm -rf dist
	@rm -rf extensions/*/node_modules extensions/*/frontend/node_modules
	@rm -rf extensions/*/frontend/dist
	@cargo clean 2>/dev/null || true
	@echo "✓ Deep clean complete"

# Run tests
test:
	@echo "Running tests..."
	cargo test --release
	@echo "✓ Tests complete"