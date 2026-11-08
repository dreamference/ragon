"""`scripts/decode_speed.py --out` writes the result.json format of ragon-profile (schema 1)."""

import importlib.util
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("decode_speed", ROOT / "scripts" / "decode_speed.py")
decode_speed = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(decode_speed)

# The top-level keys and the figure keys ragon-profile's run.rs writes (ragon-rs/profile/src/run.rs).
RESULT_KEYS = {
    "schema",
    "build",
    "flags",
    "shape",
    "workload_sha256",
    "window",
    "started_utc",
    "machine",
    "state",
    "identical",
    "figures",
    "verdict",
}
FIGURE_KEYS = {
    "ttft_s",
    "cache_state",
    "prompt_tokens",
    "reused_tokens",
    "prefill_ms_per_1k",
    "decode_tok_s",
    "inter_token_gap_ms",
    "tokens_per_pass",
    "acceptance_pct",
    "draft_ms_per_step",
    "verify_ms_per_step",
    "step_period_ms",
    "energy_j_per_token",
}


def kinds():
    return {
        k: {
            "decode_tps": 40.0,
            "ttft_s": 0.2,
            "completion_tokens": [9, 9, 9],
            "correct": True,
            "why": "",
            "identical_runs": True,
            "accept": {},
            "sample": "",
        }
        for k in decode_speed.PROMPTS
    }


def test_the_result_has_the_profile_crates_keys():
    prefill = {"prompt_tokens": 13000, "ttft_s": 2.0, "prefill_tps": 6500.0}
    result = decode_speed.result_json("m", "http://h:1", 3, kinds(), prefill, 0)
    assert set(result) == RESULT_KEYS and result["schema"] == 1
    assert result["shape"]["name"] == "one-stream" and result["shape"]["streams"] == 1
    assert result["shape"]["reps_counted"] == 3 and result["state"] == "clean"
    assert result["identical"] == f"{len(decode_speed.PROMPTS)}/{len(decode_speed.PROMPTS)}"
    for kind in decode_speed.PROMPTS:
        assert set(result["figures"][kind]) == FIGURE_KEYS
        assert result["figures"][kind]["decode_tok_s"] == 40.0
    assert result["figures"]["prefill"]["prefill_tok_s"] == 6500.0
    json.dumps(result)


def test_a_failed_check_is_in_the_state_and_the_verdict():
    result = decode_speed.result_json("m", "h", 3, kinds(), None, 2)
    assert "2 correctness" in result["state"] and result["verdict"].startswith("no verdict (2")
    assert "prefill" not in result["figures"]
