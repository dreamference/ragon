"""Root conftest: no pytest run started anywhere under the checkout sees the real home folder.

`tests/conftest.py` gives every test of `tests/` its own home, but a suite run from another
folder (a scratch test, a run that names a file elsewhere) never
loads it, and such a run wrote its fixtures over the real `~/.ssh/authorized_keys` and locked the
night scheduler out. This file is above every test folder, so it always loads; it
only moves HOME, and `tests/conftest.py` then does the rest for its own tests.
"""

import pytest


@pytest.fixture(autouse=True)
def _scratch_home_below_every_folder(tmp_path_factory, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path_factory.mktemp("root-home")))
