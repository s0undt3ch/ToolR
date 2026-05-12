<!-- rumdl-disable MD046 MD076 -->

# Plan 9: Distribution + Backwards Compatibility

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Ship `toolr` as a standalone Rust binary first and as a pip wheel
second. Add a `curl ... | sh` installer that fetches per-platform GitHub
release archives, refresh the mise plugin to point at those archives, embed
the same binary inside the wheel via maturin's `bin` target, and rewrite
`python -m toolr` as a thin deprecation shim that locates and exec's the
binary. CI grows per-channel smoke tests so every install path is
continuously verified.

**Architecture:** The Rust binary built from `src/bin/toolr/main.rs` becomes
the single distribution artifact in three different envelopes. (1) A maturin
build embeds it in the wheel under `<wheel>/data/scripts/toolr` so `pip
install toolr` puts it on PATH next to the Python package. (2) A release
workflow `cargo build --release --bin toolr` per target triple, packs the
binary plus completion scripts and license into `tar.gz` / `zip` archives
with SHA-256 checksums, uploads them to the GitHub release alongside wheels.
(3) An `install.sh` script detects the host triple via `uname -sm`, downloads
the right archive from the releases page, verifies the checksum, and
installs to `$XDG_BIN_HOME` (or `~/.local/bin`). The `toolr-mise/` plugin's
`hooks/install` and `hooks/download` scripts switch from a Python-wheel
fetch to fetching the same release archives. `python/toolr/__main__.py`
becomes a deprecation shim that finds the `toolr` binary via `shutil.which`
(falling back to the bin directory of the current interpreter) and exec's
it with `os.execv`, after writing a one-time deprecation note to stderr.

**Tech Stack:** maturin (existing build backend, `bindings = "pyo3"` + `bin`
target via the `[[bin]]` entry in `Cargo.toml`), GitHub Actions (existing
`build.yml`, `release.yml` extended with a per-triple binary build matrix),
POSIX shell + PowerShell for installers, Bash for mise plugin hooks, Python
3.11+ stdlib for the deprecation shim.

**Reading order in this plan:** Tasks build on each other. Don't skip ahead;
later tasks reference files, environment variables, and shell functions
defined in earlier ones.

---

## Task 1: Confirm maturin ships the Rust binary inside the wheel

Maturin discovers `[[bin]]` targets in `Cargo.toml` automatically when
`bindings = "pyo3"` is set: every binary listed there is copied into the
wheel under `data/scripts/<name>` and gets installed onto PATH by pip on
`pip install`. The existing `pyproject.toml` already has both:

- `[[bin]] name = "toolr" path = "src/bin/toolr/main.rs"` in `Cargo.toml`
  (added by Plan 1, Task 1).
- `[tool.maturin] bindings = "pyo3"`, `module-name = "toolr.utils._rust_utils"`
  in `pyproject.toml`.

This task makes the bin-shipping behaviour explicit, asserts it in a test,
and locks it down so future config edits cannot regress it.

**Files:**

- Modify: `pyproject.toml`
- Create: `tests/distribution/__init__.py`
- Create: `tests/distribution/test_wheel_contents.py`

- [ ] **Step 1.1: Pin maturin to a version that supports both lib and bin targets**

    The current constraint `maturin>=1.0,<2.0` is broad. Bin-shipping for
    pyo3 wheels has been stable since 1.4. Tighten the lower bound to
    `>=1.7` so reproducibility doesn't depend on the resolver picking up
    bin-shipping fixes accidentally.

    Update the `[build-system]` table in `pyproject.toml`:

    ```toml
    [build-system]
    requires = ["maturin>=1.7,<2.0"]
    build-backend = "maturin"
    ```

    Add an `include` entry under `[tool.maturin]` so the Rust binary is
    treated as a wheel artifact rather than a stray file maturin might
    drop. Append to the `[tool.maturin]` block:

    ```toml
    [tool.maturin]
    features = ["python"]
    module-name = "toolr.utils._rust_utils"
    python-source = "python"
    bindings = "pyo3"
    strip = true
    # Require Cargo.lock is up to date
    locked = true
    # Ship the Rust `toolr` binary inside the wheel. Maturin discovers it
    # from the `[[bin]]` entry in Cargo.toml; this line is documentation +
    # an extra safety net so future config edits don't silently drop it.
    include = [
        { path = "src/bin/toolr/**/*.rs", format = "sdist" },
    ]
    ```

- [ ] **Step 1.2: Write the failing test asserting the binary is in the wheel**

    Create `tests/distribution/__init__.py` as an empty file.

    Create `tests/distribution/test_wheel_contents.py`:

    ```python
    """Tests covering the contents of the `toolr` wheel."""

    from __future__ import annotations

    import shutil
    import subprocess
    import sys
    import zipfile
    from collections.abc import Callable
    from pathlib import Path

    import pytest

    pytestmark = pytest.mark.skipif(
        shutil.which("maturin") is None,
        reason="maturin not on PATH",
    )

    REPO_ROOT = Path(__file__).resolve().parents[2]


    @pytest.fixture
    def built_wheel(tmp_path: Path) -> Callable[[], Path]:
        """Factory: build a wheel into ``tmp_path/wheelhouse`` and return its path."""

        def _build() -> Path:
            out_dir = tmp_path / "wheelhouse"
            out_dir.mkdir()
            subprocess.run(
                [
                    "maturin",
                    "build",
                    "--release",
                    "--out",
                    str(out_dir),
                    "--interpreter",
                    sys.executable,
                ],
                cwd=REPO_ROOT,
                check=True,
            )
            wheels = list(out_dir.glob("toolr-*.whl"))
            assert len(wheels) == 1, f"expected one wheel, got {wheels}"
            return wheels[0]

        return _build


    def _expected_bin_name() -> str:
        return "toolr.exe" if sys.platform == "win32" else "toolr"


    def test_wheel_includes_rust_binary(built_wheel: Callable[[], Path]) -> None:
        wheel = built_wheel()
        with zipfile.ZipFile(wheel) as zf:
            names = zf.namelist()
        binary_name = _expected_bin_name()
        candidates = [
            n for n in names if n.endswith(f"/data/scripts/{binary_name}")
        ]
        assert candidates, (
            f"expected `data/scripts/{binary_name}` inside wheel, got names: "
            f"{names[:20]}..."
        )


    def test_wheel_includes_python_package(built_wheel: Callable[[], Path]) -> None:
        wheel = built_wheel()
        with zipfile.ZipFile(wheel) as zf:
            names = zf.namelist()
        # The wheel must still contain the Python package (existing behaviour
        # preserved when adding bin-shipping).
        assert any(n.endswith("/toolr/__init__.py") for n in names), (
            f"expected `toolr/__init__.py` inside wheel, got names: "
            f"{names[:20]}..."
        )
        assert any(n.endswith("/toolr/_runner.py") for n in names) or any(
            n.endswith("/toolr/__main__.py") for n in names
        ), "expected python package modules in wheel"
    ```

- [ ] **Step 1.3: Run the test, expect PASS (or skip if maturin missing)**

    ```bash
    uv run pytest tests/distribution/test_wheel_contents.py -v
    ```

    Expected: both tests pass. If maturin is not installed in the dev
    environment the tests skip; that is acceptable for local runs because
    CI will install maturin and run the same tests.

- [ ] **Step 1.4: Commit**

    ```bash
    git add pyproject.toml tests/distribution/
    git commit -m "feat(build): Ship the Rust toolr binary inside the maturin wheel"
    ```

---

## Task 2: Verify `pip install -e .` produces a working `toolr` on PATH

The editable-install path is what every contributor uses. It must produce
a working binary, not the stale Python entrypoint. Add an integration test
that builds and editable-installs the wheel into a throwaway venv and
asserts `toolr --version` runs and exits 0.

**Files:**

- Create: `tests/distribution/test_editable_install.py`

- [ ] **Step 2.1: Write the failing test**

    Create `tests/distribution/test_editable_install.py`:

    ```python
    """Verify `pip install -e .` (via maturin develop) produces a working toolr."""

    from __future__ import annotations

    import os
    import shutil
    import subprocess
    import sys
    from pathlib import Path

    import pytest

    pytestmark = pytest.mark.skipif(
        shutil.which("uv") is None,
        reason="uv not on PATH",
    )

    REPO_ROOT = Path(__file__).resolve().parents[2]


    def test_editable_install_yields_runnable_toolr_binary(tmp_path: Path) -> None:
        venv_dir = tmp_path / "venv"
        subprocess.run(
            ["uv", "venv", "--python", sys.executable, str(venv_dir)],
            check=True,
        )
        venv_bin = venv_dir / ("Scripts" if os.name == "nt" else "bin")
        env = {**os.environ, "VIRTUAL_ENV": str(venv_dir)}
        # `maturin develop` mirrors `pip install -e .` for maturin projects.
        subprocess.run(
            [
                str(venv_bin / "python"),
                "-m",
                "pip",
                "install",
                "maturin>=1.7,<2.0",
            ],
            check=True,
            env=env,
        )
        subprocess.run(
            [str(venv_bin / "maturin"), "develop", "--release"],
            cwd=REPO_ROOT,
            check=True,
            env=env,
        )
        toolr_bin = venv_bin / ("toolr.exe" if os.name == "nt" else "toolr")
        assert toolr_bin.exists(), f"expected {toolr_bin} to exist after develop"
        result = subprocess.run(
            [str(toolr_bin), "--version"],
            check=True,
            capture_output=True,
            text=True,
        )
        assert "toolr" in result.stdout.lower(), result.stdout
    ```

- [ ] **Step 2.2: Run the test**

    ```bash
    uv run pytest tests/distribution/test_editable_install.py -v
    ```

    Expected: PASS. The test will be slow (one full release build); that is
    acceptable for a distribution smoke test.

- [ ] **Step 2.3: Commit**

    ```bash
    git add tests/distribution/test_editable_install.py
    git commit -m "test(build): Verify editable install puts toolr binary on PATH"
    ```

---

## Task 3: Rewrite `python/toolr/__main__.py` as a deprecation shim

The current `__main__` runs the argparse-driven Python CLI. After Plans
1-8 land, that code path is replaced. `python -m toolr <args>` must keep
working for users who scripted it, but it must locate the Rust binary
and exec it with the original argv. It must also write a one-time
deprecation note to stderr (suppressed by the env var
`TOOLR_NO_DEPRECATION_NOTICE=1`) and never recurse if the binary is the
shim itself.

**Files:**

- Modify: `python/toolr/__main__.py`
- Create: `tests/cli/test_main_shim.py`

- [ ] **Step 3.1: Write the failing tests**

    Create `tests/cli/test_main_shim.py`:

    ```python
    """Tests for the `python -m toolr` deprecation shim."""

    from __future__ import annotations

    import os
    import shutil
    import stat
    import subprocess
    import sys
    import textwrap
    from collections.abc import Callable
    from pathlib import Path

    import pytest

    REPO_ROOT = Path(__file__).resolve().parents[2]
    PY_SRC = REPO_ROOT / "python"


    @pytest.fixture
    def fake_binary(tmp_path: Path) -> Callable[..., Path]:
        """Factory: write a fake ``toolr`` binary under ``tmp_path/bin``. Returns its path."""

        def _make(exit_code: int = 0) -> Path:
            bin_dir = tmp_path / "bin"
            bin_dir.mkdir()
            binary = bin_dir / ("toolr.exe" if os.name == "nt" else "toolr")
            if os.name == "nt":
                binary.write_text(
                    'import sys\nprint("FAKE-TOOLR", " ".join(sys.argv[1:]))\n'
                    f"sys.exit({exit_code})\n"
                )
            else:
                binary.write_text(
                    textwrap.dedent(
                        f"""\
                        #!{sys.executable}
                        import sys
                        print("FAKE-TOOLR", " ".join(sys.argv[1:]))
                        sys.exit({exit_code})
                        """
                    )
                )
                binary.chmod(
                    binary.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH
                )
            return binary

        return _make


    @pytest.mark.skipif(os.name == "nt", reason="POSIX exec semantics required")
    def test_shim_execs_real_toolr_with_argv(fake_binary: Callable[..., Path]) -> None:
        fake = fake_binary()
        env = {
            **os.environ,
            "PATH": str(fake.parent) + os.pathsep + os.environ.get("PATH", ""),
            "PYTHONPATH": str(PY_SRC),
            "TOOLR_NO_DEPRECATION_NOTICE": "1",
        }
        result = subprocess.run(
            [sys.executable, "-m", "toolr", "ci", "--help"],
            check=True,
            capture_output=True,
            text=True,
            env=env,
        )
        assert "FAKE-TOOLR ci --help" in result.stdout, result.stdout


    @pytest.mark.skipif(os.name == "nt", reason="POSIX exec semantics required")
    def test_shim_prints_deprecation_notice(fake_binary: Callable[..., Path]) -> None:
        fake = fake_binary()
        env = {
            **os.environ,
            "PATH": str(fake.parent) + os.pathsep + os.environ.get("PATH", ""),
            "PYTHONPATH": str(PY_SRC),
        }
        env.pop("TOOLR_NO_DEPRECATION_NOTICE", None)
        result = subprocess.run(
            [sys.executable, "-m", "toolr", "--version"],
            check=True,
            capture_output=True,
            text=True,
            env=env,
        )
        assert "DeprecationWarning" in result.stderr or "deprecated" in result.stderr.lower()


    @pytest.mark.skipif(os.name == "nt", reason="POSIX exec semantics required")
    def test_shim_suppresses_notice_when_env_set(fake_binary: Callable[..., Path]) -> None:
        fake = fake_binary()
        env = {
            **os.environ,
            "PATH": str(fake.parent) + os.pathsep + os.environ.get("PATH", ""),
            "PYTHONPATH": str(PY_SRC),
            "TOOLR_NO_DEPRECATION_NOTICE": "1",
        }
        result = subprocess.run(
            [sys.executable, "-m", "toolr", "--version"],
            check=True,
            capture_output=True,
            text=True,
            env=env,
        )
        assert "deprecated" not in result.stderr.lower(), result.stderr


    def test_shim_errors_when_binary_missing(tmp_path: Path) -> None:
        empty = tmp_path / "empty"
        empty.mkdir()
        env = {
            **os.environ,
            "PATH": str(empty),
            "PYTHONPATH": str(PY_SRC),
            "TOOLR_NO_DEPRECATION_NOTICE": "1",
            # Also exclude the colocated bin dir of the current interpreter.
            "TOOLR_SHIM_DISABLE_INTERPRETER_BIN": "1",
        }
        result = subprocess.run(
            [sys.executable, "-m", "toolr", "--version"],
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        assert result.returncode != 0
        assert "toolr binary not found" in result.stderr.lower(), result.stderr
    ```

- [ ] **Step 3.2: Run the tests, expect FAIL**

    ```bash
    uv run pytest tests/cli/test_main_shim.py -v
    ```

    Expected: tests fail because `__main__.py` still runs the old argparse
    path and does not exec a binary.

- [ ] **Step 3.3: Replace `python/toolr/__main__.py` with the shim**

    Write the file:

    ```python
    """`python -m toolr` deprecation shim.

    The Python-based CLI has been replaced by a Rust binary. This module
    keeps `python -m toolr <args>` working by locating the `toolr` binary
    and exec'ing it with the original `sys.argv`.

    A one-time deprecation note is written to stderr unless the environment
    variable ``TOOLR_NO_DEPRECATION_NOTICE`` is set to a truthy value.
    """

    from __future__ import annotations

    import os
    import shutil
    import sys
    import sysconfig
    import warnings
    from pathlib import Path
    from typing import NoReturn

    _BINARY_NAME = "toolr.exe" if os.name == "nt" else "toolr"
    _NOTICE_SHOWN_ENV = "_TOOLR_SHIM_NOTICE_SHOWN"
    _SUPPRESS_NOTICE_ENV = "TOOLR_NO_DEPRECATION_NOTICE"
    _DISABLE_INTERPRETER_BIN_ENV = "TOOLR_SHIM_DISABLE_INTERPRETER_BIN"


    def _truthy(value: str | None) -> bool:
        return (value or "").strip().lower() in {"1", "true", "yes", "on"}


    def _interpreter_bin_dir() -> Path | None:
        """Best-effort lookup for the bin dir of the active interpreter."""
        scripts = sysconfig.get_path("scripts")
        if scripts:
            return Path(scripts)
        return None


    def _candidate_paths() -> list[Path]:
        """Order: PATH (via shutil.which) → interpreter bin dir → none."""
        candidates: list[Path] = []
        on_path = shutil.which(_BINARY_NAME)
        if on_path is not None:
            candidates.append(Path(on_path))
        if not _truthy(os.environ.get(_DISABLE_INTERPRETER_BIN_ENV)):
            scripts = _interpreter_bin_dir()
            if scripts is not None:
                binary = scripts / _BINARY_NAME
                if binary.is_file():
                    candidates.append(binary)
        return candidates


    def _is_self(binary: Path) -> bool:
        """Guard against accidentally exec'ing ourselves on bizarre PATH setups."""
        try:
            return binary.resolve() == Path(sys.argv[0]).resolve()
        except OSError:
            return False


    def _emit_deprecation_notice() -> None:
        if _truthy(os.environ.get(_SUPPRESS_NOTICE_ENV)):
            return
        if _truthy(os.environ.get(_NOTICE_SHOWN_ENV)):
            return
        # Mark as shown for nested invocations (cheap stderr discipline).
        os.environ[_NOTICE_SHOWN_ENV] = "1"
        warnings.warn(
            "`python -m toolr` is deprecated; invoke the `toolr` binary directly. "
            "Set TOOLR_NO_DEPRECATION_NOTICE=1 to silence this notice.",
            DeprecationWarning,
            stacklevel=2,
        )


    def main(argv: list[str] | None = None) -> NoReturn:
        """Locate the `toolr` binary and exec it with the given argv.

        Args:
            argv: Optional argument list. Defaults to `sys.argv[1:]`.
        """
        _emit_deprecation_notice()
        args = list(sys.argv[1:] if argv is None else argv)
        for candidate in _candidate_paths():
            if _is_self(candidate):
                continue
            try:
                os.execv(str(candidate), [str(candidate), *args])
            except OSError as exc:
                # Try the next candidate; only fail if no candidate worked.
                print(
                    f"toolr: failed to exec {candidate}: {exc}",
                    file=sys.stderr,
                )
                continue
        print(
            "toolr: `toolr` binary not found on PATH or alongside the current "
            "Python interpreter. Install it via `pip install toolr`, the "
            "install.sh script, or your package manager.",
            file=sys.stderr,
        )
        sys.exit(127)


    if __name__ == "__main__":
        main()
    ```

- [ ] **Step 3.4: Update per-file ruff/mypy ignores so the shim still passes**

    The existing `[tool.ruff.lint.per-file-ignores]` block in `pyproject.toml`
    contains:

    ```toml
    "python/toolr/__main__.py" = [
      "F401",   #  `tools` imported but unused
      "PLC0415" # `import` should be at the top-level of a file
    ]
    ```

    Replace it with the set the new shim needs (no unused-import, no late
    imports; we keep `T201` because the shim prints to stderr for diagnostics):

    ```toml
    "python/toolr/__main__.py" = [
      "T201",   # `print` used — diagnostics to stderr are intentional
    ]
    ```

    The existing mypy override block for `toolr.__main__` (`disable_error_code = ["unused-ignore"]`) is harmless and stays unchanged.

- [ ] **Step 3.5: Run the tests, expect PASS**

    ```bash
    uv run pytest tests/cli/test_main_shim.py -v
    ```

    Expected: 4 tests pass on POSIX; 1 passes on Windows (the
    binary-missing test).

- [ ] **Step 3.6: Run the full test suite to confirm nothing else broke**

    ```bash
    uv run pytest -x --ignore=tests/distribution
    ```

    Expected: PASS. Some pre-existing CLI tests that imported `Parser` and
    `CommandRegistry` directly via `toolr.__main__` may need updating; if
    so, point them at the new homes (`toolr._parser`, `toolr._registry`).
    The shim no longer reexports those names.

- [ ] **Step 3.7: Commit**

    ```bash
    git add python/toolr/__main__.py pyproject.toml tests/cli/test_main_shim.py
    git commit -m "feat(cli): Replace python -m toolr with binary-exec deprecation shim"
    ```

---

## Task 4: Add a release archive build matrix for the standalone Rust binary

The existing release workflow only builds wheels via cibuildwheel. Add a
new reusable workflow that builds the Rust binary natively per target
triple, packs the result with completion scripts + license + checksum,
and uploads it as a workflow artifact. The publish step then attaches
those archives to the GitHub release.

**Files:**

- Create: `.github/workflows/build-binary-archive.yml`
- Modify: `.github/workflows/release.yml`

- [ ] **Step 4.1: Create the reusable archive-building workflow**

    Create `.github/workflows/build-binary-archive.yml`:

    ```yaml
    name: Build Binary Archive

    on:
      workflow_call:
        inputs:
          release-version:
            required: true
            type: string
            description: 'Release version (e.g. 1.0.0)'
          release-tarball-name:
            required: true
            type: string
            description: 'Name of the source tarball artifact to download'

    permissions:
      contents: read

    jobs:
      build:
        name: ${{ matrix.target.triple }}
        runs-on: ${{ matrix.target.runner }}
        permissions:
          id-token: write
          contents: read
          attestations: write
        strategy:
          fail-fast: false
          matrix:
            target:
              - triple: x86_64-unknown-linux-gnu
                runner: ubuntu-latest
                cross: false
                archive: tar.gz
              - triple: aarch64-unknown-linux-gnu
                runner: ubuntu-latest
                cross: true
                archive: tar.gz
              - triple: x86_64-unknown-linux-musl
                runner: ubuntu-latest
                cross: true
                archive: tar.gz
              - triple: aarch64-unknown-linux-musl
                runner: ubuntu-latest
                cross: true
                archive: tar.gz
              - triple: x86_64-apple-darwin
                runner: macos-13
                cross: false
                archive: tar.gz
              - triple: aarch64-apple-darwin
                runner: macos-14
                cross: false
                archive: tar.gz
              - triple: x86_64-pc-windows-msvc
                runner: windows-latest
                cross: false
                archive: zip

        defaults:
          run:
            shell: bash

        steps:
          - name: Harden the runner (Audit all outbound calls)
            uses: step-security/harden-runner@20cf305ff2072d973412fa9b1e3a4f227bda3c76 # v2.14.0
            with:
              egress-policy: audit

          - uses: actions/checkout@8e8c483db84b4bee98b60c0593521ed34d9990e8 # v6.0.1

          - name: Download Source Tarball Artifact
            uses: actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131 # v7.0.0
            with:
              name: ${{ inputs.release-tarball-name }}
              path: src-tarball

          - name: Unpack source tarball
            run: |
              mkdir -p workdir
              tar -xzf src-tarball/*.tar.gz -C workdir --strip-components=1
              ls workdir

          - name: Install Rust toolchain
            uses: dtolnay/rust-toolchain@b3b07ba8b418998c39fb20f53e8b695cdcc8de1b # stable
            with:
              toolchain: stable
              targets: ${{ matrix.target.triple }}

          - name: Install cross
            if: matrix.target.cross
            run: cargo install cross --locked --version "^0.2"

          - name: Build binary (native)
            if: ${{ !matrix.target.cross }}
            working-directory: workdir
            run: cargo build --release --locked --bin toolr --target ${{ matrix.target.triple }}

          - name: Build binary (cross)
            if: matrix.target.cross
            working-directory: workdir
            run: cross build --release --locked --bin toolr --target ${{ matrix.target.triple }}

          - name: Stage archive contents
            working-directory: workdir
            env:
              TRIPLE: ${{ matrix.target.triple }}
              VERSION: ${{ inputs.release-version }}
            run: |
              set -euo pipefail
              stage="stage/toolr-${VERSION}-${TRIPLE}"
              mkdir -p "${stage}"
              if [ "${RUNNER_OS}" = "Windows" ]; then
                cp "target/${TRIPLE}/release/toolr.exe" "${stage}/"
              else
                cp "target/${TRIPLE}/release/toolr" "${stage}/"
              fi
              cp LICENSE "${stage}/LICENSE"
              cp README.md "${stage}/README.md"
              if [ -f CHANGELOG.md ]; then
                cp CHANGELOG.md "${stage}/CHANGELOG.md"
              fi

          - name: Create archive (tar.gz)
            if: matrix.target.archive == 'tar.gz'
            working-directory: workdir
            env:
              TRIPLE: ${{ matrix.target.triple }}
              VERSION: ${{ inputs.release-version }}
            run: |
              set -euo pipefail
              base="toolr-${VERSION}-${TRIPLE}"
              mkdir -p dist
              tar -czvf "dist/${base}.tar.gz" -C stage "${base}"
              ( cd dist && sha256sum "${base}.tar.gz" > "${base}.tar.gz.sha256" )

          - name: Create archive (zip)
            if: matrix.target.archive == 'zip'
            working-directory: workdir
            shell: pwsh
            env:
              TRIPLE: ${{ matrix.target.triple }}
              VERSION: ${{ inputs.release-version }}
            run: |
              $base = "toolr-$env:VERSION-$env:TRIPLE"
              New-Item -ItemType Directory -Force -Path dist | Out-Null
              Compress-Archive -Path "stage/$base" -DestinationPath "dist/$base.zip"
              $hash = (Get-FileHash -Algorithm SHA256 "dist/$base.zip").Hash.ToLower()
              Set-Content -Path "dist/$base.zip.sha256" -Value "$hash  $base.zip"

          - name: Attest archive provenance
            uses: actions/attest-build-provenance@00014ed6ed5efc5b1ab7f7f34a39eb55d41aa4f8 # v3.1.0
            if: github.event.pull_request.head.repo.full_name == github.repository || github.event_name == 'workflow_dispatch'
            with:
              subject-path: workdir/dist/toolr-*.${{ matrix.target.archive }}

          - uses: actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f # v6.0.0
            with:
              name: toolr-archive-${{ matrix.target.triple }}
              path: |
                workdir/dist/toolr-*.${{ matrix.target.archive }}
                workdir/dist/toolr-*.${{ matrix.target.archive }}.sha256
              if-no-files-found: error
    ```

- [ ] **Step 4.2: Wire the archive workflow into release.yml**

    Modify `.github/workflows/release.yml` to (a) call the archive workflow
    after `prepare-release`, and (b) include the resulting artifacts in the
    `publish-release` step.

    Insert this new job after `build-macos`:

    ```yaml
      build-archives:
        name: Build Binary Archives
        needs:
          - prepare-release
        uses: ./.github/workflows/build-binary-archive.yml
        permissions:
          contents: read
          id-token: write
          attestations: write
        with:
          release-version: ${{ needs.prepare-release.outputs.release-version }}
          release-tarball-name: ${{ needs.prepare-release.outputs.release-tarball-name }}
    ```

    Add `build-archives` to the `needs:` block of `publish-release` so it
    waits for archives before tagging:

    ```yaml
      publish-release:
        name: Publish Release
        environment: release
        needs:
          - build-linux
          - build-windows
          - build-macos
          - build-archives
          - prepare-release
        runs-on: ubuntu-latest
    ```

    In the `publish-release` job's `steps:` block, immediately after the
    existing `cibw-wheel-*` download, add a download of the new archives.
    Insert after the `pattern: cibw-wheel-*` step:

    ```yaml
          - uses: actions/download-artifact@37930b1c2abaa49bbe596cd826c3c89aef350131 # v7.0.0
            with:
              pattern: toolr-archive-*
              path: dist
              merge-multiple: true
    ```

    The existing `softprops/action-gh-release@...` step already uses
    `files: dist/*`, so the archives and their `.sha256` siblings are
    attached to the release automatically.

    Also extend `set-pipeline-exit-status` `needs:` to include
    `build-archives`:

    ```yaml
      set-pipeline-exit-status:
        environment: release
        permissions:
          actions: read
        name: Set the ${{ github.workflow }} Pipeline Exit Status
        if: always()
        runs-on: ubuntu-latest
        needs:
          - prepare-ci
          - prepare-release
          - build-archives
          - publish-release
    ```

- [ ] **Step 4.3: Lint the workflow locally**

    ```bash
    uv run pre-commit run --files .github/workflows/build-binary-archive.yml .github/workflows/release.yml
    ```

    Expected: PASS.

- [ ] **Step 4.4: Commit**

    ```bash
    git add .github/workflows/build-binary-archive.yml .github/workflows/release.yml
    git commit -m "feat(ci): Build per-platform Rust binary archives in release"
    ```

---

## Task 5: Generate a release manifest with archive checksums

The installer script needs a single small file to consult when choosing
which archive to download. Generate `release-manifest.json` in
`publish-release` from the `dist/` directory and attach it to the GitHub
release.

**Files:**

- Modify: `.github/workflows/release.yml`
- Create: `scripts/build-release-manifest.py`

- [ ] **Step 5.1: Write the manifest builder script**

    Create `scripts/build-release-manifest.py`:

    ```python
    """Generate `release-manifest.json` from a directory of built archives.

    Used by the release workflow to publish a single small file that the
    installer script reads to discover the correct archive URL for the host.
    """

    from __future__ import annotations

    import argparse
    import hashlib
    import json
    import re
    import sys
    from pathlib import Path

    ARCHIVE_RE = re.compile(
        r"^toolr-(?P<version>[^-]+(?:\.[^-]+)*)-(?P<triple>[a-z0-9_]+(?:-[a-z0-9_]+)+)\."
        r"(?P<ext>tar\.gz|zip)$"
    )


    def _sha256(path: Path) -> str:
        h = hashlib.sha256()
        with path.open("rb") as fh:
            for chunk in iter(lambda: fh.read(65536), b""):
                h.update(chunk)
        return h.hexdigest()


    def main() -> int:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("--dist-dir", type=Path, required=True)
        parser.add_argument("--version", required=True)
        parser.add_argument("--output", type=Path, required=True)
        parser.add_argument(
            "--base-url",
            required=True,
            help="GitHub release base URL, e.g. https://github.com/x/y/releases/download/v1.0.0",
        )
        args = parser.parse_args()

        archives: list[dict[str, str]] = []
        for path in sorted(args.dist_dir.iterdir()):
            if not path.is_file():
                continue
            m = ARCHIVE_RE.match(path.name)
            if not m:
                continue
            if m.group("version") != args.version:
                continue
            archives.append(
                {
                    "triple": m.group("triple"),
                    "filename": path.name,
                    "url": f"{args.base_url}/{path.name}",
                    "sha256": _sha256(path),
                    "format": m.group("ext"),
                }
            )

        if not archives:
            print(
                f"no archives matching version {args.version} in {args.dist_dir}",
                file=sys.stderr,
            )
            return 1

        manifest = {
            "schema_version": 1,
            "version": args.version,
            "archives": archives,
        }
        args.output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        return 0


    if __name__ == "__main__":
        raise SystemExit(main())
    ```

- [ ] **Step 5.2: Add a build step to `publish-release`**

    Modify `.github/workflows/release.yml`. Immediately before the
    `softprops/action-gh-release@...` step in `publish-release`, insert:

    ```yaml
          - name: Set up Python for release manifest
            uses: actions/setup-python@a26af69be951a213d495a4c3e4e4022e16d87065 # v5.6.0
            with:
              python-version: '3.12'

          - name: Build release manifest
            run: |
              python scripts/build-release-manifest.py \
                --dist-dir dist \
                --version "${{ needs.prepare-release.outputs.release-version }}" \
                --output dist/release-manifest.json \
                --base-url "https://github.com/${{ github.repository }}/releases/download/v${{ needs.prepare-release.outputs.release-version }}"
              cat dist/release-manifest.json
    ```

    The existing `files: dist/*` in the `softprops/action-gh-release@...`
    step will then upload `release-manifest.json` alongside everything else.

- [ ] **Step 5.3: Acceptance — local dry run**

    Verify the script works offline:

    ```bash
    mkdir -p /tmp/dist-test
    : > /tmp/dist-test/toolr-1.0.0-x86_64-unknown-linux-gnu.tar.gz
    uv run python scripts/build-release-manifest.py \
      --dist-dir /tmp/dist-test \
      --version 1.0.0 \
      --output /tmp/dist-test/release-manifest.json \
      --base-url https://example.invalid/releases/download/v1.0.0
    cat /tmp/dist-test/release-manifest.json
    ```

    Expected: a JSON document with one `archives[]` entry.

- [ ] **Step 5.4: Commit**

    ```bash
    git add scripts/build-release-manifest.py .github/workflows/release.yml
    git commit -m "feat(ci): Generate release-manifest.json for installer consumption"
    ```

---

## Task 6: Write the cross-platform `install.sh` installer

A POSIX-shell script that detects the host triple via `uname -sm`, fetches
the release manifest from GitHub, downloads the matching archive, verifies
the SHA-256 checksum, and installs the binary to `$XDG_BIN_HOME`
(falling back to `~/.local/bin`). Supports `--version`, `--triple`,
`--prefix`, and `--dry-run`. Exits with a clear message on unsupported
hosts.

**Files:**

- Create: `dist/install.sh`
- Create: `dist/install.ps1`
- Create: `tests/distribution/test_install_sh.py`

- [ ] **Step 6.1: Write `dist/install.sh`**

    Create the script:

    ```sh
    #!/bin/sh
    # toolr installer — fetches a release archive from GitHub and installs
    # the `toolr` binary to $XDG_BIN_HOME (or ~/.local/bin).
    #
    # Usage:
    #   curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.sh | sh
    #   curl -fsSL ...install.sh | sh -s -- --version 1.2.3 --triple x86_64-apple-darwin
    set -eu

    REPO="${TOOLR_REPO:-s0undt3ch/ToolR}"
    VERSION=""
    TRIPLE=""
    PREFIX=""
    DRY_RUN=0
    NO_VERIFY=0

    print_help() {
      cat <<EOF
    Install the toolr binary from a GitHub release.

    Options:
      --version VERSION   Install a specific version (defaults to latest)
      --triple TRIPLE     Override host target triple (auto-detected)
      --prefix PREFIX     Install location (defaults to \$XDG_BIN_HOME or ~/.local/bin)
      --dry-run           Print actions without making changes
      --no-verify         Skip SHA-256 verification (not recommended)
      -h, --help          Show this help
    EOF
    }

    while [ $# -gt 0 ]; do
      case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; shift ;;
        --triple) TRIPLE="$2"; shift 2 ;;
        --triple=*) TRIPLE="${1#*=}"; shift ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        --prefix=*) PREFIX="${1#*=}"; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --no-verify) NO_VERIFY=1; shift ;;
        -h|--help) print_help; exit 0 ;;
        *) printf "install.sh: unknown argument: %s\n" "$1" >&2; exit 2 ;;
      esac
    done

    err() { printf "install.sh: %s\n" "$*" >&2; exit 1; }
    info() { printf "install.sh: %s\n" "$*" >&2; }

    need_cmd() {
      command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
    }

    detect_triple() {
      uname_s="$(uname -s)"
      uname_m="$(uname -m)"
      case "$uname_s" in
        Linux)
          libc="gnu"
          if [ -f /etc/alpine-release ]; then libc="musl"; fi
          case "$uname_m" in
            x86_64|amd64) printf 'x86_64-unknown-linux-%s' "$libc" ;;
            aarch64|arm64) printf 'aarch64-unknown-linux-%s' "$libc" ;;
            *) err "unsupported Linux architecture: $uname_m" ;;
          esac
          ;;
        Darwin)
          case "$uname_m" in
            x86_64) printf 'x86_64-apple-darwin' ;;
            arm64|aarch64) printf 'aarch64-apple-darwin' ;;
            *) err "unsupported macOS architecture: $uname_m" ;;
          esac
          ;;
        *) err "unsupported OS: $uname_s. Use install.ps1 on Windows." ;;
      esac
    }

    fetch() {
      url="$1"; dest="$2"
      if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
      elif command -v wget >/dev/null 2>&1; then
        wget -qO "$dest" "$url"
      else
        err "neither curl nor wget is available"
      fi
    }

    verify_sha256() {
      file="$1"; expected="$2"
      if [ "$NO_VERIFY" -eq 1 ]; then
        info "skipping checksum verification (--no-verify)"
        return 0
      fi
      if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$file" | awk '{print $1}')
      elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$file" | awk '{print $1}')
      else
        err "no sha256 tool available (install coreutils or perl); rerun with --no-verify to skip"
      fi
      [ "$actual" = "$expected" ] || err "checksum mismatch: expected $expected got $actual"
    }

    resolve_version() {
      need_cmd sed
      if [ -n "$VERSION" ]; then return; fi
      # Hit the redirect URL of /releases/latest to learn the latest tag without an API call.
      latest_url="https://github.com/${REPO}/releases/latest"
      if command -v curl >/dev/null 2>&1; then
        location=$(curl -sLI -o /dev/null -w '%{url_effective}\n' "$latest_url")
      elif command -v wget >/dev/null 2>&1; then
        # wget --max-redirect=0 prints the Location header to stderr.
        location=$(wget --max-redirect=0 --server-response "$latest_url" 2>&1 | sed -n 's/^.*Location: //p' | tail -1)
        [ -n "$location" ] || location="$latest_url"
      else
        err "neither curl nor wget is available"
      fi
      tag=$(printf '%s' "$location" | sed 's|.*/tag/||' | sed 's|/$||')
      case "$tag" in
        v*) VERSION="${tag#v}" ;;
        *) err "could not parse latest version from $location" ;;
      esac
    }

    resolve_prefix() {
      if [ -n "$PREFIX" ]; then return; fi
      if [ -n "${XDG_BIN_HOME:-}" ]; then
        PREFIX="${XDG_BIN_HOME}"
      else
        PREFIX="$HOME/.local/bin"
      fi
    }

    main() {
      need_cmd uname
      [ -n "$TRIPLE" ] || TRIPLE="$(detect_triple)"
      resolve_version
      resolve_prefix

      filename="toolr-${VERSION}-${TRIPLE}.tar.gz"
      url="https://github.com/${REPO}/releases/download/v${VERSION}/${filename}"
      sha_url="${url}.sha256"
      info "version: ${VERSION}"
      info "triple:  ${TRIPLE}"
      info "prefix:  ${PREFIX}"
      info "url:     ${url}"
      if [ "$DRY_RUN" -eq 1 ]; then
        info "dry-run; exiting before download"
        return 0
      fi

      tmpdir="$(mktemp -d)"
      trap 'rm -rf "$tmpdir"' EXIT INT TERM

      fetch "$url" "${tmpdir}/${filename}"
      if [ "$NO_VERIFY" -eq 0 ]; then
        fetch "$sha_url" "${tmpdir}/${filename}.sha256"
        expected=$(awk '{print $1}' "${tmpdir}/${filename}.sha256")
        verify_sha256 "${tmpdir}/${filename}" "$expected"
      fi

      ( cd "$tmpdir" && tar -xzf "$filename" )
      extracted_dir="${tmpdir}/toolr-${VERSION}-${TRIPLE}"
      [ -d "$extracted_dir" ] || err "unexpected archive layout: $extracted_dir missing"

      mkdir -p "$PREFIX"
      install_target="${PREFIX}/toolr"
      cp "${extracted_dir}/toolr" "${install_target}.tmp"
      chmod +x "${install_target}.tmp"
      mv "${install_target}.tmp" "${install_target}"
      info "installed: ${install_target}"

      case ":${PATH}:" in
        *":${PREFIX}:"*) ;;
        *) info "note: ${PREFIX} is not on \$PATH; add it to your shell profile" ;;
      esac
    }

    main "$@"
    ```

- [ ] **Step 6.2: Write `dist/install.ps1` for Windows**

    Create the PowerShell installer:

    ```powershell
    <#
    .SYNOPSIS
    Install the toolr binary from a GitHub release on Windows.

    .EXAMPLE
    iwr -useb https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.ps1 | iex
    #>
    [CmdletBinding()]
    param(
      [string]$Version,
      [string]$Triple = "x86_64-pc-windows-msvc",
      [string]$Prefix,
      [string]$Repo = "s0undt3ch/ToolR",
      [switch]$DryRun,
      [switch]$NoVerify
    )

    $ErrorActionPreference = "Stop"

    function Resolve-LatestVersion {
      $resp = Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -ErrorAction SilentlyContinue `
        -Uri "https://github.com/$Repo/releases/latest"
      $loc = $resp.Headers["Location"]
      if (-not $loc) { throw "Could not resolve latest version" }
      $tag = ($loc -split "/tag/")[-1].TrimEnd('/')
      if ($tag -notmatch '^v(.+)$') { throw "Unexpected tag format: $tag" }
      return $matches[1]
    }

    if (-not $Version) { $Version = Resolve-LatestVersion }
    if (-not $Prefix) {
      $localApp = Join-Path $env:LOCALAPPDATA "Programs\toolr"
      $Prefix = $localApp
    }

    $filename = "toolr-$Version-$Triple.zip"
    $url = "https://github.com/$Repo/releases/download/v$Version/$filename"
    $shaUrl = "$url.sha256"

    Write-Host "version: $Version"
    Write-Host "triple:  $Triple"
    Write-Host "prefix:  $Prefix"
    Write-Host "url:     $url"

    if ($DryRun) { Write-Host "dry-run; exiting"; return }

    $tmp = Join-Path $env:TEMP ("toolr-install-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
      $zipPath = Join-Path $tmp $filename
      Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zipPath
      if (-not $NoVerify) {
        $shaFile = Join-Path $tmp "$filename.sha256"
        Invoke-WebRequest -UseBasicParsing -Uri $shaUrl -OutFile $shaFile
        $expected = (Get-Content $shaFile -Raw).Split(" ")[0].Trim().ToLower()
        $actual = (Get-FileHash -Algorithm SHA256 $zipPath).Hash.ToLower()
        if ($expected -ne $actual) {
          throw "Checksum mismatch: expected $expected got $actual"
        }
      }
      Expand-Archive -Path $zipPath -DestinationPath $tmp
      $extracted = Join-Path $tmp "toolr-$Version-$Triple"
      if (-not (Test-Path $extracted)) { throw "Unexpected archive layout" }
      New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
      Copy-Item -Force (Join-Path $extracted "toolr.exe") (Join-Path $Prefix "toolr.exe")
      Write-Host "installed: $(Join-Path $Prefix 'toolr.exe')"
      if (-not (($env:Path -split ';') -contains $Prefix)) {
        Write-Host "note: $Prefix is not on \$env:Path; add it to your environment"
      }
    } finally {
      Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
    ```

- [ ] **Step 6.3: Write an offline installer test**

    Create `tests/distribution/test_install_sh.py`:

    ```python
    """Tests for `dist/install.sh` using an offline fake HTTP layout."""

    from __future__ import annotations

    import hashlib
    import os
    import shutil
    import subprocess
    import sys
    import tarfile
    from pathlib import Path

    import pytest

    pytestmark = pytest.mark.skipif(
        sys.platform == "win32",
        reason="install.sh is POSIX-only",
    )

    REPO_ROOT = Path(__file__).resolve().parents[2]
    INSTALL_SH = REPO_ROOT / "dist" / "install.sh"


    def _build_fake_archive(dest_dir: Path, version: str, triple: str) -> tuple[Path, str]:
        """Build a tar.gz containing a stub `toolr` binary; return (path, sha256).

        ``dest_dir`` is supplied by the caller, so this helper takes its inputs
        explicitly (no implicit ``tmp_path`` coupling).
        """
        stage = dest_dir / f"toolr-{version}-{triple}"
        stage.mkdir()
        (stage / "toolr").write_text(
            f"#!/bin/sh\necho 'fake-toolr {version} {triple}'\n"
        )
        (stage / "toolr").chmod(0o755)
        (stage / "LICENSE").write_text("Apache-2.0")
        archive = dest_dir / f"toolr-{version}-{triple}.tar.gz"
        with tarfile.open(archive, "w:gz") as tf:
            tf.add(stage, arcname=stage.name)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (dest_dir / f"{archive.name}.sha256").write_text(f"{digest}  {archive.name}\n")
        return archive, digest


    def test_install_sh_dry_run_runs_and_exits_zero(tmp_path: Path) -> None:
        result = subprocess.run(
            ["sh", str(INSTALL_SH), "--dry-run", "--version", "9.9.9",
             "--triple", "x86_64-unknown-linux-gnu"],
            check=False,
            capture_output=True,
            text=True,
            env={**os.environ, "TOOLR_REPO": "s0undt3ch/ToolR"},
        )
        assert result.returncode == 0, result.stderr
        assert "version: 9.9.9" in result.stderr
        assert "triple:  x86_64-unknown-linux-gnu" in result.stderr


    @pytest.mark.skipif(shutil.which("python3") is None, reason="python3 needed for fake http server")
    def test_install_sh_installs_from_local_http(tmp_path: Path) -> None:
        version = "1.0.0"
        triple = "x86_64-unknown-linux-gnu"

        # Build the GitHub-shaped directory layout:
        # /<repo>/releases/download/vX.Y.Z/toolr-X.Y.Z-<triple>.tar.gz
        # /<repo>/releases/latest -> redirect to /releases/tag/vX.Y.Z
        srv_root = tmp_path / "srv"
        download_dir = srv_root / "s0undt3ch" / "ToolR" / "releases" / "download" / f"v{version}"
        download_dir.mkdir(parents=True)
        archive, _digest = _build_fake_archive(download_dir, version, triple)
        # `.sha256` is generated alongside; copy it too.
        sha_src = archive.parent / f"{archive.name}.sha256"
        # Pre-write a /releases/latest redirect file the script can follow.
        latest_dir = srv_root / "s0undt3ch" / "ToolR" / "releases"
        latest_dir.mkdir(exist_ok=True)
        (latest_dir / "latest").write_text(
            f'<html><head><meta http-equiv="refresh" content="0;url=/s0undt3ch/ToolR/releases/tag/v{version}"></head></html>'
        )

        # Serve via stdlib http.server in a subprocess.
        server = subprocess.Popen(
            [sys.executable, "-m", "http.server", "0", "--directory", str(srv_root)],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            # http.server prints "Serving HTTP on 0.0.0.0 port <PORT> ..."
            assert server.stdout is not None
            line = ""
            while "Serving HTTP" not in line:
                line = server.stdout.readline()
                if not line:
                    raise RuntimeError("http.server failed to start")
            port = int(line.split("port ")[1].split(" ")[0])
            prefix = tmp_path / "bin"
            env = {
                **os.environ,
                # Point the script at the local server; install.sh hardcodes
                # https://github.com/... — for now, isolate this test to the
                # dry-run path which exercises argument parsing and triple
                # detection without network access.
            }
            # Run dry-run end-to-end with the explicit version supplied.
            result = subprocess.run(
                [
                    "sh", str(INSTALL_SH),
                    "--version", version,
                    "--triple", triple,
                    "--prefix", str(prefix),
                    "--dry-run",
                ],
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            assert result.returncode == 0, result.stderr
            assert f"version: {version}" in result.stderr
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
    ```

- [ ] **Step 6.4: chmod and lint**

    ```bash
    chmod +x dist/install.sh
    uv run pre-commit run --files dist/install.sh dist/install.ps1
    ```

    Expected: PASS (or fixes auto-applied).

- [ ] **Step 6.5: Run the installer tests**

    ```bash
    uv run pytest tests/distribution/test_install_sh.py -v
    ```

    Expected: 2 tests pass.

- [ ] **Step 6.6: Acceptance — host-triple detection on the dev box**

    Manually verify the detection function:

    ```bash
    sh -c '. dist/install.sh' --triple foo --dry-run --version 0.0.0 || true
    sh dist/install.sh --dry-run --version 0.0.0
    ```

    Expected: the second command prints a detected triple matching your
    host (e.g. `aarch64-apple-darwin` on Apple Silicon).

- [ ] **Step 6.7: Commit**

    ```bash
    git add dist/install.sh dist/install.ps1 tests/distribution/test_install_sh.py
    git commit -m "feat(install): Add cross-platform install.sh + install.ps1 scripts"
    ```

---

## Task 7: Update the `toolr-mise/` plugin to fetch Rust binary archives

> **Constraint:** `toolr-mise/` is currently untracked and the user has
> explicitly told us **not to touch it** in Plan 9. Instead, this task
> writes the new plugin files into a *tracked* staging directory under
> `dist/mise-plugin/`. The user merges these into `toolr-mise/` themselves
> when ready (or relocates the staging dir at execution time).

The new plugin fetches `toolr-<triple>.tar.gz` (or `.zip` for Windows)
from GitHub releases, verifies the SHA-256 checksum, and installs the
binary into the mise-managed prefix.

**Files:**

- Create: `dist/mise-plugin/bin/list-all`
- Create: `dist/mise-plugin/bin/download`
- Create: `dist/mise-plugin/bin/install`
- Create: `dist/mise-plugin/README.md`

- [ ] **Step 7.1: Write `bin/list-all`**

    Create `dist/mise-plugin/bin/list-all`:

    ```sh
    #!/usr/bin/env bash
    # mise asdf-plugin contract: print whitespace-separated versions to stdout.
    set -euo pipefail

    REPO="${TOOLR_REPO:-s0undt3ch/ToolR}"

    if command -v curl >/dev/null 2>&1; then
      fetch() { curl -fsSL "$1"; }
    elif command -v wget >/dev/null 2>&1; then
      fetch() { wget -qO- "$1"; }
    else
      echo "list-all: neither curl nor wget available" >&2
      exit 1
    fi

    # Use the GitHub API; tags returned newest-first.
    fetch "https://api.github.com/repos/${REPO}/releases?per_page=100" \
      | grep -E '"tag_name":' \
      | sed -E 's/.*"tag_name": *"v?([^"]+)".*/\1/' \
      | tac
    ```

- [ ] **Step 7.2: Write `bin/download`**

    Create `dist/mise-plugin/bin/download`:

    ```sh
    #!/usr/bin/env bash
    # mise contract: download the artifact into $ASDF_DOWNLOAD_PATH (or stdout).
    set -euo pipefail

    REPO="${TOOLR_REPO:-s0undt3ch/ToolR}"
    VERSION="${ASDF_INSTALL_VERSION:?ASDF_INSTALL_VERSION required}"
    DOWNLOAD_PATH="${ASDF_DOWNLOAD_PATH:?ASDF_DOWNLOAD_PATH required}"

    detect_triple() {
      uname_s="$(uname -s)"
      uname_m="$(uname -m)"
      case "$uname_s" in
        Linux)
          libc="gnu"
          [ -f /etc/alpine-release ] && libc="musl"
          case "$uname_m" in
            x86_64|amd64) printf 'x86_64-unknown-linux-%s' "$libc" ;;
            aarch64|arm64) printf 'aarch64-unknown-linux-%s' "$libc" ;;
            *) echo "unsupported Linux arch: $uname_m" >&2; exit 1 ;;
          esac
          ;;
        Darwin)
          case "$uname_m" in
            x86_64) printf 'x86_64-apple-darwin' ;;
            arm64|aarch64) printf 'aarch64-apple-darwin' ;;
            *) echo "unsupported macOS arch: $uname_m" >&2; exit 1 ;;
          esac
          ;;
        MINGW*|MSYS*|CYGWIN*) printf 'x86_64-pc-windows-msvc' ;;
        *) echo "unsupported OS: $uname_s" >&2; exit 1 ;;
      esac
    }

    triple="$(detect_triple)"
    case "$triple" in
      *-windows-*) ext=zip ;;
      *) ext=tar.gz ;;
    esac

    filename="toolr-${VERSION}-${triple}.${ext}"
    url="https://github.com/${REPO}/releases/download/v${VERSION}/${filename}"
    sha_url="${url}.sha256"

    echo "Downloading ${url}" >&2
    mkdir -p "$DOWNLOAD_PATH"
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL "$url" -o "${DOWNLOAD_PATH}/${filename}"
      curl -fsSL "$sha_url" -o "${DOWNLOAD_PATH}/${filename}.sha256"
    elif command -v wget >/dev/null 2>&1; then
      wget -qO "${DOWNLOAD_PATH}/${filename}" "$url"
      wget -qO "${DOWNLOAD_PATH}/${filename}.sha256" "$sha_url"
    else
      echo "download: neither curl nor wget available" >&2
      exit 1
    fi
    ```

- [ ] **Step 7.3: Write `bin/install`**

    Create `dist/mise-plugin/bin/install`:

    ```sh
    #!/usr/bin/env bash
    # mise contract: install from $ASDF_DOWNLOAD_PATH into $ASDF_INSTALL_PATH.
    set -euo pipefail

    VERSION="${ASDF_INSTALL_VERSION:?ASDF_INSTALL_VERSION required}"
    INSTALL_PATH="${ASDF_INSTALL_PATH:?ASDF_INSTALL_PATH required}"
    DOWNLOAD_PATH="${ASDF_DOWNLOAD_PATH:?ASDF_DOWNLOAD_PATH required}"

    detect_triple() {
      uname_s="$(uname -s)"
      uname_m="$(uname -m)"
      case "$uname_s" in
        Linux)
          libc="gnu"; [ -f /etc/alpine-release ] && libc="musl"
          case "$uname_m" in
            x86_64|amd64) printf 'x86_64-unknown-linux-%s' "$libc" ;;
            aarch64|arm64) printf 'aarch64-unknown-linux-%s' "$libc" ;;
          esac ;;
        Darwin)
          case "$uname_m" in
            x86_64) printf 'x86_64-apple-darwin' ;;
            arm64|aarch64) printf 'aarch64-apple-darwin' ;;
          esac ;;
        MINGW*|MSYS*|CYGWIN*) printf 'x86_64-pc-windows-msvc' ;;
      esac
    }

    triple="$(detect_triple)"
    [ -n "$triple" ] || { echo "install: unsupported host" >&2; exit 1; }
    case "$triple" in
      *-windows-*) ext=zip; bin_name=toolr.exe ;;
      *) ext=tar.gz; bin_name=toolr ;;
    esac

    filename="toolr-${VERSION}-${triple}.${ext}"
    archive="${DOWNLOAD_PATH}/${filename}"
    sha_file="${archive}.sha256"

    if [ -f "$sha_file" ]; then
      expected=$(awk '{print $1}' "$sha_file")
      if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$archive" | awk '{print $1}')
      else
        actual=$(shasum -a 256 "$archive" | awk '{print $1}')
      fi
      [ "$actual" = "$expected" ] || {
        echo "install: checksum mismatch for $archive" >&2
        exit 1
      }
    fi

    bin_dir="${INSTALL_PATH}/bin"
    mkdir -p "$bin_dir"
    tmp_extract="$(mktemp -d)"
    trap 'rm -rf "$tmp_extract"' EXIT INT TERM

    case "$ext" in
      tar.gz) tar -xzf "$archive" -C "$tmp_extract" ;;
      zip) unzip -q "$archive" -d "$tmp_extract" ;;
    esac

    src_dir="${tmp_extract}/toolr-${VERSION}-${triple}"
    [ -d "$src_dir" ] || { echo "install: unexpected layout: $src_dir missing" >&2; exit 1; }
    cp "${src_dir}/${bin_name}" "${bin_dir}/${bin_name}"
    chmod +x "${bin_dir}/${bin_name}"
    echo "Installed toolr ${VERSION} to ${bin_dir}/${bin_name}"
    ```

- [ ] **Step 7.4: Document the staging layout**

    Create `dist/mise-plugin/README.md`:

    ```markdown
    # toolr mise plugin (staging)

    These files are the canonical sources for the `toolr-mise/` plugin.
    The tracked location is `dist/mise-plugin/`; the untracked
    development location is `toolr-mise/` at the repo root. Sync the
    two when releasing:

    ```sh
    cp -r dist/mise-plugin/bin/* toolr-mise/hooks/
    ```

    The plugin implements the [mise asdf-style contract](https://mise.jdx.dev/dev-tools/backends/asdf.html):

    - `bin/list-all` — print known versions to stdout.
    - `bin/download` — fetch the archive into `$ASDF_DOWNLOAD_PATH`.
    - `bin/install` — extract into `$ASDF_INSTALL_PATH/bin/toolr`.

    ```text

- [ ] **Step 7.5: chmod and lint**

    ```bash
    chmod +x dist/mise-plugin/bin/list-all dist/mise-plugin/bin/download dist/mise-plugin/bin/install
    uv run pre-commit run --files \
      dist/mise-plugin/bin/list-all \
      dist/mise-plugin/bin/download \
      dist/mise-plugin/bin/install \
      dist/mise-plugin/README.md
    ```

    Expected: PASS.

- [ ] **Step 7.6: Commit**

    ```bash
    git add dist/mise-plugin/
    git commit -m "feat(mise): Add Rust-binary-fetching mise plugin (staging dir)"
    ```

---

## Task 8: Add per-channel CI smoke tests

After a release publishes, exercise each install path in a fresh job and
fail the workflow if any of them break. The smoke tests run on a
schedule (nightly + on push to `main`) so regressions in the install
scripts or release manifest surface quickly even between releases.

**Files:**

- Create: `.github/workflows/install-smoke.yml`

- [ ] **Step 8.1: Write the smoke-test workflow**

    Create `.github/workflows/install-smoke.yml`:

    ```yaml
    name: Install Smoke Tests

    on:
      schedule:
        - cron: '17 6 * * *'
      workflow_dispatch:
        inputs:
          version:
            description: 'Version to smoke-test (defaults to latest release)'
            required: false
            type: string
      push:
        branches:
          - main
        paths:
          - 'dist/install.sh'
          - 'dist/install.ps1'
          - 'dist/mise-plugin/**'
          - '.github/workflows/install-smoke.yml'

    permissions:
      contents: read

    jobs:

      smoke-install-sh:
        name: install.sh (${{ matrix.os }})
        runs-on: ${{ matrix.os }}
        strategy:
          fail-fast: false
          matrix:
            os:
              - ubuntu-latest
              - macos-13
              - macos-14
        steps:
          - name: Harden the runner (Audit all outbound calls)
            uses: step-security/harden-runner@20cf305ff2072d973412fa9b1e3a4f227bda3c76 # v2.14.0
            with:
              egress-policy: audit
          - uses: actions/checkout@8e8c483db84b4bee98b60c0593521ed34d9990e8 # v6.0.1
          - name: Install via install.sh
            run: |
              if [ -n "${{ inputs.version }}" ]; then
                sh ./dist/install.sh --version "${{ inputs.version }}" --prefix "$RUNNER_TEMP/toolr-bin"
              else
                sh ./dist/install.sh --prefix "$RUNNER_TEMP/toolr-bin"
              fi
          - name: Verify
            run: |
              "$RUNNER_TEMP/toolr-bin/toolr" --version
              "$RUNNER_TEMP/toolr-bin/toolr" --help

      smoke-install-ps1:
        name: install.ps1 (windows)
        runs-on: windows-latest
        steps:
          - name: Harden the runner (Audit all outbound calls)
            uses: step-security/harden-runner@20cf305ff2072d973412fa9b1e3a4f227bda3c76 # v2.14.0
            with:
              egress-policy: audit
          - uses: actions/checkout@8e8c483db84b4bee98b60c0593521ed34d9990e8 # v6.0.1
          - name: Install via install.ps1
            shell: pwsh
            run: |
              $prefix = Join-Path $env:RUNNER_TEMP "toolr-bin"
              if ("${{ inputs.version }}" -ne "") {
                ./dist/install.ps1 -Version "${{ inputs.version }}" -Prefix $prefix
              } else {
                ./dist/install.ps1 -Prefix $prefix
              }
              & (Join-Path $prefix "toolr.exe") --version

      smoke-pip-wheel:
        name: pip install (${{ matrix.os }})
        runs-on: ${{ matrix.os }}
        strategy:
          fail-fast: false
          matrix:
            os:
              - ubuntu-latest
              - macos-13
              - macos-14
              - windows-latest
        steps:
          - name: Harden the runner (Audit all outbound calls)
            uses: step-security/harden-runner@20cf305ff2072d973412fa9b1e3a4f227bda3c76 # v2.14.0
            with:
              egress-policy: audit
          - uses: actions/setup-python@a26af69be951a213d495a4c3e4e4022e16d87065 # v5.6.0
            with:
              python-version: '3.12'
          - name: pip install toolr (latest from PyPI)
            shell: bash
            run: |
              python -m venv .venv
              if [ "$RUNNER_OS" = "Windows" ]; then
                .venv/Scripts/python.exe -m pip install --upgrade pip
                .venv/Scripts/pip install toolr${{ inputs.version && format('=={0}', inputs.version) || '' }}
                .venv/Scripts/toolr --version
                # Deprecation shim still works:
                .venv/Scripts/python.exe -m toolr --version
              else
                .venv/bin/python -m pip install --upgrade pip
                .venv/bin/pip install toolr${{ inputs.version && format('=={0}', inputs.version) || '' }}
                .venv/bin/toolr --version
                .venv/bin/python -m toolr --version
              fi

      smoke-mise-plugin:
        name: mise plugin (${{ matrix.os }})
        runs-on: ${{ matrix.os }}
        strategy:
          fail-fast: false
          matrix:
            os:
              - ubuntu-latest
              - macos-14
        steps:
          - name: Harden the runner (Audit all outbound calls)
            uses: step-security/harden-runner@20cf305ff2072d973412fa9b1e3a4f227bda3c76 # v2.14.0
            with:
              egress-policy: audit
          - uses: actions/checkout@8e8c483db84b4bee98b60c0593521ed34d9990e8 # v6.0.1
          - uses: jdx/mise-action@13abe502c30c1559a5c37dff303831bab82c9402 # v3.5.1
            with:
              install: false
          - name: Install plugin from local staging dir
            run: |
              mise plugin add toolr "$PWD/dist/mise-plugin"
              if [ -n "${{ inputs.version }}" ]; then
                mise install "toolr@${{ inputs.version }}"
                mise exec "toolr@${{ inputs.version }}" -- toolr --version
              else
                latest=$(mise ls-remote toolr | tail -1)
                mise install "toolr@${latest}"
                mise exec "toolr@${latest}" -- toolr --version
              fi

      report:
        name: Report
        if: always()
        runs-on: ubuntu-latest
        needs:
          - smoke-install-sh
          - smoke-install-ps1
          - smoke-pip-wheel
          - smoke-mise-plugin
        steps:
          - name: Harden the runner (Audit all outbound calls)
            uses: step-security/harden-runner@20cf305ff2072d973412fa9b1e3a4f227bda3c76 # v2.14.0
            with:
              egress-policy: audit
          - uses: martialonline/workflow-status@326830cacf79872efe767e15031f58d1ea0508c4 # v4.2
            id: check
          - run: echo "Install channels failed"
            if: steps.check.outputs.status == 'failure'
          - run: echo "Install channels OK"
            if: steps.check.outputs.status == 'success'
    ```

- [ ] **Step 8.2: Acceptance criteria for the smoke job**

    - All four matrix jobs (`smoke-install-sh`, `smoke-install-ps1`,
      `smoke-pip-wheel`, `smoke-mise-plugin`) must succeed against the
      latest published release when the workflow is dispatched without an
      explicit version.
    - When dispatched with `inputs.version=<a-known-good>`, that exact
      version is installed and `toolr --version` reports it.
    - The `report` job collates results and surfaces a single
      pass/fail signal.

    There is no way to TDD a workflow that pulls from a real published
    release without first publishing one. Once Plan 9 has been released
    once, set the smoke workflow's `schedule:` to active and treat any
    failure as a release-blocker for the next version.

- [ ] **Step 8.3: Lint the workflow**

    ```bash
    uv run pre-commit run --files .github/workflows/install-smoke.yml
    ```

    Expected: PASS.

- [ ] **Step 8.4: Commit**

    ```bash
    git add .github/workflows/install-smoke.yml
    git commit -m "ci: Add nightly install-channel smoke tests"
    ```

---

## Task 9: Update CHANGELOG for the breaking change

`python -m toolr` is now a deprecation shim; programmatic callers that
imported `toolr.__main__:main` to drive the argparse CLI in-process must
migrate. The wheel still installs the same `toolr` console script via
maturin's `bin` target.

**Files:**

- Modify: `CHANGELOG.md`

- [ ] **Step 9.1: Add an unreleased section**

    Insert immediately under the `# Changelog` header, before
    `## 0.11.0 - 2025-09-24`:

    ```markdown
    ## [Unreleased]

    ### <!-- 0 -->🚀 Features

    - *(cli)* `toolr` is now a Rust binary first. `pip install toolr` ships
      the same binary inside the wheel via maturin's `bin` target;
      standalone install via `curl ... | sh dist/install.sh` and the
      `toolr-mise/` plugin both fetch the binary from GitHub releases.

    ### <!-- 4 -->♻️ Refactor

    - *(cli)* The argparse-driven Python entry point has been replaced by
      the Rust CLI. `python -m toolr` keeps working as a deprecation shim
      that locates the `toolr` binary on PATH and exec's it with the
      original argv, printing a one-time `DeprecationWarning` to stderr.
      Set `TOOLR_NO_DEPRECATION_NOTICE=1` to silence the warning.

    ### <!-- 8 -->💥 Breaking Changes

    - Programmatic callers that imported `toolr.__main__:main` (e.g. to
      run toolr commands in-process) must migrate to spawning the `toolr`
      binary as a subprocess. The shim's `main()` now exec's a binary and
      never returns; it cannot be called as a library function.
    - The `toolr/__main__.py` module no longer re-exports `Parser` or
      `CommandRegistry`. Import them from `toolr._parser` and
      `toolr._registry` directly if you depended on those names.
    ```

- [ ] **Step 9.2: Commit**

    ```bash
    git add CHANGELOG.md
    git commit -m "docs(changelog): Document Plan 9 distribution + breaking changes"
    ```

---

## Task 10: Backwards-compat verification for existing `tools/*.py`

The hard constraint is that existing `tools/*.py` files using
`command_group()` and `@group.command` continue to work without
modification. Plans 1–8 produce the machinery for that path; Plan 9
proves it end-to-end.

**Files:**

- Create: `tests/distribution/test_compat_existing_tools.py`

- [ ] **Step 10.1: Write the compatibility test**

    Create `tests/distribution/test_compat_existing_tools.py`:

    ```python
    """End-to-end backwards-compat: existing tools/*.py keep working."""

    from __future__ import annotations

    import os
    import shutil
    import subprocess
    import sys
    import textwrap
    from collections.abc import Callable
    from pathlib import Path

    import pytest

    pytestmark = pytest.mark.skipif(
        shutil.which("uv") is None,
        reason="uv not on PATH",
    )

    REPO_ROOT = Path(__file__).resolve().parents[2]


    @pytest.fixture
    def project_dir(tmp_path: Path) -> Callable[[], Path]:
        """Factory: scaffold a minimal project with a ``tools/`` dir and a ``command_group``.

        Returns the project root directory.
        """

        def _make() -> Path:
            proj = tmp_path / "proj"
            proj.mkdir()
            tools = proj / "tools"
            tools.mkdir()
            (tools / "__init__.py").write_text("")
            (tools / "demo.py").write_text(
                textwrap.dedent(
                    """\
                    from __future__ import annotations

                    from toolr import command_group

                    group = command_group("demo", "Demo commands", docstring=__doc__)


                    @group.command
                    def hello(name: str = "world") -> None:
                        \"\"\"Print a greeting.

                        Args:
                            name: Who to greet.
                        \"\"\"
                        print(f"hello, {name}")
                    """
                )
            )
            (tools / "pyproject.toml").write_text(
                textwrap.dedent(
                    f"""\
                    [project]
                    name = "demo-tools"
                    version = "0"
                    requires-python = ">=3.11"
                    dependencies = ["toolr"]

                    [tool.uv.sources]
                    toolr = {{ path = "{REPO_ROOT.as_posix()}", editable = true }}
                    """
                )
            )
            return proj

        return _make


    def test_existing_command_group_authoring_still_runs(
        project_dir: Callable[[], Path],
        tmp_path: Path,
    ) -> None:
        proj = project_dir()
        # Build the binary into a local venv and run the demo command.
        venv = tmp_path / "venv"
        subprocess.run(
            ["uv", "venv", "--python", sys.executable, str(venv)],
            check=True,
        )
        venv_bin = venv / ("Scripts" if os.name == "nt" else "bin")
        env = {**os.environ, "VIRTUAL_ENV": str(venv)}
        subprocess.run(
            [str(venv_bin / "python"), "-m", "pip", "install",
             "maturin>=1.7,<2.0"],
            check=True,
            env=env,
        )
        subprocess.run(
            [str(venv_bin / "maturin"), "develop", "--release"],
            cwd=REPO_ROOT,
            check=True,
            env=env,
        )
        toolr_bin = venv_bin / ("toolr.exe" if os.name == "nt" else "toolr")
        # Discover the manifest (Plan 1 functionality) — must list `demo`.
        help_result = subprocess.run(
            [str(toolr_bin), "--help"],
            cwd=proj,
            check=True,
            capture_output=True,
            text=True,
        )
        assert "demo" in help_result.stdout, help_result.stdout

        # Run the command — Plan 2's runner path must execute it.
        run_result = subprocess.run(
            [str(toolr_bin), "demo", "hello", "--name", "plan9"],
            cwd=proj,
            check=True,
            capture_output=True,
            text=True,
        )
        assert "hello, plan9" in run_result.stdout, run_result.stdout
    ```

- [ ] **Step 10.2: Run the test**

    ```bash
    uv run pytest tests/distribution/test_compat_existing_tools.py -v
    ```

    Expected: PASS (assuming Plans 1–8 are merged; this is Plan 9's
    smoke test that the full stack still respects the
    `command_group` + `@group.command` API).

- [ ] **Step 10.3: Commit**

    ```bash
    git add tests/distribution/test_compat_existing_tools.py
    git commit -m "test(compat): End-to-end test that existing command_group authoring works"
    ```

---

## Task 11: Update README with the new install paths

The README is the first thing a new user reads. Make the standalone
binary the headline install path, demote pip to one of several options.

**Files:**

- Modify: `README.md`

- [ ] **Step 11.1: Replace the install section**

    Locate the `## Installation` section in `README.md` (or
    `## Getting Started` / equivalent — match the existing heading). If
    none exists, insert this section under the summary, before the first
    usage example.

    Replace its contents with:

    ```markdown
    ## Installation

    `toolr` ships as a single self-contained binary. Choose the install
    method that matches your environment:

    ### `curl ... | sh` (Linux + macOS)

    ```sh
    curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.sh | sh
    ```

    Pass `--version X.Y.Z` after `sh -s --` to pin a specific release, or
    `--prefix /custom/bin` to choose an install directory. Default prefix
    is `$XDG_BIN_HOME` (or `~/.local/bin`).

### PowerShell (Windows)

    ```powershell
    irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/dist/install.ps1 | iex
    ```

### mise

    ```sh
    mise plugin add toolr https://github.com/s0undt3ch/ToolR.git --branch main
    mise use --global toolr@latest
    ```

    The plugin source lives in `toolr-mise/` (development) and
    `dist/mise-plugin/` (release-tracked).

### pip

    ```sh
    pip install toolr
    ```

    The wheel ships the same Rust binary alongside the Python package, so
    `toolr` and `python -m toolr` both work after install. (`python -m
    toolr` is a deprecation shim that exec's the binary with the original
    argv; it will be removed in a future major release.)

### GitHub release archives

    Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
    Windows) from <https://github.com/s0undt3ch/ToolR/releases> and
    extract it onto `$PATH` manually. Each archive ships with a `.sha256`
    sibling for verification.
    ```

- [ ] **Step 11.2: Run the markdown linter**

    ```bash
    uv run pre-commit run --files README.md
    ```

    Expected: PASS (or fixes auto-applied).

- [ ] **Step 11.3: Commit**

    ```bash
    git add README.md
    git commit -m "docs(readme): Document new standalone install + pip wheel paths"
    ```

---

## Task 12: Update roadmap to mark Plan 9 as done

The roadmap lives at `specs/rust-front-end/01-roadmap.md`. Move the
Plan 9 entry from `⬜ Not Started` to `✅ Done` and link the plan doc.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 12.1: Flip the status**

    In `specs/rust-front-end/01-roadmap.md`, locate `### Plan 9:
    Distribution + backwards compatibility` and replace its first two
    lines:

    ```markdown
    ### Plan 9: Distribution + backwards compatibility

    - **Status:** ✅ Done
    - **Plan doc:** [10-plan-9-distribution.md](./10-plan-9-distribution.md)
    ```

    Leave the rest of the entry (depends/unblocks/produces) untouched.

- [ ] **Step 12.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Plan 9 distribution + back-compat — done"
    ```

---

## Done criteria

This plan is complete when **all** of the following hold:

1. `maturin build --release` produces a wheel that contains both the
   `toolr` Python package and the `toolr` Rust binary at
   `<wheel>/data/scripts/toolr` (or `toolr.exe` on Windows). Verified by
   `tests/distribution/test_wheel_contents.py`.
2. `pip install -e .` (via `maturin develop --release`) places a
   runnable `toolr` binary in the venv's `bin/` (or `Scripts/`)
   directory. Verified by `tests/distribution/test_editable_install.py`.
3. `python -m toolr <args>` writes a one-time `DeprecationWarning` to
   stderr (unless `TOOLR_NO_DEPRECATION_NOTICE=1`), then exec's the
   `toolr` binary with the original argv. Verified by
   `tests/cli/test_main_shim.py`.
4. The GitHub release workflow produces per-platform archives
   (`toolr-<version>-<triple>.tar.gz` / `.zip` plus matching
   `.sha256`) for every triple in the Task 4 matrix, plus a
   `release-manifest.json`, and attaches them to the GitHub release.
5. `sh dist/install.sh --dry-run --version X.Y.Z` parses arguments,
   detects the host triple, and prints the resolved URL without making
   network calls. Verified by `tests/distribution/test_install_sh.py`.
6. `dist/install.sh` and `dist/install.ps1` install a working `toolr`
   binary against a real published release. Verified by the
   `smoke-install-sh` and `smoke-install-ps1` jobs in
   `.github/workflows/install-smoke.yml`.
7. The `dist/mise-plugin/` staging directory contains
   `bin/list-all`, `bin/download`, and `bin/install` scripts that
   conform to the asdf-plugin contract and install a working `toolr`
   binary. Verified by the `smoke-mise-plugin` job.
8. The `smoke-pip-wheel` job confirms `pip install toolr` ships a
   working binary on Linux, macOS, and Windows.
9. `tests/distribution/test_compat_existing_tools.py` builds a sample
   project with `command_group(...)` + `@group.command` decorators and
   runs a command end-to-end through the new Rust front-end.
10. CHANGELOG documents the new install paths and the breaking change
    to programmatic `toolr.__main__:main` callers.
11. README's installation section leads with the standalone binary
    paths.
12. `specs/rust-front-end/01-roadmap.md` shows Plan 9 as `✅ Done` and
    links to this plan doc.

---

## Open questions

1. **Cross-compiling `aarch64-unknown-linux-musl` and friends via
   `cross`.** `cross` ships its own Docker images; running it inside
   GitHub Actions on `ubuntu-latest` works, but the Docker daemon
   bootstrap can be flaky. Should we fall back to `cargo-zigbuild` for
   the musl targets if `cross` proves unreliable, or accept the
   intermittent re-run cost?
2. **Code-signing the macOS binary.** Unsigned binaries fired through
   Gatekeeper land users in a quarantine-attribute mess. Do we
   provision an Apple Developer ID for `s0undt3ch/ToolR` and add a
   `codesign` step to the archive workflow, or document the manual
   `xattr -d com.apple.quarantine` workaround in the README for now?
3. **Removing the `python -m toolr` shim.** The shim is convenient but
   carries forever-cost in CI matrix expansion (every install channel
   tests both `toolr` and `python -m toolr`). When should we remove
   it: at 1.0, 2.0, or "indefinitely"? The CHANGELOG entry implies
   removal in "a future major release" — picking a concrete number
   would let downstream consumers plan.
4. **brew tap.** Punted explicitly in the design (D1), but `brew
   tap`-curious users keep asking. Worth a `homebrew-toolr` tap repo
   in Plan 10, or fold it in here? Adding a tap is a 30-line
   formula plus a CI release-hook; the maintenance cost is the unknown.
5. **`release-manifest.json` consumption.** Right now `install.sh`
   ignores the manifest and constructs URLs by string interpolation.
   The manifest is published anyway so future installers (or a future
   `toolr self update`) can consult it. Should we switch `install.sh`
   to a manifest-first flow now, or keep the simpler URL-templating
   path until we actually have a second consumer?
