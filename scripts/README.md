# Pre-Commit Validation Scripts

This directory contains scripts to validate code quality before pushing to GitHub, catching CI/CD issues locally.

## Quick Start

```bash
# Quick check (format + clippy, ~10s)
make check
# or
./scripts/quick-check.sh

# Full pre-commit validation (format + clippy + tests, ~60s)
make pre-commit
# or
./scripts/pre-commit-check.sh

# Auto-fix issues
make fix
```

## Scripts

### `quick-check.sh` ⚡
**Purpose**: Fast pre-commit validation  
**Runtime**: ~10-15 seconds  
**Checks**:
- ✓ Rust formatting (`cargo fmt`)
- ✓ Clippy lints (`cargo clippy`)

**When to use**: Before every commit

```bash
./scripts/quick-check.sh
```

### `pre-commit-check.sh` 🔍
**Purpose**: Comprehensive pre-commit validation  
**Runtime**: ~60-90 seconds  
**Checks**:
- ✓ Rust formatting (`cargo fmt`)
- ✓ Clippy lints with warnings as errors
- ✓ Compilation check
- ✓ Unit tests (`cargo test --lib`)
- ✓ TODO/FIXME detection (informational)
- ✓ TypeScript/JavaScript linting (if files changed)
- ✓ Python linting (if files changed)

**When to use**: Before pushing to remote

```bash
./scripts/pre-commit-check.sh
```

## Makefile Targets

```bash
# Quick validation (format + clippy)
make check

# Full validation (format + clippy + tests)
make pre-commit

# Auto-fix formatting and clippy issues
make fix

# Just format code
make fmt

# Just run lints
make lint
```

## CI/CD Integration

These scripts mirror the checks performed by `.github/workflows/ci.yml`:

1. **Formatting Check**: `cargo fmt --all -- --check`
2. **Clippy**: `cargo clippy --all-targets --all-features -- -D warnings`
3. **Tests**: `cargo test --all-features --workspace`

By running these scripts locally, you catch issues before they fail in CI/CD.

## Git Hooks (Optional)

To automatically run checks before every commit:

```bash
# Create symlink to use as git hook
ln -s ../../scripts/quick-check.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

To use the full validation instead:

```bash
ln -s ../../scripts/pre-commit-check.sh .git/hooks/pre-commit
```

## Exit Codes

- **0**: All checks passed ✓
- **1**: One or more checks failed ✗

## Example Output

### Success ✓
```
================================
  Pre-Commit Validation Check
================================

>>> Running: Rust Formatting (cargo fmt)
✓ Rust Formatting (cargo fmt) passed

>>> Running: Clippy Lints (cargo clippy)
✓ Clippy Lints (cargo clippy) passed

================================
✓ All checks passed!
  Ready to commit
================================
```

### Failure ✗
```
================================
  Pre-Commit Validation Check
================================

>>> Running: Rust Formatting (cargo fmt)
✗ Rust Formatting (cargo fmt) failed

>>> Running: Clippy Lints (cargo clippy)
✗ Clippy Lints (cargo clippy) failed

================================
✗ Some checks failed
  Please fix the issues above before committing
================================

Quick fixes:
  Format code:  cargo fmt --all
  Fix clippy:   cargo clippy --all-targets --all-features --fix
  Run tests:    cargo test --lib
```

## Tips

1. **Run `make check` frequently** - It's fast (~10s) and catches most issues
2. **Run `make pre-commit` before pushing** - Ensures all tests pass
3. **Use `make fix`** - Auto-fixes most formatting and clippy issues
4. **Set up git hooks** - Never commit broken code again

## Troubleshooting

### Script won't run
```bash
# Make sure scripts are executable
chmod +x scripts/*.sh
```

### Clippy issues
```bash
# Auto-fix most clippy issues
make fix
```

### Formatting issues
```bash
# Auto-format all code
make fmt
```

### Test failures
```bash
# Run tests with output
cargo test --lib -- --nocapture
```

## What Gets Checked

### ✓ Rust Code
- Formatting (rustfmt)
- Lints (clippy)
- Compilation (cargo check)
- Unit tests (cargo test --lib)

### ✓ TypeScript/JavaScript (if present)
- npm lint scripts

### ✓ Python (if present)
- black formatting
- flake8 linting

## Performance

| Script | Checks | Runtime | When to Use |
|--------|--------|---------|-------------|
| `quick-check.sh` | Format + Clippy | ~10s | Before every commit |
| `pre-commit-check.sh` | Format + Clippy + Tests | ~60s | Before pushing |

## Contributing

To add new checks:

1. Edit `scripts/pre-commit-check.sh`
2. Add check using `run_check` function:
   ```bash
   run_check "Check Name" "command to run"
   ```
3. Test it: `./scripts/pre-commit-check.sh`
4. Update this README

## See Also

- `.github/workflows/ci.yml` - GitHub Actions CI/CD pipeline
- `Makefile` - Build and validation targets
- `CONTRIBUTING.md` - Contribution guidelines

