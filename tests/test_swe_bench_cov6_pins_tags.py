"""Pin loading from damaged files, the validated-list filter and the registry tag fetch.

Everything is mocked: no network, no docker; the pin file is a temporary one.
"""

import io
import json

import pytest

from dreamference.swe_bench import swe_bench_image_pins, swe_bench_images
from dreamference.swe_bench.swe_bench_image_pins import COMMUNITY, SweBenchImagePins
from dreamference.swe_bench.swe_bench_images import SweBenchImages

DIGEST = "sha256:" + "c" * 64


@pytest.fixture
def pin_dir(tmp_path, monkeypatch):
    monkeypatch.setattr(swe_bench_image_pins, "PINS_DIR", tmp_path)
    return tmp_path


def test_load_without_a_file_is_empty(pin_dir):
    assert SweBenchImagePins.load() == {}


@pytest.mark.parametrize("text", ["{not json", "[1, 2]", '{"images": [1]}', '{"images": null}'])
def test_load_of_a_damaged_file_is_empty(pin_dir, text):
    SweBenchImagePins.path().write_text(text)
    assert SweBenchImagePins.load() == {}


def test_load_stringifies_values(pin_dir):
    SweBenchImagePins.path().write_text(json.dumps({"images": {"a": 1}}))
    assert SweBenchImagePins.load() == {"a": "1"}


def test_from_validated_keeps_only_community_images_with_a_real_digest(monkeypatch):
    records = {
        "i1": {"source": COMMUNITY, "image": "x:1", "digest": DIGEST},
        "i2": {"source": COMMUNITY, "image": "x:2", "digest": "md5:zz"},
        "i3": {"source": COMMUNITY, "image": "", "digest": DIGEST},
        "i4": {"source": "other", "image": "x:4", "digest": DIGEST},
        "i5": {"source": COMMUNITY, "image": "x:5"},
    }
    monkeypatch.setattr(SweBenchImages, "validated", classmethod(lambda cls: records))
    assert SweBenchImagePins.from_validated() == {"x:1": DIGEST}


def test_fetch_tags_asks_for_a_token_then_lists(monkeypatch):
    seen = []

    class Response(io.BytesIO):
        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

    def urlopen(target, timeout):
        url = getattr(target, "full_url", target)
        seen.append((url, getattr(target, "headers", {})))
        body = {"token": "tok"} if "auth.docker.io" in url else {"tags": ["a", "b"]}
        return Response(json.dumps(body).encode())

    monkeypatch.setattr(swe_bench_images.urllib.request, "urlopen", urlopen)
    assert SweBenchImages._fetch_tags() == ["a", "b"]
    assert "auth.docker.io" in seen[0][0]
    assert seen[1][1]["Authorization"] == "Bearer tok"
