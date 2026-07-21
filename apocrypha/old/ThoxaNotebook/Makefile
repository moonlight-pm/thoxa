# Thoxa Language Makefile
# Common build, test, and development commands

.PHONY: help install test test-verbose test-all repl run clean format lint typecheck dev-install update example

# Default target - show help
help:
	@echo "Thoxa Language - Development Commands"
	@echo "====================================="
	@echo ""
	@echo "Setup & Installation:"
	@echo "  make install      - Install dependencies with uv"
	@echo "  make dev-install  - Install with development dependencies"
	@echo "  make update       - Update dependencies"
	@echo ""
	@echo "Testing:"
	@echo "  make test         - Run basic test suite"
	@echo "  make test-verbose - Run tests with verbose output"
	@echo "  make test-all     - Run all tests including disabled ones"
	@echo "  make test-file FILE=path/to/test.thx - Run specific test file"
	@echo ""
	@echo "Development:"
	@echo "  make repl         - Start the Thoxa REPL"
	@echo "  make run FILE=... - Run a Thoxa file"
	@echo "  make example      - Run example arithmetic test"
	@echo "  make debug FILE=... - Run with debug output"
	@echo "  make parser       - Regenerate parser from grammar"
	@echo "  make build        - Build Python wheel"
	@echo "  make wasm         - Build and deploy WASM to notebook"
	@echo "  make compile FILE=... - Compile a Thoxa file"
	@echo ""
	@echo "Notebook:"
	@echo "  make notebook     - Start notebook dev server"
	@echo "  make notebook-build - Build notebook for production"
	@echo "  make notebook-install - Install notebook dependencies"
	@echo "  make dev          - Full development build (parser + wasm)"
	@echo "  make quick        - Quick rebuild (parser + wasm)"
	@echo ""
	@echo "Code Quality:"
	@echo "  make format       - Format code with ruff"
	@echo "  make lint         - Check code with ruff"
	@echo "  make typecheck    - Run mypy type checking"
	@echo "  make check        - Run all checks (lint + typecheck)"
	@echo ""
	@echo "Maintenance:"
	@echo "  make clean        - Remove build artifacts and caches"
	@echo "  make clean-all    - Deep clean including .venv"

# Install dependencies
install:
	uv sync

# Install with dev dependencies
dev-install:
	uv sync --dev

# Update dependencies
update:
	uv lock --upgrade

# Run the test suite
test:
	@echo "Running Thoxa test suite..."
	@./run_tests.sh

# Run tests with verbose output
test-verbose:
	@echo "Running Thoxa tests (verbose)..."
	@for test in examples/tests/minimal.thx examples/tests/test_arithmetic_simple.thx; do \
		echo ""; \
		echo "=== Testing $$test ==="; \
		uv run python -m thoxa "$$test"; \
	done

# Run all tests including disabled ones
test-all:
	@echo "Running all Thoxa tests..."
	@passed=0; \
	failed=0; \
	for test in examples/tests/*.thx; do \
		printf "Testing %-45s " "$$test..."; \
		if uv run python -m thoxa "$$test" >/dev/null 2>&1; then \
			echo "✓ PASSED"; \
			passed=$$((passed + 1)); \
		else \
			echo "✗ FAILED"; \
			failed=$$((failed + 1)); \
		fi; \
	done; \
	echo ""; \
	echo "Results: $$passed passed, $$failed failed"

# Run a specific test file
test-file:
	@if [ -z "$(FILE)" ]; then \
		echo "Usage: make test-file FILE=path/to/test.thx"; \
		exit 1; \
	fi
	@echo "Running test: $(FILE)"
	@uv run python -m thoxa "$(FILE)"

# Start the REPL
repl:
	@echo "Starting Thoxa REPL..."
	@uv run python -m thoxa

# Run a Thoxa file
run:
	@if [ -z "$(FILE)" ]; then \
		echo "Usage: make run FILE=path/to/file.thx"; \
		exit 1; \
	fi
	@uv run python -m thoxa "$(FILE)"

# Run with debug output
debug:
	@if [ -z "$(FILE)" ]; then \
		echo "Usage: make debug FILE=path/to/file.thx"; \
		exit 1; \
	fi
	@uv run python -m thoxa --debug "$(FILE)"

# Run example
example:
	@echo "Running arithmetic test example..."
	@uv run python -m thoxa examples/tests/test_arithmetic_simple.thx

# Format code with ruff
format:
	uv run ruff format thoxa/
	uv run ruff check --fix thoxa/

# Lint code with ruff
lint:
	uv run ruff check thoxa/

# Type checking with mypy
typecheck:
	uv run mypy thoxa/ --ignore-missing-imports

# Run all checks
check: lint typecheck

# Clean build artifacts and caches
clean:
	rm -rf __pycache__
	rm -rf thoxa/__pycache__
	rm -rf .pytest_cache
	rm -rf .mypy_cache
	rm -rf .ruff_cache
	rm -rf *.egg-info
	rm -rf build/
	rm -rf dist/
	find . -type f -name "*.pyc" -delete
	find . -type f -name "*.pyo" -delete
	find . -type f -name "*~" -delete
	find . -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true

# Deep clean including virtual environment
clean-all: clean
	rm -rf .venv
	rm -rf uv.lock

# Notebook and WASM commands
parser:
	@echo "Regenerating parser from grammar..."
	@uv run python -m thoxa.parser

build:
	@echo "Building Python wheel..."
	@uv build

wasm: build
	@echo "Deploying WASM package to notebook..."
	@cp dist/thoxa-0.1.0-py3-none-any.whl notebook/public/thoxa-0.1.0-py3-none-any.whl

notebook:
	@echo "Starting notebook development server..."
	@cd notebook && bun run dev

notebook-build:
	@echo "Building notebook for production..."
	@cd notebook && bun run build

notebook-install:
	@echo "Installing notebook dependencies..."
	@cd notebook && bun install

# Full development build
dev: parser wasm
	@echo "Development build complete"

# Quick rebuild for testing
quick: parser wasm
	@echo "Quick rebuild complete"

# Compile a Thoxa file
compile:
	@if [ -z "$(FILE)" ]; then \
		echo "Usage: make compile FILE=path/to/file.thx [OUT=/tmp/output]"; \
		exit 1; \
	fi
	@OUT=$${OUT:-/tmp/output}; \
	echo "Compiling $(FILE) to $$OUT..."; \
	uv run python -m thoxa -c "$(FILE)" -o "$$OUT"

# Development shortcuts
r: run
t: test
f: format
l: lint
c: check
p: parser
w: wasm
n: notebook

# Watch for changes and re-run tests (requires entr)
watch:
	@command -v entr >/dev/null 2>&1 || { echo "entr is required for watch mode. Install with: brew install entr"; exit 1; }
	@echo "Watching for changes..."
	@find thoxa -name "*.py" | entr -c make test

# Run a simple smoke test
smoke:
	@echo "Running smoke test..."
	@echo 'println("Hello from Thoxa!");' | uv run python -m thoxa
	@echo "let x = 10 + 20; println(x);" | uv run python -m thoxa
	@echo "fn add(a, b) { a + b } println(add(5, 3));" | uv run python -m thoxa

# Show project statistics
stats:
	@echo "Thoxa Project Statistics"
	@echo "========================"
	@echo ""
	@echo "Python files:"
	@find thoxa -name "*.py" | wc -l
	@echo ""
	@echo "Lines of code:"
	@find thoxa -name "*.py" -exec wc -l {} + | tail -n 1
	@echo ""
	@echo "Test files:"
	@find examples/tests -name "*.thx" | wc -l
	@echo ""
	@echo "Enabled tests:"
	@find examples/tests -name "*.thx" ! -name "*.disabled" | wc -l
	@echo ""
	@echo "Disabled tests:"
	@find examples/tests -name "*.thx.disabled" | wc -l