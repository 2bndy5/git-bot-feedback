# Agent Guidelines for git-bot-feedback

This document provides conventions, architectural context, and workflow commands for AI coding agents operating on this repository.

## Project Overview

`git-bot-feedback` is a Rust library designed for CI workflows and automation tools to provide feedback on git hosting platforms:
- **Supported git servers**: GitHub, GitLab (>= v17.9), Gitea, Forgejo (BitBucket planned).
- **Core capabilities**: Thread comments (PR and commit), CI step summaries, output variables, log grouping, file annotations, Pull Request reviews, and changed file analysis.
- **Multi-language project**: Rust crate at workspace root (`git-bot-feedback`) and Python bindings in `bindings/python` (`git-bot-feedback-py` built with PyO3/Maturin).
- **License**: LGPL-3.0-or-later.

## Critical Invariants & Rules

### 1. Test Execution Rule: Never Run `cargo test` Directly
> [!IMPORTANT]
> Do **not** run raw `cargo test` for unit or integration tests!
> The test suite relies heavily on manipulating environment variables. `cargo test` executes tests concurrently within the same process space, causing severe cross-test pollution and intermittent failures.
> Always execute tests through `cargo-nextest` (or `nur test`), which provides process isolation for each individual test.

Note, this project is primarily hosted on GitHub; any mirrors on other platforms are considered unofficial. Therefore, the CI workflows and tests all are designed for GitHub Actions.

### 2. Line Endings & Whitespace
- **Line endings**: All files must strictly use LF (`\n`), never CRLF (`\r\n`).
- **Trailing whitespace**: No trailing whitespace permitted.
- **End of file**: Every file must terminate with a single trailing newline.
- **File size**: Do not commit large files (> 500 kB).

### 3. Commit Messages and Pull Request Titles
- Must strictly conform to [Conventional Commits](https://www.conventionalcommits.org) specifications.
- Allowed commit types (enforced in `.config/committed.toml`):
  `fix`, `feat`, `build`, `chore`, `docs`, `style`, `refactor`, `remove`, `deprecate`, `security`, `add`, `perf`, `test`.
- **Subject casing**: Must start with a lowercase letter (`subject_capitalized = false`).
- Pull request titles are validated in CI against Conventional Commits via `committed` and spell-checked via CSpell.
- Feature branches are squashed into a single commit upon merge into `main` so `git-cliff` can generate release notes and `CHANGELOG.md`.

### 4. Spelling & Terminology
- CI enforces CSpell across all project files (`cspell.config.yml`).
- If you introduce new technical terms, acronyms, or crate names, verify they do not fail spell checking or add them to `cspell.config.yml`.

## Task Runner: `nur`

This project uses [nur](https://crates.io/crates/nur) (a task runner using Nushell syntax) defined in `nurfile`. Prefer `nur` tasks over composite manual commands.

| Task | Command | Description |
| :--- | :--- | :--- |
| **Run Tests** | `nur test` | Runs tests with `cargo-nextest` under the `default` profile (marks tests > 10s as "SLOW"). |
| **Run CI Tests** | `nur test -p ci` | Runs all tests including slow tests with verbose output. |
| **Run Doc Tests** | `nur test docs` | Runs doc tests (`cargo test --doc --features file-changes`). |
| **Generate Coverage** | `nur test lcov` / `nur test html` | Generates LCOV or HTML coverage via `cargo-llvm-cov`. |
| **Lint & Format Check** | `nur lint --check` | Runs `cargo clippy --workspace --features file-changes --all-targets -- -D warnings` and `cargo fmt -- --check`. |
| **Auto-Fix Lint & Fmt** | `nur lint` | Applies clippy auto-fixes and formats code with `cargo fmt`. |
| **Pre-Commit Checks** | `nur pre-commit` | Runs pre-commit hooks via `uvx` on all files. |
| **Build Examples** | `nur examples` | Builds all examples in `examples/`. |
| **Build Docs** | `nur docs` | Builds Rust documentation (`cargo doc --no-deps --lib --features file-changes`). |
| **Build Python Docs**| `nur docs py` | Builds Python documentation using Sphinx and `uv`. |

### Fallback Commands (if `nur` is unavailable)
- **Run tests**:
  Without coverage measurements:
  ```shell
  cargo nextest run --features "test-skip-wait-for-rate-limit,file-changes" --profile default
  ```

  With coverage measurements:
  ```shell
  cargo llvm-cov --no-report --features "test-skip-wait-for-rate-limit,file-changes" nextest --profile default
  ```

  To generate a coverage report:
  ```shell
  cargo llvm-cov report --lcov --output-path lcov.info
  # or for json output
  cargo llvm-cov report --json --output-path coverage.json
  ```
  Git ignores both lcov.info and coverage.json files.

  Use `cargo llvm-cov clean` to flush previous coverage measurements, if fresh coverage report is needed. This will invoke a fresh compile when running the test suite.
- **Lint check**:
  ```shell
  cargo clippy --workspace --features file-changes --all-targets -- -D warnings
  cargo fmt -- --check
  ```
- **Pre-commit**:
  ```shell
  uvx pre-commit run --all-files
  ```

## CI Pipeline Overview

Automated GitHub Actions workflows run on pushes and PRs to `main`:
1. **`pre-commit.yml`**:
   - `all-files`: Runs pre-commit hooks (whitespace, LF line endings, YAML/TOML syntax, file size limits, CSpell, Ruff, Mypy).
   - `lint-rust`: Runs `nur lint --check` (`cargo clippy` with `-D warnings` and `cargo fmt -- --check`).
   - `check-pr-title`: Verifies PR title with `committed` and `cspell-cli`.
   - `lint-ci`: Checks GitHub Actions workflows with `zizmor`.
2. **`rust.yml`**:
   - Runs `nur test docs` (documentation examples).
   - Runs `nur examples` (compiles all examples).
   - Runs `nur test --profile ci` (runs nextest test suite under CI profile).
   - Generates and uploads `lcov.info` coverage report to Codecov.
   - Publishes to crates.io upon tagged release (`git-bot-feedback/v*`).
3. **`docs.yml`**:
   - Builds Python Sphinx docs and deploys to GitHub Pages on `main`.
   - Builds Rustdoc documentation and uploads artifacts.
4. **`python.yml`**:
   - Compiles and packages Python wheels across platforms (Linux, macOS, Windows) and architectures using `maturin`.
   - Validates distribution packages with `twine check`.
   - Publishes to PyPI upon tagged release (`git-bot-feedback-py/v*`).
5. **`bump-n-release.yml`**:
   - Automated version bumping and release orchestration script (`bump-n-release.nu`).

## Cargo Features & TLS Architecture

- **Default features**: `["github", "gitea", "gitlab"]`.
- **Optional features**:
  - `file-changes`: Enables git diff inspection (`fast-glob`, `regex`).
  - `pyo3`: Enables Python binding support.
  - `custom-git-server-impl`: Silences compiler error when git server implementations are disabled.
  - `test-skip-wait-for-rate-limit`: Dev-only feature to bypass rate-limit wait timeouts in unit tests.
- **TLS backend**: The library intentionally does not specify a TLS backend for `reqwest` so consumers can select their preferred backend. For dev/testing workflows, `reqwest` dev-dependencies enable `features = ["default-tls"]`.

## Repository Structure

- `src/`: Core library implementation.
  - `client/`: Git server client implementations (`github/`, `gitlab/`, `gitea/`, and common traits).
  - `types.rs`, `lib.rs`: Public APIs, data models, and error types.
- `tests/`: Integration tests using `cargo-nextest`, `mockito`, and `tempfile`.
- `examples/`: Example executables demonstrating library usage.
- `bindings/python/`: Python bindings using PyO3 and Maturin (`git-bot-feedback-py`).
- `docs/`: Sphinx documentation sources for Python and web guides.
- `.config/`: Configuration files for `nextest.toml`, `committed.toml`, and `cliff.toml`.
- `nurfile`: Nushell-based task runner definitions.
- `.pre-commit-config.yaml`: Pre-commit hook definitions.
- `cspell.config.yml`: Spelling dictionary configuration.
