"""Leftover branches of NodeProvisioner: host choice, connecting, model fetch, dry run, pairing.

Every ssh session, pairing call and Hugging Face fetch is a stand-in; nothing leaves the test.
"""

import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

from dreamference.node import node_provisioner
from dreamference.node.fleet_bundle import FleetBundle
from dreamference.node.node_provisioner import NodeProvisioner

NODE = "node-b"


@pytest.fixture
def prov(tmp_path, monkeypatch):
    """A provisioner for one machine that never sleeps, with stubbed resolver and pairing."""
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    monkeypatch.setattr(node_provisioner.time, "sleep", lambda seconds: None)
    monkeypatch.setattr(FleetBundle, "version", classmethod(lambda cls, bundle: "v1"))
    made = NodeProvisioner([NODE], {"user": "owner", "model": "org/model"})
    made.bundle = Path(tmp_path / "bundle")
    made.plan = {"folders": ["f1"], "copy_images": ["i1"], "pull_images": ["i2"], "image": "img"}
    made.results[NODE] = {"host": NODE, "done": [], "probe": {"hub": "/hub"}}
    return made


def _answer(code=0, err="", out=""):
    return SimpleNamespace(returncode=code, stderr=err, stdout=out)


class _Session:
    """A session with canned answers; every command is recorded."""

    host = NODE
    user = "owner"

    def __init__(self, code=0, pipe=0, sudo_code=0):
        self.code, self.pipe, self.sudo_code = code, pipe, sudo_code
        self.admin = []
        self.identity = None
        self.askpass_env = None

    def run(self, command, timeout=None):
        return _answer(self.code, err="bad", out="bad")

    def run_admin(self, args, docker=False):
        self.admin.append(args)
        return _answer(self.code, out="bad")

    def sudo(self, command, password):
        return _answer(self.sudo_code, out="sudo bad")

    def rsync(self, source, dest, attempts=1):
        return _answer(self.code, err="rsync bad")

    def pipe_into(self, commands, remote):
        return self.pipe

    def host_key_lines(self):
        return ["key"]

    def host_key_fingerprint(self):
        return None

    def open(self):
        return self.code == 0


# -- run -----------------------------------------------------------------------------------------


def test_run_stops_when_the_model_or_hosts_are_refused(prov, monkeypatch):
    monkeypatch.setattr(prov, "_check_model", lambda: False)
    assert prov.run() == 1
    monkeypatch.setattr(prov, "_check_model", lambda: True)
    monkeypatch.setattr(prov, "_resolve_hosts", lambda: False)
    assert prov.run() == 1


def test_run_stops_when_the_bundle_cannot_be_built(prov, monkeypatch):
    monkeypatch.setattr(prov, "_check_model", lambda: True)
    monkeypatch.setattr(prov, "_resolve_hosts", lambda: True)
    monkeypatch.setattr(FleetBundle, "build", classmethod(lambda cls, source: None))
    assert prov.run() == 1


def test_run_finishes_a_role_warns_about_mesh_and_reports_incomplete(prov, monkeypatch, capsys):
    monkeypatch.setattr(prov, "_check_model", lambda: True)
    monkeypatch.setattr(prov, "_resolve_hosts", lambda: True)
    monkeypatch.setattr(FleetBundle, "build", classmethod(lambda cls, source: Path("b")))
    monkeypatch.setattr(node_provisioner.FleetModelPlan, "plan", classmethod(lambda c, m: {}))
    finished = []
    prov.role = SimpleNamespace(finish=lambda host, session: finished.append(host))
    prov.options["mesh"] = True
    prov.sessions[NODE] = object()
    monkeypatch.setattr(prov, "_questions_first", lambda: None)
    monkeypatch.setattr(prov, "_provision", lambda host: None)
    monkeypatch.setattr(prov, "_close_all", lambda: None)
    monkeypatch.setattr(prov, "print_summary", lambda: None)
    assert prov.run() == 1
    assert finished == [NODE]
    assert "--mesh is not built yet" in capsys.readouterr().out


def test_run_starts_a_paired_host_when_there_is_no_role(prov, monkeypatch):
    monkeypatch.setattr(prov, "_check_model", lambda: True)
    monkeypatch.setattr(prov, "_resolve_hosts", lambda: True)
    monkeypatch.setattr(FleetBundle, "build", classmethod(lambda cls, source: Path("b")))
    monkeypatch.setattr(node_provisioner.FleetModelPlan, "plan", classmethod(lambda c, m: {}))
    for name in ("_questions_first", "_close_all", "print_summary"):
        monkeypatch.setattr(prov, name, lambda: None)
    started = []
    monkeypatch.setattr(prov, "_start_and_verify", started.append)
    prov.results[NODE].update(paired_record={"x": 1}, complete=True)
    assert prov.run() == 0
    assert started == [NODE]


# -- choosing the machines -----------------------------------------------------------------------


def test_an_unknown_model_is_refused(prov, capsys):
    prov.model = "no/such-model"
    assert prov._check_model() is False
    assert "not a model of the matrix" in capsys.readouterr().out


def test_all_with_no_paired_nodes_fails(prov, monkeypatch, capsys):
    prov.options["all"] = True
    monkeypatch.setattr(node_provisioner.NodePairing, "paired", classmethod(lambda c: []))
    assert prov._resolve_hosts() is False
    assert "No paired nodes" in capsys.readouterr().out


def test_all_places_every_paired_node_by_name_or_id(prov, monkeypatch):
    prov.options["all"] = True
    records = [{"name": "alpha", "node": "1234567890"}, {"node": "abcdefghij"}]
    monkeypatch.setattr(node_provisioner.NodePairing, "paired", classmethod(lambda c: records))
    placed = []
    monkeypatch.setattr(prov, "_place", lambda host, record: placed.append(host))
    assert prov._resolve_hosts() is True
    assert prov.hosts == ["alpha", "abcdefgh"]
    assert placed == ["alpha", "abcdefgh"]


def test_no_named_host_and_nothing_on_the_network_fails(prov, monkeypatch):
    prov.hosts = []
    monkeypatch.setattr(prov, "_choose_from_network", lambda: False)
    assert prov._resolve_hosts() is False


def _place_with(prov, monkeypatch, resolvable, record, resolved=None):
    monkeypatch.setattr(prov, "_resolves", lambda name: name in resolvable)
    monkeypatch.setattr(
        node_provisioner.NodePairing, "overlay_domain", classmethod(lambda c: "overlay.test")
    )
    monkeypatch.setattr(node_provisioner.NodePairing, "resolve", classmethod(lambda c, r: resolved))
    prov._place(NODE, record)


def test_place_takes_the_records_user_overlay_name_and_first_resolving_candidate(prov, monkeypatch):
    prov.options.pop("user", None)
    record = {"user": "rec", "overlay_name": "ov.test", "name": "other"}
    _place_with(prov, monkeypatch, {"ov.test"}, record)
    assert prov.users[NODE] == "rec"
    assert prov.connect[NODE] == "ov.test"


def test_place_tries_the_records_own_name_last(prov, monkeypatch):
    _place_with(prov, monkeypatch, {"other.local"}, {"name": "other"})
    assert prov.connect[NODE] == "other.local"


def test_place_falls_back_to_a_browse_address(prov, monkeypatch):
    _place_with(prov, monkeypatch, set(), {"name": NODE}, resolved="10.0.0.9")
    assert prov.connect[NODE] == "10.0.0.9"


def test_place_fails_a_paired_node_that_is_off_the_network(prov, monkeypatch):
    _place_with(prov, monkeypatch, set(), {"name": NODE})
    assert prov.results[NODE]["failed"] == "connect"
    assert "off the network" in prov.results[NODE]["reason"]


def test_place_leaves_an_unknown_machine_to_ssh(prov, monkeypatch):
    _place_with(prov, monkeypatch, set(), None)
    assert prov.connect[NODE] == f"{NODE}.local"


def test_resolves_is_false_when_the_resolver_fails(monkeypatch):
    def refuse(*args, **kwargs):
        raise OSError("no")

    monkeypatch.setattr(node_provisioner.socket, "getaddrinfo", refuse)
    assert NodeProvisioner._resolves("x") is False
    monkeypatch.setattr(node_provisioner.socket, "getaddrinfo", lambda *a, **k: [1])
    assert NodeProvisioner._resolves("x") is True


def _browser(monkeypatch, found):
    from dreamference.node import node_browser

    monkeypatch.setattr(
        node_browser.NodeBrowser, "unprovisioned", classmethod(lambda c, match=None: found)
    )


def test_choose_from_network_with_nothing_found(prov, monkeypatch, capsys):
    _browser(monkeypatch, [])
    assert prov._choose_from_network() is False
    assert "No unprovisioned GB10" in capsys.readouterr().out


def test_choose_from_network_without_a_terminal_only_lists(prov, monkeypatch, capsys):
    _browser(monkeypatch, [{"host": "a", "address": "1"}])
    monkeypatch.setattr(sys, "stdin", SimpleNamespace(isatty=lambda: False))
    assert prov._choose_from_network() is False
    assert "Name the ones to provision" in capsys.readouterr().out


def test_choose_from_network_takes_the_numbers_typed(prov, monkeypatch):
    found = [{"host": "a", "address": "1"}, {"host": "b", "address": "2"}]
    _browser(monkeypatch, found)
    monkeypatch.setattr(sys, "stdin", SimpleNamespace(isatty=lambda: True))
    monkeypatch.setattr("builtins.input", lambda prompt: "2, 9, x")
    assert prov._choose_from_network() is True
    assert prov.hosts == ["b"]


# -- questions first, passwords, sessions --------------------------------------------------------


def test_questions_first_asks_the_password_and_fails_a_host_that_will_not_open(prov, monkeypatch):
    started = []

    class _Askpass:
        def __init__(self, run_dir, passwords):
            self.passwords = passwords

        def start(self, executable):
            started.append(executable)

    monkeypatch.setattr(node_provisioner, "FleetAskpass", _Askpass)
    monkeypatch.setattr(node_provisioner.getpass, "getpass", lambda prompt: "pw")
    prov.role = SimpleNamespace()
    prov.hosts = [NODE, "node-c"]
    prov.results["node-c"] = {"failed": "connect"}
    monkeypatch.setattr(prov, "_session_for", lambda host: _Session(code=1))
    monkeypatch.setattr(prov, "_open", lambda session: False)
    prov._questions_first()
    assert started
    assert prov.results[NODE]["failed"] == "connect"
    assert prov.sessions == {}


def test_questions_first_prints_the_host_key_of_an_open_session(prov, monkeypatch, capsys):
    session = _Session()
    monkeypatch.setattr(prov, "_session_for", lambda host: session)
    monkeypatch.setattr(prov, "_open", lambda s: True)
    prov._questions_first()
    assert prov.sessions[NODE] is session
    assert "host key SHA256:?" in capsys.readouterr().out


def test_ask_passwords_per_host_and_shared(prov, monkeypatch):
    monkeypatch.setattr(node_provisioner.getpass, "getpass", lambda prompt: "pw")
    prov.options["per_host_password"] = True
    prov._ask_passwords()
    assert prov.passwords == {NODE: "pw"}
    prov.options["per_host_password"] = False
    prov.hosts = [NODE, "node-c"]
    prov._ask_passwords()
    assert prov.passwords == {NODE: "pw", "node-c": "pw"}


def test_address_prefers_via_for_a_single_host(prov):
    prov.options["via"] = "jump"
    assert prov._address(NODE) == "jump"


def test_session_for_sets_identity_and_askpass(prov, monkeypatch):
    monkeypatch.setattr(
        node_provisioner.NodePairing, "ssh_config", classmethod(lambda c, a: ("u", "idfile"))
    )
    prov.role = SimpleNamespace()
    prov.askpass = SimpleNamespace(environment=lambda address: {"A": "1"})
    session = prov._session_for("we ird")
    assert session.identity == "idfile"
    assert session.askpass_env == {"A": "1"}


def test_open_asks_for_the_hosts_own_password_after_a_refusal(prov, monkeypatch):
    attempts = iter([False, True])
    session = SimpleNamespace(open=lambda: next(attempts), host=NODE, user="owner")
    assert prov._open(SimpleNamespace(open=lambda: False)) is False
    prov.askpass = object()
    monkeypatch.setattr(node_provisioner.getpass, "getpass", lambda prompt: "own")
    assert prov._open(session) is True
    assert prov.passwords[NODE] == "own"


# -- the steps -----------------------------------------------------------------------------------


def test_provision_describes_a_dry_run_after_the_probe(prov, monkeypatch):
    prov.sessions[NODE] = _Session()
    prov.options["dry_run"] = True
    described = []
    monkeypatch.setattr(prov, "_step_probe", lambda h, s: True)
    monkeypatch.setattr(prov, "_describe_dry_run", described.append)
    prov.role = SimpleNamespace(
        steps=lambda steps: steps, describe=lambda h, s: described.append("r")
    )
    prov._provision(NODE)
    assert described == [NODE, "r"]


def test_probe_failure_and_not_a_gb10(prov, monkeypatch):
    prov.plan = {"folders": [], "copy_images": [], "pull_images": []}
    monkeypatch.setattr(node_provisioner.FleetProbe, "script", classmethod(lambda c, f, i: "s"))
    assert prov._step_probe(NODE, _Session(code=1)) is False
    assert prov.results[NODE]["failed"] == "read the state"
    monkeypatch.setattr(node_provisioner.FleetProbe, "parse", classmethod(lambda c, o: {}))
    monkeypatch.setattr(node_provisioner.FleetProbe, "is_gb10", classmethod(lambda c, s: False))
    assert prov._step_probe(NODE, _Session()) is False
    assert prov.results[NODE]["failed"] == "check the machine"


def test_venv_installs_when_missing(prov):
    prov.results[NODE]["probe"]["venv"] = "0"
    assert prov._step_venv(NODE, _Session()) is True
    assert "python3-venv installed" in prov.results[NODE]["done"]


def test_prepare_fails_and_succeeds(prov, monkeypatch):
    monkeypatch.setattr(node_provisioner.FleetProbe, "prepared", classmethod(lambda c, s: False))
    assert prov._step_prepare(NODE, _Session(sudo_code=1)) is False
    assert prov.results[NODE]["failed"] == "node prepare"
    monkeypatch.setattr(prov, "_reprobe", lambda h, s: True)
    assert prov._step_prepare(NODE, _Session()) is True
    assert prov.results[NODE]["host_check"] == "pass"
    assert prov._step_prepare(NODE, _Session(code=1, sudo_code=0)) is True
    assert prov.results[NODE]["host_check"] == "fail"


def test_advertise_options_and_failure(prov):
    prov.options.update(web=True, no_forge=True)
    session = _Session()
    assert prov._step_advertise(NODE, session) is True
    assert session.admin == [["node", "enable", "--no-forge"]]
    assert prov._step_advertise(NODE, _Session(code=1)) is False
    assert prov.results[NODE]["failed"] == "advertise"


def _models_ready(monkeypatch, fits=True):
    plan = node_provisioner.FleetModelPlan
    monkeypatch.setattr(
        node_provisioner.FleetProbe, "missing_models", classmethod(lambda c, s, f: ["f1"])
    )
    monkeypatch.setattr(plan, "local_folders", classmethod(lambda c, f: [Path("/x/f1")]))
    monkeypatch.setattr(plan, "bytes_needed", classmethod(lambda c, m: 0))
    monkeypatch.setattr(plan, "fits", classmethod(lambda c, free, need: fits))


def test_models_fail_on_disk_and_on_searxng(prov, monkeypatch):
    _models_ready(monkeypatch, fits=False)
    assert prov._step_models(NODE, _Session()) is False
    assert "not enough disk" in prov.results[NODE]["reason"]
    _models_ready(monkeypatch)
    monkeypatch.setattr(prov, "_fetch_models", lambda h, s, m: True)
    monkeypatch.setattr(prov, "_images", lambda h, s: True)
    assert prov._step_models(NODE, _Session(code=1)) is False
    assert prov.results[NODE]["failed"] == "start SearXNG"
    prov.results[NODE]["probe"]["searxng"] = node_provisioner.FLAG_TRUE
    assert prov._step_models(NODE, _Session()) is True


def test_models_stop_when_a_fetch_or_images_fail(prov, monkeypatch):
    _models_ready(monkeypatch)
    monkeypatch.setattr(prov, "_fetch_models", lambda h, s, m: False)
    assert prov._step_models(NODE, _Session()) is False
    monkeypatch.setattr(prov, "_fetch_models", lambda h, s, m: True)
    monkeypatch.setattr(prov, "_images", lambda h, s: False)
    assert prov._step_models(NODE, _Session()) is False


def test_fetch_models_rsync_failure_and_hub_route(prov, monkeypatch):
    monkeypatch.setattr(
        node_provisioner.FleetModelPlan, "local_folders", classmethod(lambda c, f: [Path("/x/f1")])
    )
    assert prov._fetch_models(NODE, _Session(code=1), ["f1"]) is False
    assert prov.results[NODE]["failed"] == "copy the model"
    prov.options["model_source"] = node_provisioner.HUB_SOURCE
    monkeypatch.setattr(prov, "_models_from_hub", lambda h, s, m: "hub")
    assert prov._fetch_models(NODE, _Session(), ["f1"]) == "hub"


class _Fetch:
    def __init__(self, outcome, lan=True, verified=True):
        self.outcome, self.lan, self.verified = outcome, lan, verified

    def ensure(self, folder, attempts=1):
        return dict(self.outcome)

    def lan_copy(self, *args):
        return self.lan

    def verify(self, folder):
        return self.verified


def _hub(prov, monkeypatch, fetch):
    monkeypatch.setattr(node_provisioner, "FleetHubFetch", lambda *a: fetch)
    monkeypatch.setattr(
        node_provisioner.FleetModelPlan, "local_folders", classmethod(lambda c, f: [Path("/x/f1")])
    )
    monkeypatch.setattr(node_provisioner.FleetModelPlan, "hub", classmethod(lambda c: "hub"))


def test_hub_models_verified_with_files_sent(prov, monkeypatch):
    _hub(prov, monkeypatch, _Fetch({"ok": True, "hub_failed": False, "sent": 2, "why": ""}))
    assert prov._models_from_hub(NODE, _Session(), ["f1"]) is True
    assert prov.results[NODE]["models_verified"] is True
    assert any("2 file(s) sent" in line for line in prov.results[NODE]["done"])


def test_hub_models_failure_without_a_donor(prov, monkeypatch):
    _hub(prov, monkeypatch, _Fetch({"ok": False, "hub_failed": True, "sent": 0, "why": "net"}))
    assert prov._models_from_hub(NODE, _Session(), ["f1"]) is False
    assert prov.results[NODE]["failed"] == "fetch the model"


def test_lan_fallback_uses_a_verified_donor_and_skips_the_rest(prov, monkeypatch):
    prov.sessions.update({NODE: _Session(), "d1": _Session(), "d2": _Session()})
    prov.results["d1"] = {"done": [], "models_verified": False}
    prov.results["d2"] = {
        "done": [],
        "models_verified": True,
        "probe": {"hostname": "h", "hub": "/h"},
    }
    assert prov._lan_fallback(NODE, _Fetch({}), "f1") is True
    assert "copied from d2" in prov.results[NODE]["done"][-1]
    assert prov._lan_fallback(NODE, _Fetch({}, lan=False), "f1") is False


def test_images_failures(prov, monkeypatch):
    monkeypatch.setattr(
        node_provisioner.FleetProbe, "missing_images", classmethod(lambda c, s, i: list(i))
    )
    assert prov._images(NODE, _Session(pipe=1)) is False
    assert prov.results[NODE]["failed"] == "copy an image"
    assert prov._images(NODE, _Session(code=1)) is False
    assert prov.results[NODE]["failed"] == "pull an image"


def test_assign_failure_and_success(prov):
    prov.results[NODE]["probe"]["model"] = "other"
    assert prov._step_assign(NODE, _Session(code=1)) is False
    assert prov.results[NODE]["failed"] == "assign the model"
    assert prov._step_assign(NODE, _Session()) is True
    assert prov.results[NODE]["model"] == prov.model


def _pairing(monkeypatch, **fns):
    for name, fn in fns.items():
        monkeypatch.setattr(node_provisioner.NodePairing, name, classmethod(fn))


def test_pair_refuses_a_changed_host_key(prov, monkeypatch):
    prov.results[NODE]["probe"]["node_id"] = "nid"
    _pairing(
        monkeypatch,
        find=lambda c, i, browse=False: {"name": "n"},
        pin_host_keys=lambda c, i, k: False,
    )
    assert prov._step_pair(NODE, _Session()) is False
    assert "host key" in prov.results[NODE]["reason"]


def test_pair_reuses_a_working_record_and_records_the_overlay_name(prov, monkeypatch):
    prov.results[NODE]["probe"]["node_id"] = "nid"
    record = {"name": "n", "node": "nid"}
    saved = []
    _pairing(
        monkeypatch,
        find=lambda c, i, browse=False: record,
        pin_host_keys=lambda c, i, k: True,
        run=lambda c, r, *a: SimpleNamespace(returncode=0),
        is_overlay_name=lambda c, a: True,
        _save=lambda c, r: saved.append(r),
    )
    prov.bundle = None
    assert prov._step_pair(NODE, _Session()) is True
    assert record["overlay_name"] == NODE
    assert record["provisioned"]["bundle"] == ""
    assert saved == [record]


def test_pair_over_the_session_when_no_record_and_when_it_fails(prov, monkeypatch):
    _pairing(monkeypatch, pair_over_session=lambda c, s: None)
    assert prov._step_pair(NODE, _Session()) is False
    assert prov.results[NODE]["reason"] == "pairing failed"
    record = {"node": "nid"}
    _pairing(
        monkeypatch,
        pair_over_session=lambda c, s: record,
        is_overlay_name=lambda c, a: False,
        _save=lambda c, r: None,
    )
    assert prov._step_pair(NODE, _Session()) is True
    assert prov.results[NODE]["paired_record"] is record


# -- bookkeeping and dry run ---------------------------------------------------------------------


def test_record_copy_without_a_size_has_no_throughput(prov):
    prov._record_copy(NODE, 0, 1.0)
    assert "throughput" not in prov.results[NODE]
    prov._record_copy(NODE, 10**9, 1.0)
    assert prov.results[NODE]["throughput"] == "1000 MB/s"


def test_wait_for_ssh_polls_until_the_machine_answers(prov, monkeypatch):
    answers = iter([False, True])
    ticks = iter([0, 1, 2, 3])
    monkeypatch.setattr(node_provisioner.time, "monotonic", lambda: next(ticks))
    assert prov._wait_for_ssh(SimpleNamespace(open=lambda: next(answers)), timeout=10) is True


def test_close_all_drops_sudo_and_stops_askpass(prov):
    ran, stopped = [], []
    prov.role = object()
    prov.askpass = SimpleNamespace(stop=lambda: stopped.append(1))
    prov.sessions[NODE] = SimpleNamespace(run=ran.append, close=lambda: ran.append("closed"))
    prov._close_all()
    assert ran == ["sudo -k", "closed"]
    assert stopped == [1]


def test_dry_run_describes_a_machine_needing_everything(prov, monkeypatch, capsys):
    monkeypatch.setattr(
        node_provisioner.NodePairing, "find", classmethod(lambda c, i, browse=False: None)
    )
    prov.options.update(restart=True, web=True)
    prov.results[NODE]["probe"].update(node_id="nid", venv="0", admin="0", searxng="no", model="x")
    monkeypatch.setattr(
        node_provisioner.FleetProbe, "missing_models", classmethod(lambda c, s, f: ["f1"])
    )
    monkeypatch.setattr(
        node_provisioner.FleetProbe, "missing_images", classmethod(lambda c, s, i: ["i1"])
    )
    monkeypatch.setattr(FleetBundle, "expected_version", classmethod(lambda c, s: "v2"))
    prov._describe_dry_run(NODE)
    out = capsys.readouterr().out
    for piece in (
        "python3-venv",
        "install Ragon (v2)",
        "node enable (ragon web",
        "image i1",
        "copy f1",
        "searxng start",
        "main-model set",
        "pair (node add",
        "(restarted)",
    ):
        assert piece in out
    assert prov.results[NODE]["left"] == "(dry run)"


def test_dry_run_install_replaces_an_old_bundle_and_reports_a_bad_source(prov, monkeypatch):
    monkeypatch.setattr(FleetBundle, "expected_version", classmethod(lambda c, s: None))
    monkeypatch.setattr(node_provisioner.FleetProbe, "prepared", classmethod(lambda c, s: True))
    changes = prov._dry_run_install({"admin": "1", "venv": "1", "advertised": "0", "bundle": "v0"})
    assert any("replace Ragon v0" in change for change in changes)
    assert "node enable --no-web" in changes
    assert any("if `host check` fails" in change for change in changes)

    def refuse(cls, source):
        raise ValueError("bad source")

    monkeypatch.setattr(FleetBundle, "expected_version", classmethod(refuse))
    assert prov._dry_run_install({}) == ["nothing, bad source"]


def test_dry_run_models_from_the_hub(prov, monkeypatch):
    prov.options["model_source"] = node_provisioner.HUB_SOURCE
    monkeypatch.setattr(
        node_provisioner.FleetProbe, "missing_models", classmethod(lambda c, s, f: ["f1"])
    )
    monkeypatch.setattr(
        node_provisioner.FleetProbe, "missing_images", classmethod(lambda c, s, i: [])
    )
    changes = prov._dry_run_models(
        {"searxng": node_provisioner.CONTAINER_RUNNING, "model": prov.model}
    )
    assert changes == ["download from Hugging Face f1"]


def test_configured_model_and_admin_executable(monkeypatch):
    assert isinstance(NodeProvisioner.configured_model(), str)
    monkeypatch.setattr(node_provisioner.shutil, "which", lambda name: None)
    assert NodeProvisioner._admin_executable()
