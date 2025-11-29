# Contributing to ToolR

First off, thank you for considering contributing to ToolR! It's people like you that make ToolR such a great tool.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [How to Contribute](#how-to-contribute)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Commit Message Guidelines](#commit-message-guidelines)
- [Security](#security)
- [Development Workflow](#development-workflow)
- [Code Review Guidelines](#code-review-guidelines)

## Code of Conduct

This project and everyone participating in it is governed by respect, professionalism, and inclusivity.
By participating, you are expected to uphold these values. Please report unacceptable behavior by opening
a [GitHub Issue](https://github.com/s0undt3ch/ToolR/issues/new) or contacting the project maintainers.

### Our Standards

**Positive behaviors include:**

- Using welcoming and inclusive language
- Being respectful of differing viewpoints and experiences
- Gracefully accepting constructive criticism
- Focusing on what is best for the community
- Showing empathy towards other community members

**Unacceptable behaviors include:**

- Trolling, insulting/derogatory comments, and personal or political attacks
- Public or private harassment
- Publishing others' private information without explicit permission
- Other conduct which could reasonably be considered inappropriate in a professional setting

## Getting Started

### Prerequisites

Before contributing, make sure you have:

- **Python 3.11+** installed
- **Rust toolchain** (for building native extensions)
- **mise** - Development environment manager
- **gh** CLI (optional, for GitHub Actions utilities)
- **git** for version control

### First-time Contributors

New to open source? Here are some resources:

- [How to Contribute to Open Source](https://opensource.guide/how-to-contribute/)
- [First Timers Only](https://www.firsttimersonly.com/)
- [GitHub's Guide to Contributing](https://guides.github.com/activities/contributing-to-open-source/)

Look for issues labeled [`good first issue`](https://github.com/s0undt3ch/ToolR/labels/good%20first%20issue)
or [`help wanted`](https://github.com/s0undt3ch/ToolR/labels/help%20wanted).

## Development Setup

### 1. Fork and Clone

```bash
# Fork the repository on GitHub, then clone your fork
git clone https://github.com/YOUR_USERNAME/ToolR.git
cd ToolR

# Add upstream remote
git remote add upstream https://github.com/s0undt3ch/ToolR.git
```

### 2. Install mise

[mise](https://mise.jdx.dev/) manages our development environment, including Python, Rust, and other tools
(see <https://mise.jdx.dev/getting-started.html>).

```bash
curl https://mise.run | sh

# Install project tools
mise install

# Activate the environment
mise activate
```

This will automatically:

- Install the correct Python version
- Install the Rust toolchain
- Install uv package manager
- Set up other project dependencies

### 3. Install Dependencies

```bash
# Sync Python dependencies and create virtual environment
uv sync --all-extras --dev
```

### 4. Install prek (Pre-commit Hooks)

We use [prek](https://github.com/j178/prek) instead of standard pre-commit:

```bash
# Install prek hooks
prek install --install-hooks
```

### 5. Verify Setup

```bash
# Run tests
uv run pytest

# Run all pre-commit checks
prek run --all-files
```

## How to Contribute

### Reporting Bugs

GitHub Issues are used for bug tracking. Before creating a bug report:

- **Search existing issues** to avoid duplicates
- **Use the latest version** of ToolR
- **Collect information** about your environment

When filing a bug report, include:

- **Clear title and description**
- **Steps to reproduce** the issue
- **Expected vs actual behavior**
- **Environment details** (OS, Python version, ToolR version)
- **Error messages** (full stack traces)
- **Screenshots** if applicable

[Create a new issue](https://github.com/s0undt3ch/ToolR/issues/new) to report bugs.

### Suggesting Enhancements

Feature requests are also tracked as GitHub Issues. When creating an enhancement suggestion:

- **Use a clear title** describing the enhancement
- **Provide a detailed description** of the proposed functionality
- **Explain why this would be useful** to most ToolR users
- **List examples** of how the enhancement would be used
- **Note any alternatives** you've considered

[Create a new issue](https://github.com/s0undt3ch/ToolR/issues/new) to suggest features.

### Pull Requests

GitHub Pull Requests are used for code contributions. Follow the [pull request process](#pull-request-process) below.

### Your First Code Contribution

1. **Find an issue** to work on (or create one)
2. **Comment** on the issue to let others know you're working on it
3. **Create a branch** from `main`:

   ```bash
   git checkout -b feature/your-feature-name
   # or
   git checkout -b fix/your-bug-fix
   ```

4. **Make your changes** following our coding standards
5. **Write/update tests** to cover your changes
6. **Run the test suite** to ensure nothing breaks
7. **Commit your changes** with descriptive messages
8. **Push to your fork** and submit a pull request

## Pull Request Process

### Before Submitting

- [ ] **Run all tests** and ensure they pass
- [ ] **Run prek** to check linting, formatting, and types
- [ ] **Update documentation** if needed
- [ ] **Add tests** for new functionality
- [ ] **Ensure commits** follow our commit message guidelines
- [ ] **Rebase on latest main** if needed

**Note:** Do NOT manually update `CHANGELOG.md` - it's automatically generated from commit messages using [git-cliff](https://git-cliff.org/).

### Submitting

1. **Push your branch** to your fork
2. **Open a pull request** against the `main` branch
3. **Write a clear PR description**:
   - What changes were made
   - Why the changes are needed
   - Any breaking changes
4. **Link related issues** (e.g., "Closes #123")
5. **Respond to feedback** and update as needed

### PR Review Process

- Maintainers will review your PR within a few days
- You may be asked to make changes
- Once approved, maintainers will merge your PR
- Your contribution will be included in the next release!

### CI Checks

All PRs must pass:

- ✅ **Tests** (pytest on Linux/macOS/Windows)
- ✅ **Linting** (ruff, via prek)
- ✅ **Type checking** (mypy, via prek)
- ✅ **Formatting** (ruff format, via prek)
- ✅ **Rust checks** (cargo clippy, cargo check, via prek)
- ✅ **GitHub Actions** (actionlint, via prek)
- ✅ **Shell scripts** (shellcheck, via prek)
- ✅ **Security** (CodeQL, dependency review)
- ✅ **Spelling** (codespell, typos, via prek)
- ✅ **Markdown** (markdownlint, via prek)
- ✅ **Documentation** (builds without errors)

All these checks run automatically via prek hooks and CI.

## Coding Standards

### Python Code

- **Follow PEP 8** for style (enforced by ruff)
- **Use type hints** for all function signatures
- **Use `from __future__ import annotations`** at the top of files
- **Maximum line length**: 120 characters
- **Use ruff** for linting and formatting (automated by prek)
- **Use mypy** for type checking (automated by prek)

```python
from __future__ import annotations

from typing import Any

def greet(name: str, *, enthusiastic: bool = False) -> str:
    """Greet someone by name.

    Args:
        name: The name of the person to greet.
        enthusiastic: Whether to add extra enthusiasm.

    Returns:
        A greeting message.
    """
    greeting = f"Hello, {name}"
    return f"{greeting}!" if enthusiastic else greeting
```

### Rust Code

- **Follow Rust style guidelines**
- **Use `cargo fmt`** for formatting (automated by prek)
- **Use `cargo clippy`** for linting (automated by prek)
- **Write doc comments** for public APIs
- **Add tests** for new functionality

```rust
/// Greet someone by name.
///
/// # Arguments
///
/// * `name` - The name of the person to greet
///
/// # Returns
///
/// A greeting message
pub fn greet(name: &str) -> String {
    format!("Hello, {}", name)
}
```

### Documentation

- **Use Google-style docstrings** for Python
- **Use Rustdoc** for Rust code
- **Include examples** in docstrings
- **Update docs/** when adding features
- **Keep README.md** up to date

### Pre-commit Hooks

All formatting and linting is handled automatically by prek:

```bash
# Run all checks
prek run --all-files

# Run specific hook
prek run ruff-format

# Bypass hooks (use sparingly!)
git commit --no-verify
```

## Testing Guidelines

### Writing Tests

- **Write tests** for all new functionality
- **Update tests** when changing behavior
- **Test edge cases** and error conditions
- **Use descriptive test names**
- **Keep tests focused** (one concept per test)

### Running Tests

```bash
# Run all tests
uv run pytest

# Run specific test file
uv run pytest tests/test_version.py

# Run with coverage
uv run coverage run -m pytest
uv run coverage report

# Run Rust tests
cargo test

# Run specific Rust test
cargo test test_name
```

### Test Organization

```text
tests/
├── cli/                 # CLI argument parsing tests
├── context/             # Context and runtime tests
├── parser/              # Parser tests
├── registry/            # Command discovery and registry tests
├── utils/               # Utility function tests
├── support/             # Test fixtures and support files
├── conftest.py          # Shared fixtures
└── test_*.py            # Additional test modules
```

### Property-Based Testing

ToolR uses [Hypothesis](https://hypothesis.works/) for property-based testing (fuzzing):

```python
from hypothesis import given
from hypothesis import strategies as st

@given(st.text())
def test_handles_any_input(text: str) -> None:
    """Test that the function handles any text input."""
    result = process_text(text)
    assert isinstance(result, str)
```

## Commit Message Guidelines

We follow [Conventional Commits](https://www.conventionalcommits.org/) for clear commit history and automated changelogs.

### Format

```commit
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style changes (formatting, etc.)
- **refactor**: Code refactoring
- **perf**: Performance improvements
- **test**: Adding or updating tests
- **build**: Build system changes
- **ci**: CI configuration changes
- **chore**: Other changes (dependencies, etc.)

### Examples

```bash
feat(cli): add --verbose flag for detailed output

Add a --verbose flag to enable detailed logging output.
This helps users debug issues with command execution.

Closes #123

---

fix(version): correct git describe pattern for multi-digit versions

The pattern v[0-9].[0-9].[0-9] only matched single digits.
Changed to v[0-9]*.[0-9]*.[0-9]* to support versions like v0.11.0.

Fixes #456

---

docs: update contributing guidelines with commit conventions
```

### Breaking Changes

For breaking changes, add `BREAKING CHANGE:` in the footer:

```bash
feat(api)!: remove deprecated get_version function

BREAKING CHANGE: The get_version() function has been removed.
Use version.current() instead.
```

### Why Conventional Commits?

- **Automated changelog** generation using git-cliff
- **Semantic versioning** automation
- **Clear history** for understanding project evolution
- **Better collaboration** through standardized messages

## Security

### Reporting Vulnerabilities

**DO NOT** open a public issue for security vulnerabilities.

Instead, use [GitHub Security Advisories](https://github.com/s0undt3ch/ToolR/security/advisories/new) to privately report security issues.

See our [Security Policy](https://github.com/s0undt3ch/ToolR/security/policy) for more details.

### Security Best Practices

When contributing:

- **Never commit secrets** (API keys, passwords, tokens)
- **Pin action versions** to commit SHAs in workflows
- **Validate user input** to prevent injection attacks
- **Use secure defaults** in configurations
- **Review dependencies** for known vulnerabilities

## Development Workflow

### Branch Naming

- `feature/description` - New features
- `fix/description` - Bug fixes
- `docs/description` - Documentation updates
- `refactor/description` - Code refactoring
- `test/description` - Test additions/updates

### Daily Development

```bash
# Update your fork
git checkout main
git pull upstream main
git push origin main

# Create feature branch
git checkout -b feature/my-feature

# Make changes, commit often
git add .
git commit -m "feat: add my feature"

# Run checks before pushing
prek run --all-files

# Push to your fork
git push origin feature/my-feature

# Open a pull request on GitHub
```

### Keeping Your Branch Updated

```bash
# Fetch latest changes
git fetch upstream

# Rebase your branch
git rebase upstream/main

# Force push if already pushed
git push --force-with-lease origin feature/my-feature
```

## Code Review Guidelines

### For Contributors

- **Respond promptly** to review feedback
- **Ask questions** if feedback is unclear
- **Make requested changes** or explain why not
- **Mark conversations** as resolved when addressed
- **Be respectful** and professional

### For Reviewers

- **Review thoroughly** but constructively
- **Provide specific feedback
