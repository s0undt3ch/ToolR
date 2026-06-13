#!/usr/bin/env python3
"""Pin GitHub Actions to commit SHAs for security."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from collections.abc import Callable
from functools import lru_cache
from pathlib import Path


class TransientResolutionError(Exception):
    """A ref could not be resolved for a *transient* reason — rate limiting,
    a network/connection failure, a timeout, an auth/5xx response, or a
    malformed reply — rather than the ref genuinely not existing.

    Callers should treat this as "unknown, try again later" (warn and skip),
    **not** as a verification failure. A definitive "this tag/repo does not
    exist" (HTTP 404) is reported as ``None`` instead, which *is* a real
    problem with the pin.
    """


def check_gh_cli() -> bool:
    """Check if gh CLI is available."""
    return shutil.which("gh") is not None


@lru_cache(maxsize=None)
def get_commit_sha(owner: str, repo: str, ref: str) -> str | None:
    """Get the commit SHA for a given ref (tag or branch) using gh CLI.

    Returns the resolved SHA on success, or ``None`` when GitHub reports the
    ref genuinely does not exist (HTTP 404) — a real, actionable pin problem.
    Raises :class:`TransientResolutionError` for everything else (rate limit,
    auth, 5xx, network, timeout, malformed response): the ref's existence is
    *unknown*, so the caller must not treat it as a verification failure.

    In-process memoised on ``(owner, repo, ref)``: a workflow/action set
    typically pins the same action+version many times over, so this
    resolves each unique ``owner/repo@ref`` once per run instead of
    re-querying `gh api` for every occurrence. The cache lives for the
    process (one hook invocation), so it never serves stale data across
    runs. (``lru_cache`` does not memoise raised exceptions, so a transient
    failure is retried on the next occurrence rather than poisoning the run.)
    """
    try:
        # Use gh api to get commit info
        result = subprocess.run(
            ["gh", "api", f"/repos/{owner}/{repo}/commits/{ref}"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except Exception as e:
        # Timeout, gh vanished mid-run, OSError, ... — all transient.
        raise TransientResolutionError(f"failed to invoke gh for {owner}/{repo}@{ref}: {e}") from e

    if result.returncode == 0:
        try:
            return json.loads(result.stdout)["sha"]
        except (json.JSONDecodeError, KeyError) as e:
            raise TransientResolutionError(
                f"malformed gh response for {owner}/{repo}@{ref}: {e}"
            ) from e

    stderr = (result.stderr or "").strip()
    # Definitive "does not exist" — the only case that is a real pin problem.
    if "HTTP 404" in stderr or "Not Found" in stderr:
        return None
    # Rate limit, auth, 5xx, connection reset, ... — existence still unknown.
    raise TransientResolutionError(
        f"could not reach GitHub to resolve {owner}/{repo}@{ref}: "
        f"{stderr or f'gh exited {result.returncode}'}"
    )


def get_latest_release(owner: str, repo: str) -> str | None:
    """Get the latest release tag for a repository using gh CLI."""
    try:
        result = subprocess.run(
            ["gh", "api", f"/repos/{owner}/{repo}/releases/latest"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            return data["tag_name"]
        return None
    except Exception as e:
        print(f"Warning: Failed to fetch latest release for {owner}/{repo}: {e}", file=sys.stderr)
        return None


def parse_action_line(line: str, include_pinned: bool = False) -> dict[str, str] | None:
    """Parse a GitHub Actions 'uses' line."""
    # Match: uses: owner/repo@ref or uses: owner/repo/path@ref
    match = re.match(r"(\s*-?\s*uses:\s+)([^/]+/[^@\s]+)@([^\s#]+)(\s*#.*)?", line)
    if not match:
        return None

    indent, action_path, ref, comment = match.groups()
    comment = comment or ""

    # Skip if already pinned to a SHA (40 hex chars), unless include_pinned is True
    is_pinned = re.match(r"^[0-9a-f]{40}$", ref)
    if is_pinned and not include_pinned:
        return None

    # Skip local actions (start with ./)
    if action_path.startswith("./"):
        return None

    return {
        "indent": indent,
        "action_path": action_path,
        "ref": ref,
        "comment": comment,
        "original_line": line,
        "is_pinned": bool(is_pinned),
    }


def verify_action_line(
    line: str,
    resolve: "Callable[[str, str, str], str | None]" = get_commit_sha,
) -> str | None:
    """Verify a single `uses:` line's pin against its `# <tag>` comment.

    Returns an error message string when the line is a SHA pin that
    cannot be verified or whose pinned SHA does not match the SHA the
    comment's tag currently resolves to. Returns ``None`` when the line
    is fine (or is not a verifiable pin — e.g. a not-yet-pinned tag ref,
    a local action, or a non-`uses:` line; those are out of scope here).

    `resolve(owner, repo, ref) -> sha | None` is injected so tests can
    supply a fake resolver and avoid any network / `gh` dependency.
    """
    info = parse_action_line(line, include_pinned=True)
    if info is None:
        return None

    action_path = info["action_path"]

    # Not-yet-pinned tag refs are the autofix hook's job, not verify's.
    if not info["is_pinned"]:
        return None

    pinned_sha = info["ref"]

    # Extract the tag from the trailing comment, e.g. "# v6.0.3" -> "v6.0.3".
    comment_match = re.match(r"\s*#\s*(\S+)", info["comment"])
    if not comment_match:
        return f"{action_path} is pinned to {pinned_sha} but has no '# <tag>' comment to verify against"

    tag = comment_match.group(1)

    parts = action_path.split("/", 2)
    if len(parts) < 2:
        return f"{action_path} could not be parsed into owner/repo"
    owner, repo = parts[0], parts[1]

    try:
        expected_sha = resolve(owner, repo, tag)
    except TransientResolutionError as e:
        # Existence unknown (rate limit / network / timeout). Don't fail the
        # run over a problem that isn't the pin's fault — warn and skip.
        print(
            f"Warning: skipping pin check for {action_path}@{pinned_sha} # {tag}: {e}",
            file=sys.stderr,
        )
        return None

    if expected_sha is None:
        # Definitive 404: the tag genuinely does not exist — a real problem.
        return (
            f"{action_path}@{pinned_sha} # {tag}: "
            f"tag '{tag}' does not exist — could not resolve tag '{tag}' to a SHA"
        )

    if expected_sha != pinned_sha:
        return (
            f"{action_path} pin mismatch for tag '{tag}': "
            f"expected {expected_sha} but file has {pinned_sha}"
        )

    return None


def pin_action(action_info: dict[str, str], use_latest: bool = False) -> str | None:
    """Pin an action to its commit SHA."""
    action_path = action_info["action_path"]
    ref = action_info["ref"]

    # Parse owner/repo (handle owner/repo/path cases)
    parts = action_path.split("/", 2)
    if len(parts) < 2:
        return None

    owner, repo = parts[0], parts[1]

    # If --latest flag is set, get the latest release
    if use_latest:
        latest_ref = get_latest_release(owner, repo)
        if latest_ref:
            print(f"Updating {action_path}@{ref} -> {latest_ref}")
            ref = latest_ref
        else:
            print(f"Warning: Could not fetch latest release for {action_path}, using current ref")

    # Get the commit SHA. A transient resolution failure (rate limit /
    # network / timeout) leaves the ref unpinned for this run rather than
    # crashing the whole pass — warn and move on.
    try:
        sha = get_commit_sha(owner, repo, ref)
    except TransientResolutionError as e:
        print(f"Warning: leaving {action_path}@{ref} unpinned this run: {e}", file=sys.stderr)
        return None
    if not sha:
        return None

    # Construct new line with SHA and comment showing the original ref
    new_comment = action_info["comment"].strip()
    if new_comment and not use_latest:
        # Keep existing comment
        new_line = f"{action_info['indent']}{action_path}@{sha} {new_comment}"
    else:
        # Add comment with original ref
        new_line = f"{action_info['indent']}{action_path}@{sha} # {ref}"

    return new_line


def process_file(filepath: Path, use_latest: bool = False) -> tuple[bool, list[str]]:
    """Pin unpinned `uses:` refs and verify already-pinned ones in one pass.

    Returns ``(modified, errors)``:
    - ``modified`` — an unpinned ref was rewritten to a SHA (or, under
      ``--latest``, every ref was re-pinned to its newest release).
    - ``errors`` — already-pinned refs whose SHA no longer matches their
      ``# <tag>`` comment. Verification is skipped under ``--latest`` (which
      re-pins everything from scratch anyway).
    """
    try:
        content = filepath.read_text()
    except Exception as e:
        return False, [f"{filepath}: could not read file: {e}"]

    modified = False
    errors: list[str] = []
    new_lines: list[str] = []

    for lineno, line in enumerate(content.splitlines(keepends=True), start=1):
        info = parse_action_line(line, include_pinned=True)
        if info is None:
            new_lines.append(line)
            continue

        if not use_latest and info["is_pinned"]:
            # Already pinned: verify it matches its tag comment; never rewrite.
            error = verify_action_line(line)
            if error:
                errors.append(f"{filepath}:{lineno}: {error}")
            new_lines.append(line)
            continue

        # Unpinned ref (or a `--latest` re-pin): rewrite to a SHA.
        new_line = pin_action(info, use_latest=use_latest)
        if new_line:
            if not use_latest:
                print(f"Pinning {info['action_path']}@{info['ref']} -> SHA")
            new_lines.append(new_line if new_line.endswith("\n") else new_line + "\n")
            modified = True
        else:
            new_lines.append(line)

    if modified:
        filepath.write_text("".join(new_lines))
        print(f"Updated: {filepath}")

    return modified, errors


def main() -> int:
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description=(
            "Lock GitHub Action `uses:` refs to commit SHAs and verify existing "
            "pins against their tag comments. Default: pin unpinned refs and "
            "verify already-pinned ones in a single pass."
        )
    )
    parser.add_argument("files", nargs="+", help="Workflow / action files to process")
    parser.add_argument(
        "--latest",
        action="store_true",
        help="Re-pin every action to its latest release SHA (instead of verifying).",
    )
    parser.add_argument(
        "--exit-zero",
        action="store_true",
        help=(
            "Report pin problems but exit 0 (advisory); also exit 0 — rather than "
            "hard-erroring — when the gh CLI is unavailable."
        ),
    )

    args = parser.parse_args()

    # gh is required to resolve refs. By default a missing gh is a HARD error
    # (exit 2) — never a silent skip, so "cannot verify" can't masquerade as
    # "verified, all good". `--exit-zero` is the explicit opt-out: it
    # downgrades the missing-gh case to a warning + exit 0 (for advisory /
    # best-effort runs and gh-less contributors).
    if not check_gh_cli():
        if args.exit_zero:
            print(
                "Warning: gh CLI not found; skipping action-pin checks (--exit-zero).",
                file=sys.stderr,
            )
            return 0
        print("Error: gh CLI not found; cannot resolve or verify action pins.", file=sys.stderr)
        print("Install gh CLI from: https://cli.github.com/", file=sys.stderr)
        return 2

    errors: list[str] = []
    modified_any = False

    for filepath_str in args.files:
        filepath = Path(filepath_str)
        if not filepath.exists():
            print(f"Error: {filepath} does not exist", file=sys.stderr)
            continue
        if filepath.suffix not in {".yml", ".yaml"}:
            continue

        modified, file_errors = process_file(filepath, use_latest=args.latest)
        modified_any = modified_any or modified
        errors.extend(file_errors)

    if errors:
        print("Action pin verification problems:", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        print(f"\n{len(errors)} problem(s) found.", file=sys.stderr)

    if args.exit_zero:
        # Advisory: report the findings above, but never fail on them. (A
        # missing gh already hard-errored before we got here.)
        return 0

    # Non-zero when we rewrote files (pre-commit should re-stage) or found a
    # pin that no longer matches its tag comment.
    return 1 if (errors or modified_any) else 0


if __name__ == "__main__":
    sys.exit(main())
