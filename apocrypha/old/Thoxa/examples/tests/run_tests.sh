#!/bin/bash

# Thoxa Test Runner
# Usage: ./run_tests.sh [pattern]
# Examples:
#   ./run_tests.sh              # Run all tests
#   ./run_tests.sh arithmetic   # Run tests matching 'arithmetic'
#   ./run_tests.sh "string|array" # Run tests matching pattern

set -e

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPILER="$PROJECT_ROOT/target/release/thoxa"
TESTS_DIR="$SCRIPT_DIR"
TEMP_DIR="/tmp/thoxa_tests"
TIMEOUT=5
PATTERN="${1:-.*}"  # Default to all tests

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# Create temp directory
mkdir -p "$TEMP_DIR"

# Build compiler if needed
if [ ! -f "$COMPILER" ]; then
    echo "Building Thoxa compiler..."
    (cd "$PROJECT_ROOT" && cargo build --release)
fi

echo -e "${BOLD}Thoxa Test Suite${NC}"
echo "═══════════════════════════════════════"
echo ""

PASSED=0
FAILED=0
SKIPPED=0
FAILED_TESTS=()

# Find all test files
TEST_FILES=$(find "$TESTS_DIR" -name "test_*.thx" -type f | sort)

# Count total tests matching pattern
TOTAL=$(echo "$TEST_FILES" | xargs -n1 basename | grep -E "$PATTERN" | wc -l | tr -d ' ')

if [ "$TOTAL" -eq 0 ]; then
    echo -e "${YELLOW}No tests found matching pattern: $PATTERN${NC}"
    exit 0
fi

echo -e "Running ${CYAN}$TOTAL${NC} tests matching pattern: ${CYAN}$PATTERN${NC}"
echo ""

# Run each test
for test_path in $TEST_FILES; do
    test_file=$(basename "$test_path")
    
    # Skip if doesn't match pattern
    if ! echo "$test_file" | grep -qE "$PATTERN"; then
        continue
    fi
    
    # Extract test name from filename (remove test_ prefix and .thx suffix)
    test_name=${test_file#test_}
    test_name=${test_name%.thx}
    test_name=$(echo "$test_name" | tr '_' ' ')
    
    output_file="$TEMP_DIR/${test_file%.thx}"
    
    # Show test name (truncate if too long)
    printf "%-40s " "$test_name"
    
    # Compile the test (run from project root for proper runtime linking)
    relative_test_path="examples/tests/$test_file"
    if (cd "$PROJECT_ROOT" && timeout $TIMEOUT "$COMPILER" -c "$relative_test_path" -o "$output_file" >/dev/null 2>&1); then
        # Run the test and capture output
        if timeout $TIMEOUT "$output_file" > "$TEMP_DIR/output.txt" 2>&1; then
            EXIT_CODE=$?
            if [ $EXIT_CODE -eq 0 ]; then
                echo -e "${GREEN}✓ PASS${NC}"
                ((PASSED++))
            else
                echo -e "${RED}✗ FAIL${NC} (exit code $EXIT_CODE)"
                FAILED_TESTS+=("$test_name (runtime error)")
                ((FAILED++))
            fi
        else
            EXIT_CODE=$?
            if [ $EXIT_CODE -eq 124 ]; then
                echo -e "${RED}✗ FAIL${NC} (execution timeout)"
                FAILED_TESTS+=("$test_name (execution timeout)")
            else
                echo -e "${RED}✗ FAIL${NC} (execution error)"
                FAILED_TESTS+=("$test_name (execution error)")
            fi
            ((FAILED++))
        fi
    else
        EXIT_CODE=$?
        if [ $EXIT_CODE -eq 124 ]; then
            echo -e "${RED}✗ FAIL${NC} (compilation timeout)"
            FAILED_TESTS+=("$test_name (compilation timeout)")
        else
            # Try to get compilation error
            ERROR_MSG=$((cd "$PROJECT_ROOT" && timeout 1 "$COMPILER" -c "$relative_test_path" -o "$output_file" 2>&1 | head -1) || echo "compilation failed")
            echo -e "${RED}✗ FAIL${NC} (compilation error)"
            FAILED_TESTS+=("$test_name - $ERROR_MSG")
        fi
        ((FAILED++))
    fi
    
    # Clean up intermediate files (from project root)
    rm -f "$PROJECT_ROOT/$relative_test_path.qbe" "$PROJECT_ROOT/$relative_test_path.s" 2>/dev/null
    rm -f "$PROJECT_ROOT/examples/tests/$test_file.qbe" "$PROJECT_ROOT/examples/tests/$test_file.s" 2>/dev/null
done

# Summary
echo ""
echo "═══════════════════════════════════════"
echo -e "${BOLD}Test Results:${NC}"
echo -e "  ${GREEN}Passed:${NC}  $PASSED/$TOTAL"
echo -e "  ${RED}Failed:${NC}  $FAILED/$TOTAL"

if [ ${#FAILED_TESTS[@]} -gt 0 ]; then
    echo ""
    echo -e "${BOLD}Failed Tests:${NC}"
    for test in "${FAILED_TESTS[@]}"; do
        echo -e "  ${RED}•${NC} $test"
    done
fi

echo ""

# Clean up
rm -rf "$TEMP_DIR"

# Exit code
if [ $FAILED -gt 0 ]; then
    exit 1
else
    echo -e "${GREEN}${BOLD}All tests passed!${NC}"
    exit 0
fi