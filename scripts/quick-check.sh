#!/bin/bash
# Quick pre-commit check (formatting + clippy only)
# For a full check, use ./scripts/pre-commit-check.sh

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Running quick checks (format + clippy)...${NC}\n"

# Format check
echo -e "${YELLOW}1/2 Checking formatting...${NC}"
if cargo fmt --all -- --check; then
    echo -e "${GREEN}✓ Formatting OK${NC}\n"
else
    echo -e "${RED}✗ Formatting issues found${NC}"
    echo -e "${YELLOW}Fix with: cargo fmt --all${NC}\n"
    exit 1
fi

# Clippy check
echo -e "${YELLOW}2/2 Running clippy...${NC}"
if cargo clippy --all-targets --all-features -- -D warnings; then
    echo -e "${GREEN}✓ Clippy OK${NC}\n"
else
    echo -e "${RED}✗ Clippy issues found${NC}"
    echo -e "${YELLOW}Fix with: cargo clippy --all-targets --all-features --fix${NC}\n"
    exit 1
fi

echo -e "${GREEN}✓ All quick checks passed!${NC}"

