"""Run worker.ordered_page on one page and print it: try_order.py file page [chars] (no sandbox; debugging)."""

import sys
from pathlib import Path
from typing import Final

import pypdfium2 as pdfium

CHARS_ARG_INDEX: Final[int] = 3
D: Final[Path] = Path(__file__).resolve().parent
src = (D / "worker.py").read_text().split("engine, path = sys.argv")[0]
ns = {}
exec(compile(src, "worker", "exec"), ns)
doc = pdfium.PdfDocument(sys.argv[1])
n = int(sys.argv[CHARS_ARG_INDEX]) if len(sys.argv) > CHARS_ARG_INDEX else 1500
print(ns["ordered_page"](doc[int(sys.argv[2]) - 1])[:n])
