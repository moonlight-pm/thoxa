# Makefile for Thoxa compiler

.PHONY: all build runtime clean test

all: build runtime

build:
	cargo build --release

runtime:
	mkdir -p target/runtime
	cc -c runtime/thoxa_runtime.c -o target/runtime/thoxa_runtime.o
	cc -c runtime/thoxa_memory.c -o target/runtime/thoxa_memory.o
	cc -c runtime/thoxa_trial.c -o target/runtime/thoxa_trial.o
	cc -c runtime/thoxa_box.c -o target/runtime/thoxa_box.o
	cc -c runtime/thoxa_box_unified.c -o target/runtime/thoxa_box_unified.o
	cc -c runtime/thoxa_types.c -o target/runtime/thoxa_types.o
	cc -c runtime/thoxa_string.c -o target/runtime/thoxa_string.o
	cc -c runtime/thoxa_error.c -o target/runtime/thoxa_error.o
	cc -c runtime/thoxa_may.c -o target/runtime/thoxa_may.o
	cc -c runtime/thoxa_union.c -o target/runtime/thoxa_union.o

test: build runtime
	cargo test
	@echo "Running Thoxa integration tests..."
	@cd examples/tests && ./run_tests.sh

clean:
	@echo "Cleaning up temporary files..."
	@find . -name "*.qbe" -type f -delete 2>/dev/null || true
	@find . -name "*.s" -type f -not -path "./runtime/*" -delete 2>/dev/null || true
	@find . -name "*.o" -type f -not -path "./runtime/*" -not -path "./target/*" -delete 2>/dev/null || true
	@rm -f /tmp/test_* 2>/dev/null || true
	@rm -f test_*.rs test_*.thx 2>/dev/null || true
	@echo "Clean complete"

clean-all: clean
	cargo clean
	rm -rf target/runtime
	@echo "Full clean complete"

install: build runtime
	cp target/release/thoxa /usr/local/bin/
	mkdir -p /usr/local/lib/thoxa
	cp target/runtime/*.o /usr/local/lib/thoxa/

examples: build runtime
	./target/release/thoxa -c examples/hello_world.thx -o tmp/hello_world
	./target/release/thoxa -c examples/print_demo.thx -o tmp/print_demo
	@echo "Built examples in tmp/"

# Compile a specific .thx file (usage: make compile FILE=examples/test.thx)
compile: build runtime
	./target/release/thoxa -c $(FILE) -o /tmp/test

# Compile and run a .thx file (usage: make run FILE=examples/test.thx)
run: build runtime
	./target/release/thoxa -c $(FILE) -o /tmp/test
	/tmp/test

# Debug compile with logging (usage: make debug FILE=examples/test.thx)
debug: build runtime
	RUST_LOG=debug timeout 2 ./target/release/thoxa -c $(FILE) -o /tmp/test

# Trace compile with full logging (usage: make trace FILE=examples/test.thx)
trace: build runtime  
	RUST_LOG=trace timeout 2 ./target/release/thoxa -c $(FILE) -o /tmp/test

# Show QBE output (usage: make qbe FILE=examples/test.thx)
qbe: build runtime
	SHOW_QBE=1 ./target/release/thoxa -c $(FILE) -o /tmp/test