#!/bin/bash
# Simple test runner for Thoxa

echo "Running Thoxa tests..."
echo "======================"

# Test files that should run successfully
SUCCESS_TESTS=(
    "examples/tests/minimal.thx"
    "examples/tests/test_arithmetic_simple.thx"
)

PASSED=0
FAILED=0

for test in "${SUCCESS_TESTS[@]}"; do
    echo -n "Testing $test... "
    if uv run python -m thoxa "$test" 2>/dev/null; then
        echo "✓ PASSED"
        ((PASSED++))
    else
        echo "✗ FAILED (exit code: $?)"
        ((FAILED++))
    fi
done

echo ""
echo "Results: $PASSED passed, $FAILED failed"

if [ $FAILED -eq 0 ]; then
    echo "All tests passed!"
    exit 0
else
    echo "Some tests failed"
    exit 1
fi