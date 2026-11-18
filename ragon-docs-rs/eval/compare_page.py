"""Show one page as two engines extracted it: compare_page.py <file-rel> <page> <engine> <engine> [chars]."""

import json
import os
import sys
from typing import Final

D = os.path.dirname(os.path.abspath(__file__))
CHARS_DEFAULT: Final[int] = 900
rel, page, engines = sys.argv[1], int(sys.argv[2]), sys.argv[3:5]
# Number of CLI arguments before the optional char-count argument.
ARGS_BEFORE_CHARS: Final[int] = 5
n = int(sys.argv[ARGS_BEFORE_CHARS]) if len(sys.argv) > ARGS_BEFORE_CHARS else CHARS_DEFAULT
for e in engines:
    r = json.load(open(f"{D}/results/pdf-{e}.json"))["corpus/" + rel]
    print(f"===== {e} =====")
    print(r["pages"][page - 1][:n])
