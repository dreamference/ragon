"""The 1.0 install matrix runner: no pass without evidence, windows rows named, check gates."""

import importlib.util
import json
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "release_matrix.py"
spec = importlib.util.spec_from_file_location("release_matrix", SCRIPT)
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)


def run(tmp_path, *args):
    return matrix.main(["--state", str(tmp_path / "m.json"), *args])


def test_the_matrix_has_the_spec_rows_and_columns():
    cell_list = matrix.cells()
    assert ("gb10-node", "rollback") in cell_list
    assert ("amd-windows-node", "rollback") in cell_list
    assert ("linux-client", "rollback") not in cell_list
    assert [c for c in cell_list if c[0] == "bridges"] == [("bridges", "update")]
    assert len(cell_list) == 3 * 3 + 7 * 2 + 1


def test_a_pass_without_evidence_is_refused(tmp_path, capsys):
    assert run(tmp_path, "record", "gb10-node", "fresh", "pass") == matrix.ERROR
    assert run(tmp_path, "record", "gb10-node", "fresh", "pass", "--evidence", "  ") == matrix.ERROR
    assert "evidence" in capsys.readouterr().err
    assert not (tmp_path / "m.json").exists()


def test_unknown_cells_are_refused(tmp_path):
    assert (
        run(tmp_path, "record", "linux-client", "rollback", "fail", "--evidence", "x")
        == matrix.ERROR
    )
    assert run(tmp_path, "record", "nope", "fresh", "fail") == matrix.ERROR


def test_check_needs_every_cell_passed_with_evidence(tmp_path):
    assert run(tmp_path, "check") == matrix.OPEN
    for row, column in matrix.cells():
        assert run(tmp_path, "record", row, column, "pass", "--evidence", f"{row} {column} ok") == 0
    assert run(tmp_path, "check") == matrix.OK
    assert run(tmp_path, "record", "bridges", "update", "fail", "--evidence", "bridge on") == 0
    assert run(tmp_path, "check") == matrix.OPEN


def test_a_hand_edited_pass_without_evidence_counts_as_open(tmp_path):
    (tmp_path / "m.json").write_text(json.dumps({"cells": {"gb10-node/fresh": {"result": "pass"}}}))
    assert matrix.status(matrix.load(tmp_path / "m.json"))["gb10-node/fresh"] == "open"


def test_show_names_the_windows_rows_and_evidence_file_works(tmp_path, capsys):
    note = tmp_path / "out.txt"
    note.write_text("ragon 0.9.1 started; conversation ok")
    assert run(tmp_path, "record", "gb10-node", "update", "pass", "--evidence-file", str(note)) == 0
    capsys.readouterr()
    assert run(tmp_path, "show") == 0
    shown = capsys.readouterr().out
    assert "AMD Windows node" in shown and "[windows programme]" in shown
    assert "update:✓" in shown
    assert run(tmp_path, "show", "--json") == 0
    data = json.loads(capsys.readouterr().out)
    assert data["cells"]["gb10-node/update"]["evidence"].startswith("ragon 0.9.1")


def test_a_corrupt_state_file_is_an_error(tmp_path):
    (tmp_path / "m.json").write_text("{nope")
    assert run(tmp_path, "show") == matrix.ERROR
