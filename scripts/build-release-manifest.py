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
        print(  # noqa: T201
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
