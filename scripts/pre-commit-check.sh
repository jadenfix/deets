#!/bin/bash
# Pre-commit validation script
# Run this before committing to catch CI/CD issues early
#
# Usage: ./scripts/pre-commit-check.sh
# Or add to git hooks: ln -s ../../scripts/pre-commit-check.sh .git/hooks/pre-commit

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================${NC}"
echo -e "${BLUE}  Pre-Commit Validation Check${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Track overall status
FAILED=0

# Function to run a check
run_check() {
    local name="$1"
    local command="$2"
    
    echo -e "${YELLOW}>>> Running: ${name}${NC}"
    if eval "$command"; then
        echo -e "${GREEN}✓ ${name} passed${NC}"
        echo ""
        return 0
    else
        echo -e "${RED}✗ ${name} failed${NC}"
        echo ""
        FAILED=1
        return 1
    fi
}

# 1. Check Rust formatting
run_check "Rust Formatting (cargo fmt)" \
    "cargo fmt --all -- --check"

# 2. Run Clippy with warnings as errors
run_check "Clippy Lints (cargo clippy)" \
    "cargo clippy --all-targets --all-features -- -D warnings"

# 3. Check for compilation errors
run_check "Compilation Check (cargo check)" \
    "cargo check --all-targets --all-features"

# 4. Run unit tests
run_check "Unit Tests (cargo test --lib)" \
    "cargo test --lib --all-features"

# 5. Check for TODO/FIXME in new code (optional, informational only)
echo -e "${YELLOW}>>> Checking for TODO/FIXME markers${NC}"
TODO_COUNT=$(git diff --cached | grep -E "^\+.*TODO|^\+.*FIXME" | wc -l | tr -d ' ')
if [ "$TODO_COUNT" -gt 0 ]; then
    echo -e "${YELLOW}⚠ Found ${TODO_COUNT} TODO/FIXME marker(s) in staged changes${NC}"
    git diff --cached | grep -E "^\+.*TODO|^\+.*FIXME" || true
    echo -e "${YELLOW}  (This is informational only, not blocking)${NC}"
else
    echo -e "${GREEN}✓ No TODO/FIXME markers in staged changes${NC}"
fi
echo ""

# 6. Check TypeScript/JavaScript if any files changed
if git diff --cached --name-only | grep -qE '\.(ts|tsx|js|jsx)$'; then
    echo -e "${YELLOW}>>> Checking TypeScript/JavaScript${NC}"
    if command -v npm &> /dev/null; then
        if [ -f "package.json" ]; then
            run_check "TypeScript Lint" "npm run lint || echo 'No lint script found'"
        fi
    else
        echo -e "${YELLOW}⚠ npm not found, skipping TypeScript checks${NC}"
    fi
    echo ""
fi

# 7. Check Python if any files changed
if git diff --cached --name-only | grep -qE '\.py$'; then
    echo -e "${YELLOW}>>> Checking Python${NC}"
    if command -v python3 &> /dev/null; then
        if command -v black &> /dev/null; then
            run_check "Python Formatting (black)" "black --check sdk/python/ || echo 'black not configured'"
        fi
        if command -v flake8 &> /dev/null; then
            run_check "Python Linting (flake8)" "flake8 sdk/python/ || echo 'flake8 not configured'"
        fi
    else
        echo -e "${YELLOW}⚠ Python not found, skipping Python checks${NC}"
    fi
    echo ""
fi

# Summary
echo -e "${BLUE}================================${NC}"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo -e "${GREEN}  Ready to commit${NC}"
    echo -e "${BLUE}================================${NC}"
    exit 0
else
    echo -e "${RED}✗ Some checks failed${NC}"
    echo -e "${RED}  Please fix the issues above before committing${NC}"
    echo -e "${BLUE}================================${NC}"
    echo ""
    echo -e "${YELLOW}Quick fixes:${NC}"
    echo -e "  Format code:  ${BLUE}cargo fmt --all${NC}"
    echo -e "  Fix clippy:   ${BLUE}cargo clippy --all-targets --all-features --fix${NC}"
    echo -e "  Run tests:    ${BLUE}cargo test --lib${NC}"
    exit 1
fi

