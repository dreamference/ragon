"""Which candidate repositories carry ONNX exports, and their licences (metadata only)."""

import json
import sys
import urllib.request


def _say(*parts: object) -> None:
    """Write one line to stdout, flush it, and end with a newline."""
    sys.stdout.write(" ".join(str(part) for part in parts) + "\n")
    sys.stdout.flush()


for repo in sys.argv[1:]:
    try:
        info = json.load(
            urllib.request.urlopen(
                f"https://huggingface.co/api/models/{repo}?blobs=true", timeout=60
            )
        )
    except Exception as exc:
        _say(repo, "ERR", exc)
        continue
    lic = (info.get("cardData") or {}).get("license")
    onnx = [
        (s["rfilename"], round((s.get("size") or 0) / 2**20))
        for s in info.get("siblings", [])
        if s["rfilename"].endswith((".onnx", ".onnx_data"))
    ]
    _say(repo, lic, onnx[:12])
