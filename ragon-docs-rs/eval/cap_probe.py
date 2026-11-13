"""At what memory cap does PDFium get through the two files it was OOM-killed on at 1 GB?

Writes results/pdfium-caps.json.
"""

import json
import sys
import time
from pathlib import Path

D = Path(__file__).resolve().parent
src = (D / "bench_pdf.py").read_text(encoding="utf-8").split("for engine in ENGINES:")[0]
sys.argv = [sys.argv[0], "none"]
ns = {"__file__": str(D / "bench_pdf.py")}
exec(compile(src, "bench_pdf", "exec"), ns)
out = {}
for f in ("flate-bomb.pdf", "text-ops.pdf"):
    for cap in ("2G", "3G", "4G"):
        t = time.perf_counter()
        r = ns["run"]("pdfium", str(D / "hostile" / f), cap=cap)
        res = "ok" if r["ok"] else r["error"][:30]
        rss = round(r["maxrss_kb"] / 1024) if r.get("maxrss_kb") else None
        out[f"{f} @ {cap}"] = {
            "result": res,
            "seconds": round(time.perf_counter() - t, 1),
            "peak_rss_mb": rss,
        }
        print(f, cap, out[f"{f} @ {cap}"], flush=True)  # noqa: T201 - progress line per cap attempt
        if r["ok"]:
            break
(D / "results" / "pdfium-caps.json").write_text(json.dumps(out, indent=1), encoding="utf-8")
