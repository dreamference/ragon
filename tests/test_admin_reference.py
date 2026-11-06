"""The product site's `ragon-admin` reference is generated from the CLI and must stay in step with it."""

import importlib.util
import os
import pathlib

ROOT = str(pathlib.Path(__file__).resolve().parent.parent)


def test_docs_admin_reference_matches_the_cli():
    spec = importlib.util.spec_from_file_location(
        "gen_admin_reference", os.fspath(pathlib.Path(ROOT, "scripts", "gen_admin_reference.py"))
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with pathlib.Path(ROOT, "docs", "admin.md").open() as handle:
        committed = handle.read()
    assert module.generate() == committed, (
        "docs/admin.md is out of date: run `.venv/bin/python scripts/gen_admin_reference.py`"
    )
