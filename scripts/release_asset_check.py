"""Fail a release that would ship fewer assets than the previous published release.

The release workflow (``.github/workflows/release.yml``) runs this before it publishes: the names
of the files about to be uploaded are compared with the names of the previous release's assets,
the previous version replaced by the new one in each name. A name that is missing stops the run,
unless ``--allow-fewer`` is given (the workflow's ``allow_fewer_assets`` input), in which case the
missing names are printed as a warning and the run goes on.

Usage: release_asset_check.py --version 0.9.3 --previous-version 0.9.2 \\
           --previous-assets previous.txt --files artifacts [--allow-fewer]

``--previous-assets`` is a file with one asset name per line; ``--files`` is the directory the
release's files were downloaded to (searched recursively) and ``--extra`` names more files
(SHA256SUMS and its signature). Standard library only.
"""

import argparse
import sys
from pathlib import Path


def missing_assets(
    previous_names: list[str], new_names: list[str], previous_version: str, version: str
) -> list[str]:
    """Return the previous release's asset names (version substituted) that the new files lack.

    Args:
        previous_names: asset names of the previous published release.
        new_names: names of the files the new release will carry.
        previous_version: the previous release's version, as it appears in names.
        version: the version being released.

    Returns:
        The expected names that are not among ``new_names``, sorted.
    """
    have = set(new_names)
    expected = {name.replace(previous_version, version) for name in previous_names if name}
    return sorted(expected - have)


def main(argv: list[str] | None = None) -> int:
    """Run the check; exit 1 when assets are missing and fewer are not allowed."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--version", required=True)
    parser.add_argument("--previous-version", required=True)
    parser.add_argument("--previous-assets", required=True, type=Path)
    parser.add_argument("--files", required=True, type=Path)
    parser.add_argument("--extra", nargs="*", default=[])
    parser.add_argument("--allow-fewer", action="store_true")
    args = parser.parse_args(argv)
    previous = args.previous_assets.read_text(encoding="utf-8").split("\n")
    names = [p.name for p in args.files.rglob("*") if p.is_file()]
    names += [Path(e).name for e in args.extra]
    missing = missing_assets(
        [n.strip() for n in previous], names, args.previous_version, args.version
    )
    if not missing:
        print(
            f"All {len([n for n in previous if n.strip()])} asset names of the previous release are present."
        )
        return 0
    listing = ", ".join(missing)
    if args.allow_fewer:
        print(
            f"::warning::Publishing with fewer assets than v{args.previous_version} (allowed by input): missing {listing}"
        )
        return 0
    print(
        f"::error::Missing against v{args.previous_version}: {listing}. Nothing was published; "
        "dispatch with allow_fewer_assets to release without them."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
