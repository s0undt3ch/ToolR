#!/usr/bin/env python3
"""Pin GitHub Actions to commit SHAs for security."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path


def check_gh_cli() -> bool:
    """Check if gh CLI is available."""
    return shutil.which("gh") is not None


def get_commit_sha(owner: str, repo: str, ref: str) -> str | None:
    """Get the commit SHA for a given ref (tag or branch) using gh CLI."""
    try:
        # Use gh api to get commit info
        result = subprocess.run(
            ["gh", "api", f"/repos/{owner}/{repo}/commits/{ref}"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            return data["sha"]
        return None
    except Exception as e:
        print(f"Warning: Failed to fetch SHA for {owner}/{repo}@{ref}: {e}", file=sys.stderr)
        return None


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

    # Get the commit SHA
    sha = get_commit_sha(owner, repo, ref)
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


def process_file(filepath: Path, use_latest: bool = False) -> bool:
    """Process a workflow file and pin actions. Returns True if changes were made."""
    try:
        content = filepath.read_text()
    except Exception as e:
        print(f"Error reading {filepath}: {e}", file=sys.stderr)
        return False

    lines = content.splitlines(keepends=True)
    modified = False
    new_lines = []

    for line in lines:
        action_info = parse_action_line(line, include_pinned=use_latest)

        if action_info:
            new_line = pin_action(action_info, use_latest=use_latest)
            if new_line:
                if not use_latest:
                    print(f"Pinning {action_info['action_path']}@{action_info['ref']} -> SHA")
                new_lines.append(new_line + "\n" if not new_line.endswith("\n") else new_line)
                modified = True
            else:
                new_lines.append(line)
        else:
            new_lines.append(line)

    if modified:
        filepath.write_text("".join(new_lines))
        print(f"Updated: {filepath}")

    return modified


def main() -> int:
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Pin GitHub Actions to commit SHAs for security.")
    parser.add_argument("files", nargs="+", help="Workflow files to process")
    parser.add_argument(
        "--latest",
        action="store_true",
        help="Update all actions to their latest release versions",
    )

    args = parser.parse_args()

    # Check if gh CLI is available
    if not check_gh_cli():
        print("Warning: gh CLI not found. Skipping GitHub Actions pinning.", file=sys.stderr)
        print("Install gh CLI from: https://cli.github.com/", file=sys.stderr)
        return 0  # Skip gracefully

    modified_files = []

    for filepath_str in args.files:
        filepath = Path(filepath_str)

        if not filepath.exists():
            print(f"Error: {filepath} does not exist", file=sys.stderr)
            continue

        if filepath.suffix not in {".yml", ".yaml"}:
            continue

        if process_file(filepath, use_latest=args.latest):
            modified_files.append(filepath)

    if modified_files:
        action = "Updated" if args.latest else "Pinned"
        print(f"\n{action} actions in {len(modified_files)} file(s)")
        return 1  # Return 1 to indicate files were modified (pre-commit should fail)

    return 0


if __name__ == "__main__":
    sys.exit(main())
