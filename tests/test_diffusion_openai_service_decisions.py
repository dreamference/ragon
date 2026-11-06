"""What the diffusion sidecar's OpenAI service refuses, and how its sampler and runner decide.

A path it does not serve is a 404 even while the model loads; a body that is not a JSON object is
a 400 that never reaches the model; a generation that fails is a 500 naming the error; a
temperature it cannot read is greedy decoding. The sampler fills whole blocks when no
end-of-sequence comes and samples when asked to, and the runner hands the tokenizer's own mask and
end-of-sequence ids to the sampler.
"""

import http.client
import json
import types

import pytest
import requests
import torch
from test_diffusion_server import _serve, _StubRunner

from dreamference.vllm_server import diffusion_openai_service as dos
from dreamference.vllm_server.diffusion_openai_service import (
    DIFFUSION_SAMPLER_DEFAULTS,
    DiffusionModelRunner,
    _restore_layer_attention_types,
    block_diffusion_sample,
)


class _Recorder(_StubRunner):
    def __init__(self, ready=True, error=None, failure=None):
        super().__init__(ready=ready, error=error)
        self.failure = failure
        self.calls = []

    def generate(self, prompt, max_new_tokens, temperature):
        self.calls.append(("generate", prompt, temperature))
        if self.failure:
            raise self.failure
        return f"gen:{prompt}"

    def chat(self, messages, max_new_tokens, temperature):
        self.calls.append(("chat", messages, temperature))
        return "chat"


@pytest.fixture
def served():
    servers = []

    def serve(runner):
        server, base = _serve(runner)
        servers.append(server)
        return base

    yield serve
    for server in servers:
        server.shutdown()
        server.server_close()


def test_a_path_the_service_does_not_serve_is_a_404_even_while_loading(served):
    runner = _Recorder(ready=False)
    base = served(runner)
    resp = requests.get(f"{base}/v1/engines", timeout=5)
    assert resp.status_code == 404 and "/v1/engines" in resp.json()["error"]["message"]
    resp = requests.post(f"{base}/v1/embeddings", json={"input": "x"}, timeout=5)
    assert resp.status_code == 404
    assert runner.calls == []


@pytest.mark.parametrize(
    ("body", "message"),
    [
        (b"{not json", "invalid JSON body"),
        (b"[1, 2]", "the JSON body must be an object"),
        (b'"text"', "the JSON body must be an object"),
    ],
)
def test_a_body_that_is_not_a_json_object_is_a_400_that_never_reaches_the_model(
    served, body, message
):
    runner = _Recorder()
    base = served(runner)
    resp = requests.post(f"{base}/v1/completions", data=body, timeout=5)
    assert resp.status_code == 400 and resp.json()["error"]["message"] == message
    assert runner.calls == []


def test_an_unreadable_content_length_is_a_400(served):
    runner = _Recorder()
    host, port = served(runner).rsplit("//", 1)[1].rsplit(":", 1)
    conn = http.client.HTTPConnection(host.strip("[]"), int(port), timeout=5)
    conn.putrequest("POST", "/v1/completions")
    conn.putheader("Content-Length", "many")
    conn.endheaders()
    resp = conn.getresponse()
    assert resp.status == 400 and json.loads(resp.read())["error"]["message"] == "invalid JSON body"
    conn.close()
    assert runner.calls == []


def test_a_generation_that_fails_is_a_500_naming_the_error(served):
    runner = _Recorder(failure=RuntimeError("CUDA error: out of memory"))
    base = served(runner)
    resp = requests.post(f"{base}/v1/completions", json={"prompt": "x"}, timeout=5)
    assert resp.status_code == 500
    assert resp.json()["error"]["message"] == "RuntimeError: CUDA error: out of memory"


@pytest.mark.parametrize("temperature", ["hot", None, [0.7]])
def test_a_temperature_that_cannot_be_read_is_greedy_decoding(served, temperature):
    runner = _Recorder()
    base = served(runner)
    payload = {"prompt": "x", "temperature": temperature}
    assert requests.post(f"{base}/v1/completions", json=payload, timeout=5).status_code == 200
    assert runner.calls == [("generate", "x", 0.0)]


@pytest.mark.parametrize(("prompt", "seen"), [(["first", "second"], "first"), ([], "")])
def test_a_prompt_list_is_answered_for_its_first_prompt_only(served, prompt, seen):
    runner = _Recorder()
    base = served(runner)
    resp = requests.post(f"{base}/v1/completions", json={"prompt": prompt}, timeout=5)
    assert resp.json()["choices"][0]["text"] == f"gen:{seen}"
    assert runner.calls == [("generate", seen, 0.0)]


def test_a_chat_without_messages_reaches_the_model_as_an_empty_conversation(served):
    runner = _Recorder()
    base = served(runner)
    resp = requests.post(f"{base}/v1/chat/completions", json={"temperature": 0.5}, timeout=5)
    assert resp.status_code == 200 and resp.json()["object"] == "chat.completion"
    assert runner.calls == [("chat", [], 0.5)]


def _toy(target, vocab=100, gap=1.0):
    """A model whose prediction for each generated position is `target[position - 2]`."""

    class Toy(torch.nn.Module):
        dtype = torch.float32

        def forward(self, input_ids, attention_mask):
            n = input_ids.shape[1]
            logits = torch.zeros(1, n, vocab)
            for position in range(2, n):
                logits[0, position, target[(position - 2) % len(target)]] = gap
            return types.SimpleNamespace(logits=logits)

    return Toy()


def test_with_no_end_of_sequence_every_block_is_filled_and_the_cap_still_holds():
    model = _toy([5, 6, 7, 8])
    prompt = torch.tensor([[3, 4]])
    tokens = block_diffusion_sample(
        model, prompt, mask_id=99, eos_id=None, max_new_tokens=6, block_size=4, steps=6
    )
    assert tokens == [5, 6, 7, 8, 5, 6], "two whole blocks, cut to the six asked for"
    tokens = block_diffusion_sample(
        model, prompt, mask_id=99, eos_id=9, max_new_tokens=8, block_size=4, steps=8
    )
    assert tokens == [5, 6, 7, 8, 5, 6, 7, 8], "an end-of-sequence id never produced cuts nothing"


def test_a_temperature_above_zero_samples_from_the_models_distribution():
    torch.manual_seed(0)
    tokens = block_diffusion_sample(
        _toy([5, 6, 7, 9, 1, 1, 1, 1], gap=1000.0),
        torch.tensor([[3, 4]]),
        mask_id=99,
        eos_id=9,
        max_new_tokens=8,
        block_size=4,
        steps=8,
        temperature=0.7,
    )
    assert tokens == [5, 6, 7]


def test_layers_keep_their_own_attention_type_and_default_to_full_attention():
    own = types.SimpleNamespace(attention_type="sliding_attention")
    bare = types.SimpleNamespace()
    model = types.SimpleNamespace(
        model=types.SimpleNamespace(layers=[own, bare]),
        config=types.SimpleNamespace(layer_types=["full_attention", "sliding_attention"]),
    )
    _restore_layer_attention_types(model)
    assert own.attention_type == "sliding_attention" and bare.attention_type == "sliding_attention"

    bare = types.SimpleNamespace()
    model = types.SimpleNamespace(
        model=types.SimpleNamespace(layers=[bare]), config=types.SimpleNamespace()
    )
    _restore_layer_attention_types(model)
    assert bare.attention_type == "full_attention"

    _restore_layer_attention_types(types.SimpleNamespace(config=types.SimpleNamespace()))


class _Tokenizer:
    mask_token_id, eos_token_id = 99, 9

    def __init__(self):
        self.decoded = []

    def __call__(self, prompt, return_tensors):
        assert return_tensors == "pt"
        return {"input_ids": types.SimpleNamespace(to=lambda device: ("ids", prompt, device))}

    def decode(self, ids, skip_special_tokens):
        self.decoded.append((ids, skip_special_tokens))
        return "decoded"

    def apply_chat_template(self, messages, tokenize, add_generation_prompt):
        assert tokenize is False and add_generation_prompt is True
        return f"<chat>{messages[-1]['content']}"


def test_the_runner_hands_the_tokenizers_own_ids_to_the_sampler(monkeypatch):
    sampled = []

    def sample(model, input_ids, **kwargs):
        sampled.append((model, input_ids, kwargs))
        return [5, 6]

    monkeypatch.setattr(dos, "block_diffusion_sample", sample)
    runner = DiffusionModelRunner("org/diffusion")
    assert runner.ready is False and runner.error is None and runner.device == "cpu"
    runner.tokenizer, runner.model = _Tokenizer(), "model"
    assert runner.chat([{"role": "user", "content": "hi"}], 16, 0.3) == "decoded"
    ((model, input_ids, kwargs),) = sampled
    assert model == "model" and input_ids == ("ids", "<chat>hi", "cpu")
    assert kwargs == {
        "mask_id": 99,
        "eos_id": 9,
        "max_new_tokens": 16,
        "block_size": DIFFUSION_SAMPLER_DEFAULTS["block_size"],
        "steps": DIFFUSION_SAMPLER_DEFAULTS["steps"],
        "temperature": 0.3,
    }
    assert runner.tokenizer.decoded == [([5, 6], True)]


def test_a_block_that_is_already_filled_stops_denoising_early():
    class Filler(torch.nn.Module):
        dtype = torch.float32

        def forward(self, input_ids, attention_mask):
            input_ids[0, input_ids[0] == 99] = 5  # the model fills the whole block at once
            return types.SimpleNamespace(logits=torch.zeros(1, input_ids.shape[1], 100))

    tokens = block_diffusion_sample(
        Filler(),
        torch.tensor([[3, 4]]),
        mask_id=99,
        eos_id=None,
        max_new_tokens=4,
        block_size=4,
        steps=4,
    )
    assert len(tokens) == 4 and tokens[2:] == [5, 5], "the first step committed one position"


def _fake_transformers(monkeypatch, cuda, move_fails=False):
    import sys

    from transformers import dynamic_module_utils

    events = []

    class Model:
        def to(self, device):
            if move_fails:
                raise RuntimeError("arch mismatch")
            events.append(("to", device))
            return self

        def eval(self):
            events.append("eval")

    class Auto:
        @staticmethod
        def from_pretrained(model_id, **kwargs):
            assert kwargs.get("trust_remote_code") is True
            events.append(("load", model_id, dynamic_module_utils.get_imports is not scan))
            return Model()

    scan = dynamic_module_utils.get_imports
    fake = types.ModuleType("transformers")
    fake.AutoModelForMaskedLM = fake.AutoTokenizer = Auto
    fake.dynamic_module_utils = dynamic_module_utils
    monkeypatch.setitem(sys.modules, "transformers", fake)
    monkeypatch.setattr(torch.cuda, "is_available", lambda: cuda)
    monkeypatch.setattr(dos, "_restore_layer_attention_types", lambda model: events.append("fix"))
    return events, scan, dynamic_module_utils


def test_load_patches_the_import_scan_only_while_loading_and_prefers_cuda(monkeypatch):
    events, scan, utils = _fake_transformers(monkeypatch, cuda=True)
    runner = DiffusionModelRunner("org/diffusion")
    runner.load()
    assert runner.ready and runner.device == "cuda" and runner.error is None
    assert ("load", "org/diffusion", True) in events and ("to", "cuda") in events
    assert utils.get_imports is scan, "the scan is put back"


def test_load_falls_back_to_cpu_when_the_gpu_move_fails_and_reports_errors(monkeypatch):
    _fake_transformers(monkeypatch, cuda=True, move_fails=True)
    runner = DiffusionModelRunner("org/diffusion")
    runner.load()
    assert runner.ready and runner.device == "cpu"

    monkeypatch.setattr(dos, "_restore_layer_attention_types", lambda m: 1 / 0)
    broken = DiffusionModelRunner("org/diffusion")
    broken.load()
    assert not broken.ready and broken.error.startswith("ZeroDivisionError")


def test_the_patched_import_scan_keeps_only_runtime_imports(monkeypatch):
    _events, _scan, utils = _fake_transformers(monkeypatch, cuda=False)
    seen = {}
    monkeypatch.setattr(utils, "get_imports", lambda filename: ["dllm", "torch"])
    monkeypatch.setattr(dos, "_imports_outside_main_guard", lambda path: {"torch"})

    class Auto:
        @staticmethod
        def from_pretrained(model_id, **kwargs):
            seen["imports"] = utils.get_imports("file.py")
            return types.SimpleNamespace(
                to=lambda d: None, eval=lambda: None, model=types.SimpleNamespace(layers=[])
            )

    import sys

    sys.modules["transformers"].AutoModelForMaskedLM = Auto
    sys.modules["transformers"].AutoTokenizer = Auto
    runner = DiffusionModelRunner("org/diffusion")
    runner.load()
    assert seen["imports"] == ["torch"] and runner.ready
