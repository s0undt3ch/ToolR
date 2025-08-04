#!/usr/bin/env python3
"""
This tool will create documentation stubs for the whole codebase.
"""

import os
import shutil
import argparse
import pathlib
import subprocess

# Define a few directories
REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC_ROOT = REPO_ROOT / "python"
DOC_REF_ROOT = REPO_ROOT / "docs" / "reference"

# Force the working directory to the root of the repository
os.chdir(REPO_ROOT)


def main():
    parser = argparse.ArgumentParser(description="Create Doc Stubs")
    parser.add_argument(
        "--ignore-pattern",
        "-I",
        action="append",
        default=[],
        help="Ignore files matching the passed pattern(fnmatch). One pattern per flag invocation.",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=pathlib.Path,
        help="The output directory to write the stubs to: %(default)s",
        default=DOC_REF_ROOT,
    )
    parser.add_argument(
        "--force-rebuild",
        action="store_true",
        help="Force the rebuild of all documentation stubs.",
        default=False,
    )

    args = parser.parse_args()
    if not args.ignore_pattern:
        args.ignore_pattern.extend(
            [
                "**/__init__.py",
                "toolr/__main__.py",
                "toolr/_context.pyi",
                "toolr/utils/_command.pyi",
                "toolr/utils/_context.pyi",
            ]
        )

    if args.force_rebuild:
        print("Forcing rebuild of all documentation stubs ...")
        try:
            subprocess.run(["git", "rm", "-f", str(DOC_REF_ROOT)], check=True, capture_output=True)
        except subprocess.CalledProcessError:
            shutil.rmtree(DOC_REF_ROOT)

    py_file_tree: list[pathlib.Path] = []

    for fpath in SRC_ROOT.rglob("*.py"):
        if not args.ignore_pattern:
            py_file_tree.append(fpath.relative_to(SRC_ROOT))
            continue

        skip_file = False
        for pattern in args.ignore_pattern:
            if fpath.match(pattern):
                skip_file = True
                break
        if skip_file:
            continue
        py_file_tree.append(fpath.relative_to(SRC_ROOT))

    md_file_tree = set(DOC_REF_ROOT.rglob("*.md"))

    for fpath in sorted(py_file_tree):
        modname = ".".join(fpath.with_suffix("").parts)
        ref_path = DOC_REF_ROOT / fpath.with_suffix(".md")
        ref_path.parent.mkdir(parents=True, exist_ok=True)
        if not ref_path.exists():
            print(f"Creating {ref_path} ...")
            ref_path.write_text(f"# {modname}\n\n::: {modname}\n")
            subprocess.run(["git", "add", str(ref_path)], check=True, capture_output=True)
            continue
        md_file_tree.remove(ref_path)

    for fpath in sorted(md_file_tree):
        print(f"Removing {fpath} ...")
        try:
            subprocess.run(["git", "rm", "-f", fpath], check=True, capture_output=True)
        except subprocess.CalledProcessError as exc:
            fpath.unlink()


if __name__ == "__main__":
    main()
